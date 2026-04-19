#[cfg(not(target_os = "macos"))]
use std::borrow::Cow;
#[cfg(target_os = "macos")]
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", test))]
use std::sync::Arc;
#[cfg(any(target_os = "macos", test))]
use std::sync::Mutex;
#[cfg(test)]
use std::sync::OnceLock;
#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicU64;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "macos")]
use std::thread::{self, Builder};
#[cfg(target_os = "macos")]
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use arboard::Clipboard;
#[cfg(not(target_os = "macos"))]
use arboard::ImageData;
#[cfg(target_os = "macos")]
use color_eyre::eyre;
use color_eyre::eyre::{Result, WrapErr};
#[cfg(target_os = "macos")]
use global_hotkey::hotkey::HotKey;
#[cfg(target_os = "macos")]
use objc::runtime::{BOOL, Object, YES};
#[cfg(target_os = "macos")]
use objc2::AnyThread;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::NSSound;
#[cfg(target_os = "macos")]
use objc2_foundation::NSString;
use winit::event_loop::ActiveEventLoop;

use crate::app::App;
#[cfg(target_os = "macos")]
use crate::app::OverlayFrozenHotkeyBinding;
#[cfg(target_os = "macos")]
use crate::app::OverlayHotkeyRegistrationState;
#[cfg(target_os = "macos")]
use crate::app::UserEvent;
#[cfg(target_os = "macos")]
use crate::app::scroll_input_macos::{
	self, ScrollInputObserverLifecycle, ScrollInputObserverWaitOutcome, SharedScrollInputState,
};
#[cfg(target_os = "macos")]
use crate::permissions_macos;
use crate::settings;
#[cfg(target_os = "macos")]
use rsnap_overlay::{
	DeferredTextRecognitionOutcomeKind, DeferredTextRecognitionRequest, MacOSCaptureHost,
};
use rsnap_overlay::{
	HudAnchor, OutputNaming, OverlayConfig, OverlayControl, OverlayExit, OverlayHostEffectRequest,
	OverlaySession,
};

#[cfg(target_os = "macos")]
macro_rules! sel {
	($($tt:tt)*) => {
		objc::sel!($($tt)*)
	};
}

#[cfg(target_os = "macos")]
macro_rules! sel_impl {
	($($tt:tt)*) => {
		objc::sel_impl!($($tt)*)
	};
}

#[cfg(all(test, target_os = "macos"))]
type CopyPngHostEffectHook = dyn Fn(&[u8]) -> Result<()> + Send + Sync + 'static;

#[cfg(all(test, target_os = "macos"))]
type DeferredTextRecognitionHandoffHook = dyn Fn(DeferredTextRecognitionRequest, Arc<AtomicU64>, Arc<AtomicU64>, u64)
	+ Send
	+ Sync
	+ 'static;

#[cfg(target_os = "macos")]
const SCROLL_INPUT_OBSERVER_READY_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(target_os = "macos")]
const OVERLAY_SESSION_PREWARM_RETRY_BACKOFF: Duration = Duration::from_secs(1);
#[cfg(target_os = "macos")]
const DEFERRED_OCR_PUBLISH_PENDING_POLL_INTERVAL: Duration = Duration::from_millis(5);
#[cfg(target_os = "macos")]
const CAPTURE_SUCCESS_SOUND_CANDIDATE_PATHS: [&str; 2] = [
	"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Screen Capture.aif",
	"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Shutter.aif",
];

#[cfg(all(test, target_os = "macos"))]
static HOST_EFFECT_TEST_HOOKS: OnceLock<Mutex<HostEffectTestHooks>> = OnceLock::new();
#[cfg(all(test, target_os = "macos"))]
static HOST_EFFECT_TEST_SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(all(test, target_os = "macos"))]
#[derive(Default)]
struct HostEffectTestHooks {
	copy_png: Option<Arc<CopyPngHostEffectHook>>,
	#[cfg(target_os = "macos")]
	deferred_text_recognition_handoff: Option<Arc<DeferredTextRecognitionHandoffHook>>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct OverlayHotkeySpec {
	hotkey: HotKey,
	hotkey_id: u32,
	hotkey_label: &'static str,
	missing_manager_message: &'static str,
	register_failure_message: &'static str,
	register_success_message: &'static str,
	unregister_failure_message: &'static str,
	unregister_success_message: &'static str,
}

impl App {
	#[cfg(target_os = "macos")]
	pub(super) fn load_capture_success_sound() -> Option<Retained<NSSound>> {
		for path in CAPTURE_SUCCESS_SOUND_CANDIDATE_PATHS {
			let ns_path = NSString::from_str(path);

			if let Some(sound) =
				NSSound::initWithContentsOfFile_byReference(NSSound::alloc(), &ns_path, true)
			{
				tracing::info!(path, sound_path = path, "Loaded native capture success sound.");

				return Some(sound);
			}
		}

		tracing::warn!(
			candidates = ?CAPTURE_SUCCESS_SOUND_CANDIDATE_PATHS,
			"Failed to load a native capture success sound; capture completion audio is unavailable."
		);

		None
	}

	#[cfg(target_os = "macos")]
	fn register_overlay_hotkey(
		&mut self,
		spec: OverlayHotkeySpec,
		registration_state: OverlayHotkeyRegistrationState,
	) -> OverlayHotkeyRegistrationState {
		if !registration_state.allows_register_attempt() {
			return registration_state;
		}

		let Some(manager) = self._hotkey_manager.as_mut() else {
			tracing::warn!(hotkey = spec.hotkey_label, "{}", spec.missing_manager_message);

			return OverlayHotkeyRegistrationState::Blocked;
		};

		match manager.register(spec.hotkey) {
			Ok(()) => {
				tracing::info!(
					hotkey = spec.hotkey_label,
					hotkey_id = %spec.hotkey_id,
					"{}",
					spec.register_success_message
				);

				OverlayHotkeyRegistrationState::Registered
			},
			Err(err) => {
				tracing::warn!(
					error = ?err,
					hotkey = spec.hotkey_label,
					hotkey_id = %spec.hotkey_id,
					"{}",
					spec.register_failure_message
				);

				OverlayHotkeyRegistrationState::next_state_after_register_error(&err)
			},
		}
	}

	#[cfg(target_os = "macos")]
	fn unregister_overlay_hotkey(
		&mut self,
		spec: OverlayHotkeySpec,
		registration_state: OverlayHotkeyRegistrationState,
	) -> OverlayHotkeyRegistrationState {
		if matches!(registration_state, OverlayHotkeyRegistrationState::Unregistered) {
			return registration_state;
		}
		if matches!(registration_state, OverlayHotkeyRegistrationState::Blocked) {
			return OverlayHotkeyRegistrationState::Unregistered;
		}

		let Some(manager) = self._hotkey_manager.as_mut() else {
			return OverlayHotkeyRegistrationState::Unregistered;
		};

		match manager.unregister(spec.hotkey) {
			Ok(()) => {
				tracing::info!(
					hotkey = spec.hotkey_label,
					hotkey_id = %spec.hotkey_id,
					"{}",
					spec.unregister_success_message
				);
			},
			Err(err) => {
				tracing::warn!(
					error = ?err,
					hotkey = spec.hotkey_label,
					hotkey_id = %spec.hotkey_id,
					"{}",
					spec.unregister_failure_message
				);
			},
		}

		OverlayHotkeyRegistrationState::Unregistered
	}

