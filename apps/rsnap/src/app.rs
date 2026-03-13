mod capture;
mod hotkeys;
mod runtime;
#[cfg(target_os = "macos")]
mod scroll_input_macos;
mod shell;

#[cfg(target_os = "macos")]
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};

use color_eyre::eyre::Result;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, hotkey::HotKey};
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
use self::scroll_input_macos::SharedScrollInputState;
use crate::settings::AppSettings;
use crate::settings_window::SettingsWindow;
use rsnap_overlay::OverlaySession;

pub(crate) enum UserEvent {
	TrayIcon,
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
	scroll_input_observer_started: bool,
	#[cfg(target_os = "macos")]
	scroll_input_shared_state: Arc<SharedScrollInputState>,
}
impl App {
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
			scroll_input_observer_started: false,
			#[cfg(target_os = "macos")]
			scroll_input_shared_state,
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
}

pub(crate) fn run() -> Result<()> {
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
