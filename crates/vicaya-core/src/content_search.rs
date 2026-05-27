//! Content search powered by local grep-compatible tools.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

/// User-facing engine selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentSearchEngineChoice {
    /// Pick the fastest available configured engine.
    Auto,
    /// Require ripgrep (`rg`) and fail if it is unavailable.
    Ripgrep,
    /// Require `git grep` in a git worktree.
    GitGrep,
    /// Require standard `grep`; callers should opt in because this can be slow.
    Grep,
}

impl ContentSearchEngineChoice {
    /// Parse a config/CLI engine value.
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "ripgrep" | "rg" => Ok(Self::Ripgrep),
            "git-grep" | "git_grep" | "gitgrep" => Ok(Self::GitGrep),
            "grep" => Ok(Self::Grep),
            other => Err(Error::Config(format!(
                "unknown content search engine '{other}'"
            ))),
        }
    }
}

/// Concrete engine used for one search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContentSearchEngine {
    /// ripgrep executed with JSON output.
    Ripgrep,
    /// `git grep` executed inside a repository.
    GitGrep,
    /// Standard grep executed over Vicaya-controlled traversal.
    Grep,
}

impl ContentSearchEngine {
    /// Return the stable user-facing label for this engine.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ripgrep => "ripgrep",
            Self::GitGrep => "git-grep",
            Self::Grep => "grep",
        }
    }
}

/// One content match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentSearchHit {
    /// Absolute or scope-relative file path reported by the engine.
    pub path: PathBuf,
    /// One-based line number for the match.
    pub line_number: usize,
    /// One-based column number when the engine reports one.
    pub column: Option<usize>,
    /// Matched line text with trailing line endings removed.
    pub line: String,
}

/// Search result bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentSearchReport {
    /// Concrete engine that served this search.
    pub engine: ContentSearchEngine,
    /// Ordered content matches, capped by the requested limit.
    pub hits: Vec<ContentSearchHit>,
}

/// Content search options.
#[derive(Debug, Clone)]
pub struct ContentSearchOptions {
    /// Literal search text; empty queries return no hits.
    pub query: String,
    /// File or directory to search.
    pub scope: PathBuf,
    /// Maximum number of hits to return.
    pub limit: usize,
    /// Requested engine selection policy.
    pub engine: ContentSearchEngineChoice,
    /// Permit the explicit slow grep fallback in automatic mode.
    pub allow_slow_fallback: bool,
    /// Optional custom path to the `rg` binary.
    pub rg_path: Option<PathBuf>,
}

impl ContentSearchOptions {
    /// Build options with automatic engine selection and slow fallback disabled.
    pub fn new(query: impl Into<String>, scope: impl Into<PathBuf>, limit: usize) -> Self {
        Self {
            query: query.into(),
            scope: scope.into(),
            limit,
            engine: ContentSearchEngineChoice::Auto,
            allow_slow_fallback: false,
            rg_path: None,
        }
    }
}

/// Run content search with the best available configured engine.
pub fn search(options: &ContentSearchOptions) -> Result<ContentSearchReport> {
    let query = options.query.trim();
    if query.is_empty() || options.limit == 0 {
        let engine = resolve_engine(options)
            .map(|resolved| resolved.engine)
            .unwrap_or(ContentSearchEngine::Ripgrep);
        return Ok(ContentSearchReport {
            engine,
            hits: Vec::new(),
        });
    }

    let resolved = resolve_engine(options)?;
    let hits = match resolved.engine {
        ContentSearchEngine::Ripgrep => search_ripgrep(options, &resolved.command)?,
        ContentSearchEngine::GitGrep => search_git_grep(options, &resolved.command)?,
        ContentSearchEngine::Grep => search_grep(options, &resolved.command)?,
    };

    Ok(ContentSearchReport {
        engine: resolved.engine,
        hits,
    })
}

struct ResolvedEngine {
    engine: ContentSearchEngine,
    command: PathBuf,
}

