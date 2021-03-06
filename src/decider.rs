use std::time::SystemTime;

use log::*;

use crate::config;
use crate::download::{BlockRequestRecord, Download, IncomingBlockRequest};

pub struct BlockRequest {
    pub download_id: u32,
    pub peer_id: usize,
    pub piece_id: usize,
    pub block_id: usize,
}

pub struct HaveBroadcast {
    peer_id: usize,
    piece_id: usize,
}

impl HaveBroadcast {
    pub fn new(peer_id: usize, piece_id: usize) -> HaveBroadcast {
        HaveBroadcast {
            peer_id: peer_id,
            piece_id: piece_id,
        }
    }

    pub fn peer_id(&self) -> usize {
        self.peer_id
    }

    pub fn piece_id(&self) -> usize {
        self.piece_id
    }
}

// TODO: should it really update Download here or when executing?
// TODO: randomize a little
pub fn decide_block_requests(download: &mut Download) -> Vec<BlockRequest> {
    let mut result = Vec::new();
    let now = SystemTime::now();

    let mut piece_id: i32 = -1;
    let mut added_in_this = vec![0; download.peers().len()];
    for piece in download.pieces() {
        piece_id += 1;
        if piece.downloaded() {
            continue;
        }
        let mut block_id: i32 = -1;
        for block in piece.blocks() {
            if result.len() >= config::MAX_REQUESTS_PER_TICK_PER_DOWNLOAD {
                break;
            }
            block_id += 1;
            if block.downloaded() || block.request_record().is_some() {
                continue;
            }

            if let Some(peer_id) = find_peer(&download, piece_id as usize, &added_in_this) {
                result.push(BlockRequest {
                    block_id: block_id as usize,
                    download_id: download.id,
                    piece_id: piece_id as usize,
                    peer_id: peer_id,
                });
                added_in_this[peer_id] += 1;
            }
        }
    }

    for request in result.iter() {
        let block = &mut download.pieces_mut()[request.piece_id].blocks_mut()[request.block_id];
        block.set_request_record(Some(BlockRequestRecord {
            peer_id: request.peer_id,
            time: now,
        }));
    }
    for peer_id in 0..download.peers().len() {
        download.peers_mut()[peer_id].outstanding_block_requests += added_in_this[peer_id];
    }

    return result;
}

pub fn decide_incoming_block_requests(download: &mut Download) -> Vec<IncomingBlockRequest> {
    // TODO: be smarter!
    // TODO: check if we have it
    // TODO: not all at once

    let mut result = Vec::new();
    while !download.pending_block_requests.is_empty() {
        result.push(download.pending_block_requests.pop_front().unwrap());
    }

    result
}

pub fn decide_peers_to_reconnect(download: &Download) -> Vec<usize> {
    let mut result = Vec::new();
    if download.is_downloaded() {
        return result;
    }
    for (peer_id, peer) in download.peers().iter().enumerate() {
        if peer.stream.is_some() {
            continue;
        }
        match &peer.peer_info {
            Some(peer_info) => {
                if peer_info.port == config::PORT {
                    // Don't connect to self.
                    // TODO: use id instead.
                    continue;
                }
            }
            None => {
                continue;
            }
        };
        if peer.being_connected {
            trace!(
                "Not requesting reconnection - being connected. download_id={}, peer_id={}",
                download.id,
                peer_id
            );
            continue;
        }
        if peer.reconnect_attempts >= config::MAX_PEER_RECONNECT_ATTEMPTS {
            trace!(
                "Not requesting reconnection - too many attempts. download_id={}, peer_id={}",
                download.id,
                peer_id
            );
            continue;
        }
        if peer.last_reconnect_attempt.elapsed().unwrap() < config::MIN_PEER_RECONNECT_INTERVAL {
            trace!(
                "Not requesting reconnection - too soon. download_id={}, peer_id={}",
                download.id,
                peer_id
            );
            continue;
        }
        debug!(
            "Going to request reconnection!. download_id={}, peer_id={}",
            download.id, peer_id
        );
        result.push(peer_id);
    }
    result
}

pub fn decide_have_broadcasts(download: &mut Download) -> Vec<HaveBroadcast> {
    let mut result: Vec<HaveBroadcast> = Vec::new();
    for recent_piece in download.recently_downloaded_pieces.iter() {
        for (peer_id, peer) in download.peers().iter().enumerate() {
            if peer.has_piece[*recent_piece] {
                // Or should we broadcast to them too?
                continue;
            }
            if peer.stream.is_none() {
                continue;
            }
            result.push(HaveBroadcast::new(peer_id, *recent_piece));
        }
    }
    download.recently_downloaded_pieces = Vec::new();
    result
}

fn find_peer(download: &Download, piece_id: usize, added_in_this: &Vec<i32>) -> Option<usize> {
    for peer_index in 0..download.peers().len() {
        let peer = download.peer(peer_index);
        if peer.stream.is_none() {
            continue;
        }
        if peer.we_choked {
            continue;
        }
        if !peer.has_piece[piece_id] {
            continue;
        }
        if peer.outstanding_block_requests + added_in_this[peer_index]
            >= config::MAX_OUTSTANDING_REQUESTS_PER_PEER
        {
            continue;
        }
        return Some(peer_index);
    }
    None
}
