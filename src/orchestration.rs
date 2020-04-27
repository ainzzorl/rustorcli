extern crate notify;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::time::Duration;

use std::collections::HashMap;

extern crate serde;
extern crate serde_bencode;
extern crate serde_bytes;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde_bytes::ByteBuf;

use std::{thread, time};

use rustorcli::torrent_entries;

use serde_bencode::de;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::io::{self, Read};

use percent_encoding::percent_encode_byte;

use failure::{bail, Error};

use ring::digest;

use serde_bencode::value::Value;

use std::convert::TryInto;

use std::collections::VecDeque;

use std::net::TcpStream;

use std::net::SocketAddr;

use std::io::Seek;
use std::io::SeekFrom;

use std::os::unix::fs::OpenOptionsExt;

extern crate crypto;

extern crate hex;

use self::crypto::digest::Digest;

pub struct Download {
    entry: torrent_entries::TorrentEntry,
    torrent: Torrent,
    announcement: Announcement,
    connections: Vec<Option<TcpStream>>,
    we_interested: Vec<bool>,
    we_choked: Vec<bool>,
    temp_location: String,
    file: File,
    have_block: Vec<Vec<bool>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Torrent {
    announce: String,
    info: TorrentInfo,
    #[serde(default)]
    info_hash: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TorrentFile {
    path: Vec<String>,
    length: i64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TorrentInfo {
    name: String,
    length: i64,
    #[serde(rename = "piece length")]
    piece_length: i64,
    pieces: ByteBuf,

    #[serde(default)]
    piece_infos: Vec<PieceInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PieceInfo {
    downloaded: bool,
    sha: Vec<u8>,
}

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

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PeerInfo {
    ip: String,
    port: u64,
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
) {
    let entries = torrent_entries::list_torrents();
    println!("Reloading config. Entries: {}", entries.len());
    for entry in entries {
        if !downloads.contains_key(&entry.id) {
            println!("Adding entry, id={}", entry.id);

            let download = to_download(&entry, my_id);

            for piece_id in 0..download.torrent.info.piece_infos.len() {
                queue.push_back(QueueEntry {
                    download_id: entry.id,
                    piece_id: piece_id,
                });
            }

            downloads.insert(entry.id, download);
        }
    }

    // TODO: remove removed downloads
}

fn to_download(entry: &torrent_entries::TorrentEntry, my_id: &String) -> Download {
    let torrent = read_torrent(&(entry.torrent_path));
    let announcement = get_announcement(&torrent, &my_id).unwrap();

    let num_peers = announcement.peers.len();
    let mut connections: Vec<Option<TcpStream>> = Vec::new();
    let mut we_interested: Vec<bool> = Vec::new();
    let mut we_choked: Vec<bool> = Vec::new();
    for _ in 0..num_peers {
        connections.push(None);
        we_interested.push(false);
        we_choked.push(true);
    }

    let mut have: Vec<Vec<bool>> = Vec::new();
    for piece_id in 0..torrent.info.piece_infos.len() {
        let mut have_blocks = Vec::new();
        let block_size = 16384;
        let mut piece_length = torrent.info.piece_length;
        if piece_id == torrent.info.piece_infos.len() - 1 {
            let standard_piece_length = piece_length;
            let in_previous = standard_piece_length * ((torrent.info.piece_infos.len() - 1) as i64);
            let remaining = torrent.info.length - in_previous;
            println!(
                "Remaining in last piece = {} = {} - {} = {} - {} * {}",
                remaining,
                torrent.info.length,
                in_previous,
                torrent.info.length,
                standard_piece_length,
                torrent.info.piece_infos.len() - 1
            );
            piece_length = remaining;
        }
        let num_blocks = ((piece_length as f64) / (block_size as f64)).ceil() as usize;
        for _ in 0..num_blocks {
            have_blocks.push(false);
        }
        have.push(have_blocks);
    }

    let name = torrent.info.name.clone();

    let temp_location = format!("{}/{}_part", entry.download_path, name);
    let file: File;

    println!("Creating temp file: {}", &temp_location);
    match fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o770)
        .open(&temp_location)
    {
        Err(_why) => panic!("Couldn't create file {}", &temp_location),
        Ok(rs) => {
            file = rs;
        }
    };

    Download {
        entry: torrent_entries::TorrentEntry::new(
            entry.id,
            entry.torrent_path.clone(),
            entry.download_path.clone(),
        ),
        torrent: torrent,
        announcement: announcement,
        connections: connections,
        we_interested: we_interested,
        we_choked: we_choked,
        temp_location: temp_location,
        file: file,
        have_block: have,
    }
}

fn read_torrent(path: &String) -> Torrent {
    println!("Reading torrent file from path: {}", path);
    let mut f = File::open(&path).expect("no file found");
    let metadata = fs::metadata(&path).expect("unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer).expect("buffer overflow");

    let mut torrent = de::from_bytes::<Torrent>(&buffer).unwrap();
    torrent.info_hash = info_hash(&buffer).unwrap();

    parse_pieces(&mut torrent);

    return torrent;
}

fn parse_pieces(torrent: &mut Torrent) {
    println!("Parsing pieces...");
    let mut piece_infos = Vec::new();

    for piece_id in 0..(torrent.info.pieces.len() / 20) {
        let from = piece_id * 20;
        let to = (piece_id + 1) * 20;
        let bts: [u8; 20] = torrent.info.pieces.as_ref()[from..to].try_into().unwrap();

        piece_infos.push(PieceInfo {
            downloaded: false,
            sha: bts.to_vec(),
        })
    }
    println!("Total pieces: {}", piece_infos.len());

    torrent.info.piece_infos = piece_infos;
}

fn info_hash(data: &[u8]) -> Result<Vec<u8>, Error> {
    let bencode = serde_bencode::from_bytes(data)?;
    if let Value::Dict(root) = bencode {
        if let Some(info) = root.get(&b"info".to_vec()) {
            let info = serde_bencode::to_bytes(info)?;
            let digest = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, &info);
            Ok(digest.as_ref().to_vec())
        } else {
            bail!("info dict not found");
        }
    } else {
        bail!("meta file is no dict");
    }
}

pub fn start() {
    println!("Starting orchestration");
    let my_id: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();
    println!("My id: {}", my_id);

    let mut downloads: HashMap<u32, Download> = HashMap::new();
    main_loop(&mut downloads, &my_id);
}

fn main_loop(downloads: &mut HashMap<u32, Download>, my_id: &String) {
    // TODO: extract this to some method
    let (tx, rx) = channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(2)).unwrap();
    let config_directory = torrent_entries::config_directory();
    watcher
        .watch(config_directory, RecursiveMode::Recursive)
        .unwrap();

    let mut queue = VecDeque::new();

    reload_config(downloads, &my_id, &mut queue);

    let mut iteration = 0;
    loop {
        println!("Main loop iteration");
        println!("Entries in the queue: {}", queue.len());

        match rx.recv_timeout(time::Duration::from_millis(1000)) {
            Ok(event) => {
                println!("{:?}", event);
                reload_config(downloads, &my_id, &mut queue);
            }
            Err(e) => {
                println!("watch error: {:?}", e);
                // TODO: remove this
                if iteration % 10 == 0 {
                    println!("Reloading on iteration {}", iteration);
                    reload_config(downloads, &my_id, &mut queue);
                }
            }
        }

        if iteration % 50 == 0 {
            open_missing_connections(downloads, my_id);
        }
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
            println!("Found peer, index={}", peer_id);
            let v: &mut Vec<Option<TcpStream>> = &mut download.connections;
            let mut o: Option<&mut TcpStream> = v[peer_id].as_mut();
            let s: &mut TcpStream = o.as_mut().expect("Expected the stream to be present");

            if !download.we_interested[peer_id] {
                send_interested(s);
                download.we_interested[peer_id] = true;
            } else {
                println!("We are already interested in the peer");
            }

            let mut piece_length = download.torrent.info.piece_length.clone();
            if piece_id == &download.torrent.info.piece_infos.len() - 1 {
                let standard_piece_length = piece_length;
                let in_previous =
                    standard_piece_length * ((download.torrent.info.piece_infos.len() - 1) as i64);
                let remaining = download.torrent.info.length - in_previous;
                piece_length = remaining;
            }

            request_piece(s, piece_id, piece_length);
            return Ok(());
        }
        None => {
            println!("Did not find appropriate peer");
            return Err(());
        }
    }
}

fn send_interested(stream: &mut TcpStream) {
    println!("Sending interested");
    send_message(stream, 2, &Vec::new());
}

fn request_piece(stream: &mut TcpStream, piece_id: usize, piece_length: i64) {
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
        send_message(stream, 6, &payload);

        remaining -= size;
    }

