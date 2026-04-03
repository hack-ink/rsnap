use std::collections::VecDeque;
use std::sync::{
	Arc, Condvar, Mutex,
	atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

type SharedScrollInputEventWaker = Arc<dyn Fn() + Send + Sync>;

const SHARED_SCROLL_INPUT_QUEUE_CAPACITY: usize = 512;

#[derive(Default)]
pub(in crate::app) struct SharedScrollInputState {
	enabled: AtomicBool,
	queue_state: Mutex<SharedScrollInputQueueState>,
	event_waker: Mutex<Option<SharedScrollInputEventWaker>>,
	next_seq: AtomicU64,
}
impl SharedScrollInputState {
	pub(in crate::app) fn set_enabled(&self, enabled: bool) {
		self.enabled.store(enabled, Ordering::Release);

		tracing::info!(
			op = "scroll_input.enabled_state_changed",
			enabled,
			"Updated native scroll input enabled state."
		);
	}

	pub(in crate::app) fn is_enabled(&self) -> bool {
		self.enabled.load(Ordering::Acquire)
	}

	pub(in crate::app) fn clear(&self) {
		let mut queue_state = match self.queue_state.lock() {
			Ok(queue_state) => queue_state,
			Err(poisoned) => poisoned.into_inner(),
		};
		let cleared_events = queue_state.queue.len();

		*queue_state = SharedScrollInputQueueState::default();

		tracing::info!(
			op = "scroll_input.queue_cleared",
			cleared_events,
			"Cleared queued native scroll input events."
		);
	}

	pub(in crate::app) fn set_event_waker(&self, event_waker: Option<SharedScrollInputEventWaker>) {
		let mut waker_slot = match self.event_waker.lock() {
			Ok(waker_slot) => waker_slot,
			Err(poisoned) => poisoned.into_inner(),
		};

		*waker_slot = event_waker;
	}

	pub(in crate::app) fn record(
		&self,
		delta_y: f64,
		global_x: f64,
		global_y: f64,
		gesture_active: bool,
		gesture_ended: bool,
	) {
		let _ = self.record_at(
			Instant::now(),
			delta_y,
			global_x,
			global_y,
			gesture_active,
			gesture_ended,
		);
	}

	fn record_at(
		&self,
		recorded_at: Instant,
		delta_y: f64,
		global_x: f64,
		global_y: f64,
		gesture_active: bool,
		gesture_ended: bool,
	) -> SharedScrollInputEvent {
		let seq = self.next_seq.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
		let mut queue_state = match self.queue_state.lock() {
			Ok(queue_state) => queue_state,
			Err(poisoned) => poisoned.into_inner(),
		};
		let (effective_delta_y, effective_global_x, effective_global_y) =
			if gesture_ended && delta_y == 0.0 {
				match queue_state.last_recorded {
					Some(last_recorded) if last_recorded.delta_y != 0.0 => {
						(last_recorded.delta_y, last_recorded.global_x, last_recorded.global_y)
					},
					_ => (delta_y, global_x, global_y),
				}
			} else {
				(delta_y, global_x, global_y)
			};
		let event = SharedScrollInputEvent {
			seq,
			recorded_at,
			delta_y: effective_delta_y,
			global_x: effective_global_x,
			global_y: effective_global_y,
			gesture_active,
			gesture_ended,
		};

		if queue_state.queue.len() >= SHARED_SCROLL_INPUT_QUEUE_CAPACITY {
			queue_state.queue.pop_front();
		}

		queue_state.queue.push_back(event);

		queue_state.last_recorded = Some(event);

		tracing::debug!(
			op = "scroll_input.queued",
			seq,
			delta_y = event.delta_y,
			global_x = event.global_x,
			global_y = event.global_y,
			gesture_active = event.gesture_active,
			gesture_ended = event.gesture_ended,
			queue_len = queue_state.queue.len(),
			"Queued native scroll input event for later overlay replay."
		);

		let event_waker = match self.event_waker.lock() {
			Ok(waker_slot) => waker_slot.clone(),
			Err(poisoned) => poisoned.into_inner().clone(),
		};

		if let Some(event_waker) = event_waker {
			event_waker();
		}

		event
	}

	pub(in crate::app) fn replay_after_seq_through(
		&self,
		after_seq: u64,
		through: Instant,
	) -> Vec<(u64, Instant, f64, f64, f64, bool, bool)> {
		let mut queue_state = match self.queue_state.lock() {
			Ok(queue_state) => queue_state,
			Err(poisoned) => poisoned.into_inner(),
		};
		let mut pruned_events = 0_usize;

		while queue_state.queue.front().is_some_and(|event| event.seq <= after_seq) {
			let _ = queue_state.queue.pop_front();

			pruned_events = pruned_events.saturating_add(1);
		}

		let queued_after_seq = queue_state.queue.len();
		let future_events =
			queue_state.queue.iter().filter(|event| event.recorded_at > through).count();
		let replay = queue_state
			.queue
			.iter()
			.copied()
			.filter(|event| event.recorded_at <= through)
			.map(SharedScrollInputEvent::tuple)
			.collect::<Vec<_>>();

		if !replay.is_empty() || future_events > 0 || pruned_events > 0 {
			let newest_seq = queue_state.queue.back().map(|event| event.seq).unwrap_or(0);

			tracing::debug!(
				op = "scroll_input.replay_window",
				after_seq,
				pruned_events,
				queued_after_seq,
				replay_count = replay.len(),
				future_events,
				newest_seq,
				"Evaluated queued native scroll input events for overlay replay."
			);
		}

		replay
	}
}

#[derive(Default)]
pub(in crate::app) struct ScrollInputObserverLifecycle {
	status: Mutex<ScrollInputObserverStatus>,
	status_changed: Condvar,
}
impl ScrollInputObserverLifecycle {
	pub(in crate::app) fn begin_start_if_needed(&self) -> bool {
		let mut status = match self.status.lock() {
			Ok(status) => status,
			Err(poisoned) => poisoned.into_inner(),
		};

		match *status {
			ScrollInputObserverStatus::Idle | ScrollInputObserverStatus::Failed => {
				*status = ScrollInputObserverStatus::Starting;

				self.status_changed.notify_all();

				true
			},
			ScrollInputObserverStatus::Starting | ScrollInputObserverStatus::Ready => false,
		}
	}

	pub(in crate::app) fn wait_until_ready(
		&self,
		timeout: Duration,
	) -> ScrollInputObserverWaitOutcome {
		let mut status = match self.status.lock() {
			Ok(status) => status,
			Err(poisoned) => poisoned.into_inner(),
		};

		if *status == ScrollInputObserverStatus::Ready {
			return ScrollInputObserverWaitOutcome::Ready;
		}
		if *status == ScrollInputObserverStatus::Failed {
			return ScrollInputObserverWaitOutcome::Failed;
		}

		let wait_result = self.status_changed.wait_timeout_while(status, timeout, |status| {
			*status == ScrollInputObserverStatus::Starting
		});
		let (status_after_wait, timeout_result) = match wait_result {
			Ok(wait_result) => wait_result,
			Err(poisoned) => poisoned.into_inner(),
		};

		status = status_after_wait;

		match *status {
			ScrollInputObserverStatus::Ready => ScrollInputObserverWaitOutcome::Ready,
			ScrollInputObserverStatus::Failed | ScrollInputObserverStatus::Idle => {
				ScrollInputObserverWaitOutcome::Failed
			},
			ScrollInputObserverStatus::Starting if timeout_result.timed_out() => {
				ScrollInputObserverWaitOutcome::TimedOut
			},
			ScrollInputObserverStatus::Starting => ScrollInputObserverWaitOutcome::TimedOut,
		}
	}

	pub(in crate::app) fn mark_ready(&self) {
		self.set_status(ScrollInputObserverStatus::Ready);
	}

	pub(in crate::app) fn mark_failed(&self) {
		self.set_status(ScrollInputObserverStatus::Failed);
	}

	pub(in crate::app) fn status(&self) -> ScrollInputObserverStatus {
		let status = match self.status.lock() {
			Ok(status) => status,
			Err(poisoned) => poisoned.into_inner(),
		};

		*status
	}

	fn set_status(&self, new_status: ScrollInputObserverStatus) {
		let mut status = match self.status.lock() {
			Ok(status) => status,
			Err(poisoned) => poisoned.into_inner(),
		};

		*status = new_status;

		self.status_changed.notify_all();
	}
}

#[derive(Clone, Copy, Debug)]
struct SharedScrollInputEvent {
	seq: u64,
	recorded_at: Instant,
	delta_y: f64,
	global_x: f64,
	global_y: f64,
	gesture_active: bool,
	gesture_ended: bool,
}
impl SharedScrollInputEvent {
	fn tuple(self) -> (u64, Instant, f64, f64, f64, bool, bool) {
		(
			self.seq,
			self.recorded_at,
			self.global_x,
			self.global_y,
			self.delta_y,
			self.gesture_active,
			self.gesture_ended,
		)
	}
}

#[derive(Default)]
struct SharedScrollInputQueueState {
	queue: VecDeque<SharedScrollInputEvent>,
	last_recorded: Option<SharedScrollInputEvent>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::app) enum ScrollInputObserverStatus {
	#[default]
	Idle,
	Starting,
	Ready,
	Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum ScrollInputObserverWaitOutcome {
	Ready,
	TimedOut,
	Failed,
}

#[cfg(test)]
mod tests {
	use std::sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	};
	use std::thread;
	use std::time::{Duration, Instant};

	use crate::app::scroll_input_macos::state::{
		SHARED_SCROLL_INPUT_QUEUE_CAPACITY, ScrollInputObserverLifecycle,
		ScrollInputObserverStatus, ScrollInputObserverWaitOutcome, SharedScrollInputState,
	};

	#[test]
	fn terminal_scroll_event_preserves_last_effective_delta() {
		let state = SharedScrollInputState::default();
		let start = Instant::now();

		state.record_at(start, -4.0, 120.0, 140.0, true, false);
		state.record_at(start + Duration::from_millis(1), 0.0, 0.0, 0.0, false, true);

		assert_eq!(
			state.replay_after_seq_through(0, start + Duration::from_millis(1)),
			vec![
				(1, start, 120.0, 140.0, -4.0, true, false),
				(2, start + Duration::from_millis(1), 120.0, 140.0, -4.0, false, true),
			]
		);
	}

	#[test]
	fn newer_non_zero_scroll_event_replaces_preserved_delta() {
		let state = SharedScrollInputState::default();
		let start = Instant::now();

		state.record_at(start, -4.0, 120.0, 140.0, true, false);
		state.record_at(start + Duration::from_millis(1), 0.0, 0.0, 0.0, false, true);
		state.record_at(start + Duration::from_millis(2), 6.0, 220.0, 260.0, true, false);

		assert_eq!(
			state.replay_after_seq_through(0, start + Duration::from_millis(2)),
			vec![
				(1, start, 120.0, 140.0, -4.0, true, false),
				(2, start + Duration::from_millis(1), 120.0, 140.0, -4.0, false, true),
				(3, start + Duration::from_millis(2), 220.0, 260.0, 6.0, true, false),
			]
		);
	}

	#[test]
	fn replay_after_seq_through_preserves_order_and_uses_sequence_cursor() {
		let state = SharedScrollInputState::default();
		let start = Instant::now();

		state.record_at(start, -4.0, 120.0, 140.0, true, false);
		state.record_at(start + Duration::from_millis(2), 6.0, 220.0, 260.0, true, false);
		state.record_at(start + Duration::from_millis(4), 0.0, 0.0, 0.0, false, true);

		assert_eq!(
			state.replay_after_seq_through(0, start + Duration::from_millis(2)),
			vec![
				(1, start, 120.0, 140.0, -4.0, true, false),
				(2, start + Duration::from_millis(2), 220.0, 260.0, 6.0, true, false),
			]
		);
		assert!(state.replay_after_seq_through(2, start + Duration::from_millis(3)).is_empty());
		assert_eq!(
			state.replay_after_seq_through(2, start + Duration::from_millis(4)),
			vec![(3, start + Duration::from_millis(4), 220.0, 260.0, 6.0, false, true)]
		);
	}

	#[test]
	fn replay_after_seq_through_keeps_only_the_bounded_tail() {
		let state = SharedScrollInputState::default();
		let start = Instant::now();

		for offset in 0..(SHARED_SCROLL_INPUT_QUEUE_CAPACITY + 2) {
			state.record_at(
				start + Duration::from_millis(offset as u64),
				-(offset as f64),
				offset as f64,
				offset as f64 + 10.0,
				true,
				false,
			);
		}

		let replay = state.replay_after_seq_through(
			0,
			start + Duration::from_millis((SHARED_SCROLL_INPUT_QUEUE_CAPACITY + 2) as u64),
		);

		assert_eq!(replay.len(), SHARED_SCROLL_INPUT_QUEUE_CAPACITY);
		assert_eq!(replay.first().map(|event| event.0), Some(3));
		assert_eq!(
			replay.last().map(|event| event.0),
			Some((SHARED_SCROLL_INPUT_QUEUE_CAPACITY + 2) as u64)
		);
	}

	#[test]
	fn replay_after_seq_through_prunes_consumed_prefix_before_future_polls() {
		let state = SharedScrollInputState::default();
		let start = Instant::now();

		state.record_at(start, -4.0, 120.0, 140.0, true, false);
		state.record_at(start + Duration::from_millis(1), -3.0, 120.0, 140.0, true, false);
		state.record_at(start + Duration::from_millis(2), -2.0, 120.0, 140.0, true, false);
		state.record_at(start + Duration::from_millis(3), -1.0, 120.0, 140.0, false, true);

		let _ = state.replay_after_seq_through(0, start + Duration::from_millis(2));
		let _ = state.replay_after_seq_through(2, start + Duration::from_millis(2));
		let queue_state = state.queue_state.lock().unwrap();
		let queued_seqs = queue_state.queue.iter().map(|event| event.seq).collect::<Vec<_>>();

		assert_eq!(queued_seqs, vec![3, 4]);
	}

	#[test]
	fn record_invokes_event_waker() {
		let state = SharedScrollInputState::default();
		let wake_count = Arc::new(AtomicUsize::new(0));

		state.set_event_waker(Some(Arc::new({
			let wake_count = Arc::clone(&wake_count);

			move || {
				wake_count.fetch_add(1, Ordering::AcqRel);
			}
		})));

		state.record(-4.0, 120.0, 140.0, true, false);

		assert_eq!(wake_count.load(Ordering::Acquire), 1);
	}

	#[test]
	fn observer_lifecycle_waits_for_ready() {
		let lifecycle = std::sync::Arc::new(ScrollInputObserverLifecycle::default());

		assert!(lifecycle.begin_start_if_needed());

		thread::spawn({
			let lifecycle = std::sync::Arc::clone(&lifecycle);

			move || {
				thread::sleep(Duration::from_millis(10));

				lifecycle.mark_ready();
			}
		});

		assert_eq!(
			lifecycle.wait_until_ready(Duration::from_millis(100)),
			ScrollInputObserverWaitOutcome::Ready
		);
		assert_eq!(lifecycle.status(), ScrollInputObserverStatus::Ready);
	}

	#[test]
	fn observer_lifecycle_times_out_while_starting() {
		let lifecycle = ScrollInputObserverLifecycle::default();

		assert!(lifecycle.begin_start_if_needed());
		assert_eq!(
			lifecycle.wait_until_ready(Duration::from_millis(1)),
			ScrollInputObserverWaitOutcome::TimedOut
		);
		assert_eq!(lifecycle.status(), ScrollInputObserverStatus::Starting);
	}

	#[test]
	fn observer_lifecycle_restarts_after_failure() {
		let lifecycle = ScrollInputObserverLifecycle::default();

		assert!(lifecycle.begin_start_if_needed());

		lifecycle.mark_failed();

		assert_eq!(
			lifecycle.wait_until_ready(Duration::from_millis(1)),
			ScrollInputObserverWaitOutcome::Failed
		);
		assert!(lifecycle.begin_start_if_needed());
		assert_eq!(lifecycle.status(), ScrollInputObserverStatus::Starting);
	}
}
