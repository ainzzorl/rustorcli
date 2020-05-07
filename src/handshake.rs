use std::io::Write;
use std::net::TcpStream;

use crate::io_primitives::read_n;

pub fn handshake(
    stream: &mut TcpStream,
    info_hash: &Vec<u8>,
    my_id: &String,
) -> Result<(), std::io::Error> {
    println!("Starting handshake...");
    let mut to_write: Vec<u8> = Vec::new();
    to_write.push(19 as u8);
    to_write.extend("BitTorrent protocol".bytes());
    to_write.extend(vec![0; 8].into_iter());

    to_write.extend(info_hash.iter().cloned());
    to_write.extend(my_id.bytes());

    let warr: &[u8] = &to_write; // c: &[u8]
    stream.write_all(warr)?;

    let pstrlen = read_n(stream, 1, true)?;
    read_n(stream, pstrlen[0] as u32, true)?;

    read_n(stream, 8, true)?;
    let in_info_hash = read_n(stream, 20, true)?;
    let in_peer_id = read_n(stream, 20, true)?;

    // validate info hash
    if in_info_hash != *info_hash {
        println!("Invalid info hash");
    }

    let peer_id_vec: Vec<u8> = my_id.bytes().collect();
    if in_peer_id == peer_id_vec {
        // TODO: do something about it!
        println!("Invalid peer id");
    }
    println!("Completed handshake!");
    return Ok(());
}
