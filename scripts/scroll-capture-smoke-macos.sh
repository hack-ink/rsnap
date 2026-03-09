#!/usr/bin/env bash
set -euo pipefail

# Deterministic macOS desktop smoke for rsnap scroll capture mode.
#
# Assumptions:
# - Runs inside a logged-in macOS GUI session.
# - Screen Recording is already granted for the rsnap binary/build command so
#   live capture frames are available.
# - Accessibility is only needed for the smoke automation (`osascript`/`swift`)
#   and optional scrollbar verification. It is not a runtime prerequisite for
#   rsnap scroll-capture availability.
# - The script intentionally launches a fresh rsnap process and closes open
#   TextEdit windows so the run is reproducible.
# - Top-level settle delays default to zero on release builds; environment
#   overrides remain available if a local GUI session needs extra slack.
# - The content movement must come from synthetic scroll-wheel input. The
#   script may read the TextEdit vertical scrollbar value for optional
#   verification, but it never drives the scrollbar through Accessibility
#   writes.
# - The required assertion surface is log-based: startup, overlay start,
#   scroll-capture start, and at least one append. The harness uses the rsnap
#   tray `Capture` menu item instead of synthetic global-hotkey injection
#   because the tray path is more stable in automated GUI sessions. Scrollbar
#   verification is an optional stronger check when AX access is available.

usage() {
  cat <<'EOF'
Usage: scroll-capture-smoke-macos.sh [--self-check] [--help]

Environment overrides:
  RSNAP_CMD           command used to launch rsnap (default: target/release/rsnap
                      when present, else cargo run --release -p rsnap)
  TEXTEDIT_BOUNDS     "left,top,right,bottom" for the fixture window
  DRAG_START          "x,y" capture drag start point
  DRAG_END            "x,y" capture drag end point
  SCROLL_POINT        "x,y" point inside the capture rect for downward scrolls
  SCROLL_EVENTS       number of downward wheel events to emit
  SCROLL_DELTA        negative pixel delta for each wheel event
  VERIFY_SCROLLBAR    set to 1 to require AX scrollbar verification, 0 to skip
                      it, or auto to use it only when available (default: auto)
  OVERLAY_SETTLE_S    delay after overlay startup before drag selection
                      (default: 0)
  DRAG_SETTLE_S       delay after drag selection before entering scroll capture
                      mode (default: 0)
  SCROLL_MODE_SETTLE_S
                      delay after scroll capture start before emitting scroll
                      events (default: 0)
  WAIT_STARTUP_S      timeout for startup log marker
  WAIT_OVERLAY_S      timeout for overlay start log marker
  WAIT_SCROLL_CAPTURE_S
                      timeout for scroll capture mode start marker
  WAIT_APPEND_S       timeout for append marker
EOF
}

self_check() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "scroll-capture smoke is macOS-only" >&2
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

case "${1:-}" in
  --help|-h)
    usage
    exit 0
    ;;
  --self-check)
    self_check
    exit $?
    ;;
  "")
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

self_check

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
LOG_DIR="$HOME/Library/Application Support/ink.hack.rsnap/logs"
DEFAULT_RSNAP_CMD="cargo run --release -p rsnap"
if [[ -x "$ROOT_DIR/target/release/rsnap" ]]; then
  DEFAULT_RSNAP_CMD="$ROOT_DIR/target/release/rsnap"
fi
RSNAP_CMD="${RSNAP_CMD:-$DEFAULT_RSNAP_CMD}"
TEXTEDIT_BOUNDS="${TEXTEDIT_BOUNDS:-120,120,1040,960}"
DRAG_START="${DRAG_START:-}"
DRAG_END="${DRAG_END:-}"
SCROLL_POINT="${SCROLL_POINT:-}"
SCROLL_EVENTS="${SCROLL_EVENTS:-28}"
SCROLL_DELTA="${SCROLL_DELTA:--32}"
OVERLAY_SETTLE_S="${OVERLAY_SETTLE_S:-0}"
DRAG_SETTLE_S="${DRAG_SETTLE_S:-0}"
SCROLL_MODE_SETTLE_S="${SCROLL_MODE_SETTLE_S:-0}"
WAIT_STARTUP_S="${WAIT_STARTUP_S:-30}"
WAIT_OVERLAY_S="${WAIT_OVERLAY_S:-10}"
WAIT_SCROLL_CAPTURE_S="${WAIT_SCROLL_CAPTURE_S:-10}"
WAIT_APPEND_S="${WAIT_APPEND_S:-10}"
VERIFY_SCROLLBAR="${VERIFY_SCROLLBAR:-auto}"

