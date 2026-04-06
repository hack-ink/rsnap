#[allow(unused_imports)]
use crate::overlay::{
	Duration, FrozenCaptureSource, GlobalPoint, HudAnchor, HudPillGeometry, HudRedrawSummary,
	Instant, LIVE_PRESENT_INTERVAL_MIN, LogicalSize, MonitorRect, OverlayControl,
	OverlayEventLoopPhase, OverlayExit, OverlayMode, OverlaySession, Pos2, Rect, Result, eyre,
	scroll_capture,
};

impl OverlaySession {
	pub(super) fn stabilized_live_hud_inner_size(
		mode: OverlayMode,
		previous: Option<(u32, u32)>,
		desired: (u32, u32),
	) -> (u32, u32) {
		if !matches!(mode, OverlayMode::Live) {
			return desired;
		}

		let Some(previous) = previous else {
			return desired;
		};

		(previous.0.max(desired.0), desired.1)
	}

	pub(super) fn hud_window_content_rect(
		_mode: OverlayMode,
		_live_loupe_in_hud: bool,
		hud_pill: HudPillGeometry,
		_loupe_tile: Option<Rect>,
	) -> Rect {
		hud_pill.rect
	}

	pub(super) fn maybe_skip_hud_redraw(&mut self) -> Option<OverlayControl> {
		if self.frozen_selection_drag_hides_auxiliary_windows() {
			if let Some(hud_window) = self.hud_window.as_ref()
				&& self.hud_window_visible
			{
				hud_window.window.set_visible(false);
			}

			self.hud_window_visible = false;
			self.last_present_at = Instant::now();

			return Some(OverlayControl::Continue);
		}
		if self.scroll_capture.active {
			if let Some(hud_window) = self.hud_window.as_ref()
				&& self.hud_window_visible
			{
				hud_window.window.set_visible(false);
			}

			self.hud_window_visible = false;
			self.last_present_at = Instant::now();

			return Some(OverlayControl::Continue);
		}
		if self.capture_windows_hidden {
			#[cfg(not(target_os = "macos"))]
			{
				if let Some(hud_window) = self.hud_window.as_ref()
					&& self.hud_window_visible
				{
					hud_window.window.set_visible(false);
				}

				self.hud_window_visible = false;
				self.last_present_at = Instant::now();

				#[cfg(not(target_os = "macos"))]
				return Some(OverlayControl::Continue);
			}
		}

		None
	}

