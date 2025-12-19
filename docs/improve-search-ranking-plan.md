# Improve Search Ranking Plan

**Created:** 2025-12-19  
**Status:** In progress  
**Goal:** Make global filename search reliably surface “your” files (project/workspace) above dependency caches and generated artifacts, especially for ambiguous queries like `server.go`.

This plan focuses on ranking (ordering) rather than exclusion (removing files from the index). It should still be possible to find files inside caches (e.g., Go module cache) when that’s what the user wants.

---

## 0. Progress (as of 2025-12-19)

Completed (implemented + tested):
- **Contextual tie-break ranking (Phase 1)**: demote common cache/build/tool-state paths as a secondary key (after match score), then sort by `mtime`, `path_depth`, and deterministic `path`.
- **Measurement suite**: deterministic corpus + query suite + metrics dashboard + regression gates in `crates/vicaya-index/tests/`, plus an end-to-end CLI JSON smoke test in `crates/vicaya-cli/tests/`.
- **Diagnostics**: daemon build info is exposed via IPC `Status`; the TUI footer shows the daemon SHA only when it mismatches the TUI build.
- **Scope-aware boost (Phase 3, initial)**: IPC `Search` supports optional `scope`; CLI sends `cwd` by default; ranking boosts results under the scope (while preserving cache demotions).

Next up (planned):
- Make the demote/boost patterns configurable via `Config`.
- Add an `--explain-rank` mode to print per-result rank features.
- Consider diversification for high-tie cases and lightweight local personalization.

---

## 1. Problem Statement

Searching for a common filename (e.g., `server.go`) currently returns a wall of identical-looking matches from dependency caches (e.g., `~/go/pkg/mod/...`) while the “obvious” project file (e.g., `~/Projects/example-app/server.go`) can be missing from the top results.

This feels wrong in practice because:
- The user intent for a bare filename query is usually “my project file”, not “some dependency’s file with the same name”.
- Many ecosystems produce huge numbers of duplicate basenames (Go modules, `node_modules`, build output dirs), so naïve ranking produces noisy results.
- The current behavior is also unstable: ties are effectively broken by index insertion order, which is not a meaningful relevance signal.

---

## 2. Current Implementation (Deep Dive)

### 2.1 Data structures

Index snapshot is: `FileTable` + `StringArena` + `TrigramIndex`.

- `crates/vicaya-index/src/file_table.rs`
  - `FileMeta` stores:
    - `path_offset/path_len` (full path in `StringArena`)
    - `name_offset/name_len` (basename in `StringArena`)
    - `size`, `mtime`, `dev`, `ino`
- `crates/vicaya-index/src/trigram.rs`
  - `TrigramIndex` maps `Trigram -> Vec<FileId>` (posting list)
  - Posting lists preserve **insertion order** (`push(file_id)`).
- `crates/vicaya-scanner/src/lib.rs`
  - The trigram index is currently built **only over the basename**:
    - `trigram_index.add(file_id, &name)`

### 2.2 Candidate generation

`crates/vicaya-index/src/query.rs` (`QueryEngine::search`)

- Normalizes query to lowercase.
- If `query.len() < 3`, does a linear scan (`linear_search`).
- Otherwise:
  1. Extract trigrams from the *query*.
  2. Fetch candidates via `TrigramIndex::query(&trigrams)`.

`TrigramIndex::query`:
- Picks the **smallest posting list** among the query trigrams.
- Filters that list by checking membership in every other trigram’s posting list.
- Returns candidates in the order they appear in the smallest posting list.

Key consequence: candidate order is a byproduct of whichever trigram happened to be smallest and the file insertion order during scanning / updates.

### 2.3 Scoring

`QueryEngine::score_candidate`:
- Reads `name` and `path` from the `StringArena`.
- Two match modes:
  - Abbreviation matching (`AbbreviationMatcher::match_path(query, path)`)
  - Substring matching:
    - Only considered if `name.contains(query)` or `path.contains(query)`
    - Score computed by `calculate_score(name, _path, query)`

`calculate_score` currently uses **only the basename**:
- `name == query` → `1.0`
- `name.starts_with(query)` → `0.9..0.99`
- word-ish contains → `0.7`
- substring contains → `0.5`
- otherwise → `0.3`

So for `server.go`:
- Every `server.go` basename gets score `1.0`.
- The path is not used to distinguish “project file” vs “dependency cache file”.

### 2.4 Sorting & the tie problem

Results are sorted by score only:

```rust
results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
```

