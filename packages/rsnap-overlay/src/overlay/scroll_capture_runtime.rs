#[cfg(target_os = "macos")]
use std::collections::VecDeque;
use std::time::Instant;

use color_eyre::Result;
use image::RgbaImage;

#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::MacLiveFrameStream;
use crate::overlay::{
	FrozenCaptureSource, Key, MonitorRect, NamedKey, OverlayControl, OverlayKeyboardInputEvent,
	OverlayMode, OverlaySession, PngAction, Pos2, Rect, SCROLL_CAPTURE_INPUT_FRESHNESS,
	SCROLL_CAPTURE_SAMPLE_INTERVAL, ScrollDirection, ScrollObserveOutcome, Vec2, WindowRenderer,
};
#[cfg(target_os = "macos")]
use crate::overlay::{
	MacOSScrollPixelResidual, RectPoints, SCROLL_CAPTURE_PREVIEW_WIDTH_PX, ScrollCaptureState,
	ScrollCaptureTraceRecorder, ScrollSession,
};

impl OverlaySession {
	#[cfg(test)]
	pub(super) fn observe_scroll_capture_frame(
		&mut self,
		frame: RgbaImage,
	) -> Option<Result<ScrollObserveOutcome>> {
		self.observe_scroll_capture_frame_at(frame, Instant::now())
	}

	#[cfg(test)]
	pub(super) fn observe_scroll_capture_frame_at(
		&mut self,
		frame: RgbaImage,
		observation_at: Instant,
	) -> Option<Result<ScrollObserveOutcome>> {
		self.observe_scroll_capture_frame_with_gate(frame, false, observation_at, false)
	}

	pub(super) fn observe_scroll_capture_frame_with_gate(
		&mut self,
		frame: RgbaImage,
		allow_stale_input: bool,
		observation_at: Instant,
		allow_post_stall_burst_search: bool,
	) -> Option<Result<ScrollObserveOutcome>> {
		let prior_block_reason = self.scroll_capture_observation_block_reason_at(observation_at);
		#[cfg(target_os = "macos")]
		let consumed_live_stream_stale_grace = !allow_stale_input
			&& prior_block_reason == Some("stale_input")
			&& self.consume_live_stream_stale_grace_if_current();
		#[cfg(not(target_os = "macos"))]
		let consumed_live_stream_stale_grace = false;
		let allow_gate_bypass = allow_stale_input || consumed_live_stream_stale_grace;
		let motion_rows_hint = self.scroll_capture_commit_motion_rows_hint_at(observation_at);

		if !allow_gate_bypass && prior_block_reason.is_some() {
			return Some(Ok(ScrollObserveOutcome::NoChange));
		}

		let result = {
			let Some(session) = self.scroll_capture.session.as_mut() else {
				self.scroll_capture_set_error("Scroll capture session is unavailable.");

				return None;
			};

			session.observe_downward_sample_with_motion_hint_and_burst(
				frame,
				motion_rows_hint,
				allow_post_stall_burst_search,
			)
		};

		if let Ok(outcome) = &result {
			self.consume_scroll_capture_downward_motion_rows_for_outcome(outcome);
		}

		Some(result)
	}

	pub(super) fn scroll_capture_commit_motion_rows_hint_at(
		&self,
		observation_at: Instant,
	) -> Option<u32> {
		if self.scroll_capture.input_direction != Some(ScrollDirection::Down) {
			return None;
		}

		let input_direction_at = self.scroll_capture.input_direction_at?;

		if !self.scroll_capture.input_gesture_active
			&& observation_at.saturating_duration_since(input_direction_at)
				> SCROLL_CAPTURE_INPUT_FRESHNESS
		{
			return None;
		}
		if !self.scroll_capture.downward_motion_rows_pending.is_finite()
			|| self.scroll_capture.downward_motion_rows_pending <= 0.0
		{
			return None;
		}

		Some(self.scroll_capture.downward_motion_rows_pending.ceil() as u32)
	}

