#[cfg(target_os = "macos")]
use crate::overlay::WindowRenderer;
#[cfg(target_os = "macos")]
use crate::overlay::frozen_selection_handles;
#[cfg(target_os = "macos")]
use crate::overlay::macos_cursor_runtime::{self, OverlayCursorRect};
use crate::overlay::{
	CursorIcon, FrozenSelectionCorner, FrozenSelectionInteractionKind, FrozenToolbarTool,
	MonitorRect, OverlayMode, OverlaySession, Pos2, RectPoints,
};
#[cfg(target_os = "macos")]
use crate::overlay::{Rect, Vec2};

impl OverlaySession {
	#[cfg(target_os = "macos")]
	pub(super) fn apply_macos_cursor_authority(&self) {
		let render_cursor_rects = macos_overlay_cursor_uses_render_rects(
			self.state.mode,
			self.should_host_live_pointer_input_in_native_shell(),
		);

		tracing::trace!(
			op = "overlay.apply_macos_cursor_authority",
			mode = ?self.state.mode,
			render_cursor_rects,
			cursor = ?self.state.cursor,
			monitor_id = self.monitor_for_mode().or(self.state.monitor).map(|monitor| monitor.id),
			"Applied macOS cursor authority."
		);

		if !render_cursor_rects {
			if self.session_active
				&& !self.capture_windows_hidden
				&& matches!(self.state.mode, OverlayMode::Live)
				&& !self.windows.is_empty()
			{
				macos_cursor_runtime::macos_set_cursor_icon(CursorIcon::Crosshair);
			}

			return;
		}

		let Some(current_monitor) = self.monitor_for_mode().or(self.state.monitor) else {
			return;
		};
		let Some(overlay_window) =
			self.windows.values().find(|overlay_window| overlay_window.monitor == current_monitor)
		else {
			return;
		};

		overlay_window.cursor_rects.apply_cursor_for_current_pointer_or_fallback(
			self.overlay_cursor_icon_for_monitor(overlay_window.monitor),
		);
	}

