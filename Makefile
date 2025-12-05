.PHONY: help all build test lint fmt fmt-check check ci bench clean install-dev install daemon-start daemon-stop daemon-dev tui tui-dev run dev

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

fmt: ## Format code with rustfmt (writes changes)
	@echo "Formatting code..."
	cargo fmt --all

fmt-check: ## Check formatting without writing changes
	@echo "Checking formatting..."
	cargo fmt --all -- --check

check: fmt-check lint test ## Run fmt-check + lint + test

bench: ## Run benchmarks
	@echo "Running benchmarks..."
	cargo bench

clean: ## Clean build artifacts
	@echo "Cleaning target..."
	cargo clean

install-dev: ## Install vicaya CLI locally for development
	@echo "Installing vicaya CLI locally..."
	cargo install --path crates/vicaya-cli

install: ## Install CLI, daemon, and TUI
	@echo "Building and installing vicaya..."
	@cargo build --release --workspace
	@echo "Installing vicaya CLI..."
	@cargo install --path crates/vicaya-cli --force
	@echo "Installing vicaya daemon..."
	@cargo install --path crates/vicaya-daemon --force
	@echo "Installing vicaya TUI..."
	@cargo install --path crates/vicaya-tui --force
	@echo "✅ Installation complete!"
	@echo ""
	@echo "Binaries installed:"
	@which vicaya || echo "  ⚠️  vicaya not in PATH"
	@which vicaya-daemon || echo "  ⚠️  vicaya-daemon not in PATH"
	@which vicaya-tui || echo "  ⚠️  vicaya-tui not in PATH"

daemon-start: ## Start the daemon (builds if needed)
	@echo "Starting vicaya daemon..."
	@if pgrep -f vicaya-daemon > /dev/null; then \
		echo "⚠️  Daemon already running (PID: $$(pgrep -f vicaya-daemon))"; \
	else \
		cargo run --package vicaya-cli --release -- daemon start && \
		echo "✅ Daemon started"; \
	fi

daemon-stop: ## Stop the daemon
	@echo "Stopping vicaya daemon..."
	@if pgrep -f vicaya-daemon > /dev/null; then \
		cargo run --package vicaya-cli --release -- daemon stop && \
		echo "✅ Daemon stopped"; \
	else \
		echo "⚠️  Daemon not running"; \
	fi

daemon-dev: ## Start daemon directly with cargo run (no install needed)
	@echo "Starting vicaya daemon (dev mode)..."
	@if pgrep -f vicaya-daemon > /dev/null; then \
		echo "⚠️  Daemon already running (PID: $$(pgrep -f vicaya-daemon))"; \
	else \
		cargo run --package vicaya-daemon --release & \
		sleep 2 && \
		if pgrep -f vicaya-daemon > /dev/null; then \
			echo "✅ Daemon started in background (PID: $$(pgrep -f vicaya-daemon))"; \
		else \
			echo "❌ Failed to start daemon"; \
			exit 1; \
		fi \
	fi

tui: daemon-start ## Launch the TUI (starts daemon if needed)
	@echo "Launching vicaya TUI..."
	@cargo run --package vicaya-tui --release

tui-dev: daemon-dev ## Launch TUI in dev mode (no install needed)
	@echo "Launching vicaya TUI (dev mode)..."
	@cargo run --package vicaya-tui --release

run: install ## Install binaries, start daemon, and launch TUI
	@echo "Starting daemon..."
	@if pgrep -f vicaya-daemon > /dev/null; then \
		echo "⚠️  Daemon already running (PID: $$(pgrep -f vicaya-daemon))"; \
	else \
		vicaya daemon start && echo "✅ Daemon started" && sleep 2; \
	fi
	@echo "Waiting for daemon to be ready..."
	@for i in 1 2 3 4 5; do \
		if vicaya daemon status >/dev/null 2>&1; then \
			echo "✅ Daemon is ready!"; \
			break; \
		fi; \
		echo "  Waiting... ($$i/5)"; \
		sleep 1; \
	done
	@echo "Launching TUI..."
	@vicaya-tui

dev: build daemon-dev ## Build, start daemon, and launch TUI (no install needed)
	@echo "Waiting for daemon to be ready..."
	@for i in 1 2 3 4 5; do \
		if pgrep -f vicaya-daemon >/dev/null 2>&1; then \
			echo "✅ Daemon is ready!"; \
			break; \
		fi; \
		echo "  Waiting... ($$i/5)"; \
		sleep 1; \
	done
	@echo "Launching vicaya TUI (dev mode)..."
	@cargo run --package vicaya-tui --release

ci: fmt-check lint test build ## Run CI pipeline (same as 'all')
	@echo "CI pipeline complete ✅"
