#!/usr/bin/env bash
# Package a release build for Linux/macOS.
# Usage: package-unix.sh <version> <rust-target> <platform-name>
# Run from the repository root after: cargo build --release -p kalem --target <rust-target>
set -euo pipefail

VERSION="${1:?version required, e.g. v0.1.0}"
TARGET="${2:?rust target required}"
PLATFORM="${3:?platform name required, e.g. linux-x86_64}"

NAME="kalem-ide-${VERSION}-${PLATFORM}"
BIN="target/${TARGET}/release/kalem"
[ -f "$BIN" ] || { echo "missing $BIN - build first" >&2; exit 1; }

rm -rf "$NAME" "$NAME.tar.gz" "$NAME.tar.gz.sha256"
mkdir "$NAME"
cp "$BIN" "$NAME/"
cp -r src/kalem-lang/languages "$NAME/languages"
cp README.md LICENSE-MIT LICENSE-APACHE "$NAME/"
tar -czf "$NAME.tar.gz" "$NAME"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256"
else
  shasum -a 256 "$NAME.tar.gz" > "$NAME.tar.gz.sha256"
fi
rm -rf "$NAME"
echo "created $NAME.tar.gz"
