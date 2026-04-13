use std::sync::{Arc, OnceLock};
use std::time::Instant;

use egui::Context;
use egui::FontDefinitions;
use egui::Galley;
use egui::RawInput;
use egui::text::CCursor;

use crate::overlay::rendering::{
	FrozenSelectionResizeHandleGeometry, FrozenToolbarButtonStyle, SelectionDashedBorderCache,
	SelectionDashedBorderCacheKey, SelectionDashedBorderMetrics, SelectionFlowGeometryCache,
	SelectionFlowGeometryCacheKey, SelectionSizeBadgeLayout, SelectionSizeBadgePadding,
	SelectionSizeBadgeTarget, WindowRenderer,
};
use crate::overlay::{
	self, Align, Align2, Area, Color32, CornerRadius, FROZEN_SELECTION_DASHED_BORDER_WIDTH_PX,
	FROZEN_SELECTION_RESIZE_HANDLE_CENTER_DOT_RADIUS_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_CORNER_KEEPOUT_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_HIT_OFFSET_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_HIT_SIZE_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_OUTER_RADIUS_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_STROKE_WIDTH_POINTS, FROZEN_SELECTION_SCRIM_ALPHA_DARK,
	FROZEN_SELECTION_SCRIM_ALPHA_LIGHT, FROZEN_TEXT_PREVIEW_PLACEHOLDER,
	FROZEN_TOOLBAR_BUTTON_SIZE_POINTS, FROZEN_TOOLBAR_ITEM_SPACING_POINTS, FontFamily, FontId,
	FrozenBrushState, FrozenCaptureSource, FrozenCommittedOverlay, FrozenEditKind,
	FrozenSelectionCorner, FrozenTextAnnotation, FrozenTextColor, FrozenTextEditState,
	FrozenTextStyle, FrozenToolbarPointerState, FrozenToolbarState, FrozenToolbarTool,
	HUD_PILL_INNER_MARGIN_X_POINTS, HUD_PILL_STROKE_WIDTH_POINTS, HudPillGeometry, HudTheme, Id,
	LIVE_DRAG_SELECTION_SCRIM_ALPHA_DARK, LIVE_DRAG_SELECTION_SCRIM_ALPHA_LIGHT,
	LIVE_DRAG_START_THRESHOLD_PX, LayerId, Layout, Mesh, MonitorRect, Order, OverlayMode,
	OverlaySession, OverlayState, Painter, Pos2, Rect, RectPoints, SELECTION_DASHED_BORDER_ALPHA,
	SELECTION_DASHED_BORDER_DASH_LENGTH_PX, SELECTION_DASHED_BORDER_GAP_LENGTH_PX,
	SELECTION_DASHED_BORDER_WIDTH_PX, SELECTION_FLOW_CORE_FLOW_WIDTH,
	SELECTION_FLOW_CORNER_RADIUS_PX, SELECTION_FLOW_FLOW_BOOST, SELECTION_FLOW_LIGHT_PALETTE,
	SELECTION_FLOW_MAX_SEGMENTS, SELECTION_FLOW_MIN_SEGMENTS, SELECTION_FLOW_PALETTE,
	SELECTION_FLOW_SAMPLE_STEP_PX, SELECTION_FLOW_SPEED, SELECTION_SIZE_BADGE_FAR_SHADOW_OFFSET_PX,
	SELECTION_SIZE_BADGE_FONT_SIZE_POINTS, SELECTION_SIZE_BADGE_GAP_PX,
	SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX, SELECTION_SIZE_BADGE_NEAR_SHADOW_OFFSET_PX,
	SELECTION_SIZE_BADGE_OUTLINE_OFFSET_PX, SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX,
	SELECTION_SIZE_BADGE_TEXT_OUTSET_POINTS, SelectionFlowStyle, Sense, Shape, Stroke, StrokeKind,
	TOOLBAR_CAPTURE_GAP_PX, TOOLBAR_EXPANDED_HEIGHT_PX, TOOLBAR_PILL_INNER_MARGIN_Y_POINTS,
	TOOLBAR_SCREEN_MARGIN_PX, ToolbarPlacement, Ui, UiBuilder, Vec2,
	frozen_toolbar_corner_radius_u8, regular,
};

const FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS: f32 = 4.0;
const FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS: f32 = 24.0;
const FROZEN_ANNOTATION_TOOLBAR_SECTION_DIVIDER_ALPHA_DARK: u8 = 60;
const FROZEN_ANNOTATION_TOOLBAR_SECTION_DIVIDER_ALPHA_LIGHT: u8 = 72;
const FROZEN_ANNOTATION_TOOLBAR_SWATCH_SIZE_POINTS: f32 = 16.0;
const FROZEN_ANNOTATION_TOOLBAR_SWATCH_GAP_POINTS: f32 = 6.0;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_BUTTON_WIDTH_POINTS: f32 = 20.0;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_DISPLAY_WIDTH_POINTS: f32 = 58.0;
const FROZEN_ANNOTATION_TOOLBAR_PEN_SIZE_DISPLAY_WIDTH_POINTS: f32 = 84.0;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_CAPSULE_CORNER_RADIUS_POINTS: u8 = 8;
const FROZEN_ANNOTATION_TOOLBAR_SIZE_PREVIEW_GAP_POINTS: f32 = 8.0;
const FROZEN_ANNOTATION_TOOLBAR_PEN_PREVIEW_LENGTH_POINTS: f32 = 18.0;
const FROZEN_TEXT_INTERACTION_PADDING_X_POINTS: f32 = 8.0;
const FROZEN_TEXT_INTERACTION_PADDING_Y_POINTS: f32 = 6.0;

#[derive(Clone, Copy)]
pub(in crate::overlay) struct SelectionScrimStyle {
	pub(in crate::overlay) scrim_fill: Color32,
	pub(in crate::overlay) stroke_width_override: Option<f32>,
	pub(in crate::overlay) exclude_resize_handle_corners: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum FrozenAnnotationStyleToolbarKind {
	Pen,
	Text,
}

#[derive(Clone, Copy)]
struct FrozenAnnotationSizeControlAppearance {
	capsule_fill: Color32,
	capsule_stroke: Color32,
	divider_color: Color32,
	button_hover_fill: Color32,
	text_color: Color32,
}

impl FrozenAnnotationStyleToolbarKind {
	fn from_toolbar_state(toolbar_state: &FrozenToolbarState) -> Option<Self> {
		match toolbar_state.selected_tool {
			FrozenToolbarTool::Pen => Some(Self::Pen),
			FrozenToolbarTool::Text => Some(Self::Text),
			_ => None,
		}
	}

	const fn size_hover_text(self) -> &'static str {
		match self {
			Self::Pen => "Scroll or use +/- to adjust stroke size",
			Self::Text => "Scroll or use +/- to adjust text size",
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

	const fn decrease_hover_text(self) -> &'static str {
		match self {
			Self::Pen => "Smaller stroke",
			Self::Text => "Smaller text",
		}
	}

	const fn increase_hover_text(self) -> &'static str {
		match self {
			Self::Pen => "Larger stroke",
			Self::Text => "Larger text",
		}
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

	fn selected_color(self, toolbar_state: &FrozenToolbarState) -> FrozenTextColor {
		match self {
			Self::Pen => toolbar_state.brush_style.color,
			Self::Text => toolbar_state.text_style.color,
		}
	}

