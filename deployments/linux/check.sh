#!/usr/bin/env bash
# Linux check for the desktop app, run inside deployments/linux/Dockerfile (see there for usage):
# clippy, build and tests; no missing shared libraries; the IDE starts with no arguments (as the
# app launcher starts it) and with a folder, and draws something (screenshot is not one colour).
# Output: dist/linux-check/kalem-ide.png and kalem-ide.log.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
# The mounted repo belongs to another user; without this git (and so Solution Explorer) refuses it.
git config --global --add safe.directory "$PWD"

cargo clippy -p kalem-ui --all-targets -- -D warnings
cargo build -p kalem-ui
cargo test

BIN=target/debug/kalem-ide
ldd "$BIN"
if ldd "$BIN" | grep -q 'not found'; then echo "kalem-ide: missing shared libraries" >&2; exit 1; fi

OUT=dist/linux-check
mkdir -p "$OUT"
# Without Composite: GPUI draws into a 32-bit transparent window whenever the server offers one,
# and with software Vulkan and no compositor running such a window stays blank (see
# docs/ROADMAP.md, "Known platform issues"). Without the extension it uses a normal 24-bit window.
Xvfb :99 -screen 0 1600x1000x24 -extension Composite &
XVFB=$!
APP=
trap 'kill $APP $XVFB 2>/dev/null || true' EXIT
export DISPLAY=:99
export XDG_RUNTIME_DIR=/tmp/xdg-runtime
mkdir -p -m 700 "$XDG_RUNTIME_DIR"
# Software Vulkan: the container has no GPU.
VK_ICD_FILENAMES="$(ls /usr/share/vulkan/icd.d/lvp_icd.*.json | head -n1)"
export VK_ICD_FILENAMES
sleep 1

# start <seconds> [args...]: start kalem-ide, fail if it exits within <seconds>.
start() {
  local wait="$1"; shift
  "$BIN" "$@" >"$OUT/kalem-ide.log" 2>&1 &
  APP=$!
  sleep "$wait"
  if ! kill -0 "$APP" 2>/dev/null; then
    cat "$OUT/kalem-ide.log"
    echo "kalem-ide $* exited during start-up" >&2
    exit 1
  fi
}

start 5
kill "$APP"; wait "$APP" 2>/dev/null || true

start 10 "$PWD"
import -window root "$OUT/kalem-ide.png"
# The dock ties the window to kalem-ide.desktop through WM_CLASS (main.rs APP_ID).
win="$(xwininfo -root -tree | awk '/"Kalem IDE"/{print $1; exit}')"
if ! xprop -id "$win" WM_CLASS | grep -q '"kalem-ide"'; then
  echo "window WM_CLASS is not kalem-ide: $(xprop -id "$win" WM_CLASS)" >&2
  exit 1
fi
kill "$APP"; wait "$APP" 2>/dev/null || true

colours="$(identify -format %k "$OUT/kalem-ide.png")"
if [ "$colours" -le 1 ]; then echo "screenshot is blank" >&2; exit 1; fi
echo "OK: $OUT/kalem-ide.png ($colours colours)"
