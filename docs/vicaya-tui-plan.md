# vicaya-tui vNext Plan (Drishti / Ksetra UX)

**Created:** 2025-12-18  
**Status:** Plan + implementation tracking (Phase 1–2 shipped; Phase 3 next)  
**Goal:** Make `vicaya-tui` meaningfully more useful than a results list by adding preview, modes (“views”), scope navigation, richer actions, and uniquely vicaya features—while preserving vicaya’s “instant, index-first” feel.

> Note: This document proposes **UI/UX concepts and terminology** only. It intentionally does **not** rename or refactor code artifacts (modules/types/etc). Any future implementation should keep internal names stable and update only user-facing strings and behavior.

---

## 1. Why This Exists

Today, `vicaya-tui` is fast and minimal, but functionally close to “type → list → open/copy”. Tools like `television` (`tv`) show that preview + mode switching + customizable actions unlock much more value without losing speed.

vicaya’s advantage is different: **daemon-backed, always-hot, global index**. This plan leans into that with features that generic fuzzy finders typically can’t do well:

- Index-aware UX (freshness, reconciliation, exclusions, explain ranking).
- Fluid global ↔ project workflows via **scope stack**.
- A curated set of **views** tuned for filesystem search (not a generic channel framework).

---

## 2. Design North Star

Turn `vicaya-tui` into a **filesystem cockpit**:

- **Instant global filename search** stays the default superpower.
- Add **context** (preview + metadata + relatedness + history).
- Add **workflows** (scoping, batch actions, grep/symbol search in a project scope).
- Stay keyboard-first, low-latency, and transparent.

### 2.1 Non-Goals (For vNext)

- Becoming a full terminal file manager.
- Full-disk content indexing by default.
- Network/distributed search.
- Renaming internal code artifacts to match Sanskrit terms (UI-only for now).

---

## 3. Landscape Inspiration (What We Borrow / Avoid)

**Borrow**

- `television (tv)`: preview pane that’s always useful; mode switching; action/keybinding customization.
- `broot`: “verbs” (action mode), small query language (content searches), panels and preview.
- `yazi`: async I/O mentality + robust preview pipeline; persistent state and multi-instance niceties.
- `skim/fzf`: multi-select, preview toggles, shell-friendly output, predictable ergonomics.

**Avoid**

- A purely “run external command each keystroke” model for global search (vicaya should use the daemon).
- A wide-open “channels for everything” surface area that dilutes product identity.

---

## 4. Terminology & Copywriting (Sanskrit UI Lexicon)

The UI uses **romanized Sanskrit** (English alphabet). Docs may show Devanagari on first mention.

### 4.1 Glossary (Canonical)

| UI Term | Devanagari | Plain English | Meaning in Vicaya |
| --- | --- | --- | --- |
| `Drishti` | दृष्टि | Lens / View mode | A curated “mode” for searching and acting on a specific kind of information. |
| `Ksetra` | क्षेत्र | Scope / Field | The active search domain; can be stacked/pushed/popped. |
| `Prashna` | प्रश्न | Query | What the user types; drives filtering/search. |
| `Niyama` | नियम | Filter / Constraint | Structured chips (type/ext/mtime/size/path/etc) that narrow results. |
| `Phala` | फल | Results | The ranked list of matches. |
| `Purvadarshana` | पूर्वदर्शन | Preview | The right-side panel (or toggle) showing content/metadata/related info. |
| `Kriya` | क्रिया | Action | A single operation (open/copy/reveal/pin/etc). |
| `Kriya-Suchi` | क्रिया-सूची | Action palette | Searchable list of actions relevant to the current selection/Drishti. |
| `Sangraha` | संग्रह | Basket / Selection set | Multi-select collection for batch actions. |
| `Smriti` | स्मृति | History / Recents | Usage memory: recents + (optional) frecency ranking. |
| `Sambandha` | सम्बन्ध | Relatedness | “Related” files for a selected item (siblings, same stem, nearest README, etc). |
| `Hetu` | हेतु | Reason / Why | “Explain ranking” panel: why this result matched/scored. |
| `Varga` | वर्ग | Grouping | Group-by modes: by directory, extension, repo root, etc. |
| `Suchi` | सूची | Index | The daemon’s indexed corpus and health/status. |
| `Rakshaka` | रक्षक | Daemon / Guardian | The background service keeping the `Suchi` hot. |