	fn set_color(self, toolbar_state: &mut FrozenToolbarState, color: FrozenTextColor) -> bool {
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

impl WindowRenderer {
	fn frozen_text_measurement_ctx() -> &'static Context {
		static CTX: OnceLock<Context> = OnceLock::new();

		CTX.get_or_init(|| {
			let ctx = Context::default();
			let mut fonts = FontDefinitions::default();

			ctx.set_fonts({
				super::configure_egui_fonts(&mut fonts);

				fonts
			});

			let _ = ctx.run_ui(RawInput::default(), |_ui| {});

			ctx
		})
	}

	fn frozen_text_edit_layout(painter: &Painter, text: &str, font_id: &FontId) -> Arc<Galley> {
		painter.layout_no_wrap(text.to_owned(), font_id.clone(), Color32::WHITE)
	}

	fn frozen_text_edit_measurement_layout(text: &str, font_id: &FontId) -> Arc<Galley> {
		Self::frozen_text_measurement_ctx().fonts_mut(|fonts| {
			fonts.layout_no_wrap(text.to_owned(), font_id.clone(), Color32::WHITE)
		})
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_live_capture_affordances(
		ctx: &Context,
		painter: &Painter,
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		theme: HudTheme,
		selection_flow_enabled: bool,
		selection_flow_stroke_width_px: f32,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let mut has_rect = false;

		if !matches!(state.mode, OverlayMode::Live) {
			return false;
		}

		let primary_not_down = !ctx.input(|i| i.pointer.primary_down());

		if let Some(hovered_window) = state.hovered_window_rect
			&& hovered_window.monitor_id == monitor.id
		{
			let rect = Rect::from_min_size(
				Pos2::new(hovered_window.rect.x as f32, hovered_window.rect.y as f32),
				Vec2::new(hovered_window.rect.width as f32, hovered_window.rect.height as f32),
			);
			let rect = rect.intersect(screen_rect);

			if rect.width() >= LIVE_DRAG_START_THRESHOLD_PX
				&& rect.height() >= LIVE_DRAG_START_THRESHOLD_PX
			{
				Self::render_live_drag_selection_scrim(painter, rect, screen_rect, theme);

				if selection_flow_enabled {
					Self::render_selection_flow_ring(
						painter,
						rect,
						ctx,
						theme,
						SelectionFlowStyle::Band,
						selection_flow_stroke_width_px,
						selection_flow_geometry_cache,
					);
				}

				has_rect = true;
			}
		}
		if let Some(rect) = Self::live_drag_focus_rect(state, monitor, screen_rect) {
			Self::render_live_drag_selection_affordance(
				painter,
				rect,
				screen_rect,
				theme,
				selection_dashed_border_cache,
			);

			has_rect = true;
		}
		if let Some(target) =
			Self::live_capture_size_badge_target(state, monitor, screen_rect, primary_not_down)
		{
			Self::render_selection_size_badge(
				ctx,
				painter,
				monitor,
				screen_rect,
				target,
				None,
				false,
				theme,
			);

			has_rect = true;
		}

		has_rect
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_frozen_capture_affordance(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		theme: HudTheme,
		frozen_selection_resize_handles_enabled: bool,
		frozen_capture_source: FrozenCaptureSource,
		frozen_toolbar_reserved_rect: Option<Rect>,
		frozen_edit_history: &[FrozenEditKind],
		frozen_brush_state: Option<&FrozenBrushState>,
		frozen_text_annotations: &[FrozenTextAnnotation],
		frozen_text_edit: Option<&FrozenTextEditState>,
		frozen_text_style: FrozenTextStyle,
		_frozen_capture_is_fullscreen_fallback: bool,
		_selection_flow_enabled: bool,
		_selection_flow_stroke_width_px: f32,
		_selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let Some(rect) = Self::frozen_capture_focus_rect(state, screen_rect) else {
			return false;
		};
		let layer =
			LayerId::new(Order::Foreground, Id::new(format!("frozen-pending-{}", monitor.id)));
		let painter = ctx.layer_painter(layer);
		let show_resize_handles = frozen_selection_resize_handles_enabled
			&& frozen_capture_source == FrozenCaptureSource::DragRegion;
		let mut has_affordance = Self::render_frozen_selection_scrim(
			&painter,
			rect,
			screen_rect,
			theme,
			show_resize_handles,
			selection_dashed_border_cache,
		);
		let brush_painter = painter.with_clip_rect(rect);

		has_affordance |= Self::render_frozen_committed_overlay_annotations(
			&brush_painter,
			frozen_edit_history,
			frozen_brush_state,
			frozen_text_annotations,
		);

		if let Some(target) = Self::frozen_capture_size_badge_target(state, screen_rect) {
			Self::render_selection_size_badge(
				ctx,
				&painter,
				monitor,
				screen_rect,
				target,
				frozen_toolbar_reserved_rect,
				frozen_capture_source == FrozenCaptureSource::DragRegion,
				theme,
			);

			has_affordance = true;
		}

		if show_resize_handles && let Some(capture_rect) = state.frozen_capture_rect {
			has_affordance |=
				Self::render_frozen_selection_resize_handles(&painter, capture_rect, theme);
		}

		if let Some(mosaic_preview_rect) = state.frozen_mosaic_preview_rect {
			let preview_rect = Self::selection_focus_rect(mosaic_preview_rect, screen_rect);
			let preview_fill = match theme {
				HudTheme::Dark => Color32::from_rgba_unmultiplied(110, 196, 255, 38),
				HudTheme::Light => Color32::from_rgba_unmultiplied(34, 132, 214, 30),
			};

			painter.rect_filled(preview_rect, 10.0, preview_fill);

			has_affordance |= Self::render_selection_dashed_border(
				&painter,
				preview_rect,
				screen_rect,
				theme,
				Some(2.1),
				false,
				selection_dashed_border_cache,
			);
		}

		has_affordance |= Self::render_frozen_text_annotations(
			&brush_painter,
			theme,
			&[],
			frozen_text_edit,
			frozen_text_style,
		);

		if let Some(frozen_brush_state) = frozen_brush_state {
			has_affordance |=
				Self::render_active_frozen_brush_stroke(&brush_painter, frozen_brush_state);
		}

		has_affordance
	}

	fn render_frozen_committed_overlay_annotations(
		brush_painter: &Painter,
		frozen_edit_history: &[FrozenEditKind],
		frozen_brush_state: Option<&FrozenBrushState>,
		frozen_text_annotations: &[FrozenTextAnnotation],
	) -> bool {
		let font_fill = |annotation: &FrozenTextAnnotation| {
			(
				FontId::proportional(annotation.style.font_size_points),
				annotation.style.color.swatch_fill(),
			)
		};
		let brush_strokes =
			frozen_brush_state.map_or_else(|| &[][..], |state| state.committed_strokes.as_slice());
		let mut drew = false;

		OverlaySession::for_each_frozen_committed_overlay(
			frozen_edit_history,
			brush_strokes,
			frozen_text_annotations,
			|overlay| match overlay {
				FrozenCommittedOverlay::Brush(stroke) => {
					drew |= Self::paint_frozen_brush_stroke(
						brush_painter,
						&stroke.points,
						stroke.style.stroke_width_points * 0.5,
						stroke.style.color.swatch_fill(),
					);
				},
				FrozenCommittedOverlay::Text(annotation) => {
					let (font_id, fill) = font_fill(annotation);

					Self::paint_frozen_text_label(
						brush_painter,
						annotation.anchor,
						annotation.text.as_str(),
						&font_id,
						fill,
					);

					drew = true;
				},
			},
		);

		drew
	}

	fn render_active_frozen_brush_stroke(
		painter: &Painter,
		frozen_brush_state: &FrozenBrushState,
	) -> bool {
		let Some(active_stroke) = &frozen_brush_state.active_stroke else {
			return false;
		};
		let preview_points = OverlaySession::preview_frozen_brush_points(active_stroke);

		Self::paint_frozen_brush_stroke(
			painter,
			&preview_points,
			active_stroke.style.stroke_width_points * 0.5,
			active_stroke.style.color.swatch_fill(),
		)
	}

	fn paint_frozen_brush_stroke(
		painter: &Painter,
		points: &[Pos2],
		radius: f32,
		color: Color32,
	) -> bool {
		let rendered_points = OverlaySession::rendered_frozen_brush_points(
			points,
			overlay::FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
		);

		match rendered_points.as_slice() {
			[] => false,
			[point] => {
				painter.circle_filled(*point, radius, color);

				true
			},
			_ => {
				let first = rendered_points[0];
				let second = rendered_points[1];
				let penultimate = rendered_points[rendered_points.len().saturating_sub(2)];
				let last = rendered_points[rendered_points.len().saturating_sub(1)];
				let total_length = rendered_points
					.windows(2)
					.fold(0.0, |length, window| length + window[0].distance(window[1]));
				let max_cap_inset = (total_length * 0.5 - 0.01).max(0.0);
				let cap_inset = radius.min(max_cap_inset);
				let mut body_points = rendered_points.clone();

				if cap_inset > 0.0 {
					let start_delta = second - first;

					if start_delta.length_sq() > f32::EPSILON {
						let start_dir = start_delta.normalized();

						body_points[0] = first + (start_dir * cap_inset);
					}

					let end_delta = last - penultimate;

					if end_delta.length_sq() > f32::EPSILON {
						let end_dir = end_delta.normalized();
						let last_index = body_points.len().saturating_sub(1);

						body_points[last_index] = last - (end_dir * cap_inset);
					}
				}

				painter.add(Shape::line(body_points, Stroke::new(radius * 2.0, color)));
				painter.circle_filled(first, radius, color);
				painter.circle_filled(last, radius, color);

				true
			},
		}
	}

	fn render_frozen_text_annotations(
		painter: &Painter,
		theme: HudTheme,
		annotations: &[FrozenTextAnnotation],
		text_edit: Option<&FrozenTextEditState>,
		text_style: FrozenTextStyle,
	) -> bool {
		let mut rendered = false;

		for annotation in annotations {
			let font_id = FontId::proportional(annotation.style.font_size_points);

			Self::paint_frozen_text_label(
				painter,
				annotation.anchor,
				annotation.text.as_str(),
				&font_id,
				annotation.style.color.swatch_fill(),
			);

			rendered = true;
		}

		if let Some(text_edit) = text_edit {
			let (visible_text, caret_char_index) = text_edit.visible_text_and_caret_char_index();
			let font_id = FontId::proportional(text_style.font_size_points);
			let (text, color) = if visible_text.is_empty() {
				(
					FROZEN_TEXT_PREVIEW_PLACEHOLDER,
					Self::frozen_text_placeholder_fill(text_style.color, theme),
				)
			} else {
				(visible_text.as_str(), text_style.color.swatch_fill())
			};

			Self::paint_frozen_text_label(painter, text_edit.anchor, text, &font_id, color);

			if let Some(caret_char_index) = caret_char_index
				&& Self::frozen_text_caret_visible(
					text_edit.caret_blink_elapsed_secs_at(Instant::now()),
				) {
				Self::paint_frozen_text_caret(
					painter,
					text_edit.anchor,
					visible_text.as_str(),
					&font_id,
					caret_char_index,
					text_style.color.swatch_fill(),
				);
			}

			rendered = true;
		}

		rendered
	}

	fn paint_frozen_text_label(
		painter: &Painter,
		anchor: Pos2,
		text: &str,
		font_id: &FontId,
		fill: Color32,
	) {
		let galley = painter.layout_no_wrap(text.to_owned(), font_id.clone(), fill);

		painter.galley(anchor, galley, fill);
	}

	fn paint_frozen_text_caret(
		painter: &Painter,
		anchor: Pos2,
		text: &str,
		font_id: &FontId,
		caret_char_index: usize,
		fill: Color32,
	) {
		let caret_rect = Self::frozen_text_edit_caret_rect_at_char_index(
			painter,
			anchor,
			text,
			font_id,
			caret_char_index,
		);
		let caret_top = caret_rect.min;
		let caret_bottom = Pos2::new(caret_rect.min.x, caret_rect.max.y);

		painter.line_segment([caret_top, caret_bottom], Stroke::new(1.5, fill));
	}

	pub(in crate::overlay) fn frozen_text_placeholder_fill(
		color: FrozenTextColor,
		theme: HudTheme,
	) -> Color32 {
		let [r, g, b, _] = color.swatch_fill().to_array();
		let soften_ratio = match theme {
			HudTheme::Dark => 0.46,
			HudTheme::Light => 0.56,
		};
		let alpha = match theme {
			HudTheme::Dark => 196,
			HudTheme::Light => 172,
		};

		Color32::from_rgba_unmultiplied(
			Self::blend_color_channel(r, 255, soften_ratio),
			Self::blend_color_channel(g, 255, soften_ratio),
			Self::blend_color_channel(b, 255, soften_ratio),
			alpha,
		)
	}

	fn blend_color_channel(from: u8, to: u8, ratio: f32) -> u8 {
		(from as f32 + (to as f32 - from as f32) * ratio).clamp(0.0, 255.0).round() as u8
	}

	#[cfg_attr(not(test), allow(dead_code))]
	pub(in crate::overlay) fn frozen_text_edit_caret_rect(
		painter: &Painter,
		anchor: Pos2,
		text: &str,
		font_id: &FontId,
	) -> Rect {
		Self::frozen_text_edit_caret_rect_at_char_index(
			painter,
			anchor,
			text,
			font_id,
			text.chars().count(),
		)
	}

	pub(in crate::overlay) fn frozen_text_edit_caret_rect_at_char_index(
		painter: &Painter,
		anchor: Pos2,
		text: &str,
		font_id: &FontId,
		caret_char_index: usize,
	) -> Rect {
		let galley = Self::frozen_text_edit_layout(painter, text, font_id);
		let caret =
			galley.pos_from_cursor(CCursor::new(caret_char_index.min(text.chars().count())));
		let caret_height = caret.height().max(font_id.size);

		Rect::from_min_max(
			Pos2::new(anchor.x + caret.min.x, anchor.y + caret.min.y),
			Pos2::new(anchor.x + caret.max.x, anchor.y + caret.min.y + caret_height),
		)
	}

	pub(in crate::overlay) fn frozen_text_edit_caret_rect_for_window(
		&self,
		anchor: Pos2,
		text: &str,
		font_id: &FontId,
		caret_char_index: usize,
	) -> Rect {
		let painter = self
			.egui_ctx
			.layer_painter(LayerId::new(Order::Foreground, Id::new("frozen-text-ime-caret")));

		Self::frozen_text_edit_caret_rect_at_char_index(
			&painter,
			anchor,
			text,
			font_id,
			caret_char_index,
		)
	}

	pub(in crate::overlay) fn frozen_text_edit_interaction_rect(
		anchor: Pos2,
		text: &str,
		font_id: &FontId,
	) -> Rect {
		let text = if text.is_empty() { FROZEN_TEXT_PREVIEW_PLACEHOLDER } else { text };
		let galley = Self::frozen_text_edit_measurement_layout(text, font_id);
		let text_size =
			Vec2::new(galley.size().x.max(font_id.size), galley.size().y.max(font_id.size));

		Rect::from_min_max(
			Pos2::new(
				anchor.x - FROZEN_TEXT_INTERACTION_PADDING_X_POINTS,
				anchor.y - FROZEN_TEXT_INTERACTION_PADDING_Y_POINTS,
			),
			Pos2::new(
				anchor.x + text_size.x + FROZEN_TEXT_INTERACTION_PADDING_X_POINTS,
				anchor.y + text_size.y + FROZEN_TEXT_INTERACTION_PADDING_Y_POINTS,
			),
		)
	}

	pub(in crate::overlay) fn frozen_text_caret_visible(time_secs: f64) -> bool {
		(time_secs.rem_euclid(crate::overlay::FROZEN_TEXT_CARET_BLINK_PERIOD_SECS))
			< crate::overlay::FROZEN_TEXT_CARET_BLINK_PERIOD_SECS * 0.5
	}

	pub(in crate::overlay) fn frozen_capture_focus_rect(
		state: &OverlayState,
		screen_rect: Rect,
	) -> Option<Rect> {
		let capture_rect = state.frozen_capture_rect?;

		Some(Self::selection_focus_rect(capture_rect, screen_rect))
	}

	pub(in crate::overlay) fn live_drag_focus_rect(
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
	) -> Option<Rect> {
		let drag_rect = state.drag_rect?;

		if drag_rect.monitor_id != monitor.id {
			return None;
		}

		let rect = Self::selection_focus_rect(drag_rect.rect, screen_rect);

		if rect.width() < LIVE_DRAG_START_THRESHOLD_PX
			|| rect.height() < LIVE_DRAG_START_THRESHOLD_PX
		{
			return None;
		}

		Some(rect)
	}

	pub(in crate::overlay) fn selection_focus_rect(rect: RectPoints, screen_rect: Rect) -> Rect {
		Rect::from_min_size(
			Pos2::new(rect.x as f32, rect.y as f32),
			Vec2::new(rect.width as f32, rect.height as f32),
		)
		.intersect(screen_rect)
	}

	pub(in crate::overlay) fn selection_size_badge_target_from_rect(
		rect_points: RectPoints,
		screen_rect: Rect,
	) -> Option<SelectionSizeBadgeTarget> {
		let rect = Self::selection_focus_rect(rect_points, screen_rect);

		if rect.width() <= 0.0 || rect.height() <= 0.0 {
			return None;
		}

		Some(SelectionSizeBadgeTarget { rect, size_points: rect_points })
	}

	pub(in crate::overlay) fn live_capture_size_badge_target(
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		primary_not_down: bool,
	) -> Option<SelectionSizeBadgeTarget> {
		if let Some(drag_rect) = state.drag_rect
			&& drag_rect.monitor_id == monitor.id
			&& let Some(target) =
				Self::selection_size_badge_target_from_rect(drag_rect.rect, screen_rect)
		{
			return Some(target);
		}
		if let Some(hovered_window) = state.hovered_window_rect
			&& hovered_window.monitor_id == monitor.id
			&& let Some(target) =
				Self::selection_size_badge_target_from_rect(hovered_window.rect, screen_rect)
		{
			return Some(target);
		}

		if primary_not_down && state.cursor.is_some_and(|cursor| monitor.contains(cursor)) {
			return Some(SelectionSizeBadgeTarget {
				rect: screen_rect,
				size_points: RectPoints::new(0, 0, monitor.width, monitor.height),
			});
		}

		None
	}

	pub(in crate::overlay) fn frozen_capture_size_badge_target(
		state: &OverlayState,
		screen_rect: Rect,
	) -> Option<SelectionSizeBadgeTarget> {
		let capture_rect = state.frozen_capture_rect?;

		Self::selection_size_badge_target_from_rect(capture_rect, screen_rect)
	}

	pub(in crate::overlay) fn frozen_selection_resize_handles(
		capture_rect: RectPoints,
	) -> [FrozenSelectionResizeHandleGeometry; 4] {
		let rect = Rect::from_min_size(
			Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
			Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
		);

		[
			Self::frozen_selection_resize_handle(FrozenSelectionCorner::TopLeft, rect.min),
			Self::frozen_selection_resize_handle(
				FrozenSelectionCorner::TopRight,
				Pos2::new(rect.max.x, rect.min.y),
			),
			Self::frozen_selection_resize_handle(
				FrozenSelectionCorner::BottomLeft,
				Pos2::new(rect.min.x, rect.max.y),
			),
			Self::frozen_selection_resize_handle(FrozenSelectionCorner::BottomRight, rect.max),
		]
	}

	fn frozen_selection_resize_handle(
		corner: FrozenSelectionCorner,
		anchor: Pos2,
	) -> FrozenSelectionResizeHandleGeometry {
		let hit_size = Vec2::splat(FROZEN_SELECTION_RESIZE_HANDLE_HIT_SIZE_POINTS);
		let hit_offset = FROZEN_SELECTION_RESIZE_HANDLE_HIT_OFFSET_POINTS;
		let hit_center = match corner {
			FrozenSelectionCorner::TopLeft => {
				Pos2::new(anchor.x - hit_offset, anchor.y - hit_offset)
			},
			FrozenSelectionCorner::TopRight => {
				Pos2::new(anchor.x + hit_offset, anchor.y - hit_offset)
			},
			FrozenSelectionCorner::BottomLeft => {
				Pos2::new(anchor.x - hit_offset, anchor.y + hit_offset)
			},
			FrozenSelectionCorner::BottomRight => {
				Pos2::new(anchor.x + hit_offset, anchor.y + hit_offset)
			},
		};

		FrozenSelectionResizeHandleGeometry {
			corner,
			anchor,
			hit_rect: Rect::from_center_size(hit_center, hit_size),
		}
	}

	pub(in crate::overlay) fn frozen_selection_resize_hit_test(
		capture_rect: RectPoints,
		cursor_local: Pos2,
	) -> Option<FrozenSelectionCorner> {
		let rect = Rect::from_min_size(
			Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
			Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
		);
		let mut best_corner = None;
		let mut best_distance_sq = f32::MAX;

		for handle in Self::frozen_selection_resize_handles(capture_rect) {
			if !handle.hit_rect.contains(cursor_local) {
				continue;
			}
			if rect.contains(cursor_local)
				&& !Self::frozen_selection_resize_handle_interior_hit(
					handle.corner,
					rect,
					cursor_local,
				) {
				continue;
			}

			let distance_sq = handle.anchor.distance_sq(cursor_local);

			if distance_sq < best_distance_sq {
				best_corner = Some(handle.corner);
				best_distance_sq = distance_sq;
			}
		}

		best_corner
	}

	fn frozen_selection_resize_handle_interior_hit(
		corner: FrozenSelectionCorner,
		rect: Rect,
		cursor_local: Pos2,
	) -> bool {
		let interior_reach_x =
			(rect.width() * 0.35).min(FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS);
		let interior_reach_y =
			(rect.height() * 0.35).min(FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS);

		match corner {
			FrozenSelectionCorner::TopLeft => {
				cursor_local.x <= rect.min.x + interior_reach_x
					&& cursor_local.y <= rect.min.y + interior_reach_y
			},
			FrozenSelectionCorner::TopRight => {
				cursor_local.x >= rect.max.x - interior_reach_x
					&& cursor_local.y <= rect.min.y + interior_reach_y
			},
			FrozenSelectionCorner::BottomLeft => {
				cursor_local.x <= rect.min.x + interior_reach_x
					&& cursor_local.y >= rect.max.y - interior_reach_y
			},
			FrozenSelectionCorner::BottomRight => {
				cursor_local.x >= rect.max.x - interior_reach_x
					&& cursor_local.y >= rect.max.y - interior_reach_y
			},
		}
	}

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
		let toolbar_size = Self::frozen_toolbar_size(toolbar_state);
		let default_pos = Self::frozen_toolbar_default_pos(
			screen_rect,
			capture_rect,
			toolbar_size,
			toolbar_placement,
		);
		let toolbar_pos = toolbar_state.floating_position.unwrap_or(default_pos);

		if !overlay::frozen_toolbar_matches_default_slot(toolbar_pos, default_pos) {
			return None;
		}

		Some(Rect::from_min_size(toolbar_pos, toolbar_size))
	}

	pub(in crate::overlay) fn selection_size_badge_text(
		monitor: MonitorRect,
		size_points: RectPoints,
	) -> String {
		let size_pixels = monitor.local_rect_to_pixels(size_points);

		format!("{}x{}", size_pixels.width, size_pixels.height)
	}

	fn selection_size_badge_visual_overflow(pixels_per_point: f32) -> SelectionSizeBadgePadding {
		let points_per_pixel = 1.0 / pixels_per_point.max(f32::MIN_POSITIVE);
		let outline_offset = SELECTION_SIZE_BADGE_OUTLINE_OFFSET_PX * points_per_pixel;
		let near_shadow_offset = SELECTION_SIZE_BADGE_NEAR_SHADOW_OFFSET_PX * points_per_pixel;
		let far_shadow_offset = SELECTION_SIZE_BADGE_FAR_SHADOW_OFFSET_PX * points_per_pixel;

		SelectionSizeBadgePadding {
			left: outline_offset,
			right: outline_offset.max(near_shadow_offset),
			top: outline_offset,
			bottom: outline_offset.max(near_shadow_offset).max(far_shadow_offset),
		}
	}

	pub(in crate::overlay) fn selection_size_badge_layout(
		ctx: &Context,
		text: &str,
		theme: HudTheme,
		pixels_per_point: f32,
	) -> SelectionSizeBadgeLayout {
		let text_color = Self::hud_text_colors(theme).0;
		let font_id = FontId::new(SELECTION_SIZE_BADGE_FONT_SIZE_POINTS, FontFamily::Monospace);
		let galley = ctx
			.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), font_id.clone(), text_color));
		let text_size = galley.size();
		let visual_overflow = Self::selection_size_badge_visual_overflow(pixels_per_point);
		let base_padding = SELECTION_SIZE_BADGE_TEXT_OUTSET_POINTS * 0.5;
		let padding = SelectionSizeBadgePadding {
			left: base_padding + visual_overflow.left,
			right: base_padding + visual_overflow.right,
			top: base_padding + visual_overflow.top,
			bottom: base_padding + visual_overflow.bottom,
		};

