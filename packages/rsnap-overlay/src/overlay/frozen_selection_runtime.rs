use std::time::{Duration, Instant};

use image::{Rgba, RgbaImage};

use crate::overlay::{
	CursorIcon, FrozenCaptureSource, FrozenMosaicDragState, FrozenSelectionCorner,
	FrozenSelectionDragState, FrozenSelectionInteractionKind, FrozenToolbarTool, GlobalPoint,
	LiveCaptureInteraction, LiveClickCaptureTarget, MonitorRect, MonitorRectPoints, OverlayMode,
	OverlaySession, Pos2, RectPoints, WindowRenderer,
};
#[cfg(target_os = "macos")]
use crate::overlay::{
	FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS, OverlayCursorRect, Rect, Vec2,
};

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
				super::macos_set_cursor_icon(CursorIcon::Crosshair);
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

		overlay_window
			.cursor_rects
			.apply_cursor_for_current_pointer_or_fallback(
				self.overlay_cursor_icon_for_monitor(overlay_window.monitor),
			);
	}

	fn live_capture_interaction_hovered_window_rect(
		interaction: LiveCaptureInteraction,
	) -> Option<MonitorRectPoints> {
		match interaction {
			LiveCaptureInteraction::HoverWindow { monitor, target }
			| LiveCaptureInteraction::PressPending {
				monitor, click_target: Some(target), ..
			} => target.capture_rect.map(|rect| MonitorRectPoints { monitor_id: monitor.id, rect }),
			LiveCaptureInteraction::FrozenFromClick {
				monitor,
				target: LiveClickCaptureTarget { capture_rect: Some(target), .. },
			} => Some(MonitorRectPoints { monitor_id: monitor.id, rect: target }),
			_ => None,
		}
	}

	fn live_capture_interaction_drag_rect(
		interaction: LiveCaptureInteraction,
	) -> Option<MonitorRectPoints> {
		match interaction {
			LiveCaptureInteraction::DraggingSelection { monitor, press_global, current_global } => {
				let rect = monitor.local_rect_from_points(press_global, current_global)?;

				(!rect.is_empty()).then_some(MonitorRectPoints { monitor_id: monitor.id, rect })
			},
			LiveCaptureInteraction::FrozenFromDrag { monitor, capture_rect } => {
				Some(MonitorRectPoints { monitor_id: monitor.id, rect: capture_rect })
			},
			_ => None,
		}
	}

	pub(super) fn sync_live_capture_visual_state(&mut self) {
		self.state.hovered_window_rect =
			Self::live_capture_interaction_hovered_window_rect(self.live_capture_interaction);
		self.state.drag_rect =
			Self::live_capture_interaction_drag_rect(self.live_capture_interaction);
	}

	pub(super) fn set_live_capture_interaction(&mut self, interaction: LiveCaptureInteraction) {
		let was_hiding_auxiliary_windows = self.live_capture_hides_auxiliary_windows();

		self.live_capture_interaction = interaction;

		self.sync_live_capture_visual_state();

		let now_hiding_auxiliary_windows = self.live_capture_hides_auxiliary_windows();

		if now_hiding_auxiliary_windows
			&& (!was_hiding_auxiliary_windows
				|| self.hud_window_visible
				|| self.loupe_window_visible)
		{
			self.hide_auxiliary_windows_for_live_capture();
		}
	}

	pub(super) fn live_capture_interaction_is_press_pending(&self) -> bool {
		matches!(self.live_capture_interaction, LiveCaptureInteraction::PressPending { .. })
	}

	pub(super) fn live_capture_interaction_is_dragging(&self) -> bool {
		matches!(self.live_capture_interaction, LiveCaptureInteraction::DraggingSelection { .. })
	}

	pub(super) fn live_capture_interaction_is_frozen_handoff(&self) -> bool {
		matches!(
			self.live_capture_interaction,
			LiveCaptureInteraction::FrozenFromClick { .. }
				| LiveCaptureInteraction::FrozenFromDrag { .. }
		)
	}

	pub(super) fn live_capture_target_from_snapshot(
		&self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) -> Option<LiveClickCaptureTarget> {
		self.window_list_snapshot.as_ref()?;

		let target = self
			.hovered_window_hit_from_window_list_snapshot(monitor, cursor)
			.map_or_else(LiveClickCaptureTarget::fullscreen_fallback, |hit| {
				LiveClickCaptureTarget::from_window_hit(monitor, hit)
			});

		Some(target)
	}

	fn live_capture_drag_threshold_reached(
		monitor: MonitorRect,
		press_global: GlobalPoint,
		current_global: GlobalPoint,
	) -> bool {
		monitor.local_rect_from_points(press_global, current_global).is_some_and(|rect| {
			rect.width as f32 >= crate::overlay::LIVE_DRAG_START_THRESHOLD_PX
				&& rect.height as f32 >= crate::overlay::LIVE_DRAG_START_THRESHOLD_PX
		})
	}

	pub(super) fn begin_live_capture_press(
		&mut self,
		monitor: MonitorRect,
		press_global: GlobalPoint,
	) {
		// Lock click intent at mouse-down so release never has to recompute against a changed
		// desktop snapshot.
		let click_target = self.live_capture_target_from_snapshot(monitor, press_global);

		self.note_frozen_transition_press_pending(monitor, click_target);
		self.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
			monitor,
			press_global,
			click_target,
			release_global: None,
			released: false,
		});
		#[cfg(target_os = "macos")]
		self.prewarm_frozen_capture_live_stream_refresh(monitor);
	}

	pub(super) fn resolve_live_capture_click_target(
		&mut self,
		monitor: MonitorRect,
		click_target: LiveClickCaptureTarget,
	) {
		let LiveCaptureInteraction::PressPending { press_global, release_global, released, .. } =
			self.live_capture_interaction
		else {
			return;
		};

		if let Some(cursor) = release_global.or(self.state.cursor).or(Some(press_global))
			&& released
		{
			self.begin_frozen_capture_from_click(monitor, click_target, cursor);

			return;
		}

		self.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
			monitor,
			press_global,
			click_target: Some(click_target),
			release_global,
			released,
		});
	}

	pub(super) fn begin_frozen_capture_from_click(
		&mut self,
		monitor: MonitorRect,
		target: LiveClickCaptureTarget,
		cursor: GlobalPoint,
	) {
		self.set_live_capture_interaction(LiveCaptureInteraction::FrozenFromClick {
			monitor,
			target,
		});
		self.begin_frozen_capture_with_rect(
			monitor,
			target.capture_rect,
			target.window_target,
			Some(cursor),
		);
	}

	pub(super) fn begin_frozen_capture_from_drag(
		&mut self,
		monitor: MonitorRect,
		capture_rect: RectPoints,
		cursor: GlobalPoint,
	) {
		self.set_live_capture_interaction(LiveCaptureInteraction::FrozenFromDrag {
			monitor,
			capture_rect,
		});
		self.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, Some(cursor));
	}

	pub(super) fn update_live_drag_rect(&mut self, monitor: MonitorRect, global: GlobalPoint) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			self.set_live_capture_interaction(LiveCaptureInteraction::Idle);

			return;
		}
		if self.frozen_display_handoff_pending() {
			return;
		}

		match self.live_capture_interaction {
			LiveCaptureInteraction::PressPending {
				monitor: press_monitor,
				press_global,
				click_target,
				release_global,
				released: false,
			} => {
				if press_monitor == monitor
					&& Self::live_capture_drag_threshold_reached(
						press_monitor,
						press_global,
						global,
					) {
					self.set_live_capture_interaction(LiveCaptureInteraction::DraggingSelection {
						monitor: press_monitor,
						press_global,
						current_global: global,
					});
				} else if press_monitor != monitor {
					self.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
						monitor: press_monitor,
						press_global,
						click_target,
						release_global,
						released: false,
					});
				}
			},
			LiveCaptureInteraction::DraggingSelection {
				monitor: drag_monitor,
				press_global,
				..
			} => {
				self.set_live_capture_interaction(LiveCaptureInteraction::DraggingSelection {
					monitor: drag_monitor,
					press_global,
					current_global: global,
				});
			},
			_ => {},
		}
	}

	fn frozen_capture_rect_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.frozen_capture_source != FrozenCaptureSource::DragRegion
			|| self.scroll_capture.active
			|| !self.frozen_display_ready()
		{
			return None;
		}

		let monitor = self.state.monitor?;
		let capture_rect = self.state.frozen_capture_rect?;

		if capture_rect.is_empty() {
			return None;
		}

		Some((monitor, capture_rect))
	}

	pub(super) fn frozen_selection_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		(self.toolbar_state.selected_tool == FrozenToolbarTool::Pointer)
			.then(|| self.frozen_capture_rect_drag_target())
			.flatten()
	}

	pub(super) fn frozen_auto_center_available(&self) -> bool {
		self.frozen_capture_rect_drag_target().is_some()
	}

	fn frozen_mosaic_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.scroll_capture.active
			|| !self.frozen_display_ready()
			|| !self.frozen_final_capture_ready()
			|| self.toolbar_state.selected_tool != FrozenToolbarTool::Mosaic
		{
			return None;
		}

		let monitor = self.state.monitor?;
		let capture_rect = self.state.frozen_capture_rect?;

		if capture_rect.is_empty() {
			return None;
		}

		Some((monitor, capture_rect))
	}

	fn frozen_arrow_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		(self.toolbar_state.selected_tool == FrozenToolbarTool::Arrow)
			.then(|| self.frozen_brush_capture_target())
			.flatten()
	}

	fn frozen_spotlight_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		(self.toolbar_state.selected_tool == FrozenToolbarTool::Spotlight)
			.then(|| self.frozen_brush_capture_target())
			.flatten()
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

	pub(super) fn begin_frozen_selection_drag(&mut self, global: GlobalPoint) -> bool {
		let Some((monitor, capture_rect)) = self.frozen_selection_drag_target() else {
			return false;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(global) else {
			return false;
		};
		let Some(interaction) =
			Self::frozen_selection_interaction_kind(capture_rect, cursor_x, cursor_y)
		else {
			return false;
		};

		self.frozen_selection_drag = FrozenSelectionDragState {
			active: true,
			interaction,
			anchor_rect: capture_rect,
			pointer_offset_x: cursor_x.saturating_sub(capture_rect.x),
			pointer_offset_y: cursor_y.saturating_sub(capture_rect.y),
			press_cursor_x: cursor_x,
			press_cursor_y: cursor_y,
		};

		self.hide_auxiliary_windows_for_frozen_selection_drag();

		true
	}

	pub(super) fn begin_frozen_mosaic_drag(&mut self, global: GlobalPoint) -> bool {
		let Some((monitor, capture_rect)) = self.frozen_mosaic_drag_target() else {
			return false;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(global) else {
			return false;
		};

		if !capture_rect.contains((cursor_x, cursor_y)) {
			return false;
		}

		self.frozen_mosaic_drag =
			FrozenMosaicDragState { active: true, anchor_x: cursor_x, anchor_y: cursor_y };
		self.state.frozen_mosaic_preview_rect = Some(RectPoints::new(cursor_x, cursor_y, 1, 1));

		true
	}

	pub(super) fn frozen_selection_interaction_kind(
		capture_rect: RectPoints,
		cursor_x: u32,
		cursor_y: u32,
	) -> Option<FrozenSelectionInteractionKind> {
		if let Some(corner) = WindowRenderer::frozen_selection_resize_hit_test(
			capture_rect,
			Pos2::new(cursor_x as f32, cursor_y as f32),
		) {
			return Some(FrozenSelectionInteractionKind::Resize(corner));
		}

		capture_rect.contains((cursor_x, cursor_y)).then_some(FrozenSelectionInteractionKind::Move)
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
		let interior_reach_x = (selection_rect.width() * 0.35)
			.min(FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS);
		let interior_reach_y = (selection_rect.height() * 0.35)
			.min(FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS);
		let mut x_edges = vec![
			selection_rect.min.x,
			selection_rect.min.x + interior_reach_x,
			selection_rect.center().x,
			selection_rect.max.x - interior_reach_x,
			selection_rect.max.x,
		];
		let mut y_edges = vec![
			selection_rect.min.y,
			selection_rect.min.y + interior_reach_y,
			selection_rect.center().y,
			selection_rect.max.y - interior_reach_y,
			selection_rect.max.y,
		];

		for handle in WindowRenderer::frozen_selection_resize_handles(capture_rect) {
			x_edges.push(handle.hit_rect.min.x);
			x_edges.push(handle.hit_rect.max.x);
			y_edges.push(handle.hit_rect.min.y);
			y_edges.push(handle.hit_rect.max.y);
		}

		super::sort_unique_axis_values(&mut x_edges);
		super::sort_unique_axis_values(&mut y_edges);

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
					adjusted_min.x = super::trim_rect_min_edge(adjusted_min.x, adjusted_max.x);
				}
				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(rect.max.x, point.y),
				) != Some(corner)
				{
					adjusted_max.x = super::trim_rect_max_edge(adjusted_max.x, adjusted_min.x);
				}
				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(point.x, rect.min.y),
				) != Some(corner)
				{
					adjusted_min.y = super::trim_rect_min_edge(adjusted_min.y, adjusted_max.y);
				}
				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(point.x, rect.max.y),
				) != Some(corner)
				{
					adjusted_max.y = super::trim_rect_max_edge(adjusted_max.y, adjusted_min.y);
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

	pub(super) fn frozen_selection_drag_hides_auxiliary_windows(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen) && self.frozen_selection_drag.active
	}

	pub(super) fn live_capture_hides_auxiliary_windows(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Live)
			&& (self.live_capture_interaction_is_press_pending()
				|| self.live_capture_interaction_is_dragging()
				|| self.live_capture_interaction_is_frozen_handoff()
				|| self.frozen_display_handoff_pending())
	}

	fn hide_auxiliary_windows_for_frozen_selection_drag(&mut self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			hud_window.window.set_visible(false);
		}

		self.hud_window_visible = false;

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
		}

		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();

		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			toolbar_window.window.set_visible(false);
		}

		self.skip_toolbar_focus_on_next_show = true;
		self.toolbar_window_visible = false;
		self.toolbar_window_drawn_once = false;
		self.toolbar_badge_slot_ready = false;
		self.toolbar_window_warmup_redraws_remaining = 0;

		if let Some(preview_window) = self.scroll_preview_window.as_ref() {
			preview_window.window.set_visible(false);
		}

		self.last_present_at = Instant::now();
	}

	fn hide_auxiliary_windows_for_live_capture(&mut self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			hud_window.window.set_visible(false);
		}

		self.hud_window_visible = false;

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
		}

		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();

		self.last_present_at = Instant::now();
	}

	pub(super) fn stop_frozen_selection_drag(&mut self) {
		let was_active = self.frozen_selection_drag.active;

		self.frozen_selection_drag = FrozenSelectionDragState::default();

		if was_active && let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}
	}

	pub(super) fn stop_frozen_mosaic_drag(&mut self) {
		self.frozen_mosaic_drag = FrozenMosaicDragState::default();
		self.state.frozen_mosaic_preview_rect = None;
	}

	pub(super) fn update_frozen_selection_drag_rect(&mut self, global: GlobalPoint) -> bool {
		if !self.frozen_selection_drag.active {
			return false;
		}

		let Some((monitor, _capture_rect)) = self.frozen_selection_drag_target() else {
			self.stop_frozen_selection_drag();

			return false;
		};
		let anchor_rect = self.frozen_selection_drag.anchor_rect;
		let next_rect = match self.frozen_selection_drag.interaction {
			FrozenSelectionInteractionKind::Move => {
				let (cursor_x, cursor_y) = Self::clamped_local_point_in_monitor(monitor, global);
				let desired_x =
					i64::from(cursor_x) - i64::from(self.frozen_selection_drag.pointer_offset_x);
				let desired_y =
					i64::from(cursor_y) - i64::from(self.frozen_selection_drag.pointer_offset_y);

				Self::clamp_frozen_capture_rect_to_monitor(
					monitor,
					anchor_rect.width,
					anchor_rect.height,
					desired_x,
					desired_y,
				)
			},
			FrozenSelectionInteractionKind::Resize(corner) => {
				let (cursor_x, cursor_y) = Self::local_point_in_monitor_space(monitor, global);

				Self::resize_frozen_capture_rect_from_corner(
					monitor,
					anchor_rect,
					corner,
					self.frozen_selection_drag.press_cursor_x,
					self.frozen_selection_drag.press_cursor_y,
					cursor_x,
					cursor_y,
				)
			},
		};

		self.apply_frozen_capture_rect_update(monitor, next_rect)
	}

	pub(super) fn update_frozen_mosaic_drag_rect(&mut self, global: GlobalPoint) -> bool {
		if !self.frozen_mosaic_drag.active {
			return false;
		}

		let Some((monitor, capture_rect)) = self.frozen_mosaic_drag_target() else {
			self.stop_frozen_mosaic_drag();

			return false;
		};
		let (cursor_x, cursor_y) = Self::clamped_local_point_in_rect(monitor, capture_rect, global);
		let next_rect = Self::rect_from_drag_points(
			self.frozen_mosaic_drag.anchor_x,
			self.frozen_mosaic_drag.anchor_y,
			cursor_x,
			cursor_y,
		);

		if self.state.frozen_mosaic_preview_rect == Some(next_rect) {
			return false;
		}

		self.state.frozen_mosaic_preview_rect = Some(next_rect);

		self.request_redraw_for_monitor(monitor);

		true
	}

	pub(super) fn clamped_local_point_in_monitor(
		monitor: MonitorRect,
		global: GlobalPoint,
	) -> (u32, u32) {
		let max_x = i64::from(monitor.width.saturating_sub(1));
		let max_y = i64::from(monitor.height.saturating_sub(1));
		let local_x = (i64::from(global.x) - i64::from(monitor.origin.x)).clamp(0, max_x) as u32;
		let local_y = (i64::from(global.y) - i64::from(monitor.origin.y)).clamp(0, max_y) as u32;

		(local_x, local_y)
	}

	pub(super) fn clamped_local_point_in_rect(
		monitor: MonitorRect,
		capture_rect: RectPoints,
		global: GlobalPoint,
	) -> (u32, u32) {
		let (local_x, local_y) = Self::clamped_local_point_in_monitor(monitor, global);
		let max_x = capture_rect.x.saturating_add(capture_rect.width.saturating_sub(1));
		let max_y = capture_rect.y.saturating_add(capture_rect.height.saturating_sub(1));

		(local_x.clamp(capture_rect.x, max_x), local_y.clamp(capture_rect.y, max_y))
	}

	pub(super) fn rect_from_drag_points(
		anchor_x: u32,
		anchor_y: u32,
		cursor_x: u32,
		cursor_y: u32,
	) -> RectPoints {
		let left = anchor_x.min(cursor_x);
		let top = anchor_y.min(cursor_y);
		let right = anchor_x.max(cursor_x).saturating_add(1);
		let bottom = anchor_y.max(cursor_y).saturating_add(1);

		RectPoints::new(left, top, right.saturating_sub(left), bottom.saturating_sub(top))
	}

	fn local_point_in_monitor_space(monitor: MonitorRect, global: GlobalPoint) -> (i64, i64) {
		(
			i64::from(global.x) - i64::from(monitor.origin.x),
			i64::from(global.y) - i64::from(monitor.origin.y),
		)
	}

	fn clamp_frozen_capture_rect_to_monitor(
		monitor: MonitorRect,
		width: u32,
		height: u32,
		desired_x: i64,
		desired_y: i64,
	) -> RectPoints {
		let max_x = i64::from(monitor.width.saturating_sub(width));
		let max_y = i64::from(monitor.height.saturating_sub(height));
		let x = desired_x.clamp(0, max_x) as u32;
		let y = desired_y.clamp(0, max_y) as u32;

		RectPoints::new(x, y, width, height)
	}

	fn resize_frozen_capture_rect_from_corner(
		monitor: MonitorRect,
		anchor_rect: RectPoints,
		corner: FrozenSelectionCorner,
		press_cursor_x: u32,
		press_cursor_y: u32,
		cursor_x: i64,
		cursor_y: i64,
	) -> RectPoints {
		let left = i64::from(anchor_rect.x);
		let top = i64::from(anchor_rect.y);
		let right = i64::from(anchor_rect.x.saturating_add(anchor_rect.width));
		let bottom = i64::from(anchor_rect.y.saturating_add(anchor_rect.height));
		let delta_x = cursor_x - i64::from(press_cursor_x);
		let delta_y = cursor_y - i64::from(press_cursor_y);
		let monitor_width = i64::from(monitor.width);
		let monitor_height = i64::from(monitor.height);

		match corner {
			FrozenSelectionCorner::TopLeft => {
				let next_left = (left + delta_x).clamp(0, right.saturating_sub(1)) as u32;
				let next_top = (top + delta_y).clamp(0, bottom.saturating_sub(1)) as u32;

				RectPoints::new(
					next_left,
					next_top,
					(right as u32).saturating_sub(next_left),
					(bottom as u32).saturating_sub(next_top),
				)
			},
			FrozenSelectionCorner::TopRight => {
				let next_right =
					(right + delta_x).clamp(left.saturating_add(1), monitor_width) as u32;
				let next_top = (top + delta_y).clamp(0, bottom.saturating_sub(1)) as u32;

				RectPoints::new(
					left as u32,
					next_top,
					next_right.saturating_sub(left as u32),
					(bottom as u32).saturating_sub(next_top),
				)
			},
			FrozenSelectionCorner::BottomLeft => {
				let next_left = (left + delta_x).clamp(0, right.saturating_sub(1)) as u32;
				let next_bottom =
					(bottom + delta_y).clamp(top.saturating_add(1), monitor_height) as u32;

				RectPoints::new(
					next_left,
					top as u32,
					(right as u32).saturating_sub(next_left),
					next_bottom.saturating_sub(top as u32),
				)
			},
			FrozenSelectionCorner::BottomRight => {
				let next_right =
					(right + delta_x).clamp(left.saturating_add(1), monitor_width) as u32;
				let next_bottom =
					(bottom + delta_y).clamp(top.saturating_add(1), monitor_height) as u32;

				RectPoints::new(
					left as u32,
					top as u32,
					next_right.saturating_sub(left as u32),
					next_bottom.saturating_sub(top as u32),
				)
			},
		}
	}

	fn apply_frozen_capture_rect_update(
		&mut self,
		monitor: MonitorRect,
		next_rect: RectPoints,
	) -> bool {
		if self.state.frozen_capture_rect == Some(next_rect) {
			return false;
		}

		self.state.frozen_capture_rect = Some(next_rect);

		let toolbar_default_pos =
			self.frozen_toolbar_default_position_for_capture_rect(monitor, next_rect);
		let toolbar_pos = match (
			self.toolbar_state.floating_position,
			self.toolbar_state.default_slot_position,
		) {
			(Some(floating_pos), Some(default_pos))
				if !super::frozen_toolbar_matches_default_slot(floating_pos, default_pos) =>
			{
				floating_pos
			},
			_ => toolbar_default_pos,
		};

		self.toolbar_state.default_slot_position = Some(toolbar_default_pos);
		self.toolbar_state.floating_position = Some(toolbar_pos);

		let should_trace_frozen_selection_drag_timing =
			self.should_trace_frozen_selection_drag_timing();
		let toolbar_position_elapsed: Option<Duration> =
			if should_trace_frozen_selection_drag_timing {
				let toolbar_position_started_at = Instant::now();
				let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);

				Some(toolbar_position_started_at.elapsed())
			} else {
				let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);

				None
			};
		let redraw_request_elapsed: Option<Duration> = if should_trace_frozen_selection_drag_timing
		{
			let redraw_request_started_at = Instant::now();

			self.request_redraw_for_monitor(monitor);
			self.request_redraw_toolbar_window();

			if self.scroll_capture.active {
				self.request_redraw_scroll_preview_window();
			}

			Some(redraw_request_started_at.elapsed())
		} else {
			self.request_redraw_for_monitor(monitor);
			self.request_redraw_toolbar_window();

			if self.scroll_capture.active {
				self.request_redraw_scroll_preview_window();
			}

			None
		};

		if should_trace_frozen_selection_drag_timing {
			tracing::trace!(
				op = "overlay.frozen_selection_drag.rect_update",
				monitor_id = monitor.id,
				rect_x = next_rect.x,
				rect_y = next_rect.y,
				rect_width = next_rect.width,
				rect_height = next_rect.height,
				toolbar_position_us = toolbar_position_elapsed.map_or(0, |elapsed| elapsed.as_micros()),
				redraw_request_us = redraw_request_elapsed.map_or(0, |elapsed| elapsed.as_micros()),
				scroll_capture_active = self.scroll_capture.active,
				toolbar_outer_pos = ?self.toolbar_outer_pos,
				toolbar_floating_position = ?self.toolbar_state.floating_position,
				"Applied frozen selection rect update."
			);
		}

		true
	}

	pub(super) fn auto_center_frozen_capture_rect(&mut self) -> bool {
		let Some((monitor, capture_rect)) = self.frozen_capture_rect_drag_target() else {
			return false;
		};
		let Some(capture_image) = self.cropped_frozen_capture_image() else {
			return false;
		};
		let Some(content_bounds) = Self::detect_auto_center_content_bounds(&capture_image) else {
			return false;
		};
		let delta_x_points = Self::auto_center_shift_points(
			content_bounds.x,
			content_bounds.width,
			capture_image.width(),
			capture_rect.width,
		);
		let delta_y_points = Self::auto_center_shift_points(
			content_bounds.y,
			content_bounds.height,
			capture_image.height(),
			capture_rect.height,
		);
		let next_rect = Self::clamp_frozen_capture_rect_to_monitor(
			monitor,
			capture_rect.width,
			capture_rect.height,
			i64::from(capture_rect.x) + delta_x_points,
			i64::from(capture_rect.y) + delta_y_points,
		);

		self.apply_frozen_capture_rect_update(monitor, next_rect)
	}

	fn auto_center_shift_points(
		content_origin_px: u32,
		content_size_px: u32,
		crop_size_px: u32,
		capture_size_points: u32,
	) -> i64 {
		if crop_size_px == 0 || capture_size_points == 0 {
			return 0;
		}

		let content_center_px = content_origin_px as f32 + (content_size_px as f32 * 0.5);
		let crop_center_px = crop_size_px as f32 * 0.5;
		let delta_px = content_center_px - crop_center_px;

		((delta_px * capture_size_points as f32) / crop_size_px as f32).round() as i64
	}

	fn detect_auto_center_content_bounds(image: &RgbaImage) -> Option<RectPoints> {
		let width = image.width();
		let height = image.height();

		if width < 2 || height < 2 {
			return None;
		}

		let edge_strip = Self::auto_center_edge_strip_extent(width.min(height));
		let top_mean = Self::region_rgb_mean(image, 0, width, 0, edge_strip)?;
		let bottom_mean =
			Self::region_rgb_mean(image, 0, width, height.saturating_sub(edge_strip), height)?;
		let left_mean = Self::region_rgb_mean(image, 0, edge_strip, 0, height)?;
		let right_mean =
			Self::region_rgb_mean(image, width.saturating_sub(edge_strip), width, 0, height)?;
		let threshold = {
			let edge_noise = [
				Self::region_rgb_mean_distance(image, 0, width, 0, edge_strip, top_mean),
				Self::region_rgb_mean_distance(
					image,
					0,
					width,
					height.saturating_sub(edge_strip),
					height,
					bottom_mean,
				),
				Self::region_rgb_mean_distance(image, 0, edge_strip, 0, height, left_mean),
				Self::region_rgb_mean_distance(
					image,
					width.saturating_sub(edge_strip),
					width,
					0,
					height,
					right_mean,
				),
			]
			.into_iter()
			.fold(0.0, f32::max);

			(edge_noise * 3.0).round().clamp(24.0, 96.0) as u32
		};
		let min_salient_per_row = (width / 64).max(1) as usize;
		let min_salient_per_column = (height / 64).max(1) as usize;
		let mut row_counts = vec![0_usize; height as usize];
		let mut column_counts = vec![0_usize; width as usize];

		for (x, y, pixel) in image.enumerate_pixels() {
			let salient_distance = [
				Self::rgb_distance_to_mean(pixel, top_mean),
				Self::rgb_distance_to_mean(pixel, bottom_mean),
				Self::rgb_distance_to_mean(pixel, left_mean),
				Self::rgb_distance_to_mean(pixel, right_mean),
			]
			.into_iter()
			.min()
			.unwrap_or(0);

			if salient_distance < threshold {
				continue;
			}

			row_counts[y as usize] += 1;
			column_counts[x as usize] += 1;
		}

		let top = row_counts.iter().position(|count| *count >= min_salient_per_row)?;
		let bottom = row_counts.iter().rposition(|count| *count >= min_salient_per_row)?;
		let left = column_counts.iter().position(|count| *count >= min_salient_per_column)?;
		let right = column_counts.iter().rposition(|count| *count >= min_salient_per_column)?;

		if left > right || top > bottom {
			return None;
		}

		let bounds = RectPoints::new(
			left as u32,
			top as u32,
			(right - left + 1) as u32,
			(bottom - top + 1) as u32,
		);
		let fills_crop_width = bounds.width.saturating_mul(100) >= width.saturating_mul(92);
		let fills_crop_height = bounds.height.saturating_mul(100) >= height.saturating_mul(92);

		if fills_crop_width && fills_crop_height {
			return None;
		}

		Some(bounds)
	}

	fn auto_center_edge_strip_extent(length: u32) -> u32 {
		((length as f32) * 0.08).round().clamp(1.0, 24.0) as u32
	}

	fn region_rgb_mean(image: &RgbaImage, x0: u32, x1: u32, y0: u32, y1: u32) -> Option<[f32; 3]> {
		if x0 >= x1 || y0 >= y1 {
			return None;
		}

		let mut r_total = 0_u64;
		let mut g_total = 0_u64;
		let mut b_total = 0_u64;
		let mut sample_count = 0_u64;

		for y in y0..y1 {
			for x in x0..x1 {
				let pixel = image.get_pixel(x, y);

				r_total += u64::from(pixel[0]);
				g_total += u64::from(pixel[1]);
				b_total += u64::from(pixel[2]);
				sample_count += 1;
			}
		}

		if sample_count == 0 {
			return None;
		}

		Some([
			r_total as f32 / sample_count as f32,
			g_total as f32 / sample_count as f32,
			b_total as f32 / sample_count as f32,
		])
	}

	fn region_rgb_mean_distance(
		image: &RgbaImage,
		x0: u32,
		x1: u32,
		y0: u32,
		y1: u32,
		mean: [f32; 3],
	) -> f32 {
		if x0 >= x1 || y0 >= y1 {
			return 0.0;
		}

		let mut total_distance = 0_u64;
		let mut sample_count = 0_u64;

		for y in y0..y1 {
			for x in x0..x1 {
				total_distance +=
					u64::from(Self::rgb_distance_to_mean(image.get_pixel(x, y), mean));
				sample_count += 1;
			}
		}

		if sample_count == 0 { 0.0 } else { total_distance as f32 / sample_count as f32 }
	}

	fn rgb_distance_to_mean(pixel: &Rgba<u8>, mean: [f32; 3]) -> u32 {
		(pixel[0] as f32 - mean[0]).abs().round() as u32
			+ (pixel[1] as f32 - mean[1]).abs().round() as u32
			+ (pixel[2] as f32 - mean[2]).abs().round() as u32
	}
}
