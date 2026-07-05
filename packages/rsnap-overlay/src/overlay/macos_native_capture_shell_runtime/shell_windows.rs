use std::ptr;
use std::sync::{Arc, Mutex};

use objc::runtime::Object;
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::overlay::macos_native_capture_shell_runtime::macos_key_focus_shell_runtime;
use crate::overlay::macos_native_capture_shell_runtime::macos_passive_shell_runtime;
use crate::overlay::macos_native_capture_shell_runtime::shell_model;
use crate::overlay::macos_native_capture_shell_runtime::shell_model::{
	MacOSKeyFocusShellState, MacOSKeyFocusShellTarget, MacOSPassiveShellCallback,
	MacOSPassiveToolbarShellState,
};
use crate::overlay::{self, GlobalPoint, MacOSNativeCaptureInputDispatch, MonitorRect, Window};

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

pub(super) struct MacOSPassiveShellWindow {
	window_key: usize,
	view_key: usize,
	tracking_area_key: usize,
	toolbar_state: Option<Arc<Mutex<MacOSPassiveToolbarShellState>>>,
}
impl MacOSPassiveShellWindow {
	pub(super) fn sync_from_render_window(&self, render_window: &Window, visible: bool) {
		let Some(render_ns_window) = macos_overlay_window_ns_window(render_window) else {
			return;
		};
		let Some(frame) = macos_ns_window_frame(render_ns_window) else {
			return;
		};

		self.sync_frame_and_visibility(frame, visible);
	}

	fn sync_frame_and_visibility(&self, frame: NSRect, visible: bool) {
		let ns_window = self.window_key as *mut Object;
		let view = self.view_key as *mut Object;

		if ns_window.is_null() {
			return;
		}

		unsafe {
			let _: () = objc::msg_send![ns_window, setFrame: frame display: objc::runtime::NO];

			if visible {
				let _: () = objc::msg_send![ns_window, orderFrontRegardless];
			} else {
				let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
			}
			if !view.is_null() {
				let _: () = objc::msg_send![ns_window, invalidateCursorRectsForView: view];
			}
		}

		if let Some(callback) = shell_model::macos_shell_callback(self.view_key) {
			match callback {
				MacOSPassiveShellCallback::Overlay { .. } => {
					let seeded_point = visible
						.then(|| {
							macos_passive_shell_runtime::macos_seed_passive_shell_cursor_point(
								ns_window, view,
							)
						})
						.flatten();

					macos_passive_shell_runtime::macos_update_passive_shell_cursor_point(
						self.view_key,
						seeded_point,
					);
				},
				MacOSPassiveShellCallback::Toolbar { .. }
				| MacOSPassiveShellCallback::KeyFocus { .. } => {
					macos_passive_shell_runtime::macos_update_passive_shell_cursor_point(
						self.view_key,
						None,
					);
				},
			}
		}
	}

	pub(super) fn set_toolbar_state(&self, monitor: MonitorRect, outer_position: GlobalPoint) {
		let Some(state) = self.toolbar_state.as_ref() else {
			return;
		};
		let Ok(mut state) = state.lock() else {
			return;
		};

		state.monitor = Some(monitor);
		state.outer_position = Some(outer_position);
	}
}

impl Drop for MacOSPassiveShellWindow {
	fn drop(&mut self) {
		shell_model::macos_unregister_shell_callback(self.view_key);
		macos_passive_shell_runtime::macos_clear_passive_shell_cursor_point(self.view_key);

		unsafe {
			let ns_window = self.window_key as *mut Object;

			if !ns_window.is_null() {
				let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
				let _: () = objc::msg_send![ns_window, close];
				let _: () = objc::msg_send![ns_window, release];
			}

			let tracking_area = self.tracking_area_key as *mut Object;

			if !tracking_area.is_null() {
				let _: () = objc::msg_send![tracking_area, release];
			}

			let view = self.view_key as *mut Object;

			if !view.is_null() {
				let _: () = objc::msg_send![view, release];
			}
		}
	}
}

