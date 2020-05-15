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

use std::cmp::min;
use std::collections::VecDeque;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use std::convert::TryInto;
use std::os::unix::fs::OpenOptionsExt;

use failure::{bail, Error};

use self::crypto::digest::Digest;

use ring::digest;

use serde_bencode::value::Value;

use log::*;

use crate::announcement::PeerInfo;
use crate::torrent_entries;

pub struct Download {
    pub id: u32,
    pub download_path: String,

    pub announcement_url: String,
    pub name: String,

    pub temp_location: String,
    pub file: File,

    peers: Vec<Peer>,

    pub piece_length: i64,
    pub length: i64,
    pieces: Vec<Piece>,

    pub info_hash: Vec<u8>,

    pub pending_block_requests: VecDeque<IncomingBlockRequest>,

    pub recently_downloaded_pieces: Vec<usize>,

    downloaded: bool,

    stats: Stats,

    pub last_check_with_tracker: std::time::SystemTime,
    pub tracker_interval: u64,
}

pub struct Piece {
    downloaded: bool,
    pub sha: Vec<u8>,
    blocks: Vec<Block>,
    offset: u32,
    len: u32,
}

pub struct Block {
    downloaded: bool,
    offset: u64,
    len: u32,

    request_record: Option<BlockRequestRecord>,
}

pub struct BlockRequestRecord {
    pub peer_id: usize,
    pub time: std::time::SystemTime,
}

pub struct IncomingBlockRequest {
    pub peer_id: usize,
    pub begin: usize,
    pub length: usize,
    pub piece_id: usize,
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

pub struct Peer {
    pub stream: Option<TcpStream>,

    pub we_interested: bool,
    pub we_choked: bool,

    pub peer_info: Option<PeerInfo>,

    pub outstanding_block_requests: i32,

    pub has_piece: Vec<bool>,

    pub last_reconnect_attempt: std::time::SystemTime,
    pub reconnect_attempts: u32,
    pub being_connected: bool,
}

impl Peer {
    pub fn new(stream: Option<TcpStream>, peer_info: Option<PeerInfo>, num_pieces: usize) -> Peer {
        Peer {
            stream: stream,
            peer_info: peer_info,
            we_interested: false,
            we_choked: true,
            outstanding_block_requests: 0,
            has_piece: vec![false; num_pieces],
            last_reconnect_attempt: std::time::SystemTime::UNIX_EPOCH,
            reconnect_attempts: 0,
            being_connected: false,
        }
    }
}

pub struct Stats {
    pub downloaded: u32,
    pub uploaded: u32,
}

impl Stats {
    pub fn new() -> Stats {
        Stats {
            downloaded: 0,
            uploaded: 0,
        }
    }

    pub fn downloaded(&self) -> u32 {
        self.downloaded
    }

    pub fn uploaded(&self) -> u32 {
        self.uploaded
    }

    pub fn add_downloaded(&mut self, delta: u32) {
        self.downloaded += delta;
        debug!("Total downloaded: {}", self.downloaded);
    }

