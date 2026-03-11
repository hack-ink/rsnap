use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Instant;

type CFMachPortRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopMode = *const c_void;
type CFAllocatorRef = *const c_void;
type CGEventRef = *const c_void;
type CGEventTapProxy = *const c_void;

const KCG_EVENT_SCROLL_WHEEL: u32 = 22;
const KCG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const KCG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const KCG_SESSION_EVENT_TAP: u32 = 1;
const KCG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const KCG_EVENT_TAP_LISTEN_ONLY: u32 = 1;
const KCG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1_FIELD: u32 = 11;
const KCG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS_FIELD: u32 = 88;
const KCG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1_FIELD: u32 = 96;
const KCG_SCROLL_WHEEL_EVENT_SCROLL_PHASE_FIELD: u32 = 99;
const KCG_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE_FIELD: u32 = 123;
const NSEVENT_PHASE_BEGAN: u64 = 0x1 << 0;
const NSEVENT_PHASE_STATIONARY: u64 = 0x1 << 1;
const NSEVENT_PHASE_CHANGED: u64 = 0x1 << 2;
const NSEVENT_PHASE_ENDED: u64 = 0x1 << 3;
const NSEVENT_PHASE_CANCELLED: u64 = 0x1 << 4;
const NSEVENT_PHASE_MAY_BEGIN: u64 = 0x1 << 5;
const SHARED_SCROLL_INPUT_QUEUE_CAPACITY: usize = 64;

#[repr(C)]
#[derive(Clone, Copy)]
struct MacOSCGPoint {
	x: f64,
	y: f64,
}

struct ScrollInputTapContext {
	shared_state: Arc<SharedScrollInputState>,
	tap: std::sync::atomic::AtomicPtr<c_void>,
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct DecodedScrollInput {
	raw_delta_y: f64,
	delta_y: f64,
	global_x: f64,
	global_y: f64,
	gesture_active: bool,
	gesture_ended: bool,
}

#[derive(Default)]
struct SharedScrollInputQueueState {
	queue: VecDeque<SharedScrollInputEvent>,
	last_recorded: Option<SharedScrollInputEvent>,
}

#[derive(Default)]
pub(super) struct SharedScrollInputState {
	enabled: AtomicBool,
	queue_state: Mutex<SharedScrollInputQueueState>,
	next_seq: AtomicU64,
}
impl SharedScrollInputState {
	pub(super) fn set_enabled(&self, enabled: bool) {
		self.enabled.store(enabled, Ordering::Release);
	}

	fn is_enabled(&self) -> bool {
		self.enabled.load(Ordering::Acquire)
	}

	pub(super) fn clear(&self) {
		let mut queue_state = match self.queue_state.lock() {
			Ok(queue_state) => queue_state,
			Err(poisoned) => poisoned.into_inner(),
		};

		*queue_state = SharedScrollInputQueueState::default();
	}

