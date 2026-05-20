.DEFAULT = run

.PHONY: all
all: run

.PHONY: run
run:
	cargo run

.PHONY: check
check:
	cargo check

.PHONY: format
format:
	cargo fmt

.PHONY: test
test:
	cargo test

.PHONY: lint
lint:
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: build
build:
	cargo build --release

.PHONY: clean
clean:
	cargo clean
