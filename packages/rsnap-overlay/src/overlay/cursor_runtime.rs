use crate::overlay::{
	CURSOR_POLL_INTERVAL_MIN, DeviceCursorPointSource, Duration, GlobalPoint, Instant,
	LIVE_HOVER_HIT_TEST_INTERVAL, MonitorRect, OverlayMode, OverlaySession,
};

impl OverlaySession {
	pub(super) fn maybe_tick_frozen_cursor_tracking(&mut self) {
		if !self.is_active() || !matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}

		let interval =
			self.frozen_cursor_tracking_interval(self.state.monitor).max(CURSOR_POLL_INTERVAL_MIN);
		let now = Instant::now();
		let brush_sampling_active = self.frozen_brush.active_stroke.is_some();
		let poll_due = now.duration_since(self.last_frozen_cursor_poll_at) >= interval;

		self.schedule_egui_repaint_after(interval);

		if (!brush_sampling_active || !poll_due)
			&& let Some((monitor, global)) = self.last_fresh_event_cursor()
		{
			let old_monitor = self.active_cursor_monitor();

			if tracing::enabled!(tracing::Level::TRACE) {
				tracing::trace!(
					mode = "frozen",
					source = DeviceCursorPointSource::EventRecentFallback.as_str(),
					monitor_id = monitor.id,
					"Resolved event cursor for frozen tick."
				);
			}
			if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
				return;
			}

			self.apply_frozen_cursor_tracking_update(old_monitor, monitor, global);

			return;
		}
		if self.toolbar_pointer_local.is_some()
			&& self.toolbar_state.visible
			&& let Some((monitor, global)) = self.last_event_cursor
		{
			let old_monitor = self.active_cursor_monitor();

			if tracing::enabled!(tracing::Level::TRACE) {
				tracing::trace!(
					mode = "frozen",
					source = DeviceCursorPointSource::EventRecentFallback.as_str(),
					monitor_id = monitor.id,
					toolbar_hover = true,
					"Resolved toolbar hover cursor for frozen tick."
				);
			}
			if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
				return;
			}

			self.apply_frozen_cursor_tracking_update(old_monitor, monitor, global);

			return;
		}
		if !poll_due {
			return;
		}

		self.last_frozen_cursor_poll_at = now;

		let raw = self.sample_mouse_location();
		let old_monitor = self.active_cursor_monitor();
		let Some((monitor, global, source)) = self.resolve_device_cursor_point(raw) else {
			return;
		};

		if tracing::enabled!(tracing::Level::TRACE) {
			tracing::trace!(
				mode = "frozen",
				source = source.as_str(),
				monitor_id = monitor.id,
				"Resolved device cursor for frozen tick."
			);
		}
		if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
			return;
		}

		self.apply_frozen_cursor_tracking_update(old_monitor, monitor, global);
	}

	fn apply_frozen_cursor_tracking_update(
		&mut self,
		old_monitor: Option<MonitorRect>,
		monitor: MonitorRect,
		global: GlobalPoint,
	) {
		let previous_drag_rect = self.state.drag_rect;

		self.update_cursor_state(monitor, global);
		self.update_live_drag_rect(monitor, global);
		self.update_frozen_selection_drag_rect(global);
		self.update_frozen_arrow_drag(global);
		self.update_frozen_mosaic_drag_rect(global);
		self.update_frozen_spotlight_drag_rect(global);

		let brush_changed = self.update_frozen_brush_stroke(global);

		self.sync_overlay_cursor_icons();

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		if brush_changed
			|| Self::live_overlay_redraw_needed_for_cursor_update(
				old_monitor,
				monitor,
				previous_drag_rect,
				self.state.drag_rect,
			) {
			self.request_redraw_for_monitor(monitor);
		}
	}

	pub(super) fn maybe_tick_live_cursor_tracking(&mut self) {
		if !self.is_active() || !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		let interval = self
			.repaint_interval_for_monitor(self.active_cursor_monitor())
			.max(CURSOR_POLL_INTERVAL_MIN);
		let now = Instant::now();

		// Keep this loop alive even if CursorMoved events are sparse or coalesced.
		self.schedule_egui_repaint_after(interval);

		if let Some((monitor, global)) = self.last_fresh_event_cursor() {
			let old_monitor = self.active_cursor_monitor();

			if tracing::enabled!(tracing::Level::TRACE) {
				tracing::trace!(
					mode = "live",
					source = DeviceCursorPointSource::EventRecentFallback.as_str(),
					monitor_id = monitor.id,
					"Resolved event cursor for live tick."
				);
			}
			if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
				return;
			}

			let previous_drag_rect = self.state.drag_rect;
			let old_cursor = self.state.cursor;

			self.update_cursor_for_live_move(old_monitor, old_cursor, monitor, global);
			self.update_live_drag_rect(monitor, global);
			self.sync_overlay_cursor_icons();

			if let Some(old_monitor) = old_monitor
				&& old_monitor != monitor
			{
				self.request_redraw_for_monitor(old_monitor);
			}

			if Self::live_overlay_redraw_needed_for_cursor_update(
				old_monitor,
				monitor,
				previous_drag_rect,
				self.state.drag_rect,
			) {
				self.request_redraw_for_monitor(monitor);
			}

			return;
		}

		// If we're already repainting at a higher cadence (for example selection flow), avoid
		// sampling the OS cursor position at that same cadence.
		if now.duration_since(self.last_live_cursor_poll_at) < interval {
			return;
		}

		self.last_live_cursor_poll_at = now;

		let raw = self.sample_mouse_location();
		let old_monitor = self.active_cursor_monitor();
		let Some((monitor, global, source)) = self.resolve_live_cursor_point(raw) else {
			return;
		};

		if tracing::enabled!(tracing::Level::TRACE) {
			tracing::trace!(
				mode = "live",
				source = source.as_str(),
				monitor_id = monitor.id,
				"Resolved device cursor for live tick."
			);
		}
		if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
			return;
		}

		let previous_drag_rect = self.state.drag_rect;
		let old_cursor = self.state.cursor;

		self.update_cursor_for_live_move(old_monitor, old_cursor, monitor, global);
		self.update_live_drag_rect(monitor, global);
		self.sync_overlay_cursor_icons();

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		if Self::live_overlay_redraw_needed_for_cursor_update(
			old_monitor,
			monitor,
			previous_drag_rect,
			self.state.drag_rect,
		) {
			self.request_redraw_for_monitor(monitor);
		}
	}

	pub(super) fn maybe_request_keepalive_redraw(&mut self) {
		// Avoid a tight present loop if the OS delivers spurious redraws.
		if self.is_active() && self.last_present_at.elapsed() > Duration::from_secs(30) {
			self.request_redraw_all();
		}
	}

	pub(super) fn maybe_tick_live_sampling(&mut self) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}
		if self.pending_click_hit_test_request_id.is_some() {
			return;
		}

		let now = Instant::now();
		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};

		if self
			.last_event_cursor_at
			.is_some_and(|at| now.duration_since(at) <= LIVE_HOVER_HIT_TEST_INTERVAL)
		{
			return;
		}
		if self.live_sample_request_pending() {
			return;
		}
		if !self.idle_live_sampling_request_allowed(now, monitor) {
			return;
		}

		self.record_live_sample_stall(cursor, monitor);

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}
		if self.request_live_samples_for_cursor(monitor, cursor) {
			self.last_idle_live_sample_request_at = Some(now);
		}
	}
}