	fn frozen_capture_tool_cursor_icon(
		&self,
		monitor: MonitorRect,
		target: Option<(MonitorRect, RectPoints)>,
		active: bool,
	) -> Option<CursorIcon> {
		let (target_monitor, capture_rect) = target?;

		if target_monitor != monitor {
			return Some(CursorIcon::Default);
		}
		if active {
			return Some(CursorIcon::Crosshair);
		}

		let Some(cursor) = self.state.cursor else {
			return Some(CursorIcon::Default);
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(cursor) else {
			return Some(CursorIcon::Default);
		};

		Some(if capture_rect.contains((cursor_x, cursor_y)) {
			CursorIcon::Crosshair
		} else {
			CursorIcon::Default
		})
	}

	fn frozen_selection_resize_cursor_icon(corner: FrozenSelectionCorner) -> CursorIcon {
		match corner {
			FrozenSelectionCorner::TopLeft | FrozenSelectionCorner::BottomRight => {
				CursorIcon::NwseResize
			},
			FrozenSelectionCorner::TopRight | FrozenSelectionCorner::BottomLeft => {
				CursorIcon::NeswResize
			},
		}
	}

	#[cfg_attr(target_os = "macos", allow(dead_code))]
	pub(super) fn frozen_selection_cursor_icon_for_monitor(
		&self,
		monitor: MonitorRect,
	) -> CursorIcon {
		let pen_target = (self.toolbar_state.selected_tool == FrozenToolbarTool::Pen)
			.then(|| self.frozen_brush_capture_target())
			.flatten();

		for (target, active) in [
			(self.frozen_mosaic_drag_target(), self.frozen_mosaic_drag.active),
			(self.frozen_spotlight_drag_target(), self.frozen_spotlight_drag.active),
			(self.frozen_arrow_drag_target(), self.frozen_arrow_drag.active),
			(pen_target, self.frozen_brush.active_stroke.is_some()),
		] {
			if let Some(icon) = self.frozen_capture_tool_cursor_icon(monitor, target, active) {
				return icon;
			}
		}

		let Some((target_monitor, capture_rect)) = self.frozen_selection_drag_target() else {
			return CursorIcon::Default;
		};

		if target_monitor != monitor {
			return CursorIcon::Default;
		}
		if self.frozen_selection_drag.active {
			return match self.frozen_selection_drag.interaction {
				FrozenSelectionInteractionKind::Resize(corner) => {
					Self::frozen_selection_resize_cursor_icon(corner)
				},
				FrozenSelectionInteractionKind::Move => CursorIcon::Grabbing,
			};
		}

		let Some(cursor) = self.state.cursor else {
			return CursorIcon::Default;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(cursor) else {
			return CursorIcon::Default;
		};
		let cursor_local = Pos2::new(cursor_x as f32, cursor_y as f32);

		if let Some(hit_rect) = self.frozen_text_edit_hit_rect_for_monitor(monitor)
			&& hit_rect.contains(cursor_local)
		{
			return if self.frozen_text_edit.as_ref().is_some_and(|edit| edit.dragging) {
				CursorIcon::Grabbing
			} else {
				CursorIcon::Grab
			};
		}

		if self.frozen_text_tool_active() && capture_rect.contains((cursor_x, cursor_y)) {
			return CursorIcon::Text;
		}

		match Self::frozen_selection_interaction_kind(capture_rect, cursor_x, cursor_y) {
			Some(FrozenSelectionInteractionKind::Resize(corner)) => {
				Self::frozen_selection_resize_cursor_icon(corner)
			},
			Some(FrozenSelectionInteractionKind::Move) => CursorIcon::Grab,
			None => CursorIcon::Default,
		}
	}

	#[cfg_attr(target_os = "macos", allow(dead_code))]
	pub(super) fn overlay_cursor_icon_for_monitor(&self, monitor: MonitorRect) -> CursorIcon {
		match self.state.mode {
			OverlayMode::Frozen => self.frozen_selection_cursor_icon_for_monitor(monitor),
			OverlayMode::Live => CursorIcon::Crosshair,
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn frozen_selection_cursor_rects_for_monitor(
		&self,
		monitor: MonitorRect,
	) -> Vec<OverlayCursorRect> {
		let overlay_bounds =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let pen_target = (self.toolbar_state.selected_tool == FrozenToolbarTool::Pen)
			.then(|| self.frozen_brush_capture_target())
			.flatten();

		if matches!(self.state.mode, OverlayMode::Live) {
			return vec![OverlayCursorRect::new(overlay_bounds, CursorIcon::Crosshair)];
		}
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return Vec::new();
		}

		for (target, active) in [
			(self.frozen_mosaic_drag_target(), self.frozen_mosaic_drag.active),
			(self.frozen_spotlight_drag_target(), self.frozen_spotlight_drag.active),
			(self.frozen_arrow_drag_target(), self.frozen_arrow_drag.active),
			(pen_target, self.frozen_brush.active_stroke.is_some()),
		] {
			let Some((target_monitor, capture_rect)) = target else {
				continue;
			};

			if target_monitor != monitor {
				return Vec::new();
			}
			if active {
				return vec![OverlayCursorRect::new(overlay_bounds, CursorIcon::Crosshair)];
			}

			let capture_rect = Rect::from_min_size(
				Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
				Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
			)
			.intersect(overlay_bounds);

			return (capture_rect.width() > 0.0 && capture_rect.height() > 0.0)
				.then_some(vec![OverlayCursorRect::new(capture_rect, CursorIcon::Crosshair)])
				.unwrap_or_default();
		}

		let Some((target_monitor, capture_rect)) = self.frozen_selection_drag_target() else {
			return Vec::new();
		};

		if target_monitor != monitor {
			return Vec::new();
		}
		if self.frozen_selection_drag.active {
			return match self.frozen_selection_drag.interaction {
				FrozenSelectionInteractionKind::Resize(corner) => vec![OverlayCursorRect::new(
					overlay_bounds,
					Self::frozen_selection_resize_cursor_icon(corner),
				)],
				FrozenSelectionInteractionKind::Move => {
					vec![OverlayCursorRect::new(overlay_bounds, CursorIcon::Grabbing)]
				},
			};
		}

		if let Some(hit_rect) = self.frozen_text_edit_hit_rect_for_monitor(monitor) {
			let text_edit_dragging =
				self.frozen_text_edit.as_ref().is_some_and(|edit| edit.dragging);
			let rect = hit_rect.intersect(overlay_bounds);

			if rect.width() > 0.0 && rect.height() > 0.0 {
				return vec![OverlayCursorRect::new(
					rect,
					if text_edit_dragging { CursorIcon::Grabbing } else { CursorIcon::Grab },
				)];
			}
		}

		if self.frozen_text_tool_active() {
			let capture_rect = Rect::from_min_size(
				Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
				Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
			)
			.intersect(overlay_bounds);

			return (capture_rect.width() > 0.0 && capture_rect.height() > 0.0)
				.then_some(vec![OverlayCursorRect::new(capture_rect, CursorIcon::Text)])
				.unwrap_or_default();
		}

		Self::frozen_selection_hover_cursor_rects(capture_rect)
			.into_iter()
			.filter_map(|cursor_rect| {
				let rect = cursor_rect.rect.intersect(overlay_bounds);

				(rect.width() > 0.0 && rect.height() > 0.0)
					.then_some(OverlayCursorRect::new(rect, cursor_rect.icon))
			})
			.collect()
	}

	#[cfg(target_os = "macos")]
	fn frozen_selection_hover_cursor_rects(capture_rect: RectPoints) -> Vec<OverlayCursorRect> {
		let selection_rect = Rect::from_min_size(
			Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
			Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
		);
		let interior_reach = frozen_selection_handles::resize_handle_interior_reach(selection_rect);
		let mut x_edges = vec![
			selection_rect.min.x,
			selection_rect.min.x + interior_reach.x,
			selection_rect.center().x,
			selection_rect.max.x - interior_reach.x,
			selection_rect.max.x,
		];
		let mut y_edges = vec![
			selection_rect.min.y,
			selection_rect.min.y + interior_reach.y,
			selection_rect.center().y,
			selection_rect.max.y - interior_reach.y,
			selection_rect.max.y,
		];

		for handle in WindowRenderer::frozen_selection_resize_handles(capture_rect) {
			x_edges.push(handle.hit_rect.min.x);
			x_edges.push(handle.hit_rect.max.x);
			y_edges.push(handle.hit_rect.min.y);
			y_edges.push(handle.hit_rect.max.y);
		}

		macos_cursor_runtime::sort_unique_axis_values(&mut x_edges);
		macos_cursor_runtime::sort_unique_axis_values(&mut y_edges);

		let mut rects = Vec::new();

		for x_pair in x_edges.windows(2) {
			let [min_x, max_x] = [x_pair[0], x_pair[1]];

			if max_x <= min_x {
				continue;
			}

			for y_pair in y_edges.windows(2) {
				let [min_y, max_y] = [y_pair[0], y_pair[1]];

				if max_y <= min_y {
					continue;
				}

				let rect = Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y));
				let point = rect.center();
				let Some(interaction) = Self::frozen_selection_interaction_kind(
					capture_rect,
					point.x as u32,
					point.y as u32,
				) else {
					continue;
				};
				let FrozenSelectionInteractionKind::Resize(corner) = interaction else {
					rects.push(OverlayCursorRect::new(rect, CursorIcon::Grab));

					continue;
				};
				let icon = Self::frozen_selection_resize_cursor_icon(corner);
				let mut adjusted_min = rect.min;
				let mut adjusted_max = rect.max;

				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(rect.min.x, point.y),
				) != Some(corner)
				{
					adjusted_min.x =
						macos_cursor_runtime::trim_rect_min_edge(adjusted_min.x, adjusted_max.x);
				}
				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(rect.max.x, point.y),
				) != Some(corner)
				{
					adjusted_max.x =
						macos_cursor_runtime::trim_rect_max_edge(adjusted_max.x, adjusted_min.x);
				}
				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(point.x, rect.min.y),
				) != Some(corner)
				{
					adjusted_min.y =
						macos_cursor_runtime::trim_rect_min_edge(adjusted_min.y, adjusted_max.y);
				}
				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(point.x, rect.max.y),
				) != Some(corner)
				{
					adjusted_max.y =
						macos_cursor_runtime::trim_rect_max_edge(adjusted_max.y, adjusted_min.y);
				}
				if adjusted_max.x <= adjusted_min.x || adjusted_max.y <= adjusted_min.y {
					continue;
				}