	#[cfg(target_os = "macos")]
	fn overlay_cancel_hotkey_spec(&self) -> OverlayHotkeySpec {
		OverlayHotkeySpec {
			hotkey: self.overlay_cancel_hotkey,
			hotkey_id: self.overlay_cancel_hotkey_id,
			hotkey_label: "Esc",
			missing_manager_message: "Capture cancel hotkey is unavailable because the global hotkey manager is missing.",
			register_failure_message: "Failed to register the capture cancel hotkey.",
			register_success_message: "Registered the capture cancel hotkey.",
			unregister_failure_message: "Failed to unregister the capture cancel hotkey.",
			unregister_success_message: "Unregistered the capture cancel hotkey.",
		}
	}

	#[cfg(target_os = "macos")]
	fn overlay_loupe_hotkey_spec(&self) -> OverlayHotkeySpec {
		OverlayHotkeySpec {
			hotkey: self.overlay_loupe_hotkey,
			hotkey_id: self.overlay_loupe_hotkey_id,
			hotkey_label: "Tab",
			missing_manager_message: "Capture loupe hotkey is unavailable because the global hotkey manager is missing.",
			register_failure_message: "Failed to register the capture loupe hotkey.",
			register_success_message: "Registered the capture loupe hotkey.",
			unregister_failure_message: "Failed to unregister the capture loupe hotkey.",
			unregister_success_message: "Unregistered the capture loupe hotkey.",
		}
	}

	#[cfg(target_os = "macos")]
	fn overlay_frozen_hotkey_spec(binding: &OverlayFrozenHotkeyBinding) -> OverlayHotkeySpec {
		OverlayHotkeySpec {
			hotkey: binding.hotkey,
			hotkey_id: binding.hotkey_id,
			hotkey_label: binding.label,
			missing_manager_message: "Frozen overlay hotkeys are unavailable because the global hotkey manager is missing.",
			register_failure_message: "Failed to register a frozen overlay hotkey.",
			register_success_message: "Registered a frozen overlay hotkey.",
			unregister_failure_message: "Failed to unregister a frozen overlay hotkey.",
			unregister_success_message: "Unregistered a frozen overlay hotkey.",
		}
	}

	#[cfg(target_os = "macos")]
	fn register_overlay_cancel_hotkey(&mut self) {
		let spec = self.overlay_cancel_hotkey_spec();

		self.overlay_cancel_hotkey_registration_state =
			self.register_overlay_hotkey(spec, self.overlay_cancel_hotkey_registration_state);
	}

	#[cfg(target_os = "macos")]
	fn unregister_overlay_cancel_hotkey(&mut self) {
		let spec = self.overlay_cancel_hotkey_spec();

		self.overlay_cancel_hotkey_registration_state =
			self.unregister_overlay_hotkey(spec, self.overlay_cancel_hotkey_registration_state);
	}

	#[cfg(target_os = "macos")]
	fn register_overlay_loupe_hotkey(&mut self) {
		let spec = self.overlay_loupe_hotkey_spec();

		self.overlay_loupe_hotkey_registration_state =
			self.register_overlay_hotkey(spec, self.overlay_loupe_hotkey_registration_state);
	}

	#[cfg(target_os = "macos")]
	fn unregister_overlay_loupe_hotkey(&mut self) {
		let spec = self.overlay_loupe_hotkey_spec();

		self.overlay_loupe_hotkey_registration_state =
			self.unregister_overlay_hotkey(spec, self.overlay_loupe_hotkey_registration_state);
	}

	#[cfg(target_os = "macos")]
	fn register_overlay_frozen_hotkeys(&mut self) {
		for index in 0..self.overlay_frozen_hotkeys.len() {
			let binding = self.overlay_frozen_hotkeys[index];
			let spec = Self::overlay_frozen_hotkey_spec(&binding);
			let registration_state = self.register_overlay_hotkey(spec, binding.registration_state);

			self.overlay_frozen_hotkeys[index].registration_state = registration_state;
		}
	}

	#[cfg(target_os = "macos")]
	fn unregister_overlay_frozen_hotkeys(&mut self) {
		for index in 0..self.overlay_frozen_hotkeys.len() {
			let binding = self.overlay_frozen_hotkeys[index];
			let spec = Self::overlay_frozen_hotkey_spec(&binding);
			let registration_state =
				self.unregister_overlay_hotkey(spec, binding.registration_state);

			self.overlay_frozen_hotkeys[index].registration_state = registration_state;
		}
	}

	#[cfg(target_os = "macos")]
	fn sync_overlay_hotkey_registrations(&mut self) {
		let should_register_cancel =
			self.overlay_session.as_ref().is_some_and(OverlaySession::wants_global_cancel_hotkey);
		let should_register_loupe =
			self.overlay_session.as_ref().is_some_and(OverlaySession::wants_global_loupe_hotkey);
		let should_register_frozen =
			self.overlay_session.as_ref().is_some_and(OverlaySession::wants_global_frozen_hotkeys);

		if should_register_cancel {
			self.register_overlay_cancel_hotkey();
		} else {
			self.unregister_overlay_cancel_hotkey();
		}
		if should_register_loupe {
			self.register_overlay_loupe_hotkey();
		} else {
			self.unregister_overlay_loupe_hotkey();
		}
		if should_register_frozen {
			self.register_overlay_frozen_hotkeys();
		} else {
			self.unregister_overlay_frozen_hotkeys();
		}
	}

	#[cfg(target_os = "macos")]
	fn play_capture_success_feedback(&self) {
		let Some(sound) = self.capture_success_sound.as_ref() else {
			return;
		};
		let _ = sound.stop();

		sound.setCurrentTime(0.0);

		if !sound.play() {
			tracing::warn!("Failed to play the native capture success sound.");
		}
	}

	#[cfg(not(target_os = "macos"))]
	fn play_capture_success_feedback(&self) {}

	fn self_capture_exception_window_ids(&self) -> Vec<u32> {
		self_capture_exception_window_ids_from_sources(
			self.settings_window.as_ref().and_then(|window| window.capture_window_id()),
			self.settings_window_capture_window_id,
		)
	}

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
			selection_flow_enabled: self.settings.selection_flow_enabled,
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
			self_capture_exception_window_ids: self.self_capture_exception_window_ids(),
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

	pub(super) fn apply_overlay_settings(&mut self) {
		let config = self.overlay_config();

		if let Some(session) = self.overlay_session.as_mut() {
			session.set_config(config.clone());
		}
		#[cfg(target_os = "macos")]
		if let Some(session) = self.prewarmed_overlay_session.as_mut() {
			session.set_config(config);
		}
	}

