#[cfg(target_os = "macos")]
use crate::overlay::Arc;
use crate::overlay::toolbar_geometry::TOOLBAR_DRAG_START_THRESHOLD_PX;
#[cfg(target_os = "macos")]
use crate::overlay::toolbar_layout_model;
use crate::overlay::{
	GlobalPoint, HudOverlayWindow, Instant, MonitorRect, OverlayControl, OverlayMode,
	OverlaySession, PhysicalPosition, Pos2, Vec2, WindowId,
};

impl OverlaySession {
	pub(in crate::overlay) fn handle_toolbar_cursor_left(&mut self) -> OverlayControl {
		self.toolbar_state.annotation_size_control_hovered = false;
		self.toolbar_state.annotation_size_wheel_accumulator = 0.0;

		if !self.toolbar_left_button_down && !self.toolbar_state.dragging {
			self.toolbar_pointer_local = None;
			self.toolbar_left_button_went_down = false;
			self.toolbar_left_button_went_up = false;
			self.toolbar_state.drag_offset = Vec2::ZERO;
			self.toolbar_state.drag_start_eligible = false;
			self.toolbar_state.drag_anchor = None;
		}

		#[cfg(target_os = "macos")]
		{
			self.sync_toolbar_window_cursor_hittest(None);
			self.request_redraw_toolbar_window();
		}

		OverlayControl::Continue
	}

