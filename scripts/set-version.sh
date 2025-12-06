#!/usr/bin/env bash
# Updates workspace version in Cargo.toml
# Usage: ./scripts/set-version.sh <version>

set -euo pipefail

VERSION="${1:-}"

if [[ -z "$VERSION" ]]; then
    echo "Usage: $0 <version>" >&2
    exit 1
fi

# Strip leading 'v' if present
VERSION="${VERSION#v}"

# Validate semver format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
    echo "Invalid semver version: $VERSION" >&2
    exit 1
fi

CARGO_TOML="Cargo.toml"

if [[ ! -f "$CARGO_TOML" ]]; then
    echo "Error: $CARGO_TOML not found" >&2
    exit 1
fi

# Update version in [workspace.package] section
# Match: version = "x.y.z" under [workspace.package]
# Use temp file for cross-platform compatibility (macOS vs Linux sed)
TEMP_FILE=$(mktemp)
sed -E 's/^(version = ")[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(")/\1'"$VERSION"'\3/' "$CARGO_TOML" > "$TEMP_FILE"
mv "$TEMP_FILE" "$CARGO_TOML"

echo "Updated $CARGO_TOML to version $VERSION"

# Verify the change
grep -E '^version = "' "$CARGO_TOML" | head -1
