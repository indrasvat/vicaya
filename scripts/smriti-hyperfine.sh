#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VICAYA_DIR="${VICAYA_DIR:-$ROOT/.shux/out/smriti-bench-state}"
OUT="${OUT:-$ROOT/.shux/out/smriti}"

mkdir -p "$VICAYA_DIR" "$OUT"
cat > "$VICAYA_DIR/config.toml" <<EOF
index_roots = ["$HOME"]
exclusions = ["System", "Library", ".git", "node_modules", "target", ".cargo", ".rustup"]
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

export VICAYA_DIR
export VICAYA_DAEMON_BIN="$ROOT/target/release/vicaya-daemon"
export VICAYA_NO_UPDATE_CHECK=1

REPO_A="$ROOT"
REPO_B="$(find "$HOME/code/github.com" -mindepth 1 -maxdepth 1 -type d ! -name '.*' ! -path "$ROOT" | head -n 1 || true)"
if [[ -z "$REPO_B" ]]; then
  REPO_B="$ROOT"
fi

"$ROOT/target/release/vicaya" daemon stop >/dev/null 2>&1 || true
"$ROOT/target/release/vicaya" rebuild

SMRITI_MAIN="$(find "$ROOT" -path "$ROOT/target" -prune -o -name main.rs -type f -print | head -n 1 || true)"
if [[ -z "$SMRITI_MAIN" ]]; then
  SMRITI_MAIN="$ROOT/Cargo.toml"
fi
python3 - "$VICAYA_DIR/smriti.json" "$(date +%s)" \
  "$SMRITI_MAIN" "$ROOT/Cargo.toml" "$REPO_B/README.md" <<'PY'
import json
import pathlib
import sys

output = pathlib.Path(sys.argv[1])
now = int(sys.argv[2])
paths = []
for raw in sys.argv[3:]:
    path = pathlib.Path(raw)
    if path.exists() and str(path) not in paths:
        paths.append(str(path))

entries = {}
for idx, path in enumerate(paths):
    count = max(1, 12 - idx * 3)
    entries[path] = {
        "path": path,
        "name": pathlib.Path(path).name,
        "total_count": count,
        "open_count": count,
        "copy_count": 0,
        "reveal_count": 0,
        "print_count": 0,
        "enter_count": 0,
        "first_used": now - 3600 * (idx + 1),
        "last_used": now - 60 * idx,
        "last_query": pathlib.Path(path).stem,
        "last_action": "open",
    }

output.write_text(json.dumps({"version": 1, "entries": entries}, separators=(",", ":")))
print(f"Seeded {len(entries)} Smriti benchmark entries in {output}")
PY

"$ROOT/target/release/vicaya" daemon start
trap '"$ROOT/target/release/vicaya" daemon stop >/dev/null 2>&1 || true' EXIT

hyperfine --warmup 20 --runs 100 \
  "$ROOT/target/release/vicaya search main.rs --limit 20 --format plain" \
  "$ROOT/target/release/vicaya search config --scope \"$HOME\" --limit 20 --format plain" \
  "$ROOT/target/release/vicaya search Cargo.toml --scope \"$REPO_A\" --limit 20 --format plain" \
  "$ROOT/target/release/vicaya search README --scope \"$REPO_B\" --limit 20 --format plain" \
  "$ROOT/target/release/vicaya smriti list --limit 50 --format plain" \
  --export-json "$OUT/hyperfine-smriti.json"
