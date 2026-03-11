# Task 004: Normalize Default Exclusions for GH Issue #18

- **Phase:** Bug remediation
- **Status:** TODO
- **GitHub Issue:** `#18`
- **Recommended Worktree:** `codex/config-exclusions`
- **Depends on:** None
- **Blocks:** None
- **L4 Visual:** Optional smoke

## Problem

Default exclusion entries like `/System` and `/target` do not match the current component-based filter logic because matching ignores root separators and compares raw components.

## Files Likely In Scope

- [`crates/vicaya-core/src/config.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-core/src/config.rs)
- [`crates/vicaya-core/src/filter.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-core/src/filter.rs)
- [`config/default.toml`](/Users/indrasvat/code/github.com/indrasvat-vicaya/config/default.toml)

## Proposed Solution

Normalize exclusions before matching. The simplest safe version is to strip a leading `/` from exclusions and keep matching on path components. If needed, also normalize configuration loading so defaults and user-provided values follow the same rule.

## Definition of Done

- Default exclusions actually suppress the intended directories.
- User-provided exclusions with or without a leading `/` behave consistently.
- Existing wildcard behavior remains intact.
- Regression tests cover default config values explicitly.

## Testing Strategy

### L1 / L2

- Add tests for:
  - exact component exclusion with and without leading `/`
  - wildcard exclusions after normalization
  - default config exclusion list against representative paths

### L3

- Run scanner/filter-focused tests against representative paths from macOS system roots and common repo directories.

### L4

- Optional smoke:
  - `uv run .claude/automations/test_vicaya_tui_core_visual.py`

This issue is not visual, but a quick TUI smoke is useful because search result sets may change under default config.
