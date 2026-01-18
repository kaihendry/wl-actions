.PHONY: all build release install clean test check

BIN = target/release/wl-actions
PREFIX ?= $(HOME)/.local

all: release

build:
	cargo build

release: $(BIN)

$(BIN): src/*.rs Cargo.toml
	cargo build --release

install: $(BIN)
	install -Dm755 $(BIN) $(PREFIX)/bin/wl-actions

clean:
	cargo clean

test:
	cargo test

check:
	cargo check
	cargo clippy -- -D warnings
	cargo fmt --check
