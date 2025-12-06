#!/usr/bin/env bash
# Builds release artifacts for semantic-release
# Called by semantic-release prepareCmd

set -euo pipefail

echo "==> Building for aarch64-apple-darwin..."
cargo build --workspace --release --target aarch64-apple-darwin

echo "==> Building for x86_64-apple-darwin..."
cargo build --workspace --release --target x86_64-apple-darwin

echo "==> Creating universal binaries..."
mkdir -p artifacts/bin
for bin in vicaya vicaya-daemon vicaya-tui; do
    lipo -create \
        target/aarch64-apple-darwin/release/$bin \
        target/x86_64-apple-darwin/release/$bin \
        -output artifacts/bin/$bin
done

echo "==> Packaging tarball..."
cd artifacts
tar -czvf vicaya-universal.tar.gz bin
shasum -a 256 vicaya-universal.tar.gz > vicaya-universal.tar.gz.sha256
cd ..

echo "==> Build complete!"
ls -la artifacts/
