# Task 006: Skip Stale Journal Replay After Fresh Build for GH Issue #20

- **Phase:** Bug remediation
- **Status:** TODO
- **GitHub Issue:** `#20`
- **Recommended Worktree:** `codex/daemon-safety`
- **Depends on:** None
- **Blocks:** None
- **L4 Visual:** Shared smoke only
- **Shared Learnings:** Consult [`docs/LEARNINGS.md`](/Users/indrasvat/code/github.com/indrasvat-vicaya/docs/LEARNINGS.md) before implementation or automation

## Problem

Startup currently replays the journal unconditionally after loading or building the index. If the index is freshly rebuilt because `index.bin` is missing, replaying an old journal can corrupt the fresh snapshot with stale history.

## Files Likely In Scope

- [`crates/vicaya-daemon/src/main.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-daemon/src/main.rs)
- [`crates/vicaya-daemon/src/ipc_server.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-daemon/src/ipc_server.rs)

## Proposed Solution

Align startup behavior with the safer full-rebuild path:

- if `had_index == true`, replay the journal as today;
- if `had_index == false`, either skip stale replay entirely or record a pre-scan journal offset and replay only entries written during the scan;
- clear stale journal contents once the fresh snapshot is authoritative.

## Definition of Done

- Fresh rebuilds no longer apply stale pre-scan journal history.
- Startup behavior is consistent with full rebuild semantics.
- Journal truncation/offset handling is explicit and tested.
- Existing restart behavior with a valid on-disk index still works.

## Testing Strategy

### L1 / L2

- Add tests for:
  - startup with existing index plus journal
  - startup without index plus stale journal
  - startup without index plus updates written during scan window

### L3

- Add a daemon startup integration scenario using temporary index/journal files to confirm final search state matches disk state.

### L4

- Run:
  - `uv run .claude/automations/test_vicaya_tui_core_visual.py`
  - `uv run .claude/automations/test_vicaya_tui_scope_navigation.py`

This issue is backend-only, but it affects the truth of the result set presented in the TUI.
