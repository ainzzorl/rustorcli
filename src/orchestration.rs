use std::collections::HashMap;

extern crate serde;
extern crate serde_bencode;
extern crate serde_bytes;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde_bytes::ByteBuf;

use std::{thread, time};

use serde_bencode::de;
use std::fs;
use std::io::Write;
use std::io::{self, Read};

use percent_encoding::percent_encode_byte;

use failure::Error;

use std::collections::VecDeque;

use std::net::{TcpListener, TcpStream};

use std::io::Seek;
use std::io::SeekFrom;

extern crate crypto;

extern crate hex;

use self::crypto::digest::Digest;

use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

use crate::announcement::PeerInfo;
use crate::download::{Download, Peer};
use crate::io_primitives::read_n;
use crate::outgoing_connections::*;
use crate::torrent_entries;

#[derive(Debug, Deserialize, Serialize)]
struct Announcement {
    interval: i64,
    peers: Vec<PeerInfo>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AnnouncementAltPeers {
    interval: i64,
    peers: ByteBuf,
}

struct QueueEntry {
    download_id: u32,
    piece_id: usize,
}

pub type Sha1 = Vec<u8>;

pub fn calculate_sha1(input: &[u8]) -> Sha1 {
    let mut hasher = crypto::sha1::Sha1::new();
    hasher.input(input);

    let mut buf: Vec<u8> = vec![0; hasher.output_bytes()];
    hasher.result(&mut buf);
    buf
}

fn reload_config(
    downloads: &mut HashMap<u32, Download>,
    my_id: &String,
    queue: &mut VecDeque<QueueEntry>,
    is_local: bool,
) {
    let entries = torrent_entries::list_torrents();
    println!("Reloading config. Entries: {}", entries.len());
    for entry in entries {
        if !downloads.contains_key(&entry.id) {
            println!("Adding entry, id={}", entry.id);

            let download = to_download(&entry, my_id, is_local);

            for piece_id in 0..download.pieces().len() {
                if !&download.pieces()[piece_id].downloaded() {
                    queue.push_back(QueueEntry {
                        download_id: entry.id,
                        piece_id: piece_id,
                    });
                }
            }

            downloads.insert(entry.id, download);
        }
    }

    // TODO: remove removed downloads
}

fn to_download(entry: &torrent_entries::TorrentEntry, my_id: &String, is_local: bool) -> Download {
    let mut download = Download::new(entry);
    let announcement = get_announcement(&download, &my_id, is_local).unwrap();

    for peer in announcement.peers {
        download.register_outgoing_peer(peer);
    }

    return download;
}

pub fn start(is_local: bool) {
    println!("Starting orchestration");
    // TODO: consistent id
    // TODO: encode client name
    let my_id: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();
    println!("My id: {}", my_id);

    let mut downloads: HashMap<u32, Download> = HashMap::new();
    main_loop(&mut downloads, &my_id, is_local);
}

fn receive_incoming_connections(
    tcp_listener: &mut TcpListener,
    my_id: &String,
    downloads: &mut HashMap<u32, Download>,
) {
    println!("Receiving incoming connections");

    match tcp_listener.accept() {
        Ok((s, _addr)) => handle_incoming_connection(s, my_id, downloads),
        Err(_e) => {
            // Do nothing
        }
    }
}

fn handle_incoming_connection(
    stream: TcpStream,
    my_id: &String,
    downloads: &mut HashMap<u32, Download>,
) {
    let mut s = stream;
    println!("Incoming connection!");
    match handshake_incoming(&mut s, my_id) {
        Ok(info_hash) => {
            println!("Successful incoming handshake!!!");

            let mut found = false;
            for (download_id, download) in downloads {
                let d_info_hash = &download.info_hash;
                if *d_info_hash == info_hash {
                    println!("Found corresponding download, download_id={}", download_id);
                    found = true;
                    s.set_nonblocking(true).unwrap();
                    let peer_index = download.register_incoming_peer(s);
                    send_bitfield(peer_index, download);
                    send_unchoke(peer_index, download);
                    println!("Done adding the connection to download");
                    break;
                }
            }

            if !found {
                println!("Did not find corresponding download!");
            }
        }
        Err(e) => {
            println!("Handshake failure: {:?}", e);
        }
    }
}

fn request_connections(
    downloads: &mut HashMap<u32, Download>,
    my_id: &String,
    outx: Sender<OpenConnectionRequest>,
) {
    for (download_id, download) in downloads {
        for peer_index in 0..download.peers().len() {
            if download
                .peers()
                .get(peer_index)
                .expect("expected peer to be in the vec")
                .stream
                .is_none()
            {
                let peer = download
                    .peers()
                    .get(peer_index)
                    .expect("Expected peer to be in the list");

                if peer.peer_info.is_none() {
                    continue;
                }

                let peer_info = peer.peer_info.as_ref().unwrap();

                if peer_info.port == 6881 {
                    // Don't connect to self.
                    // TODO: use id instead.
                    continue;
                }

                println!(
                    "Requesting connection through the channel. peer_id={}, download_id={}",
                    peer_index, download_id
                );
                outx.send(OpenConnectionRequest {
                    ip: peer_info.ip.clone(),
                    port: peer_info.port.clone(),
                    my_id: my_id.clone(),
                    peer_id: peer_index,
                    download_id: *download_id as usize,
                    info_hash: download.info_hash.clone(),
                })
                .expect("Expected send to succeed");
            }
        }
    }
}

// TODO: consider moving to outgoing_connections
fn process_new_connections(
    downloads: &mut HashMap<u32, Download>,
    inx: &Receiver<OpenConnectionResponse>,
) {
    println!("Processing established connections");

    loop {
        match inx.try_recv() {
            Ok(response_res) => {
                match response_res {
                    Ok(response) => {
                        println!(
                            "Received established connection. download_id={}, peer_id={}",
                            response.download_id, response.peer_id
                        );
                        let stream = response.stream;
                        stream.set_nonblocking(true).unwrap();
                        let download = downloads
                            .get_mut(&(response.download_id as u32))
                            .expect("Download must exist");
                        download.peers()[response.peer_id].stream = Some(stream);
                        send_bitfield(response.peer_id, download);
                        send_unchoke(response.peer_id, download);
                    }
                    Err(e) => {
                        // TODO: message should include details!
                        println!("Failed to establish connection, {:?}", e);
                    }
                }
            }
            Err(_) => {
                break;
            }
        }
    }
}

fn main_loop(downloads: &mut HashMap<u32, Download>, my_id: &String, is_local: bool) {
    // TODO: extract this to some method
    // let (tx, rx) = channel();
    // let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(2)).unwrap();
    // let config_directory = torrent_entries::config_directory();
    // watcher
    //     .watch(config_directory, RecursiveMode::Recursive)
    //     .unwrap();

    let mut queue = VecDeque::new();

    reload_config(downloads, &my_id, &mut queue, is_local);

    let mut tcp_listener = start_listeners(6881);

    let mut iteration = 0;

    let (open_connections_request_sender, open_connections_request_receiver): (
        Sender<OpenConnectionRequest>,
        Receiver<OpenConnectionRequest>,
    ) = mpsc::channel();
    let (open_connections_response_sender, open_connections_response_receiver): (
        Sender<OpenConnectionResponse>,
        Receiver<OpenConnectionResponse>,
    ) = mpsc::channel();

    // TODO: periodically open missing connections
    thread::spawn(move || {
        open_missing_connections(
            open_connections_request_receiver,
            open_connections_response_sender,
        );
    });
    request_connections(downloads, &my_id, open_connections_request_sender);

    loop {
        println!("Main loop iteration #{}", iteration);
        println!("Entries in the queue: {}", queue.len());

        // match rx.recv_timeout(time::Duration::from_millis(1000)) {
        //     Ok(event) => {
        //         println!("{:?}", event);
        //         reload_config(downloads, &my_id, &mut queue);
        //     }
        //     Err(e) => {
        //         println!("watch error: {:?}", e);
        //         // TODO: remove this
        //         if iteration % 10 == 0 {
        //             println!("Reloading on iteration {}", iteration);
        //             reload_config(downloads, &my_id, &mut queue);
        //         }
        //     }
        // }
        if iteration % 100 == 0 {
            println!("Reloading config on iteration {}", iteration);
            reload_config(downloads, &my_id, &mut queue, is_local);
        }

        process_new_connections(downloads, &open_connections_response_receiver);
        receive_incoming_connections(&mut tcp_listener, my_id, downloads);
        receive_messages(downloads);

        match queue.pop_front() {
            Some(c) => match try_download(downloads, c.download_id, c.piece_id) {
                Ok(()) => {
                    println!("Successfuly requested piece, keeping it out of the queue");
                }
                Err(()) => {
                    println!("Failed to download the piece, moving it back to the queue");
                    queue.push_back(c);
                }
            },
            None => {}
        }

        thread::sleep(time::Duration::from_millis(100));
        iteration += 1;
    }
}

fn try_download(
    downloads: &mut HashMap<u32, Download>,
    download_id: u32,
    piece_id: usize,
) -> Result<(), ()> {
    println!(
        "Trying to download next chunk: download_id={}, piece_id={}",
        download_id, piece_id
    );
    let download = downloads.get_mut(&download_id).unwrap();

    match find_peer_for_piece(download, piece_id) {
        Some(peer_id) => {
            println!("Found peer, peer_id={}", peer_id);

            let piece_len = download.pieces()[piece_id].len();

            let mut peer: &mut Peer = download.peer(peer_id);
            let s: &mut TcpStream = peer
                .stream
                .as_mut()
                .expect("Expected the stream to be present");

            if !peer.we_interested {
                if send_interested(s).is_err() {
                    println!(
                        "Couldn't send interested - resetting connection with peer_id={}",
                        peer_id
                    );
                    peer.stream = None;
                    return Err(());
                }
                peer.we_interested = true;
            } else {
                println!("We are already interested in the peer");
            }

            if request_piece(s, piece_id, piece_len as i64).is_err() {
                println!(
                    "Couldn't request piece - resetting connection with peer_id={}",
                    peer_id
                );
                peer.stream = None;
                return Err(());
            }
            return Ok(());
        }
        None => {
            return Err(());
        }
    }
}

fn send_interested(stream: &mut TcpStream) -> Result<(), std::io::Error> {
    println!("Sending interested");
    return send_message(stream, 2, &Vec::new());
}

fn request_piece(
    stream: &mut TcpStream,
    piece_id: usize,
    piece_length: i64,
) -> Result<(), std::io::Error> {
    println!("Requesting piece {} of length {}", piece_id, piece_length); // TODO: what about last piece?

    let block_size = 16384 as i64;
    let num_blocks = ((piece_length as f64) / (block_size as f64)).ceil() as usize;

    let mut remaining = piece_length;

    for block in 0..num_blocks {
        let start = (block as i64) * block_size;
        let size = if remaining > block_size {
            block_size
        } else {
            remaining
        };

        println!("Requesting block {} of size {}", block, size);

        let mut payload: Vec<u8> = Vec::new();
        payload.extend(u32_to_bytes(piece_id as u32));
        payload.extend(u32_to_bytes(start as u32));
        payload.extend(u32_to_bytes(size as u32));
        send_message(stream, 6, &payload)?;

        remaining -= size;
    }

    println!("Done requesting block");
    return Ok(());
}

fn find_peer_for_piece(download: &mut Download, _piece_id: usize) -> Option<usize> {
    let mut no_connection = 0;
    let mut choked = 0;
    for peer_index in 0..download.peers().len() {
        let peer = download.peer(peer_index);
        // TODO: check if has piece!
        if peer.stream.is_none() {
            no_connection += 1;
            continue;
        }
        if peer.we_choked {
            choked += 1;
            continue;
        }
        return Some(peer_index);
    }
    println!(
        "Did not find appropriate peer. No connection: {}, choked: {}",
        no_connection, choked
    );
    None
}

// TODO: move to Download
fn send_bitfield(peer_id: usize, download: &mut Download) {
    println!(
        "Sending bitfield to peer_id={}, download_id={}",
        peer_id, download.id
    );

    let num_pieces = download.pieces().len();

    let mut payload: Vec<u8> = vec![0; (num_pieces as f64 / 8 as f64).ceil() as usize];
    for have_index in 0..num_pieces {
        let bytes_index = have_index / 8;
        let index_into_byte = have_index % 8;
        if download.pieces()[have_index].downloaded() {
            let mask = 1 << (7 - index_into_byte);
            payload[bytes_index] |= mask;
        }
    }

    let peer: &mut Peer = download.peer(peer_id);
    let s: &mut TcpStream = peer
        .stream
        .as_mut()
        .expect("Expected the stream to be present");

    send_message(s, 5, &payload).unwrap();
}

fn send_unchoke(peer_id: usize, download: &mut Download) {
    println!(
        "Sending unchoked to peer_id={}, download_id={}",
        peer_id, download.id
    );

    let peer: &mut Peer = download.peer(peer_id);
    let s: &mut TcpStream = peer
        .stream
        .as_mut()
        .expect("Expected the stream to be present");

    send_message(s, 1, &Vec::new()).unwrap();
}

fn receive_messages(downloads: &mut HashMap<u32, Download>) {
    println!("Receiving messages connections");
    for download in downloads.values_mut() {
        let mut peer_id: usize = 0;

        struct Msg {
            message: Vec<u8>,
            peer_id: usize,
        }
        let mut to_process = Vec::new();
        let mut connections_to_reset: Vec<usize> = Vec::new();
        for peer in download.peers() {
            match &peer.stream {
                Some(stream) => loop {
                    println!("Getting message size... peer_id={}", peer_id);
                    match read_n(&stream, 4, false) {
                        Ok(sizebytes) => {
                            let message_size = bytes_to_u32(&sizebytes);
                            println!("Message size: {}", message_size);
                            if message_size == 0 {
                                println!("Looks like keepalive");
                            } else {
                                println!("Reading message payload...");
                                match read_n(&stream, message_size, true) {
                                    Ok(message) => {
                                        to_process.push(Msg {
                                            message: message,
                                            peer_id: peer_id,
                                        });
                                        break;
                                    }
                                    Err(e) => {
                                        println!(
                                            "Error reading message from peer_id={}: {:?}, resetting connection",
                                            peer_id, e);
                                        connections_to_reset.push(peer_id);
                                        break;
                                    }
                                }
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            println!("Would-block error from peer_id={}: {:?}", peer_id, e);
                            break;
                        }
                        Err(e) => {
                            println!("Unexpected error from peer_id={}: {:?}", peer_id, e);
                            break;
                        }
                    }
                },
                None => {
                    // Connection is not open
                }
            }
            peer_id += 1;
        }

        for msg in to_process {
            process_message(msg.message, download, msg.peer_id);
        }
        for p in connections_to_reset {
            download.peers()[p].stream = None;
        }
    }
}

fn process_message(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let resptype = message[0];
    println!("Response type: {}", resptype);
    let mut peer = download.peer(peer_id);
    match resptype {
        0 => {
            println!("Choked! peer_id={}", peer_id);
            peer.we_choked = true;
        }
        1 => {
            println!("Unchoked! peer_id={}", peer_id);
            peer.we_choked = false;
        }
        2 => {
            println!("Interested! peer_id={}", peer_id);
            // TODO: do something
        }
        3 => {
            println!("Not interested! peer_id={}", peer_id);
            // TODO: do something
        }
        4 => {
            println!("Have! peer_id={}", peer_id);
            // TODO: do something
        }
        5 => {
            println!("Bitfield! peer_id={}", peer_id);
            // TODO: do something
        }
        6 => {
            println!("Request! peer_id={}", peer_id);
            on_request(message, download, peer_id);
        }
        7 => {
            println!("Piece! peer_id={}", peer_id);
            on_piece(message, download, peer_id);
        }
        _ => {
            println!("Unknown type! peer_id={}", peer_id);
        }
    }
}

fn on_request(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let pieceindex = bytes_to_u32(&message[1..=4]);
    let begin = bytes_to_u32(&message[5..=8]) as usize;
    let length = bytes_to_u32(&message[9..=12]) as usize;
    println!(
        "Got request {} from peer_id={}; from {}, len={}",
        pieceindex, peer_id, begin, length
    );

    let mut file = &download.file;

    let seek_pos: u64 =
        ((pieceindex as i64) * (download.piece_length as i64) + (begin as i64)) as u64;
    file.seek(SeekFrom::Start(seek_pos)).unwrap();

    let mut data = vec![];
    file.take(length as u64).read_to_end(&mut data).unwrap();

    let peer: &mut Peer = download.peer(peer_id);
    let s: &mut TcpStream = peer
        .stream
        .as_mut()
        .expect("Expected the stream to be present");

    let mut payload: Vec<u8> = Vec::new();
    payload.extend(u32_to_bytes(pieceindex));
    payload.extend(u32_to_bytes(begin as u32));
    payload.extend(data);

    send_message(s, 7, &payload).unwrap();
}

// TODO: move most logic to Download
fn on_piece(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let path = &download.temp_location;
    let pieceindex = bytes_to_u32(&message[1..=4]);
    let begin = bytes_to_u32(&message[5..=8]) as usize;
    let blocklen = (message.len() - 9) as usize;
    let block_size = 16384;
    let block_id = begin / block_size;
    println!(
        "Got piece {} from peer_id={}; from {}, len={}, writing to {}",
        pieceindex, peer_id, begin, blocklen, path
    );

    let mut file = &download.file;

    let seek_pos: u64 =
        ((pieceindex as i64) * (download.piece_length as i64) + (begin as i64)) as u64;
    println!("Seeking position: {}", seek_pos);
    file.seek(SeekFrom::Start(seek_pos)).unwrap();
    println!("Writing to file");
    file.write(&message[9..]).unwrap();

    download.pieces_mut()[pieceindex as usize].set_block_downloaded(block_id);

    check_if_piece_done(download, pieceindex as usize);
    check_if_done(download);
}

pub fn start_listeners(port: u16) -> TcpListener {
    println!("Starting to listen on port {}", port);
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", port)).unwrap();
    tcp_listener.set_nonblocking(true).unwrap();
    let listen_addr = tcp_listener.local_addr().unwrap();
    println!("Listener started on {}", listen_addr);
    return tcp_listener;
}

// TODO: move most logic to Download
fn check_if_piece_done(download: &mut Download, piece_id: usize) {
    let piece = &download.pieces()[piece_id];
    if !piece.downloaded() {
        return;
    }
    println!("Piece {} seems to be downloaded...", piece_id);

    let mut file = &download.file;

    file.seek(io::SeekFrom::Start(piece.offset().into()))
        .unwrap();
    let mut data = vec![];
    file.take(download.piece_length as u64)
        .read_to_end(&mut data)
        .unwrap();

    let expected_hash = download.pieces()[piece_id].sha();

    let actual_hash = calculate_sha1(&data);

    let is_complete = *expected_hash == actual_hash;
    if is_complete {
        println!("Hashes match for piece {}", piece_id);
    } else {
        panic!(
            "Hashes don't match for piece {}. Expected: {}, actual: {}",
            piece_id,
            hex::encode(expected_hash),
            hex::encode(actual_hash)
        );
        // TODO: do something smarter, e.g. retry
    }
}

// TODO: move most logic to Download
fn is_done(download: &Download) -> bool {
    println!("Checking if the download is done...");
    let mut num_blocks = 0;
    let mut downloaded_blocks = 0;
    let mut missing: String = String::from("");

    for p_id in 0..download.pieces().len() {
        let p = &download.pieces()[p_id];
        for b_id in 0..p.blocks().len() {
            let b = p.blocks()[b_id].downloaded();
            num_blocks += 1;
            if b {
                downloaded_blocks += 1;
            } else {
                missing = format!("piece={}, block={}", p_id, b_id);
            }
        }
    }
    if num_blocks != downloaded_blocks {
        println!(
            "Not yet done: downloaded {}/{} blocks. E.g. missing: {}",
            downloaded_blocks, num_blocks, missing
        );
    }
    return num_blocks == downloaded_blocks;
}

fn check_if_done(download: &Download) {
    if is_done(download) {
        on_done(download);
    }
}

fn on_done(download: &Download) {
    println!("Download is done!");
    let dest = format!("{}/{}", download.download_path, download.name);
    println!("Moving {} to {}", download.temp_location, dest);
    fs::rename(&download.temp_location, dest).unwrap();
}

fn get_announcement(
    download: &Download,
    peer_id: &String,
    is_local: bool,
) -> Result<Announcement, Error> {
    let client = reqwest::Client::new();
    let info_hash = &download.info_hash;
    let urlencodedih: String = info_hash
        .iter()
        .map(|byte| percent_encode_byte(*byte))
        .collect();

    let query = [
        ("peer_id", peer_id.clone()),
        ("uploaded", "0".to_string()),
        ("downloaded", "0".to_string()),
        ("port", "6881".to_string()),
        ("left", "0".to_string()),
    ];
    let request = client
        .get(&download.announcement_url)
        .query(&query)
        .build()
        .unwrap();

    let url = request.url();
    let url = format!("{}&info_hash={}", url, urlencodedih);
    println!("Announcement URL: {}", url);

    let mut req_builder = client.get(&url);
    if is_local {
        req_builder = req_builder.header("x-forwarded-for", "127.0.0.1");
    }
    let mut response = req_builder.send()?;
    let mut buffer: Vec<u8> = vec![];
    response.copy_to(&mut buffer)?;

    println!("Tracker response: {}", show(&buffer));

    let announcement: Announcement;

    match de::from_bytes::<Announcement>(&buffer) {
        Ok(t) => announcement = t,
        Err(e) => {
            println!(
                "Could not parse tracker response: {:?}. Tring alternative structure...",
                e
            );
            match de::from_bytes::<AnnouncementAltPeers>(&buffer) {
                Ok(announcement_alt) => {
                    println!("Managed to parse alternative announcement!");
                    let peers = announcement_alt.peers;
                    let num_peers = peers.len() / 6;
                    let mut peers_parsed: Vec<PeerInfo> = vec![];
                    for i in 0..num_peers {
                        println!("peer_id=#{}", i);
                        println!(
                            "{}.{}.{}.{}:{}",
                            peers[i * 6],
                            peers[i * 6 + 1],
                            peers[i * 6 + 2],
                            peers[i * 6 + 3],
                            (peers[i * 6 + 4] as u32) * 256 + (peers[i * 6 + 5] as u32)
                        );
                        let peer_info = PeerInfo {
                            port: (peers[i * 6 + 4] as u64) * 256 + (peers[i * 6 + 5] as u64),
                            ip: format!(
                                "{}.{}.{}.{}",
                                peers[i * 6],
                                peers[i * 6 + 1],
                                peers[i * 6 + 2],
                                peers[i * 6 + 3]
                            ),
                        };
                        peers_parsed.push(peer_info);
                    }

                    announcement = Announcement {
                        interval: announcement_alt.interval,
                        peers: peers_parsed,
                    };
                }
                Err(e) => {
                    panic!(
                        "Could not parse tracker response with alternative structure either: {:?}",
                        e
                    );
                }
            }
        }
    }

    println!("Num peers: {}", announcement.peers.len());

    for peer in &announcement.peers {
        println!("Peer - {}:{}", peer.ip, peer.port);
    }

    return Ok(announcement);
}

fn show(bs: &Vec<u8>) -> String {
    let mut visible = String::new();
    for &b in bs {
        let part: Vec<u8> = std::ascii::escape_default(b).collect();
        visible.push_str(std::str::from_utf8(&part).unwrap());
    }
    visible
}

fn handshake_incoming(stream: &mut TcpStream, my_id: &String) -> Result<Vec<u8>, std::io::Error> {
    println!("Starting handshake...");

    let pstrlen = read_n(stream, 1, true)?;
    read_n(stream, pstrlen[0] as u32, true)?;

    read_n(stream, 8, true)?;
    let in_info_hash = read_n(stream, 20, true)?;
    let _in_peer_id = read_n(stream, 20, true)?;

    let mut to_write: Vec<u8> = Vec::new();
    to_write.push(19 as u8);
    to_write.extend("BitTorrent protocol".bytes());
    to_write.extend(vec![0; 8].into_iter());

    to_write.extend(in_info_hash.iter().cloned());
    to_write.extend(my_id.bytes());

    let warr: &[u8] = &to_write; // c: &[u8]
    stream.write_all(warr)?;

    return Ok(in_info_hash);
}

const BYTE_0: u32 = 256 * 256 * 256;
const BYTE_1: u32 = 256 * 256;
const BYTE_2: u32 = 256;
const BYTE_3: u32 = 1;

fn bytes_to_u32(bytes: &[u8]) -> u32 {
    bytes[0] as u32 * BYTE_0
        + bytes[1] as u32 * BYTE_1
        + bytes[2] as u32 * BYTE_2
        + bytes[3] as u32 * BYTE_3
}

fn u32_to_bytes(integer: u32) -> Vec<u8> {
    let mut rest = integer;
    let first = rest / BYTE_0;
    rest -= first * BYTE_0;
    let second = rest / BYTE_1;
    rest -= second * BYTE_1;
    let third = rest / BYTE_2;
    rest -= third * BYTE_2;
    let fourth = rest;
    vec![first as u8, second as u8, third as u8, fourth as u8]
}

fn send_message(
    stream: &mut TcpStream,
    msgtype: u8,
    payload: &Vec<u8>,
) -> Result<(), std::io::Error> {
    let mut payload_with_type: Vec<u8> = Vec::new();
    payload_with_type.push(msgtype);
    payload_with_type.extend(payload);
    let mut to_write: Vec<u8> = Vec::new();
    to_write.extend(u32_to_bytes(payload_with_type.len() as u32));
    to_write.extend(payload_with_type);
    return stream.write_all(&to_write);
}
