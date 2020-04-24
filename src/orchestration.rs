extern crate notify;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::time::Duration;

use std::collections::HashMap;

extern crate serde;
extern crate serde_bencode;
extern crate serde_bytes;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde_bytes::ByteBuf;

use std::{thread, time};

use rustorcli::torrent_entries;

use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;

use serde_bencode::de;

use percent_encoding::percent_encode_byte;

use failure::{bail, Error};

use ring::digest;

use serde_bencode::value::Value;

use std::convert::TryInto;

use std::collections::VecDeque;

use std::net::TcpStream;

pub struct Download {
    entry: torrent_entries::TorrentEntry,
    torrent: Torrent,
    announcement: Announcement,
    connections: Vec<Option<TcpStream>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Torrent {
    announce: String,
    info: TorrentInfo,
    #[serde(default)]
    info_hash: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TorrentFile {
    path: Vec<String>,
    length: i64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TorrentInfo {
    name: String,
    #[serde(rename = "piece length")]
    piece_length: i64,
    pieces: ByteBuf,

    #[serde(default)]
    piece_infos: Vec<PieceInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PieceInfo {
    downloaded: bool,
    sha: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Announcement {
    interval: i64,
    peers: Vec<PeerInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct PeerInfo {
    ip: String,
    port: u64,
}

struct QueueEntry {
    download_id: u32,
    piece_id: u32,
}

fn reload_config(
    downloads: &mut HashMap<u32, Download>,
    my_id: &String,
    queue: &mut VecDeque<QueueEntry>,
) {
    let entries = torrent_entries::list_torrents();
    println!("Reloading config. Entries: {}", entries.len());
    for entry in entries {
        if !downloads.contains_key(&entry.id) {
            println!("Adding entry, id={}", entry.id);
            let download = to_download(&entry, my_id);

            for piece_id in 0..download.torrent.info.piece_infos.len() {
                queue.push_back(QueueEntry {
                    download_id: entry.id,
                    piece_id: piece_id as u32,
                });
            }

            downloads.insert(entry.id, download);
        }
    }

    // TODO: remove removed downloads
}

fn to_download(entry: &torrent_entries::TorrentEntry, my_id: &String) -> Download {
    let torrent = read_torrent(&(entry.torrent_path));
    let announcement = get_announcement(&torrent, &my_id).unwrap();

    let num_peers = announcement.peers.len();
    let mut connections: Vec<Option<TcpStream>> = Vec::new();
    for _ in 0..num_peers {
        connections.push(None);
    }

    Download {
        entry: torrent_entries::TorrentEntry::new(
            entry.id,
            entry.torrent_path.clone(),
            entry.download_path.clone(),
        ),
        torrent: torrent,
        announcement: announcement,
        connections: connections,
    }
}

fn read_torrent(path: &String) -> Torrent {
    println!("Reading torrent file from path: {}", path);
    let mut f = File::open(&path).expect("no file found");
    let metadata = fs::metadata(&path).expect("unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer).expect("buffer overflow");

    let mut torrent = de::from_bytes::<Torrent>(&buffer).unwrap();
    torrent.info_hash = info_hash(&buffer).unwrap();

    parse_pieces(&mut torrent);

    return torrent;
}

fn parse_pieces(torrent: &mut Torrent) {
    println!("Parsing pieces...");
    let mut piece_infos = Vec::new();

    for piece_id in 0..(torrent.info.pieces.len() / 20) {
        let from = piece_id * 20;
        let to = (piece_id + 1) * 20;
        let bts: [u8; 20] = torrent.info.pieces.as_ref()[from..to].try_into().unwrap();

        piece_infos.push(PieceInfo {
            downloaded: false,
            sha: bts.to_vec(),
        })
    }
    println!("Total pieces: {}", piece_infos.len());

    torrent.info.piece_infos = piece_infos;
}

fn info_hash(data: &[u8]) -> Result<Vec<u8>, Error> {
    let bencode = serde_bencode::from_bytes(data)?;
    if let Value::Dict(root) = bencode {
        if let Some(info) = root.get(&b"info".to_vec()) {
            let info = serde_bencode::to_bytes(info)?;
            let digest = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, &info);
            Ok(digest.as_ref().to_vec())
        } else {
            bail!("info dict not found");
        }
    } else {
        bail!("meta file is no dict");
    }
}

pub fn start() {
    println!("Starting orchestration");
    let my_id: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();
    println!("My id: {}", my_id);

    let mut downloads: HashMap<u32, Download> = HashMap::new();
    main_loop(&mut downloads, &my_id);
}

fn main_loop(downloads: &mut HashMap<u32, Download>, my_id: &String) {
    // TODO: extract this to some method
    let (tx, rx) = channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(2)).unwrap();
    let config_directory = torrent_entries::config_directory();
    watcher
        .watch(config_directory, RecursiveMode::Recursive)
        .unwrap();

    let mut queue = VecDeque::new();

    reload_config(downloads, &my_id, &mut queue);

    let mut iteration = 0;
    loop {
        println!("Main loop iteration");
        println!("Entries in the queue: {}", queue.len());

        match queue.pop_front() {
            Some(c) => {
                println!(
                    "Next chunk: download_id={}, piece_id={}",
                    c.download_id, c.piece_id
                );
            }
            None => {}
        }
        match rx.recv_timeout(time::Duration::from_millis(1000)) {
            Ok(event) => {
                println!("{:?}", event);
                reload_config(downloads, &my_id, &mut queue);
            }
            Err(e) => {
                println!("watch error: {:?}", e);
                // TODO: remove this
                if iteration % 10 == 0 {
                    println!("Reloading on iteration {}", iteration);
                    reload_config(downloads, &my_id, &mut queue);
                }
            }
        }

        open_missing_connections(downloads, my_id);

        // TODO: do work!

        thread::sleep(time::Duration::from_millis(1000));
        iteration += 1;
    }
}

fn open_missing_connections(downloads: &mut HashMap<u32, Download>, my_id: &String) {
    println!("Opening missing connections");
    for (download_id, download) in downloads {
        for peer_index in 0..download.connections.len() {
            if download
                .connections
                .get(peer_index)
                .expect("expected connection to be in the vec")
                .is_none()
            {
                let peer = download
                    .announcement
                    .peers
                    .get(peer_index)
                    .expect("Expected peer to be in the list");
                let ip = if peer.ip == "::1" {
                    String::from("127.0.0.1")
                } else {
                    peer.ip.clone()
                }; // TODO: remove this
                if peer.port == 6881 {
                    // Don't connect to self.
                    // TODO: remove this
                    continue;
                }
                let address = format!("{}:{}", ip, peer.port);
                println!(
                    "Trying to open missing connection; download_id={}, peer_id={}, address={}",
                    download_id, peer_index, address
                );
                match TcpStream::connect(address) {
                    Ok(mut stream) => {
                        println!("Connected to the peer!");
                        handshake(&mut stream, &download.torrent.info_hash, my_id);
                        download.connections[peer_index] = Some(stream);
                    }
                    Err(e) => {
                        println!("Could not connect to peer: {:?}", e);
                    }
                }
            }
        }
    }
}

fn get_announcement(torrent: &Torrent, peer_id: &String) -> Result<Announcement, Error> {
    let client = reqwest::Client::new();
    let info_hash = &torrent.info_hash;
    let urlencodedih: String = info_hash
        .iter()
        .map(|byte| percent_encode_byte(*byte))
        .collect();

    let request = client
        .get(&torrent.announce)
        .query(&[
            ("peer_id", peer_id.clone()),
            ("uploaded", "0".to_string()),
            ("downloaded", "0".to_string()),
            ("port", "6881".to_string()),
            ("left", "0".to_string()),
        ])
        .build()
        .unwrap();
    let url = request.url();
    let url = format!("{}&info_hash={}", url, urlencodedih);
    println!("Announcement URL: {}", url);

    let mut response = client.get(&url).send()?;
    let mut buffer: Vec<u8> = vec![];
    response.copy_to(&mut buffer)?;

    let announcement: Announcement;

    match de::from_bytes::<Announcement>(&buffer) {
        Ok(t) => announcement = t,
        Err(e) => {
            println!("ERROR: {:?}", e);
            bail!("Could not parse tracker response");
        }
    }

    println!("Num peers: {}", announcement.peers.len());

    return Ok(announcement);
}

fn handshake(stream: &mut TcpStream, info_hash: &Vec<u8>, my_id: &String) {
    println!("Starting handshake...");
    let mut to_write: Vec<u8> = Vec::new();
    to_write.push(19 as u8);
    to_write.extend("BitTorrent protocol".bytes());
    to_write.extend(vec![0; 8].into_iter());

    to_write.extend(info_hash.iter().cloned());
    to_write.extend(my_id.bytes());

    let warr: &[u8] = &to_write; // c: &[u8]
    stream.write_all(warr).unwrap();

    let pstrlen = read_n(stream, 1).unwrap();
    read_n(stream, pstrlen[0] as u32).unwrap();

    read_n(stream, 8).unwrap();
    let in_info_hash = read_n(stream, 20).unwrap();
    let in_peer_id = read_n(stream, 20).unwrap();

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
}

fn read_n(stream: &mut TcpStream, bytes_to_read: u32) -> Result<Vec<u8>, Error> {
    let mut buf = vec![];
    read_n_to_buf(stream, &mut buf, bytes_to_read).unwrap();
    Ok(buf)
}

fn read_n_to_buf(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    bytes_to_read: u32,
) -> Result<(), Error> {
    if bytes_to_read == 0 {
        return Ok(());
    }

    let bytes_read = stream.take(bytes_to_read as u64).read_to_end(buf);
    match bytes_read {
        Ok(0) => bail!("Read 0"),
        Ok(n) if n == bytes_to_read as usize => Ok(()),
        Ok(n) => read_n_to_buf(stream, buf, bytes_to_read - n as u32),
        Err(e) => bail!(format!("{}", e)),
    }
}
