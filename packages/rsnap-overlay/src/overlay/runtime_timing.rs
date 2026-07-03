use std::time::Duration;

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
