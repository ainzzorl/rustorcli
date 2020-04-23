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

use serde_bencode::de;

use percent_encoding::percent_encode_byte;

use failure::{bail, Error};

use ring::digest;

use serde_bencode::value::Value;

use std::convert::TryInto;

#[derive(Debug, Deserialize, Serialize)]
pub struct Download {
    entry: torrent_entries::TorrentEntry,
    torrent: Torrent,
    announcement: Announcement,
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

fn reload_config(downloads: &mut HashMap<u32, Download>, my_id: &String) {
    let entries = torrent_entries::list_torrents();
    println!("Reloading config. Entries: {}", entries.len());
    for entry in entries {
        if !downloads.contains_key(&entry.id) {
            println!("Adding entry, id={}", entry.id);
            downloads.insert(entry.id, to_download(&entry, my_id));
        }
    }

    // TODO: remove removed downloads
}

fn to_download(entry: &torrent_entries::TorrentEntry, my_id: &String) -> Download {
    let torrent = read_torrent(&(entry.torrent_path));
    let announcement = get_announcement(&torrent, &my_id).unwrap();

    Download {
        entry: torrent_entries::TorrentEntry::new(
            entry.id,
            entry.torrent_path.clone(),
            entry.download_path.clone(),
        ),
        torrent: torrent,
        announcement: announcement,
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
    reload_config(&mut downloads, &my_id);
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

    let mut iteration = 0;
    loop {
        println!("Main loop iteration");

        match rx.recv_timeout(time::Duration::from_millis(1000)) {
            Ok(event) => {
                println!("{:?}", event);
                reload_config(downloads, my_id);
            }
            Err(e) => {
                println!("watch error: {:?}", e);
                // TODO: remove this
                if iteration % 10 == 0 {
                    println!("Reloading on iteration {}", iteration);
                    reload_config(downloads, my_id);
                }
            }
        }

        // TODO: do work!

        thread::sleep(time::Duration::from_millis(1000));
        iteration += 1;
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
