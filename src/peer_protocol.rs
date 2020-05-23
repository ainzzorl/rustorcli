use crate::config;
use crate::download::{Download, IncomingBlockRequest, Peer};
use crate::io_primitives;

use std::io::Write;

use std::net::TcpStream;

use log::*;

const TYPE_CHOKED: u8 = 0;
const TYPE_UNCHOKED: u8 = 1;
const TYPE_INTERESTED: u8 = 2;
const TYPE_NOT_INTERESTED: u8 = 3;
const TYPE_HAVE: u8 = 4;
const TYPE_BITFIELD: u8 = 5;
const TYPE_REQUEST: u8 = 6;
const TYPE_PIECE: u8 = 7;

pub fn process_message(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let resptype = message[0];
    info!("Response type: {}", resptype);
    let mut peer = download.peer_mut(peer_id);
    match resptype {
        TYPE_CHOKED => {
            info!("Choked! peer_id={}", peer_id);
            peer.we_choked = true;
        }
        TYPE_UNCHOKED => {
            info!("Unchoked! peer_id={}", peer_id);
            peer.we_choked = false;
        }
        TYPE_INTERESTED => {
            info!("Interested! peer_id={}", peer_id);
            // TODO: do something
        }
        TYPE_NOT_INTERESTED => {
            info!("Not interested! peer_id={}", peer_id);
            // TODO: do something
        }
        TYPE_HAVE => {
            info!("Have! peer_id={}", peer_id);
            on_have(message, download, peer_id);
        }
        TYPE_BITFIELD => {
            info!("Bitfield! peer_id={}", peer_id);
            on_bitfield(message, download, peer_id);
        }
        TYPE_REQUEST => {
            info!("Request! peer_id={}", peer_id);
            download.add_incoming_block_request(to_incoming_block_request(peer_id, message));
        }
        TYPE_PIECE => {
            info!("Piece! download_id={}, peer_id={}", download.id, peer_id);
            on_piece(message, download, peer_id);
        }
        _ => {
            info!("Unknown type! peer_id={}", peer_id);
        }
    }
}

pub fn send_interested(stream: &mut TcpStream) -> Result<(), std::io::Error> {
    info!("Sending interested");
    return send_message(stream, TYPE_INTERESTED, &Vec::new());
}

pub fn send_have(
    download: &mut Download,
    peer_id: usize,
    piece_id: usize,
) -> Result<(), std::io::Error> {
    info!(
        "Sending have, download_id={}, peer_id={}, piece_id={}",
        download.id, peer_id, piece_id
    );

    let peer = download.peers_mut()[peer_id].stream.as_mut();
    if peer.is_none() {
        info!("Stream is already closed");
        return Ok(());
    }

    let mut payload: Vec<u8> = Vec::new();
    payload.extend(io_primitives::u32_to_bytes(piece_id as u32));
    send_message(peer.unwrap(), TYPE_HAVE, &payload)?;

    return Ok(());
}

pub fn request_block(
    download: &mut Download,
    piece_id: usize,
    block_id: usize,
    peer_id: usize,
) -> Result<(), std::io::Error> {
    let block = &download.pieces()[piece_id].blocks()[block_id];

    info!(
        "Requesting download_id={}, peer_id={}, block={} from piece_id={}, offset={}, len={}",
        download.id,
        peer_id,
        block_id,
        piece_id,
        block.offset(),
        block.len()
    );

    let mut payload: Vec<u8> = Vec::new();
    payload.extend(io_primitives::u32_to_bytes(piece_id as u32));
    payload.extend(io_primitives::u32_to_bytes(block.offset() as u32));
    payload.extend(io_primitives::u32_to_bytes(block.len() as u32));
    send_message(
        download.peers_mut()[peer_id]
            .stream
            .as_mut()
            .expect("Expect the stream to be present"),
        TYPE_REQUEST,
        &payload,
    )?;

    trace!("Done requesting block");
    return Ok(());
}

