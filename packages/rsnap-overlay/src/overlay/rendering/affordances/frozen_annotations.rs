use std::sync::{Arc, OnceLock};
use std::time::Instant;

use egui::text::CCursor;
use egui::{Context, FontDefinitions, FontId, Galley, RawInput};

use crate::overlay::frozen_brush_runtime::FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS;
use crate::overlay::rendering;
use crate::overlay::rendering::FROZEN_TEXT_CARET_BLINK_PERIOD_SECS;
use crate::overlay::rendering::{SelectionDashedBorderCache, WindowRenderer};
use crate::overlay::{
	Color32, FrozenAnnotationColor, FrozenArrowAnnotation, FrozenBrushState,
	FrozenCommittedOverlay, FrozenEditKind, FrozenSpotlightAnnotation, FrozenTextAnnotation,
	FrozenTextEditState, FrozenTextStyle, HudTheme, Id, LayerId, Order, OverlaySession, Painter,
	Pos2, Rect, RectPoints, Shape, Stroke, Vec2,
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
				rendering::configure_egui_fonts(&mut fonts);

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

	pub(super) fn render_frozen_committed_overlay_annotations(
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

	pub(super) fn render_frozen_spotlight_annotations(
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

	pub(super) fn render_active_frozen_brush_stroke(
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

	pub(super) fn paint_frozen_arrow(
		painter: &Painter,
		annotation: &FrozenArrowAnnotation,
	) -> bool {
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

	pub(super) fn render_frozen_text_annotations(
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
}