	pub(super) fn scroll_capture_set_error(&mut self, message: impl Into<String>) {
		let message = message.into();

		tracing::warn!(
			op = "scroll_capture.error",
			error = %message,
			"Scroll capture paused on error."
		);

		if let Some(trace_recorder) = self.scroll_capture.trace_recorder.as_mut() {
			trace_recorder.record_error(&message);
		}

		self.scroll_capture.paused = true;

		self.state.set_error(message);
		self.request_redraw_all();
	}

	pub(super) fn handle_scroll_capture_key_event(
		&mut self,
		event: &OverlayKeyboardInputEvent,
	) -> OverlayControl {
		match &event.logical_key {
			Key::Named(NamedKey::Escape) => self.cancel_overlay("scroll_capture_escape_key"),
			Key::Named(NamedKey::Space) => {
				self.begin_png_action(PngAction::Copy);

				OverlayControl::Continue
			},
			Key::Character(key_text)
				if key_text.as_str().eq_ignore_ascii_case("s")
					&& self.is_save_shortcut_pressed() =>
			{
				self.begin_png_action(PngAction::Save);

				OverlayControl::Continue
			},
			Key::Character(key_text) if key_text.as_str().eq_ignore_ascii_case("u") => {
				self.undo_scroll_capture_append();

				OverlayControl::Continue
			},
			Key::Character(key_text) if key_text.as_str().eq_ignore_ascii_case("p") => {
				self.toggle_scroll_capture_paused();

				OverlayControl::Continue
			},
			_ => OverlayControl::Continue,
		}
	}

	pub(super) fn scroll_capture_selection_is_ready(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor.is_some()
			&& self.state.frozen_capture_rect.is_some()
			&& self.frozen_capture_source == FrozenCaptureSource::DragRegion
			&& self.frozen_final_capture_ready()
	}

	pub(super) fn scroll_capture_is_available(&self) -> bool {
		if !self.scroll_capture_selection_is_ready() {
			return false;
		}

		#[cfg(target_os = "macos")]
		{
			true
		}
		#[cfg(not(target_os = "macos"))]
		{
			false
		}
	}

	pub(super) fn toolbar_scroll_capture_slot_available(&self) -> bool {
		if self.scroll_capture.active {
			return true;
		}

		#[cfg(target_os = "macos")]
		{
			matches!(self.state.mode, OverlayMode::Frozen)
				&& self.state.monitor.is_some()
				&& self.state.frozen_capture_rect.is_some()
				&& self.frozen_capture_source == FrozenCaptureSource::DragRegion
		}

		#[cfg(not(target_os = "macos"))]
		{
			false
		}
	}

	#[cfg(target_os = "macos")]
	fn try_prepare_scroll_capture_start(
		&mut self,
	) -> Option<(MonitorRect, RectPoints, RectPoints, RgbaImage)> {
		if !self.scroll_capture_selection_is_ready() {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "selection_not_ready",
				frozen_capture_source = ?self.frozen_capture_source,
				state_mode = ?self.state.mode,
				"Skipped starting scroll capture because the current frozen selection was not eligible."
			);

			self.state
				.set_error(String::from("Scroll capture requires a dragged region selection."));
			self.request_redraw_all();

			return None;
		}

		let Some(monitor) = self.state.monitor else {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "missing_monitor",
				"Skipped starting scroll capture because the frozen monitor was unavailable."
			);

