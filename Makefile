.PHONY: help all build test lint fmt check ci bench clean install-dev

.DEFAULT_GOAL := help

help: ## Show this help message
	@echo "vicaya - विचय (macOS filesystem search tool)"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Available targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-12s %s\n", $$1, $$2}'

all: ci ## Run full CI pipeline (fmt + lint + test + build)

build: ## Build the workspace
	@echo "Building workspace..."
	cargo build --workspace

test: ## Run all tests
	@echo "Running tests..."
	cargo test --workspace --all-features

lint: ## Run clippy lints
	@echo "Running clippy..."
	cargo clippy --workspace --all-targets --all-features -- -D warnings

fmt: ## Format code with rustfmt
	@echo "Formatting code..."
	cargo fmt --all

check: fmt lint test ## Run fmt + lint + test

bench: ## Run benchmarks
	@echo "Running benchmarks..."
	cargo bench

clean: ## Clean build artifacts
	@echo "Cleaning target..."
	cargo clean

install-dev: ## Install vicaya CLI locally for development
	@echo "Installing vicaya CLI locally..."
	cargo install --path crates/vicaya-cli

ci: fmt lint test build ## Run CI pipeline (same as 'all')
	@echo "CI pipeline complete ✅"
