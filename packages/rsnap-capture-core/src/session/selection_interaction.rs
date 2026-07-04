use crate::geometry::{GlobalPoint, GlobalRect, MonitorRect, WindowRect};
use crate::protocol::{CursorIntent, ToolbarItemKind};

const RESIZE_HANDLE_RADIUS_POINTS: i32 = 12;
const RESIZE_EDGE_TOLERANCE_POINTS: i32 = 4;
const LIVE_SELECTION_DEFAULT_WIDTH: u32 = 320;
const LIVE_SELECTION_DEFAULT_HEIGHT: u32 = 200;
const LIVE_SELECTION_DRAG_THRESHOLD_POINTS: u32 = 1;

pub(super) fn drag_preview(
	live_press_start: Option<GlobalPoint>,
	point: GlobalPoint,
	active_monitor: Option<MonitorRect>,
) -> Option<GlobalRect> {
	let live_press_start = live_press_start?;
	let active_monitor = active_monitor?;

	if !active_monitor.contains(live_press_start) || !active_monitor.contains(point) {
		return None;
	}

	let left = live_press_start.x.min(point.x);
	let top = live_press_start.y.min(point.y);
	let width = live_press_start.x.abs_diff(point.x);
	let height = live_press_start.y.abs_diff(point.y);

	if width.max(height) < LIVE_SELECTION_DRAG_THRESHOLD_POINTS {
		return None;
	}

	Some(GlobalRect::new(left, top, width.max(1), height.max(1)))
}

pub(super) fn default_selection(
	point: GlobalPoint,
	active_monitor: Option<MonitorRect>,
) -> GlobalRect {
	let half_width = (LIVE_SELECTION_DEFAULT_WIDTH / 2) as i32;
	let half_height = (LIVE_SELECTION_DEFAULT_HEIGHT / 2) as i32;
	let unclamped_x = point.x.saturating_sub(half_width);
	let unclamped_y = point.y.saturating_sub(half_height);
	let (origin_x, origin_y) = if let Some(monitor) = active_monitor {
		let max_x = if monitor.width > LIVE_SELECTION_DEFAULT_WIDTH {
			monitor
				.origin
				.x
				.saturating_add_unsigned(monitor.width)
				.saturating_sub_unsigned(LIVE_SELECTION_DEFAULT_WIDTH)
		} else {
			monitor.origin.x
		};
		let max_y = if monitor.height > LIVE_SELECTION_DEFAULT_HEIGHT {
			monitor
				.origin
				.y
				.saturating_add_unsigned(monitor.height)
				.saturating_sub_unsigned(LIVE_SELECTION_DEFAULT_HEIGHT)
		} else {
			monitor.origin.y
		};

		(unclamped_x.clamp(monitor.origin.x, max_x), unclamped_y.clamp(monitor.origin.y, max_y))
	} else {
		(unclamped_x, unclamped_y)
	};

	GlobalRect::new(origin_x, origin_y, LIVE_SELECTION_DEFAULT_WIDTH, LIVE_SELECTION_DEFAULT_HEIGHT)
}

pub(super) fn resolve_target(
	active_monitor: Option<MonitorRect>,
	highlighted_window: Option<WindowRect>,
) -> Option<GlobalRect> {
	highlighted_window.and_then(WindowRect::global_rect).or_else(|| {
		active_monitor.map(|monitor| {
			GlobalRect::new(monitor.origin.x, monitor.origin.y, monitor.width, monitor.height)
		})
	})
}

pub(super) fn frozen_cursor_intent(
	point: GlobalPoint,
	selection: GlobalRect,
	selected_toolbar_item: ToolbarItemKind,
) -> CursorIntent {
	let selection_left = selection.x;
	let selection_top = selection.y;
	let selection_right = selection.x.saturating_add_unsigned(selection.width);
	let selection_bottom = selection.y.saturating_add_unsigned(selection.height);

	if point_in_handle(point, selection_left, selection_top, RESIZE_HANDLE_RADIUS_POINTS) {
		return CursorIntent::ResizeNorthWest;
	}
	if point_in_handle(point, selection_right, selection_bottom, RESIZE_HANDLE_RADIUS_POINTS) {
		return CursorIntent::ResizeSouthEast;
	}
	if point_in_handle(point, selection_right, selection_top, RESIZE_HANDLE_RADIUS_POINTS) {
		return CursorIntent::ResizeNorthEast;
	}
	if point_in_handle(point, selection_left, selection_bottom, RESIZE_HANDLE_RADIUS_POINTS) {
		return CursorIntent::ResizeSouthWest;
	}

	let on_vertical_edge = point.y >= selection_top
		&& point.y <= selection_bottom
		&& (point.x - selection_left).abs() <= RESIZE_EDGE_TOLERANCE_POINTS;

	if on_vertical_edge {
		return CursorIntent::ResizeWest;
	}

	let on_right_edge = point.y >= selection_top
		&& point.y <= selection_bottom
		&& (point.x - selection_right).abs() <= RESIZE_EDGE_TOLERANCE_POINTS;

	if on_right_edge {
		return CursorIntent::ResizeEast;
	}

	let on_top_edge = point.x >= selection_left
		&& point.x <= selection_right
		&& (point.y - selection_top).abs() <= RESIZE_EDGE_TOLERANCE_POINTS;

	if on_top_edge {
		return CursorIntent::ResizeNorth;
	}

	let on_bottom_edge = point.x >= selection_left
		&& point.x <= selection_right
		&& (point.y - selection_bottom).abs() <= RESIZE_EDGE_TOLERANCE_POINTS;

	if on_bottom_edge {
		return CursorIntent::ResizeSouth;
	}
	if selection.contains(point) {
		return match selected_toolbar_item {
			ToolbarItemKind::Text => CursorIntent::Text,
			ToolbarItemKind::Pointer => CursorIntent::Grab,
			_ => CursorIntent::Default,
		};
	}

	CursorIntent::Default
}

