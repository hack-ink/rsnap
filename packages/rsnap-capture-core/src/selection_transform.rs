//! Frozen selection hit-testing and transform geometry.

use crate::DisplayPointRect;

/// Frozen selection transform operation selected by pointer hit-testing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrozenSelectionTransformKind {
	/// Move the whole selection rectangle.
	Move,
	/// Resize the left edge.
	ResizeLeft,
	/// Resize the right edge.
	ResizeRight,
	/// Resize the top edge.
	ResizeTop,
	/// Resize the bottom edge.
	ResizeBottom,
	/// Resize the top-left corner.
	ResizeTopLeft,
	/// Resize the top-right corner.
	ResizeTopRight,
	/// Resize the bottom-left corner.
	ResizeBottomLeft,
	/// Resize the bottom-right corner.
	ResizeBottomRight,
}

/// Input payload for resolving a frozen selection transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenSelectionTransformInput {
	/// The active transform operation.
	pub kind: FrozenSelectionTransformKind,
	/// The selection rect when the interaction started.
	pub initial_selection: DisplayPointRect,
	/// The monitor bounds that constrain the selection.
	pub monitor_frame: DisplayPointRect,
	/// Pointer x-position when the interaction started.
	pub initial_pointer_x: f64,
	/// Pointer y-position when the interaction started.
	pub initial_pointer_y: f64,
	/// Current pointer x-position.
	pub point_x: f64,
	/// Current pointer y-position.
	pub point_y: f64,
	/// Minimum allowed selection width and height.
	pub minimum_size: f64,
}

/// Hit-tests a pointer against a frozen selection transform target.
#[must_use]
pub fn frozen_selection_transform_hit_test(
	point_x: f64,
	point_y: f64,
	selection: DisplayPointRect,
	handle_radius: f64,
	edge_tolerance: f64,
) -> Option<FrozenSelectionTransformKind> {
	if !rect_is_valid(selection)
		|| !point_is_finite(point_x, point_y)
		|| !handle_radius.is_finite()
		|| !edge_tolerance.is_finite()
	{
		return None;
	}

	let handle_radius = handle_radius.max(0.0);
	let edge_tolerance = edge_tolerance.max(0.0);
	let left = selection.x;
	let right = max_x(selection);
	let top = max_y(selection);
	let bottom = selection.y;

	if (point_x - left).abs() <= handle_radius && (point_y - top).abs() <= handle_radius {
		return Some(FrozenSelectionTransformKind::ResizeTopLeft);
	}
	if (point_x - right).abs() <= handle_radius && (point_y - top).abs() <= handle_radius {
		return Some(FrozenSelectionTransformKind::ResizeTopRight);
	}
	if (point_x - left).abs() <= handle_radius && (point_y - bottom).abs() <= handle_radius {
		return Some(FrozenSelectionTransformKind::ResizeBottomLeft);
	}
	if (point_x - right).abs() <= handle_radius && (point_y - bottom).abs() <= handle_radius {
		return Some(FrozenSelectionTransformKind::ResizeBottomRight);
	}
	if point_y >= bottom && point_y <= top && (point_x - left).abs() <= edge_tolerance {
		return Some(FrozenSelectionTransformKind::ResizeLeft);
	}
	if point_y >= bottom && point_y <= top && (point_x - right).abs() <= edge_tolerance {
		return Some(FrozenSelectionTransformKind::ResizeRight);
	}
	if point_x >= left && point_x <= right && (point_y - top).abs() <= edge_tolerance {
		return Some(FrozenSelectionTransformKind::ResizeTop);
	}
	if point_x >= left && point_x <= right && (point_y - bottom).abs() <= edge_tolerance {
		return Some(FrozenSelectionTransformKind::ResizeBottom);
	}
	if point_x >= left && point_x < right && point_y >= bottom && point_y < top {
		return Some(FrozenSelectionTransformKind::Move);
	}

	None
}

