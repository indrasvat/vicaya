# Task 003: Make TUI Input UTF-8 Safe for GH Issue #17

- **Phase:** Bug remediation
- **Status:** TODO
- **GitHub Issue:** `#17`
- **Recommended Worktree:** `codex/tui-utf8-and-visual`
- **Depends on:** None
- **Blocks:** None
- **L4 Visual:** Required
- **Shared Learnings:** Consult [`docs/LEARNINGS.md`](../LEARNINGS.md) before implementation or automation

## Problem

The TUI currently treats cursor movement and deletion in some input states as byte-wise operations instead of character-boundary operations. Multi-byte UTF-8 input can therefore panic the TUI or corrupt cursor state.

## Files Likely In Scope

- [`crates/vicaya-tui/src/state/mod.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-tui/src/state/mod.rs)
- [`crates/vicaya-tui/src/app.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-tui/src/app.rs)
- `.claude/automations/test_vicaya_tui_utf8_input.py`

## Proposed Solution

Replace byte-step cursor math in `SearchState` and `PreviewState` with UTF-8-safe movement and deletion. Reuse the correct model already present in `KsetraInputState`: move only on `is_char_boundary()` and advance inserts by `c.len_utf8()`.

## Definition of Done

- Main search input accepts multi-byte characters without panic.
- Preview search overlay accepts multi-byte characters without panic.
- Left/right/backspace/delete preserve valid UTF-8 boundaries.
- Existing ASCII behavior remains unchanged.
- Visual automation captures the relevant keyboard flows with screenshots.

## Testing Strategy

### L1 / L2

- Add unit tests for:
  - inserting UTF-8 into empty and non-empty search input
  - moving left/right across multi-byte characters
  - deleting immediately before and after a multi-byte character
  - preview-search state using the same cases

### L3

- Run focused `vicaya-tui` tests covering both state structs and event handlers.

### L4

- Run all three automation suites:
  - `uv run .claude/automations/test_vicaya_tui_core_visual.py`
  - `uv run .claude/automations/test_vicaya_tui_scope_navigation.py`
  - `uv run .claude/automations/test_vicaya_tui_utf8_input.py`

- Review the screenshots under `.claude/screenshots/` and confirm:
  - UTF-8 glyphs render in the search line
  - preview-search overlay remains intact
  - keyboard navigation still works after UTF-8 interactions
