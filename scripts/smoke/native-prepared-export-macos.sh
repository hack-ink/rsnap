#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/smoke/lib/live-hud.sh
source "$SCRIPT_DIR/lib/live-hud.sh"

usage() {
	cat <<'EOF'
Usage: native-prepared-export-macos.sh [--self-check] [--help]

Runs focused native-host prepared-export smokes:
  1. build and launch Rsnap
  2. drag-freeze a region
  3. optionally add a pen annotation for annotation-* cases
  4. invoke Copy, Save, and/or OCR and require prepared cache hits

Useful overrides:
  PREPARED_EXPORT_CASES=annotation-copy,annotation-save,plain-ocr,annotation-ocr-disabled
  POST_FREEZE_SETTLE_S=0.25
  POST_PREPARED_SETTLE_S=0.75
  POST_ANNOTATION_SETTLE_S=0.75
  POST_ACTION_SETTLE_S=0.35
  EXPECT_ANNOTATION_COPY_CACHE_HIT=1
  EXPECT_ANNOTATION_SAVE_CACHE_HIT=1
  EXPECT_OCR_CACHE_HIT=1
  POST_DRAG_FREEZE_SETTLE_S=2.8
  MAX_FREEZE_COMMIT_TOTAL_MS=1800
  MAX_FREEZE_SNAPSHOT_WAIT_MS=2200
  MAX_FROZEN_FIRST_DISPLAY_HANDOFF_MS=120
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
PREF_SNAPSHOT="$(mktemp "${TMPDIR:-/tmp}/rsnap-prepared-export-prefs.XXXXXX.plist")"
PREF_SNAPSHOT_EXISTS=0
PREPARED_EXPORT_CASES="${PREPARED_EXPORT_CASES:-annotation-copy,annotation-save,plain-ocr,annotation-ocr-disabled}"
POST_FREEZE_SETTLE_S="${POST_FREEZE_SETTLE_S:-0.25}"
POST_DRAG_FREEZE_SETTLE_S="${POST_DRAG_FREEZE_SETTLE_S:-2.8}"
POST_PREPARED_SETTLE_S="${POST_PREPARED_SETTLE_S:-0.75}"
POST_ANNOTATION_SETTLE_S="${POST_ANNOTATION_SETTLE_S:-0.75}"
POST_ACTION_SETTLE_S="${POST_ACTION_SETTLE_S:-0.35}"
EXPECT_ANNOTATION_COPY_CACHE_HIT="${EXPECT_ANNOTATION_COPY_CACHE_HIT:-1}"
EXPECT_ANNOTATION_SAVE_CACHE_HIT="${EXPECT_ANNOTATION_SAVE_CACHE_HIT:-1}"
EXPECT_OCR_CACHE_HIT="${EXPECT_OCR_CACHE_HIT:-${EXPECT_ANNOTATION_OCR_CACHE_HIT:-1}}"
MAX_FREEZE_COMMIT_TOTAL_MS="${MAX_FREEZE_COMMIT_TOTAL_MS:-1800}"
MAX_FREEZE_SNAPSHOT_WAIT_MS="${MAX_FREEZE_SNAPSHOT_WAIT_MS:-2200}"
MAX_FROZEN_FIRST_DISPLAY_HANDOFF_MS="${MAX_FROZEN_FIRST_DISPLAY_HANDOFF_MS:-120}"
PATH_CYCLES="${PATH_CYCLES:-1}"
SAVE_OUTPUT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rsnap-prepared-export-save.XXXXXX")"

restore_preferences() {
	live_hud_stop_awake_assertion
	live_hud_cancel_capture_if_present
	if [[ "$PREF_SNAPSHOT_EXISTS" == "1" ]]; then
		defaults import "$PREF_DOMAIN" "$PREF_SNAPSHOT" >/dev/null 2>&1 || true
	else
		for key in captureHotkey hudGlassEnabled hudGlassMode liquidGlassStyle toolbarPlacement loupeSampleSize outputDirectory outputFilenamePrefix outputNaming; do
			defaults delete "$PREF_DOMAIN" "$key" >/dev/null 2>&1 || true
		done
	fi
	rm -f "$PREF_SNAPSHOT"
	rm -rf "$SAVE_OUTPUT_DIR"
}

if defaults export "$PREF_DOMAIN" "$PREF_SNAPSHOT" >/dev/null 2>&1; then
	PREF_SNAPSHOT_EXISTS=1
fi
trap restore_preferences EXIT

configure_preferences() {
	defaults write "$PREF_DOMAIN" captureHotkey -string Option-X
	defaults write "$PREF_DOMAIN" hudGlassEnabled -bool true
	defaults write "$PREF_DOMAIN" hudGlassMode -string liquid_glass
	defaults write "$PREF_DOMAIN" liquidGlassStyle -string clear
	defaults write "$PREF_DOMAIN" toolbarPlacement -string bottom
	defaults write "$PREF_DOMAIN" loupeSampleSize -string small
	defaults write "$PREF_DOMAIN" outputDirectory -string "$SAVE_OUTPUT_DIR"
	defaults write "$PREF_DOMAIN" outputFilenamePrefix -string rsnap-smoke
	defaults write "$PREF_DOMAIN" outputNaming -string sequence
}

press_capture_hotkey() {
	live_hud_start_new_capture
}

press_space() {
	osascript <<'APPLESCRIPT' >/dev/null
tell application "System Events"
	key code 49
end tell
APPLESCRIPT
}

press_command_s() {
	osascript <<'APPLESCRIPT' >/dev/null
tell application "System Events"
	key code 1 using command down
end tell
APPLESCRIPT
}

press_plain_r() {
	osascript <<'APPLESCRIPT' >/dev/null
tell application "System Events"
	key code 15
end tell
APPLESCRIPT
}

click_frozen_toolbar_item() {
	local item_index="$1"
	local point

	point="$(
		python3 - "$DRAG_POINTS" "$DISPLAY_BOUNDS" "$item_index" <<'PY'
import sys

drag_points, display_bounds, item_index_raw = sys.argv[1:4]
start_raw, end_raw = drag_points.split(";")
x1, y1 = map(float, start_raw.split(","))
x2, y2 = map(float, end_raw.split(","))
left, top, right, bottom = map(float, display_bounds.replace(" ", "").split(","))
item_index = float(item_index_raw)

selection_min_x = min(x1, x2)
selection_max_x = max(x1, x2)
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
item_count = float(__import__("os").environ.get("PREPARED_EXPORT_TOOLBAR_ITEM_COUNT", "13"))
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
center_x = cursor_x + item_index * (button_size + item_spacing) + button_size / 2.0
center_y = bottom - (frame_y + height / 2.0)
print(f"{round(center_x)},{round(center_y)}")
PY
	)"
	local click_index click_repeats
	click_repeats="${PREPARED_EXPORT_TOOLBAR_CLICK_REPEATS:-2}"
	for ((click_index = 1; click_index <= click_repeats; click_index += 1)); do
		smoke_log "clicking toolbar item index=$item_index at $point repeat=$click_index/$click_repeats"
		PATH_POINTS="$point;$point" \
			PATH_MODE="click-point" \
			PATH_DRIVER="${PATH_DRIVER:-event}" \
			PATH_SEGMENT_STEPS="${PATH_SEGMENT_STEPS:-1}" \
			PATH_STEP_DELAY_MS="${PATH_STEP_DELAY_MS:-0}" \
			PATH_DURATION_MS="${PATH_DURATION_MS:-0}" \
			PATH_RATE_HZ="${PATH_RATE_HZ:-120}" \
			PATH_CYCLES="${PATH_CYCLES:-1}" \
			live_hud_run_mouse_path
		sleep "${PREPARED_EXPORT_TOOLBAR_CLICK_REPEAT_SETTLE_S:-0.12}"
	done
}