pub(super) struct MacOSKeyFocusShellWindow {
	window_key: usize,
	view_key: usize,
	state: Arc<Mutex<MacOSKeyFocusShellState>>,
}
impl MacOSKeyFocusShellWindow {
	pub(super) fn sync_from_render_window(
		&self,
		render_window: &Window,
		target: MacOSKeyFocusShellTarget,
		visible: bool,
	) {
		let Some(render_ns_window) = macos_overlay_window_ns_window(render_window) else {
			return;
		};
		let Some(frame) = macos_ns_window_frame(render_ns_window) else {
			return;
		};

		self.set_target(target);
		self.sync_frame_and_visibility(frame, visible);
	}

	fn set_target(&self, target: MacOSKeyFocusShellTarget) {
		let Ok(mut state) = self.state.lock() else {
			return;
		};

		state.target = Some(target);

		let view = self.view_key as *mut Object;

		if view.is_null() {
			return;
		}

		unsafe {
			let input_context: *mut Object = objc::msg_send![view, inputContext];

			if !input_context.is_null() {
				let _: () = objc::msg_send![input_context, invalidateCharacterCoordinates];
			}
		}
	}

	pub(super) fn clear_target(&self) {
		let Ok(mut state) = self.state.lock() else {
			return;
		};

		state.target = None;

		state.marked_text.clear();

		state.forward_key_event_to_app = false;
		state.had_ime_input_during_keydown = false;
	}

	fn sync_frame_and_visibility(&self, frame: NSRect, visible: bool) {
		let ns_window = self.window_key as *mut Object;

		if ns_window.is_null() {
			return;
		}

		unsafe {
			let _: () = objc::msg_send![ns_window, setFrame: frame display: objc::runtime::NO];

			if visible {
				let _: () = objc::msg_send![ns_window, orderFrontRegardless];
			} else {
				let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
			}
		}
	}

	pub(super) fn hide(&self) {
		let ns_window = self.window_key as *mut Object;

		if ns_window.is_null() {
			return;
		}

		unsafe {
			let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
		}
	}

	pub(super) fn ensure_key_focus(&self) {
		let ns_window = self.window_key as *mut Object;
		let view = self.view_key as *mut Object;

		if ns_window.is_null() || view.is_null() {
			return;
		}

		overlay::macos_activate_app();

		unsafe {
			let input_context: *mut Object = objc::msg_send![view, inputContext];

			if !input_context.is_null() {
				let _: () = objc::msg_send![input_context, activate];
				let _: () = objc::msg_send![input_context, invalidateCharacterCoordinates];
			}

			let _: () = objc::msg_send![ns_window, makeFirstResponder: view];
			let _: () = objc::msg_send![ns_window, makeKeyAndOrderFront: ptr::null_mut::<Object>()];
		}
	}
}

impl Drop for MacOSKeyFocusShellWindow {
	fn drop(&mut self) {
		shell_model::macos_unregister_shell_callback(self.view_key);

		unsafe {
			let ns_window = self.window_key as *mut Object;

			if !ns_window.is_null() {
				let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
				let _: () = objc::msg_send![ns_window, close];
				let _: () = objc::msg_send![ns_window, release];
			}

			let view = self.view_key as *mut Object;

			if !view.is_null() {
				let _: () = objc::msg_send![view, release];
			}
		}
	}
}

pub(super) fn macos_create_passive_overlay_shell_window(
	render_window: &Window,
	monitor: MonitorRect,
	dispatch: MacOSNativeCaptureInputDispatch,
) -> Result<MacOSPassiveShellWindow, String> {
	macos_create_passive_shell_window(
		render_window,
		MacOSPassiveShellCallback::Overlay { monitor, dispatch },
		None,
	)
}

