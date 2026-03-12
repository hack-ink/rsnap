mod decode;
mod state;
mod tap;

pub(super) use self::state::SharedScrollInputState;
pub(super) use self::tap::spawn_scroll_input_observer;

use std::ffi::c_void;

use crate::app::scroll_input_macos::decode::MacOSCGPoint;

type CFMachPortRef = *mut c_void;

type CFRunLoopRef = *mut c_void;

type CFRunLoopMode = *const c_void;

type CFAllocatorRef = *const c_void;

type CGEventRef = *const c_void;

type CGEventTapProxy = *const c_void;

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
