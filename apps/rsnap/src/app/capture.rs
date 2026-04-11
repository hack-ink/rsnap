#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicU64;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "macos")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use std::thread::Builder;
#[cfg(target_os = "macos")]
use std::time::Duration;
use std::time::Instant;

#[cfg(target_os = "macos")]
use color_eyre::eyre;
#[cfg(target_os = "macos")]
use color_eyre::eyre::Result;
use winit::event_loop::ActiveEventLoop;

use crate::app::App;
#[cfg(target_os = "macos")]
use crate::app::UserEvent;
#[cfg(target_os = "macos")]
use crate::app::scroll_input_macos::{
	self, ScrollInputObserverLifecycle, ScrollInputObserverWaitOutcome, SharedScrollInputState,
};
#[cfg(target_os = "macos")]
use crate::permissions_macos;
#[cfg(target_os = "macos")]
use rsnap_overlay::DeferredTextRecognitionRequest;
use rsnap_overlay::{HudAnchor, OverlayConfig, OverlayControl, OverlayExit, OverlaySession};

#[cfg(target_os = "macos")]
const SCROLL_INPUT_OBSERVER_READY_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(target_os = "macos")]
const OVERLAY_SESSION_PREWARM_RETRY_BACKOFF: Duration = Duration::from_secs(1);

impl App {
	#[cfg(target_os = "macos")]
	fn register_overlay_cancel_hotkey(&mut self) {
		if !self.overlay_cancel_hotkey_registration_state.allows_register_attempt() {
			return;
		}

		let Some(manager) = self._hotkey_manager.as_mut() else {
			self.overlay_cancel_hotkey_registration_state =
				super::OverlayCancelHotkeyRegistrationState::Blocked;
			tracing::warn!(
				hotkey = "Esc",
				"Capture cancel hotkey is unavailable because the global hotkey manager is missing."
			);

			return;
		};

		if let Err(err) = manager.register(self.overlay_cancel_hotkey) {
			self.overlay_cancel_hotkey_registration_state =
				super::OverlayCancelHotkeyRegistrationState::next_state_after_register_error(&err);
			tracing::warn!(
				error = ?err,
				hotkey = "Esc",
				hotkey_id = %self.overlay_cancel_hotkey_id,
				"Failed to register the capture cancel hotkey."
			);
		} else {
			self.overlay_cancel_hotkey_registration_state =
				super::OverlayCancelHotkeyRegistrationState::Registered;
			tracing::info!(
				hotkey = "Esc",
				hotkey_id = %self.overlay_cancel_hotkey_id,
				"Registered the capture cancel hotkey."
			);
		}
	}

	#[cfg(target_os = "macos")]
	fn unregister_overlay_cancel_hotkey(&mut self) {
		if matches!(
			self.overlay_cancel_hotkey_registration_state,
			super::OverlayCancelHotkeyRegistrationState::Unregistered
		) {
			return;
		}
		if matches!(
			self.overlay_cancel_hotkey_registration_state,
			super::OverlayCancelHotkeyRegistrationState::Blocked
		) {
			self.overlay_cancel_hotkey_registration_state =
				super::OverlayCancelHotkeyRegistrationState::Unregistered;

			return;
		}

		let Some(manager) = self._hotkey_manager.as_mut() else {
			self.overlay_cancel_hotkey_registration_state =
				super::OverlayCancelHotkeyRegistrationState::Unregistered;

			return;
		};

		if let Err(err) = manager.unregister(self.overlay_cancel_hotkey) {
			self.overlay_cancel_hotkey_registration_state =
				super::OverlayCancelHotkeyRegistrationState::Unregistered;
			tracing::warn!(
				error = ?err,
				hotkey = "Esc",
				hotkey_id = %self.overlay_cancel_hotkey_id,
				"Failed to unregister the capture cancel hotkey."
			);
		} else {
			self.overlay_cancel_hotkey_registration_state =
				super::OverlayCancelHotkeyRegistrationState::Unregistered;
			tracing::info!(
				hotkey = "Esc",
				hotkey_id = %self.overlay_cancel_hotkey_id,
				"Unregistered the capture cancel hotkey."
			);
		}
	}

