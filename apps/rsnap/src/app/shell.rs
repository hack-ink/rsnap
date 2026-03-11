use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use tray_icon::menu::MenuEvent;
#[cfg(target_os = "macos")]
use tray_icon::menu::Submenu;
use tray_icon::menu::{MenuItem, PredefinedMenuItem, accelerator};
use winit::event_loop::ActiveEventLoop;

use crate::app::App;
use crate::icon;
use rsnap_overlay::OverlayExit;

impl App {
	#[cfg(target_os = "macos")]
	pub(super) fn install_menubar(&mut self, event_loop: &ActiveEventLoop) {
		if self.menubar_menu.is_some() {
			return;
		}

		let menubar = tray_icon::menu::Menu::new();
		let settings_item = MenuItem::new(
			"Settings…",
			true,
			Some(accelerator::Accelerator::new(
				Some(accelerator::Modifiers::SUPER),
				accelerator::Code::Comma,
			)),
		);
		let quit_item = MenuItem::new(
			"Quit rsnap",
			true,
			Some(accelerator::Accelerator::new(
				Some(accelerator::Modifiers::SUPER),
				accelerator::Code::KeyQ,
			)),
		);
		let app_menu = match Submenu::with_items(
			"rsnap",
			true,
			&[&settings_item, &PredefinedMenuItem::separator(), &quit_item],
		) {
			Ok(menu) => menu,
			Err(err) => {
				tracing::warn!(error = ?err, "Failed to build menubar menu.");

				event_loop.exit();

				return;
			},
		};

		if let Err(err) = menubar.append(&app_menu) {
			tracing::warn!(error = ?err, "Failed to append menubar submenu.");

			event_loop.exit();

			return;
		}

		menubar.init_for_nsapp();

		self.menubar_settings_menu_id = Some(settings_item.id().clone());
		self.menubar_quit_menu_id = Some(quit_item.id().clone());
		self.menubar_menu = Some(menubar);
	}

	pub(super) fn install_tray(&mut self, event_loop: &ActiveEventLoop) {
		if self.tray_icon.is_some() {
			return;
		}

		let tray_menu = tray_icon::menu::Menu::new();
		let capture_item = MenuItem::new(
			"Capture",
			true,
			Some(accelerator::Accelerator::new(
				Some(accelerator::Modifiers::ALT),
				accelerator::Code::KeyX,
			)),
		);
		let settings_item = MenuItem::new(
			"Settings…",
			true,
			Some(accelerator::Accelerator::new(
				Some(accelerator::CMD_OR_CTRL),
				accelerator::Code::Comma,
			)),
		);
		let quit_item = MenuItem::new(
			"Quit",
			true,
			Some(accelerator::Accelerator::new(
				Some(accelerator::CMD_OR_CTRL),
				accelerator::Code::KeyQ,
			)),
		);

		if let Err(err) = tray_menu.append_items(&[
			&capture_item,
			&PredefinedMenuItem::separator(),
			&settings_item,
			&PredefinedMenuItem::separator(),
			&quit_item,
		]) {
			tracing::warn!(error = ?err, "Failed to build tray menu.");

			event_loop.exit();

			return;
		}

		let icon = match icon::default_tray_icon() {
			Ok(icon) => icon,
			Err(err) => {
				tracing::warn!(error = ?err, "Failed to create tray icon image.");

				event_loop.exit();

				return;
			},
		};
		let tray_icon = match tray_icon::TrayIconBuilder::new()
			.with_tooltip("rsnap")
			.with_menu(Box::new(tray_menu))
			.with_icon(icon)
			.with_icon_as_template(cfg!(target_os = "macos"))
			.build()
		{
			Ok(icon) => icon,
			Err(err) => {
				tracing::warn!(error = ?err, "Failed to build tray icon.");

				event_loop.exit();

				return;
			},
		};

		self.settings_menu_id = Some(settings_item.id().clone());
		self.capture_menu_id = Some(capture_item.id().clone());
		self.quit_menu_id = Some(quit_item.id().clone());
		self.tray_icon = Some(tray_icon);
	}

	pub(super) fn handle_menu_event(&mut self, event_loop: &ActiveEventLoop, event: &MenuEvent) {
		let id = event.id();
		let mut handled = false;

		if Some(id) == self.settings_menu_id.as_ref() {
			handled = true;

			tracing::info!("Settings requested from tray menu.");

			self.open_settings_window(event_loop, "tray-menu");
		}
		if Some(id) == self.capture_menu_id.as_ref() {
			handled = true;

			tracing::info!("Capture requested from tray menu.");

			self.start_capture_session(event_loop, "tray-menu");
		}
		if Some(id) == self.quit_menu_id.as_ref() {
			handled = true;

			tracing::info!("Quit requested from tray menu.");

			self.end_overlay_session(OverlayExit::Cancelled);

			self.settings_window = None;

			event_loop.exit();
		}

		#[cfg(target_os = "macos")]
		{
			if Some(id) == self.menubar_settings_menu_id.as_ref() {
				handled = true;

				tracing::info!("Settings requested from menubar menu.");

				self.open_settings_window(event_loop, "menubar-menu");
			}
			if Some(id) == self.menubar_quit_menu_id.as_ref() {
				handled = true;

				tracing::info!("Quit requested from menubar menu.");

				self.end_overlay_session(OverlayExit::Cancelled);

				self.settings_window = None;

				event_loop.exit();
			}
		}

		if !handled {
			tracing::warn!(menu_id = ?id.as_ref(), "Ignoring unknown menu event.");
		}
	}

	pub(super) fn handle_hotkey_event(
		&mut self,
		event_loop: &ActiveEventLoop,
		event: GlobalHotKeyEvent,
	) {
		if event.state() != HotKeyState::Pressed {
			return;
		}
		if event.id() == self.capture_hotkey_id {
			tracing::info!(
				hotkey = %self.capture_key_label(),
				"Capture requested from hotkey."
			);

			self.start_capture_session(event_loop, "global-hotkey");
		} else if self.settings_hotkey_id == Some(event.id()) {
			tracing::info!(
				hotkey = %self.settings_key_label(),
				"Settings requested from hotkey."
			);

			self.open_settings_window(event_loop, "global-hotkey");
		}
	}
}
