//! Scroll-capture minimap layout planning owned by the Rust product core.

use crate::DisplayPointRect;

/// Inputs used to resolve a scroll-capture minimap layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollMinimapInput {
	/// Frozen selection rect in the host view coordinate space.
	pub selection: DisplayPointRect,
	/// Stitched export width in pixels.
	pub export_width: f64,
	/// Stitched export height in pixels.
	pub export_height: f64,
	/// Host view bounds.
	pub bounds: DisplayPointRect,
	/// Preferred minimap width.
	pub preferred_width: f64,
	/// Minimum useful minimap width.
	pub minimum_width: f64,
	/// Gap between the frozen selection and the minimap.
	pub gap: f64,
	/// Outer margin inside the host view bounds.
	pub margin: f64,
	/// Inset applied to the preview image inside the minimap frame.
	pub image_inset: f64,
	/// Current viewport top in stitched export pixels.
	pub viewport_top_pixels: f64,
	/// Current viewport height in stitched export pixels.
	pub viewport_height_pixels: f64,
}

/// Planned scroll-capture minimap rectangles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollMinimapPlan {
	/// Outer minimap frame.
	pub frame: DisplayPointRect,
	/// Preview image frame inside `frame`.
	pub image_frame: DisplayPointRect,
	/// Viewport marker inside `image_frame`, when visible.
	pub viewport_frame: Option<DisplayPointRect>,
}

/// Resolves the scroll-capture minimap frame, image frame, and viewport marker.
#[must_use]
pub fn scroll_minimap_plan(input: ScrollMinimapInput) -> Option<ScrollMinimapPlan> {
	if !minimap_input_is_valid(input) {
		return None;
	}

	let right_space = max_x(input.bounds) - max_x(input.selection) - input.gap - input.margin;
	let left_space = input.selection.x - input.bounds.x - input.gap - input.margin;
	let (use_right, side_space) = if right_space >= input.minimum_width {
		(true, right_space)
	} else if left_space >= input.minimum_width {
		(false, left_space)
	} else {
		(right_space >= left_space, right_space.max(left_space))
	};

	let max_height = input.bounds.height - input.margin * 2.0;
	let aspect_height_per_width = input.export_height / input.export_width;
	let height_limited_width = max_height / aspect_height_per_width.max(f64::MIN_POSITIVE);
	let width = input.preferred_width.min(side_space).min(height_limited_width);
	if width < input.minimum_width.min(input.preferred_width) * 0.55 {
		return None;
	}

	let height = width * aspect_height_per_width;
	let max_y = input.margin.max(max_y(input.bounds) - input.margin - height);
	let y = (mid_y(input.selection) - height / 2.0).clamp(input.margin, max_y);
	let x = if use_right {
		max_x(input.selection) + input.gap
	} else {
		input.selection.x - input.gap - width
	};
	let frame = DisplayPointRect::new(x, y, width, height);
	let image_frame = inset_rect(frame, input.image_inset);
	if !rect_is_valid(image_frame) {
		return None;
	}
	let viewport_frame = scroll_minimap_viewport_frame(
		image_frame,
		input.export_height,
		input.viewport_top_pixels,
		input.viewport_height_pixels,
	);

	Some(ScrollMinimapPlan { frame, image_frame, viewport_frame })
}

fn minimap_input_is_valid(input: ScrollMinimapInput) -> bool {
	rect_is_valid(input.selection)
		&& rect_is_valid(input.bounds)
		&& input.export_width.is_finite()
		&& input.export_width > 0.0
		&& input.export_height.is_finite()
		&& input.export_height > 0.0
		&& finite_positive(input.preferred_width)
		&& finite_positive(input.minimum_width)
		&& finite_nonnegative(input.gap)
		&& finite_nonnegative(input.margin)
		&& finite_nonnegative(input.image_inset)
		&& input.viewport_top_pixels.is_finite()
		&& input.viewport_height_pixels.is_finite()
		&& input.bounds.width > input.margin * 2.0
		&& input.bounds.height > input.margin * 2.0
}