	pub(super) fn start_capture_session(
		&mut self,
		event_loop: &ActiveEventLoop,
		requested_by: &'static str,
	) {
		let capture_start_started_at = Instant::now();

		if self.overlay_session.is_some() {
			tracing::info!(
				requested_by = %requested_by,
				"Capture already active; ignoring additional start request."
			);

			return;
		}

		let Some(screen_recording_preflight_ms) =
			self.capture_screen_recording_preflight(requested_by, capture_start_started_at)
		else {
			return;
		};
		let (overlay_session_source, overlay_session_build_ms, mut overlay_session) = {
			let overlay_session_build_started_at = Instant::now();
			let (overlay_session_source, overlay_session) =
				self.take_overlay_session_for_capture_start();

			(
				overlay_session_source,
				overlay_session_build_started_at.elapsed().as_millis(),
				overlay_session,
			)
		};

		#[cfg(target_os = "macos")]
		{
			self.overlay_session_prewarm_requested = false;
			self.overlay_session_prewarm_retry_not_before = None;
			self.overlay_session_generation = self.overlay_session_generation.wrapping_add(1);

			self.pending_deferred_ocr_generation
				.store(self.overlay_session_generation, Ordering::Release);
		}

		let scroll_input_reset_ms = self.reset_scroll_input_for_capture_start();
		let hook_wiring_started_at = Instant::now();

		self.wire_capture_session_hooks(&mut overlay_session);

		let hook_wiring_ms = hook_wiring_started_at.elapsed().as_millis();
		let overlay_start_started_at = Instant::now();
		#[cfg(target_os = "macos")]
		let mut overlay_capture_host = self.begin_overlay_capture_host_session();

		match overlay_session.start(event_loop) {
			Ok(()) => {
				let overlay_start_ms = overlay_start_started_at.elapsed().as_millis();

				#[cfg(target_os = "macos")]
				{
					self.latest_deferred_ocr_generation
						.store(self.overlay_session_generation, Ordering::Release);
					self.pending_deferred_ocr_generation.store(0, Ordering::Release);
				}

				tracing::info!(
				op = "capture.start_phase_timing",
				requested_by = %requested_by,
				result = "started",
				overlay_session_source,
				hotkey = %self.capture_key_label(),
				overlay_session_build_ms,
				hook_wiring_ms,
				overlay_start_ms,
				total_ms = capture_start_started_at.elapsed().as_millis(),
				screen_recording_preflight_ms,
					scroll_input_reset_ms,
					"Capture startup phase timing."
				);
				tracing::info!(
					requested_by = %requested_by,
					hotkey = %self.capture_key_label(),
					"Capture overlay started."
				);

				self.overlay_session = Some(overlay_session);

				#[cfg(target_os = "macos")]
				if !self.attach_overlay_capture_host_after_start(overlay_capture_host) {
					return;
				}

				#[cfg(target_os = "macos")]
				self.sync_overlay_hotkey_registrations();
			},
			Err(err) => {
				let overlay_start_ms = overlay_start_started_at.elapsed().as_millis();

				#[cfg(target_os = "macos")]
				overlay_capture_host.cancel_session_start();
				#[cfg(target_os = "macos")]
				self.reset_capture_start_after_failure();

				tracing::warn!(
					op = "capture.start_phase_timing",
					error = %err,
					requested_by = %requested_by,
					result = "error",
					overlay_session_source,
					overlay_session_build_ms,
					hook_wiring_ms,
					overlay_start_ms,
					total_ms = capture_start_started_at.elapsed().as_millis(),
					screen_recording_preflight_ms,
					scroll_input_reset_ms,
					"Failed to start overlay session."
				);

				self.note_capture_start_failure_for_prewarm();
			},
		}
	}

	#[cfg(target_os = "macos")]
	fn begin_overlay_capture_host_session(&self) -> MacOSCaptureHost {
		let mut overlay_capture_host = self.build_overlay_capture_host();

		overlay_capture_host.begin_session();

		overlay_capture_host
	}

	#[cfg(target_os = "macos")]
	fn attach_overlay_capture_host_after_start(
		&mut self,
		overlay_capture_host: MacOSCaptureHost,
	) -> bool {
		self.overlay_capture_host = Some(overlay_capture_host);

		self.sync_overlay_capture_host();

		self.overlay_session.is_some()
	}

	#[cfg(target_os = "macos")]
	fn reset_capture_start_after_failure(&mut self) {
		self.reset_overlay_native_capture_input_dispatch();
		self.pending_deferred_ocr_generation.store(0, Ordering::Release);
		self.scroll_input_shared_state.set_enabled(false);
		self.scroll_input_shared_state.set_event_waker(None);
		self.scroll_input_shared_state.clear();
	}

	#[cfg(target_os = "macos")]
	fn note_capture_start_failure_for_prewarm(&mut self) {
		self.overlay_session_prewarm_requested = true;
	}

	#[cfg(not(target_os = "macos"))]
	fn note_capture_start_failure_for_prewarm(&mut self) {}

	#[cfg(target_os = "macos")]
	fn take_overlay_session_for_capture_start(&mut self) -> (&'static str, OverlaySession) {
		if let Some(overlay_session) = self.prewarmed_overlay_session.take() {
			("prewarmed", overlay_session)
		} else {
			("fresh", OverlaySession::with_config(self.overlay_config()))
		}
	}

	#[cfg(not(target_os = "macos"))]
	fn take_overlay_session_for_capture_start(&mut self) -> (&'static str, OverlaySession) {
		("fresh", OverlaySession::with_config(self.overlay_config()))
	}

	#[cfg(target_os = "macos")]
	pub(super) fn maybe_prewarm_overlay_session(&mut self, event_loop: &ActiveEventLoop) {
		if !self.overlay_session_prewarm_requested
			|| self.overlay_session.is_some()
			|| self.prewarmed_overlay_session.is_some()
		{
			return;
		}
		if self
			.overlay_session_prewarm_retry_not_before
			.is_some_and(|not_before| Instant::now() < not_before)
		{
			return;
		}

		let prewarm_started_at = Instant::now();
		let mut overlay_session = OverlaySession::with_config(self.overlay_config());

		match overlay_session.prewarm(event_loop) {
			Ok(()) => {
				self.prewarmed_overlay_session = Some(overlay_session);
				self.overlay_session_prewarm_requested = false;
				self.overlay_session_prewarm_retry_not_before = None;

				tracing::info!(
					op = "capture.prewarm_phase_timing",
					result = "prewarmed",
					total_ms = prewarm_started_at.elapsed().as_millis(),
					"Capture startup resources prewarmed."
				);
			},
			Err(err) => {
				self.overlay_session_prewarm_requested = true;
				self.overlay_session_prewarm_retry_not_before =
					Some(Instant::now() + OVERLAY_SESSION_PREWARM_RETRY_BACKOFF);

				tracing::warn!(
					op = "capture.prewarm_phase_timing",
					error = %err,
					result = "error",
					retry_backoff_ms = OVERLAY_SESSION_PREWARM_RETRY_BACKOFF.as_millis(),
					total_ms = prewarm_started_at.elapsed().as_millis(),
					"Failed to prewarm capture startup resources."
				);
			},
		}
	}

