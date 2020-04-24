test :
			 cargo test -- --test-threads=1

e2e :
			 cargo test e2e -- --nocapture --ignored

cleanup:
			 cargo run stop
