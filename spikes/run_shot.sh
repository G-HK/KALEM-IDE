#!/usr/bin/env bash
# Usage: run_shot.sh <binary> <out.png> [seconds]
# Runs a GUI binary under Xvfb with software Vulkan (lavapipe) and saves a screenshot.
BIN="$1"; OUT="$2"; SECS="${3:-12}"
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan LIBGL_ALWAYS_SOFTWARE=1
xvfb-run -a -s "-screen 0 1500x920x24" bash -c '
  "$0" >/tmp/spike.log 2>&1 &
  PID=$!
  sleep '"$SECS"'
  import -window root "'"$OUT"'" 2>>/tmp/spike.log
  kill $PID 2>/dev/null
' "$BIN"
tail -5 /tmp/spike.log
