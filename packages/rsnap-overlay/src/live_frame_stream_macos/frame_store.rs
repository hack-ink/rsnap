use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

use crate::live_frame_stream_macos::STREAM_FRAME_QUEUE_CAPACITY;
use crate::live_frame_stream_macos::STREAM_POST_SETUP_FRAME_GRACE;
use crate::live_frame_stream_macos::live_frame_buffer::{
	QueuedPixelBufferFrame, SharedQueuedPixelBufferFrames,
};

#[derive(Default)]
pub(super) struct SharedLatestFrame {
	frames: Mutex<Option<SharedQueuedPixelBufferFrames>>,
	pending_monitor: Mutex<Option<u32>>,
	pending_refresh_monitor: Mutex<Option<PendingMonitorRequest>>,
	waiting_for_frame_until: Mutex<Option<(u32, Instant)>>,
	active_stream_generation: Mutex<Option<StreamGenerationStatus>>,
	stream_filter_status: Mutex<Option<StreamFilterStatus>>,
	pending_stream_filter_complete_monitor: Mutex<Option<StreamGenerationStatus>>,
}
impl SharedLatestFrame {
	pub(super) fn reset(&self, retired_stream: Option<StreamGenerationStatus>) {
		match self.frames.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
		match self.pending_monitor.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
		match self.pending_refresh_monitor.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
		match self.waiting_for_frame_until.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}

		if let Some(retired_stream) = retired_stream {
			match self.active_stream_generation.lock() {
				Ok(mut guard) => {
					*guard = Some(StreamGenerationStatus::retired_after(retired_stream))
				},
				Err(poisoned) => {
					let mut guard = poisoned.into_inner();

					*guard = Some(StreamGenerationStatus::retired_after(retired_stream));
				},
			}
		}