	pub(super) fn draw_hud_window_frame(
		&mut self,
		live_loupe_in_hud: bool,
	) -> Result<HudRedrawSummary> {
		let Some(gpu) = self.gpu.as_ref() else {
			return Err(eyre::eyre!("Missing GPU context"));
		};
		let monitor =
			self.monitor_for_mode().or_else(|| self.windows.values().next().map(|w| w.monitor));
		let mut summary = HudRedrawSummary::default();

		if let (Some(monitor), Some(hud_window)) = (monitor, self.hud_window.as_mut()) {
			summary.redraw_window_id = Some(hud_window.window.id());
			summary.redraw_monitor_id = Some(monitor.id);

			if !self.hud_window_visible {
				hud_window.window.set_visible(true);

				self.hud_window_visible = true;
			}

			let draw_started_at = Instant::now();

			hud_window.renderer.draw(
				gpu,
				&self.state,
				monitor,
				true,
				Some(Pos2::new(-14.0, -14.0)),
				!live_loupe_in_hud,
				HudAnchor::Cursor,
				self.config.toolbar_placement,
				self.config.show_alt_hint_keycap,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
				self.config.selection_flow_enabled,
				self.config.selection_flow_stroke_width_px,
				true,
				false,
				false,
				self.frozen_capture_source,
				self.frozen_capture_source == FrozenCaptureSource::FullscreenFallback,
				None,
				None,
				None,
			)?;

			summary.renderer_draw_elapsed = Some(draw_started_at.elapsed());

			if let Some(hud_pill) = hud_window.renderer.hud_pill {
				let height_points = hud_pill.rect.height();
				let height_changed = self
					.toolbar_state
					.pill_height_points
					.is_none_or(|prev| (prev - height_points).abs() > 0.1);

				self.toolbar_state.pill_height_points = Some(height_points);

				if height_changed
					&& matches!(self.state.mode, OverlayMode::Frozen)
					&& self.toolbar_state.visible
					&& self.state.monitor == Some(monitor)
				{
					self.toolbar_state.needs_redraw = true;
					summary.request_toolbar_redraw = Some(monitor);
				}

				let combined_rect = Self::hud_window_content_rect(
					self.state.mode,
					live_loupe_in_hud,
					hud_pill,
					hud_window.renderer.loupe_tile,
				);
				let desired_w = combined_rect.width().ceil().max(1.0) as u32;
				let desired_h = combined_rect.height().ceil().max(1.0) as u32;
				let desired = Self::stabilized_live_hud_inner_size(
					self.state.mode,
					self.hud_inner_size_points,
					(desired_w, desired_h),
				);

				if self.hud_inner_size_points != Some(desired) {
					self.hud_inner_size_points = Some(desired);
					summary.resize_target = Some(desired);

					let request_inner_size_started_at = Instant::now();
					let _ = hud_window.window.request_inner_size(LogicalSize::new(
						f64::from(desired.0),
						f64::from(desired.1),
					));

					summary.request_inner_size_elapsed =
						Some(request_inner_size_started_at.elapsed());

					if let Some(cursor) = self.state.cursor {
						let position_update_started_at = Instant::now();

						self.update_hud_window_position(monitor, cursor);

						summary.position_update_elapsed =
							Some(position_update_started_at.elapsed());
					}
				}
			}
		}

		Ok(summary)
	}

	pub(super) fn should_try_pending_hud_window_move_on_redraw(
		&self,
		summary: &HudRedrawSummary,
	) -> bool {
		summary.position_update_elapsed.is_some()
			|| (matches!(self.state.mode, OverlayMode::Live)
				&& self.pending_hud_outer_pos.is_some())
	}

