mod runtime;
#[cfg(target_os = "macos")]
mod scroll_input_macos;
mod shell;

#[cfg(target_os = "macos")]
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};
#[cfg(target_os = "macos")]
use std::thread::JoinHandle;

use color_eyre::eyre::Result;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, hotkey::HotKey};
use tray_icon::{
	TrayIcon, TrayIconEvent,
	menu::{Menu, MenuEvent, MenuId},
};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};

#[cfg(target_os = "macos")]
use self::scroll_input_macos::SharedScrollInputState;
use crate::settings::AppSettings;
use crate::settings_window::CaptureHotkeyNotice;
use crate::settings_window::{SettingsWindow, SettingsWindowAction};
use rsnap_overlay::{HudAnchor, OverlayConfig, OverlayControl, OverlayExit, OverlaySession};

pub enum UserEvent {
	TrayIcon(TrayIconEvent),
	Menu(MenuEvent),
	HotKey(GlobalHotKeyEvent),
	#[cfg(target_os = "macos")]
	OverlayStreamFrame,
	#[cfg(target_os = "macos")]
	OverlayWorkerResponse,
}

struct App {
	capture_hotkey: HotKey,
	capture_hotkey_id: u32,
	settings_hotkey: Option<HotKey>,
	settings_hotkey_id: Option<u32>,
	_hotkey_manager: Option<GlobalHotKeyManager>,
	capture_hotkey_recording_suspended: bool,
	tray_icon: Option<TrayIcon>,
	#[cfg(target_os = "macos")]
	menubar_menu: Option<Menu>,
	settings_menu_id: Option<MenuId>,
	capture_menu_id: Option<MenuId>,
	quit_menu_id: Option<MenuId>,
	#[cfg(target_os = "macos")]
	menubar_settings_menu_id: Option<MenuId>,
	#[cfg(target_os = "macos")]
	menubar_quit_menu_id: Option<MenuId>,
	overlay_session: Option<OverlaySession>,
	settings_window: Option<SettingsWindow>,
	settings: AppSettings,
	#[cfg(target_os = "macos")]
	overlay_proxy: EventLoopProxy<UserEvent>,
	#[cfg(target_os = "macos")]
	overlay_stream_event_pending: Arc<AtomicBool>,
	#[cfg(target_os = "macos")]
	scroll_input_event_tap_thread: Option<JoinHandle<()>>,
	#[cfg(target_os = "macos")]
	scroll_input_shared_state: Arc<SharedScrollInputState>,
}
impl App {
	fn overlay_config(&self) -> OverlayConfig {
		let glass = self.settings.hud_glass_enabled;
		let hud_opacity = self.settings.hud_opacity.clamp(0.0, 1.0);
		let hud_blur = self.settings.hud_blur.clamp(0.0, 1.0);
		let hud_tint = self.settings.hud_tint.clamp(0.0, 1.0);
		let hud_tint_hue = self.settings.hud_tint_hue;
		let loupe_sample_side_px = self.settings.loupe_sample_size.side_px();
		let hud_opaque = !glass || hud_opacity >= 0.999;
		let show_hud_blur = glass && hud_blur > 0.0 && !hud_opaque;

		OverlayConfig {
			hud_anchor: HudAnchor::Cursor,
			show_alt_hint_keycap: self.settings.show_alt_hint_keycap,
			selection_particles: self.settings.selection_particles,
			selection_flow_stroke_width_px: self
				.settings
				.selection_flow_stroke_width_px
				.clamp(1.0, 8.0),
			show_hud_blur,
			hud_opaque,
			hud_opacity,
			hud_fog_amount: hud_blur,
			hud_milk_amount: hud_tint,
			hud_tint_hue,
			alt_activation: Self::map_alt_activation(self.settings.alt_activation),
			toolbar_placement: self.settings.toolbar_placement,
			loupe_sample_side_px,
			theme_mode: self.settings.theme_mode,
			output_dir: self.settings.output_dir.clone(),
			output_filename_prefix: self.settings.output_filename_prefix.clone(),
			output_naming: self.settings.output_naming,
			window_capture_alpha_mode: self.settings.window_capture_alpha_mode,
		}
	}