    pub fn add_uploaded(&mut self, delta: u32) {
        self.uploaded += delta;
        debug!("Total uploaded: {}", self.uploaded);
    }
}

impl Download {
    pub fn new(entry: &torrent_entries::TorrentEntry) -> Download {
        let torrent_serializable = read_torrent(&entry.torrent_path);

        let name = torrent_serializable.info.name.clone();

        let temp_location = format!("{}/{}_part", entry.download_path, name);
        let final_location = format!("{}/{}", entry.download_path, name);
        let mut file: File;

        if !Path::new(&temp_location).exists() && !Path::new(&final_location).exists() {
            info!("Creating temp file: {}", &temp_location);
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

        let pieces = Download::init_pieces(&torrent_serializable, &mut file);

        let mut download = Download {
            id: entry.id,
            download_path: entry.download_path.clone(),
            announcement_url: torrent_serializable.announce,
            name: torrent_serializable.info.name,
            temp_location: temp_location,
            file: file,
            peers: Vec::new(),
            piece_length: torrent_serializable.info.piece_length,
            length: torrent_serializable.info.length,
            pieces: pieces,
            info_hash: torrent_serializable.info_hash,
            pending_block_requests: VecDeque::new(),
            downloaded: false,
            recently_downloaded_pieces: Vec::new(),
            stats: Stats::new(),
            last_check_with_tracker: std::time::SystemTime::UNIX_EPOCH,
            tracker_interval: 60,
        };
        download.downloaded = download.get_is_downloaded();
        download
    }

    pub fn register_outgoing_peer(&mut self, peer_info: PeerInfo) {
        for peer in self.peers.iter() {
            if let Some(info) = &peer.peer_info {
                if info.ip == peer_info.ip && info.port == peer_info.port {
                    info!(
                        "Skipping peer - already registered. download_id={}, ip={}, port={}",
                        self.id, peer_info.ip, peer_info.port
                    );
                    return;
                }
            }
        }
        info!(
            "Registered new peer. download_id={}, ip={}, port={}",
            self.id, peer_info.ip, peer_info.port
        );
        self.peers
            .push(Peer::new(None, Some(peer_info), self.pieces.len()));
    }

    pub fn register_incoming_peer(&mut self, stream: TcpStream) -> usize {
        self.peers
            .push(Peer::new(Some(stream), None, self.pieces.len()));
        return self.peers.len() - 1;
    }

    pub fn peers(&self) -> &Vec<Peer> {
        &self.peers
    }

    pub fn peers_mut(&mut self) -> &mut Vec<Peer> {
        &mut self.peers
    }

    pub fn peer(&self, peer_id: usize) -> &Peer {
        &self.peers[peer_id]
    }

    pub fn peer_mut(&mut self, peer_id: usize) -> &mut Peer {
        &mut self.peers[peer_id]
    }

    pub fn pieces(&self) -> &Vec<Piece> {
        &self.pieces
    }

    pub fn pieces_mut(&mut self) -> &mut Vec<Piece> {
        &mut self.pieces
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn set_stats(&mut self, stats: Stats) {
        self.stats = stats;
    }

    pub fn stats_mut(&mut self) -> &mut Stats {
        &mut self.stats
    }

    pub fn add_incoming_block_request(&mut self, request: IncomingBlockRequest) {
        self.pending_block_requests.push_back(request);
    }

    pub fn set_block_downloaded(&mut self, piece_id: usize, block_id: usize) {
        self.pieces[piece_id].set_block_downloaded(block_id);
        self.check_if_piece_done(piece_id);
        if self.get_is_downloaded() {
            self.downloaded = true;
            self.on_done();
        }
    }

    pub fn is_downloaded(&self) -> bool {
        self.downloaded
    }

    fn init_pieces(torrent_serializable: &TorrentSerializable, file: &mut File) -> Vec<Piece> {
        let piece_infos = parse_pieces(&torrent_serializable);

        info!("In init_pieces");
        let num_pieces = piece_infos.len();

        let mut pieces = Vec::new();

        for piece_id in 0..piece_infos.len() {
            let block_size = 16384;
            let mut piece_length = torrent_serializable.info.piece_length;

            if piece_id == num_pieces - 1 {
                let standard_piece_length = piece_length;
                let in_previous = standard_piece_length * ((num_pieces - 1) as i64);
                let remaining = torrent_serializable.info.length - in_previous;
                info!(
                    "Remaining in last piece = {} = {} - {} = {} - {} * {}",
                    remaining,
                    torrent_serializable.info.length,
                    in_previous,
                    torrent_serializable.info.length,
                    standard_piece_length,
                    num_pieces - 1
                );
                piece_length = remaining;
            }

            let piece_offset = ((piece_id as i64) * &torrent_serializable.info.piece_length) as u64;

            file.seek(SeekFrom::Start(piece_offset)).unwrap();
            let mut data = vec![];
            file.take(piece_length as u64)
                .read_to_end(&mut data)
                .unwrap();

            // TODO: don't do this every time, store per piece!
            let pieces_shas: Vec<Sha1> = torrent_serializable
                .info
                .pieces
                .chunks(20)
                .map(|v| v.to_owned())
                .collect();
            let expected_hash = pieces_shas[piece_id].clone();

            let actual_hash = calculate_sha1(&data);

            let have_this_piece = expected_hash == actual_hash;
            info!("Have piece_id={}? {}", piece_id, have_this_piece);

            let num_blocks = ((piece_length as f64) / (block_size as f64)).ceil() as usize;

            let mut blocks = Vec::new();

            for block_id in 0..num_blocks {
                let block_offset = (block_id * block_size) as u64;
                let block_len = min(
                    torrent_serializable.info.length - (piece_offset as i64) - block_offset as i64,
                    block_size as i64,
                );
                blocks.push(Block {
                    downloaded: have_this_piece,
                    offset: block_offset,
                    len: block_len as u32,
                    request_record: None,
                })
            }

            pieces.push(Piece {
                downloaded: have_this_piece,
                sha: expected_hash,
                blocks: blocks,
                offset: piece_offset as u32,
                len: piece_length as u32,
            });
        }

        return pieces;
    }

    pub fn check_if_piece_done(&mut self, piece_id: usize) {
        let piece = &self.pieces()[piece_id];
        if !piece.downloaded() {
            return;
        }
        info!("Piece {} seems to be downloaded...", piece_id);

        let mut file = &self.file;

        file.seek(std::io::SeekFrom::Start(piece.offset().into()))
            .unwrap();
        let mut data = vec![];
        file.take(self.piece_length as u64)
            .read_to_end(&mut data)
            .unwrap();

        let expected_hash = self.pieces()[piece_id].sha();

        let actual_hash = calculate_sha1(&data);

        let is_complete = *expected_hash == actual_hash;
        if is_complete {
            info!("Hashes match for piece {}", piece_id);
            self.recently_downloaded_pieces.push(piece_id);
        } else {
            panic!(
                "Hashes don't match for piece {}. Expected: {}, actual: {}",
                piece_id,
                hex::encode(expected_hash),
                hex::encode(actual_hash)
            );
            // TODO: do something smarter, e.g. retry
        }
    }

    pub fn on_broken_connection(&mut self, peer_id: usize) {
        warn!(
            "Resetting connection, download_id={}, peer_id={}",
            self.id, peer_id
        );
        self.peers[peer_id].stream = None;
    }

    fn on_done(&self) {
        info!("Download is done!");
        let dest = format!("{}/{}", self.download_path, self.name);
        info!("Moving {} to {}", self.temp_location, dest);
        fs::rename(&self.temp_location, dest).unwrap();
    }

    fn get_is_downloaded(&self) -> bool {
        info!("Checking if the download is done...");
        let mut num_blocks = 0;
        let mut downloaded_blocks = 0;
        let mut missing: String = String::from("");

        for p_id in 0..self.pieces().len() {
            let p = &self.pieces()[p_id];
            if p.downloaded {
                num_blocks += p.blocks.len();
                downloaded_blocks += p.blocks.len();
                continue;
            }
            for b_id in 0..p.blocks().len() {
                let b = p.blocks()[b_id].downloaded();
                num_blocks += 1;
                if b {
                    downloaded_blocks += 1;
                } else {
                    missing = format!("piece={}, block={}", p_id, b_id);
                }
            }
        }
        if num_blocks != downloaded_blocks {
            info!(
                "Not yet done: downloaded {}/{} blocks. E.g. missing: {}",
                downloaded_blocks, num_blocks, missing
            );
        }
        return num_blocks == downloaded_blocks;
    }
}

impl Piece {
    pub fn downloaded(&self) -> bool {
        self.downloaded
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn set_block_downloaded(&mut self, block_id: usize) {
        self.blocks[block_id].set_downloaded();
        for block in &self.blocks {
            if !block.downloaded {
                return;
            }
        }
        self.downloaded = true;
    }

    pub fn blocks(&self) -> &Vec<Block> {
        &self.blocks
    }

    pub fn blocks_mut(&mut self) -> &mut Vec<Block> {
        &mut self.blocks
    }

    pub fn sha(&self) -> &Vec<u8> {
        &self.sha
    }

    pub fn offset(&self) -> u32 {
        self.offset
    }
}

impl Block {
    pub fn set_downloaded(&mut self) {
        self.downloaded = true;
    }

    pub fn downloaded(&self) -> bool {
        self.downloaded
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn request_record(&self) -> &Option<BlockRequestRecord> {
        &self.request_record
    }

    pub fn set_request_record(&mut self, request_record: Option<BlockRequestRecord>) {
        self.request_record = request_record;
    }
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
    info!("Parsing pieces...");
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
    info!("Total pieces: {}", piece_infos.len());

    return piece_infos;
}

fn read_torrent(path: &String) -> TorrentSerializable {
    info!("Reading torrent file from path: {}", path);
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
