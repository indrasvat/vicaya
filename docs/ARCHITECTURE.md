# vicaya Architecture

vicaya (विचय) is a macOS-native filesystem search tool that provides instant
search-as-you-type results using a trigram-based inverted index, achieving
sub-20ms query latency over millions of files.

```
┌──────────────────────────────────────────────────────────────────────┐
│                        vicaya System Overview                        │
│                                                                      │
│  ┌───────────┐  ┌───────────┐     ┌──────────────────────────────┐  │
│  │  vicaya    │  │ vicaya-tui│     │        vicaya-daemon         │  │
│  │  (CLI)     │  │  (TUI)   │     │                              │  │
│  └─────┬─────┘  └─────┬─────┘     │  ┌────────┐  ┌───────────┐  │  │
│        │               │           │  │Watcher │  │Reconcile  │  │  │
│        │    Unix Socket IPC        │  │Thread  │  │Thread     │  │  │
│        └───────┬───────┘           │  └───┬────┘  └─────┬─────┘  │  │
│                │                   │      │             │         │  │
│                ▼                   │      ▼             ▼         │  │
│         ┌─────────────┐           │  ┌──────────────────────┐    │  │
│         │  IPC Server │◄──────────┤  │Arc<RwLock<DaemonState>>│   │  │
│         │ (main thread)│          │  └──────────┬───────────┘    │  │
│         └─────────────┘           │             │                │  │
│                                   │             ▼                │  │
│                                   │  ┌──────────────────────┐    │  │
│                                   │  │   IndexSnapshot       │    │  │
│                                   │  │  ┌────────────────┐  │    │  │
│                                   │  │  │  FileTable      │  │    │  │
│                                   │  │  │  StringArena    │  │    │  │
│                                   │  │  │  TrigramIndex   │  │    │  │
│                                   │  │  └────────────────┘  │    │  │
│                                   │  └──────────────────────┘    │  │
│                                   └──────────────────────────────┘  │
│                                                                      │
│  ┌───────────┐  ┌───────────┐  ┌─────────────┐                     │
│  │vicaya-core│  │vicaya-index│  │vicaya-scanner│                    │
│  │(shared)   │  │(index lib) │  │(fs walker)   │                    │
│  └───────────┘  └───────────┘  └─────────────┘                     │
│  ┌─────────────┐                                                    │
│  │vicaya-watcher│                                                   │
│  │(FSEvents)    │                                                   │
│  └─────────────┘                                                    │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Table of Contents

- [Crate Overview](#crate-overview)
- [Crate Dependencies](#crate-dependencies)
- [Data Flow](#data-flow)
- [Index Structures](#index-structures)
- [Query Engine](#query-engine)
- [Daemon Architecture](#daemon-architecture)
- [Filesystem Event Handling](#filesystem-event-handling)
- [TUI Architecture](#tui-architecture)
- [Design Decisions](#design-decisions)
- [Performance Characteristics](#performance-characteristics)
- [Keeping This Doc Updated](#keeping-this-doc-updated)

---

## Crate Overview

| Crate | Purpose | Binary? |
|---|---|---|
| `vicaya-core` | Config, logging, error types, IPC protocol, path utilities, filter rules | No (lib) |
| `vicaya-index` | FileTable, StringArena, TrigramIndex, QueryEngine, AbbreviationMatcher | No (lib) |
| `vicaya-scanner` | Filesystem walker (walkdir/rayon), builds `IndexSnapshot` | No (lib) |
| `vicaya-watcher` | FSEvents wrapper (notify crate), emits `IndexUpdate` events | No (lib) |
| `vicaya-daemon` | Background service: loads index, handles IPC, applies live updates | Yes |
| `vicaya-cli` | CLI binary (`vicaya`): search, rebuild, daemon control, metrics | Yes |
| `vicaya-tui` | Terminal UI (`vicaya-tui`): streaming search with preview pane | Yes |

## Crate Dependencies

```
vicaya-cli ──────┬──► vicaya-core
                 ├──► vicaya-index
                 └──► vicaya-scanner ──┬──► vicaya-core
                                       └──► vicaya-index

