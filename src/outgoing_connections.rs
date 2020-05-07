use failure::Error;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::handshake::handshake;

pub struct OpenConnectionRequest {
    pub ip: String,
    pub port: u64,
    pub my_id: String,
    pub info_hash: Vec<u8>,
    pub download_id: usize,
    pub peer_id: usize,
}

pub struct OpenConnectionResponseBody {
    pub stream: TcpStream,
    pub download_id: usize,
    pub peer_id: usize,
}

pub type OpenConnectionResponse = Result<OpenConnectionResponseBody, Error>;

pub fn open_missing_connections(
    inx: Receiver<OpenConnectionRequest>,
    outx: Sender<OpenConnectionResponse>,
) {
    println!("In open_missing_connections");

    loop {
        let request = inx.recv().expect("Expected to receive a request");
        let ip = if request.ip == "::1" {
            String::from("127.0.0.1")
        } else {
            request.ip.clone()
        }; // TODO: remove this

        let address = format!("{}:{}", ip, request.port);
        println!("Trying to open missing connection; address={}", address);
        let socket_address: SocketAddr = address.parse().expect("Unable to parse socket address");

        match TcpStream::connect_timeout(&socket_address, Duration::from_secs(1)) {
            Ok(mut stream) => {
                println!("Connected to the peer!");
                match handshake(&mut stream, &request.info_hash, &request.my_id) {
                    Ok(()) => {
                        stream.set_nonblocking(true).unwrap();
                        outx.send(Ok(OpenConnectionResponseBody {
                            stream: stream,
                            download_id: request.download_id,
                            peer_id: request.peer_id,
                        }))
                        .unwrap();
                    }
                    Err(e) => {
                        println!("Handshake failure: {:?}", e);
                        outx.send(Err(Error::from(e))).unwrap();
                    }
                }
            }
            Err(e) => {
                println!("Could not connect to peer: {:?}", e);
                outx.send(Err(Error::from(e))).unwrap();
            }
        }
    }
}
