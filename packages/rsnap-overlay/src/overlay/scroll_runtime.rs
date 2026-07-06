mod live_stream;
mod observation_logging;
mod worker_sampling;

use std::time::Instant;

use color_eyre::Result;
use image::RgbaImage;

#[cfg(target_os = "macos")]
use crate::overlay::ScrollCaptureTraceInputRecord;
use crate::overlay::{MonitorRect, RectPoints};
use crate::overlay::{
	OverlayControl, OverlaySession, ScrollCaptureFrameSource, ScrollCaptureTraceFrameRecord,
	ScrollObserveOutcome,
};

impl OverlaySession {
	pub(super) fn maybe_tick_scroll_capture(&mut self) {
		if !self.scroll_capture.active || self.scroll_capture.paused {
			return;
		}

		#[cfg(target_os = "macos")]
		{
			let now = Instant::now();

			self.sync_scroll_overlay_mouse_passthrough_window(now);
			self.drain_external_scroll_input_events_through(now);

			if self.should_use_scroll_capture_worker_sampling() {
				self.request_scroll_capture_worker_sample_at(now);

				return;
			}

			self.poll_scroll_stream_fallback_if_due(now);
			self.poll_scroll_stream_regular_if_due(now);
		}

		#[cfg(not(target_os = "macos"))]
		{
			self.request_scroll_capture_worker_sample_at(Instant::now());
		}
	}

	#[cfg(target_os = "macos")]
	/// Drains queued external scroll input and opportunistically polls the stream fallback path.
	pub fn handle_scroll_input_ready(&mut self) -> OverlayControl {
		if self.scroll_capture.active && !self.scroll_capture.paused {
			let now = Instant::now();

			self.sync_scroll_overlay_mouse_passthrough_window(now);
			self.drain_external_scroll_input_events_through(now);

			if self.should_use_scroll_capture_worker_sampling() {
				self.request_scroll_capture_worker_sample_at(now);

				return OverlayControl::Continue;
			}

			self.poll_scroll_stream_fallback_if_due(now);
			self.consume_scroll_capture_backlog(usize::MAX);
		}

		OverlayControl::Continue
	}

	/// Drains worker responses that were signaled through the response waker.
	pub fn handle_worker_response_ready(&mut self) -> OverlayControl {
		self.drain_worker_responses()
	}

	#[cfg(target_os = "macos")]
	pub(super) fn drain_external_scroll_input_events_through(&mut self, through: Instant) {
		let Some(reader) = self.scroll_capture.external_scroll_input_drain_reader.clone() else {
			return;
		};

		for (seq, recorded_at, global_x, global_y, delta_y, gesture_active, gesture_ended) in
			reader(self.scroll_capture.last_external_scroll_input_seq, through)
		{
			if seq <= self.scroll_capture.last_external_scroll_input_seq {
				continue;
			}

			let inferred_direction =
				Self::scroll_capture_direction_from_external_input_delta_y(delta_y);
			let input_age_ms =
				u64::try_from(through.saturating_duration_since(recorded_at).as_millis())
					.unwrap_or(u64::MAX);
			let prior_direction = self.scroll_capture.input_direction;
			let prior_gesture_active = self.scroll_capture.input_gesture_active;

			tracing::debug!(
				op = "scroll_capture.replayed_input",
				seq,
				prior_seq = self.scroll_capture.last_external_scroll_input_seq,
				delta_y,
				gesture_active,
				gesture_ended,
				direction = ?inferred_direction,
				input_age_ms,
				prior_direction = ?prior_direction,
				prior_gesture_active,
				"Replayed external scroll input event into scroll capture."
			);

			self.scroll_capture.last_external_scroll_input_seq = seq;

			self.apply_external_scroll_input_delta_y(
				global_x,
				global_y,
				delta_y,
				gesture_active,
				gesture_ended,
				through,
			);

			let snapshot_after = self.scroll_capture_trace_snapshot_at(through);

			if let Some(trace_recorder) = self.scroll_capture.trace_recorder.as_mut() {
				trace_recorder.record_replayed_input(ScrollCaptureTraceInputRecord {
					seq,
					cursor_global: (global_x, global_y),
					delta_y,
					gesture_active,
					gesture_ended,
					recorded_age: through.saturating_duration_since(recorded_at),
					applied_at: through,
					snapshot_after,
				});
			}

			if !self.should_use_scroll_capture_worker_sampling() {
				self.refresh_live_stream_stale_grace_for_external_input(seq);
			}
			if self.scroll_capture.active && !self.scroll_capture.paused {
				self.refresh_scroll_preview_display_image();
				self.sync_scroll_preview_segments();
				self.request_redraw_scroll_preview_window();
			}

			tracing::debug!(
				op = "scroll_capture.replayed_input_result",
				seq,
				recorded_at_ms_behind_pairing = u64::try_from(
					through.saturating_duration_since(recorded_at).as_millis()
				)
				.unwrap_or(u64::MAX),
				paired_at_age_ms = self.scroll_capture_input_age_ms(),
				after_direction = ?self.scroll_capture.input_direction,
				after_gesture_active = self.scroll_capture.input_gesture_active,
				downward_motion_rows_pending = self.scroll_capture.downward_motion_rows_pending,
				"Applied replayed external scroll input event to scroll-capture state."
			);
		}
	}

