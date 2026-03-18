#[cfg(target_os = "macos")]
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};

#[cfg(target_os = "macos")]
use color_eyre::eyre;
#[cfg(target_os = "macos")]
use color_eyre::eyre::Result;
use winit::event_loop::ActiveEventLoop;

use crate::app::App;
#[cfg(target_os = "macos")]
use crate::app::scroll_input_macos::{self, SharedScrollInputState};
#[cfg(target_os = "macos")]
use crate::app::{self, UserEvent};
#[cfg(target_os = "macos")]
use crate::permissions_macos;
use rsnap_overlay::{HudAnchor, OverlayConfig, OverlayControl, OverlayExit, OverlaySession};

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
		if self.overlay_session.is_some() {
			tracing::info!(
				requested_by = %requested_by,
				"Capture already active; ignoring additional start request."
			);

			return;
		}
		#[cfg(target_os = "macos")]
		if !self.ensure_screen_recording_access(event_loop, requested_by) {
			return;
		}

		let mut overlay_session = OverlaySession::with_config(self.overlay_config());

		#[cfg(target_os = "macos")]
		self.scroll_input_shared_state.clear();

		#[cfg(target_os = "macos")]
		overlay_session.set_scroll_frame_waker(Arc::new({
			let overlay_proxy = self.overlay_proxy.clone();
			let overlay_stream_event_pending = Arc::clone(&self.overlay_stream_event_pending);

			move || {
				if !app::begin_coalesced_overlay_user_event_send(&overlay_stream_event_pending) {
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

		#[cfg(target_os = "macos")]
		overlay_session.set_scroll_capture_start_guard(Arc::new({
			move || Self::ensure_scroll_capture_permissions()
		}));

		#[cfg(target_os = "macos")]
		overlay_session.set_scroll_capture_started_hook(Arc::new({
			let shared_state = Arc::clone(&self.scroll_input_shared_state);
			let observer_started = Arc::clone(&self.scroll_input_observer_started);

			move || Self::enable_external_scroll_input(&shared_state, &observer_started)
		}));

		match overlay_session.start(event_loop) {
			Ok(()) => {
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

	#[cfg(target_os = "macos")]
	fn ensure_screen_recording_access(
		&mut self,
		event_loop: &ActiveEventLoop,
		requested_by: &'static str,
	) -> bool {
		if permissions_macos::screen_recording_access_granted() {
			return true;
		}

		let requested = permissions_macos::request_screen_recording_access();

		if requested || permissions_macos::screen_recording_access_granted() {
			tracing::info!(
				requested_by = %requested_by,
				"Screen Recording access is available after a just-in-time permission check."
			);

			return true;
		}

		if let Err(err) = permissions_macos::open_screen_recording_settings() {
			tracing::warn!(error = %err, "Failed to open Screen Recording settings.");
		}

		tracing::warn!(
			requested_by = %requested_by,
			settings_path = %permissions_macos::SCREEN_RECORDING_SETTINGS_PATH,
			"Screen Recording access is missing; redirecting the user to the in-app recovery path."
		);

		self.open_settings_window(event_loop, "screen-recording-permission");

		false
	}

	pub(super) fn end_overlay_session(&mut self, exit: OverlayExit) {
		let Some(_session) = self.overlay_session.take() else {
			return;
		};

		#[cfg(target_os = "macos")]
		{
			self.scroll_input_shared_state.set_enabled(false);
			self.scroll_input_shared_state.clear();
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
	fn ensure_scroll_capture_permissions() -> Result<()> {
		if !permissions_macos::accessibility_access_granted() {
			let granted_after = permissions_macos::request_accessibility_access()
				|| permissions_macos::accessibility_access_granted();

			if !granted_after {
				if let Err(err) = permissions_macos::open_accessibility_settings() {
					tracing::warn!(error = %err, "Failed to open Accessibility settings.");
				}

				return Err(eyre::eyre!(
					"Scroll capture needs Accessibility. Enable rsnap in {} and retry.",
					permissions_macos::ACCESSIBILITY_SETTINGS_PATH
				));
			}
		}
		if !permissions_macos::input_monitoring_access_granted() {
			let requested = permissions_macos::request_input_monitoring_access();

			if !requested && !permissions_macos::input_monitoring_access_granted() {
				if let Err(err) = permissions_macos::open_input_monitoring_settings() {
					tracing::warn!(error = %err, "Failed to open Input Monitoring settings.");
				}

				return Err(eyre::eyre!(
					"Scroll capture needs Input Monitoring. Enable rsnap in {} and retry.",
					permissions_macos::INPUT_MONITORING_SETTINGS_PATH
				));
			}
		}

		Ok(())
	}

	#[cfg(target_os = "macos")]
	fn enable_external_scroll_input(
		shared_state: &Arc<SharedScrollInputState>,
		observer_started: &Arc<AtomicBool>,
	) {
		shared_state.clear();
		shared_state.set_enabled(true);

		if observer_started
			.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
			.is_ok()
		{
			scroll_input_macos::spawn_scroll_input_observer(Arc::clone(shared_state));
		}
	}

	pub(super) fn handle_overlay_control(&mut self, control: OverlayControl) {
		let OverlayControl::Exit(exit) = control else {
			return;
		};

		self.end_overlay_session(exit);
	}
}
