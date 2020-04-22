extern crate daemonize;
extern crate sysinfo;

mod torrent_entries;

use clap::{App, Arg, SubCommand};
use nix::sys::signal::Signal;
use std::fs;
use std::fs::File;
use std::process;
use std::{thread, time};
use sysinfo::SystemExt;

use daemonize::Daemonize;

fn main() -> () {
    torrent_entries::init_entries();

    let pid_opt = read_pid();

    let matches = App::new("rustorcli")
        .version("0.1")
        .author("ainzzorl <ainzzorl@gmail.com>")
        .about("BitTorrent client")
        .subcommand(SubCommand::with_name("start").about("starts the app"))
        .subcommand(SubCommand::with_name("stop").about("stops the app"))
        .subcommand(SubCommand::with_name("list").about("lists downloads"))
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
            start(pid_opt);
        }
        ("stop", _) => {
            stop(pid_opt);
        }
        ("list", _) => {
            list();
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

fn start(pid_opt: Option<i32>) {
    match pid_opt {
        Some(pid) => {
            let sys = sysinfo::System::new_all();
            let process = sys.get_process(pid);
            match process {
                Some(_) => {
                    println!("Process is already running!");
                    process::exit(1);
                }
                None => {
                    println!("Process not found");
                }
            }
        }
        None => {
            println!("Pid not present");
        }
    }
    let stdout = File::create("/tmp/rustorcli.out").unwrap();
    let stderr = File::create("/tmp/rustorcli.err").unwrap();

    let daemonize = Daemonize::new()
        .pid_file("/tmp/rustorcli.pid")
        .working_directory("/tmp")
        .umask(0o777)
        .stdout(stdout)
        .stderr(stderr)
        .privileged_action(|| "Executed before drop privileges");

    match daemonize.start() {
        Ok(_) => run(),
        Err(e) => eprintln!("Error, {}", e),
    }
}

fn run() {
    loop {
        println!("Kinda running");
        thread::sleep(time::Duration::from_secs(10));
    }
}

fn stop(pid_opt: Option<i32>) {
    match pid_opt {
        Some(pid) => {
            println!("Pid is present");
            let sys = sysinfo::System::new_all();
            let process = sys.get_process(pid);
            match process {
                Some(_) => {
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
        None => {
            println!("No pid - do nothing");
        }
    }
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

fn list() {
    let entries = torrent_entries::list_torrents();
    for entry in entries {
        println!(
            "{} - {} - {}",
            entry.id, entry.torrent_path, entry.download_path
        );
    }
}
