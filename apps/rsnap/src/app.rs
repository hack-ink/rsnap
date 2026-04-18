mod capture;
#[cfg(target_os = "macos")]
mod capture_host_macos;
mod hotkeys;
mod runtime;
#[cfg(target_os = "macos")]
mod scroll_input_macos;
mod shell;

#[cfg(target_os = "macos")]
use std::sync::{
	Arc,
	atomic::{AtomicBool, AtomicU64, Ordering},
};
#[cfg(target_os = "macos")]
use std::time::Instant;

use color_eyre::eyre::Result;
#[cfg(target_os = "macos")]
use global_hotkey::Error;
#[cfg(target_os = "macos")]
use global_hotkey::hotkey::{Code, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, hotkey::HotKey};
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::NSSound;
#[cfg(target_os = "macos")]
use tray_icon::menu::Menu;
use tray_icon::{
	TrayIcon,
	menu::{MenuEvent, MenuId},
};
use winit::event_loop::ActiveEventLoop;
#[cfg(target_os = "macos")]
use winit::event_loop::EventLoopProxy;

#[cfg(target_os = "macos")]
use self::scroll_input_macos::ScrollInputObserverLifecycle;
#[cfg(target_os = "macos")]
use self::scroll_input_macos::SharedScrollInputState;
#[cfg(target_os = "macos")]
use crate::permissions_macos;
use crate::settings::AppSettings;
use crate::settings_window::{SettingsWindow, SettingsWindowEntry};
use rsnap_overlay::OverlaySession;
#[cfg(target_os = "macos")]
use rsnap_overlay::{FrozenGlobalHotkey, MacOSCaptureHost, MacOSNativeCaptureInputEvent};

pub(crate) enum UserEvent {
	TrayIcon,
	Menu(MenuEvent),
	HotKey(GlobalHotKeyEvent),
	#[cfg(target_os = "macos")]
	OverlayStartupAuxWindows(u64),
	#[cfg(target_os = "macos")]
	OverlayStreamFrame,
	#[cfg(target_os = "macos")]
	OverlayScrollInput,
	#[cfg(target_os = "macos")]
	OverlayWorkerResponse,
	#[cfg(target_os = "macos")]
	OverlayNativeCaptureInput(u64, MacOSNativeCaptureInputEvent),
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OverlayHotkeyRegistrationState {
	Unregistered,
	Registered,
	Blocked,
}
#[cfg(target_os = "macos")]
impl OverlayHotkeyRegistrationState {
	fn allows_register_attempt(self) -> bool {
		matches!(self, Self::Unregistered)
	}