pub(super) fn macos_create_passive_toolbar_shell_window(
	render_window: &Window,
	dispatch: MacOSNativeCaptureInputDispatch,
) -> Result<MacOSPassiveShellWindow, String> {
	let toolbar_state = Arc::new(Mutex::new(MacOSPassiveToolbarShellState::default()));

	macos_create_passive_shell_window(
		render_window,
		MacOSPassiveShellCallback::Toolbar { state: Arc::clone(&toolbar_state), dispatch },
		Some(toolbar_state),
	)
}

pub(super) fn macos_create_key_focus_shell_window(
	render_window: &Window,
	dispatch: MacOSNativeCaptureInputDispatch,
) -> Result<MacOSKeyFocusShellWindow, String> {
	let render_ns_window = macos_overlay_window_ns_window(render_window)
		.ok_or_else(|| String::from("Missing macOS render NSWindow for key-focus shell"))?;
	let frame = macos_ns_window_frame(render_ns_window)
		.ok_or_else(|| String::from("Missing macOS render frame for key-focus shell"))?;
	let level = unsafe { macos_ns_window_level(render_ns_window) };
	let collection_behavior = unsafe { macos_ns_window_collection_behavior(render_ns_window) };
	let panel_class = macos_key_focus_shell_runtime::macos_key_focus_shell_panel_class();
	let view_class = macos_key_focus_shell_runtime::macos_key_focus_shell_view_class();
	let borderless_panel_mask: usize = 0;
	let backing_buffered: usize = 2;
	let state = Arc::new(Mutex::new(MacOSKeyFocusShellState::default()));

	unsafe {
		let ns_window_alloc: *mut Object = objc::msg_send![panel_class, alloc];
		let ns_window: *mut Object = objc::msg_send![
			ns_window_alloc,
			initWithContentRect: frame
			styleMask: borderless_panel_mask
			backing: backing_buffered
			defer: objc::runtime::NO
		];

		if ns_window.is_null() {
			return Err(String::from("Failed to create key-focus capture shell panel"));
		}

		let clear: *mut Object = objc::msg_send![objc::class!(NSColor), clearColor];
		let view_alloc: *mut Object = objc::msg_send![view_class, alloc];
		let view_frame =
			NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(frame.size.width, frame.size.height));
		let view: *mut Object = objc::msg_send![view_alloc, initWithFrame: view_frame];

		if view.is_null() {
			let _: () = objc::msg_send![ns_window, release];

			return Err(String::from("Failed to create key-focus capture shell view"));
		}

		let _: () = objc::msg_send![ns_window, setContentView: view];
		let _: () = objc::msg_send![ns_window, setReleasedWhenClosed: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, setOpaque: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, setHasShadow: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let _: () = objc::msg_send![ns_window, setLevel: level];
		let _: () = objc::msg_send![ns_window, setCollectionBehavior: collection_behavior];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, setIgnoresMouseEvents: objc::runtime::YES];
		let _: () = objc::msg_send![ns_window, setHidesOnDeactivate: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];

		shell_model::macos_register_shell_callback(
			view as usize,
			MacOSPassiveShellCallback::KeyFocus { state: Arc::clone(&state), dispatch },
		);

		Ok(MacOSKeyFocusShellWindow {
			window_key: ns_window as usize,
			view_key: view as usize,
			state,
		})
	}
}

