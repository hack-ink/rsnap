mod size_badge;
mod toolbar;

use std::f32::consts::FRAC_PI_2;
use std::f32::consts::PI;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use egui::Context;
use egui::FontDefinitions;
use egui::Galley;
use egui::RawInput;
use egui::text::CCursor;

use crate::overlay::rendering::{
	FrozenSelectionResizeHandleGeometry, SelectionDashedBorderCache, SelectionDashedBorderCacheKey,
	SelectionDashedBorderMetrics, SelectionFlowGeometryCache, SelectionFlowGeometryCacheKey,
	WindowRenderer,
};
use crate::overlay::{
	Color32, FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS, FROZEN_SELECTION_DASHED_BORDER_WIDTH_PX,
	FROZEN_SELECTION_RESIZE_HANDLE_CENTER_DOT_RADIUS_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_CORNER_KEEPOUT_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_HIT_OFFSET_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_HIT_SIZE_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_OUTER_RADIUS_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_STROKE_WIDTH_POINTS, FROZEN_SELECTION_SCRIM_ALPHA_DARK,
	FROZEN_SELECTION_SCRIM_ALPHA_LIGHT, FROZEN_TEXT_CARET_BLINK_PERIOD_SECS,
	FROZEN_TEXT_PREVIEW_PLACEHOLDER, FontId, FrozenAnnotationColor, FrozenArrowAnnotation,
	FrozenBrushState, FrozenCaptureSource, FrozenCommittedOverlay, FrozenEditKind,
	FrozenSelectionCorner, FrozenSpotlightAnnotation, FrozenTextAnnotation, FrozenTextEditState,
	FrozenTextStyle, HudTheme, Id, LIVE_DRAG_SELECTION_SCRIM_ALPHA_DARK,
	LIVE_DRAG_SELECTION_SCRIM_ALPHA_LIGHT, LIVE_DRAG_START_THRESHOLD_PX, LayerId, Mesh,
	MonitorRect, Order, OverlayMode, OverlaySession, OverlayState, Painter, Pos2, Rect, RectPoints,
	SELECTION_DASHED_BORDER_ALPHA, SELECTION_DASHED_BORDER_DASH_LENGTH_PX,
	SELECTION_DASHED_BORDER_GAP_LENGTH_PX, SELECTION_DASHED_BORDER_WIDTH_PX,
	SELECTION_FLOW_CORE_FLOW_WIDTH, SELECTION_FLOW_CORNER_RADIUS_PX, SELECTION_FLOW_FLOW_BOOST,
	SELECTION_FLOW_LIGHT_PALETTE, SELECTION_FLOW_MAX_SEGMENTS, SELECTION_FLOW_MIN_SEGMENTS,
	SELECTION_FLOW_PALETTE, SELECTION_FLOW_SAMPLE_STEP_PX, SELECTION_FLOW_SPEED,
	SelectionFlowStyle, Shape, Stroke, Vec2,
};

const FROZEN_TEXT_INTERACTION_PADDING_X_POINTS: f32 = 8.0;
const FROZEN_TEXT_INTERACTION_PADDING_Y_POINTS: f32 = 6.0;
#[derive(Clone, Copy)]
pub(in crate::overlay) struct SelectionScrimStyle {
	pub(in crate::overlay) scrim_fill: Color32,
	pub(in crate::overlay) stroke_width_override: Option<f32>,
	pub(in crate::overlay) exclude_resize_handle_corners: bool,
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

