use std::{
	collections::HashMap,
	time::{Duration, Instant},
};

use crate::overlay::{
	DeviceCursorPointSource, FrozenToolbarTool, GlobalPoint, LIVE_PRESENT_INTERVAL_MIN,
	MonitorRect, PhysicalPosition, Pos2, REDRAW_SUBSTEP_CONTRIBUTION_FLOOR, RectPoints,
	SLOW_OP_WARN_INTERVAL, ScrollDirection, ScrollSession, Vec2, WindowId,
};
#[cfg(target_os = "macos")]
use crate::overlay::{ExternalScrollInputDrainReader, MacLiveFrameStream};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WindowFreezeCaptureTarget {
	pub(super) monitor: MonitorRect,
	pub(super) window_id: u32,
	pub(super) rect: RectPoints,
}

#[derive(Default)]
pub(super) struct SlowOperationLogger {
	last_warn_at: HashMap<&'static str, Instant>,
}
impl SlowOperationLogger {
	pub(super) fn warn_if_slow<F>(
		&mut self,
		op: &'static str,
		elapsed: Duration,
		threshold: Duration,
		describe: F,
	) where
		F: FnOnce() -> String,
	{
		if elapsed < threshold {
			return;
		}

		let now = Instant::now();
		let should_log = self
			.last_warn_at
			.get(op)
			.is_none_or(|last| now.duration_since(*last) >= SLOW_OP_WARN_INTERVAL);

		if !should_log {
			return;
		}

		let details = describe();

		tracing::warn!(op = op, elapsed_ms = elapsed.as_millis(), details = %details, "Slow operation detected");

		let _ = self.last_warn_at.insert(op, now);
	}

	pub(super) fn warn_if_redraw_substep_slow<F>(
		&mut self,
		op: &'static str,
		elapsed: Duration,
		total: Duration,
		describe: F,
	) where
		F: FnOnce() -> String,
	{
		let exceeds_frame_budget = elapsed >= LIVE_PRESENT_INTERVAL_MIN;
		let materially_contributes = total >= LIVE_PRESENT_INTERVAL_MIN
			&& elapsed >= REDRAW_SUBSTEP_CONTRIBUTION_FLOOR
			&& elapsed.as_nanos().saturating_mul(2) >= total.as_nanos();

		if !exceeds_frame_budget && !materially_contributes {
			return;
		}

		self.warn_if_slow(op, elapsed, Duration::ZERO, || {
			format!("handler_total_ms={} {}", total.as_millis(), describe())
		});
	}
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Default)]
pub(super) struct MacOSHudWindowConfigState {
	blur_enabled: bool,
	blur_amount_bits: u32,
	corner_radius_bits: u64,
}
#[cfg(target_os = "macos")]
impl MacOSHudWindowConfigState {
	pub(super) fn new(blur_enabled: bool, blur_amount: f32, corner_radius: f64) -> Self {
		Self {
			blur_enabled,
			blur_amount_bits: blur_amount.to_bits(),
			corner_radius_bits: corner_radius.to_bits(),
		}
	}

	pub(super) fn same(&self, other: &Self) -> bool {
		self.blur_enabled == other.blur_enabled
			&& self.blur_amount_bits == other.blur_amount_bits
			&& self.corner_radius_bits == other.corner_radius_bits
	}
}