`slice::sort_by` is stable, so when scores are equal (common), the final order is effectively:
- “whatever order `TrigramIndex::query` returned candidates”, i.e.
- “whatever order the scanner/daemon happened to insert files”.

This is exactly why you can see a long run of `~/go/pkg/mod/.../server.go` at the top: they tie at `1.0` and win via insertion order.

---

## 3. What “Good” Ranking Should Do

### 3.1 Principles

1. **Match quality dominates**  
   A weak match in a “nice” directory shouldn’t outrank a strong match elsewhere.
2. **User intent defaults to “my code”**  
   With ambiguous/common basenames, prefer files that look like user-owned project code.
3. **Heuristics must be overridable**  
   Power users should be able to tune or disable the “demote caches” behavior.
4. **Stable & deterministic**  
   Ties must be broken by meaningful, deterministic signals (not insertion order).
5. **Fast**  
   Ranking must stay cheap enough for “search-as-you-type”.

### 3.2 Practical goals (for the `server.go` case)

For a query that’s an exact filename, and many results share that exact basename:
- Prefer files in “project-like” locations (repos/workspaces) over package caches.
- Prefer shallower paths over extremely deep paths (typical of caches/build dirs).
- Prefer recently modified files over ancient ones (as a proxy for “active”).

---

## 4. Proposed Ranking Strategy

### 4.1 Two scores: match score vs rank score

Today `SearchResult.score` is treated as “match score”.

Proposal:
- Keep the existing “string match” logic (or refine it later).
- Add a **second stage** that produces a **rank score / rank key** using path + metadata signals.

We can implement this without changing the IPC/UI surface immediately by:
- continuing to return `score` as the match score for display, but
- sorting by a richer key (tuple) that includes contextual signals.

Later, we can optionally expose both `match_score` and `rank_score` for `--explain` output.

### 4.2 Path & metadata signals (context features)

Compute a lightweight `ContextFeatures` per candidate:

| Signal | Rationale | Example impact |
|---|---|---|
| **Dependency/cache penalty** | Users rarely want cache hits first | `~/go/pkg/mod/...` gets demoted |
| **Build artifact penalty** | Generated output is noisy | `target/`, `dist/`, `build/` demoted |
| **Path depth penalty** | Very deep paths correlate with caches | `~/go/pkg/mod/.../cmd/go/server.go` demoted |
| **Recency boost (mtime)** | Recently changed files are likely active | edited `server.go` rises |
| **Hidden/tool state penalty** | Low-value results for most users | `.idea/`, `.cache/` slightly demoted |
| **Deterministic fallback** | Avoid jitter | tie-break by full path lexicographically |

Important: “cache” should be treated as a **ranking penalty**, not an exclusion, so results remain discoverable.

### 4.3 Classification: “cache/build/tool state” detection

Do not rely on absolute prefixes only; use path component patterns so it’s portable.

Start with a conservative default list (configurable), e.g.:

- Dependency caches:
  - Go: `go/pkg/mod`
  - Rust: `.cargo/registry`, `.cargo/git`, `.rustup`
  - Node: `node_modules`, `.pnpm-store`, `.yarn/cache`
  - Python: `.venv`, `venv`, `__pycache__`, `.mypy_cache`, `.pytest_cache`
  - macOS: `Library/Caches` (under home)
- Build output:
  - `target`, `dist`, `build`, `out`, `.next`, `.nuxt`, `.svelte-kit`
- Tool state:
  - `.git`, `.hg`, `.svn`, `.idea`, `.vscode`

Also add a small “entropy/version” penalty for path segments that look like:
- Semver/versioned dirs (`@v0.34.0`, `v1.2.3`, `1.75.0`)
- Hashy dirs (`.fingerprint/<hex>`, long hex segments)

These patterns are extremely common in dependency stores and build artifacts.

### 4.4 Proposed sort key (implementation-friendly)

Compute:
- `match_score` (existing)
- `context_score` (small additive adjustment from features)

Then sort by:
1. `match_score` descending (primary relevance)
2. `context_score` descending (project-ish over cache-ish)
3. `mtime` descending (recent first)
4. `path_depth` ascending (shallower first)
5. `path` ascending (deterministic final tie-break)

This solves the `server.go` tie case immediately: all exact matches tie on `match_score`, and the cache penalty + depth penalty pushes `~/go/pkg/mod/...` below `~/Projects/...`.

### 4.5 Optional: result diversification (“don’t show 10 copies from the same cache”)