vicaya-tui ──────┬──► vicaya-core
                 └──► vicaya-index

vicaya-daemon ───┬──► vicaya-core
                 ├──► vicaya-index
                 ├──► vicaya-scanner
                 └──► vicaya-watcher ──┬──► vicaya-core
                                       └──► vicaya-index

vicaya-index ────┬──► vicaya-core

vicaya-core ─────┘  (leaf dependency — no workspace deps)
```

---

## Data Flow

### Startup Sequence

```
1. Load config        ~/Library/Application Support/vicaya/config.toml
         │
         ▼
2. Check index        index/index.bin exists?
         │
    ┌────┴────┐
    │ Yes     │ No
    ▼         ▼
3a. Load    3b. Full scan
    snapshot     via Scanner
         │         │
         └────┬────┘
              ▼
4. Replay journal     index/index.journal (line-delimited JSON)
              │
              ▼
5. Init DaemonState   Arc<RwLock<DaemonState>>
              │
         ┌────┼────────────────┐
         ▼    ▼                ▼
6a. IPC  6b. Watcher      6c. Reconcile
    Server    Thread           Thread
    (main)    (spawn)          (spawn)
```

### Query Execution Flow

```
Client                    Daemon                      Index
  │                         │                           │
  │  Request::Search        │                           │
  │ ────────────────────►   │                           │
  │  (JSON over Unix sock)  │                           │
  │                         │  state.read()             │
  │                         │──────────────►            │
  │                         │                           │
  │                         │  QueryEngine::search()    │
  │                         │──────────────────────────►│
  │                         │                           │
  │                         │  Extract trigrams          │
  │                         │  Intersect posting lists   │
  │                         │  Score & rank candidates   │
  │                         │                           │
  │                         │  Vec<SearchResult>         │
  │                         │◄──────────────────────────│
  │                         │                           │
  │  Response::SearchResults│                           │
  │ ◄────────────────────── │                           │
```

### Live Update Flow

```
Filesystem Event (FSEvents via notify)
         │
         ▼
┌─────────────────┐
│  vicaya-watcher  │    Converts to IndexUpdate:
│                  │    Create | Modify | Delete | Move
└────────┬─────────┘
         │
         ▼
┌─────────────────┐
│  Watcher Thread  │    1. Filter internal paths (vicaya state dir)
│  (daemon)        │    2. Acquire journal_lock → append to journal
│                  │    3. Acquire state.write() → apply_update()
└──────────────────┘
         │
         ▼
┌─────────────────┐
│  DaemonState     │    Updates in-memory:
│  .apply_update() │    - FileTable (add/modify/tombstone entries)
│                  │    - StringArena (append new paths)
│                  │    - TrigramIndex (update posting lists)
│                  │    - path_to_id / inode_to_id maps
└──────────────────┘
```

---

## Index Structures

### StringArena

A contiguous byte buffer storing all file paths. Strings are accessed via
`(offset, length)` pairs, providing zero-copy lookups with minimal overhead.

```
StringArena.data: Vec<u8>
┌───────────────────────────────────────────────────────────┐
│ /Users/a/foo.rs\0/Users/a/bar.rs\0/Users/b/baz.txt\0... │
└───────────────────────────────────────────────────────────┘
  ▲ offset=0, len=16   ▲ offset=17, len=16
```

Each file stores two arena references: one for the full path and one for the
basename. This avoids redundant string allocations and keeps all path data
cache-friendly.

### FileTable

A dense `Vec<FileMeta>` indexed by `FileId(u32)`, supporting up to ~4.2 billion
entries.

```rust
struct FileMeta {
    path_offset: usize,   // Full path in StringArena
    path_len: usize,
    name_offset: usize,   // Basename in StringArena
    name_len: usize,
    size: u64,            // File size in bytes
    mtime: i64,           // Modification time (Unix epoch)
    dev: u64,             // Device ID (for inode identity)
    ino: u64,             // Inode number
}
```

Deleted entries are tombstoned in place (path_len=0, name_len=0, mtime=0)
rather than removed, keeping FileId indices stable.

### TrigramIndex

An inverted index mapping 3-character sequences to the files containing them.

```
Trigram encoding: (byte0 << 16) | (byte1 << 8) | byte2