fn resolve_engine(options: &ContentSearchOptions) -> Result<ResolvedEngine> {
    match options.engine {
        ContentSearchEngineChoice::Ripgrep => {
            let command = resolve_rg(options).ok_or_else(|| {
                Error::Other("ripgrep is required for content search but 'rg' was not found".into())
            })?;
            Ok(ResolvedEngine {
                engine: ContentSearchEngine::Ripgrep,
                command,
            })
        }
        ContentSearchEngineChoice::GitGrep => {
            let command = find_command("git").ok_or_else(|| {
                Error::Other("git-grep was requested but 'git' was not found".into())
            })?;
            ensure_git_scope(&options.scope, &command)?;
            Ok(ResolvedEngine {
                engine: ContentSearchEngine::GitGrep,
                command,
            })
        }
        ContentSearchEngineChoice::Grep => {
            let command = find_command("grep").ok_or_else(|| {
                Error::Other("grep was requested but 'grep' was not found".into())
            })?;
            Ok(ResolvedEngine {
                engine: ContentSearchEngine::Grep,
                command,
            })
        }
        ContentSearchEngineChoice::Auto => {
            if let Some(command) = resolve_rg(options) {
                return Ok(ResolvedEngine {
                    engine: ContentSearchEngine::Ripgrep,
                    command,
                });
            }

            if let Some(command) = find_command("git") {
                if ensure_git_scope(&options.scope, &command).is_ok() {
                    return Ok(ResolvedEngine {
                        engine: ContentSearchEngine::GitGrep,
                        command,
                    });
                }
            }

            if options.allow_slow_fallback {
                if let Some(command) = find_command("grep") {
                    return Ok(ResolvedEngine {
                        engine: ContentSearchEngine::Grep,
                        command,
                    });
                }
            }

            Err(Error::Other(
                "content search unavailable: install ripgrep, run inside a git worktree for git-grep fallback, or enable the explicit slow grep fallback".into(),
            ))
        }
    }
}

fn resolve_rg(options: &ContentSearchOptions) -> Option<PathBuf> {
    options
        .rg_path
        .as_ref()
        .filter(|path| is_executable(path))
        .cloned()
        .or_else(|| find_command("rg"))
}

fn find_command(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return is_executable(candidate).then(|| candidate.to_path_buf());
    }

    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|path| is_executable(path))
    })
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn search_ripgrep(options: &ContentSearchOptions, rg: &Path) -> Result<Vec<ContentSearchHit>> {
    let mut command = Command::new(rg);
    command
        .arg("--json")
        .arg("--fixed-strings")
        .arg("--smart-case")
        .arg("--color")
        .arg("never")
        .arg("--max-columns")
        .arg("1000")
        .arg("--max-filesize")
        .arg("2M")
        .arg("--")
        .arg(&options.query)
        .arg(&options.scope);

    collect_json_rg(command, options.limit)
}

fn search_git_grep(options: &ContentSearchOptions, git: &Path) -> Result<Vec<ContentSearchHit>> {
    let repo = git_repo_root(&options.scope, git)?;
    let pathspec = scope_pathspec(&repo, &options.scope);

    let mut command = Command::new(git);
    command
        .arg("-C")
        .arg(&repo)
        .arg("grep")
        .arg("--untracked")
        .arg("-n")
        .arg("--column")
        .arg("-I")
        .arg("-F")
        .arg("-e")
        .arg(&options.query)
        .arg("--");
    if let Some(pathspec) = pathspec {
        command.arg(pathspec);
    }

    collect_colon_matches(command, options.limit, Some(&repo), true)
}

fn search_grep(options: &ContentSearchOptions, grep: &Path) -> Result<Vec<ContentSearchHit>> {
    let mut hits = Vec::with_capacity(options.limit.min(128));
    let mut scanned = 0usize;
    grep_scope(
        grep,
        &options.query,
        &options.scope,
        options.limit,
        &mut scanned,
        &mut hits,
    )?;
    Ok(hits)
}

fn collect_json_rg(mut command: Command, limit: usize) -> Result<Vec<ContentSearchHit>> {
    let mut child = spawn_piped(&mut command)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Other("failed to capture ripgrep output".into()))?;
    let reader = BufReader::new(stdout);
    let mut hits = Vec::with_capacity(limit.min(128));

    for line in reader.lines() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|v| v.as_str()) != Some("match") {
            continue;
        }
        let Some(data) = value.get("data") else {
            continue;
        };
        let Some(path) = data
            .get("path")
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let line_number = data
            .get("line_number")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        if line_number == 0 {
            continue;
        }
        let text = data
            .get("lines")
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let column = data
            .get("submatches")
            .and_then(|v| v.as_array())
            .and_then(|items| items.first())
            .and_then(|item| item.get("start"))
            .and_then(|v| v.as_u64())
            .map(|start| start as usize + 1);

        hits.push(ContentSearchHit {
            path: PathBuf::from(path),
            line_number,
            column,
            line: clean_match_line(text),
        });

        if hits.len() >= limit {
            terminate_child(&mut child);
            return Ok(hits);
        }
    }

    finish_child(child)?;
    Ok(hits)
}

