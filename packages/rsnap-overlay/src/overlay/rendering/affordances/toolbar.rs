use egui::{Context, Response};
use egui_phosphor::regular::{MINUS, PLUS};

use crate::overlay::rendering::{FrozenToolbarButtonStyle, WindowRenderer};
use crate::overlay::session_state::FrozenAnnotationStyleCapsulePlacement;
use crate::overlay::{
	Align, Align2, Area, Color32, CornerRadius, FROZEN_TOOLBAR_BUTTON_SIZE_POINTS,
	FROZEN_TOOLBAR_ITEM_SPACING_POINTS, FontFamily, FontId, FrozenAnnotationColor,
	FrozenToolbarPointerState, FrozenToolbarState, FrozenToolbarTool,
	HUD_PILL_INNER_MARGIN_X_POINTS, HUD_PILL_STROKE_WIDTH_POINTS, HudPillGeometry, HudTheme, Id,
	Layout, MonitorRect, Order, OverlayMode, OverlayState, Pos2, Rect, Sense, Stroke, StrokeKind,
	TOOLBAR_CAPTURE_GAP_PX, TOOLBAR_EXPANDED_HEIGHT_PX, TOOLBAR_PILL_INNER_MARGIN_Y_POINTS,
	TOOLBAR_SCREEN_MARGIN_PX, ToolbarPlacement, Ui, UiBuilder, Vec2, toolbar_layout_model,
};

const FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS: f32 = 4.0;
const FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS: f32 = 24.0;
const FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS: f32 = 16.0;
const FROZEN_ANNOTATION_TOOLBAR_SWATCH_GAP_POINTS: f32 = 6.0;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_BUTTON_WIDTH_POINTS: f32 = 20.0;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_DISPLAY_WIDTH_POINTS: f32 = 58.0;
const FROZEN_ANNOTATION_TOOLBAR_PEN_SIZE_DISPLAY_WIDTH_POINTS: f32 = 84.0;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_CAPSULE_CORNER_RADIUS_POINTS: u8 = 8;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_PREVIEW_GAP_POINTS: f32 = 8.0;
const FROZEN_ANNOTATION_TOOLBAR_PEN_PREVIEW_LENGTH_POINTS: f32 = 18.0;
#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_SCROLL_MODE: [FrozenToolbarTool; 3] =
	[FrozenToolbarTool::Ocr, FrozenToolbarTool::Copy, FrozenToolbarTool::Save];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_SCROLL_MODE: [FrozenToolbarTool; 2] =
	[FrozenToolbarTool::Copy, FrozenToolbarTool::Save];
#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_WITH_SCROLL_AND_AUTO_CENTER: [FrozenToolbarTool; 13] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::AutoCenter,
	FrozenToolbarTool::Scroll,
	FrozenToolbarTool::Ocr,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_WITH_SCROLL_AND_AUTO_CENTER: [FrozenToolbarTool; 12] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::AutoCenter,
	FrozenToolbarTool::Scroll,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_WITH_AUTO_CENTER: [FrozenToolbarTool; 12] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::AutoCenter,
	FrozenToolbarTool::Ocr,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_WITH_AUTO_CENTER: [FrozenToolbarTool; 11] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::AutoCenter,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_WITH_SCROLL: [FrozenToolbarTool; 12] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::Scroll,
	FrozenToolbarTool::Ocr,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_WITH_SCROLL: [FrozenToolbarTool; 11] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::Scroll,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_WITHOUT_SCROLL: [FrozenToolbarTool; 11] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::Ocr,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_WITHOUT_SCROLL: [FrozenToolbarTool; 10] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum FrozenAnnotationStyleToolbarKind {
	Pen,
	Text,
}
impl FrozenAnnotationStyleToolbarKind {
	fn from_toolbar_state(toolbar_state: &FrozenToolbarState) -> Option<Self> {
		match toolbar_state.selected_tool {
			FrozenToolbarTool::Pen | FrozenToolbarTool::Arrow => Some(Self::Pen),
			FrozenToolbarTool::Text => Some(Self::Text),
			_ => None,
		}
	}

	const fn size_display_width(self) -> f32 {
		match self {
			Self::Pen => FROZEN_ANNOTATION_TOOLBAR_PEN_SIZE_DISPLAY_WIDTH_POINTS,
			Self::Text => FROZEN_ANNOTATION_TOOLBAR_SIZE_DISPLAY_WIDTH_POINTS,
		}
	}

	const fn size_control_width(self) -> f32 {
		self.size_display_width() + FROZEN_ANNOTATION_TOOLBAR_SIZE_BUTTON_WIDTH_POINTS * 2.0
	}

	fn size_value(self, toolbar_state: &FrozenToolbarState) -> f64 {
		match self {
			Self::Pen => toolbar_state.brush_style.stroke_width_points,
			Self::Text => toolbar_state.text_style.font_size_points,
		}
		.into()
	}

	fn formatted_size_text(self, toolbar_state: &FrozenToolbarState) -> String {
		match self {
			Self::Pen => {
				let size_points = self.size_value(toolbar_state);
				let mut text = format!("{size_points:.2}");

				while text.contains('.') && text.ends_with('0') {
					let _ = text.pop();
				}

				if text.ends_with('.') {
					let _ = text.pop();
				}

				text
			},
			Self::Text => {
				let font_size = toolbar_state.text_style.font_size_points;

				if (font_size - font_size.round()).abs() <= f32::EPSILON {
					format!("{}", font_size.round() as i32)
				} else {
					format!("{font_size:.1}")
				}
			},
		}
	}

	fn selected_color(self, toolbar_state: &FrozenToolbarState) -> FrozenAnnotationColor {
		match self {
			Self::Pen => toolbar_state.brush_style.color,
			Self::Text => toolbar_state.text_style.color,
		}
	}

