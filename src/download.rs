extern crate crypto;
extern crate serde;

use serde::Deserialize;
use serde::Serialize;

use serde_bytes::ByteBuf;

use std::fs;
use std::fs::File;
use std::net::TcpStream;
use std::path::Path;

use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use std::os::unix::fs::OpenOptionsExt;

use self::crypto::digest::Digest;

use crate::announcement::PeerInfo;
use crate::torrent_entries;

pub struct Download {
    pub entry: torrent_entries::TorrentEntry,
    pub torrent: Torrent,
    pub connections: Vec<Option<TcpStream>>,
    pub we_interested: Vec<bool>,
    pub we_choked: Vec<bool>,
    pub temp_location: String,
    pub file: File,
    pub have_block: Vec<Vec<bool>>,
    pub peers: Vec<PeerInfo>,
}

// TODO: does it belong to this mod?
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TorrentSerializable {
    announce: String,
    info: TorrentInfoSerializable,
    #[serde(default)]
    info_hash: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TorrentInfoSerializable {
    name: String,
    length: i64,
    #[serde(rename = "piece length")]
    piece_length: i64,
    pieces: ByteBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Torrent {
    pub announce: String,
    pub info: TorrentInfo,
    #[serde(default)]
    pub info_hash: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TorrentFile {
    path: Vec<String>,
    length: i64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TorrentInfo {
    pub name: String,
    pub length: i64,
    #[serde(rename = "piece length")]
    pub piece_length: i64,
    pub pieces: ByteBuf,

    #[serde(default)]
    pub piece_infos: Vec<PieceInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PieceInfo {
    pub downloaded: bool,
    pub sha: Vec<u8>,
}

impl Download {
    pub fn new(
        entry: &torrent_entries::TorrentEntry,
        peer_infos: Vec<PeerInfo>,
        torrent: Torrent,
    ) -> Download {
        // TODO: implement for real

        let num_peers = peer_infos.len();
        let mut connections: Vec<Option<TcpStream>> = Vec::new();
        let mut we_interested: Vec<bool> = Vec::new();
        let mut we_choked: Vec<bool> = Vec::new();
        for _ in 0..num_peers {
            connections.push(None);
            we_interested.push(false);
            we_choked.push(true);
        }

        let name = torrent.info.name.clone();

        let temp_location = format!("{}/{}_part", entry.download_path, name);
        let final_location = format!("{}/{}", entry.download_path, name);
        let file: File;

        if !Path::new(&temp_location).exists() && !Path::new(&final_location).exists() {
            println!("Creating temp file: {}", &temp_location);
            match fs::OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .mode(0o770)
                .open(&temp_location)
            {
                Err(_why) => panic!("Couldn't create file {}", &temp_location),
                Ok(rs) => {
                    file = rs;
                }
            };
        } else if Path::new(&final_location).exists() {
            file = fs::OpenOptions::new()
                .create(false)
                .read(true)
                .write(false)
                .mode(0o770)
                .open(&final_location)
                .unwrap();
        } else {
            file = fs::OpenOptions::new()
                .create(false)
                .read(true)
                .write(true)
                .mode(0o770)
                .open(&temp_location)
                .unwrap();
        }

        let mut download = Download {
            entry: torrent_entries::TorrentEntry::new(
                entry.id,
                entry.torrent_path.clone(),
                entry.download_path.clone(),
            ),
            torrent: torrent,
            connections: connections,
            we_interested: we_interested,
            we_choked: we_choked,
            temp_location: temp_location,
            file: file,
            have_block: Vec::new(),
            peers: peer_infos,
        };

        let have = get_have(&mut download);
        download.have_block = have;

        return download;
    }
}

fn get_have(download: &mut Download) -> Vec<Vec<bool>> {
    println!("Populating _have_");
    let mut file = &download.file;
    let mut have: Vec<Vec<bool>> = Vec::new();
    let torrent = &download.torrent;
    for piece_id in 0..torrent.info.piece_infos.len() {
        let mut have_blocks = Vec::new();
        let block_size = 16384;
        let mut piece_length = torrent.info.piece_length;

        if piece_id == torrent.info.piece_infos.len() - 1 {
            let standard_piece_length = piece_length;
            let in_previous = standard_piece_length * ((torrent.info.piece_infos.len() - 1) as i64);
            let remaining = torrent.info.length - in_previous;
            println!(
                "Remaining in last piece = {} = {} - {} = {} - {} * {}",
                remaining,
                torrent.info.length,
                in_previous,
                torrent.info.length,
                standard_piece_length,
                torrent.info.piece_infos.len() - 1
            );
            piece_length = remaining;
        }

        let num_blocks = ((piece_length as f64) / (block_size as f64)).ceil() as usize;

        let offset = ((piece_id as i64) * &download.torrent.info.piece_length) as u64;
        file.seek(SeekFrom::Start(offset)).unwrap();
        let mut data = vec![];
        file.take(piece_length as u64)
            .read_to_end(&mut data)
            .unwrap();

        // TODO: don't do this every time, store per piece!
        let pieces_shas: Vec<Sha1> = download
            .torrent
            .info
            .pieces
            .chunks(20)
            .map(|v| v.to_owned())
            .collect();
        let expected_hash = &pieces_shas[piece_id];

        let actual_hash = calculate_sha1(&data);

        let have_this_piece = *expected_hash == actual_hash;
        println!("Have piece_id={}? {}", piece_id, have_this_piece);

        for _ in 0..num_blocks {
            have_blocks.push(have_this_piece);
        }
        have.push(have_blocks);
    }
    return have;
}

type Sha1 = Vec<u8>;

fn calculate_sha1(input: &[u8]) -> Sha1 {
    let mut hasher = crypto::sha1::Sha1::new();
    hasher.input(input);

    let mut buf: Vec<u8> = vec![0; hasher.output_bytes()];
    hasher.result(&mut buf);
    buf
}
