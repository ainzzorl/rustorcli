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
			 rm -rf target/tmp/
			 # TODO: other OS
			 rm -rf "$$HOME/Library/Application Support/rustorcli/"

test-all: test e2e cleanup

run-current:
			 mkdir -p target/tmp/current
			 cargo run stop
			 rm -rf "$$HOME/Library/Application Support/rustorcli/"
			 cargo run add -t "$$(pwd)/data/current.torrent" -d "$$(pwd)/target/tmp/current"
			 RUST_LOG=trace cargo run start
