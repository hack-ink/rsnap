#!/usr/bin/env bash
set -euo pipefail

# Deterministic macOS live/loupe performance smoke for rsnap.
#
# This harness launches rsnap in release mode, enters live capture from the
# tray Capture menu, holds Alt while moving the cursor along a deterministic
# path, then parses the latest rsnap log for concrete live-path performance
# signals.

usage() {
  cat <<'EOF'
Usage: live-loupe-perf-smoke-macos.sh [--self-check] [--help]

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

self_check() {
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
SWIFT_HELPER_BASE="$(mktemp -t rsnap-live-loupe-perf-smoke-swift)"
SWIFT_HELPER="${SWIFT_HELPER_BASE}.swift"
mv "$SWIFT_HELPER_BASE" "$SWIFT_HELPER"

fail() {
  echo "[smoke] $*" >&2
  if [[ -n "$RSNAP_LOG" && -f "$RSNAP_LOG" ]]; then
    echo "[smoke] recent rsnap log excerpt:" >&2
    tail -n 120 "$RSNAP_LOG" >&2 || true
  fi
  exit 1
}

release_option_key() {
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
  release_option_key

  if [[ -n "$RSNAP_PID" ]] && kill -0 "$RSNAP_PID" >/dev/null 2>&1; then
    kill "$RSNAP_PID" >/dev/null 2>&1 || true
    wait "$RSNAP_PID" >/dev/null 2>&1 || true
  fi

  rm -f "$SWIFT_HELPER"
}
trap cleanup EXIT

refresh_log_path() {
  RSNAP_LOG="$(ls -1t "$LOG_DIR"/rsnap*.log 2>/dev/null | head -n 1 || true)"
}

capture_session_end_ts() {
  SESSION_END_TS="$(python3 - <<'PY'
from datetime import datetime, timezone

print(datetime.now(timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z"))
PY
)"
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

read_main_display_bounds() {
  osascript <<'APPLESCRIPT'
tell application "Finder"
    return bounds of window of desktop
end tell
APPLESCRIPT
}

derive_live_path_points() {
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

focus_rsnap_overlay() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    tell process "rsnap"
        set frontmost to true
    end tell
end tell
delay 0.15
APPLESCRIPT
}

hold_option_key() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    key down option
end tell
APPLESCRIPT
  OPTION_HELD=1
}

press_escape() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    key code 53
end tell
APPLESCRIPT
}

cat > "$SWIFT_HELPER" <<'SWIFT'
import Cocoa
import ApplicationServices

func readInt(_ key: String, default value: Int? = nil) -> Int {
    if let raw = ProcessInfo.processInfo.environment[key], let parsed = Int(raw) {
        return parsed
    }
    if let value {
        return value
    }
    fputs("invalid int env for \(key)\n", stderr)
    exit(2)
}

func readPoints(_ key: String) -> [CGPoint] {
    let raw = ProcessInfo.processInfo.environment[key] ?? ""
    let points = raw.split(separator: ";").compactMap { item -> CGPoint? in
        let parts = item.split(separator: ",")
        guard parts.count == 2,
              let x = Double(parts[0]),
              let y = Double(parts[1]) else {
            return nil
        }
        return CGPoint(x: x, y: y)
    }
    guard points.count >= 2 else {
        fputs("invalid points env for \(key): \(raw)\n", stderr)
        exit(2)
    }
    return points
}

func sleepMs(_ ms: useconds_t) { usleep(ms * 1000) }

func mouseEvent(_ type: CGEventType, at point: CGPoint) {
    let src = CGEventSource(stateID: .hidSystemState)
    let event = CGEvent(mouseEventSource: src, mouseType: type, mouseCursorPosition: point, mouseButton: .left)
    event?.post(tap: .cghidEventTap)
}

func moveAlong(points: [CGPoint], stepsPerSegment: Int, delayMs: useconds_t, cycles: Int) {
    mouseEvent(.mouseMoved, at: points[0])
    sleepMs(120)

    for _ in 0..<max(1, cycles) {
        for (start, end) in zip(points, points.dropFirst()) {
            for step in 1...max(1, stepsPerSegment) {
                let t = CGFloat(step) / CGFloat(max(1, stepsPerSegment))
                let point = CGPoint(
                    x: start.x + (end.x - start.x) * t,
                    y: start.y + (end.y - start.y) * t
                )
                mouseEvent(.mouseMoved, at: point)
                sleepMs(delayMs)
            }
        }
    }
}

let points = readPoints("PATH_POINTS")
moveAlong(
    points: points,
    stepsPerSegment: readInt("PATH_SEGMENT_STEPS", default: 18),
    delayMs: useconds_t(readInt("PATH_STEP_DELAY_MS", default: 10)),
    cycles: readInt("PATH_CYCLES", default: 2)
)
SWIFT

launch_rsnap() {
  (
    cd "$ROOT_DIR"
    export RUST_LOG="$RSNAP_RUST_LOG"
    exec zsh -lc "$RSNAP_CMD"
  ) >/tmp/rsnap-live-loupe-perf-smoke-rsnap.out 2>&1 &
  RSNAP_PID=$!
}

summarize_and_gate_log() {
  local log_path="$1"
  python3 - "$log_path" \
    "$SESSION_END_TS" \
    "$MAX_ACQUIRE_FRAME_WARNS" \
    "$MAX_EVENT_LOOP_STALLS" \
    "$MAX_RESIZED_EVENTS" \
    "$MAX_SLOW_OP_WARNINGS" \
    "$MAX_LIVE_SAMPLE_APPLY_LATENCY_WARNS" \
    "$MAX_LIVE_SAMPLE_APPLY_LATENCY_MS" <<'PY'
import re
import sys
from collections import Counter

(
    log_path,
    session_end_ts,
    max_acquire,
    max_stalls,
    max_resized,
    max_slow,
    max_live_apply_warns,
    max_live_apply_ms,
) = sys.argv[1:]
max_acquire = int(max_acquire)
max_stalls = int(max_stalls)
max_resized = int(max_resized)
max_slow = int(max_slow)
max_live_apply_warns = int(max_live_apply_warns) if max_live_apply_warns else None
max_live_apply_ms = int(max_live_apply_ms) if max_live_apply_ms else None

metrics = {
    "acquire_frame_warns": 0,
    "event_loop_stalls": 0,
    "resized_events": 0,
    "slow_op_warnings": 0,
    "live_sample_apply_latency_warns": 0,
    "max_acquire_ms": 0,
    "max_stall_ms": 0,
    "max_live_sample_apply_latency_ms": 0,
}
resize_sizes = Counter()
slow_ops = Counter()
redraw_breakdown_counts = Counter()
redraw_breakdown_max_ms = {}

redraw_breakdown_ops = {
    "overlay.hud_redraw.total",
    "overlay.hud_redraw.renderer_draw",
    "overlay.hud_redraw.request_inner_size",
    "overlay.hud_redraw.position_update",
    "overlay.loupe_redraw.tile_draw",
    "overlay.loupe_redraw.request_inner_size",
    "overlay.loupe_redraw.reposition",
    "overlay.hud_window_set_outer_position",
    "overlay.loupe_window_set_outer_position",
}

re_elapsed = re.compile(r'elapsed_ms=(\d+)')
re_stall = re.compile(r'stall_ms=(\d+)')
re_latency = re.compile(r'latency_ms=(\d+)')
re_resize = re.compile(r'PhysicalSize \{ width: (\d+), height: (\d+) \}')
re_op = re.compile(r'op="([^"]+)"')
session_start_idx = None
session_end_idx = None
end_markers = (
    "Capture cancelled.",
    "Capture copied to clipboard.",
    "Capture saved to file.",
    "Capture failed.",
    "Capture overlay ended.",
)

with open(log_path, "r", encoding="utf-8", errors="replace") as handle:
    lines = handle.readlines()

for idx, line in enumerate(lines):
    if session_start_idx is None and "Capture overlay started." in line:
        session_start_idx = idx
        continue
    if session_start_idx is not None and any(marker in line for marker in end_markers):
        session_end_idx = idx
        break

if session_start_idx is None:
    print("[smoke] FAIL missing live session start marker", file=sys.stderr)
    sys.exit(1)

window = []
for line in lines[session_start_idx:session_end_idx]:
    if not session_end_ts:
        window.append(line)
        continue

    timestamp = line.split(" ", 1)[0]
    if timestamp > session_end_ts:
        break
    window.append(line)

if not window:
    window = lines[session_start_idx:]

for line in window:
    if 'op="overlay.window_renderer_acquire_frame"' in line:
        metrics["acquire_frame_warns"] += 1
        match = re_elapsed.search(line)
        if match:
            metrics["max_acquire_ms"] = max(metrics["max_acquire_ms"], int(match.group(1)))
    if 'op="overlay.event_loop_stall"' in line:
        metrics["event_loop_stalls"] += 1
        match = re_stall.search(line)
        if match:
            metrics["max_stall_ms"] = max(metrics["max_stall_ms"], int(match.group(1)))
    if 'op="overlay.live_sample_apply_latency"' in line:
        metrics["live_sample_apply_latency_warns"] += 1
        match = re_latency.search(line)
        if match:
            metrics["max_live_sample_apply_latency_ms"] = max(
                metrics["max_live_sample_apply_latency_ms"],
                int(match.group(1)),
            )
    if "WindowEvent::Resized" in line:
        metrics["resized_events"] += 1
        match = re_resize.search(line)
        if match:
            resize_sizes[f'{match.group(1)}x{match.group(2)}'] += 1
    if "Slow operation detected" in line:
        metrics["slow_op_warnings"] += 1
        match = re_op.search(line)
        if match:
            op = match.group(1)
            slow_ops[op] += 1
            if op in redraw_breakdown_ops:
                redraw_breakdown_counts[op] += 1
                elapsed_match = re_elapsed.search(line)
                if elapsed_match:
                    redraw_breakdown_max_ms[op] = max(
                        redraw_breakdown_max_ms.get(op, 0),
                        int(elapsed_match.group(1)),
                    )

top_sizes = ",".join(f"{size}:{count}" for size, count in resize_sizes.most_common(5)) or "none"
top_slow_ops = ",".join(f"{op}:{count}" for op, count in slow_ops.most_common(5)) or "none"
redraw_breakdown = ",".join(
    f"{op}:max_ms={redraw_breakdown_max_ms.get(op, 0)}:count={redraw_breakdown_counts[op]}"
    for op in sorted(
        redraw_breakdown_counts,
        key=lambda op: (redraw_breakdown_max_ms.get(op, 0), redraw_breakdown_counts[op], op),
        reverse=True,
    )[:10]
) or "none"
session_end_label = (
    "script_path_end"
    if session_end_idx is None or (
        session_end_ts
        and lines[session_end_idx].split(" ", 1)[0] > session_end_ts
    )
    else end_markers[next(i for i, marker in enumerate(end_markers) if marker in lines[session_end_idx])]
)

print(
    "[smoke] metrics "
    f'session_lines={len(window)} '
    f'session_end="{session_end_label}" '
    f'acquire_frame_warns={metrics["acquire_frame_warns"]} '
    f'max_acquire_ms={metrics["max_acquire_ms"]} '
    f'event_loop_stalls={metrics["event_loop_stalls"]} '
    f'max_stall_ms={metrics["max_stall_ms"]} '
    f'resized_events={metrics["resized_events"]} '
    f'slow_op_warnings={metrics["slow_op_warnings"]} '
    f'live_sample_apply_latency_warns={metrics["live_sample_apply_latency_warns"]} '
    f'max_live_sample_apply_latency_ms={metrics["max_live_sample_apply_latency_ms"]}'
)
print(f"[smoke] top_resize_sizes {top_sizes}")
print(f"[smoke] top_slow_ops {top_slow_ops}")
print(f"[smoke] redraw_breakdown {redraw_breakdown}")

failures = []
if metrics["acquire_frame_warns"] > max_acquire:
    failures.append(
        f'acquire_frame_warns={metrics["acquire_frame_warns"]} exceeds {max_acquire}'
    )
if metrics["event_loop_stalls"] > max_stalls:
    failures.append(
        f'event_loop_stalls={metrics["event_loop_stalls"]} exceeds {max_stalls}'
    )
if metrics["resized_events"] > max_resized:
    failures.append(f'resized_events={metrics["resized_events"]} exceeds {max_resized}')
if metrics["slow_op_warnings"] > max_slow:
    failures.append(
        f'slow_op_warnings={metrics["slow_op_warnings"]} exceeds {max_slow}'
    )
if (
    max_live_apply_warns is not None
    and metrics["live_sample_apply_latency_warns"] > max_live_apply_warns
):
    failures.append(
        "live_sample_apply_latency_warns="
        f'{metrics["live_sample_apply_latency_warns"]} exceeds {max_live_apply_warns}'
    )
if (
    max_live_apply_ms is not None
    and metrics["max_live_sample_apply_latency_ms"] > max_live_apply_ms
):
    failures.append(
        "max_live_sample_apply_latency_ms="
        f'{metrics["max_live_sample_apply_latency_ms"]} exceeds {max_live_apply_ms}'
    )

if failures:
    for item in failures:
        print(f"[smoke] FAIL {item}", file=sys.stderr)
    sys.exit(1)
PY
}

mkdir -p "$LOG_DIR"
stop_existing_rsnap
rm -f "$LOG_DIR"/rsnap*.log
launch_rsnap
wait_for_pattern 'Starting rsnap\.' "$WAIT_STARTUP_S" || fail "rsnap did not log startup"
trigger_capture_from_tray_menu
wait_for_pattern 'Capture overlay started\.' "$WAIT_OVERLAY_S" || fail "capture overlay did not start"
focus_rsnap_overlay

if [[ -z "$DISPLAY_BOUNDS" ]]; then
  DISPLAY_BOUNDS="$(read_main_display_bounds | tr -d ' ')"
fi
if [[ -z "$PATH_POINTS" ]]; then
  PATH_POINTS="$(derive_live_path_points "$DISPLAY_BOUNDS")"
fi

echo "[smoke] display bounds: $DISPLAY_BOUNDS"
echo "[smoke] path points: $PATH_POINTS"

sleep "$OVERLAY_SETTLE_S"
hold_option_key
PATH_POINTS="$PATH_POINTS" \
PATH_SEGMENT_STEPS="$PATH_SEGMENT_STEPS" \
PATH_STEP_DELAY_MS="$PATH_STEP_DELAY_MS" \
PATH_CYCLES="$PATH_CYCLES" \
swift "$SWIFT_HELPER"
release_option_key
capture_session_end_ts
sleep "$POST_PATH_SETTLE_S"
press_escape >/dev/null 2>&1 || true
sleep 0.2

refresh_log_path
if [[ -z "$RSNAP_LOG" || ! -f "$RSNAP_LOG" ]]; then
  fail "could not locate rsnap log"
fi

summarize_and_gate_log "$RSNAP_LOG" || fail "live perf metrics exceeded thresholds"

echo "[smoke] PASS"
rg -n 'Starting rsnap\.|Capture requested from tray menu\.|Capture overlay started\.|op="overlay.window_renderer_acquire_frame"|op="overlay.event_loop_stall"|WindowEvent::Resized|op="overlay\.hud_redraw|op="overlay\.loupe_redraw|op="overlay\.hud_window_set_outer_position"|op="overlay\.loupe_window_set_outer_position"|Slow operation detected' "$RSNAP_LOG" | tail -n 120 || true