	#[cfg(target_os = "macos")]
	fn sync_overlay_cancel_hotkey_registration(&mut self) {
		let should_register =
			self.overlay_session.as_ref().is_some_and(OverlaySession::wants_global_cancel_hotkey);

		if should_register {
			self.register_overlay_cancel_hotkey();
		} else {
			self.unregister_overlay_cancel_hotkey();
		}
	}

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
				self.sync_overlay_cancel_hotkey_registration();
			},
			Err(err) => {
				let overlay_start_ms = overlay_start_started_at.elapsed().as_millis();

				#[cfg(target_os = "macos")]
				self.pending_deferred_ocr_generation.store(0, Ordering::Release);
				#[cfg(target_os = "macos")]
				{
					self.scroll_input_shared_state.set_enabled(false);
					self.scroll_input_shared_state.set_event_waker(None);
					self.scroll_input_shared_state.clear();
				}

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

				#[cfg(target_os = "macos")]
				{
					self.overlay_session_prewarm_requested = true;
				}
			},
		}
	}

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
		let Some(_session) = self.overlay_session.take() else {
			return;
		};
		#[cfg(target_os = "macos")]
		self.unregister_overlay_cancel_hotkey();

		#[cfg(target_os = "macos")]
		{
			self.prewarmed_overlay_session = None;
			self.overlay_session_prewarm_requested = true;
			self.overlay_session_prewarm_retry_not_before = None;

			self.scroll_input_shared_state.set_enabled(false);
			self.scroll_input_shared_state.set_event_waker(None);
			self.scroll_input_shared_state.clear();
		}

		match exit {
			OverlayExit::Cancelled => tracing::info!("Capture cancelled."),
			OverlayExit::PngBytes(png_bytes) => {
				tracing::info!(bytes = png_bytes.len(), "Capture copied to clipboard.");
			},
			OverlayExit::TextCopied(character_count) => {
				tracing::info!(
					characters = character_count,
					"Recognized text copied to clipboard."
				);
			},
			#[cfg(target_os = "macos")]
			OverlayExit::DeferredTextRecognition(request) => {
				let request_id = request.request_id;
				let request_generation = self.overlay_session_generation;
				let latest_deferred_ocr_generation =
					Arc::clone(&self.latest_deferred_ocr_generation);
				let pending_deferred_ocr_generation =
					Arc::clone(&self.pending_deferred_ocr_generation);
				let request_slot = Arc::new(Mutex::new(Some(request)));
				let request_slot_for_worker = Arc::clone(&request_slot);
				let latest_deferred_ocr_generation_for_worker =
					Arc::clone(&latest_deferred_ocr_generation);
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

						let Some(request) =
							Self::take_deferred_text_recognition_request(&request_slot)
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
			},
			OverlayExit::Saved(path) => {
				tracing::info!(path = %path.display(), "Capture saved to file.");
			},
			OverlayExit::Error(message) => tracing::warn!(error = %message, "Capture failed."),
		};

		tracing::info!("Capture overlay ended.");
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
		let _ = rsnap_overlay::process_deferred_text_recognition_for_latest_capture(
			request,
			latest_deferred_ocr_generation,
			pending_deferred_ocr_generation,
			request_generation,
		);
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
		if let OverlayControl::Exit(exit) = control {
			self.end_overlay_session(exit);
		}
		#[cfg(target_os = "macos")]
		self.sync_overlay_cancel_hotkey_registration();
	}
}

fn self_capture_exception_window_ids_from_sources(
	current_window_id: Option<u32>,
	cached_window_id: Option<u32>,
) -> Vec<u32> {
	current_window_id.or(cached_window_id).into_iter().collect()
}

#[cfg(test)]
mod tests {
	use crate::app::capture;

	#[test]
	fn self_capture_exception_window_ids_fall_back_to_cached_settings_window_id() {
		assert_eq!(
			capture::self_capture_exception_window_ids_from_sources(None, Some(41)),
			vec![41]
		);
	}

	#[test]
	fn self_capture_exception_window_ids_prefer_live_settings_window_id() {
		assert_eq!(
			capture::self_capture_exception_window_ids_from_sources(Some(7), Some(41)),
			vec![7]
		);
	}
}
