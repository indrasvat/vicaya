# Task 005: Harden RenameMode::Both Handling for GH Issue #19

- **Phase:** Bug remediation
- **Status:** TODO
- **GitHub Issue:** `#19`
- **Recommended Worktree:** `codex/watcher-rename-hardening`
- **Depends on:** None
- **Blocks:** None
- **L4 Visual:** Not required

## Problem

The current watcher code uses an existence heuristic for `RenameMode::Both`, `Any`, and `Other` in the same branch. For `RenameMode::Both`, that is unnecessary and may be semantically wrong because the notify contract already provides ordered `[from, to]` paths.

The bug severity is lower than the others because the exact failure mode is not yet strongly proven for `RenameMode::Both`, but the implementation should still be tightened.

## Files Likely In Scope

- [`crates/vicaya-watcher/src/lib.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-watcher/src/lib.rs)

## Proposed Solution

Split rename handling by mode:

- `RenameMode::Both`: trust the ordered pair directly.
- `RenameMode::Any` / `RenameMode::Other`: keep a conservative fallback path.
- If path cardinality is ambiguous, prefer a safer best-effort update path rather than a guessed move direction.

## Definition of Done

- `RenameMode::Both` no longer depends on filesystem existence to infer direction.
- Ambiguous rename modes remain handled without panics.
- Tests document the expected semantics for ordered and ambiguous rename events.

## Testing Strategy

### L1 / L2

- Expand watcher tests for:
  - ordered `RenameMode::Both`
  - reversed-path input proving the code does not silently "correct" a broken event for `Both`
  - ambiguous `Any` / `Other` path sets

### L3

- Run focused watcher crate tests only.

### Notes

- Before merge, sanity-check the assumptions against Task 002 so watcher-produced move events still align with daemon overwrite handling.