Example: "hello.rs" → trigrams: ["hel", "ell", "llo", "lo.", "o.r", ".rs"]

TrigramIndex: HashMap<Trigram, Vec<FileId>>
┌─────────┬──────────────────┐
│ "hel"   │ [42, 87, 1203]   │
│ "ell"   │ [42, 87, 556]    │
│ "llo"   │ [42, 1203, 3001] │
│ ...     │ ...               │
└─────────┴──────────────────┘

Query "hello": trigrams ["hel","ell","llo"]
  → intersect posting lists
  → candidates: [42] (appears in all three)
  → verify substring match on candidate filenames
```

**Key optimization:** Intersection starts with the smallest posting list,
reducing the number of candidates checked against subsequent lists.

Uses `hashbrown::HashMap` for faster hashing than the standard library.

### IndexSnapshot

The serializable bundle that ties all three structures together:

```rust
struct IndexSnapshot {
    file_table: FileTable,
    string_arena: StringArena,
    trigram_index: TrigramIndex,
}
```

Serialized to disk via `bincode` as `index/index.bin`. Trigrams are indexed
from the **basename only** (not the full path) to keep index size manageable
and search focused on filenames.

---

## Query Engine

### Search Algorithm

```
Input: query string, result limit, optional scope directory

                    ┌──────────────┐
                    │ query.len()  │
                    │   < 3 chars? │
                    └──────┬───────┘
                     yes   │    no
                ┌──────────┘    └──────────┐
                ▼                          ▼
        Linear Scan              Trigram Intersection
        (early termination       1. Extract query trigrams
         after 1000 misses)      2. Intersect posting lists
                │                3. Score each candidate
                │                          │
                └──────────┬───────────────┘
                           ▼
                    Score & Rank
                    1. Abbreviation match
                    2. Substring match
                    3. Context penalties
                    4. Sort & limit
