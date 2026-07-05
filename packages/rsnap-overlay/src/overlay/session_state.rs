mod frozen_capture;
mod scroll_capture;

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
	Color32, DeviceCursorPointSource, FrozenSelectionInteractionKind, FrozenToolbarTool,
	GlobalPoint, MonitorRect, MouseScrollDelta, PhysicalPosition, Pos2, RectPoints, Vec2, WindowId,
};

pub(in crate::overlay) const FROZEN_BRUSH_STROKE_WIDTH_POINTS: f32 = 3.5;
pub(in crate::overlay) const FROZEN_BRUSH_STROKE_WIDTH_MIN_POINTS: f32 = 1.0;
pub(in crate::overlay) const FROZEN_BRUSH_STROKE_WIDTH_MAX_POINTS: f32 = 24.0;
pub(in crate::overlay) const FROZEN_TEXT_FONT_SIZE_POINTS: f32 = 16.0;
pub(in crate::overlay) const FROZEN_TEXT_FONT_SIZE_MIN_POINTS: f32 = 12.0;
pub(in crate::overlay) const FROZEN_TEXT_FONT_SIZE_MAX_POINTS: f32 = 72.0;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrozenAnnotationStyleCapsulePlacement {
	Above,
	Below,
}

#[derive(Clone, Debug)]
pub(super) struct FrozenToolbarState {
	pub(super) visible: bool,
	pub(super) dragging: bool,
	pub(super) drag_start_eligible: bool,
	pub(super) annotation_size_control_hovered: bool,
	pub(super) annotation_size_wheel_accumulator: f32,
	pub(super) annotation_style_capsule_placement: FrozenAnnotationStyleCapsulePlacement,
	pub(super) selected_tool: FrozenToolbarTool,
	pub(super) brush_style: FrozenBrushStyle,
	pub(super) text_style: FrozenTextStyle,
	pub(super) auto_center_available: bool,
	pub(super) undo_available: bool,
	pub(super) redo_available: bool,
	pub(super) scroll_capture_active: bool,
	pub(super) scroll_capture_available: bool,
	pub(super) final_capture_ready: bool,
	pub(super) pending_action: Option<FrozenToolbarTool>,
	pub(super) needs_redraw: bool,
	pub(super) pill_height_points: Option<f32>,
	// Both positions track the primary capsule anchor, never the full toolbar union origin.
	pub(super) default_slot_position: Option<Pos2>,
	pub(super) floating_position: Option<Pos2>,
	pub(super) layout_last_screen_size_points: Option<Vec2>,
	pub(super) layout_stable_frames: u8,
	pub(super) drag_offset: Vec2,
	pub(super) drag_anchor: Option<Pos2>,
}
impl FrozenToolbarState {
	fn consume_annotation_size_wheel_steps(&mut self, delta: &MouseScrollDelta) -> i32 {
		match delta {
			MouseScrollDelta::LineDelta(_, y) => {
				self.annotation_size_wheel_accumulator = 0.0;

				discrete_toolbar_wheel_steps(*y)
			},
			MouseScrollDelta::PixelDelta(position) => {
				self.annotation_size_wheel_accumulator +=
					(position.y as f32 / 24.0).clamp(-2.0, 2.0);

				let mut steps = 0_i32;

				while self.annotation_size_wheel_accumulator >= 1.0 {
					steps += 1;
					self.annotation_size_wheel_accumulator -= 1.0;
				}
				while self.annotation_size_wheel_accumulator <= -1.0 {
					steps -= 1;
					self.annotation_size_wheel_accumulator += 1.0;
				}

				steps
			},
		}
	}

	fn apply_text_size_wheel_steps(&mut self, steps: i32) -> bool {
		if steps == 0 {
			return false;
		}

		let mut next_size = self.text_style.font_size_points;

		for _ in 0..steps.abs() {
			next_size = if steps > 0 {
				if (next_size - next_size.round()).abs() <= f32::EPSILON {
					next_size + 1.0
				} else {
					next_size.ceil()
				}
			} else if (next_size - next_size.round()).abs() <= f32::EPSILON {
				next_size - 1.0
			} else {
				next_size.floor()
			};
		}

		self.text_style.set_font_size(next_size)
	}

	fn brush_size_wheel_step(&self) -> f32 {
		match self.brush_style.stroke_width_points {
			width if width < 4.0 => 0.25,
			width if width < 12.0 => 0.5,
			_ => 1.0,
		}
	}

	fn apply_brush_size_wheel_steps(&mut self, steps: i32) -> bool {
		if steps == 0 {
			return false;
		}

		let direction = steps.signum() as f32;
		let mut changed = false;

		for _ in 0..steps.abs() {
			changed |=
				self.brush_style.offset_stroke_width(direction * self.brush_size_wheel_step());
		}

		changed
	}

	pub(super) fn apply_annotation_size_steps(&mut self, steps: i32) -> bool {
		if steps == 0 {
			return false;
		}

		match self.selected_tool {
			FrozenToolbarTool::Pen | FrozenToolbarTool::Arrow => {
				self.apply_brush_size_wheel_steps(steps)
			},
			FrozenToolbarTool::Text => self.apply_text_size_wheel_steps(steps),
			_ => false,
		}
	}

