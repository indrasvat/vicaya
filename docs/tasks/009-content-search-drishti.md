# 009 - Content Search Drishti

## Goal

Add `Antarvicaya` content search for daily repo use without slowing filename search.

## Plan

1. Add a shared content-search engine in `vicaya-core`.
   - Prefer `rg`.
   - Fall back to `git grep` automatically inside a git worktree.
   - Use plain recursive `grep` only when explicitly requested or enabled.
2. Add `vicaya grep <query>`.
   - Support `--scope`, `--limit`, `--format`, `--engine`, and `--allow-slow-fallback`.
   - Emit JSON for scripts and compact table/plain output for terminals.
3. Enable `Antarvicaya` in the TUI.
   - Reuse the existing Drishti switcher, results list, preview pane, and scope stack.
   - Show `file:line:column` rows and jump preview near the selected match.
   - Surface a clear unavailable message when no safe engine exists.
4. Verify.
   - Unit and integration tests for engine parsing, fallback policy, CLI parsing, TUI preview anchors, and worker content results.
   - `make check`.
   - `hyperfine` against scoped repo searches.
   - shux visual automation with desktop, narrow, wide, disabled-engine, and fallback screenshots.
5. Ship.
   - Update README, architecture, TUI plan, benchmark notes, and default config.
   - Open a PR with screenshot comments and validation output.
   - Run local dootsabha review, address relevant comments, then monitor CI/review.

## Definition of Done

- `vicaya grep` returns scoped content matches and does not require the daemon.
- `Antarvicaya` is selectable from `Ctrl+T` and works with normal TUI navigation.
- Missing `rg` degrades to `git grep` inside repos; slow `grep` fallback is explicit.
- Filename search path remains unchanged and fast.
- No TUI layout regressions in shux screenshots across tested sizes.
- Relevant docs and config examples describe the feature and fallback policy.
