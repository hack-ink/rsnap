use std::ffi::c_void;
use std::process;
use std::ptr;
use std::sync::Arc;

use color_eyre::eyre::{self, Result};
use objc::runtime::{BOOL, NO, Object, YES};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::overlay::MacOSNativeCaptureInputEvent;
use crate::overlay::session_state::MacOSScrollWheelEvent;
use crate::state::GlobalPoint;

macro_rules! sel {
	($($tt:tt)*) => {
		objc::sel!($($tt)*)
	};
}

macro_rules! sel_impl {
	($($tt:tt)*) => {
		objc::sel_impl!($($tt)*)
	};
}

type CFTypeRef = *const c_void;
type CGEventRef = *mut c_void;

const KCG_HID_EVENT_TAP: u32 = 0;
const KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: u32 = 0;
const MACOS_HUD_WINDOW_LEVEL: isize = 26;
const MACOS_OVERLAY_WINDOW_LEVEL: isize = 25;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::overlay) struct MacOSFrontmostApplication {
	pub(in crate::overlay) process_id: i32,
}

#[derive(Clone)]
pub(in crate::overlay) struct MacOSNativeCaptureInputDispatch {
	pub(in crate::overlay) sink: Arc<dyn Fn(MacOSNativeCaptureInputEvent) + Send + Sync>,
}
impl MacOSNativeCaptureInputDispatch {
	pub(in crate::overlay) fn enqueue(&self, event: MacOSNativeCaptureInputEvent) {
		(self.sink)(event);
	}
}

#[repr(C)]
struct MacOSCGPoint {
	x: f64,
	y: f64,
}

pub(in crate::overlay) fn macos_hid_event_source_state_id() -> u32 {
	KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
	fn CGEventGetLocation(event: CGEventRef) -> MacOSCGPoint;
	fn CGEventCreate(source: *const c_void) -> CGEventRef;
	fn CGEventSourceCreate(source_state_id: u32) -> CFTypeRef;
	fn CGEventCreateScrollWheelEvent2(
		source: *const c_void,
		units: u32,
		wheel_count: u32,
		wheel1: i32,
		wheel2: i32,
		wheel3: i32,
	) -> CGEventRef;
	fn CGEventPost(tap_location: u32, event: CGEventRef);
	fn CGEventSetLocation(event: CGEventRef, location: MacOSCGPoint);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	fn CFRelease(obj: CFTypeRef);
}

pub(in crate::overlay) fn macos_mouse_location() -> Option<GlobalPoint> {
	let event = unsafe { CGEventCreate(ptr::null()) };

	if event.is_null() {
		return None;
	}

	let point = unsafe { CGEventGetLocation(event) };

	unsafe { CFRelease(event) };

	Some(GlobalPoint::new(point.x as i32, point.y as i32))
}

pub(in crate::overlay) fn macos_overlay_window_ns_view(window: &Window) -> Option<*mut Object> {
	let Ok(handle) = window.window_handle() else {
		return None;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return None;
	};

	Some(appkit.ns_view.as_ptr().cast::<Object>())
}

pub(in crate::overlay) fn macos_activate_app() {
	unsafe {
		let app: *mut Object = objc::msg_send![objc::class!(NSApplication), sharedApplication];

		if app.is_null() {
			return;
		}

		let _: () = objc::msg_send![app, activateIgnoringOtherApps: YES];
	}
}

pub(in crate::overlay) fn macos_frontmost_application() -> Option<MacOSFrontmostApplication> {
	unsafe {
		let workspace: *mut Object = objc::msg_send![objc::class!(NSWorkspace), sharedWorkspace];

		if workspace.is_null() {
			return None;
		}

		let app: *mut Object = objc::msg_send![workspace, frontmostApplication];

		if app.is_null() {
			return None;
		}

		let process_id: i32 = objc::msg_send![app, processIdentifier];

		(process_id > 0).then_some(MacOSFrontmostApplication { process_id })
	}
}

pub(in crate::overlay) fn macos_restore_frontmost_application(
	target: MacOSFrontmostApplication,
) -> bool {
	if target.process_id == process::id() as i32 {
		macos_activate_app();

		return true;
	}

	unsafe {
		let running_application_class = objc::class!(NSRunningApplication);
		let app: *mut Object = objc::msg_send![
			running_application_class,
			runningApplicationWithProcessIdentifier: target.process_id
		];

		if app.is_null() {
			return false;
		}

		let options: usize = 1 << 1;
		let activated: BOOL = objc::msg_send![app, activateWithOptions: options];

		activated == YES
	}
}

