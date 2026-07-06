use std::time::Instant;

use crate::overlay::{
	Color32, FrozenSelectionInteractionKind, FrozenToolbarTool, MouseScrollDelta, Pos2, RectPoints,
	Vec2,
};

pub(in crate::overlay) const FROZEN_BRUSH_STROKE_WIDTH_POINTS: f32 = 3.5;
pub(in crate::overlay) const FROZEN_BRUSH_STROKE_WIDTH_MIN_POINTS: f32 = 1.0;
pub(in crate::overlay) const FROZEN_BRUSH_STROKE_WIDTH_MAX_POINTS: f32 = 24.0;
pub(in crate::overlay) const FROZEN_TEXT_FONT_SIZE_POINTS: f32 = 16.0;
pub(in crate::overlay) const FROZEN_TEXT_FONT_SIZE_MIN_POINTS: f32 = 12.0;
pub(in crate::overlay) const FROZEN_TEXT_FONT_SIZE_MAX_POINTS: f32 = 72.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::overlay) enum FrozenAnnotationStyleCapsulePlacement {
	Above,
	Below,
}

#[derive(Clone, Debug)]
pub(in crate::overlay) struct FrozenToolbarState {
	pub(in crate::overlay) visible: bool,
	pub(in crate::overlay) dragging: bool,
	pub(in crate::overlay) drag_start_eligible: bool,
	pub(in crate::overlay) annotation_size_control_hovered: bool,
	pub(in crate::overlay) annotation_size_wheel_accumulator: f32,
	pub(in crate::overlay) annotation_style_capsule_placement:
		FrozenAnnotationStyleCapsulePlacement,
	pub(in crate::overlay) selected_tool: FrozenToolbarTool,
	pub(in crate::overlay) brush_style: FrozenBrushStyle,
	pub(in crate::overlay) text_style: FrozenTextStyle,
	pub(in crate::overlay) auto_center_available: bool,
	pub(in crate::overlay) undo_available: bool,
	pub(in crate::overlay) redo_available: bool,
	pub(in crate::overlay) scroll_capture_active: bool,
	pub(in crate::overlay) scroll_capture_available: bool,
	pub(in crate::overlay) final_capture_ready: bool,
	pub(in crate::overlay) pending_action: Option<FrozenToolbarTool>,
	pub(in crate::overlay) needs_redraw: bool,
	pub(in crate::overlay) pill_height_points: Option<f32>,
	// Both positions track the primary capsule anchor, never the full toolbar union origin.
	pub(in crate::overlay) default_slot_position: Option<Pos2>,
	pub(in crate::overlay) floating_position: Option<Pos2>,
	pub(in crate::overlay) layout_last_screen_size_points: Option<Vec2>,
	pub(in crate::overlay) layout_stable_frames: u8,
	pub(in crate::overlay) drag_offset: Vec2,
	pub(in crate::overlay) drag_anchor: Option<Pos2>,
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