	fn next_state_after_register_error(error: &Error) -> Self {
		match error {
			Error::AlreadyRegistered(_) => Self::Registered,
			_ => Self::Blocked,
		}
	}
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OverlayFrozenHotkeyBinding {
	action: FrozenGlobalHotkey,
	hotkey: HotKey,
	hotkey_id: u32,
	registration_state: OverlayHotkeyRegistrationState,
	label: &'static str,
}
#[cfg(target_os = "macos")]
impl OverlayFrozenHotkeyBinding {
	fn new(action: FrozenGlobalHotkey, hotkey: HotKey, label: &'static str) -> Self {
		Self {
			action,
			hotkey_id: hotkey.id(),
			hotkey,
			registration_state: OverlayHotkeyRegistrationState::Unregistered,
			label,
		}
	}
}

struct App {
	capture_hotkey: HotKey,
	capture_hotkey_id: u32,
	settings_hotkey: Option<HotKey>,
	settings_hotkey_id: Option<u32>,
	_hotkey_manager: Option<GlobalHotKeyManager>,
	#[cfg(target_os = "macos")]
	overlay_cancel_hotkey: HotKey,
	#[cfg(target_os = "macos")]
	overlay_cancel_hotkey_id: u32,
	#[cfg(target_os = "macos")]
	overlay_cancel_hotkey_registration_state: OverlayHotkeyRegistrationState,
	#[cfg(target_os = "macos")]
	overlay_loupe_hotkey: HotKey,
	#[cfg(target_os = "macos")]
	overlay_loupe_hotkey_id: u32,
	#[cfg(target_os = "macos")]
	overlay_loupe_hotkey_registration_state: OverlayHotkeyRegistrationState,
	#[cfg(target_os = "macos")]
	overlay_frozen_hotkeys: Vec<OverlayFrozenHotkeyBinding>,
	capture_hotkey_recording_suspended: bool,
	tray_icon: Option<TrayIcon>,
	#[cfg(target_os = "macos")]
	menubar_menu: Option<Menu>,
	settings_menu_id: Option<MenuId>,
	#[cfg(target_os = "macos")]
	permissions_menu_id: Option<MenuId>,
	capture_menu_id: Option<MenuId>,
	quit_menu_id: Option<MenuId>,
	#[cfg(target_os = "macos")]
	menubar_permissions_menu_id: Option<MenuId>,
	#[cfg(target_os = "macos")]
	menubar_settings_menu_id: Option<MenuId>,
	#[cfg(target_os = "macos")]
	menubar_quit_menu_id: Option<MenuId>,
	overlay_session: Option<OverlaySession>,
	#[cfg(target_os = "macos")]
	overlay_capture_host: Option<MacOSCaptureHost>,
	#[cfg(target_os = "macos")]
	prewarmed_overlay_session: Option<OverlaySession>,
	settings_window: Option<SettingsWindow>,
	settings_window_capture_window_id: Option<u32>,
	settings: AppSettings,
	#[cfg(target_os = "macos")]
	capture_success_sound: Option<Retained<NSSound>>,
	#[cfg(target_os = "macos")]
	overlay_proxy: EventLoopProxy<UserEvent>,
	#[cfg(target_os = "macos")]
	scroll_input_observer_lifecycle: Arc<ScrollInputObserverLifecycle>,
	#[cfg(target_os = "macos")]
	scroll_input_shared_state: Arc<SharedScrollInputState>,
	#[cfg(target_os = "macos")]
	overlay_stream_event_pending: Arc<AtomicBool>,
	#[cfg(target_os = "macos")]
	latest_deferred_ocr_generation: Arc<AtomicU64>,
	#[cfg(target_os = "macos")]
	pending_deferred_ocr_generation: Arc<AtomicU64>,
	#[cfg(target_os = "macos")]
	overlay_session_generation: u64,
	#[cfg(target_os = "macos")]
	overlay_session_prewarm_requested: bool,
	#[cfg(target_os = "macos")]
	overlay_session_prewarm_retry_not_before: Option<Instant>,
	#[cfg(target_os = "macos")]
	startup_permissions_checked: bool,
}
impl App {
	#[cfg(target_os = "macos")]
	fn overlay_cancel_hotkey() -> HotKey {
		HotKey::new(None, Code::Escape)
	}

	#[cfg(target_os = "macos")]
	fn overlay_loupe_hotkey() -> HotKey {
		HotKey::new(None, Code::Tab)
	}

	#[cfg(target_os = "macos")]
	fn overlay_frozen_hotkeys() -> Vec<OverlayFrozenHotkeyBinding> {
		vec![
			OverlayFrozenHotkeyBinding::new(
				FrozenGlobalHotkey::Copy,
				HotKey::new(None, Code::Space),
				"Space",
			),
			OverlayFrozenHotkeyBinding::new(
				FrozenGlobalHotkey::AutoCenter,
				HotKey::new(None, Code::KeyC),
				"C",
			),
			OverlayFrozenHotkeyBinding::new(
				FrozenGlobalHotkey::ToggleToolbar,
				HotKey::new(None, Code::KeyH),
				"H",
			),
			OverlayFrozenHotkeyBinding::new(
				FrozenGlobalHotkey::StartScrollCapture,
				HotKey::new(None, Code::KeyS),
				"S",
			),
			OverlayFrozenHotkeyBinding::new(
				FrozenGlobalHotkey::Save,
				HotKey::new(Some(Modifiers::SUPER), Code::KeyS),
				"Cmd+S",
			),
		]
	}

