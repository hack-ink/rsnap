#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/smoke/lib/live-hud.sh
source "$SCRIPT_DIR/lib/live-hud.sh"

usage() {
	cat <<'EOF'
Usage: native-visual-contract-macos.sh [--self-check] [--help]

Runs the native macOS visual/behavior contract smoke:
  1. force a representative native-host visual mode
  2. build, sign, and launch the native host app
  3. run one real click freeze and one real held-drag freeze
  4. capture the in-drag and frozen overlays
  5. gate click/drag editability, scrim stability, border leakage, and handoff telemetry

Useful overrides:
  VISUAL_CONTRACT_CASES=liquid          optional: liquid,classic
  REPEATED_CLICK_FREEZES=1
  REPEATED_DRAG_FREEZES=2
  DRAG_DURATION_MS=260
  DRAG_HOLD_BEFORE_RELEASE_MS=700
  PATH_RATE_HZ=120
  MAX_FREEZE_COMMIT_MS=90
  MAX_FREEZE_PRESENT_MS=35
  MAX_FREEZE_SNAPSHOT_WAIT_MS=45
  MAX_FROZEN_HANDOFF_MS=35
  CLASSIC_MAX_FREEZE_PRESENT_MS=55
  CLASSIC_MAX_FROZEN_HANDOFF_MS=55
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
ROOT_DIR="$(live_hud_repo_root)"
live_hud_init_environment "$ROOT_DIR"

DRAG_DURATION_MS="${DRAG_DURATION_MS:-260}"
DRAG_HOLD_BEFORE_RELEASE_MS="${DRAG_HOLD_BEFORE_RELEASE_MS:-700}"
PATH_RATE_HZ="${PATH_RATE_HZ:-120}"
VISUAL_CONTRACT_CASES="${VISUAL_CONTRACT_CASES:-liquid}"
OVERLAY_SETTLE_S="${OVERLAY_SETTLE_S:-0.25}"
POST_FREEZE_SETTLE_S="${POST_FREEZE_SETTLE_S:-0.25}"
POST_CLOSE_SETTLE_S="${POST_CLOSE_SETTLE_S:-0.25}"
REPEATED_CLICK_FREEZES="${REPEATED_CLICK_FREEZES:-1}"
REPEATED_DRAG_FREEZES="${REPEATED_DRAG_FREEZES:-2}"
RSNAP_TELEMETRY_LAST="${RSNAP_TELEMETRY_LAST:-3m}"
export RSNAP_TELEMETRY_LAST

PREF_DOMAIN="${RSNAP_PREF_DOMAIN:-ink.hack.rsnap}"
PREF_SNAPSHOT="$(mktemp "${TMPDIR:-/tmp}/rsnap-prefs.XXXXXX.plist")"
PREF_SNAPSHOT_EXISTS=0
VISUAL_BACKGROUND_PID=""

restore_preferences() {
	if [[ -n "$VISUAL_BACKGROUND_PID" ]]; then
		kill "$VISUAL_BACKGROUND_PID" >/dev/null 2>&1 || true
	fi
	if [[ "$PREF_SNAPSHOT_EXISTS" == "1" ]]; then
		defaults import "$PREF_DOMAIN" "$PREF_SNAPSHOT" >/dev/null 2>&1 || true
	else
		for key in captureHotkey hudGlassEnabled hudGlassMode liquidGlassStyle toolbarPlacement loupeSampleSize; do
			defaults delete "$PREF_DOMAIN" "$key" >/dev/null 2>&1 || true
		done
	fi
	rm -f "$PREF_SNAPSHOT"
}

if defaults export "$PREF_DOMAIN" "$PREF_SNAPSHOT" >/dev/null 2>&1; then
	PREF_SNAPSHOT_EXISTS=1
fi
trap restore_preferences EXIT

press_capture_hotkey() {
	osascript <<'APPLESCRIPT' >/dev/null
tell application "System Events"
	key code 7 using option down
end tell
APPLESCRIPT
}

capture_visual_screenshot() {
	local screenshot_path="$1"
	local attempt

	for attempt in 1 2 3; do
		if /usr/sbin/screencapture -x "$screenshot_path" >/dev/null 2>&1; then
			return 0
		fi
		sleep 0.2
	done
	return 1
}

start_visual_background() {
	local ready_path="$1"
	rm -f "$ready_path"
	swift "$SCRIPT_DIR/lib/visual-background-window.swift" >"$ready_path" &
	VISUAL_BACKGROUND_PID="$!"
}

stop_visual_background() {
	if [[ -n "$VISUAL_BACKGROUND_PID" ]]; then
		kill "$VISUAL_BACKGROUND_PID" >/dev/null 2>&1 || true
		VISUAL_BACKGROUND_PID=""
	fi
}

wait_visual_background_ready() {
	local ready_path="$1"
	local attempt

	for attempt in {1..40}; do
		if grep -q '^ready$' "$ready_path" >/dev/null 2>&1; then
			return 0
		fi
		sleep 0.05
	done
	return 1
}

wait_mask_probe_ready() {
	local ready_path="$1"
	local attempt

	for attempt in {1..200}; do
		if grep -q '^ready$' "$ready_path" >/dev/null 2>&1; then
			return 0
		fi
		sleep 0.05
	done
	return 1
}

wait_drag_hold_ready() {
	local phase_path="$1"
	local attempt

	for attempt in {1..120}; do
		if grep -q '^holding$' "$phase_path" >/dev/null 2>&1; then
			return 0
		fi
		sleep 0.025
	done
	return 1
}

wait_file_nonempty() {
	local path="$1"
	local attempt

	for attempt in {1..120}; do
		if [[ -s "$path" ]]; then
			return 0
		fi
		sleep 0.025
	done
	return 1
}

if [[ -z "$DISPLAY_BOUNDS" ]]; then
	DISPLAY_BOUNDS="$(live_hud_read_main_display_bounds | tr -d ' ')"
fi
if [[ -z "$PATH_POINTS" ]]; then
	PATH_POINTS="$(
		python3 - "$DISPLAY_BOUNDS" <<'PY'
import sys

left, top, right, bottom = map(int, sys.argv[1].replace(" ", "").split(","))
width = right - left
height = bottom - top
if width < 500 or height < 400:
    raise SystemExit("display too small for native visual contract smoke")

start = (left + width * 38 // 100, top + height * 38 // 100)
end = (left + width * 62 // 100, top + height * 62 // 100)
print(f"{start[0]},{start[1]};{end[0]},{end[1]}")
PY
	)"
fi

echo "[smoke] display bounds: $DISPLAY_BOUNDS"
echo "[smoke] drag points: $PATH_POINTS duration_ms=$DRAG_DURATION_MS hold_ms=$DRAG_HOLD_BEFORE_RELEASE_MS rate_hz=$PATH_RATE_HZ"

CLICK_POINTS="$(
	python3 - "$DISPLAY_BOUNDS" <<'PY'
import sys

left, top, right, bottom = map(int, sys.argv[1].replace(" ", "").split(","))
width = right - left
height = bottom - top
if width < 500 or height < 400:
    raise SystemExit("display too small for native visual contract smoke")

x = left + width * 50 // 100
y = top + height * 50 // 100
print(f"{x},{y};{x},{y}")
PY
)"
echo "[smoke] click point: ${CLICK_POINTS%%;*} repeated=$REPEATED_CLICK_FREEZES"

configure_case_preferences() {
	local case_name="$1"

	defaults write "$PREF_DOMAIN" captureHotkey -string Option-X
	defaults write "$PREF_DOMAIN" hudGlassEnabled -bool true
	defaults write "$PREF_DOMAIN" liquidGlassStyle -string clear
	defaults write "$PREF_DOMAIN" toolbarPlacement -string bottom
	defaults write "$PREF_DOMAIN" loupeSampleSize -string small
	case "$case_name" in
		liquid)
			defaults write "$PREF_DOMAIN" hudGlassMode -string liquid_glass
			;;
		classic)
			defaults write "$PREF_DOMAIN" hudGlassMode -string classic_glass
			;;
		*)
			echo "unknown visual contract case: $case_name" >&2
			exit 2
			;;
	esac
}

