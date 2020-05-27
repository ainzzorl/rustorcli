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
use std::time::SystemTime;

use std::cmp::min;
use std::collections::VecDeque;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;

use std::convert::TryInto;
use std::os::unix::fs::OpenOptionsExt;

use failure::{bail, Error};

use self::crypto::digest::Digest;

use ring::digest;

use serde_bencode::value::Value;

use log::*;

use crate::announcement::PeerInfo;
use crate::config;
use crate::torrent_entries;

pub struct Download {
    pub id: u32,
    pub download_path: String,

    pub announcement_url: String,
    pub name: String,

    pub temp_location: String,

    pub files: Vec<DownloadFile>,

    peers: Vec<Peer>,

    pub piece_length: u64,
    pub length: u64,
    pieces: Vec<Piece>,

    pub info_hash: Vec<u8>,

    pub pending_block_requests: VecDeque<IncomingBlockRequest>,

    pub recently_downloaded_pieces: Vec<usize>,

    downloaded: bool,

    stats: Stats,

    pub last_check_with_tracker: std::time::SystemTime,
    pub tracker_interval: u64,

    pub single_file: bool,
}

pub struct DownloadFile {
    pub path: String,
    pub handle: File,
    pub length: u64,
}

pub struct Piece {
    downloaded: bool,
    pub sha: Vec<u8>,
    blocks: Vec<Block>,
    offset: u64,
    len: u64,
}

pub struct Block {
    downloaded: bool,
    offset: u64,
    len: u64,

    request_record: Option<BlockRequestRecord>,
}

pub struct BlockRequestRecord {
    pub peer_id: usize,
    pub time: std::time::SystemTime,
}

