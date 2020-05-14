use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use log::*;

use serde::{Deserialize, Serialize};

use crate::download::Download;

#[derive(Debug, Serialize, Deserialize)]
pub struct PersistentDownloadState {
    pub uploaded: u32,
    pub downloaded: u32,
}

pub fn persist(downloads: &mut HashMap<u32, Download>, location: &String) {
    info!("Persisting state to {}", location);
    let mut state_map: HashMap<u32, PersistentDownloadState> = HashMap::new();
    for (download_id, download) in downloads {
        state_map.insert(
            *download_id,
            PersistentDownloadState {
                uploaded: download.stats().uploaded(),
                downloaded: download.stats().downloaded(),
            },
        );
    }
    serde_json::to_writer(&File::create(location).unwrap(), &state_map).unwrap();
}

pub fn load(location: &String) -> HashMap<u32, PersistentDownloadState> {
    info!("Loading state to {}", location);
    if !Path::new(location).exists() {
        info!("State file does not exist");
        return HashMap::new();
    }
    let mut file = File::open(location).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    serde_json::from_str(&contents).unwrap()
}
