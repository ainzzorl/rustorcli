use crate::download::{Download, IncomingBlockRequest};
use crate::io_primitives;

use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;

// TODO: constants
pub fn process_message(message: Vec<u8>, download: &mut Download, peer_id: usize) {
    let resptype = message[0];
    println!("Response type: {}", resptype);
    let mut peer = download.peer_mut(peer_id);
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
            download.add_incoming_block_request(to_incoming_block_request(peer_id, message));
        }
        7 => {
            println!("Piece! download_id={}, peer_id={}", download.id, peer_id);
            on_piece(message, download, peer_id);
        }
        _ => {
            println!("Unknown type! peer_id={}", peer_id);
        }
    }
}

fn to_incoming_block_request(peer_id: usize, message: Vec<u8>) -> IncomingBlockRequest {
    let pieceindex = io_primitives::bytes_to_u32(&message[1..=4]);
    let begin = io_primitives::bytes_to_u32(&message[5..=8]) as usize;
    let length = io_primitives::bytes_to_u32(&message[9..=12]) as usize;
    println!(
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

    download.peer_mut(peer_id).outstanding_block_requests -= 1;
    download.set_block_downloaded(pieceindex as usize, block_id);
    download.check_if_piece_done(pieceindex as usize);
}
