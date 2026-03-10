#[cfg(target_os = "macos")]
use std::collections::VecDeque;
#[cfg(target_os = "macos")]
use std::ffi::c_void;
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicBool, AtomicU64, Ordering},
};
#[cfg(target_os = "macos")]
use std::thread::{self, JoinHandle};
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
use crate::settings_window::CaptureHotkeyNotice;
use crate::settings_window::{SettingsControl, SettingsWindow, SettingsWindowAction};
use rsnap_overlay::{HudAnchor, OverlayConfig, OverlayControl, OverlayExit, OverlaySession};

#[cfg(target_os = "macos")]
type CFMachPortRef = *mut c_void;

#[cfg(target_os = "macos")]
type CFRunLoopRef = *mut c_void;

#[cfg(target_os = "macos")]
type CFRunLoopMode = *const c_void;

#[cfg(target_os = "macos")]
type CFAllocatorRef = *const c_void;

#[cfg(target_os = "macos")]
type CGEventRef = *const c_void;

#[cfg(target_os = "macos")]
type CGEventTapProxy = *const c_void;

#[cfg(target_os = "macos")]
const KCG_EVENT_SCROLL_WHEEL: u32 = 22;
#[cfg(target_os = "macos")]
const KCG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
#[cfg(target_os = "macos")]
const KCG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
#[cfg(target_os = "macos")]
const KCG_SESSION_EVENT_TAP: u32 = 1;
#[cfg(target_os = "macos")]
const KCG_HEAD_INSERT_EVENT_TAP: u32 = 0;
#[cfg(target_os = "macos")]
const KCG_EVENT_TAP_LISTEN_ONLY: u32 = 1;
#[cfg(target_os = "macos")]
const KCG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1_FIELD: u32 = 11;
#[cfg(target_os = "macos")]
const KCG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS_FIELD: u32 = 88;
#[cfg(target_os = "macos")]
const KCG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1_FIELD: u32 = 96;
#[cfg(target_os = "macos")]
const KCG_SCROLL_WHEEL_EVENT_SCROLL_PHASE_FIELD: u32 = 99;
#[cfg(target_os = "macos")]
const KCG_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE_FIELD: u32 = 123;
#[cfg(target_os = "macos")]
const NSEVENT_PHASE_BEGAN: u64 = 0x1 << 0;
#[cfg(target_os = "macos")]
const NSEVENT_PHASE_STATIONARY: u64 = 0x1 << 1;
#[cfg(target_os = "macos")]
const NSEVENT_PHASE_CHANGED: u64 = 0x1 << 2;
#[cfg(target_os = "macos")]
const NSEVENT_PHASE_ENDED: u64 = 0x1 << 3;
#[cfg(target_os = "macos")]
const NSEVENT_PHASE_CANCELLED: u64 = 0x1 << 4;
#[cfg(target_os = "macos")]
const NSEVENT_PHASE_MAY_BEGIN: u64 = 0x1 << 5;
#[cfg(target_os = "macos")]
const SHARED_SCROLL_INPUT_QUEUE_CAPACITY: usize = 64;

pub enum UserEvent {
	TrayIcon(TrayIconEvent),
	Menu(MenuEvent),
	HotKey(GlobalHotKeyEvent),
	#[cfg(target_os = "macos")]
	OverlayStreamFrame,
	#[cfg(target_os = "macos")]
	OverlayWorkerResponse,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MacOSCGPoint {
	x: f64,
	y: f64,
}

#[cfg(target_os = "macos")]
struct ScrollInputTapContext {
	shared_state: Arc<SharedScrollInputState>,
	tap: std::sync::atomic::AtomicPtr<c_void>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug)]
