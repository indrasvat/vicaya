# Smart Abbreviation Matching Implementation Plan

**Created:** 2025-11-27
**Status:** In Progress
**Goal:** Enable developers to find files using abbreviations (e.g., "vcs" → "vicaya-core/src/main.rs")

---

## 1. Overview

### 1.1 Problem Statement

Current vicaya uses trigram substring matching. Users must type enough consecutive characters to match:
- "Cargo.toml" requires typing "Car" or "tom" or "rgo"
- "vicaya-core/src/main.rs" requires typing "core/src" or "main"

Developers think in **abbreviations**:
- `vcs` → "**v**icaya-**c**ore/**s**rc"
- `vcm` → "**v**icaya-**c**ore/**m**ain.rs"
- `CT` → "**C**argo.**T**oml"

### 1.2 Goals

1. **Match abbreviations flexibly**: Support multiple matching strategies
2. **Score intelligently**: Better matches rank higher
3. **Maintain performance**: Don't slow down existing trigram search
4. **Work generically**: Apply to any file path, not just specific patterns
5. **Testable**: Comprehensive unit and integration tests

### 1.3 Non-Goals (for this iteration)

- Fuzzy matching with edit distance (typo tolerance)
- Learning from user behavior
- Content-based abbreviations

---

## 2. Matching Strategies

### 2.1 Strategy 1: Path Component First-Letter (Highest Priority)

Match first letters of path components:

```
Query: "vcs"
Path: "vicaya-core/src/main.rs"
Match: v[icaya-]c[ore/]s[rc/main.rs] ✓
```

**Algorithm**:
1. Split path into components: `["vicaya-core", "src", "main.rs"]`
2. For each component, extract "first letter" (handle extensions specially)
3. Try to match query characters sequentially to component first letters

**Special cases**:
- Extensions: "main.rs" → first letters are "m" and "r" (filename + extension)
- Hyphens/underscores: "vicaya-core" → "vc" or just "v"
- Numbers: Treat as separate tokens

### 2.2 Strategy 2: CamelCase Matching

Match uppercase letters in CamelCase or boundaries:

```
Query: "CT"
Path: "CargoToml" or "Cargo.Toml" or "cargo_toml"
Match: C[argo]T[oml] ✓
```

**Algorithm**:
1. Extract "capital" positions: uppercase letters, positions after `_`, `-`, `.`
2. Match query characters to these positions

### 2.3 Strategy 3: Sequential Character Matching (Fallback)

Match characters in sequence, allowing gaps:

```
Query: "main"
Path: "mod/admin/src/main.rs"
Match: [mod/ad]m[in/src/]ain[.rs] ✓
```

**Algorithm**:
1. Walk through path, trying to match each query character in order
2. Allow any number of characters between matches
3. Score based on gap sizes and positions

### 2.4 Strategy 4: Exact Prefix (Highest Score)

Exact match at component start:

```
Query: "main"
Path: "src/main.rs"
Match: [src/]main[.rs] ✓✓✓ (EXACT PREFIX)
```

---

## 3. Scoring Algorithm

### 3.1 Base Scores by Strategy

```rust
const SCORE_EXACT_PREFIX: f32 = 1.0;      // "main" matches "main.rs"
const SCORE_COMPONENT_FIRST: f32 = 0.95;  // "vcs" matches "v/c/s"
const SCORE_CAMELCASE: f32 = 0.90;        // "CT" matches "CargoToml"
const SCORE_SEQUENTIAL: f32 = 0.70;       // "main" matches "admin/main"
```

### 3.2 Modifiers

**Consecutive character bonus**:
```rust
bonus = 0.05 * consecutive_count / query.len()
```

**Early match bonus** (matches closer to filename):
```rust
bonus = 0.10 * (1.0 - match_start_pos / path.len())
```

**Case sensitivity bonus**:
```rust
bonus = 0.05 if query is uppercase && match is uppercase
```

