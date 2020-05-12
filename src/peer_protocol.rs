use crate::download::{Download, IncomingBlockRequest, Peer};
use crate::io_primitives;

use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;

use std::net::TcpStream;

use log::*;

// TODO: constants
pub fn process_message(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let resptype = message[0];
    info!("Response type: {}", resptype);
    let mut peer = download.peer_mut(peer_id);
    match resptype {
        0 => {
            info!("Choked! peer_id={}", peer_id);
            peer.we_choked = true;
        }
        1 => {
            info!("Unchoked! peer_id={}", peer_id);
            peer.we_choked = false;
        }
        2 => {
            info!("Interested! peer_id={}", peer_id);
            // TODO: do something
        }
        3 => {
            info!("Not interested! peer_id={}", peer_id);
            // TODO: do something
        }
        4 => {
            info!("Have! peer_id={}", peer_id);
            on_have(message, download, peer_id);
        }
        5 => {
            info!("Bitfield! peer_id={}", peer_id);
            on_bitfield(message, download, peer_id);
        }
        6 => {
            info!("Request! peer_id={}", peer_id);
            download.add_incoming_block_request(to_incoming_block_request(peer_id, message));
        }
        7 => {
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
    return send_message(stream, 2, &Vec::new());
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
        6,
        &payload,
    )?;

    info!("Done requesting block");
    return Ok(());
}

pub fn send_bitfield(peer_id: usize, download: &mut Download) {
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

    send_message(s, 5, &payload).unwrap();
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

pub fn send_unchoke(peer_id: usize, download: &mut Download) {
    info!(
        "Sending unchoked to peer_id={}, download_id={}",
        peer_id, download.id
    );

    let peer: &mut Peer = download.peer_mut(peer_id);
    let s: &mut TcpStream = peer
        .stream
        .as_mut()
        .expect("Expected the stream to be present");

    send_message(s, 1, &Vec::new()).unwrap();
}

pub fn send_block(download: &mut Download, request: &IncomingBlockRequest) {
    let mut file = &download.file;

    let seek_pos: u64 = ((request.piece_id as i64) * (download.piece_length as i64)
        + (request.begin as i64)) as u64;
    file.seek(SeekFrom::Start(seek_pos)).unwrap();

    let mut data = vec![];
    file.take(request.length as u64)
        .read_to_end(&mut data)
        .unwrap();

    let peer: &mut Peer = download.peer_mut(request.peer_id);
    let s: &mut TcpStream = peer
        .stream
        .as_mut()
        .expect("Expected the stream to be present");

    let mut payload: Vec<u8> = Vec::new();
    payload.extend(io_primitives::u32_to_bytes(request.piece_id as u32));
    payload.extend(io_primitives::u32_to_bytes(request.begin as u32));
    payload.extend(data);

    send_message(s, 7, &payload).unwrap();
}

pub fn receive_message(
    stream: &mut TcpStream,
    download_id: usize,
    peer_id: usize,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    debug!("Getting message size... peer_id={}", peer_id);
    match io_primitives::read_n(&stream, 4, false) {
        Ok(sizebytes) => {
            let message_size = io_primitives::bytes_to_u32(&sizebytes);
            debug!("Message size: {}", message_size);
            if message_size == 0 {
                debug!("Looks like keepalive");
                return Ok(None);
            }
            debug!("Reading message payload...");
            match io_primitives::read_n(&stream, message_size, true) {
                Ok(message) => Ok(Some(message)),
                Err(e) => {
                    warn!(
                        "Error reading message, download_id={} peer_id={}: {:?}, resetting connection",
                        download_id, peer_id, e);
                    Err(std::boxed::Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Read reading message after getting size",
                    )))
                }
            }
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            debug!(
                "Would-block error from peer_id={} - returning empty",
                peer_id
            );
            Ok(None)
        }
        Err(e) => {
            warn!(
                "Unexpected error, dowload_id={}, peer_id={}: {:?}",
                download_id, peer_id, e
            );
            Err(std::boxed::Box::new(e))
        }
    }
}

fn to_incoming_block_request(peer_id: usize, message: Vec<u8>) -> IncomingBlockRequest {
    let pieceindex = io_primitives::bytes_to_u32(&message[1..=4]);
    let begin = io_primitives::bytes_to_u32(&message[5..=8]) as usize;
    let length = io_primitives::bytes_to_u32(&message[9..=12]) as usize;
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
    let block_size = 16384;
    let block_id = begin / block_size;
    info!(
        "Got piece {} from peer_id={}; from {}, len={}, writing to {}",
        pieceindex, peer_id, begin, blocklen, path
    );

    let mut file = &download.file;

    let seek_pos: u64 =
        ((pieceindex as i64) * (download.piece_length as i64) + (begin as i64)) as u64;
    info!("Seeking position: {}", seek_pos);
    file.seek(SeekFrom::Start(seek_pos)).unwrap();
    info!("Writing to file");
    file.write(&message[9..]).unwrap();

    download.peer_mut(peer_id).outstanding_block_requests -= 1;
    download.set_block_downloaded(pieceindex as usize, block_id);
    download.check_if_piece_done(pieceindex as usize);
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