	fn set_color(
		self,
		toolbar_state: &mut FrozenToolbarState,
		color: FrozenAnnotationColor,
	) -> bool {
		let selected_color = match self {
			Self::Pen => &mut toolbar_state.brush_style.color,
			Self::Text => &mut toolbar_state.text_style.color,
		};

		if *selected_color == color {
			return false;
		}

		*selected_color = color;

		true
	}

	fn apply_size_steps(self, toolbar_state: &mut FrozenToolbarState, steps: i32) -> bool {
		toolbar_state.apply_annotation_size_steps(steps)
	}
}

#[derive(Clone, Copy)]
struct FrozenAnnotationSizeControlAppearance {
	capsule_fill: Color32,
	capsule_stroke: Color32,
	divider_color: Color32,
	button_hover_fill: Color32,
	text_color: Color32,
}
impl WindowRenderer {
	pub(in crate::overlay) fn frozen_toolbar_reserved_rect(
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		toolbar_placement: ToolbarPlacement,
		toolbar_state: &FrozenToolbarState,
	) -> Option<Rect> {
		if !toolbar_state.visible
			|| !matches!(state.mode, OverlayMode::Frozen)
			|| state.monitor != Some(monitor)
		{
			return None;
		}

		let capture_rect = Self::frozen_toolbar_capture_rect(state, monitor, screen_rect);
		let toolbar_primary_size = Self::frozen_toolbar_primary_size(toolbar_state);
		let toolbar_positioning_size = Self::frozen_toolbar_positioning_size(toolbar_state);
		let default_pos = Self::frozen_toolbar_default_window_pos(
			screen_rect,
			capture_rect,
			toolbar_primary_size,
			toolbar_positioning_size,
			toolbar_placement,
		);
		let toolbar_pos = toolbar_state.floating_position.unwrap_or(default_pos);

		if !toolbar_layout_model::frozen_toolbar_matches_default_slot(toolbar_pos, default_pos) {
			return None;
		}

		let mut reserved_toolbar_state = toolbar_state.clone();

		Self::sync_frozen_annotation_style_capsule_placement(
			&mut reserved_toolbar_state,
			screen_rect,
			toolbar_pos,
		);

		Some(Self::frozen_toolbar_window_rect(&reserved_toolbar_state, toolbar_pos))
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_frozen_toolbar_ui(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		theme: HudTheme,
		toolbar_placement: ToolbarPlacement,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		toolbar_state: Option<&mut FrozenToolbarState>,
		pointer_state: Option<FrozenToolbarPointerState>,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let Some(toolbar_state) = toolbar_state else {
			return;
		};
		#[cfg(target_os = "macos")]
		let _ = pointer_state;

		if !matches!(state.mode, OverlayMode::Frozen) || !toolbar_state.visible {
			return;
		}
		if state.monitor != Some(monitor) {
			return;
		}

		#[cfg(not(target_os = "macos"))]
		let (cursor, left_button_down) = if let Some(pointer_state) = pointer_state {
			(pointer_state.cursor_local, pointer_state.left_button_down)
		} else {
			(Pos2::new(-1.0, -1.0), false)
		};
		let toolbar_primary_size = Self::frozen_toolbar_primary_size(toolbar_state);
		let toolbar_positioning_size = Self::frozen_toolbar_positioning_size(toolbar_state);
		let toolbar_size = Self::frozen_toolbar_size(toolbar_state);
		let screen_rect = ctx.input(|i| i.viewport_rect());
		let capture_rect = Self::frozen_toolbar_capture_rect(state, monitor, screen_rect);
		let Some(toolbar_pos) = Self::resolve_frozen_toolbar_birth(
			ctx,
			state,
			monitor,
			toolbar_state,
			screen_rect,
			capture_rect,
			toolbar_primary_size,
			toolbar_positioning_size,
			toolbar_placement,
		) else {
			return;
		};

		#[cfg(any(not(target_os = "macos"), test))]
		{
			if !toolbar_layout_model::advance_frozen_toolbar_readiness_sample_state(
				toolbar_state,
				screen_rect,
			) {
				ctx.request_repaint();

				return;
			}
		}

		Self::draw_frozen_toolbar(
			ctx,
			toolbar_state,
			monitor,
			screen_rect,
			toolbar_pos,
			toolbar_size,
			theme,
			hud_blur_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
			#[cfg(not(target_os = "macos"))]
			cursor,
			#[cfg(not(target_os = "macos"))]
			left_button_down,
			hud_pill_out,
		);
	}

	pub(in crate::overlay) fn frozen_toolbar_tools(
		toolbar_state: &FrozenToolbarState,
	) -> &'static [FrozenToolbarTool] {
		if toolbar_state.scroll_capture_active {
			&FROZEN_TOOLBAR_TOOLS_SCROLL_MODE
		} else if toolbar_state.auto_center_available && toolbar_state.scroll_capture_available {
			&FROZEN_TOOLBAR_TOOLS_WITH_SCROLL_AND_AUTO_CENTER
		} else if toolbar_state.auto_center_available {
			&FROZEN_TOOLBAR_TOOLS_WITH_AUTO_CENTER
		} else if toolbar_state.scroll_capture_available {
			&FROZEN_TOOLBAR_TOOLS_WITH_SCROLL
		} else {
			&FROZEN_TOOLBAR_TOOLS_WITHOUT_SCROLL
		}
	}

