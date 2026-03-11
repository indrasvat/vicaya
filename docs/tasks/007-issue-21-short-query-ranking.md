# Task 007: Fix Short-Query Top-N Ranking for GH Issue #21

- **Phase:** Bug remediation
- **Status:** TODO
- **GitHub Issue:** `#21`
- **Recommended Worktree:** `codex/index-short-query-ranking`
- **Depends on:** None
- **Blocks:** None
- **L4 Visual:** Optional smoke
- **Shared Learnings:** Consult [`docs/LEARNINGS.md`](../LEARNINGS.md) before implementation or automation

## Problem

The short-query linear-search path stops once it has collected `limit` matches, then sorts only that partial set. For one- and two-character queries, the TUI and CLI therefore receive the first `N` matches found, not the best `N` matches overall.

## Files Likely In Scope

- [`crates/vicaya-index/src/query.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-index/src/query.rs)

## Proposed Solution

Remove the pre-sort `ranked.len() >= limit` early break from `linear_search`. Rank the complete candidate set and truncate only after sorting, or replace the vector with a bounded top-K heap if full scans prove too expensive.

Keep the zero-match fast exit (`MAX_EMPTY_SCAN`) unless measurement shows it also harms correctness.

## Definition of Done

- Short queries return the globally best-ranked results, not the first `N` encountered.
- The fix is covered by a regression test that fails under the old behavior.
- Query performance remains acceptable and is measured if implementation changes materially.

## Testing Strategy

### L1 / L2

- Add deterministic ranking tests with:
  - early poor substring matches
  - later stronger prefix matches
  - limits smaller than total candidate count

### L3

- Run focused index tests and, if the implementation changes significantly, capture a small benchmark comparison before/after.

### L4

- Optional smoke:
  - `uv run .claude/automations/test_vicaya_tui_core_visual.py`

This is mainly a ranking correctness task, but a quick visual check is useful because result ordering is user-visible in the TUI.
