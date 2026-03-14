use std::collections::VecDeque;
#[cfg(target_os = "macos")]
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use color_eyre::eyre;
use color_eyre::eyre::Result;
use global_hotkey::hotkey::Code;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, hotkey::HotKey};
use tray_icon::{TrayIconEvent, menu::MenuEvent};
use winit::error::EventLoopError;
use winit::event::WindowEvent;
use winit::event_loop::EventLoop;
#[cfg(target_os = "macos")]
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
use winit::{
	application::ApplicationHandler,
	event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy},
	window::WindowId,
};

#[cfg(target_os = "macos")]
use crate::app::scroll_input_macos::SharedScrollInputState;
use crate::app::{App, UserEvent};
use crate::settings::AppSettings;
use crate::settings_window::{CaptureHotkeyNotice, SettingsControl, SettingsWindowAction};

impl ApplicationHandler<UserEvent> for App {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		#[cfg(target_os = "macos")]
		self.install_menubar(event_loop);
		self.install_tray(event_loop);
	}

	fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
		match event {
			UserEvent::Menu(event) => self.handle_menu_event(event_loop, &event),
			UserEvent::HotKey(event) => self.handle_hotkey_event(event_loop, event),
			UserEvent::TrayIcon => {},
			#[cfg(target_os = "macos")]
			UserEvent::OverlayStreamFrame => {
				self.overlay_stream_event_pending.store(false, Ordering::Release);

				if let Some(session) = self.overlay_session.as_mut() {
					let control = session.handle_scroll_stream_frame_ready();

					self.handle_overlay_control(control);
				}
			},
			#[cfg(target_os = "macos")]
			UserEvent::OverlayWorkerResponse => {
				if let Some(session) = self.overlay_session.as_mut() {
					let control = session.handle_worker_response_ready();

					self.handle_overlay_control(control);
				}
			},
		}
	}

	fn window_event(
		&mut self,
		event_loop: &ActiveEventLoop,
		window_id: WindowId,
		event: WindowEvent,
	) {
		if let Some(existing_window) = self.settings_window.as_ref()
			&& existing_window.window_id() == window_id
		{
			let Some(mut settings_window) = self.settings_window.take() else {
				return;
			};
			let mut should_close = false;
			let mut settings_changed = false;
			let mut overlay_changed = false;
			let mut action_queue = VecDeque::new();
			let mut ui_updates: VecDeque<(Option<bool>, Option<Option<CaptureHotkeyNotice>>)> =
				VecDeque::new();

			match event {
				WindowEvent::RedrawRequested => match settings_window.draw(&mut self.settings) {
					Ok(changed) => {
						overlay_changed = changed;
						settings_changed = changed;
					},
					Err(err) => tracing::warn!(error = %err, "Settings window draw failed."),
				},
				_ => match settings_window.handle_window_event(&event) {
					SettingsControl::Continue => {},
					SettingsControl::CloseRequested => {
						should_close = true;

						action_queue.push_back(SettingsWindowAction::Cancel);
					},
				},
			}

			action_queue.extend(settings_window.drain_actions());

			let mut action_changed = false;

			for action in action_queue {
				let (changed, recording_active, notice) = self.apply_settings_window_action(action);

				if let Some(recording_active) = recording_active {
					ui_updates.push_back((Some(recording_active), None));
				}
				if let Some(notice) = notice {
					ui_updates.push_back((None, Some(notice)));
				}

				if changed {
					action_changed = true;
				}
			}

			while let Some((recording_active, notice)) = ui_updates.pop_front() {
				if let Some(recording_active) = recording_active {
					settings_window.set_capture_hotkey_recording_active(recording_active);
				}
				if let Some(notice) = notice {
					settings_window.set_capture_hotkey_notice(notice);
				}
			}

			if action_changed {
				settings_changed = true;
			}
			if overlay_changed {
				self.apply_overlay_settings();
			}
			if settings_changed && let Err(err) = self.settings.save() {
				tracing::warn!(error = ?err, "Failed to save settings.");
			}
			if should_close {
				return;
			}

			self.settings_window = Some(settings_window);

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

pub(super) fn run() -> Result<()> {
	let settings = AppSettings::load();
	let capture_hotkey = settings.capture_hotkey();
	let capture_hotkey_id = capture_hotkey.id();
	let settings_hotkey = if cfg!(target_os = "macos") {
		None
	} else {
		Some(HotKey::new(Some(global_hotkey::hotkey::CMD_OR_CTRL), Code::Comma))
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

	let mut event_loop_builder = EventLoop::with_user_event();

	#[cfg(target_os = "macos")]
	{
		event_loop_builder.with_activation_policy(ActivationPolicy::Accessory);
		event_loop_builder.with_activate_ignoring_other_apps(false);
		event_loop_builder.with_default_menu(false);
	}

	let event_loop = event_loop_builder.build()?;
	let tray_proxy: EventLoopProxy<UserEvent> = event_loop.create_proxy();
	#[cfg(target_os = "macos")]
	let overlay_proxy: EventLoopProxy<UserEvent> = event_loop.create_proxy();
	#[cfg(target_os = "macos")]
	let overlay_stream_event_pending = Arc::new(AtomicBool::new(false));
	#[cfg(target_os = "macos")]
	let scroll_input_shared_state = Arc::new(SharedScrollInputState::default());
	let mut app = App::new(
		capture_hotkey,
		settings,
		settings_hotkey,
		hotkey_manager,
		#[cfg(target_os = "macos")]
		overlay_proxy,
		#[cfg(target_os = "macos")]
		overlay_stream_event_pending,
		#[cfg(target_os = "macos")]
		scroll_input_shared_state,
	);

	TrayIconEvent::set_event_handler(Some(move |event| {
		let _ = event;
		let _ = tray_proxy.send_event(UserEvent::TrayIcon);
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