	pub(in crate::overlay) fn apply_annotation_size_steps(&mut self, steps: i32) -> bool {
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

	pub(in crate::overlay) fn apply_annotation_size_wheel_delta(
		&mut self,
		delta: &MouseScrollDelta,
	) -> bool {
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
pub(in crate::overlay) struct FrozenBrushStroke {
	pub(in crate::overlay) points: Vec<Pos2>,
	pub(in crate::overlay) style: FrozenBrushStyle,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::overlay) struct FrozenBrushModelState {
	pub(in crate::overlay) filtered_input_point: Pos2,
	pub(in crate::overlay) modeled_point: Pos2,
	pub(in crate::overlay) modeled_velocity: Vec2,
	pub(in crate::overlay) modeled_elapsed_seconds: f32,
}

#[derive(Clone, Debug)]
pub(in crate::overlay) struct ActiveFrozenBrushStroke {
	pub(in crate::overlay) raw_points: Vec<Pos2>,
	pub(in crate::overlay) points: Vec<Pos2>,
	pub(in crate::overlay) style: FrozenBrushStyle,
	pub(in crate::overlay) model_state: FrozenBrushModelState,
	pub(in crate::overlay) started_at: Instant,
	pub(in crate::overlay) last_sample_at: Instant,
}

#[derive(Debug, Default)]
pub(in crate::overlay) struct FrozenBrushState {
	pub(in crate::overlay) committed_strokes: Vec<FrozenBrushStroke>,
	pub(in crate::overlay) redo_strokes: Vec<FrozenBrushStroke>,
	pub(in crate::overlay) active_stroke: Option<ActiveFrozenBrushStroke>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::overlay) struct FrozenBrushStyle {
	pub(in crate::overlay) stroke_width_points: f32,
	pub(in crate::overlay) color: FrozenAnnotationColor,
}
impl FrozenBrushStyle {
	pub(in crate::overlay) fn set_stroke_width(&mut self, stroke_width_points: f32) -> bool {
		set_clamped_points(
			&mut self.stroke_width_points,
			stroke_width_points,
			FROZEN_BRUSH_STROKE_WIDTH_MIN_POINTS,
			FROZEN_BRUSH_STROKE_WIDTH_MAX_POINTS,
		)
	}

	pub(in crate::overlay) fn offset_stroke_width(&mut self, delta_points: f32) -> bool {
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
pub(in crate::overlay) struct FrozenTextStyle {
	pub(in crate::overlay) font_size_points: f32,
	pub(in crate::overlay) color: FrozenAnnotationColor,
}
impl FrozenTextStyle {
	pub(in crate::overlay) fn set_font_size(&mut self, font_size_points: f32) -> bool {
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
pub(in crate::overlay) struct FrozenTextAnnotation {
	pub(in crate::overlay) anchor: Pos2,
	pub(in crate::overlay) text: String,
	pub(in crate::overlay) style: FrozenTextStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::overlay) struct FrozenArrowAnnotation {
	pub(in crate::overlay) start: Pos2,
	pub(in crate::overlay) end: Pos2,
	pub(in crate::overlay) style: FrozenBrushStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::overlay) struct FrozenSpotlightAnnotation {
	pub(in crate::overlay) rect: RectPoints,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::overlay) struct FrozenTextEditState {
	pub(in crate::overlay) anchor: Pos2,
	pub(in crate::overlay) text: String,
	pub(in crate::overlay) ime_preedit: Option<String>,
	pub(in crate::overlay) ime_preedit_cursor_char_range: Option<(usize, usize)>,
	pub(in crate::overlay) caret_blink_started_at: Instant,
	pub(in crate::overlay) dragging: bool,
	pub(in crate::overlay) drag_offset: Vec2,
}
impl FrozenTextEditState {
	pub(in crate::overlay) fn new(anchor: Pos2) -> Self {
		Self::new_at(anchor, Instant::now())
	}

	pub(in crate::overlay) fn new_at(anchor: Pos2, caret_blink_started_at: Instant) -> Self {
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

	pub(in crate::overlay) fn visible_text(&self) -> String {
		self.visible_text_and_caret_char_index().0
	}

	pub(in crate::overlay) fn has_ime_preedit(&self) -> bool {
		self.ime_preedit.is_some()
	}

	pub(in crate::overlay) fn reset_caret_blink(&mut self) {
		self.reset_caret_blink_at(Instant::now());
	}

	pub(in crate::overlay) fn reset_caret_blink_at(&mut self, caret_blink_started_at: Instant) {
		self.caret_blink_started_at = caret_blink_started_at;
	}

	pub(in crate::overlay) fn caret_blink_elapsed_secs_at(&self, now: Instant) -> f64 {
		now.duration_since(self.caret_blink_started_at).as_secs_f64()
	}

	pub(in crate::overlay) fn visible_text_and_caret_char_index(&self) -> (String, Option<usize>) {
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

	pub(in crate::overlay) fn normalize_ime_preedit_cursor_char_range(
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
pub(in crate::overlay) struct FrozenSelectionDragState {
	pub(in crate::overlay) active: bool,
	pub(in crate::overlay) interaction: FrozenSelectionInteractionKind,
	pub(in crate::overlay) anchor_rect: RectPoints,
	pub(in crate::overlay) pointer_offset_x: u32,
	pub(in crate::overlay) pointer_offset_y: u32,
	pub(in crate::overlay) press_cursor_x: u32,
	pub(in crate::overlay) press_cursor_y: u32,
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
pub(in crate::overlay) struct FrozenMosaicDragState {
	pub(in crate::overlay) active: bool,
	pub(in crate::overlay) anchor_x: u32,
	pub(in crate::overlay) anchor_y: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::overlay) struct FrozenArrowDragState {
	pub(in crate::overlay) active: bool,
	pub(in crate::overlay) anchor_x: u32,
	pub(in crate::overlay) anchor_y: u32,
	pub(in crate::overlay) current_x: u32,
	pub(in crate::overlay) current_y: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::overlay) struct FrozenSpotlightDragState {
	pub(in crate::overlay) active: bool,
	pub(in crate::overlay) anchor_x: u32,
	pub(in crate::overlay) anchor_y: u32,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::overlay) struct FrozenToolbarPointerState {
	pub(in crate::overlay) cursor_local: Pos2,
	#[cfg(not(target_os = "macos"))]
	pub(in crate::overlay) left_button_down: bool,
	pub(in crate::overlay) left_button_went_down: bool,
	pub(in crate::overlay) left_button_went_up: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::overlay) enum FrozenAnnotationColor {
	White,
	Yellow,
	Green,
	Blue,
	Red,
	Black,
}
impl FrozenAnnotationColor {
	pub(in crate::overlay) const ALL: [Self; 6] =
		[Self::White, Self::Yellow, Self::Green, Self::Blue, Self::Red, Self::Black];

	pub(in crate::overlay) const fn swatch_fill(self) -> Color32 {
		match self {
			Self::White => Color32::from_rgb(255, 255, 255),
			Self::Yellow => Color32::from_rgb(255, 219, 77),
			Self::Green => Color32::from_rgb(92, 214, 149),
			Self::Blue => Color32::from_rgb(102, 178, 255),
			Self::Red => Color32::from_rgb(255, 107, 107),
			Self::Black => Color32::from_rgb(24, 24, 24),
		}
	}

	pub(in crate::overlay) const fn export_rgba(self) -> [u8; 4] {
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
