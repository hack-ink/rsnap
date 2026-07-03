use crate::overlay::{GlobalPoint, MonitorRect, OverlayControl, OverlayMode, OverlaySession};

impl OverlaySession {
	pub(super) fn set_alt_held(&mut self, alt: bool) {
		if self.state.alt_held == alt {
			return;
		}
		if alt && !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		self.state.alt_held = alt;

		if !alt {
			self.handle_alt_release();

			return;
		}
		if self.live_capture_hides_auxiliary_windows() {
			self.state.loupe = None;

			self.set_alt_loupe_window_visible(None, false);

			return;
		}

		let Some((monitor, cursor)) = self.loupe_activation_cursor_context() else {
			return;
		};

		self.set_alt_loupe_window_visible(Some(monitor), true);

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}

		self.request_live_alt_samples(monitor, cursor);
	}

	pub(super) fn apply_loupe_activation_input(&mut self, pressed: bool, repeat: bool) -> bool {
		let previous_alt_held = self.state.alt_held;

		if pressed && !repeat {
			self.set_alt_held(!self.state.alt_held);
		}

		previous_alt_held != self.state.alt_held
	}

	pub(super) fn apply_loupe_activation_key_event(&mut self, pressed: bool, repeat: bool) -> bool {
		if self.loupe_activation_key_down == pressed && !repeat {
			return false;
		}
		if !matches!(self.state.mode, OverlayMode::Live) {
			self.loupe_activation_key_down = false;

			return false;
		}

		self.loupe_activation_key_down = pressed;

		if !pressed && !self.state.alt_held {
			return false;
		}
		if pressed && !self.loupe_activation_shortcut_available() {
			return false;
		}

		self.apply_loupe_activation_input(pressed, repeat)
	}

	pub(super) fn clear_loupe_activation_on_focus_loss(&mut self) {
		if !self.loupe_activation_key_down {
			return;
		}

		self.loupe_activation_key_down = false;
	}

	pub(super) fn maybe_clear_loupe_activation_after_focus_loss(&mut self) {
		if !self.pending_focus_loss_cleanup || !self.focused_window_ids.is_empty() {
			return;
		}

		self.pending_focus_loss_cleanup = false;

		self.clear_loupe_activation_on_focus_loss();
	}

	pub(super) fn request_redraw_for_alt_state_change(&mut self) -> OverlayControl {
		if matches!(self.state.mode, OverlayMode::Live) {
			self.request_redraw_hud_window();

			if !self.live_loupe_uses_hud_window()
				&& (self.state.alt_held || self.loupe_window_visible)
			{
				self.request_redraw_loupe_window();
			}
		}

		OverlayControl::Continue
	}

	pub(super) fn loupe_activation_cursor_context(&mut self) -> Option<(MonitorRect, GlobalPoint)> {
		if let Some((monitor, cursor)) = self.last_fresh_event_cursor() {
			self.seed_loupe_activation_cursor_context(monitor, cursor);

			return Some((monitor, cursor));
		}

		let cursor = self.sample_mouse_location();
		let Some(monitor) = self.monitor_at(cursor) else {
			if self.state.cursor.is_none() {
				self.state.cursor = Some(cursor);
			}

			return self.active_cursor_monitor().zip(self.state.cursor);
		};

		self.seed_loupe_activation_cursor_context(monitor, cursor);

		Some((monitor, cursor))
	}

	pub(super) fn seed_loupe_activation_cursor_context(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) {
		let old_monitor = self.active_cursor_monitor();
		let old_cursor = self.state.cursor;

		match self.state.mode {
			OverlayMode::Live => {
				self.update_cursor_for_live_move(old_monitor, old_cursor, monitor, cursor)
			},
			OverlayMode::Frozen => self.update_cursor_state(monitor, cursor),
		}
	}

	fn handle_alt_release(&mut self) {
		self.state.loupe = None;
		self.loupe_outer_pos = None;
		self.pending_loupe_outer_pos = None;

		self.set_alt_loupe_window_visible(None, false);

		if matches!(self.state.mode, OverlayMode::Live) {
			self.request_redraw_hud_window();
		}
	}

	pub(super) fn set_alt_loupe_window_visible(
		&mut self,
		monitor: Option<MonitorRect>,
		visible: bool,
	) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			self.loupe_window_visible = false;

			self.reset_loupe_window_warmup_redraws();

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(false);
				loupe_window.window.request_redraw();
			}

			return;
		}
		if self.live_loupe_uses_hud_window() {
			self.loupe_window_visible = false;

			self.reset_loupe_window_warmup_redraws();

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(false);
			}

			return;
		}
		if visible {
			let Some(monitor) = monitor else {
				return;
			};

			#[cfg(target_os = "macos")]
			if self.loupe_window.is_none() {
				self.request_aux_window_creation_if_needed();

				return;
			}

			self.maybe_apply_pending_startup_aux_live_stream_filter_upgrade(monitor);

			let visible = self.update_loupe_window_position(monitor);
			let was_visible = self.loupe_window_visible;

			self.loupe_window_visible = visible;

			if visible {
				self.force_apply_pending_loupe_window_move();
			}
			if visible {
				if !was_visible {
					self.maybe_start_loupe_window_warmup_redraw();
				}
			} else {
				self.reset_loupe_window_warmup_redraws();
			}

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(visible);
				loupe_window.window.request_redraw();
			}

			return;
		}

		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
			loupe_window.window.request_redraw();
		}
	}

	pub(super) fn request_live_alt_samples(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		let sample_updated = self.request_live_cursor_sample(monitor, cursor, true);
		let apply = self.live_sample_request_redraw_intent(false, sample_updated, true);

		if apply.any_changed() {
			self.request_redraw_live_sample_targets(monitor, apply);
		}
	}
}
