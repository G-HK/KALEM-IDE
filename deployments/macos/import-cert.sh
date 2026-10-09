#!/usr/bin/env bash
# CI only: import the Developer ID certificates into a temporary keychain so codesign and
# productsign can use them.
#
# Inputs (env):
#   APPLE_CERTIFICATE            base64 .p12 holding "Developer ID Application" (and optionally
#                                "Developer ID Installer")
#   APPLE_CERTIFICATE_PASSWORD
#   KEYCHAIN_PASSWORD            optional; random if unset
# Outputs, appended to $GITHUB_ENV in Actions (printed otherwise):
#   MACOS_SIGN_IDENTITY, MACOS_INSTALLER_IDENTITY (when present), MACOS_KEYCHAIN
#
# A missing certificate only warns (exit 0): package.sh then falls back to ad-hoc signing.
set -euo pipefail
umask 077
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

if [ -z "${APPLE_CERTIFICATE:-}" ]; then
  warn "APPLE_CERTIFICATE is not set: macOS artifacts will be ad-hoc signed and not notarized"
  exit 0
fi

TMP="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
KEYCHAIN="$TMP/kalem-signing.keychain-db"
CERT="$TMP/kalem-signing.p12"
KC_PASS="${KEYCHAIN_PASSWORD:-$(openssl rand -hex 24)}"

trap 'rm -f "$CERT"' EXIT
printf '%s' "$APPLE_CERTIFICATE" | base64 --decode >"$CERT"
security create-keychain -p "$KC_PASS" "$KEYCHAIN"
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$KC_PASS" "$KEYCHAIN"
security import "$CERT" -P "${APPLE_CERTIFICATE_PASSWORD:-}" -T /usr/bin/codesign -T /usr/bin/productsign -T /usr/bin/security -t cert -f pkcs12 -k "$KEYCHAIN"
rm -f "$CERT"
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KC_PASS" "$KEYCHAIN" >/dev/null
# shellcheck disable=SC2046
security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')

APP_ID="$(security find-identity -v -p codesigning "$KEYCHAIN" | awk '/Developer ID Application/ { print $2; exit }')"
INST_ID="$(security find-identity -v "$KEYCHAIN" | awk '/Developer ID Installer/ { print $2; exit }')"
if [ -z "$APP_ID" ]; then
  security find-identity -v "$KEYCHAIN" >&2
  warn "no 'Developer ID Application' identity in the certificate: falling back to ad-hoc signing"
  exit 0
fi
[ -n "$INST_ID" ] || warn "no 'Developer ID Installer' identity: the .pkg will be unsigned"

emit() { if [ -n "${GITHUB_ENV:-}" ]; then echo "$1=$2" >>"$GITHUB_ENV"; else echo "export $1=$2"; fi; }
emit MACOS_SIGN_IDENTITY "$APP_ID"
[ -z "$INST_ID" ] || emit MACOS_INSTALLER_IDENTITY "$INST_ID"
emit MACOS_KEYCHAIN "$KEYCHAIN"
