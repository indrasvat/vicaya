#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="$ROOT/.shux/out/smriti"
SESSION="vicaya-smriti-${USER:-user}"
VICAYA_DIR="${VICAYA_DIR:-$OUT/state}"

mkdir -p "$OUT" "$VICAYA_DIR"
cat > "$VICAYA_DIR/config.toml" <<EOF
index_roots = ["$ROOT"]
exclusions = ["target", ".git"]
respect_ignore_files = true
index_path = "$VICAYA_DIR/index"
max_memory_mb = 512

[performance]
scanner_threads = 0
reconcile_hour = 3

[smriti]
enabled = true
max_entries = 10000
max_boost = 0.08
EOF

cd "$ROOT"
cargo build --release --workspace

if pgrep -f "$ROOT/target/release/vicaya-daemon" >/dev/null 2>&1; then
  "$ROOT/target/release/vicaya" daemon stop >/dev/null 2>&1 || true
fi

export VICAYA_DIR
export VICAYA_DAEMON_BIN="$ROOT/target/release/vicaya-daemon"
export VICAYA_NO_UPDATE_CHECK=1

"$ROOT/target/release/vicaya" rebuild
"$ROOT/target/release/vicaya" daemon start
trap '"$ROOT/target/release/vicaya" daemon stop >/dev/null 2>&1 || true; shux session kill "$SESSION" >/dev/null 2>&1 || true' EXIT

"$ROOT/target/release/vicaya" smriti clear --yes >/dev/null

shux --format json session create "$SESSION" -d --title vicaya-smriti --cwd "$ROOT" -- env VICAYA_DIR="$VICAYA_DIR" VICAYA_DAEMON_BIN="$VICAYA_DAEMON_BIN" VICAYA_NO_UPDATE_CHECK=1 "$ROOT/target/release/vicaya-tui" "$ROOT" >/dev/null
shux pane set-size -s "$SESSION" --cols 140 --rows 42
shux pane wait-for -s "$SESSION" --text "vicaya" --timeout-ms 10000
shux --format json pane snapshot -s "$SESSION" | jq -r .png_base64 | base64 -d > "$OUT/01-patra-baseline.png"

shux pane send-keys -s "$SESSION" --text "Cargo"
shux pane wait-for -s "$SESSION" --text "Cargo.toml" --timeout-ms 10000
shux --format json pane snapshot -s "$SESSION" | jq -r .png_base64 | base64 -d > "$OUT/02-patra-query.png"

shux pane send-keys -s "$SESSION" --data G1tC
shux pane send-keys -s "$SESSION" --data DQ==
sleep 1
shux session kill "$SESSION" >/dev/null 2>&1 || true

shux --format json session create "$SESSION" -d --title vicaya-smriti --cwd "$ROOT" -- env VICAYA_DIR="$VICAYA_DIR" VICAYA_DAEMON_BIN="$VICAYA_DAEMON_BIN" VICAYA_NO_UPDATE_CHECK=1 "$ROOT/target/release/vicaya-tui" "$ROOT" >/dev/null
shux pane set-size -s "$SESSION" --cols 140 --rows 42
shux pane wait-for -s "$SESSION" --text "vicaya" --timeout-ms 10000
shux pane send-keys -s "$SESSION" --data FA==
shux pane wait-for -s "$SESSION" --text "Smriti" --timeout-ms 10000
shux pane send-keys -s "$SESSION" --text "smi"
shux pane send-keys -s "$SESSION" --data DQ==
shux pane wait-for -s "$SESSION" --text "Smriti" --timeout-ms 10000
sleep 1
shux --format json pane snapshot -s "$SESSION" | jq -r .png_base64 | base64 -d > "$OUT/03-smriti-view.png"

shux pane send-keys -s "$SESSION" --data EA==
shux pane wait-for -s "$SESSION" --text "kriya" --timeout-ms 10000
shux pane send-keys -s "$SESSION" --text "forget"
sleep 1
shux --format json pane snapshot -s "$SESSION" | jq -r .png_base64 | base64 -d > "$OUT/04-smriti-forget-action.png"

shux pane set-size -s "$SESSION" --cols 80 --rows 24
sleep 1
shux --format json pane snapshot -s "$SESSION" | jq -r .png_base64 | base64 -d > "$OUT/05-smriti-narrow.png"

shux pane set-size -s "$SESSION" --cols 160 --rows 48
sleep 1
shux --format json pane snapshot -s "$SESSION" | jq -r .png_base64 | base64 -d > "$OUT/06-smriti-wide.png"

echo "Smriti shux screenshots written to $OUT"