	fn map_alt_activation(
		mode: crate::settings::AltActivationMode,
	) -> rsnap_overlay::AltActivationMode {
		match mode {
			crate::settings::AltActivationMode::Hold => rsnap_overlay::AltActivationMode::Hold,
			crate::settings::AltActivationMode::Toggle => rsnap_overlay::AltActivationMode::Toggle,
		}
	}

	fn apply_overlay_settings(&mut self) {
		let config = self.overlay_config();
		let Some(session) = self.overlay_session.as_mut() else {
			return;
		};

		session.set_config(config);
	}

	fn new(
		capture_hotkey: HotKey,
		settings: AppSettings,
		settings_hotkey: Option<HotKey>,
		hotkey_manager: Option<GlobalHotKeyManager>,
		#[cfg(target_os = "macos")] overlay_proxy: EventLoopProxy<UserEvent>,
		#[cfg(target_os = "macos")] overlay_stream_event_pending: Arc<AtomicBool>,
		#[cfg(target_os = "macos")] scroll_input_shared_state: Arc<SharedScrollInputState>,
	) -> Self {
		Self {
			capture_hotkey_id: capture_hotkey.id(),
			capture_hotkey,
			settings_hotkey,
			settings_hotkey_id: settings_hotkey.as_ref().map(HotKey::id),
			capture_hotkey_recording_suspended: false,
			_hotkey_manager: hotkey_manager,
			tray_icon: None,
			#[cfg(target_os = "macos")]
			menubar_menu: None,
			settings_menu_id: None,
			capture_menu_id: None,
			quit_menu_id: None,
			#[cfg(target_os = "macos")]
			menubar_settings_menu_id: None,
			#[cfg(target_os = "macos")]
			menubar_quit_menu_id: None,
			overlay_session: None,
			settings_window: None,
			settings,
			#[cfg(target_os = "macos")]
			overlay_proxy,
			#[cfg(target_os = "macos")]
			overlay_stream_event_pending,
			#[cfg(target_os = "macos")]
			scroll_input_event_tap_thread: None,
			#[cfg(target_os = "macos")]
			scroll_input_shared_state,
		}
	}

	fn capture_key_label(&self) -> String {
		self.capture_hotkey.to_string()
	}

	fn settings_key_label(&self) -> String {
		self.settings_hotkey
			.as_ref()
			.map(ToString::to_string)
			.unwrap_or_else(|| String::from("disabled"))
	}

	fn start_capture_session(&mut self, event_loop: &ActiveEventLoop, requested_by: &'static str) {
		if self.overlay_session.is_some() {
			tracing::info!(
				requested_by = %requested_by,
				"Capture already active; ignoring additional start request."
			);

			return;
		}

		let overlay_config = self.overlay_config();
		let mut overlay_session = OverlaySession::with_config(overlay_config);

		#[cfg(target_os = "macos")]
		self.scroll_input_shared_state.clear();
		#[cfg(target_os = "macos")]
		self.scroll_input_shared_state.set_enabled(true);

		#[cfg(target_os = "macos")]
		overlay_session.set_scroll_frame_waker(Arc::new({
			let overlay_proxy = self.overlay_proxy.clone();
			let overlay_stream_event_pending = Arc::clone(&self.overlay_stream_event_pending);

			move || {
				if !begin_coalesced_overlay_user_event_send(&overlay_stream_event_pending) {
					return;
				}
				if overlay_proxy.send_event(UserEvent::OverlayStreamFrame).is_err() {
					overlay_stream_event_pending.store(false, Ordering::Release);
				}
			}
		}));
		#[cfg(target_os = "macos")]
		overlay_session.set_response_waker(Arc::new({
			let overlay_proxy = self.overlay_proxy.clone();

			move || {
				let _ = overlay_proxy.send_event(UserEvent::OverlayWorkerResponse);
			}
		}));
		#[cfg(target_os = "macos")]
		overlay_session.set_external_scroll_input_drain_reader(Arc::new({
			let shared_state = Arc::clone(&self.scroll_input_shared_state);

			move |after_seq, through| shared_state.replay_after_seq_through(after_seq, through)
		}));

		match overlay_session.start(event_loop) {
			Ok(()) => {
				#[cfg(target_os = "macos")]
				self.install_scroll_input_observer();

				tracing::info!(
					requested_by = %requested_by,
					hotkey = %self.capture_key_label(),
					"Capture overlay started."
				);

				self.overlay_session = Some(overlay_session);
			},
			Err(err) => {
				#[cfg(target_os = "macos")]
				{
					self.scroll_input_shared_state.set_enabled(false);
					self.scroll_input_shared_state.clear();
				}

				tracing::warn!(
					error = %err,
					requested_by = %requested_by,
					"Failed to start overlay session."
				)
			},
		}
	}

