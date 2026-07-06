mod frozen_annotation;
mod frozen_capture;
mod scroll_capture;

pub(super) use self::frozen_annotation::{
	ActiveFrozenBrushStroke, FrozenAnnotationColor, FrozenAnnotationStyleCapsulePlacement,
	FrozenArrowAnnotation, FrozenArrowDragState, FrozenBrushModelState, FrozenBrushState,
	FrozenBrushStroke, FrozenBrushStyle, FrozenMosaicDragState, FrozenSelectionDragState,
	FrozenSpotlightAnnotation, FrozenSpotlightDragState, FrozenTextAnnotation, FrozenTextEditState,
	FrozenTextStyle, FrozenToolbarPointerState, FrozenToolbarState,
};
pub(super) use self::frozen_capture::{
	FrozenCaptureSessionState, FrozenCaptureWorkerState, FrozenExportSessionState,
	WindowFreezeCaptureTarget,
};
pub(super) use self::scroll_capture::ScrollCaptureState;
#[cfg(target_os = "macos")]
pub(super) use self::scroll_capture::{
	InflightScrollCaptureObservation, LiveStreamStaleGrace, MacOSScrollPixelResidual,
	MacOSScrollWheelEvent, ScrollCaptureLiveFrame,
};

use std::{
	collections::HashMap,
	time::{Duration, Instant},
};

use crate::overlay::runtime_timing::{
	LIVE_PRESENT_INTERVAL_MIN, REDRAW_SUBSTEP_CONTRIBUTION_FLOOR, SLOW_OP_WARN_INTERVAL,
};
use crate::overlay::{
	DeviceCursorPointSource, GlobalPoint, MonitorRect, PhysicalPosition, WindowId,
};

pub(in crate::overlay) const FROZEN_BRUSH_STROKE_WIDTH_POINTS: f32 =
	self::frozen_annotation::FROZEN_BRUSH_STROKE_WIDTH_POINTS;
#[cfg(test)]
pub(in crate::overlay) const FROZEN_BRUSH_STROKE_WIDTH_MIN_POINTS: f32 =
	self::frozen_annotation::FROZEN_BRUSH_STROKE_WIDTH_MIN_POINTS;
#[cfg(test)]
pub(in crate::overlay) const FROZEN_BRUSH_STROKE_WIDTH_MAX_POINTS: f32 =
	self::frozen_annotation::FROZEN_BRUSH_STROKE_WIDTH_MAX_POINTS;
#[cfg(test)]
pub(in crate::overlay) const FROZEN_TEXT_FONT_SIZE_POINTS: f32 =
	self::frozen_annotation::FROZEN_TEXT_FONT_SIZE_POINTS;
#[cfg(test)]
pub(in crate::overlay) const FROZEN_TEXT_FONT_SIZE_MIN_POINTS: f32 =
	self::frozen_annotation::FROZEN_TEXT_FONT_SIZE_MIN_POINTS;
#[cfg(test)]
pub(in crate::overlay) const FROZEN_TEXT_FONT_SIZE_MAX_POINTS: f32 =
	self::frozen_annotation::FROZEN_TEXT_FONT_SIZE_MAX_POINTS;

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

#[derive(Clone, Copy)]
pub(super) struct FrozenSelectionDragCursorMoveTiming {
	pub(super) cursor_update_elapsed: Duration,
	pub(super) live_drag_update_elapsed: Duration,
	pub(super) frozen_drag_update_elapsed: Duration,
	pub(super) frozen_rect_changed: bool,
	pub(super) sync_cursor_icons_elapsed: Duration,
	pub(super) request_samples_elapsed: Duration,
	pub(super) total_elapsed: Duration,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct HudDrawConfig {
	pub(super) can_draw_hud: bool,
	pub(super) needs_surface_bg: bool,
	pub(super) needs_shader_blur_bg: bool,
	pub(super) hud_glass_active: bool,
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
