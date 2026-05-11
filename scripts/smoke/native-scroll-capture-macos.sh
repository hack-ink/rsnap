#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/smoke/lib/live-hud.sh
source "$SCRIPT_DIR/lib/live-hud.sh"

usage() {
	cat <<'EOF'
Usage: native-scroll-capture-macos.sh [--self-check] [--help]

Runs a real macOS scroll-capture smoke:
  1. start a deterministic scrollable native window
  2. build and launch Rsnap
  3. drag-freeze a region, start Scroll Capture, and scroll the region
  4. copy the stitched result and require multiple committed growth events

Useful overrides:
  SCROLL_COUNT=14
  SCROLL_DRIVER=wheel                  optional: wheel,notification
  SCROLL_START_METHOD=keyboard         optional: keyboard,toolbar
  SCROLL_DELTA_Y=36
  SCROLL_INTERVAL_MS=220
  MIN_SCROLL_COMMITS=3
  MIN_EXPORT_GROWTH_PX=180
  APP_POST_VERIFY_SETTLE_S=0
EOF
}

case "${1:-}" in
	--help|-h)
		usage
		exit 0
		;;
	--self-check)
		live_hud_self_check
		exit $?
		;;
	"")
		;;
	*)
		usage >&2
		exit 2
		;;
esac

live_hud_self_check
live_hud_start_awake_assertion
live_hud_assert_interactive_session
live_hud_assert_shareable_display
ROOT_DIR="$(live_hud_repo_root)"
live_hud_init_environment "$ROOT_DIR"

smoke_log() {
	printf '[smoke] +%ss %s\n' "$SECONDS" "$*"
}

PREF_DOMAIN="${RSNAP_PREF_DOMAIN:-ink.hack.rsnap}"
PREF_SNAPSHOT="$(mktemp "${TMPDIR:-/tmp}/rsnap-scroll-prefs.XXXXXX.plist")"
PREF_SNAPSHOT_EXISTS=0
SCROLL_BACKGROUND_PID=""
SCROLL_BACKGROUND_READY="$(mktemp "${TMPDIR:-/tmp}/rsnap-scroll-bg.XXXXXX.ready")"
SCROLL_COUNT="${SCROLL_COUNT:-14}"
SCROLL_DRIVER="${SCROLL_DRIVER:-wheel}"
SCROLL_START_METHOD="${SCROLL_START_METHOD:-keyboard}"
SCROLL_DELTA_Y="${SCROLL_DELTA_Y:-36}"
SCROLL_INTERVAL_MS="${SCROLL_INTERVAL_MS:-220}"
MIN_SCROLL_COMMITS="${MIN_SCROLL_COMMITS:-3}"
MIN_EXPORT_GROWTH_PX="${MIN_EXPORT_GROWTH_PX:-180}"
APP_POST_VERIFY_SETTLE_S="${APP_POST_VERIFY_SETTLE_S:-0}"
OVERLAY_SETTLE_S="${OVERLAY_SETTLE_S:-0.10}"
POST_FREEZE_SETTLE_S="${POST_FREEZE_SETTLE_S:-0.16}"
POST_COPY_SETTLE_S="${POST_COPY_SETTLE_S:-0.30}"
PATH_CYCLES="${PATH_CYCLES:-1}"
if [[ -z "${POST_SCROLL_SETTLE_S:-}" ]]; then
	POST_SCROLL_SETTLE_S=2.20
fi

restore_preferences() {
	live_hud_stop_awake_assertion
	live_hud_cancel_capture_if_present
	if [[ -n "$SCROLL_BACKGROUND_PID" ]]; then
		kill "$SCROLL_BACKGROUND_PID" >/dev/null 2>&1 || true
	fi
	if [[ "$PREF_SNAPSHOT_EXISTS" == "1" ]]; then
		defaults import "$PREF_DOMAIN" "$PREF_SNAPSHOT" >/dev/null 2>&1 || true
	else
		for key in captureHotkey hudGlassEnabled hudGlassMode liquidGlassStyle toolbarPlacement loupeSampleSize scrollCaptureAutoScrollEnabled; do
			defaults delete "$PREF_DOMAIN" "$key" >/dev/null 2>&1 || true
		done
	fi
	rm -f "$PREF_SNAPSHOT" "$SCROLL_BACKGROUND_READY"
}

