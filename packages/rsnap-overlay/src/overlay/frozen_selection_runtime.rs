use std::time::{Duration, Instant};

use crate::overlay::frozen_selection_geometry::LIVE_DRAG_START_THRESHOLD_PX;
use crate::overlay::live_capture_target::LiveClickCaptureTarget;
use crate::overlay::toolbar_layout_model;
use crate::overlay::{
	FrozenCaptureSource, FrozenMosaicDragState, FrozenSelectionCorner, FrozenSelectionDragState,
	FrozenSelectionInteractionKind, FrozenToolbarTool, GlobalPoint, LiveCaptureInteraction,
	MonitorRect, MonitorRectPoints, OverlayMode, OverlaySession, Pos2, RectPoints, WindowRenderer,
};

impl OverlaySession {
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
			rect.width as f32 >= LIVE_DRAG_START_THRESHOLD_PX
				&& rect.height as f32 >= LIVE_DRAG_START_THRESHOLD_PX
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

	pub(super) fn frozen_capture_rect_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
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

	pub(super) fn frozen_mosaic_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
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

	pub(super) fn frozen_arrow_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		(self.toolbar_state.selected_tool == FrozenToolbarTool::Arrow)
			.then(|| self.frozen_brush_capture_target())
			.flatten()
	}

	pub(super) fn frozen_spotlight_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		(self.toolbar_state.selected_tool == FrozenToolbarTool::Spotlight)
			.then(|| self.frozen_brush_capture_target())
			.flatten()
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

	pub(super) fn clamp_frozen_capture_rect_to_monitor(
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

	pub(super) fn apply_frozen_capture_rect_update(
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
				if !toolbar_layout_model::frozen_toolbar_matches_default_slot(
					floating_pos,
					default_pos,
				) =>
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
}
