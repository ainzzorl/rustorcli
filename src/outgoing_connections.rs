use log::*;
use std::fmt;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use std::{thread, time};

use crate::handshake::handshake;

pub struct OpenConnectionRequest {
    pub ip: String,
    pub port: u32,
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

#[derive(Clone, Debug)]
pub struct ConnectionError {
    peer_id: usize,
    download_id: u32,
}

impl ConnectionError {
    pub fn peer_id(&self) -> usize {
        self.peer_id
    }

    pub fn download_id(&self) -> u32 {
        self.download_id
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ConnectionError(peer_id={}, download_id={})",
            self.peer_id,
            self.download_id()
        )
    }
}

impl std::error::Error for ConnectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

pub type OpenConnectionResponse = Result<OpenConnectionResponseBody, ConnectionError>;

pub fn open_missing_connections(
    inx: Receiver<OpenConnectionRequest>,
    outx: Sender<OpenConnectionResponse>,
) {
    info!("In open_missing_connections");

    loop {
        let request_opt = inx.recv();
        if request_opt.is_err() {
            thread::sleep(time::Duration::from_secs(1));
            continue;
        }
        let request = request_opt.unwrap();
        let ip = if request.ip == "::1" {
            String::from("127.0.0.1")
        } else if request.ip == "::ffff:127.0.0.1" {
            String::from("127.0.0.1")
        } else {
            request.ip.clone()
        }; // TODO: remove this

        let address = format!("{}:{}", ip, request.port);
        info!("Trying to open missing connection; address={}", address);
        let socket_address: SocketAddr = address.parse().expect("Unable to parse socket address");

        match TcpStream::connect_timeout(&socket_address, Duration::from_secs(1)) {
            Ok(mut stream) => {
                info!("Connected to the peer!");
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
                        info!("Handshake failure: {:?}", e);
                        outx.send(Err(ConnectionError {
                            peer_id: request.peer_id,
                            download_id: request.download_id as u32,
                        }))
                        .unwrap();
                    }
                }
            }
            Err(e) => {
                info!("Could not connect to peer: {:?}", e);
                outx.send(Err(ConnectionError {
                    peer_id: request.peer_id,
                    download_id: request.download_id as u32,
                }))
                .unwrap();
            }
        }
    }
}
