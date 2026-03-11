# Shared Learnings

This document captures execution-time learnings that should be applied to every follow-up task in this issue-remediation batch.

## Branch Runtime Hygiene

- Always test with this branch's binaries, not whatever is already running on the machine.
- Before any TUI or daemon automation:
  - rebuild `target/release` for `vicaya`, `vicaya-daemon`, and `vicaya-tui`
  - stop resident `vicaya-daemon` and `vicaya-tui` processes
  - restart the daemon from this checkout's `target/release/vicaya`
- Verify the binary under test with `./target/release/vicaya --version` and related `--version` calls. Capture the reported git revision when debugging automation drift.
- When the final release is under test, make the automation select binaries from `~/.cargo/bin` (or another explicit binary root) instead of assuming `target/release`.

## iTerm2 Automation Guardrails

- Run iTerm2 automation serially. Parallel runs fight over window focus, tab/session IDs, and key delivery, which makes failures non-deterministic even when the product is healthy.
- `session.async_send_text(...)` is reliable for plain text, `Esc`, arrow keys, and most ordinary navigation.
- Control-chord delivery is not equally reliable. `Ctrl+O` is especially suspect in terminal automation because tty discard handling can intercept it before the TUI sees it.
- `stty discard undef` should be set before launching `vicaya-tui`, but that does not guarantee every control chord will be delivered consistently.
- Do not treat a control key as "verified" just because a script sent it. Only pass a step when the screen shows a unique state change.
- Concrete finding from Task `003`: after rebuilding the branch binaries, killing resident `vicaya*` processes, and verifying `rev f019227`, `Ctrl+O` sent via `async_send_text` still did not produce a visible preview pane in iTerm2 automation. Keep preview-specific assertions in `UNVERIFIED` state unless an actual preview marker appears.

## Visual Assertion Discipline

- Never use footer hints as proof that a feature is active.
  - Bad example: asserting preview opened because the footer contains `Ctrl+O: purvadarshana`
  - Good example: asserting preview opened only when the screen shows `purvadarshana — ...`, `loading preview…`, or the preview empty-state text
- Prefer markers that are unique to the target state and not repeated elsewhere on screen.
- If a state cannot be proven visually because the driver did not deliver the required key sequence, mark the automation step `UNVERIFIED` rather than `PASS`.
- On every failed or unverified transition:
  - capture a screenshot
  - dump the screen text
  - record the exact missing marker
- Concrete finding from scope automation:
  - the ksetra overlay hint is `Enter: set`, not `Enter: apply`
  - ksetra completion requires one `Tab` to open completions and a second `Tab` to apply the selected completion
  - Sthana directory rows render as `name/ (path)`, not `name (path)`
- Concrete finding from Task `007`:
  - the help overlay no longer contains `Enter / o`; use current markers actually rendered on screen, such as `Shift+Tab`, `Ctrl+P`, `Ctrl+K`, and `Ctrl+O`
  - do not use global search-result assertions while the header still shows `reconciling…`; wait for reconcile idle first or downgrade the assertion to `UNVERIFIED`

## Screenshot and Screen-Dump Strategy

- Take screenshots at every meaningful state transition, not just at the end.
- Pair screenshots with screen-text dumps for terminal UI debugging. Screenshots catch layout issues; dumps catch missing textual markers and false-positive assertions.
- Keep screenshots under `.claude/screenshots`. Do not commit them.

## Scope Path Hygiene

- Do not canonicalize user-facing scope paths if the indexed filesystem may preserve a different lexical alias for the same directory.
- Concrete finding from Task `008`: canonicalizing `/var/...` to `/private/var/...` caused explicit `filter_scope` matching to miss indexed paths recorded under `/var/...`.
- Prefer:
  - expand `~` and environment variables
  - make the path absolute relative to the launch cwd
  - normalize `.` / `..`
  - validate the directory exists
- Avoid rewriting the lexical root unless the indexed path space is rewritten the same way.

## Fixture and PII Discipline

- Repo-tracked automation must not hardcode personal document names, personal directories, or other sensitive local file names.
- Use runtime fixture discovery for user directories such as `~/Documents`, `~/Desktop`, and `~/Downloads`.
- Keep checked-in assertions generic:
  - use labels like `documents_fixture` or `downloads_fixture`
  - prefer generic query names such as `README.md`, `CLAUDE.md`, `query.rs`, `verification_report`, or `Screenshot`
- If a useful local fixture name is sensitive, it can still appear in local runtime output or screenshots, but it must not be baked into git-tracked source or docs.

## Existing Runtime Interference

- A user may already have `vicaya` binaries running outside the current branch.
- Those resident processes can:
  - hold the daemon socket
  - make the TUI talk to the wrong daemon
  - leave stale state that makes automation nondeterministic
- Treat process cleanup as part of test setup, not as optional cleanup.
- Even after branch-local daemon restart, startup reconcile can still be in progress for a noticeable window on large indexes. Visual tests that assert specific result counts or presence of global-search matches must wait for reconcile idle.

## Accessibility / OS Permissions

- `screencapture` requires Screen Recording permission.
- `System Events` keystroke injection requires Accessibility permission. If not granted, AppleScript-based key injection will fail with permission errors.
- Do not assume that permission for screenshots implies permission for synthetic keystrokes.

## Release Binary Freshness

- "Build only if binary is missing" is insufficient for TUI verification.
- Automation must rebuild branch binaries before runs so the visual pass exercises the code that was just changed.
- This matters especially for Task `003`, where unit tests may pass against current source while the TUI automation accidentally exercises an older release binary.

## Task Workflow Rule

- Before starting any later task in this issue batch:
  1. read this file
  2. rebuild the branch binaries
  3. clean resident `vicaya` processes
  4. verify versions
  5. only then run iTerm2 automation
- PR merge gate:
  1. wait for CI to finish
  2. wait for Codex review to settle as well
  3. only merge after Codex has either reduced to a `👍`/non-actionable settled state or after any real Codex comments have been fixed and resolved
  4. do not treat "green CI + no current threads" as sufficient if Codex is still actively reviewing
