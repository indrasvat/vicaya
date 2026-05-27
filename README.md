# vicaya (विचय)

[![Release](https://img.shields.io/github/v/release/indrasvat/vicaya?sort=semver)](https://github.com/indrasvat/vicaya/releases) [![codecov](https://codecov.io/gh/indrasvat/vicaya/branch/main/graph/badge.svg)](https://codecov.io/gh/indrasvat/vicaya)

**विचय** — blazing-fast terminal-native file finding for macOS in Rust.

Project site and release manifest: <https://indrasvat.github.io/vicaya/>

vicaya is a lightweight developer file finder inspired by "Everything" on Windows. It finds files and folders by filename, path, and metadata; it does not index file contents. Use vicaya to locate the right path, then use tools like `ripgrep` when you need content search.

## Features

- **Instant Search**: Sub-20ms search latency over millions of files
- **Trigram Index**: Fast substring matching using trigram-based inverted index
- **Live Updates**: FSEvents-based file watcher keeps index up-to-date
- **Repo-Aware Indexing**: Honors `.gitignore`, `.ignore`, and `.git/info/exclude` by default
- **Developer Ranking**: Prefers exact/prefix/abbreviation matches and demotes noisy dependency/cache paths
- **Smriti Frecency**: Learns local usage from opens/copies/reveals/prints to promote the paths you repeatedly choose
- **Composable Output**: Table, plain, and JSON formats for shells, `fzf`, scripts, and agents
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

- Rust 1.91+ (stable toolchain; see `rust-toolchain.toml`)
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

### Install Latest Release

```bash
tmpdir=$(mktemp -d)
cd "$tmpdir"

curl -fsSLO https://github.com/indrasvat/vicaya/releases/latest/download/vicaya-universal.tar.gz
curl -fsSLO https://github.com/indrasvat/vicaya/releases/latest/download/vicaya-universal.tar.gz.sha256
shasum -a 256 -c vicaya-universal.tar.gz.sha256

tar -xzf vicaya-universal.tar.gz
mkdir -p "$HOME/.cargo/bin"
install -m 0755 bin/vicaya bin/vicaya-daemon bin/vicaya-tui "$HOME/.cargo/bin/"

vicaya --version
vicaya-tui --version
```

Future upgrades can be done in place:

```bash
vicaya upgrade --check
vicaya upgrade
```

`vicaya update` is an alias. `vicaya --version` prints immediately and, when a
fresh cached check knows about a newer release, adds an inline update notice
telling you to run `vicaya upgrade` or `vicaya update`. The check reads the
small static manifest at <https://indrasvat.github.io/vicaya/version.json>, so
normal users avoid GitHub REST API rate limits. Set `VICAYA_NO_UPDATE_CHECK=1`
to disable the background release check.

### Run the Stack

```bash
# Fast dev loop: build, start daemon, and launch the TUI
make dev              # uses release binaries for realistic perf

# Build release binaries locally (no global install)
make build-release    # binaries at ./target/release/
make tui-local        # run local TUI binary directly

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
vicaya search "query.rs" --scope ~/code/github.com/example-repo --limit 10

# Check daemon/index status
vicaya status

# Inspect runtime memory/health (includes `vmmap -summary` on macOS)
vicaya metrics
vicaya metrics --format json
vicaya metrics --no-vmmap

# Live metrics stream (JSONL); `vmmap` is throttled by default
vicaya metrics watch --format jsonl --interval 1s --vmmap-every 30

# End-to-end IPC latency benchmark (percentiles + optional vmmap before/after)
vicaya metrics bench --queries /tmp/vicaya-bench-queries.txt --warmup 50 --runs 500 --limit 20 --vmmap-before-after

# Output formats
vicaya search "config" --format json
vicaya search "test" --format plain

# Inspect or reset local Smriti usage memory
vicaya smriti list --limit 20
vicaya smriti list config --scope ~/code/github.com/example-repo --format json
vicaya smriti forget ~/code/github.com/example-repo/Cargo.toml
vicaya smriti clear --yes

# Manage the daemon manually
vicaya daemon start
vicaya daemon status
vicaya daemon stop

# Upgrade installed release binaries
vicaya upgrade --check
vicaya upgrade
```

Note: `vicaya search` auto-starts the daemon if needed. If an existing on-disk index is present,
the daemon performs a background reconciliation on startup to catch missed filesystem changes.
Run `vicaya status` to see whether reconciliation is in progress.

### TUI Usage

The TUI connects to the same daemon and gives you instant, fuzzy-as-you-type results.

Highlights:

- Split view: `phala` (results) + `purvadarshana` (preview with syntax highlighting)
- `Ctrl+T` opens the searchable `drishti` switcher (Patra = Files, Sthana = Directories, Smriti = usage memory)
- `Enter` on a directory pushes `ksetra` scope; `h` pops scope (breadcrumbs in header)
- Launch with `vicaya-tui .` or `vicaya-tui /some/dir` to start with `ksetra` already applied
- `Niyama` filters in `prashna`: `type:file|dir`, `ext:rs,md`, `path:src/`, `mtime:<7d`, `size:>10mb`
- `Ctrl+K` opens direct `ksetra` path input; `Ctrl+P` opens `kriya-suchi` (action palette)
- `Ctrl+O` toggles `purvadarshana`; `Tab` / `Shift+Tab` cycles focus (input/results/preview)
- Preview: scroll with `j/k`, arrows, `PgUp/PgDn`, `Ctrl+U/Ctrl+D`, `g/G`; search with `/`, jump `n/N`, toggle line numbers `Ctrl+N`, clear `Ctrl+L`
- Actions (in `phala`): `Enter/o` open in `$EDITOR` (or enter scope on dirs), `y` copy path, `p` print path and exit, `r` reveal in file manager
- Smriti records accepted open/copy/reveal/print/scope actions locally and uses a bounded frecency boost for future matching searches
- Press `?` for in-app help (when not focused on `prashna`)

Terminology note: the UI uses romanized Sanskrit labels (e.g. `drishti`, `ksetra`, `prashna`, `phala`, `purvadarshana`). See `docs/vicaya-tui-plan.md` for the glossary and longer-term roadmap.

```bash
# Dev mode (builds + starts daemon if needed)
make dev

# Assuming binaries are installed already
make daemon-start
vicaya-tui .
vicaya-tui

# Stop the daemon when done
make daemon-stop
```

### State Directory

By default, vicaya stores state under `~/Library/Application Support/vicaya`:

- `config.toml` (configuration)
- `daemon.sock` / `daemon.pid` (daemon IPC + lifecycle)
- `index/index.bin` / `index/index.journal` (snapshot + incremental updates)
- `smriti.json` (local usage memory for frecency ranking)

Use `VICAYA_DIR=/path/to/dir` to override the base directory (useful for tests and multi-instance setups).

`respect_ignore_files = true` is the default. It honors `.gitignore`, `.ignore`,
and `.git/info/exclude` during indexing; toggle it in `config.toml` only when you
want ignored build artifacts or generated files to appear in results. Because
this changes index membership, restart the daemon and run `vicaya rebuild` after
changing it.

`[smriti] enabled = true` is the default. Smriti never indexes file contents or
sends usage data anywhere; it only stores local path/action counters in
`smriti.json`. Set `VICAYA_NO_SMRITI=1` or disable `[smriti] enabled` when you
want searches to ignore usage memory entirely.

## Make Targets Reference

`make help` prints the full list, but the most common targets are below:

| Target | Description |
| --- | --- |
| `make build` | Compile the entire workspace (debug). |
| `make build-release` | Build release binaries to `./target/release/` without installing. |
| `make fmt` / `make lint` / `make test` | Run rustfmt, clippy (all targets/features), or the full test suite. |
| `make check` | Convenience combo: fmt → lint → test. |
| `make bench` | Execute the Criterion benchmarks. |
| `make install` | Install CLI, daemon, and TUI binaries into `~/.cargo/bin`. |
| `make dev` | Build, spawn the daemon in the background, and launch the TUI (dev workflow). |
| `make tui-local` | Build release and run local TUI binary (no global install). |
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

Releases are managed by Release Please:

1. After CI succeeds on `main`, the **Release** workflow opens or updates a release PR when conventional commits require a new version. That PR updates `Cargo.toml`, `.release-please-manifest.json`, and `CHANGELOG.md`.
2. Merge the release PR only after its CI and Codecov upload are green.
3. After the release PR lands on `main`, CI runs again. When that CI run succeeds, the **Release** workflow creates the `v<semver>` tag, builds the universal macOS tarball, uploads SHA256 sums, and publishes the GitHub Release.
4. Download and smoke-test the latest release with the commands in [Install Latest Release](#install-latest-release).

Each artifact bundle contains the CLI (`vicaya`), daemon (`vicaya-daemon`), and TUI (`vicaya-tui`) binaries so you can exercise the full stack.

The self-updater consumes the same release artifacts. After the release assets
are uploaded, the Release workflow publishes the GitHub Pages site at
<https://indrasvat.github.io/vicaya/> and writes
<https://indrasvat.github.io/vicaya/version.json>. `vicaya upgrade` reads that
static manifest first, downloads `vicaya-universal.tar.gz` and its SHA256 file,
verifies the checksum, stops the daemon if it is running, replaces `vicaya`,
`vicaya-daemon`, and `vicaya-tui` atomically in the current install directory,
then restarts the daemon unless `--no-restart-daemon` is passed. Use
`--install-dir <dir>` for non-standard installs and `--force` to reinstall the
latest version. If the Pages manifest is unavailable, the CLI can still fall
back to the GitHub Releases API.

For fully automated release PRs, configure a `RELEASE_PLEASE_TOKEN` repository secret backed by a fine-grained PAT or GitHub App token with contents and pull-request write access. The workflow can fall back to `GITHUB_TOKEN`, but GitHub suppresses CI triggers for pull requests created by `GITHUB_TOKEN`, so the dedicated token is the production-safe path.

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

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

## Etymology

**विचय** (vicaya) is a Sanskrit word meaning "inquiry" or "search" — perfectly capturing the essence of this tool