```

### Scoring (0.0 to 1.0)

| Match Type | Score Range | Example |
|---|---|---|
| Exact basename match | 1.0 | query "main.rs" matches file "main.rs" |
| Prefix match | 0.9 - 0.99 | query "main" matches "main.rs" |
| Word boundary match | 0.7 | query "table" matches "file_table.rs" |
| Substring match | 0.5 | query "tab" matches "filetable.rs" |
| Trigram-only match | 0.3 | trigrams match but no clean substring |

### Abbreviation Matching

Four strategies evaluated in order (best score wins):

1. **Exact Prefix** (0.98-1.0) — Query matches start of filename or path component
2. **Component First Letters** (0.90-0.99) — First letters of path segments
3. **CamelCase / Word Boundary** (0.85-0.96) — Matches at uppercase letters or separators
4. **Sequential** (0.50-0.88) — Characters appear in order with gaps allowed

### Secondary Ranking

When primary scores are equal, tie-breaking uses (in order):

1. Context score — penalizes dependency caches, build outputs, tool directories
2. Modification time — prefer recently changed files
3. Path depth — prefer shallower paths
4. Path alphabetical

### Context Score Penalties

| Path Pattern | Penalty | Rationale |
|---|---|---|
| `/go/pkg/mod/` | -100 | Go module cache |
| `node_modules/` | -90 | npm packages |
| `.cargo/` | -90 | Rust crate cache |
| `library/caches/`, `.cache/` | -80 | OS/app caches |
| `library/developer/xcode/deriveddata/` | -80 | Xcode build cache |
| `target/`, `dist/`, `build/`, `out/` | -60 | Build outputs |
| `.git/` | -40 | Git internals |
| `.idea/`, `.vscode/` | -20 | IDE configuration |

### Scope Boost

When a scope directory is active, files within it receive a bonus of up to
+120 points, decreasing with relative depth.

---

## Daemon Architecture

### Thread Model

The daemon runs three threads that share state via `Arc<RwLock<DaemonState>>`:

```
┌────────────────────────────────────────────────────────┐
│                    vicaya-daemon                        │
│                                                        │
│  Main Thread          Watcher Thread    Reconcile Thread│
│  ┌──────────────┐    ┌──────────────┐  ┌─────────────┐│
│  │ IPC Server   │    │ Poll FSEvents│  │ Startup     ││
│  │              │    │ every 50ms   │  │  reconcile  ││
│  │ Accept conn  │    │              │  │             ││
│  │ Parse JSON   │    │ Filter self  │  │ Daily       ││
│  │ Handle req   │    │  updates     │  │  rebuild    ││
│  │ Send resp    │    │              │  │  (3 AM)     ││
│  │              │    │ Journal +    │  │             ││
│  │ read lock    │    │  apply       │  │ write lock  ││
│  │ for queries  │    │              │  │ during      ││
│  │              │    │ write lock   │  │  finalize   ││
│  └──────────────┘    └──────────────┘  └─────────────┘│
│         │                   │                │         │
│         └───────────┬───────┴────────────────┘         │
│                     ▼                                  │
│         ┌──────────────────────┐                       │
│         │ Arc<RwLock           │                       │
│         │   <DaemonState>>    │                       │
│         └──────────────────────┘                       │
│                                                        │
│  Synchronization primitives:                           │
│  - RwLock<DaemonState>  multiple readers / one writer  │
│  - Mutex<()> journal_lock   journal file writes        │
│  - Mutex<()> rebuild_lock   serializes full rebuilds   │
│  - AtomicBool shutdown      graceful shutdown signal   │
│                                                        │
│  Lock ordering: rebuild_lock → state.write → journal   │
└────────────────────────────────────────────────────────┘
```

### DaemonState

```rust
struct DaemonState {
    config: Config,
    index_file: PathBuf,                          // index/index.bin
    journal_file: PathBuf,                        // index/index.journal
    snapshot: IndexSnapshot,                      // In-memory index
    path_hasher: RandomState,                     // Deterministic path hashing
    path_to_id: HashMap<u64, FileId>,             // path_hash → FileId
    path_hash_collisions: HashMap<u64, Vec<FileId>>,  // Collision overflow
    inode_to_id: HashMap<(u64, u64), FileId>,     // (dev, ino) → FileId
    last_updated: i64,                            // Last update epoch seconds
    reconciling: bool,                            // True during rebuild
}
```

The dual path map (`path_to_id` + `path_hash_collisions`) avoids allocating
vectors for the common case where path hashes are unique, while still handling
collisions correctly.

### Journal Persistence

The journal provides crash recovery by recording every `IndexUpdate` before
applying it to memory:

```
┌──────────────────────────────────────────────────────────┐
│                     Journal Lifecycle                     │
│                                                          │
│  Startup                                                 │
│  ├── Load index.bin (snapshot)                           │
│  └── Replay index.journal line by line                   │
│       └── apply_update() for each entry                  │
│                                                          │
│  Runtime (watcher thread)                                │
│  ├── Acquire journal_lock                                │
│  ├── Append IndexUpdate as JSON line                     │
│  ├── Release journal_lock                                │
│  └── Acquire state.write() → apply_update()              │
│                                                          │
│  Rebuild (reconcile or manual)                           │
│  ├── Scan filesystem → new IndexSnapshot                 │
│  ├── Record journal offset before scan                   │
│  ├── Acquire state.write() + journal_lock                │
│  ├── Apply journal entries since offset (catch up)       │
│  ├── Save new snapshot to index.bin                      │
│  ├── Truncate journal                                    │
│  └── Release locks                                       │
└──────────────────────────────────────────────────────────┘
```

Journal format: newline-delimited JSON, one `IndexUpdate` per line.

```json
{"Create":{"path":"/Users/a/new_file.rs"}}
{"Modify":{"path":"/Users/a/changed.rs"}}
{"Delete":{"path":"/Users/a/removed.rs"}}
{"Move":{"from":"/Users/a/old.rs","to":"/Users/a/new.rs"}}
```

### IPC Protocol

Communication uses newline-delimited JSON over a Unix domain socket
(`daemon.sock`).

**Requests** (client → daemon):

| Variant | Fields | Purpose |
|---|---|---|
| `Search` | query, limit, scope, recent_if_empty | Execute search or return recent files |
| `Status` | — | Get daemon statistics |
| `Rebuild` | dry_run | Trigger full index rebuild |
| `Shutdown` | — | Graceful daemon shutdown |

**Responses** (daemon → client):

| Variant | Fields | Purpose |
|---|---|---|
| `SearchResults` | results (vec) | Search matches with path, name, score, size, mtime |
| `Status` | pid, build, indexed_files, trigram_count, arena_size, etc. | Daemon health and index stats |
| `RebuildComplete` | files_indexed | Confirmation after rebuild |
| `Ok` | — | Generic success (shutdown) |
| `Error` | message | Error description |

### Single-Instance Enforcement

Before binding the socket, the daemon checks if an existing socket is
connectable. If so, it exits with "Daemon already running". If the socket
exists but is stale (not connectable), it removes it and binds fresh.

The CLI also checks `daemon.pid` + signal 0 to verify liveness.

### Full Rebuild Process

```
1. Acquire rebuild_lock
2. Set state.reconciling = true
3. Record current journal file size (journal_offset)
4. Scan filesystem via Scanner (may take minutes)
5. Finalize under exclusive locks:
   a. Swap new snapshot into state
   b. Rebuild path_to_id and inode_to_id maps
   c. Apply journal entries written since journal_offset
   d. Save snapshot to index.bin
   e. Truncate journal
   f. Set state.reconciling = false
