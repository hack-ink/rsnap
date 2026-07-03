use std::time::Duration;

#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_STREAM_EVENT_FALLBACK_POLL_INTERVAL: Duration =
	Duration::from_millis(40);
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_STREAM_POLL_INTERVAL: Duration =
	Duration::from_millis(8);
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_STREAM_BACKLOG_MAX_FRAMES: usize = 12;
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW: Duration =
	Duration::from_millis(320);
// macOS trackpad/wheel sequences can keep delivering usable follow-up frames after the
// initiating input event. Keep the observation window wide enough for the capture pipeline
// to pair those frames before declaring the input stale.
pub(in crate::overlay) const SCROLL_CAPTURE_INPUT_FRESHNESS: Duration = Duration::from_millis(600);
pub(in crate::overlay) const SCROLL_CAPTURE_INPUT_MOTION_PRIOR_ROWS_MAX: f64 = 4_096.0;
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES: u8 = 5;
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_DUPLICATE_STREAM_STALL_THRESHOLD: u8 = 3;
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_DUPLICATE_STREAM_REFRESH_INTERVAL: Duration =
	Duration::from_millis(80);
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
#[cfg(not(target_os = "macos"))]
pub(in crate::overlay) const SCROLL_CAPTURE_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_DUPLICATE_WORKER_FRAME_RETRY_INTERVAL: Duration =
	Duration::from_millis(60);
#[cfg(target_os = "macos")]
pub(in crate::overlay) const SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE: Duration =
	Duration::from_millis(180);
