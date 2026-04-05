#[cfg(target_os = "macos")]
#[allow(unused_imports)]
use crate::overlay::TOOLBAR_WINDOW_WARMUP_REDRAWS;
#[allow(unused_imports)]
use crate::overlay::{
	Arc, FrozenCaptureSource, FrozenToolbarPointerState, GlobalPoint,
	HUD_PILL_CORNER_RADIUS_POINTS, HudAnchor, HudOverlayWindow, Instant, LogicalSize, MonitorRect,
	OverlayControl, OverlayEventLoopPhase, OverlayExit, OverlayMode, OverlaySession,
	PhysicalPosition, PhysicalSize, Pos2, Result, TOOLBAR_DRAG_START_THRESHOLD_PX, Vec2, WindowId,
	scroll_capture,
};

impl OverlaySession {
	pub(super) fn handle_toolbar_cursor_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
	) -> OverlayControl {
		let Some(toolbar_window) = self.toolbar_window.as_ref() else {
			return OverlayControl::Continue;
		};

		if toolbar_window.window.id() != window_id
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
		{
			return OverlayControl::Continue;
		}

		let scale = toolbar_window.window.scale_factor().max(1.0);
		let cursor_local = Pos2::new((position.x / scale) as f32, (position.y / scale) as f32);

		self.toolbar_pointer_local = Some(cursor_local);

		if self.frozen_selection_drag.active {
			if let Some(global_cursor) =
				self.toolbar_cursor_global_position(toolbar_window, cursor_local)
			{
				self.update_frozen_selection_drag_rect(global_cursor);
			}

			return OverlayControl::Continue;
		}

		let monitor = match self.state.monitor.or_else(|| self.active_cursor_monitor()) {
			Some(monitor) => monitor,
			None => return OverlayControl::Continue,
		};
		let global_cursor = self.toolbar_cursor_global_position(toolbar_window, cursor_local);
		let drag_monitor =
			global_cursor.and_then(|cursor| self.monitor_at(cursor)).unwrap_or(monitor);
		let mut mouse_drag = self.toolbar_left_button_down && self.toolbar_state.dragging;

		if self.toolbar_left_button_down && self.toolbar_state.drag_anchor.is_none() {
			self.toolbar_state.drag_anchor = Some(cursor_local);
		}
		if !mouse_drag && let Some(drag_anchor) = self.toolbar_state.drag_anchor {
			let dx = cursor_local.x - drag_anchor.x;
			let dy = cursor_local.y - drag_anchor.y;
			let threshold_sq = TOOLBAR_DRAG_START_THRESHOLD_PX * TOOLBAR_DRAG_START_THRESHOLD_PX;

			if dx * dx + dy * dy >= threshold_sq {
				let toolbar_outer_pos = self.toolbar_outer_pos.or_else(|| {
					self.toolbar_state.floating_position.map(|floating_position| {
						GlobalPoint::new(
							monitor.origin.x.saturating_add(floating_position.x.round() as i32),
							monitor.origin.y.saturating_add(floating_position.y.round() as i32),
						)
					})
				});

				if let (Some(global_cursor), Some(toolbar_outer_pos)) =
					(global_cursor, toolbar_outer_pos)
				{
					self.toolbar_state.drag_offset = Vec2::new(
						global_cursor.x as f32 - toolbar_outer_pos.x as f32,
						global_cursor.y as f32 - toolbar_outer_pos.y as f32,
					);
					self.toolbar_state.dragging = true;
					self.toolbar_state.drag_anchor = None;
					mouse_drag = true;
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
		}

		self.request_redraw_toolbar_window();

		OverlayControl::Continue
	}

	pub(super) fn toolbar_cursor_global_position(
		&self,
		toolbar_window: &HudOverlayWindow,
		cursor_local: Pos2,
	) -> Option<GlobalPoint> {
		let toolbar_scale = toolbar_window.window.scale_factor().max(1.0);
		let outer_position = toolbar_window.window.outer_position().ok()?;
		let global_cursor = Pos2::new(
			(outer_position.x as f64 / toolbar_scale) as f32 + cursor_local.x,
			(outer_position.y as f64 / toolbar_scale) as f32 + cursor_local.y,
		);

		Some(GlobalPoint::new(global_cursor.x.round() as i32, global_cursor.y.round() as i32))
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

				self.configure_hud_window_common(
					window.as_ref(),
					Some(f64::from(HUD_PILL_CORNER_RADIUS_POINTS)),
				);

				OverlayControl::Continue
			},
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	pub(super) fn should_hide_toolbar_window(&self, monitor: MonitorRect) -> bool {
		!matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
			|| self.state.frozen_image.is_none()
			|| self.state.monitor != Some(monitor)
	}

	pub(super) fn set_toolbar_window_hidden(&mut self) {
		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			toolbar_window.window.set_visible(false);
		}

		self.toolbar_window_visible = false;
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
			let should_focus_frozen_keyboard = !self.toolbar_window_visible
				&& matches!(self.state.mode, OverlayMode::Frozen)
				&& !self.scroll_capture.active;

			if !self.toolbar_window_visible {
				self.maybe_apply_pending_startup_aux_live_stream_filter_upgrade(monitor);
			}

			let Some(gpu) = self.gpu.as_ref() else {
				return Ok(());
			};
			let Some(toolbar_window) = self.toolbar_window.as_ref() else {
				return Ok(());
			};

			toolbar_window.window.set_visible(true);

			if !self.toolbar_window_visible {
				self.toolbar_window_visible = true;
				self.toolbar_window_warmup_redraws_remaining = TOOLBAR_WINDOW_WARMUP_REDRAWS;
			}
			if should_focus_frozen_keyboard {
				self.focus_frozen_keyboard_window();
			}

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
				Some(&mut self.toolbar_state),
				toolbar_input,
			);

			self.toolbar_state.floating_position = previous_floating_position;

			draw_result?;

			let desired_inner_size = toolbar_window.renderer.hud_pill.map(|hud_pill| {
				(
					hud_pill.rect.width().ceil().max(1.0) as u32,
					hud_pill.rect.height().ceil().max(1.0) as u32,
				)
			});
			let toolbar_window = Arc::clone(&toolbar_window.window);

			if let Some(desired) = desired_inner_size
				&& self.toolbar_inner_size_points != Some(desired)
			{
				self.toolbar_inner_size_points = Some(desired);

				let _ = toolbar_window.request_inner_size(LogicalSize::new(
					f64::from(desired.0),
					f64::from(desired.1),
				));
			}

			Ok(())
		}
	}

	pub(super) fn handle_toolbar_window_redraw_requested(&mut self) -> OverlayControl {
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

		if let Err(err) = self.draw_toolbar_window_frame(monitor, toolbar_input) {
			return self.exit(OverlayExit::Error(format!("{err:#}")));
		}

		self.update_scroll_toolbar_default_position(monitor);

		if let Some(toolbar_pos) = self.toolbar_state.floating_position {
			let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);
		}
		if let Some(action) = self.toolbar_state.pending_action.take() {
			let control = self.handle_toolbar_action(action);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}

		self.last_present_at = Instant::now();

		if self.toolbar_state.needs_redraw {
			self.toolbar_state.needs_redraw = false;

			self.request_redraw_toolbar_window();
		}

		OverlayControl::Continue
	}
}