fn macos_create_passive_shell_window(
	render_window: &Window,
	callback: MacOSPassiveShellCallback,
	toolbar_state: Option<Arc<Mutex<MacOSPassiveToolbarShellState>>>,
) -> Result<MacOSPassiveShellWindow, String> {
	let render_ns_window = macos_overlay_window_ns_window(render_window)
		.ok_or_else(|| String::from("Missing macOS render NSWindow for passive shell"))?;
	let frame = macos_ns_window_frame(render_ns_window)
		.ok_or_else(|| String::from("Missing macOS render frame for passive shell"))?;
	let level = unsafe { macos_ns_window_level(render_ns_window) };
	let collection_behavior = unsafe { macos_ns_window_collection_behavior(render_ns_window) };
	let panel_class = macos_passive_shell_runtime::macos_passive_shell_panel_class();
	let view_class = macos_passive_shell_runtime::macos_passive_shell_view_class();
	let nonactivating_panel_mask: usize = 1 << 7;
	let backing_buffered: usize = 2;

	unsafe {
		let ns_window_alloc: *mut Object = objc::msg_send![panel_class, alloc];
		let ns_window: *mut Object = objc::msg_send![
			ns_window_alloc,
			initWithContentRect: frame
			styleMask: nonactivating_panel_mask
			backing: backing_buffered
			defer: objc::runtime::NO
		];

		if ns_window.is_null() {
			return Err(String::from("Failed to create passive capture shell panel"));
		}

		let clear: *mut Object = objc::msg_send![objc::class!(NSColor), clearColor];
		let view_alloc: *mut Object = objc::msg_send![view_class, alloc];
		let view_frame =
			NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(frame.size.width, frame.size.height));
		let view: *mut Object = objc::msg_send![view_alloc, initWithFrame: view_frame];

		if view.is_null() {
			let _: () = objc::msg_send![ns_window, release];

			return Err(String::from("Failed to create passive capture shell view"));
		}

		let tracking_area_alloc: *mut Object = objc::msg_send![objc::class!(NSTrackingArea), alloc];
		let tracking_options: usize = 0x01 | 0x02 | 0x04 | 0x80 | 0x200 | 0x400;
		let tracking_rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
		let tracking_area: *mut Object = objc::msg_send![
			tracking_area_alloc,
			initWithRect: tracking_rect
			options: tracking_options
			owner: view
			userInfo: ptr::null_mut::<Object>()
		];

		if tracking_area.is_null() {
			let _: () = objc::msg_send![view, release];
			let _: () = objc::msg_send![ns_window, release];

			return Err(String::from("Failed to create passive capture shell tracking area"));
		}

		let _: () = objc::msg_send![view, addTrackingArea: tracking_area];
		let _: () = objc::msg_send![ns_window, setContentView: view];
		let _: () = objc::msg_send![ns_window, setReleasedWhenClosed: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, setOpaque: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, setHasShadow: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let _: () = objc::msg_send![ns_window, setLevel: level];
		let _: () = objc::msg_send![ns_window, setCollectionBehavior: collection_behavior];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: objc::runtime::YES];
		let _: () = objc::msg_send![ns_window, setIgnoresMouseEvents: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, setHidesOnDeactivate: objc::runtime::NO];
		let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];

		shell_model::macos_register_shell_callback(view as usize, callback);

		Ok(MacOSPassiveShellWindow {
			window_key: ns_window as usize,
			view_key: view as usize,
			tracking_area_key: tracking_area as usize,
			toolbar_state,
		})
	}
}

fn macos_overlay_window_ns_window(window: &Window) -> Option<*mut Object> {
	let ns_view = overlay::macos_overlay_window_ns_view(window)?;

	unsafe {
		let ns_window: *mut Object = objc::msg_send![ns_view, window];

		(!ns_window.is_null()).then_some(ns_window)
	}
}

fn macos_ns_window_frame(ns_window: *mut Object) -> Option<NSRect> {
	if ns_window.is_null() {
		return None;
	}

	unsafe {
		let frame: NSRect = objc::msg_send![ns_window, frame];

		Some(frame)
	}
}

unsafe fn macos_ns_window_level(ns_window: *mut Object) -> i64 {
	let level: i64 = objc::msg_send![ns_window, level];

	level
}

unsafe fn macos_ns_window_collection_behavior(ns_window: *mut Object) -> usize {
	let behavior: usize = objc::msg_send![ns_window, collectionBehavior];

	behavior
}
