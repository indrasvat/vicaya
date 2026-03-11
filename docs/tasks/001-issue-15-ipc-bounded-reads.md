# Task 001: Bound IPC Reads for GH Issue #15

- **Phase:** Bug remediation
- **Status:** TODO
- **GitHub Issue:** `#15`
- **Recommended Worktree:** `codex/daemon-safety`
- **Depends on:** None
- **Blocks:** None
- **L4 Visual:** Shared smoke only
- **Shared Learnings:** Consult [`docs/LEARNINGS.md`](/Users/indrasvat/code/github.com/indrasvat-vicaya/docs/LEARNINGS.md) before implementation or automation

## Problem

The IPC protocol currently relies on unbounded `read_line()` calls in daemon and client code. A malformed newline-less payload can force arbitrary memory growth and locally DoS the daemon or client.

## Files Likely In Scope

- [`crates/vicaya-daemon/src/ipc_server.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-daemon/src/ipc_server.rs)
- [`crates/vicaya-cli/src/ipc_client.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-cli/src/ipc_client.rs)
- [`crates/vicaya-tui/src/client/mod.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-tui/src/client/mod.rs)
- [`crates/vicaya-core/src/daemon.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-core/src/daemon.rs)

## Proposed Solution

Introduce a shared bounded message reader for newline-delimited JSON IPC. The helper must stop reading once a configured byte cap is exceeded and return a typed error before the process allocates an unbounded `String`.

Prefer one shared implementation over ad hoc guards added after `read_line()`.

## Definition of Done

- Oversized or newline-less IPC payloads are rejected before unbounded allocation.
- Daemon remains responsive after a malformed client connection attempt.
- CLI and TUI clients reject oversized daemon responses cleanly.
- Message size cap is centralized and documented.
- Regression tests cover both valid-under-limit and invalid-over-limit cases.

## Testing Strategy

### L1 / L2

- Add unit tests for the bounded reader helper:
  - newline-terminated message under limit
  - EOF without newline under limit
  - payload over limit with no newline
  - payload over limit before newline

### L3

- Add a local Unix-socket integration test proving the daemon rejects a malformed oversized request and still serves a valid request afterward.
- Run focused tests for daemon/client IPC paths.

### L4

- Run:
  - `uv run .claude/automations/test_vicaya_tui_core_visual.py`
  - `uv run .claude/automations/test_vicaya_tui_scope_navigation.py`

This issue is not visual, but it touches the TUI client IPC path and should not regress startup/search behavior.
