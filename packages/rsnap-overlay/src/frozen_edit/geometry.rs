use crate::frozen_edit::{FrozenOverlayEditPoint, FrozenOverlayEditRect, FrozenOverlayEditText};
use crate::text_rendering::{self, TextBounds};

pub(super) const PEN_SAMPLE_MIN_DISTANCE_POINTS: f64 = 1.5;
pub(super) const ARROW_MIN_DISTANCE_POINTS: f64 = 6.0;
pub(super) const RECT_MIN_SIZE_POINTS: f64 = 6.0;

const TEXT_HIT_PADDING_POINTS: f64 = 4.0;

pub(super) fn normalized_rect(
	anchor: FrozenOverlayEditPoint,
	current: FrozenOverlayEditPoint,
) -> FrozenOverlayEditRect {
	FrozenOverlayEditRect::new(
		anchor.x.min(current.x),
		anchor.y.min(current.y),
		(current.x - anchor.x).abs(),
		(current.y - anchor.y).abs(),
	)
}

pub(super) fn moved_rect(
	rect: FrozenOverlayEditRect,
	drag_offset: FrozenOverlayEditPoint,
	point: FrozenOverlayEditPoint,
	selection: FrozenOverlayEditRect,
) -> FrozenOverlayEditRect {
	let max_min_x = selection.x.max(selection.max_x() - rect.width);
	let max_min_y = selection.y.max(selection.max_y() - rect.height);

	FrozenOverlayEditRect::new(
		(point.x - drag_offset.x).clamp(selection.x, max_min_x),
		(point.y - drag_offset.y).clamp(selection.y, max_min_y),
		rect.width,
		rect.height,
	)
}

pub(super) fn moved_text_annotation(
	annotation: FrozenOverlayEditText,
	drag_offset: FrozenOverlayEditPoint,
	point: FrozenOverlayEditPoint,
	selection: FrozenOverlayEditRect,
) -> FrozenOverlayEditText {
	let bounds = text_bounds(&annotation);
	let max_anchor_x = selection.x.max(selection.max_x() - bounds.width);
	let max_anchor_y = selection.y.max(selection.max_y() - bounds.height);
	let anchor = FrozenOverlayEditPoint::new(
		(point.x - drag_offset.x).clamp(selection.x, max_anchor_x),
		(point.y - drag_offset.y).clamp(selection.y, max_anchor_y),
	);

	FrozenOverlayEditText { anchor, ..annotation }
}

pub(super) fn text_hit_bounds(annotation: &FrozenOverlayEditText) -> FrozenOverlayEditRect {
	text_bounds(annotation).inset(-TEXT_HIT_PADDING_POINTS, -TEXT_HIT_PADDING_POINTS)
}

fn text_bounds(annotation: &FrozenOverlayEditText) -> FrozenOverlayEditRect {
	let font_size = annotation.style.font_size_points.max(1.0) as f32;
	let bounds =
		text_rendering::measure_text_bounds(&annotation.text, font_size).unwrap_or_else(|| {
			let width = annotation.text.chars().count().max(1) as f32 * font_size * 0.6;

			TextBounds { width, height: font_size * 1.2 }
		});

	FrozenOverlayEditRect::new(
		annotation.anchor.x,
		annotation.anchor.y,
		f64::from(bounds.width.ceil().max(1.0)),
		f64::from(bounds.height.ceil().max(1.0)),
	)
}
