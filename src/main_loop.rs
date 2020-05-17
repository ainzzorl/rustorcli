use std::collections::HashMap;

extern crate serde;
extern crate serde_bencode;
extern crate serde_bytes;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::{thread, time};

use std::net::{TcpListener, TcpStream};

extern crate crypto;

extern crate hex;

use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

use log::*;

use crate::announcement;
use crate::config;
use crate::decider::*;
use crate::download::Download;
use crate::download::Stats;
use crate::handshake::handshake_incoming;
use crate::outgoing_connections::*;
use crate::peer_protocol;
use crate::state_persistence;
use crate::torrent_entries;
use crate::util;

fn reload_config(
    downloads: &mut HashMap<u32, Download>,
    persistent_states: &HashMap<u32, state_persistence::PersistentDownloadState>,
) {
    let entries = torrent_entries::list_torrents();
    info!("Reloading config. Entries: {}", entries.len());
    for entry in entries {
        if !downloads.contains_key(&entry.id) {
            info!("Adding entry, id={}", entry.id);

            let mut download = Download::new(&entry);

            match persistent_states.get(&download.id) {
                Some(state) => {
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

pub fn start(is_local: bool) {
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
    debug!("Receiving incoming connections");

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
            info!("Successful incoming handshake");

            let mut found = false;
            for (download_id, download) in downloads {
                let d_info_hash = &download.info_hash;
                if *d_info_hash == info_hash {
                    info!("Found corresponding download, download_id={}", download_id);
                    found = true;
                    s.set_nonblocking(true).unwrap();
                    let peer_index = download.register_incoming_peer(s);
                    thread::sleep(time::Duration::from_secs(1));
                    peer_protocol::send_bitfield(peer_index, download).ok();
                    thread::sleep(time::Duration::from_secs(1));
                    peer_protocol::send_unchoke(peer_index, download).ok();
                    debug!("Done adding the connection to download");
                    break;
                }
            }

            if !found {
                info!("Did not find corresponding download!");
            }
        }
        Err(e) => {
            warn!("Handshake failure: {:?}", e);
        }
    }
}

fn request_new_connections(
    download: &mut Download,
    my_id: &String,
    outx: &mut Sender<OpenConnectionRequest>,
) {
    let info_hash = download.info_hash.clone();
    let download_id = download.id.clone();
    for (peer_id, peer) in download.peers_mut().into_iter().enumerate() {
        if !peer.stream.is_none() {
            continue;
        }
        if peer.peer_info.is_none() {
            continue;
        }

        let peer_info = peer.peer_info.as_ref().unwrap();

        if peer_info.port == config::PORT {
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
            download_id: download_id as usize,
            info_hash: info_hash.clone(),
        })
        .expect("Expected send to succeed");
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

fn request_checking_with_tracker(
    downloads: &mut HashMap<u32, Download>,
    my_id: &String,
    is_local: bool,
    outx: &mut Sender<announcement::GetAnnouncementRequest>,
) {
    for (download_id, download) in downloads {
        // TODO: to decider?
        if download.last_check_with_tracker.elapsed().unwrap()
            < std::time::Duration::from_secs(download.tracker_interval)
        {
            continue;
        }
        download.last_check_with_tracker = std::time::SystemTime::now();
        outx.send(announcement::GetAnnouncementRequest {
            url: download.announcement_url.clone(),
            info_hash: download.info_hash.clone(),
            is_local: is_local,
            my_id: my_id.clone(),
            download_id: *download_id,
            downloaded: download.stats().downloaded,
            uploaded: download.stats().uploaded,
        })
        .unwrap();
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
                    peer_protocol::send_bitfield(response.peer_id, download).ok();
                    peer_protocol::send_unchoke(response.peer_id, download).ok();
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

fn process_new_announcements(
    downloads: &mut HashMap<u32, Download>,
    my_id: &String,
    inx: &Receiver<announcement::GetAnnouncementResponse>,
    open_connection_request_sender: &mut Sender<OpenConnectionRequest>,
) {
    debug!("Processing announcements connections");

    loop {
        match inx.try_recv() {
            Ok(response_res) => match response_res.result {
                Ok(result) => {
                    info!(
                        "Received new announcement. download_id={}, interval={}, num_peers={}",
                        response_res.download_id,
                        result.interval,
                        result.peers.len()
                    );
                    let download = downloads.get_mut(&response_res.download_id).unwrap();
                    for peer in result.peers {
                        download.register_outgoing_peer(peer);
                    }
                    request_new_connections(download, my_id, open_connection_request_sender)
                }
                Err(e) => {
                    warn!(
                        "Failed to get announcement, download_id={}, {:?}",
                        response_res.download_id, e
                    );
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

    let mut tcp_listener = start_listeners(config::PORT);

    let mut iteration = 0;

    let mut last_state_persistence = std::time::SystemTime::now();

    reload_config(downloads, &initial_persistent_state);

    let (mut open_connections_request_sender, open_connections_request_receiver): (
        Sender<OpenConnectionRequest>,
        Receiver<OpenConnectionRequest>,
    ) = mpsc::channel();
    let (open_connections_response_sender, open_connections_response_receiver): (
        Sender<OpenConnectionResponse>,
        Receiver<OpenConnectionResponse>,
    ) = mpsc::channel();

    thread::spawn(move || {
        open_missing_connections(
            open_connections_request_receiver,
            open_connections_response_sender,
        );
    });

    let (mut get_announcement_request_sender, get_announcement_request_receiver): (
        Sender<announcement::GetAnnouncementRequest>,
        Receiver<announcement::GetAnnouncementRequest>,
    ) = mpsc::channel();
    let (get_announcement_response_sender, get_announcement_response_receiver): (
        Sender<announcement::GetAnnouncementResponse>,
        Receiver<announcement::GetAnnouncementResponse>,
    ) = mpsc::channel();

    thread::spawn(move || {
        announcement::get_announcements(
            get_announcement_request_receiver,
            get_announcement_response_sender,
        );
    });

    let mut last_loop_log = std::time::SystemTime::UNIX_EPOCH;
    let loop_log_interval = std::time::Duration::from_secs(3);

    loop {
        if last_loop_log.elapsed().unwrap() > loop_log_interval {
            info!("Main loop iteration #{}", iteration);
            last_loop_log = std::time::SystemTime::now();
        } else {
            trace!("Main loop iteration #{}", iteration);
        }

        if iteration % 100 == 0 {
            info!("Reloading config on iteration {}", iteration);
            reload_config(downloads, &initial_persistent_state);
        }

        if last_state_persistence.elapsed().unwrap() > config::STATE_PERISTENCE_INTERVAL {
            state_persistence::persist(downloads, &persistent_state_location);
            last_state_persistence = std::time::SystemTime::now();
        }

        request_checking_with_tracker(
            downloads,
            &my_id,
            is_local,
            &mut get_announcement_request_sender,
        );

        process_new_connections(downloads, &open_connections_response_receiver);
        process_new_announcements(
            downloads,
            &my_id,
            &get_announcement_response_receiver,
            &mut open_connections_request_sender,
        );
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

        thread::sleep(config::TICK_INTERVAL);
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
        if requests.is_empty() {
            trace!("No outgoing block requests");
        } else {
            info!(
                "Executing outgoing {} request(s) for download_id={}",
                requests.len(),
                download.id
            );
        }

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
        if requests.is_empty() {
            trace!(
                "No incoming block request(s) for download_id={}",
                download.id
            );
        } else {
            info!(
                "Executing incoming {} request(s) for download_id={}",
                requests.len(),
                download.id
            );
        }

        for request in requests {
            peer_protocol::send_block(download, &request).ok();
        }
    }
}

fn receive_messages(downloads: &mut HashMap<u32, Download>) {
    debug!("Receiving messages connections");
    for (download_id, download) in downloads {
        struct Msg {
            message: Vec<u8>,
            peer_id: usize,
        }
        let mut to_process = Vec::new();
        let mut peers_to_reset: Vec<usize> = Vec::new();
        let peers = download.peers_mut();
        for (peer_id, p) in peers.into_iter().enumerate() {
            let mut peer = p;
            if peer.stream.is_none() {
                continue;
            }
            for _ in 0..config::MAX_INCOMING_MESSAGES_PER_TICK_PER_DOWNLOAD {
                // Guard against too many incoming messages
                match peer_protocol::receive_message(&mut peer, *download_id as usize, peer_id) {
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

pub fn start_listeners(port: u32) -> TcpListener {
    info!("Starting to listen on port {}", port);
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", port)).unwrap();
    tcp_listener.set_nonblocking(true).unwrap();
    let listen_addr = tcp_listener.local_addr().unwrap();
    info!("Listener started on {}", listen_addr);
    return tcp_listener;
}