	fn record(
		&self,
		delta_y: f64,
		global_x: f64,
		global_y: f64,
		gesture_active: bool,
		gesture_ended: bool,
	) -> SharedScrollInputEvent {
		self.record_at(Instant::now(), delta_y, global_x, global_y, gesture_active, gesture_ended)
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

	pub(super) fn replay_after_seq_through(
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

pub(super) fn spawn_scroll_input_observer(
	shared_state: Arc<SharedScrollInputState>,
) -> JoinHandle<()> {
	thread::Builder::new()
		.name(String::from("rsnap-scroll-input-tap"))
		.spawn(move || {
			run_scroll_input_event_tap_thread(shared_state);
		})
		.unwrap_or_else(|error| panic!("failed to spawn rsnap scroll-input tap thread: {error}"))
}

fn run_scroll_input_event_tap_thread(shared_state: Arc<SharedScrollInputState>) {
	let context = Box::new(ScrollInputTapContext {
		shared_state,
		tap: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()),
	});
	let context_ptr = Box::into_raw(context);
	let tap = unsafe {
		CGEventTapCreate(
			KCG_SESSION_EVENT_TAP,
			KCG_HEAD_INSERT_EVENT_TAP,
			KCG_EVENT_TAP_LISTEN_ONLY,
			cg_event_mask_bit(KCG_EVENT_SCROLL_WHEEL),
			scroll_input_event_tap_callback,
			context_ptr.cast(),
		)
	};

	if tap.is_null() {
		unsafe {
			drop(Box::from_raw(context_ptr));
		}

		tracing::warn!("Failed to create scroll input event tap.");

		return;
	}

	unsafe {
		(*context_ptr).tap.store(tap, Ordering::Release);
	}

	let loop_source = unsafe { CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0) };

	if loop_source.is_null() {
		unsafe {
			CFMachPortInvalidate(tap);
			CFRelease(tap.cast());
			drop(Box::from_raw(context_ptr));
		}

		tracing::warn!("Failed to create run-loop source for scroll input event tap.");

		return;
	}

	unsafe {
		let run_loop = CFRunLoopGetCurrent();

		CFRunLoopAddSource(run_loop, loop_source, kCFRunLoopCommonModes);
		CGEventTapEnable(tap, true);
	}

	tracing::info!(
		op = "scroll_input.tap_installed",
		tap = tap as usize,
		loop_source = loop_source as usize,
		"Installed native scroll input event tap."
	);

	unsafe {
		CFRunLoopRun();
		CFMachPortInvalidate(tap);
		CFRelease(loop_source.cast());
		CFRelease(tap.cast());
		drop(Box::from_raw(context_ptr));
	}
}

fn reenable_scroll_input_event_tap(context: &ScrollInputTapContext, event_type: u32) {
	let tap = context.tap.load(Ordering::Acquire);

	if tap.is_null() {
		tracing::warn!(
			op = "scroll_input.tap_disabled",
			event_type,
			"Scroll input event tap was disabled before the tap pointer was initialized."
		);

		return;
	}

	unsafe {
		CGEventTapEnable(tap, true);
	}

	tracing::warn!(
		op = "scroll_input.tap_reenabled",
		event_type,
		tap = tap as usize,
		"Scroll input event tap was disabled and has been re-enabled."
	);
}

unsafe extern "C" fn scroll_input_event_tap_callback(
	_proxy: CGEventTapProxy,
	event_type: u32,
	event: CGEventRef,
	user_info: *const c_void,
) -> CGEventRef {
	if user_info.is_null() {
		return event;
	}

	let context = unsafe { &*(user_info.cast::<ScrollInputTapContext>()) };

	match event_type {
		KCG_EVENT_SCROLL_WHEEL => {},
		KCG_EVENT_TAP_DISABLED_BY_TIMEOUT | KCG_EVENT_TAP_DISABLED_BY_USER_INPUT => {
			reenable_scroll_input_event_tap(context, event_type);

			return event;
		},
		_ => return event,
	}

	if event.is_null() {
		return event;
	}

	send_overlay_scroll_input(context, event);

	event
}

fn send_overlay_scroll_input(context: &ScrollInputTapContext, cg_event: CGEventRef) {
	if !context.shared_state.is_enabled() {
		return;
	}

	let Some(decoded) = decode_scroll_input_from_cg_event(cg_event) else {
		return;
	};

	context.shared_state.record(
		decoded.delta_y,
		decoded.global_x,
		decoded.global_y,
		decoded.gesture_active,
		decoded.gesture_ended,
	);
}

fn decode_scroll_input_from_cg_event(cg_event: CGEventRef) -> Option<DecodedScrollInput> {
	let location = unsafe { CGEventGetLocation(cg_event) };
	let raw_delta_y = scroll_delta_y_from_cg_event(cg_event);
	let scroll_phase = scroll_phase_bits_from_cg_event(cg_event);
	let momentum_phase = scroll_momentum_phase_bits_from_cg_event(cg_event);
	let gesture_active =
		scroll_phase_bits_are_active(scroll_phase) || scroll_phase_bits_are_active(momentum_phase);
	let gesture_ended = scroll_phase_bits_are_terminal(scroll_phase)
		|| scroll_phase_bits_are_terminal(momentum_phase);

	decode_scroll_input_from_fields(raw_delta_y, location, gesture_active, gesture_ended)
}

fn decode_scroll_input_from_fields(
	raw_delta_y: f64,
	location: MacOSCGPoint,
	gesture_active: bool,
	gesture_ended: bool,
) -> Option<DecodedScrollInput> {
	if raw_delta_y == 0.0 && !gesture_ended {
		return None;
	}

	Some(DecodedScrollInput {
		raw_delta_y,
		delta_y: raw_delta_y,
		global_x: location.x,
		global_y: location.y,
		gesture_active,
		gesture_ended,
	})
}

fn scroll_delta_y_from_cg_event(cg_event: CGEventRef) -> f64 {
	let is_continuous = unsafe {
		CGEventGetIntegerValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS_FIELD)
	} != 0;