struct SharedScrollInputEvent {
	seq: u64,
	recorded_at: Instant,
	delta_y: f64,
	global_x: f64,
	global_y: f64,
	gesture_active: bool,
	gesture_ended: bool,
}
#[cfg(target_os = "macos")]
impl SharedScrollInputEvent {
	fn tuple(self) -> (u64, Instant, f64, f64, f64, bool, bool) {
		(
			self.seq,
			self.recorded_at,
			self.global_x,
			self.global_y,
			self.delta_y,
			self.gesture_active,
			self.gesture_ended,
		)
	}
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq)]
struct DecodedScrollInput {
	raw_delta_y: f64,
	delta_y: f64,
	global_x: f64,
	global_y: f64,
	gesture_active: bool,
	gesture_ended: bool,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct SharedScrollInputQueueState {
	queue: VecDeque<SharedScrollInputEvent>,
	last_recorded: Option<SharedScrollInputEvent>,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct SharedScrollInputState {
	enabled: AtomicBool,
	queue_state: Mutex<SharedScrollInputQueueState>,
	next_seq: AtomicU64,
}
#[cfg(target_os = "macos")]
impl SharedScrollInputState {
	fn set_enabled(&self, enabled: bool) {
		self.enabled.store(enabled, Ordering::Release);
	}

	fn is_enabled(&self) -> bool {
		self.enabled.load(Ordering::Acquire)
	}

	fn clear(&self) {
		let mut queue_state = match self.queue_state.lock() {
			Ok(queue_state) => queue_state,
			Err(poisoned) => poisoned.into_inner(),
		};

		*queue_state = SharedScrollInputQueueState::default();
	}

	fn record(
		&self,
		delta_y: f64,
		global_x: f64,
		global_y: f64,
		gesture_active: bool,
		gesture_ended: bool,
	) -> SharedScrollInputEvent {
		self.record_at(Instant::now(), delta_y, global_x, global_y, gesture_active, gesture_ended)
	}

	fn record_at(
		&self,
		recorded_at: Instant,
		delta_y: f64,
		global_x: f64,
		global_y: f64,
		gesture_active: bool,
		gesture_ended: bool,
	) -> SharedScrollInputEvent {
		let seq = self.next_seq.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
		let mut queue_state = match self.queue_state.lock() {
			Ok(queue_state) => queue_state,
			Err(poisoned) => poisoned.into_inner(),
		};
		let (effective_delta_y, effective_global_x, effective_global_y) =
			if gesture_ended && delta_y == 0.0 {
				match queue_state.last_recorded {
					Some(last_recorded) if last_recorded.delta_y != 0.0 => {
						(last_recorded.delta_y, last_recorded.global_x, last_recorded.global_y)
					},
					_ => (delta_y, global_x, global_y),
				}
			} else {
				(delta_y, global_x, global_y)
			};
		let event = SharedScrollInputEvent {
			seq,
			recorded_at,
			delta_y: effective_delta_y,
			global_x: effective_global_x,
			global_y: effective_global_y,
			gesture_active,
			gesture_ended,
		};

		if queue_state.queue.len() >= SHARED_SCROLL_INPUT_QUEUE_CAPACITY {
			queue_state.queue.pop_front();
		}

		queue_state.queue.push_back(event);

		queue_state.last_recorded = Some(event);

		event
	}

	fn replay_after_seq_through(
		&self,
		after_seq: u64,
		through: Instant,
	) -> Vec<(u64, Instant, f64, f64, f64, bool, bool)> {
		let queue_state = match self.queue_state.lock() {
			Ok(queue_state) => queue_state,
			Err(poisoned) => poisoned.into_inner(),
		};

		queue_state
			.queue
			.iter()
			.copied()
			.filter(|event| event.seq > after_seq && event.recorded_at <= through)
			.map(SharedScrollInputEvent::tuple)
			.collect()
	}
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

		let shared_state = Arc::clone(&self.scroll_input_shared_state);
		let handle = thread::Builder::new()
			.name(String::from("rsnap-scroll-input-tap"))
			.spawn(move || {
				run_scroll_input_event_tap_thread(shared_state);
			})
			.unwrap_or_else(|error| {
				panic!("failed to spawn rsnap scroll-input tap thread: {error}")
			});

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

						action_queue.push_back(SettingsWindowAction::CancelCaptureHotkey);
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

pub fn run() -> Result<()> {
	let settings = AppSettings::load();
	let capture_hotkey = settings.capture_hotkey();
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

#[cfg(target_os = "macos")]
fn begin_coalesced_overlay_user_event_send(pending: &AtomicBool) -> bool {
	pending.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok()
}

#[cfg(target_os = "macos")]
fn run_scroll_input_event_tap_thread(shared_state: Arc<SharedScrollInputState>) {
	let context = Box::new(ScrollInputTapContext {
		shared_state,
		tap: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()),
	});
	let context_ptr = Box::into_raw(context);
	let tap = unsafe {
		CGEventTapCreate(
			KCG_SESSION_EVENT_TAP,
			KCG_HEAD_INSERT_EVENT_TAP,
			KCG_EVENT_TAP_LISTEN_ONLY,
			cg_event_mask_bit(KCG_EVENT_SCROLL_WHEEL),
			scroll_input_event_tap_callback,
			context_ptr.cast(),
		)
	};

	if tap.is_null() {
		unsafe {
			drop(Box::from_raw(context_ptr));
		}

		tracing::warn!("Failed to create scroll input event tap.");

		return;
	}

	unsafe {
		(*context_ptr).tap.store(tap, Ordering::Release);
	}

	let loop_source = unsafe { CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0) };

	if loop_source.is_null() {
		unsafe {
			CFMachPortInvalidate(tap);
			CFRelease(tap.cast());
			drop(Box::from_raw(context_ptr));
		}

		tracing::warn!("Failed to create run-loop source for scroll input event tap.");

		return;
	}

	unsafe {
		let run_loop = CFRunLoopGetCurrent();

		CFRunLoopAddSource(run_loop, loop_source, kCFRunLoopCommonModes);
		CGEventTapEnable(tap, true);
	}

	tracing::info!(
		op = "scroll_input.tap_installed",
		tap = tap as usize,
		loop_source = loop_source as usize,
		"Installed native scroll input event tap."
	);

	unsafe {
		CFRunLoopRun();
		CFMachPortInvalidate(tap);
		CFRelease(loop_source.cast());
		CFRelease(tap.cast());
		drop(Box::from_raw(context_ptr));
	}
}

#[cfg(target_os = "macos")]
fn reenable_scroll_input_event_tap(context: &ScrollInputTapContext, event_type: u32) {
	let tap = context.tap.load(Ordering::Acquire);

	if tap.is_null() {
		tracing::warn!(
			op = "scroll_input.tap_disabled",
			event_type,
			"Scroll input event tap was disabled before the tap pointer was initialized."
		);

		return;
	}

	unsafe {
		CGEventTapEnable(tap, true);
	}

	tracing::warn!(
		op = "scroll_input.tap_reenabled",
		event_type,
		tap = tap as usize,
		"Scroll input event tap was disabled and has been re-enabled."
	);
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn scroll_input_event_tap_callback(
	_proxy: CGEventTapProxy,
	event_type: u32,
	event: CGEventRef,
	user_info: *const c_void,
) -> CGEventRef {
	if user_info.is_null() {
		return event;
	}

	let context = unsafe { &*(user_info.cast::<ScrollInputTapContext>()) };

	match event_type {
		KCG_EVENT_SCROLL_WHEEL => {},
		KCG_EVENT_TAP_DISABLED_BY_TIMEOUT | KCG_EVENT_TAP_DISABLED_BY_USER_INPUT => {
			reenable_scroll_input_event_tap(context, event_type);

			return event;
		},
		_ => return event,
	}

	if event.is_null() {
		return event;
	}

	send_overlay_scroll_input(context, event);

	event
}

#[cfg(target_os = "macos")]
fn send_overlay_scroll_input(context: &ScrollInputTapContext, cg_event: CGEventRef) {
	if !context.shared_state.is_enabled() {
		return;
	}

	let Some(decoded) = decode_scroll_input_from_cg_event(cg_event) else {
		return;
	};

	context.shared_state.record(
		decoded.delta_y,
		decoded.global_x,
		decoded.global_y,
		decoded.gesture_active,
		decoded.gesture_ended,
	);
}

#[cfg(target_os = "macos")]
fn decode_scroll_input_from_cg_event(cg_event: CGEventRef) -> Option<DecodedScrollInput> {
	let location = unsafe { CGEventGetLocation(cg_event) };
	let raw_delta_y = scroll_delta_y_from_cg_event(cg_event);
	let scroll_phase = scroll_phase_bits_from_cg_event(cg_event);
	let momentum_phase = scroll_momentum_phase_bits_from_cg_event(cg_event);
	let gesture_active =
		scroll_phase_bits_are_active(scroll_phase) || scroll_phase_bits_are_active(momentum_phase);
	let gesture_ended = scroll_phase_bits_are_terminal(scroll_phase)
		|| scroll_phase_bits_are_terminal(momentum_phase);

	decode_scroll_input_from_fields(raw_delta_y, location, gesture_active, gesture_ended)
}

#[cfg(target_os = "macos")]
fn decode_scroll_input_from_fields(
	raw_delta_y: f64,
	location: MacOSCGPoint,
	gesture_active: bool,
	gesture_ended: bool,
) -> Option<DecodedScrollInput> {
	if raw_delta_y == 0.0 && !gesture_ended {
		return None;
	}

	Some(DecodedScrollInput {
		raw_delta_y,
		delta_y: raw_delta_y,
		global_x: location.x,
		global_y: location.y,
		gesture_active,
		gesture_ended,
	})
}

#[cfg(target_os = "macos")]
fn scroll_delta_y_from_cg_event(cg_event: CGEventRef) -> f64 {
	let is_continuous = unsafe {
		CGEventGetIntegerValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS_FIELD)
	} != 0;

	if is_continuous {
		unsafe {
			CGEventGetDoubleValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1_FIELD)
		}
	} else {
		unsafe { CGEventGetDoubleValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1_FIELD) }
	}
}

