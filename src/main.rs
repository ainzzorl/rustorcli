extern crate serde_derive;

extern crate daemonize;
extern crate sysinfo;

pub mod announcement;
pub mod config;
pub mod decider;
pub mod download;
pub mod handshake;
pub mod io_primitives;
pub mod main_loop;
pub mod outgoing_connections;
pub mod peer_protocol;
pub mod state_persistence;
pub mod torrent_entries;
pub mod util;

use clap::{App, Arg, SubCommand};
use nix::sys::signal::Signal;
use std::fs;
use std::fs::File;
use std::process;
use sysinfo::SystemExt;

use daemonize::Daemonize;

fn main() -> () {
    torrent_entries::init_entries();

    let matches = App::new("rustorcli")
        .version("0.1")
        .author("ainzzorl <ainzzorl@gmail.com>")
        .about("BitTorrent client")
        .subcommand(
            SubCommand::with_name("start").about("starts the app").arg(
                Arg::with_name("local")
                    .short("l")
                    .long("local")
                    .help("start in local mode"),
            ),
        )
        .subcommand(SubCommand::with_name("stop").about("stops the app"))
        .subcommand(
            SubCommand::with_name("list").about("lists downloads").arg(
                Arg::with_name("peers")
                    .short("p")
                    .long("peers")
                    .help("show per-peer info"),
            ),
        )
        .subcommand(
            App::new("add")
                .about("adds download")
                .arg(
                    Arg::with_name("torrent")
                        .short("t")
                        .help("path to .torrent file")
                        .takes_value(true)
                        .value_name("torrent")
                        .required(true),
                )
                .arg(
                    Arg::with_name("destination")
                        .short("d")
                        .help("path to destination directory")
                        .takes_value(true)
                        .value_name("destination")
                        .required(true),
                ),
        )
        .subcommand(
            App::new("remove").about("removes download").arg(
                Arg::with_name("id")
                    .short("i")
                    .help("id")
                    .takes_value(true)
                    .value_name("id")
                    .required(true),
            ),
        )
        .get_matches();

    match matches.subcommand() {
        ("start", _) => {
            let subcommand_mathes = matches.subcommand_matches("start").unwrap();
            let is_local = subcommand_mathes.is_present("local");
            start(is_local);
        }
        ("stop", _) => {
            stop();
        }
        ("list", _) => {
            let subcommand_mathes = matches.subcommand_matches("list").unwrap();
            let show_per_peer = subcommand_mathes.is_present("peers");
            list(show_per_peer);
        }
        ("add", _) => {
            let subcommand_mathes = matches.subcommand_matches("add").unwrap();
            let torrent = subcommand_mathes.value_of("torrent").unwrap();
            let destination = subcommand_mathes.value_of("destination").unwrap();
            add(torrent, destination);
        }
        ("remove", _) => {
            let subcommand_mathes = matches.subcommand_matches("remove").unwrap();
            let id = subcommand_mathes.value_of("id").unwrap();
            remove(id);
        }
        _ => {
            eprintln!("Unknown subcommand!");
        }
    }
}

fn start(is_local: bool) {
    if get_pid_if_running().is_some() {
        println!("Process is already running!");
        process::exit(1);
    }

    let stdout = File::create("/tmp/rustorcli.out").unwrap();
    let stderr = File::create("/tmp/rustorcli.err").unwrap();

    let daemonize = Daemonize::new()
        .pid_file("/tmp/rustorcli.pid")
        .working_directory("/tmp")
        .umask(0o000)
        .stdout(stdout)
        .stderr(stderr)
        .privileged_action(|| "Executed before drop privileges");

    match daemonize.start() {
        Ok(_) => run(is_local),
        Err(e) => eprintln!("Error, {}", e),
    }
}

fn run(is_local: bool) {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "rustorcli=info");
    }

    let mut builder = env_logger::Builder::from_default_env();
    builder.target(env_logger::Target::Stdout);
    builder.init();

    main_loop::start(is_local);
}

fn stop() {
    match get_pid_if_running() {
        Some(pid) => {
            println!("Process is running - stopping...");
            let pd = nix::unistd::Pid::from_raw(pid);
            nix::sys::signal::kill(pd, Signal::SIGINT).unwrap();
            println!("Successfully stopped!");
        }
        None => {
            println!("Process is not running");
        }
    }
}

fn get_pid_if_running() -> Option<i32> {
    if let Some(pid) = read_pid() {
        let sys = sysinfo::System::new_all();
        match sys.get_process(pid) {
            Some(_) => {
                return Some(pid);
            }
            None => return None,
        }
    }
    None
}

