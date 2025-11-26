.PHONY: help all build test lint fmt check ci bench clean install-dev

.DEFAULT_GOAL := help

help: ## Show this help message
	@echo "\033[1;36mvicaya - विचय\033[0m \033[2m(macOS filesystem search tool)\033[0m"
	@echo ""
	@echo "\033[1mUsage:\033[0m make \033[33m[target]\033[0m"
	@echo ""
	@echo "\033[1mAvailable targets:\033[0m"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[32m%-12s\033[0m %s\n", $$1, $$2}'

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