	pub(super) fn apply_annotation_size_wheel_delta(&mut self, delta: &MouseScrollDelta) -> bool {
		if !self.annotation_size_control_hovered {
			self.annotation_size_wheel_accumulator = 0.0;

			return false;
		}

		let steps = self.consume_annotation_size_wheel_steps(delta);

		self.apply_annotation_size_steps(steps)
	}
}

impl Default for FrozenToolbarState {
	fn default() -> Self {
		Self {
			visible: true,
			dragging: false,
			drag_start_eligible: false,
			annotation_size_control_hovered: false,
			annotation_size_wheel_accumulator: 0.0,
			annotation_style_capsule_placement: FrozenAnnotationStyleCapsulePlacement::Below,
			selected_tool: FrozenToolbarTool::Pointer,
			brush_style: FrozenBrushStyle::default(),
			text_style: FrozenTextStyle::default(),
			auto_center_available: false,
			undo_available: false,
			redo_available: false,
			scroll_capture_active: false,
			scroll_capture_available: false,
			final_capture_ready: false,
			pending_action: None,
			needs_redraw: false,
			pill_height_points: None,
			default_slot_position: None,
			floating_position: None,
			layout_last_screen_size_points: None,
			layout_stable_frames: 0,
			drag_offset: Vec2::ZERO,
			drag_anchor: None,
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct FrozenBrushStroke {
	pub(super) points: Vec<Pos2>,
	pub(super) style: FrozenBrushStyle,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrozenBrushModelState {
	pub(super) filtered_input_point: Pos2,
	pub(super) modeled_point: Pos2,
	pub(super) modeled_velocity: Vec2,
	pub(super) modeled_elapsed_seconds: f32,
}

#[derive(Clone, Debug)]
pub(super) struct ActiveFrozenBrushStroke {
	pub(super) raw_points: Vec<Pos2>,
	pub(super) points: Vec<Pos2>,
	pub(super) style: FrozenBrushStyle,
	pub(super) model_state: FrozenBrushModelState,
	pub(super) started_at: Instant,
	pub(super) last_sample_at: Instant,
}

#[derive(Debug, Default)]
pub(super) struct FrozenBrushState {
	pub(super) committed_strokes: Vec<FrozenBrushStroke>,
	pub(super) redo_strokes: Vec<FrozenBrushStroke>,
	pub(super) active_stroke: Option<ActiveFrozenBrushStroke>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FrozenBrushStyle {
	pub(super) stroke_width_points: f32,
	pub(super) color: FrozenAnnotationColor,
}
impl FrozenBrushStyle {
	pub(super) fn set_stroke_width(&mut self, stroke_width_points: f32) -> bool {
		set_clamped_points(
			&mut self.stroke_width_points,
			stroke_width_points,
			FROZEN_BRUSH_STROKE_WIDTH_MIN_POINTS,
			FROZEN_BRUSH_STROKE_WIDTH_MAX_POINTS,
		)
	}

	pub(super) fn offset_stroke_width(&mut self, delta_points: f32) -> bool {
		self.set_stroke_width(self.stroke_width_points + delta_points)
	}
}

impl Default for FrozenBrushStyle {
	fn default() -> Self {
		Self {
			stroke_width_points: FROZEN_BRUSH_STROKE_WIDTH_POINTS,
			color: FrozenAnnotationColor::Blue,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FrozenTextStyle {
	pub(super) font_size_points: f32,
	pub(super) color: FrozenAnnotationColor,
}
impl FrozenTextStyle {
	pub(super) fn set_font_size(&mut self, font_size_points: f32) -> bool {
		set_clamped_points(
			&mut self.font_size_points,
			font_size_points,
			FROZEN_TEXT_FONT_SIZE_MIN_POINTS,
			FROZEN_TEXT_FONT_SIZE_MAX_POINTS,
		)
	}
}

impl Default for FrozenTextStyle {
	fn default() -> Self {
		Self { font_size_points: FROZEN_TEXT_FONT_SIZE_POINTS, color: FrozenAnnotationColor::Blue }
	}
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FrozenTextAnnotation {
	pub(super) anchor: Pos2,
	pub(super) text: String,
	pub(super) style: FrozenTextStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FrozenArrowAnnotation {
	pub(super) start: Pos2,
	pub(super) end: Pos2,
	pub(super) style: FrozenBrushStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FrozenSpotlightAnnotation {
	pub(super) rect: RectPoints,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FrozenTextEditState {
	pub(super) anchor: Pos2,
	pub(super) text: String,
	pub(super) ime_preedit: Option<String>,
	pub(super) ime_preedit_cursor_char_range: Option<(usize, usize)>,
	pub(super) caret_blink_started_at: Instant,
	pub(super) dragging: bool,
	pub(super) drag_offset: Vec2,
}
impl FrozenTextEditState {
	pub(super) fn new(anchor: Pos2) -> Self {
		Self::new_at(anchor, Instant::now())
	}

	pub(super) fn new_at(anchor: Pos2, caret_blink_started_at: Instant) -> Self {
		Self {
			anchor,
			text: String::new(),
			ime_preedit: None,
			ime_preedit_cursor_char_range: None,
			caret_blink_started_at,
			dragging: false,
			drag_offset: Vec2::ZERO,
		}
	}

	pub(super) fn visible_text(&self) -> String {
		self.visible_text_and_caret_char_index().0
	}

	pub(super) fn has_ime_preedit(&self) -> bool {
		self.ime_preedit.is_some()
	}

	pub(super) fn reset_caret_blink(&mut self) {
		self.reset_caret_blink_at(Instant::now());
	}

	pub(super) fn reset_caret_blink_at(&mut self, caret_blink_started_at: Instant) {
		self.caret_blink_started_at = caret_blink_started_at;
	}

	pub(super) fn caret_blink_elapsed_secs_at(&self, now: Instant) -> f64 {
		now.duration_since(self.caret_blink_started_at).as_secs_f64()
	}

	pub(super) fn visible_text_and_caret_char_index(&self) -> (String, Option<usize>) {
		let committed_char_count = self.text.chars().count();

		match self.ime_preedit.as_deref() {
			Some(preedit) if !preedit.is_empty() => {
				let mut visible = self.text.clone();

				visible.push_str(preedit);

				(
					visible,
					self.ime_preedit_cursor_char_range
						.map(|(_, end)| committed_char_count.saturating_add(end)),
				)
			},
			_ => (self.text.clone(), Some(committed_char_count)),
		}
	}

	pub(super) fn normalize_ime_preedit_cursor_char_range(
		preedit: &str,
		cursor_range: Option<(usize, usize)>,
	) -> Option<(usize, usize)> {
		let (start, end) = cursor_range?;

		Some((
			Self::char_index_from_byte_offset(preedit, start),
			Self::char_index_from_byte_offset(preedit, end),
		))
	}

	fn char_index_from_byte_offset(text: &str, byte_offset: usize) -> usize {
		let clamped = byte_offset.min(text.len());

		if clamped == text.len() {
			return text.chars().count();
		}

		text.char_indices().take_while(|(index, _)| *index < clamped).count()
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FrozenSelectionDragState {
	pub(super) active: bool,
	pub(super) interaction: FrozenSelectionInteractionKind,
	pub(super) anchor_rect: RectPoints,
	pub(super) pointer_offset_x: u32,
	pub(super) pointer_offset_y: u32,
	pub(super) press_cursor_x: u32,
	pub(super) press_cursor_y: u32,
}
impl Default for FrozenSelectionDragState {
	fn default() -> Self {
		Self {
			active: false,
			interaction: FrozenSelectionInteractionKind::Move,
			anchor_rect: RectPoints::new(0, 0, 0, 0),
			pointer_offset_x: 0,
			pointer_offset_y: 0,
			press_cursor_x: 0,
			press_cursor_y: 0,
		}
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct FrozenMosaicDragState {
	pub(super) active: bool,
	pub(super) anchor_x: u32,
	pub(super) anchor_y: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct FrozenArrowDragState {
	pub(super) active: bool,
	pub(super) anchor_x: u32,
	pub(super) anchor_y: u32,
	pub(super) current_x: u32,
	pub(super) current_y: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct FrozenSpotlightDragState {
	pub(super) active: bool,
	pub(super) anchor_x: u32,
	pub(super) anchor_y: u32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrozenToolbarPointerState {
	pub(super) cursor_local: Pos2,
	#[cfg(not(target_os = "macos"))]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrozenAnnotationColor {
	White,
	Yellow,
	Green,
	Blue,
	Red,
	Black,
}
impl FrozenAnnotationColor {
	pub(super) const ALL: [Self; 6] =
		[Self::White, Self::Yellow, Self::Green, Self::Blue, Self::Red, Self::Black];

	pub(super) const fn swatch_fill(self) -> Color32 {
		match self {
			Self::White => Color32::from_rgb(255, 255, 255),
			Self::Yellow => Color32::from_rgb(255, 219, 77),
			Self::Green => Color32::from_rgb(92, 214, 149),
			Self::Blue => Color32::from_rgb(102, 178, 255),
			Self::Red => Color32::from_rgb(255, 107, 107),
			Self::Black => Color32::from_rgb(24, 24, 24),
		}
	}

	pub(super) const fn export_rgba(self) -> [u8; 4] {
		let [r, g, b, a] = self.swatch_fill().to_array();

		[r, g, b, a]
	}
}

fn set_clamped_points(current_value: &mut f32, next_value: f32, min: f32, max: f32) -> bool {
	let next_size = next_value.clamp(min, max);

	if (next_size - *current_value).abs() <= f32::EPSILON {
		return false;
	}

	*current_value = next_size;

	true
}

fn discrete_toolbar_wheel_steps(units: f32) -> i32 {
	if units.abs() <= f32::EPSILON {
		return 0;
	}

	let magnitude = if units.abs() < 1.0 { 1.0 } else { units.abs().round() };

	units.signum() as i32 * magnitude as i32
}