if defaults export "$PREF_DOMAIN" "$PREF_SNAPSHOT" >/dev/null 2>&1; then
	PREF_SNAPSHOT_EXISTS=1
fi
trap restore_preferences EXIT

wait_ready_file() {
	local path="$1"
	local attempt

	for attempt in {1..80}; do
		if grep -q '^ready$' "$path" >/dev/null 2>&1; then
			return 0
		fi
		sleep 0.05
	done
	return 1
}

assert_scroll_background_moved() {
	local last_offset

	last_offset="$(
		awk -F= '/^offsetY=/ { value=$2 } END { if (value != "") print value }' "$SCROLL_BACKGROUND_READY"
	)"
	if [[ -z "$last_offset" ]]; then
		echo "[smoke] FAIL scroll background did not report any movement" >&2
		return 1
	fi
	smoke_log "scroll background offsetY=$last_offset"
	python3 - "$last_offset" <<'PY'
import sys

offset = float(sys.argv[1])
if offset <= 0:
    raise SystemExit("[smoke] FAIL scroll background offset did not advance")
PY
}

click_scroll_toolbar_icon() {
	local point

	point="$(
		python3 - "$DRAG_POINTS" "$DISPLAY_BOUNDS" <<'PY'
import os
import sys

drag_points, display_bounds = sys.argv[1:3]
start_raw, end_raw = drag_points.split(";")
x1, y1 = map(float, start_raw.split(","))
x2, y2 = map(float, end_raw.split(","))
left, top, right, bottom = map(float, display_bounds.replace(" ", "").split(","))

selection_min_x = min(x1, x2)
selection_max_x = max(x1, x2)
# The mouse helper posts Quartz-style display coordinates, while AppKit lays out
# the toolbar in bottom-left screen coordinates.
selection_min_y = bottom - max(y1, y2)
selection_max_y = bottom - min(y1, y2)
selection_mid_x = (selection_min_x + selection_max_x) / 2

scale = min(1.0, 30.0 / ((5.0 * 2.0) + 24.0))
button_size = 24.0 * scale
item_spacing = 4.0 * scale
horizontal_padding = 12.0 * scale
vertical_padding = 5.0 * scale
gap = 10.0 * scale
screen_margin = 10.0

# Current frozen toolbar order has scroll after auto-center. The 12.5 default
# lands inside the scroll button for the 12- and 13-item variants.
item_count = float(os.environ.get("SCROLL_TOOLBAR_ITEM_COUNT", "12.5"))
scroll_index = float(os.environ.get("SCROLL_TOOLBAR_SCROLL_INDEX", "9"))
primary_content_width = item_count * button_size + max(0.0, item_count - 1.0) * item_spacing
width = primary_content_width + horizontal_padding * 2.0
height = vertical_padding * 2.0 + button_size

desired_y = selection_max_y + gap
placed_above = desired_y + height > bottom - screen_margin
if placed_above:
    frame_y = max(top + screen_margin, selection_min_y - gap - height)
else:
    frame_y = min(bottom - screen_margin - height, desired_y)
frame_min_x = max(left + screen_margin, min(selection_mid_x - width / 2.0, right - screen_margin - width))
cursor_x = frame_min_x + horizontal_padding
center_x = cursor_x + scroll_index * (button_size + item_spacing) + button_size / 2.0
center_y = bottom - (frame_y + height / 2.0)
print(f"{round(center_x)},{round(center_y)}")
PY
	)"
	smoke_log "clicking scroll toolbar icon at $point"
	PATH_POINTS="$point;$point" \
		PATH_MODE="click-point" \
		PATH_DRIVER="${PATH_DRIVER:-event}" \
		PATH_SEGMENT_STEPS="${PATH_SEGMENT_STEPS:-1}" \
		PATH_STEP_DELAY_MS="${PATH_STEP_DELAY_MS:-0}" \
		PATH_DURATION_MS="${PATH_DURATION_MS:-0}" \
		PATH_RATE_HZ="${PATH_RATE_HZ:-120}" \
		PATH_CYCLES="${PATH_CYCLES:-1}" \
		live_hud_run_mouse_path
}

