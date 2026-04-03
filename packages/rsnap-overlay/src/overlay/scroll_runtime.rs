#[cfg(target_os = "macos")]
use std::time::Duration;
use std::time::Instant;

use color_eyre::Result;
use image::RgbaImage;

#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::MacLiveFrameStream;
#[cfg(target_os = "macos")]
use crate::overlay::SCROLL_CAPTURE_DUPLICATE_WORKER_FRAME_RETRY_INTERVAL;
use crate::overlay::SCROLL_CAPTURE_SAMPLE_INTERVAL;
#[cfg(target_os = "macos")]
use crate::overlay::ScrollCaptureTraceInputRecord;
#[cfg(target_os = "macos")]
use crate::overlay::session_state::ScrollCaptureLiveFrame;
#[cfg(target_os = "macos")]
use crate::overlay::{
	LiveStreamStaleGrace, SCROLL_CAPTURE_DUPLICATE_STREAM_REFRESH_INTERVAL,
	SCROLL_CAPTURE_DUPLICATE_STREAM_STALL_THRESHOLD, SCROLL_CAPTURE_INPUT_FRESHNESS,
	SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
	SCROLL_CAPTURE_STREAM_EVENT_FALLBACK_POLL_INTERVAL, SCROLL_CAPTURE_STREAM_POLL_INTERVAL,
};
use crate::overlay::{MonitorRect, RectPoints};
use crate::overlay::{
	OverlayControl, OverlaySession, ScrollCaptureFrameSource, ScrollCaptureTraceFrameRecord,
	ScrollObserveOutcome, ScrollSession,
};
use crate::scroll_capture::ScrollDirection;
#[cfg(target_os = "macos")]
use crate::scroll_capture::{self};
use crate::worker::WorkerRequestSendError;

impl OverlaySession {
	#[cfg(target_os = "macos")]
	pub(super) fn should_use_scroll_capture_worker_sampling(&self) -> bool {
		if !cfg!(test) {
			return true;
		}

		#[cfg(test)]
		{
			return self.scroll_capture.force_worker_sampling_in_tests;
		}

		#[allow(unreachable_code)]
		false
	}

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