	pub(super) fn handle_captured_scroll_region(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
		image: RgbaImage,
	) {
		let frame_px = image.dimensions();

		if !self.scroll_capture.active {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "inactive",
				request_id,
				paused = self.scroll_capture.paused,
				frame_px = ?frame_px,
				"Dropped worker-fed scroll-capture frame before observation."
			);

			return;
		}
		if self.scroll_capture.monitor != Some(monitor) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "monitor_mismatch",
				request_id,
				expected_monitor_id = ?self.scroll_capture.monitor.map(|current_monitor| current_monitor.id),
				received_monitor_id = monitor.id,
				frame_px = ?frame_px,
				"Dropped worker-fed scroll-capture frame before observation."
			);

			return;
		}
		if self.scroll_capture.capture_rect_pixels != Some(rect_px) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "rect_mismatch",
				request_id,
				expected_rect_px = ?self.scroll_capture.capture_rect_pixels,
				received_rect_px = ?rect_px,
				frame_px = ?frame_px,
				"Dropped worker-fed scroll-capture frame before observation."
			);

			return;
		}
		if self.scroll_capture.inflight_request_id != Some(request_id) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "inflight_request_mismatch",
				request_id,
				expected_request_id = ?self.scroll_capture.inflight_request_id,
				frame_px = ?frame_px,
				"Dropped worker-fed scroll-capture frame before observation."
			);

			return;
		}

		#[cfg(target_os = "macos")]
		self.drain_external_scroll_input_events_through(Instant::now());

		#[cfg(target_os = "macos")]
		let allow_stale_input_for_request =
			self.allow_worker_frame_with_latched_request_input(request_id);
		#[cfg(target_os = "macos")]
		let request_input_was_superseded = self.worker_frame_request_input_was_superseded(request_id);
		#[cfg(not(target_os = "macos"))]
		let allow_stale_input_for_request = false;
		#[cfg(not(target_os = "macos"))]
		let request_input_was_superseded = false;

		if request_input_was_superseded {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "superseded_input_context",
				request_id,
				frame_px = ?frame_px,
				input_direction = ?self.scroll_capture.input_direction,
				last_external_scroll_input_seq = self.scroll_capture.last_external_scroll_input_seq,
				"Dropped worker-fed scroll-capture frame because newer external input superseded the request context."
			);

			self.clear_scroll_capture_inflight_request();

			return;
		}

		self.clear_scroll_capture_inflight_request();

		let _ = self.handle_scroll_capture_frame(
			image,
			ScrollCaptureFrameSource::Worker { request_id },
			allow_stale_input_for_request,
			Instant::now(),
		);
	}

	pub(super) fn handle_missing_scroll_region(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
	) {
		if !self.scroll_capture.active {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "inactive",
				request_id,
				paused = self.scroll_capture.paused,
				"Dropped worker scroll-capture no-frame notification before observation."
			);

			return;
		}
		if self.scroll_capture.monitor != Some(monitor) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "monitor_mismatch",
				request_id,
				expected_monitor_id = ?self.scroll_capture.monitor.map(|current_monitor| current_monitor.id),
				received_monitor_id = monitor.id,
				"Dropped worker scroll-capture no-frame notification before observation."
			);

			return;
		}
		if self.scroll_capture.capture_rect_pixels != Some(rect_px) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "rect_mismatch",
				request_id,
				expected_rect_px = ?self.scroll_capture.capture_rect_pixels,
				received_rect_px = ?rect_px,
				"Dropped worker scroll-capture no-frame notification before observation."
			);

			return;
		}
		if self.scroll_capture.inflight_request_id != Some(request_id) {
			tracing::info!(
				op = "scroll_capture.worker_frame_dropped",
				reason = "inflight_request_mismatch",
				request_id,
				expected_request_id = ?self.scroll_capture.inflight_request_id,
				"Dropped worker scroll-capture no-frame notification before observation."
			);

			return;
		}

		self.clear_scroll_capture_inflight_request();
		#[cfg(target_os = "macos")]
		self.schedule_immediate_scroll_capture_worker_retry_if_fresh_downward_input(
			Instant::now(),
			"worker_no_new_frame",
		);

		tracing::info!(
			op = "scroll_capture.worker_frame_unavailable",
			request_id,
			reason = "no_new_frame",
			input_direction = ?self.scroll_capture.input_direction,
			"Worker scroll-capture request completed without a fresh frame."
		);
	}

	pub(super) fn handle_scroll_capture_frame(
		&mut self,
		frame: RgbaImage,
		source: ScrollCaptureFrameSource,
		allow_stale_input: bool,
		observation_at: Instant,
	) -> Option<Result<ScrollObserveOutcome>> {
		let trace_frame = self.scroll_capture.trace_recorder.as_ref().map(|_| frame.clone());
		let preview_frame = frame.clone();
		let frame_px = frame.dimensions();
		let prior_block_reason = self.scroll_capture_observation_block_reason_at(observation_at);
		let allow_stale_input = {
			#[cfg(target_os = "macos")]
			{
				allow_stale_input
					|| prior_block_reason == Some("stale_input")
						&& matches!(source, ScrollCaptureFrameSource::LiveStream { .. })
						&& self.consume_live_stream_stale_grace_if_current()
			}

			#[cfg(not(target_os = "macos"))]
			{
				allow_stale_input
			}
		};

		if let Some(reason) = prior_block_reason {
			let input_age_ms = self.scroll_capture_input_age_ms_at(observation_at);

			tracing::info!(
				op = "scroll_capture.observation_prior_state",
				frame_source = source.as_str(),
				worker_request_id = ?source.worker_request_id(),
				reason,
				frame_px = ?frame_px,
				input_direction = ?self.scroll_capture.input_direction,
				input_gesture_active = self.scroll_capture.input_gesture_active,
				input_age_ms = ?input_age_ms,
				allow_stale_input,
				"Observed a scroll-capture frame while input metadata would previously have blocked observation."
			);
		}

		let allow_post_stall_burst_search = match source {
			ScrollCaptureFrameSource::LiveStream { frame_seq } => self
				.scroll_capture_should_allow_post_stall_burst_search_at(frame_seq, observation_at),
			ScrollCaptureFrameSource::Worker { .. } => false,
		};

		if allow_post_stall_burst_search {
			tracing::info!(
				op = "scroll_capture.post_stall_burst_search_enabled",
				frame_source = source.as_str(),
				worker_request_id = ?source.worker_request_id(),
				pending_after_seq = ?self.scroll_capture.pending_post_stall_burst_after_seq,
				downward_motion_rows_pending = self.scroll_capture.downward_motion_rows_pending,
				"Kept a burst registration window enabled after an explicit stalled-refresh episode while fresh downward backlog remained."
			);
		}

		let worker_pairwise_path =
			cfg!(target_os = "macos") && matches!(source, ScrollCaptureFrameSource::Worker { .. });
		let outcome = if worker_pairwise_path {
			if !allow_stale_input && prior_block_reason.is_some() {
				Ok(ScrollObserveOutcome::NoChange)
			} else {
				let Some(session) = self.scroll_capture.session.as_mut() else {
					self.scroll_capture_set_error("Scroll capture session is unavailable.");

					return None;
				};

				session.observe_worker_pairwise_vision_frame(frame)
			}
		} else {
			self.observe_scroll_capture_frame_with_gate(
				frame,
				allow_stale_input,
				observation_at,
				allow_post_stall_burst_search,
			)?
		};

		if worker_pairwise_path && let Ok(outcome) = &outcome {
			self.consume_scroll_capture_downward_motion_rows_for_outcome(outcome);
		}
		if matches!(source, ScrollCaptureFrameSource::LiveStream { .. })
			&& !allow_post_stall_burst_search
		{
			self.scroll_capture.pending_post_stall_burst_after_seq = None;
		}

		self.scroll_capture.preview_latest_frame = Some(preview_frame);

		self.refresh_scroll_preview_display_image();
		self.sync_scroll_preview_segments();
		self.request_redraw_scroll_preview_window();
		self.handle_scroll_capture_frame_outcome(&outcome, source, frame_px);

		let snapshot_after = self.scroll_capture_trace_snapshot_at(observation_at);

		if let (Some(trace_recorder), Some(trace_frame)) =
			(self.scroll_capture.trace_recorder.as_mut(), trace_frame.as_ref())
		{
			trace_recorder.record_frame_observation(ScrollCaptureTraceFrameRecord {
				frame: trace_frame,
				source,
				allow_stale_input,
				prior_block_reason,
				observed_at: observation_at,
				snapshot_after,
				outcome: &outcome,
			});
		}

		Some(outcome)
	}

	pub(super) fn clear_scroll_capture_inflight_request(&mut self) {
		self.scroll_capture.inflight_request_id = None;
		#[cfg(target_os = "macos")]
		{
			self.scroll_capture.inflight_request_observation = None;
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn allow_worker_frame_with_latched_request_input(&self, request_id: u64) -> bool {
		if self.scroll_capture.inflight_request_id != Some(request_id) {
			return false;
		}

		let Some(observation) = self.scroll_capture.inflight_request_observation else {
			return false;
		};

		if !observation.was_observable {
			return false;
		}

		matches!(
			(observation.input_direction, self.scroll_capture.input_direction),
			(Some(request_direction), Some(current_direction))
				if request_direction == current_direction
		)
	}

	#[cfg(target_os = "macos")]
	fn worker_frame_request_input_was_superseded(&self, request_id: u64) -> bool {
		if self.scroll_capture.inflight_request_id != Some(request_id) {
			return false;
		}

		let Some(observation) = self.scroll_capture.inflight_request_observation else {
			return false;
		};

		if !observation.was_observable
			|| observation.external_input_seq == self.scroll_capture.last_external_scroll_input_seq
		{
			return false;
		}

		matches!(
			(observation.input_direction, self.scroll_capture.input_direction),
			(Some(request_direction), Some(current_direction))
				if request_direction != current_direction
		)
	}
}
