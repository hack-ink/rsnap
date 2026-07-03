use std::time::Duration;

pub(in crate::overlay) const LIVE_EVENT_CURSOR_CACHE_TTL: Duration = Duration::from_millis(120);
pub(in crate::overlay) const CURSOR_EVENT_TICK_TTL: Duration = Duration::from_millis(24);
pub(in crate::overlay) const LIVE_HOVER_HIT_TEST_INTERVAL: Duration = Duration::from_millis(60);
pub(in crate::overlay) const LIVE_WINDOW_LIST_REFRESH_INTERVAL: Duration =
	Duration::from_millis(120);
pub(in crate::overlay) const PENDING_CLICK_HIT_TEST_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(target_os = "macos")]
pub(in crate::overlay) const DISPLAY_FIRST_FREEZE_LIVE_TIMEOUT: Duration =
	Duration::from_millis(600);
pub(in crate::overlay) const LIVE_PRESENT_INTERVAL_MIN: Duration = Duration::from_nanos(8_333_333);
pub(in crate::overlay) const HUD_LOUPE_MOVE_INTERVAL_MIN: Duration = LIVE_PRESENT_INTERVAL_MIN;
pub(in crate::overlay) const CURSOR_POLL_INTERVAL_MIN: Duration = LIVE_PRESENT_INTERVAL_MIN;
pub(in crate::overlay) const LOUPE_WINDOW_WARMUP_REDRAWS: u8 = 30;
pub(in crate::overlay) const INTERACTIVE_REPAINT_TARGET_FPS: f32 = 120.0;
pub(in crate::overlay) const OCCLUDED_FRAME_REDRAW_RETRY_WINDOW: Duration = Duration::from_secs(2);
pub(in crate::overlay) const OVERLAY_EVENT_LOOP_STALL_THRESHOLD: Duration =
	Duration::from_millis(250);
pub(in crate::overlay) const REDRAW_SUBSTEP_CONTRIBUTION_FLOOR: Duration = Duration::from_millis(4);
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SLOW_OP_WARN_CURSOR_LOCATION: Duration = Duration::from_millis(8);
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SLOW_OP_WARN_HUD_CONFIG: Duration = Duration::from_millis(40);
pub(in crate::overlay) const SLOW_OP_WARN_INTERVAL: Duration = Duration::from_secs(1);
pub(in crate::overlay) const SLOW_OP_WARN_OUTER_POSITION: Duration = Duration::from_millis(24);
pub(in crate::overlay) const SLOW_OP_WARN_RENDER: Duration = Duration::from_millis(24);
pub(in crate::overlay) const SLOW_OP_WARN_WINDOW_EVENT: Duration = Duration::from_millis(40);