    println!("Done requesting block");
}

fn find_peer_for_piece(download: &Download, piece_id: usize) -> Option<usize> {
    for peer_index in 0..download.announcement.peers.len() {
        // TODO: check if has piece!
        if download.connections[peer_index].is_some() && !download.we_choked[peer_index] {
            return Some(peer_index);
        }
    }
    None
}

fn open_missing_connections(downloads: &mut HashMap<u32, Download>, my_id: &String) {
    println!("Opening missing connections");
    for (download_id, download) in downloads {
        for peer_index in 0..download.connections.len() {
            if download
                .connections
                .get(peer_index)
                .expect("expected connection to be in the vec")
                .is_none()
            {
                let peer = download
                    .announcement
                    .peers
                    .get(peer_index)
                    .expect("Expected peer to be in the list");
                let ip = if peer.ip == "::1" {
                    String::from("127.0.0.1")
                } else {
                    peer.ip.clone()
                }; // TODO: remove this
                if peer.port == 6881 {
                    // Don't connect to self.
                    // TODO: remove this
                    continue;
                }
                let address = format!("{}:{}", ip, peer.port);
                println!(
                    "Trying to open missing connection; download_id={}, peer_id={}, address={}",
                    download_id, peer_index, address
                );
                let socket_address: SocketAddr =
                    address.parse().expect("Unable to parse socket address");
                match TcpStream::connect_timeout(&socket_address, Duration::from_secs(1)) {
                    Ok(mut stream) => {
                        println!("Connected to the peer!");
                        handshake(&mut stream, &download.torrent.info_hash, my_id);
                        stream.set_nonblocking(true).unwrap();
                        download.connections[peer_index] = Some(stream);
                    }
                    Err(e) => {
                        println!("Could not connect to peer: {:?}", e);
                    }
                }
            }
        }
    }
}