	fn open_settings_window(&mut self, event_loop: &ActiveEventLoop, requested_by: &'static str) {
		if let Some(window) = self.settings_window.as_ref() {
			tracing::info!(requested_by = %requested_by, "Settings already open; focusing.");

			window.focus();

			return;
		}

		match SettingsWindow::open(event_loop) {
			Ok(window) => {
				tracing::info!(requested_by = %requested_by, "Settings window opened.");

				window.focus();

				self.settings_window = Some(window);
			},
			Err(err) => {
				tracing::warn!(
					error = %err,
					requested_by = %requested_by,
					"Failed to open settings window."
				);
			},
		}
	}

	fn end_overlay_session(&mut self, exit: OverlayExit) {
		let Some(_session) = self.overlay_session.take() else {
			return;
		};

		#[cfg(target_os = "macos")]
		{
			self.scroll_input_shared_state.set_enabled(false);
			self.scroll_input_shared_state.clear();
			self.remove_scroll_input_observer();
		}

		match exit {
			OverlayExit::Cancelled => tracing::info!("Capture cancelled."),
			OverlayExit::PngBytes(png_bytes) => {
				tracing::info!(bytes = png_bytes.len(), "Capture copied to clipboard.");
			},
			OverlayExit::Saved(path) => {
				tracing::info!(path = %path.display(), "Capture saved to file.");
			},
			OverlayExit::Error(message) => tracing::warn!(error = %message, "Capture failed."),
		};

		tracing::info!("Capture overlay ended.");
	}

	#[cfg(target_os = "macos")]
	fn install_scroll_input_observer(&mut self) {
		if self.scroll_input_event_tap_thread.is_some() {
			return;
		}

		let handle = scroll_input_macos::spawn_scroll_input_observer(Arc::clone(
			&self.scroll_input_shared_state,
		));

		self.scroll_input_event_tap_thread = Some(handle);
	}

	#[cfg(target_os = "macos")]
	fn remove_scroll_input_observer(&mut self) {}

	fn handle_overlay_control(&mut self, control: OverlayControl) {
		let OverlayControl::Exit(exit) = control else {
			return;
		};

		self.end_overlay_session(exit);
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

	fn apply_settings_window_action(
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

pub fn run() -> Result<()> {
	runtime::run()
}

#[cfg(target_os = "macos")]
fn begin_coalesced_overlay_user_event_send(pending: &AtomicBool) -> bool {
	pending.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok()
}

#[cfg(test)]
mod tests {
	#[cfg(target_os = "macos")]
	use std::sync::atomic::AtomicBool;

	#[cfg(target_os = "macos")]
	#[test]
	fn begin_coalesced_overlay_user_event_send_only_allows_first_sender_per_flag() {
		let pending = AtomicBool::new(false);

		assert!(super::begin_coalesced_overlay_user_event_send(&pending));
		assert!(!super::begin_coalesced_overlay_user_event_send(&pending));
	}
}