When the query is an exact basename match and there are many ties, apply a soft cap:
- At most `N` results per “bucket” (e.g., per top-level directory under `HOME`, or per detected cache root).
- Fill remaining slots with the next-best buckets.

This keeps the top page useful even when a single cache directory contains thousands of identical basenames.

This should be enabled only in the “high-tie” case (exact basename / stem matches), to avoid surprising behavior for more specific queries.

---

## 5. Implementation Plan (Phased)

### Phase 1: Fix ties with path-aware ranking (minimal surface change)

- Add a `ContextFeatures` computation in `crates/vicaya-index/src/query.rs`.
  - Compute `path_depth` cheaply (count separators / components).
  - Detect cache/build/tool dirs via component scanning.
  - Compute `context_score` as a small additive value.
- Sort by the tuple described in §4.4.
- Add unit tests demonstrating:
  - `~/Projects/example-app/server.go` ranks above `~/go/pkg/mod/.../server.go` for query `server.go`.
  - Deterministic ordering for equal candidates.

### Phase 2: Make it configurable (no “magic” without an escape hatch)

- Extend `vicaya-core::Config` with a `ranking` section:
  - `demote_paths: [String]` (component patterns, supports `*` like exclusions)
  - `boost_paths: [String]`
  - weights for depth/mtime/cache penalties
- Add `vicaya init` defaults that include common dependency caches.

### Phase 3: Add context inputs (scope-aware ranking)

- Extend IPC `Request::Search` to accept optional `scope` (cwd/ksetra).
- Apply a scope proximity boost:
  - files inside scope are boosted
  - shallower relative-to-scope paths are boosted further
- This makes CLI searches from inside a repo feel “IDE-like” without forcing the user to type `path:`.

### Phase 4 (optional): Learn from user behavior

- Track “last opened” / “frequently opened” paths (local-only).
- Add a small usage-based boost.
- Expose a “privacy/off” toggle in config.

---

## 6. Validation & Success Criteria

### Functional
- Exact common filenames (`main.rs`, `server.go`, `index.ts`) show project files above caches.
- Ranking is stable across runs given the same index state.
- Users can still find cache files with:
  - `path:` filter (today), and/or
  - configuring boosts, and/or
  - more specific queries.

### Performance
- No noticeable regression in interactive search latency.
- Context scoring adds only cheap string/path operations per candidate.

### Developer ergonomics
- Add an opt-in debug mode (e.g., `vicaya search --explain-rank`) that prints a per-result breakdown:
  - match score
  - applied penalties/boosts
  - final sort key

---

## 7. Measuring Improvement (Methodical Strategy)

Ranking work is notoriously easy to “feel better” while silently regressing other common searches. The approach below treats ranking as an experiment with:
- a fixed corpus,
- a fixed query suite,
- explicit relevance expectations,
- and metrics gates that must not regress.

### 7.1 Build a deterministic test corpus

Create a small, synthetic filesystem tree used only for ranking evaluation. It must include:

- **User-like content**
  - `~/Documents/`-style: `invoice_2024.pdf`, `taxes_2023.xlsx`, `meeting-notes.txt`, `resume.pdf`
  - `~/Downloads/`-style: `Screenshot_2025-12-18.png`, `IMG_0001.JPG`, `report-final(1).pdf`
  - “Projects”: a few repo-shaped trees with common filenames (`server.go`, `main.rs`, `index.ts`, `README.md`, `Dockerfile`)
- **Noise sources that should not dominate**
  - Dependency caches: `go/pkg/mod/...`, `node_modules/...`, `.cargo/registry/...`
  - Build output: `target/...`, `dist/...`, `build/...`
  - Tool state: `.git/...`, `.idea/...`, `.vscode/...`

Make it deterministic:
- Create files/directories in a temporary directory during tests.
- Set mtimes explicitly (so recency boosts are testable and stable).

### 7.2 Add two layers of tests

#### Layer A: `vicaya-index` ranking regression tests (fast, deterministic)

Add tests that:
- build an in-memory snapshot (or scan the synthetic corpus),
- run queries,
- and assert ordering invariants.

Examples (invariants, not brittle full lists):
- For query `server.go`, `.../Projects/example-app/server.go` must rank above `.../go/pkg/mod/.../server.go`.
- For query `invoice`, a document in `Documents/` must rank above a log file in `Library/Caches/...` that happens to match.
- For query `node_modules`, results inside `node_modules/` should still appear (penalty must not “hide” them; it only reorders when there are alternatives).

#### Layer B: End-to-end CLI smoke tests (exercise the tool)

