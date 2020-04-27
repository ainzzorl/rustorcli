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
