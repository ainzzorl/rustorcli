use std::io::Write;
use std::net::TcpStream;

use log::*;

use crate::io_primitives::read_n;

pub fn handshake_outgoing(
    stream: &mut TcpStream,
    info_hash: &Vec<u8>,
    my_id: &String,
) -> Result<(), std::io::Error> {
    info!("Starting handshake...");
    let mut to_write: Vec<u8> = Vec::new();
    to_write.push(19 as u8);
    to_write.extend("BitTorrent protocol".bytes());
    to_write.extend(vec![0; 8].into_iter());

    to_write.extend(info_hash.iter().cloned());
    to_write.extend(my_id.bytes());

    let warr: &[u8] = &to_write;
    stream.write_all(warr)?;

    let pstrlen = read_n(stream, 1, true)?;
    read_n(stream, pstrlen[0] as u32, true)?;

    read_n(stream, 8, true)?;
    let in_info_hash = read_n(stream, 20, true)?;
    let _in_peer_id = read_n(stream, 20, true)?;

    // validate info hash
    if in_info_hash != *info_hash {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Invalid info hash",
        ));
    }

    info!("Completed handshake!");
    return Ok(());
}

pub fn handshake_incoming(
    stream: &mut TcpStream,
    my_id: &String,
) -> Result<Vec<u8>, std::io::Error> {
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

    let warr: &[u8] = &to_write;
    stream.write_all(warr)?;

    return Ok(in_info_hash);
}
