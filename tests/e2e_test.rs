use assert_cmd::prelude::*;
use std::process::{Command, Stdio};

extern crate subprocess;

use subprocess::Exec;

extern crate rustorcli;

use rustorcli::torrent_entries;

use std::fs;

use std::{thread, time};

use std::path::Path;

use std::time::Duration;

use sha2::{Digest, Sha256};

use std::fs::File;

use std::io;

static TEMP_DIRECTORY: &str = "./target/tmp/e2e";
static RUSTORCLI_DIRECTORY: &str = "./target/tmp/e2e/rustorcli";
static TRANSMISSION_DIRECTORY: &str = "./target/tmp/e2e/transmission";

static ATTEMPTS: u32 = 10;
static BETWEEN_ATTEMPTS: Duration = time::Duration::from_secs(10);

// TODO: cleanup after the test, somehow
// TODO: version control test files, or generate them on the fly
// TODO: don't ignore, somehow

#[test]
#[ignore]
fn e2e_outgoing() -> Result<(), Box<dyn std::error::Error>> {
    return e2e(false, true, true);
}

#[test]
#[ignore]
fn e2e_incoming() -> Result<(), Box<dyn std::error::Error>> {
    return e2e(true, true, true);
}

fn e2e(rustorcli_first: bool, do_download: bool, do_upload: bool) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running end-to-end test. rustorcli_first={}, do_download={}, do_upload={}", rustorcli_first, do_download, do_upload);

    println!("Killing tracker");
    Exec::shell("lsof -i tcp:8000 | awk 'NR!=1 {print $2}' | xargs kill").join()?;

    println!("Starting torrent tracker");
    Command::new("bittorrent-tracker")
        .arg("--http")
        .arg("--port")
        .arg("8000")
        .stdout(Stdio::null())
        .spawn()
        .expect("bittorrent-tracker command failed to start");

    println!("Deleting past temp dir");
    std::fs::remove_dir_all(TEMP_DIRECTORY).ok();
    println!("Creating temp dirs");
    fs::create_dir_all(RUSTORCLI_DIRECTORY)?;
    fs::create_dir_all(TRANSMISSION_DIRECTORY)?;

    println!("Copying files");
    fs::copy(
        "./data/torrent_a_data",
        format!("{}/torrent_a_data", RUSTORCLI_DIRECTORY),
    )?;
    fs::copy(
        "./data/torrent_b_data",
        format!("{}/torrent_b_data", TRANSMISSION_DIRECTORY),
    )?;

    let expected_hash_a = get_hash("./data/torrent_a_data");
    let expected_hash_b = get_hash("./data/torrent_b_data");

    if rustorcli_first {
        restart_rustorcli(do_upload, do_download)?;
        restart_transmission(do_upload, do_download)?;
    } else {
        restart_transmission(do_upload, do_download)?;
        restart_rustorcli(do_upload, do_download)?;
    }

    for attempt in 1..=ATTEMPTS {
        println!("Attempt {}/{}", attempt, ATTEMPTS);

        let rustorcli_a_exists =
            Path::new(&format!("{}/torrent_a_data", RUSTORCLI_DIRECTORY)).exists();
        let rustorcli_b_exists =
            Path::new(&format!("{}/torrent_b_data", RUSTORCLI_DIRECTORY)).exists();
        let transmission_a_exists =
            Path::new(&format!("{}/torrent_a_data", TRANSMISSION_DIRECTORY)).exists();
        let transmission_b_exists =
            Path::new(&format!("{}/torrent_b_data", TRANSMISSION_DIRECTORY)).exists();

        println!(
            "Exists? rustorcli-a={}, rustorcli-b={}, transmission-a={}, transmission-b={}",
            rustorcli_a_exists, rustorcli_b_exists, transmission_a_exists, transmission_b_exists
        );

        let upload_completed = rustorcli_a_exists && transmission_a_exists;
        let download_completed = rustorcli_b_exists && transmission_b_exists;

        if (upload_completed || !do_upload) && (download_completed || !do_download)
        {
            println!("All completed!!");

            if do_upload {
                let actual_transmission_hash_a =
                get_hash(&format!("{}/torrent_a_data", TRANSMISSION_DIRECTORY));
                assert_eq!(expected_hash_a, actual_transmission_hash_a);
            }

            if do_download {
                let actual_rustorcli_hash_b =
                get_hash(&format!("{}/torrent_b_data", RUSTORCLI_DIRECTORY));
                assert_eq!(expected_hash_b, actual_rustorcli_hash_b);
            }

            return Ok(());
        }

        thread::sleep(BETWEEN_ATTEMPTS);
    }

    panic!("Never reached the desired state");
}

fn get_absolute(relative: &str) -> String {
    let cur = std::env::current_dir().unwrap();
    let cur_str = cur.to_str().unwrap();
    return format!("{}/{}", cur_str, relative);
}

fn get_hash(path: &str) -> String {
    let mut file = File::open(path).unwrap();
    let mut sha256 = Sha256::new();
    io::copy(&mut file, &mut sha256).unwrap();
    let hash = sha256.result();
    return format!("{:x}", hash);
}

fn restart_transmission(do_upload: bool, do_download: bool) -> Result<(), Box<dyn std::error::Error>> {
    println!("Killing transmission");
    Exec::shell("killall transmission-daemon").join()?;

    println!("Starting transmission");
    Command::new("transmission-daemon")
        .arg("--download-dir")
        .arg(TRANSMISSION_DIRECTORY)
        .spawn()
        .expect("Failed to start transmission");
    thread::sleep(time::Duration::from_secs(10));

    println!("Deleting everything from transmission");
    Command::new("transmission-remote")
        .arg("-t")
        .arg("all")
        .arg("-r")
        .spawn()
        .expect("Failed to cleanup transmission-remote");
    thread::sleep(time::Duration::from_secs(3));

    println!("Adding torrents to trasmission");
    if do_upload {
        Command::new("transmission-remote")
        .arg("-a")
        .arg("./data/torrent_a_data.torrent")
        .spawn()
        .expect("Failed to add torrent A to transmission");
    }
    if do_download {
        Command::new("transmission-remote")
        .arg("-a")
        .arg("./data/torrent_b_data.torrent")
        .spawn()
        .expect("Failed to add torrent B to transmission");
    }

    return Ok(());
}

fn restart_rustorcli(do_upload: bool, do_download: bool) -> Result<(), Box<dyn std::error::Error>> {
    println!("Stopping rustorcli");
    Command::main_binary()?.arg("stop").assert().success();

    println!("Cleaning up rustorcli");
    let config_directory = torrent_entries::config_directory();
    std::fs::remove_dir_all(config_directory).ok();

    println!("Adding torrents to rustorcli");
    if do_upload {
        Command::main_binary()?
        .arg("add")
        .arg("-t")
        .arg(get_absolute("./data/torrent_a_data.torrent"))
        .arg("-d")
        .arg(get_absolute(RUSTORCLI_DIRECTORY))
        .assert()
        .success();
    }
    if do_download {
        Command::main_binary()?
        .arg("add")
        .arg("-t")
        .arg(get_absolute("./data/torrent_b_data.torrent"))
        .arg("-d")
        .arg(get_absolute(RUSTORCLI_DIRECTORY))
        .assert()
        .success();
    }

    println!("Starting rustorcli");
    Command::main_binary()?.arg("start").assert().success();

    return Ok(());
}
