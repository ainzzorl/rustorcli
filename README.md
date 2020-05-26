## rustorcli (RUSt TORrent CLIent)

A simple BitTorrent client in Rust.

It was developed as an excercise for learning Rust, and shouldn't be used by anyone for anything.

## Usage

```
USAGE:
    rustorcli [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    add       adds download
    help      Prints this message or the help of the given subcommand(s)
    list      lists downloads
    remove    removes download
    start     starts the app
    stop      stops the app
```

## Installation

`make && make install`

## Testing

* `make install-test-dependencies` - install a tracker and two other BitTorrent clients used for end-to-end testing.
* `make generate-test-data`
* `make test`