#[cfg(target_os = "macos")]
fn scroll_phase_bits_from_cg_event(cg_event: CGEventRef) -> u64 {
	unsafe {
		CGEventGetIntegerValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_SCROLL_PHASE_FIELD) as u64
	}
}

#[cfg(target_os = "macos")]
fn scroll_momentum_phase_bits_from_cg_event(cg_event: CGEventRef) -> u64 {
	unsafe {
		CGEventGetIntegerValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE_FIELD) as u64
	}
}

#[cfg(target_os = "macos")]
fn scroll_phase_bits_are_active(phase_bits: u64) -> bool {
	phase_bits
		& (NSEVENT_PHASE_BEGAN
			| NSEVENT_PHASE_STATIONARY
			| NSEVENT_PHASE_CHANGED
			| NSEVENT_PHASE_MAY_BEGIN)
		!= 0
}

#[cfg(target_os = "macos")]
fn scroll_phase_bits_are_terminal(phase_bits: u64) -> bool {
	phase_bits & (NSEVENT_PHASE_ENDED | NSEVENT_PHASE_CANCELLED) != 0
}

#[cfg(target_os = "macos")]
fn cg_event_mask_bit(event_type: u32) -> u64 {
	1_u64 << event_type
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
	fn CGEventTapCreate(
		tap: u32,
		place: u32,
		options: u32,
		events_of_interest: u64,
		callback: unsafe extern "C" fn(
			CGEventTapProxy,
			u32,
			CGEventRef,
			*const c_void,
		) -> CGEventRef,
		user_info: *const c_void,
	) -> CFMachPortRef;
	fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
	fn CGEventGetLocation(event: CGEventRef) -> MacOSCGPoint;
	fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
	fn CGEventGetDoubleValueField(event: CGEventRef, field: u32) -> f64;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	static kCFAllocatorDefault: CFAllocatorRef;
	static kCFRunLoopCommonModes: CFRunLoopMode;

	fn CFRunLoopGetCurrent() -> CFRunLoopRef;
	fn CFMachPortCreateRunLoopSource(
		allocator: CFAllocatorRef,
		port: CFMachPortRef,
		order: isize,
	) -> *mut c_void;
	fn CFMachPortInvalidate(port: CFMachPortRef);
	fn CFRunLoopAddSource(run_loop: CFRunLoopRef, source: *mut c_void, mode: CFRunLoopMode);
	fn CFRunLoopRun();
	fn CFRelease(value: *const c_void);
}

