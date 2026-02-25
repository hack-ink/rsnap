use std::time::{Duration, Instant};

use color_eyre::eyre;
use color_eyre::eyre::Result;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use tray_icon::{
	TrayIcon, TrayIconBuilder, TrayIconEvent,
	menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
};
use winit::error::EventLoopError;
use winit::event::WindowEvent;
use winit::{
	application::ApplicationHandler,
	event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy},
	window::WindowId,
};

use crate::icon;
use crate::settings::AppSettings;
use crate::settings_window::{SettingsControl, SettingsWindow};
use rsnap_overlay::{HudAnchor, OverlayConfig, OverlayControl, OverlayExit, OverlaySession};

pub enum UserEvent {
	TrayIcon(TrayIconEvent),
	Menu(MenuEvent),
	HotKey(GlobalHotKeyEvent),
}

struct App {
	capture_hotkey: HotKey,
	capture_hotkey_id: u32,
	settings_hotkey: Option<HotKey>,
	settings_hotkey_id: Option<u32>,
	_hotkey_manager: Option<GlobalHotKeyManager>,
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
}
impl App {
	fn overlay_config(&self) -> OverlayConfig {
		OverlayConfig {
			hud_anchor: HudAnchor::Cursor,
			show_alt_hint_keycap: self.settings.show_alt_hint_keycap,
			show_hud_blur: self.settings.show_hud_blur,
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
		settings_hotkey: Option<HotKey>,
		hotkey_manager: Option<GlobalHotKeyManager>,
	) -> Self {
		Self {
			capture_hotkey_id: capture_hotkey.id(),
			capture_hotkey,
			settings_hotkey,
			settings_hotkey_id: settings_hotkey.as_ref().map(HotKey::id),
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
			settings: AppSettings::load(),
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

		match overlay_session.start(event_loop) {
			Ok(()) => {
				tracing::info!(
					requested_by = %requested_by,
					hotkey = %self.capture_key_label(),
					"Capture overlay started."
				);

				self.overlay_session = Some(overlay_session);
			},
			Err(err) => tracing::warn!(
				error = %err,
				requested_by = %requested_by,
				"Failed to start overlay session."
			),
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

	#[cfg(target_os = "macos")]
	fn install_menubar(&mut self, event_loop: &ActiveEventLoop) {
		if self.menubar_menu.is_some() {
			return;
		}

		use tray_icon::menu::accelerator;

		use tray_icon::menu::Submenu;

		let menubar = Menu::new();
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

	fn install_tray(&mut self, event_loop: &ActiveEventLoop) {
		if self.tray_icon.is_some() {
			return;
		}

		use tray_icon::menu::accelerator;

		let tray_menu = Menu::new();
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
		let tray_icon = match TrayIconBuilder::new()
			.with_tooltip("rsnap")
			.with_menu(Box::new(tray_menu))
			.with_icon(icon)
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

	fn end_overlay_session(&mut self, exit: OverlayExit) {
		let Some(_session) = self.overlay_session.take() else {
			return;
		};

		match exit {
			OverlayExit::Cancelled => tracing::info!("Capture cancelled."),
			OverlayExit::PngBytes(png_bytes) => {
				tracing::info!(bytes = png_bytes.len(), "Capture copied to clipboard.");
			},
			OverlayExit::Error(message) => tracing::warn!(error = %message, "Capture failed."),
		};

		tracing::info!("Capture overlay ended.");
	}

	fn handle_overlay_control(&mut self, control: OverlayControl) {
		let OverlayControl::Exit(exit) = control else {
			return;
		};

		self.end_overlay_session(exit);
	}
}

impl ApplicationHandler<UserEvent> for App {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		#[cfg(target_os = "macos")]
		self.install_menubar(event_loop);
		self.install_tray(event_loop);
	}

	fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
		match event {
			UserEvent::Menu(event) => {
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
			},

			UserEvent::HotKey(event) => {
				if event.state() == HotKeyState::Pressed {
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
			},

			UserEvent::TrayIcon(_) => {},
		}
	}

	fn window_event(
		&mut self,
		event_loop: &ActiveEventLoop,
		window_id: WindowId,
		event: WindowEvent,
	) {
		if let Some(settings_window) = self.settings_window.as_mut()
			&& settings_window.window_id() == window_id
		{
			match event {
				WindowEvent::RedrawRequested => match settings_window.draw(&mut self.settings) {
					Ok(changed) => {
						if changed {
							self.apply_overlay_settings();
						}
						if changed && let Err(err) = self.settings.save() {
							tracing::warn!(error = ?err, "Failed to save settings.");
						}
					},
					Err(err) => tracing::warn!(error = %err, "Settings window draw failed."),
				},
				_ => match settings_window.handle_window_event(&event) {
					SettingsControl::Continue => {},
					SettingsControl::CloseRequested => {
						self.settings_window = None;
					},
				},
			}

			return;
		}
		if let Some(session) = self.overlay_session.as_mut() {
			let control = session.handle_window_event(window_id, &event);

			self.handle_overlay_control(control);
		} else if let WindowEvent::CloseRequested = event {
			event_loop.exit();
		}
	}

	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		if self.overlay_session.is_some() || self.settings_window.is_some() {
			event_loop.set_control_flow(ControlFlow::WaitUntil(
				Instant::now() + Duration::from_millis(16),
			));
		} else {
			event_loop.set_control_flow(ControlFlow::Wait);
		}

		if let Some(session) = self.overlay_session.as_mut() {
			let control = session.about_to_wait();

			self.handle_overlay_control(control);
		}
	}
}

pub fn run() -> Result<()> {
	let capture_hotkey =
		HotKey::new(Some(global_hotkey::hotkey::Modifiers::ALT), global_hotkey::hotkey::Code::KeyX);
	let capture_hotkey_id = capture_hotkey.id();
	let settings_hotkey = if cfg!(target_os = "macos") {
		None
	} else {
		Some(HotKey::new(
			Some(global_hotkey::hotkey::CMD_OR_CTRL),
			global_hotkey::hotkey::Code::Comma,
		))
	};
	let settings_hotkey_id = settings_hotkey.as_ref().map(HotKey::id);
	let mut hotkey_manager = match GlobalHotKeyManager::new() {
		Ok(manager) => Some(manager),
		Err(err) => {
			tracing::warn!(error = ?err, "Failed to create global hotkey manager.");

			None
		},
	};

	if let Some(manager) = hotkey_manager.as_mut() {
		if let Err(err) = manager.register(capture_hotkey) {
			tracing::warn!(
				error = ?err,
				hotkey_id = %capture_hotkey_id,
				"Failed to register capture hotkey."
			);
		} else {
			tracing::info!(hotkey_id = %capture_hotkey_id, "Registered capture hotkey.");
		}
		if let Some(settings_hotkey) = settings_hotkey.as_ref() {
			if let Err(err) = manager.register(*settings_hotkey) {
				tracing::warn!(
					error = ?err,
					hotkey_id = %settings_hotkey_id.unwrap_or_default(),
					"Failed to register settings hotkey."
				);
			} else {
				tracing::info!(
					hotkey_id = %settings_hotkey_id.unwrap_or_default(),
					"Registered settings hotkey."
				);
			}
		}
	}

	let mut event_loop_builder = winit::event_loop::EventLoop::<UserEvent>::with_user_event();

	#[cfg(target_os = "macos")]
	{
		use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

		event_loop_builder.with_activation_policy(ActivationPolicy::Accessory);
		event_loop_builder.with_activate_ignoring_other_apps(false);
		event_loop_builder.with_default_menu(false);
	}

	let event_loop = event_loop_builder.build()?;
	let tray_proxy: EventLoopProxy<UserEvent> = event_loop.create_proxy();
	let mut app = App::new(capture_hotkey, settings_hotkey, hotkey_manager);

	TrayIconEvent::set_event_handler(Some(move |event| {
		let _ = tray_proxy.send_event(UserEvent::TrayIcon(event));
	}));

	let menu_proxy: EventLoopProxy<UserEvent> = event_loop.create_proxy();

	MenuEvent::set_event_handler(Some(move |event| {
		let _ = menu_proxy.send_event(UserEvent::Menu(event));
	}));

	let hotkey_proxy: EventLoopProxy<UserEvent> = event_loop.create_proxy();

	GlobalHotKeyEvent::set_event_handler(Some(move |event| {
		let _ = hotkey_proxy.send_event(UserEvent::HotKey(event));
	}));

	tracing::info!(
		hotkey = %app.capture_key_label(),
		settings_hotkey = %app.settings_key_label(),
		"Starting menubar-only rsnap app."
	);

	event_loop.run_app(&mut app).map_err(|err: EventLoopError| eyre::eyre!(err))?;

	Ok(())
}
