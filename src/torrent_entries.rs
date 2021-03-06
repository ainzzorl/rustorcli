use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::process;

use crate::util;

static TORRENT_ENTRIES_FILE: &str = "torrent-entries.json";

#[derive(Debug, Serialize, Deserialize)]
pub struct TorrentEntry {
    pub id: u32,
    pub torrent_path: String,
    pub download_path: String,
}

impl TorrentEntry {
    pub fn new(id: u32, torrent_path: String, download_path: String) -> TorrentEntry {
        TorrentEntry {
            id: id,
            torrent_path: torrent_path,
            download_path: download_path,
        }
    }
}

pub fn init_entries() {
    let entries_file_path = get_entries_file_path();
    if Path::new(&entries_file_path).exists() {
        return;
    }
    fs::create_dir_all(util::config_directory()).unwrap();
    fs::write(&entries_file_path, "[]").expect("Unable to write entries file");
}

pub fn list_torrents() -> Vec<TorrentEntry> {
    let mut file = File::open(get_entries_file_path()).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    let entries: Vec<TorrentEntry> = serde_json::from_str(&contents).unwrap();
    entries
}

pub fn add_torrent(torrent_path: &str, download_path: &str) {
    let mut entries: Vec<TorrentEntry> = list_torrents();
    let new_id = get_new_id(&entries);

    let next_id_path = format!("{}/next-id", util::config_directory());

    let next_id_str = match std::fs::read_to_string(&next_id_path) {
        Ok(content) => content,
        Err(_) => "1".to_string(),
    };
    let mut next_id = next_id_str.parse::<u32>().unwrap();
    if next_id < new_id {
        next_id = new_id;
    }

    let new_entry = TorrentEntry::new(
        next_id,
        get_absolute(torrent_path),
        get_absolute(download_path),
    );
    entries.push(new_entry);

    save(entries);

    File::create(&next_id_path)
        .unwrap()
        .write_all((next_id + 1).to_string().as_bytes())
        .unwrap();
}

pub fn remove_torrent(id: &str) {
    let entries: Vec<TorrentEntry> = list_torrents();
    let mut new_entries: Vec<TorrentEntry> = Vec::new();

    let mut found = false;
    for entry in entries {
        if entry.id.to_string() == id {
            found = true;
        } else {
            new_entries.push(entry);
        }
    }

    if !found {
        eprintln!("Id not found: {}", id);
        process::exit(1);
    }
    save(new_entries);
}

fn get_entries_file_path() -> String {
    return format!("{}/{}", util::config_directory(), TORRENT_ENTRIES_FILE);
}

fn get_new_id(entries: &Vec<TorrentEntry>) -> u32 {
    let mut new_id = 1;
    for entry in entries {
        if entry.id >= new_id {
            new_id = entry.id + 1;
        }
    }
    return new_id;
}

fn save(entries: Vec<TorrentEntry>) {
    let result_str = serde_json::to_string(&entries).unwrap();
    fs::write(get_entries_file_path(), &result_str).expect("Unable to write entries file");
}

fn get_absolute(relative: &str) -> String {
    Path::new(relative)
        .canonicalize()
        .unwrap()
        .into_os_string()
        .into_string()
        .unwrap()
}
