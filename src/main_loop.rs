use std::collections::HashMap;

extern crate serde;
extern crate serde_bencode;
extern crate serde_bytes;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde_bytes::ByteBuf;

use std::{thread, time};

use serde_bencode::de;
use std::io::Write;

use percent_encoding::percent_encode_byte;

use failure::Error;

use std::net::{TcpListener, TcpStream};

extern crate crypto;

extern crate hex;

use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

use log::*;

use crate::announcement::PeerInfo;
use crate::decider::*;
use crate::download::Download;
use crate::download::Stats;
use crate::io_primitives::read_n;
use crate::outgoing_connections::*;
use crate::peer_protocol;
use crate::state_persistence;
use crate::torrent_entries;
use crate::util;

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

fn reload_config(
    downloads: &mut HashMap<u32, Download>,
    my_id: &String,
    is_local: bool,
    persistent_states: &HashMap<u32, state_persistence::PersistentDownloadState>,
) {
    let entries = torrent_entries::list_torrents();
    info!("Reloading config. Entries: {}", entries.len());
    for entry in entries {
        if !downloads.contains_key(&entry.id) {
            info!("Adding entry, id={}", entry.id);

            let mut download = to_download(&entry, my_id, is_local);

            match persistent_states.get(&download.id) {
                Some(state) => {
                    // TODO: check info hash
                    download.set_stats(Stats {
                        downloaded: state.downloaded,
                        uploaded: state.uploaded,
                    });
                }
                None => {}
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
    info!("Starting orchestration");
    // TODO: consistent id
    // TODO: encode client name
    let my_id: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();
    info!("My id: {}", my_id);

    let mut downloads: HashMap<u32, Download> = HashMap::new();
    main_loop(&mut downloads, &my_id, is_local);
}

fn receive_incoming_connections(
    tcp_listener: &mut TcpListener,
    my_id: &String,
    downloads: &mut HashMap<u32, Download>,
) {
    info!("Receiving incoming connections");

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
    info!("Incoming connection!");
    match handshake_incoming(&mut s, my_id) {
        Ok(info_hash) => {
            info!("Successful incoming handshake!!!");

            let mut found = false;
            for (download_id, download) in downloads {
                let d_info_hash = &download.info_hash;
                if *d_info_hash == info_hash {
                    info!("Found corresponding download, download_id={}", download_id);
                    found = true;
                    s.set_nonblocking(true).unwrap();
                    let peer_index = download.register_incoming_peer(s);
                    thread::sleep(time::Duration::from_secs(1));
                    peer_protocol::send_bitfield(peer_index, download);
                    thread::sleep(time::Duration::from_secs(1));
                    peer_protocol::send_unchoke(peer_index, download);
                    info!("Done adding the connection to download");
                    break;
                }
            }

            if !found {
                info!("Did not find corresponding download!");
            }
        }
        Err(e) => {
            info!("Handshake failure: {:?}", e);
        }
    }
}

fn request_new_connections(
    downloads: &mut HashMap<u32, Download>,
    my_id: &String,
    outx: &mut Sender<OpenConnectionRequest>,
) {
    for (download_id, download) in downloads {
        let info_hash = download.info_hash.clone();
        for (peer_id, peer) in download.peers_mut().into_iter().enumerate() {
            if !peer.stream.is_none() {
                continue;
            }
            if peer.peer_info.is_none() {
                continue;
            }

            let peer_info = peer.peer_info.as_ref().unwrap();

            if peer_info.port == 6881 {
                // Don't connect to self.
                // TODO: use id instead.
                continue;
            }

            info!(
                "Requesting connection through the channel. peer_id={}, download_id={}",
                peer_id, download_id
            );
            peer.being_connected = true;
            outx.send(OpenConnectionRequest {
                ip: peer_info.ip.clone(),
                port: peer_info.port.clone(),
                my_id: my_id.clone(),
                peer_id: peer_id,
                download_id: *download_id as usize,
                info_hash: info_hash.clone(),
            })
            .expect("Expected send to succeed");
        }
    }
}

fn request_resetting_broken_connections(
    downloads: &mut HashMap<u32, Download>,
    my_id: &String,
    outx: &mut Sender<OpenConnectionRequest>,
) {
    for (download_id, download) in downloads {
        let info_hash = download.info_hash.clone();
        for peer_id in decide_peers_to_reconnect(download) {
            let peer = download.peer_mut(peer_id);
            peer.reconnect_attempts += 1;
            peer.last_reconnect_attempt = std::time::SystemTime::now();
            peer.being_connected = true;
            let peer_info = peer.peer_info.as_ref().unwrap();
            info!(
                "Requesting reconnection through the channel. peer_id={}, download_id={}, attempt={}",
                peer_id, download_id, peer.reconnect_attempts
            );
            outx.send(OpenConnectionRequest {
                ip: peer_info.ip.clone(),
                port: peer_info.port.clone(),
                my_id: my_id.clone(),
                peer_id: peer_id,
                download_id: *download_id as usize,
                info_hash: info_hash.clone(),
            })
            .expect("Expected send to succeed");
        }
    }
}

// TODO: consider moving to outgoing_connections
fn process_new_connections(
    downloads: &mut HashMap<u32, Download>,
    inx: &Receiver<OpenConnectionResponse>,
) {
    info!("Processing established connections");

    loop {
        match inx.try_recv() {
            Ok(response_res) => match response_res {
                Ok(response) => {
                    info!(
                        "Received established connection. download_id={}, peer_id={}",
                        response.download_id, response.peer_id
                    );
                    let stream = response.stream;
                    stream.set_nonblocking(true).unwrap();
                    let download = downloads
                        .get_mut(&(response.download_id as u32))
                        .expect("Download must exist");
                    download.peers_mut()[response.peer_id].stream = Some(stream);
                    download.peers_mut()[response.peer_id].being_connected = false;
                    peer_protocol::send_bitfield(response.peer_id, download);
                    peer_protocol::send_unchoke(response.peer_id, download);
                    if peer_protocol::send_interested(
                        &mut download.peers_mut()[response.peer_id]
                            .stream
                            .as_mut()
                            .unwrap(),
                    )
                    .is_ok()
                    {
                        download.peer_mut(response.peer_id).we_interested = true;
                    };
                }
                Err(e) => {
                    warn!("Failed to establish connection, {:?}", e);
                    let download = downloads.get_mut(&e.download_id()).unwrap();
                    download.peer_mut(e.peer_id()).being_connected = false;
                }
            },
            Err(_) => {
                break;
            }
        }
    }
}

fn main_loop(downloads: &mut HashMap<u32, Download>, my_id: &String, is_local: bool) {
    let persistent_state_location = format!("{}/{}", util::config_directory(), "state.json");
    let initial_persistent_state = state_persistence::load(&persistent_state_location);

    let mut tcp_listener = start_listeners(6881);

    let mut iteration = 0;

    let mut last_state_persistence = std::time::SystemTime::now();

    reload_config(downloads, &my_id, is_local, &initial_persistent_state);

    let (mut open_connections_request_sender, open_connections_request_receiver): (
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
    request_new_connections(downloads, &my_id, &mut open_connections_request_sender);

    loop {
        info!("Main loop iteration #{}", iteration);

        if iteration % 100 == 0 {
            info!("Reloading config on iteration {}", iteration);
            reload_config(downloads, &my_id, is_local, &initial_persistent_state);
        }

        if last_state_persistence.elapsed().unwrap() > std::time::Duration::from_secs(3) {
            state_persistence::persist(downloads, &persistent_state_location);
            last_state_persistence = std::time::SystemTime::now();
        }

        process_new_connections(downloads, &open_connections_response_receiver);
        receive_incoming_connections(&mut tcp_listener, my_id, downloads);
        receive_messages(downloads);
        broadcast_have(downloads);

        execute_outgoing_block_requests(downloads);
        execute_incoming_block_requests(downloads);

        request_resetting_broken_connections(
            downloads,
            &my_id,
            &mut open_connections_request_sender,
        );

        thread::sleep(time::Duration::from_millis(100));
        iteration += 1;
    }
}

fn broadcast_have(downloads: &mut HashMap<u32, Download>) {
    for d in downloads.values_mut() {
        let mut download: &mut Download = d;
        for have_broadcast in decide_have_broadcasts(&mut download) {
            // TODO: do something about failed connections.
            peer_protocol::send_have(
                download,
                have_broadcast.peer_id(),
                have_broadcast.piece_id(),
            )
            .ok();
        }
    }
}

fn execute_outgoing_block_requests(downloads: &mut HashMap<u32, Download>) {
    for d in downloads.values_mut() {
        let mut download: &mut Download = d;
        let requests = decide_block_requests(download);
        info!(
            "Executing outgoing {} requests for download_id={}",
            requests.len(),
            download.id
        );

        let mut to_reset = Vec::new();
        for request in requests {
            let peer = &mut download.peers_mut()[request.peer_id];
            let stream = &mut peer
                .stream
                .as_mut()
                .expect("Expected the stream to be present");

            if !peer.we_interested {
                if peer_protocol::send_interested(stream).is_err() {
                    info!(
                        "Couldn't send interested - resetting connection with peer_id={}",
                        request.peer_id
                    );
                    to_reset.push(request.peer_id);
                    continue;
                }
                peer.we_interested = true;
            } else {
                info!("We are already interested in the peer");
            }

            if peer_protocol::request_block(
                &mut download,
                request.piece_id,
                request.block_id,
                request.peer_id,
            )
            .is_err()
            {
                info!(
                    "Couldn't request piece - resetting connection with peer_id={}",
                    request.peer_id
                );
                to_reset.push(request.peer_id);
            }
        }

        for peer_id in to_reset {
            let peer = &mut download.peers_mut()[peer_id];
            peer.stream = None;
        }
    }
}

fn execute_incoming_block_requests(downloads: &mut HashMap<u32, Download>) {
    for d in downloads.values_mut() {
        let download: &mut Download = d;
        let requests = decide_incoming_block_requests(download);
        info!(
            "Executing incoming {} requests for download_id={}",
            requests.len(),
            download.id
        );

        for request in requests {
            peer_protocol::send_block(download, &request);
        }
    }
}

fn receive_messages(downloads: &mut HashMap<u32, Download>) {
    info!("Receiving messages connections");
    for (download_id, download) in downloads {
        struct Msg {
            message: Vec<u8>,
            peer_id: usize,
        }
        let mut to_process = Vec::new();
        let mut peers_to_reset: Vec<usize> = Vec::new();
        let peers = download.peers_mut();
        for (peer_id, peer) in peers.into_iter().enumerate() {
            if peer.stream.is_none() {
                continue;
            }
            let stream: &mut TcpStream = peer.stream.as_mut().unwrap();
            for _ in 0..100 {
                // Guard against too many incoming messages
                match peer_protocol::receive_message(stream, *download_id as usize, peer_id) {
                    Ok(Some(message)) => {
                        to_process.push(Msg {
                            message: message,
                            peer_id: peer_id,
                        });
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(_) => {
                        peers_to_reset.push(peer_id);
                        break;
                    }
                }
            }
        }

        for msg in to_process {
            peer_protocol::process_message(msg.message, download, msg.peer_id);
        }
        for p in peers_to_reset {
            download.on_broken_connection(p);
        }
    }
}

pub fn start_listeners(port: u16) -> TcpListener {
    info!("Starting to listen on port {}", port);
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", port)).unwrap();
    tcp_listener.set_nonblocking(true).unwrap();
    let listen_addr = tcp_listener.local_addr().unwrap();
    info!("Listener started on {}", listen_addr);
    return tcp_listener;
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
    info!("Announcement URL: {}", url);

    let mut req_builder = client.get(&url);
    if is_local {
        req_builder = req_builder.header("x-forwarded-for", "127.0.0.1");
    }
    let mut response = req_builder.send()?;
    let mut buffer: Vec<u8> = vec![];
    response.copy_to(&mut buffer)?;

    info!("Tracker response: {}", show(&buffer));

    let announcement: Announcement;

    match de::from_bytes::<Announcement>(&buffer) {
        Ok(t) => announcement = t,
        Err(e) => {
            info!(
                "Could not parse tracker response: {:?}. Tring alternative structure...",
                e
            );
            match de::from_bytes::<AnnouncementAltPeers>(&buffer) {
                Ok(announcement_alt) => {
                    info!("Managed to parse alternative announcement!");
                    let peers = announcement_alt.peers;
                    let num_peers = peers.len() / 6;
                    let mut peers_parsed: Vec<PeerInfo> = vec![];
                    for i in 0..num_peers {
                        info!("peer_id=#{}", i);
                        info!(
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

    info!("Num peers: {}", announcement.peers.len());

    for peer in &announcement.peers {
        info!("Peer - {}:{}", peer.ip, peer.port);
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
    info!("Starting handshake...");

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
