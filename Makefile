test :
			 cargo test -- --test-threads=1

e2e :
			 cargo test e2e -- --nocapture --ignored
			 cargo run stop

cleanup:
			 cargo run stop
