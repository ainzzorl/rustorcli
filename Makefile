test :
			 cargo test -- --test-threads=1

e2e :
			 cargo test e2e -- --nocapture --ignored
			 cargo run stop

cleanup:
			 cargo run stop
			 rm -rf target/tmp/
			 # TODO: other OS
			 rm -rf "$$HOME/Library/Application Support/rustorcli/"

run-current:
			 mkdir -p target/tmp/current
			 cargo run stop
			 rm -rf "$$HOME/Library/Application Support/rustorcli/"
			 cargo run add -t "$$(pwd)/data/current.torrent" -d "$$(pwd)/target/tmp/current"
			 cargo run start
