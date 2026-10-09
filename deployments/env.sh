# Shared by the packaging scripts. Source it: . "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
#
# Sets ROOT, VERSION, DIST, CARGO_TARGET_DIR and the warn() helper.
# The version lives in one place: [workspace.package] version in the root Cargo.toml.
# KALEM_VERSION overrides it (CI test runs); a leading "v" is dropped.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

if [ -z "${VERSION:-}" ]; then
  VERSION="${KALEM_VERSION:-}"
  VERSION="${VERSION#v}"
fi
if [ -z "${VERSION:-}" ]; then
  VERSION="$(awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]"); next }
    in_pkg && /^version[ \t]*=/ { gsub(/.*=[ \t]*"|".*/, ""); print; exit }
  ' "$ROOT/Cargo.toml")"
fi
[ -n "$VERSION" ] || { echo "could not determine the version" >&2; exit 1; }
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
export DIST CARGO_TARGET_DIR
mkdir -p "$DIST"

# Where the language packs live in the source tree.
LANG_SRC="$ROOT/src/kalem-lang/languages"
export LANG_SRC

warn() {
  if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$(dirname "$1")" && sha256sum "$(basename "$1")" >"$(basename "$1").sha256")
  else
    (cd "$(dirname "$1")" && shasum -a 256 "$(basename "$1")" >"$(basename "$1").sha256")
  fi
}