6. Release all locks
```

Step 5c is critical: the watcher thread continues recording events during the
scan. These events are applied after the new snapshot is loaded so no updates
are lost.

---

## Filesystem Event Handling

### Event Translation

The `vicaya-watcher` crate wraps the `notify` crate (which uses FSEvents on
macOS) and translates raw filesystem events into `IndexUpdate` variants:

| FSEvents Notification | IndexUpdate |
|---|---|
| Create | `Create { path }` |
| Modify (content) | `Modify { path }` |
| Remove | `Delete { path }` |
| Rename (both paths available) | `Move { from, to }` |
| Rename (one path, file exists) | `Modify { path }` |
| Rename (one path, file gone) | `Delete { path }` |

### Move Detection via Inodes

File renames are notoriously hard to track because FSEvents may report just the
old path, just the new path, or both. Vicaya uses `(device, inode)` tuples as
the true file identity:

```
inode_to_id: HashMap<(u64, u64), FileId>

Move scenario:
1. File moves from /a/foo.rs → /b/foo.rs
2. FSEvents may report:
   - Delete /a/foo.rs + Create /b/foo.rs  (two events)
   - or: Rename with both paths           (one event)
3. On Create /b/foo.rs:
   - Read inode of /b/foo.rs → (dev=1, ino=12345)
   - Look up (1, 12345) in inode_to_id → existing FileId
   - Update the entry in place (new path, same FileId)
