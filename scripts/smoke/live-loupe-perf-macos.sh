#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/smoke/lib/live-loupe.sh
source "$SCRIPT_DIR/lib/live-loupe.sh"

case "${1:-}" in
  --help|-h)
    live_loupe_usage
    exit 0
    ;;
  --self-check)
    live_loupe_self_check
    exit $?
    ;;
  "")
    ;;
  *)
    live_loupe_usage >&2
    exit 2
    ;;
esac

live_loupe_self_check
ROOT_DIR="$(live_loupe_repo_root)"
live_loupe_init_environment "$ROOT_DIR"
live_loupe_install_trap

mkdir -p "$LOG_DIR"
live_loupe_stop_existing_rsnap
rm -f "$LOG_DIR"/rsnap*.log
live_loupe_launch_rsnap
live_loupe_wait_for_pattern 'Starting rsnap\.' "$WAIT_STARTUP_S" || live_loupe_fail "rsnap did not log startup"
live_loupe_trigger_capture_from_tray_menu
live_loupe_wait_for_pattern 'Capture overlay started\.' "$WAIT_OVERLAY_S" || live_loupe_fail "capture overlay did not start"
live_loupe_focus_rsnap_overlay

if [[ -z "$DISPLAY_BOUNDS" ]]; then
  DISPLAY_BOUNDS="$(live_loupe_read_main_display_bounds | tr -d ' ')"
fi
if [[ -z "$PATH_POINTS" ]]; then
  PATH_POINTS="$(live_loupe_derive_live_path_points "$DISPLAY_BOUNDS")"
fi

echo "[smoke] display bounds: $DISPLAY_BOUNDS"
echo "[smoke] path points: $PATH_POINTS"

sleep "$OVERLAY_SETTLE_S"
live_loupe_hold_option_key
live_loupe_run_mouse_path
live_loupe_release_option_key
live_loupe_capture_session_end_ts
sleep "$POST_PATH_SETTLE_S"
live_loupe_press_escape >/dev/null 2>&1 || true
sleep 0.2

live_loupe_refresh_log_path
if [[ -z "$RSNAP_LOG" || ! -f "$RSNAP_LOG" ]]; then
  live_loupe_fail "could not locate rsnap log"
fi

live_loupe_summarize_and_gate_log "$RSNAP_LOG" || live_loupe_fail "live perf metrics exceeded thresholds"

echo "[smoke] PASS"
rg -n 'Starting rsnap\.|Capture requested from tray menu\.|Capture overlay started\.|op="overlay.window_renderer_acquire_frame"|op="overlay.event_loop_stall"|WindowEvent::Resized|op="overlay\.hud_redraw|op="overlay\.loupe_redraw|op="overlay\.hud_window_set_outer_position"|op="overlay\.loupe_window_set_outer_position"|Slow operation detected' "$RSNAP_LOG" | tail -n 120 || true