			return None;
		};
		let Some(capture_rect_points) = self.state.frozen_capture_rect else {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "missing_capture_rect",
				monitor_id = monitor.id,
				"Skipped starting scroll capture because the frozen capture rect was unavailable."
			);

			return None;
		};
		let capture_rect_pixels = monitor.local_rect_to_pixels(capture_rect_points);
		let Some(base_frame) =
			self.cropped_monitor_frozen_region_image(monitor, capture_rect_pixels)
		else {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "base_frame_unavailable",
				monitor_id = monitor.id,
				capture_rect_points = ?capture_rect_points,
				capture_rect_pixels = ?capture_rect_pixels,
				"Skipped starting scroll capture because the selected frozen region could not be read."
			);

			self.state
				.set_error(String::from("Scroll capture could not read the selected region."));
			self.request_redraw_all();

			return None;
		};

		Some((monitor, capture_rect_points, capture_rect_pixels, base_frame))
	}

	#[cfg(target_os = "macos")]
	fn build_scroll_capture_state(
		&self,
		monitor: MonitorRect,
		capture_rect_points: RectPoints,
		capture_rect_pixels: RectPoints,
		base_frame: RgbaImage,
	) -> Result<ScrollCaptureState> {
		let use_worker_sampling = self.should_use_scroll_capture_worker_sampling();
		let trace_recorder = ScrollCaptureTraceRecorder::from_env(
			monitor,
			capture_rect_pixels,
			SCROLL_CAPTURE_PREVIEW_WIDTH_PX,
			&base_frame,
		);
		let preview_latest_frame = Some(base_frame.clone());
		let session = ScrollSession::new(base_frame, SCROLL_CAPTURE_PREVIEW_WIDTH_PX)?;
		let preview_committed_image = Some(session.preview_image().clone());
		let preview_display_image = preview_committed_image.clone();

		Ok(ScrollCaptureState {
			active: true,
			paused: false,
			monitor: Some(monitor),
			#[cfg(target_os = "macos")]
			capture_rect_points: Some(capture_rect_points),
			capture_rect_pixels: Some(capture_rect_pixels),
			input_direction: None,
			input_direction_at: None,
			input_gesture_active: false,
			downward_motion_rows_pending: 0.0,
			#[cfg(target_os = "macos")]
			overlay_mouse_passthrough_active: false,
			#[cfg(target_os = "macos")]
			overlay_mouse_passthrough_persistent: false,
			#[cfg(target_os = "macos")]
			overlay_mouse_passthrough_until: None,
			#[cfg(target_os = "macos")]
			external_scroll_input_drain_reader: self
				.scroll_capture
				.external_scroll_input_drain_reader
				.clone(),
			last_external_scroll_input_seq: 0,
			#[cfg(target_os = "macos")]
			pixel_delta_residual: MacOSScrollPixelResidual::default(),
			#[cfg(target_os = "macos")]
			live_stream: (!use_worker_sampling).then(|| {
				MacLiveFrameStream::with_scroll_capture_region_and_waker(
					self.config.self_capture_exception_window_ids.clone(),
					capture_rect_points,
					capture_rect_pixels,
					self.scroll_frame_waker.clone(),
				)
			}),
			#[cfg(target_os = "macos")]
			live_stream_backlog: VecDeque::new(),
			last_stream_frame_seq: 0,
			#[cfg(target_os = "macos")]
			last_stream_frame_fingerprint: None,
			#[cfg(target_os = "macos")]
			consecutive_identical_stream_frames: 0,
			#[cfg(target_os = "macos")]
			last_consumed_stream_frame_captured_at: None,
			#[cfg(target_os = "macos")]
			last_stream_event_at: None,
			#[cfg(target_os = "macos")]
			last_stream_poll_at: None,
			#[cfg(target_os = "macos")]
			last_duplicate_stream_refresh_at: None,
			pending_post_stall_burst_after_seq: None,
			#[cfg(target_os = "macos")]
			live_stream_stale_grace: None,
			next_sample_at: Some(Instant::now() + SCROLL_CAPTURE_SAMPLE_INTERVAL),
			next_request_id: 0,
			inflight_request_id: None,
			#[cfg(target_os = "macos")]
			inflight_request_observation: None,
			#[cfg(all(test, target_os = "macos"))]
			force_worker_sampling_in_tests: false,
			session: Some(session),
			preview_committed_image,
			preview_latest_frame,
			preview_display_image,
			retained_overlay_preview_image: None,
			retained_overlay_preview_motion_rows_hint: None,
			last_overlay_preview_motion_rows_hint: None,
			last_overlay_preview_provisional_motion_rows_hint: None,
			last_overlay_preview_existing_candidate_height: None,
			last_overlay_preview_existing_candidate_motion_rows_hint: None,
			last_overlay_preview_ledger_candidate_height: None,
			last_overlay_preview_ledger_candidate_motion_rows_hint: None,
			last_overlay_preview_retained_candidate_height: None,
			last_overlay_preview_retained_candidate_motion_rows_hint: None,
			last_overlay_preview_retained_hint_matches_motion_rows: false,
			last_overlay_preview_fresh_latest_frame_can_drive: false,
			last_overlay_preview_strong_unresolved_registration: false,
			last_overlay_preview_latest_frame_present: false,
			last_overlay_preview_used_provisional: false,
			trace_recorder,
		})
	}

	pub(super) fn start_scroll_capture(&mut self) -> OverlayControl {
		if self.scroll_capture.active {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "already_active",
				"Skipped starting scroll capture because a session is already active."
			);

			return OverlayControl::Continue;
		}

		#[cfg(not(target_os = "macos"))]
		{
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "unsupported_platform",
				"Skipped starting scroll capture because the current platform is unsupported."
			);

			OverlayControl::Continue
		}
		#[cfg(target_os = "macos")]
		{
			let Some((monitor, capture_rect_points, capture_rect_pixels, base_frame)) =
				self.try_prepare_scroll_capture_start()
			else {
				return OverlayControl::Continue;
			};

			if let Some(guard) = self.scroll_capture_start_guard.clone() {
				match guard() {
					Ok(true) => {},
					Ok(false) => return OverlayControl::Continue,
					Err(err) => {
						self.state.set_error(format!("{err:#}"));
						self.request_redraw_all();

						return OverlayControl::Continue;
					},
				}
			}
			if let Some(hook) = self.scroll_capture_starting_hook.clone()
				&& let Err(err) = hook()
			{
				self.state.set_error(format!("{err:#}"));
				self.request_redraw_all();

				return OverlayControl::Continue;
			}

			let base_frame_dimensions = base_frame.dimensions();

			self.scroll_capture = match self.build_scroll_capture_state(
				monitor,
				capture_rect_points,
				capture_rect_pixels,
				base_frame,
			) {
				Ok(scroll_capture) => scroll_capture,
				Err(err) => {
					self.state.set_error(format!("{err:#}"));
					self.request_redraw_all();

					return OverlayControl::Continue;
				},
			};

			if let Some(hook) = self.scroll_capture_started_hook.clone() {
				hook();
			}
			if let Some(trace_recorder) = self.scroll_capture.trace_recorder.as_ref() {
				tracing::info!(
					op = "scroll_capture.trace_recording_enabled",
					manifest_path = %trace_recorder.manifest_path().display(),
					"Enabled scroll-capture live trace recording for this session."
				);
			}

			tracing::info!(
				op = "scroll_capture.start",
				frozen_capture_source = ?self.frozen_capture_source,
				monitor_id = monitor.id,
				monitor_origin = ?monitor.origin,
				monitor_size_points = ?(monitor.width, monitor.height),
				monitor_scale_factor = monitor.scale_factor(),
				capture_rect_points = ?capture_rect_points,
				capture_rect_pixels = ?capture_rect_pixels,
				base_frame_px = ?base_frame_dimensions,
				"Entered scroll-capture mode."
			);

			self.request_aux_window_creation_if_needed();
			self.sync_frozen_toolbar_state();
			self.refresh_scroll_preview_committed_image();
			self.refresh_scroll_preview_display_image();
			self.sync_scroll_preview_segments();
			self.position_scroll_preview_window(monitor);
			self.update_scroll_toolbar_default_position(monitor);
			self.set_scroll_overlay_mouse_passthrough_persistent(true, "scroll_capture_started");
			self.focus_scroll_keyboard_window();
			self.maybe_apply_pending_startup_aux_live_stream_filter_upgrade(monitor);

			if let Some(preview) = self.scroll_preview_window.as_ref() {
				preview.window.set_visible(true);
				preview.window.request_redraw();
			}
			if let (Some(monitor), Some(live_stream)) =
				(self.scroll_capture.monitor, self.scroll_capture.live_stream.as_ref())
			{
				live_stream.prime_monitor_nonblocking(monitor);
			}

			self.request_redraw_for_monitor(monitor);

			OverlayControl::Continue
		}
	}

	pub(super) fn toggle_scroll_capture_paused(&mut self) {
		if !self.scroll_capture.active {
			return;
		}

		self.scroll_capture.paused = !self.scroll_capture.paused;

		#[cfg(target_os = "macos")]
		if self.scroll_capture.paused {
			self.set_scroll_overlay_mouse_passthrough_persistent(false, "paused");
		}
		if !self.scroll_capture.paused {
			#[cfg(target_os = "macos")]
			{
				self.set_scroll_overlay_mouse_passthrough_persistent(true, "resumed");

				if let (Some(monitor), Some(live_stream)) =
					(self.scroll_capture.monitor, self.scroll_capture.live_stream.as_ref())
				{
					live_stream.prime_monitor_nonblocking(monitor);
				}
			}
			#[cfg(not(target_os = "macos"))]
			{
				self.scroll_capture.next_sample_at =
					Some(Instant::now() + SCROLL_CAPTURE_SAMPLE_INTERVAL);
			}
		}

		self.request_redraw_scroll_preview_window();
	}

	pub(super) fn prepare_active_scroll_capture_output(&mut self) {
		if !self.scroll_capture.active {
			return;
		}

		self.maybe_tick_scroll_capture();
		self.refresh_scroll_preview_committed_image();
		self.refresh_scroll_preview_display_image();
		self.sync_scroll_preview_segments();
	}

	pub(super) fn undo_scroll_capture_append(&mut self) {
		if !self.scroll_capture.active {
			return;
		}

		let Some(session) = self.scroll_capture.session.as_mut() else {
			return;
		};

		if !session.undo_last_append() {
			return;
		}

		self.refresh_scroll_preview_committed_image();
		self.clear_scroll_capture_inflight_request();

		#[cfg(target_os = "macos")]
		{
			if let (Some(monitor), Some(live_stream)) =
				(self.scroll_capture.monitor, self.scroll_capture.live_stream.as_ref())
			{
				live_stream.prime_monitor_nonblocking(monitor);
			}
		}
		#[cfg(not(target_os = "macos"))]
		{
			self.scroll_capture.next_sample_at =
				Some(Instant::now() + SCROLL_CAPTURE_SAMPLE_INTERVAL);
		}

		self.refresh_scroll_preview_display_image();
		self.sync_scroll_preview_segments();
	}

	#[cfg(target_os = "macos")]
	fn focus_scroll_keyboard_window(&mut self) {
		super::macos_activate_app();

		let _ = self.sync_native_capture_shells();
	}

	pub(super) fn update_scroll_toolbar_default_position(&mut self, monitor: MonitorRect) {
		if !self.scroll_capture.active || self.toolbar_state.dragging {
			return;
		}

		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let preview_rect = self.scroll_preview_local_rect(monitor);
		let toolbar_primary_size = WindowRenderer::frozen_toolbar_primary_size(&self.toolbar_state);
		let toolbar_window_size = self.toolbar_positioning_size();
		let toolbar_pos = WindowRenderer::frozen_toolbar_default_window_pos(
			screen_rect,
			preview_rect,
			toolbar_primary_size,
			toolbar_window_size,
			self.config.toolbar_placement,
		);

		self.toolbar_state.default_slot_position = Some(toolbar_pos);
		self.toolbar_state.floating_position = Some(toolbar_pos);

		let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);
	}

	pub(super) fn finalize_scroll_capture_for_exit(&mut self) {
		if self.scroll_capture.active {
			self.maybe_tick_scroll_capture();
			self.refresh_scroll_preview_committed_image();
			self.refresh_scroll_preview_display_image();
			self.sync_scroll_preview_segments();
		}

		let scroll_capture_final_snapshot = self.scroll_capture_trace_snapshot_at(Instant::now());
		let final_preview_image = self.current_scroll_preview_render_image();

		if let (Some(trace_recorder), Some(session)) =
			(self.scroll_capture.trace_recorder.as_mut(), self.scroll_capture.session.as_ref())
		{
			let final_preview_image =
				final_preview_image.unwrap_or_else(|| session.preview_image().clone());

			trace_recorder.finalize_session(
				session,
				&final_preview_image,
				scroll_capture_final_snapshot,
			);
		}
	}
}
