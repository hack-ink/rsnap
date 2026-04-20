#!/usr/bin/env bash

live_loupe_smoke_dir() {
  cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd
}

live_loupe_repo_root() {
  cd -- "$(live_loupe_smoke_dir)/../.." && pwd
}

live_loupe_cursor_helper() {
  printf '%s/lib/live-loupe-mouse-path.swift\n' "$(live_loupe_smoke_dir)"
}

live_loupe_log_summary_helper() {
  printf '%s/lib/live-loupe-log-summary.py\n' "$(live_loupe_smoke_dir)"
}

live_loupe_usage() {
  cat <<'EOF'
Usage: live-loupe-perf-macos.sh [--self-check] [--help]

Environment overrides:
  RSNAP_CMD              command used to launch rsnap (default: target/release/rsnap
                         when present, else cargo run --release -p rsnap)
  RSNAP_RUST_LOG         log filter for the rsnap process
                         (default: rsnap=info,rsnap_overlay=trace)
  DISPLAY_BOUNDS         "left,top,right,bottom" override for the main display
  PATH_POINTS            semicolon-separated "x,y" cursor waypoints override
  PATH_SEGMENT_STEPS     interpolation steps per segment (default: 18)
  PATH_STEP_DELAY_MS     delay between move events in ms (default: 10)
  PATH_CYCLES            repeat count for the waypoint path (default: 2)
  OVERLAY_SETTLE_S       delay after live overlay startup before the Alt path
                         (default: 0)
  POST_PATH_SETTLE_S     delay after releasing Alt before parsing logs
                         (default: 0.25)
  WAIT_STARTUP_S         timeout for startup log marker (default: 30)
  WAIT_OVERLAY_S         timeout for overlay start log marker (default: 10)
  MAX_ACQUIRE_FRAME_WARNS
                         fail if overlay.window_renderer_acquire_frame warns exceed
                         this count (default: 2)
  MAX_EVENT_LOOP_STALLS  fail if overlay.event_loop_stall warns exceed this count
                         (default: 2)
  MAX_RESIZED_EVENTS     fail if WindowEvent::Resized trace lines exceed this count
                         (default: 24)
  MAX_SLOW_OP_WARNINGS   fail if total "Slow operation detected" warnings exceed this
                         count (default: 6)
  MAX_LIVE_SAMPLE_APPLY_LATENCY_WARNS
                         optional gate on overlay.live_sample_apply_latency warning count
                         (default: unset = disabled)
  MAX_LIVE_SAMPLE_APPLY_LATENCY_MS
                         optional gate on max overlay.live_sample_apply_latency latency_ms
                         (default: unset = disabled)
EOF
}

live_loupe_self_check() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "live-loupe perf smoke is macOS-only" >&2
    return 1
  fi

  for cmd in osascript swift python3 rg; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "missing required tool: $cmd" >&2
      return 1
    fi
  done

  echo "[smoke] self-check ok"
}

live_loupe_init_environment() {
  ROOT_DIR="$1"
  LOG_DIR="$HOME/Library/Application Support/ink.hack.rsnap/logs"

  local default_rsnap_cmd="cargo run --release -p rsnap"
  if [[ -x "$ROOT_DIR/target/release/rsnap" ]]; then
    default_rsnap_cmd="$ROOT_DIR/target/release/rsnap"
  fi

  RSNAP_CMD="${RSNAP_CMD:-$default_rsnap_cmd}"
  RSNAP_RUST_LOG="${RSNAP_RUST_LOG:-rsnap=info,rsnap_overlay=trace}"
  DISPLAY_BOUNDS="${DISPLAY_BOUNDS:-}"
  PATH_POINTS="${PATH_POINTS:-}"
  PATH_SEGMENT_STEPS="${PATH_SEGMENT_STEPS:-18}"
  PATH_STEP_DELAY_MS="${PATH_STEP_DELAY_MS:-10}"
  PATH_CYCLES="${PATH_CYCLES:-2}"
  OVERLAY_SETTLE_S="${OVERLAY_SETTLE_S:-0}"
  POST_PATH_SETTLE_S="${POST_PATH_SETTLE_S:-0.25}"
  WAIT_STARTUP_S="${WAIT_STARTUP_S:-30}"
  WAIT_OVERLAY_S="${WAIT_OVERLAY_S:-10}"
  MAX_ACQUIRE_FRAME_WARNS="${MAX_ACQUIRE_FRAME_WARNS:-2}"
  MAX_EVENT_LOOP_STALLS="${MAX_EVENT_LOOP_STALLS:-2}"
  MAX_RESIZED_EVENTS="${MAX_RESIZED_EVENTS:-24}"
  MAX_SLOW_OP_WARNINGS="${MAX_SLOW_OP_WARNINGS:-6}"
  MAX_LIVE_SAMPLE_APPLY_LATENCY_WARNS="${MAX_LIVE_SAMPLE_APPLY_LATENCY_WARNS:-}"
  MAX_LIVE_SAMPLE_APPLY_LATENCY_MS="${MAX_LIVE_SAMPLE_APPLY_LATENCY_MS:-}"

  RSNAP_LOG=""
  RSNAP_PID=""
  OPTION_HELD=0
  SESSION_END_TS=""
}

