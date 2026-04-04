#[allow(unused_imports)]
use crate::overlay::{
	Duration, Event, GlobalPoint, HUD_LOUPE_MOVE_INTERVAL_MIN, INTERACTIVE_REPAINT_FPS_CAP,
	Instant, LOUPE_WINDOW_WARMUP_REDRAWS, LogicalPosition, MonitorRect, MonitorRectPoints,
	OVERLAY_EVENT_LOOP_STALL_THRESHOLD, Ordering, OverlayControl, OverlayEventLoopPhase,
	OverlayMode, OverlaySession, SLOW_OP_WARN_INTERVAL, SLOW_OP_WARN_OUTER_POSITION, WindowEvent,
	scroll_capture,
};

impl OverlaySession {
	pub(super) fn live_loupe_uses_hud_window(&self) -> bool {
		false
	}

	pub(super) fn live_loupe_renders_in_hud_window(&self) -> bool {
		self.live_loupe_uses_hud_window() && self.state.alt_held
	}

	pub(super) fn maybe_tick_loupe_window_warmup_redraw(&mut self) {
		if self.loupe_window_warmup_redraws_remaining == 0 {
			return;
		}
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.loupe_window_visible
			|| self.state.frozen_image.is_none()
			|| self.state.monitor.is_none()
		{
			self.loupe_window_warmup_redraws_remaining = 0;

			return;
		}

		self.loupe_window_warmup_redraws_remaining =
			self.loupe_window_warmup_redraws_remaining.saturating_sub(1);

		self.request_redraw_loupe_window();
		self.schedule_egui_repaint_after(self.repaint_interval_for_monitor(self.state.monitor));
	}

	pub(super) fn maybe_start_loupe_window_warmup_redraw(&mut self) {
		if self.loupe_window_warmup_redraws_remaining > 0 {
			return;
		}
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.state.alt_held
			|| !self.loupe_window_visible
			|| self.state.frozen_image.is_none()
			|| self.state.monitor.is_none()
		{
			return;
		}

		self.loupe_window_warmup_redraws_remaining = LOUPE_WINDOW_WARMUP_REDRAWS;
	}

	pub(super) fn reset_loupe_window_warmup_redraws(&mut self) {
		self.loupe_window_warmup_redraws_remaining = 0;
	}

	/// Advances periodic session work before the event loop goes idle.
	pub fn about_to_wait(&mut self) -> OverlayControl {
		let now = Instant::now();

		self.maybe_log_event_loop_stall(now);
		self.mark_progress(OverlayEventLoopPhase::AboutToWait);
		self.maybe_clear_loupe_activation_after_focus_loss();
		self.maybe_request_keepalive_redraw();
		self.maybe_keep_selection_flow_repaint();
		self.maybe_keep_frozen_capture_redraw();
		self.maybe_tick_toolbar_window_warmup_redraw();
		self.maybe_tick_loupe_window_warmup_redraw();
		self.maybe_tick_live_cursor_tracking();
		self.maybe_tick_live_sampling();
		self.maybe_tick_frozen_cursor_tracking();
		self.maybe_apply_pending_hud_and_loupe_moves();
		self.maybe_tick_scroll_capture();
		self.maybe_keep_live_cursor_sample_redraw();

		self.drain_worker_responses()
	}

	pub(super) fn mark_progress(&mut self, phase: OverlayEventLoopPhase) {
		self.mark_progress_with_detail(phase, None);
	}

	pub(super) fn mark_progress_with_detail(
		&mut self,
		phase: OverlayEventLoopPhase,
		detail: Option<&'static str>,
	) {
		self.event_loop_phase = phase;
		self.event_loop_last_progress_detail = detail;
		self.event_loop_progress_seq = self.event_loop_progress_seq.saturating_add(1);
		self.event_loop_last_progress_at = Instant::now();
	}