	fn capture_screen_recording_preflight(
		&self,
		requested_by: &'static str,
		capture_start_started_at: Instant,
	) -> Option<u128> {
		#[cfg(target_os = "macos")]
		{
			let preflight_started_at = Instant::now();
			let screen_recording_granted = self.ensure_screen_recording_access(requested_by);
			let preflight_ms = preflight_started_at.elapsed().as_millis();

			if !screen_recording_granted {
				tracing::info!(
					op = "capture.start_phase_timing",
					requested_by = %requested_by,
					result = "blocked_missing_screen_recording",
					screen_recording_preflight_ms = preflight_ms,
					total_ms = capture_start_started_at.elapsed().as_millis(),
					"Capture startup phase timing."
				);

				return None;
			}

			Some(preflight_ms)
		}
		#[cfg(not(target_os = "macos"))]
		{
			let _ = requested_by;
			let _ = capture_start_started_at;

			Some(0)
		}
	}

	fn reset_scroll_input_for_capture_start(&mut self) -> u128 {
		#[cfg(target_os = "macos")]
		{
			let reset_started_at = Instant::now();

			self.finish_coalesced_overlay_stream_frame_send();
			self.reset_overlay_native_capture_input_dispatch();
			self.scroll_input_shared_state.clear();

			reset_started_at.elapsed().as_millis()
		}

		#[cfg(not(target_os = "macos"))]
		{
			0
		}
	}

	fn wire_capture_session_hooks(&mut self, overlay_session: &mut OverlaySession) {
		#[cfg(target_os = "macos")]
		{
			self.scroll_input_shared_state.set_event_waker(Some(Arc::new({
				let overlay_proxy = self.overlay_proxy.clone();

				move || {
					let _ = overlay_proxy.send_event(UserEvent::OverlayScrollInput);
				}
			})));
			overlay_session.set_scroll_frame_waker(Arc::new({
				let overlay_proxy = self.overlay_proxy.clone();
				let overlay_stream_event_pending = Arc::clone(&self.overlay_stream_event_pending);

				move || {
					if overlay_stream_event_pending
						.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
						.is_ok() && overlay_proxy.send_event(UserEvent::OverlayStreamFrame).is_err()
					{
						overlay_stream_event_pending.store(false, Ordering::Release);
					}
				}
			}));
			overlay_session.set_startup_aux_window_waker(Arc::new({
				let overlay_proxy = self.overlay_proxy.clone();
				let overlay_session_generation = self.overlay_session_generation;

				move || {
					let _ = overlay_proxy.send_event(UserEvent::OverlayStartupAuxWindows(
						overlay_session_generation,
					));
				}
			}));
			overlay_session.set_response_waker(Arc::new({
				let overlay_proxy = self.overlay_proxy.clone();

				move || {
					let _ = overlay_proxy.send_event(UserEvent::OverlayWorkerResponse);
				}
			}));
			overlay_session.set_external_scroll_input_drain_reader(Arc::new({
				let shared_state = Arc::clone(&self.scroll_input_shared_state);

				move |after_seq, through| shared_state.replay_after_seq_through(after_seq, through)
			}));

			overlay_session
				.set_scroll_capture_start_guard(Arc::new(Self::ensure_scroll_capture_permissions));

			overlay_session.set_scroll_capture_starting_hook(Arc::new({
				let shared_state = Arc::clone(&self.scroll_input_shared_state);
				let observer_lifecycle = Arc::clone(&self.scroll_input_observer_lifecycle);

				move || Self::prepare_external_scroll_input(&shared_state, &observer_lifecycle)
			}));
			overlay_session.set_scroll_capture_started_hook(Arc::new({
				let shared_state = Arc::clone(&self.scroll_input_shared_state);

				move || Self::enable_external_scroll_input(&shared_state)
			}));
		}
		#[cfg(not(target_os = "macos"))]
		{
			let _ = overlay_session;
		}
	}

	#[cfg(target_os = "macos")]
	fn ensure_screen_recording_access(&self, requested_by: &'static str) -> bool {
		if permissions_macos::screen_recording_access_granted() {
			return true;
		}

		tracing::info!(
			requested_by = %requested_by,
			settings_path = %permissions_macos::SCREEN_RECORDING_SETTINGS_PATH,
			"Screen Recording access is missing; capture stays unavailable until it is enabled from Settings."
		);

		false
	}

	pub(super) fn end_overlay_session(&mut self, exit: OverlayExit) {
		if !self.finish_overlay_session_teardown() {
			return;
		}

		match exit {
			OverlayExit::Cancelled => tracing::info!("Capture cancelled."),
			OverlayExit::HostEffect(_) => {},
			OverlayExit::Error(message) => tracing::warn!(error = %message, "Capture failed."),
		};

		tracing::info!("Capture overlay ended.");
	}

	fn finish_overlay_session_teardown(&mut self) -> bool {
		if self.overlay_session.is_none() {
			return false;
		}

		#[cfg(target_os = "macos")]
		{
			self.teardown_overlay_capture_host();
			self.reset_overlay_native_capture_input_dispatch();
			self.unregister_overlay_cancel_hotkey();
			self.unregister_overlay_loupe_hotkey();
			self.unregister_overlay_frozen_hotkeys();
		}

		let Some(_session) = self.overlay_session.take() else {
			return false;
		};

		#[cfg(target_os = "macos")]
		{
			self.prewarmed_overlay_session = None;
			self.overlay_session_prewarm_requested = true;
			self.overlay_session_prewarm_retry_not_before = None;

			self.scroll_input_shared_state.set_enabled(false);
			self.scroll_input_shared_state.set_event_waker(None);
			self.scroll_input_shared_state.clear();
		}

		true
	}

	#[cfg(target_os = "macos")]
	fn ensure_scroll_capture_permissions() -> Result<bool> {
		let accessibility_granted = permissions_macos::accessibility_access_granted();
		let input_monitoring_granted = permissions_macos::input_monitoring_access_granted();

		if !accessibility_granted || !input_monitoring_granted {
			tracing::info!(
				accessibility_granted = accessibility_granted,
				input_monitoring_granted = input_monitoring_granted,
				"Scroll capture prerequisites are missing; rejecting the start request without a HUD permission message."
			);

			return Ok(false);
		}

		Ok(true)
	}