fn collect_colon_matches(
    mut command: Command,
    limit: usize,
    base: Option<&Path>,
    has_column: bool,
) -> Result<Vec<ContentSearchHit>> {
    let mut child = spawn_piped(&mut command)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Other("failed to capture grep output".into()))?;
    let reader = BufReader::new(stdout);
    let mut hits = Vec::with_capacity(limit.min(128));

    for line in reader.lines() {
        let line = line?;
        let Some(mut hit) = parse_colon_match(&line, has_column) else {
            continue;
        };
        if let Some(base) = base {
            if hit.path.is_relative() {
                hit.path = base.join(&hit.path);
            }
        }
        hits.push(hit);

        if hits.len() >= limit {
            terminate_child(&mut child);
            return Ok(hits);
        }
    }

    finish_child(child)?;
    Ok(hits)
}

fn grep_scope(
    grep: &Path,
    query: &str,
    path: &Path,
    limit: usize,
    scanned: &mut usize,
    hits: &mut Vec<ContentSearchHit>,
) -> Result<()> {
    const MAX_GREP_FILES: usize = 20_000;

    if hits.len() >= limit || *scanned >= MAX_GREP_FILES {
        return Ok(());
    }

    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return Ok(()),
    };

    if meta.is_dir() {
        if should_skip_grep_dir(path) {
            return Ok(());
        }
        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => return Ok(()),
        };
        for entry in entries.flatten() {
            grep_scope(grep, query, &entry.path(), limit, scanned, hits)?;
            if hits.len() >= limit || *scanned >= MAX_GREP_FILES {
                break;
            }
        }
        return Ok(());
    }

    if !meta.is_file() {
        return Ok(());
    }

    *scanned += 1;
    grep_file(grep, query, path, limit, hits)
}

fn should_skip_grep_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | "target"
            | "node_modules"
            | ".cargo"
            | ".rustup"
            | ".shux"
            | ".venv"
            | "venv"
            | "__pycache__"
    )
}

fn grep_file(
    grep: &Path,
    query: &str,
    path: &Path,
    limit: usize,
    hits: &mut Vec<ContentSearchHit>,
) -> Result<()> {
    let mut command = Command::new(grep);
    command
        .arg("-n")
        .arg("-F")
        .arg("-I")
        .arg("--")
        .arg(query)
        .arg(path);

    let mut child = spawn_piped(&mut command)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Other("failed to capture grep output".into()))?;
    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        let line = line?;
        let Some((line_number, text)) = parse_grep_file_line(&line) else {
            continue;
        };
        hits.push(ContentSearchHit {
            path: path.to_path_buf(),
            line_number,
            column: None,
            line: clean_match_line(text),
        });
        if hits.len() >= limit {
            terminate_child(&mut child);
            return Ok(());
        }
    }

    finish_child(child)
}

fn parse_grep_file_line(line: &str) -> Option<(usize, &str)> {
    let (line_number, text) = line.split_once(':')?;
    if line_number.is_empty() || !line_number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((line_number.parse().ok()?, text))
}

fn spawn_piped(command: &mut Command) -> Result<Child> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(Error::Io)
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn finish_child(mut child: Child) -> Result<()> {
    let status = child.wait()?;
    if status.success() || status.code() == Some(1) {
        Ok(())
    } else {
        Err(Error::Other(format!("content search exited with {status}")))
    }
}

fn parse_colon_match(line: &str, has_column: bool) -> Option<ContentSearchHit> {
    if has_column {
        return parse_colon_match_with_column(line);
    }

    let (path, line_number, rest) = split_numeric_field(line)?;
    Some(ContentSearchHit {
        path: PathBuf::from(path),
        line_number,
        column: None,
        line: clean_match_line(rest),
    })
}

