use crate::overlay::session_state::WindowFreezeCaptureTarget;
use crate::state::{MonitorRect, RectPoints, WindowHit};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::overlay) struct LiveClickCaptureTarget {
	pub(in crate::overlay) capture_rect: Option<RectPoints>,
	pub(in crate::overlay) window_target: Option<WindowFreezeCaptureTarget>,
}
impl LiveClickCaptureTarget {
	pub(in crate::overlay) fn fullscreen_fallback() -> Self {
		Self { capture_rect: None, window_target: None }
	}

	pub(in crate::overlay) fn from_window_hit(monitor: MonitorRect, hit: WindowHit) -> Self {
		Self {
			capture_rect: Some(hit.rect),
			window_target: hit.window_id.map(|window_id| WindowFreezeCaptureTarget {
				monitor,
				window_id,
				rect: hit.rect,
			}),
		}
	}
}
