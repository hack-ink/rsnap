#!/usr/bin/env bash

live_hud_smoke_dir() {
  cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd
}

live_hud_repo_root() {
  cd -- "$(live_hud_smoke_dir)/../.." && pwd
}

live_hud_cursor_helper() {
  printf '%s/lib/live-hud-mouse-path.swift\n' "$(live_hud_smoke_dir)"
}

live_hud_self_check() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "live HUD smoke is macOS-only" >&2
    return 1
  fi

  for cmd in osascript swift swiftc python3; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "missing required tool: $cmd" >&2
      return 1
    fi
  done

  echo "[smoke] self-check ok"
}

live_hud_init_environment() {
  ROOT_DIR="$1"
  DISPLAY_BOUNDS="${DISPLAY_BOUNDS:-}"
  PATH_POINTS="${PATH_POINTS:-}"
  PATH_SEGMENT_STEPS="${PATH_SEGMENT_STEPS:-18}"
  PATH_STEP_DELAY_MS="${PATH_STEP_DELAY_MS:-10}"
}

live_hud_read_main_display_bounds() {
  osascript <<'APPLESCRIPT'
tell application "Finder"
    return bounds of window of desktop
end tell
APPLESCRIPT
}

live_hud_focus_rsnap_overlay() {
  local focus_settle_s="${RSNAP_FOCUS_SETTLE_S:-0.03}"
  osascript <<APPLESCRIPT
tell application "System Events"
    tell process "Rsnap"
        set frontmost to true
    end tell
end tell
delay $focus_settle_s
APPLESCRIPT
}

live_hud_derive_live_path_points() {
  local bounds="$1"
  python3 - "$bounds" <<'PY'
import sys

left, top, right, bottom = map(int, sys.argv[1].replace(" ", "").split(","))
width = right - left
height = bottom - top

if width < 400 or height < 300:
    raise SystemExit("display too small for live HUD smoke")

x1 = left + max(160, width * 28 // 100)
x2 = left + width // 2
x3 = right - max(160, width * 28 // 100)
y1 = top + max(180, height * 30 // 100)
y2 = top + height // 2
y3 = bottom - max(180, height * 22 // 100)

points = [
    (x1, y1),
    (x2, y1),
    (x3, y2),
    (x2, y3),
    (x1, y2),
    (x2, y1),
]

print(";".join(f"{x},{y}" for x, y in points))
PY
}

live_hud_press_escape() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    key code 53
end tell
APPLESCRIPT
}

live_hud_press_tab() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    key code 48
end tell
APPLESCRIPT
}

live_hud_run_mouse_path() {
  PATH_POINTS="$PATH_POINTS" \
  PATH_MODE="$PATH_MODE" \
  PATH_DRIVER="$PATH_DRIVER" \
  PATH_SEGMENT_STEPS="$PATH_SEGMENT_STEPS" \
  PATH_STEP_DELAY_MS="$PATH_STEP_DELAY_MS" \
  PATH_DURATION_MS="$PATH_DURATION_MS" \
  PATH_RATE_HZ="$PATH_RATE_HZ" \
  PATH_CYCLES="$PATH_CYCLES" \
  swift "$(live_hud_cursor_helper)"
}