**Gap penalty** (for sequential matching):
```rust
penalty = 0.01 * total_gap_size / path.len()
```

### 3.3 Final Score

```rust
final_score = base_score + bonuses - penalties
final_score = final_score.clamp(0.0, 1.0)
```

---

## 4. Architecture & Integration

### 4.1 New Module: `vicaya-index/src/abbreviation.rs`

```rust
pub struct AbbreviationMatcher {
    // Configuration for matching strategies
}

impl AbbreviationMatcher {
    pub fn new() -> Self;

    /// Try to match query as abbreviation against path
    /// Returns Some(score) if match, None otherwise
    pub fn match_path(&self, query: &str, path: &str) -> Option<AbbreviationMatch>;
}

pub struct AbbreviationMatch {
    pub score: f32,
    pub strategy: MatchStrategy,
    pub matched_indices: Vec<usize>, // For highlighting
}

pub enum MatchStrategy {
    ExactPrefix,
    ComponentFirst,
    CamelCase,
    Sequential,
}
```

### 4.2 Integration with Query Engine

Modify `vicaya-index/src/query.rs`:

```rust
pub fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
    // 1. Existing trigram search
    let mut results = self.trigram_search(query, limit * 2);

    // 2. Try abbreviation matching if trigram results are insufficient
    if results.len() < limit / 2 || query.len() <= 4 {
        let abbr_matcher = AbbreviationMatcher::new();

        // Scan file table for abbreviation matches
        for file_id in self.file_table.iter() {
            let path = self.get_path(file_id);
            if let Some(abbr_match) = abbr_matcher.match_path(query, path) {
                results.push(SearchResult {
                    file_id,
                    score: abbr_match.score,
                    match_type: MatchType::Abbreviation(abbr_match.strategy),
                });
            }
        }
    }

    // 3. Sort by score and return top results
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    results.truncate(limit);
    results
}
```

### 4.3 Performance Considerations

**Optimization 1: Early termination**
- If trigram search returns high-quality results (score > 0.95), skip abbreviation matching
- If query is very long (> 8 chars), prefer trigram matching

**Optimization 2: Selective scanning**
- Only scan file table if query is "abbreviation-like" (short, mixed case, etc.)
- Cache abbreviation patterns for common paths

**Optimization 3: Parallel processing**
- If scanning entire file table, use rayon for parallel matching

---

## 5. Implementation Phases

### Phase 1: Core Matching Logic ✓ (Target: 2 hours)

- [ ] Create `crates/vicaya-index/src/abbreviation.rs`
- [ ] Implement `AbbreviationMatcher` struct
- [ ] Implement Strategy 4: Exact Prefix
- [ ] Implement Strategy 1: Component First-Letter
- [ ] Implement Strategy 2: CamelCase
- [ ] Implement Strategy 3: Sequential
- [ ] Basic scoring algorithm

### Phase 2: Comprehensive Testing ✓ (Target: 1.5 hours)

- [ ] Unit tests for each strategy
- [ ] Edge cases: empty strings, special chars, Unicode
- [ ] Score comparison tests
- [ ] Test with real-world paths from vicaya codebase

### Phase 3: Integration ✓ (Target: 1 hour)

- [ ] Integrate with query engine
- [ ] Add `MatchType` enum to distinguish match types
- [ ] Update search result display to show match strategy (debug mode)

### Phase 4: End-to-End Verification ✓ (Target: 1 hour)

- [ ] Install updated binaries
- [ ] Test with vicaya codebase:
  - `vicaya search vcs` → should find "vicaya-core/src/"
  - `vicaya search vcm` → should find "vicaya-core/.../main.rs"
  - `vicaya search CT` → should find "Cargo.toml"
- [ ] Performance check: ensure searches complete in < 50ms

### Phase 5: Documentation & Polish ✓ (Target: 0.5 hours)