run_visual_case() {
	local case_name="$1"
	local case_tmp_dir drag_screenshot_path drag_screenshot_paths screenshot_path background_ready_path case_started_epoch out_dir

	case_tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/rsnap-visual-${case_name}.XXXXXX")"
	drag_screenshot_paths=""
	drag_screenshot_path="$case_tmp_dir/dragging-1.png"
	screenshot_path="$case_tmp_dir/frozen-1.png"
	background_ready_path="$case_tmp_dir/background.ready"
	case_started_epoch="$(date +%s)"

	configure_case_preferences "$case_name"
	echo "[smoke] case: $case_name"
	start_visual_background "$background_ready_path"
	if ! wait_visual_background_ready "$background_ready_path"; then
		stop_visual_background
		echo "[smoke] FAIL visual background did not become ready" >&2
		return 1
	fi
	"$ROOT_DIR/scripts/build_and_run.sh" verify >/tmp/rsnap-native-visual-contract-build.out
	sleep 1.0
	live_hud_press_escape >/dev/null 2>&1 || true

	for ((click_index = 1; click_index <= REPEATED_CLICK_FREEZES; click_index++)); do
		echo "[smoke] repeated click freeze $click_index/$REPEATED_CLICK_FREEZES"
		press_capture_hotkey
		sleep "$OVERLAY_SETTLE_S"
		live_hud_focus_rsnap_overlay
		PATH_MODE=click-point \
		PATH_DRIVER=event \
		PATH_POINTS="$CLICK_POINTS" \
		swift "$(live_hud_cursor_helper)"
		sleep "$POST_FREEZE_SETTLE_S"
		live_hud_focus_rsnap_overlay
		live_hud_press_escape >/dev/null 2>&1 || true
		sleep "$POST_CLOSE_SETTLE_S"
	done

	for ((drag_index = 1; drag_index <= REPEATED_DRAG_FREEZES; drag_index++)); do
		echo "[smoke] repeated drag freeze $drag_index/$REPEATED_DRAG_FREEZES"
		drag_screenshot_path="$case_tmp_dir/dragging-$drag_index.png"
		screenshot_path="$case_tmp_dir/frozen-$drag_index.png"
		press_capture_hotkey
		sleep "$OVERLAY_SETTLE_S"
		live_hud_focus_rsnap_overlay

		local mask_probe_path mask_probe_phase_path mask_probe_ready_path mask_probe_stderr_path mask_probe_point mask_probe_pid drag_pid
		mask_probe_path="$case_tmp_dir/mask-probe-$drag_index.csv"
		mask_probe_phase_path="$case_tmp_dir/mask-probe-$drag_index.phase"
		mask_probe_ready_path="$case_tmp_dir/mask-probe-$drag_index.ready"
		mask_probe_stderr_path="$case_tmp_dir/mask-probe-$drag_index.stderr"
		printf 'pre' >"$mask_probe_phase_path"
		rm -f "$mask_probe_ready_path"
		mask_probe_point="$(
			python3 - "$DISPLAY_BOUNDS" "$PATH_POINTS" <<'PY'
import sys

left, top, right, bottom = map(int, sys.argv[1].replace(" ", "").split(","))
start_raw, end_raw = sys.argv[2].split(";")[:2]
sx, sy = map(float, start_raw.split(","))
ex, ey = map(float, end_raw.split(","))
min_x, max_x = sorted((sx, ex))
min_y, max_y = sorted((sy, ey))
selection_width = max_x - min_x
selection_height = max_y - min_y
sample_x = max_x + max(96, selection_width * 0.18)
if sample_x > right - 96:
    sample_x = min_x - max(96, selection_width * 0.18)
sample_y = min_y + selection_height * 0.45
sample_x = min(max(sample_x, left + 32), right - 32)
sample_y = min(max(sample_y, top + 32), bottom - 32)
print(f"{sample_x:.0f},{sample_y:.0f}")
PY
		)"

		MASK_PROBE_OUTPUT="$mask_probe_path" \
		MASK_PROBE_PHASE_PATH="$mask_probe_phase_path" \
		MASK_PROBE_READY_PATH="$mask_probe_ready_path" \
		MASK_PROBE_SCREENSHOT_PATH="$drag_screenshot_path" \
		MASK_PROBE_POINT="$mask_probe_point" \
		MASK_PROBE_DURATION_MS="${MASK_PROBE_DURATION_MS:-2400}" \
		MASK_PROBE_RATE_HZ="${MASK_PROBE_RATE_HZ:-60}" \
		swift "$SCRIPT_DIR/lib/mask-probe-capture.swift" 2>"$mask_probe_stderr_path" &
		mask_probe_pid="$!"
		if ! wait_mask_probe_ready "$mask_probe_ready_path"; then
			kill "$mask_probe_pid" >/dev/null 2>&1 || true
			wait "$mask_probe_pid" >/dev/null 2>&1 || true
			live_hud_focus_rsnap_overlay
			live_hud_press_escape >/dev/null 2>&1 || true
			stop_visual_background
			echo "[smoke] FAIL mask probe did not produce a ready sample" >&2
			if [[ -s "$mask_probe_stderr_path" ]]; then
				sed 's/^/[smoke] mask probe stderr: /' "$mask_probe_stderr_path" >&2
			fi
			return 1
		fi

		PATH_MODE=drag-region \
		PATH_DRIVER=event \
		PATH_POINTS="$PATH_POINTS" \
		PATH_DURATION_MS="$DRAG_DURATION_MS" \
		PATH_RATE_HZ="$PATH_RATE_HZ" \
		PATH_HOLD_BEFORE_RELEASE_MS="$DRAG_HOLD_BEFORE_RELEASE_MS" \
		MASK_PROBE_PHASE_PATH="$mask_probe_phase_path" \
		swift "$(live_hud_cursor_helper)" &
		drag_pid="$!"
		if ! wait_drag_hold_ready "$mask_probe_phase_path"; then
			kill "$drag_pid" >/dev/null 2>&1 || true
			wait "$drag_pid" >/dev/null 2>&1 || true
			kill "$mask_probe_pid" >/dev/null 2>&1 || true
			wait "$mask_probe_pid" >/dev/null 2>&1 || true
			live_hud_focus_rsnap_overlay
			live_hud_press_escape >/dev/null 2>&1 || true
			stop_visual_background
			echo "[smoke] FAIL drag did not reach held preview phase" >&2
			return 1
		fi
		if ! wait_file_nonempty "$drag_screenshot_path"; then
			kill "$drag_pid" >/dev/null 2>&1 || true
			wait "$drag_pid" >/dev/null 2>&1 || true
			kill "$mask_probe_pid" >/dev/null 2>&1 || true
			wait "$mask_probe_pid" >/dev/null 2>&1 || true
			live_hud_focus_rsnap_overlay
			live_hud_press_escape >/dev/null 2>&1 || true
			stop_visual_background
			echo "[smoke] FAIL mask probe did not write drag screenshot: $drag_screenshot_path" >&2
			return 1
		fi
		if ! wait "$drag_pid"; then
			kill "$mask_probe_pid" >/dev/null 2>&1 || true
			wait "$mask_probe_pid" >/dev/null 2>&1 || true
			live_hud_focus_rsnap_overlay
			live_hud_press_escape >/dev/null 2>&1 || true
			stop_visual_background
			echo "[smoke] FAIL drag cursor driver failed" >&2
			return 1
		fi
		if ! wait "$mask_probe_pid"; then
			live_hud_focus_rsnap_overlay
			live_hud_press_escape >/dev/null 2>&1 || true
			stop_visual_background
			echo "[smoke] FAIL mask probe capture failed" >&2
			if [[ -s "$mask_probe_stderr_path" ]]; then
				sed 's/^/[smoke] mask probe stderr: /' "$mask_probe_stderr_path" >&2
			fi
			return 1
		fi

		sleep "$POST_FREEZE_SETTLE_S"
		if ! capture_visual_screenshot "$screenshot_path"; then
			live_hud_focus_rsnap_overlay
			live_hud_press_escape >/dev/null 2>&1 || true
			stop_visual_background
			echo "[smoke] FAIL could not capture visual screenshot: $screenshot_path" >&2
			return 1
		fi
		drag_screenshot_paths="${drag_screenshot_paths:+$drag_screenshot_paths:}$drag_screenshot_path"
		live_hud_focus_rsnap_overlay
		live_hud_press_escape >/dev/null 2>&1 || true
		sleep "$POST_CLOSE_SETTLE_S"
	done
	stop_visual_background

	local expected_editability
	expected_editability="$(
		python3 - "$REPEATED_CLICK_FREEZES" "$REPEATED_DRAG_FREEZES" <<'PY'