#[derive(Clone, Copy)]
pub(super) struct CursorMoveTrace {
	pub(super) window_id: WindowId,
	pub(super) position: PhysicalPosition<f64>,
	pub(super) old_cursor: Option<GlobalPoint>,
	pub(super) device_cursor: GlobalPoint,
	pub(super) event_global: GlobalPoint,
	pub(super) monitor: MonitorRect,
	pub(super) global: GlobalPoint,
	pub(super) source: DeviceCursorPointSource,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct HudDrawConfig {
	pub(super) can_draw_hud: bool,
	pub(super) needs_frozen_surface_bg: bool,
	pub(super) needs_shader_blur_bg: bool,
	pub(super) hud_glass_active: bool,
}

#[derive(Debug)]
pub(super) struct FrozenToolbarState {
	pub(super) visible: bool,
	pub(super) dragging: bool,
	pub(super) selected_tool: FrozenToolbarTool,
	pub(super) scroll_capture_active: bool,
	pub(super) scroll_capture_available: bool,
	pub(super) pending_action: Option<FrozenToolbarTool>,
	pub(super) needs_redraw: bool,
	pub(super) pill_height_points: Option<f32>,
	pub(super) floating_position: Option<Pos2>,
	pub(super) layout_last_screen_size_points: Option<Vec2>,
	pub(super) layout_stable_frames: u8,
	pub(super) drag_offset: Vec2,
	pub(super) drag_anchor: Option<Pos2>,
}
impl Default for FrozenToolbarState {
	fn default() -> Self {
		Self {
			visible: true,
			dragging: false,
			selected_tool: FrozenToolbarTool::Pointer,
			scroll_capture_active: false,
			scroll_capture_available: false,
			pending_action: None,
			needs_redraw: false,
			pill_height_points: None,
			floating_position: None,
			layout_last_screen_size_points: None,
			layout_stable_frames: 0,
			drag_offset: Vec2::ZERO,
			drag_anchor: None,
		}
	}
}

#[derive(Default)]
pub(super) struct ScrollCaptureState {
	pub(super) active: bool,
	pub(super) paused: bool,
	pub(super) monitor: Option<MonitorRect>,
	pub(super) capture_rect_pixels: Option<RectPoints>,
	pub(super) input_direction: Option<ScrollDirection>,
	pub(super) input_direction_at: Option<Instant>,
	pub(super) input_gesture_active: bool,
	#[cfg(target_os = "macos")]
	pub(super) overlay_mouse_passthrough_active: bool,
	#[cfg(target_os = "macos")]
	pub(super) overlay_mouse_passthrough_until: Option<Instant>,
	#[cfg(target_os = "macos")]
	pub(super) external_scroll_input_drain_reader: Option<ExternalScrollInputDrainReader>,
	#[cfg(target_os = "macos")]
	pub(super) last_external_scroll_input_seq: u64,
	#[cfg(target_os = "macos")]
	pub(super) pixel_delta_residual: MacOSScrollPixelResidual,
	#[cfg(target_os = "macos")]
	pub(super) live_stream: Option<MacLiveFrameStream>,
	#[cfg(target_os = "macos")]
	pub(super) last_stream_frame_seq: u64,
	#[cfg(target_os = "macos")]
	pub(super) live_stream_stale_grace: Option<LiveStreamStaleGrace>,
	#[cfg(not(target_os = "macos"))]
	pub(super) next_sample_at: Option<Instant>,
	#[cfg(not(target_os = "macos"))]
	pub(super) next_request_id: u64,
	pub(super) inflight_request_id: Option<u64>,
	#[cfg(target_os = "macos")]
	pub(super) inflight_request_observation: Option<InflightScrollCaptureObservation>,
	pub(super) session: Option<ScrollSession>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct InflightScrollCaptureObservation {
	pub(super) input_direction: Option<ScrollDirection>,
	pub(super) was_observable: bool,
	pub(super) external_input_seq: u64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LiveStreamStaleGrace {
	pub(super) external_input_seq: u64,
	pub(super) input_direction: ScrollDirection,
	pub(super) remaining_stale_frames: u8,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct MacOSScrollPixelResidual {
	pub(super) x: f64,
	pub(super) y: f64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MacOSScrollWheelEvent {
	pub(super) units: u32,
	pub(super) normalized_x: f64,
	pub(super) normalized_y: f64,
	pub(super) posted_x: i32,
	pub(super) posted_y: i32,
	pub(super) residual: MacOSScrollPixelResidual,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrozenToolbarPointerState {
	pub(super) cursor_local: Pos2,
	pub(super) left_button_down: bool,
	pub(super) left_button_went_down: bool,
	pub(super) left_button_went_up: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct LiveSampleApplyResult {
	pub(super) overlay_changed: bool,
	pub(super) hud_changed: bool,
	pub(super) loupe_changed: bool,
}
impl LiveSampleApplyResult {
	pub(super) fn any_changed(self) -> bool {
		self.overlay_changed || self.hud_changed || self.loupe_changed
	}
}
