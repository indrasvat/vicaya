#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: $0 <universal-bin-dir> <output-tar-gz>" >&2
  exit 1
fi

BIN_DIR="$1"
OUTPUT="$2"
WORKDIR="$(mktemp -d)"
INSTALL_SH="$WORKDIR/install.sh"

mkdir -p "$WORKDIR/bin"
cp "$BIN_DIR"/vicaya* "$WORKDIR/bin/"

cat > "$INSTALL_SH" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
PREFIX="${PREFIX:-/usr/local/bin}"
BIN_DIR="$(cd "$(dirname "$0")" && pwd)/bin"
mkdir -p "$PREFIX"
for bin in vicaya vicaya-daemon vicaya-tui; do
  install -m 0755 "$BIN_DIR/$bin" "$PREFIX/$bin"
  echo "Installed $PREFIX/$bin"
done
SCRIPT
chmod +x "$INSTALL_SH"

tar -C "$WORKDIR" -czf "$OUTPUT" .
rm -rf "$WORKDIR"
