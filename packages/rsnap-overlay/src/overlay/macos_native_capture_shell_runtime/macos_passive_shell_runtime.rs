use std::collections::HashMap;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use objc::declare::ClassDecl;
use objc::runtime::{BOOL, Class, NO, Object, Sel, YES};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use winit::event::{ElementState, MouseButton};
use winit::window::CursorIcon;

use crate::overlay::MacOSNativeCaptureScrollDelta;
use crate::overlay::macos_cursor_runtime::{self, MacOSOverlayPoint};
use crate::overlay::macos_native_capture_shell_runtime::shell_model;
use crate::overlay::macos_native_capture_shell_runtime::shell_model::{
	MacOSPassiveShellCallback, MacOSRect,
};

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

pub(super) fn macos_passive_shell_panel_class() -> *const Class {
	macos_passive_shell_panel_class_impl()
}

pub(super) fn macos_passive_shell_view_class() -> *const Class {
	macos_passive_shell_view_class_impl()
}

pub(super) fn macos_set_crosshair_cursor() {
	macos_cursor_runtime::macos_set_cursor_icon(CursorIcon::Crosshair);
}

pub(super) fn macos_clear_passive_shell_cursor_point(view_key: usize) {
	macos_clear_passive_shell_cursor_point_impl(view_key);
}

pub(super) fn macos_update_passive_shell_cursor_point(
	view_key: usize,
	next_point: Option<NSPoint>,
) {
	macos_update_passive_shell_cursor_point_impl(view_key, next_point);
}

pub(super) fn macos_seed_passive_shell_cursor_point(
	ns_window: *mut Object,
	view: *mut Object,
) -> Option<NSPoint> {
	macos_seed_passive_shell_cursor_point_impl(ns_window, view)
}

fn macos_passive_shell_panel_class_impl() -> *const Class {
	static CLASS: OnceLock<usize> = OnceLock::new();

	(*CLASS.get_or_init(|| {
		if let Some(class) = Class::get("RsnapPassiveCaptureShellPanel") {
			return class as *const Class as usize;
		}

		let superclass = objc::class!(NSPanel);
		let mut decl = ClassDecl::new("RsnapPassiveCaptureShellPanel", superclass)
			.expect("passive capture shell panel class");

		unsafe {
			decl.add_method(
				objc::sel!(canBecomeMainWindow),
				macos_passive_shell_can_become_main_window as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(canBecomeKeyWindow),
				macos_passive_shell_can_become_key_window as extern "C" fn(&Object, Sel) -> BOOL,
			);
		}

		decl.register() as *const Class as usize
	})) as *const Class
}

fn macos_passive_shell_view_class_impl() -> *const Class {
	static CLASS: OnceLock<usize> = OnceLock::new();

	(*CLASS.get_or_init(|| {
		if let Some(class) = Class::get("RsnapPassiveCaptureShellView") {
			return class as *const Class as usize;
		}

		let superclass = objc::class!(NSView);
		let mut decl = ClassDecl::new("RsnapPassiveCaptureShellView", superclass)
			.expect("passive capture shell view class");

		unsafe {
			decl.add_method(
				objc::sel!(isFlipped),
				super::macos_capture_shell_view_is_flipped as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(acceptsFirstMouse:),
				macos_passive_shell_view_accepts_first_mouse
					as extern "C" fn(&Object, Sel, *mut Object) -> BOOL,
			);
			decl.add_method(
				objc::sel!(hitTest:),
				macos_passive_shell_view_hit_test
					as extern "C" fn(&Object, Sel, MacOSOverlayPoint) -> *mut Object,
			);
			decl.add_method(
				objc::sel!(drawRect:),
				macos_passive_shell_view_draw_rect as extern "C" fn(&Object, Sel, MacOSRect),
			);
			decl.add_method(
				objc::sel!(resetCursorRects),
				macos_passive_shell_view_reset_cursor_rects as extern "C" fn(&Object, Sel),
			);
			decl.add_method(
				objc::sel!(mouseMoved:),
				macos_passive_shell_view_mouse_moved as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(cursorUpdate:),
				macos_passive_shell_view_cursor_update as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(mouseDragged:),
				macos_passive_shell_view_mouse_dragged as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(mouseDown:),
				macos_passive_shell_view_mouse_down as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(mouseUp:),
				macos_passive_shell_view_mouse_up as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(rightMouseDown:),
				macos_passive_shell_view_right_mouse_down
					as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(mouseExited:),
				macos_passive_shell_view_mouse_exited as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(scrollWheel:),
				macos_passive_shell_view_scroll_wheel as extern "C" fn(&Object, Sel, *mut Object),
			);
		}

		decl.register() as *const Class as usize
	})) as *const Class
}

