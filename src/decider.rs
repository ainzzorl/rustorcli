use std::time::{Duration, SystemTime};

use crate::download::{BlockRequestRecord, Download, IncomingBlockRequest};

static REQUEST_EXPIRATION: Duration = Duration::from_secs(30);
static MAX_OUTSTANDING_REQUESTS_PER_PEER: i32 = 10;
static MAX_REQUESTS_PER_TICK: usize = 10;

static MAX_RECONNECT_ATTEMPTS: u32 = 32;
static MIN_RECONNECT_INTERVAL: Duration = Duration::from_secs(5);

pub struct BlockRequest {
    pub download_id: u32,
    pub peer_id: usize,
    pub piece_id: usize,
    pub block_id: usize,
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
            if result.len() >= MAX_REQUESTS_PER_TICK {
                break;
            }
            block_id += 1;
            if block.downloaded() || request_is_active(block.request_record(), now) {
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
                if peer_info.port == 6881 {
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
            continue;
        }
        if peer.reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
            continue;
        }
        if std::time::SystemTime::now().elapsed().unwrap() < MIN_RECONNECT_INTERVAL {
            continue;
        }
        result.push(peer_id);
    }
    result
}

fn find_peer(download: &Download, piece_id: usize, added_in_this: &Vec<i32>) -> Option<usize> {
    // let mut no_connection = 0;
    // let mut choked = 0;
    // let mut too_many_outstanding_requests = 0;
    for peer_index in 0..download.peers().len() {
        let peer = download.peer(peer_index);
        // TODO: check if has piece!
        if peer.stream.is_none() {
            //no_connection += 1;
            continue;
        }
        if peer.we_choked {
            //choked += 1;
            continue;
        }
        if !peer.has_piece[piece_id] {
            continue;
        }
        if peer.outstanding_block_requests + added_in_this[peer_index]
            >= MAX_OUTSTANDING_REQUESTS_PER_PEER
        {
            //too_many_outstanding_requests += 1;
            continue;
        }
        return Some(peer_index);
    }
    // info!(
    //     "Did not find appropriate peer. No connection: {}, choked: {}, too-many-outstanding: {}",
    //     no_connection, choked, too_many_outstanding_requests
    // );
    None
}

fn request_is_active(record: &Option<BlockRequestRecord>, now: SystemTime) -> bool {
    if record.is_none() {
        return false;
    }
    let request_time = record.as_ref().unwrap().time;
    let diff = now.duration_since(request_time).unwrap();

    return diff <= REQUEST_EXPIRATION;
}