RSNAP_LOG=""
RSNAP_PID=""
SCROLLBAR_VERIFICATION_ACTIVE=0
SCROLLBAR_AUTO_PROBE_PENDING=0
FIXTURE_FILE_BASE="$(mktemp -t rsnap-scroll-capture-fixture)"
FIXTURE_FILE="${FIXTURE_FILE_BASE}.txt"
mv "$FIXTURE_FILE_BASE" "$FIXTURE_FILE"
SWIFT_HELPER_BASE="$(mktemp -t rsnap-scroll-capture-swift)"
SWIFT_HELPER="${SWIFT_HELPER_BASE}.swift"
mv "$SWIFT_HELPER_BASE" "$SWIFT_HELPER"

close_textedit_windows() {
  osascript <<'APPLESCRIPT' >/dev/null 2>&1 || true
try
    tell application "TextEdit"
        if it is running then
            repeat with docRef in (documents as list)
                try
                    close docRef saving no
                end try
            end repeat
            try
                close every window saving no
            end try
        end if
    end tell
end try
APPLESCRIPT
}

stop_existing_rsnap() {
  if ! pgrep -x rsnap >/dev/null 2>&1; then
    return
  fi

  echo "[smoke] stopping existing rsnap processes" >&2
  pkill -x rsnap >/dev/null 2>&1 || true

  local deadline=$((SECONDS + 10))
  while pgrep -x rsnap >/dev/null 2>&1; do
    if (( SECONDS > deadline )); then
      fail "existing rsnap process did not stop"
    fi
    sleep 0.2
  done
}

cleanup() {
  if [[ -n "$RSNAP_PID" ]] && kill -0 "$RSNAP_PID" >/dev/null 2>&1; then
    kill "$RSNAP_PID" >/dev/null 2>&1 || true
    wait "$RSNAP_PID" >/dev/null 2>&1 || true
  fi

  close_textedit_windows
  rm -f "$FIXTURE_FILE" "$SWIFT_HELPER"
}
trap cleanup EXIT

fail() {
  echo "[smoke] $*" >&2
  if [[ -n "$RSNAP_LOG" && -f "$RSNAP_LOG" ]]; then
    echo "[smoke] recent rsnap log excerpt:" >&2
    tail -n 80 "$RSNAP_LOG" >&2 || true
  fi
  exit 1
}

refresh_log_path() {
  RSNAP_LOG="$(ls -1t "$LOG_DIR"/rsnap*.log 2>/dev/null | head -n 1 || true)"
}

wait_for_pattern() {
  local pattern="$1"
  local timeout_s="$2"
  local deadline=$((SECONDS + timeout_s))

  while (( SECONDS <= deadline )); do
    refresh_log_path
    if [[ -n "$RSNAP_LOG" && -f "$RSNAP_LOG" ]] && rg -q "$pattern" "$RSNAP_LOG"; then
      return 0
    fi
    sleep 0.25
  done

  return 1
}

configure_scrollbar_verification() {
  case "$VERIFY_SCROLLBAR" in
    1|true|yes|on)
      SCROLLBAR_VERIFICATION_ACTIVE=1
      ;;
    0|false|no|off)
      SCROLLBAR_VERIFICATION_ACTIVE=0
      ;;
    auto|"")
      SCROLLBAR_VERIFICATION_ACTIVE=0
      SCROLLBAR_AUTO_PROBE_PENDING=1
      ;;
    *)
      fail "invalid VERIFY_SCROLLBAR value: $VERIFY_SCROLLBAR"
      ;;
  esac
}

maybe_capture_scrollbar_value() {
  local value=""

  if (( SCROLLBAR_VERIFICATION_ACTIVE )); then
    value="$(read_textedit_scrollbar_value)"
    printf '%s\n' "$value"
    return 0
  fi

  if (( SCROLLBAR_AUTO_PROBE_PENDING )); then
    SCROLLBAR_AUTO_PROBE_PENDING=0
    if ! value="$(read_textedit_scrollbar_value 2>/dev/null)"; then
      return 0
    fi

    SCROLLBAR_VERIFICATION_ACTIVE=1
    printf '%s\n' "$value"
    return 0
  fi

  return 0
}

scrollbar_value_increased() {
  local initial="$1"
  local final="$2"
  python3 - "$initial" "$final" <<'PY'
import sys

initial = float(sys.argv[1])
final = float(sys.argv[2])
sys.exit(0 if final > initial else 1)
PY
}

write_fixture() {
  python3 - "$FIXTURE_FILE" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
with path.open("w") as handle:
    for i in range(1, 701):
        handle.write(f"Line {i:03d} -- rsnap scroll capture smoke fixture with deterministic text.\n")
PY
}

