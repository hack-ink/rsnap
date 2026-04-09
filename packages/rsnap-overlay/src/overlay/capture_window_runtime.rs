#[allow(unused_imports)]
use crate::overlay::{GlobalPoint, MonitorRect, OverlayMode, OverlaySession};

impl OverlaySession {
	pub(super) fn update_cursor_state(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		self.cursor_monitor = Some(monitor);
		self.state.cursor = Some(cursor);
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
}