import sys

click_count = int(sys.argv[1])
drag_count = int(sys.argv[2])
print(",".join(["true"] * (click_count + drag_count)))
PY
	)"
	out_dir="$("$ROOT_DIR/scripts/telemetry/native-host.sh" collect)"
	echo "[smoke] telemetry: $out_dir"
	EXPECTED_HUD_GLASS_MODE="$case_name" \
	EXPECTED_MIN_FREEZE_COMMITS="$((REPEATED_CLICK_FREEZES + REPEATED_DRAG_FREEZES))" \
	EXPECTED_FREEZE_EDITABILITY="$expected_editability" \
	VISUAL_DRAG_SCREENSHOT_PATH="$drag_screenshot_paths" \
	VISUAL_DISPLAY_BOUNDS="$DISPLAY_BOUNDS" \
	VISUAL_DRAG_POINTS="$PATH_POINTS" \
	VISUAL_SCREENSHOT_PATH="$screenshot_path" \
	MASK_PROBE_PATH="$mask_probe_path" \
	SMOKE_STARTED_EPOCH="$case_started_epoch" \
	python3 "$SCRIPT_DIR/lib/native-visual-contract-summary.py" "$out_dir/all.log"
}

IFS=',' read -r -a VISUAL_CONTRACT_CASE_ARRAY <<<"$VISUAL_CONTRACT_CASES"
for case_name in "${VISUAL_CONTRACT_CASE_ARRAY[@]}"; do
	case_name="$(echo "$case_name" | tr -d '[:space:]')"
	[[ -n "$case_name" ]] || continue
	run_visual_case "$case_name"
done
echo "[smoke] PASS"
