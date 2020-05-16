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

test :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test -- --test-threads=1

e2e-incoming :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_incoming_with_webtorrent -- --nocapture --ignored
			 cargo run stop

e2e-outgoing-with-transmission :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_outgoing_with_transmission -- --nocapture --ignored
			 cargo run stop

e2e-outgoing-three-way :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_outgoing_three_way -- --nocapture --ignored
			 cargo run stop

e2e :
	RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_tests -- --test-threads=1 --nocapture --ignored

cleanup:
			 cargo run stop
			 killall transmission-daemon || true
			 killall WebTorrent || true
			 killall rustorcli || true
			 lsof -i:8000 | awk 'NR!=1 {print $$2}' | xargs kill -9
			 rm -rf target/tmp/
			 rm -rf $(CONFIG_PATH)

test-all: test e2e cleanup

run-current:
			 mkdir -p target/tmp/current
			 cargo run stop
			 rm -rf $(CONFIG_PATH)
			 cargo run add -t "$$(pwd)/data/current.torrent" -d "$$(pwd)/target/tmp/current"
			 RUST_LOG=trace cargo run start

install-test-dependencies:
			 sudo npm install -g bittorrent-tracker
			 sudo npm install -g webtorrent-cli
			 sudo apt install transmission-cli

generate-test-data:
			 python tests/generate_test_data.py
