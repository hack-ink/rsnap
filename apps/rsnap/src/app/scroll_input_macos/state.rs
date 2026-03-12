use std::collections::VecDeque;
use std::sync::{
	Mutex,
	atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Instant;

const SHARED_SCROLL_INPUT_QUEUE_CAPACITY: usize = 64;

#[derive(Default)]
pub(in crate::app) struct SharedScrollInputState {
	enabled: AtomicBool,
	queue_state: Mutex<SharedScrollInputQueueState>,
	next_seq: AtomicU64,
}

impl SharedScrollInputState {
	pub(in crate::app) fn set_enabled(&self, enabled: bool) {
		self.enabled.store(enabled, Ordering::Release);
	}

	pub(in crate::app) fn is_enabled(&self) -> bool {
		self.enabled.load(Ordering::Acquire)
	}

	pub(in crate::app) fn clear(&self) {
		let mut queue_state = match self.queue_state.lock() {
			Ok(queue_state) => queue_state,
			Err(poisoned) => poisoned.into_inner(),
		};

		*queue_state = SharedScrollInputQueueState::default();
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

		event
	}

	pub(in crate::app) fn replay_after_seq_through(
		&self,
		after_seq: u64,
		through: Instant,
	) -> Vec<(u64, Instant, f64, f64, f64, bool, bool)> {
		let queue_state = match self.queue_state.lock() {
			Ok(queue_state) => queue_state,
			Err(poisoned) => poisoned.into_inner(),
		};

		queue_state
			.queue
			.iter()
			.copied()
			.filter(|event| event.seq > after_seq && event.recorded_at <= through)
			.map(SharedScrollInputEvent::tuple)
			.collect()
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

#[cfg(test)]
mod tests {
	use std::time::{Duration, Instant};

	use super::{SHARED_SCROLL_INPUT_QUEUE_CAPACITY, SharedScrollInputState};

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
}