### 4.2 Copy Rules (So It Feels Coherent)

- Headers show Sanskrit term + helpful English hint when first introduced:
  - `Drishti: Patra (Files)`
  - `Ksetra: ~/Projects/foo (Scope)`
- Help and tooltips always include English meaning at least once per session.
- Keep compound terms hyphenated in UI: `Kriya-Suchi`.
- Plurals: `Drishtis`, `Niyamas`, `Kriyas`.

---

## 5. Default Layout (Stable, Recognizable)

Three regions that stay consistent across `Drishtis`:

```text
┌ vicaya | Drishti: Patra | Ksetra: ~/Projects/foo (stack) | Rakshaka: OK | Suchi: 1,058,793 ┐
│ Prashna:  abbr tokens…   [Niyama: ext:rs  mtime:<7d  git:tracked]                         │
├───────────────────────────────┬───────────────────────────────────────────────────────────┤
│ Phala (list / Varga)          │ Purvadarshana (content / metadata / sambandha)            │
│  ▸ src/main.rs   …/src        │  main.rs (Rust) 12.4 KB  modified 2h ago                  │
│    src/lib.rs    …/src        │  1 use …                                                  │
│    README.md     …/           │  2 …                                                      │
│    …                           │  … (scroll; search-in-preview; highlight)                 │
├───────────────────────────────┴───────────────────────────────────────────────────────────┤
│ Tab focus | Ctrl+T Drishti | Ctrl+P Kriya-Suchi | Ctrl+O Purvadarshana | / Niyama | ? help │
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

### 5.1 Layout Principles

- `Purvadarshana` is **on by default** (toggleable), because it’s the highest usefulness-per-pixel.
- UI should degrade gracefully if `Rakshaka` is offline (clear messaging + limited local fallback).
- Stable layout reduces cognitive load while switching `Drishtis`.

---

## 6. Interaction Model (Core Workflows)

### 6.1 Focus & Navigation

- Default focus: `Prashna` input.
- `Tab`: cycle focus (`Prashna` → `Phala` → `Purvadarshana` when visible).
- `Shift+Tab`: cycle focus (reverse).
- `j/k` + arrows: move selection.
- `Enter`: primary action (contextual by `Drishti`).

### 6.2 `Drishti` Switching

- `Ctrl+T`: open a `Drishti` chooser overlay (searchable list).
- Switching `Drishti` should preserve:
  - `Ksetra` stack
  - `Prashna` (when reasonable)
  - `Niyamas` (when compatible), otherwise visibly dropped with explanation

### 6.3 `Ksetra` Scope Stack (Global ↔ Project Fluidity)

`Ksetra` is a stack, not a single value.

- `Pravesha` (enter): push scope
  - Enter a directory result
  - “Scope to git root”
  - “Scope to parent”
- `Nirgama` (exit): pop scope
- Header shows breadcrumbs (stack) to make scope obvious.

### 6.4 `Niyama` Filters (Chips)

Filters are visible, removable chips that also map to query syntax.

**Initial set (high ROI)**

- `type:file|dir`
- `ext:rs` (multi-value allowed)
- `path:src/` (contains / prefix)
- `mtime:<7d`, `mtime:>2025-01-01`
- `size:>10mb`
- `git:tracked|modified` (scoped Drishtis only)

### 6.5 `Varga` Grouping

Grouping improves scanability without losing ranking.

- Group by:
  - Directory (within `Ksetra`)
  - Extension
  - Repo root (in project scopes)
- Groups should be collapsible and keyboard-navigable.

### 6.6 `Kriya-Suchi` (Action Palette)

- `Ctrl+P`: searchable palette of `Kriyas`.
- Context-sensitive by:
  - Current `Drishti`
  - Selection type (file/dir/content match)
  - Multi-select (`Sangraha`) state
- Actions show keybinding if assigned and indicate side-effects (safe vs destructive).

### 6.7 `Sangraha` (Multi-Select)

Multi-select should be simple:

- Toggle selected items into `Sangraha`
- Batch actions: copy list, open all, pin group, export, etc.
- UI always shows count of selected items when active.

---

## 7. `Purvadarshana` (Preview) — Without Content Indexing

Preview can be useful immediately without indexing content.

### 7.1 Preview Modes (By File Type)

- Text/code: syntax highlight (optional), line numbers (toggle), search-within-preview, scroll.
- Directory: mini tree + counts (files/dirs) + recent children.
- Large files: bounded read + “load more” paging.
- Binary/non-UTF8: safe summary (kind/size/mtime) + limited strings/hex sample.

### 7.2 Preview Performance & Safety Rules

- Never block UI on I/O.
- Never attempt to execute files.
- Set strict limits (bytes/lines/time) with a clear “truncated” indicator.
- Handle permission failures gracefully (macOS TCC, Full Disk Access).

### 7.3 macOS-First Enhancements (Optional)

- `Kriya`: “Quick Look” via `qlmanage -p` (external viewer).
- `Kriya`: “Reveal in Finder”.

---

## 8. The `Drishti` System (Curated Views)

A `Drishti` is: **data source + ranking + row template + preview strategy + actions**.

### 8.1 Core `Drishtis` (MVP)

1. `Patra` (Files)
   - Source: `Rakshaka` index search
   - Default: global `Ksetra`
   - Row: name + parent + score + (optional) mtime/size badges
   - Preview: file content / directory summary
   - Actions: open, open-with, reveal, copy path/relative path, print

2. `Sthana` (Directories)
   - Source: index search filtered to directories
   - Primary action: `Pravesha` (push `Ksetra`)
   - Secondary: print path for shell `cd`, reveal/open

3. `Smriti` (Recent / Frecency-ready)
   - Source: local history of opens/selections
   - Helps “I was just here” navigation

4. `Navatama` (Recently Modified)
   - Source: index sorted by mtime (global or scoped)
   - Great for “what changed today”

5. `Brihat` (Large Files)
   - Source: index sorted by size (global or scoped)
   - Great for “why is disk full”

### 8.2 Power `Drishtis` (High Value)

6. `Antarvicaya` (Content, Grep)
   - Default `Ksetra`: current dir / git root (avoid full disk by default)
   - Source: streaming `ripgrep` results (`path:line:col` + snippet)
   - Preview: file centered on match with highlight
   - Actions: open at line/col, copy location, refine query into filename search, narrow scope

7. `Sanketa` (Symbols)
   - Default `Ksetra`: project scope
   - Source: on-demand symbol extraction + caching (ctags/tree-sitter)
   - Preview: definition context
   - Actions: open at location

8. `Itihasa` (Git)
   - Default `Ksetra`: project scope
   - Source: `git status`, `git diff --name-only`, recent commits
   - Preview: diff or file content
   - Actions: open diff, reveal, stage (optional; keep conservative)

### 8.3 Uniquely Vicaya `Drishtis` (Differentiators)

9. `Parivartana` (Changed Since…)
   - Source: daemon/watch timeline of add/remove/modify events since timestamp
   - Use cases: “what changed while I was away”, “what did the build generate”

10. `Sambandha` (Related)
   - Source: heuristics from selected item:
     - same stem (`foo.rs` ↔ `foo_test.rs`)
     - sibling files
     - nearest README/config/module root
     - same extension cluster
   - Makes vicaya feel like a “knowledge cockpit” instead of a picker

11. `Ankita` (Pinned / Bookmarks)
   - Source: user-curated sets (“workspaces”) stored locally
   - Actions: pin/unpin, tag, rename, export

---

## 9. Vicaya-Unique UX Features

### 9.1 `Hetu` Panel (Explain Ranking)

For a selected result, show why it ranked:

- Abbreviation vs substring vs trigram match
- Prefix match / exact match signals
- `Smriti` boost (if enabled)
- `Ksetra` match boost

This makes the system trustworthy and debuggable—especially when exclusions/scopes are involved.

### 9.2 `Smriti` Personalization (Optional)

Local-only and transparent:

- Boost frequently/recently opened files
- Display subtle indicator when `Smriti` affected ordering
- Opt-out with a clear toggle; no telemetry implied

### 9.3 `Suchi` + `Rakshaka` Health Surfacing

Header and help should clearly show:

- Online/offline daemon
- Reconciliation state/progress
- Last updated time
- “Rebuild index” action with confirmation

---

## 10. Configuration & Extensibility (Planned, Not Required for MVP)

Keep defaults excellent; allow power customization later:

- Theme (colors), layout (preview position/size), keybindings
- Enabled `Drishtis` + ordering
- Custom `Kriyas` with templates (`{path}`, `{line}`, `{col}`, `{ksetra}`)
- Preview rules (max bytes/lines, filetype overrides)

---

## 11. Backend / Protocol Considerations (Only Where It Pays Off)

Most UX can be implemented in the TUI without daemon changes, but a few daemon upgrades unlock big wins:

- Search request enhancements: optional `Ksetra` prefix, `type` filter (file/dir), sort mode.
- Optional “recent changes feed” for `Parivartana`.
- Keep `Purvadarshana` file reading in the TUI initially to avoid IPC bloat.

---

## 12. Implementation Roadmap (Phased)

### Current Implementation Status (As Of 2025-12-18)

Shipped (Phase 1–2, initial):

- [x] Non-blocking TUI loop with a background worker (daemon IPC + preview loading)
- [x] Componentized layout (header, `prashna`, `phala`, `purvadarshana`, footer, overlays)
- [x] `Ctrl+T` `Drishti` switcher overlay (navigation via arrows / `j/k`)
- [x] Split view: `phala` + `purvadarshana` (toggle via `Ctrl+O`)
- [x] Preview focus + full scrolling (`Tab`/`Shift+Tab` + `PgUp/PgDn`, `Ctrl+U/Ctrl+D`, `g/G`)
- [x] Preview safety: sanitize control chars, avoid wrap/bleed, truncate large/binary files
- [x] Syntax-highlighted previews (best-effort via `syntect`)
- [x] Header health indicators (`rakshaka` online/offline, `suchi` count, reconciling)
- [x] Compact build info in footer (`vX.Y.Z@sha`)

`Drishtis` shipped:

- [x] `Patra` (Files)
- [x] `Sthana` (Directories) — currently filter-only (next: `Pravesha` / scope push)

Follow-ups still pending (within Phase 1–2 scope):

- [ ] Make the `Drishti` switcher searchable (type-to-filter)
- [ ] Optional: line numbers + search-within-preview

Next up (Phase 3):

- [ ] Implement `Ksetra` stack with breadcrumbs + `Pravesha`/`Nirgama`
- [ ] Add `Niyama` chips (`type`, `ext`, `path`, minimal `mtime/size`)
- [ ] Add `Varga` grouping toggle (dir / ext) without losing ranking

### Phase 0 — Spec Lock (1–3 days)

- Finalize `Drishti/Ksetra/Niyama/Phala/Purvadarshana/Kriya-Suchi` terminology and UI copy rules.
- Decide default keymap and help text.
- Define MVP `Drishtis` and what “done” means.

### Phase 1 — TUI Architecture Upgrade (≈ 1 week)

- Non-blocking event loop and background tasks (search + preview loading).
- Componentized UI rendering (header, query, results, preview, footer, overlays).
- `Drishti` switching overlay and registry (opinionated built-ins).

### Phase 2 — `Purvadarshana` MVP (≈ 1 week)

- Preview pane with scroll + safe fallbacks.
- Metadata header for selection (size/mtime/path).
- Toggle preview and search-within-preview (optional in this phase).

### Phase 3 — `Ksetra` + `Niyama` + `Varga` (≈ 1 week)

- `Ksetra` stack push/pop + breadcrumbs.
- Filter chips + minimal query syntax.
- Grouping toggle(s) and UI affordances.

### Phase 4 — `Antarvicaya` (Grep) Drishti (1–2 weeks)

- `rg` streaming results in scoped mode by default.
- Preview anchored to match; open-at-line actions.
- Optional hybrid “index-assisted grep” to narrow candidate set.

### Phase 5 — Differentiators (Ongoing)

- `Hetu` ranking explanations.
- `Smriti` personalization.
- `Sambandha` related view.
- `Parivartana` changed-since view.
- `Ankita` bookmarks/workspaces.

---

## 13. Acceptance Criteria (Quality Bar)

- UI stays responsive under heavy indices (no visible stalls on selection changes).
- `Patra` remains “Everything-like”: results update as-you-type with minimal latency.
- Preview never blocks input; truncation and errors are explicit and non-fatal.
- Scope is always obvious (`Ksetra` breadcrumbs); filters are visible and removable (`Niyama` chips).
- Offline daemon mode is clearly communicated and doesn’t feel broken.

---

## 14. Open Questions / Risks

- How far to go on content features without content indexing (likely: `rg` lens + optional project index).
- How to respect exclusions consistently across daemon search and `rg` invocations.
- macOS permissions/TCC: ensure helpful messaging when preview or grep can’t read a path.