parse_telemetry() {
	local log_path="$1"
	local case_name="$2"
	local expect_copy_cache_hit="$3"
	local expect_save_cache_hit="$4"
	local expect_ocr_cache_hit="$5"
	local max_freeze_commit_total_ms="$6"
	local max_freeze_snapshot_wait_ms="$7"
	local max_frozen_first_display_handoff_ms="$8"

	python3 - "$log_path" "$case_name" "$expect_copy_cache_hit" "$expect_save_cache_hit" "$expect_ocr_cache_hit" "$max_freeze_commit_total_ms" "$max_freeze_snapshot_wait_ms" "$max_frozen_first_display_handoff_ms" <<'PY'
import re
import sys

(
    path,
    case_name,
    expect_copy_raw,
    expect_save_raw,
    expect_ocr_raw,
    max_freeze_commit_total_raw,
    max_freeze_snapshot_wait_raw,
    max_frozen_first_display_handoff_raw,
) = sys.argv[1:9]
expect_copy = expect_copy_raw != "0"
expect_save = expect_save_raw != "0"
expect_ocr = expect_ocr_raw != "0"
max_freeze_commit_total = float(max_freeze_commit_total_raw)
max_freeze_snapshot_wait = float(max_freeze_snapshot_wait_raw)
max_frozen_first_display_handoff = float(max_frozen_first_display_handoff_raw)
text = open(path, "r", encoding="utf-8", errors="replace").read()
all_lines = text.splitlines()

capture_ids = [
    match.group(1)
    for line in all_lines
    if (match := re.search(r"\bcaptureID=(\d+)\b.*\bevent=capture\.freeze_snapshot_requested\b", line))
]
if not capture_ids:
    capture_ids = [
        match.group(1)
        for line in all_lines
        if (match := re.search(r"\bcaptureID=(\d+)\b.*\bevent=capture_timing\.freeze_commit\b", line))
    ]
current_capture_id = capture_ids[-1] if capture_ids else None

def current_capture_lines(lines):
    if current_capture_id is None:
        return lines
    return [
        line
        for line in lines
        if re.search(rf"\bcaptureID={re.escape(current_capture_id)}\b", line)
    ]

def timing_lines(event):
	return current_capture_lines(
		[line for line in all_lines if f"event=capture_timing.{event}" in line]
	)

prepared_annotation_exports = [
    line
    for line in timing_lines("prepared_frozen_export")
    if "success=true" in line and "reason=annotation_" in line
]
freeze_commits = timing_lines("freeze_commit")
frozen_first_display_handoffs = timing_lines("frozen_first_display_handoff")
prepared_ocr_images = [
    line
    for line in timing_lines("prepared_recognize_text_image")
    if "success=true" in line
]
prepared_freeze_ocr_images = [
    line
    for line in prepared_ocr_images
    if "reason=freeze_commit" in line
]
prepared_annotation_ocr_images = [
    line
    for line in prepared_ocr_images
    if "success=true" in line and "reason=annotation_" in line
]
copy_successes = [
    line
    for line in timing_lines("copy_capture")
    if "success=true" in line
]
copy_cache_hits = [line for line in copy_successes if "cacheHit=true" in line]
save_successes = [
    line
    for line in timing_lines("save_capture")
    if "success=true" in line
]
save_cache_hits = [line for line in save_successes if "cacheHit=true" in line]
ocr_successes = [
    line
    for line in timing_lines("recognize_text")
    if "success=true" in line
]
ocr_cache_hits = [line for line in ocr_successes if "cacheHit=true" in line]

def metric(line, name):
    match = re.search(rf"{name}=([0-9.]+)", line)
    return match.group(1) if match else "none"

def metric_float(line, name):
    raw = metric(line, name)
    return float(raw) if raw != "none" else None

freeze_tail = freeze_commits[-1] if freeze_commits else ""
handoff_tail = frozen_first_display_handoffs[-1] if frozen_first_display_handoffs else ""
copy_tail = copy_successes[-1] if copy_successes else ""
save_tail = save_successes[-1] if save_successes else ""
ocr_tail = ocr_successes[-1] if ocr_successes else ""
print(
    f"[smoke] telemetry case={case_name} "
    f"capture_id={current_capture_id or 'unknown'} "
    f"freeze_commit_total_ms={metric(freeze_tail, 'totalMs')} "
    f"freeze_snapshot_wait_ms={metric(freeze_tail, 'snapshotWaitMs')} "
    f"frozen_first_display_handoff_ms={metric(handoff_tail, 'totalMs')} "
    f"prepared_annotation_exports={len(prepared_annotation_exports)} "
    f"prepared_ocr_images={len(prepared_ocr_images)} "
    f"copy_cache_hits={len(copy_cache_hits)} "
    f"copy_total_ms={metric(copy_tail, 'totalMs')} "
    f"copy_capture_image_ms={metric(copy_tail, 'captureImageMs')} "
    f"save_cache_hits={len(save_cache_hits)} "
    f"save_total_ms={metric(save_tail, 'totalMs')} "
    f"save_capture_image_ms={metric(save_tail, 'captureImageMs')} "
    f"ocr_cache_hits={len(ocr_cache_hits)} "
    f"ocr_total_ms={metric(ocr_tail, 'totalMs')} "
    f"ocr_capture_image_ms={metric(ocr_tail, 'captureImageMs')} "
    f"ocr_vision_request_ms={metric(ocr_tail, 'visionRequestMs')}"
)

failures = []
freeze_commit_total = metric_float(freeze_tail, "totalMs")
freeze_snapshot_wait = metric_float(freeze_tail, "snapshotWaitMs")
frozen_first_display_handoff = metric_float(handoff_tail, "totalMs")
if freeze_commit_total is None:
    failures.append("freeze commit timing was not recorded")
elif max_freeze_commit_total > 0 and freeze_commit_total > max_freeze_commit_total:
    failures.append(
        f"freeze commit total {freeze_commit_total:.2f}ms > {max_freeze_commit_total:.2f}ms"
    )
if freeze_snapshot_wait is None:
    failures.append("freeze snapshot wait timing was not recorded")
elif max_freeze_snapshot_wait > 0 and freeze_snapshot_wait > max_freeze_snapshot_wait:
    failures.append(
        f"freeze snapshot wait {freeze_snapshot_wait:.2f}ms > {max_freeze_snapshot_wait:.2f}ms"
    )
if frozen_first_display_handoff is None:
    failures.append("frozen first display handoff timing was not recorded")
elif (
    max_frozen_first_display_handoff > 0
    and frozen_first_display_handoff > max_frozen_first_display_handoff
):
    failures.append(
        "frozen first display handoff "
        f"{frozen_first_display_handoff:.2f}ms > {max_frozen_first_display_handoff:.2f}ms"
    )
if case_name in ("annotation-copy", "annotation-save") and not prepared_annotation_exports:
    failures.append("annotation prepared export timing was not recorded")
if case_name == "annotation-copy":
    if not copy_successes:
        failures.append("copy capture timing was not recorded")
    elif expect_copy and not copy_cache_hits:
        failures.append("annotation copy did not use prepared export cache")
if case_name == "annotation-save":
    if not save_successes:
        failures.append("save capture timing was not recorded")
    elif expect_save and not save_cache_hits:
        failures.append("annotation save did not use prepared export cache")
if case_name == "annotation-ocr-disabled":
    if prepared_annotation_ocr_images:
        failures.append("annotation OCR image was prepared while OCR was disabled")
    if ocr_successes:
        failures.append("annotation OCR shortcut ran while OCR was disabled")
if case_name == "plain-ocr":
    if not prepared_freeze_ocr_images:
        failures.append("plain OCR prepared image timing was not recorded")
    if not ocr_successes:
        failures.append("OCR timing was not recorded")
    elif expect_ocr and not ocr_cache_hits:
        failures.append("plain OCR did not use prepared image cache")

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
    raise SystemExit("display too small for native prepared-export smoke")

start = (left + width * 36 // 100, top + height * 36 // 100)
end = (left + width * 64 // 100, top + height * 64 // 100)
print(f"{start[0]},{start[1]};{end[0]},{end[1]}")
PY
	)"
fi

ANNOTATION_POINTS="$(
	python3 - "$DRAG_POINTS" <<'PY'
import sys

start_raw, end_raw = sys.argv[1].split(";")
x1, y1 = map(int, start_raw.split(","))
x2, y2 = map(int, end_raw.split(","))
min_x, max_x = sorted((x1, x2))
min_y, max_y = sorted((y1, y2))
start = (min_x + (max_x - min_x) * 30 // 100, min_y + (max_y - min_y) * 35 // 100)
end = (min_x + (max_x - min_x) * 70 // 100, min_y + (max_y - min_y) * 65 // 100)
print(f"{start[0]},{start[1]};{end[0]},{end[1]}")
PY
)"

run_prepared_export_case() {
	local case_name="$1"
	local case_started_epoch telemetry_last out_dir

	case_started_epoch="$(date +%s)"
	press_capture_hotkey
	sleep "$POST_FREEZE_SETTLE_S"
	live_hud_focus_rsnap_overlay
	PATH_POINTS="$DRAG_POINTS" \
		PATH_MODE="drag-region" \
		PATH_DRIVER="${PATH_DRIVER:-event}" \
		PATH_DURATION_MS="${DRAG_DURATION_MS:-260}" \
		PATH_RATE_HZ="${PATH_RATE_HZ:-120}" \
		PATH_HOLD_BEFORE_RELEASE_MS="${DRAG_HOLD_BEFORE_RELEASE_MS:-180}" \
		live_hud_run_mouse_path
	sleep "$POST_DRAG_FREEZE_SETTLE_S"
	live_hud_focus_rsnap_overlay

	if [[ "$case_name" == annotation-* ]]; then
		click_frozen_toolbar_item 1
		sleep 0.10
		PATH_POINTS="$ANNOTATION_POINTS" \
			PATH_MODE="drag-region" \
			PATH_DRIVER="${PATH_DRIVER:-event}" \
			PATH_DURATION_MS="${ANNOTATION_DURATION_MS:-180}" \
			PATH_RATE_HZ="${PATH_RATE_HZ:-120}" \
			PATH_HOLD_BEFORE_RELEASE_MS="${ANNOTATION_HOLD_BEFORE_RELEASE_MS:-40}" \
			live_hud_run_mouse_path
		sleep "$POST_ANNOTATION_SETTLE_S"
	else
		sleep "$POST_PREPARED_SETTLE_S"
	fi
	live_hud_focus_rsnap_overlay

	case "$case_name" in
		annotation-copy)
			osascript -e 'set the clipboard to ""' >/dev/null
			press_space
			;;
		annotation-save)
			press_command_s
			;;
		annotation-ocr-disabled|plain-ocr)
			press_plain_r
			;;
		*)
			echo "[smoke] FAIL unknown prepared export case=$case_name" >&2
			return 2
			;;
	esac
	sleep "$POST_ACTION_SETTLE_S"
	telemetry_last="$(( $(date +%s) - case_started_epoch + 10 ))s"
	out_dir="$(RSNAP_TELEMETRY_LAST="$telemetry_last" "$ROOT_DIR/scripts/telemetry/native-host.sh" collect)"
	smoke_log "telemetry: $out_dir"
	parse_telemetry "$out_dir/native-host.oslog" \
		"$case_name" \
		"$EXPECT_ANNOTATION_COPY_CACHE_HIT" \
		"$EXPECT_ANNOTATION_SAVE_CACHE_HIT" \
		"$EXPECT_OCR_CACHE_HIT" \
		"$MAX_FREEZE_COMMIT_TOTAL_MS" \
		"$MAX_FREEZE_SNAPSHOT_WAIT_MS" \
		"$MAX_FROZEN_FIRST_DISPLAY_HANDOFF_MS"
	if [[ "$case_name" == "annotation-copy" ]]; then
		PASTEBOARD_WAIT_MS="${PASTEBOARD_WAIT_MS:-5000}" \
			swift "$SCRIPT_DIR/lib/pasteboard-image-info.swift" >/dev/null
	fi
}

smoke_log "display bounds: $DISPLAY_BOUNDS"
smoke_log "drag points: $DRAG_POINTS annotation_points=$ANNOTATION_POINTS"
smoke_log "cases: $PREPARED_EXPORT_CASES"
configure_preferences
RSNAP_NATIVE_HOST_FORCE_REBUILD="${RSNAP_NATIVE_HOST_FORCE_REBUILD:-1}" \
	APP_POST_VERIFY_SETTLE_S="${APP_POST_VERIFY_SETTLE_S:-0}" \
	"$ROOT_DIR/scripts/build_and_run.sh" verify

IFS=',' read -r -a PREPARED_EXPORT_CASE_ARRAY <<<"$PREPARED_EXPORT_CASES"
for case_name in "${PREPARED_EXPORT_CASE_ARRAY[@]}"; do
	case_name="$(echo "$case_name" | tr -d '[:space:]')"
	[[ -n "$case_name" ]] || continue
	smoke_log "case: $case_name"
	run_prepared_export_case "$case_name"
done

smoke_log "native prepared-export smoke passed"
