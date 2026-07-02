pub(super) mod size_badge;

mod selection_flow;
mod selection_scrim;
mod toolbar;

use std::sync::{Arc, OnceLock};
use std::time::Instant;

use egui::Context;
use egui::FontDefinitions;
use egui::Galley;
use egui::RawInput;
use egui::text::CCursor;

use crate::overlay::frozen_brush_runtime::FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS;
use crate::overlay::rendering::FROZEN_TEXT_CARET_BLINK_PERIOD_SECS;
use crate::overlay::rendering::{
	FrozenSelectionResizeHandleGeometry, SelectionDashedBorderCache, SelectionFlowGeometryCache,
	WindowRenderer,
};
use crate::overlay::{
	Color32, FROZEN_SELECTION_RESIZE_HANDLE_CENTER_DOT_RADIUS_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_HIT_OFFSET_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_HIT_SIZE_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_OUTER_RADIUS_POINTS,
	FROZEN_SELECTION_RESIZE_HANDLE_STROKE_WIDTH_POINTS, FontId, FrozenAnnotationColor,
	FrozenArrowAnnotation, FrozenBrushState, FrozenCaptureSource, FrozenCommittedOverlay,
	FrozenEditKind, FrozenSelectionCorner, FrozenSpotlightAnnotation, FrozenTextAnnotation,
	FrozenTextEditState, FrozenTextStyle, HudTheme, Id, LIVE_DRAG_START_THRESHOLD_PX, LayerId,
	MonitorRect, Order, OverlayMode, OverlaySession, OverlayState, Painter, Pos2, Rect, RectPoints,
	SelectionFlowStyle, Shape, Stroke, Vec2,
};

const FROZEN_TEXT_PREVIEW_PLACEHOLDER: &str = "Type";
const FROZEN_TEXT_INTERACTION_PADDING_X_POINTS: f32 = 8.0;
const FROZEN_TEXT_INTERACTION_PADDING_Y_POINTS: f32 = 6.0;

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
}