pub(in crate::overlay) fn macos_post_scroll_wheel_event(
	delta: MacOSScrollWheelEvent,
	target_point: GlobalPoint,
) -> Result<()> {
	let units = delta.units;
	let wheel1 = delta.posted_y;
	let wheel2 = delta.posted_x;

	if wheel1 == 0 && wheel2 == 0 {
		return Ok(());
	}

	let source = unsafe { CGEventSourceCreate(macos_hid_event_source_state_id()) };

	if source.is_null() {
		return Err(eyre::eyre!("failed to create macOS scroll wheel event source"));
	}

	let wheel_count = if wheel2 != 0 { 2 } else { 1 };
	let event =
		unsafe { CGEventCreateScrollWheelEvent2(source, units, wheel_count, wheel1, wheel2, 0) };

	if event.is_null() {
		unsafe {
			CFRelease(source);
		}

		return Err(eyre::eyre!("failed to create macOS scroll wheel event"));
	}

	unsafe {
		CGEventSetLocation(
			event,
			MacOSCGPoint { x: f64::from(target_point.x), y: f64::from(target_point.y) },
		);
		CGEventPost(KCG_HID_EVENT_TAP, event);
		CFRelease(event);
		CFRelease(source);
	}

	Ok(())
}

pub(in crate::overlay) fn macos_configure_overlay_window_mouse_moved_events(window: &Window) {
	let Ok(handle) = window.window_handle() else {
		return;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return;
	};
	let ns_view = appkit.ns_view.as_ptr().cast::<Object>();

	unsafe {
		let ns_window: *mut Object = objc::msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		macos_configure_nonactivating_capture_window_with_ns_window(ns_window);

		let _: () = objc::msg_send![ns_window, setOpaque: false];
		let _: () = objc::msg_send![ns_window, setHasShadow: false];
		let clear: *mut Object = objc::msg_send![objc::class!(NSColor), clearColor];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let _: () = objc::msg_send![ns_window, setLevel: MACOS_OVERLAY_WINDOW_LEVEL];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: YES];
	}
}

pub(in crate::overlay) fn macos_configure_hud_window(
	window: &Window,
	blur_enabled: bool,
	blur_amount: f32,
	corner_radius_points: Option<f64>,
) {
	let Ok(handle) = window.window_handle() else {
		return;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return;
	};
	let ns_view = appkit.ns_view.as_ptr().cast::<Object>();

	unsafe {
		let ns_window: *mut Object = objc::msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		macos_configure_nonactivating_capture_window_with_ns_window(ns_window);

		// winit exposes blur as a boolean. We also set an explicit radius so we can drive it from
		// settings (this uses the same private CGS API that winit uses internally).
		{
			#[link(name = "CoreGraphics", kind = "framework")]
			unsafe extern "C" {
				fn CGSMainConnectionID() -> *mut c_void;

				fn CGSSetWindowBackgroundBlurRadius(
					connection_id: *mut c_void,
					window_id: isize,
					radius: i64,
				) -> i32;
			}

			let amount = blur_amount.clamp(0.0, 1.0);
			let radius = if blur_enabled {
				// Map the slider linearly (0..=1) to the native blur radius.
				// Keep the upper bound conservative; CGS blur radius gets strong quickly.
				let max_radius = 12.0;

				(amount * max_radius).round().clamp(0.0, 200.0) as i64
			} else {
				0
			};
			let window_number: isize = objc::msg_send![ns_window, windowNumber];
			let _ = CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), window_number, radius);
		}

		let _: () = objc::msg_send![ns_window, setOpaque: false];
		let _: () = objc::msg_send![ns_window, setHasShadow: false];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: YES];
		let _: () = objc::msg_send![ns_window, setLevel: MACOS_HUD_WINDOW_LEVEL];
		let clear: *mut Object = objc::msg_send![objc::class!(NSColor), clearColor];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let content_view: *mut Object = objc::msg_send![ns_window, contentView];

		if content_view.is_null() {
			return;
		}

		let _: () = objc::msg_send![content_view, setWantsLayer: YES];
		let layer: *mut Object = objc::msg_send![content_view, layer];

		if layer.is_null() {
			return;
		}

		// Round the window itself so native blur doesn't show a rectangular boundary.
		let scale = window.scale_factor().max(1.0);
		let size = window.inner_size();
		let height_points = (size.height as f64) / scale;
		let radius = corner_radius_points.unwrap_or(height_points * 0.5);
		let _: () = objc::msg_send![layer, setCornerRadius: radius];
		let _: () = objc::msg_send![layer, setMasksToBounds: YES];
	}
}

pub(in crate::overlay) fn macos_configure_nonactivating_capture_window_with_ns_window(
	ns_window: *mut Object,
) {
	if ns_window.is_null() {
		return;
	}

	unsafe {
		let style_mask: usize = objc::msg_send![ns_window, styleMask];
		let nonactivating_panel_mask: usize = 1 << 7;
		let _: () = objc::msg_send![ns_window, setStyleMask: style_mask | nonactivating_panel_mask];
		let _: () = objc::msg_send![ns_window, setHidesOnDeactivate: false];
	}
}

pub(in crate::overlay) fn macos_set_capture_window_mouse_passthrough(
	window: &Window,
	passthrough: bool,
) {
	let Some(ns_view) = macos_overlay_window_ns_view(window) else {
		return;
	};

	unsafe {
		let ns_window: *mut Object = objc::msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		let ignores_mouse_events = if passthrough { YES } else { NO };
		let _: () = objc::msg_send![ns_window, setIgnoresMouseEvents: ignores_mouse_events];
	}
}
