use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, atomic::Ordering};
use std::thread::{self, JoinHandle};

use crate::app::scroll_input_macos::decode;
use crate::app::scroll_input_macos::state::SharedScrollInputState;
use crate::app::scroll_input_macos::{
	CFMachPortCreateRunLoopSource, CFMachPortInvalidate, CFRelease, CFRunLoopAddSource,
	CFRunLoopGetCurrent, CFRunLoopRun, CGEventRef, CGEventTapCreate, CGEventTapEnable,
	CGEventTapProxy, kCFAllocatorDefault, kCFRunLoopCommonModes,
};

const KCG_EVENT_SCROLL_WHEEL: u32 = 22;
const KCG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const KCG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const KCG_SESSION_EVENT_TAP: u32 = 1;
const KCG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const KCG_EVENT_TAP_LISTEN_ONLY: u32 = 1;

struct ScrollInputTapContext {
	shared_state: Arc<SharedScrollInputState>,
	tap: std::sync::atomic::AtomicPtr<c_void>,
}

pub(in crate::app) fn spawn_scroll_input_observer(
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
		tap: std::sync::atomic::AtomicPtr::new(ptr::null_mut()),
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

	let Some(decoded) = decode::decode_scroll_input_from_cg_event(cg_event) else {
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

fn cg_event_mask_bit(event_type: u32) -> u64 {
	1_u64 << event_type
}