press_capture_hotkey() {
	live_hud_start_new_capture
}

press_plain_s() {
	osascript <<'APPLESCRIPT' >/dev/null
tell application "System Events"
	key code 1
end tell
APPLESCRIPT
}

press_space() {
	osascript <<'APPLESCRIPT' >/dev/null
tell application "System Events"
	key code 49
end tell
APPLESCRIPT
}

configure_preferences() {
	defaults write "$PREF_DOMAIN" captureHotkey -string Option-X
	defaults write "$PREF_DOMAIN" hudGlassEnabled -bool true
	defaults write "$PREF_DOMAIN" hudGlassMode -string liquid_glass
	defaults write "$PREF_DOMAIN" liquidGlassStyle -string clear
	defaults write "$PREF_DOMAIN" toolbarPlacement -string bottom
	defaults write "$PREF_DOMAIN" loupeSampleSize -string small
	defaults delete "$PREF_DOMAIN" scrollCaptureAutoScrollEnabled >/dev/null 2>&1 || true
}

start_scroll_background() {
	rm -f "$SCROLL_BACKGROUND_READY"
	swift "$SCRIPT_DIR/lib/scroll-background-window.swift" >"$SCROLL_BACKGROUND_READY" &
	SCROLL_BACKGROUND_PID="$!"
	if ! wait_ready_file "$SCROLL_BACKGROUND_READY"; then
		echo "[smoke] FAIL scroll background did not become ready" >&2
		return 1
	fi
}

