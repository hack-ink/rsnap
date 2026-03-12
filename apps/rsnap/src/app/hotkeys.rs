use global_hotkey::hotkey::HotKey;

use crate::app::App;
use crate::settings_window::{CaptureHotkeyNotice, SettingsWindow, SettingsWindowAction};

impl App {
	pub(super) fn capture_key_label(&self) -> String {
		self.capture_hotkey.to_string()
	}

	pub(super) fn settings_key_label(&self) -> String {
		self.settings_hotkey
			.as_ref()
			.map(ToString::to_string)
			.unwrap_or_else(|| String::from("disabled"))
	}

	fn suspend_capture_hotkey(&mut self) {
		self.capture_hotkey_recording_suspended = true;

		let Some(manager) = self._hotkey_manager.as_mut() else {
			return;
		};

		if let Err(err) = manager.unregister(self.capture_hotkey) {
			tracing::warn!(error = %err, "Failed to suspend current capture hotkey.");
		}
	}

	fn resume_capture_hotkey(&mut self) {
		if !self.capture_hotkey_recording_suspended {
			return;
		}

		self.capture_hotkey_recording_suspended = false;

		let Some(manager) = self._hotkey_manager.as_mut() else {
			return;
		};

		if let Err(err) = manager.register(self.capture_hotkey) {
			tracing::warn!(error = %err, "Failed to resume capture hotkey.");
		}
	}

	fn apply_capture_hotkey(&mut self, hotkey: HotKey, suspended: bool) -> bool {
		let old_hotkey = self.capture_hotkey;

		if hotkey == old_hotkey {
			self.settings.capture_hotkey = hotkey.to_string();

			if !suspended {
				return true;
			}

			let Some(manager) = self._hotkey_manager.as_mut() else {
				return true;
			};

			if let Err(err) = manager.register(hotkey) {
				tracing::warn!(
					error = %err,
					hotkey = %hotkey.to_string(),
					"Failed to register capture hotkey; keeping existing binding state."
				);

				return false;
			}

			return true;
		}

		let Some(manager) = self._hotkey_manager.as_mut() else {
			self.capture_hotkey = hotkey;
			self.capture_hotkey_id = hotkey.id();
			self.settings.capture_hotkey = hotkey.to_string();

			return true;
		};

		if !suspended && let Err(err) = manager.unregister(old_hotkey) {
			tracing::warn!(error = %err, "Failed to unregister capture hotkey before rebind.");
		}

		if let Err(err) = manager.register(hotkey) {
			tracing::warn!(
				error = %err,
				old_hotkey = %old_hotkey.to_string(),
				new_hotkey = %hotkey.to_string(),
				"Failed to register new capture hotkey; restoring previous."
			);

			if !suspended && let Err(restore_error) = manager.register(old_hotkey) {
				tracing::warn!(error = %restore_error, "Failed to restore previous capture hotkey.");
			}

			return false;
		}

		self.capture_hotkey = hotkey;
		self.capture_hotkey_id = hotkey.id();
		self.settings.capture_hotkey = hotkey.to_string();

		true
	}

	pub(super) fn apply_settings_window_action(
		&mut self,
		action: SettingsWindowAction,
	) -> (bool, Option<bool>, Option<Option<CaptureHotkeyNotice>>) {
		match action {
			SettingsWindowAction::BeginCaptureHotkey => {
				self.suspend_capture_hotkey();

				(false, Some(true), Some(None))
			},
			SettingsWindowAction::CancelCaptureHotkey => {
				self.resume_capture_hotkey();

				(false, Some(false), Some(None))
			},
			SettingsWindowAction::ApplyCaptureHotkey(hotkey) => {
				if self.apply_capture_hotkey(hotkey, self.capture_hotkey_recording_suspended) {
					self.capture_hotkey_recording_suspended = false;

					let display_hotkey =
						SettingsWindow::format_capture_hotkey(&self.capture_hotkey.to_string());

					(
						true,
						Some(false),
						Some(Some(CaptureHotkeyNotice::Success(format!(
							"Capture hotkey updated to {display_hotkey}."
						)))),
					)
				} else {
					tracing::warn!("Capture hotkey update rejected; keeping previous binding.");

					(
						false,
						Some(true),
						Some(Some(CaptureHotkeyNotice::Error(String::from(
							"Capture hotkey unavailable. Try another shortcut.",
						)))),
					)
				}
			},
		}
	}
}
