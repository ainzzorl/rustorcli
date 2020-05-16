extern crate serde;

use failure::Error;
use percent_encoding::percent_encode_byte;
use serde::Deserialize;
use serde::Serialize;
use serde_bencode::de;

use log::*;

use std::sync::mpsc::{Receiver, Sender};

use std::{thread, time};

use serde_bytes::ByteBuf;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PeerInfo {
    pub ip: String,
    pub port: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Announcement {
    pub interval: i64,
    pub peers: Vec<PeerInfo>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AnnouncementAltPeers {
    interval: i64,
    peers: ByteBuf,
}

pub struct GetAnnouncementRequest {
    pub url: String,
    pub my_id: String,
    pub info_hash: Vec<u8>,
    pub is_local: bool,
    pub download_id: u32,
    pub downloaded: u32,
    pub uploaded: u32,
}

pub struct GetAnnouncementResponse {
    pub download_id: u32,
    pub result: Result<Announcement, Error>,
}

pub fn get_announcements(
    inx: Receiver<GetAnnouncementRequest>,
    outx: Sender<GetAnnouncementResponse>,
) {
    loop {
        let request_opt = inx.recv();
        if request_opt.is_err() {
            thread::sleep(time::Duration::from_secs(1));
            continue;
        }
        let request = request_opt.unwrap();

        outx.send(GetAnnouncementResponse {
            result: get_announcement(&request),
            download_id: request.download_id,
        })
        .unwrap();
    }
}

fn get_announcement(
    get_announcement_request: &GetAnnouncementRequest,
) -> Result<Announcement, Error> {
    let client = reqwest::Client::new();
    let urlencodedih: String = get_announcement_request
        .info_hash
        .iter()
        .map(|byte| percent_encode_byte(*byte))
        .collect();

    let query = [
        ("peer_id", get_announcement_request.my_id.clone()),
        ("uploaded", get_announcement_request.uploaded.to_string()),
        ("downloaded", get_announcement_request.downloaded.to_string()),
        ("port", "6881".to_string()),
        ("left", "0".to_string()),
    ];
    let request = client
        .get(&get_announcement_request.url)
        .query(&query)
        .build()
        .unwrap();

    let url = request.url();
    let url = format!("{}&info_hash={}", url, urlencodedih);
    info!("Announcement URL: {}", url);

    let mut req_builder = client.get(&url);
    if get_announcement_request.is_local {
        req_builder = req_builder.header("x-forwarded-for", "127.0.0.1");
    }
    let mut response = req_builder.send()?;
    let mut buffer: Vec<u8> = vec![];
    response.copy_to(&mut buffer)?;

    info!("Tracker response: {}", show(&buffer));

    let announcement: Announcement;

    match de::from_bytes::<Announcement>(&buffer) {
        Ok(t) => announcement = t,
        Err(e) => {
            info!(
                "Could not parse tracker response: {:?}. Tring alternative structure...",
                e
            );
            match de::from_bytes::<AnnouncementAltPeers>(&buffer) {
                Ok(announcement_alt) => {
                    info!("Managed to parse alternative announcement!");
                    let peers = announcement_alt.peers;
                    let num_peers = peers.len() / 6;
                    let mut peers_parsed: Vec<PeerInfo> = vec![];
                    for i in 0..num_peers {
                        info!("peer_id=#{}", i);
                        info!(
                            "{}.{}.{}.{}:{}",
                            peers[i * 6],
                            peers[i * 6 + 1],
                            peers[i * 6 + 2],
                            peers[i * 6 + 3],
                            (peers[i * 6 + 4] as u32) * 256 + (peers[i * 6 + 5] as u32)
                        );
                        let peer_info = PeerInfo {
                            port: (peers[i * 6 + 4] as u64) * 256 + (peers[i * 6 + 5] as u64),
                            ip: format!(
                                "{}.{}.{}.{}",
                                peers[i * 6],
                                peers[i * 6 + 1],
                                peers[i * 6 + 2],
                                peers[i * 6 + 3]
                            ),
                        };
                        peers_parsed.push(peer_info);
                    }

                    announcement = Announcement {
                        interval: announcement_alt.interval,
                        peers: peers_parsed,
                    };
                }
                Err(e) => {
                    panic!(
                        "Could not parse tracker response with alternative structure either: {:?}",
                        e
                    );
                }
            }
        }
    }

    info!("Num peers: {}", announcement.peers.len());

    for peer in &announcement.peers {
        info!("Peer - {}:{}", peer.ip, peer.port);
    }

    return Ok(announcement);
}

fn show(bs: &Vec<u8>) -> String {
    let mut visible = String::new();
    for &b in bs {
        let part: Vec<u8> = std::ascii::escape_default(b).collect();
        visible.push_str(std::str::from_utf8(&part).unwrap());
    }
    visible
}
