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

    pub temp_location: String,
    pub file: File,

    peers: Vec<Peer>,

    pub piece_length: i64,
    pub length: i64,
    pieces: Vec<Piece>,

    pub info_hash: Vec<u8>,
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
}

impl Peer {
    pub fn new(stream: Option<TcpStream>, peer_info: Option<PeerInfo>) -> Peer {
        Peer {
            stream: stream,
            peer_info: peer_info,
            we_interested: false,
            we_choked: true,
            outstanding_block_requests: 0,
        }
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

        let pieces = Download::init_pieces(&torrent_serializable, &mut file);
        Download {
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
        }
    }

    pub fn register_outgoing_peer(&mut self, peer_info: PeerInfo) -> usize {
        self.peers.push(Peer::new(None, Some(peer_info)));
        return self.peers.len() - 1;
    }

    pub fn register_incoming_peer(&mut self, stream: TcpStream) -> usize {
        self.peers.push(Peer::new(Some(stream), None));
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

    fn init_pieces(torrent_serializable: &TorrentSerializable, file: &mut File) -> Vec<Piece> {
        let piece_infos = parse_pieces(&torrent_serializable);

        println!("In init_pieces");
        let num_pieces = piece_infos.len();

        let mut pieces = Vec::new();

        for piece_id in 0..piece_infos.len() {
            let block_size = 16384;
            let mut piece_length = torrent_serializable.info.piece_length;

            if piece_id == num_pieces - 1 {
                let standard_piece_length = piece_length;
                let in_previous = standard_piece_length * ((num_pieces - 1) as i64);
                let remaining = torrent_serializable.info.length - in_previous;
                println!(
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
            println!("Have piece_id={}? {}", piece_id, have_this_piece);

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