		SelectionSizeBadgeLayout {
			text_size,
			badge_size: Vec2::new(
				(text_size.x + padding.left + padding.right).ceil(),
				(text_size.y + padding.top + padding.bottom).ceil(),
			),
			padding,
		}
	}

	#[cfg(test)]
	pub(in crate::overlay) fn selection_size_badge_rect(
		screen_rect: Rect,
		capture_rect: Rect,
		badge_size: Vec2,
	) -> Rect {
		Self::selection_size_badge_rect_with_reserved_rect(
			screen_rect,
			capture_rect,
			badge_size,
			None,
		)
	}

	pub(in crate::overlay) fn selection_size_badge_rect_with_reserved_rect(
		screen_rect: Rect,
		capture_rect: Rect,
		badge_size: Vec2,
		reserved_rect: Option<Rect>,
	) -> Rect {
		// Geometry priority contract:
		// 1. Keep the badge fully visible inside the viewport whenever the viewport can fit it.
		// 2. Keep the badge right-aligned to the capture rect whenever that still satisfies (1).
		// 3. Prefer the below-capture slot when it fits and does not hit a reserved rect.
		// 4. Otherwise stay inside the capture while avoiding the reserved rect when a
		//    non-overlapping inside band exists.
		// 5. If the reserved rect exhausts the in-capture space, try a right-aligned
		//    above-capture slot before accepting overlap.
		let min_x = screen_rect.min.x;
		let max_x = (screen_rect.max.x - badge_size.x).max(min_x);
		let aligned_x = capture_rect.max.x - badge_size.x;
		let x = aligned_x.clamp(min_x, max_x);
		let below_y = capture_rect.max.y + SELECTION_SIZE_BADGE_GAP_PX;
		let below_rect = Rect::from_min_size(Pos2::new(x, below_y), badge_size);
		let fits_below = below_rect.max.y
			<= screen_rect.max.y - SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX
			&& reserved_rect.is_none_or(|rect| !below_rect.intersects(rect));

		if fits_below {
			return below_rect;
		}

		let screen_max_y = (screen_rect.max.y - badge_size.y).max(screen_rect.min.y);
		let max_inside_y =
			(capture_rect.max.y - badge_size.y).min(screen_max_y).max(screen_rect.min.y);
		let min_inside_y = capture_rect.min.y.min(max_inside_y).max(screen_rect.min.y);
		let preferred_inside_y =
			(capture_rect.max.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX - badge_size.y)
				.clamp(min_inside_y, max_inside_y);
		let preferred_inside_rect =
			Rect::from_min_size(Pos2::new(x, preferred_inside_y), badge_size);

		if reserved_rect.is_none_or(|rect| !preferred_inside_rect.intersects(rect)) {
			return preferred_inside_rect;
		}

		if let Some(reserved_rect) = reserved_rect {
			let upper_y =
				reserved_rect.min.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX - badge_size.y;
			let lower_y = reserved_rect.max.y + SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX;
			let candidate_ys = if reserved_rect.center().y <= capture_rect.center().y {
				[Some(lower_y), Some(upper_y)]
			} else {
				[Some(upper_y), Some(lower_y)]
			};

			for candidate_y in candidate_ys.into_iter().flatten() {
				if candidate_y < min_inside_y || candidate_y > max_inside_y {
					continue;
				}

				let candidate_rect = Rect::from_min_size(Pos2::new(x, candidate_y), badge_size);

				if !candidate_rect.intersects(reserved_rect) {
					return candidate_rect;
				}
			}

			let above_y = capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX - badge_size.y;

			if above_y >= screen_rect.min.y {
				let above_rect = Rect::from_min_size(Pos2::new(x, above_y), badge_size);

				if !above_rect.intersects(reserved_rect) {
					return above_rect;
				}
			}
		}

		preferred_inside_rect
	}

