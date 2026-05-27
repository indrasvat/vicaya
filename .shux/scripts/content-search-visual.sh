#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/.shux/out/content-search"
STATE="$ROOT/.shux/out/content-search-state"
SESSION="vicaya-content-search"
VICAYA_BIN="$ROOT/target/release/vicaya"
VICAYA_TUI_BIN="$ROOT/target/release/vicaya-tui"
VICAYA_DAEMON_BIN="$ROOT/target/release/vicaya-daemon"

mkdir -p "$OUT" "$STATE"
rm -f "$OUT"/*.png

cargo build --release --workspace

cat > "$STATE/config.toml" <<EOF
index_roots = ["$ROOT"]
exclusions = [".git", "target", "node_modules", ".shux/out"]
respect_ignore_files = true
index_path = "$STATE/index"
max_memory_mb = 128

[performance]
scanner_threads = 2
reconcile_hour = 3

[smriti]
enabled = true
max_entries = 10000
max_boost = 0.08

[content_search]
enabled = true
engine = "auto"
allow_slow_fallback = false
EOF

cleanup() {
  env VICAYA_DIR="$STATE" VICAYA_DAEMON_BIN="$VICAYA_DAEMON_BIN" "$VICAYA_BIN" daemon stop >/dev/null 2>&1 || true
  shux session kill "$SESSION" >/dev/null 2>&1 || true
}
trap cleanup EXIT

cleanup
env VICAYA_DIR="$STATE" VICAYA_DAEMON_BIN="$VICAYA_DAEMON_BIN" "$VICAYA_BIN" daemon start >/dev/null

snapshot() {
  local name="$1"
  shux --format json pane snapshot -s "$SESSION" | jq -r .png_base64 | base64 -d > "$OUT/$name.png"
}

start_tui() {
  local title="$1"
  shift
  shux session kill "$SESSION" >/dev/null 2>&1 || true
  shux --format json session create "$SESSION" -d --title "$title" --cwd "$ROOT" -- \
    env VICAYA_DIR="$STATE" \
      VICAYA_DAEMON_BIN="$VICAYA_DAEMON_BIN" \
      VICAYA_NO_UPDATE_CHECK=1 \
      "$@" \
      "$VICAYA_TUI_BIN" "$ROOT" >/dev/null
  shux pane set-size -s "$SESSION" --cols 140 --rows 42
  shux pane wait-for -s "$SESSION" --text "vicaya" --timeout-ms 10000
}

select_content_drishti() {
  shux pane send-keys -s "$SESSION" --data FA==
  shux pane wait-for -s "$SESSION" --text "drishti" --timeout-ms 10000
  shux pane send-keys -s "$SESSION" --text "antar"
  snapshot "$1"
  shux pane send-keys -s "$SESSION" --data DQ==
  shux pane wait-for -s "$SESSION" --text "Antarvicaya" --timeout-ms 10000
}

start_tui "vicaya-content-search"
snapshot "01-patra-startup"
select_content_drishti "02-drishti-antar-filter"
shux pane send-keys -s "$SESSION" --text "fn main"
shux pane wait-for -s "$SESSION" --text "main.rs" --timeout-ms 10000
snapshot "03-content-results-ripgrep"

shux pane set-size -s "$SESSION" --cols 82 --rows 24
sleep 0.4
snapshot "04-content-narrow"

shux pane set-size -s "$SESSION" --cols 170 --rows 48
sleep 0.4
snapshot "05-content-wide"

start_tui "vicaya-content-git-grep" VICAYA_CONTENT_SEARCH_ENGINE=git-grep
select_content_drishti "06-git-grep-drishti"
shux pane send-keys -s "$SESSION" --text "content search"
shux pane wait-for -s "$SESSION" --text "README.md" --timeout-ms 10000
snapshot "07-content-results-git-grep"

start_tui "vicaya-content-disabled" VICAYA_NO_CONTENT_SEARCH=1
select_content_drishti "08-disabled-drishti"
shux pane send-keys -s "$SESSION" --text "needle"
sleep 1
snapshot "09-content-disabled-error"

env VICAYA_DIR="$STATE" VICAYA_DAEMON_BIN="$VICAYA_DAEMON_BIN" "$VICAYA_BIN" status --format json > "$OUT/daemon-status.json"

echo "Content search shux screenshots written to $OUT"