Add an integration test that uses `VICAYA_DIR` (already supported) to isolate state:

1. Create a temp `VICAYA_DIR`.
2. Write a `config.toml` pointing `index_roots` at the synthetic corpus and `index_path` inside `VICAYA_DIR`.
3. Run:
   - `vicaya rebuild`
   - `vicaya search "<query>" --format=json --limit=20`
4. Parse JSON and assert the same invariants as Layer A.

This catches:
- scanner/index construction issues,
- daemon IPC issues (if `search` uses the daemon),
- and formatting/output regressions.

### 7.3 Define a query suite that reflects “general-purpose search”

Create a table of ~30–60 queries grouped by intent, not by programming language:

- **Exact filename**: `server.go`, `README.md`, `Dockerfile`, `Makefile`, `notes.txt`
- **Stem/prefix**: `invoice`, `resume`, `meeting`, `Screenshot`, `IMG_000`
- **Extensions**: `.pdf`, `.png`, `.toml`
- **Path-ish queries**: `src/main`, `Documents/taxes`, `go/pkg/mod`
- **Short queries** (linear scan path): `go`, `cv`, `db`
- **Mixed-case / abbreviations**: `CT` (Cargo.toml), `vcs`-like cases
- **Numbers/dates**: `2025-12-18`, `2024`, `v0.34.0`
- **Directories**: `example-app` (ensure directory search behaves well too)

Each query should have a small set of “relevant” expected results for the synthetic corpus.

### 7.4 Metrics and gates (before/after)

Add a small evaluation harness that computes metrics for both:
- **baseline ranker** (today’s behavior: sort by match score only),
- **candidate ranker** (new behavior).

Metrics to track:
- **MRR@10** (mean reciprocal rank): how quickly the first “good” result appears.
- **Top-1 / Top-3 hit rate**: % queries where a relevant result appears in top 1 / 3.
- **Noise@10**: for “ambiguous filename” queries, % of top 10 results classified as cache/build/tool-state.
- **Diversity@10**: number of distinct “buckets” in top 10 (prevents 10 results from a single cache root).
- **Stability**: ordering is identical across repeated runs for same corpus.
- **Latency budget** (microbench): ranking overhead per candidate stays under a small threshold.

Gates:
- Candidate ranker must not regress MRR@10 or Top-3 hit rate versus baseline on the suite.
- Candidate ranker must reduce Noise@10 on ambiguous queries.

### 7.5 Workflow: change → measure → accept/reject

For every ranking change:
1. Run Layer A tests (cheap, immediate feedback).
2. Run Layer B smoke tests (end-to-end sanity).
3. Run the evaluation harness and compare metrics vs baseline.
4. Only then keep the change; otherwise revise weights/heuristics.

### 7.6 Extend the suite with real-world regressions

When a user reports “ranking feels off”:
- add a minimal reproduction to the synthetic corpus,
- add the query + expected relevant result(s),
- and lock it in as a regression test.

---

## 8. Research Landscape (Dec 2025) & “Stronger Than Heuristics” Options

Desktop/file search ranking has two relevant bodies of work:

1. **Classic desktop/personal search research** (mid-2000s → early 2010s): emphasized metadata, activity signals, and heterogeneous “personal corpus” types.  
2. **Modern IR ranking research** (2015 → 2025): multi-stage retrieval + learning-to-rank + neural retrieval/reranking; heavy emphasis on debiasing, personalization, and efficiency.

vicaya can borrow ideas from both while staying local-first and fast.

### 8.1 Key papers worth reading (starting points)

Desktop / personal search:
- Cohen, Domshlak, Zwerdling — “On ranking techniques for desktop search” (2007/2008): https://www.semanticscholar.org/paper/0a8e1114ff0833eb4e21af39e33dce2074ec76d3  
- Kim, Croft — “Ranking using multiple document types in desktop search” (2010): https://www.semanticscholar.org/paper/07f0df7ec782b3a8e01c0a659c725bd189fe1c28  
- Chirita et al. — “Activity Based Metadata for Semantic Desktop Search” (2005): https://www.semanticscholar.org/paper/cded21a95f221ab936f22784a474945705a077af  
- Chen et al. — “iMecho: an associative memory based desktop search system” (2009): https://www.semanticscholar.org/paper/c34b8c1ddcc7fda74d99fb29ada1e599abeaa363