	pub(in crate::overlay) fn frozen_toolbar_primary_size(
		toolbar_state: &FrozenToolbarState,
	) -> Vec2 {
		let tool_count = Self::frozen_toolbar_tools(toolbar_state).len() as f32;
		let spacing_count = (tool_count - 1.0).max(0.0);
		let width = tool_count * FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
			+ spacing_count * FROZEN_TOOLBAR_ITEM_SPACING_POINTS
			+ 2.0 * HUD_PILL_INNER_MARGIN_X_POINTS
			+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
		let height = toolbar_state.pill_height_points.unwrap_or(TOOLBAR_EXPANDED_HEIGHT_PX);

		Vec2::new(width, height)
	}

	pub(in crate::overlay) fn frozen_toolbar_primary_rect(
		toolbar_state: &FrozenToolbarState,
		toolbar_pos: Pos2,
	) -> Rect {
		Rect::from_min_size(toolbar_pos, Self::frozen_toolbar_primary_size(toolbar_state))
	}

	pub(in crate::overlay) fn frozen_annotation_style_capsule_size(
		toolbar_state: &FrozenToolbarState,
	) -> Option<Vec2> {
		let style_kind = FrozenAnnotationStyleToolbarKind::from_toolbar_state(toolbar_state)?;
		let swatch_count = FrozenAnnotationColor::ALL.len() as f32;
		let swatches_width = swatch_count * FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS
			+ (swatch_count - 1.0).max(0.0) * FROZEN_ANNOTATION_TOOLBAR_SWATCH_GAP_POINTS;
		let content_width = style_kind.size_control_width() + 4.0 + swatches_width;
		let width = content_width
			+ 2.0 * HUD_PILL_INNER_MARGIN_X_POINTS
			+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
		let height = toolbar_state.pill_height_points.unwrap_or(TOOLBAR_EXPANDED_HEIGHT_PX);

		Some(Vec2::new(width, height))
	}

	pub(in crate::overlay) fn frozen_toolbar_positioning_size(
		toolbar_state: &FrozenToolbarState,
	) -> Vec2 {
		Self::frozen_toolbar_primary_size(toolbar_state)
	}

	pub(in crate::overlay) fn frozen_toolbar_window_top_padding_points() -> f32 {
		[
			FrozenToolbarState {
				selected_tool: FrozenToolbarTool::Pen,
				..FrozenToolbarState::default()
			},
			FrozenToolbarState {
				selected_tool: FrozenToolbarTool::Text,
				..FrozenToolbarState::default()
			},
		]
		.into_iter()
		.map(|toolbar_state| {
			Self::frozen_annotation_style_capsule_size(&toolbar_state).map_or(0.0, |style_size| {
				style_size.y + FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS
			})
		})
		.fold(0.0, f32::max)
	}

	fn frozen_annotation_style_capsule_placement_for_toolbar_pos(
		toolbar_state: &FrozenToolbarState,
		screen_rect: Rect,
		toolbar_pos: Pos2,
	) -> FrozenAnnotationStyleCapsulePlacement {
		let Some(style_size) = Self::frozen_annotation_style_capsule_size(toolbar_state) else {
			return FrozenAnnotationStyleCapsulePlacement::Below;
		};
		let toolbar_rect = Self::frozen_toolbar_primary_rect(toolbar_state, toolbar_pos);
		let below_y = toolbar_rect.max.y + FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS;
		let above_y =
			toolbar_rect.min.y - FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS - style_size.y;
		let fits_below = below_y + style_size.y + TOOLBAR_SCREEN_MARGIN_PX <= screen_rect.max.y;
		let fits_above = above_y >= screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;

		if fits_below {
			FrozenAnnotationStyleCapsulePlacement::Below
		} else if fits_above {
			FrozenAnnotationStyleCapsulePlacement::Above
		} else {
			let below_space = screen_rect.max.y - below_y;
			let above_space = toolbar_rect.min.y
				- FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS
				- screen_rect.min.y;

			if above_space > below_space {
				FrozenAnnotationStyleCapsulePlacement::Above
			} else {
				FrozenAnnotationStyleCapsulePlacement::Below
			}
		}
	}

	pub(in crate::overlay) fn sync_frozen_annotation_style_capsule_placement(
		toolbar_state: &mut FrozenToolbarState,
		screen_rect: Rect,
		toolbar_pos: Pos2,
	) {
		toolbar_state.annotation_style_capsule_placement =
			Self::frozen_annotation_style_capsule_placement_for_toolbar_pos(
				toolbar_state,
				screen_rect,
				toolbar_pos,
			);
	}

	fn frozen_annotation_style_capsule_rect(
		toolbar_state: &FrozenToolbarState,
		toolbar_rect: Rect,
	) -> Option<Rect> {
		let style_size = Self::frozen_annotation_style_capsule_size(toolbar_state)?;
		let min_x = toolbar_rect.left();
		let max_x = (toolbar_rect.right() - style_size.x).max(min_x);
		let x = (toolbar_rect.center().x - style_size.x * 0.5).clamp(min_x, max_x);
		let y = match toolbar_state.annotation_style_capsule_placement {
			FrozenAnnotationStyleCapsulePlacement::Above => {
				toolbar_rect.min.y - FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS - style_size.y
			},
			FrozenAnnotationStyleCapsulePlacement::Below => {
				toolbar_rect.max.y + FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS
			},
		};

		Some(Rect::from_min_size(Pos2::new(x, y), style_size))
	}

	pub(in crate::overlay) fn frozen_toolbar_window_rect(
		toolbar_state: &FrozenToolbarState,
		toolbar_pos: Pos2,
	) -> Rect {
		let toolbar_rect = Self::frozen_toolbar_primary_rect(toolbar_state, toolbar_pos);

		Self::frozen_annotation_style_capsule_rect(toolbar_state, toolbar_rect)
			.map_or(toolbar_rect, |style_rect| toolbar_rect.union(style_rect))
	}