- [ ] Add examples to CLI help
- [ ] Document abbreviation matching in README
- [ ] Commit and push changes

---

## 6. Test Cases

### 6.1 Component First-Letter Matching

| Query | Path | Should Match | Expected Score |
|-------|------|--------------|----------------|
| `vcs` | `vicaya-core/src/main.rs` | ✓ | ~0.95 |
| `vcm` | `vicaya-core/src/main.rs` | ✓ | ~0.95 |
| `vsm` | `vicaya-scanner/src/main.rs` | ✓ | ~0.95 |
| `abc` | `admin/backup/config.toml` | ✓ | ~0.95 |
| `xyz` | `vicaya-core/src/main.rs` | ✗ | - |

### 6.2 CamelCase Matching

| Query | Path | Should Match | Expected Score |
|-------|------|--------------|----------------|
| `CT` | `Cargo.toml` | ✓ | ~0.90 |
| `MT` | `MyTest.rs` | ✓ | ~0.90 |
| `ct` | `Cargo.toml` | ✓ | ~0.85 (case mismatch) |

### 6.3 Exact Prefix Matching

| Query | Path | Should Match | Expected Score |
|-------|------|--------------|----------------|
| `main` | `src/main.rs` | ✓ | 1.0 |
| `cargo` | `Cargo.toml` | ✓ | 1.0 |
| `lib` | `src/lib.rs` | ✓ | 1.0 |

### 6.4 Edge Cases

| Query | Path | Should Match | Notes |
|-------|------|--------------|-------|
| `` | `any/path` | ✗ | Empty query |
| `a` | `admin/file.txt` | ✓ | Single char |
| `日本` | `日本語/test.txt` | ✓ | Unicode |
| `...` | `config.yaml` | ? | Special chars |
| `123` | `test123.txt` | ✓ | Numbers |

---

## 7. Success Criteria

### 7.1 Functional

- [ ] All test cases pass
- [ ] End-to-end searches work correctly
- [ ] No regressions in existing trigram search

### 7.2 Performance

- [ ] Abbreviation matching adds < 10ms latency for short queries
- [ ] Full file table scan completes in < 50ms for 10k files
- [ ] No degradation for long queries (still use trigrams)

### 7.3 User Experience

- [ ] Users can find files with 3-4 character abbreviations
- [ ] Most relevant results appear first
- [ ] No surprising mis-matches or false positives

---

## 8. Future Enhancements (Out of Scope)

1. **Fuzzy matching**: Tolerate typos (edit distance)
2. **Context awareness**: Boost recent/frequent files
3. **Multi-word queries**: "core main" → vicaya-core/main.rs
4. **Configurable strategies**: Let users tune matching behavior
5. **Match highlighting**: Show which characters matched in UI

---

## 9. Implementation Notes

### 9.1 Path Tokenization

```rust
fn tokenize_path(path: &str) -> Vec<Token> {
    // Split on: / \ . - _ space
    // Preserve: uppercase boundaries, digit boundaries
    // Example: "vicaya-core/src/main.rs"
    //   → ["vicaya", "core", "src", "main", "rs"]
}
```

### 9.2 First Letter Extraction

```rust
fn extract_first_letters(tokens: &[Token]) -> String {
    // "vicaya-core/src/main.rs" → "vcsm" or "vcsmr"
    // Handle extensions separately
}
```

### 9.3 Score Caching

For performance, consider caching:
- Tokenized paths (computed once during indexing)
- First-letter sequences (computed once during indexing)
- CamelCase positions (computed once during indexing)

These can be stored in the index alongside file metadata.

---

## 10. Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Performance degradation | High | Benchmark before/after; add early termination |
| False positives | Medium | Tune scoring thresholds; prefer exact matches |
| Complex code | Low | Comprehensive tests; clear documentation |
| Integration bugs | Medium | End-to-end testing; gradual rollout |

---

**Next Steps**: Begin Phase 1 - Core Matching Logic
