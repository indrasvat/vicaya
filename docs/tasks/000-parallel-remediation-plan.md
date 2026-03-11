# Task 000: Parallel Remediation Plan for Open Issue Batch

- **Phase:** Bug remediation
- **Status:** TODO
- **Scope:** GH issues `#15` through `#21`
- **Primary Goal:** Maximize parallel throughput without creating avoidable merge churn
- **Shared Learnings:** Consult [`docs/LEARNINGS.md`](../LEARNINGS.md) before implementation or automation

## Recommendation

Use a hybrid plan: keep one task file per GitHub issue, but execute the fixes in **five worktrees**, not seven.

### Recommended worktree lanes

1. `codex/daemon-safety`
   Covers Task 001 / GH `#15`, Task 002 / GH `#16`, Task 006 / GH `#20`
   Reason: all three touch daemon/client IPC and will otherwise fight over the same files.

2. `codex/tui-utf8-and-visual`
   Covers Task 003 / GH `#17`
   Reason: isolated to `vicaya-tui`, plus this lane should own the iTerm2 visual automation harness.

3. `codex/config-exclusions`
   Covers Task 004 / GH `#18`
   Reason: small, isolated, low conflict risk.

4. `codex/watcher-rename-hardening`
   Covers Task 005 / GH `#19`
   Reason: separate crate, mostly independent, but semantics should be reviewed against Task 002 before merge.

5. `codex/index-short-query-ranking`
   Covers Task 007 / GH `#21`
   Reason: isolated to query/ranking code and ranking tests.

## Why not one worktree per issue?

- GH `#15`, `#16`, and `#20` all touch daemon-adjacent code paths.
- GH `#15` and `#16` both want [`crates/vicaya-daemon/src/ipc_server.rs`](/Users/indrasvat/code/github.com/indrasvat-vicaya/crates/vicaya-daemon/src/ipc_server.rs).
- GH `#20` may also want shared replay helpers already living in the same area.
- Splitting those three into separate branches is possible, but it buys little and increases rebase risk.

## Merge order

1. Task 003 / GH `#17`
   Reason: unlocks trustworthy UTF-8 TUI validation and strengthens the visual test suite early.

2. Task 001 / GH `#15`, Task 002 / GH `#16`, Task 006 / GH `#20`
   Reason: highest operational risk and best handled in one daemon-focused lane.

3. Task 004 / GH `#18` and Task 007 / GH `#21`
   Reason: isolated correctness fixes with low blast radius.

4. Task 005 / GH `#19`
   Reason: lower severity and partly a hardening pass rather than a fully proven defect.

## Shared validation policy

- Every branch must run targeted unit/integration coverage for its issue.
- Every branch that can affect the TUI user journey directly or indirectly should run:
  - `uv run .claude/automations/test_vicaya_tui_core_visual.py`
  - `uv run .claude/automations/test_vicaya_tui_scope_navigation.py`
- The TUI lane must also run:
  - `uv run .claude/automations/test_vicaya_tui_utf8_input.py`

## Exit criteria for the batch

- All seven task files are completed or explicitly re-scoped.
- The daemon lane lands without unresolved merge conflicts in IPC/index state code.
- The TUI automation suite produces screenshot evidence under `.claude/screenshots/`.
- Every merged fix has a regression test that fails before the fix and passes after it.