	#[allow(clippy::too_many_arguments)]
	fn new(
		capture_hotkey: HotKey,
		settings: AppSettings,
		settings_hotkey: Option<HotKey>,
		hotkey_manager: Option<GlobalHotKeyManager>,
		#[cfg(target_os = "macos")] overlay_proxy: EventLoopProxy<UserEvent>,
		#[cfg(target_os = "macos")] scroll_input_observer_lifecycle: Arc<
			ScrollInputObserverLifecycle,
		>,
		#[cfg(target_os = "macos")] scroll_input_shared_state: Arc<SharedScrollInputState>,
	) -> Self {
		Self {
			capture_hotkey_id: capture_hotkey.id(),
			capture_hotkey,
			settings_hotkey,
			settings_hotkey_id: settings_hotkey.as_ref().map(HotKey::id),
			#[cfg(target_os = "macos")]
			overlay_cancel_hotkey: Self::overlay_cancel_hotkey(),
			#[cfg(target_os = "macos")]
			overlay_cancel_hotkey_id: Self::overlay_cancel_hotkey().id(),
			#[cfg(target_os = "macos")]
			overlay_cancel_hotkey_registration_state: OverlayHotkeyRegistrationState::Unregistered,
			#[cfg(target_os = "macos")]
			overlay_loupe_hotkey: Self::overlay_loupe_hotkey(),
			#[cfg(target_os = "macos")]
			overlay_loupe_hotkey_id: Self::overlay_loupe_hotkey().id(),
			#[cfg(target_os = "macos")]
			overlay_loupe_hotkey_registration_state: OverlayHotkeyRegistrationState::Unregistered,
			#[cfg(target_os = "macos")]
			overlay_frozen_hotkeys: Self::overlay_frozen_hotkeys(),
			capture_hotkey_recording_suspended: false,
			_hotkey_manager: hotkey_manager,
			tray_icon: None,
			#[cfg(target_os = "macos")]
			menubar_menu: None,
			settings_menu_id: None,
			#[cfg(target_os = "macos")]
			permissions_menu_id: None,
			capture_menu_id: None,
			quit_menu_id: None,
			#[cfg(target_os = "macos")]
			menubar_permissions_menu_id: None,
			#[cfg(target_os = "macos")]
			menubar_settings_menu_id: None,
			#[cfg(target_os = "macos")]
			menubar_quit_menu_id: None,
			overlay_session: None,
			#[cfg(target_os = "macos")]
			overlay_capture_host: None,
			#[cfg(target_os = "macos")]
			prewarmed_overlay_session: None,
			settings_window: None,
			settings_window_capture_window_id: None,
			settings,
			#[cfg(target_os = "macos")]
			capture_success_sound: Self::load_capture_success_sound(),
			#[cfg(target_os = "macos")]
			overlay_proxy,
			#[cfg(target_os = "macos")]
			scroll_input_observer_lifecycle,
			#[cfg(target_os = "macos")]
			scroll_input_shared_state,
			#[cfg(target_os = "macos")]
			overlay_stream_event_pending: Arc::new(AtomicBool::new(false)),
			#[cfg(target_os = "macos")]
			latest_deferred_ocr_generation: Arc::new(AtomicU64::new(0)),
			#[cfg(target_os = "macos")]
			pending_deferred_ocr_generation: Arc::new(AtomicU64::new(0)),
			#[cfg(target_os = "macos")]
			overlay_session_generation: 0,
			#[cfg(target_os = "macos")]
			overlay_session_prewarm_requested: true,
			#[cfg(target_os = "macos")]
			overlay_session_prewarm_retry_not_before: None,
			#[cfg(target_os = "macos")]
			startup_permissions_checked: false,
		}
	}

	#[cfg(target_os = "macos")]
	fn finish_coalesced_overlay_stream_frame_send(&self) {
		self.overlay_stream_event_pending.store(false, Ordering::Release);
	}

