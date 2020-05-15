use std::io::{self, Read};
use std::net::TcpStream;

const BYTE_0: u32 = 256 * 256 * 256;
const BYTE_1: u32 = 256 * 256;
const BYTE_2: u32 = 256;
const BYTE_3: u32 = 1;

pub fn read_n(
    stream: &TcpStream,
    bytes_to_read: u32,
    blocking: bool,
) -> Result<Vec<u8>, std::io::Error> {
    let mut buf = vec![];
    read_n_to_buf(stream, &mut buf, bytes_to_read, blocking)?;
    Ok(buf)
}

pub fn bytes_to_u32(bytes: &[u8]) -> u32 {
    bytes[0] as u32 * BYTE_0
        + bytes[1] as u32 * BYTE_1
        + bytes[2] as u32 * BYTE_2
        + bytes[3] as u32 * BYTE_3
}

pub fn u32_to_bytes(integer: u32) -> Vec<u8> {
    let mut rest = integer;
    let first = rest / BYTE_0;
    rest -= first * BYTE_0;
    let second = rest / BYTE_1;
    rest -= second * BYTE_1;
    let third = rest / BYTE_2;
    rest -= third * BYTE_2;
    let fourth = rest;
    vec![first as u8, second as u8, third as u8, fourth as u8]
}

fn read_n_to_buf(
    stream: &TcpStream,
    buf: &mut Vec<u8>,
    bytes_to_read: u32,
    blocking: bool,
) -> Result<(), std::io::Error> {
    if bytes_to_read == 0 {
        return Ok(());
    }

    if blocking {
        stream.set_nonblocking(false).unwrap();
        let bytes_read = stream.take(bytes_to_read as u64).read_to_end(buf);
        match bytes_read {
            Ok(0) => return Err(std::io::Error::new(io::ErrorKind::Other, "Read 0 bytes!")),
            Ok(n) if n == bytes_to_read as usize => Ok(()),
            Ok(n) => read_n_to_buf(stream, buf, bytes_to_read - n as u32, blocking),
            Err(e) => Err(e),
        }
    } else {
        stream.set_nonblocking(true).unwrap();
        let bytes_read = stream.take(bytes_to_read as u64).read_to_end(buf);
        match bytes_read {
            Ok(0) => return Err(std::io::Error::new(io::ErrorKind::Other, "Read 0 bytes!")),
            Ok(n) if n == bytes_to_read as usize => Ok(()),
            Ok(n) => read_n_to_buf(stream, buf, bytes_to_read - n as u32, blocking),
            Err(e) => Err(e),
        }
    }
}

pub fn read_upto_n_nonblocking(
    stream: &TcpStream,
    buf: &mut Vec<u8>,
    bytes_to_read: u32,
) -> Result<(), std::io::Error> {
    if bytes_to_read == 0 {
        return Ok(());
    }
    stream.set_nonblocking(true).unwrap();
    let bytes_read = stream.take(bytes_to_read as u64).read_to_end(buf);
    match bytes_read {
        Ok(0) => return Err(std::io::Error::new(io::ErrorKind::Other, "Read 0 bytes!")),
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}