#[cfg(test)]
mod tests {
	#[cfg(target_os = "macos")]
	use crate::app::{
		DecodedScrollInput, MacOSCGPoint, NSEVENT_PHASE_BEGAN, NSEVENT_PHASE_CANCELLED,
		NSEVENT_PHASE_ENDED, NSEVENT_PHASE_MAY_BEGIN, SHARED_SCROLL_INPUT_QUEUE_CAPACITY,
		SharedScrollInputState, begin_coalesced_overlay_user_event_send,
		decode_scroll_input_from_fields, scroll_phase_bits_are_active,
		scroll_phase_bits_are_terminal,
	};
	#[cfg(target_os = "macos")]
	use std::sync::atomic::AtomicBool;
	#[cfg(target_os = "macos")]
	use std::time::{Duration, Instant};

	#[cfg(target_os = "macos")]
	#[test]
	fn decode_scroll_input_ignores_zero_non_terminal_delta() {
		assert_eq!(
			decode_scroll_input_from_fields(0.0, MacOSCGPoint { x: 10.0, y: 20.0 }, false, false,),
			None
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn decode_scroll_input_preserves_terminal_zero_delta() {
		assert_eq!(
			decode_scroll_input_from_fields(0.0, MacOSCGPoint { x: 10.0, y: 20.0 }, false, true,),
			Some(DecodedScrollInput {
				raw_delta_y: 0.0,
				delta_y: 0.0,
				global_x: 10.0,
				global_y: 20.0,
				gesture_active: false,
				gesture_ended: true,
			})
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn phase_bits_classify_active_and_terminal_states() {
		assert!(scroll_phase_bits_are_active(NSEVENT_PHASE_BEGAN));
		assert!(scroll_phase_bits_are_active(NSEVENT_PHASE_MAY_BEGIN));
		assert!(scroll_phase_bits_are_terminal(NSEVENT_PHASE_ENDED));
		assert!(scroll_phase_bits_are_terminal(NSEVENT_PHASE_CANCELLED));
		assert!(!scroll_phase_bits_are_terminal(NSEVENT_PHASE_BEGAN));
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn terminal_scroll_event_preserves_last_effective_delta() {
		let state = SharedScrollInputState::default();
		let start = Instant::now();

		state.record_at(start, -4.0, 120.0, 140.0, true, false);
		state.record_at(start + Duration::from_millis(1), 0.0, 0.0, 0.0, false, true);

		assert_eq!(
			state.replay_after_seq_through(0, start + Duration::from_millis(1)),
			vec![
				(1, start, 120.0, 140.0, -4.0, true, false),
				(2, start + Duration::from_millis(1), 120.0, 140.0, -4.0, false, true,),
			]
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn newer_non_zero_scroll_event_replaces_preserved_delta() {
		let state = SharedScrollInputState::default();
		let start = Instant::now();

		state.record_at(start, -4.0, 120.0, 140.0, true, false);
		state.record_at(start + Duration::from_millis(1), 0.0, 0.0, 0.0, false, true);
		state.record_at(start + Duration::from_millis(2), 6.0, 220.0, 260.0, true, false);

		assert_eq!(
			state.replay_after_seq_through(0, start + Duration::from_millis(2)),
			vec![
				(1, start, 120.0, 140.0, -4.0, true, false),
				(2, start + Duration::from_millis(1), 120.0, 140.0, -4.0, false, true,),
				(3, start + Duration::from_millis(2), 220.0, 260.0, 6.0, true, false,),
			]
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn replay_after_seq_through_preserves_order_and_uses_sequence_cursor() {
		let state = SharedScrollInputState::default();
		let start = Instant::now();

		state.record_at(start, -4.0, 120.0, 140.0, true, false);
		state.record_at(start + Duration::from_millis(2), 6.0, 220.0, 260.0, true, false);
		state.record_at(start + Duration::from_millis(4), 0.0, 0.0, 0.0, false, true);

		assert_eq!(
			state.replay_after_seq_through(0, start + Duration::from_millis(2)),
			vec![
				(1, start, 120.0, 140.0, -4.0, true, false),
				(2, start + Duration::from_millis(2), 220.0, 260.0, 6.0, true, false,),
			]
		);
		assert!(state.replay_after_seq_through(2, start + Duration::from_millis(3)).is_empty());
		assert_eq!(
			state.replay_after_seq_through(2, start + Duration::from_millis(4)),
			vec![(3, start + Duration::from_millis(4), 220.0, 260.0, 6.0, false, true,)]
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn replay_after_seq_through_keeps_only_the_bounded_tail() {
		let state = SharedScrollInputState::default();
		let start = Instant::now();

		for offset in 0..(SHARED_SCROLL_INPUT_QUEUE_CAPACITY + 2) {
			state.record_at(
				start + Duration::from_millis(offset as u64),
				-(offset as f64),
				offset as f64,
				offset as f64 + 10.0,
				true,
				false,
			);
		}

		let replay = state.replay_after_seq_through(
			0,
			start + Duration::from_millis((SHARED_SCROLL_INPUT_QUEUE_CAPACITY + 2) as u64),
		);

		assert_eq!(replay.len(), SHARED_SCROLL_INPUT_QUEUE_CAPACITY);
		assert_eq!(replay.first().map(|event| event.0), Some(3));
		assert_eq!(
			replay.last().map(|event| event.0),
			Some((SHARED_SCROLL_INPUT_QUEUE_CAPACITY + 2) as u64)
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn begin_coalesced_overlay_user_event_send_only_allows_first_sender_per_flag() {
		let pending = AtomicBool::new(false);

		assert!(begin_coalesced_overlay_user_event_send(&pending));
		assert!(!begin_coalesced_overlay_user_event_send(&pending));
	}
}
