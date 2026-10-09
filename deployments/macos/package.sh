#!/usr/bin/env bash
# Build, sign and (optionally) notarize the macOS release artifacts:
#
#   $DIST/kalem-ide-<version>-macos-<arch>.pkg      installer: /usr/local/bin/kalem + share/kalem/languages
#   $DIST/kalem-ide-<version>-macos-<arch>.tar.gz   plain tree (bin/, share/) for Homebrew or manual installs
#   plus a .sha256 beside each
#
# Usage: deployments/macos/package.sh [--arch universal|aarch64|x86_64] [--skip-build]
#
# Until the GUI exists the product is a command line tool, so the installer is a .pkg. A
# drag-to-Applications Kalem.app in a .dmg follows with the GUI.
#
# Signing (env; import-cert.sh sets these in CI):
#   MACOS_SIGN_IDENTITY       codesign identity. Default "-" = ad-hoc (local testing only;
#                             Gatekeeper rejects the result on other Macs).
#   MACOS_INSTALLER_IDENTITY  "Developer ID Installer" identity for the .pkg (optional)
#   MACOS_KEYCHAIN            keychain holding them (optional)
# Notarization (all three needed, plus real identities):
#   APPLE_ID, APPLE_PASSWORD (app-specific password), APPLE_TEAM_ID
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

ARCH=universal
SKIP_BUILD=0
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$ARCH" in
  universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  aarch64) TARGETS=(aarch64-apple-darwin) ;;
  x86_64) TARGETS=(x86_64-apple-darwin) ;;
  *) echo "unknown --arch $ARCH" >&2; exit 2 ;;
esac

export MACOSX_DEPLOYMENT_TARGET=11.0
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
INSTALLER_IDENTITY="${MACOS_INSTALLER_IDENTITY:-}"
PKG_ID="dev.kalem.ide"   # placeholder reverse-DNS identifier; change with the real domain
BASENAME="kalem-ide-$VERSION-macos-$ARCH"
WORK="$CARGO_TARGET_DIR/macos-package"
ROOT_FS="$WORK/root"

NOTARIZE=0
if [ "$IDENTITY" = "-" ]; then
  warn "macOS: ad-hoc signing (no MACOS_SIGN_IDENTITY); artifacts are not notarized"
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  NOTARIZE=1
else
  warn "macOS: APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID incomplete; signed but not notarized"
fi
echo "==> Kalem IDE $VERSION for macOS ($ARCH), identity: $IDENTITY, notarize: $NOTARIZE"

if [ "$SKIP_BUILD" = 0 ]; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  (cd "$ROOT" && cargo build --release --locked -p kalem "${args[@]}")
fi

rm -rf "$WORK"
mkdir -p "$ROOT_FS/usr/local/bin" "$ROOT_FS/usr/local/share/kalem/languages"
inputs=()
for t in "${TARGETS[@]}"; do inputs+=("$CARGO_TARGET_DIR/$t/release/kalem"); done
lipo -create -output "$ROOT_FS/usr/local/bin/kalem" "${inputs[@]}"
lipo -info "$ROOT_FS/usr/local/bin/kalem"
cp "$LANG_SRC"/*.toml "$ROOT_FS/usr/local/share/kalem/languages/"

# ---- sign the binary (hardened runtime + secure timestamp for a real identity) -----------------
ts=(--timestamp)
[ "$IDENTITY" = "-" ] && ts=(--timestamp=none)
kc=()
[ -n "${MACOS_KEYCHAIN:-}" ] && kc=(--keychain "$MACOS_KEYCHAIN")
codesign --force --options runtime --sign "$IDENTITY" ${kc[@]+"${kc[@]}"} "${ts[@]}" \
  --entitlements "$ROOT/deployments/macos/entitlements.plist" "$ROOT_FS/usr/local/bin/kalem"
codesign --verify --strict --verbose=2 "$ROOT_FS/usr/local/bin/kalem"

# ---- tar.gz ------------------------------------------------------------------------------------
TREE="$WORK/$BASENAME"
mkdir -p "$TREE/bin" "$TREE/share"
cp "$ROOT_FS/usr/local/bin/kalem" "$TREE/bin/"
cp -R "$ROOT_FS/usr/local/share/kalem" "$TREE/share/"
cp "$ROOT/README.md" "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$TREE/"
tar -C "$WORK" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
sha256_file "$DIST/$BASENAME.tar.gz"
echo "created $DIST/$BASENAME.tar.gz"

# ---- pkg ---------------------------------------------------------------------------------------
PKG="$DIST/$BASENAME.pkg"
RAW="$WORK/component.pkg"
pkgbuild --root "$ROOT_FS" --identifier "$PKG_ID" --version "${VERSION%%-*}" --install-location / "$RAW"
if [ -n "$INSTALLER_IDENTITY" ]; then
  kcp=()
  [ -n "${MACOS_KEYCHAIN:-}" ] && kcp=(--keychain "$MACOS_KEYCHAIN")
  productbuild --package "$RAW" --sign "$INSTALLER_IDENTITY" ${kcp[@]+"${kcp[@]}"} "$PKG"
else
  warn "macOS: no MACOS_INSTALLER_IDENTITY; the .pkg is unsigned and cannot be notarized"
  productbuild --package "$RAW" "$PKG"
  NOTARIZE=0
fi

if [ "$NOTARIZE" = 1 ]; then
  echo "==> notarizing $PKG"
  xcrun notarytool submit "$PKG" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait
  xcrun stapler staple "$PKG"
fi
sha256_file "$PKG"
echo "created $PKG"