parse_telemetry() {
	local log_path="$1"

	python3 - "$log_path" "$MIN_SCROLL_COMMITS" "$MIN_EXPORT_GROWTH_PX" "$SCROLL_START_METHOD" <<'PY'
import re
import sys

path, min_commits_raw, min_growth_raw, start_method = sys.argv[1:5]
min_commits = int(min_commits_raw)
min_growth = int(min_growth_raw)
expected_start_source = {
    "keyboard": "keyboard_s",
    "toolbar": "toolbar",
}[start_method]
text = open(path, "r", encoding="utf-8", errors="replace").read()

froze = "event=capture_timing.freeze_commit" in text
handoff = "event=capture_timing.frozen_first_display_handoff" in text
entry_started = bool(
    re.search(
        rf"event=capture\.scroll_capture_entry\b[^\n]*outcome=requested[^\n]*source={expected_start_source}\b",
        text,
    )
)
started = "event=capture.scroll_capture_started" in text
manual_mode = bool(
    re.search(
        r"event=capture\.scroll_capture_mode\b[^\n]*outcome=manual_universal\b",
        text,
    )
)
tap_not_used = bool(
    re.search(
        r"event=capture\.scroll_input_tap\b[^\n]*outcome=not_used\b",
        text,
    )
)
wheel_intercepted = bool(
    re.search(
        r"event=capture\.scroll_wheel_intercepted\b[^\n]*source=overlay\b",
        text,
    )
)
wheel_observed = bool(
    re.search(
        r"event=capture\.scroll_wheel_observed\b[^\n]*source=global_monitor\b",
        text,
    )
)
wheel_input_seen = wheel_intercepted or wheel_observed
sampled = "event=capture.scroll_sample_observed" in text
started_match = re.search(r"event=capture\.scroll_capture_started\b[^\n]*height=([0-9]+)", text)
base_height = int(started_match.group(1)) if started_match else 0
commits = re.findall(
    r"event=capture\.scroll_sample_observed\b[^\n]*outcome=committed\b[^\n]*",
    text,
)
heights = []
for line in re.findall(r"event=capture\.scroll_sample_observed\b[^\n]*", text):
    match = re.search(r"exportHeight=([0-9]+)", line)
    if match:
        heights.append(int(match.group(1)))
fallback = "source=below_overlay_capture" in text
missing = "event=capture.scroll_sample_missing" in text
auto_event = bool(re.search(r"event=capture\.scroll_auto_", text))
max_height = max(heights) if heights else 0
growth = max_height - base_height

print(
    f"[smoke] telemetry froze={froze} handoff={handoff} "
    f"entry_started={entry_started} start_source={expected_start_source} "
    f"started={started} manual_mode={manual_mode} sampled={sampled} commits={len(commits)} "
    f"tap_not_used={tap_not_used} wheel_intercepted={wheel_intercepted} "
    f"wheel_observed={wheel_observed} "
    f"max_export_height={max_height} base_height={base_height} growth={growth} "
    f"missing_live_frame={missing} auto_event={auto_event}"
)

failures = []
if not froze:
    failures.append("drag selection did not freeze")
if not handoff:
    failures.append("frozen first display handoff was not recorded")
if not entry_started:
    failures.append(f"scroll capture did not start from {expected_start_source}")
if not started:
    failures.append("scroll capture did not start")
if not manual_mode:
    failures.append("scroll capture did not use universal manual mode")
if not tap_not_used:
    failures.append("scroll capture did not use overlay-local wheel forwarding")
if not wheel_input_seen:
    failures.append("scroll capture did not receive wheel input")
if not sampled:
    failures.append("scroll capture did not sample")
if base_height <= 0:
    failures.append("scroll capture start height was not recorded")
if len(commits) < min_commits:
    failures.append(f"committed growth count {len(commits)} < {min_commits}")
if growth < min_growth:
    failures.append(f"export growth {growth}px < {min_growth}px")
if auto_event:
    failures.append("unexpected legacy auto-scroll telemetry")

if failures:
    for failure in failures:
        print(f"[smoke] FAIL {failure}", file=sys.stderr)
    sys.exit(1)
PY
}

if [[ -z "${DISPLAY_BOUNDS:-}" ]]; then
	DISPLAY_BOUNDS="$(live_hud_read_main_display_bounds | tr -d ' ')"
fi

if [[ -z "${DRAG_POINTS:-}" ]]; then
	DRAG_POINTS="$(
		python3 - "$DISPLAY_BOUNDS" <<'PY'
import sys

left, top, right, bottom = map(int, sys.argv[1].replace(" ", "").split(","))
width = right - left
height = bottom - top
if width < 700 or height < 520:
    raise SystemExit("display too small for native scroll-capture smoke")

start = (left + width * 30 // 100, top + height * 30 // 100)
end = (left + width * 70 // 100, top + height * 70 // 100)
print(f"{start[0]},{start[1]};{end[0]},{end[1]}")
PY
	)"
fi
if [[ -z "${SCROLL_POINT:-}" ]]; then
	SCROLL_POINT="$(
		python3 - "$DRAG_POINTS" <<'PY'
import sys

start_raw, end_raw = sys.argv[1].split(";")
x1, y1 = map(int, start_raw.split(","))
x2, y2 = map(int, end_raw.split(","))
print(f"{(x1 + x2) // 2},{(y1 + y2) // 2}")
PY
	)"
fi
BASE_HEIGHT="$(
	python3 - "$DRAG_POINTS" <<'PY'
import sys

start_raw, end_raw = sys.argv[1].split(";")
_, y1 = map(int, start_raw.split(","))
_, y2 = map(int, end_raw.split(","))
print(abs(y2 - y1))
PY
)"