	pub(super) fn maybe_log_event_loop_stall(&mut self, now: Instant) {
		let stall = now.duration_since(self.event_loop_last_progress_at);

		if stall < OVERLAY_EVENT_LOOP_STALL_THRESHOLD {
			return;
		}
		if self
			.event_loop_last_stall_warn_at
			.is_none_or(|last| now.duration_since(last) >= SLOW_OP_WARN_INTERVAL)
		{
			let _ = self.event_loop_last_stall_warn_at.insert(now);

			tracing::warn!(
				op = "overlay.event_loop_stall",
				stall_ms = stall.as_millis(),
				phase = %self.event_loop_phase.as_str(),
				progress_seq = self.event_loop_progress_seq,
				mode = ?self.state.mode,
				window_id = ?self.event_loop_last_progress_window_id,
				monitor_id = ?self.event_loop_last_progress_monitor_id,
				detail = ?self.event_loop_last_progress_detail,
				"Event loop stalled"
			);
		}
	}

	pub(super) fn window_event_kind(event: &WindowEvent) -> &'static str {
		match event {
			WindowEvent::ActivationTokenDone { .. } => "activation_token_done",
			WindowEvent::CloseRequested => "close_requested",
			WindowEvent::Destroyed => "destroyed",
			WindowEvent::DroppedFile(_) => "dropped_file",
			WindowEvent::HoveredFile(_) => "hovered_file",
			WindowEvent::HoveredFileCancelled => "hovered_file_cancelled",
			WindowEvent::Focused(_) => "focused",
			WindowEvent::Moved(_) => "moved",
			WindowEvent::Resized(_) => "resized",
			WindowEvent::ScaleFactorChanged { .. } => "scale_factor_changed",
			WindowEvent::Ime(_) => "ime",
			WindowEvent::CursorEntered { .. } => "cursor_entered",
			WindowEvent::CursorLeft { .. } => "cursor_left",
			WindowEvent::CursorMoved { .. } => "cursor_moved",
			WindowEvent::MouseWheel { .. } => "mouse_wheel",
			WindowEvent::MouseInput { .. } => "mouse_input",
			WindowEvent::PinchGesture { .. } => "pinch_gesture",
			WindowEvent::PanGesture { .. } => "pan_gesture",
			WindowEvent::DoubleTapGesture { .. } => "double_tap_gesture",
			WindowEvent::RotationGesture { .. } => "rotation_gesture",
			WindowEvent::TouchpadPressure { .. } => "touchpad_pressure",
			WindowEvent::AxisMotion { .. } => "axis_motion",
			WindowEvent::Touch(_) => "touch",
			WindowEvent::ThemeChanged(_) => "theme_changed",
			WindowEvent::KeyboardInput { .. } => "keyboard_input",
			WindowEvent::ModifiersChanged(_) => "modifiers_changed",
			WindowEvent::Occluded(_) => "occluded",
			WindowEvent::RedrawRequested => "redraw_requested",
		}
	}

	pub(super) fn maybe_keep_live_cursor_sample_redraw(&mut self) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}
		if self.latest_live_cursor_sample_request_id.is_none() {
			return;
		}
		if !self.live_sample_request_pending() {
			return;
		}

		self.schedule_egui_repaint_after(
			self.repaint_interval_for_monitor(self.active_cursor_monitor()),
		);
	}

	pub(super) fn maybe_keep_selection_flow_repaint(&self) {
		if !self.is_active() || !self.config.selection_flow_enabled {
			return;
		}

		let keep_repaint = match self.state.mode {
			OverlayMode::Live => self.live_overlay_selection_flow_repaint_active(),
			OverlayMode::Frozen => self.state.frozen_capture_rect.is_some(),
		};

		if keep_repaint {
			let monitor = match self.state.mode {
				OverlayMode::Live => self.active_cursor_monitor(),
				OverlayMode::Frozen => self.state.monitor,
			};
			let repaint_interval = self.selection_flow_repaint_interval(monitor);

			if let Some(monitor) = monitor {
				self.request_redraw_for_monitor(monitor);
			} else {
				self.request_redraw_all();
			}

			self.schedule_egui_repaint_after(repaint_interval);
		}
	}

	pub(super) fn live_overlay_selection_flow_repaint_active(&self) -> bool {
		if !self.config.selection_flow_enabled {
			return false;
		}

		self.state.hovered_window_rect.is_some_and(|hovered| {
			self.active_cursor_monitor().is_some_and(|monitor| hovered.monitor_id == monitor.id)
		})
	}

	pub(super) fn live_overlay_redraw_needed_for_cursor_update(
		old_monitor: Option<MonitorRect>,
		monitor: MonitorRect,
		previous_drag_rect: Option<MonitorRectPoints>,
		next_drag_rect: Option<MonitorRectPoints>,
	) -> bool {
		old_monitor != Some(monitor) || previous_drag_rect != next_drag_rect
	}

	pub(super) fn live_hud_redraw_needed_for_cursor_update(
		old_cursor: Option<GlobalPoint>,
		cursor: GlobalPoint,
		old_monitor: Option<MonitorRect>,
		monitor: MonitorRect,
	) -> bool {
		old_cursor != Some(cursor) || old_monitor != Some(monitor)
	}

	pub(super) fn repaint_interval_for_monitor(&self, monitor: Option<MonitorRect>) -> Duration {
		let monitor_fps = monitor
			.and_then(|target| {
				self.windows.values().find_map(|window| {
					(target == window.monitor).then_some(window.refresh_rate_millihertz)
				})
			})
			.flatten()
			.and_then(|hz| {
				let fps = (hz as f32) / 1_000.0;

				if fps.is_finite() && fps > 0.0 { Some(fps) } else { None }
			});
		let fallback_fps = self
			.windows
			.values()
			.filter_map(|window| window.refresh_rate_millihertz)
			.filter_map(|hz| {
				let fps = (hz as f32) / 1_000.0;

				if fps.is_finite() && fps > 0.0 { Some(fps) } else { None }
			})
			.max_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
		let fps = Self::interactive_repaint_fps(monitor_fps, fallback_fps);

		Duration::from_secs_f32(1.0 / fps)
	}

	pub(super) fn interactive_repaint_fps(
		monitor_fps: Option<f32>,
		fallback_fps: Option<f32>,
	) -> f32 {
		monitor_fps
			.or(fallback_fps)
			.map_or(INTERACTIVE_REPAINT_FPS_CAP, |fps| fps.min(INTERACTIVE_REPAINT_FPS_CAP))
	}

	pub(super) fn selection_flow_repaint_interval(&self, monitor: Option<MonitorRect>) -> Duration {
		self.repaint_interval_for_monitor(monitor)
	}

	pub(super) fn frozen_cursor_tracking_interval(&self, monitor: Option<MonitorRect>) -> Duration {
		self.repaint_interval_for_monitor(monitor)
	}

	/// Returns the active repaint cadence that keeps interactive overlays responsive.
	pub fn interactive_wait_interval(&self) -> Duration {
		let monitor = if self.scroll_capture.active {
			self.scroll_capture.monitor.or(self.state.monitor)
		} else {
			self.active_cursor_monitor()
		};

		self.repaint_interval_for_monitor(monitor)
	}

	pub(super) fn live_sample_request_pending(&self) -> bool {
		self.latest_live_cursor_sample_request_id.is_some()
			&& self.applied_live_cursor_sample_request_id
				!= self.latest_live_cursor_sample_request_id
	}

	pub(super) fn note_live_cursor_sample_request_started(&mut self, request_id: u64) {
		self.live_cursor_sample_request_id = request_id;
		self.latest_live_cursor_sample_request_id = Some(request_id);
		self.latest_live_cursor_sample_requested_at = Some(Instant::now());
	}

	#[cfg(target_os = "macos")]
	pub(super) fn finish_sync_live_cursor_sample_attempt(&mut self, request_id: u64) {
		// Synchronous latest-frame reads on the current thread either produce a sample now or miss
		// now. They must not leave async-style "pending" bookkeeping behind.

		debug_assert_eq!(self.latest_live_cursor_sample_request_id, Some(request_id));

		self.applied_live_cursor_sample_request_id = Some(request_id);
	}

	pub(super) fn maybe_apply_pending_hud_and_loupe_moves(&mut self) {
		let now = Instant::now();

		self.maybe_apply_pending_hud_window_move(now);
		self.maybe_apply_pending_loupe_window_move(now);
	}

	pub(super) fn maybe_apply_pending_hud_window_move(&mut self, now: Instant) {
		self.apply_pending_hud_window_move(now, false);
	}

	pub(super) fn force_apply_pending_hud_window_move(&mut self) {
		self.apply_pending_hud_window_move(Instant::now(), true);
	}

	pub(super) fn apply_pending_hud_window_move(&mut self, now: Instant, force: bool) {
		let Some(desired) = self.pending_hud_outer_pos else {
			return;
		};
		let elapsed = now.duration_since(self.last_hud_window_move_at);
		let interval = self
			.repaint_interval_for_monitor(self.active_cursor_monitor())
			.max(HUD_LOUPE_MOVE_INTERVAL_MIN);

		if !force && elapsed < interval {
			let delay = interval.saturating_sub(elapsed);

			self.schedule_egui_repaint_after(delay);

			return;
		}

		let Some(hud_window) = self.hud_window.as_ref() else {
			return;
		};
		let started_at = Instant::now();

		hud_window
			.window
			.set_outer_position(LogicalPosition::new(desired.x as f64, desired.y as f64));

		let elapsed = started_at.elapsed();

		self.slow_op_logger.warn_if_slow(
			"overlay.hud_window_set_outer_position",
			elapsed,
			SLOW_OP_WARN_OUTER_POSITION,
			|| format!("window_id={:?} pos=({}, {})", hud_window.window.id(), desired.x, desired.y),
		);

		self.pending_hud_outer_pos = None;
		self.last_hud_window_move_at = now;
	}

	pub(super) fn force_apply_pending_hud_and_loupe_moves(&mut self) {
		self.force_apply_pending_hud_window_move();
		self.force_apply_pending_loupe_window_move();
	}

	pub(super) fn maybe_apply_pending_loupe_window_move(&mut self, now: Instant) {
		self.apply_pending_loupe_window_move(now, false);
	}

	pub(super) fn force_apply_pending_loupe_window_move(&mut self) {
		self.apply_pending_loupe_window_move(Instant::now(), true);
	}

	pub(super) fn apply_pending_loupe_window_move(&mut self, now: Instant, force: bool) {
		let Some(desired) = self.pending_loupe_outer_pos else {
			return;
		};
		let elapsed = now.duration_since(self.last_loupe_window_move_at);
		let interval = self
			.repaint_interval_for_monitor(self.active_cursor_monitor())
			.max(HUD_LOUPE_MOVE_INTERVAL_MIN);

		if !force && elapsed < interval {
			let delay = interval.saturating_sub(elapsed);

			self.schedule_egui_repaint_after(delay);

			return;
		}

		let Some(loupe_window) = self.loupe_window.as_ref() else {
			return;
		};
		let started_at = Instant::now();

		loupe_window
			.window
			.set_outer_position(LogicalPosition::new(desired.x as f64, desired.y as f64));

		let elapsed = started_at.elapsed();

		self.slow_op_logger.warn_if_slow(
			"overlay.loupe_window_set_outer_position",
			elapsed,
			SLOW_OP_WARN_OUTER_POSITION,
			|| {
				format!(
					"window_id={:?} pos=({}, {})",
					loupe_window.window.id(),
					desired.x,
					desired.y
				)
			},
		);

		self.pending_loupe_outer_pos = None;
		self.last_loupe_window_move_at = now;
	}

	pub(super) fn schedule_egui_repaint_after(&self, delay: Duration) {
		let deadline = Instant::now() + delay;
		let mut next_repaint =
			self.egui_repaint_deadline.lock().unwrap_or_else(|err| err.into_inner());

		if next_repaint.is_none_or(|next| deadline < next) {
			*next_repaint = Some(deadline);
		}
	}
}