	pub(super) fn should_try_pending_follow_window_move_on_live_cursor_update(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Live)
			&& (self.pending_hud_outer_pos.is_some() || self.pending_loupe_outer_pos.is_some())
	}

	pub(super) fn log_hud_redraw_metrics(
		&mut self,
		redraw_elapsed: Duration,
		summary: &HudRedrawSummary,
	) {
		tracing::trace!(
			op = "overlay.hud_redraw_phase_timing",
			window_id = ?summary.redraw_window_id,
			monitor_id = ?summary.redraw_monitor_id,
			total_us = redraw_elapsed.as_micros(),
			renderer_draw_us = summary.renderer_draw_elapsed.map_or(0, |elapsed| elapsed.as_micros()),
			request_inner_size_us = summary
				.request_inner_size_elapsed
				.map_or(0, |elapsed| elapsed.as_micros()),
			position_update_us = summary
				.position_update_elapsed
				.map_or(0, |elapsed| elapsed.as_micros()),
			toolbar_followup = summary.request_toolbar_redraw.is_some(),
			resize_target = ?summary.resize_target,
			"HUD redraw phase timing."
		);

		if let Some(elapsed) = summary.renderer_draw_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.hud_redraw.renderer_draw",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={:?} monitor_id={:?} toolbar_followup={}",
						summary.redraw_window_id,
						summary.redraw_monitor_id,
						summary.request_toolbar_redraw.is_some()
					)
				},
			);
		}
		if let Some(elapsed) = summary.request_inner_size_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.hud_redraw.request_inner_size",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={:?} monitor_id={:?} desired_size={:?}",
						summary.redraw_window_id, summary.redraw_monitor_id, summary.resize_target
					)
				},
			);
		}
		if let Some(elapsed) = summary.position_update_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.hud_redraw.position_update",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={:?} monitor_id={:?} pending_outer_pos={:?}",
						summary.redraw_window_id,
						summary.redraw_monitor_id,
						self.pending_hud_outer_pos
					)
				},
			);
		}

		self.slow_op_logger.warn_if_slow(
			"overlay.hud_redraw.total",
			redraw_elapsed,
			LIVE_PRESENT_INTERVAL_MIN,
			|| {
				format!(
					"window_id={:?} monitor_id={:?} toolbar_followup={}",
					summary.redraw_window_id,
					summary.redraw_monitor_id,
					summary.request_toolbar_redraw.is_some()
				)
			},
		);
	}

	pub(super) fn handle_hud_redraw_requested(&mut self) -> OverlayControl {
		let redraw_started_at = Instant::now();
		let live_loupe_in_hud = self.live_loupe_renders_in_hud_window();

		self.event_loop_last_progress_window_id =
			self.hud_window.as_ref().map(|hud_window| hud_window.window.id());
		self.event_loop_last_progress_monitor_id =
			self.monitor_for_mode().map(|monitor| monitor.id);

		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::HudRedraw);

		if let Some(control) = self.maybe_skip_hud_redraw() {
			return control;
		}

		let summary = match self.draw_hud_window_frame(live_loupe_in_hud) {
			Ok(summary) => summary,
			Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
		};

		if summary.position_update_elapsed.is_some() {
			self.force_apply_pending_hud_window_move();
		} else if self.should_try_pending_hud_window_move_on_redraw(&summary) {
			self.maybe_apply_pending_hud_window_move(Instant::now());
		}

		if let Some(monitor) = summary.request_toolbar_redraw {
			self.request_redraw_for_monitor(monitor);
		}

		let redraw_elapsed = redraw_started_at.elapsed();

		self.log_hud_redraw_metrics(redraw_elapsed, &summary);

		self.last_present_at = Instant::now();

		OverlayControl::Continue
	}

	pub(super) fn hide_loupe_window(&mut self) {
		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
		}

		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();

		self.last_present_at = Instant::now();
	}

	pub(super) fn should_skip_loupe_redraw(&self) -> bool {
		self.frozen_selection_drag_hides_auxiliary_windows()
			|| self.scroll_capture.active
			|| self.capture_windows_hidden
			|| !self.state.alt_held
			|| (matches!(self.state.mode, OverlayMode::Live) && self.live_loupe_uses_hud_window())
	}

	pub(super) fn current_loupe_draw_target(&self) -> Option<(MonitorRect, GlobalPoint)> {
		let monitor =
			self.monitor_for_mode().or_else(|| self.windows.values().next().map(|w| w.monitor))?;
		let cursor = self.state.cursor?;

		Some((monitor, cursor))
	}

	pub(super) fn draw_loupe_window_frame(
		&mut self,
		monitor: MonitorRect,
		_cursor: GlobalPoint,
	) -> Result<bool> {
		let redraw_started_at = Instant::now();
		let Some(loupe_window) = self.loupe_window.as_mut() else {
			return Ok(false);
		};
		let loupe_window_id = loupe_window.window.id();

		#[cfg(not(target_os = "macos"))]
		loupe_window.window.set_visible(true);

		let Some(gpu) = self.gpu.as_ref() else {
			return Err(eyre::eyre!("Missing GPU context"));
		};
		let tile_draw_started_at = Instant::now();

		loupe_window.renderer.draw_loupe_tile_window(
			gpu,
			&self.state,
			monitor,
			self.config.show_hud_blur,
			self.config.hud_opaque,
			self.config.hud_opacity,
			self.config.hud_fog_amount,
			self.config.hud_milk_amount,
			self.config.hud_tint_hue,
			self.config.theme_mode,
		)?;

		let tile_draw_elapsed = tile_draw_started_at.elapsed();
		let mut needs_reposition = false;
		let mut request_inner_size_elapsed = None;
		let mut resize_target = None;

		if let Some(tile_rect) = loupe_window.renderer.loupe_tile {
			let desired_w = tile_rect.max.x.ceil().max(1.0) as u32;
			let desired_h = tile_rect.max.y.ceil().max(1.0) as u32;
			let desired = (desired_w, desired_h);

			if self.loupe_inner_size_points != Some(desired) {
				self.loupe_inner_size_points = Some(desired);
				resize_target = Some(desired);

				let request_inner_size_started_at = Instant::now();
				let _ = loupe_window.window.request_inner_size(LogicalSize::new(
					f64::from(desired_w),
					f64::from(desired_h),
				));

				request_inner_size_elapsed = Some(request_inner_size_started_at.elapsed());
				needs_reposition = true;
			}
		}

		let redraw_elapsed = redraw_started_at.elapsed();

		self.slow_op_logger.warn_if_redraw_substep_slow(
			"overlay.loupe_redraw.tile_draw",
			tile_draw_elapsed,
			redraw_elapsed,
			|| format!("window_id={loupe_window_id:?} monitor_id={}", monitor.id),
		);

		if let Some(elapsed) = request_inner_size_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.loupe_redraw.request_inner_size",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={loupe_window_id:?} monitor_id={} desired_size={resize_target:?}",
						monitor.id
					)
				},
			);
		}

		Ok(needs_reposition)
	}

	pub(super) fn handle_loupe_redraw_requested(&mut self) -> OverlayControl {
		let redraw_started_at = Instant::now();

		self.event_loop_last_progress_window_id =
			self.loupe_window.as_ref().map(|loupe_window| loupe_window.window.id());
		self.event_loop_last_progress_monitor_id =
			self.monitor_for_mode().map(|monitor| monitor.id);

		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::LoupeRedraw);

		if self.gpu.is_none() {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};
		if self.should_skip_loupe_redraw() {
			self.hide_loupe_window();

			return OverlayControl::Continue;
		}

		let Some((monitor, cursor)) = self.current_loupe_draw_target() else {
			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		};
		let redraw_window_id =
			self.loupe_window.as_ref().map(|loupe_window| loupe_window.window.id());
		let was_visible = self.loupe_window_visible;
		let needs_reposition = match self.draw_loupe_window_frame(monitor, cursor) {
			Ok(needs_reposition) => needs_reposition,
			Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
		};
		let mut reposition_elapsed = None;

		if needs_reposition {
			let reposition_started_at = Instant::now();
			let _ = self.update_loupe_window_position(monitor);

			self.force_apply_pending_loupe_window_move();

			reposition_elapsed = Some(reposition_started_at.elapsed());
		}

		if let Some(loupe_window) = self.loupe_window.as_ref()
			&& !was_visible
		{
			loupe_window.window.set_visible(true);
		}

		self.loupe_window_visible = true;

		if !was_visible {
			self.maybe_start_loupe_window_warmup_redraw();
		}

		let redraw_elapsed = redraw_started_at.elapsed();

		if let Some(elapsed) = reposition_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.loupe_redraw.reposition",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={redraw_window_id:?} monitor_id={} pending_outer_pos={:?}",
						monitor.id, self.pending_loupe_outer_pos
					)
				},
			);
		}

		tracing::trace!(
			op = "overlay.loupe_redraw_phase_timing",
			window_id = ?redraw_window_id,
			monitor_id = monitor.id,
			total_us = redraw_elapsed.as_micros(),
			reposition_us = reposition_elapsed.map_or(0, |elapsed| elapsed.as_micros()),
			was_visible,
			needs_reposition,
			"Loupe redraw phase timing."
		);

		self.slow_op_logger.warn_if_slow(
			"overlay.loupe_redraw.total",
			redraw_elapsed,
			LIVE_PRESENT_INTERVAL_MIN,
			|| {
				format!(
					"window_id={redraw_window_id:?} monitor_id={} was_visible={} needs_reposition={}",
					monitor.id, was_visible, needs_reposition
				)
			},
		);

		self.last_present_at = Instant::now();

		OverlayControl::Continue
	}
}