4. Result: no duplicate entries, stable FileId
```

### Internal Update Filtering

The watcher thread filters out events from vicaya's own state directory and
index path to prevent feedback loops (e.g., writing to the journal triggering
a new event).

---

## TUI Architecture

### Event Loop

```
┌──────────────────────────────────────────────────────┐
│                  TUI Main Loop                        │
│                                                      │
│  ┌───────────────────────────────────────────────┐   │
│  │ 1. Collect worker events (non-blocking)       │   │
│  │ 2. Draw UI (ratatui)                          │   │
│  │ 3. Auto-clear stale messages (2s TTL)         │   │
│  │ 4. Check view/scope changes → trigger search  │   │
│  │ 5. Check query changes → 150ms debounce       │   │
│  │ 6. Schedule preview for selected result       │   │
│  │ 7. Poll keyboard events (50ms timeout)        │   │
│  │ 8. Handle input                               │   │
│  │ 9. Check quit                                 │   │
│  └───────────────────────────────────────────────┘   │
│           │                     ▲                    │
│           │ WorkerCommand       │ WorkerEvent        │
│           ▼                     │                    │
│  ┌──────────────────────────────┴──────────────┐     │
│  │           Worker Thread                      │     │
│  │  - 100ms receive timeout                    │     │
│  │  - Coalesces burst requests                 │     │
│  │  - IPC to daemon for search                 │     │
│  │  - Syntax-highlighted file preview          │     │
│  │  - Status polling every 2s                  │     │
│  └─────────────────────────────────────────────┘     │
│           │                                          │
│           │ Unix Socket IPC                          │
│           ▼                                          │
│  ┌─────────────────┐                                 │
│  │  vicaya-daemon   │                                 │
│  └─────────────────┘                                 │
└──────────────────────────────────────────────────────┘
```

### Two-Layer Debouncing

The TUI uses two complementary debouncing mechanisms to prevent query flooding
during search-as-you-type:

```
User types: "h"  →  50ms  →  "e"  →  50ms  →  "l"  →  50ms  →  "l"  →  50ms  →  "o"
                                                                          │
Layer 1 (TUI, 150ms): ─────────────────────────────────── only "hello" sent ──►
                                                                          │
Layer 2 (Worker, coalesce): ─────────────── if burst arrives, keep latest ──►
                                                                          │