	#[cfg(target_os = "macos")]
	fn process_deferred_text_recognition_request(
		request: DeferredTextRecognitionRequest,
		latest_deferred_ocr_generation: Arc<AtomicU64>,
		pending_deferred_ocr_generation: Arc<AtomicU64>,
		request_generation: u64,
	) {
		let latest_deferred_ocr_generation_for_publish =
			Arc::clone(&latest_deferred_ocr_generation);
		let pending_deferred_ocr_generation_for_publish =
			Arc::clone(&pending_deferred_ocr_generation);
		let outcome = rsnap_overlay::process_deferred_text_recognition_for_latest_capture(
			request,
			latest_deferred_ocr_generation,
			pending_deferred_ocr_generation,
			request_generation,
		);

		match outcome.kind {
			DeferredTextRecognitionOutcomeKind::TextReady => {
				let Some(recognized_text) = outcome.recognized_text.as_deref() else {
					tracing::warn!(
						request_id = outcome.request_id,
						"Deferred OCR reported text readiness without recognized text."
					);

					return;
				};

				if !deferred_text_recognition_publish_allowed(
					&latest_deferred_ocr_generation_for_publish,
					&pending_deferred_ocr_generation_for_publish,
					request_generation,
				) {
					tracing::info!(
						request_id = outcome.request_id,
						"Deferred OCR publish was suppressed after recognition because a newer capture took ownership before host clipboard publish."
					);

					return;
				}

				match write_text_to_clipboard(recognized_text) {
					Ok(()) => {
						tracing::info!(
							request_id = outcome.request_id,
							characters = outcome.recognized_chars,
							lines = outcome.recognized_lines,
							"Recognized text copied to clipboard."
						);
					},
					Err(err) => {
						tracing::warn!(
							request_id = outcome.request_id,
							error = %err,
							characters = outcome.recognized_chars,
							lines = outcome.recognized_lines,
							"Failed to copy recognized text to the host clipboard."
						);
					},
				}
			},
			DeferredTextRecognitionOutcomeKind::NoText => {
				tracing::info!(
					request_id = outcome.request_id,
					lines = outcome.recognized_lines,
					characters = outcome.recognized_chars,
					"Deferred OCR finished without recognized text."
				);
			},
			DeferredTextRecognitionOutcomeKind::StaleRequestSuppressed => {
				tracing::info!(
					request_id = outcome.request_id,
					"Deferred OCR publish was suppressed because a newer capture took ownership."
				);
			},
			DeferredTextRecognitionOutcomeKind::RecognizeError => {
				tracing::warn!(
					request_id = outcome.request_id,
					"Deferred OCR failed before host publish."
				);
			},
		}
	}

	fn handle_overlay_host_effect_request(&mut self, request: OverlayHostEffectRequest) {
		match request {
			OverlayHostEffectRequest::CopyPng { png_bytes } => {
				self.handle_copy_png_host_effect(png_bytes)
			},
			OverlayHostEffectRequest::SavePng {
				png_bytes,
				output_dir,
				output_filename_prefix,
				output_naming,
			} => self.handle_save_png_host_effect(
				png_bytes,
				output_dir,
				output_filename_prefix,
				output_naming,
			),
			#[cfg(target_os = "macos")]
			OverlayHostEffectRequest::DeferredTextRecognition(request) => {
				self.handle_deferred_text_recognition_host_effect(request);
			},
		}
	}

	fn handle_copy_png_host_effect(&mut self, png_bytes: Vec<u8>) {
		match write_png_bytes_to_clipboard(&png_bytes) {
			Ok(()) => {
				tracing::info!(bytes = png_bytes.len(), "Capture copied to clipboard.");

				self.play_capture_success_feedback();

				let completed_request = OverlayHostEffectRequest::CopyPng { png_bytes };

				self.complete_overlay_host_effect_request(&completed_request);
			},
			Err(err) => {
				let message = format!("{err:#}");

				tracing::warn!(
					error = %err,
					bytes = png_bytes.len(),
					"Failed to copy capture PNG through the host clipboard."
				);

				if let Some(session) = self.overlay_session.as_mut() {
					session.report_host_effect_error(message);
				}
			},
		}
	}

	fn handle_save_png_host_effect(
		&mut self,
		png_bytes: Vec<u8>,
		output_dir: PathBuf,
		output_filename_prefix: String,
		output_naming: OutputNaming,
	) {
		match save_png_bytes_to_configured_dir(
			&png_bytes,
			&output_dir,
			&output_filename_prefix,
			output_naming,
		) {
			Ok(path) => {
				tracing::info!(path = %path.display(), "Capture saved to file.");

				self.play_capture_success_feedback();

				let completed_request = OverlayHostEffectRequest::SavePng {
					png_bytes,
					output_dir,
					output_filename_prefix,
					output_naming,
				};

				self.complete_overlay_host_effect_request(&completed_request);
			},
			Err(err) => {
				let message = format!("{err:#}");

				tracing::warn!(
					error = %err,
					bytes = png_bytes.len(),
					"Failed to save capture PNG through the host output path."
				);

				if let Some(session) = self.overlay_session.as_mut() {
					session.report_host_effect_error(message);
				}
			},
		}
	}

	#[cfg(target_os = "macos")]
	fn handle_deferred_text_recognition_host_effect(
		&mut self,
		request: DeferredTextRecognitionRequest,
	) {
		let completed_request = OverlayHostEffectRequest::DeferredTextRecognition(request);

		self.complete_overlay_host_effect_request(&completed_request);

		let OverlayHostEffectRequest::DeferredTextRecognition(request) = completed_request else {
			unreachable!("constructed deferred text recognition request")
		};
		let request_id = request.request_id;
		let request_generation = self.overlay_session_generation;
		let latest_deferred_ocr_generation = Arc::clone(&self.latest_deferred_ocr_generation);
		let pending_deferred_ocr_generation = Arc::clone(&self.pending_deferred_ocr_generation);

		#[cfg(test)]
		if let Some(handoff_hook) = deferred_text_recognition_handoff_test_hook() {
			handoff_hook(
				request,
				latest_deferred_ocr_generation,
				pending_deferred_ocr_generation,
				request_generation,
			);

			return;
		}

		let request_slot = Arc::new(Mutex::new(Some(request)));
		let request_slot_for_worker = Arc::clone(&request_slot);
		let latest_deferred_ocr_generation_for_worker = Arc::clone(&latest_deferred_ocr_generation);
		let pending_deferred_ocr_generation_for_worker =
			Arc::clone(&pending_deferred_ocr_generation);

		match Builder::new().name(format!("rsnap-ocr-{request_id}")).spawn(move || {
			let Some(request) =
				Self::take_deferred_text_recognition_request(&request_slot_for_worker)
			else {
				tracing::warn!(
					request_id = request_id,
					"Deferred OCR request was unavailable when the background worker started."
				);

				return;
			};

			Self::process_deferred_text_recognition_request(
				request,
				latest_deferred_ocr_generation_for_worker,
				pending_deferred_ocr_generation_for_worker,
				request_generation,
			);
		}) {
			Ok(_handle) => {
				tracing::info!(
					request_id = request_id,
					"Capture handed OCR work to the background worker."
				);
			},
			Err(err) => {
				tracing::warn!(
					request_id,
					error = %err,
					"Failed to start the background OCR worker; running deferred OCR inline."
				);

				let Some(request) = Self::take_deferred_text_recognition_request(&request_slot)
				else {
					tracing::warn!(
						request_id = request_id,
						"Deferred OCR request was unavailable after background worker startup failed."
					);

					return;
				};

				Self::process_deferred_text_recognition_request(
					request,
					latest_deferred_ocr_generation,
					pending_deferred_ocr_generation,
					request_generation,
				);
			},
		}
	}

	fn complete_overlay_host_effect_request(&mut self, request: &OverlayHostEffectRequest) {
		if let Some(session) = self.overlay_session.as_mut() {
			session.complete_host_effect_request(request);
		}

		if self.finish_overlay_session_teardown() {
			tracing::info!("Capture overlay ended.");
		}
	}

