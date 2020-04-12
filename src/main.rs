extern crate daemonize;
extern crate sysinfo;

use std::fs;
use std::fs::File;
use std::{thread, time};
use std::process;
use nix::sys::signal::Signal;
use clap::{App, SubCommand};
use sysinfo::SystemExt;


use daemonize::Daemonize;

fn main() -> () {
    let pid_opt = read_pid();

    let matches = App::new("rustorcli")
                    .version("0.1")
                    .author("ainzzorl <ainzzorl@gmail.com>")
                    .about("BitTorrent client")
                    .subcommand(SubCommand::with_name("start")
                        .about("starts the app"))
                    .subcommand(SubCommand::with_name("stop")
                        .about("stops the app"))
                    .subcommand(SubCommand::with_name("list")
                        .about("lists downloads"))
                    .subcommand(SubCommand::with_name("add")
                        .about("adds download"))
                    .subcommand(SubCommand::with_name("remove")
                        .about("removes download"))
                    .get_matches();

                    println!("Something matched");
    match matches.subcommand() {
        ("start", _) => {
            println!("Start subcommand!");
            start(pid_opt);
        },
        ("stop", _) => {
            println!("Stop subcommand!");
            stop(pid_opt);
        },
        ("list", _) => {
            println!("List subcommand!");
        },
        ("add", _) => {
            println!("Add subcommand!");
        },
        ("remove", _) => {
            println!("Remove subcommand!");
        },
        _ => {
            println!("Unknown subcommand!");
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
                },
                None => {
                    println!("Process not found");
                }
            }
        },
        None => {
            println!("Pid not present");
        }
    }
    let stdout = File::create("/tmp/rustorcli.out").unwrap();
    let stderr = File::create("/tmp/rustorcli.err").unwrap();

    let daemonize = Daemonize::new()
        .pid_file("/tmp/rustorcli.pid") // Every method except `new` and `start`
        .working_directory("/tmp")      // for default behaviour.
        .umask(0o777)                   // Set umask, `0o027` by default.
        .stdout(stdout)                 // Redirect stdout to `/tmp/daemon.out`.
        .stderr(stderr)                 // Redirect stderr to `/tmp/daemon.err`.
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
                },
                None => {
                    println!("Process is not running");
                }
            }
        },
        None => {
            println!("No pid - do nothing");
        }
    }
}

fn read_pid() -> Option<i32> {
    let content_opt = fs::read_to_string("/tmp/rustorcli.pid");
    match content_opt {
        Ok(content) => {
            println!("Pid: {}", content);
            return Some(content.parse().unwrap());
        }
        Err(_) => {
            return None;
        }
    }
}