Daemon receives: one search for "hello"                                   │
```

**Layer 1 — TUI Query Debounce (150ms):** The main event loop tracks
`last_search_sent_at`. A new search is triggered only if 150ms have elapsed
since the last search was sent. Empty queries bypass the debounce for immediate
recent-file display.

**Layer 2 — Worker Request Coalescing (100ms timeout):** The worker thread
receives commands with a 100ms timeout, then drains any remaining commands
non-blocking. Only the most recent search/preview request is kept; earlier
ones in the burst are discarded.

### Worker Thread

The worker thread handles all I/O off the main thread:

**Commands** (main → worker):
- `Search { id, query, limit, view, scope, niyamas }` — Execute search via daemon IPC
- `Preview { id, path }` — Load and syntax-highlight file preview
- `Quit` — Shut down worker

**Events** (worker → main):
- `SearchResults { id, results, error }` — Search completed
- `PreviewReady { id, path, title, lines, truncated }` — Preview loaded
- `Status { status }` — Periodic daemon status update

Both search and preview use incrementing IDs so the main loop can discard
stale results when the user has already moved on.

### Client-Side Filtering (Niyamas)

The TUI parses structured filters from the query string and applies them
after receiving results from the daemon:

| Filter | Syntax | Example |
|---|---|---|
| Type | `type:file` or `type:dir` | `main type:file` |
| Extension | `ext:rs,go,py` | `config ext:toml` |
| Path | `path:src/` | `main path:crates/` |
| Size | `size:>1mb,<100mb` | `dump size:>10mb` |
| Modified | `mtime:>7d` or `mtime:<2024-01-15` | `readme mtime:>30d` |

### Preview

File previews are built in the worker thread with syntax highlighting via
the `syntect` crate. Limits: 256KB max file size, 4000 max lines. Directory
previews list up to 200 entries.

### Key Timings

| Constant | Value | Location |
|---|---|---|
| Event poll interval | 50ms | `app.rs` main loop |
| Search debounce | 150ms | `app.rs` query change check |
| Worker receive timeout | 100ms | `worker.rs` command receive |
| Status poll interval | 2s | `worker.rs` |
| Message auto-clear | 2s | `app.rs` error/success display |
| Daemon startup wait | 500ms | `main.rs` |

---

## Design Decisions

### Why trigrams over suffix arrays?

Suffix arrays provide exact substring matching but require O(n) space
proportional to the total text length. For millions of filenames, the suffix
array would be enormous. Trigrams trade precision for compactness: the inverted
index is much smaller, and false positives are eliminated by a verification
step. The "smallest posting list first" intersection strategy makes query time
proportional to the rarest trigram rather than the total file count.

### Why bincode for index serialization?

Bincode is a compact binary format with fast encode/decode and native Rust
serde support. It produces smaller files than JSON or MessagePack and avoids
the complexity of zero-copy formats like rkyv (which requires unsafe code and
careful lifetime management). The tradeoff is that bincode is not
human-readable, but the index is a cache that can always be rebuilt.

### Why RwLock over lock-free structures?

The read-heavy workload (many concurrent queries, infrequent writes) maps
naturally to a read-write lock. Lock-free structures (dashmap, crossbeam
skip lists) would add complexity without measurable benefit because:
- Writes are infrequent (filesystem events are batched)
- Write locks are held briefly (individual update application)
- Read locks are never contended with each other

### Why two layers of debouncing?

Neither layer alone is sufficient:
- **TUI debounce only:** Burst keystrokes within the 150ms window would each
  generate a request, wasting IPC round-trips
- **Worker coalescing only:** Every keystroke would cross the channel boundary,
  creating unnecessary allocations and channel pressure
- **Both together:** The TUI suppresses most redundant requests at source, and
  the worker catches any remaining bursts that slip through

### Why inode-based identity?

Path-based identity breaks on rename: a moved file appears as a delete + create,
causing duplicate index entries or lost state. Inodes are stable across renames
on the same filesystem, making `(device, inode)` a reliable identity key.
The extra metadata cost (two u64 fields per file) is negligible compared to
the robustness gained.

### Why journal + snapshot instead of WAL-only?

A write-ahead log alone would grow unboundedly and require full replay on
every startup. The hybrid approach keeps startup fast (load snapshot, replay
only recent journal entries) while still providing durability for incremental
updates. The daily reconciliation resets the journal, bounding its size.

---

## Performance Characteristics

### Query Latency

- **Target:** <20ms for trigram-indexed queries over millions of files
- **Short queries (<3 chars):** Linear scan with early termination after 1000
  consecutive misses
- **Long queries (>=3 chars):** Trigram intersection, typically sub-millisecond
  for the index lookup, with scoring overhead proportional to candidate count

### Memory Efficiency

- **StringArena:** Single contiguous allocation for all paths — no per-string
  heap overhead
- **FileTable:** Dense `Vec<FileMeta>` — no hash table overhead, O(1) lookup by
  FileId
- **TrigramIndex:** One `HashMap<Trigram, Vec<FileId>>` — trigram space is
  bounded (at most 256^3 ≈ 16.7M unique trigrams, far fewer in practice)
- **Tombstoning:** Deleted files are zeroed in place rather than removed,
  avoiding vector compaction costs

### Indexing

- Scanner uses `walkdir` for traversal with configurable exclusion patterns
- Trigrams are extracted only from basenames, keeping the index compact
- `rayon` and `ignore` crates are available for parallel scanning

### IPC

- Unix domain socket — no network overhead, kernel-buffered
- Newline-delimited JSON — simple, debuggable, adequate throughput for
  interactive use
- Blocking writes prevent truncation on large result sets

---

## Keeping This Doc Updated

This document should be updated when:

- Core data structures change (FileTable, StringArena, TrigramIndex, DaemonState)
- IPC protocol or message types are added or modified
- Daemon thread model changes (new threads, different synchronization)
- Persistence format changes (index.bin encoding, journal format)
- New crates are added to the workspace
- Query scoring algorithm changes
- TUI debouncing or event loop timing changes

**Review checklist before committing architecture changes:**

1. Do the ASCII diagrams still reflect the actual component relationships?
2. Are all crate dependencies accurate?
3. Do the listed constants match the source code?
4. Are the design decision rationales still valid?
