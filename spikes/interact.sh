#!/usr/bin/env bash
# Usage: interact.sh <binary> <out-prefix>   (egui spike: click Sync, then open another file)
BIN="$1"; OUT="$2"
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan LIBGL_ALWAYS_SOFTWARE=1
xvfb-run -a -s "-screen 0 1500x920x24" bash -c '
  "$0" >/tmp/spike.log 2>&1 &
  PID=$!
  sleep 18
  xdotool mousemove 87 98 click 1; sleep 3
  import -window root "'"$OUT"'-sync.png"
  kill $PID 2>/dev/null
' "$BIN"