fn parse_colon_match_with_column(line: &str) -> Option<ContentSearchHit> {
    for (idx, ch) in line.char_indices() {
        if ch != ':' {
            continue;
        }
        let path = &line[..idx];
        let rest = &line[idx + 1..];
        let Some((line_number, rest)) = split_leading_number(rest) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let Some((column, text)) = split_leading_number(rest) else {
            continue;
        };
        let Some(text) = text.strip_prefix(':') else {
            continue;
        };

        return Some(ContentSearchHit {
            path: PathBuf::from(path),
            line_number,
            column: Some(column),
            line: clean_match_line(text),
        });
    }

    None
}

fn split_leading_number(input: &str) -> Option<(usize, &str)> {
    let end = input
        .char_indices()
        .find_map(|(idx, ch)| (!ch.is_ascii_digit()).then_some(idx))
        .unwrap_or(input.len());
    if end == 0 {
        return None;
    }
    Some((input[..end].parse().ok()?, &input[end..]))
}

fn split_numeric_field(input: &str) -> Option<(&str, usize, &str)> {
    for (idx, ch) in input.char_indices() {
        if ch != ':' {
            continue;
        }
        let rest = &input[idx + 1..];
        let next_colon = rest.find(':')?;
        let number = &rest[..next_colon];
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let parsed = number.parse().ok()?;
        return Some((&input[..idx], parsed, &rest[next_colon + 1..]));
    }

    None
}

fn clean_match_line(line: &str) -> String {
    line.trim_end_matches(['\r', '\n']).to_string()
}

fn ensure_git_scope(scope: &Path, git: &Path) -> Result<()> {
    git_repo_root(scope, git).map(|_| ())
}

