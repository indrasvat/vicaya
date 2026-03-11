# Task 008: Add Startup Directory Scoping for GH Issue #30

- **Phase:** UX enhancement
- **Status:** TODO
- **GitHub Issue:** `#30`
- **Recommended Worktree:** `codex/task-008-startup-scope-cli-tui`
- **Depends on:** None
- **Blocks:** None
- **L4 Visual:** Required
- **Shared Learnings:** Consult [`docs/LEARNINGS.md`](../LEARNINGS.md) before implementation or automation

## Problem

`vicaya-tui .` and `vicaya-tui /path` do not currently set startup `ksetra`. The CLI also has no explicit `--scope` option, and the existing internal scope input is only used as a ranking-context hint rather than a strict subtree restriction.

That means users must launch globally and then manually narrow `ksetra`, and scoped CLI searches cannot be expressed as an explicit public interface.

## Files Likely In Scope

- [`crates/vicaya-tui/src/main.rs`](../../crates/vicaya-tui/src/main.rs)
- [`crates/vicaya-tui/src/app.rs`](../../crates/vicaya-tui/src/app.rs)
- [`crates/vicaya-cli/src/main.rs`](../../crates/vicaya-cli/src/main.rs)
- [`crates/vicaya-core/src/ipc.rs`](../../crates/vicaya-core/src/ipc.rs)
- [`crates/vicaya-index/src/query.rs`](../../crates/vicaya-index/src/query.rs)
- [`.claude/automations/test_vicaya_tui_startup_scope.py`](../../.claude/automations/test_vicaya_tui_startup_scope.py)
- [`README.md`](../../README.md)

## Proposed Solution

Add an optional positional directory argument to `vicaya-tui`, and add an explicit `--scope <DIR>` flag to `vicaya search`.

Carry scope through the full search pipeline as two separate concepts:

- `scope`: ranking-context boost
- `filter_scope`: hard subtree restriction

When startup scope or explicit CLI scope is provided, set both fields to the same directory. When no explicit scope is provided, preserve existing ranking behavior.

## Definition of Done

- `vicaya-tui .` and `vicaya-tui /abs/path` start with `ksetra` already set
- TUI startup recents are scoped when startup scope is provided
- `vicaya search <query> --scope <DIR>` returns only paths under `DIR`
- invalid scope inputs fail clearly and non-interactively
- no-scope behavior remains unchanged
- docs and help text are updated with sanitized examples only
- hyperfine results are captured and summarized
- iTerm2 startup-scope automation captures screenshots and verifies realistic scoped workflows

## Testing Strategy

### L1 / L2

- Add TUI arg-parsing tests for:
  - no arg
  - relative scope
  - missing path rejection
  - file path rejection
- Add CLI parsing/request-construction tests for:
  - `search foo --scope .`
  - default no-scope behavior
- Add IPC serde coverage for `filter_scope`
- Add query-engine coverage proving:
  - out-of-scope exact matches are excluded when `filter_scope` is set
  - recent-files obey `filter_scope`
- Add integration coverage proving scoped daemon/CLI searches only return in-scope results

### L3

- Run targeted crate tests for `vicaya-core`, `vicaya-index`, `vicaya-daemon`, `vicaya-cli`, and `vicaya-tui`
- Run full workspace `fmt`, `clippy`, and `cargo test`

### L4

- Run:
  - `uv run .claude/automations/test_vicaya_tui_startup_scope.py`
  - `uv run .claude/automations/test_vicaya_tui_core_visual.py`
  - `uv run .claude/automations/test_vicaya_tui_scope_navigation.py`
  - `uv run .claude/automations/test_vicaya_tui_utf8_input.py`
- Review screenshots under `.claude/screenshots/`
- Use runtime fixture discovery for `~/code/github.com`, `~/Documents`, `~/Desktop`, and `~/Downloads`
- Keep repo-tracked assertions and docs sanitized; do not commit personal local filenames or paths

### Benchmarks

- Run `hyperfine` comparisons for:
  - unscoped repo query baseline
  - scoped repo query
  - scoped code-file query
  - scoped downloads-style query if a generic runtime fixture exists
- Record results in the PR and [`docs/BENCHMARKS.md`](../BENCHMARKS.md) without personal path leakage