pub fn send_bitfield(peer_id: usize, download: &mut Download) -> Result<(), std::io::Error> {
    info!(
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

    let peer: &mut Peer = download.peer_mut(peer_id);
    let s: &mut TcpStream = peer
        .stream
        .as_mut()
        .expect("Expected the stream to be present");

    send_message(s, TYPE_BITFIELD, &payload)
}

fn on_bitfield(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let num_pieces = download.pieces().len();

    let mut has_piece = vec![false; num_pieces];
    let mut has = 0;

    for have_index in 0..num_pieces {
        let bytes_index = have_index / 8;
        let index_into_byte = have_index % 8;
        let byte = message[1 + bytes_index];
        let mask = 1 << (7 - index_into_byte);
        let value = (byte & mask) != 0;
        has_piece[have_index] = value;
        if value {
            has += 1;
        }
    }

    info!(
        "Received bitfield, download_id={}, peer_id={}, has={}/{}",
        download.id, peer_id, has, num_pieces
    );
    download.peer_mut(peer_id).has_piece = has_piece;
}

fn on_have(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let piece_id = io_primitives::bytes_to_u32(&message[1..=4]) as usize;

    info!(
        "Received have, download_id={}, peer_id={}, piece_id={}",
        download.id, peer_id, piece_id
    );
    download.peer_mut(peer_id).has_piece[piece_id] = true;
}

pub fn send_unchoke(peer_id: usize, download: &mut Download) -> Result<(), std::io::Error> {
    info!(
        "Sending unchoked to peer_id={}, download_id={}",
        peer_id, download.id
    );

    let peer: &mut Peer = download.peer_mut(peer_id);
    let s: &mut TcpStream = peer
        .stream
        .as_mut()
        .expect("Expected the stream to be present");

    send_message(s, TYPE_UNCHOKED, &Vec::new())
}

pub fn send_block(
    download: &mut Download,
    request: &IncomingBlockRequest,
) -> Result<(), std::io::Error> {
    let offset: u64 = ((request.piece_id as u64) * (download.piece_length as u64)
        + (request.begin as u64)) as u64;
    let data = download.get_content(offset, request.length);
    let blocklen = data.len();

    let peer: &mut Peer = download.peer_mut(request.peer_id);
    let s: &mut TcpStream = peer
        .stream
        .as_mut()
        .expect("Expected the stream to be present");

    let mut payload: Vec<u8> = Vec::new();
    payload.extend(io_primitives::u32_to_bytes(request.piece_id as u32));
    payload.extend(io_primitives::u32_to_bytes(request.begin as u32));
    payload.extend(data);

    send_message(s, TYPE_PIECE, &payload)?;
    download.stats_mut().add_uploaded(blocklen as u64);
    return Ok(());
}

pub fn receive_message(
    peer: &mut Peer,
    download_id: usize,
    peer_id: usize,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    debug!("Getting message size... peer_id={}", peer_id);
    let stream: &mut TcpStream = peer.stream.as_mut().unwrap();
    if peer.next_message_length == 0 {
        match io_primitives::read_n(&stream, 4, false) {
            Ok(sizebytes) => {
                let message_size = io_primitives::bytes_to_u32(&sizebytes);
                if message_size == 0 {
                    debug!("Looks like keepalive");
                    return Ok(None);
                }
                trace!("Message size: {}", message_size);
                peer.next_message_length = message_size;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                debug!(
                    "Would-block error from peer_id={} - returning empty",
                    peer_id
                );
                return Ok(None);
            }
            Err(e) => {
                warn!(
                    "Unexpected error, dowload_id={}, peer_id={}: {:?}",
                    download_id, peer_id, e
                );
                return Err(std::boxed::Box::new(e));
            }
        }
    }

    let remaining = peer.next_message_length - peer.buf.len() as u32;
    trace!(
        "Remaining to read: {}/{}",
        remaining,
        peer.next_message_length
    );
    match io_primitives::read_upto_n_nonblocking(&stream, &mut peer.buf, remaining) {
        Ok(_) => {
            if peer.buf.len() == peer.next_message_length as usize {
                let result = peer.buf.clone();
                peer.buf = Vec::new();
                peer.next_message_length = 0;
                trace!("Successfully read the message of len {}", result.len());
                return Ok(Some(result));
            } else {
                debug!(
                    "Haven't read enough yet: {}/{}",
                    peer.buf.len(),
                    peer.next_message_length
                );
                return Ok(None);
            }
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            debug!(
                "Would-block error from peer_id={} - returning empty",
                peer_id
            );
            return Ok(None);
        }
        Err(e) => {
            warn!(
                "Unexpected error, dowload_id={}, peer_id={}: {:?}",
                download_id, peer_id, e
            );
            return Err(std::boxed::Box::new(e));
        }
    }
}

fn to_incoming_block_request(peer_id: usize, message: Vec<u8>) -> IncomingBlockRequest {
    let pieceindex = io_primitives::bytes_to_u32(&message[1..=4]);
    let begin = io_primitives::bytes_to_u32(&message[5..=8]) as u64;
    let length = io_primitives::bytes_to_u32(&message[9..=12]) as u64;
    info!(
        "Got request {} from peer_id={}; from {}, len={}",
        pieceindex, peer_id, begin, length
    );

    IncomingBlockRequest {
        begin: begin,
        length: length,
        piece_id: pieceindex as usize,
        peer_id: peer_id,
    }
}

// TODO: consider separating file io operations
fn on_piece(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let path = &download.temp_location;
    let pieceindex = io_primitives::bytes_to_u32(&message[1..=4]);
    let begin = io_primitives::bytes_to_u32(&message[5..=8]) as usize;
    let blocklen = (message.len() - 9) as usize;
    let block_id = begin / config::BLOCK_SIZE as usize;
    info!(
        "Got piece {} from peer_id={}; from {}, len={}, writing to {}",
        pieceindex, peer_id, begin, blocklen, path
    );

    let seek_pos: u64 =
        ((pieceindex as i64) * (download.piece_length as i64) + (begin as i64)) as u64;
    download.set_content(seek_pos, &message[9..]);

    download.peer_mut(peer_id).outstanding_block_requests -= 1;
    download.set_block_downloaded(pieceindex as usize, block_id);
    download.stats_mut().add_downloaded(blocklen as u64);
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
    to_write.extend(io_primitives::u32_to_bytes(payload_with_type.len() as u32));
    to_write.extend(payload_with_type);
    return stream.write_all(&to_write);
}
