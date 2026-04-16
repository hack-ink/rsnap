#[cfg(target_os = "macos")]
use crate::overlay::Instant;
use crate::overlay::{GlobalPoint, MonitorRect, OverlayMode, OverlaySession};

impl OverlaySession {
	#[cfg(target_os = "macos")]
	pub(super) const fn should_hide_overlay_windows_during_capture(&self) -> bool {
		// Display-first frozen entry no longer depends on hiding overlays. Keep the legacy hide policy
		// available only for explicit last-resort macOS capture paths until the backend contract is
		// fully retired.
		true
	}

	pub(super) fn update_cursor_state(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		self.cursor_monitor = Some(monitor);
		self.state.cursor = Some(cursor);

		#[cfg(target_os = "macos")]
		self.sync_toolbar_window_cursor_hittest(Some(cursor));
	}

	#[cfg(target_os = "macos")]
	#[allow(dead_code)]
	pub(super) fn hide_capture_windows(&mut self) {
		self.capture_windows_hidden = true;

		let _ = self.sync_native_capture_shells();
		let hide_overlay_windows = self.should_hide_overlay_windows_during_capture();

		if hide_overlay_windows {
			for overlay_window in self.windows.values() {
				overlay_window.window.set_visible(false);
			}
		}

		if let Some(hud_window) = &self.hud_window {
			hud_window.window.set_visible(false);
		}

		self.hud_window_visible = false;

		if let Some(loupe_window) = &self.loupe_window {
			loupe_window.window.set_visible(false);
		}

		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();

		if let Some(toolbar_window) = &self.toolbar_window {
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

		if let Some(preview_window) = &self.scroll_preview_window {
			preview_window.window.set_visible(false);
		}

		self.last_present_at = Instant::now();
	}

	#[cfg(not(target_os = "macos"))]
	pub(super) fn hide_capture_windows(&mut self) {
		self.capture_windows_hidden = true;

		if let Some(hud_window) = &self.hud_window {
			hud_window.window.set_visible(false);
		}

		self.hud_window_visible = false;

		if let Some(loupe_window) = &self.loupe_window {
			loupe_window.window.set_visible(false);
		}
	}

	pub(super) fn restore_capture_windows_visibility(&mut self) {
		if !self.capture_windows_hidden {
			return;
		}

		self.capture_windows_hidden = false;

		#[cfg(target_os = "macos")]
		let _ = self.sync_native_capture_shells();

		#[cfg(target_os = "macos")]
		{
			for overlay_window in self.windows.values() {
				overlay_window.window.set_visible(true);
				overlay_window.window.request_redraw();
			}

			if matches!(self.state.mode, OverlayMode::Live) {
				if let Some(hud_window) = &self.hud_window {
					hud_window.window.set_visible(true);
					hud_window.window.request_redraw();
				}

				self.hud_window_visible = self.hud_window.is_some();

				if let Some(loupe_window) = &self.loupe_window {
					loupe_window.window.set_visible(self.state.alt_held);
					loupe_window.window.request_redraw();
				}

				self.loupe_window_visible = self.state.alt_held && self.loupe_window.is_some();

				return;
			}

			self.hud_window_visible = false;
			self.loupe_window_visible = false;

			if let Some(toolbar_window) = &self.toolbar_window {
				let show_toolbar = matches!(self.state.mode, OverlayMode::Frozen)
					&& self.toolbar_state.visible
					&& self.frozen_preview_visible();

				toolbar_window.window.set_visible(show_toolbar);

				if show_toolbar {
					toolbar_window.window.request_redraw();
				}

				self.toolbar_window_visible = show_toolbar;

				if !show_toolbar {
					self.toolbar_window_drawn_once = false;
					self.toolbar_badge_slot_ready = false;
				}
			} else {
				self.toolbar_window_visible = false;
				self.toolbar_window_drawn_once = false;
				self.toolbar_badge_slot_ready = false;
			}
			if let Some(preview_window) = &self.scroll_preview_window {
				preview_window.window.set_visible(self.scroll_capture.active);

				if self.scroll_capture.active {
					preview_window.window.request_redraw();
				}
			}
		}
		#[cfg(not(target_os = "macos"))]
		{
			if matches!(self.state.mode, OverlayMode::Live) {
				if let Some(hud_window) = &self.hud_window {
					hud_window.window.set_visible(true);
				}

				self.hud_window_visible = true;

				if let Some(loupe_window) = &self.loupe_window {
					loupe_window.window.set_visible(self.state.alt_held);
				}
			} else {
				self.hud_window_visible = false;
				self.loupe_window_visible = false;
			}
		}
	}

	#[cfg(not(target_os = "macos"))]
	pub(super) fn raise_hud_windows(&self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			hud_window.window.focus_window();
		}

		if self.state.alt_held
			&& let Some(loupe_window) = self.loupe_window.as_ref()
		{
			loupe_window.window.focus_window();
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn destroy_live_only_aux_windows(&mut self) {
		if let Some(loupe_window) = self.loupe_window.take() {
			self.remove_macos_hud_window_config_cache_entry(loupe_window.window.id());
		}

		self.loupe_inner_size_points = None;
		self.loupe_outer_pos = None;
		self.pending_loupe_outer_pos = None;
		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();
	}
}
