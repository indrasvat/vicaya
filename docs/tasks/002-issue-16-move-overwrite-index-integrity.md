# Task 002: Preserve Index Integrity on Move-Overwrite for GH Issue #16

- **Phase:** Bug remediation
- **Status:** TODO
- **GitHub Issue:** `#16`
- **Recommended Worktree:** `codex/daemon-safety`
- **Depends on:** None
- **Blocks:** None
- **L4 Visual:** Shared smoke only
- **Shared Learnings:** Consult [`docs/LEARNINGS.md`](../LEARNINGS.md) before implementation or automation

## Problem

When a move/rename overwrites an existing destination path, the overwritten file's stale inode mapping can survive in daemon state. Later inode reuse can corrupt the index by causing an unrelated path update to mutate or remove the wrong entry.

## Files Likely In Scope

- [`crates/vicaya-daemon/src/ipc_server.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-daemon/src/ipc_server.rs)

## Proposed Solution

In `move_path`, detect whether the destination already resolves to a different file ID. If so, tombstone and fully clean that destination entry before remapping the moved file.

Do not rely on watcher ordering to implicitly clean the overwritten destination first.

## Definition of Done

- Move-overwrite does not leave stale `inode_to_id` entries behind.
- Reused inode scenarios no longer delete or mutate unrelated paths.
- Regression coverage proves correct behavior for overwrite plus later inode reuse.
- Existing move behavior without overwrite remains unchanged.

## Testing Strategy

### L1 / L2

- Add focused daemon-state tests for:
  - plain rename without overwrite
  - rename that overwrites an indexed destination
  - subsequent inode reuse at the old overwritten inode

### L3

- Add an integration-style scenario that simulates watcher updates in sequence and validates final path/inode maps.

### L4

- Run:
  - `uv run .claude/automations/test_vicaya_tui_core_visual.py`
  - `uv run .claude/automations/test_vicaya_tui_scope_navigation.py`

This is a backend integrity fix, but it affects what the TUI can surface from the index.