			if self.scroll_capture.live_stream.is_some()
				&& self.scroll_capture.last_stream_poll_at.is_none_or(|last| {
					now.saturating_duration_since(last) >= SCROLL_CAPTURE_STREAM_POLL_INTERVAL
				}) {
				self.scroll_capture.last_stream_poll_at = Some(now);

				let _ = self.try_consume_scroll_stream_frame();
			}
		}

		#[cfg(not(target_os = "macos"))]
		{
			self.request_scroll_capture_worker_sample_at(Instant::now());
		}
	}

	fn request_scroll_capture_worker_sample_at(&mut self, now: Instant) {
		if self.scroll_capture.inflight_request_id.is_some() {
			return;
		}

		let Some(next_sample_at) = self.scroll_capture.next_sample_at else {
			self.scroll_capture.next_sample_at = Some(now + SCROLL_CAPTURE_SAMPLE_INTERVAL);

			return;
		};

		if now < next_sample_at {
			return;
		}

		let Some(monitor) = self.scroll_capture.monitor else {
			self.scroll_capture_set_error("Scroll capture lost its monitor.");

			return;
		};
		let Some(capture_rect) = self.scroll_capture.capture_rect_pixels else {
			self.scroll_capture_set_error("Scroll capture lost its region.");

			return;
		};
		let Some(worker) = self.worker.as_ref() else {
			self.scroll_capture_set_error("Scroll capture worker is unavailable.");

			return;
		};
		let request_id = self.scroll_capture.next_request_id.wrapping_add(1);

		match worker.request_capture_monitor_region(monitor, capture_rect, request_id) {
			Ok(()) => {
				self.scroll_capture.next_request_id = request_id;
				self.scroll_capture.inflight_request_id = Some(request_id);
				#[cfg(target_os = "macos")]
				{
					self.scroll_capture.inflight_request_observation =
						Some(crate::overlay::session_state::InflightScrollCaptureObservation {
							was_observable: self
								.scroll_capture_observation_block_reason_at(now)
								.is_none(),
							external_input_seq: self.scroll_capture.last_external_scroll_input_seq,
							input_direction: self.scroll_capture.input_direction,
						});
				}
				self.scroll_capture.next_sample_at = Some(now + SCROLL_CAPTURE_SAMPLE_INTERVAL);
			},
			Err(WorkerRequestSendError::Full) => {
				self.scroll_capture.next_sample_at =
					Some(now + SCROLL_CAPTURE_SAMPLE_INTERVAL.saturating_mul(2));
			},
			Err(WorkerRequestSendError::Disconnected) => {
				self.scroll_capture_set_error("Scroll capture worker disconnected.");
			},
		}
	}

	#[cfg(target_os = "macos")]
	fn schedule_immediate_scroll_capture_worker_retry_if_fresh_downward_input(
		&mut self,
		now: Instant,
		why: &'static str,
	) {
		let fresh_downward_input = self.scroll_capture.input_direction
			== Some(ScrollDirection::Down)
			&& self.scroll_capture.input_direction_at.is_some_and(|input_direction_at| {
				now.saturating_duration_since(input_direction_at) <= SCROLL_CAPTURE_INPUT_FRESHNESS
			});

		if !fresh_downward_input {
			return;
		}

		self.scroll_capture.next_sample_at = Some(now);

		tracing::info!(
			op = "scroll_capture.worker_retry_scheduled_immediately",
			reason = why,
			last_external_scroll_input_seq = self.scroll_capture.last_external_scroll_input_seq,
			downward_motion_rows_pending = self.scroll_capture.downward_motion_rows_pending,
			"Scheduled an immediate worker retry because fresh downward input was still active."
		);
	}

	#[cfg(target_os = "macos")]
	fn schedule_backoff_scroll_capture_worker_retry_if_fresh_downward_input(
		&mut self,
		now: Instant,
		why: &'static str,
		delay: Duration,
	) {
		let fresh_downward_input = self.scroll_capture.input_direction
			== Some(ScrollDirection::Down)
			&& self.scroll_capture.input_direction_at.is_some_and(|input_direction_at| {
				now.saturating_duration_since(input_direction_at) <= SCROLL_CAPTURE_INPUT_FRESHNESS
			});

		if !fresh_downward_input {
			return;
		}

		self.scroll_capture.next_sample_at = Some(now + delay);

		tracing::info!(
			op = "scroll_capture.worker_retry_scheduled_with_backoff",
			reason = why,
			delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
			last_external_scroll_input_seq = self.scroll_capture.last_external_scroll_input_seq,
			downward_motion_rows_pending = self.scroll_capture.downward_motion_rows_pending,
			"Scheduled a delayed worker retry because fresh downward input was still active but the latest worker frame repeated the committed content."
		);
	}

	#[cfg(target_os = "macos")]
	pub(super) fn try_consume_scroll_stream_frame(&mut self) -> bool {
		let Some(monitor) = self.scroll_capture.monitor else {
			self.scroll_capture_set_error("Scroll capture lost its monitor.");

			return true;
		};
		let Some(capture_rect) = self.scroll_capture.capture_rect_pixels else {
			self.scroll_capture_set_error("Scroll capture lost its region.");

			return true;
		};
		let query_started_at = Instant::now();
		let force_refresh = self.scroll_capture_should_force_stream_refresh_at(query_started_at);
		let allow_stale_refresh =
			self.scroll_capture_should_schedule_stale_stream_refresh_at(query_started_at);
		let fresh_downward_backlog =
			self.scroll_capture_has_fresh_downward_backlog_at(query_started_at);
		let Some(live_stream) = self.scroll_capture.live_stream.as_mut() else {
			return false;
		};
		let last_frame_seq = self.scroll_capture.last_stream_frame_seq;
		let Some(frames) = live_stream.ordered_rgba_regions_after_seq_nonblocking(
			monitor,
			capture_rect,
			last_frame_seq,
		) else {
			let (query_ms, refresh_scheduled) = Self::query_empty_scroll_stream_result(
				live_stream,
				monitor,
				last_frame_seq,
				query_started_at,
				allow_stale_refresh,
				force_refresh,
			);
			let _ = live_stream;

			if refresh_scheduled && fresh_downward_backlog {
				self.scroll_capture.pending_post_stall_burst_after_seq = Some(last_frame_seq);
			}

			self.log_empty_scroll_stream_query(
				last_frame_seq,
				query_ms,
				refresh_scheduled,
				allow_stale_refresh,
				force_refresh,
			);

			return false;
		};
		let Some(newest_frame_seq) = frames.last().map(|frame| frame.frame_seq) else {
			let (query_ms, refresh_scheduled) = Self::query_empty_scroll_stream_result(
				live_stream,
				monitor,
				last_frame_seq,
				query_started_at,
				allow_stale_refresh,
				force_refresh,
			);
			let _ = live_stream;

			if refresh_scheduled && fresh_downward_backlog {
				self.scroll_capture.pending_post_stall_burst_after_seq = Some(last_frame_seq);
			}

			self.log_empty_scroll_stream_query(
				last_frame_seq,
				query_ms,
				refresh_scheduled,
				allow_stale_refresh,
				force_refresh,
			);

			return false;
		};
		let query_ms = u64::try_from(query_started_at.elapsed().as_millis()).unwrap_or(u64::MAX);

		if query_ms >= 16 {
			tracing::warn!(
				op = "scroll_capture.stream_frame_query_slow",
				last_frame_seq,
				query_ms,
				result = "ready",
				frame_seq = newest_frame_seq,
				frame_count = frames.len(),
				"Slow nonblocking live-stream query delayed scroll-capture observation."
			);
		}

		tracing::info!(
			op = "scroll_capture.stream_frame_ready",
			prior_frame_seq = last_frame_seq,
			frame_seq = newest_frame_seq,
			frame_gap = newest_frame_seq.saturating_sub(last_frame_seq),
			frame_count = frames.len(),
			query_ms,
			"Pulled live-stream frame for scroll-capture observation."
		);

		for frame in frames {
			self.push_scroll_capture_live_frame(ScrollCaptureLiveFrame {
				frame_seq: frame.frame_seq,
				captured_at: frame.captured_at,
				image: frame.image,
			});
		}

		self.consume_scroll_capture_backlog(usize::MAX);

		true
	}

	#[cfg(target_os = "macos")]
	#[allow(clippy::too_many_arguments)]
	fn query_empty_scroll_stream_result(
		live_stream: &mut MacLiveFrameStream,
		monitor: MonitorRect,
		last_frame_seq: u64,
		query_started_at: Instant,
		allow_stale_refresh: bool,
		force_refresh: bool,
	) -> (u64, bool) {
		let query_ms = u64::try_from(query_started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
		let refresh_scheduled = if allow_stale_refresh {
			live_stream.refresh_monitor_nonblocking_if_stale(monitor, last_frame_seq, force_refresh)
		} else {
			false
		};

		(query_ms, refresh_scheduled)
	}

	#[cfg(target_os = "macos")]
	fn log_empty_scroll_stream_query(
		&self,
		last_frame_seq: u64,
		query_ms: u64,
		refresh_scheduled: bool,
		allow_stale_refresh: bool,
		force_refresh: bool,
	) {
		if query_ms >= 16 {
			tracing::warn!(
				op = "scroll_capture.stream_frame_query_slow",
				last_frame_seq,
				query_ms,
				refresh_scheduled,
				stale_refresh_suppressed = !allow_stale_refresh,
				force_refresh,
				result = "empty",
				"Slow nonblocking live-stream query delayed scroll-capture observation."
			);
		}

		tracing::info!(
			op = "scroll_capture.stream_frame_empty",
			last_frame_seq,
			query_ms,
			refresh_scheduled,
			stale_refresh_suppressed = !allow_stale_refresh,
			force_refresh,
			"Did not receive a newer live-stream frame for scroll-capture observation."
		);
	}

	#[cfg(target_os = "macos")]
	/// Consumes any queued macOS live-stream frames for scroll capture.
	pub fn handle_scroll_stream_frame_ready(&mut self) -> OverlayControl {
		if !cfg!(test) {
			return OverlayControl::Continue;
		}
		if self.scroll_capture.active && !self.scroll_capture.paused {
			if self.should_use_scroll_capture_worker_sampling() {
				return OverlayControl::Continue;
			}

			let _ = self.try_consume_scroll_stream_frame();

			self.consume_scroll_capture_backlog(usize::MAX);
		}

		OverlayControl::Continue
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

	#[cfg(target_os = "macos")]
	pub(super) fn note_scroll_capture_live_stream_frame_activity(
		&mut self,
		frame: &ScrollCaptureLiveFrame,
	) -> bool {
		let fingerprint = scroll_capture::scroll_capture_fingerprint(&frame.image);
		let is_distinct =
			self.scroll_capture.last_stream_frame_fingerprint.as_ref().is_none_or(|previous| {
				scroll_capture::scroll_capture_fingerprint_delta(previous, &fingerprint) > 0
			});

		self.scroll_capture.last_stream_frame_fingerprint = Some(fingerprint);

		if is_distinct {
			self.scroll_capture.last_stream_event_at = Some(frame.captured_at);
			self.scroll_capture.consecutive_identical_stream_frames = 0;
			self.scroll_capture.last_duplicate_stream_refresh_at = None;
		} else {
			self.scroll_capture.consecutive_identical_stream_frames =
				self.scroll_capture.consecutive_identical_stream_frames.saturating_add(1);
		}

		is_distinct
	}

	#[cfg(target_os = "macos")]
	pub(super) fn maybe_schedule_duplicate_stream_refresh(
		&mut self,
		frame_seq: u64,
		observation_at: Instant,
	) {
		if self.scroll_capture.consecutive_identical_stream_frames
			< SCROLL_CAPTURE_DUPLICATE_STREAM_STALL_THRESHOLD
		{
			return;
		}
		if !self.scroll_capture_has_fresh_downward_backlog_at(observation_at) {
			return;
		}
		if self.scroll_capture.last_duplicate_stream_refresh_at.is_some_and(|last| {
			observation_at.saturating_duration_since(last)
				< SCROLL_CAPTURE_DUPLICATE_STREAM_REFRESH_INTERVAL
		}) {
			return;
		}

		let Some(monitor) = self.scroll_capture.monitor else {
			return;
		};
		let Some(live_stream) = self.scroll_capture.live_stream.as_ref() else {
			return;
		};
		let refresh_scheduled =
			live_stream.refresh_monitor_nonblocking_if_stale(monitor, frame_seq, true);

		if !refresh_scheduled {
			return;
		}

		self.scroll_capture.last_duplicate_stream_refresh_at = Some(observation_at);
		self.scroll_capture.pending_post_stall_burst_after_seq = Some(frame_seq);

		tracing::info!(
			op = "scroll_capture.duplicate_frame_refresh_scheduled",
			frame_seq,
			identical_streak = self.scroll_capture.consecutive_identical_stream_frames,
			downward_motion_rows_pending = self.scroll_capture.downward_motion_rows_pending,
			"Scheduled a forced live-stream refresh after repeated identical frames during fresh downward backlog."
		);
	}

	#[cfg(target_os = "macos")]
	fn push_scroll_capture_live_frame(&mut self, frame: ScrollCaptureLiveFrame) {
		let backlog = &mut self.scroll_capture.live_stream_backlog;

		if backlog.len() >= super::SCROLL_CAPTURE_STREAM_BACKLOG_MAX_FRAMES {
			backlog.pop_front();
		}

		backlog.push_back(frame);
	}

	#[cfg(all(test, target_os = "macos"))]
	pub(super) fn test_push_scroll_capture_live_frame(&mut self, frame: ScrollCaptureLiveFrame) {
		self.push_scroll_capture_live_frame(frame);
	}

	#[cfg(target_os = "macos")]
	fn consume_scroll_capture_backlog(&mut self, max_frames: usize) {
		let mut consumed = 0;

		while consumed < max_frames {
			let Some(frame) = self.scroll_capture.live_stream_backlog.pop_front() else {
				break;
			};

			self.drain_external_scroll_input_events_through(frame.captured_at);

			let arm_time_gap_burst =
				self.scroll_capture_should_arm_post_stall_burst_for_time_gap_at(frame.captured_at);

			if arm_time_gap_burst {
				self.scroll_capture.pending_post_stall_burst_after_seq =
					Some(frame.frame_seq.saturating_sub(1));

				tracing::info!(
					op = "scroll_capture.post_stall_burst_search_armed_for_time_gap",
					frame_seq = frame.frame_seq,
					downward_motion_rows_pending = self.scroll_capture.downward_motion_rows_pending,
					last_consumed_captured_at = ?self.scroll_capture.last_consumed_stream_frame_captured_at,
					current_captured_at = ?frame.captured_at,
					"Armed a burst registration window because the next live-stream frame arrived after a large capture-time gap while fresh downward backlog remained."
				);
			}

			let _is_distinct = self.note_scroll_capture_live_stream_frame_activity(&frame);

			self.scroll_capture.last_stream_frame_seq = frame.frame_seq;

			let _ = self.handle_scroll_capture_frame(
				frame.image,
				ScrollCaptureFrameSource::LiveStream { frame_seq: frame.frame_seq },
				false,
				frame.captured_at,
			);

			self.scroll_capture.last_consumed_stream_frame_captured_at = Some(frame.captured_at);

			self.maybe_schedule_duplicate_stream_refresh(frame.frame_seq, frame.captured_at);

			consumed += 1;
		}
	}

	#[cfg(all(test, target_os = "macos"))]
	pub(super) fn test_consume_scroll_capture_backlog(&mut self, max_frames: usize) {
		self.consume_scroll_capture_backlog(max_frames);
	}

	pub(super) fn replay_recorded_live_stream_frame(
		&mut self,
		frame: RgbaImage,
		frame_seq: u64,
		observed_at: Instant,
		allow_stale_input: bool,
	) -> Option<Result<ScrollObserveOutcome>> {
		#[cfg(target_os = "macos")]
		if self.scroll_capture_should_arm_post_stall_burst_for_time_gap_at(observed_at) {
			self.scroll_capture.pending_post_stall_burst_after_seq =
				Some(frame_seq.saturating_sub(1));
		}

		#[cfg(target_os = "macos")]
		let frame_for_activity =
			ScrollCaptureLiveFrame { frame_seq, captured_at: observed_at, image: frame.clone() };
		#[cfg(target_os = "macos")]
		let _ = self.note_scroll_capture_live_stream_frame_activity(&frame_for_activity);

		self.scroll_capture.last_stream_frame_seq = frame_seq;

		let outcome = self.handle_scroll_capture_frame(
			frame,
			ScrollCaptureFrameSource::LiveStream { frame_seq },
			allow_stale_input,
			observed_at,
		);

		#[cfg(target_os = "macos")]
		{
			self.scroll_capture.last_consumed_stream_frame_captured_at = Some(observed_at);

			self.maybe_schedule_duplicate_stream_refresh(frame_seq, observed_at);
		}

		outcome
	}

	#[cfg(target_os = "macos")]
	fn poll_scroll_stream_fallback_if_due(&mut self, now: Instant) {
		let should_poll_stream_fallback =
			self.scroll_capture.last_stream_event_at.is_none_or(|last| {
				now.duration_since(last) >= SCROLL_CAPTURE_STREAM_EVENT_FALLBACK_POLL_INTERVAL
			});

		if should_poll_stream_fallback {
			let _ = self.try_consume_scroll_stream_frame();
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
			let Some(session) = self.scroll_capture.session.as_mut() else {
				self.scroll_capture_set_error("Scroll capture session is unavailable.");

				return None;
			};

			session.observe_worker_pairwise_vision_frame(frame)
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

	#[allow(clippy::too_many_lines)]
	fn handle_scroll_capture_frame_outcome(
		&mut self,
		outcome: &Result<ScrollObserveOutcome>,
		source: ScrollCaptureFrameSource,
		frame_px: (u32, u32),
	) {
		match outcome {
			Ok(ScrollObserveOutcome::NoChange) => {
				self.log_scroll_capture_no_change(source, frame_px)
			},
			Ok(ScrollObserveOutcome::PreviewUpdated) => {
				self.log_scroll_capture_preview_updated(source, frame_px);
			},
			Ok(ScrollObserveOutcome::UnsupportedDirection { direction }) => {
				let export_size = self
					.scroll_capture
					.session
					.as_ref()
					.map_or((0, 0), ScrollSession::export_dimensions);

				tracing::info!(
					op = "scroll_capture.unsupported_direction",
					frame_source = source.as_str(),
					worker_request_id = ?source.worker_request_id(),
					direction = ?direction,
					frame_px = ?frame_px,
					export_px = ?export_size,
					"Scroll-capture sample moved in an unsupported direction."
				);
			},
			Ok(ScrollObserveOutcome::Committed { direction, growth_rows }) => {
				self.log_scroll_capture_committed(source, frame_px, *direction, *growth_rows);
			},
			Err(err) => {
				self.scroll_capture_set_error(format!("{err:#}"));
			},
		}
	}

	fn log_scroll_capture_no_change(
		&mut self,
		source: ScrollCaptureFrameSource,
		frame_px: (u32, u32),
	) {
		let last_block_reason =
			self.scroll_capture.session.as_ref().and_then(ScrollSession::last_block_reason);

		tracing::info!(
			op = "scroll_capture.frame_observed",
			frame_source = source.as_str(),
			worker_request_id = ?source.worker_request_id(),
			outcome = "no_change",
			frame_px = ?frame_px,
			input_direction = ?self.scroll_capture.input_direction,
			input_gesture_active = self.scroll_capture.input_gesture_active,
			last_block_reason = ?last_block_reason,
			export_px = ?self.scroll_capture.session.as_ref().map(ScrollSession::export_dimensions),
			"Scroll-capture observed a frame but kept session state unchanged."
		);

		if let Some(request_id) = source.worker_request_id() {
			#[cfg(target_os = "macos")]
			{
				let now = Instant::now();

				match last_block_reason {
					Some("frame_matches_last_committed_frame") => self
						.schedule_backoff_scroll_capture_worker_retry_if_fresh_downward_input(
							now,
							"worker_duplicate_committed_frame",
							SCROLL_CAPTURE_DUPLICATE_WORKER_FRAME_RETRY_INTERVAL,
						),
					_ => self
						.schedule_immediate_scroll_capture_worker_retry_if_fresh_downward_input(
							now,
							"worker_no_change",
						),
				}
			}

			tracing::info!(
				op = "scroll_capture.worker_frame_processed",
				request_id,
				outcome = "no_change",
				frame_px = ?frame_px,
				input_direction = ?self.scroll_capture.input_direction,
				last_block_reason = ?last_block_reason,
				"Worker-fed scroll-capture frame reached the session without changing preview or export state."
			);
		}
	}

	fn log_scroll_capture_preview_updated(
		&self,
		source: ScrollCaptureFrameSource,
		frame_px: (u32, u32),
	) {
		tracing::info!(
			op = "scroll_capture.frame_observed",
			frame_source = source.as_str(),
			worker_request_id = ?source.worker_request_id(),
			outcome = "preview_updated",
			frame_px = ?frame_px,
			input_direction = ?self.scroll_capture.input_direction,
			input_gesture_active = self.scroll_capture.input_gesture_active,
			export_px = ?self.scroll_capture.session.as_ref().map(ScrollSession::export_dimensions),
			preview_px = ?self.scroll_capture_preview_dimensions().map(|[w, h]| (w, h)),
			"Scroll-capture observed a frame and advanced session sampling state without committing stitched growth."
		);

		if let Some(request_id) = source.worker_request_id() {
			tracing::info!(
				op = "scroll_capture.worker_frame_processed",
				request_id,
				outcome = "preview_updated",
				frame_px = ?frame_px,
				input_direction = ?self.scroll_capture.input_direction,
				"Worker-fed scroll-capture frame refreshed preview state without committing stitched growth."
			);
		}
	}

	fn log_scroll_capture_committed(
		&mut self,
		source: ScrollCaptureFrameSource,
		frame_px: (u32, u32),
		direction: ScrollDirection,
		growth_rows: u32,
	) {
		self.refresh_scroll_preview_committed_image();
		self.refresh_scroll_preview_display_image();
		self.sync_scroll_preview_segments();
		self.request_redraw_scroll_preview_window();

		let telemetry = self.scroll_capture.session.as_ref().map(ScrollSession::commit_telemetry);
		let export_size =
			telemetry.as_ref().map_or((0, 0), |telemetry| telemetry.export_dimensions);
		let preview_size =
			telemetry.as_ref().map_or((0, 0), |telemetry| telemetry.preview_dimensions);

		tracing::info!(
			op = "scroll_capture.committed",
			frame_source = source.as_str(),
			worker_request_id = ?source.worker_request_id(),
			direction = ?direction,
			growth_rows,
			frame_px = ?frame_px,
			export_px = ?export_size,
			preview_px = ?preview_size,
			current_viewport_top_y = ?telemetry.as_ref().map(|telemetry| telemetry.current_viewport_top_y),
			growth_commit_count = ?telemetry.as_ref().map(|telemetry| telemetry.growth_commit_count),
			preview_segment_count = ?telemetry.as_ref().map(|telemetry| telemetry.preview_segment_count),
			export_segment_count = ?telemetry.as_ref().map(|telemetry| telemetry.export_segment_count),
			last_commit_decision_source = ?telemetry.as_ref().map(|telemetry| telemetry.last_commit_decision_source),
			last_commit_detected_motion_rows = ?telemetry.as_ref().map(|telemetry| telemetry.last_commit_detected_motion_rows),
			last_commit_effective_motion_rows_hint = ?telemetry.as_ref().map(|telemetry| telemetry.last_commit_effective_motion_rows_hint),
			last_preview_segment_height_px = ?telemetry.as_ref().map(|telemetry| telemetry.last_preview_segment_height_px),
			last_export_segment_height_px = ?telemetry.as_ref().map(|telemetry| telemetry.last_export_segment_height_px),
			preview_export_segments_aligned = ?telemetry.as_ref().map(|telemetry| telemetry.preview_export_segments_aligned),
			"Scroll sample committed stitched growth."
		);
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

	#[cfg(target_os = "macos")]
	pub(super) fn clear_incompatible_live_stream_stale_grace(&mut self) {
		let Some(grace) = self.scroll_capture.live_stream_stale_grace else {
			return;
		};
		let grace_is_current =
			grace.external_input_seq == self.scroll_capture.last_external_scroll_input_seq;
		let grace_is_compatible = self.scroll_capture.input_direction.is_some()
			&& !self.scroll_capture.input_gesture_active;

		if !(grace_is_current && grace_is_compatible) {
			self.scroll_capture.live_stream_stale_grace = None;
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn refresh_live_stream_stale_grace_for_external_input(
		&mut self,
		external_input_seq: u64,
	) {
		self.scroll_capture.live_stream_stale_grace = match (
			self.scroll_capture.input_direction.is_some(),
			self.scroll_capture.input_gesture_active,
		) {
			(true, false) => Some(LiveStreamStaleGrace {
				external_input_seq,
				remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
			}),
			_ => None,
		};
	}

	#[cfg(target_os = "macos")]
	pub(super) fn consume_live_stream_stale_grace_if_current(&mut self) -> bool {
		let Some(grace) = self.scroll_capture.live_stream_stale_grace else {
			return false;
		};

		if grace.external_input_seq != self.scroll_capture.last_external_scroll_input_seq
			|| self.scroll_capture.input_direction.is_none()
			|| self.scroll_capture.input_gesture_active
			|| grace.remaining_stale_frames == 0
		{
			self.scroll_capture.live_stream_stale_grace = None;

			return false;
		}
		if grace.remaining_stale_frames == 1 {
			self.scroll_capture.live_stream_stale_grace = None;
		} else {
			self.scroll_capture.live_stream_stale_grace = Some(LiveStreamStaleGrace {
				remaining_stale_frames: grace.remaining_stale_frames - 1,
				..grace
			});
		}

		true
	}
}