open_fixture_in_textedit() {
  local bounds="$1"
  local left top right bottom
  IFS=, read -r left top right bottom <<<"$bounds"
  close_textedit_windows
  open -a TextEdit "$FIXTURE_FILE"
  osascript <<APPLESCRIPT
tell application "TextEdit"
    activate
    delay 0.8
    try
        set bounds of front window to {$left, $top, $right, $bottom}
    end try
end tell
APPLESCRIPT
}

read_textedit_front_window_bounds() {
  osascript <<'APPLESCRIPT'
tell application "TextEdit"
    activate
    delay 0.4
    return bounds of front window
end tell
APPLESCRIPT
}

read_textedit_scrollbar_value() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    tell process "TextEdit"
        set targetArea to scroll area 1 of front window
        repeat with targetBar in (scroll bars of targetArea)
            if (orientation of targetBar) is equal to "AXVerticalOrientation" then
                return value of attribute "AXValue" of targetBar
            end if
        end repeat
        error "vertical scrollbar not found"
    end tell
end tell
APPLESCRIPT
}

derive_capture_points_from_bounds() {
  local bounds="$1"
  python3 - "$bounds" <<'PY'
import sys

left, top, right, bottom = map(int, sys.argv[1].replace(" ", "").split(","))
width = right - left
height = bottom - top

if width < 220 or height < 260:
    raise SystemExit("TextEdit window too small for smoke capture")

x_inset = max(72, min(140, width // 8))
top_inset = max(110, min(170, height // 5))
bottom_inset = max(72, min(130, height // 7))

start_x = left + x_inset
start_y = top + top_inset
end_x = right - x_inset
end_y = bottom - bottom_inset

if end_x <= start_x + 80 or end_y <= start_y + 80:
    raise SystemExit("Derived capture rect is too small")

scroll_x = (start_x + end_x) // 2
scroll_y = min(start_y + 48, end_y - 48)

print(f"drag_start={start_x},{start_y}")
print(f"drag_end={end_x},{end_y}")
print(f"scroll_point={scroll_x},{scroll_y}")
PY
}

resolve_capture_points() {
  local bounds="$1"
  local derived
  derived="$(derive_capture_points_from_bounds "$bounds")"

  local derived_drag_start="" derived_drag_end="" derived_scroll_point=""
  while IFS='=' read -r key value; do
    case "$key" in
      drag_start) derived_drag_start="$value" ;;
      drag_end) derived_drag_end="$value" ;;
      scroll_point) derived_scroll_point="$value" ;;
    esac
  done <<<"$derived"

  if [[ -z "$DRAG_START" ]]; then
    DRAG_START="$derived_drag_start"
  fi
  if [[ -z "$DRAG_END" ]]; then
    DRAG_END="$derived_drag_end"
  fi
  if [[ -z "$SCROLL_POINT" ]]; then
    SCROLL_POINT="$derived_scroll_point"
  fi
}

trigger_capture_from_tray_menu() {
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

focus_rsnap_frozen_ui() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    tell process "rsnap"
        set frontmost to true
    end tell
end tell
delay 0.15
APPLESCRIPT
}

press_s() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    key code 1
end tell
APPLESCRIPT
}

cat > "$SWIFT_HELPER" <<'SWIFT'
import Cocoa
import ApplicationServices

func readPoint(_ key: String) -> CGPoint {
    let value = ProcessInfo.processInfo.environment[key] ?? ""
    let parts = value.split(separator: ",")
    guard parts.count == 2,
          let x = Double(parts[0]),
          let y = Double(parts[1]) else {
        fputs("invalid point env for \(key): \(value)\n", stderr)
        exit(2)
    }
    return CGPoint(x: x, y: y)
}

func readInt(_ key: String) -> Int {
    guard let raw = ProcessInfo.processInfo.environment[key], let value = Int(raw) else {
        fputs("invalid int env for \(key)\n", stderr)
        exit(2)
    }
    return value
}

func sleepMs(_ ms: useconds_t) { usleep(ms * 1000) }

func mouseEvent(_ type: CGEventType, at point: CGPoint, button: CGMouseButton = .left) {
    let src = CGEventSource(stateID: .hidSystemState)
    let event = CGEvent(mouseEventSource: src, mouseType: type, mouseCursorPosition: point, mouseButton: button)
    event?.post(tap: .cghidEventTap)
}

func drag(from start: CGPoint, to end: CGPoint, steps: Int) {
    mouseEvent(.mouseMoved, at: start)
    sleepMs(120)
    mouseEvent(.leftMouseDown, at: start)
    sleepMs(120)
    for step in 1...steps {
        let t = CGFloat(step) / CGFloat(steps)
        let point = CGPoint(x: start.x + (end.x - start.x) * t, y: start.y + (end.y - start.y) * t)
        mouseEvent(.leftMouseDragged, at: point)
        sleepMs(25)
    }
    mouseEvent(.leftMouseUp, at: end)
}

func scroll(at point: CGPoint, deltaY: Int32, times: Int) {
    mouseEvent(.mouseMoved, at: point)
    sleepMs(120)
    for _ in 0..<times {
        let src = CGEventSource(stateID: .hidSystemState)
        let event = CGEvent(scrollWheelEvent2Source: src, units: .pixel, wheelCount: 1, wheel1: deltaY, wheel2: 0, wheel3: 0)
        event?.location = point
        event?.post(tap: .cghidEventTap)
        sleepMs(110)
    }
}

let mode = ProcessInfo.processInfo.environment["MODE"] ?? ""
switch mode {
case "drag":
    drag(from: readPoint("START_POINT"), to: readPoint("END_POINT"), steps: max(10, readInt("DRAG_STEPS")))
case "scroll":
    scroll(at: readPoint("SCROLL_POINT"), deltaY: Int32(readInt("SCROLL_DELTA")), times: max(1, readInt("SCROLL_EVENTS")))
default:
    fputs("unsupported MODE\n", stderr)
    exit(2)
}
SWIFT

launch_rsnap() {
  (
    cd "$ROOT_DIR"
    exec zsh -lc "$RSNAP_CMD"
  ) >/tmp/rsnap-scroll-capture-smoke-rsnap.out 2>&1 &
  RSNAP_PID=$!
}

mkdir -p "$LOG_DIR"
stop_existing_rsnap
rm -f "$LOG_DIR"/rsnap*.log
write_fixture
launch_rsnap
configure_scrollbar_verification
wait_for_pattern 'Starting rsnap\.' "$WAIT_STARTUP_S" || fail "rsnap did not log startup"
open_fixture_in_textedit "$TEXTEDIT_BOUNDS"
ACTUAL_TEXTEDIT_BOUNDS="$(read_textedit_front_window_bounds | tr -d ' ')"
resolve_capture_points "$ACTUAL_TEXTEDIT_BOUNDS"
INITIAL_SCROLLBAR_VALUE="$(maybe_capture_scrollbar_value | tr -d ' ')"
echo "[smoke] textedit bounds: $ACTUAL_TEXTEDIT_BOUNDS"
echo "[smoke] drag start: $DRAG_START"
echo "[smoke] drag end: $DRAG_END"
echo "[smoke] scroll point: $SCROLL_POINT"
if (( SCROLLBAR_VERIFICATION_ACTIVE )); then
  echo "[smoke] initial scrollbar value: $INITIAL_SCROLLBAR_VALUE"
else
  echo "[smoke] scrollbar verification: skipped"
fi
trigger_capture_from_tray_menu
wait_for_pattern 'Capture overlay started\.' "$WAIT_OVERLAY_S" || fail "capture overlay did not start"
sleep "$OVERLAY_SETTLE_S"
MODE=drag START_POINT="$DRAG_START" END_POINT="$DRAG_END" DRAG_STEPS=28 swift "$SWIFT_HELPER"
sleep "$DRAG_SETTLE_S"
focus_rsnap_frozen_ui
press_s
wait_for_pattern 'op="scroll_capture.start"' "$WAIT_SCROLL_CAPTURE_S" || fail "scroll capture mode did not start"
sleep "$SCROLL_MODE_SETTLE_S"
MODE=scroll SCROLL_POINT="$SCROLL_POINT" SCROLL_DELTA="$SCROLL_DELTA" SCROLL_EVENTS="$SCROLL_EVENTS" swift "$SWIFT_HELPER"
FINAL_SCROLLBAR_VALUE="$(maybe_capture_scrollbar_value | tr -d ' ')"
if (( SCROLLBAR_VERIFICATION_ACTIVE )); then
  echo "[smoke] final scrollbar value: $FINAL_SCROLLBAR_VALUE"
  scrollbar_value_increased "$INITIAL_SCROLLBAR_VALUE" "$FINAL_SCROLLBAR_VALUE" \
    || fail "synthetic scroll input did not move the TextEdit vertical scrollbar"
fi
wait_for_pattern 'op="scroll_capture.appended"' "$WAIT_APPEND_S" || fail "scroll capture did not append any rows"

echo "[smoke] PASS"
if [[ -n "$RSNAP_LOG" ]]; then
  rg -n 'Starting rsnap\.|Capture overlay started\.|op="scroll_capture.start"|op="scroll_capture.appended"' "$RSNAP_LOG"
fi
press_escape >/dev/null 2>&1 || true
