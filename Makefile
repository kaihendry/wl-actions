.PHONY: all build release install clean test check

all: release

build:
	cargo build

release:
	cargo build --release

install:
	cargo install --path .

clean:
	cargo clean

test:
	cargo test

check:
	cargo check
	cargo clippy -- -D warnings
	cargo fmt --check
