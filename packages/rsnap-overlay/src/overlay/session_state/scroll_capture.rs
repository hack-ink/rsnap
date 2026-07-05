#[cfg(target_os = "macos")]
use std::collections::VecDeque;
use std::time::Instant;

use image::RgbaImage;

#[cfg(target_os = "macos")]
use crate::overlay::{ExternalScrollInputDrainReader, MacLiveFrameStream};
use crate::overlay::{
	MonitorRect, RectPoints, ScrollCaptureTraceRecorder, ScrollDirection, ScrollSession,
};

#[derive(Default)]
pub(in crate::overlay) struct ScrollCaptureState {
	pub(in crate::overlay) active: bool,
	pub(in crate::overlay) paused: bool,
	pub(in crate::overlay) monitor: Option<MonitorRect>,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) capture_rect_points: Option<RectPoints>,
	pub(in crate::overlay) capture_rect_pixels: Option<RectPoints>,
	pub(in crate::overlay) input_direction: Option<ScrollDirection>,
	pub(in crate::overlay) input_direction_at: Option<Instant>,
	pub(in crate::overlay) input_gesture_active: bool,
	pub(in crate::overlay) downward_motion_rows_pending: f64,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) overlay_mouse_passthrough_active: bool,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) overlay_mouse_passthrough_persistent: bool,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) overlay_mouse_passthrough_until: Option<Instant>,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) external_scroll_input_drain_reader:
		Option<ExternalScrollInputDrainReader>,
	pub(in crate::overlay) last_external_scroll_input_seq: u64,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) pixel_delta_residual: MacOSScrollPixelResidual,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) live_stream: Option<MacLiveFrameStream>,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) live_stream_backlog: VecDeque<ScrollCaptureLiveFrame>,
	pub(in crate::overlay) last_stream_frame_seq: u64,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) last_stream_frame_fingerprint: Option<Vec<u8>>,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) consecutive_identical_stream_frames: u8,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) last_consumed_stream_frame_captured_at: Option<Instant>,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) last_stream_event_at: Option<Instant>,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) last_stream_poll_at: Option<Instant>,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) last_duplicate_stream_refresh_at: Option<Instant>,
	pub(in crate::overlay) pending_post_stall_burst_after_seq: Option<u64>,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) live_stream_stale_grace: Option<LiveStreamStaleGrace>,
	pub(in crate::overlay) next_sample_at: Option<Instant>,
	pub(in crate::overlay) next_request_id: u64,
	pub(in crate::overlay) inflight_request_id: Option<u64>,
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) inflight_request_observation: Option<InflightScrollCaptureObservation>,
	#[cfg(all(test, target_os = "macos"))]
	pub(in crate::overlay) force_worker_sampling_in_tests: bool,
	pub(in crate::overlay) session: Option<ScrollSession>,
	pub(in crate::overlay) preview_committed_image: Option<RgbaImage>,
	pub(in crate::overlay) preview_latest_frame: Option<RgbaImage>,
	pub(in crate::overlay) preview_display_image: Option<RgbaImage>,
	pub(in crate::overlay) retained_overlay_preview_image: Option<RgbaImage>,
	pub(in crate::overlay) retained_overlay_preview_motion_rows_hint: Option<u32>,
	pub(in crate::overlay) last_overlay_preview_motion_rows_hint: Option<u32>,
	pub(in crate::overlay) last_overlay_preview_provisional_motion_rows_hint: Option<u32>,
	pub(in crate::overlay) last_overlay_preview_existing_candidate_height: Option<u32>,
	pub(in crate::overlay) last_overlay_preview_existing_candidate_motion_rows_hint: Option<u32>,
	pub(in crate::overlay) last_overlay_preview_ledger_candidate_height: Option<u32>,
	pub(in crate::overlay) last_overlay_preview_ledger_candidate_motion_rows_hint: Option<u32>,
	pub(in crate::overlay) last_overlay_preview_retained_candidate_height: Option<u32>,
	pub(in crate::overlay) last_overlay_preview_retained_candidate_motion_rows_hint: Option<u32>,
	pub(in crate::overlay) last_overlay_preview_retained_hint_matches_motion_rows: bool,
	pub(in crate::overlay) last_overlay_preview_fresh_latest_frame_can_drive: bool,
	pub(in crate::overlay) last_overlay_preview_strong_unresolved_registration: bool,
	pub(in crate::overlay) last_overlay_preview_latest_frame_present: bool,
	pub(in crate::overlay) last_overlay_preview_used_provisional: bool,
	pub(in crate::overlay) trace_recorder: Option<ScrollCaptureTraceRecorder>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug)]
pub(in crate::overlay) struct ScrollCaptureLiveFrame {
	pub(in crate::overlay) frame_seq: u64,
	pub(in crate::overlay) captured_at: Instant,
	pub(in crate::overlay) image: RgbaImage,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in crate::overlay) struct InflightScrollCaptureObservation {
	pub(in crate::overlay) was_observable: bool,
	pub(in crate::overlay) external_input_seq: u64,
	pub(in crate::overlay) input_direction: Option<ScrollDirection>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::overlay) struct LiveStreamStaleGrace {
	pub(in crate::overlay) external_input_seq: u64,
	pub(in crate::overlay) remaining_stale_frames: u8,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in crate::overlay) struct MacOSScrollPixelResidual {
	pub(in crate::overlay) x: f64,
	pub(in crate::overlay) y: f64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::overlay) struct MacOSScrollWheelEvent {
	pub(in crate::overlay) units: u32,
	pub(in crate::overlay) normalized_x: f64,
	pub(in crate::overlay) normalized_y: f64,
	pub(in crate::overlay) posted_x: i32,
	pub(in crate::overlay) posted_y: i32,
	pub(in crate::overlay) residual: MacOSScrollPixelResidual,
}