	if is_continuous {
		unsafe {
			CGEventGetDoubleValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1_FIELD)
		}
	} else {
		unsafe { CGEventGetDoubleValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1_FIELD) }
	}
}

fn scroll_phase_bits_from_cg_event(cg_event: CGEventRef) -> u64 {
	unsafe {
		CGEventGetIntegerValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_SCROLL_PHASE_FIELD) as u64
	}
}

fn scroll_momentum_phase_bits_from_cg_event(cg_event: CGEventRef) -> u64 {
	unsafe {
		CGEventGetIntegerValueField(cg_event, KCG_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE_FIELD) as u64
	}
}

fn scroll_phase_bits_are_active(phase_bits: u64) -> bool {
	phase_bits
		& (NSEVENT_PHASE_BEGAN
			| NSEVENT_PHASE_STATIONARY
			| NSEVENT_PHASE_CHANGED
			| NSEVENT_PHASE_MAY_BEGIN)
		!= 0
}

fn scroll_phase_bits_are_terminal(phase_bits: u64) -> bool {
	phase_bits & (NSEVENT_PHASE_ENDED | NSEVENT_PHASE_CANCELLED) != 0
}

fn cg_event_mask_bit(event_type: u32) -> u64 {
	1_u64 << event_type
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
	fn CGEventTapCreate(
		tap: u32,
		place: u32,
		options: u32,
		events_of_interest: u64,
		callback: unsafe extern "C" fn(
			CGEventTapProxy,
			u32,
			CGEventRef,
			*const c_void,
		) -> CGEventRef,
		user_info: *const c_void,
	) -> CFMachPortRef;
	fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
	fn CGEventGetLocation(event: CGEventRef) -> MacOSCGPoint;
	fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
	fn CGEventGetDoubleValueField(event: CGEventRef, field: u32) -> f64;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	static kCFAllocatorDefault: CFAllocatorRef;
	static kCFRunLoopCommonModes: CFRunLoopMode;

	fn CFRunLoopGetCurrent() -> CFRunLoopRef;
	fn CFMachPortCreateRunLoopSource(
		allocator: CFAllocatorRef,
		port: CFMachPortRef,
		order: isize,
	) -> *mut c_void;
	fn CFMachPortInvalidate(port: CFMachPortRef);
	fn CFRunLoopAddSource(run_loop: CFRunLoopRef, source: *mut c_void, mode: CFRunLoopMode);
	fn CFRunLoopRun();
	fn CFRelease(value: *const c_void);
}

#[cfg(test)]
mod tests {
	use super::{
		DecodedScrollInput, MacOSCGPoint, NSEVENT_PHASE_BEGAN, NSEVENT_PHASE_CANCELLED,
		NSEVENT_PHASE_ENDED, NSEVENT_PHASE_MAY_BEGIN, SHARED_SCROLL_INPUT_QUEUE_CAPACITY,
		SharedScrollInputState, decode_scroll_input_from_fields, scroll_phase_bits_are_active,
		scroll_phase_bits_are_terminal,
	};
	use std::time::{Duration, Instant};

	#[test]
	fn decode_scroll_input_ignores_zero_non_terminal_delta() {
		assert_eq!(
			decode_scroll_input_from_fields(0.0, MacOSCGPoint { x: 10.0, y: 20.0 }, false, false),
			None
		);
	}

	#[test]
	fn decode_scroll_input_preserves_terminal_zero_delta() {
		assert_eq!(
			decode_scroll_input_from_fields(0.0, MacOSCGPoint { x: 10.0, y: 20.0 }, false, true),
			Some(DecodedScrollInput {
				raw_delta_y: 0.0,
				delta_y: 0.0,
				global_x: 10.0,
				global_y: 20.0,
				gesture_active: false,
				gesture_ended: true,
			})
		);
	}

	#[test]
	fn phase_bits_classify_active_and_terminal_states() {
		assert!(scroll_phase_bits_are_active(NSEVENT_PHASE_BEGAN));
		assert!(scroll_phase_bits_are_active(NSEVENT_PHASE_MAY_BEGIN));
		assert!(scroll_phase_bits_are_terminal(NSEVENT_PHASE_ENDED));
		assert!(scroll_phase_bits_are_terminal(NSEVENT_PHASE_CANCELLED));
		assert!(!scroll_phase_bits_are_terminal(NSEVENT_PHASE_BEGAN));
	}

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