	pub(in crate::overlay) fn selection_size_badge_rect_preferring_outside_with_reserved_rect(
		screen_rect: Rect,
		capture_rect: Rect,
		badge_size: Vec2,
		reserved_rect: Option<Rect>,
	) -> Rect {
		let min_x = screen_rect.min.x;
		let max_x = (screen_rect.max.x - badge_size.x).max(min_x);
		let aligned_x = capture_rect.max.x - badge_size.x;
		let x = aligned_x.clamp(min_x, max_x);
		let below_y = capture_rect.max.y + SELECTION_SIZE_BADGE_GAP_PX;
		let below_rect = Rect::from_min_size(Pos2::new(x, below_y), badge_size);
		let fits_below = below_rect.max.y
			<= screen_rect.max.y - SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX
			&& reserved_rect.is_none_or(|rect| !below_rect.intersects(rect));

		if fits_below {
			return below_rect;
		}

		let above_y = capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX - badge_size.y;
		let above_rect = Rect::from_min_size(Pos2::new(x, above_y), badge_size);
		let fits_above = above_rect.min.y >= screen_rect.min.y
			&& reserved_rect.is_none_or(|rect| !above_rect.intersects(rect));

		if fits_above {
			return above_rect;
		}

		Self::selection_size_badge_rect_with_reserved_rect(
			screen_rect,
			capture_rect,
			badge_size,
			reserved_rect,
		)
	}

	pub(in crate::overlay) fn snap_points_to_pixel_grid(value: f32, pixels_per_point: f32) -> f32 {
		let pixels_per_point = pixels_per_point.max(f32::MIN_POSITIVE);

		(value * pixels_per_point).round() / pixels_per_point
	}

	pub(in crate::overlay) fn snap_pos_to_pixel_grid(pos: Pos2, pixels_per_point: f32) -> Pos2 {
		Pos2::new(
			Self::snap_points_to_pixel_grid(pos.x, pixels_per_point),
			Self::snap_points_to_pixel_grid(pos.y, pixels_per_point),
		)
	}

	pub(in crate::overlay) fn selection_size_badge_text_anchor(
		badge_rect: Rect,
		layout: SelectionSizeBadgeLayout,
		pixels_per_point: f32,
	) -> Pos2 {
		Self::snap_pos_to_pixel_grid(
			Pos2::new(
				badge_rect.max.x - layout.padding.right,
				badge_rect.min.y + layout.padding.top + layout.text_size.y * 0.5,
			),
			pixels_per_point,
		)
	}

	#[cfg(test)]
	pub(in crate::overlay) fn selection_size_badge_visual_bounds(
		text_anchor: Pos2,
		text_size: Vec2,
		pixels_per_point: f32,
	) -> Rect {
		let visual_overflow = Self::selection_size_badge_visual_overflow(pixels_per_point);

		Rect::from_min_max(
			Pos2::new(
				text_anchor.x - text_size.x - visual_overflow.left,
				text_anchor.y - text_size.y * 0.5 - visual_overflow.top,
			),
			Pos2::new(
				text_anchor.x + visual_overflow.right,
				text_anchor.y + text_size.y * 0.5 + visual_overflow.bottom,
			),
		)
	}

