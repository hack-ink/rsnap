use winit::window::Window;
#[cfg(target_os = "macos")]
use winit::window::WindowId;

#[cfg(target_os = "macos")]
use crate::backend;
#[cfg(target_os = "macos")]
use crate::overlay::OverlayWorker;
use crate::overlay::{self, toolbar_layout_model};
use crate::overlay::{
	Arc, Instant, LOUPE_TILE_CORNER_RADIUS_POINTS, OverlayConfig, OverlayMode, OverlaySession,
	WindowRenderer,
};
#[cfg(target_os = "macos")]
use crate::overlay::{
	FrozenCaptureWorkerState, MacLiveFrameStream, MacOSHudWindowConfigState,
	SLOW_OP_WARN_HUD_CONFIG,
};

impl OverlaySession {
	/// Applies updated runtime configuration to an existing session.
	pub fn set_config(&mut self, config: OverlayConfig) {
		let prev = self.config.clone();
		let previous_loupe_patch = self.loupe_patch_width_px;
		let loupe_sample_side = Self::normalized_loupe_sample_side_px(config.loupe_sample_side_px);
		#[cfg(target_os = "macos")]
		let self_capture_exception_window_ids_changed =
			prev.self_capture_exception_window_ids != config.self_capture_exception_window_ids;

		self.config = config;
		self.loupe_patch_width_px = loupe_sample_side;
		self.loupe_patch_height_px = loupe_sample_side;
		self.state.loupe_patch_side_px = loupe_sample_side;

		let patch_changed = self.loupe_patch_width_px != previous_loupe_patch;

		if patch_changed {
			self.state.loupe = None;
		}
		if !self.is_active() {
			if self.has_prewarmed_startup_resources() {
				self.configure_hud_windows_for_config();
			}

			return;
		}

		self.configure_hud_windows_for_config();

		let prev_fake_blur = prev.show_hud_blur && !cfg!(target_os = "macos");
		let new_fake_blur = self.use_fake_hud_blur();

		self.handle_fake_hud_blur_toggle(prev_fake_blur, new_fake_blur);

		if patch_changed {
			self.request_loupe_sample_for_patch_change();
		}
		#[cfg(target_os = "macos")]
		if self_capture_exception_window_ids_changed {
			self.apply_self_capture_exception_window_ids_to_active_streams();
		}

		self.request_redraw_all();
	}

	#[cfg(target_os = "macos")]
	pub(super) fn apply_self_capture_exception_window_ids_to_active_streams(&mut self) {
		self.invalidate_window_list_snapshot_for_self_capture_exception_window_ids_change();

		self.live_sample_stream = Some(MacLiveFrameStream::with_self_capture_exception_window_ids(
			self.config.self_capture_exception_window_ids.clone(),
		));

		self.rebuild_active_scroll_capture_live_stream();
		self.refresh_active_worker_for_self_capture_exception_window_ids_if_safe();
	}

	#[cfg(target_os = "macos")]
	pub(super) fn rebuild_active_scroll_capture_live_stream(&mut self) -> bool {
		if !self.scroll_capture.active {
			return false;
		}

		self.scroll_capture.live_stream = if self.should_use_scroll_capture_worker_sampling() {
			None
		} else {
			match (self.scroll_capture.capture_rect_points, self.scroll_capture.capture_rect_pixels)
			{
				(Some(capture_rect_points), Some(capture_rect_pixels)) => {
					Some(MacLiveFrameStream::with_scroll_capture_region_and_waker(
						self.config.self_capture_exception_window_ids.clone(),
						capture_rect_points,
						capture_rect_pixels,
						self.scroll_frame_waker.clone(),
					))
				},
				_ => Some(MacLiveFrameStream::with_self_capture_exception_window_ids_and_waker(
					self.config.self_capture_exception_window_ids.clone(),
					self.scroll_frame_waker.clone(),
				)),
			}
		};

		self.scroll_capture.live_stream_backlog.clear();

		self.scroll_capture.last_stream_frame_seq = 0;
		self.scroll_capture.last_stream_frame_fingerprint = None;
		self.scroll_capture.consecutive_identical_stream_frames = 0;
		self.scroll_capture.last_consumed_stream_frame_captured_at = None;
		self.scroll_capture.last_stream_event_at = None;
		self.scroll_capture.last_stream_poll_at = None;
		self.scroll_capture.pending_post_stall_burst_after_seq = None;
		self.scroll_capture.live_stream_stale_grace = None;
		self.scroll_capture.last_duplicate_stream_refresh_at = None;

		self.scroll_capture.live_stream.is_some()
	}

	#[cfg(target_os = "macos")]
	fn invalidate_window_list_snapshot_for_self_capture_exception_window_ids_change(&mut self) {
		self.window_list_snapshot = None;
		self.drop_next_window_list_refresh_snapshot = self.window_list_refresh_inflight;
		self.last_window_list_refresh_request_at =
			Instant::now() - self.window_list_refresh_interval;
	}

	#[cfg(target_os = "macos")]
	fn refresh_active_worker_for_self_capture_exception_window_ids_if_safe(&mut self) {
		if self.has_inflight_worker_response_state() {
			self.pending_self_capture_exception_window_ids_worker_refresh = true;

			return;
		}

		self.rebuild_active_worker_for_self_capture_exception_window_ids();
	}

