extern crate crypto;
extern crate serde;

use serde::Deserialize;
use serde::Serialize;

use serde_bencode::de;
use serde_bytes::ByteBuf;

use std::fs;
use std::fs::File;
use std::net::TcpStream;
use std::path::Path;

use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use std::convert::TryInto;
use std::os::unix::fs::OpenOptionsExt;

use failure::{bail, Error};

use self::crypto::digest::Digest;

use ring::digest;

use serde_bencode::value::Value;

use crate::announcement::PeerInfo;
use crate::torrent_entries;

pub struct Download {
    pub id: u32,
    pub download_path: String,

    pub announcement_url: String,
    pub name: String,

    pub connections: Vec<Option<TcpStream>>,
    pub we_interested: Vec<bool>,
    pub we_choked: Vec<bool>,
    pub temp_location: String,
    pub file: File,
    pub have_block: Vec<Vec<bool>>,

    pub peers: Vec<PeerInfo>,
    pub piece_infos: Vec<PieceInfo>,
    pub piece_length: i64,
    pub length: i64,
    pub pieces: ByteBuf,

    pub info_hash: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TorrentSerializable {
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PieceInfo {
    pub downloaded: bool,
    pub sha: Vec<u8>,
}

impl Download {
    pub fn new(entry: &torrent_entries::TorrentEntry) -> Download {
        let torrent_serializable = read_torrent(&entry.torrent_path);

        let name = torrent_serializable.info.name.clone();

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

        let piece_infos = parse_pieces(&torrent_serializable);
        let mut download = Download {
            id: entry.id,
            download_path: entry.download_path.clone(),
            announcement_url: torrent_serializable.announce,
            name: torrent_serializable.info.name,
            connections: Vec::new(),
            we_interested: Vec::new(),
            we_choked: Vec::new(),
            temp_location: temp_location,
            file: file,
            have_block: Vec::new(),
            peers: Vec::new(),
            piece_infos: piece_infos,
            piece_length: torrent_serializable.info.piece_length,
            length: torrent_serializable.info.length,
            pieces: torrent_serializable.info.pieces,
            info_hash: torrent_serializable.info_hash,
        };

        let have = get_have(&mut download);
        download.have_block = have;

        return download;
    }

    pub fn register_peer(&mut self, peer_info: PeerInfo) {
        self.connections.push(None);
        self.we_interested.push(false);
        self.we_choked.push(true);
        self.peers.push(peer_info);
    }
}

fn get_have(download: &mut Download) -> Vec<Vec<bool>> {
    println!("Populating _have_");
    let mut file = &download.file;
    let mut have: Vec<Vec<bool>> = Vec::new();
    for piece_id in 0..download.piece_infos.len() {
        let mut have_blocks = Vec::new();
        let block_size = 16384;
        let mut piece_length = download.piece_length;

        if piece_id == download.piece_infos.len() - 1 {
            let standard_piece_length = piece_length;
            let in_previous = standard_piece_length * ((download.piece_infos.len() - 1) as i64);
            let remaining = download.length - in_previous;
            println!(
                "Remaining in last piece = {} = {} - {} = {} - {} * {}",
                remaining,
                download.length,
                in_previous,
                download.length,
                standard_piece_length,
                download.piece_infos.len() - 1
            );
            piece_length = remaining;
        }

        let num_blocks = ((piece_length as f64) / (block_size as f64)).ceil() as usize;

        let offset = ((piece_id as i64) * &download.piece_length) as u64;
        file.seek(SeekFrom::Start(offset)).unwrap();
        let mut data = vec![];
        file.take(piece_length as u64)
            .read_to_end(&mut data)
            .unwrap();

        // TODO: don't do this every time, store per piece!
        let pieces_shas: Vec<Sha1> = download.pieces.chunks(20).map(|v| v.to_owned()).collect();
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

fn parse_pieces(torrent: &TorrentSerializable) -> Vec<PieceInfo> {
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

    return piece_infos;
}

fn read_torrent(path: &String) -> TorrentSerializable {
    println!("Reading torrent file from path: {}", path);
    let mut f = File::open(&path).expect("no file found");
    let metadata = fs::metadata(&path).expect("unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer).expect("buffer overflow");

    let mut result = de::from_bytes::<TorrentSerializable>(&buffer).unwrap();
    result.info_hash = get_info_hash(&buffer).unwrap();

    return result;
}

fn get_info_hash(data: &[u8]) -> Result<Vec<u8>, Error> {
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