extern "C" fn macos_passive_shell_can_become_main_window(_this: &Object, _cmd: Sel) -> BOOL {
	NO
}

extern "C" fn macos_passive_shell_can_become_key_window(_this: &Object, _cmd: Sel) -> BOOL {
	NO
}

extern "C" fn macos_passive_shell_view_accepts_first_mouse(
	_this: &Object,
	_cmd: Sel,
	_event: *mut Object,
) -> BOOL {
	YES
}

extern "C" fn macos_passive_shell_view_hit_test(
	this: &Object,
	_cmd: Sel,
	_point: MacOSOverlayPoint,
) -> *mut Object {
	this as *const Object as *mut Object
}

extern "C" fn macos_passive_shell_view_draw_rect(this: &Object, _cmd: Sel, dirty_rect: MacOSRect) {
	let Some(callback) = shell_model::macos_shell_callback(this as *const Object as usize) else {
		return;
	};

	if !matches!(callback, MacOSPassiveShellCallback::Overlay { .. }) {
		return;
	}

	let clear: *mut Object = unsafe { objc::msg_send![objc::class!(NSColor), clearColor] };

	if !clear.is_null() {
		unsafe {
			let _: () = objc::msg_send![clear, setFill];
			let _: () =
				objc::msg_send![objc::class!(NSBezierPath), fillRect: NSRect::from(dirty_rect)];
		}
	}
}

extern "C" fn macos_passive_shell_view_reset_cursor_rects(this: &Object, _cmd: Sel) {
	let bounds: NSRect = unsafe { objc::msg_send![this, bounds] };
	let Some(callback) = shell_model::macos_shell_callback(this as *const Object as usize) else {
		return;
	};
	let cursor = match callback {
		MacOSPassiveShellCallback::Overlay { .. } => {
			macos_cursor_runtime::macos_cursor_object_for_icon(CursorIcon::Crosshair)
		},
		MacOSPassiveShellCallback::Toolbar { .. } | MacOSPassiveShellCallback::KeyFocus { .. } => {
			macos_cursor_runtime::macos_cursor_object_for_icon(CursorIcon::Default)
		},
	};

	if cursor.is_null() {
		return;
	}

	unsafe {
		let _: () = objc::msg_send![this, addCursorRect: bounds cursor: cursor];
	}
}

extern "C" fn macos_passive_shell_view_mouse_moved(this: &Object, _cmd: Sel, event: *mut Object) {
	macos_dispatch_shell_pointer_moved(this, event);
}

extern "C" fn macos_passive_shell_view_cursor_update(
	this: &Object,
	_cmd: Sel,
	_event: *mut Object,
) {
	let Some(callback) = shell_model::macos_shell_callback(this as *const Object as usize) else {
		return;
	};

	tracing::trace!(
		op = "overlay.macos_passive_shell_cursor_update",
		callback = %shell_model::macos_shell_callback_name(&callback),
		"Passive shell received cursorUpdate."
	);

	match callback {
		MacOSPassiveShellCallback::Overlay { .. } => {
			macos_cursor_runtime::macos_set_cursor_icon(CursorIcon::Crosshair);
		},
		MacOSPassiveShellCallback::Toolbar { .. } => {
			macos_cursor_runtime::macos_set_cursor_icon(CursorIcon::Default);
		},
		MacOSPassiveShellCallback::KeyFocus { .. } => {},
	}
}

extern "C" fn macos_passive_shell_view_mouse_dragged(this: &Object, _cmd: Sel, event: *mut Object) {
	macos_dispatch_shell_pointer_moved(this, event);
}