smoke_log "display bounds: $DISPLAY_BOUNDS"
smoke_log "drag points: $DRAG_POINTS scroll_point=$SCROLL_POINT base_height=$BASE_HEIGHT"
case "$SCROLL_START_METHOD" in
	keyboard|toolbar)
		;;
	*)
		echo "[smoke] FAIL unknown SCROLL_START_METHOD=$SCROLL_START_METHOD" >&2
		exit 2
		;;
esac
configure_preferences

RSNAP_NATIVE_HOST_FORCE_REBUILD="${RSNAP_NATIVE_HOST_FORCE_REBUILD:-1}" \
	APP_POST_VERIFY_SETTLE_S="$APP_POST_VERIFY_SETTLE_S" \
	"$ROOT_DIR/scripts/build_and_run.sh" verify
start_scroll_background

case_started_epoch="$(date +%s)"
press_capture_hotkey
sleep "$OVERLAY_SETTLE_S"
live_hud_focus_rsnap_overlay

PATH_POINTS="$DRAG_POINTS" \
	PATH_MODE="drag-region" \
	PATH_DRIVER="${PATH_DRIVER:-event}" \
	PATH_DURATION_MS="${DRAG_DURATION_MS:-280}" \
	PATH_RATE_HZ="${PATH_RATE_HZ:-120}" \
	PATH_HOLD_BEFORE_RELEASE_MS="${DRAG_HOLD_BEFORE_RELEASE_MS:-180}" \
	live_hud_run_mouse_path
sleep "$POST_FREEZE_SETTLE_S"
live_hud_focus_rsnap_overlay
case "$SCROLL_START_METHOD" in
	keyboard)
		press_plain_s
		;;
	toolbar)
		click_scroll_toolbar_icon
		;;
esac
sleep 0.25

case "$SCROLL_DRIVER" in
	notification)
		SCROLL_COUNT="$SCROLL_COUNT" \
			SCROLL_DELTA_Y="$SCROLL_DELTA_Y" \
			SCROLL_INTERVAL_MS="$SCROLL_INTERVAL_MS" \
			swift "$SCRIPT_DIR/lib/scroll-background-command.swift"
		;;
	wheel)
		SCROLL_POINT="$SCROLL_POINT" \
			SCROLL_COUNT="$SCROLL_COUNT" \
			SCROLL_DELTA_Y="$SCROLL_DELTA_Y" \
			SCROLL_INTERVAL_MS="$SCROLL_INTERVAL_MS" \
			swift "$SCRIPT_DIR/lib/scroll-wheel-burst.swift"
		;;
	*)
		echo "[smoke] FAIL unknown SCROLL_DRIVER=$SCROLL_DRIVER" >&2
		exit 2
		;;
esac
if [[ "$SCROLL_DRIVER" == "notification" || "$SCROLL_DRIVER" == "wheel" ]]; then
	assert_scroll_background_moved
fi
sleep "$POST_SCROLL_SETTLE_S"

live_hud_focus_rsnap_overlay
osascript -e 'set the clipboard to ""' >/dev/null
press_space
sleep "$POST_COPY_SETTLE_S"
pasteboard_ok=0
pasteboard_info=""
if pasteboard_info="$(
	PASTEBOARD_WAIT_MS="${PASTEBOARD_WAIT_MS:-5000}" \
		swift "$SCRIPT_DIR/lib/pasteboard-image-info.swift" 2>&1
)"; then
	pasteboard_ok=1
	smoke_log "pasteboard $pasteboard_info"
else
	smoke_log "pasteboard unavailable: $pasteboard_info"
fi

telemetry_last="$(( $(date +%s) - case_started_epoch + 10 ))s"
out_dir="$(RSNAP_TELEMETRY_LAST="$telemetry_last" "$ROOT_DIR/scripts/telemetry/native-host.sh" collect)"
smoke_log "telemetry: $out_dir"
parse_telemetry "$out_dir/native-host.oslog"
if [[ "$pasteboard_ok" != "1" ]]; then
	echo "[smoke] FAIL no scroll capture image was copied to the pasteboard" >&2
	exit 1
fi
smoke_log "native scroll-capture smoke passed"