fn git_repo_root(scope: &Path, git: &Path) -> Result<PathBuf> {
    let cwd = if scope.is_file() {
        scope.parent().unwrap_or(scope)
    } else {
        scope
    };
    let output = Command::new(git)
        .arg("-C")
        .arg(cwd)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()?;
    if !output.status.success() {
        return Err(Error::Other("scope is not inside a git worktree".into()));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let root = text.trim();
    if root.is_empty() {
        Err(Error::Other("git worktree root was empty".into()))
    } else {
        Ok(PathBuf::from(root))
    }
}

fn scope_pathspec(repo: &Path, scope: &Path) -> Option<PathBuf> {
    let canonical_repo = std::fs::canonicalize(repo).ok()?;
    let canonical_scope = std::fs::canonicalize(scope).ok()?;
    if canonical_scope == canonical_repo {
        return None;
    }
    canonical_scope
        .strip_prefix(canonical_repo)
        .ok()
        .map(PathBuf::from)
}

#[cfg(test)]
fn command_exists_for_tests(name: &str) -> bool {
    find_command(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn normalize_test_path(path: &Path) -> PathBuf {
        let raw = path.to_string_lossy();
        raw.strip_prefix("/private/var/")
            .map(|suffix| PathBuf::from(format!("/var/{suffix}")))
            .unwrap_or_else(|| path.to_path_buf())
    }

    #[test]
    fn parses_engine_aliases() {
        assert_eq!(
            ContentSearchEngineChoice::parse("rg").unwrap(),
            ContentSearchEngineChoice::Ripgrep
        );
        assert_eq!(
            ContentSearchEngineChoice::parse("git_grep").unwrap(),
            ContentSearchEngineChoice::GitGrep
        );
        assert!(ContentSearchEngineChoice::parse("ack").is_err());
    }

    #[test]
    fn parses_git_grep_line_with_colons_in_path() {
        let hit = parse_colon_match("src/a:b.rs:12:8:let needle = true;", true).unwrap();
        assert_eq!(hit.path, PathBuf::from("src/a:b.rs"));
        assert_eq!(hit.line_number, 12);
        assert_eq!(hit.column, Some(8));
        assert_eq!(hit.line, "let needle = true;");
    }

    #[test]
    fn parses_git_grep_line_with_digit_colon_in_path() {
        let hit = parse_colon_match("src/foo:12:bar.rs:5:1:needle", true).unwrap();
        assert_eq!(hit.path, PathBuf::from("src/foo:12:bar.rs"));
        assert_eq!(hit.line_number, 5);
        assert_eq!(hit.column, Some(1));
        assert_eq!(hit.line, "needle");
    }

    #[test]
    fn parses_git_grep_line_before_numeric_colons_in_match_text() {
        let hit = parse_colon_match("src/main.rs:5:1:error code: 12:3: later", true).unwrap();
        assert_eq!(hit.path, PathBuf::from("src/main.rs"));
        assert_eq!(hit.line_number, 5);
        assert_eq!(hit.column, Some(1));
        assert_eq!(hit.line, "error code: 12:3: later");
    }

    #[test]
    fn parses_plain_grep_line() {
        let hit = parse_colon_match("/tmp/demo.rs:3:fn main() {}", false).unwrap();
        assert_eq!(hit.path, PathBuf::from("/tmp/demo.rs"));
        assert_eq!(hit.line_number, 3);
        assert_eq!(hit.column, None);
        assert_eq!(hit.line, "fn main() {}");
    }

    #[test]
    fn returns_empty_hits_for_empty_query() {
        let mut options = ContentSearchOptions::new("", ".", 10);
        options.engine = ContentSearchEngineChoice::Grep;
        let report = search(&options).unwrap();
        assert_eq!(report.engine, ContentSearchEngine::Grep);
        assert!(report.hits.is_empty());
    }

    #[test]
    fn grep_engine_finds_literal_matches_when_requested() {
        if !command_exists_for_tests("grep") {
            return;
        }

        let dir = tempdir().unwrap();
        let file = dir.path().join("note.txt");
        let mut f = std::fs::File::create(&file).unwrap();
        writeln!(f, "first").unwrap();
        writeln!(f, "needle here").unwrap();

        let mut options = ContentSearchOptions::new("needle", dir.path(), 10);
        options.engine = ContentSearchEngineChoice::Grep;
        let report = search(&options).unwrap();

        assert_eq!(report.engine, ContentSearchEngine::Grep);
        assert_eq!(report.hits.len(), 1);
        assert_eq!(
            normalize_test_path(&report.hits[0].path),
            normalize_test_path(&file)
        );
        assert_eq!(report.hits[0].line_number, 2);
    }

    #[test]
    fn grep_engine_skips_heavy_dirs_and_stops_at_limit() {
        if !command_exists_for_tests("grep") {
            return;
        }

        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/ignored.txt"), "needle in metadata").unwrap();
        let visible = dir.path().join("visible.txt");
        std::fs::write(&visible, "needle in visible file\nneedle again").unwrap();

        let mut options = ContentSearchOptions::new("needle", dir.path(), 1);
        options.engine = ContentSearchEngineChoice::Grep;
        let report = search(&options).unwrap();

        assert_eq!(report.engine, ContentSearchEngine::Grep);
        assert_eq!(report.hits.len(), 1);
        assert_eq!(report.hits[0].path, visible);
        assert_eq!(report.hits[0].line, "needle in visible file");
    }

    #[test]
    fn git_grep_engine_finds_untracked_matches_with_colon_path() {
        if !command_exists_for_tests("git") {
            return;
        }

        let dir = tempdir().unwrap();
        let init = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .arg("init")
            .output()
            .unwrap();
        if !init.status.success() {
            return;
        }

        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let file = src.join("foo:12:bar.rs");
        std::fs::write(&file, "fn main() {}\nlet needle = true;\n").unwrap();

        let mut options = ContentSearchOptions::new("needle", dir.path(), 10);
        options.engine = ContentSearchEngineChoice::GitGrep;
        let report = search(&options).unwrap();

        assert_eq!(report.engine, ContentSearchEngine::GitGrep);
        assert_eq!(report.hits.len(), 1);
        assert_eq!(
            normalize_test_path(&report.hits[0].path),
            normalize_test_path(&file)
        );
        assert_eq!(report.hits[0].line_number, 2);
        assert_eq!(report.hits[0].column, Some(5));
    }

    #[test]
    fn ripgrep_engine_finds_matches_when_available() {
        if !command_exists_for_tests("rg") {
            return;
        }

        let dir = tempdir().unwrap();
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "fn main() {\n    let needle = true;\n}\n").unwrap();

        let mut options = ContentSearchOptions::new("needle", dir.path(), 10);
        options.engine = ContentSearchEngineChoice::Ripgrep;
        let report = search(&options).unwrap();

        assert_eq!(report.engine, ContentSearchEngine::Ripgrep);
        assert_eq!(report.hits.len(), 1);
        assert_eq!(
            normalize_test_path(&report.hits[0].path),
            normalize_test_path(&file)
        );
        assert_eq!(report.hits[0].line_number, 2);
        assert_eq!(report.hits[0].column, Some(9));
    }
}
