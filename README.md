# vicaya (विचय)

[![codecov](https://codecov.io/gh/indrasvat/vicaya/branch/main/graph/badge.svg)](https://codecov.io/gh/indrasvat/vicaya)

**विचय** — blazing-fast filesystem search for macOS in Rust.

vicaya is a macOS-native filesystem search tool inspired by "Everything" on Windows. It provides instant, interactive search-as-you-type results for finding files and folders by name.

## Features

- **Instant Search**: Sub-20ms search latency over millions of files
- **Trigram Index**: Fast substring matching using trigram-based inverted index
- **Live Updates**: FSEvents-based file watcher keeps index up-to-date
- **Low Memory**: Efficient memory-mapped file storage
- **CLI & Daemon**: Command-line interface with background daemon

## Status

**Current Version**: 0.2.0
**Status**: Under Active Development (Phase 1 Complete)

Latest coverage report: [Codecov dashboard](https://codecov.io/gh/indrasvat/vicaya).

### Completed
- ✅ Rust workspace structure
- ✅ Core types and configuration
- ✅ File table and string arena
- ✅ Trigram index and query engine
- ✅ Parallel filesystem scanner
- ✅ Basic CLI interface

### In Progress
- 🚧 Documentation
- 🚧 FSEvents integration
- 🚧 Daemon IPC server

### Planned
- ⏳ macOS UI with global hotkey
- ⏳ Performance optimization
- ⏳ Signed/notarized builds

## Quick Start

### Prerequisites

- Rust 1.70+ (stable toolchain)
- macOS 10.15+ (Catalina or later)

### Build

```bash
# Clone the repository
git clone https://github.com/indrasvat/vicaya.git
cd vicaya

# Build the workspace
make build

# Run tests
make test

# Install CLI locally
make install-dev
```

### Usage

```bash
# Build an index (first time)
vicaya rebuild

# Search for files
vicaya search "main.rs" --limit 10

# Check index status
vicaya status

# Output formats
vicaya search "config" --format json
vicaya search "test" --format plain
```

## Architecture

vicaya is organized as a Rust workspace with multiple crates:

- **vicaya-core**: Configuration, logging, and error types
- **vicaya-index**: File table, string arena, and trigram index
- **vicaya-scanner**: Parallel filesystem scanner
- **vicaya-watcher**: FSEvents-based file watcher
- **vicaya-daemon**: Background service
- **vicaya-cli**: Command-line interface

See [docs/vicaya.md](docs/vicaya.md) for the complete implementation guide.

## Performance Targets

- **Indexing**: 200k+ files/sec on SSD
- **Query Latency**: p95 ≤ 20ms for 5M+ entries
- **Memory**: ≤ 500MB for 5M indexed paths
- **Responsiveness**: UI updates within 16ms

## Development

```bash
# Format code
make fmt

# Run linter
make lint

# Run all checks
make check

# Run benchmarks
make bench
```

## Releases

1. Run the **Release Prepare** workflow (from GitHub Actions) or via CLI: `gh workflow run release-prepare.yml -f level=minor -f dry_run=true` for a rehearsal. Once satisfied, rerun with `dry_run=false`. This invokes [`cargo release`](https://github.com/crate-ci/cargo-release) using `release.toml` to bump versions, tag `v<semver>`, and push the metadata.
2. When the tag lands on `main`, the **Release** workflow builds universal macOS binaries, packages `.pkg` and `.tar.gz` installers, uploads SHA256 sums, and publishes a GitHub Release with auto-generated notes.
3. Download artifacts from the release page or from CI pull requests (see the `vicaya-universal` and `vicaya-linux-binaries` artifacts) for manual validation.

Always run `cargo release <level> --workspace --no-publish --execute --dry-run` locally before firing the workflow.

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Etymology

**विचय** (vicaya) is a Sanskrit word meaning "inquiry" or "search" — perfectly capturing the essence of this tool