		if !matches!(state.mode, OverlayMode::Live | OverlayMode::Frozen) {
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
	pub(in crate::overlay) fn render_pending_frozen_display_handoff_affordance(
		ctx: &Context,
		painter: &Painter,
		state: &OverlayState,
		monitor: MonitorRect,
		pending_handoff_monitor: Option<MonitorRect>,
		screen_rect: Rect,
		theme: HudTheme,
		selection_flow_enabled: bool,
		selection_flow_stroke_width_px: f32,
		frozen_capture_source: FrozenCaptureSource,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		if !matches!(state.mode, OverlayMode::Live | OverlayMode::Frozen) {
			return false;
		}
		if pending_handoff_monitor != Some(monitor) {
			return false;
		}

		let Some(capture_rect) = state.frozen_capture_rect else {
			return false;
		};
		let focus_rect = Self::selection_focus_rect(capture_rect, screen_rect);

		if focus_rect.width() <= 0.0 || focus_rect.height() <= 0.0 {
			return false;
		}

		match frozen_capture_source {
			FrozenCaptureSource::None => false,
			FrozenCaptureSource::DragRegion => Self::render_live_drag_selection_affordance(
				painter,
				focus_rect,
				screen_rect,
				theme,
				selection_dashed_border_cache,
			),
			FrozenCaptureSource::Window | FrozenCaptureSource::FullscreenFallback => {
				let mut rendered =
					Self::render_live_drag_selection_scrim(painter, focus_rect, screen_rect, theme);

				if selection_flow_enabled
					&& focus_rect.width() >= LIVE_DRAG_START_THRESHOLD_PX
					&& focus_rect.height() >= LIVE_DRAG_START_THRESHOLD_PX
				{
					Self::render_selection_flow_ring(
						painter,
						focus_rect,
						ctx,
						theme,
						SelectionFlowStyle::Band,
						selection_flow_stroke_width_px,
						selection_flow_geometry_cache,
					);

					rendered = true;
				}

				rendered
			},
		}
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
		frozen_arrow_annotations: &[FrozenArrowAnnotation],
		frozen_arrow_preview: Option<&FrozenArrowAnnotation>,
		frozen_spotlight_annotations: &[FrozenSpotlightAnnotation],
		frozen_spotlight_preview_rect: Option<RectPoints>,
		frozen_text_annotations: &[FrozenTextAnnotation],
		frozen_text_edit: Option<&FrozenTextEditState>,
		frozen_text_style: FrozenTextStyle,
		_frozen_capture_is_fullscreen_fallback: bool,
		_selection_flow_enabled: bool,
		_selection_flow_stroke_width_px: f32,
		_selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let Some(capture_rect_points) = state.frozen_capture_rect else {
			return false;
		};
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

		has_affordance |= Self::render_frozen_spotlight_annotations(
			&brush_painter,
			capture_rect_points,
			screen_rect,
			frozen_spotlight_annotations,
			frozen_spotlight_preview_rect,
			theme,
			selection_dashed_border_cache,
		);
		has_affordance |= Self::render_frozen_committed_overlay_annotations(
			&brush_painter,
			frozen_edit_history,
			frozen_brush_state,
			frozen_arrow_annotations,
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
		if let Some(arrow_preview) = frozen_arrow_preview {
			has_affordance |= Self::paint_frozen_arrow(&brush_painter, arrow_preview);
		}

		has_affordance
	}

	fn render_frozen_committed_overlay_annotations(
		brush_painter: &Painter,
		frozen_edit_history: &[FrozenEditKind],
		frozen_brush_state: Option<&FrozenBrushState>,
		frozen_arrow_annotations: &[FrozenArrowAnnotation],
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
			frozen_arrow_annotations,
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
				FrozenCommittedOverlay::Arrow(annotation) => {
					drew |= Self::paint_frozen_arrow(brush_painter, annotation);
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

	fn render_frozen_spotlight_annotations(
		painter: &Painter,
		capture_rect: RectPoints,
		screen_rect: Rect,
		annotations: &[FrozenSpotlightAnnotation],
		preview_rect: Option<RectPoints>,
		theme: HudTheme,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let spotlight_rects = OverlaySession::clipped_frozen_spotlight_rects(
			capture_rect,
			annotations.iter().map(|annotation| annotation.rect).chain(preview_rect),
		);

		if spotlight_rects.is_empty() {
			return false;
		}

		let fill = Color32::from_black_alpha(OverlaySession::frozen_spotlight_scrim_alpha());

		for scrim_rect in
			OverlaySession::frozen_spotlight_scrim_rects(capture_rect, &spotlight_rects)
		{
			let scrim_rect = Self::selection_focus_rect(scrim_rect, screen_rect);

			if scrim_rect.width() > 0.0 && scrim_rect.height() > 0.0 {
				painter.rect_filled(scrim_rect, 0.0, fill);
			}
		}

		let Some(preview_rect) = preview_rect else {
			return true;
		};
		let Some(preview_rect) =
			OverlaySession::clipped_frozen_spotlight_rects(capture_rect, [preview_rect])
				.into_iter()
				.next()
		else {
			return true;
		};

		Self::render_selection_dashed_border(
			painter,
			Self::selection_focus_rect(preview_rect, screen_rect),
			screen_rect,
			theme,
			Some(2.1),
			false,
			selection_dashed_border_cache,
		)
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
			FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
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

	fn paint_frozen_arrow(painter: &Painter, annotation: &FrozenArrowAnnotation) -> bool {
		let Some(geometry) = OverlaySession::frozen_arrow_geometry(annotation) else {
			return false;
		};
		let stroke_color = annotation.style.color.swatch_fill();
		let stroke_width =
			OverlaySession::frozen_arrow_stroke_width_points(annotation.style.stroke_width_points);
		let outline_width =
			OverlaySession::frozen_arrow_outline_width_points(annotation.style.stroke_width_points);
		let outline_stroke_width = OverlaySession::frozen_arrow_outline_stroke_width_points(
			annotation.style.stroke_width_points,
		);
		let outline_color = Color32::from_rgba_unmultiplied(255, 255, 255, 208);
		let (outline_tip, outline_left, outline_right) =
			OverlaySession::frozen_arrow_expanded_triangle(
				geometry.tip,
				geometry.head_left,
				geometry.head_right,
				outline_width,
			);

		painter.line_segment(
			[annotation.start, geometry.shaft_end],
			Stroke::new(outline_stroke_width, outline_color),
		);
		painter.circle_filled(annotation.start, stroke_width * 0.5 + outline_width, outline_color);
		painter.add(Shape::convex_polygon(
			vec![outline_tip, outline_left, outline_right],
			outline_color,
			Stroke::NONE,
		));
		painter.line_segment(
			[annotation.start, geometry.shaft_end],
			Stroke::new(stroke_width, stroke_color),
		);
		painter.circle_filled(annotation.start, stroke_width * 0.5, stroke_color);
		painter.add(Shape::convex_polygon(
			vec![geometry.tip, geometry.head_left, geometry.head_right],
			stroke_color,
			Stroke::NONE,
		));

		true
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
		color: FrozenAnnotationColor,
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
		(time_secs.rem_euclid(FROZEN_TEXT_CARET_BLINK_PERIOD_SECS))
			< FROZEN_TEXT_CARET_BLINK_PERIOD_SECS * 0.5
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
		let corner_len = FRAC_PI_2 * corner_radius;

		if remain < edge_top_len {
			return Pos2::new(x0 + corner_radius + remain, y0);
		}

		let mut offset = remain - edge_top_len;

		if offset < corner_len {
			let angle = -FRAC_PI_2 + offset / corner_radius;

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
			let angle = FRAC_PI_2 + offset / corner_radius;

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
			let angle = PI + offset / corner_radius;

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
		let corner_len = FRAC_PI_2 * corner_radius;

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
}