	#[cfg(any(target_os = "macos", test))]
	pub(in crate::overlay) fn frozen_toolbar_visible_capsules_contain(
		toolbar_state: &FrozenToolbarState,
		toolbar_pos: Pos2,
		cursor_local: Pos2,
	) -> bool {
		let toolbar_rect = Self::frozen_toolbar_primary_rect(toolbar_state, toolbar_pos);

		if toolbar_rect.contains(cursor_local) {
			return true;
		}

		Self::frozen_annotation_style_capsule_rect(toolbar_state, toolbar_rect)
			.is_some_and(|style_rect| style_rect.contains(cursor_local))
	}

	pub(in crate::overlay) fn frozen_toolbar_size(toolbar_state: &FrozenToolbarState) -> Vec2 {
		Self::frozen_toolbar_window_rect(toolbar_state, Pos2::ZERO).size()
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn resolve_frozen_toolbar_birth(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		toolbar_state: &mut FrozenToolbarState,
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_primary_size: Vec2,
		toolbar_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Option<Pos2> {
		if let Some(pos) = toolbar_state.floating_position {
			#[cfg(any(not(target_os = "macos"), test))]
			Self::sync_frozen_annotation_style_capsule_placement(toolbar_state, screen_rect, pos);

			return Some(pos);
		}

		let screen_size_points = screen_rect.size();

		tracing::trace!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			screen_rect = ?screen_rect,
			screen_size_points = ?screen_size_points,
			pixels_per_point = ctx.pixels_per_point(),
			last_screen_size_points = ?toolbar_state.layout_last_screen_size_points,
			stable_frames = toolbar_state.layout_stable_frames,
			"Frozen toolbar birth attempt."
		);

		let needs_new_sample = toolbar_layout_model::frozen_toolbar_needs_new_sample(
			toolbar_state.layout_last_screen_size_points,
			screen_size_points,
		);

		if needs_new_sample {
			toolbar_state.layout_last_screen_size_points = Some(screen_size_points);
			toolbar_state.layout_stable_frames = 0;
			toolbar_state.needs_redraw = true;

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = state.frozen_generation,
				new_screen_size_points = ?screen_size_points,
				"Frozen toolbar waiting for stable screen rect (new sample)."
			);

			ctx.request_repaint();

			return None;
		}
		if toolbar_state.layout_stable_frames < 1 {
			toolbar_state.layout_stable_frames =
				toolbar_state.layout_stable_frames.saturating_add(1);
			toolbar_state.needs_redraw = true;

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = state.frozen_generation,
				screen_size_points = ?screen_size_points,
				stable_frames = toolbar_state.layout_stable_frames,
				"Frozen toolbar waiting for stable screen rect (confirm)."
			);

			ctx.request_repaint();

			return None;
		}

		let default_pos = Self::frozen_toolbar_default_window_pos(
			screen_rect,
			capture_rect,
			toolbar_primary_size,
			toolbar_size,
			toolbar_placement,
		);

