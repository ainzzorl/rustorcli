UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
	CONFIG_PATH := "$$HOME/Library/Application Support/rustorcli/"
else
	CONFIG_PATH := "$$HOME/.rustorcli/"
endif

.PHONY: build-release
build-release :
			 cargo build --release

.PHONY: install
install :
			 cp target/release/rustorcli /usr/local/bin/

.PHONY: e2e-incoming
e2e-incoming :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_incoming_with_webtorrent -- --nocapture
			 cargo run stop

.PHONY: e2e-outgoing-with-transmission
e2e-outgoing-with-transmission :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_outgoing_with_transmission -- --nocapture
			 cargo run stop

.PHONY: e2e-outgoing-three-way
e2e-outgoing-three-way :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_outgoing_three_way -- --nocapture
			 cargo run stop

.PHONY: e2e-outgoing-with-transmission-directories
e2e-outgoing-with-transmission-directories :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_outgoing_with_transmission_directories -- --nocapture

.PHONY: test-e2e
test-e2e :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test e2e_tests -- --test-threads=1 --nocapture

.PHONY: test-cli
test-cli :
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test cli_tests -- --test-threads=1 --nocapture

.PHONY: test
test:
			 RUST_BACKTRACE=1 RUST_LOG=rustorcli=trace cargo test -- --test-threads=1 --nocapture

.PHONY: cleanup
cleanup:
			 cargo run stop
			 killall transmission-daemon || true
			 pgrep WebTorrent | xargs kill -9 || true
			 killall rustorcli || true
			 lsof -i:8000 | awk 'NR!=1 {print $$2}' | xargs kill -9
			 rm -rf target/tmp/
			 rm -rf $(CONFIG_PATH)

.PHONY: run-current
run-current:
			 mkdir -p target/tmp/current
			 cargo run stop
			 rm -rf $(CONFIG_PATH)
			 cargo run add -t "$$(pwd)/data/current.torrent" -d "$$(pwd)/target/tmp/current"
			 RUST_LOG=rustorcli=trace cargo run start

.PHONY: install-test-dependencie
install-test-dependencies:
			 sudo npm install -g bittorrent-tracker
			 sudo npm install -g webtorrent-cli
			 sudo apt install transmission-cli
			 sudo apt install transmission-daemon

.PHONY: generate-test-data
generate-test-data:
			 python tests/generate_test_data.py