Personal search learning-to-rank / bias correction (highly relevant if vicaya learns from user opens):
- Wang et al. — “Learning to Rank with Selection Bias in Personal Search” (2016): https://www.semanticscholar.org/paper/b3ca9c2e302073264d2a3c2c2cee4d40b6fe908d  
- Qin et al. — “Matching Cross Network for Learning to Rank in Personal Search” (2020): https://www.semanticscholar.org/paper/6c3eda8ba387b03d1c9561ebe48affaa05e05000

Neural retrieval/reranking (useful as an *optional* reranker for ambiguous ties):
- Khattab, Zaharia — “ColBERT: Efficient and Effective Passage Search via Contextualized Late Interaction over BERT” (2020): https://arxiv.org/abs/2004.12832  
- Formal et al. — “SPLADE v2: Sparse Lexical and Expansion Model for Information Retrieval” (2021): https://arxiv.org/abs/2109.10086  
- Zhuang et al. — “RankT5: Fine-Tuning T5 for Text Ranking with Ranking Losses” (2022): https://arxiv.org/abs/2210.10634

### 8.2 What “state of the art” looks like (and what maps to vicaya)

In production search systems (web and large enterprise), the dominant pattern is:

- **Stage 1: Fast candidate generation** (lexical inverted index; sometimes plus dense ANN)  
- **Stage 2: Feature-rich learning-to-rank** (GBDT like LambdaMART, or deep ranking model)  
- **Stage 3: Neural reranking** (cross-encoder / late-interaction) on a small top-K  
- **Personalization** from user behavior, with explicit handling of **selection/position bias**  
- **Multi-objective ranking**: relevance + freshness + diversity + policy constraints

vicaya already has an excellent Stage 1 for substring/prefix use-cases (trigram + abbreviation matching). The biggest gaps are:
- no feature-rich tie-breaking, and
- no way to learn from user actions.

### 8.3 Advanced options for vicaya (beyond hand-tuned heuristics)

These are listed in increasing sophistication and resource cost. All of them should still be evaluated against the same query suite + metrics gates in §7.

#### Option A: Unsupervised “path rarity” priors (cheap, surprisingly powerful)

Instead of hard-coded cache lists, compute an IDF-like rarity score for path components:
- Build `df(token)` over path tokens (directory names, repo names, etc.).
- Define a file’s `rarity_score` as the sum/mean of `idf(token)` over its path.

Intuition:
- cache paths contain very common tokens (`pkg`, `mod`, `registry`, `target`, version-like segments),
- project paths contain at least one rare token (the project name).

Use this only as a **tie-breaker** (never overrule a much better match score).

#### Option B: Feature-based Learning-to-Rank (GBDT) trained offline (strong, still fast)

Treat ranking as a supervised model over inexpensive features:
- match features (exact/prefix/abbr/substring; match position)
- path features (depth, component patterns, rarity_score, “under home”)
- metadata (mtime/size buckets)
- diversity features (bucket ID; directory file-count)

Train a LambdaMART/XGBoost-style ranker on:
- synthetic corpus labels + curated query suite,
- plus optional user-labeled “prefer A vs B” judgments from a small local tool.

Advantages:
- avoids endless hand-tuning of weights,
- can generalize to “general-purpose” queries better than ad-hoc rules.

#### Option C: Local personalization from implicit feedback (best UX over time)

Log (locally) the file the user selects for a query; use it as implicit relevance feedback:
- simplest: per-path or per-directory “recently/frequently opened” boosts,
- stronger: counterfactual/propensity-weighted learning-to-rank to correct position bias (see Wang et al. 2016).

This is the most likely way to beat Spotlight/Raycast long-term because it adapts to *your* machine.

#### Option D: Neural reranking for the top-K (expensive but very effective in many IR settings)

Run a small transformer ranker only on a short list (e.g., top 200 lexical candidates):
- Input: query + path (and optionally a content snippet if content indexing is ever added).
- Output: rerank score.

This can learn “developer cache patterns” without explicitly encoding them, and can also help with non-code searches if content signals exist.

Practical constraints:
- needs on-device model distribution + quantization,
- must be carefully latency-bounded (only run when the query is ambiguous / high-tie).

### 8.4 Recommendation for vicaya (practical but ambitious)

1. Ship Phase 1–3 from §5 and §7 first (feature-rich tie-breaking + robust measurement).  
2. Add **Option A** (path rarity priors) as a low-risk, high-return tie-breaker.  
3. If you want “Spotlight but better”, add **Option C** (local personalization) with a simple, transparent boost first, then graduate to LTR if needed.  
4. Keep **Option D** as a future “power mode” once evaluation infrastructure is solid; don’t lead with it.
