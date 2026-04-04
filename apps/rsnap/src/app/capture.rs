#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
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
use rsnap_overlay::{HudAnchor, OverlayConfig, OverlayControl, OverlayExit, OverlaySession};

#[cfg(target_os = "macos")]
const SCROLL_INPUT_OBSERVER_READY_TIMEOUT: Duration = Duration::from_millis(250);

impl App {
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
		let Some(session) = self.overlay_session.as_mut() else {
			return;
		};

		session.set_config(config);
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
		#[cfg(target_os = "macos")]
		let screen_recording_preflight_started_at = Instant::now();
		#[cfg(target_os = "macos")]
		let screen_recording_granted = self.ensure_screen_recording_access(requested_by);
		#[cfg(target_os = "macos")]
		let screen_recording_preflight_ms = screen_recording_preflight_started_at.elapsed().as_millis();
		#[cfg(not(target_os = "macos"))]
		let screen_recording_preflight_ms = 0;
		#[cfg(target_os = "macos")]
		if !screen_recording_granted {
			tracing::info!(
				op = "capture.start_phase_timing",
				requested_by = %requested_by,
				result = "blocked_missing_screen_recording",
				screen_recording_preflight_ms,
				total_ms = capture_start_started_at.elapsed().as_millis(),
				"Capture startup phase timing."
			);

			return;
		}

		let overlay_session_build_started_at = Instant::now();
		let mut overlay_session = OverlaySession::with_config(self.overlay_config());
		let overlay_session_build_ms = overlay_session_build_started_at.elapsed().as_millis();
		#[cfg(target_os = "macos")]
		{
			self.overlay_session_generation = self.overlay_session_generation.wrapping_add(1);
		}

		#[cfg(target_os = "macos")]
		let scroll_input_reset_started_at = Instant::now();
		#[cfg(target_os = "macos")]
		self.finish_coalesced_overlay_stream_frame_send();
		#[cfg(target_os = "macos")]
		self.scroll_input_shared_state.clear();
		#[cfg(target_os = "macos")]
		let scroll_input_reset_ms = scroll_input_reset_started_at.elapsed().as_millis();
		#[cfg(not(target_os = "macos"))]
		let scroll_input_reset_ms = 0;

		let hook_wiring_started_at = Instant::now();
		#[cfg(target_os = "macos")]
		self.scroll_input_shared_state.set_event_waker(Some(Arc::new({
			let overlay_proxy = self.overlay_proxy.clone();

			move || {
				let _ = overlay_proxy.send_event(UserEvent::OverlayScrollInput);
			}
		})));
		#[cfg(target_os = "macos")]
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
		#[cfg(target_os = "macos")]
		overlay_session.set_startup_aux_window_waker(Arc::new({
			let overlay_proxy = self.overlay_proxy.clone();
			let overlay_session_generation = self.overlay_session_generation;

			move || {
				let _ = overlay_proxy
					.send_event(UserEvent::OverlayStartupAuxWindows(overlay_session_generation));
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

		#[cfg(target_os = "macos")]
		overlay_session.set_scroll_capture_start_guard(Arc::new({
			move || Self::ensure_scroll_capture_permissions()
		}));

		#[cfg(target_os = "macos")]
		overlay_session.set_scroll_capture_starting_hook(Arc::new({
			let shared_state = Arc::clone(&self.scroll_input_shared_state);
			let observer_lifecycle = Arc::clone(&self.scroll_input_observer_lifecycle);

			move || Self::prepare_external_scroll_input(&shared_state, &observer_lifecycle)
		}));
		#[cfg(target_os = "macos")]
		overlay_session.set_scroll_capture_started_hook(Arc::new({
			let shared_state = Arc::clone(&self.scroll_input_shared_state);

			move || Self::enable_external_scroll_input(&shared_state)
		}));
		let hook_wiring_ms = hook_wiring_started_at.elapsed().as_millis();

		let overlay_start_started_at = Instant::now();
		match overlay_session.start(event_loop) {
			Ok(()) => {
				let overlay_start_ms = overlay_start_started_at.elapsed().as_millis();

				tracing::info!(
					op = "capture.start_phase_timing",
					requested_by = %requested_by,
					result = "started",
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
			},
			Err(err) => {
				let overlay_start_ms = overlay_start_started_at.elapsed().as_millis();

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
					overlay_session_build_ms,
					hook_wiring_ms,
					overlay_start_ms,
					total_ms = capture_start_started_at.elapsed().as_millis(),
					screen_recording_preflight_ms,
					scroll_input_reset_ms,
					"Failed to start overlay session."
				)
			},
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
		{
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
		let OverlayControl::Exit(exit) = control else {
			return;
		};

		self.end_overlay_session(exit);
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
