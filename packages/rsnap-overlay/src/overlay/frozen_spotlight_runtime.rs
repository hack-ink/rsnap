use crate::overlay::{
	FrozenEditKind, FrozenSpotlightAnnotation, FrozenSpotlightDragState, FrozenToolbarTool,
	GlobalPoint, OverlaySession,
};

impl OverlaySession {
	pub(super) const fn frozen_spotlight_outside_brightness_numerator() -> u16 {
		148
	}

	pub(super) const fn frozen_spotlight_scrim_alpha() -> u8 {
		u8::MAX - Self::frozen_spotlight_outside_brightness_numerator() as u8
	}

	fn rect_points_right(rect: crate::RectPoints) -> u32 {
		rect.x.saturating_add(rect.width)
	}

	fn rect_points_bottom(rect: crate::RectPoints) -> u32 {
		rect.y.saturating_add(rect.height)
	}

	fn rect_points_contains_rect(outer: crate::RectPoints, inner: crate::RectPoints) -> bool {
		inner.x >= outer.x
			&& inner.y >= outer.y
			&& Self::rect_points_right(inner) <= Self::rect_points_right(outer)
			&& Self::rect_points_bottom(inner) <= Self::rect_points_bottom(outer)
	}

	pub(super) fn clipped_frozen_spotlight_rects(
		capture_rect: crate::RectPoints,
		spotlight_rects: impl IntoIterator<Item = crate::RectPoints>,
	) -> Vec<crate::RectPoints> {
		spotlight_rects
			.into_iter()
			.filter_map(|rect| Self::intersect_rect_points(rect, capture_rect))
			.filter(|rect| !rect.is_empty())
			.collect()
	}

	pub(super) fn frozen_spotlight_scrim_rects(
		capture_rect: crate::RectPoints,
		spotlight_rects: &[crate::RectPoints],
	) -> Vec<crate::RectPoints> {
		if capture_rect.is_empty() || spotlight_rects.is_empty() {
			return Vec::new();
		}

		let mut x_edges = vec![capture_rect.x, Self::rect_points_right(capture_rect)];
		let mut y_edges = vec![capture_rect.y, Self::rect_points_bottom(capture_rect)];

		for rect in spotlight_rects {
			x_edges.push(rect.x);
			x_edges.push(Self::rect_points_right(*rect));
			y_edges.push(rect.y);
			y_edges.push(Self::rect_points_bottom(*rect));
		}

		x_edges.sort_unstable();
		x_edges.dedup();
		y_edges.sort_unstable();
		y_edges.dedup();

		let mut scrim_rects = Vec::new();

		for y_window in y_edges.windows(2) {
			let [top, bottom] = [y_window[0], y_window[1]];

			if bottom <= top {
				continue;
			}

			for x_window in x_edges.windows(2) {
				let [left, right] = [x_window[0], x_window[1]];

				if right <= left {
					continue;
				}

				let rect = crate::RectPoints::new(
					left,
					top,
					right.saturating_sub(left),
					bottom.saturating_sub(top),
				);

				if spotlight_rects
					.iter()
					.any(|spotlight_rect| Self::rect_points_contains_rect(*spotlight_rect, rect))
				{
					continue;
				}

				scrim_rects.push(rect);
			}
		}

		scrim_rects
	}

	pub(super) fn begin_frozen_spotlight_drag(&mut self, global: GlobalPoint) -> bool {
		if self.toolbar_state.selected_tool != FrozenToolbarTool::Spotlight
			|| self.frozen_spotlight_drag.active
		{
			return false;
		}

		let Some((monitor, capture_rect)) = self.frozen_brush_capture_target() else {
			return false;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(global) else {
			return false;
		};

		if !capture_rect.contains((cursor_x, cursor_y)) {
			return false;
		}

		self.frozen_spotlight_drag =
			FrozenSpotlightDragState { active: true, anchor_x: cursor_x, anchor_y: cursor_y };
		self.frozen_spotlight_preview_rect = Some(crate::RectPoints::new(cursor_x, cursor_y, 1, 1));

		self.request_redraw_for_monitor(monitor);

		true
	}

	pub(super) fn update_frozen_spotlight_drag_rect(&mut self, global: GlobalPoint) -> bool {
		if !self.frozen_spotlight_drag.active {
			return false;
		}

		let Some((monitor, capture_rect)) = self.frozen_brush_capture_target() else {
			self.stop_frozen_spotlight_drag();

			return false;
		};
		let (cursor_x, cursor_y) = Self::clamped_local_point_in_rect(monitor, capture_rect, global);
		let next_rect = Self::rect_from_drag_points(
			self.frozen_spotlight_drag.anchor_x,
			self.frozen_spotlight_drag.anchor_y,
			cursor_x,
			cursor_y,
		);

		if self.frozen_spotlight_preview_rect == Some(next_rect) {
			return false;
		}

		self.frozen_spotlight_preview_rect = Some(next_rect);

		self.request_redraw_for_monitor(monitor);

		true
	}

	pub(super) fn stop_frozen_spotlight_drag(&mut self) {
		self.frozen_spotlight_drag = FrozenSpotlightDragState::default();
		self.frozen_spotlight_preview_rect = None;
	}

	pub(super) fn commit_frozen_spotlight_drag(&mut self) -> bool {
		let preview_rect = self.frozen_spotlight_preview_rect;

		self.stop_frozen_spotlight_drag();

		let Some(preview_rect) = preview_rect else {
			return false;
		};

		if preview_rect.width <= 1 && preview_rect.height <= 1 {
			return false;
		}

		self.frozen_spotlight_annotations.push(FrozenSpotlightAnnotation { rect: preview_rect });
		self.push_frozen_edit_to_undo_history(FrozenEditKind::SpotlightAnnotation);
		self.sync_frozen_toolbar_state();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	pub(super) fn undo_frozen_spotlight_annotation(&mut self) -> bool {
		let Some(annotation) = self.frozen_spotlight_annotations.pop() else {
			return false;
		};

		self.frozen_spotlight_redo_annotations.push(annotation);
		self.sync_frozen_toolbar_state();

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	pub(super) fn redo_frozen_spotlight_annotation(&mut self) -> bool {
		let Some(annotation) = self.frozen_spotlight_redo_annotations.pop() else {
			return false;
		};

		self.frozen_spotlight_annotations.push(annotation);
		self.sync_frozen_toolbar_state();

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}
}
