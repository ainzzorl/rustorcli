use std::io::{self, Read};
use std::net::TcpStream;

pub fn read_n(
    stream: &TcpStream,
    bytes_to_read: u32,
    blocking: bool,
) -> Result<Vec<u8>, std::io::Error> {
    let mut buf = vec![];
    read_n_to_buf(stream, &mut buf, bytes_to_read, blocking)?;
    Ok(buf)
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
            Err(e) => return Err(std::io::Error::new(io::ErrorKind::Other, e)),
        }
    } else {
        stream.set_nonblocking(true).unwrap();
        let bytes_read = stream.take(bytes_to_read as u64).read_to_end(buf);
        match bytes_read {
            Ok(0) => return Err(std::io::Error::new(io::ErrorKind::Other, "Read 0 bytes!")),
            Ok(n) if n == bytes_to_read as usize => Ok(()),
            Ok(n) => read_n_to_buf(stream, buf, bytes_to_read - n as u32, blocking),
            Err(e) => return Err(std::io::Error::new(io::ErrorKind::WouldBlock, e)),
        }
    }
}