extern "C" fn macos_passive_shell_view_mouse_down(this: &Object, _cmd: Sel, event: *mut Object) {
	macos_dispatch_shell_mouse_input(this, event, MouseButton::Left, ElementState::Pressed);
}

extern "C" fn macos_passive_shell_view_mouse_up(this: &Object, _cmd: Sel, event: *mut Object) {
	macos_dispatch_shell_mouse_input(this, event, MouseButton::Left, ElementState::Released);
}

extern "C" fn macos_passive_shell_view_right_mouse_down(
	this: &Object,
	_cmd: Sel,
	event: *mut Object,
) {
	macos_dispatch_shell_mouse_input(this, event, MouseButton::Right, ElementState::Pressed);
}

extern "C" fn macos_passive_shell_view_mouse_exited(this: &Object, _cmd: Sel, _event: *mut Object) {
	let view_key = this as *const Object as usize;
	let Some(callback) = shell_model::macos_shell_callback(this as *const Object as usize) else {
		return;
	};

	callback.dispatch_mouse_exited(view_key);
}

extern "C" fn macos_passive_shell_view_scroll_wheel(this: &Object, _cmd: Sel, event: *mut Object) {
	let Some(callback) = shell_model::macos_shell_callback(this as *const Object as usize) else {
		return;
	};
	let Some(delta) = macos_shell_scroll_delta(event) else {
		return;
	};

	callback.dispatch_scroll_wheel(delta);
}

fn macos_dispatch_shell_pointer_moved(this: &Object, event: *mut Object) {
	let Some(callback) = shell_model::macos_shell_callback(this as *const Object as usize) else {
		return;
	};
	let Some(local_point) = macos_shell_local_point(this, event) else {
		return;
	};

	tracing::trace!(
		op = "overlay.macos_passive_shell_pointer_moved",
		callback = %shell_model::macos_shell_callback_name(&callback),
		x = local_point.x,
		y = local_point.y,
		"Passive shell received pointer movement."
	);

	match callback {
		MacOSPassiveShellCallback::Overlay { .. } => {
			macos_update_passive_shell_cursor_point(
				this as *const Object as usize,
				Some(local_point),
			);

			macos_cursor_runtime::macos_set_cursor_icon(CursorIcon::Crosshair);
		},
		MacOSPassiveShellCallback::Toolbar { .. } => {
			macos_update_passive_shell_cursor_point(this as *const Object as usize, None);

			macos_cursor_runtime::macos_set_cursor_icon(CursorIcon::Default);
		},
		MacOSPassiveShellCallback::KeyFocus { .. } => {
			macos_update_passive_shell_cursor_point(this as *const Object as usize, None);
		},
	}

	callback.dispatch_pointer_moved(local_point);
}

fn macos_dispatch_shell_mouse_input(
	this: &Object,
	event: *mut Object,
	button: MouseButton,
	state: ElementState,
) {
	let Some(callback) = shell_model::macos_shell_callback(this as *const Object as usize) else {
		return;
	};
	let local_point = macos_shell_local_point(this, event);

	match callback {
		MacOSPassiveShellCallback::Overlay { .. } => {
			macos_update_passive_shell_cursor_point(this as *const Object as usize, local_point);

			macos_cursor_runtime::macos_set_cursor_icon(CursorIcon::Crosshair);
		},
		MacOSPassiveShellCallback::Toolbar { .. } => {
			macos_update_passive_shell_cursor_point(this as *const Object as usize, None);

			macos_cursor_runtime::macos_set_cursor_icon(CursorIcon::Default);
		},
		MacOSPassiveShellCallback::KeyFocus { .. } => {
			macos_update_passive_shell_cursor_point(this as *const Object as usize, None);
		},
	}

	callback.dispatch_mouse_input(local_point, button, state);
}

fn macos_shell_local_point(this: &Object, event: *mut Object) -> Option<NSPoint> {
	if event.is_null() {
		return None;
	}

	unsafe {
		let window_point: NSPoint = objc::msg_send![event, locationInWindow];
		let local_point: NSPoint =
			objc::msg_send![this, convertPoint: window_point fromView: ptr::null_mut::<Object>()];

		Some(local_point)
	}
}

