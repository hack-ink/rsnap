use crate::overlay::{LiveSampleApplyResult, MonitorRect, OverlayMode, OverlaySession};

impl OverlaySession {
	pub(in crate::overlay) fn request_redraw_all(&self) {
		for w in self.windows.values() {
			w.window.request_redraw();
		}

		if let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
		if let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}
		if let Some(toolbar) = self.toolbar_window.as_ref() {
			toolbar.window.request_redraw();
		}
		if let Some(preview) = self.scroll_preview_window.as_ref() {
			preview.window.request_redraw();
		}
	}

	pub(in crate::overlay) fn request_redraw_for_monitor(&self, monitor: MonitorRect) {
		for w in self.windows.values() {
			if w.monitor == monitor {
				w.window.request_redraw();
			}
		}

		let hide_auxiliary_windows = self.frozen_selection_drag_hides_auxiliary_windows();
		let hide_live_drag_auxiliary_windows = self.live_capture_hides_auxiliary_windows();
		let request_hud_window = !hide_auxiliary_windows
			&& !hide_live_drag_auxiliary_windows
			&& self.hud_window.is_some();
		let request_loupe_window = !hide_auxiliary_windows
			&& !hide_live_drag_auxiliary_windows
			&& self.loupe_window.is_some();
		let request_toolbar_window = !hide_auxiliary_windows
			&& cfg!(target_os = "macos")
			&& self.frozen_display_ready_for_monitor(monitor)
			&& self.toolbar_state.visible;
		let request_scroll_preview_window =
			!hide_auxiliary_windows && self.scroll_preview_window.is_some();

		if tracing::enabled!(tracing::Level::TRACE)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.frozen_selection_drag.active
			&& self.state.monitor == Some(monitor)
		{
			let overlay_windows =
				self.windows.values().filter(|window| window.monitor == monitor).count();

			tracing::trace!(
				op = "overlay.frozen_selection_drag.redraw_fanout",
				monitor_id = monitor.id,
				overlay_window_count = overlay_windows,
				request_hud_window,
				request_loupe_window,
				request_toolbar_window,
				request_scroll_preview_window,
				hide_auxiliary_windows,
				scroll_capture_active = self.scroll_capture.active,
				alt_held = self.state.alt_held,
				"Requested redraw fan-out for frozen selection drag."
			);
		}
		if hide_auxiliary_windows {
			return;
		}
		if request_hud_window && let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
		if request_loupe_window && let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}
		// macOS uses a native toolbar popup window with compositor blur; keep shader-viewport
		// toolbar redraw on the fullscreen overlay path disabled for this platform.
		// Future direction: if toolbar styling moves off native blur, add a dedicated capture
		// pass feeding a toolbar-local shader-blur texture.
		if request_toolbar_window {
			self.request_redraw_toolbar_window();
		}
		if request_scroll_preview_window {
			self.request_redraw_scroll_preview_window();
		}
	}

	pub(in crate::overlay) fn request_redraw_hud_window(&self) {
		if self.frozen_selection_drag_hides_auxiliary_windows()
			|| self.live_capture_hides_auxiliary_windows()
		{
			return;
		}

		if let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
	}

	pub(in crate::overlay) fn request_redraw_toolbar_window(&self) {
		if self.frozen_selection_drag_hides_auxiliary_windows() {
			return;
		}

		if let Some(toolbar) = self.toolbar_window.as_ref() {
			toolbar.window.request_redraw();
		}
	}

	pub(in crate::overlay) fn request_redraw_loupe_window(&self) {
		if self.frozen_selection_drag_hides_auxiliary_windows()
			|| self.live_capture_hides_auxiliary_windows()
		{
			return;
		}

		if let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}
	}

	pub(in crate::overlay) fn request_redraw_scroll_preview_window(&self) {
		if self.frozen_selection_drag_hides_auxiliary_windows() {
			return;
		}

		if let Some(preview) = self.scroll_preview_window.as_ref() {
			preview.window.request_redraw();
		}
	}

	pub(in crate::overlay) fn request_redraw_live_sample_targets(
		&self,
		monitor: MonitorRect,
		apply: LiveSampleApplyResult,
	) {
		if apply.overlay_changed {
			for window in self.windows.values() {
				if window.monitor == monitor {
					window.window.request_redraw();
				}
			}
		}
		if apply.hud_changed {
			self.request_redraw_hud_window();
		}
		if apply.loupe_changed {
			if self.live_loupe_uses_hud_window() {
				self.request_redraw_hud_window();
			} else {
				self.request_redraw_loupe_window();
			}
		}
	}
}
