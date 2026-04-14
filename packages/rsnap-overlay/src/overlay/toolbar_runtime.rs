use crate::overlay::{
	self, Arc, Duration, FrozenToolbarPointerState, GlobalPoint, HudOverlayWindow, Instant,
	MonitorRect, OverlayControl, OverlayEventLoopPhase, OverlayExit, OverlayMode, OverlaySession,
	PhysicalPosition, PhysicalSize, Pos2, Result, TOOLBAR_DRAG_START_THRESHOLD_PX, Vec2, WindowId,
	WindowRenderer,
};
#[cfg(target_os = "macos")]
use crate::overlay::{FrozenCaptureSource, HudAnchor, TOOLBAR_WINDOW_WARMUP_REDRAWS};

impl OverlaySession {
	pub(super) fn handle_toolbar_window_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<i32>,
	) -> OverlayControl {
		let Some((toolbar_window_id, toolbar_scale)) =
			self.toolbar_window.as_ref().map(|toolbar_window| {
				(toolbar_window.window.id(), toolbar_window.window.scale_factor().max(1.0))
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

		let Some(monitor) = self.state.monitor.or_else(|| self.active_cursor_monitor()) else {
			return OverlayControl::Continue;
		};
		let outer_position = GlobalPoint::new(
			(f64::from(position.x) / toolbar_scale).round() as i32,
			(f64::from(position.y) / toolbar_scale).round() as i32,
		);
		let changed = self.sync_toolbar_outer_position_from_window(monitor, outer_position);

		#[cfg(target_os = "macos")]
		self.sync_toolbar_window_cursor_hittest(super::macos_mouse_location());

		if self.pending_toolbar_outer_pos.is_some() {
			self.force_apply_pending_toolbar_window_move();
		} else {
			self.last_toolbar_window_move_at = Instant::now();
		}
		if changed {
			self.request_redraw_toolbar_window();
		}

		OverlayControl::Continue
	}

	pub(super) fn handle_toolbar_cursor_left(&mut self) -> OverlayControl {
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

	pub(super) fn note_frozen_toolbar_cursor_event(
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

	pub(super) fn handle_toolbar_cursor_moved(
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

		self.toolbar_pointer_local = Some(cursor_local);

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
		let global_cursor = toolbar_outer_pos
			.map(|outer| Self::toolbar_cursor_global_position_from_outer(outer, cursor_local));

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

		#[cfg(not(target_os = "macos"))]
		let drag_monitor = global_cursor.and_then(|cursor| self.monitor_at(cursor)).unwrap_or(monitor);
		#[cfg(target_os = "macos")]
		let mouse_drag = self.toolbar_left_button_down && self.toolbar_state.dragging;
		#[cfg(not(target_os = "macos"))]
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
			#[cfg(target_os = "macos")]
			{
				return self.begin_native_toolbar_drag();
			}

			#[cfg(not(target_os = "macos"))]
			if let (Some(global_cursor), Some(toolbar_outer_pos)) =
				(global_cursor, toolbar_outer_pos)
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
		}
		#[cfg(not(target_os = "macos"))]
		if mouse_drag && global_cursor.is_none() {
			mouse_drag = false;
		}
		#[cfg(not(target_os = "macos"))]
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
		}
		#[cfg(target_os = "macos")]
		if self.toolbar_state.dragging {
			return OverlayControl::Continue;
		}

		self.request_redraw_toolbar_window();

		OverlayControl::Continue
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
		if self.frozen_mosaic_drag.active {
			if let Some(global_cursor) = global_cursor {
				self.update_frozen_mosaic_drag_rect(global_cursor);
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

	pub(super) fn toolbar_event_outer_position_from_sources(
		monitor: MonitorRect,
		window_toolbar_outer_pos: Option<GlobalPoint>,
		cached_toolbar_outer_pos: Option<GlobalPoint>,
		floating_position: Option<Pos2>,
	) -> Option<GlobalPoint> {
		window_toolbar_outer_pos.or(cached_toolbar_outer_pos).or_else(|| {
			floating_position.map(|floating_position| {
				GlobalPoint::new(
					monitor.origin.x.saturating_add(floating_position.x.round() as i32),
					monitor.origin.y.saturating_add(floating_position.y.round() as i32),
				)
			})
		})
	}

	pub(super) fn toolbar_window_outer_position(
		toolbar_window: &HudOverlayWindow,
	) -> Option<GlobalPoint> {
		let toolbar_scale = toolbar_window.window.scale_factor().max(1.0);
		let outer_position = toolbar_window.window.outer_position().ok()?;

		Some(GlobalPoint::new(
			(outer_position.x as f64 / toolbar_scale).round() as i32,
			(outer_position.y as f64 / toolbar_scale).round() as i32,
		))
	}

	pub(super) fn toolbar_cursor_global_position_from_outer(
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

	pub(super) fn handle_toolbar_window_resized(
		&mut self,
		size: PhysicalSize<u32>,
	) -> OverlayControl {
		let Some(toolbar_window) = self.toolbar_window.as_mut() else {
			return OverlayControl::Continue;
		};

		match toolbar_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	pub(super) fn handle_toolbar_window_scale_factor_changed(
		&mut self,
		window_id: WindowId,
	) -> OverlayControl {
		let Some(toolbar_window) = self
			.toolbar_window
			.as_mut()
			.filter(|toolbar_window| toolbar_window.window.id() == window_id)
		else {
			return OverlayControl::Continue;
		};
		let size = toolbar_window.window.inner_size();

		match toolbar_window.renderer.resize(size) {
			Ok(()) => {
				let window = Arc::clone(&toolbar_window.window);
				let toolbar_height_points = self
					.toolbar_inner_size_points
					.map(|(_, height)| height as f32)
					.unwrap_or_else(|| super::frozen_toolbar_window_startup_size_points().y);

				self.configure_hud_window_common(
					window.as_ref(),
					Some(overlay::frozen_toolbar_corner_radius_points(toolbar_height_points)),
				);

				OverlayControl::Continue
			},
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	pub(super) fn should_hide_toolbar_window(&self, monitor: MonitorRect) -> bool {
		self.frozen_selection_drag_hides_auxiliary_windows()
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
			|| self.state.frozen_image.is_none()
			|| self.state.monitor != Some(monitor)
	}

	#[cfg(any(target_os = "macos", test))]
	pub(super) fn should_focus_frozen_toolbar_window_on_show(&self) -> bool {
		!self.toolbar_window_visible
			&& !self.skip_toolbar_focus_on_next_show
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& !self.scroll_capture.active
	}

	pub(super) fn set_toolbar_window_hidden(&mut self) {
		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			#[cfg(target_os = "macos")]
			let _ = toolbar_window.window.set_cursor_hittest(false);

			toolbar_window.window.set_visible(false);
		}

		self.toolbar_window_visible = false;
		#[cfg(target_os = "macos")]
		{
			self.toolbar_window_cursor_hittest_enabled = false;
		}
		self.toolbar_window_warmup_redraws_remaining = 0;
		self.last_present_at = Instant::now();
	}

	pub(super) fn draw_toolbar_window_frame(
		&mut self,
		monitor: MonitorRect,
		toolbar_input: Option<FrozenToolbarPointerState>,
	) -> Result<()> {
		self.sync_frozen_toolbar_state();

		if self.maybe_recenter_frozen_toolbar_default_slot(monitor) {
			self.request_redraw_for_monitor(monitor);
		}

		#[cfg(not(target_os = "macos"))]
		{
			let _ = (&monitor, &toolbar_input);
			let Some(toolbar_window) = self.toolbar_window.as_ref() else {
				return Ok(());
			};

			toolbar_window.window.set_visible(false);

			self.last_present_at = Instant::now();

			Ok(())
		}
		#[cfg(target_os = "macos")]
		{
			let Some(toolbar_became_visible) = self.prepare_toolbar_window_for_draw(monitor) else {
				return Ok(());
			};
			let Some(gpu) = self.gpu.as_ref() else {
				return Ok(());
			};
			let previous_floating_position = self.toolbar_state.floating_position;

			self.toolbar_state.floating_position = Some(Pos2::ZERO);

			let Some(toolbar_window) = self.toolbar_window.as_mut() else {
				return Ok(());
			};
			let draw_result = toolbar_window.renderer.draw(
				gpu,
				&self.state,
				monitor,
				false,
				Some(Pos2::ZERO),
				false,
				HudAnchor::Cursor,
				self.config.toolbar_placement,
				self.config.show_alt_hint_keycap,
				false,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
				self.config.selection_flow_enabled,
				self.config.selection_flow_stroke_width_px,
				false,
				false,
				false,
				self.frozen_capture_source,
				self.frozen_capture_source == FrozenCaptureSource::FullscreenFallback,
				None,
				&[],
				None,
				&self.frozen_text_annotations,
				self.frozen_text_edit.as_ref(),
				self.toolbar_state.text_style,
				Some(&mut self.toolbar_state),
				toolbar_input,
			);

			self.toolbar_state.floating_position = previous_floating_position;

			draw_result?;

			if toolbar_became_visible {
				self.note_frozen_transition_toolbar_visible(monitor);
			}

			Ok(())
		}
	}

	#[cfg(target_os = "macos")]
	fn prepare_toolbar_window_for_draw(&mut self, monitor: MonitorRect) -> Option<bool> {
		let should_focus_frozen_keyboard = self.should_focus_frozen_toolbar_window_on_show();

		if !self.toolbar_window_visible {
			self.maybe_apply_pending_startup_aux_live_stream_filter_upgrade(monitor);
		}

		let toolbar_window = self.toolbar_window.as_ref()?;

		toolbar_window.window.set_visible(true);

		let mut toolbar_became_visible = false;

		if !self.toolbar_window_visible {
			self.toolbar_window_visible = true;
			self.skip_toolbar_focus_on_next_show = false;
			self.toolbar_window_warmup_redraws_remaining = TOOLBAR_WINDOW_WARMUP_REDRAWS;
			toolbar_became_visible = true;
		}
		if should_focus_frozen_keyboard {
			self.focus_frozen_keyboard_window();
		}

		Some(toolbar_became_visible)
	}
	pub(super) fn handle_toolbar_window_redraw_requested(&mut self) -> OverlayControl {
		let redraw_started_at = Instant::now();

		self.event_loop_last_progress_window_id =
			self.toolbar_window.as_ref().map(|toolbar_window| toolbar_window.window.id());
		self.event_loop_last_progress_monitor_id = self.state.monitor.map(|monitor| monitor.id);

		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::ToolbarRedraw);

		let Some(monitor) = self.state.monitor else {
			return OverlayControl::Continue;
		};
		let toolbar_input = self.toolbar_pointer_state(monitor, self.toolbar_pointer_local);
		let should_hide_toolbar_window = self.should_hide_toolbar_window(monitor);

		if should_hide_toolbar_window {
			self.set_toolbar_window_hidden();

			return OverlayControl::Continue;
		}

		let draw_frame_started_at = Instant::now();

		if let Err(err) = self.draw_toolbar_window_frame(monitor, toolbar_input) {
			return self.exit(OverlayExit::Error(format!("{err:#}")));
		}

		if self.sync_frozen_text_edit_for_selected_tool() {
			self.request_redraw_for_monitor(monitor);
		}

		self.sync_overlay_cursor_icons();

		let draw_frame_elapsed = draw_frame_started_at.elapsed();

		self.update_scroll_toolbar_default_position(monitor);

		let position_update_elapsed = self.update_toolbar_position_after_redraw(monitor);

		#[cfg(target_os = "macos")]
		self.sync_toolbar_window_cursor_hittest(super::macos_mouse_location());

		if let Some(action) = self.toolbar_state.pending_action.take() {
			let control = self.handle_toolbar_action(action);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}

		self.last_present_at = Instant::now();

		if self.toolbar_state.needs_redraw {
			self.toolbar_state.needs_redraw = false;

			self.refresh_frozen_text_ime_cursor_area_for_text_style_change(monitor);
			self.request_redraw_for_monitor(monitor);
			self.request_redraw_toolbar_window();
		}

		self.log_toolbar_redraw_phase_timing(
			monitor,
			redraw_started_at,
			draw_frame_elapsed,
			position_update_elapsed,
			should_hide_toolbar_window,
		);

		OverlayControl::Continue
	}

	fn update_toolbar_position_after_redraw(&mut self, monitor: MonitorRect) -> Option<Duration> {
		let toolbar_pos = self.toolbar_state.floating_position?;
		let position_update_started_at = Instant::now();
		let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);

		self.force_apply_pending_toolbar_window_move();

		Some(position_update_started_at.elapsed())
	}

	#[cfg(target_os = "macos")]
	pub(super) fn sync_toolbar_window_cursor_hittest(
		&mut self,
		current_cursor: Option<GlobalPoint>,
	) {
		let enabled = self.toolbar_window_cursor_hittest_should_be_enabled(current_cursor);
		let Some(toolbar_window) = self.toolbar_window.as_ref() else {
			self.toolbar_window_cursor_hittest_enabled = false;

			return;
		};

		if enabled == self.toolbar_window_cursor_hittest_enabled {
			return;
		}

		let _ = toolbar_window.window.set_cursor_hittest(enabled);

		self.toolbar_window_cursor_hittest_enabled = enabled;
	}

	#[cfg(target_os = "macos")]
	fn toolbar_window_cursor_hittest_should_be_enabled(
		&self,
		current_cursor: Option<GlobalPoint>,
	) -> bool {
		if !self.toolbar_window_visible
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
			|| self.state.frozen_image.is_none()
		{
			return false;
		}

		let Some(monitor) = self.state.monitor.or_else(|| self.active_cursor_monitor()) else {
			return false;
		};
		let Some(cursor_global) = current_cursor.or(self.state.cursor) else {
			return false;
		};
		let window_toolbar_outer_pos =
			self.toolbar_window.as_ref().and_then(Self::toolbar_window_outer_position);
		let toolbar_outer_pos = window_toolbar_outer_pos
			.or(self.pending_toolbar_outer_pos)
			.or(self.toolbar_outer_pos)
			.or_else(|| {
				self.toolbar_state.floating_position.map(|floating_position| {
					GlobalPoint::new(
						monitor.origin.x.saturating_add(floating_position.x.round() as i32),
						monitor.origin.y.saturating_add(floating_position.y.round() as i32),
					)
				})
			});
		let Some(toolbar_outer_pos) = toolbar_outer_pos else {
			return false;
		};
		let cursor_local =
			Self::toolbar_cursor_local_position_from_outer(toolbar_outer_pos, cursor_global);

		WindowRenderer::frozen_toolbar_visible_capsules_contain(&self.toolbar_state, cursor_local)
	}

	fn log_toolbar_redraw_phase_timing(
		&self,
		monitor: MonitorRect,
		redraw_started_at: Instant,
		draw_frame_elapsed: Duration,
		position_update_elapsed: Option<Duration>,
		should_hide_toolbar_window: bool,
	) {
		if !tracing::enabled!(tracing::Level::TRACE)
			|| !matches!(self.state.mode, OverlayMode::Frozen)
		{
			return;
		}

		tracing::trace!(
			op = "overlay.toolbar_redraw_phase_timing",
			monitor_id = monitor.id,
			total_us = redraw_started_at.elapsed().as_micros(),
			draw_frame_us = draw_frame_elapsed.as_micros(),
			position_update_us = position_update_elapsed.map_or(0, |elapsed| elapsed.as_micros()),
			hidden = should_hide_toolbar_window,
			frozen_selection_drag_active = self.frozen_selection_drag.active,
			"Toolbar redraw phase timing."
		);
	}
}