	#[cfg(target_os = "macos")]
	fn take_deferred_text_recognition_request(
		request_slot: &Mutex<Option<DeferredTextRecognitionRequest>>,
	) -> Option<DeferredTextRecognitionRequest> {
		match request_slot.lock() {
			Ok(mut guard) => guard.take(),
			Err(poisoned) => {
				tracing::warn!(
					"Deferred OCR request slot was poisoned while recovering the request."
				);

				poisoned.into_inner().take()
			},
		}
	}

	#[cfg(target_os = "macos")]
	fn prepare_external_scroll_input(
		shared_state: &Arc<SharedScrollInputState>,
		observer_lifecycle: &Arc<ScrollInputObserverLifecycle>,
	) -> Result<()> {
		tracing::info!(
			op = "scroll_input.prepare_start",
			observer_status = ?observer_lifecycle.status(),
			enabled = shared_state.is_enabled(),
			"Preparing native scroll input for scroll capture."
		);

		if observer_lifecycle.begin_start_if_needed()
			&& let Err(err) = scroll_input_macos::spawn_scroll_input_observer(
				Arc::clone(shared_state),
				Arc::clone(observer_lifecycle),
			) {
			observer_lifecycle.mark_failed();

			return Err(eyre::eyre!(
				"Scroll capture could not start the native scroll observer: {err}"
			));
		}

		match observer_lifecycle.wait_until_ready(SCROLL_INPUT_OBSERVER_READY_TIMEOUT) {
			ScrollInputObserverWaitOutcome::Ready => {
				tracing::info!(
					op = "scroll_input.prepare_ready",
					observer_status = ?observer_lifecycle.status(),
					enabled = shared_state.is_enabled(),
					"Native scroll input is ready for scroll capture."
				);

				Ok(())
			},
			ScrollInputObserverWaitOutcome::TimedOut => Err(eyre::eyre!(
				"Scroll capture is still starting the native scroll observer. Retry once."
			)),
			ScrollInputObserverWaitOutcome::Failed => Err(eyre::eyre!(
				"Scroll capture could not activate the native scroll observer. Retry once."
			)),
		}
	}

	#[cfg(target_os = "macos")]
	fn enable_external_scroll_input(shared_state: &Arc<SharedScrollInputState>) {
		shared_state.clear();
		shared_state.set_enabled(true);

		tracing::info!(
			op = "scroll_input.enabled",
			enabled = shared_state.is_enabled(),
			"Enabled native scroll input replay for scroll capture."
		);
	}

	pub(super) fn handle_overlay_control(&mut self, control: OverlayControl) {
		match control {
			OverlayControl::Continue => {},
			OverlayControl::HostEffect(request) => self.handle_overlay_host_effect_request(request),
			OverlayControl::Exit(exit) => self.end_overlay_session(exit),
		}

		#[cfg(target_os = "macos")]
		self.sync_overlay_hotkey_registrations();
		#[cfg(target_os = "macos")]
		self.sync_overlay_capture_host();
	}
}

#[cfg(all(test, target_os = "macos"))]
fn host_effect_test_hooks() -> &'static Mutex<HostEffectTestHooks> {
	HOST_EFFECT_TEST_HOOKS.get_or_init(|| Mutex::new(HostEffectTestHooks::default()))
}

#[cfg(all(test, target_os = "macos"))]
fn set_host_effect_test_hooks(hooks: HostEffectTestHooks) {
	let mut guard =
		host_effect_test_hooks().lock().expect("host-effect test hooks lock should be available");

	*guard = hooks;
}

#[cfg(all(test, target_os = "macos"))]
fn host_effect_test_serial() -> &'static Mutex<()> {
	HOST_EFFECT_TEST_SERIAL.get_or_init(|| Mutex::new(()))
}

#[cfg(all(test, target_os = "macos"))]
fn copy_png_host_effect_test_hook() -> Option<Arc<CopyPngHostEffectHook>> {
	host_effect_test_hooks()
		.lock()
		.expect("host-effect test hooks lock should be available")
		.copy_png
		.as_ref()
		.map(Arc::clone)
}

#[cfg(all(test, target_os = "macos"))]
fn deferred_text_recognition_handoff_test_hook() -> Option<Arc<DeferredTextRecognitionHandoffHook>>
{
	host_effect_test_hooks()
		.lock()
		.expect("host-effect test hooks lock should be available")
		.deferred_text_recognition_handoff
		.as_ref()
		.map(Arc::clone)
}

#[cfg(target_os = "macos")]
fn deferred_text_recognition_publish_allowed(
	latest_generation: &Arc<AtomicU64>,
	pending_generation: &Arc<AtomicU64>,
	request_generation: u64,
) -> bool {
	loop {
		let pending_generation = pending_generation.load(Ordering::Acquire);

		if pending_generation > request_generation {
			thread::sleep(DEFERRED_OCR_PUBLISH_PENDING_POLL_INTERVAL);

			continue;
		}

		return latest_generation.load(Ordering::Acquire) == request_generation;
	}
}

fn save_png_bytes_to_configured_dir(
	png_bytes: &[u8],
	output_dir: &Path,
	output_filename_prefix: &str,
	output_naming: OutputNaming,
) -> Result<PathBuf> {
	let output_dir = if output_dir.as_os_str().is_empty() {
		PathBuf::from(".")
	} else {
		output_dir.to_path_buf()
	};

	fs::create_dir_all(&output_dir)
		.wrap_err_with(|| format!("Failed to create output directory: {}", output_dir.display()))?;

	let prefix = settings::sanitize_output_filename_prefix(output_filename_prefix);
	let target_path = next_output_png_path(&output_dir, &prefix, output_naming);

	write_png_bytes_atomic(&target_path, png_bytes)?;

	Ok(target_path)
}

#[cfg(target_os = "macos")]
fn write_png_bytes_to_clipboard(png_bytes: &[u8]) -> Result<()> {
	#[cfg(all(test, target_os = "macos"))]
	if let Some(copy_hook) = copy_png_host_effect_test_hook() {
		return copy_hook(png_bytes);
	}

	let pasteboard_type = CString::new("public.png").wrap_err("Invalid NSPasteboard type")?;

	unsafe {
		let data: *mut Object = objc::msg_send![
			objc::class!(NSData),
			dataWithBytes: png_bytes.as_ptr()
			length: png_bytes.len()
		];
		let pasteboard: *mut Object =
			objc::msg_send![objc::class!(NSPasteboard), generalPasteboard];
		let _: i64 = objc::msg_send![pasteboard, clearContents];
		let ty: *mut Object = objc::msg_send![
			objc::class!(NSString),
			stringWithUTF8String: pasteboard_type.as_ptr()
		];
		let ok: BOOL = objc::msg_send![pasteboard, setData: data forType: ty];

		if ok != YES {
			return Err(eyre::eyre!("NSPasteboard setData:forType failed"));
		}
	}

	Ok(())
}

