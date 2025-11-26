# vicaya (विचय) Implementation Guide

**Document Type:** Living Implementation Guide
**Last Updated:** 2025-11-26
**Agent:** Claude Code / GPT-5.1 Thinking / Gemini (or equivalent)
**Project Status:** Planning → Active

vicaya (विचय) is a macOS-native, Rust-based, *blazing-fast* filesystem search tool inspired by "Everything" on Windows. This guide is optimized for AI coding agents working incrementally.

---

## Quick Navigation

- [Executive Summary](#executive-summary)
- [Design Goals](#design-goals)
- [Architecture Overview](#architecture-overview)
- [UI/UX Specifications](#uiux-specifications)
- [Implementation Phases](#implementation-phases)
- [Testing Strategy](#testing-strategy)
- [Development Standards](#development-standards)
- [Project Structure](#project-structure)
- [Documentation Standards](#documentation-standards)
- [Performance Considerations](#performance-considerations)
- [Security Standards](#security-standards)
- [Release Process](#release-process)
- [Document Maintenance](#document-maintenance)
- [Appendix: Rust Crates & Decisions](#appendix-rust-crates--decisions)

---

## Executive Summary

### What We're Building

vicaya (विचय) is a macOS filesystem search engine that locates files and folders by name (and basic metadata) *instantly*, with interactive results as you type. It mirrors the "Everything for Windows" experience but is built for APFS/macOS using Rust, FSEvents, and a highly optimized in-memory + on-disk index.

### Key Deliverables

- [ ] **Core Indexing Engine**
  - Full-disk initial scan (configurable roots) using a fast, parallel walker.
  - Persistent file table (metadata store) backed by memory-mapped files.
  - Trigram-based inverted index for ultra-fast substring search.
- [ ] **Live Update Engine**
  - FSEvents-based watcher that incrementally updates the index.
  - Robust handling of batched/delayed events and volume changes.
- [ ] **Search Frontends**
  - [ ] CLI search tool (`vicaya`): instant results, filters, scripting-friendly.
  - [ ] macOS menu-bar / quick-switch UI with global hotkey and result list.
- [ ] **Config & Preferences**
  - Exclusion rules, per-volume settings, performance knobs.
- [ ] **Testing & Tooling**
  - Unit, integration, and performance tests for scanner, index, and watcher.
  - CI pipeline (GitHub Actions) and Makefile workflow.
- [ ] **Installable Build**
  - Signed/notarized macOS app bundle + CLI binary.
  - Optionally, Homebrew tap for CLI installation.

### Success Metrics

- **Indexing speed**
  - Fresh index of 1M files on SSD in ≤ 90 seconds on a modern Mac.
- **Query latency**
  - p95 search latency ≤ 20 ms for substring queries over 5M+ entries.
- **Memory footprint**
  - ≤ 500 MB resident memory for 5M indexed paths on typical workloads.
- **Responsiveness**
  - UI update on keystroke within a single frame (~16 ms) for top 200 results.
- **Reliability**
  - Index remains consistent across sleep/wake cycles, reboots, and volume attach/detach.

### Constraints & Non-Goals

- **Platform**: macOS only (APFS/HFS+). No Windows/Linux support in v1.
- **Scope**: *Filename / path search only* in v1.
  - No file content indexing (may be future plugin).
- **Permissions**: Respects macOS TCC (Full Disk Access). No bypasses.
- **Non-goals (v1)**:
  - Network search across multiple machines.
  - Cloud storage APIs (iCloud Drive beyond what the OS exposes as files).

---

## Design Goals

### Primary Goals

1. **Interactive, near-instant search-as-you-type**
   The user must feel " Everything-level instant " results: no noticeable lag even with millions of files.

2. **Robust, self-healing index**
   The index must stay correct using FSEvents and periodic reconciliation, even when events are dropped, volumes change, or the machine sleeps.

3. **Low-friction macOS experience**
   A small, unobtrusive background app with a global hotkey and menu-bar icon, minimal prompts for permissions, and safe defaults.

4. **Agent-friendly codebase**
   Clean, modular Rust workspace that AI agents can navigate easily: clear boundaries, good tests, and predictable tooling.

### Architecture Principles

- **Separation of Concerns**
  - `scanner` (initial crawl) is independent from `watcher` (FSEvents) and `index` (data structures).
- **SOLID & Rust idioms**
  - Traits for pluggable backends (e.g., mock FS for tests vs real FS).
  - Clear ownership and lifetimes for mmap'd data.
- **Open/Closed Principle**
  - Core index and file-table are closed to modification but open to new frontends (CLI, GUI) and filters (e.g., future content or tag plugins).
- **Fail-Fast & Observable**
  - Use structured logging (`tracing`) with clear error boundaries.
  - Panic only on unrecoverable corruption; otherwise degrade gracefully.

### Performance Targets

- **Response Time**
  - Query engine: ≤ 5 ms for in-memory candidate computation; ≤ 20 ms end-to-end.
- **Throughput**
  - Sustain 30+ queries/sec (user + scripts) without degradation.
- **Indexing**
  - 200k+ file entries/sec during initial scan on SSD when not limited by TCC.
- **Durability**
  - Index rebuild after crash should be idempotent and safe; worst case is full rescan.

---

## Architecture Overview

### Crate Layout (Rust Workspace)

- `vicaya-core`
  - Domain types, configuration, errors, logging setup.
- `vicaya-index`
  - File table, string arena, trigram index, query engine.
- `vicaya-scanner`
  - Parallel filesystem scanner for initial index build.
- `vicaya-watcher`
  - FSEvents integration, incremental updates, reconciliation.
- `vicaya-daemon`
  - Background service; coordinates scanner, watcher, index; IPC server.
- `vicaya-cli`
  - CLI frontend; talks to daemon via IPC (e.g., Unix domain sockets).
- `vicaya-ui-macos` (optional v1.5+ if heavy)
  - macOS UI (Tauri/egui/Cocoa wrapper) that talks to daemon.

### Data Flow

1. **Initial Setup**
   - CLI/daemon config → `vicaya-core::Config`.
   - `vicaya-scanner` walks configured roots → file metadata stream.
   - `vicaya-index` builds file table + trigram index; persists to mmap'd files.
2. **Steady-State Operation**
   - `vicaya-watcher` subscribes to FSEvents on roots.
   - On events, `vicaya-watcher` translates events into `IndexUpdate` operations.
   - `vicaya-daemon` applies `IndexUpdate` to `vicaya-index` and persists.
3. **Search Queries**
   - CLI/UI sends query to daemon via IPC.
   - `vicaya-index` runs trigram lookup + ranking → returns `Vec<SearchResult>`.
4. **Reconciliation**
   - Periodic "lightweight rescan" tasks from daemon (e.g., nightly) compare sampled filesystem snapshot vs index; queue fixes.

---

## UI/UX Specifications

vicaya must feel like a native macOS utility: instant, minimal, keyboard-first but mouse-friendly where applicable.

### ASCII Mock – Quick Search Popup

```text
┌────────────────────────────────────────────────────────────┐
│ 🔍 vicaya — Search files                                  │
│ query:  src/main.rs                                       │
├───────┬─────────────────────────────────────┬──────────────┤
│ Rank  │ Path                                │ Modified     │
├───────┼─────────────────────────────────────┼──────────────┤
│  1    │ ~/projects/vicaya/src/main.rs      │ 2025-11-20   │
│  2    │ ~/projects/app/src/main.rs         │ 2025-10-02   │
│  3    │ ~/archive/rust-demo/src/main.rs    │ 2024-07-11   │
├───────┴─────────────────────────────────────┴──────────────┤
│ ↑↓ navigate  ⏎ open  ⌘O reveal in Finder  ⌘C copy path    │
│ ⌘L filter…  ⌘, preferences  Esc close                     │
└────────────────────────────────────────────────────────────┘
```

### ASCII Mock – Preferences Window

```text
┌──────────────── vicaya Preferences ────────────────┐
│ [ General ]  [ Indexing ]  [ Shortcuts ]  [ About ]│
├────────────────────────────────────────────────────┤
│ Index Roots:                                       │
│   [x] Macintosh HD ( / )                           │
│   [x] Home ( ~/ )                                  │
│   [ ] External: BackupSSD ( /Volumes/BackupSSD )   │
│                                                    │
│ Exclusions:                                        │
│   - /System                                        │
│   - /Library                                       │
│   - ~/.git                                         │
│                                                    │
│ Performance:                                       │
│   Max memory: 512 MB   [ slider ----|---- ]        │
│   Reconcile nightly at: 03:00                      │
├────────────────────────────────────────────────────┤
│                 ( Cancel )   (   Save   )          │
└────────────────────────────────────────────────────┘
```

### ASCII Mock – CLI Interaction

```text
$ vicaya search "main.rs" --limit 5 --sort=rank
RANK  SCORE  MODIFIED            PATH
1     0.98   2025-11-20 10:21    /Users/robin/projects/vicaya/src/main.rs
2     0.91   2025-10-02 19:05    /Users/robin/projects/app/src/main.rs
3     0.76   2024-07-11 08:09    /Users/robin/archive/rust-demo/src/main.rs
```

### State Transitions (High-Level)

- `Idle` → user presses global hotkey → `PopupOpen`
- `PopupOpen` + query input → `Searching`
- `Searching` → results ready → `ResultsDisplayed`
- `ResultsDisplayed` + open action → `Launching` (open file / reveal)
- `PopupOpen` + Esc → `Idle`
- `Daemon` states: `ColdStart` → `Scanning` → `Ready` → (`Updating` as events flow)

---

## Implementation Phases

> Agents: use timestamps when you actually work. Below dates are initial targets.

### Phase 1: Foundation (Target: Day 1–2)

**Timestamp:** 2025-11-26 (Started)

#### Objectives

- [x] Set up Rust workspace & core crates.
- [ ] Integrate basic logging and configuration.
- [ ] Establish CI, linting, and formatting.

#### Specific Tasks

1. Initialize Git repository and Rust workspace:
   - `cargo new --vcs git vicaya`
   - Convert to workspace with `Cargo.toml` at root and member crates (`core`, `index`, etc.).
2. Create crates:
   - `vicaya-core`: config, error types, logging (`tracing`), feature flags.
   - `vicaya-index`: skeleton types for `FileId`, `FileMeta`, `Index` trait.
   - `vicaya-scanner`: scaffold with trait `FileSystemScanner` and dummy implementation.
   - `vicaya-daemon`: minimal `main` that initializes logging and loads config.
   - `vicaya-cli`: minimal CLI using `clap` that prints version and pings daemon (stub).
3. Configure logging:
   - `tracing-subscriber` with env filter (`RUST_LOG=vicaya=info`).
4. Add base config file:
   - `config/default.toml` with index path, default roots, exclusions.
5. Tooling:
   - Add `rust-toolchain.toml` (track stable).
   - Add `Makefile` wrapping `cargo fmt`, `cargo clippy`, `cargo test`.

#### Validation Checkpoint

- [ ] `cargo fmt`, `cargo clippy`, `cargo test` all pass.
- [ ] `vicaya-daemon` and `vicaya` (CLI) build and print basic info.
- [ ] GitHub Actions CI pipeline green for main branch.

---

### Phase 2: Core Features – Index & Scanner (Target: Day 3–5)

**Timestamp:** 2025-11-25

#### Objectives

- [ ] Implement efficient file-table + string arena.
- [ ] Implement trigram index and basic query engine.
- [ ] Implement parallel scanner to populate index from filesystem.

#### Specific Tasks

1. **File Table & String Arena (`vicaya-index`)**
   - Implement `StringArena`:
     - Backed by `memmap2` or plain `Vec<u8>` with serialization.
     - Store all path/basename strings as UTF-8 with offsets.
   - Implement `FileMeta` and `FileId`:
     - `FileId(u32 or u64)`.
     - Fields: `path_offset`, `name_offset`, `size`, `mtime`, `dev`, `ino`.
   - Implement `FileTable`:
     - `Vec<FileMeta>` + `StringArena`.
     - Methods: `insert`, `remove`, `get(FileId)`, `iter()`.

2. **Trigram Index**
   - Implement `Trigram` type (`u32` from 3 bytes).
   - Build `trigram -> Vec<FileId>` map:
     - Use `fxhash` or `hashbrown` for fast hashing.
     - Posting lists kept sorted; consider `roaring` for future optimization.
   - Index builder API:
     - `IndexBuilder::add(file_id, basename_str)`.
     - `IndexBuilder::finalize() -> Index` (compact representation).

3. **Query Engine**
   - Normalize input (lowercase, Unicode NFC, ASCII fallbacks).
   - For queries `< 3 chars`: linear scan over basenames (with limit).
   - For queries `>= 3` chars:
     - Extract trigrams.
     - Intersect posting lists for those trigrams.
     - Validate substring match against basename and/or full path.
     - Rank by match location (prefix > infix), frequency, recency.

4. **Scanner (`vicaya-scanner`)**
   - Use `ignore`/`walkdir` + `rayon` for parallel walking.
   - Read config for roots and exclusions.
   - For each file:
     - Build `FileMeta`, add to `FileTable`, update `IndexBuilder`.
   - Expose `scan_and_build_index(config) -> IndexSnapshot`.

5. **Persistence**
   - Define `IndexSnapshot`:
     - Contains serialized file table + index, storable under `~Library/Application Support/vicaya/`.
   - Implement serialization with `bincode` or `rkyv`.
   - Add versioning header to allow migration later.

#### Validation Checkpoint

- [ ] Unit tests for trigram index and query engine.
- [ ] Basic benchmark: in-memory index of 100k synthetic files + substring queries under 5 ms.
- [ ] CLI command `vicaya rebuild` performs a scan and writes index snapshot.

---

### Phase 3: Live Updates – FSEvents Watcher (Target: Day 6–7)

**Timestamp:** 2025-11-26

#### Objectives

- [ ] Integrate FSEvents-based watcher for configured roots.
- [ ] Translate events into index updates.
- [ ] Implement basic reconciliation for robustness.

#### Specific Tasks

1. **FSEvents Integration (`vicaya-watcher`)**
   - Use `notify` (FSEvents backend) or a dedicated `fsevent-sys` binding.
   - Subscribe to all root directories from config.
   - Persist last FSEvent ID in index metadata for resume.

2. **Event Translation**
   - Define `IndexUpdate` enum: `Create`, `Modify`, `Delete`, `Move`.
   - Map FSEvents flags to these updates.
   - For `Move` events, update path/basename but keep stable `FileId` when possible.

3. **Daemon Coordination (`vicaya-daemon`)**
   - Run scanner on first start or when no index file exists.
   - Start watcher; feed updates into index via async channel.
   - Apply updates:
     - `Create` → add new file to file table + index.
     - `Modify` → update metadata (size, mtime); maybe ranking heuristics.
     - `Delete` → mark entry as tombstoned and remove from index.
     - `Move` → update path/name strings and relevant trigrams.

4. **Reconciliation**
   - Periodic job (e.g., nightly) that:
     - Samples paths from index and checks they still exist.
     - Optionally walks small hot directories fully.
     - Repairs discrepancies (remove stale entries, add missing ones).

5. **IPC Interface**
   - Implement a simple IPC protocol over a Unix domain socket:
     - Request: `{ "type": "search", "query": "...", "limit": N }`.
     - Response: `{ "results": [ { "path": "...", "score": f32, ... } ] }`.
   - CLI uses this instead of loading index itself (fast and allows sharing).

#### Validation Checkpoint

- [ ] Manual test: create/rename/delete files and observe search results updating within seconds.
- [ ] Integration tests using a temporary directory and synthetic FSEvents (or mocked watcher).
- [ ] Daemon can restart without needing a full rescan (uses saved index + FSEvent ID).

---

### Phase 4: UX & Frontends (Target: Day 8–9)

**Timestamp:** 2025-11-27

#### Objectives

- [ ] Implement CLI search subcommands.
- [ ] Implement minimal macOS quick search UI with global hotkey.
- [ ] Add preferences / configuration editing.

#### Specific Tasks

1. **CLI (`vicaya-cli`)**
   - Subcommands:
     - `vicaya search <query> [--path-only] [--limit N] [--sort=rank|recent|path]`.
     - `vicaya rebuild [--roots ...] [--dry-run]`.
     - `vicaya status` (index size, last updated, watcher health).
   - Output formats:
     - Table (default), `--json`, `--plain` (just paths).

2. **Global Hotkey + Popup UI (`vicaya-ui-macos` or in `vicaya-daemon`)**
   - Choose UI stack (Tauri or egui/winit/Cocoa wrapper).
   - Implement:
     - Menu-bar icon with status menu.
     - Keyboard shortcut (configurable; default ⌘⌥Space).
     - Search popup with debounced query sending.
   - Basic features:
     - Keyboard navigation, open file, reveal in Finder, copy path.
     - Optional filters (extension, path prefix) via simple syntax or UI controls.

3. **Preferences Storage**
   - Config file (TOML/JSON) under `~/Library/Application Support/vicaya/config.toml`.
   - Read on startup, watch for changes; UI can edit these settings and trigger daemon reload.

4. **Error Handling & UX**
   - Notify user when Full Disk Access is missing; link to System Settings.
   - Graceful messages when index is building ("Indexing… results may be incomplete").

#### Validation Checkpoint

- [ ] User can install app, run daemon, press hotkey, search files, and open them.
- [ ] CLI search returns same results as UI for equivalent queries.
- [ ] Preferences changes (roots, exclusions) take effect after restart or explicit reload.

---

### Phase 5: Optimization, Hardening & Release (Target: Day 10+)

**Timestamp:** 2025-11-28

#### Objectives

- [ ] Profile and optimize index/search hot paths.
- [ ] Finalize documentation and examples.
- [ ] Package signed/notarized macOS builds and CLI binaries.

#### Specific Tasks

1. **Performance Profiling**
   - Use `cargo bench` & `criterion` to profile:
     - Index build time for synthetic large datasets.
     - Query performance for various query types and sizes.
   - Optimize:
     - Trigram posting list intersection (bitsets or SIMD if needed).
     - String normalization and comparison.
     - Mmap layout to avoid page thrashing.

2. **Robustness & Edge Cases**
   - Handle:
     - Very long paths.
     - Non-UTF8 paths (fallback representation).
     - Network & external drives appearing/disappearing.
   - Add telemetry counters (internal metrics; not networked) to monitor errors.

3. **Packaging & Distribution**
   - Build `.app` bundle and `.dmg` with `cargo-bundle`/custom script.
   - Sign & notarize using Apple Developer ID.
   - Provide separate CLI binary (can be symlinked from within .app bundle).
   - Optionally publish a Homebrew tap for CLI-only install.

4. **Release Documentation**
   - Update `README.md`, `CHANGELOG.md`, and `docs/ARCHITECTURE.md`.
   - Add usage examples and troubleshooting.

#### Validation Checkpoint

- [ ] Benchmarks meet or exceed performance targets.
- [ ] Manual testing on a "real" user environment with millions of files.
- [ ] Release artifacts tested on a fresh macOS install.

---

## Testing Strategy

### Test Categories

1. **Unit Tests**
   - Target coverage: ≥ 80% of `vicaya-index` and `vicaya-core`.
   - Key areas:
     - Trigram encoding/decoding.
     - Index building & query logic.
     - File table serialization/deserialization.
   - Framework:
     - Rust built-in test harness (`cargo test`).

2. **Integration Tests**
   - Scenarios:
     - Scanning a temporary directory tree with known files.
     - Interacting with a mocked watcher to apply updates.
     - CLI ↔ daemon IPC round trips.
   - Framework:
     - `cargo test` with integration tests under `tests/`.
     - Optional use of `assert_cmd` and `tempfile` crates.

3. **Grounding Tests (Agent-Specific)**
   - Validate that:
     - Index version mismatches cause explicit, logged errors.
     - Config parsing errors are surfaced clearly with file paths and line numbers.
     - Timestamps in logs are in a consistent format.
   - Ensure tests fail when assumptions change (e.g., config schema).

4. **Performance Tests**
   - Use `criterion` benchmarks in `benches/`:
     - Index construction for N synthetic entries.
     - Query performance for varied query lengths and result counts.
   - Optional load tests:
     - Simple scripts firing many queries concurrently via CLI/IPC.

### Test-Driven Checkpoints

- [ ] For each index feature, add failing unit tests first (e.g., substring match semantics) before implementing.
- [ ] For watcher integration, build tests with a mock FS layer first.
- [ ] Before merging to `main`, CI must run full test suite and benchmarks (smoke subset).

### Test Organization

```text
tests/
├── unit/
│   ├── index_trigram_tests.rs
│   ├── file_table_tests.rs
│   └── config_tests.rs
├── integration/
│   ├── scan_and_search.rs
│   ├── daemon_ipc.rs
│   └── watcher_mock.rs
├── performance/
│   ├── benches_index.rs
│   └── benches_query.rs
└── fixtures/
    ├── small_fs_tree/
    └── synthetic_index_dumps/
```

---

## Development Standards

### Environment Setup

- **Runtime:** Latest stable Rust (via `rustup`, pinned in `rust-toolchain.toml`).
- **Package Manager:** `cargo`.
- **Target Platform:** macOS (Apple Silicon + Intel).
- **Key Dependencies (initial)**:
  - `clap` – CLI argument parsing.
  - `tracing` + `tracing-subscriber` – structured logging.
  - `serde` + `serde_json` + `toml` – config & IPC (JSON) parsing.
  - `hashbrown` – fast hash maps/sets.
  - `memmap2` – memory-mapped file support.
  - `rayon` – parallel scanning.
  - `ignore` / `walkdir` – fast filesystem walking with ignore rules.
  - `notify` or `fsevent-sys` – FSEvents-based watcher.
  - `bincode` or `rkyv` – compact binary serialization.
  - `criterion` – benchmarks.
  - (Optional) `tokio` – async runtime for daemon + IPC.

### Code Quality Standards

- **Linting:** `cargo clippy --all-targets --all-features` (no warnings allowed).
- **Formatting:** `cargo fmt` (must be clean before commit).
- **Type Checking:** Rust compiler (no `unsafe` unless well-justified & reviewed).
- **Security Scanning:** `cargo audit` for known vulnerable dependencies (in CI).

### Git Workflow

#### Commit Standards

- Format: `type(scope): brief description`
- Types: `feat|fix|docs|style|refactor|test|chore|perf|build`
- Example scopes: `index`, `scanner`, `daemon`, `cli`, `ui`

Examples:

```bash
git add crates/vicaya-index/src/lib.rs tests/unit/index_trigram_tests.rs
git commit -m "feat(index): add trigram-based substring search"

git add docs/ARCHITECTURE.md
git commit -m "docs(architecture): document index persistence layout"

git add crates/vicaya-scanner/src/lib.rs
git commit -m "perf(scanner): parallelize directory traversal"
```

Bad (do not use):

```bash
git add -A
git commit -m "updates"

git add .
git commit -m "fix stuff and more changes"
```

#### Staging Discipline

```bash
# Always explicit staging
git add crates/vicaya-index/src/index.rs
git add tests/unit/index_trigram_tests.rs
git status

# Avoid bulk staging
# ❌ git add -A
# ❌ git add .
# ❌ git add *
```

#### Branch Strategy

- `main`: production-ready, tagged releases only.
- `develop`: integration branch for upcoming release.
- `feature/*`: feature work (e.g., `feature/index-trigram`).
- `fix/*`: bug fixes.
- `release/*`: release prep branches.

### CI/CD Configuration

#### Makefile Targets (Rust-Oriented)

```makefile
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
```

#### Git Hooks (pre-push)

Use `lefthook` or simple `git` hooks:

```yaml
# .lefthook.yml
pre-push:
  parallel: false
  commands:
    fmt:
      run: make fmt
    lint:
      run: make lint
    test:
      run: make test
```

#### GitHub Actions Workflow

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [ main, develop ]
  pull_request:
    branches: [ main, develop ]

jobs:
  build-and-test:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      - name: Install dev tools
        run: |
          cargo install cargo-audit || true

      - name: Run CI
        run: |
          make ci
          cargo audit || true
```

---

## Project Structure

```text
vicaya/
├── Cargo.toml                # Workspace definition
├── rust-toolchain.toml
├── Makefile
├── .github/
│   └── workflows/
│       ├── ci.yml
│       └── release.yml
├── crates/
│   ├── vicaya-core/
│   │   ├── src/
│   │   │   ├── config.rs
│   │   │   ├── logging.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── vicaya-index/
│   │   ├── src/
│   │   │   ├── file_table.rs
│   │   │   ├── string_arena.rs
│   │   │   ├── trigram_index.rs
│   │   │   ├── query.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── vicaya-scanner/
│   │   ├── src/lib.rs
│   │   └── Cargo.toml
│   ├── vicaya-watcher/
│   │   ├── src/lib.rs
│   │   └── Cargo.toml
│   ├── vicaya-daemon/
│   │   ├── src/main.rs
│   │   └── Cargo.toml
│   ├── vicaya-cli/
│   │   ├── src/main.rs
│   │   └── Cargo.toml
│   └── vicaya-ui-macos/      # optional / later
│       ├── src/main.rs
│       └── Cargo.toml
├── config/
│   └── default.toml
├── docs/
│   ├── vicaya.md             # This implementation guide
│   ├── ARCHITECTURE.md
│   ├── API.md
│   ├── DEVELOPMENT.md
│   └── CONTRIBUTING.md
├── tests/
│   ├── unit/
│   ├── integration/
│   ├── performance/
│   └── fixtures/
└── CHANGELOG.md
```

---

## Documentation Standards

### Required Documentation

1. **README.md**
   - Short description: "vicaya — blazing-fast filesystem search for macOS in Rust."
   - Quick start:
     - Install (CLI & app).
     - Start daemon.
     - Run first search.
   - Screenshots / GIFs of the UI.
2. **CHANGELOG.md**
   - Keep a Changelog format.
   - Semantic versions (`v0.1.0`, `v0.2.0`, …).
3. **CONTRIBUTING.md**
   - How to set up Rust, run tests, and build the app.
   - Coding standards and commit guidelines.
4. **ARCHITECTURE.md**
   - Deep dive into scanner, watcher, index, IPC protocol, and UI.
5. **API / IPC Documentation (API.md)**
   - JSON schema for IPC messages (search requests, status, etc.).
6. **Inline Docs**
   - Rustdoc for all public types and functions.
   - Comments for tricky performance-sensitive code.

### Documentation Updates

- [ ] After each implementation phase, ensure this `vicaya.md` is updated with reality.
- [ ] Update `ARCHITECTURE.md` whenever persistence or IPC formats change.
- [ ] Maintain `CHANGELOG.md` with every functional change merged to `main`.

### Documentation Tools

- Rustdoc (`cargo doc`) for API docs.
- Markdown + Mermaid diagrams in `docs/` for architecture visuals.

---

## Performance Considerations

### Benchmarks

Establish baseline before heavy optimization:

- Index build benchmark:
  - 1M synthetic file entries.
  - Target: ≤ 90 seconds on typical dev Mac.
- Query benchmarks:
  - Mix of short (2–3 letters) and medium (5–10 letters) substring queries.
  - Target: p95 ≤ 20 ms.

### Key Metrics

1. **Response Time**
   - p50: ≤ 5 ms
   - p95: ≤ 20 ms
   - p99: ≤ 40 ms

2. **Throughput**
   - Queries/sec: sustain 30+ without significant tail latencies.

3. **Resource Usage**
   - Memory: ≤ 500 MB at 5M entries (configurable cap).
   - CPU: Idle when not handling events/queries.
   - Disk I/O: Burst during scan; minimal steady-state writes.

### Optimization Areas

1. **Index Structures**
   - Evaluate roaring bitmaps vs sorted `Vec<FileId>` for posting lists.
   - Tune trigram selection & intersection order (smallest lists first).

2. **Memory Layout**
   - Optimize `FileMeta` struct size & alignment.
   - Group hot fields (name offsets, path offsets) close together.

3. **Filesystem Interaction**
   - Batch stat calls where possible.
   - Respect ignore rules to reduce traversal.

### Profiling Checkpoints

- After Phase 2 (index & scanner): run early benchmarks.
- After Phase 3 (watcher): ensure FSEvents overhead is minimal.
- After Phase 4 (frontends): confirm UI layer doesn't dominate latency.
- Pre-release: final perf test on real user environment.

### Performance Testing Commands

```bash
make bench          # criterion benchmarks
make profile-cpu    # (custom script using perf/instruments)
make profile-mem    # memory profiling using instruments
```

---

## Security Standards

### Security Checklist

- [ ] Input validation for CLI/IPC (no path traversal beyond interpretation).
- [ ] No elevation or bypass of macOS TCC.
- [ ] Index files stored under user's home directory with appropriate permissions.
- [ ] Safe handling of non-UTF8 paths (no crashes).
- [ ] No external network communication (offline-by-default utility).
- [ ] Potential future option: encrypt index at rest (if multi-user concerns arise).

### Dependency Security

- [ ] `cargo audit` runs in CI to detect vulnerable crates.
- [ ] `Cargo.lock` committed for reproducible builds.
- [ ] External crates kept minimal and well-maintained.

### Security Testing

- [ ] Fuzz targeted parsing code (config, IPC messages).
- [ ] Manual tests around symlinks, hard links, network volumes.
- [ ] Validate that index does not expose paths user would not see in Finder given current permissions.

---

## Release Process

### Release Checklist

- [ ] Bump version in `Cargo.toml` and `CHANGELOG.md`.
- [ ] Update docs (`README`, `vicaya.md`, `ARCHITECTURE.md`).
- [ ] Ensure CI green and benchmarks acceptable.
- [ ] Build signed/notarized `.app` and CLI binaries.
- [ ] Tag release (`git tag vX.Y.Z && git push --tags`).

### GitHub Release Workflow (Sketch)

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - "v*.*.*"

jobs:
  build-release:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      - name: Build release binaries
        run: |
          cargo build --workspace --release
          # TODO: bundle .app, create dmg, sign & notarize
      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: vicaya-macos-release
          path: target/release/*
```

### Semantic Versioning

- `MAJOR`: breaking changes to index format or IPC protocol.
- `MINOR`: new features, backward compatible.
- `PATCH`: bug fixes and small improvements.

---

## Document Maintenance

### Update Triggers

- [ ] End of each implementation phase.
- [ ] Changes to data formats (index, IPC).
- [ ] New frontends or major features added.
- [ ] Significant performance findings or regressions.
- [ ] Security or dependency changes.

### Timestamp Protocol

When making a notable change to this guide, append an update entry:

```bash
echo "### Update - $(date '+%Y-%m-%d %H:%M:%S %Z')" >> docs/vicaya.md
echo "- [Short description of change]" >> docs/vicaya.md
echo "" >> docs/vicaya.md
```

### Review Schedule

- **During active development:** review this doc at the start of each workday.
- **Before releases:** ensure all checklists & sections reflect reality.
- **Post-release:** add "Lessons Learned" to `ARCHITECTURE.md` or a `RETROSPECTIVE.md` if needed.

### Version Control for Docs

```bash
git add docs/vicaya.md docs/ARCHITECTURE.md CHANGELOG.md
git commit -m "docs(vicaya): update implementation plan after Phase 2"
```

---

## Appendix: Rust Crates & Decisions

This section summarizes chosen crates and rationale for agents.

### Core Crates

- `clap`: ergonomic CLI, good help text, stable.
- `tracing` / `tracing-subscriber`: structured logs that can later be wired to log views.
- `hashbrown`: performant hash maps/sets that may out-perform std for hot paths.
- `memmap2`: widely used for cross-platform mmap.

### FS & Concurrency

- `ignore` + `walkdir`: high-performance walker, supports `.gitignore`-style patterns.
- `rayon`: dead-simple data-parallelism for scanning.
- `notify` or `fsevent-sys`: FSEvents integration (to be decided once PoC is done).

### Serialization & Config

- `serde`, `serde_json`, `toml`: standard ecosystem for config + IPC.
- `bincode` or `rkyv`: compact binary formats for index on disk.

### Async Runtime (Optional)

- `tokio`: if daemon needs async IPC and timers; otherwise, can use standard threads + channels.

---

**End of vicaya (विचय) Implementation Guide v0.1.0**

---

## Implementation Progress

### Update - 2025-11-26 02:33 UTC

**Phase 1: Foundation - COMPLETED ✅**
- ✅ Rust workspace with 6 crates created
- ✅ Core types, configuration, and logging integrated
- ✅ CI pipeline with GitHub Actions configured
- ✅ Makefile with standard dev tasks
- ✅ All code formatted and linted (cargo fmt, cargo clippy pass)

**Phase 2: Core Features - COMPLETED ✅**
- ✅ File table with efficient serialization
- ✅ String arena for path storage
- ✅ Trigram index with fast substring search
- ✅ Query engine with scoring and ranking
- ✅ Parallel filesystem scanner
- ✅ Index persistence with bincode

**Phase 3: Live Updates - IN PROGRESS 🚧**
- ✅ Basic FSEvents watcher skeleton
- ⏳ Event translation to index updates
- ⏳ Daemon coordination
- ⏳ IPC protocol implementation

**Testing Status:**
- All unit tests passing
- CLI tested with sample data
- Index rebuild: ✅ Working
- Search: ✅ Working with trigram matching
- Status command: ✅ Working

**Next Steps:**
1. Implement daemon IPC server (Unix domain sockets)
2. Wire watcher updates to index
3. Add reconciliation logic
4. Begin macOS UI development

