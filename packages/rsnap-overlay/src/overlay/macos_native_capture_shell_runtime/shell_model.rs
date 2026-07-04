use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use objc::{Encode, Encoding};
use objc2_foundation::{NSPoint, NSRange, NSRect, NSSize};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::ModifiersState;

use crate::overlay;
use crate::overlay::macos_cursor_runtime::MacOSOverlayPoint;
use crate::overlay::macos_native_capture_shell_runtime::macos_passive_shell_runtime;
use crate::overlay::{
	GlobalPoint, MacOSNativeCaptureInputDispatch, MacOSNativeCaptureInputEvent,
	MacOSNativeCaptureScrollDelta, MonitorRect, Pos2,
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MacOSRange {
	pub(super) location: usize,
	pub(super) length: usize,
}
unsafe impl Encode for MacOSRange {
	fn encode() -> Encoding {
		unsafe { Encoding::from_str("{_NSRange=QQ}") }
	}
}

impl From<NSRange> for MacOSRange {
	fn from(value: NSRange) -> Self {
		Self { location: value.location, length: value.length }
	}
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MacOSSize {
	pub(super) width: f64,
	pub(super) height: f64,
}
unsafe impl Encode for MacOSSize {
	fn encode() -> Encoding {
		unsafe { Encoding::from_str("{CGSize=dd}") }
	}
}

impl From<NSSize> for MacOSSize {
	fn from(value: NSSize) -> Self {
		Self { width: value.width, height: value.height }
	}
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct MacOSRect {
	pub(super) origin: MacOSOverlayPoint,
	pub(super) size: MacOSSize,
}
unsafe impl Encode for MacOSRect {
	fn encode() -> Encoding {
		unsafe { Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
	}
}

impl From<NSRect> for MacOSRect {
	fn from(value: NSRect) -> Self {
		Self {
			origin: MacOSOverlayPoint { x: value.origin.x, y: value.origin.y },
			size: MacOSSize::from(value.size),
		}
	}
}

#[derive(Clone, Copy, Default)]
pub(super) struct MacOSPassiveToolbarShellState {
	pub(super) monitor: Option<MonitorRect>,
	pub(super) outer_position: Option<GlobalPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MacOSKeyFocusShellTarget {
	pub(super) kind: MacOSKeyFocusShellKind,
	pub(super) monitor: Option<MonitorRect>,
	pub(super) ime_allowed: bool,
	pub(super) ime_origin: NSPoint,
	pub(super) ime_size: NSSize,
}

#[derive(Debug)]
pub(super) struct MacOSKeyFocusShellState {
	pub(super) target: Option<MacOSKeyFocusShellTarget>,
	pub(super) keyboard_modifiers: ModifiersState,
	pub(super) marked_text: String,
	pub(super) forward_key_event_to_app: bool,
	pub(super) had_ime_input_during_keydown: bool,
}
impl Default for MacOSKeyFocusShellState {
	fn default() -> Self {
		Self {
			target: None,
			keyboard_modifiers: ModifiersState::empty(),
			marked_text: String::new(),
			forward_key_event_to_app: false,
			had_ime_input_during_keydown: false,
		}
	}
}

impl From<MacOSRange> for NSRange {
	fn from(value: MacOSRange) -> Self {
		Self::new(value.location, value.length)
	}
}

impl From<MacOSSize> for NSSize {
	fn from(value: MacOSSize) -> Self {
		Self::new(value.width, value.height)
	}
}

impl From<MacOSRect> for NSRect {
	fn from(value: MacOSRect) -> Self {
		Self::new(NSPoint::new(value.origin.x, value.origin.y), NSSize::from(value.size))
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MacOSKeyFocusShellKind {
	FrozenText,
	ScrollCapture,
}

#[derive(Clone)]
pub(super) enum MacOSPassiveShellCallback {
	Overlay {
		monitor: MonitorRect,
		dispatch: MacOSNativeCaptureInputDispatch,
	},
	Toolbar {
		state: Arc<Mutex<MacOSPassiveToolbarShellState>>,
		dispatch: MacOSNativeCaptureInputDispatch,
	},
	KeyFocus {
		state: Arc<Mutex<MacOSKeyFocusShellState>>,
		dispatch: MacOSNativeCaptureInputDispatch,
	},
}
impl MacOSPassiveShellCallback {
	pub(super) fn dispatch_pointer_moved(&self, local_point: NSPoint) {
		match self {
			Self::Overlay { monitor, dispatch } => {
				dispatch.enqueue(MacOSNativeCaptureInputEvent::OverlayPointerMoved {
					monitor: *monitor,
					global: clamp_monitor_local_point(*monitor, local_point),
				});
			},
			Self::Toolbar { state, dispatch } => {
				let Ok(state) = state.lock() else {
					return;
				};
				let (Some(monitor), Some(outer_position)) = (state.monitor, state.outer_position)
				else {
					return;
				};
				let local = Pos2::new(local_point.x as f32, local_point.y as f32);
				let global = GlobalPoint::new(
					outer_position.x.saturating_add(local.x.round() as i32),
					outer_position.y.saturating_add(local.y.round() as i32),
				);

				dispatch.enqueue(MacOSNativeCaptureInputEvent::ToolbarPointerMoved {
					monitor,
					local,
					global,
					outer_position,
				});
			},
			Self::KeyFocus { .. } => {},
		}
	}

	pub(super) fn dispatch_mouse_input(
		&self,
		local_point: Option<NSPoint>,
		button: MouseButton,
		state: ElementState,
	) {
		match self {
			Self::Overlay { monitor, dispatch } => {
				let Some(local_point) = local_point else {
					return;
				};

				dispatch.enqueue(MacOSNativeCaptureInputEvent::OverlayMouseInput {
					monitor: *monitor,
					global: clamp_monitor_local_point(*monitor, local_point),
					button,
					state,
				});
			},
			Self::Toolbar { dispatch, .. } => {
				dispatch.enqueue(MacOSNativeCaptureInputEvent::ToolbarMouseInput { button, state });
			},
			Self::KeyFocus { .. } => {},
		}
	}

	pub(super) fn dispatch_mouse_exited(&self, view_key: usize) {
		match self {
			Self::Overlay { .. } => {
				macos_passive_shell_runtime::macos_update_passive_shell_cursor_point(
					view_key, None,
				);
			},
			Self::Toolbar { dispatch, .. } => {
				dispatch.enqueue(MacOSNativeCaptureInputEvent::ToolbarPointerLeft);
			},
			Self::KeyFocus { .. } => {},
		}
	}

	pub(super) fn dispatch_scroll_wheel(&self, delta: MacOSNativeCaptureScrollDelta) {
		if let Self::Toolbar { dispatch, .. } = self {
			dispatch.enqueue(MacOSNativeCaptureInputEvent::ToolbarScrollWheel { delta });
		}
	}
}

pub(super) fn macos_capture_shell_mouse_location() -> Option<GlobalPoint> {
	overlay::macos_mouse_location()
}

pub(super) fn macos_register_shell_callback(view_key: usize, callback: MacOSPassiveShellCallback) {
	macos_shell_callbacks().lock().expect("shell callback map poisoned").insert(view_key, callback);
}

pub(super) fn macos_unregister_shell_callback(view_key: usize) {
	macos_shell_callbacks().lock().expect("shell callback map poisoned").remove(&view_key);
}

pub(super) fn macos_shell_callback(view_key: usize) -> Option<MacOSPassiveShellCallback> {
	macos_shell_callbacks().lock().expect("shell callback map poisoned").get(&view_key).cloned()
}

pub(super) fn macos_shell_callback_name(callback: &MacOSPassiveShellCallback) -> &'static str {
	match callback {
		MacOSPassiveShellCallback::Overlay { .. } => "overlay",
		MacOSPassiveShellCallback::Toolbar { .. } => "toolbar",
		MacOSPassiveShellCallback::KeyFocus { .. } => "key_focus",
	}
}

fn clamp_monitor_local_point(monitor: MonitorRect, local_point: NSPoint) -> GlobalPoint {
	let max_x = monitor.width.saturating_sub(1) as i32;
	let max_y = monitor.height.saturating_sub(1) as i32;
	let local_x = (local_point.x.round() as i32).clamp(0, max_x);
	let local_y = (local_point.y.round() as i32).clamp(0, max_y);

	GlobalPoint::new(
		monitor.origin.x.saturating_add(local_x),
		monitor.origin.y.saturating_add(local_y),
	)
}

fn macos_shell_callbacks() -> &'static Mutex<HashMap<usize, MacOSPassiveShellCallback>> {
	static CALLBACKS: OnceLock<Mutex<HashMap<usize, MacOSPassiveShellCallback>>> = OnceLock::new();

	CALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}
