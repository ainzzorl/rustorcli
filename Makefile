test :
			 cargo test -- --test-threads=1

e2e :
			 cargo test e2e -- --nocapture --ignored
			 cargo run stop
			 rm -rf target/tmp/
			 # TODO: other OS
			 rm -rf "$HOME/Application\ Support/rustorcli/"

cleanup:
			 cargo run stop

run-current:
			 mkdir -p target/tmp/current
			 cargo run stop
			 rm -rf "$HOME/Application\ Support/rustorcli/"
			 cargo run add -t ~/data/current.torrent -d ~/target/tmp/current
			 cargo run start
