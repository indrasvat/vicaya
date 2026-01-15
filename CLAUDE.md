# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

vicaya (विचय) is a macOS-native filesystem search tool written in Rust. It provides instant search-as-you-type results using a trigram-based inverted index, with sub-20ms query latency over millions of files.

## Build & Development Commands

```bash
# Build
make build                    # Compile all crates
cargo build --workspace       # Same thing

# Test
make test                     # Run all tests with all features
cargo test --workspace --all-features
cargo test -p vicaya-index    # Single crate

# Lint & Format
make fmt                      # Format code
make lint                     # Clippy with -D warnings
make check                    # fmt-check + lint + test

# Development workflow
make dev                      # Build, start daemon, launch TUI (release mode)
make daemon-dev               # Start daemon without installing
make tui-dev                  # Launch TUI without installing

# Install
make install-dev              # Install CLI only
make install                  # Install CLI + daemon + TUI to ~/.cargo/bin
```

## Architecture

Rust workspace with 7 crates in `crates/`:

```
vicaya-core     → Config, logging, error types, IPC protocol
vicaya-index    → FileTable, StringArena, TrigramIndex, QueryEngine
vicaya-scanner  → Parallel filesystem walker (walkdir), builds IndexSnapshot
vicaya-watcher  → FSEvents wrapper (notify crate), emits IndexUpdate events
vicaya-daemon   → Background service: loads index, handles IPC, applies updates
vicaya-cli      → CLI binary (`vicaya`): search, rebuild, daemon control, metrics
vicaya-tui      → Terminal UI: streaming search results from daemon
```

**Data flow**: Scanner builds initial index → Daemon loads and serves queries via Unix socket IPC → Watcher sends live updates → Daemon applies to in-memory index + journals to disk.

**Key types**:
- `IndexSnapshot` (scanner): serializable bundle of FileTable + StringArena + TrigramIndex
- `IndexUpdate` (watcher): Create/Modify/Delete/Move events
- `DaemonState` (daemon): holds live index, handles queries and updates

## State Directory

Default: `~/Library/Application Support/vicaya/`
- `config.toml` - configuration
- `daemon.sock` / `daemon.pid` - IPC socket and process ID
- `index/index.bin` - serialized index snapshot
- `index/index.journal` - incremental updates (replayed on restart)

Override with `VICAYA_DIR` environment variable.

## Commit Convention

Use one-liner Conventional Commits: `type(scope): summary`
- Types: feat, fix, docs, style, refactor, test, chore, perf, build
- Scopes: core, index, scanner, watcher, daemon, cli, tui
- Keep commits atomic, brief, and relevant—one logical change per commit
- Commit early, commit often

## Testing

- Unit tests: `#[cfg(test)] mod tests` within modules
- Integration tests: `crates/<name>/tests/`
- Target >80% coverage for vicaya-core and vicaya-index
- Always run `make check` before pushing

## Pre-push Hook

lefthook runs `make ci` (fmt-check + lint + test + build) before push. Install with:
```bash
make hooks    # or: lefthook install
```

## macOS Permissions

FSEvents-based watching requires Full Disk Access or appropriate permissions. Verify Spotlight/full-disk access before investigating watcher issues.