		match self.stream_filter_status.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
		match self.pending_stream_filter_complete_monitor.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
	}

	pub(super) fn store(
		&self,
		monitor_id: u32,
		frame: &QueuedPixelBufferFrame,
	) -> StoreFrameOutcome {
		if !self.stream_generation_is_active_for_monitor(monitor_id, frame.stream_generation) {
			return StoreFrameOutcome { completed_ensure: false, completed_refresh: false };
		}

		match self.frames.lock() {
			Ok(mut guard) => {
				let shared = guard.get_or_insert_with(|| SharedQueuedPixelBufferFrames {
					monitor_id,
					frames: VecDeque::with_capacity(STREAM_FRAME_QUEUE_CAPACITY),
				});

				if shared.monitor_id != monitor_id {
					shared.monitor_id = monitor_id;

					shared.frames.clear();
				}
				if shared.frames.len() >= STREAM_FRAME_QUEUE_CAPACITY {
					shared.frames.pop_front();
				}

				shared.frames.push_back(frame.clone());
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();
				let shared = guard.get_or_insert_with(|| SharedQueuedPixelBufferFrames {
					monitor_id,
					frames: VecDeque::with_capacity(STREAM_FRAME_QUEUE_CAPACITY),
				});

				if shared.monitor_id != monitor_id {
					shared.monitor_id = monitor_id;

					shared.frames.clear();
				}
				if shared.frames.len() >= STREAM_FRAME_QUEUE_CAPACITY {
					shared.frames.pop_front();
				}

				shared.frames.push_back(frame.clone());
			},
		}

		self.complete_pending_requests_for_stored_frame(monitor_id, frame.stream_generation)
	}

	pub(super) fn complete_pending_requests_for_stored_frame(
		&self,
		monitor_id: u32,
		stream_generation: u64,
	) -> StoreFrameOutcome {
		self.complete_pending_stream_filter_status(monitor_id, stream_generation);
		self.clear_waiting_for_frame(monitor_id);
		StoreFrameOutcome {
			completed_ensure: self.finish_ensure_monitor(monitor_id),
			completed_refresh: self.finish_refresh_monitor(monitor_id),
		}
	}

	pub(super) fn latest_frame_for_monitor(
		&self,
		monitor_id: u32,
	) -> Option<QueuedPixelBufferFrame> {
		let active_stream_generation = self.active_stream_generation_for_monitor(monitor_id);

		match self.frames.lock() {
			Ok(guard) => guard
				.as_ref()
				.and_then(|latest| {
					(latest.monitor_id == monitor_id).then(|| {
						latest
							.frames
							.iter()
							.rev()
							.find(|frame| {
								active_stream_generation
									.is_none_or(|generation| frame.stream_generation == generation)
							})
							.cloned()
					})
				})
				.flatten(),
			Err(poisoned) => poisoned
				.into_inner()
				.as_ref()
				.and_then(|latest| {
					(latest.monitor_id == monitor_id).then(|| {
						latest
							.frames
							.iter()
							.rev()
							.find(|frame| {
								active_stream_generation
									.is_none_or(|generation| frame.stream_generation == generation)
							})
							.cloned()
					})
				})
				.flatten(),
		}
	}

	pub(super) fn frames_after_seq_for_monitor(
		&self,
		monitor_id: u32,
		after_frame_seq: u64,
	) -> Vec<QueuedPixelBufferFrame> {
		let active_stream_generation = self.active_stream_generation_for_monitor(monitor_id);

		match self.frames.lock() {
			Ok(guard) => guard
				.as_ref()
				.filter(|shared| shared.monitor_id == monitor_id)
				.map(|shared| {
					shared
						.frames
						.iter()
						.filter(|frame| {
							frame.frame_seq > after_frame_seq
								&& active_stream_generation
									.is_none_or(|generation| frame.stream_generation == generation)
						})
						.cloned()
						.collect()
				})
				.unwrap_or_default(),
			Err(poisoned) => poisoned
				.into_inner()
				.as_ref()
				.filter(|shared| shared.monitor_id == monitor_id)
				.map(|shared| {
					shared
						.frames
						.iter()
						.filter(|frame| {
							frame.frame_seq > after_frame_seq
								&& active_stream_generation
									.is_none_or(|generation| frame.stream_generation == generation)
						})
						.cloned()
						.collect()
				})
				.unwrap_or_default(),
		}
	}

	pub(super) fn begin_ensure_monitor(&self, monitor_id: u32) -> bool {
		match self.pending_monitor.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					return false;
				}

				*guard = Some(monitor_id);
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					return false;
				}

				*guard = Some(monitor_id);
			},
		}

		true
	}

	pub(super) fn finish_ensure_monitor(&self, monitor_id: u32) -> bool {
		match self.pending_monitor.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					*guard = None;

					return true;
				}
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					*guard = None;

					return true;
				}
			},
		}

		false
	}

	pub(super) fn begin_refresh_monitor(
		&self,
		monitor_id: u32,
		stalled_after_frame_seq: u64,
		now: Instant,
	) -> bool {
		match self.pending_refresh_monitor.lock() {
			Ok(mut guard) => {
				if let Some(pending) = *guard {
					if pending.monitor_id != monitor_id {
						return false;
					}
					if pending.stalled_after_frame_seq != stalled_after_frame_seq {
						*guard = Some(PendingMonitorRequest {
							monitor_id,
							stalled_after_frame_seq,
							started_at: now,
						});

						return true;
					}
					if now.saturating_duration_since(pending.started_at)
						< STREAM_POST_SETUP_FRAME_GRACE
					{
						return false;
					}

					tracing::info!(
						op = "live_frame_stream.stale_pending_refresh_recovered",
						monitor_id,
						stalled_after_frame_seq,
						pending_age_ms =
							now.saturating_duration_since(pending.started_at).as_millis() as u64,
						"Recovered a stale pending ScreenCaptureKit refresh so a new refresh can be scheduled."
					);

					*guard = Some(PendingMonitorRequest {
						monitor_id,
						stalled_after_frame_seq,
						started_at: now,
					});

					return true;
				}

				*guard = Some(PendingMonitorRequest {
					monitor_id,
					stalled_after_frame_seq,
					started_at: now,
				});
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if let Some(pending) = *guard {
					if pending.monitor_id != monitor_id {
						return false;
					}
					if pending.stalled_after_frame_seq != stalled_after_frame_seq {
						*guard = Some(PendingMonitorRequest {
							monitor_id,
							stalled_after_frame_seq,
							started_at: now,
						});

						return true;
					}
					if now.saturating_duration_since(pending.started_at)
						< STREAM_POST_SETUP_FRAME_GRACE
					{
						return false;
					}

					tracing::info!(
						op = "live_frame_stream.stale_pending_refresh_recovered",
						monitor_id,
						stalled_after_frame_seq,
						pending_age_ms =
							now.saturating_duration_since(pending.started_at).as_millis() as u64,
						"Recovered a stale pending ScreenCaptureKit refresh so a new refresh can be scheduled."
					);

					*guard = Some(PendingMonitorRequest {
						monitor_id,
						stalled_after_frame_seq,
						started_at: now,
					});

					return true;
				}

				*guard = Some(PendingMonitorRequest {
					monitor_id,
					stalled_after_frame_seq,
					started_at: now,
				});
			},
		}

		true
	}

	pub(super) fn mark_waiting_for_frame(&self, monitor_id: u32) {
		self.mark_waiting_for_frame_until(
			monitor_id,
			Instant::now() + STREAM_POST_SETUP_FRAME_GRACE,
		);
	}

	pub(super) fn mark_waiting_for_frame_until(&self, monitor_id: u32, until: Instant) {
		match self.waiting_for_frame_until.lock() {
			Ok(mut guard) => {
				*guard = Some((monitor_id, until));
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = Some((monitor_id, until));
			},
		}
	}

	pub(super) fn waiting_for_frame_after_setup(&self, monitor_id: u32) -> bool {
		self.waiting_for_frame_after_setup_at(monitor_id, Instant::now())
	}

	pub(super) fn waiting_for_frame_after_setup_at(&self, monitor_id: u32, now: Instant) -> bool {
		match self.waiting_for_frame_until.lock() {
			Ok(mut guard) => {
				let Some((pending_monitor_id, until)) = *guard else {
					return false;
				};

				if pending_monitor_id != monitor_id {
					return false;
				}
				if now < until {
					return true;
				}

				*guard = None;
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();
				let Some((pending_monitor_id, until)) = *guard else {
					return false;
				};

				if pending_monitor_id != monitor_id {
					return false;
				}
				if now < until {
					return true;
				}

				*guard = None;
			},
		}

		false
	}

	pub(super) fn clear_waiting_for_frame(&self, monitor_id: u32) {
		match self.waiting_for_frame_until.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|(pending_monitor_id, _)| pending_monitor_id == monitor_id) {
					*guard = None;
				}
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|(pending_monitor_id, _)| pending_monitor_id == monitor_id) {
					*guard = None;
				}
			},
		}
	}

	pub(super) fn finish_refresh_monitor(&self, monitor_id: u32) -> bool {
		match self.pending_refresh_monitor.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|pending| pending.monitor_id == monitor_id) {
					*guard = None;

					return true;
				}
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|pending| pending.monitor_id == monitor_id) {
					*guard = None;

					return true;
				}
			},
		}

		false
	}

	pub(super) fn set_stream_filter_status(
		&self,
		monitor_id: u32,
		self_capture_filter_complete: bool,
	) {
		match self.stream_filter_status.lock() {
			Ok(mut guard) => {
				*guard = Some(StreamFilterStatus { monitor_id, self_capture_filter_complete });
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = Some(StreamFilterStatus { monitor_id, self_capture_filter_complete });
			},
		}
	}

	pub(super) fn self_capture_filter_complete_for_monitor(&self, monitor_id: u32) -> bool {
		match self.stream_filter_status.lock() {
			Ok(guard) => guard.as_ref().is_some_and(|status| {
				status.monitor_id == monitor_id && status.self_capture_filter_complete
			}),
			Err(poisoned) => poisoned.into_inner().as_ref().is_some_and(|status| {
				status.monitor_id == monitor_id && status.self_capture_filter_complete
			}),
		}
	}

	pub(super) fn activate_stream_generation(&self, monitor_id: u32, stream_generation: u64) {
		match self.active_stream_generation.lock() {
			Ok(mut guard) => {
				*guard = Some(StreamGenerationStatus { monitor_id, stream_generation });
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = Some(StreamGenerationStatus { monitor_id, stream_generation });
			},
		}
	}

	pub(super) fn active_stream_generation_for_monitor(&self, monitor_id: u32) -> Option<u64> {
		match self.active_stream_generation.lock() {
			Ok(guard) => guard.as_ref().and_then(|status| {
				(status.monitor_id == monitor_id).then_some(status.stream_generation)
			}),
			Err(poisoned) => poisoned.into_inner().as_ref().and_then(|status| {
				(status.monitor_id == monitor_id).then_some(status.stream_generation)
			}),
		}
	}

	pub(super) fn stream_generation_is_active_for_monitor(
		&self,
		monitor_id: u32,
		stream_generation: u64,
	) -> bool {
		self.active_stream_generation_for_monitor(monitor_id)
			.is_none_or(|active_generation| active_generation == stream_generation)
	}

	pub(super) fn defer_stream_filter_complete_until_next_frame(
		&self,
		monitor_id: u32,
		stream_generation: u64,
		self_capture_filter_complete: bool,
	) {
		self.set_stream_filter_status(monitor_id, false);

		match self.pending_stream_filter_complete_monitor.lock() {
			Ok(mut guard) => {
				*guard = self_capture_filter_complete
					.then_some(StreamGenerationStatus { monitor_id, stream_generation });
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = self_capture_filter_complete
					.then_some(StreamGenerationStatus { monitor_id, stream_generation });
			},
		}
	}

	pub(super) fn complete_pending_stream_filter_status(
		&self,
		monitor_id: u32,
		stream_generation: u64,
	) {
		let should_mark_complete = match self.pending_stream_filter_complete_monitor.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|pending| {
					pending.monitor_id == monitor_id
						&& pending.stream_generation == stream_generation
				}) {
					*guard = None;

					true
				} else {
					false
				}
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|pending| {
					pending.monitor_id == monitor_id
						&& pending.stream_generation == stream_generation
				}) {
					*guard = None;

					true
				} else {
					false
				}
			},
		};

		if should_mark_complete {
			self.set_stream_filter_status(monitor_id, true);
		}
	}

	#[cfg(test)]
	pub(super) fn pending_monitor_is_none(&self) -> bool {
		match self.pending_monitor.lock() {
			Ok(guard) => guard.is_none(),
			Err(poisoned) => poisoned.into_inner().is_none(),
		}
	}
}

#[derive(Clone, Copy)]
pub(super) struct StreamGenerationStatus {
	pub(super) monitor_id: u32,
	pub(super) stream_generation: u64,
}
impl StreamGenerationStatus {
	fn retired_after(status: Self) -> Self {
		Self {
			monitor_id: status.monitor_id,
			stream_generation: status.stream_generation.wrapping_add(1),
		}
	}
}

pub(super) struct StoreFrameOutcome {
	pub(super) completed_ensure: bool,
	pub(super) completed_refresh: bool,
}

#[derive(Clone, Copy)]
struct PendingMonitorRequest {
	monitor_id: u32,
	stalled_after_frame_seq: u64,
	started_at: Instant,
}

#[derive(Clone, Copy)]
struct StreamFilterStatus {
	monitor_id: u32,
	self_capture_filter_complete: bool,
}
