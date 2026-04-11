#[allow(unused_imports)]
use crate::overlay::{GlobalPoint, Instant, MonitorRect, OverlayMode, OverlaySession};

impl OverlaySession {
	pub(super) fn update_cursor_state(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		self.cursor_monitor = Some(monitor);
		self.state.cursor = Some(cursor);
	}

	#[cfg(target_os = "macos")]
	pub(super) fn hide_capture_windows(&mut self) {
		self.capture_windows_hidden = true;

		for overlay_window in self.windows.values() {
			overlay_window.window.set_visible(false);
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
			toolbar_window.window.set_visible(false);
		}

		self.toolbar_window_visible = false;
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
					&& self.authoritative_frozen_capture_ready
					&& self.state.frozen_image.is_some();

				toolbar_window.window.set_visible(show_toolbar);

				if show_toolbar {
					toolbar_window.window.request_redraw();
				}

				self.toolbar_window_visible = show_toolbar;
			} else {
				self.toolbar_window_visible = false;
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
