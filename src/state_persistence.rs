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
            },
        );
    }
    serde_json::to_writer(&File::create(location).unwrap(), &state_map).unwrap();
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