fn read_pid() -> Option<i32> {
    let content_opt = fs::read_to_string("/tmp/rustorcli.pid");
    match content_opt {
        Ok(content) => {
            return Some(content.parse().unwrap());
        }
        Err(_) => {
            return None;
        }
    }
}

fn add(torrent: &str, destination: &str) {
    torrent_entries::add_torrent(torrent, destination);
}

fn remove(id: &str) {
    torrent_entries::remove_torrent(id);
}

fn list(show_per_peer: bool) {
    let separator =
        "################################################################################";
    let entries = torrent_entries::list_torrents();
    let states = state_persistence::load(&format!("{}/{}", util::config_directory(), "state.json"));
    let running = get_pid_if_running().is_some();
    for (cnt, entry) in entries.iter().enumerate() {
        let mut done = String::from("?");
        let mut downloaded = String::from("?");
        let mut uploaded = String::from("?");
        let mut size = String::from("?");
        let mut total_peers = String::from("?");
        let mut incoming_peers = String::from("?");
        let mut outgoing_peers = String::from("?");
        let mut connected_peers = String::from("?");
        let mut downloaded_pieces = String::from("?");
        let mut total_pieces = String::from("?");
        let mut pieces_ratio = String::from("?");
        let mut we_unchoked = String::from("?");
        let mut they_interested = String::from("?");
        let mut name = String::from("?");
        let mut download_speed = String::from("?");
        match states.get(&entry.id) {
            Some(state) => {
                downloaded = state.downloaded.to_string();
                uploaded = state.uploaded.to_string();
                size = state.total_size.to_string();
                done = state.done.to_string();
                total_peers = (state.incoming_peers + state.outgoing_peers).to_string();
                incoming_peers = state.incoming_peers.to_string();
                outgoing_peers = state.outgoing_peers.to_string();
                connected_peers = state.connected_peers.to_string();
                they_interested = state.they_interested.to_string();
                we_unchoked = state.we_unchoked.to_string();
                name = state.name.clone();
                total_pieces = state.total_pieces.to_string();
                downloaded_pieces = state.downloaded_pieces.to_string();
                let ratio = state.downloaded_pieces as f64 / state.total_pieces as f64;
                pieces_ratio = format!("{:.4}", ratio * 100f64);
                if running {
                    download_speed = state.download_speed.to_string();
                }
            }
            None => {}
        }
        println!("{}", separator);
        println!("Id: {}", entry.id);
        println!("Torrent: {}", entry.torrent_path);
        println!("Destination: {}/{}", entry.download_path, name);
        println!("Done: {}", done);
        println!("Size: {} bytes", size);
        println!(
            "Downloaded: {} bytes, speed: {}/sec",
            downloaded, download_speed
        );
        println!("Uploaded: {} bytes", uploaded);
        println!(
            "Have {}% of all pieces ({}/{})",
            pieces_ratio, downloaded_pieces, total_pieces
        );
        if running {
            println!(
                "Connected to {} peer(s) of total {} ({} incoming + {} outgoing)",
                connected_peers, total_peers, incoming_peers, outgoing_peers
            );
            println!(
                "Not choking us: {}/{}, interested in us: {}/{}",
                we_unchoked, connected_peers, they_interested, connected_peers
            );
            if show_per_peer {
                if let Some(state) = states.get(&entry.id) {
                    for peer in state.peers.iter() {
                        let addr = format!("{}:{}", peer.ip, peer.port);
                        println!("#{:id$} - {:addr$} Outgoing: {}, connected: {}, being connected: {}, choking: {}, interested: {}, reconnection attempts: {:reconatt$}, last incoming message: {}s, last reconnection attempt: {}s",
                            peer.id,
                            addr,
                            short_bool(peer.outgoing),
                            short_bool(peer.connected),
                            short_bool(peer.being_connected),
                            short_bool(peer.we_choked),
                            short_bool(peer.they_interested),
                            peer.reconnect_attempts,
                            peer.last_incoming_message.as_secs(),
                            peer.last_reconnect_attempt.as_secs(),
                            id = 2,
                            addr = 25,
                            reconatt = 2,
                        );
                    }
                }
            }
        }
        if cnt == entries.len() - 1 {
            println!("{}", separator);
        }
    }
}

fn short_bool(val: bool) -> &'static str {
    if val {
        "Y"
    } else {
        "N"
    }
}
