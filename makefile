test-info:
	RUST_LOG=debug cargo test --package cli-music-player --lib -- --nocapture

.PHONY: test-info