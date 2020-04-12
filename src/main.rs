use clap::{App, SubCommand};

fn main() -> () {
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
        },
        ("stop", _) => {
            println!("Stop subcommand!");
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