pub struct IncomingBlockRequest {
    pub peer_id: usize,
    pub begin: u64,
    pub length: u64,
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
    #[serde(default)]
    length: u64,
    #[serde(rename = "piece length")]
    piece_length: u64,
    pieces: ByteBuf,
    #[serde(default)]
    files: Vec<FileSerializable>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PieceInfo {
    pub downloaded: bool,
    pub sha: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct FileSerializable {
    pub length: u64,
    pub path: Vec<String>,
}

pub struct Peer {
    pub stream: Option<TcpStream>,
    pub buf: Vec<u8>,
    pub next_message_length: u32,

    pub we_interested: bool,
    pub we_choked: bool,
    pub they_interested: bool,

    pub peer_info: Option<PeerInfo>,

    pub outstanding_block_requests: i32,

    pub has_piece: Vec<bool>,

    pub last_incoming_message: std::time::SystemTime,
    pub last_reconnect_attempt: std::time::SystemTime,
    pub reconnect_attempts: u32,
    pub being_connected: bool,
}

impl Peer {
    pub fn new(stream: Option<TcpStream>, peer_info: Option<PeerInfo>, num_pieces: usize) -> Peer {
        Peer {
            stream: stream,
            buf: Vec::new(),
            next_message_length: 0,
            peer_info: peer_info,
            we_interested: false,
            they_interested: false,
            we_choked: true,
            outstanding_block_requests: 0,
            has_piece: vec![false; num_pieces],
            last_reconnect_attempt: std::time::SystemTime::UNIX_EPOCH,
            last_incoming_message: std::time::SystemTime::UNIX_EPOCH,
            reconnect_attempts: 0,
            being_connected: false,
        }
    }

    pub fn is_incoming(&self) -> bool {
        self.peer_info.is_none()
    }

    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    pub fn on_incoming_message(&mut self) {
        self.last_incoming_message = std::time::SystemTime::now();
    }
}

pub struct Stats {
    pub downloaded: u64,
    pub uploaded: u64,
    pub recent_downloaded: VecDeque<RecentDowloadedEntry>,
}

pub struct RecentDowloadedEntry {
    _peer_id: usize,
    size: u64,
    time: SystemTime,
}

impl Stats {
    pub fn new() -> Stats {
        Stats {
            downloaded: 0,
            uploaded: 0,
            recent_downloaded: VecDeque::new(),
        }
    }

    pub fn downloaded(&self) -> u64 {
        self.downloaded
    }

    pub fn uploaded(&self) -> u64 {
        self.uploaded
    }

    pub fn add_downloaded(&mut self, delta: u64, peer_id: usize) {
        self.downloaded += delta;
        self.recent_downloaded.push_back(RecentDowloadedEntry {
            _peer_id: peer_id,
            size: delta,
            time: SystemTime::now(),
        });
        debug!("Total downloaded: {}", self.downloaded);
    }

    pub fn add_uploaded(&mut self, delta: u64) {
        self.uploaded += delta;
        debug!("Total uploaded: {}", self.uploaded);
    }

    pub fn get_download_speed(&mut self) -> u64 {
        let size_before = self.recent_downloaded.len();
        while !self.recent_downloaded.is_empty()
            && self
                .recent_downloaded
                .front()
                .unwrap()
                .time
                .elapsed()
                .unwrap()
                > config::DOWNLOAD_SPEED_EVALUATION_INTERVAL
        {
            self.recent_downloaded.pop_front();
        }
        let size_after = self.recent_downloaded.len();
        let mut total = 0;
        for v in self.recent_downloaded.iter() {
            total += v.size;
        }
        let speed = total / config::DOWNLOAD_SPEED_EVALUATION_INTERVAL.as_secs();
        debug!(
            "recent_downloaded size before drain: {}, after: {}, total: {}, speed: {}",
            size_before, size_after, total, speed
        );
        speed
    }
}

impl Download {
    pub fn new(entry: &torrent_entries::TorrentEntry) -> Download {
        let torrent_serializable = read_torrent(&entry.torrent_path);
        let serializable_clone = torrent_serializable.clone();

        let name = torrent_serializable.info.name.clone();

        let temp_location = format!("{}/{}_part", entry.download_path, name);
        let final_location = format!("{}/{}", entry.download_path, name);

        let single_file = torrent_serializable.info.files.is_empty();
        let location = if Path::new(&final_location).exists() {
            &final_location
        } else {
            &temp_location
        };

        let mut files = Vec::new();
        if single_file {
            let path_from_download_root = "";
            let actual_path = location;
            let read = true;
            let write = location == &temp_location;
            let create = !Path::new(&location).exists();
            let handle = fs::OpenOptions::new()
                .create(create)
                .read(read)
                .write(write)
                .mode(0o770)
                .open(&actual_path)
                .unwrap();
            files.push(DownloadFile {
                path: String::from(path_from_download_root),
                length: torrent_serializable.info.length,
                handle: handle,
            });
        } else {
            if !Path::new(&location).exists() {
                fs::create_dir_all(&location).unwrap();
            }
            for file_info in torrent_serializable.info.files {
                let path_from_download_root = format!("/{}", file_info.path.join("/"));
                let file_dir = format!(
                    "{}/{}",
                    location,
                    file_info.path[0..file_info.path.len() - 1].join("/")
                );
                let actual_path = format!("{}{}", location, path_from_download_root);
                let read = true;
                let write = location == &temp_location;
                let create = !Path::new(&actual_path).exists();

                if !Path::new(&file_dir).exists() {
                    trace!("Creating directory: {}", file_dir);
                    fs::create_dir_all(&file_dir).unwrap();
                }

                debug!("Creating file at {}", actual_path);
                let handle = fs::OpenOptions::new()
                    .create(create)
                    .read(read)
                    .write(write)
                    .mode(0o770)
                    .open(&actual_path)
                    .unwrap();
                files.push(DownloadFile {
                    path: String::from(path_from_download_root),
                    length: file_info.length,
                    handle: handle,
                });
            }
        }

        let mut total_length: u64 = 0;
        for file in files.iter() {
            total_length += file.length;
        }

        let mut download = Download {
            id: entry.id,
            download_path: entry.download_path.clone(),
            announcement_url: torrent_serializable.announce,
            name: torrent_serializable.info.name,
            temp_location: temp_location,
            files: files,
            peers: Vec::new(),
            piece_length: torrent_serializable.info.piece_length,
            length: total_length,
            pieces: Vec::new(),
            info_hash: torrent_serializable.info_hash,
            pending_block_requests: VecDeque::new(),
            downloaded: false,
            recently_downloaded_pieces: Vec::new(),
            stats: Stats::new(),
            last_check_with_tracker: std::time::SystemTime::UNIX_EPOCH,
            tracker_interval: 60,
            single_file: single_file,
        };
        download.init_pieces(&serializable_clone);
        download.downloaded = download.get_is_downloaded();
        download
    }

    pub fn register_outgoing_peer(&mut self, peer_info: PeerInfo) {
        if peer_info.port == config::PORT {
            // Don't connect to self.
            // TODO: use id instead.
            debug!(
                "Skipping peer - looks like it's us! download_id={}, ip={}, port={}",
                self.id, peer_info.ip, peer_info.port
            );
            return;
        }
        for peer in self.peers.iter() {
            if let Some(info) = &peer.peer_info {
                if info.ip == peer_info.ip && info.port == peer_info.port {
                    debug!(
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
        if self.pieces[piece_id].downloaded() {
            self.on_piece_done(piece_id);
        }
        if self.get_is_downloaded() {
            self.downloaded = true;
            self.on_done();
        }
    }

    pub fn is_downloaded(&self) -> bool {
        self.downloaded
    }

    fn init_pieces(&mut self, torrent_serializable: &TorrentSerializable) {
        let piece_infos = parse_pieces(&torrent_serializable);
        let num_pieces = piece_infos.len();

        let mut pieces = Vec::new();

        for piece_id in 0..piece_infos.len() {
            let mut piece_length = torrent_serializable.info.piece_length;

            if piece_id == num_pieces - 1 {
                let standard_piece_length = piece_length;
                let in_previous = standard_piece_length * ((num_pieces - 1) as u64);
                let remaining = self.length - in_previous;
                piece_length = remaining;
            }

            let piece_offset = ((piece_id as u64) * &torrent_serializable.info.piece_length) as u64;
            let data = self.get_content(piece_offset as u64, piece_length);

            let pieces_shas: Vec<Sha1> = torrent_serializable
                .info
                .pieces
                .chunks(20)
                .map(|v| v.to_owned())
                .collect();
            let expected_hash = pieces_shas[piece_id].clone();

            let actual_hash = calculate_sha1(&data);

            let have_this_piece = expected_hash == actual_hash;

            let num_blocks = ((piece_length as f64) / (config::BLOCK_SIZE as f64)).ceil() as usize;

            let mut blocks = Vec::new();

            for block_id in 0..num_blocks {
                let block_offset = (block_id as u64 * config::BLOCK_SIZE) as u64;
                let block_len = min(
                    self.length - (piece_offset as u64) - block_offset as u64,
                    config::BLOCK_SIZE,
                );
                blocks.push(Block {
                    downloaded: have_this_piece,
                    offset: block_offset,
                    len: block_len,
                    request_record: None,
                })
            }

            pieces.push(Piece {
                downloaded: have_this_piece,
                sha: expected_hash,
                blocks: blocks,
                offset: piece_offset,
                len: piece_length,
            });
        }
        self.pieces = pieces;
    }

    pub fn get_content(&mut self, offset: u64, len: u64) -> Vec<u8> {
        trace!("Reading content. offset={}, len={}", offset, len);
        let mut result = vec![];

        let mut file_start: u64 = 0;
        for file in self.files.iter_mut() {
            let next_file_start = file_start + file.length;
            if next_file_start <= offset {
                file_start = next_file_start;
                continue;
            }

            // Need to read from this file!
            let remaining_to_read = len - result.len() as u64;
            let file_offset = if file_start <= offset {
                offset - file_start
            } else {
                0
            };
            let left_in_file = file.length as u64 - file_offset;
            let read_from_this_file = std::cmp::min(left_in_file, remaining_to_read);

            let handle: &mut File = &mut file.handle;

            handle.seek(std::io::SeekFrom::Start(file_offset)).unwrap();
            let mut data = vec![];
            handle
                .take(read_from_this_file)
                .read_to_end(&mut data)
                .unwrap();
            // To deal with empty and partial files.
            while data.len() < read_from_this_file as usize {
                data.push(0);
            }
            result.extend(data);
            if result.len() as u64 >= len {
                return result;
            }
            file_start = next_file_start;
        }
        panic!(
            "Reached the end of the file but never found the content! offset={}, len={}",
            offset, len
        );
    }

    pub fn set_content(&mut self, offset: u64, data: &[u8]) {
        trace!("Writing content, offset={}, len={}", offset, data.len());
        let mut remaining_data = data;

        let mut file_start: u64 = 0;
        for file in self.files.iter_mut() {
            let next_file_start = file_start + file.length;
            if next_file_start <= offset {
                file_start = next_file_start;
                continue;
            }

            // Need to write to this file!
            let remaining_to_write = remaining_data.len() as u64;
            let file_offset = if file_start <= offset {
                offset - file_start
            } else {
                0
            };
            let left_in_file = file.length as u64 - file_offset;
            let write_to_this_file = std::cmp::min(left_in_file, remaining_to_write);

            let handle: &mut File = &mut file.handle;

            handle.seek(SeekFrom::Start(file_offset)).unwrap();
            let data_to_write = &remaining_data[..write_to_this_file as usize];
            handle.write(data_to_write).unwrap();
            remaining_data = &remaining_data[write_to_this_file as usize..];

            if remaining_data.is_empty() {
                return;
            }
            file_start = next_file_start;
        }
        panic!(
            "Reached the end of the file but never wrote all content! offset={}, remaining data={}",
            offset,
            remaining_data.len()
        );
    }

    pub fn on_piece_done(&mut self, piece_id: usize) {
        let piece = &self.pieces()[piece_id];
        let offset = piece.offset;
        let len = piece.len;

        let data = self.get_content(offset, len);

        let expected_hash = self.pieces()[piece_id].sha();

        let actual_hash = calculate_sha1(&data);

        let is_complete = *expected_hash == actual_hash;
        if is_complete {
            info!("Hashes match for piece {}", piece_id);
            self.recently_downloaded_pieces.push(piece_id);
        } else {
            error!(
                "Hashes don't match for piece {}. Expected: {}, actual: {}. Resetting the piece...",
                piece_id,
                hex::encode(expected_hash),
                hex::encode(actual_hash)
            );
            let piece = &mut self.pieces_mut()[piece_id];
            piece.reset();
        }
    }

    pub fn on_broken_connection(&mut self, peer_id: usize) {
        warn!(
            "Resetting connection, download_id={}, peer_id={}",
            self.id, peer_id
        );
        self.peers[peer_id].stream = None;
        self.peers[peer_id].buf = Vec::new();
        self.peers[peer_id].next_message_length = 0;
    }

    fn on_done(&self) {
        info!("Download is done!");
        let dest = format!("{}/{}", self.download_path, self.name);
        info!("Moving {} to {}", self.temp_location, dest);
        fs::rename(&self.temp_location, dest).unwrap();
    }

    fn get_is_downloaded(&self) -> bool {
        debug!("Checking if the download is done...");
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
            debug!(
                "Not yet done: downloaded {}/{} blocks. E.g. missing: {}",
                downloaded_blocks, num_blocks, missing
            );
            return false;
        }
        info!("The download is done!");
        true
    }
}

impl Piece {
    pub fn downloaded(&self) -> bool {
        self.downloaded
    }

    pub fn len(&self) -> u64 {
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

    pub fn offset(&self) -> u64 {
        self.offset
    }

    fn reset(&mut self) {
        self.downloaded = false;
        for block in self.blocks.iter_mut() {
            block.reset();
        }
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

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn request_record(&self) -> &Option<BlockRequestRecord> {
        &self.request_record
    }

    pub fn set_request_record(&mut self, request_record: Option<BlockRequestRecord>) {
        self.request_record = request_record;
    }

    fn reset(&mut self) {
        // TODO: reset request record? But then would need to update outstanding count...
        self.downloaded = false;
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
