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

static TEMP_DIRECTORY: &str = "./target/tmp/e2e";
static RUSTORCLI_DIRECTORY: &str = "./target/tmp/e2e/rustorcli";
static TRANSMISSION_DIRECTORY: &str = "./target/tmp/transmission";

static ATTEMPTS: u32 = 10;
static BETWEEN_ATTEMPTS: Duration = time::Duration::from_secs(10);

// TODO: cleanup after the test, somehow
// TODO: verify hashes
// TODO: version control test files, or generate them on the fly
// TODO: don't ignore, somehow

#[test]
#[ignore]
fn e2e() -> Result<(), Box<dyn std::error::Error>> {
    println!("Running end-to-end test");

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
    Command::new("transmission-remote")
        .arg("-a")
        .arg("./data/torrent_a_data.torrent")
        .spawn()
        .expect("Failed to add torrent A to transmission");
    Command::new("transmission-remote")
        .arg("-a")
        .arg("./data/torrent_b_data.torrent")
        .spawn()
        .expect("Failed to add torrent B to transmission");

    println!("Cleaning up rustorcli");
    let config_directory = torrent_entries::config_directory();
    std::fs::remove_dir_all(config_directory).unwrap();

    println!("Stopping rustorcli");
    Command::main_binary()?.arg("stop").assert().success();

    println!("Starting rustorcli");
    Command::main_binary()?.arg("start").assert().success();

    println!("Adding torrents to rustorcli");
    Command::main_binary()?
        .arg("add")
        .arg("-t")
        .arg("./data/torrent_a_data.torrent")
        .arg("-d")
        .arg(RUSTORCLI_DIRECTORY)
        .assert()
        .success();
    Command::main_binary()?
        .arg("add")
        .arg("-t")
        .arg("./data/torrent_b_data.torrent")
        .arg("-d")
        .arg(RUSTORCLI_DIRECTORY)
        .assert()
        .success();

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

        if rustorcli_a_exists
            && rustorcli_b_exists
            && transmission_a_exists
            && transmission_b_exists
        {
            println!("All files exist!");
            return Ok(());
        }

        thread::sleep(BETWEEN_ATTEMPTS);
    }

    panic!("Never reached the desired state");
}