live_loupe_fail() {
  echo "[smoke] $*" >&2
  if [[ -n "$RSNAP_LOG" && -f "$RSNAP_LOG" ]]; then
    echo "[smoke] recent rsnap log excerpt:" >&2
    tail -n 120 "$RSNAP_LOG" >&2 || true
  fi
  exit 1
}

live_loupe_release_option_key() {
  if (( ! OPTION_HELD )); then
    return
  fi

  osascript <<'APPLESCRIPT' >/dev/null 2>&1 || true
tell application "System Events"
    key up option
end tell
APPLESCRIPT
  OPTION_HELD=0
}

live_loupe_stop_existing_rsnap() {
  if ! pgrep -x rsnap >/dev/null 2>&1; then
    return
  fi

  echo "[smoke] stopping existing rsnap processes" >&2
  pkill -x rsnap >/dev/null 2>&1 || true

  local deadline=$((SECONDS + 10))
  while pgrep -x rsnap >/dev/null 2>&1; do
    if (( SECONDS > deadline )); then
      live_loupe_fail "existing rsnap process did not stop"
    fi
    sleep 0.2
  done
}

live_loupe_cleanup() {
  live_loupe_release_option_key

  if [[ -n "$RSNAP_PID" ]] && kill -0 "$RSNAP_PID" >/dev/null 2>&1; then
    kill "$RSNAP_PID" >/dev/null 2>&1 || true
    wait "$RSNAP_PID" >/dev/null 2>&1 || true
  fi
}

live_loupe_install_trap() {
  trap live_loupe_cleanup EXIT
}

live_loupe_refresh_log_path() {
  RSNAP_LOG="$(ls -1t "$LOG_DIR"/rsnap*.log 2>/dev/null | head -n 1 || true)"
}

live_loupe_capture_session_end_ts() {
  SESSION_END_TS="$(python3 - <<'PY'
from datetime import datetime, timezone

print(datetime.now(timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z"))
PY
)"
}

live_loupe_wait_for_pattern() {
  local pattern="$1"
  local timeout_s="$2"
  local deadline=$((SECONDS + timeout_s))

  while (( SECONDS <= deadline )); do
    live_loupe_refresh_log_path
    if [[ -n "$RSNAP_LOG" && -f "$RSNAP_LOG" ]] && rg -q "$pattern" "$RSNAP_LOG"; then
      return 0
    fi
    sleep 0.25
  done

  return 1
}

live_loupe_read_main_display_bounds() {
  osascript <<'APPLESCRIPT'
tell application "Finder"
    return bounds of window of desktop
end tell
APPLESCRIPT
}

live_loupe_derive_live_path_points() {
  local bounds="$1"
  python3 - "$bounds" <<'PY'
import sys

left, top, right, bottom = map(int, sys.argv[1].replace(" ", "").split(","))
width = right - left
height = bottom - top

if width < 400 or height < 300:
    raise SystemExit("display too small for live perf smoke")

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

live_loupe_trigger_capture_from_tray_menu() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    tell process "rsnap"
        click menu bar item 1 of menu bar 2
        delay 0.2
        click menu item "Capture" of menu 1 of menu bar item 1 of menu bar 2
    end tell
end tell
APPLESCRIPT
}

live_loupe_focus_rsnap_overlay() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    tell process "rsnap"
        set frontmost to true
    end tell
end tell
delay 0.15
APPLESCRIPT
}

live_loupe_hold_option_key() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    key down option
end tell
APPLESCRIPT
  OPTION_HELD=1
}

live_loupe_press_escape() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    key code 53
end tell
APPLESCRIPT
}

live_loupe_launch_rsnap() {
  (
    cd "$ROOT_DIR"
    export RUST_LOG="$RSNAP_RUST_LOG"
    exec zsh -lc "$RSNAP_CMD"
  ) >/tmp/rsnap-live-loupe-perf-smoke-rsnap.out 2>&1 &
  RSNAP_PID=$!
}

live_loupe_run_mouse_path() {
  PATH_POINTS="$PATH_POINTS" \
  PATH_SEGMENT_STEPS="$PATH_SEGMENT_STEPS" \
  PATH_STEP_DELAY_MS="$PATH_STEP_DELAY_MS" \
  PATH_CYCLES="$PATH_CYCLES" \
  swift "$(live_loupe_cursor_helper)"
}

live_loupe_summarize_and_gate_log() {
  local log_path="$1"

  python3 "$(live_loupe_log_summary_helper)" \
    "$log_path" \
    "$SESSION_END_TS" \
    "$MAX_ACQUIRE_FRAME_WARNS" \
    "$MAX_EVENT_LOOP_STALLS" \
    "$MAX_RESIZED_EVENTS" \
    "$MAX_SLOW_OP_WARNINGS" \
    "$MAX_LIVE_SAMPLE_APPLY_LATENCY_WARNS" \
    "$MAX_LIVE_SAMPLE_APPLY_LATENCY_MS"
}
