#!/usr/bin/env bash
# Build and package Kalem IDE for Linux (x86_64 or aarch64, whatever this machine is):
#
#   $DIST/kalem-ide-<version>-linux-<arch>.tar.gz   FHS-style tree (bin/, share/)
#   $DIST/kalem-ide-<version>-linux-<arch>.deb      Debian, Ubuntu, Mint, ...
#   $DIST/kalem-ide-<version>-linux-<arch>.rpm      Fedora, openSUSE, RHEL, ...
#   plus a .sha256 beside each
#
# Usage: deployments/linux/package.sh [--skip-build] [--formats "tar deb rpm"]
#
# deb/rpm need nfpm (https://nfpm.goreleaser.com); they are skipped with a warning if it is
# missing. Build on an old distro (CI: Ubuntu 22.04, glibc 2.35) so the binaries run on newer ones.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/deployments/linux"

SKIP_BUILD=0
FORMATS="tar deb rpm"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    -h | --help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
BASENAME="kalem-ide-$VERSION-linux-$ARCH"
echo "==> Kalem IDE $VERSION for Linux $ARCH ($FORMATS)"

if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p kalem -p kalem-ui)
fi
BIN="$CARGO_TARGET_DIR/release/kalem"
IDE="$CARGO_TARGET_DIR/release/kalem-ide"
for f in "$BIN" "$IDE"; do
  [ -f "$f" ] || { echo "missing $f - build first" >&2; exit 1; }
done

WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"
install -Dm755 "$BIN" "$STAGE/usr/bin/kalem"
install -Dm755 "$IDE" "$STAGE/usr/bin/kalem-ide"
install -Dm644 "$HERE/kalem-ide.desktop" "$STAGE/usr/share/applications/kalem-ide.desktop"
install -Dm644 "$ROOT/src/kalem-ui/assets/kalem-ide.png" "$STAGE/usr/share/icons/hicolor/512x512/apps/kalem-ide.png"
mkdir -p "$STAGE/usr/share/kalem/languages" "$STAGE/usr/share/doc/kalem-ide"
cp "$LANG_SRC"/*.toml "$STAGE/usr/share/kalem/languages/"
cp "$ROOT/README.md" "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$STAGE/usr/share/doc/kalem-ide/"
strip "$STAGE/usr/bin/kalem" "$STAGE/usr/bin/kalem-ide" 2>/dev/null || true

for fmt in $FORMATS; do
  case "$fmt" in
    tar)
      out="$DIST/$BASENAME.tar.gz"
      tar -C "$STAGE" --transform "s,^usr,$BASENAME," --owner=0 --group=0 -czf "$out" usr
      sha256_file "$out"; echo "created $out" ;;
    deb | rpm)
      if ! command -v nfpm >/dev/null 2>&1; then
        warn "nfpm not found: skipping $fmt (install: https://nfpm.goreleaser.com/install/)"
        continue
      fi
      case "$ARCH" in x86_64) NFPM_ARCH=amd64 ;; *) NFPM_ARCH=arm64 ;; esac
      conf="$WORK/nfpm.yaml"
      sed "s,\${STAGE},$STAGE,g" "$HERE/nfpm.yaml" >"$conf"
      out="$DIST/$BASENAME.$fmt"
      NFPM_ARCH="$NFPM_ARCH" VERSION="$VERSION" nfpm package -f "$conf" -p "$fmt" -t "$out"
      sha256_file "$out"; echo "created $out" ;;
    *) echo "unknown format '$fmt'" >&2; exit 2 ;;
  esac
done
