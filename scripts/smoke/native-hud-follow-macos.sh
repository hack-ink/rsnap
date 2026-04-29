#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/smoke/lib/live-hud.sh
source "$SCRIPT_DIR/lib/live-hud.sh"

usage() {
	cat <<'EOF'
Usage: native-hud-follow-macos.sh [--self-check] [--help]

Runs the native macOS host HUD-follow performance smoke:
  1. build and launch the native host app
  2. start capture with the configured Option-X hotkey
  3. drive a smooth high-rate cursor path
  4. collect native-host telemetry and gate HUD-follow metrics

Useful overrides:
  PATH_MODE=smooth|waypoints       default: smooth
  PATH_DRIVER=event|warp           cursor driver; event exercises app input
  PATH_RATE_HZ=120                 smooth path event rate
  PATH_DURATION_MS=2500            smooth path duration
  PATH_CYCLES=3                    smooth path lissajous cycles
  HUD_FOLLOW_CASES=hud,loupe       run collapsed HUD and expanded loupe phases
  MAX_SAMPLE_REFRESH_GAP_P95_MS    default: pointer/sample target budget + 1ms
  MAX_ACTIVE_LAYER_CHROME_RENDER_GAP_P95_MS default: active display target budget + 1ms
  MAX_LAYER_CHROME_RENDER_DURATION_P95_MS default: active display target budget
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
USER_PATH_MODE="${PATH_MODE:-}"
USER_PATH_DRIVER="${PATH_DRIVER:-}"
USER_PATH_DURATION_MS="${PATH_DURATION_MS:-}"
USER_PATH_RATE_HZ="${PATH_RATE_HZ:-}"
USER_PATH_CYCLES="${PATH_CYCLES:-}"
USER_HUD_FOLLOW_CASES="${HUD_FOLLOW_CASES:-}"
live_hud_init_environment "$ROOT_DIR"

PATH_MODE="${USER_PATH_MODE:-smooth}"
PATH_DRIVER="${USER_PATH_DRIVER:-event}"
PATH_DURATION_MS="${USER_PATH_DURATION_MS:-2500}"
PATH_RATE_HZ="${USER_PATH_RATE_HZ:-120}"
PATH_CYCLES="${USER_PATH_CYCLES:-3}"
HUD_FOLLOW_CASES="${USER_HUD_FOLLOW_CASES:-hud,loupe}"
OVERLAY_SETTLE_S="${OVERLAY_SETTLE_S:-0.35}"
POST_PATH_SETTLE_S="${POST_PATH_SETTLE_S:-0.6}"
POST_CLOSE_SETTLE_S="${POST_CLOSE_SETTLE_S:-0.25}"
RSNAP_TELEMETRY_LAST="${RSNAP_TELEMETRY_LAST:-3m}"
export PATH_MODE PATH_DRIVER PATH_DURATION_MS PATH_RATE_HZ PATH_CYCLES RSNAP_TELEMETRY_LAST

"$ROOT_DIR/scripts/build_and_run.sh" verify >/tmp/rsnap-native-hud-follow-build.out
sleep 0.4

osascript <<'APPLESCRIPT' >/dev/null
tell application "System Events"
	key code 7 using option down
end tell
APPLESCRIPT
sleep 0.2
live_hud_focus_rsnap_overlay

if [[ -z "$DISPLAY_BOUNDS" ]]; then
	DISPLAY_BOUNDS="$(live_hud_read_main_display_bounds | tr -d ' ')"
fi
if [[ -z "$PATH_POINTS" ]]; then
	PATH_POINTS="$(live_hud_derive_live_path_points "$DISPLAY_BOUNDS")"
fi

echo "[smoke] display bounds: $DISPLAY_BOUNDS"
echo "[smoke] path mode: $PATH_MODE driver=$PATH_DRIVER rate_hz=$PATH_RATE_HZ duration_ms=$PATH_DURATION_MS cycles=$PATH_CYCLES"
echo "[smoke] cases: $HUD_FOLLOW_CASES"
echo "[smoke] path points: $PATH_POINTS"

sleep "$OVERLAY_SETTLE_S"
IFS=',' read -r -a HUD_FOLLOW_CASE_ARRAY <<<"$HUD_FOLLOW_CASES"
for case_name in "${HUD_FOLLOW_CASE_ARRAY[@]}"; do
	case_name="$(echo "$case_name" | tr -d '[:space:]')"
	case "$case_name" in
		hud)
			echo "[smoke] phase: hud"
			live_hud_run_mouse_path
			;;
		loupe)
			echo "[smoke] phase: loupe"
			live_hud_focus_rsnap_overlay
			live_hud_press_tab
			# Let the loupe patch populate once before testing expanded HUD rendering.
			sleep 0.2
			live_hud_run_mouse_path
			;;
		"")
			;;
		*)
			echo "unknown HUD follow case: $case_name" >&2
			exit 2
			;;
	esac
done
sleep "$POST_PATH_SETTLE_S"
live_hud_focus_rsnap_overlay
live_hud_press_escape >/dev/null 2>&1 || true
sleep "$POST_CLOSE_SETTLE_S"

OUT_DIR="$("$ROOT_DIR/scripts/telemetry/native-host.sh" collect)"
echo "[smoke] telemetry: $OUT_DIR"
python3 "$SCRIPT_DIR/lib/native-hud-follow-summary.py" "$OUT_DIR/all.log"
echo "[smoke] PASS"
