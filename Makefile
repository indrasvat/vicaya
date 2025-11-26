.PHONY: all build test lint fmt check ci bench clean install-dev

all: ci

build:
	@echo "Building workspace..."
	cargo build --workspace

test:
	@echo "Running tests..."
	cargo test --workspace --all-features

lint:
	@echo "Running clippy..."
	cargo clippy --workspace --all-targets --all-features -- -D warnings

fmt:
	@echo "Formatting code..."
	cargo fmt --all

check: fmt lint test

bench:
	@echo "Running benchmarks..."
	cargo bench

clean:
	@echo "Cleaning target..."
	cargo clean

install-dev:
	@echo "Installing vicaya CLI locally..."
	cargo install --path crates/vicaya-cli

ci: fmt lint test build
	@echo "CI pipeline complete ✅"