fn scroll_minimap_viewport_frame(
	frame: DisplayPointRect,
	export_height: f64,
	viewport_top_pixels: f64,
	viewport_height_pixels: f64,
) -> Option<DisplayPointRect> {
	if !rect_is_valid(frame) {
		return None;
	}

	let export_height = export_height.max(1.0);
	let viewport_height = viewport_height_pixels.clamp(1.0, export_height);
	let max_top = (export_height - viewport_height).max(0.0);
	let viewport_top = viewport_top_pixels.clamp(0.0, max_top);
	let marker_height = 2.0_f64.max(frame.height * viewport_height / export_height);
	let marker_y = max_y(frame) - frame.height * (viewport_top + viewport_height) / export_height;
	let marker = DisplayPointRect::new(frame.x, marker_y, frame.width, marker_height);

	intersect_rect(marker, frame)
}

fn rect_is_valid(rect: DisplayPointRect) -> bool {
	rect.x.is_finite()
		&& rect.y.is_finite()
		&& rect.width.is_finite()
		&& rect.height.is_finite()
		&& rect.width > 0.0
		&& rect.height > 0.0
}

fn finite_nonnegative(value: f64) -> bool {
	value.is_finite() && value >= 0.0
}

fn finite_positive(value: f64) -> bool {
	value.is_finite() && value > 0.0
}

fn inset_rect(rect: DisplayPointRect, inset: f64) -> DisplayPointRect {
	DisplayPointRect::new(
		rect.x + inset,
		rect.y + inset,
		rect.width - inset * 2.0,
		rect.height - inset * 2.0,
	)
}

fn intersect_rect(a: DisplayPointRect, b: DisplayPointRect) -> Option<DisplayPointRect> {
	let min_x = a.x.max(b.x);
	let min_y = a.y.max(b.y);
	let max_x = max_x(a).min(max_x(b));
	let max_y = max_y(a).min(max_y(b));
	let width = max_x - min_x;
	let height = max_y - min_y;
	if width <= 0.0 || height <= 0.0 {
		return None;
	}

	Some(DisplayPointRect::new(min_x, min_y, width, height))
}

fn max_x(rect: DisplayPointRect) -> f64 {
	rect.x + rect.width
}

fn max_y(rect: DisplayPointRect) -> f64 {
	rect.y + rect.height
}

fn mid_y(rect: DisplayPointRect) -> f64 {
	rect.y + rect.height / 2.0
}

#[cfg(test)]
mod tests {
	use super::{ScrollMinimapInput, scroll_minimap_plan};
	use crate::DisplayPointRect;

	#[test]
	fn scroll_minimap_prefers_right_side_when_space_exists() {
		let plan =
			scroll_minimap_plan(test_input(DisplayPointRect::new(100.0, 100.0, 100.0, 100.0)))
				.expect("right-side minimap plan");

		assert_eq!(plan.frame, DisplayPointRect::new(210.0, 54.0, 96.0, 192.0));
		assert_eq!(plan.image_frame, DisplayPointRect::new(213.0, 57.0, 90.0, 186.0));
		assert_eq!(plan.viewport_frame, Some(DisplayPointRect::new(213.0, 131.4, 90.0, 93.0)));
	}

	#[test]
	fn scroll_minimap_falls_back_to_left_when_right_side_is_tight() {
		let mut input = test_input(DisplayPointRect::new(130.0, 100.0, 100.0, 100.0));
		input.bounds = DisplayPointRect::new(0.0, 0.0, 250.0, 500.0);
		let plan = scroll_minimap_plan(input).expect("left-side minimap plan");

		assert_eq!(plan.frame, DisplayPointRect::new(24.0, 54.0, 96.0, 192.0));
	}

	#[test]
	fn scroll_minimap_rejects_tiny_available_space() {
		let mut input = test_input(DisplayPointRect::new(100.0, 100.0, 100.0, 100.0));
		input.bounds = DisplayPointRect::new(0.0, 0.0, 230.0, 60.0);

		assert_eq!(scroll_minimap_plan(input), None);
	}

	fn test_input(selection: DisplayPointRect) -> ScrollMinimapInput {
		ScrollMinimapInput {
			selection,
			export_width: 100.0,
			export_height: 200.0,
			bounds: DisplayPointRect::new(0.0, 0.0, 500.0, 500.0),
			preferred_width: 96.0,
			minimum_width: 44.0,
			gap: 10.0,
			margin: 10.0,
			image_inset: 3.0,
			viewport_top_pixels: 20.0,
			viewport_height_pixels: 100.0,
		}
	}
}
