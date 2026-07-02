use crate::overlay::rendering::{
	SelectionDashedBorderCache, SelectionDashedBorderCacheKey, SelectionDashedBorderMetrics,
	WindowRenderer,
};
use crate::overlay::{
	Color32, FROZEN_SELECTION_DASHED_BORDER_WIDTH_PX,
	FROZEN_SELECTION_RESIZE_HANDLE_CORNER_KEEPOUT_POINTS, FROZEN_SELECTION_SCRIM_ALPHA_DARK,
	FROZEN_SELECTION_SCRIM_ALPHA_LIGHT, HudTheme, LIVE_DRAG_SELECTION_SCRIM_ALPHA_DARK,
	LIVE_DRAG_SELECTION_SCRIM_ALPHA_LIGHT, Painter, Pos2, Rect, SELECTION_DASHED_BORDER_ALPHA,
	SELECTION_DASHED_BORDER_DASH_LENGTH_PX, SELECTION_DASHED_BORDER_GAP_LENGTH_PX,
	SELECTION_DASHED_BORDER_WIDTH_PX, Shape, Stroke,
};

#[derive(Clone, Copy)]
pub(in crate::overlay) struct SelectionScrimStyle {
	pub(in crate::overlay) scrim_fill: Color32,
	pub(in crate::overlay) stroke_width_override: Option<f32>,
	pub(in crate::overlay) exclude_resize_handle_corners: bool,
}

impl WindowRenderer {
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
}
