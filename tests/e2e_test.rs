mod e2e_tests {

    use assert_cmd::prelude::*;
    use std::process::{Command, Stdio};

    extern crate subprocess;

    use subprocess::Exec;

    extern crate rustorcli;

    use rustorcli::util;

    use std::fs;

    use std::{thread, time};

    use std::path::Path;

    use std::time::Duration;

    use sha2::{Digest, Sha256};

    use std::fs::File;

    use std::io;

    static TEMP_DIRECTORY: &str = "./target/tmp/e2e";
    static ATTEMPTS: u32 = 100;
    static BETWEEN_ATTEMPTS: Duration = time::Duration::from_secs(1);

    // TODO: don't ignore, somehow

    #[test]
    #[ignore]
    fn e2e_outgoing_with_transmission() -> Result<(), Box<dyn std::error::Error>> {
        e2e_from_definition(E2ETestDefinition {
            description: String::from(
                "Outgoing connection from rustorcli to transmission; both upload and download",
            ),
            clients: vec![
                Client {
                    client_type: ClientType::TRANSMISSION,
                    starting_files: vec![String::from("torrent_a_data")],
                    expected_files: vec![
                        String::from("torrent_a_data"),
                        String::from("torrent_b_data"),
                    ],
                    torrents: vec![
                        String::from("torrent_a_data.torrent"),
                        String::from("torrent_b_data.torrent"),
                    ],
                },
                Client {
                    client_type: ClientType::RUSTORCLI,
                    starting_files: vec![String::from("torrent_b_data")],
                    expected_files: vec![
                        String::from("torrent_a_data"),
                        String::from("torrent_b_data"),
                    ],
                    torrents: vec![
                        String::from("torrent_a_data.torrent"),
                        String::from("torrent_b_data.torrent"),
                    ],
                },
            ],
        })
    }

    #[test]
    #[ignore]
    fn e2e_outgoing_three_way() -> Result<(), Box<dyn std::error::Error>> {
        e2e_from_definition(E2ETestDefinition {
        description: String::from(
            "Outgoing connection from rustorcli to transmission and webtorrent; both upload and download; all three start with different files",
        ),
        clients: vec![
            Client {
                client_type: ClientType::TRANSMISSION,
                starting_files: vec![String::from("torrent_a_data")],
                expected_files: vec![
                    String::from("torrent_a_data"),
                    String::from("torrent_b_data"),
                    String::from("torrent_c_data"),
                ],
                torrents: vec![
                    String::from("torrent_a_data.torrent"),
                    String::from("torrent_b_data.torrent"),
                    String::from("torrent_c_data.torrent"),
                ],
            },
            Client {
                client_type: ClientType::WEBTORRENT,
                starting_files: vec![String::from("torrent_b_data")],
                expected_files: vec![
                    String::from("torrent_a_data"),
                    String::from("torrent_b_data"),
                    String::from("torrent_c_data"),
                ],
                torrents: vec![
                    String::from("torrent_a_data.torrent"),
                    String::from("torrent_b_data.torrent"),
                    String::from("torrent_c_data.torrent"),
                ],
            },
            Client {
                client_type: ClientType::RUSTORCLI,
                starting_files: vec![String::from("torrent_c_data")],
                expected_files: vec![
                    String::from("torrent_a_data"),
                    String::from("torrent_b_data"),
                    String::from("torrent_c_data"),
                ],
                torrents: vec![
                    String::from("torrent_a_data.torrent"),
                    String::from("torrent_b_data.torrent"),
                    String::from("torrent_c_data.torrent"),
                ],
            },
        ],
    })
    }

    #[test]
    #[ignore]
    fn e2e_incoming_with_webtorrent() -> Result<(), Box<dyn std::error::Error>> {
        e2e_from_definition(E2ETestDefinition {
            description: String::from(
                "Incoming connection to rustorcli from webtorrent; only upload",
            ),
            clients: vec![
                Client {
                    client_type: ClientType::RUSTORCLI,
                    starting_files: vec![String::from("generated/torrent_a_data")],
                    expected_files: vec![String::from("torrent_a_data")],
                    torrents: vec![String::from("torrent_a_data.torrent")],
                },
                Client {
                    client_type: ClientType::WEBTORRENT,
                    starting_files: vec![],
                    expected_files: vec![String::from("torrent_a_data")],
                    torrents: vec![String::from("torrent_a_data.torrent")],
                },
            ],
        })
    }

    fn e2e_from_definition(
        definition: E2ETestDefinition,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("Running end-to-end test: {}", definition.description);

        restart_tracker();
        reset_destination_directories(&definition.clients);

        println!("Copying files");
        for client in definition.clients.iter() {
            for file in client.starting_files.iter() {
                println!("Copying {} to {}", format!("./tests/data/generated/{}", file), format!("{}/{}", client.directory(), file));
                fs::copy(
                    format!("./tests/data/generated/{}", file),
                    format!("{}/{}", client.directory(), file),
                )?;
            }
        }

        for client in definition.clients.iter() {
            client.client_type.stop_and_cleanup();
        }

        for (i, client) in definition.clients.iter().enumerate() {
            client.start();
            if i != definition.clients.len() - 1 {
                thread::sleep(Duration::from_secs(3));
            }
        }

        for attempt in 1..=ATTEMPTS {
            println!("Attempt {}/{}", attempt, ATTEMPTS);

            let mut all_exist = true;
            let mut missing = String::from("");
            'outer: for client in definition.clients.iter() {
                for file in client.expected_files.iter() {
                    let path = format!("{}/{}", client.directory(), file);

                    if !Path::new(&path).exists() {
                        all_exist = false;
                        missing = path;
                        break 'outer;
                    }
                }
            }

            if !all_exist {
                println!("Some expected files don't yet exist, e.g. {}", missing);
                if attempt < ATTEMPTS {
                    thread::sleep(BETWEEN_ATTEMPTS);
                }
                continue;
            }

            for client in definition.clients.iter() {
                for file in client.expected_files.iter() {
                    let origin_path = format!("./tests/data/generated/{}", file);
                    let destination_path = format!("{}/{}", client.directory(), file);

                    let expected_hash = get_hash(&origin_path);
                    let actual_hash = get_hash(&destination_path);

                    println!("Comparing {} and {}", origin_path, destination_path);

                    assert_eq!(expected_hash, actual_hash);
                }
            }

            println!("Success!");
            return Ok(());
        }

        panic!("Never reached the desired state");
    }

    fn restart_tracker() {
        println!("Killing tracker");
        Exec::shell("lsof -i tcp:8000 | awk 'NR!=1 {print $2}' | xargs kill")
            .join()
            .unwrap();

        println!("Starting torrent tracker");
        Command::new("bittorrent-tracker")
            .arg("--trust-proxy")
            .arg("--http")
            .arg("--port")
            .arg("8000")
            .stdout(Stdio::null())
            .spawn()
            .expect("bittorrent-tracker command failed to start");
    }

    fn reset_destination_directories(clients: &Vec<Client>) {
        println!("Deleting past temp dir");
        std::fs::remove_dir_all(TEMP_DIRECTORY).ok();
        println!("Creating temp dirs");
        for client in clients.iter() {
            fs::create_dir_all(client.directory()).unwrap();
            fs::create_dir_all(client.temp_directory()).unwrap();
        }
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

    fn stop_and_clean_transmission() -> Result<(), Box<dyn std::error::Error>> {
        println!("Deleting everything from transmission");
        Command::new("transmission-remote")
            .arg("-t")
            .arg("all")
            .arg("-r")
            .spawn()
            .ok();
        println!("Killing transmission");
        Exec::shell("killall transmission-daemon").join()?;
        return Ok(());
    }

    fn run_until_success(command: &mut Command) {
        let max_attempts = 10;
        for attempt in 1..=max_attempts {
            let output = command.output();
            if output.is_ok() && output.unwrap().status.success() {
                if attempt > 1 {
                    println!("The command succeeded after {} attempts", attempt);
                }
                return;
            }
            if attempt < max_attempts {
                thread::sleep(Duration::from_secs(1));
            }
        }
        panic!("Exceeded all attempts to run the command");
    }

    fn stop_and_clean_rustorcli() -> Result<(), Box<dyn std::error::Error>> {
        println!("Stopping rustorcli");
        Command::main_binary()?
            .arg("stop")
            .stdout(Stdio::inherit())
            .assert()
            .success();

        println!("Cleaning up rustorcli");
        let config_directory = util::config_directory();
        std::fs::remove_dir_all(config_directory).ok();

        return Ok(());
    }

    fn stop_and_clean_webtorrent() -> Result<(), Box<dyn std::error::Error>> {
        println!("Stopping webtorrent");
        Exec::shell("killall WebTorrent").join()?;
        return Ok(());
    }

    enum ClientType {
        RUSTORCLI,
        WEBTORRENT,
        TRANSMISSION,
    }

    impl std::fmt::Display for ClientType {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            match *self {
                ClientType::RUSTORCLI => write!(f, "rustorcli"),
                ClientType::TRANSMISSION => write!(f, "transmission"),
                ClientType::WEBTORRENT => write!(f, "webtorrent"),
            }
        }
    }

    impl ClientType {
        fn stop_and_cleanup(&self) {
            match *self {
                ClientType::RUSTORCLI => stop_and_clean_rustorcli().unwrap(),
                ClientType::TRANSMISSION => stop_and_clean_transmission().unwrap(),
                ClientType::WEBTORRENT => stop_and_clean_webtorrent().unwrap(),
            }
        }
    }

    struct Client {
        client_type: ClientType,
        starting_files: Vec<String>,
        expected_files: Vec<String>,
        torrents: Vec<String>,
    }

    impl Client {
        fn directory(&self) -> String {
            format!("{}/{}", TEMP_DIRECTORY, self.client_type)
        }

        // Hack to support WebTorrent - download to temp directory first, and move to "actual" directory when it's finished,
        // so that the test doesn't prematurily decide the download is completed
        // and doesn't fail when checking hashes.
        fn temp_directory(&self) -> String {
            match self.client_type {
                ClientType::WEBTORRENT => format!("{}_temp", self.directory()),
                _ => self.directory(),
            }
        }

        fn start(&self) {
            match self.client_type {
                ClientType::RUSTORCLI => {
                    println!("Adding torrents to rustorcli");

                    for file in self.torrents.iter() {
                        Command::main_binary()
                            .unwrap()
                            .arg("add")
                            .arg("-t")
                            .arg(get_absolute(format!("./tests/data/{}", file).as_str()))
                            .arg("-d")
                            .arg(get_absolute(self.directory().as_str()))
                            .assert()
                            .success();
                    }

                    println!("Starting rustorcli");
                    Command::main_binary()
                        .unwrap()
                        .arg("start")
                        .arg("--local")
                        .assert()
                        .success();
                }
                ClientType::WEBTORRENT => {
                    println!("Starting webtorrent");
                    for file in self.torrents.iter() {
                        let has_already = self
                            .starting_files
                            .iter()
                            .any(|f| f.clone() + &String::from(".torrent") == *file);

                        if has_already {
                            Command::new("webtorrent")
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .arg("seed")
                                .arg(format!("./tests/data/{}", file.as_str()))
                                .arg("-o")
                                .arg(self.directory())
                                .arg("--keep-seeding")
                                .spawn()
                                .expect("Failed to start webtorrent");
                        } else {
                            Command::new("webtorrent")
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .arg("download")
                                .arg(format!("./tests/data/{}", file.as_str()))
                                .arg("-o")
                                .arg(self.temp_directory())
                                .arg("--on-done")
                                .arg(format!("tests/on-webtorrent-done-{}.sh", file))
                                .arg("--keep-seeding")
                                .spawn()
                                .expect("Failed to start webtorrent");
                        };
                    }
                }
                ClientType::TRANSMISSION => {
                    println!("Starting transmission");
                    Command::new("transmission-daemon")
                        .arg("--download-dir")
                        .arg(self.directory())
                        .assert()
                        .success();

                    println!("Deleting everything from transmission");
                    run_until_success(
                        Command::new("transmission-remote")
                            .arg("-t")
                            .arg("all")
                            .arg("-r"),
                    );

                    println!("Adding torrents to trasmission");
                    for file in self.torrents.iter() {
                        run_until_success(
                            Command::new("transmission-remote")
                                .arg("-a")
                                .arg(format!("./tests/data/{}", file.as_str())),
                        );
                    }
                }
            }
        }
    }

    struct E2ETestDefinition {
        clients: Vec<Client>,
        description: String,
    }
}
