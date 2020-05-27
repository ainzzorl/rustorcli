use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use log::*;

use serde::{Deserialize, Serialize};

use crate::download::Download;

#[derive(Debug, Serialize, Deserialize)]
pub struct PersistentDownloadState {
    pub uploaded: u64,
    pub downloaded: u64,
    pub total_size: u64,
    pub incoming_peers: u32,
    pub outgoing_peers: u32,
    pub connected_peers: u32,
    pub downloaded_pieces: u32,
    pub total_pieces: usize,
    pub done: bool,
    pub name: String,
    pub they_interested: u32,
    pub we_unchoked: u32,
    pub download_speed: u64,

    pub peers: Vec<PersistentPeerState>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PersistentPeerState {
    pub id: usize,
    pub ip: String,
    pub port: String,
    pub outgoing: bool,
    pub connected: bool,
    pub being_connected: bool,
    pub we_choked: bool,
    pub they_interested: bool,
    pub reconnect_attempts: u32,
    pub last_reconnect_attempt: std::time::Duration,
    pub last_incoming_message: std::time::Duration,
}

pub fn persist(downloads: &mut HashMap<u32, Download>, location: &String) {
    info!("Persisting state to {}", location);
    let mut state_map: HashMap<u32, PersistentDownloadState> = HashMap::new();
    for (download_id, download) in downloads {
        let mut incoming_peers = 0;
        let mut outgoing_peers = 0;
        let mut connected_peers = 0;
        let mut downloaded_pieces = 0;
        let mut we_unchoked = 0;
        let mut they_interested = 0;

        for peer in download.peers() {
            if peer.is_incoming() {
                incoming_peers += 1;
            } else {
                outgoing_peers += 1;
            }
            if peer.is_connected() {
                connected_peers += 1;
                if peer.they_interested {
                    they_interested += 1;
                }
                if !peer.we_choked {
                    we_unchoked += 1;
                }
            }
        }
        for piece in download.pieces() {
            if piece.downloaded() {
                downloaded_pieces += 1;
            }
        }

        state_map.insert(
            *download_id,
            PersistentDownloadState {
                uploaded: download.stats().uploaded(),
                downloaded: download.stats().downloaded(),
                total_size: download.length,
                incoming_peers: incoming_peers,
                outgoing_peers: outgoing_peers,
                connected_peers: connected_peers,
                downloaded_pieces: downloaded_pieces,
                total_pieces: download.pieces().len(),
                done: download.is_downloaded(),
                they_interested: they_interested,
                we_unchoked: we_unchoked,
                name: download.name.clone(),
                peers: get_persistent_peers(&download),
                download_speed: download.stats_mut().get_download_speed(),
            },
        );
    }
    serde_json::to_writer(&File::create(location).unwrap(), &state_map).unwrap();
}

fn get_persistent_peers(download: &Download) -> Vec<PersistentPeerState> {
    let mut result = Vec::new();
    for (peer_id, peer) in download.peers().iter().enumerate() {
        let ip;
        let port;
        match &peer.peer_info {
            Some(peer_info) => {
                ip = peer_info.ip.to_string();
                port = peer_info.port.to_string();
            }
            None => {
                ip = "".to_string();
                port = "".to_string();
            }
        };
        result.push(PersistentPeerState {
            id: peer_id,
            ip: ip,
            port: port,
            being_connected: peer.being_connected,
            connected: peer.stream.is_some(),
            last_incoming_message: peer.last_incoming_message.elapsed().unwrap(),
            last_reconnect_attempt: peer.last_reconnect_attempt.elapsed().unwrap(),
            reconnect_attempts: peer.reconnect_attempts,
            outgoing: peer.peer_info.is_some(),
            they_interested: peer.they_interested,
            we_choked: peer.we_choked,
        });
    }
    result
}

pub fn load(location: &String) -> HashMap<u32, PersistentDownloadState> {
    info!("Loading state to {}", location);
    if !Path::new(location).exists() {
        info!("State file does not exist");
        return HashMap::new();
    }
    let mut file = File::open(location).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    serde_json::from_str(&contents).unwrap()
}
