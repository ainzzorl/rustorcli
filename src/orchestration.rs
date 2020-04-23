extern crate notify;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::time::Duration;

use std::collections::HashMap;

use std::{thread, time};

use rustorcli::torrent_entries;

fn reload_config(downloads: &mut HashMap<u32, torrent_entries::TorrentEntry>) {
    let entries = torrent_entries::list_torrents();
    for entry in entries {
        if !downloads.contains_key(&entry.id) {
            println!("Adding entry, id={}", entry.id);
            downloads.insert(entry.id, entry);
        }
    }

    // TODO: remove removed downloads
}

pub fn start() {
    println!("Starting orchestration");
    let mut downloads: HashMap<u32, torrent_entries::TorrentEntry> = HashMap::new();
    reload_config(&mut downloads);
    main_loop(&mut downloads);
}

fn main_loop(downloads: &mut HashMap<u32, torrent_entries::TorrentEntry>) {
    // TODO: extract this to some method
    let (tx, rx) = channel();
    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(2)).unwrap();
    let config_directory = torrent_entries::config_directory();
    watcher
        .watch(config_directory, RecursiveMode::Recursive)
        .unwrap();

    loop {
        println!("Main loop iteration");

        match rx.recv_timeout(time::Duration::from_millis(1000)) {
            Ok(event) => {
                println!("{:?}", event);
                reload_config(downloads);
            }
            Err(e) => println!("watch error: {:?}", e),
        }

        // TODO: do work!

        thread::sleep(time::Duration::from_millis(10000));
    }
}