fn receive_messages(downloads: &mut HashMap<u32, Download>) {
    println!("Receiving messages connections");
    for download in downloads.values_mut() {
        let mut peer_id: usize = 0;

        let connections = &download.connections;

        struct Msg {
            message: Vec<u8>,
            peer_id: usize,
        }
        let mut to_process = Vec::new();
        for stream_opt in connections {
            match stream_opt {
                Some(stream) => {
                    println!("Getting message size...");
                    match read_n(&stream, 4) {
                        Ok(sizebytes) => {
                            let message_size = bytes_to_u32(&sizebytes);
                            println!("Message size: {}", message_size);
                            if message_size == 0 {
                                println!("Looks like keepalive");
                            } else {
                                println!("Reading message payload...");
                                let message = read_n(&stream, message_size).unwrap();
                                to_process.push(Msg {
                                    message: message,
                                    peer_id: peer_id,
                                });
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            println!("Would-block error");
                        }
                        Err(e) => {
                            println!("Unexpected error: {:?}", e);
                        }
                    }
                }
                None => {
                    // Connection is not open
                }
            }
            peer_id += 1;
        }

        for msg in to_process {
            process_message(msg.message, download, msg.peer_id);
        }
    }
}

fn process_message(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let resptype = message[0];
    println!("Response type: {}", resptype);
    match resptype {
        0 => {
            println!("Choked!");
            download.we_choked[peer_id] = true;
        }
        1 => {
            println!("Unchoked!");
            download.we_choked[peer_id] = false;
        }
        2 => {
            println!("Interested!");
            // TODO: do something
        }
        3 => {
            println!("Not interested!");
            // TODO: do something
        }
        4 => {
            println!("Have!");
            // TODO: do something
        }
        5 => {
            println!("Bitfield!");
            // TODO: do something
        }
        6 => {
            println!("Request!");
            // TODO: do something
        }
        7 => {
            println!("Piece!");
            on_piece(message, download);
        }
        _ => {
            println!("Unknown type!");
        }
    }
}

fn on_piece(message: Vec<u8>, download: &mut Download) {
    let path = &download.temp_location;
    let pieceindex = bytes_to_u32(&message[1..=4]);
    let begin = bytes_to_u32(&message[5..=8]) as usize;
    let blocklen = (message.len() - 9) as usize;
    let block_size = 16384;
    let block_id = begin / block_size;
    println!(
        "Got piece {} from {}, len={}, writing to {}",
        pieceindex, begin, blocklen, path
    );

    let mut file = &download.file;

    let seek_pos: u64 =
        ((pieceindex as i64) * (download.torrent.info.piece_length as i64) + (begin as i64)) as u64;
    println!("Seeking position: {}", seek_pos);
    file.seek(SeekFrom::Start(seek_pos)).unwrap();
    println!("Writing to file");
    file.write(&message[9..]).unwrap();

    download.have_block[pieceindex as usize][block_id] = true;

    check_if_piece_done(download, pieceindex as usize);
    check_if_done(download);
}

fn check_if_piece_done(download: &mut Download, piece_id: usize) {
    for b in &download.have_block[piece_id] {
        if !b {
            return;
        }
    }
    println!("Piece {} seems to be downloaded...", piece_id);

    let mut file = &download.file;

    let offset = ((piece_id as i64) * &download.torrent.info.piece_length) as u64;

    file.seek(io::SeekFrom::Start(offset)).unwrap();
    let mut data = vec![];
    file.take(download.torrent.info.piece_length as u64)
        .read_to_end(&mut data)
        .unwrap();

    // TODO: don't do this every time, store per piece!
    let pieces_shas: Vec<Sha1> = download
        .torrent
        .info
        .pieces
        .chunks(20)
        .map(|v| v.to_owned())
        .collect();
    let expected_hash = &pieces_shas[piece_id];

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

fn check_if_done(download: &Download) {
    println!("Checking if the download is done...");
    let mut num_blocks = 0;
    let mut downloaded_blocks = 0;
    let mut missing: String = String::from("");

    for p_id in 0..download.have_block.len() {
        let p = &download.have_block[p_id];
        for b_id in 0..p.len() {
            let b = p[b_id];
            num_blocks += 1;
            if b {
                downloaded_blocks += 1;
            } else {
                missing = format!("piece={}, block={}", p_id, b_id);
            }
        }
    }
    if num_blocks == downloaded_blocks {
        on_done(download);
    } else {
        println!(
            "Not yet done: downloaded {}/{} blocks. E.g. missing: {}",
            downloaded_blocks, num_blocks, missing
        );
    }
}

fn on_done(download: &Download) {
    println!("Download is done!");
    let dest = format!(
        "{}/{}",
        download.entry.download_path, download.torrent.info.name
    );
    println!("Moving {} to {}", download.temp_location, dest);
    fs::rename(&download.temp_location, dest).unwrap();
}

fn get_announcement(torrent: &Torrent, peer_id: &String) -> Result<Announcement, Error> {
    let client = reqwest::Client::new();
    let info_hash = &torrent.info_hash;
    let urlencodedih: String = info_hash
        .iter()
        .map(|byte| percent_encode_byte(*byte))
        .collect();

    let request = client
        .get(&torrent.announce)
        .query(&[
            ("peer_id", peer_id.clone()),
            ("uploaded", "0".to_string()),
            ("downloaded", "0".to_string()),
            ("port", "6881".to_string()),
            ("left", "0".to_string()),
        ])
        .build()
        .unwrap();
    let url = request.url();
    let url = format!("{}&info_hash={}", url, urlencodedih);
    println!("Announcement URL: {}", url);

    let mut response = client.get(&url).send()?;
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
                        println!("Peer #{}", i);
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

fn handshake(stream: &mut TcpStream, info_hash: &Vec<u8>, my_id: &String) {
    println!("Starting handshake...");
    let mut to_write: Vec<u8> = Vec::new();
    to_write.push(19 as u8);
    to_write.extend("BitTorrent protocol".bytes());
    to_write.extend(vec![0; 8].into_iter());

    to_write.extend(info_hash.iter().cloned());
    to_write.extend(my_id.bytes());

    let warr: &[u8] = &to_write; // c: &[u8]
    stream.write_all(warr).unwrap();

    let pstrlen = read_n(stream, 1).unwrap();
    read_n(stream, pstrlen[0] as u32).unwrap();

    read_n(stream, 8).unwrap();
    let in_info_hash = read_n(stream, 20).unwrap();
    let in_peer_id = read_n(stream, 20).unwrap();

    // validate info hash
    if in_info_hash != *info_hash {
        println!("Invalid info hash");
    }

    let peer_id_vec: Vec<u8> = my_id.bytes().collect();
    if in_peer_id == peer_id_vec {
        // TODO: do something about it!
        println!("Invalid peer id");
    }
    println!("Completed handshake!");
}

fn read_n(stream: &TcpStream, bytes_to_read: u32) -> Result<Vec<u8>, std::io::Error> {
    let mut buf = vec![];
    read_n_to_buf(stream, &mut buf, bytes_to_read)?;
    Ok(buf)
}

fn read_n_to_buf(
    stream: &TcpStream,
    buf: &mut Vec<u8>,
    bytes_to_read: u32,
) -> Result<(), std::io::Error> {
    if bytes_to_read == 0 {
        return Ok(());
    }

    let bytes_read = stream.take(bytes_to_read as u64).read_to_end(buf);
    match bytes_read {
        Ok(0) => panic!("Read 0"),
        Ok(n) if n == bytes_to_read as usize => Ok(()),
        Ok(n) => read_n_to_buf(stream, buf, bytes_to_read - n as u32),
        Err(e) => return Err(std::io::Error::new(io::ErrorKind::WouldBlock, e)),
    }
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

fn send_message(stream: &mut TcpStream, msgtype: u8, payload: &Vec<u8>) {
    let mut payload_with_type: Vec<u8> = Vec::new();
    payload_with_type.push(msgtype);
    payload_with_type.extend(payload);
    let mut to_write: Vec<u8> = Vec::new();
    to_write.extend(u32_to_bytes(payload_with_type.len() as u32));
    to_write.extend(payload_with_type);
    stream.write_all(&to_write).unwrap();
}