/// Resolves the transformed frozen selection constrained to the monitor bounds.
#[must_use]
pub fn frozen_selection_transform_rect(
	input: FrozenSelectionTransformInput,
) -> Option<DisplayPointRect> {
	if !rect_is_valid(input.initial_selection)
		|| !rect_is_valid(input.monitor_frame)
		|| !point_is_finite(input.initial_pointer_x, input.initial_pointer_y)
		|| !point_is_finite(input.point_x, input.point_y)
		|| !input.minimum_size.is_finite()
		|| input.minimum_size <= 0.0
	{
		return None;
	}

	let selection = input.initial_selection;
	let monitor = input.monitor_frame;
	let min_size = input.minimum_size;
	let delta_x = input.point_x - input.initial_pointer_x;
	let delta_y = input.point_y - input.initial_pointer_y;

	match input.kind {
		FrozenSelectionTransformKind::Move => clamped_rect(
			selection.width,
			selection.height,
			selection.x + delta_x,
			selection.y + delta_y,
			monitor,
		),
		FrozenSelectionTransformKind::ResizeLeft => {
			let new_min_x = clamp(selection.x + delta_x, monitor.x, max_x(selection) - min_size);

			Some(DisplayPointRect::new(
				new_min_x,
				selection.y,
				max_x(selection) - new_min_x,
				selection.height,
			))
		},
		FrozenSelectionTransformKind::ResizeRight => {
			let new_max_x =
				clamp(max_x(selection) + delta_x, selection.x + min_size, max_x(monitor));

			Some(DisplayPointRect::new(
				selection.x,
				selection.y,
				new_max_x - selection.x,
				selection.height,
			))
		},
		FrozenSelectionTransformKind::ResizeTop => {
			let new_max_y =
				clamp(max_y(selection) + delta_y, selection.y + min_size, max_y(monitor));

			Some(DisplayPointRect::new(
				selection.x,
				selection.y,
				selection.width,
				new_max_y - selection.y,
			))
		},
		FrozenSelectionTransformKind::ResizeBottom => {
			let new_min_y = clamp(selection.y + delta_y, monitor.y, max_y(selection) - min_size);

			Some(DisplayPointRect::new(
				selection.x,
				new_min_y,
				selection.width,
				max_y(selection) - new_min_y,
			))
		},
		FrozenSelectionTransformKind::ResizeTopLeft => {
			let new_min_x = clamp(selection.x + delta_x, monitor.x, max_x(selection) - min_size);
			let new_max_y =
				clamp(max_y(selection) + delta_y, selection.y + min_size, max_y(monitor));

			Some(DisplayPointRect::new(
				new_min_x,
				selection.y,
				max_x(selection) - new_min_x,
				new_max_y - selection.y,
			))
		},
		FrozenSelectionTransformKind::ResizeTopRight => {
			let new_max_x =
				clamp(max_x(selection) + delta_x, selection.x + min_size, max_x(monitor));
			let new_max_y =
				clamp(max_y(selection) + delta_y, selection.y + min_size, max_y(monitor));

			Some(DisplayPointRect::new(
				selection.x,
				selection.y,
				new_max_x - selection.x,
				new_max_y - selection.y,
			))
		},
		FrozenSelectionTransformKind::ResizeBottomLeft => {
			let new_min_x = clamp(selection.x + delta_x, monitor.x, max_x(selection) - min_size);
			let new_min_y = clamp(selection.y + delta_y, monitor.y, max_y(selection) - min_size);

			Some(DisplayPointRect::new(
				new_min_x,
				new_min_y,
				max_x(selection) - new_min_x,
				max_y(selection) - new_min_y,
			))
		},
		FrozenSelectionTransformKind::ResizeBottomRight => {
			let new_max_x =
				clamp(max_x(selection) + delta_x, selection.x + min_size, max_x(monitor));
			let new_min_y = clamp(selection.y + delta_y, monitor.y, max_y(selection) - min_size);

			Some(DisplayPointRect::new(
				selection.x,
				new_min_y,
				new_max_x - selection.x,
				max_y(selection) - new_min_y,
			))
		},
	}
}