	#[cfg(target_os = "macos")]
	pub(super) fn maybe_apply_pending_self_capture_exception_window_ids_worker_refresh(&mut self) {
		if self.pending_self_capture_exception_window_ids_worker_refresh
			&& !self.has_inflight_worker_response_state()
		{
			self.rebuild_active_worker_for_self_capture_exception_window_ids();
		}
	}

	#[cfg(target_os = "macos")]
	fn rebuild_active_worker_for_self_capture_exception_window_ids(&mut self) {
		self.worker = Some(OverlayWorker::new(
			backend::default_capture_backend_with_self_capture_exception_window_ids(
				self.config.self_capture_exception_window_ids.clone(),
			),
			self.response_waker.clone(),
		));
		self.pending_self_capture_exception_window_ids_worker_refresh = false;
	}

	#[cfg(target_os = "macos")]
	pub(super) fn has_inflight_worker_response_state(&self) -> bool {
		self.frozen_capture_worker_state() == Some(FrozenCaptureWorkerState::Inflight)
			|| self.pending_click_hit_test_request_id.is_some()
			|| self.window_list_refresh_inflight
			|| self.png_encode_inflight
	}

	pub(super) fn configure_hud_windows_for_config(&mut self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			let window = Arc::clone(&hud_window.window);

			self.configure_hud_window_common(window.as_ref(), None);
		}
		if let Some(loupe_window) = self.loupe_window.as_ref() {
			let window = Arc::clone(&loupe_window.window);

			self.configure_hud_window_common(
				window.as_ref(),
				Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
			);
		}
		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			let window = Arc::clone(&toolbar_window.window);
			let toolbar_height_points =
				WindowRenderer::frozen_toolbar_primary_size(&self.toolbar_state).y;

			self.configure_hud_window_common(
				window.as_ref(),
				Some(toolbar_layout_model::frozen_toolbar_corner_radius_points(
					toolbar_height_points,
				)),
			);
		}
	}

	pub(super) fn configure_hud_window_common(
		&mut self,
		window: &Window,
		corner_radius: Option<f64>,
	) {
		window.set_transparent(true);

		#[cfg(not(target_os = "macos"))]
		let _ = corner_radius;

		#[cfg(not(target_os = "macos"))]
		window.set_blur(self.config.show_hud_blur);
		#[cfg(target_os = "macos")]
		self.configure_macos_hud_window_cached(
			window,
			self.macos_hud_window_blur_enabled(),
			self.config.hud_fog_amount,
			corner_radius,
		);
	}

	#[cfg(target_os = "macos")]
	fn configure_macos_hud_window_cached(
		&mut self,
		window: &Window,
		blur_enabled: bool,
		blur_amount: f32,
		corner_radius: Option<f64>,
	) {
		let effective_corner_radius = corner_radius.unwrap_or_else(|| {
			let scale = window.scale_factor().max(1.0);
			let size = window.inner_size();

			((size.height as f64) / scale) * 0.5
		});
		let desired =
			MacOSHudWindowConfigState::new(blur_enabled, blur_amount, effective_corner_radius);

		if self
			.macos_hud_window_config_cache
			.get(&window.id())
			.is_some_and(|cached| cached.same(&desired))
		{
			return;
		}

		let started_at = Instant::now();

		overlay::macos_configure_hud_window(
			window,
			blur_enabled,
			blur_amount,
			Some(effective_corner_radius),
		);

		let elapsed = started_at.elapsed();

		self.slow_op_logger.warn_if_slow(
			"overlay.macos_hud_window_configure",
			elapsed,
			SLOW_OP_WARN_HUD_CONFIG,
			|| {
				format!(
					"window_id={:?} blur_enabled={} blur_amount={} corner_radius={effective_corner_radius}",
					window.id(),
					blur_enabled,
					blur_amount,
				)
			},
		);

		let _ = self.macos_hud_window_config_cache.insert(window.id(), desired);
	}

	#[cfg(target_os = "macos")]
	pub(super) fn remove_macos_hud_window_config_cache_entry(&mut self, window_id: WindowId) {
		let _ = self.macos_hud_window_config_cache.remove(&window_id);
	}

	fn handle_fake_hud_blur_toggle(&mut self, prev_fake_blur: bool, new_fake_blur: bool) {
		if prev_fake_blur == new_fake_blur {
			return;
		}
		if new_fake_blur {
			self.last_live_bg_request_at = Instant::now() - self.live_bg_request_interval;

			if matches!(self.state.mode, OverlayMode::Live)
				&& let Some(_cursor) = self.state.cursor
				&& let Some(monitor) = self.active_cursor_monitor()
			{
				self.maybe_request_live_bg(monitor);
			}

			return;
		}

		self.state.live_bg_monitor = None;
		self.state.live_bg_image = None;
	}

	fn request_loupe_sample_for_patch_change(&mut self) {
		let cursor = match self.state.cursor {
			Some(cursor) => cursor,
			None => return,
		};
		let monitor = match self.active_cursor_monitor() {
			Some(monitor) => monitor,
			None => return,
		};
		let _ = self.apply_live_hover_cache_state(monitor, cursor);
		let _ = self.request_live_cursor_sample(monitor, cursor, true);
		let _ = self.request_live_window_list_refresh_if_needed();
	}
}