fn point_in_handle(point: GlobalPoint, handle_x: i32, handle_y: i32, radius: i32) -> bool {
	(point.x - handle_x).abs() <= radius && (point.y - handle_y).abs() <= radius
}

#[cfg(test)]
mod tests {
	use crate::geometry::{GlobalPoint, GlobalRect, MonitorRect, WindowRect};
	use crate::protocol::{CursorIntent, ToolbarItemKind};
	use crate::session::selection_interaction;

	fn monitor() -> MonitorRect {
		MonitorRect {
			id: 1,
			origin: GlobalPoint::new(100, 200),
			width: 640,
			height: 480,
			scale_factor_x1000: 2_000,
		}
	}

	#[test]
	fn drag_preview_requires_start_and_end_inside_monitor() {
		assert_eq!(
			selection_interaction::drag_preview(
				Some(GlobalPoint::new(120, 220)),
				GlobalPoint::new(180, 300),
				Some(monitor()),
			),
			Some(GlobalRect::new(120, 220, 60, 80))
		);
		assert_eq!(
			selection_interaction::drag_preview(
				Some(GlobalPoint::new(120, 220)),
				GlobalPoint::new(90, 300),
				Some(monitor()),
			),
			None
		);
	}

	#[test]
	fn drag_preview_ignores_unstarted_or_tiny_drags() {
		assert_eq!(
			selection_interaction::drag_preview(None, GlobalPoint::new(120, 220), Some(monitor()),),
			None
		);
		assert_eq!(
			selection_interaction::drag_preview(
				Some(GlobalPoint::new(120, 220)),
				GlobalPoint::new(120, 220),
				Some(monitor()),
			),
			None
		);
		assert_eq!(
			selection_interaction::drag_preview(
				Some(GlobalPoint::new(120, 220)),
				GlobalPoint::new(121, 220),
				None,
			),
			None
		);
	}

	#[test]
	fn default_selection_clamps_to_monitor_bounds() {
		assert_eq!(
			selection_interaction::default_selection(GlobalPoint::new(120, 230), Some(monitor())),
			GlobalRect::new(100, 200, 320, 200)
		);
		assert_eq!(
			selection_interaction::default_selection(GlobalPoint::new(730, 670), Some(monitor())),
			GlobalRect::new(420, 480, 320, 200)
		);
	}

	#[test]
	fn resolve_target_prefers_highlighted_window_over_monitor() {
		let window = WindowRect { window_id: Some(9), x: 130, y: 240, width: 70, height: 80 };

		assert_eq!(
			selection_interaction::resolve_target(Some(monitor()), Some(window)),
			Some(GlobalRect::new(130, 240, 70, 80))
		);
		assert_eq!(
			selection_interaction::resolve_target(Some(monitor()), None),
			Some(GlobalRect::new(100, 200, 640, 480))
		);
	}

	#[test]
	fn frozen_cursor_intent_tracks_resize_edges_and_tools() {
		let selection = GlobalRect::new(100, 200, 80, 60);

		assert_eq!(
			selection_interaction::frozen_cursor_intent(
				GlobalPoint::new(180, 230),
				selection,
				ToolbarItemKind::Pointer,
			),
			CursorIntent::ResizeEast
		);
		assert_eq!(
			selection_interaction::frozen_cursor_intent(
				GlobalPoint::new(120, 220),
				selection,
				ToolbarItemKind::Pointer,
			),
			CursorIntent::Grab
		);
		assert_eq!(
			selection_interaction::frozen_cursor_intent(
				GlobalPoint::new(120, 220),
				selection,
				ToolbarItemKind::Text,
			),
			CursorIntent::Text
		);
	}
}