fn clamped_rect(
	width: f64,
	height: f64,
	x: f64,
	y: f64,
	monitor: DisplayPointRect,
) -> Option<DisplayPointRect> {
	if width <= 0.0 || height <= 0.0 || !point_is_finite(x, y) {
		return None;
	}

	let max_rect_x = monitor.x.max(max_x(monitor) - width);
	let max_rect_y = monitor.y.max(max_y(monitor) - height);

	Some(DisplayPointRect::new(
		clamp(x, monitor.x, max_rect_x),
		clamp(y, monitor.y, max_rect_y),
		width,
		height,
	))
}

fn rect_is_valid(rect: DisplayPointRect) -> bool {
	rect.x.is_finite()
		&& rect.y.is_finite()
		&& rect.width.is_finite()
		&& rect.height.is_finite()
		&& rect.width > 0.0
		&& rect.height > 0.0
}

fn point_is_finite(x: f64, y: f64) -> bool {
	x.is_finite() && y.is_finite()
}

fn max_x(rect: DisplayPointRect) -> f64 {
	rect.x + rect.width
}

fn max_y(rect: DisplayPointRect) -> f64 {
	rect.y + rect.height
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
	value.clamp(min.min(max), min.max(max))
}

#[cfg(test)]
mod tests {
	use crate::DisplayPointRect;
	use crate::selection_transform::{
		self, FrozenSelectionTransformInput, FrozenSelectionTransformKind,
	};

	#[test]
	fn hit_test_prefers_corner_handles_before_edges() {
		let selection = DisplayPointRect::new(100.0, 80.0, 240.0, 160.0);

		assert_eq!(
			selection_transform::frozen_selection_transform_hit_test(
				102.0, 238.0, selection, 12.0, 4.0
			),
			Some(FrozenSelectionTransformKind::ResizeTopLeft)
		);
		assert_eq!(
			selection_transform::frozen_selection_transform_hit_test(
				220.0, 240.0, selection, 12.0, 4.0
			),
			Some(FrozenSelectionTransformKind::ResizeTop)
		);
		assert_eq!(
			selection_transform::frozen_selection_transform_hit_test(
				180.0, 120.0, selection, 12.0, 4.0
			),
			Some(FrozenSelectionTransformKind::Move)
		);
	}

	#[test]
	fn transform_move_clamps_to_monitor() {
		let rect =
			selection_transform::frozen_selection_transform_rect(FrozenSelectionTransformInput {
				kind: FrozenSelectionTransformKind::Move,
				initial_selection: DisplayPointRect::new(100.0, 80.0, 240.0, 160.0),
				monitor_frame: DisplayPointRect::new(0.0, 0.0, 500.0, 400.0),
				initial_pointer_x: 150.0,
				initial_pointer_y: 120.0,
				point_x: -100.0,
				point_y: 500.0,
				minimum_size: 1.0,
			});

		assert_eq!(rect, Some(DisplayPointRect::new(0.0, 240.0, 240.0, 160.0)));
	}

	#[test]
	fn transform_resize_bottom_right_preserves_minimum_size() {
		let rect =
			selection_transform::frozen_selection_transform_rect(FrozenSelectionTransformInput {
				kind: FrozenSelectionTransformKind::ResizeBottomRight,
				initial_selection: DisplayPointRect::new(100.0, 80.0, 240.0, 160.0),
				monitor_frame: DisplayPointRect::new(0.0, 0.0, 500.0, 400.0),
				initial_pointer_x: 340.0,
				initial_pointer_y: 80.0,
				point_x: 50.0,
				point_y: 300.0,
				minimum_size: 12.0,
			});

		assert_eq!(rect, Some(DisplayPointRect::new(100.0, 228.0, 12.0, 12.0)));
	}

	#[test]
	fn rejects_invalid_transform_input() {
		let rect =
			selection_transform::frozen_selection_transform_rect(FrozenSelectionTransformInput {
				kind: FrozenSelectionTransformKind::ResizeRight,
				initial_selection: DisplayPointRect::new(100.0, 80.0, 240.0, 160.0),
				monitor_frame: DisplayPointRect::new(0.0, 0.0, 500.0, 400.0),
				initial_pointer_x: 340.0,
				initial_pointer_y: 80.0,
				point_x: f64::NAN,
				point_y: 80.0,
				minimum_size: 12.0,
			});

		assert_eq!(rect, None);
	}
}
