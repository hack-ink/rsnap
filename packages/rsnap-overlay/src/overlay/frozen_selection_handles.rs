use crate::overlay::{FrozenSelectionCorner, Pos2, Rect, RectPoints, Vec2};

pub(super) const RESIZE_HANDLE_OUTER_RADIUS_POINTS: f32 = 4.25;
pub(super) const RESIZE_HANDLE_CENTER_DOT_RADIUS_POINTS: f32 = 1.15;
pub(super) const RESIZE_HANDLE_STROKE_WIDTH_POINTS: f32 = 1.3;

const RESIZE_HANDLE_HIT_SIZE_POINTS: f32 = 24.0;
const RESIZE_HANDLE_HIT_OFFSET_POINTS: f32 = 4.0;
const RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FrozenSelectionResizeHandleGeometry {
	pub(super) corner: FrozenSelectionCorner,
	pub(super) anchor: Pos2,
	pub(super) hit_rect: Rect,
}

pub(super) fn resize_handles(capture_rect: RectPoints) -> [FrozenSelectionResizeHandleGeometry; 4] {
	let rect = Rect::from_min_size(
		Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
		Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
	);

	[
		resize_handle(FrozenSelectionCorner::TopLeft, rect.min),
		resize_handle(FrozenSelectionCorner::TopRight, Pos2::new(rect.max.x, rect.min.y)),
		resize_handle(FrozenSelectionCorner::BottomLeft, Pos2::new(rect.min.x, rect.max.y)),
		resize_handle(FrozenSelectionCorner::BottomRight, rect.max),
	]
}

pub(super) fn resize_handle_interior_reach(rect: Rect) -> Vec2 {
	Vec2::new(
		(rect.width() * 0.35).min(RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS),
		(rect.height() * 0.35).min(RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS),
	)
}

pub(super) fn resize_handle_interior_hit(
	corner: FrozenSelectionCorner,
	rect: Rect,
	cursor_local: Pos2,
) -> bool {
	let interior_reach = resize_handle_interior_reach(rect);

	match corner {
		FrozenSelectionCorner::TopLeft => {
			cursor_local.x <= rect.min.x + interior_reach.x
				&& cursor_local.y <= rect.min.y + interior_reach.y
		},
		FrozenSelectionCorner::TopRight => {
			cursor_local.x >= rect.max.x - interior_reach.x
				&& cursor_local.y <= rect.min.y + interior_reach.y
		},
		FrozenSelectionCorner::BottomLeft => {
			cursor_local.x <= rect.min.x + interior_reach.x
				&& cursor_local.y >= rect.max.y - interior_reach.y
		},
		FrozenSelectionCorner::BottomRight => {
			cursor_local.x >= rect.max.x - interior_reach.x
				&& cursor_local.y >= rect.max.y - interior_reach.y
		},
	}
}

fn resize_handle(
	corner: FrozenSelectionCorner,
	anchor: Pos2,
) -> FrozenSelectionResizeHandleGeometry {
	let hit_size = Vec2::splat(RESIZE_HANDLE_HIT_SIZE_POINTS);
	let hit_offset = RESIZE_HANDLE_HIT_OFFSET_POINTS;
	let hit_center = match corner {
		FrozenSelectionCorner::TopLeft => Pos2::new(anchor.x - hit_offset, anchor.y - hit_offset),
		FrozenSelectionCorner::TopRight => Pos2::new(anchor.x + hit_offset, anchor.y - hit_offset),
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
