#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 3 ]; then
  echo "usage: $0 <universal-bin-dir> <pkg-path> <version>" >&2
  exit 1
fi

BIN_DIR="$1"
PKG_PATH="$2"
VERSION="$3"
IDENTIFIER="${PKG_IDENTIFIER:-org.indrasvat.vicaya}"
PKGROOT="$(mktemp -d)"

mkdir -p "$PKGROOT/usr/local/bin"
cp "$BIN_DIR"/vicaya* "$PKGROOT/usr/local/bin/"

pkgbuild \
  --root "$PKGROOT" \
  --identifier "$IDENTIFIER" \
  --version "$VERSION" \
  "$PKG_PATH"

rm -rf "$PKGROOT"
