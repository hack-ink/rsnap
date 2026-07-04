#[cfg(target_os = "macos")]
use std::time::Duration;
use std::time::Instant;

use crate::overlay::OverlaySession;
#[cfg(target_os = "macos")]
use crate::overlay::ScrollCaptureHostFrameRequestError;
#[cfg(target_os = "macos")]
use crate::overlay::scroll_capture_timing::SCROLL_CAPTURE_INPUT_FRESHNESS;
use crate::overlay::scroll_capture_timing::SCROLL_CAPTURE_SAMPLE_INTERVAL;
#[cfg(target_os = "macos")]
use crate::overlay::session_state::InflightScrollCaptureObservation;
#[cfg(any(not(target_os = "macos"), all(test, target_os = "macos")))]
use crate::overlay::{MonitorRect, RectPoints};
#[cfg(target_os = "macos")]
use crate::scroll_capture::ScrollDirection;
#[cfg(any(not(target_os = "macos"), all(test, target_os = "macos")))]
use crate::worker::WorkerRequestSendError;

impl OverlaySession {
	#[cfg(target_os = "macos")]
	pub(in crate::overlay) fn should_use_scroll_capture_worker_sampling(&self) -> bool {
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

	#[cfg(any(not(target_os = "macos"), all(test, target_os = "macos")))]
	fn request_scroll_capture_worker_sample_with_worker(
		&mut self,
		now: Instant,
		monitor: MonitorRect,
		capture_rect: RectPoints,
	) {
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
						Some(InflightScrollCaptureObservation {
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

	pub(super) fn request_scroll_capture_worker_sample_at(&mut self, now: Instant) {
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

		#[cfg(all(test, target_os = "macos"))]
		if self.scroll_capture_host_adapter.is_none() {
			self.request_scroll_capture_worker_sample_with_worker(now, monitor, capture_rect);

			return;
		}

		#[cfg(not(target_os = "macos"))]
		{
			self.request_scroll_capture_worker_sample_with_worker(now, monitor, capture_rect);
		}

		#[cfg(target_os = "macos")]
		{
			let Some(host_adapter) = self.scroll_capture_host_adapter.as_ref() else {
				self.scroll_capture_set_error("Scroll capture capability is unavailable.");

				return;
			};
			let request_id = self.scroll_capture.next_request_id.wrapping_add(1);

			match (host_adapter.request_frame)(monitor, capture_rect, request_id) {
				Ok(()) => {
					self.scroll_capture.next_request_id = request_id;
					self.scroll_capture.inflight_request_id = Some(request_id);
					#[cfg(target_os = "macos")]
					{
						self.scroll_capture.inflight_request_observation =
							Some(InflightScrollCaptureObservation {
								was_observable: self
									.scroll_capture_observation_block_reason_at(now)
									.is_none(),
								external_input_seq: self
									.scroll_capture
									.last_external_scroll_input_seq,
								input_direction: self.scroll_capture.input_direction,
							});
					}
					self.scroll_capture.next_sample_at = Some(now + SCROLL_CAPTURE_SAMPLE_INTERVAL);
				},
				Err(ScrollCaptureHostFrameRequestError::Busy) => {
					self.scroll_capture.next_sample_at =
						Some(now + SCROLL_CAPTURE_SAMPLE_INTERVAL.saturating_mul(2));
				},
				Err(ScrollCaptureHostFrameRequestError::Unavailable(message)) => {
					self.scroll_capture_set_error(message);
				},
			}
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn schedule_immediate_scroll_capture_worker_retry_if_fresh_downward_input(
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
	pub(super) fn schedule_backoff_scroll_capture_worker_retry_if_fresh_downward_input(
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
}