#[cfg(not(target_os = "macos"))]
fn write_png_bytes_to_clipboard(png_bytes: &[u8]) -> Result<()> {
	let image = image::load_from_memory(png_bytes).wrap_err("Failed to decode PNG bytes")?;
	let rgba = image.to_rgba8();
	let (width, height) = rgba.dimensions();
	let mut clipboard = Clipboard::new().wrap_err("Failed to initialize clipboard")?;

	clipboard
		.set_image(ImageData {
			width: width as usize,
			height: height as usize,
			bytes: Cow::Owned(rgba.into_raw()),
		})
		.wrap_err("Failed to write image to clipboard")?;

	Ok(())
}

#[cfg(target_os = "macos")]
fn write_text_to_clipboard(text: &str) -> Result<()> {
	let mut clipboard = Clipboard::new().wrap_err("Failed to initialize clipboard")?;

	clipboard.set_text(text.to_string()).wrap_err("Failed to write text to clipboard")?;

	Ok(())
}

fn next_output_png_path(output_dir: &Path, prefix: &str, naming: OutputNaming) -> PathBuf {
	let base = match naming {
		OutputNaming::Timestamp => format!("{prefix}-{}", current_unix_millis()),
		OutputNaming::Sequence => {
			format!("{prefix}-{:04}", next_sequence_index(output_dir, prefix))
		},
	};

	unique_png_path(output_dir, &base)
}

fn current_unix_millis() -> u128 {
	SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_millis())
}

fn next_sequence_index(output_dir: &Path, prefix: &str) -> u32 {
	let Ok(entries) = fs::read_dir(output_dir) else {
		return 1;
	};
	let mut max_seen = 0_u32;

	for entry in entries.flatten() {
		let file_name = entry.file_name();
		let Some(file_name) = file_name.to_str() else {
			continue;
		};
		let Some(stem) = file_name.strip_suffix(".png") else {
			continue;
		};
		let Some(number_text) = stem.strip_prefix(prefix).and_then(|rest| rest.strip_prefix('-'))
		else {
			continue;
		};

		if !number_text.chars().all(|ch| ch.is_ascii_digit()) {
			continue;
		}

		if let Ok(value) = number_text.parse::<u32>() {
			max_seen = max_seen.max(value);
		}
	}

	max_seen.saturating_add(1).max(1)
}

fn unique_png_path(output_dir: &Path, base: &str) -> PathBuf {
	let direct_path = output_dir.join(format!("{base}.png"));

	if !direct_path.exists() {
		return direct_path;
	}

	let mut suffix = 2_u32;

	loop {
		let candidate = output_dir.join(format!("{base}-{suffix}.png"));

		if !candidate.exists() {
			return candidate;
		}

		suffix = suffix.saturating_add(1);
	}
}

fn write_png_bytes_atomic(target_path: &Path, png_bytes: &[u8]) -> Result<()> {
	let tmp_path = target_path.with_extension("png.tmp");

	fs::write(&tmp_path, png_bytes)
		.wrap_err_with(|| format!("Failed to write temporary PNG file: {}", tmp_path.display()))?;
	fs::rename(&tmp_path, target_path)
		.wrap_err_with(|| format!("Failed to finalize PNG file: {}", target_path.display()))?;

	Ok(())
}

fn self_capture_exception_window_ids_from_sources(
	current_window_id: Option<u32>,
	_cached_window_id: Option<u32>,
) -> Vec<u32> {
	current_window_id.into_iter().collect()
}

#[cfg(test)]
mod tests {
	#[cfg(target_os = "macos")]
	use std::sync::{
		Arc,
		atomic::{AtomicU64, AtomicUsize, Ordering},
	};
	#[cfg(target_os = "macos")]
	use std::time::Instant;
	#[cfg(target_os = "macos")]
	use std::{fs, thread, time::Duration};
	use std::env;
	use std::process;

	use crate::app::capture;
	#[cfg(target_os = "macos")]
	use crate::app::scroll_input_macos::{ScrollInputObserverLifecycle, SharedScrollInputState};
	#[cfg(target_os = "macos")]
	use crate::app::{App, OverlayEventProxy, UserEvent};
	#[cfg(target_os = "macos")]
	use crate::settings::AppSettings;
	#[cfg(target_os = "macos")]
	use rsnap_overlay::{
		DeferredTextRecognitionRequest, OutputNaming, OverlayConfig, OverlayControl,
		OverlayHostEffectRequest, OverlaySession,
	};


	#[cfg(target_os = "macos")]
	struct HostEffectTestHooksGuard {
		_serial: std::sync::MutexGuard<'static, ()>,
	}

	#[cfg(target_os = "macos")]
	impl Drop for HostEffectTestHooksGuard {
		fn drop(&mut self) {
			capture::set_host_effect_test_hooks(capture::HostEffectTestHooks::default());
		}
	}

	#[test]
	fn self_capture_exception_window_ids_ignore_stale_cached_settings_window_id() {
		assert_eq!(
			capture::self_capture_exception_window_ids_from_sources(None, Some(41)),
			Vec::<u32>::new()
		);
	}