	fn open_settings_window(&mut self, event_loop: &ActiveEventLoop, requested_by: &'static str) {
		if let Some(window) = self.settings_window.as_ref() {
			tracing::info!(requested_by = %requested_by, "Settings already open; focusing.");

			window.focus();

			return;
		}

		let entry = settings_window_entry(requested_by);

		match SettingsWindow::open(event_loop, entry) {
			Ok(window) => {
				tracing::info!(requested_by = %requested_by, "Settings window opened.");

				window.focus();

				self.settings_window = Some(window);
				self.settings_window_capture_window_id =
					self.settings_window.as_ref().and_then(|window| window.capture_window_id());

				self.apply_overlay_settings();
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

	#[cfg(target_os = "macos")]
	fn maybe_present_startup_permissions(&mut self, event_loop: &ActiveEventLoop) {
		if self.startup_permissions_checked {
			return;
		}

		self.startup_permissions_checked = true;

		let screen_recording_granted = permissions_macos::screen_recording_access_granted();
		let accessibility_granted = permissions_macos::accessibility_access_granted();
		let input_monitoring_granted = permissions_macos::input_monitoring_access_granted();

		if screen_recording_granted && accessibility_granted && input_monitoring_granted {
			return;
		}

		tracing::info!(
			screen_recording_granted = screen_recording_granted,
			accessibility_granted = accessibility_granted,
			input_monitoring_granted = input_monitoring_granted,
			"One or more macOS permissions are missing at startup; opening the Settings window."
		);

		self.open_settings_window(event_loop, "startup-permission-check");
	}
}

/// Runs the desktop application event loop until shutdown.
pub fn run() -> Result<()> {
	runtime::run()
}

fn settings_window_entry(requested_by: &'static str) -> SettingsWindowEntry {
	match requested_by {
		"startup-permission-check" => SettingsWindowEntry::Permissions,
		_ => SettingsWindowEntry::Standard,
	}
}

#[cfg(test)]
mod tests {
	#[cfg(target_os = "macos")]
	use global_hotkey::hotkey::{Code, Modifiers};

	#[cfg(target_os = "macos")]
	use crate::app::OverlayHotkeyRegistrationState;
	use crate::app::{self, SettingsWindowEntry};
	#[cfg(target_os = "macos")]
	use rsnap_overlay::FrozenGlobalHotkey;

	#[test]
	fn startup_permission_check_uses_permissions_entry() {
		assert_eq!(
			app::settings_window_entry("startup-permission-check"),
			SettingsWindowEntry::Permissions
		);
	}

	#[test]
	fn non_startup_settings_entries_use_standard_entry() {
		assert_eq!(
			app::settings_window_entry("tray-permissions-menu"),
			SettingsWindowEntry::Standard
		);
		assert_eq!(
			app::settings_window_entry("menubar-permissions-menu"),
			SettingsWindowEntry::Standard
		);
		assert_eq!(app::settings_window_entry("tray-settings-menu"), SettingsWindowEntry::Standard);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn overlay_cancel_hotkey_is_plain_escape() {
		let hotkey = app::App::overlay_cancel_hotkey();

		assert_eq!(hotkey.key, Code::Escape);
		assert_eq!(hotkey.mods, Modifiers::empty());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn overlay_loupe_hotkey_is_plain_tab() {
		let hotkey = app::App::overlay_loupe_hotkey();

		assert_eq!(hotkey.key, Code::Tab);
		assert_eq!(hotkey.mods, Modifiers::empty());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn overlay_frozen_hotkeys_cover_copy_center_toolbar_scroll_and_save() {
		let bindings = app::App::overlay_frozen_hotkeys();

		assert_eq!(bindings.len(), 5);
		assert!(bindings.iter().any(|binding| {
			binding.action == FrozenGlobalHotkey::Copy
				&& binding.hotkey.key == Code::Space
				&& binding.hotkey.mods == Modifiers::empty()
		}));
		assert!(bindings.iter().any(|binding| {
			binding.action == FrozenGlobalHotkey::Save
				&& binding.hotkey.key == Code::KeyS
				&& binding.hotkey.mods == Modifiers::SUPER
		}));
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn blocked_overlay_hotkey_registration_skips_retries_until_reset() {
		assert!(OverlayHotkeyRegistrationState::Unregistered.allows_register_attempt());
		assert!(!OverlayHotkeyRegistrationState::Registered.allows_register_attempt());
		assert!(!OverlayHotkeyRegistrationState::Blocked.allows_register_attempt());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn already_registered_overlay_hotkey_error_keeps_registered_state() {
		let error = global_hotkey::Error::AlreadyRegistered(app::App::overlay_cancel_hotkey());

		assert_eq!(
			OverlayHotkeyRegistrationState::next_state_after_register_error(&error),
			OverlayHotkeyRegistrationState::Registered
		);
	}
}
