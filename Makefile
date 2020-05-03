test :
			 cargo test -- --test-threads=1

e2e-incoming :
			 cargo test e2e_incoming -- --nocapture --ignored
			 cargo run stop

e2e-outgoing :
			 cargo test e2e_outgoing -- --nocapture --ignored
			 cargo run stop

cleanup:
			 cargo run stop
			 killall transmission-daemon || true
			 killall WebTorrent || true
			 killall rustorcli || true
			 rm -rf target/tmp/
			 # TODO: other OS
			 rm -rf "$$HOME/Library/Application Support/rustorcli/"

run-current:
			 mkdir -p target/tmp/current
			 cargo run stop
			 rm -rf "$$HOME/Library/Application Support/rustorcli/"
			 cargo run add -t "$$(pwd)/data/current.torrent" -d "$$(pwd)/target/tmp/current"
			 cargo run start