	pub(in crate::overlay) fn selection_size_badge_text_colors(
		theme: HudTheme,
	) -> (Color32, Color32, Color32, Color32) {
		match theme {
			HudTheme::Dark => (
				Color32::from_rgba_unmultiplied(255, 255, 255, 248),
				Color32::from_rgba_unmultiplied(0, 0, 0, 108),
				Color32::from_rgba_unmultiplied(0, 0, 0, 154),
				Color32::from_rgba_unmultiplied(0, 0, 0, 72),
			),
			HudTheme::Light => (
				Color32::from_rgba_unmultiplied(255, 255, 255, 252),
				Color32::from_rgba_unmultiplied(0, 0, 0, 156),
				Color32::from_rgba_unmultiplied(0, 0, 0, 196),
				Color32::from_rgba_unmultiplied(0, 0, 0, 96),
			),
		}
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn render_selection_size_badge(
		ctx: &Context,
		painter: &Painter,
		monitor: MonitorRect,
		screen_rect: Rect,
		target: SelectionSizeBadgeTarget,
		reserved_rect: Option<Rect>,
		prefer_outside_fallback: bool,
		theme: HudTheme,
	) {
		let text = Self::selection_size_badge_text(monitor, target.size_points);
		let pixels_per_point = painter.pixels_per_point();
		let layout = Self::selection_size_badge_layout(ctx, &text, theme, pixels_per_point);
		let badge_rect = if prefer_outside_fallback {
			Self::selection_size_badge_rect_preferring_outside_with_reserved_rect(
				screen_rect,
				target.rect,
				layout.badge_size,
				reserved_rect,
			)
		} else {
			Self::selection_size_badge_rect_with_reserved_rect(
				screen_rect,
				target.rect,
				layout.badge_size,
				reserved_rect,
			)
		};
		let font_id = FontId::new(SELECTION_SIZE_BADGE_FONT_SIZE_POINTS, FontFamily::Monospace);
		let points_per_pixel = 1.0 / pixels_per_point.max(f32::MIN_POSITIVE);
		let outline_offset = SELECTION_SIZE_BADGE_OUTLINE_OFFSET_PX * points_per_pixel;
		let near_shadow_offset = SELECTION_SIZE_BADGE_NEAR_SHADOW_OFFSET_PX * points_per_pixel;
		let far_shadow_offset = SELECTION_SIZE_BADGE_FAR_SHADOW_OFFSET_PX * points_per_pixel;
		let text_anchor =
			Self::selection_size_badge_text_anchor(badge_rect, layout, pixels_per_point);
		let (text_color, outline_color, near_shadow_color, far_shadow_color) =
			Self::selection_size_badge_text_colors(theme);

		painter.text(
			Self::snap_pos_to_pixel_grid(
				text_anchor + Vec2::new(0.0, far_shadow_offset),
				pixels_per_point,
			),
			Align2::RIGHT_CENTER,
			text.clone(),
			font_id.clone(),
			far_shadow_color,
		);

		for offset in [
			Vec2::new(-outline_offset, 0.0),
			Vec2::new(outline_offset, 0.0),
			Vec2::new(0.0, -outline_offset),
			Vec2::new(0.0, outline_offset),
		] {
			painter.text(
				Self::snap_pos_to_pixel_grid(text_anchor + offset, pixels_per_point),
				Align2::RIGHT_CENTER,
				text.clone(),
				font_id.clone(),
				outline_color,
			);
		}

		painter.text(
			Self::snap_pos_to_pixel_grid(
				text_anchor + Vec2::new(near_shadow_offset, near_shadow_offset),
				pixels_per_point,
			),
			Align2::RIGHT_CENTER,
			text.clone(),
			font_id.clone(),
			near_shadow_color,
		);
		painter.text(text_anchor, Align2::RIGHT_CENTER, text, font_id, text_color);
	}

	pub(in crate::overlay) fn frozen_selection_scrim_rects(
		screen_rect: Rect,
		focus_rect: Rect,
	) -> [Rect; 4] {
		[
			Rect::from_min_max(screen_rect.min, Pos2::new(screen_rect.max.x, focus_rect.min.y)),
			Rect::from_min_max(Pos2::new(screen_rect.min.x, focus_rect.max.y), screen_rect.max),
			Rect::from_min_max(
				Pos2::new(screen_rect.min.x, focus_rect.min.y),
				Pos2::new(focus_rect.min.x, focus_rect.max.y),
			),
			Rect::from_min_max(
				Pos2::new(focus_rect.max.x, focus_rect.min.y),
				Pos2::new(screen_rect.max.x, focus_rect.max.y),
			),
		]
	}

	pub(in crate::overlay) fn frozen_selection_scrim_color(theme: HudTheme) -> Color32 {
		let alpha = match theme {
			HudTheme::Light => FROZEN_SELECTION_SCRIM_ALPHA_LIGHT,
			HudTheme::Dark => FROZEN_SELECTION_SCRIM_ALPHA_DARK,
		};

		Color32::from_rgba_unmultiplied(0, 0, 0, alpha)
	}

	pub(in crate::overlay) fn live_drag_selection_scrim_color(theme: HudTheme) -> Color32 {
		let alpha = match theme {
			HudTheme::Light => LIVE_DRAG_SELECTION_SCRIM_ALPHA_LIGHT,
			HudTheme::Dark => LIVE_DRAG_SELECTION_SCRIM_ALPHA_DARK,
		};

		Color32::from_rgba_unmultiplied(0, 0, 0, alpha)
	}

	pub(in crate::overlay) fn render_frozen_selection_scrim(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		theme: HudTheme,
		exclude_resize_handle_corners: bool,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		Self::render_selection_scrim(
			painter,
			focus_rect,
			screen_rect,
			theme,
			SelectionScrimStyle {
				scrim_fill: Self::frozen_selection_scrim_color(theme),
				stroke_width_override: Some(FROZEN_SELECTION_DASHED_BORDER_WIDTH_PX),
				exclude_resize_handle_corners,
			},
			selection_dashed_border_cache,
		)
	}

	pub(in crate::overlay) fn render_live_drag_selection_scrim(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		theme: HudTheme,
	) -> bool {
		Self::render_selection_scrim_fill(
			painter,
			focus_rect,
			screen_rect,
			Self::live_drag_selection_scrim_color(theme),
		)
	}

	pub(in crate::overlay) fn render_live_drag_selection_affordance(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		theme: HudTheme,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		Self::render_selection_scrim(
			painter,
			focus_rect,
			screen_rect,
			theme,
			SelectionScrimStyle {
				scrim_fill: Self::live_drag_selection_scrim_color(theme),
				stroke_width_override: None,
				exclude_resize_handle_corners: false,
			},
			selection_dashed_border_cache,
		)
	}

	pub(in crate::overlay) fn render_selection_scrim(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		theme: HudTheme,
		style: SelectionScrimStyle,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let drew_scrim =
			Self::render_selection_scrim_fill(painter, focus_rect, screen_rect, style.scrim_fill);
		let drew_border = Self::render_selection_dashed_border(
			painter,
			focus_rect,
			screen_rect,
			theme,
			style.stroke_width_override,
			style.exclude_resize_handle_corners,
			selection_dashed_border_cache,
		);

		drew_scrim || drew_border
	}

	pub(in crate::overlay) fn render_selection_scrim_fill(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		scrim_fill: Color32,
	) -> bool {
		let scrim_rects = Self::frozen_selection_scrim_rects(screen_rect, focus_rect);
		let mut drew_scrim = false;

		for rect in scrim_rects {
			if rect.width() <= 0.0 || rect.height() <= 0.0 {
				continue;
			}

			painter.rect_filled(rect, 0.0, scrim_fill);

			drew_scrim = true;
		}

		drew_scrim
	}

	pub(in crate::overlay) fn render_selection_dashed_border(
		painter: &Painter,
		focus_rect: Rect,
		screen_rect: Rect,
		theme: HudTheme,
		stroke_width_override: Option<f32>,
		exclude_resize_handle_corners: bool,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let mut metrics = Self::selection_dashed_border_metrics(painter.pixels_per_point());

		if let Some(stroke_width) = stroke_width_override {
			metrics.stroke_width = stroke_width;
		}

		let border_outset =
			Self::selection_dashed_border_outset(metrics.stroke_width, painter.pixels_per_point());
		let Some(border_rect) =
			Self::selection_dashed_border_rect(screen_rect, focus_rect, border_outset)
		else {
			return false;
		};
		let corner_keepout = exclude_resize_handle_corners
			.then_some(FROZEN_SELECTION_RESIZE_HANDLE_CORNER_KEEPOUT_POINTS);
		let segments = Self::selection_dashed_border_cached_segments(
			selection_dashed_border_cache,
			border_rect,
			metrics.dash_length,
			metrics.gap_length,
			corner_keepout.unwrap_or(0.0),
		);

		if segments.is_empty() {
			return false;
		}

		let (outline_stroke, stroke) = Self::selection_dashed_border_strokes(metrics, theme);

		for segment in segments {
			painter.add(Shape::line_segment(*segment, outline_stroke));
			painter.add(Shape::line_segment(*segment, stroke));
		}

		true
	}

	fn selection_dashed_border_strokes(
		metrics: SelectionDashedBorderMetrics,
		theme: HudTheme,
	) -> (Stroke, Stroke) {
		let _ = theme;
		let outline = Stroke::new(
			metrics.stroke_width + 0.75,
			Color32::from_rgba_unmultiplied(229, 247, 255, 116),
		);
		let stroke = Stroke::new(
			metrics.stroke_width,
			Color32::from_rgba_unmultiplied(167, 223, 255, SELECTION_DASHED_BORDER_ALPHA),
		);

		(outline, stroke)
	}

	pub(in crate::overlay) fn selection_dashed_border_metrics(
		pixels_per_point: f32,
	) -> SelectionDashedBorderMetrics {
		let points_per_pixel = 1.0 / pixels_per_point.max(f32::MIN_POSITIVE);

		SelectionDashedBorderMetrics {
			stroke_width: SELECTION_DASHED_BORDER_WIDTH_PX * points_per_pixel,
			dash_length: SELECTION_DASHED_BORDER_DASH_LENGTH_PX * points_per_pixel,
			gap_length: SELECTION_DASHED_BORDER_GAP_LENGTH_PX * points_per_pixel,
		}
	}

	fn frozen_selection_resize_handle_outline_stroke(theme: HudTheme) -> Stroke {
		let _ = theme;
		let color = Color32::from_rgba_unmultiplied(229, 247, 255, 124);

		Stroke::new(FROZEN_SELECTION_RESIZE_HANDLE_STROKE_WIDTH_POINTS + 0.6, color)
	}

	fn frozen_selection_resize_handle_stroke(theme: HudTheme) -> Stroke {
		let _ = theme;

		Stroke::new(
			FROZEN_SELECTION_RESIZE_HANDLE_STROKE_WIDTH_POINTS,
			Color32::from_rgba_unmultiplied(167, 223, 255, 246),
		)
	}

	fn frozen_selection_resize_handle_center(handle: FrozenSelectionResizeHandleGeometry) -> Pos2 {
		handle.hit_rect.center()
	}

	fn frozen_selection_resize_handle_center_dot_color(theme: HudTheme) -> Color32 {
		let _ = theme;

		Color32::from_rgba_unmultiplied(167, 223, 255, 252)
	}

	pub(in crate::overlay) fn render_frozen_selection_resize_handles(
		painter: &Painter,
		capture_rect: RectPoints,
		theme: HudTheme,
	) -> bool {
		let outline_stroke = Self::frozen_selection_resize_handle_outline_stroke(theme);
		let stroke = Self::frozen_selection_resize_handle_stroke(theme);
		let center_dot_color = Self::frozen_selection_resize_handle_center_dot_color(theme);

		for handle in Self::frozen_selection_resize_handles(capture_rect) {
			let center = Self::frozen_selection_resize_handle_center(handle);

			painter.circle_stroke(
				center,
				FROZEN_SELECTION_RESIZE_HANDLE_OUTER_RADIUS_POINTS,
				outline_stroke,
			);
			painter.circle_stroke(
				center,
				FROZEN_SELECTION_RESIZE_HANDLE_OUTER_RADIUS_POINTS,
				stroke,
			);
			painter.circle_filled(
				center,
				FROZEN_SELECTION_RESIZE_HANDLE_CENTER_DOT_RADIUS_POINTS,
				center_dot_color,
			);
		}

		true
	}

	pub(in crate::overlay) fn selection_dashed_border_rect(
		screen_rect: Rect,
		focus_rect: Rect,
		border_outset: f32,
	) -> Option<Rect> {
		Self::selection_has_outside_region(screen_rect, focus_rect)
			.then_some(focus_rect.expand(border_outset))
	}

	pub(in crate::overlay) fn selection_dashed_border_outset(
		stroke_width: f32,
		pixels_per_point: f32,
	) -> f32 {
		let feathering = 1.0 / pixels_per_point.max(f32::MIN_POSITIVE);

		// Match epaint's outer stroke radius so the anti-aliased dashed keyline
		// stays fully in the scrim instead of bleeding into the capture rect.
		(stroke_width + feathering) * 0.5
	}

	pub(in crate::overlay) fn selection_has_outside_region(
		screen_rect: Rect,
		focus_rect: Rect,
	) -> bool {
		Self::frozen_selection_scrim_rects(screen_rect, focus_rect)
			.into_iter()
			.any(|rect| rect.width() > 0.0 && rect.height() > 0.0)
	}

	pub(in crate::overlay) fn selection_dashed_border_segments(
		rect: Rect,
		target_dash_length: f32,
		target_gap_length: f32,
	) -> Vec<[Pos2; 2]> {
		let perimeter = Self::selection_dashed_border_perimeter(rect);

		if perimeter <= 0.0 {
			return Vec::new();
		}

		let mut segments = Vec::new();

		for (dash_start, dash_end) in Self::selection_dashed_border_dash_ranges(
			perimeter,
			target_dash_length,
			target_gap_length,
		) {
			Self::append_selection_dashed_border_dash_segments(
				rect,
				dash_start,
				dash_end,
				&mut segments,
			);
		}

		segments
	}

	pub(in crate::overlay) fn selection_dashed_border_segments_with_corner_keepout(
		rect: Rect,
		target_dash_length: f32,
		target_gap_length: f32,
		corner_keepout: f32,
	) -> Vec<[Pos2; 2]> {
		if corner_keepout <= 0.0 {
			return Self::selection_dashed_border_segments(
				rect,
				target_dash_length,
				target_gap_length,
			);
		}

		let horizontal_ranges = Self::selection_dashed_border_edge_dash_ranges(
			rect.width(),
			corner_keepout,
			target_dash_length,
			target_gap_length,
		);
		let vertical_ranges = Self::selection_dashed_border_edge_dash_ranges(
			rect.height(),
			corner_keepout,
			target_dash_length,
			target_gap_length,
		);
		let mut segments = Vec::new();

		for (start, end) in &horizontal_ranges {
			segments.push([
				Pos2::new(rect.min.x + *start, rect.min.y),
				Pos2::new(rect.min.x + *end, rect.min.y),
			]);
		}
		for (start, end) in &vertical_ranges {
			segments.push([
				Pos2::new(rect.max.x, rect.min.y + *start),
				Pos2::new(rect.max.x, rect.min.y + *end),
			]);
		}
		for (start, end) in &horizontal_ranges {
			segments.push([
				Pos2::new(rect.min.x + *start, rect.max.y),
				Pos2::new(rect.min.x + *end, rect.max.y),
			]);
		}
		for (start, end) in &vertical_ranges {
			segments.push([
				Pos2::new(rect.min.x, rect.min.y + *start),
				Pos2::new(rect.min.x, rect.min.y + *end),
			]);
		}

		segments
	}

	pub(in crate::overlay) fn selection_dashed_border_cached_segments(
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
		rect: Rect,
		target_dash_length: f32,
		target_gap_length: f32,
		corner_keepout: f32,
	) -> &[[Pos2; 2]] {
		let key = SelectionDashedBorderCacheKey::new(
			rect,
			target_dash_length,
			target_gap_length,
			corner_keepout,
		);

		if selection_dashed_border_cache.key != Some(key) {
			selection_dashed_border_cache.segments.clear();
			selection_dashed_border_cache.segments.extend(
				Self::selection_dashed_border_segments_with_corner_keepout(
					rect,
					target_dash_length,
					target_gap_length,
					corner_keepout,
				),
			);

			selection_dashed_border_cache.key = Some(key);
		}

		selection_dashed_border_cache.segments.as_slice()
	}

	pub(in crate::overlay) fn selection_dashed_border_edge_dash_ranges(
		edge_length: f32,
		corner_keepout: f32,
		target_dash_length: f32,
		target_gap_length: f32,
	) -> Vec<(f32, f32)> {
		let usable_length = edge_length - corner_keepout * 2.0;

		if usable_length <= 0.0 {
			return Vec::new();
		}
		if usable_length <= target_dash_length {
			return vec![(corner_keepout, edge_length - corner_keepout)];
		}

		let dash_length = target_dash_length.min(usable_length);
		let cycle_span = (target_dash_length + target_gap_length).max(f32::MIN_POSITIVE);
		let dash_count =
			(((usable_length + target_gap_length) / cycle_span).floor() as usize).max(1);

		if dash_count == 1 {
			return vec![(corner_keepout, edge_length - corner_keepout)];
		}

		let occupied_length = dash_count as f32 * dash_length
			+ dash_count.saturating_sub(1) as f32 * target_gap_length;
		let gap_count = dash_count.saturating_sub(1);
		let gap_length = if gap_count == 0 {
			target_gap_length
		} else {
			target_gap_length + (usable_length - occupied_length).max(0.0) / gap_count as f32
		};

		(0..dash_count)
			.map(|index| {
				let start = corner_keepout + index as f32 * (dash_length + gap_length);

				(start, start + dash_length)
			})
			.collect()
	}

	pub(in crate::overlay) fn selection_dashed_border_dash_ranges(
		perimeter: f32,
		target_dash_length: f32,
		target_gap_length: f32,
	) -> Vec<(f32, f32)> {
		if perimeter <= 0.0 {
			return Vec::new();
		}

		let target_cycle = (target_dash_length + target_gap_length).max(f32::MIN_POSITIVE);
		let cycle_count = (perimeter / target_cycle).round().max(1.0) as usize;
		let cycle_span = perimeter / cycle_count as f32;
		let dash_length = target_dash_length.min(cycle_span);

		(0..cycle_count)
			.map(|index| {
				let dash_start = index as f32 * cycle_span;

				(dash_start, dash_start + dash_length)
			})
			.collect()
	}

	pub(in crate::overlay) fn append_selection_dashed_border_dash_segments(
		rect: Rect,
		dash_start: f32,
		dash_end: f32,
		segments: &mut Vec<[Pos2; 2]>,
	) {
		let mut segment_start = dash_start;

		for corner_distance in Self::selection_dashed_border_corner_distances(rect) {
			if segment_start >= dash_end {
				break;
			}
			if corner_distance <= segment_start || corner_distance >= dash_end {
				continue;
			}

			Self::push_selection_dashed_border_segment(
				rect,
				segment_start,
				corner_distance,
				segments,
			);

			segment_start = corner_distance;
		}

		if segment_start < dash_end {
			Self::push_selection_dashed_border_segment(rect, segment_start, dash_end, segments);
		}
	}

	pub(in crate::overlay) fn push_selection_dashed_border_segment(
		rect: Rect,
		start_distance: f32,
		end_distance: f32,
		segments: &mut Vec<[Pos2; 2]>,
	) {
		let start = Self::selection_dashed_border_point_at(rect, start_distance);
		let end = Self::selection_dashed_border_point_at(rect, end_distance);

		if start != end {
			segments.push([start, end]);
		}
	}

	pub(in crate::overlay) fn selection_dashed_border_point_at(rect: Rect, distance: f32) -> Pos2 {
		let width = rect.width();
		let height = rect.height();
		let perimeter = Self::selection_dashed_border_perimeter(rect);
		let distance = distance.rem_euclid(perimeter);

		if distance < width {
			return Pos2::new(rect.min.x + distance, rect.min.y);
		}
		if distance < width + height {
			return Pos2::new(rect.max.x, rect.min.y + (distance - width));
		}
		if distance < width * 2.0 + height {
			return Pos2::new(rect.max.x - (distance - width - height), rect.max.y);
		}

		Pos2::new(rect.min.x, rect.max.y - (distance - width * 2.0 - height))
	}

	pub(in crate::overlay) fn selection_dashed_border_corner_distances(rect: Rect) -> [f32; 4] {
		let width = rect.width();
		let height = rect.height();

		[width, width + height, width * 2.0 + height, Self::selection_dashed_border_perimeter(rect)]
	}

	pub(in crate::overlay) fn selection_dashed_border_perimeter(rect: Rect) -> f32 {
		if rect.width() <= 0.0 || rect.height() <= 0.0 {
			return 0.0;
		}

		(rect.width() + rect.height()) * 2.0
	}

	pub(in crate::overlay) fn render_selection_flow_ring(
		painter: &Painter,
		rect: Rect,
		ctx: &Context,
		theme: HudTheme,
		style: SelectionFlowStyle,
		selection_flow_stroke_width_px: f32,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
	) {
		if rect.width() < LIVE_DRAG_START_THRESHOLD_PX
			|| rect.height() < LIVE_DRAG_START_THRESHOLD_PX
		{
			return;
		}

		let corner_radius = Self::selection_flow_corner_radius(rect);
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);
		let time = ctx.input(|i| i.time) as f32;
		let sample_count = Self::selection_flow_sample_count(perimeter);
		let seam_offset = if rect.width() > corner_radius * 2.0 {
			(rect.width() - corner_radius * 2.0) * 0.5
		} else {
			0.0
		};
		let (samples, normals) = Self::selection_flow_cached_geometry(
			selection_flow_geometry_cache,
			rect,
			corner_radius,
			sample_count,
			seam_offset,
		);
		let base_alpha_scale = 1.0;
		let stroke_width = selection_flow_stroke_width_px.clamp(1.0, 8.0);

		if samples.is_empty() {
			return;
		}

		let flow_time = time * SELECTION_FLOW_SPEED;
		let phase = flow_time * 1.28 + 0.72;

		match style {
			SelectionFlowStyle::Band => Self::selection_flow_draw_layer(
				painter,
				samples,
				normals,
				stroke_width,
				base_alpha_scale * 0.52,
				phase,
				SELECTION_FLOW_CORE_FLOW_WIDTH,
				theme,
			),
		}
	}

