use std::time::Instant;

use color_eyre::Result;
use image::RgbaImage;

#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::MacLiveFrameStream;
#[cfg(target_os = "macos")]
use crate::overlay::scroll_capture_timing::{
	SCROLL_CAPTURE_DUPLICATE_STREAM_REFRESH_INTERVAL,
	SCROLL_CAPTURE_DUPLICATE_STREAM_STALL_THRESHOLD, SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
	SCROLL_CAPTURE_STREAM_BACKLOG_MAX_FRAMES, SCROLL_CAPTURE_STREAM_EVENT_FALLBACK_POLL_INTERVAL,
	SCROLL_CAPTURE_STREAM_POLL_INTERVAL,
};
#[cfg(target_os = "macos")]
use crate::overlay::session_state::{LiveStreamStaleGrace, ScrollCaptureLiveFrame};
#[cfg(target_os = "macos")]
use crate::overlay::{MonitorRect, OverlayControl};
use crate::overlay::{OverlaySession, ScrollCaptureFrameSource, ScrollObserveOutcome};
#[cfg(target_os = "macos")]
use crate::scroll_capture;

impl OverlaySession {
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) fn try_consume_scroll_stream_frame(&mut self) -> bool {
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
	pub(in crate::overlay) fn note_scroll_capture_live_stream_frame_activity(
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
	pub(in crate::overlay) fn maybe_schedule_duplicate_stream_refresh(
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

		if backlog.len() >= SCROLL_CAPTURE_STREAM_BACKLOG_MAX_FRAMES {
			backlog.pop_front();
		}

		backlog.push_back(frame);
	}

	#[cfg(all(test, target_os = "macos"))]
	pub(in crate::overlay) fn test_push_scroll_capture_live_frame(
		&mut self,
		frame: ScrollCaptureLiveFrame,
	) {
		self.push_scroll_capture_live_frame(frame);
	}

	#[cfg(target_os = "macos")]
	pub(super) fn consume_scroll_capture_backlog(&mut self, max_frames: usize) {
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
	pub(in crate::overlay) fn test_consume_scroll_capture_backlog(&mut self, max_frames: usize) {
		self.consume_scroll_capture_backlog(max_frames);
	}

	pub(in crate::overlay) fn replay_recorded_live_stream_frame(
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
	pub(super) fn poll_scroll_stream_fallback_if_due(&mut self, now: Instant) {
		let should_poll_stream_fallback =
			self.scroll_capture.last_stream_event_at.is_none_or(|last| {
				now.duration_since(last) >= SCROLL_CAPTURE_STREAM_EVENT_FALLBACK_POLL_INTERVAL
			});

		if should_poll_stream_fallback {
			let _ = self.try_consume_scroll_stream_frame();
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn poll_scroll_stream_regular_if_due(&mut self, now: Instant) {
		if self.scroll_capture.live_stream.is_none() {
			return;
		}
		if self.scroll_capture.last_stream_poll_at.is_some_and(|last| {
			now.saturating_duration_since(last) < SCROLL_CAPTURE_STREAM_POLL_INTERVAL
		}) {
			return;
		}

		self.scroll_capture.last_stream_poll_at = Some(now);

		let _ = self.try_consume_scroll_stream_frame();
	}

	#[cfg(target_os = "macos")]
	pub(in crate::overlay) fn clear_incompatible_live_stream_stale_grace(&mut self) {
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
	pub(in crate::overlay) fn refresh_live_stream_stale_grace_for_external_input(
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
	pub(in crate::overlay) fn consume_live_stream_stale_grace_if_current(&mut self) -> bool {
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