		tracing::debug!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			toolbar_primary_size_points = ?toolbar_primary_size,
			toolbar_size_points = ?toolbar_size,
			default_pos = ?default_pos,
			"Frozen toolbar birth resolved."
		);

		toolbar_state.default_slot_position = Some(default_pos);
		toolbar_state.floating_position = Some(default_pos);

		#[cfg(any(not(target_os = "macos"), test))]
		{
			Self::sync_frozen_annotation_style_capsule_placement(
				toolbar_state,
				screen_rect,
				default_pos,
			);
		}

		Some(default_pos)
	}

	pub(in crate::overlay) fn frozen_toolbar_capture_rect(
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
	) -> Rect {
		let Some(capture_rect) = state.frozen_capture_rect else {
			return screen_rect;
		};
		let Some(frozen_monitor) = state.monitor else {
			return screen_rect;
		};

		if frozen_monitor != monitor {
			return screen_rect;
		}

		let capture_rect = Rect::from_min_size(
			Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
			Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
		);

		capture_rect.intersect(screen_rect)
	}

	pub(in crate::overlay) fn frozen_toolbar_default_window_pos(
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_primary_size: Vec2,
		toolbar_positioning_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Pos2 {
		let y = match toolbar_placement {
			ToolbarPlacement::Bottom => {
				let below_y = capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX;
				let within_screen = below_y + toolbar_primary_size.y + TOOLBAR_SCREEN_MARGIN_PX
					<= screen_rect.max.y;

				if within_screen {
					below_y
				} else {
					capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_primary_size.y
				}
			},
			ToolbarPlacement::Top => {
				let above_y = capture_rect.min.y - TOOLBAR_CAPTURE_GAP_PX - toolbar_primary_size.y;
				let within_screen = above_y >= screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;

				if within_screen { above_y } else { capture_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX }
			},
		};
		let min_y = screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;
		let max_y =
			(screen_rect.max.y - toolbar_positioning_size.y - TOOLBAR_SCREEN_MARGIN_PX).max(min_y);
		let ideal_x = capture_rect.center().x - toolbar_primary_size.x / 2.0;
		let min_x = screen_rect.min.x + TOOLBAR_SCREEN_MARGIN_PX;
		let max_x =
			(screen_rect.max.x - toolbar_positioning_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(min_x);
		let x = ideal_x.clamp(min_x, max_x);
		let y = y.max(min_y).min(max_y);

		Pos2::new(x, y)
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn draw_frozen_toolbar(
		ctx: &Context,
		toolbar_state: &mut FrozenToolbarState,
		monitor: MonitorRect,
		screen_rect: Rect,
		toolbar_pos: Pos2,
		toolbar_size: Vec2,
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		#[cfg(not(target_os = "macos"))] cursor: Pos2,
		#[cfg(not(target_os = "macos"))] left_button_down: bool,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		#[cfg(target_os = "macos")]
		let _ = screen_rect;
		let area_id = Id::new(format!("frozen-toolbar-{}", monitor.id));
		let window_rect = Self::frozen_toolbar_window_rect(toolbar_state, toolbar_pos);

		Area::new(area_id).order(Order::Foreground).fixed_pos(window_rect.min).show(ctx, |ui| {
			let (_area_rect, _window_response) =
				ui.allocate_exact_size(toolbar_size, Sense::hover());
			let toolbar_rect = Self::frozen_toolbar_primary_rect(toolbar_state, toolbar_pos);
			let style_rect =
				Self::frozen_annotation_style_capsule_rect(toolbar_state, toolbar_rect);
			let toolbar_response = ui.interact(
				toolbar_rect,
				area_id.with("primary-capsule"),
				if cfg!(target_os = "macos") { Sense::hover() } else { Sense::click_and_drag() },
			);
			#[cfg(target_os = "macos")]
			let _ = &toolbar_response;

			toolbar_state.annotation_size_control_hovered = false;

			#[cfg(not(target_os = "macos"))]
			Self::update_frozen_toolbar_drag_state(
				toolbar_state,
				toolbar_response.drag_started(),
				toolbar_pos,
				screen_rect,
				Self::frozen_toolbar_positioning_size(toolbar_state),
				cursor,
				left_button_down,
			);
			Self::paint_frozen_toolbar_capsule(
				ui,
				toolbar_rect,
				theme,
				hud_blur_active,
				hud_opaque,
				hud_opacity,
				hud_milk_amount,
				hud_tint_hue,
			);

			let toolbar_inner_rect = toolbar_rect.shrink2(egui::vec2(
				HUD_PILL_INNER_MARGIN_X_POINTS,
				TOOLBAR_PILL_INNER_MARGIN_Y_POINTS,
			));
			let _ = ui.scope_builder(UiBuilder::new().max_rect(toolbar_inner_rect), |ui| {
				Self::render_frozen_toolbar_primary_row(
					ui,
					toolbar_inner_rect.width(),
					toolbar_state,
					theme,
				);
			});

			if let Some(style_rect) = style_rect {
				Self::paint_frozen_toolbar_capsule(
					ui,
					style_rect,
					theme,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					hud_tint_hue,
				);

				let style_inner_rect = style_rect.shrink2(egui::vec2(
					HUD_PILL_INNER_MARGIN_X_POINTS,
					TOOLBAR_PILL_INNER_MARGIN_Y_POINTS,
				));
				let _ = ui.scope_builder(UiBuilder::new().max_rect(style_inner_rect), |ui| {
					let _ = ui.allocate_ui_with_layout(
						Vec2::new(
							style_inner_rect.width(),
							FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS,
						),
						Layout::left_to_right(Align::Center),
						|ui| {
							Self::render_frozen_annotation_toolbar_controls(
								ui,
								toolbar_state,
								theme,
							)
						},
					);
				});
			}

			*hud_pill_out = Some(HudPillGeometry {
				rect: window_rect,
				radius_points: f32::from(toolbar_layout_model::frozen_toolbar_corner_radius_u8(
					window_rect.height(),
				)),
			});
		});
	}

	#[allow(clippy::too_many_arguments)]
	fn paint_frozen_toolbar_capsule(
		ui: &Ui,
		rect: Rect,
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
	) {
		let corner_radius = toolbar_layout_model::frozen_toolbar_corner_radius_u8(rect.height());
		let body_fill = Self::tinted_hud_body_fill(
			theme,
			hud_blur_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
		);
		let toolbar_frame = Self::hud_pill_frame(theme, hud_opaque, hud_opacity, body_fill, false);

		ui.painter().rect_filled(rect, f32::from(corner_radius), toolbar_frame.fill);
		ui.painter().rect_stroke(
			rect.shrink(0.5),
			CornerRadius::same(corner_radius),
			toolbar_frame.stroke,
			StrokeKind::Inside,
		);

		let inner_stroke_color = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
			HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
		};
		let inner_rect = rect.shrink(1.0);

		ui.painter().rect_stroke(
			inner_rect,
			CornerRadius::same(corner_radius.saturating_sub(1)),
			Stroke::new(1.0, inner_stroke_color),
			StrokeKind::Inside,
		);
	}

	#[cfg(not(target_os = "macos"))]
	fn update_frozen_toolbar_drag_state(
		toolbar_state: &mut FrozenToolbarState,
		drag_started: bool,
		toolbar_pos: Pos2,
		screen_rect: Rect,
		toolbar_size: Vec2,
		cursor: Pos2,
		left_button_down: bool,
	) {
		if drag_started {
			toolbar_state.dragging = true;
			toolbar_state.floating_position = Some(toolbar_pos);
			toolbar_state.drag_offset = cursor - toolbar_pos;
		}
		if toolbar_state.dragging && left_button_down {
			let desired_pos = cursor - toolbar_state.drag_offset;

			toolbar_state.floating_position = Some(Self::clamp_toolbar_position(
				screen_rect,
				toolbar_size,
				desired_pos,
				TOOLBAR_SCREEN_MARGIN_PX,
				TOOLBAR_SCREEN_MARGIN_PX,
			));
		}
	}

	fn render_frozen_toolbar_primary_row(
		ui: &mut Ui,
		width: f32,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		let _ = ui.allocate_ui_with_layout(
			Vec2::new(width, FROZEN_TOOLBAR_BUTTON_SIZE_POINTS),
			Layout::left_to_right(Align::Center),
			|ui| {
				ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);

				Self::render_frozen_toolbar_controls(ui, toolbar_state, theme);
			},
		);
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frozen_toolbar_controls(
		ui: &mut Ui,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		if toolbar_state.selected_tool == FrozenToolbarTool::Scroll {
			toolbar_state.selected_tool = FrozenToolbarTool::Pointer;
		}

		let tools = Self::frozen_toolbar_tools(toolbar_state);
		let button_size = FROZEN_TOOLBAR_BUTTON_SIZE_POINTS;
		let button_font_size = 18.0;
		let item_spacing = FROZEN_TOOLBAR_ITEM_SPACING_POINTS;
		let hit_area_inset = 5.0;

		ui.horizontal_centered(|ui| {
			ui.spacing_mut().item_spacing.x = item_spacing;

			for tool in tools {
				let is_mode_tool = tool.is_mode_tool();
				let action_ready = tool.is_available(toolbar_state)
					&& (!tool.requires_final_capture() || toolbar_state.final_capture_ready);
				let response =
					ui.allocate_response(Vec2::new(button_size, button_size), Sense::click());
				let hovered = action_ready && response.hovered();
				let response = if action_ready {
					response.on_hover_text(tool.label())
				} else {
					response.on_hover_text(tool.unavailable_label(toolbar_state))
				};
				let hover_anim: f32 = if hovered { 1.0 } else { 0.0 };

				if action_ready && response.clicked() {
					let tool = *tool;

					if is_mode_tool {
						toolbar_state.selected_tool = tool;
					} else {
						toolbar_state.pending_action = Some(tool);
					}

					toolbar_state.needs_redraw = true;
				}

				let selected = is_mode_tool && *tool == toolbar_state.selected_tool;
				let selected_anim: f32 = if selected { 1.0 } else { 0.0 };
				let glow = hover_anim.max(selected_anim);
				let icon_font = if selected {
					FontFamily::Name("phosphor-fill".into())
				} else {
					FontFamily::Proportional
				};
				let style =
					Self::frozen_toolbar_button_style(theme, action_ready, hovered, selected);

				if glow > 0.0 {
					let bg_rect = response.rect.shrink(hit_area_inset);

					ui.painter().rect_filled(bg_rect, 8.0, style.bg_color);
				}

				if let Some(border_color) = style.border_color {
					ui.painter().rect_stroke(
						response.rect.shrink(hit_area_inset),
						8.0,
						Stroke::new(1.0, border_color),
						StrokeKind::Inside,
					);
				}

				ui.painter().text(
					response.rect.center(),
					Align2::CENTER_CENTER,
					tool.icon(),
					FontId::new(button_font_size, icon_font),
					style.icon_color,
				);
			}
		});
	}

	fn render_frozen_annotation_toolbar_controls(
		ui: &mut Ui,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		let Some(style_kind) = FrozenAnnotationStyleToolbarKind::from_toolbar_state(toolbar_state)
		else {
			toolbar_state.annotation_size_control_hovered = false;

			return;
		};
		let size_label = match style_kind {
			FrozenAnnotationStyleToolbarKind::Text => {
				format!("{} pt", style_kind.formatted_size_text(toolbar_state))
			},
			FrozenAnnotationStyleToolbarKind::Pen => style_kind.formatted_size_text(toolbar_state),
		};

		ui.horizontal_centered(|ui| {
			ui.spacing_mut().item_spacing.x = FROZEN_ANNOTATION_TOOLBAR_SWATCH_GAP_POINTS;

			Self::render_frozen_annotation_size_control(
				ui,
				toolbar_state,
				theme,
				style_kind,
				&size_label,
			);

			ui.add_space(4.0);

			for color in FrozenAnnotationColor::ALL {
				if Self::render_frozen_annotation_color_swatch(
					ui,
					color,
					style_kind.selected_color(toolbar_state) == color,
					theme,
				) && style_kind.set_color(toolbar_state, color)
				{
					toolbar_state.needs_redraw = true;
				}
			}
		});

		if !toolbar_state.annotation_size_control_hovered {
			toolbar_state.annotation_size_wheel_accumulator = 0.0;
		}
	}

	fn render_frozen_annotation_size_control(
		ui: &mut Ui,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
		style_kind: FrozenAnnotationStyleToolbarKind,
		size_label: &str,
	) {
		let (size_rect, size_response) = ui.allocate_exact_size(
			Vec2::new(
				style_kind.size_control_width(),
				FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS,
			),
			Sense::hover(),
		);
		let minus_rect = Rect::from_min_max(
			size_rect.min,
			Pos2::new(
				size_rect.min.x + FROZEN_ANNOTATION_TOOLBAR_SIZE_BUTTON_WIDTH_POINTS,
				size_rect.max.y,
			),
		);
		let plus_rect = Rect::from_min_max(
			Pos2::new(
				size_rect.max.x - FROZEN_ANNOTATION_TOOLBAR_SIZE_BUTTON_WIDTH_POINTS,
				size_rect.min.y,
			),
			size_rect.max,
		);
		let display_rect = Rect::from_min_max(
			Pos2::new(minus_rect.max.x, size_rect.min.y),
			Pos2::new(plus_rect.min.x, size_rect.max.y),
		);
		let minus_response = ui.interact(
			minus_rect,
			ui.id().with(("annotation-size-decrease", style_kind)),
			Sense::click(),
		);
		let plus_response = ui.interact(
			plus_rect,
			ui.id().with(("annotation-size-increase", style_kind)),
			Sense::click(),
		);
		let hovered =
			size_response.hovered() || minus_response.hovered() || plus_response.hovered();
		let capsule_rect = size_rect.shrink2(egui::vec2(1.0, 3.0));
		let appearance = Self::frozen_annotation_size_control_appearance(theme, hovered);

		toolbar_state.annotation_size_control_hovered = hovered;

		Self::paint_frozen_annotation_size_control_frame(
			ui,
			capsule_rect,
			display_rect,
			&minus_response,
			&plus_response,
			appearance,
		);
		Self::paint_frozen_annotation_size_step_button(ui, theme, &minus_response, MINUS);
		Self::paint_frozen_annotation_size_step_button(ui, theme, &plus_response, PLUS);
		Self::apply_frozen_annotation_size_control_clicks(
			toolbar_state,
			style_kind,
			&minus_response,
			&plus_response,
		);
		Self::paint_frozen_annotation_size_display(
			ui,
			toolbar_state,
			style_kind,
			display_rect,
			size_label,
			appearance.text_color,
		);
	}

	fn frozen_annotation_size_control_appearance(
		theme: HudTheme,
		hovered: bool,
	) -> FrozenAnnotationSizeControlAppearance {
		match theme {
			HudTheme::Dark => FrozenAnnotationSizeControlAppearance {
				capsule_fill: Color32::from_rgba_unmultiplied(
					255,
					255,
					255,
					if hovered { 22 } else { 12 },
				),
				capsule_stroke: Color32::from_rgba_unmultiplied(
					255,
					255,
					255,
					if hovered { 34 } else { 22 },
				),
				divider_color: Color32::from_white_alpha(if hovered { 34 } else { 22 }),
				button_hover_fill: Color32::from_rgba_unmultiplied(255, 255, 255, 16),
				text_color: Self::frozen_toolbar_button_style(theme, true, hovered, false)
					.icon_color,
			},
			HudTheme::Light => FrozenAnnotationSizeControlAppearance {
				capsule_fill: Color32::from_rgba_unmultiplied(
					0,
					0,
					0,
					if hovered { 18 } else { 10 },
				),
				capsule_stroke: Color32::from_rgba_unmultiplied(
					0,
					0,
					0,
					if hovered { 28 } else { 18 },
				),
				divider_color: Color32::from_black_alpha(if hovered { 30 } else { 18 }),
				button_hover_fill: Color32::from_rgba_unmultiplied(0, 0, 0, 14),
				text_color: Self::frozen_toolbar_button_style(theme, true, hovered, false)
					.icon_color,
			},
		}
	}

	fn paint_frozen_annotation_size_control_frame(
		ui: &Ui,
		capsule_rect: Rect,
		display_rect: Rect,
		minus_response: &Response,
		plus_response: &Response,
		appearance: FrozenAnnotationSizeControlAppearance,
	) {
		ui.painter().rect_filled(
			capsule_rect,
			CornerRadius::same(FROZEN_ANNOTATION_TOOLBAR_SIZE_CAPSULE_CORNER_RADIUS_POINTS),
			appearance.capsule_fill,
		);
		ui.painter().rect_stroke(
			capsule_rect,
			CornerRadius::same(FROZEN_ANNOTATION_TOOLBAR_SIZE_CAPSULE_CORNER_RADIUS_POINTS),
			Stroke::new(1.0, appearance.capsule_stroke),
			StrokeKind::Inside,
		);

		for response in [minus_response, plus_response] {
			if response.hovered() {
				ui.painter().rect_filled(
					response.rect.shrink2(egui::vec2(2.0, 4.0)),
					CornerRadius::same(6),
					appearance.button_hover_fill,
				);
			}
		}
		for divider_x in [display_rect.left(), display_rect.right()] {
			ui.painter().line_segment(
				[
					Pos2::new(divider_x, capsule_rect.top() + 5.0),
					Pos2::new(divider_x, capsule_rect.bottom() - 5.0),
				],
				Stroke::new(1.0, appearance.divider_color),
			);
		}
	}

	fn paint_frozen_annotation_size_step_button(
		ui: &Ui,
		theme: HudTheme,
		response: &Response,
		icon: &str,
	) {
		let button_style =
			Self::frozen_toolbar_button_style(theme, true, response.hovered(), false);

		ui.painter().text(
			response.rect.center(),
			Align2::CENTER_CENTER,
			icon,
			FontId::new(13.0, FontFamily::Proportional),
			button_style.icon_color,
		);
	}

	fn apply_frozen_annotation_size_control_clicks(
		toolbar_state: &mut FrozenToolbarState,
		style_kind: FrozenAnnotationStyleToolbarKind,
		minus_response: &Response,
		plus_response: &Response,
	) {
		let mut size_changed = false;

		if minus_response.clicked() {
			toolbar_state.annotation_size_wheel_accumulator = 0.0;
			size_changed |= style_kind.apply_size_steps(toolbar_state, -1);
		}
		if plus_response.clicked() {
			toolbar_state.annotation_size_wheel_accumulator = 0.0;
			size_changed |= style_kind.apply_size_steps(toolbar_state, 1);
		}
		if size_changed {
			toolbar_state.needs_redraw = true;
		}
	}

	fn paint_frozen_annotation_size_display(
		ui: &Ui,
		toolbar_state: &FrozenToolbarState,
		style_kind: FrozenAnnotationStyleToolbarKind,
		display_rect: Rect,
		size_label: &str,
		text_color: Color32,
	) {
		match style_kind {
			FrozenAnnotationStyleToolbarKind::Text => {
				ui.painter().text(
					display_rect.center(),
					Align2::CENTER_CENTER,
					size_label,
					FontId::new(13.0, FontFamily::Proportional),
					text_color,
				);
			},
			FrozenAnnotationStyleToolbarKind::Pen => {
				let preview_width = toolbar_state.brush_style.stroke_width_points.clamp(1.0, 10.0);
				let preview_center = Pos2::new(
					display_rect.left()
						+ 10.0 + FROZEN_ANNOTATION_TOOLBAR_PEN_PREVIEW_LENGTH_POINTS * 0.5,
					display_rect.center().y,
				);
				let preview_half_length = FROZEN_ANNOTATION_TOOLBAR_PEN_PREVIEW_LENGTH_POINTS * 0.5;
				let preview_start =
					Pos2::new(preview_center.x - preview_half_length, preview_center.y);
				let preview_end =
					Pos2::new(preview_center.x + preview_half_length, preview_center.y);
				let preview_color = toolbar_state.brush_style.color.swatch_fill();

				ui.painter().line_segment(
					[preview_start, preview_end],
					Stroke::new(preview_width, preview_color),
				);
				ui.painter().circle_filled(preview_start, preview_width * 0.5, preview_color);
				ui.painter().circle_filled(preview_end, preview_width * 0.5, preview_color);
				ui.painter().text(
					Pos2::new(
						preview_end.x + FROZEN_ANNOTATION_TOOLBAR_SIZE_PREVIEW_GAP_POINTS,
						display_rect.center().y,
					),
					Align2::LEFT_CENTER,
					size_label,
					FontId::new(13.0, FontFamily::Proportional),
					text_color,
				);
			},
		}
	}

	fn render_frozen_annotation_color_swatch(
		ui: &mut Ui,
		color: FrozenAnnotationColor,
		selected: bool,
		theme: HudTheme,
	) -> bool {
		let response = ui.allocate_response(
			Vec2::splat(FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS),
			Sense::click(),
		);
		let radius = FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS * 0.5 - 1.0;
		let stroke_color = match theme {
			HudTheme::Dark => {
				if selected {
					Color32::WHITE
				} else {
					Color32::from_white_alpha(96)
				}
			},
			HudTheme::Light => {
				if selected {
					Color32::BLACK
				} else {
					Color32::from_black_alpha(96)
				}
			},
		};

		ui.painter().circle_filled(response.rect.center(), radius, color.swatch_fill());
		ui.painter().circle_stroke(
			response.rect.center(),
			radius,
			Stroke::new(if selected { 2.0 } else { 1.0 }, stroke_color),
		);

		response.clicked()
	}

	pub(in crate::overlay) fn frozen_toolbar_button_style(
		theme: HudTheme,
		action_ready: bool,
		hovered: bool,
		selected: bool,
	) -> FrozenToolbarButtonStyle {
		let hover_anim = if hovered { 1.0 } else { 0.0 };
		let selected_anim = if selected { 1.0 } else { 0.0 };
		let (normal_color, hover_color, selected_color, hover_bg, selected_bg) =
			Self::frozen_toolbar_colors(theme);
		let mut icon_color = if action_ready {
			normal_color
		} else {
			Color32::from_rgba_unmultiplied(
				normal_color.r(),
				normal_color.g(),
				normal_color.b(),
				(normal_color.a() as f32 * 0.45).round() as u8,
			)
		};
		let mut bg_color = Color32::from_rgba_unmultiplied(255, 255, 255, 0);

		if selected_anim > 0.0 {
			icon_color = Self::blend_color(icon_color, selected_color, selected_anim);
			bg_color = Self::blend_color(bg_color, selected_bg, selected_anim);
		}
		if hover_anim > 0.0 {
			icon_color = Self::blend_color(icon_color, hover_color, hover_anim);
			bg_color = Self::blend_color(bg_color, hover_bg, hover_anim * (1.0 - selected_anim));
		}

		FrozenToolbarButtonStyle { icon_color, bg_color, border_color: None }
	}

	pub(in crate::overlay) fn frozen_toolbar_colors(
		theme: HudTheme,
	) -> (Color32, Color32, Color32, Color32, Color32) {
		let (normal_color, hover_color, selected_color) = match theme {
			HudTheme::Dark => (
				Color32::from_rgba_unmultiplied(255, 255, 255, 160),
				Color32::from_rgba_unmultiplied(255, 255, 255, 222),
				Color32::from_rgba_unmultiplied(255, 255, 255, 255),
			),
			HudTheme::Light => (
				Color32::from_rgba_unmultiplied(28, 28, 32, 182),
				Color32::from_rgba_unmultiplied(28, 28, 32, 220),
				Color32::from_rgba_unmultiplied(28, 28, 32, 255),
			),
		};
		let hover_bg = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 20),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 20),
		};
		let selected_bg = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 28),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 24),
		};

		(normal_color, hover_color, selected_color, hover_bg, selected_bg)
	}

	pub(in crate::overlay) fn blend_color(a: Color32, b: Color32, t: f32) -> Color32 {
		let t = t.clamp(0.0, 1.0);
		let u = 1.0 - t;

		Color32::from_rgba_unmultiplied(
			((f32::from(a.r()) * u + f32::from(b.r()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.g()) * u + f32::from(b.g()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.b()) * u + f32::from(b.b()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.a()) * u + f32::from(b.a()) * t).round().clamp(0.0, 255.0)) as u8,
		)
	}

	pub(in crate::overlay) fn clamp_toolbar_position(
		screen_rect: Rect,
		toolbar_size: Vec2,
		cursor: Pos2,
		side_margin: f32,
		top_margin: f32,
	) -> Pos2 {
		let min_x = screen_rect.min.x + side_margin;
		let min_y = screen_rect.min.y + top_margin;
		let max_x = (screen_rect.max.x - toolbar_size.x - side_margin).max(min_x);
		let max_y = (screen_rect.max.y - toolbar_size.y - top_margin * 0.5).max(min_y);

		Pos2::new(cursor.x.clamp(min_x, max_x.max(min_x)), cursor.y.clamp(min_y, max_y.max(min_y)))
	}
}