	pub(in crate::overlay) fn note_frozen_toolbar_cursor_event(
		&mut self,
		monitor: MonitorRect,
		global_cursor: GlobalPoint,
	) {
		let old_monitor = self.active_cursor_monitor();
		let old_cursor = self.state.cursor;

		self.last_event_cursor = Some((monitor, global_cursor));
		self.last_event_cursor_at = Some(Instant::now());

		if old_monitor == Some(monitor) && old_cursor == Some(global_cursor) {
			return;
		}

		self.update_cursor_state(monitor, global_cursor);

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}
	}

	pub(in crate::overlay) fn handle_toolbar_cursor_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
	) -> OverlayControl {
		let Some((toolbar_window_id, toolbar_scale, window_toolbar_outer_pos)) =
			self.toolbar_window.as_ref().map(|toolbar_window| {
				(
					toolbar_window.window.id(),
					toolbar_window.window.scale_factor().max(1.0),
					Self::toolbar_window_outer_position(toolbar_window),
				)
			})
		else {
			return OverlayControl::Continue;
		};

		if toolbar_window_id != window_id
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
		{
			return OverlayControl::Continue;
		}

		let cursor_local =
			Pos2::new((position.x / toolbar_scale) as f32, (position.y / toolbar_scale) as f32);
		let cached_toolbar_outer_pos = self.toolbar_outer_pos;
		let monitor = match self.state.monitor.or_else(|| self.active_cursor_monitor()) {
			Some(monitor) => monitor,
			None => return OverlayControl::Continue,
		};
		let toolbar_outer_pos = Self::toolbar_event_outer_position_from_sources(
			monitor,
			window_toolbar_outer_pos,
			cached_toolbar_outer_pos,
			self.toolbar_state.floating_position,
		);

		self.handle_toolbar_pointer_moved_from_positions(
			monitor,
			cursor_local,
			toolbar_outer_pos
				.map(|outer| Self::toolbar_cursor_global_position_from_outer(outer, cursor_local)),
			cached_toolbar_outer_pos,
			window_toolbar_outer_pos,
		)
	}

	fn handle_toolbar_cursor_move_for_active_selection(
		&mut self,
		global_cursor: Option<GlobalPoint>,
	) -> bool {
		if self.frozen_selection_drag.active {
			if let Some(global_cursor) = global_cursor {
				self.update_frozen_selection_drag_rect(global_cursor);
				self.update_frozen_mosaic_drag_rect(global_cursor);
			}

			return true;
		}
		if self.frozen_arrow_drag.active {
			if let Some(global_cursor) = global_cursor {
				self.update_frozen_arrow_drag(global_cursor);
			}

			return true;
		}
		if self.frozen_mosaic_drag.active {
			if let Some(global_cursor) = global_cursor {
				self.update_frozen_mosaic_drag_rect(global_cursor);
			}

			return true;
		}
		if self.frozen_spotlight_drag.active {
			if let Some(global_cursor) = global_cursor {
				self.update_frozen_spotlight_drag_rect(global_cursor);
			}

			return true;
		}

		false
	}

	#[cfg(target_os = "macos")]
	fn begin_native_toolbar_drag(&mut self) -> OverlayControl {
		self.toolbar_state.dragging = true;
		self.toolbar_state.drag_start_eligible = false;
		self.toolbar_state.drag_anchor = None;

		let Some(toolbar_window_handle) =
			self.toolbar_window.as_ref().map(|toolbar_window| Arc::clone(&toolbar_window.window))
		else {
			return OverlayControl::Continue;
		};
		let _ = toolbar_window_handle.drag_window();

		OverlayControl::Continue
	}

	#[cfg(target_os = "macos")]
	pub(in crate::overlay) fn handle_native_toolbar_pointer_moved(
		&mut self,
		monitor: MonitorRect,
		cursor_local: Pos2,
		global_cursor: GlobalPoint,
		toolbar_outer_pos: Option<GlobalPoint>,
	) -> OverlayControl {
		if !matches!(self.state.mode, OverlayMode::Frozen) || !self.toolbar_state.visible {
			return OverlayControl::Continue;
		}

		self.handle_toolbar_pointer_moved_from_positions(
			monitor,
			cursor_local,
			Some(global_cursor),
			self.toolbar_outer_pos,
			toolbar_outer_pos,
		)
	}

	fn handle_toolbar_pointer_moved_from_positions(
		&mut self,
		monitor: MonitorRect,
		cursor_local: Pos2,
		global_cursor: Option<GlobalPoint>,
		cached_toolbar_outer_pos: Option<GlobalPoint>,
		window_toolbar_outer_pos: Option<GlobalPoint>,
	) -> OverlayControl {
		self.toolbar_pointer_local = Some(cursor_local);

		if self.handle_toolbar_cursor_move_for_active_selection(global_cursor) {
			return OverlayControl::Continue;
		}

		self.update_toolbar_cursor_event_from_global(
			monitor,
			cursor_local,
			global_cursor,
			cached_toolbar_outer_pos,
			window_toolbar_outer_pos,
		);

		let drag_monitor =
			global_cursor.and_then(|cursor| self.monitor_at(cursor)).unwrap_or(monitor);
		#[cfg(target_os = "macos")]
		let manual_toolbar_drag = self.should_host_toolbar_pointer_input_in_native_shell();
		#[cfg(not(target_os = "macos"))]
		let manual_toolbar_drag = true;
		let mut mouse_drag = self.toolbar_left_button_down && self.toolbar_state.dragging;

		if self.toolbar_left_button_down
			&& self.toolbar_state.drag_start_eligible
			&& self.toolbar_state.drag_anchor.is_none()
		{
			self.toolbar_state.drag_anchor = Some(cursor_local);
		}
		if !mouse_drag
			&& let Some(drag_anchor) = self.toolbar_state.drag_anchor
			&& Self::toolbar_drag_threshold_reached(cursor_local, drag_anchor)
		{
			if manual_toolbar_drag {
				if let (Some(global_cursor), Some(toolbar_outer_pos)) =
					(global_cursor, window_toolbar_outer_pos.or(cached_toolbar_outer_pos))
				{
					self.toolbar_state.drag_offset = Vec2::new(
						global_cursor.x as f32 - toolbar_outer_pos.x as f32,
						global_cursor.y as f32 - toolbar_outer_pos.y as f32,
					);
					self.toolbar_state.dragging = true;
					self.toolbar_state.drag_start_eligible = false;
					self.toolbar_state.drag_anchor = None;
					mouse_drag = true;
				}
			} else {
				#[cfg(target_os = "macos")]
				{
					return self.begin_native_toolbar_drag();
				}
			}
		}
		if mouse_drag && global_cursor.is_none() {
			mouse_drag = false;
		}
		if mouse_drag && let Some(global_cursor) = global_cursor {
			let desired_global = Pos2::new(
				global_cursor.x as f32 - self.toolbar_state.drag_offset.x,
				global_cursor.y as f32 - self.toolbar_state.drag_offset.y,
			);
			let desired_local = Pos2::new(
				desired_global.x - drag_monitor.origin.x as f32,
				desired_global.y - drag_monitor.origin.y as f32,
			);
			let _ = self.update_toolbar_outer_position(drag_monitor, desired_local);

			self.force_apply_pending_toolbar_window_move();
		}
		#[cfg(target_os = "macos")]
		if !manual_toolbar_drag && self.toolbar_state.dragging {
			return OverlayControl::Continue;
		}

		self.request_redraw_toolbar_window();

		OverlayControl::Continue
	}

	fn update_toolbar_cursor_event_from_global(
		&mut self,
		monitor: MonitorRect,
		cursor_local: Pos2,
		global_cursor: Option<GlobalPoint>,
		cached_toolbar_outer_pos: Option<GlobalPoint>,
		window_toolbar_outer_pos: Option<GlobalPoint>,
	) {
		if let Some(global_cursor) = global_cursor {
			self.maybe_log_suspicious_toolbar_cursor_translation(
				monitor,
				self.state.cursor,
				cursor_local,
				global_cursor,
				cached_toolbar_outer_pos,
				window_toolbar_outer_pos,
			);
			self.note_frozen_toolbar_cursor_event(monitor, global_cursor);
		}
	}

	fn toolbar_drag_threshold_reached(cursor_local: Pos2, drag_anchor: Pos2) -> bool {
		let dx = cursor_local.x - drag_anchor.x;
		let dy = cursor_local.y - drag_anchor.y;
		let threshold_sq = TOOLBAR_DRAG_START_THRESHOLD_PX * TOOLBAR_DRAG_START_THRESHOLD_PX;

		dx * dx + dy * dy >= threshold_sq
	}

	pub(in crate::overlay) fn toolbar_event_outer_position_from_sources(
		monitor: MonitorRect,
		window_toolbar_outer_pos: Option<GlobalPoint>,
		cached_toolbar_outer_pos: Option<GlobalPoint>,
		floating_position: Option<Pos2>,
	) -> Option<GlobalPoint> {
		window_toolbar_outer_pos.or(cached_toolbar_outer_pos).or_else(|| {
			floating_position.map(|floating_position| {
				#[cfg(target_os = "macos")]
				let floating_position = floating_position
					- toolbar_layout_model::frozen_toolbar_window_primary_origin().to_vec2();

				GlobalPoint::new(
					monitor.origin.x.saturating_add(floating_position.x.round() as i32),
					monitor.origin.y.saturating_add(floating_position.y.round() as i32),
				)
			})
		})
	}

	pub(in crate::overlay) fn toolbar_window_outer_position(
		toolbar_window: &HudOverlayWindow,
	) -> Option<GlobalPoint> {
		let toolbar_scale = toolbar_window.window.scale_factor().max(1.0);
		let outer_position = toolbar_window.window.outer_position().ok()?;

		Some(GlobalPoint::new(
			(outer_position.x as f64 / toolbar_scale).round() as i32,
			(outer_position.y as f64 / toolbar_scale).round() as i32,
		))
	}

	pub(in crate::overlay) fn toolbar_cursor_global_position_from_outer(
		outer_position: GlobalPoint,
		cursor_local: Pos2,
	) -> GlobalPoint {
		let global_cursor = Pos2::new(
			outer_position.x as f32 + cursor_local.x,
			outer_position.y as f32 + cursor_local.y,
		);

		GlobalPoint::new(global_cursor.x.round() as i32, global_cursor.y.round() as i32)
	}

	fn toolbar_cursor_translation_suspicious(
		monitor: MonitorRect,
		old_cursor: Option<GlobalPoint>,
		global_cursor: GlobalPoint,
	) -> bool {
		if !monitor.contains(global_cursor) {
			return true;
		}

		let Some(old_cursor) = old_cursor else {
			return false;
		};
		let delta_x = old_cursor.x.abs_diff(global_cursor.x);
		let delta_y = old_cursor.y.abs_diff(global_cursor.y);
		let jumps_far = delta_x > 160 || delta_y > 160;
		let snaps_to_origin = global_cursor.x <= monitor.origin.x + 8
			&& global_cursor.y <= monitor.origin.y + 8
			&& (old_cursor.x > monitor.origin.x + 32 || old_cursor.y > monitor.origin.y + 32);

		jumps_far || snaps_to_origin
	}

	fn maybe_log_suspicious_toolbar_cursor_translation(
		&self,
		monitor: MonitorRect,
		old_cursor: Option<GlobalPoint>,
		cursor_local: Pos2,
		global_cursor: GlobalPoint,
		cached_toolbar_outer_pos: Option<GlobalPoint>,
		window_toolbar_outer_pos: Option<GlobalPoint>,
	) {
		if self.frozen_selection_drag.active
			|| self.frozen_mosaic_drag.active
			|| self.toolbar_state.dragging
			|| !Self::toolbar_cursor_translation_suspicious(monitor, old_cursor, global_cursor)
		{
			return;
		}

		tracing::warn!(
			op = "overlay.toolbar_cursor_translation_suspicious",
			monitor_id = monitor.id,
			old_cursor = ?old_cursor,
			cursor_local = ?cursor_local,
			global_cursor = ?global_cursor,
			cached_toolbar_outer_pos = ?cached_toolbar_outer_pos,
			window_toolbar_outer_pos = ?window_toolbar_outer_pos,
			pending_toolbar_outer_pos = ?self.pending_toolbar_outer_pos,
			toolbar_floating_position = ?self.toolbar_state.floating_position,
			"Toolbar cursor translation jumped unexpectedly."
		);
	}
}
