# vicaya (विचय)

[![Release](https://img.shields.io/github/v/release/indrasvat/vicaya?sort=semver)](https://github.com/indrasvat/vicaya/releases) [![codecov](https://codecov.io/gh/indrasvat/vicaya/branch/main/graph/badge.svg)](https://codecov.io/gh/indrasvat/vicaya)

**विचय** — blazing-fast filesystem search for macOS in Rust.

vicaya is a macOS-native filesystem search tool inspired by "Everything" on Windows. It provides instant, interactive search-as-you-type results for finding files and folders by name.

## Features

- **Instant Search**: Sub-20ms search latency over millions of files
- **Trigram Index**: Fast substring matching using trigram-based inverted index
- **Live Updates**: FSEvents-based file watcher keeps index up-to-date
- **Compact Index**: Efficient string arena + trigram index persisted to disk
- **CLI, Daemon & TUI**: Command-line tools, always-on daemon, and terminal UI for instant results

## Status

**Status**: Core functionality complete; ongoing UX/perf improvements

Latest coverage report: [Codecov dashboard](https://codecov.io/gh/indrasvat/vicaya).

### Completed
- ✅ Rust workspace structure
- ✅ Core types and configuration
- ✅ File table and string arena
- ✅ Trigram index and query engine
- ✅ Parallel filesystem scanner
- ✅ Unix socket IPC daemon (single-instance)
- ✅ Live updates (FSEvents/notify) + index journal
- ✅ Startup + daily reconciliation (self-healing)
- ✅ CLI + TUI interfaces

### In Progress
- 🚧 Documentation + UX polish
- 🚧 Performance work (persistence/mmap, faster rebuilds)

### Planned
- ⏳ macOS UI with global hotkey
- ⏳ Signed/notarized builds

## Quick Start

### Prerequisites

- Rust 1.70+ (stable toolchain)
- macOS 10.15+ (Catalina or later)

### Build & Test

```bash
# Clone the repository
git clone https://github.com/indrasvat/vicaya.git
cd vicaya

# Compile everything
make build            # or `cargo build --workspace`

# Format, lint, and test
make check            # runs fmt + clippy + tests

# Individual steps are also available: make fmt | make lint | make test
```

### Run the Stack

```bash
# Fast dev loop: build, start daemon, and launch the TUI
make dev              # uses release binaries for realistic perf

# Install the CLI only (for scripting)
make install-dev      # cargo install --path crates/vicaya-cli

# Full install (CLI + daemon + TUI in ~/.cargo/bin)
make install

# Once installed, the one-shot demo target will start the daemon
# and open the TUI with the published binaries
make run
```

### CLI Usage

```bash
# Build an index (first run)
vicaya rebuild

# Search for files
vicaya search "main.rs" --limit 10

# Check daemon/index status
vicaya status

# Output formats
vicaya search "config" --format json
vicaya search "test" --format plain

# Manage the daemon manually
vicaya daemon start
vicaya daemon status
vicaya daemon stop
```

Note: `vicaya search` auto-starts the daemon if needed. If an existing on-disk index is present,
the daemon performs a background reconciliation on startup to catch missed filesystem changes.
Run `vicaya status` to see whether reconciliation is in progress.

### TUI Usage

The TUI connects to the same daemon and gives you instant, fuzzy-as-you-type results.

Highlights:

- Split view: `phala` (results) + `purvadarshana` (preview with syntax highlighting)
- `Ctrl+T` opens the `drishti` switcher (Patra = Files, Sthana = Directories)
- `Ctrl+O` toggles `purvadarshana`; `Tab` / `Shift+Tab` cycles focus (input/results/preview)
- Preview scrolling: `j/k`, arrows, `PgUp/PgDn`, `Ctrl+U/Ctrl+D`, `g/G`
- Actions (in `phala`): `Enter/o` open in `$EDITOR`, `y` copy path, `p` print path and exit, `r` reveal in file manager
- Press `?` for in-app help (when not focused on `prashna`)

Terminology note: the UI uses romanized Sanskrit labels (e.g. `drishti`, `ksetra`, `prashna`, `phala`, `purvadarshana`). See `docs/vicaya-tui-plan.md` for the glossary and longer-term roadmap.

```bash
# Dev mode (builds + starts daemon if needed)
make dev

# Assuming binaries are installed already
make daemon-start
vicaya-tui

# Stop the daemon when done
make daemon-stop
```

### State Directory

By default, vicaya stores state under `~/Library/Application Support/vicaya`:

- `config.toml` (configuration)
- `daemon.sock` / `daemon.pid` (daemon IPC + lifecycle)
- `index/index.bin` / `index/index.journal` (snapshot + incremental updates)

Use `VICAYA_DIR=/path/to/dir` to override the base directory (useful for tests and multi-instance setups).

## Make Targets Reference

`make help` prints the full list, but the most common targets are below:

| Target | Description |
| --- | --- |
| `make build` | Compile the entire workspace (all crates). |
| `make fmt` / `make lint` / `make test` | Run rustfmt, clippy (all targets/features), or the full test suite. |
| `make check` | Convenience combo: fmt → lint → test. |
| `make bench` | Execute the Criterion benchmarks. |
| `make install-dev` | `cargo install` the CLI locally for quick scripting. |
| `make install` | Install CLI, daemon, and TUI binaries into `~/.cargo/bin`. |
| `make dev` | Build, spawn the daemon in the background, and launch the TUI (dev workflow). |
| `make run` | Install release binaries, start the daemon, and open the TUI (simulates end-user setup). |
| `make daemon-start` / `make daemon-stop` | Manage the daemon using the installed CLI. |
| `make daemon-dev` / `make tui-dev` | Run daemon or TUI straight from source without installing. |
| `make clean` | Remove `target/` artifacts. |
| `make ci` | Local CI parity: fmt + lint + test + build. |

## Architecture

vicaya is organized as a Rust workspace with multiple crates:

- **vicaya-core**: Configuration, logging, and error types
- **vicaya-index**: File table, string arena, and trigram index
- **vicaya-scanner**: Parallel filesystem scanner
- **vicaya-watcher**: FSEvents-based file watcher
- **vicaya-daemon**: Background service
- **vicaya-cli**: Command-line interface
- **vicaya-tui**: Terminal UI that streams live results from the daemon

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

Each artifact bundle contains the CLI (`vicaya`), daemon (`vicaya-daemon`), and TUI (`vicaya-tui`) binaries so you can exercise the full stack.

Always run `cargo release <level> --workspace --no-publish --execute --dry-run` locally before firing the workflow.

### Running Unsigned macOS Artifacts

CI artifacts are currently unsigned, so macOS adds a quarantine flag when you download them via a browser. If Finder refuses to launch them (or moves them to Trash), strip the flag manually before running:

```bash
xattr -dr com.apple.quarantine /Users/you/Downloads/vicaya-aarch64-apple-darwin-binaries
```

**Note:** Replace `/Users/you/Downloads/vicaya-aarch64-apple-darwin-binaries` with the actual path where you downloaded the artifact.

After clearing the attribute you can run `./vicaya --version`, `./vicaya-daemon --version`, etc. This cannot be automated in CI because Gatekeeper attaches the flag on the downloader’s machine; the long-term fix is to codesign/notarize artifacts once a Developer ID certificate is available.

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Etymology

**विचय** (vicaya) is a Sanskrit word meaning "inquiry" or "search" — perfectly capturing the essence of this tool