	#[test]
	fn self_capture_exception_window_ids_prefer_live_settings_window_id() {
		assert_eq!(
			capture::self_capture_exception_window_ids_from_sources(Some(7), Some(41)),
			vec![7]
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn deferred_text_recognition_publish_allowed_waits_for_newer_capture_resolution() {
		let latest_generation = Arc::new(AtomicU64::new(7));
		let pending_generation = Arc::new(AtomicU64::new(8));
		let latest_generation_writer = Arc::clone(&latest_generation);
		let pending_generation_writer = Arc::clone(&pending_generation);
		let resolver = thread::spawn(move || {
			thread::sleep(Duration::from_millis(15));

			latest_generation_writer.store(8, Ordering::Release);
			pending_generation_writer.store(0, Ordering::Release);
		});

		assert!(!capture::deferred_text_recognition_publish_allowed(
			&latest_generation,
			&pending_generation,
			7,
		));

		resolver.join().expect("publish gate resolver should finish");
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn deferred_text_recognition_publish_allowed_accepts_failed_newer_capture_start() {
		let latest_generation = Arc::new(AtomicU64::new(7));
		let pending_generation = Arc::new(AtomicU64::new(8));
		let pending_generation_writer = Arc::clone(&pending_generation);
		let resolver = thread::spawn(move || {
			thread::sleep(Duration::from_millis(15));

			pending_generation_writer.store(0, Ordering::Release);
		});

		assert!(capture::deferred_text_recognition_publish_allowed(
			&latest_generation,
			&pending_generation,
			7,
		));

		resolver.join().expect("publish gate resolver should finish");
	}

	#[cfg(target_os = "macos")]
	fn install_host_effect_test_hooks(
		hooks: capture::HostEffectTestHooks,
	) -> HostEffectTestHooksGuard {
		let serial = capture::host_effect_test_serial()
			.lock()
			.expect("host-effect test serial lock should be available");

		capture::set_host_effect_test_hooks(hooks);

		HostEffectTestHooksGuard { _serial: serial }
	}

	#[cfg(target_os = "macos")]
	fn test_app() -> App {
		let settings = AppSettings::default();
		let capture_hotkey = settings.capture_hotkey();
		let overlay_proxy = OverlayEventProxy::for_test(Arc::new(|_event: UserEvent| Ok(())));
		let mut app = App::new(
			capture_hotkey,
			settings,
			None,
			None,
			overlay_proxy,
			Arc::new(ScrollInputObserverLifecycle::default()),
			Arc::new(SharedScrollInputState::default()),
		);

		app.capture_success_sound = None;

		app
	}

	#[cfg(target_os = "macos")]
	fn unique_test_dir(label: &str) -> std::path::PathBuf {
		env::temp_dir().join(format!(
			"rsnap-{label}-{}-{}",
			process::id(),
			capture::current_unix_millis()
		))
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn copy_png_host_effect_success_tears_down_the_overlay_session() {
		let call_count = Arc::new(AtomicUsize::new(0));
		let _hooks = install_host_effect_test_hooks(capture::HostEffectTestHooks {
			copy_png: Some(Arc::new({
				let call_count = Arc::clone(&call_count);

				move |png_bytes| {
					call_count.fetch_add(1, Ordering::AcqRel);

					assert_eq!(png_bytes, [1, 2, 3, 4]);

					Ok(())
				}
			})),
			deferred_text_recognition_handoff: None,
		});
		let mut app = test_app();

		app.overlay_session = Some(OverlaySession::with_config(OverlayConfig::default()));

		app.handle_overlay_control(OverlayControl::HostEffect(OverlayHostEffectRequest::CopyPng {
			png_bytes: vec![1, 2, 3, 4],
		}));

		assert_eq!(call_count.load(Ordering::Acquire), 1);
		assert!(app.overlay_session.is_none());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn copy_png_host_effect_failure_keeps_the_overlay_session_alive() {
		let call_count = Arc::new(AtomicUsize::new(0));
		let _hooks = install_host_effect_test_hooks(capture::HostEffectTestHooks {
			copy_png: Some(Arc::new({
				let call_count = Arc::clone(&call_count);

				move |_png_bytes| {
					call_count.fetch_add(1, Ordering::AcqRel);

					Err(color_eyre::eyre::eyre!("copy failed"))
				}
			})),
			deferred_text_recognition_handoff: None,
		});
		let mut app = test_app();

		app.overlay_session = Some(OverlaySession::with_config(OverlayConfig::default()));

		app.handle_overlay_control(OverlayControl::HostEffect(OverlayHostEffectRequest::CopyPng {
			png_bytes: vec![9, 8, 7, 6],
		}));

		assert_eq!(call_count.load(Ordering::Acquire), 1);

		let session = app
			.overlay_session
			.as_ref()
			.expect("copy failure should keep the overlay session active");

		assert_eq!(session.debug_error_message(), Some("copy failed"));
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn save_png_host_effect_success_writes_the_png_and_tears_down_the_overlay_session() {
		let output_dir = unique_test_dir("save-success");
		let png_bytes = vec![4, 3, 2, 1];
		let mut app = test_app();

		app.overlay_session = Some(OverlaySession::with_config(OverlayConfig::default()));

		app.handle_overlay_control(OverlayControl::HostEffect(OverlayHostEffectRequest::SavePng {
			png_bytes: png_bytes.clone(),
			output_dir: output_dir.clone(),
			output_filename_prefix: String::from("capture"),
			output_naming: OutputNaming::Sequence,
		}));

		let entries = fs::read_dir(&output_dir)
			.expect("save host effect should create the output directory")
			.collect::<Result<Vec<_>, _>>()
			.expect("saved directory entries should be readable");

		assert_eq!(entries.len(), 1);
		assert_eq!(fs::read(entries[0].path()).expect("saved PNG should be readable"), png_bytes);
		assert!(app.overlay_session.is_none());

		fs::remove_dir_all(&output_dir).expect("save-success output directory should be removable");
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn save_png_host_effect_failure_keeps_the_overlay_session_alive() {
		let base_dir = unique_test_dir("save-failure");
		let output_dir = base_dir.join("occupied");
		let mut app = test_app();

		fs::create_dir_all(&base_dir).expect("save-failure base directory should be creatable");
		fs::write(&output_dir, b"not a directory")
			.expect("occupied output path should be creatable as a file");

		app.overlay_session = Some(OverlaySession::with_config(OverlayConfig::default()));

		app.handle_overlay_control(OverlayControl::HostEffect(OverlayHostEffectRequest::SavePng {
			png_bytes: vec![6, 7, 8, 9],
			output_dir: output_dir.clone(),
			output_filename_prefix: String::from("capture"),
			output_naming: OutputNaming::Sequence,
		}));

		let session = app
			.overlay_session
			.as_ref()
			.expect("save failure should keep the overlay session active");

		assert!(
			session
				.debug_error_message()
				.is_some_and(|message| message.contains("Failed to create output directory"))
		);

		fs::remove_dir_all(&base_dir).expect("save-failure output directory should be removable");
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn deferred_text_recognition_host_effect_hands_the_request_to_the_app_worker_after_teardown() {
		let handoff_count = Arc::new(AtomicUsize::new(0));
		let handed_off_request_id = Arc::new(AtomicU64::new(u64::MAX));
		let handed_off_generation = Arc::new(AtomicU64::new(u64::MAX));
		let mut app = test_app();
		let shared_state = Arc::clone(&app.scroll_input_shared_state);

		shared_state.set_enabled(true);
		shared_state.record(-12.0, 140.0, 220.0, true, false);

		assert_eq!(
			shared_state.replay_after_seq_through(0, Instant::now() + Duration::from_secs(1)).len(),
			1
		);

		let _hooks = install_host_effect_test_hooks(capture::HostEffectTestHooks {
			copy_png: None,
			deferred_text_recognition_handoff: Some(Arc::new({
				let handoff_count = Arc::clone(&handoff_count);
				let handed_off_request_id = Arc::clone(&handed_off_request_id);
				let handed_off_generation = Arc::clone(&handed_off_generation);
				let shared_state = Arc::clone(&shared_state);

				move |request, _latest_generation, _pending_generation, request_generation| {
					handoff_count.fetch_add(1, Ordering::AcqRel);
					handed_off_request_id.store(request.request_id, Ordering::Release);
					handed_off_generation.store(request_generation, Ordering::Release);

					assert!(!shared_state.is_enabled());
					assert!(
						shared_state
							.replay_after_seq_through(0, Instant::now() + Duration::from_secs(1))
							.is_empty()
					);
				}
			})),
		});
		let request_id = 44;

		app.overlay_session = Some(OverlaySession::with_config(OverlayConfig::default()));
		app.overlay_session_generation = 17;

		app.latest_deferred_ocr_generation.store(17, Ordering::Release);
		app.pending_deferred_ocr_generation.store(0, Ordering::Release);
		app.handle_overlay_control(OverlayControl::HostEffect(
			OverlayHostEffectRequest::DeferredTextRecognition(
				DeferredTextRecognitionRequest::debug_prepared_for_test(
					request_id,
					Instant::now(),
					image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255])),
				),
			),
		));

		assert_eq!(handoff_count.load(Ordering::Acquire), 1);
		assert_eq!(handed_off_request_id.load(Ordering::Acquire), request_id);
		assert_eq!(handed_off_generation.load(Ordering::Acquire), 17);
		assert!(app.overlay_session.is_none());
	}
}