fn macos_shell_scroll_delta(event: *mut Object) -> Option<MacOSNativeCaptureScrollDelta> {
	if event.is_null() {
		return None;
	}

	unsafe {
		let precise: BOOL = objc::msg_send![event, hasPreciseScrollingDeltas];
		let delta_x: f64 = if precise == YES {
			objc::msg_send![event, scrollingDeltaX]
		} else {
			objc::msg_send![event, deltaX]
		};
		let delta_y: f64 = if precise == YES {
			objc::msg_send![event, scrollingDeltaY]
		} else {
			objc::msg_send![event, deltaY]
		};

		if precise == YES {
			Some(MacOSNativeCaptureScrollDelta::Pixel { x: delta_x, y: delta_y })
		} else {
			Some(MacOSNativeCaptureScrollDelta::Line { x: delta_x as f32, y: delta_y as f32 })
		}
	}
}

fn macos_passive_shell_cursor_points() -> &'static Mutex<HashMap<usize, Option<NSPoint>>> {
	static POINTS: OnceLock<Mutex<HashMap<usize, Option<NSPoint>>>> = OnceLock::new();

	POINTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn macos_clear_passive_shell_cursor_point_impl(view_key: usize) {
	let points = macos_passive_shell_cursor_points();

	match points.lock() {
		Ok(mut guard) => {
			guard.remove(&view_key);
		},
		Err(poisoned) => {
			poisoned.into_inner().remove(&view_key);
		},
	}
}

fn macos_update_passive_shell_cursor_point_impl(view_key: usize, next_point: Option<NSPoint>) {
	let previous = {
		let points = macos_passive_shell_cursor_points();

		match points.lock() {
			Ok(mut guard) => {
				let previous = guard.get(&view_key).copied().flatten();

				if previous == next_point {
					return;
				}
				if next_point.is_some() {
					guard.insert(view_key, next_point);
				} else {
					guard.remove(&view_key);
				}

				previous
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();
				let previous = guard.get(&view_key).copied().flatten();

				if previous == next_point {
					return;
				}
				if next_point.is_some() {
					guard.insert(view_key, next_point);
				} else {
					guard.remove(&view_key);
				}

				previous
			},
		}
	};
	let view = view_key as *mut Object;

	if view.is_null() {
		return;
	}

	unsafe {
		if let Some(previous) = previous {
			let _: () = objc::msg_send![
				view,
				setNeedsDisplayInRect: macos_passive_shell_cursor_dirty_rect(previous)
			];
		}
		if let Some(next) = next_point {
			let _: () = objc::msg_send![view, setNeedsDisplayInRect: macos_passive_shell_cursor_dirty_rect(next)];
		}
	}
}

fn macos_passive_shell_cursor_dirty_rect(point: NSPoint) -> NSRect {
	const CURSOR_DIRTY_RADIUS: f64 = 18.0;

	NSRect::new(
		NSPoint::new(point.x - CURSOR_DIRTY_RADIUS, point.y - CURSOR_DIRTY_RADIUS),
		NSSize::new(CURSOR_DIRTY_RADIUS * 2.0, CURSOR_DIRTY_RADIUS * 2.0),
	)
}

fn macos_seed_passive_shell_cursor_point_impl(
	ns_window: *mut Object,
	view: *mut Object,
) -> Option<NSPoint> {
	if ns_window.is_null() || view.is_null() {
		return None;
	}

	let global = shell_model::macos_capture_shell_mouse_location()?;

	unsafe {
		let screen_point = NSPoint::new(f64::from(global.x), f64::from(global.y));
		let window_point: NSPoint =
			objc::msg_send![ns_window, convertPointFromScreen: screen_point];
		let local_point: NSPoint =
			objc::msg_send![view, convertPoint: window_point fromView: ptr::null_mut::<Object>()];
		let bounds: NSRect = objc::msg_send![view, bounds];
		let contains = local_point.x >= bounds.origin.x
			&& local_point.y >= bounds.origin.y
			&& local_point.x <= bounds.origin.x + bounds.size.width
			&& local_point.y <= bounds.origin.y + bounds.size.height;

		contains.then_some(local_point)
	}
}
