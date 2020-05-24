UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
	CONFIG_PATH := "$$HOME/Library/Application Support/rustorcli/"
else
	CONFIG_PATH := "$$HOME/.rustorcli/"
endif

build-release :
			 cargo build --release

install : build-release
			 sudo cp target/release/rustorcli /usr/local/bin/

e2e-incoming :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_incoming_with_webtorrent -- --nocapture
			 cargo run stop

e2e-outgoing-with-transmission :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_outgoing_with_transmission -- --nocapture
			 cargo run stop

e2e-outgoing-three-way :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_outgoing_three_way -- --nocapture
			 cargo run stop

e2e-outgoing-with-transmission-directories :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_outgoing_with_transmission_directories -- --nocapture

test-e2e :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_tests -- --test-threads=1 --nocapture

test-cli :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test cli_tests -- --test-threads=1 --nocapture

test:
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test -- --test-threads=1 --nocapture

cleanup:
			 cargo run stop
			 killall transmission-daemon || true
			 killall WebTorrent || true
			 killall rustorcli || true
			 lsof -i:8000 | awk 'NR!=1 {print $$2}' | xargs kill -9
			 rm -rf target/tmp/
			 rm -rf $(CONFIG_PATH)

run-current:
			 mkdir -p target/tmp/current
			 cargo run stop
			 rm -rf $(CONFIG_PATH)
			 cargo run add -t "$$(pwd)/data/current.torrent" -d "$$(pwd)/target/tmp/current"
			 RUST_LOG=rustorcli=trace cargo run start

install-test-dependencies:
			 sudo npm install -g bittorrent-tracker
			 sudo npm install -g webtorrent-cli
			 sudo apt install transmission-cli
			 sudo apt install transmission-daemon

generate-test-data:
			 python tests/generate_test_data.py