	pub(in crate::overlay) fn selection_flow_corner_radius(rect: Rect) -> f32 {
		SELECTION_FLOW_CORNER_RADIUS_PX
			.min(rect.width() / 2.0 - 0.25)
			.min(rect.height() / 2.0 - 0.25)
			.max(0.0)
	}

	pub(in crate::overlay) fn selection_flow_palette(
		theme: HudTheme,
	) -> &'static [(u8, u8, u8); 3] {
		match theme {
			HudTheme::Dark => &SELECTION_FLOW_PALETTE,
			HudTheme::Light => &SELECTION_FLOW_LIGHT_PALETTE,
		}
	}

	pub(in crate::overlay) fn selection_flow_cached_geometry(
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		rect: Rect,
		corner_radius: f32,
		sample_count: usize,
		seam_offset: f32,
	) -> (&[(Pos2, f32)], &[Vec2]) {
		let key =
			SelectionFlowGeometryCacheKey::new(rect, corner_radius, seam_offset, sample_count);

		if selection_flow_geometry_cache.key == Some(key)
			&& !selection_flow_geometry_cache.samples.is_empty()
		{
			return (
				&selection_flow_geometry_cache.samples,
				&selection_flow_geometry_cache.normals,
			);
		}

		let samples =
			Self::selection_flow_path_samples(rect, corner_radius, sample_count, seam_offset);
		let normals = Self::selection_flow_compute_normals(&samples);

		selection_flow_geometry_cache.key = Some(key);
		selection_flow_geometry_cache.samples = samples;
		selection_flow_geometry_cache.normals = normals;

		(&selection_flow_geometry_cache.samples, &selection_flow_geometry_cache.normals)
	}

	pub(in crate::overlay) fn selection_flow_compute_normals(samples: &[(Pos2, f32)]) -> Vec<Vec2> {
		let n = samples.len();

		if n == 0 {
			return Vec::new();
		}

		let mut normals = Vec::with_capacity(n);
		let mut first_non_zero = None;

		for i in 0..n {
			let (current_point, _) = samples[i];
			let (prev_point, _) = samples[(i + n - 1) % n];
			let (next_point, _) = samples[(i + 1) % n];
			let prev_tangent = current_point - prev_point;
			let next_tangent = next_point - current_point;
			let mut normal = Vec2::ZERO;

			if prev_tangent.length_sq() > f32::EPSILON {
				let prev_len = prev_tangent.length();

				normal += Vec2::new(-prev_tangent.y / prev_len, prev_tangent.x / prev_len);
			}
			if next_tangent.length_sq() > f32::EPSILON {
				let next_len = next_tangent.length();

				normal += Vec2::new(-next_tangent.y / next_len, next_tangent.x / next_len);
			}
			if normal.length_sq() <= f32::EPSILON {
				if next_tangent.length_sq() > f32::EPSILON {
					let next_len = next_tangent.length();

					normal = Vec2::new(-next_tangent.y / next_len, next_tangent.x / next_len);
				} else if prev_tangent.length_sq() > f32::EPSILON {
					let prev_len = prev_tangent.length();

					normal = Vec2::new(-prev_tangent.y / prev_len, prev_tangent.x / prev_len);
				}
			}

			let normal = if normal.length_sq() > f32::EPSILON {
				let normalized = normal / normal.length();

				if first_non_zero.is_none() && normalized.length_sq() > f32::EPSILON {
					first_non_zero = Some(i);
				}

				normalized
			} else {
				Vec2::ZERO
			};

			normals.push(normal);
		}

		if let Some(first_idx) = first_non_zero {
			let mut previous = normals[first_idx];

			for normal in normals.iter_mut().skip(first_idx + 1) {
				if normal.length_sq() > f32::EPSILON && normal.dot(previous) < 0.0 {
					*normal = -*normal;
				}
				if normal.length_sq() > f32::EPSILON {
					previous = *normal;
				}
			}
			for normal in normals.iter_mut().take(first_idx).rev() {
				if normal.length_sq() > f32::EPSILON && normal.dot(previous) < 0.0 {
					*normal = -*normal;
				}
				if normal.length_sq() > f32::EPSILON {
					previous = *normal;
				}
			}

			if normals[first_idx].length_sq() > f32::EPSILON
				&& normals[(first_idx + n - 1) % n].length_sq() > f32::EPSILON
				&& normals[first_idx].dot(normals[(first_idx + n - 1) % n]) < 0.0
			{
				for normal in &mut normals {
					*normal = -*normal;
				}
			}
		}

		normals
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn selection_flow_draw_layer(
		painter: &Painter,
		samples: &[(Pos2, f32)],
		normals: &[Vec2],
		line_width: f32,
		alpha_scale: f32,
		phase: f32,
		flow_band_width: f32,
		theme: HudTheme,
	) {
		if samples.is_empty() || normals.is_empty() || samples.len() != normals.len() {
			return;
		}

		let half = (line_width * 0.5).max(0.1);
		let n = samples.len();
		let mut mesh = Mesh::default();

		for i in 0..n {
			let (current_point, t) = samples[i];
			let movement = Self::selection_flow_flow_band(t, phase, flow_band_width);
			let intensity = SELECTION_FLOW_FLOW_BOOST * movement;
			let color = Self::selection_flow_color(t + phase, theme, alpha_scale, intensity);
			let normal = normals[i] * half;

			mesh.colored_vertex(current_point + normal, color);
			mesh.colored_vertex(current_point - normal, color);
		}
		for i in 0..n {
			let i0 = (i * 2) as u32;
			let i1 = ((i * 2) + 1) as u32;
			let n0 = (((i + 1) % n) * 2) as u32;
			let n1 = (((i + 1) % n) * 2 + 1) as u32;

			mesh.add_triangle(i0, i1, n0);
			mesh.add_triangle(i1, n1, n0);
		}

		painter.add(Shape::Mesh(mesh.into()));
	}

	pub(in crate::overlay) fn selection_flow_flow_band(
		progress: f32,
		phase: f32,
		band_width: f32,
	) -> f32 {
		let width = band_width.clamp(0.001, 0.5);
		let distance = (progress - phase).rem_euclid(1.0);
		let distance = distance.min(1.0 - distance);
		let normalized = (distance / width).min(1.0);

		(1.0 - normalized).powf(2.0)
	}

	pub(in crate::overlay) fn selection_flow_sample_count(perimeter: f32) -> usize {
		if perimeter <= 0.0 || !perimeter.is_finite() {
			return SELECTION_FLOW_MIN_SEGMENTS;
		}

		let by_step = (perimeter / SELECTION_FLOW_SAMPLE_STEP_PX).ceil() as usize;

		by_step.clamp(SELECTION_FLOW_MIN_SEGMENTS, SELECTION_FLOW_MAX_SEGMENTS)
	}

	pub(in crate::overlay) fn selection_flow_path_samples(
		rect: Rect,
		corner_radius: f32,
		sample_count: usize,
		start_offset: f32,
	) -> Vec<(Pos2, f32)> {
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);

		if perimeter <= 0.0 {
			return Vec::new();
		}

		let start = (start_offset / perimeter).rem_euclid(1.0);

		(0..sample_count)
			.map(|index| {
				let t = (index as f32 + 0.5) / sample_count as f32;
				let progress = (t + start).rem_euclid(1.0);

				(
					Self::selection_flow_sample_at_distance(
						rect,
						corner_radius,
						perimeter * progress,
					),
					t,
				)
			})
			.collect()
	}

	pub(in crate::overlay) fn selection_flow_sample_at_distance(
		rect: Rect,
		corner_radius: f32,
		distance: f32,
	) -> Pos2 {
		if corner_radius <= f32::EPSILON {
			let perimeter = Self::selection_flow_perimeter(rect, 0.0);
			let keep = distance.rem_euclid(perimeter);
			let edge_top = rect.width();
			let edge_right = rect.height();

			if keep < edge_top {
				return Pos2::new(rect.min.x + keep, rect.min.y);
			}
			if keep < edge_top + edge_right {
				return Pos2::new(rect.max.x, rect.min.y + (keep - edge_top));
			}
			if keep < edge_top * 2.0 + edge_right {
				return Pos2::new(rect.max.x - (keep - edge_top - edge_right), rect.max.y);
			}

			return Pos2::new(rect.min.x, rect.max.y - (keep - edge_top * 2.0 - edge_right));
		}

		let x0 = rect.min.x;
		let x1 = rect.max.x;
		let y0 = rect.min.y;
		let y1 = rect.max.y;
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);
		let remain = distance.rem_euclid(perimeter);
		let edge_top_len = (rect.width() - corner_radius * 2.0).max(0.0);
		let edge_right_len = (rect.height() - corner_radius * 2.0).max(0.0);
		let corner_len = std::f32::consts::FRAC_PI_2 * corner_radius;

		if remain < edge_top_len {
			return Pos2::new(x0 + corner_radius + remain, y0);
		}

		let mut offset = remain - edge_top_len;

		if offset < corner_len {
			let angle = -std::f32::consts::FRAC_PI_2 + offset / corner_radius;

			return Pos2::new(
				x1 - corner_radius + corner_radius * angle.cos(),
				y0 + corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_right_len {
			return Pos2::new(x1, y0 + corner_radius + offset);
		}

		offset -= edge_right_len;

		if offset < corner_len {
			let angle = offset / corner_radius;

			return Pos2::new(
				x1 - corner_radius + corner_radius * angle.cos(),
				y1 - corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_top_len {
			return Pos2::new(x1 - corner_radius - offset, y1);
		}

		offset -= edge_top_len;

		if offset < corner_len {
			let angle = std::f32::consts::FRAC_PI_2 + offset / corner_radius;

			return Pos2::new(
				x0 + corner_radius + corner_radius * angle.cos(),
				y1 - corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_right_len {
			return Pos2::new(x0, y1 - corner_radius - offset);
		}

		offset -= edge_right_len;

		if offset < corner_len {
			let angle = std::f32::consts::PI + offset / corner_radius;

			return Pos2::new(
				x0 + corner_radius + corner_radius * angle.cos(),
				y0 + corner_radius + corner_radius * angle.sin(),
			);
		}

		Pos2::new(x0 + corner_radius, y0)
	}

	pub(in crate::overlay) fn selection_flow_perimeter(rect: Rect, corner_radius: f32) -> f32 {
		let edge_top_len = (rect.width() - corner_radius * 2.0).max(0.0);
		let edge_right_len = (rect.height() - corner_radius * 2.0).max(0.0);
		let corner_len = std::f32::consts::FRAC_PI_2 * corner_radius;

		2.0 * (edge_top_len + edge_right_len) + 4.0 * corner_len
	}

	pub(in crate::overlay) fn selection_flow_color(
		progress: f32,
		theme: HudTheme,
		alpha_scale: f32,
		intensity: f32,
	) -> Color32 {
		let palette = Self::selection_flow_palette(theme);
		let normalized = progress.rem_euclid(1.0);
		let band_position = normalized * palette.len() as f32;
		let band = band_position.floor() as usize % palette.len();
		let local = band_position - band as f32;
		let (r0, g0, b0) = palette[band];
		let (r1, g1, b1) = palette[(band + 1) % palette.len()];
		let blend = |a: u8, b: u8, ratio: f32| -> u8 {
			(a as f32 + (b as f32 - a as f32) * ratio).clamp(0.0, 255.0).round() as u8
		};
		let theme_alpha = 1.0;
		let alpha = (255.0 * alpha_scale * intensity * theme_alpha).clamp(0.0, 255.0);

		Color32::from_rgba_unmultiplied(
			blend(r0, r1, local),
			blend(g0, g1, local),
			blend(b0, b1, local),
			alpha as u8,
		)
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
			toolbar_size,
			toolbar_placement,
		) else {
			return;
		};

		#[cfg(any(not(target_os = "macos"), test))]
		{
			if !overlay::advance_frozen_toolbar_readiness_sample_state(toolbar_state, screen_rect) {
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
		#[cfg(target_os = "macos")]
		const TOOLS_SCROLL_MODE: [FrozenToolbarTool; 3] =
			[FrozenToolbarTool::Ocr, FrozenToolbarTool::Copy, FrozenToolbarTool::Save];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_SCROLL_MODE: [FrozenToolbarTool; 2] =
			[FrozenToolbarTool::Copy, FrozenToolbarTool::Save];
		#[cfg(target_os = "macos")]
		const TOOLS_WITH_SCROLL_AND_AUTO_CENTER: [FrozenToolbarTool; 11] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::AutoCenter,
			FrozenToolbarTool::Scroll,
			FrozenToolbarTool::Ocr,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_WITH_SCROLL_AND_AUTO_CENTER: [FrozenToolbarTool; 10] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::AutoCenter,
			FrozenToolbarTool::Scroll,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(target_os = "macos")]
		const TOOLS_WITH_AUTO_CENTER: [FrozenToolbarTool; 10] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::AutoCenter,
			FrozenToolbarTool::Ocr,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_WITH_AUTO_CENTER: [FrozenToolbarTool; 9] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::AutoCenter,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(target_os = "macos")]
		const TOOLS_WITH_SCROLL: [FrozenToolbarTool; 10] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Scroll,
			FrozenToolbarTool::Ocr,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_WITH_SCROLL: [FrozenToolbarTool; 9] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Scroll,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(target_os = "macos")]
		const TOOLS_WITHOUT_SCROLL: [FrozenToolbarTool; 9] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Ocr,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		#[cfg(not(target_os = "macos"))]
		const TOOLS_WITHOUT_SCROLL: [FrozenToolbarTool; 8] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];

		if toolbar_state.scroll_capture_active {
			&TOOLS_SCROLL_MODE
		} else if toolbar_state.auto_center_available && toolbar_state.scroll_capture_available {
			&TOOLS_WITH_SCROLL_AND_AUTO_CENTER
		} else if toolbar_state.auto_center_available {
			&TOOLS_WITH_AUTO_CENTER
		} else if toolbar_state.scroll_capture_available {
			&TOOLS_WITH_SCROLL
		} else {
			&TOOLS_WITHOUT_SCROLL
		}
	}

	pub(in crate::overlay) fn frozen_toolbar_size(toolbar_state: &FrozenToolbarState) -> Vec2 {
		let tool_count = Self::frozen_toolbar_tools(toolbar_state).len() as f32;
		let spacing_count = (tool_count - 1.0).max(0.0);
		let width = tool_count * FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
			+ spacing_count * FROZEN_TOOLBAR_ITEM_SPACING_POINTS
			+ 2.0 * HUD_PILL_INNER_MARGIN_X_POINTS
			+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
		let mut height = toolbar_state.pill_height_points.unwrap_or(TOOLBAR_EXPANDED_HEIGHT_PX);

		if Self::frozen_annotation_style_toolbar_visible(toolbar_state) {
			height += FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS
				+ FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS;
		}

		Vec2::new(width, height)
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn resolve_frozen_toolbar_birth(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		toolbar_state: &mut FrozenToolbarState,
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Option<Pos2> {
		if let Some(pos) = toolbar_state.floating_position {
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

		let needs_new_sample = overlay::frozen_toolbar_needs_new_sample(
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

		let default_pos = Self::frozen_toolbar_default_pos(
			screen_rect,
			capture_rect,
			toolbar_size,
			toolbar_placement,
		);

		tracing::debug!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			toolbar_size_points = ?toolbar_size,
			default_pos = ?default_pos,
			"Frozen toolbar birth resolved."
		);

		toolbar_state.default_slot_position = Some(default_pos);
		toolbar_state.floating_position = Some(default_pos);

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

	pub(in crate::overlay) fn frozen_toolbar_default_pos(
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Pos2 {
		let y = match toolbar_placement {
			ToolbarPlacement::Bottom => {
				let below_y = capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX;
				let within_screen =
					below_y + toolbar_size.y + TOOLBAR_SCREEN_MARGIN_PX <= screen_rect.max.y;

				if within_screen {
					below_y
				} else {
					capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y
				}
			},
			ToolbarPlacement::Top => {
				let above_y = capture_rect.min.y - TOOLBAR_CAPTURE_GAP_PX - toolbar_size.y;
				let within_screen = above_y >= screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;

				if within_screen { above_y } else { capture_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX }
			},
		};
		let min_y = screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;
		let max_y = (screen_rect.max.y - toolbar_size.y - TOOLBAR_SCREEN_MARGIN_PX).max(min_y);
		let x = Self::frozen_toolbar_default_x(screen_rect, toolbar_size, capture_rect.center().x);
		let y = y.max(min_y).min(max_y);

		Pos2::new(x, y)
	}

	pub(in crate::overlay) fn frozen_toolbar_default_x(
		screen_rect: Rect,
		toolbar_size: Vec2,
		anchor_center_x: f32,
	) -> f32 {
		let min_x = screen_rect.min.x + TOOLBAR_SCREEN_MARGIN_PX;
		let max_x = (screen_rect.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(min_x);

		(anchor_center_x - toolbar_size.x / 2.0).clamp(min_x, max_x)
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

		Area::new(Id::new(format!("frozen-toolbar-{}", monitor.id)))
			.order(Order::Foreground)
			.fixed_pos(toolbar_pos)
			.show(ctx, |ui| {
				let (rect, response) = ui.allocate_exact_size(
					toolbar_size,
					if cfg!(target_os = "macos") {
						Sense::hover()
					} else {
						Sense::click_and_drag()
					},
				);
				#[cfg(target_os = "macos")]
				let _ = &response;
				let corner_radius = frozen_toolbar_corner_radius_u8(rect.height());
				let body_fill = Self::tinted_hud_body_fill(
					theme,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					hud_tint_hue,
				);
				let toolbar_frame =
					Self::hud_pill_frame(theme, hud_opaque, hud_opacity, body_fill, false);
				toolbar_state.annotation_size_control_hovered = false;
				#[cfg(not(target_os = "macos"))]
				Self::update_frozen_toolbar_drag_state(
					toolbar_state,
					response.drag_started(),
					toolbar_pos,
					screen_rect,
					toolbar_size,
					cursor,
					left_button_down,
				);

				// Draw the capsule ourselves at the exact allocated rect. This keeps the visible pill
				// and the blur rect perfectly aligned (no shrink-to-content surprises on first frame).
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
				let inner_stroke = Stroke::new(1.0, inner_stroke_color);
				let inner_rect = rect.shrink(1.0);

				ui.painter().rect_stroke(
					inner_rect,
					CornerRadius::same(corner_radius.saturating_sub(1)),
					inner_stroke,
					StrokeKind::Inside,
				);

				let inner_rect = rect.shrink2(egui::vec2(
					HUD_PILL_INNER_MARGIN_X_POINTS,
					TOOLBAR_PILL_INNER_MARGIN_Y_POINTS,
				));

				Self::render_frozen_toolbar_body(ui, inner_rect, toolbar_state, theme);

				*hud_pill_out =
					Some(HudPillGeometry { rect, radius_points: f32::from(corner_radius) });
			});
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

	fn render_frozen_toolbar_body(
		ui: &mut Ui,
		inner_rect: Rect,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		let _ = ui.scope_builder(UiBuilder::new().max_rect(inner_rect), |ui| {
			ui.with_layout(Layout::top_down(Align::Center), |ui| {
				Self::render_frozen_toolbar_primary_row(
					ui,
					inner_rect.width(),
					toolbar_state,
					theme,
				);

				if Self::frozen_annotation_style_toolbar_visible(toolbar_state) {
					Self::render_frozen_annotation_toolbar_section(
						ui,
						inner_rect,
						toolbar_state,
						theme,
					);
				}
			});
		});
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

	fn paint_frozen_annotation_toolbar_spacing(ui: &mut Ui, inner_rect: Rect, theme: HudTheme) {
		ui.add_space(FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS * 0.5);

		Self::paint_frozen_annotation_toolbar_divider(ui, inner_rect, theme);

		ui.add_space(FROZEN_ANNOTATION_TOOLBAR_SECTION_GAP_POINTS * 0.5);
	}

	fn render_frozen_annotation_toolbar_section(
		ui: &mut Ui,
		inner_rect: Rect,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		Self::paint_frozen_annotation_toolbar_spacing(ui, inner_rect, theme);

		let _ = ui.allocate_ui_with_layout(
			Vec2::new(inner_rect.width(), FROZEN_ANNOTATION_TOOLBAR_SECTION_HEIGHT_POINTS),
			Layout::left_to_right(Align::Center),
			|ui| Self::render_frozen_annotation_toolbar_controls(ui, toolbar_state, theme),
		);
	}

	fn paint_frozen_annotation_toolbar_divider(ui: &Ui, inner_rect: Rect, theme: HudTheme) {
		let divider_color = match theme {
			HudTheme::Dark => {
				Color32::from_white_alpha(FROZEN_ANNOTATION_TOOLBAR_SECTION_DIVIDER_ALPHA_DARK)
			},
			HudTheme::Light => {
				Color32::from_black_alpha(FROZEN_ANNOTATION_TOOLBAR_SECTION_DIVIDER_ALPHA_LIGHT)
			},
		};
		let divider_y = ui.cursor().min.y;

		ui.painter().line_segment(
			[Pos2::new(inner_rect.left(), divider_y), Pos2::new(inner_rect.right(), divider_y)],
			Stroke::new(1.0, divider_color),
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

	fn frozen_annotation_style_toolbar_visible(toolbar_state: &FrozenToolbarState) -> bool {
		FrozenAnnotationStyleToolbarKind::from_toolbar_state(toolbar_state).is_some()
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

			for color in FrozenTextColor::ALL {
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
		let size_response = size_response.on_hover_text(style_kind.size_hover_text());
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
		let minus_response = ui
			.interact(
				minus_rect,
				ui.id().with(("annotation-size-decrease", style_kind)),
				Sense::click(),
			)
			.on_hover_text(style_kind.decrease_hover_text());
		let plus_response = ui
			.interact(
				plus_rect,
				ui.id().with(("annotation-size-increase", style_kind)),
				Sense::click(),
			)
			.on_hover_text(style_kind.increase_hover_text());
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
		Self::paint_frozen_annotation_size_step_button(ui, theme, &minus_response, regular::MINUS);
		Self::paint_frozen_annotation_size_step_button(ui, theme, &plus_response, regular::PLUS);
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
		minus_response: &egui::Response,
		plus_response: &egui::Response,
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
		response: &egui::Response,
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
		minus_response: &egui::Response,
		plus_response: &egui::Response,
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
		color: FrozenTextColor,
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

		response.on_hover_text("Annotation color").clicked()
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