				rects.push(OverlayCursorRect::new(
					Rect::from_min_max(adjusted_min, adjusted_max),
					icon,
				));
			}
		}

		rects
	}

	pub(super) fn sync_overlay_cursor_icons(&self) {
		let current_monitor = self.monitor_for_mode().or(self.state.monitor);
		#[cfg(target_os = "macos")]
		let render_cursor_rects = macos_overlay_cursor_uses_render_rects(
			self.state.mode,
			self.should_host_live_pointer_input_in_native_shell(),
		);
		#[cfg(not(target_os = "macos"))]
		let render_cursor_rects = false;

		for overlay_window in self.windows.values() {
			let icon = self.overlay_cursor_icon_for_monitor(overlay_window.monitor);

			tracing::trace!(
				op = "overlay.sync_overlay_cursor_icons",
				monitor_id = overlay_window.monitor.id,
				mode = ?self.state.mode,
				icon = ?icon,
				active_monitor_id = ?current_monitor.map(|monitor| monitor.id),
				cursor = ?self.state.cursor,
				render_cursor_rects,
				"Synced overlay cursor icons."
			);

			#[cfg(not(target_os = "macos"))]
			overlay_window.window.set_cursor(icon);

			#[cfg(target_os = "macos")]
			{
				overlay_window.window.set_cursor(icon);

				let rects = if render_cursor_rects {
					self.frozen_selection_cursor_rects_for_monitor(overlay_window.monitor)
				} else {
					Vec::new()
				};

				overlay_window
					.cursor_rects
					.sync_cursor_rects(overlay_window.window.as_ref(), &rects);
			}
		}

		#[cfg(target_os = "macos")]
		self.apply_macos_cursor_authority();
	}
}

#[cfg(target_os = "macos")]
pub(super) const fn macos_overlay_cursor_uses_render_rects(
	mode: OverlayMode,
	_host_live_pointer_input: bool,
) -> bool {
	// Keep native cursor rects active in live mode as well. The passive input shell still owns
	// pointer dispatch, but the render window must continue advertising a native cursor shape so
	// AppKit does not fall back to an empty-rect arrow during live/frozen handoff.
	match mode {
		OverlayMode::Live | OverlayMode::Frozen => true,
	}
}
