use std::collections::HashMap;
use std::fs::File;

use log::*;

use serde::{Deserialize, Serialize};

use crate::download::Download;

#[derive(Debug, Serialize, Deserialize)]
struct PersistentDownloadState {
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
