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

  for cmd in caffeinate osascript swift swiftc python3; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "missing required tool: $cmd" >&2
      return 1
    fi
  done

  echo "[smoke] self-check ok"
}

live_hud_start_awake_assertion() {
  LIVE_HUD_CAFFEINATE_PID=""
  caffeinate -u -t "${RSNAP_DISPLAY_WAKE_SECONDS:-5}" >/dev/null 2>&1 || true
  caffeinate -dimsu -w "$$" >/dev/null 2>&1 &
  LIVE_HUD_CAFFEINATE_PID="$!"
}

live_hud_stop_awake_assertion() {
  if [[ -n "${LIVE_HUD_CAFFEINATE_PID:-}" ]]; then
    kill "$LIVE_HUD_CAFFEINATE_PID" >/dev/null 2>&1 || true
    wait "$LIVE_HUD_CAFFEINATE_PID" >/dev/null 2>&1 || true
    LIVE_HUD_CAFFEINATE_PID=""
  fi
}

live_hud_assert_interactive_session() {
  swift - <<'SWIFT'
import CoreGraphics
import Foundation

func boolValue(_ value: Any?) -> Bool {
	if let bool = value as? Bool {
		return bool
	}
	if let number = value as? NSNumber {
		return number.boolValue
	}
	if let int = value as? Int {
		return int != 0
	}
	return false
}

guard let session = CGSessionCopyCurrentDictionary() as? [String: Any] else {
	fputs("[smoke] FAIL could not read CG session state\n", stderr)
	exit(1)
}

if boolValue(session["CGSSessionScreenIsLocked"]) {
	fputs("[smoke] FAIL macOS session is locked; unlock the display before running native smoke\n", stderr)
	exit(1)
}
SWIFT
}

live_hud_assert_shareable_display() {
  swift - <<'SWIFT'
import ScreenCaptureKit

do {
	let content = try await SCShareableContent.current
	guard content.displays.isEmpty == false else {
		fputs("[smoke] FAIL ScreenCaptureKit returned no displays; check unlocked display and Screen Recording permission\n", stderr)
		exit(1)
	}
} catch {
	fputs("[smoke] FAIL ScreenCaptureKit shareable content unavailable: \(error)\n", stderr)
	exit(1)
}
SWIFT
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
    if exists process "RsnapNativeHost" then
        tell process "RsnapNativeHost"
            set frontmost to true
        end tell
    else
        tell process "Rsnap"
            set frontmost to true
        end tell
    end if
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

live_hud_capture_window_count() {
  osascript <<'APPLESCRIPT' 2>/dev/null || true
tell application "System Events"
    set captureWindowCount to 0
    if exists process "RsnapNativeHost" then
        tell process "RsnapNativeHost"
            repeat with captureWindow in windows
                try
                    set windowSize to size of captureWindow
                    if (item 1 of windowSize) > 32 or (item 2 of windowSize) > 32 then
                        set captureWindowCount to captureWindowCount + 1
                    end if
                end try
            end repeat
        end tell
    else if exists process "Rsnap" then
        tell process "Rsnap"
            repeat with captureWindow in windows
                try
                    set windowSize to size of captureWindow
                    if (item 1 of windowSize) > 32 or (item 2 of windowSize) > 32 then
                        set captureWindowCount to captureWindowCount + 1
                    end if
                end try
            end repeat
        end tell
    end if
    return captureWindowCount
end tell
APPLESCRIPT
}

live_hud_cancel_capture_if_present() {
  if [[ "${RSNAP_SMOKE_CANCEL_ON_EXIT:-1}" != "1" ]]; then
    return 0
  fi
  local attempt window_count
  for attempt in {1..5}; do
    window_count="$(live_hud_capture_window_count | tr -d '[:space:]')"
    if [[ -z "$window_count" || "$window_count" == "0" ]]; then
      return 0
    fi
    osascript <<'APPLESCRIPT' >/dev/null 2>&1 || true
tell application "System Events"
    if exists process "RsnapNativeHost" then
        tell process "RsnapNativeHost"
            set frontmost to true
        end tell
        delay 0.04
        key code 53
        delay 0.04
        key code 53
    else if exists process "Rsnap" then
        tell process "Rsnap"
            set frontmost to true
        end tell
        delay 0.04
        key code 53
        delay 0.04
        key code 53
    end if
end tell
APPLESCRIPT
    sleep 0.12
  done
  window_count="$(live_hud_capture_window_count | tr -d '[:space:]')"
  if [[ -n "$window_count" && "$window_count" != "0" ]]; then
    echo "[smoke] WARN capture cleanup still sees $window_count Rsnap window(s)" >&2
  fi
}

live_hud_press_tab() {
  osascript <<'APPLESCRIPT'
tell application "System Events"
    key code 48
end tell
APPLESCRIPT
}

live_hud_start_new_capture() {
  osascript <<'APPLESCRIPT' >/dev/null
tell application "System Events"
    if exists process "RsnapNativeHost" then
        tell process "RsnapNativeHost"
            click menu bar item 1 of menu bar 1
            delay 0.06
            click menu item "New Screenshot" of menu 1 of menu bar item 1 of menu bar 1
        end tell
    else
        tell process "Rsnap"
            click menu bar item 1 of menu bar 1
            delay 0.06
            click menu item "New Screenshot" of menu 1 of menu bar item 1 of menu bar 1
        end tell
    end if
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
  PATH_HOLD_BEFORE_RELEASE_MS="${PATH_HOLD_BEFORE_RELEASE_MS:-}" \
  swift "$(live_hud_cursor_helper)"
}

live_hud_release_primary_button() {
  PATH_POINTS="$PATH_POINTS" \
  PATH_MODE="release-primary" \
  PATH_DRIVER="$PATH_DRIVER" \
  swift "$(live_hud_cursor_helper)"
}
