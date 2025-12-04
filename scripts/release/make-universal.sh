#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 3 ]; then
  echo "usage: $0 <arm64-bin-dir> <x86_64-bin-dir> <output-bin-dir>" >&2
  exit 1
fi

ARM64_DIR="$1"
X86_DIR="$2"
OUT_DIR="$3"
BINS=("vicaya" "vicaya-daemon" "vicaya-tui")

mkdir -p "$OUT_DIR"
for bin in "${BINS[@]}"; do
  if [ ! -f "$ARM64_DIR/$bin" ] || [ ! -f "$X86_DIR/$bin" ]; then
    echo "missing binary $bin in $ARM64_DIR or $X86_DIR" >&2
    exit 1
  fi
  lipo -create "$ARM64_DIR/$bin" "$X86_DIR/$bin" -output "$OUT_DIR/$bin"
  chmod +x "$OUT_DIR/$bin"
  echo "Created universal $OUT_DIR/$bin"

  file "$OUT_DIR/$bin"
done
