use std::collections::HashMap;
use std::ffi::CStr;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

use egui::FontId;
use objc::declare::ClassDecl;
use objc::runtime::{BOOL, Class, NO, Object, Sel, YES};
use objc::{Encode, Encoding};
use objc2_foundation::{NSPoint, NSRange, NSRect, NSSize};
use winit::event::{ElementState, Ime, MouseButton};
use winit::keyboard::{Key, ModifiersState, NamedKey, NativeKey};
use winit::window::WindowId;

use crate::overlay::OverlayWindow;
use crate::overlay::{
	CursorIcon, GlobalPoint, MacOSNativeCaptureInputDispatch, MacOSNativeCaptureInputEvent,
	MacOSNativeCaptureScrollDelta, MonitorRect, OverlayKeyboardInputEvent, OverlayMode,
	OverlaySession, Pos2, Window,
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

pub(super) struct MacOSNativeCaptureShells {
	overlay_shells: HashMap<u32, MacOSPassiveShellWindow>,
	toolbar_shell: Option<MacOSPassiveShellWindow>,
	key_focus_shell: Option<MacOSKeyFocusShellWindow>,
	key_focus_shell_active: bool,
	key_focus_shell_target: Option<MacOSKeyFocusShellTarget>,
	dispatch: MacOSNativeCaptureInputDispatch,
}
impl MacOSNativeCaptureShells {
	fn new(
		overlay_shells: HashMap<u32, MacOSPassiveShellWindow>,
		dispatch: MacOSNativeCaptureInputDispatch,
	) -> Self {
		Self {
			overlay_shells,
			toolbar_shell: None,
			key_focus_shell: None,
			key_focus_shell_active: false,
			key_focus_shell_target: None,
			dispatch,
		}
	}

	fn sync_overlay_shells(&self, windows: &HashMap<WindowId, OverlayWindow>, visible: bool) {
		tracing::trace!(
			op = "overlay.macos_passive_shell_sync_overlay_shells",
			visible,
			window_count = windows.len(),
			"Synced passive overlay shells."
		);

		for overlay_window in windows.values() {
			if let Some(shell) = self.overlay_shells.get(&overlay_window.monitor.id) {
				shell.sync_from_render_window(overlay_window.window.as_ref(), visible);
			}
		}

		if visible {
			macos_set_crosshair_cursor();
		}
	}

	fn ensure_toolbar_shell(
		&mut self,
		render_window: &Window,
	) -> Result<&MacOSPassiveShellWindow, String> {
		if self.toolbar_shell.is_none() {
			self.toolbar_shell = Some(macos_create_passive_toolbar_shell_window(
				render_window,
				self.dispatch.clone(),
			)?);
		}

		self.toolbar_shell
			.as_ref()
			.ok_or_else(|| String::from("Toolbar shell should exist after creation"))
	}

	fn sync_toolbar_shell(
		&mut self,
		render_window: &Window,
		monitor: MonitorRect,
		outer_position: GlobalPoint,
		visible: bool,
	) -> Result<(), String> {
		let shell = self.ensure_toolbar_shell(render_window)?;

		shell.set_toolbar_state(monitor, outer_position);
		shell.sync_from_render_window(render_window, visible);

		Ok(())
	}

	fn clear_toolbar_shell(&mut self) {
		self.toolbar_shell = None;
	}

	fn ensure_key_focus_shell(
		&mut self,
		render_window: &Window,
	) -> Result<&MacOSKeyFocusShellWindow, String> {
		if self.key_focus_shell.is_none() {
			self.key_focus_shell =
				Some(macos_create_key_focus_shell_window(render_window, self.dispatch.clone())?);
		}

		self.key_focus_shell
			.as_ref()
			.ok_or_else(|| String::from("Key-focus shell should exist after creation"))
	}

	fn sync_key_focus_shell(
		&mut self,
		render_window: &Window,
		target: MacOSKeyFocusShellTarget,
		visible: bool,
	) -> Result<(), String> {
		let should_focus = visible
			&& (!self.key_focus_shell_active
				|| self.key_focus_shell_target != Some(target)
				|| matches!(target.kind, MacOSKeyFocusShellKind::FrozenText));

		self.key_focus_shell_active = visible;
		self.key_focus_shell_target = visible.then_some(target);

		let shell = self.ensure_key_focus_shell(render_window)?;

		shell.sync_from_render_window(render_window, target, visible);

		if should_focus {
			shell.ensure_key_focus();
		}

		Ok(())
	}

	fn clear_key_focus_shell(&mut self) {
		if let Some(shell) = self.key_focus_shell.as_ref() {
			shell.clear_target();
			shell.hide();
		}

		self.key_focus_shell_active = false;
		self.key_focus_shell_target = None;
	}
}

pub(super) struct MacOSNativeCaptureRootOwner {
	window_key: usize,
}
impl MacOSNativeCaptureRootOwner {
	fn sync_frame_and_visibility(&self, frame: NSRect, visible: bool) {
		let ns_window = self.window_key as *mut Object;

		if ns_window.is_null() {
			return;
		}

		unsafe {
			let _: () = objc::msg_send![ns_window, setFrame: frame display: NO];

			if visible {
				let _: () = objc::msg_send![ns_window, orderFrontRegardless];
			} else {
				let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
			}
		}
	}

	fn attach_child_window(&self, child_window: *mut Object) {
		let ns_window = self.window_key as *mut Object;

		if ns_window.is_null() || child_window.is_null() || ns_window == child_window {
			return;
		}

		unsafe {
			let current_parent: *mut Object = objc::msg_send![child_window, parentWindow];

			if current_parent == ns_window {
				return;
			}
			if !current_parent.is_null() {
				let _: () = objc::msg_send![current_parent, removeChildWindow: child_window];
			}

			let _: () = objc::msg_send![ns_window, addChildWindow: child_window ordered: 1_isize];
		}
	}

	fn detach_child_window(&self, child_window: *mut Object) {
		let ns_window = self.window_key as *mut Object;

		if ns_window.is_null() || child_window.is_null() {
			return;
		}

		unsafe {
			let current_parent: *mut Object = objc::msg_send![child_window, parentWindow];

			if current_parent == ns_window {
				let _: () = objc::msg_send![ns_window, removeChildWindow: child_window];
			}
		}
	}
}

impl Drop for MacOSNativeCaptureRootOwner {
	fn drop(&mut self) {
		unsafe {
			let ns_window = self.window_key as *mut Object;

			if !ns_window.is_null() {
				let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
				let _: () = objc::msg_send![ns_window, close];
				let _: () = objc::msg_send![ns_window, release];
			}
		}
	}
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MacOSRange {
	location: usize,
	length: usize,
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
struct MacOSSize {
	width: f64,
	height: f64,
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
struct MacOSRect {
	origin: super::MacOSOverlayPoint,
	size: MacOSSize,
}
unsafe impl Encode for MacOSRect {
	fn encode() -> Encoding {
		unsafe { Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
	}
}

impl From<NSRect> for MacOSRect {
	fn from(value: NSRect) -> Self {
		Self {
			origin: super::MacOSOverlayPoint { x: value.origin.x, y: value.origin.y },
			size: MacOSSize::from(value.size),
		}
	}
}

#[derive(Clone, Copy, Default)]
struct MacOSPassiveToolbarShellState {
	monitor: Option<MonitorRect>,
	outer_position: Option<GlobalPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MacOSKeyFocusShellTarget {
	kind: MacOSKeyFocusShellKind,
	monitor: Option<MonitorRect>,
	ime_allowed: bool,
	ime_origin: NSPoint,
	ime_size: NSSize,
}

#[derive(Debug)]
struct MacOSKeyFocusShellState {
	target: Option<MacOSKeyFocusShellTarget>,
	keyboard_modifiers: ModifiersState,
	marked_text: String,
	forward_key_event_to_app: bool,
	had_ime_input_during_keydown: bool,
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

struct MacOSPassiveShellWindow {
	window_key: usize,
	view_key: usize,
	tracking_area_key: usize,
	toolbar_state: Option<Arc<Mutex<MacOSPassiveToolbarShellState>>>,
}
impl MacOSPassiveShellWindow {
	fn ns_window(&self) -> *mut Object {
		self.window_key as *mut Object
	}

	fn sync_from_render_window(&self, render_window: &Window, visible: bool) {
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
			let _: () = objc::msg_send![ns_window, setFrame: frame display: NO];

			if visible {
				let _: () = objc::msg_send![ns_window, orderFrontRegardless];
			} else {
				let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
			}
			if !view.is_null() {
				let _: () = objc::msg_send![ns_window, invalidateCursorRectsForView: view];
			}
		}
	}

	fn set_toolbar_state(&self, monitor: MonitorRect, outer_position: GlobalPoint) {
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
		macos_unregister_shell_callback(self.view_key);

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

struct MacOSKeyFocusShellWindow {
	window_key: usize,
	view_key: usize,
	state: Arc<Mutex<MacOSKeyFocusShellState>>,
}
impl MacOSKeyFocusShellWindow {
	fn ns_window(&self) -> *mut Object {
		self.window_key as *mut Object
	}

	fn sync_from_render_window(
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

	fn clear_target(&self) {
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
			let _: () = objc::msg_send![ns_window, setFrame: frame display: NO];

			if visible {
				let _: () = objc::msg_send![ns_window, orderFrontRegardless];
			} else {
				let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
			}
		}
	}

	fn hide(&self) {
		let ns_window = self.window_key as *mut Object;

		if ns_window.is_null() {
			return;
		}

		unsafe {
			let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];
		}
	}

	fn ensure_key_focus(&self) {
		let ns_window = self.window_key as *mut Object;
		let view = self.view_key as *mut Object;

		if ns_window.is_null() || view.is_null() {
			return;
		}

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
		macos_unregister_shell_callback(self.view_key);

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

impl OverlaySession {
	pub(super) fn ensure_native_capture_root_owner(&mut self) -> Result<(), String> {
		if self.native_capture_root_owner.is_some() {
			return Ok(());
		}

		let Some(reference_window) = self.native_capture_root_owner_reference_window() else {
			return Ok(());
		};
		let render_ns_window = macos_overlay_window_ns_window(reference_window)
			.ok_or_else(|| String::from("Missing macOS render NSWindow for native capture root"))?;
		let frame = macos_ns_window_frame(render_ns_window)
			.ok_or_else(|| String::from("Missing macOS render frame for native capture root"))?;
		let level = unsafe { macos_ns_window_level(render_ns_window) };
		let collection_behavior = unsafe { macos_ns_window_collection_behavior(render_ns_window) };

		self.native_capture_root_owner =
			Some(macos_create_native_capture_root_owner(frame, level, collection_behavior)?);

		Ok(())
	}

	pub(super) fn sync_native_capture_root_owner(&mut self) -> Result<(), String> {
		self.ensure_native_capture_root_owner()?;

		let Some(root_owner) = self.native_capture_root_owner.as_ref() else {
			return Ok(());
		};

		if let Some(frame) = self.native_capture_root_owner_frame() {
			root_owner.sync_frame_and_visibility(frame, self.session_active);
		} else {
			root_owner.sync_frame_and_visibility(
				NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
				false,
			);

			return Ok(());
		}

		for overlay_window in self.windows.values() {
			if let Some(ns_window) = macos_overlay_window_ns_window(overlay_window.window.as_ref())
			{
				root_owner.attach_child_window(ns_window);
			}
		}
		for aux_window in [
			self.hud_window.as_ref().map(|window| window.window.as_ref()),
			self.loupe_window.as_ref().map(|window| window.window.as_ref()),
			self.toolbar_window.as_ref().map(|window| window.window.as_ref()),
			self.scroll_preview_window.as_ref().map(|window| window.window.as_ref()),
		]
		.into_iter()
		.flatten()
		{
			if let Some(ns_window) = macos_overlay_window_ns_window(aux_window) {
				root_owner.attach_child_window(ns_window);
			}
		}

		if let Some(shells) = self.native_capture_shells.as_ref() {
			for shell in shells.overlay_shells.values() {
				root_owner.attach_child_window(shell.ns_window());
			}

			if let Some(shell) = shells.toolbar_shell.as_ref() {
				root_owner.attach_child_window(shell.ns_window());
			}
			if let Some(shell) = shells.key_focus_shell.as_ref() {
				root_owner.attach_child_window(shell.ns_window());
			}
		}

		Ok(())
	}

	pub(super) fn destroy_native_capture_root_owner(&mut self) {
		self.detach_native_capture_root_owned_windows();

		self.native_capture_root_owner = None;
	}

	fn detach_native_capture_root_owned_windows(&self) {
		let Some(root_owner) = self.native_capture_root_owner.as_ref() else {
			return;
		};

		for overlay_window in self.windows.values() {
			if let Some(ns_window) = macos_overlay_window_ns_window(overlay_window.window.as_ref())
			{
				root_owner.detach_child_window(ns_window);
			}
		}
		for aux_window in [
			self.hud_window.as_ref().map(|window| window.window.as_ref()),
			self.loupe_window.as_ref().map(|window| window.window.as_ref()),
			self.toolbar_window.as_ref().map(|window| window.window.as_ref()),
			self.scroll_preview_window.as_ref().map(|window| window.window.as_ref()),
		]
		.into_iter()
		.flatten()
		{
			if let Some(ns_window) = macos_overlay_window_ns_window(aux_window) {
				root_owner.detach_child_window(ns_window);
			}
		}

		if let Some(shells) = self.native_capture_shells.as_ref() {
			for shell in shells.overlay_shells.values() {
				root_owner.detach_child_window(shell.ns_window());
			}

			if let Some(shell) = shells.toolbar_shell.as_ref() {
				root_owner.detach_child_window(shell.ns_window());
			}
			if let Some(shell) = shells.key_focus_shell.as_ref() {
				root_owner.detach_child_window(shell.ns_window());
			}
		}
	}

	fn native_capture_root_owner_reference_window(&self) -> Option<&Window> {
		self.windows
			.values()
			.next()
			.map(|window| window.window.as_ref())
			.or_else(|| self.hud_window.as_ref().map(|window| window.window.as_ref()))
			.or_else(|| self.loupe_window.as_ref().map(|window| window.window.as_ref()))
			.or_else(|| self.toolbar_window.as_ref().map(|window| window.window.as_ref()))
			.or_else(|| self.scroll_preview_window.as_ref().map(|window| window.window.as_ref()))
	}

	fn native_capture_root_owner_frame(&self) -> Option<NSRect> {
		let mut union_frame: Option<NSRect> = None;

		for ns_window in self
			.windows
			.values()
			.filter_map(|window| macos_overlay_window_ns_window(window.window.as_ref()))
			.chain(
				[
					self.hud_window
						.as_ref()
						.and_then(|window| macos_overlay_window_ns_window(window.window.as_ref())),
					self.loupe_window
						.as_ref()
						.and_then(|window| macos_overlay_window_ns_window(window.window.as_ref())),
					self.toolbar_window
						.as_ref()
						.and_then(|window| macos_overlay_window_ns_window(window.window.as_ref())),
					self.scroll_preview_window
						.as_ref()
						.and_then(|window| macos_overlay_window_ns_window(window.window.as_ref())),
				]
				.into_iter()
				.flatten(),
			) {
			let Some(frame) = macos_ns_window_frame(ns_window) else {
				continue;
			};

			union_frame = Some(match union_frame {
				Some(current_union) => macos_union_ns_rect(current_union, frame),
				None => frame,
			});
		}

		union_frame
	}

	pub(super) fn ensure_native_capture_shells(&mut self) -> Result<(), String> {
		let Some(dispatch) = self.native_capture_input_dispatch() else {
			return Ok(());
		};

		if self.native_capture_shells.is_some() {
			self.sync_native_capture_shells()?;

			return Ok(());
		}

		let mut overlay_shells = HashMap::with_capacity(self.windows.len());

		for overlay_window in self.windows.values() {
			let shell = macos_create_passive_overlay_shell_window(
				overlay_window.window.as_ref(),
				overlay_window.monitor,
				dispatch.clone(),
			)?;

			overlay_shells.insert(overlay_window.monitor.id, shell);
		}

		self.native_capture_shells = Some(MacOSNativeCaptureShells::new(overlay_shells, dispatch));

		self.sync_native_capture_shells()?;

		Ok(())
	}

	pub(super) fn sync_native_capture_shells(&mut self) -> Result<(), String> {
		let live_shell_visible = self.should_host_live_pointer_input_in_native_shell();
		let toolbar_shell_visible = self.should_host_toolbar_pointer_input_in_native_shell();
		let overlay_mouse_passthrough =
			live_shell_visible || self.scroll_capture.overlay_mouse_passthrough_active;
		let toolbar_context = self.toolbar_window.as_ref().map(|toolbar_window| {
			let monitor = self.state.monitor.or_else(|| self.active_cursor_monitor());
			let outer_position = Self::toolbar_window_outer_position(toolbar_window)
				.or(self.pending_toolbar_outer_pos)
				.or(self.toolbar_outer_pos)
				.or_else(|| {
					monitor.and_then(|monitor| {
						self.toolbar_state.floating_position.map(|floating_position| {
							self.toolbar_outer_position_from_primary_anchor(
								monitor,
								floating_position,
							)
						})
					})
				});

			(Arc::clone(&toolbar_window.window), monitor, outer_position)
		});
		let key_focus_context = self.native_key_focus_shell_context();

		for overlay_window in self.windows.values() {
			super::macos_set_capture_window_mouse_passthrough(
				overlay_window.window.as_ref(),
				overlay_mouse_passthrough,
			);
		}

		let Some(shells) = self.native_capture_shells.as_mut() else {
			if let Some((toolbar_window, _, _)) = toolbar_context.as_ref() {
				super::macos_set_capture_window_mouse_passthrough(toolbar_window.as_ref(), false);
			}

			return self.sync_native_capture_root_owner();
		};

		shells.sync_overlay_shells(&self.windows, live_shell_visible);

		if let Some((toolbar_window, monitor, outer_position)) = toolbar_context {
			if let (Some(monitor), Some(outer_position)) = (monitor, outer_position) {
				shells.sync_toolbar_shell(
					toolbar_window.as_ref(),
					monitor,
					outer_position,
					toolbar_shell_visible,
				)?;

				super::macos_set_capture_window_mouse_passthrough(
					toolbar_window.as_ref(),
					toolbar_shell_visible,
				);
			} else {
				super::macos_set_capture_window_mouse_passthrough(toolbar_window.as_ref(), false);

				shells.clear_toolbar_shell();
			}
		} else {
			shells.clear_toolbar_shell();
		}
		if let Some((render_window, target)) = key_focus_context {
			shells.sync_key_focus_shell(render_window.as_ref(), target, true)?;
		} else {
			shells.clear_key_focus_shell();
		}

		self.sync_native_capture_root_owner()?;

		Ok(())
	}

	pub(super) fn destroy_native_capture_shells(&mut self) {
		for overlay_window in self.windows.values() {
			super::macos_set_capture_window_mouse_passthrough(
				overlay_window.window.as_ref(),
				false,
			);
		}

		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			super::macos_set_capture_window_mouse_passthrough(
				toolbar_window.window.as_ref(),
				false,
			);
		}

		self.destroy_native_capture_root_owner();

		self.native_capture_shells = None;
	}

	fn native_key_focus_shell_context(&self) -> Option<(Arc<Window>, MacOSKeyFocusShellTarget)> {
		if self.scroll_capture.active {
			let render_window = self
				.scroll_preview_window
				.as_ref()
				.map(|preview_window| Arc::clone(&preview_window.window))
				.or_else(|| {
					self.scroll_capture.monitor.and_then(|target_monitor| {
						self.windows
							.values()
							.find(|overlay_window| overlay_window.monitor == target_monitor)
							.map(|overlay_window| Arc::clone(&overlay_window.window))
					})
				})?;
			let target = MacOSKeyFocusShellTarget {
				kind: MacOSKeyFocusShellKind::ScrollCapture,
				monitor: self.scroll_capture.monitor,
				ime_allowed: false,
				ime_origin: NSPoint::new(0.0, 0.0),
				ime_size: NSSize::new(1.0, 1.0),
			};

			return Some((render_window, target));
		}

		let edit_state = self.frozen_text_edit.as_ref()?;

		if !self.frozen_text_tool_active() {
			return None;
		}

		let monitor = self.state.monitor?;
		let overlay_window = self.windows.values().find(|window| window.monitor == monitor)?;
		let (visible_text, caret_char_index) = edit_state.visible_text_and_caret_char_index();
		let caret_rect = overlay_window.renderer.frozen_text_edit_caret_rect_for_window(
			edit_state.anchor,
			visible_text.as_str(),
			&FontId::proportional(self.toolbar_state.text_style.font_size_points),
			caret_char_index.unwrap_or_else(|| visible_text.chars().count()),
		);
		let target = MacOSKeyFocusShellTarget {
			kind: MacOSKeyFocusShellKind::FrozenText,
			monitor: Some(monitor),
			ime_allowed: true,
			ime_origin: NSPoint::new(
				f64::from(caret_rect.min.x.max(0.0)),
				f64::from(caret_rect.min.y.max(0.0)),
			),
			ime_size: NSSize::new(
				f64::from(caret_rect.width().max(1.0)),
				f64::from(caret_rect.height().max(self.toolbar_state.text_style.font_size_points)),
			),
		};

		Some((Arc::clone(&overlay_window.window), target))
	}

	pub(super) fn should_host_live_pointer_input_in_native_shell(&self) -> bool {
		self.session_active
			&& !self.capture_windows_hidden
			&& matches!(self.state.mode, OverlayMode::Live)
			&& !self.windows.is_empty()
	}

	pub(super) fn should_host_toolbar_pointer_input_in_native_shell(&self) -> bool {
		self.session_active
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.toolbar_window_visible
	}
}

impl MonitorRect {
	fn clamp_local_point(self, local_point: NSPoint) -> GlobalPoint {
		let max_x = self.width.saturating_sub(1) as i32;
		let max_y = self.height.saturating_sub(1) as i32;
		let local_x = (local_point.x.round() as i32).clamp(0, max_x);
		let local_y = (local_point.y.round() as i32).clamp(0, max_y);

		GlobalPoint::new(
			self.origin.x.saturating_add(local_x),
			self.origin.y.saturating_add(local_y),
		)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MacOSKeyFocusShellKind {
	FrozenText,
	ScrollCapture,
}

#[derive(Clone)]
enum MacOSPassiveShellCallback {
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
	fn dispatch_pointer_moved(&self, local_point: NSPoint) {
		match self {
			Self::Overlay { monitor, dispatch } => {
				dispatch.enqueue(MacOSNativeCaptureInputEvent::OverlayPointerMoved {
					monitor: *monitor,
					global: monitor.clamp_local_point(local_point),
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

	fn dispatch_mouse_input(
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
					global: monitor.clamp_local_point(local_point),
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

	fn dispatch_mouse_exited(&self, view_key: usize) {
		match self {
			Self::Overlay { .. } => {
				let _ = view_key;
			},
			Self::Toolbar { dispatch, .. } => {
				dispatch.enqueue(MacOSNativeCaptureInputEvent::ToolbarPointerLeft);
			},
			Self::KeyFocus { .. } => {},
		}
	}

	fn dispatch_scroll_wheel(&self, delta: MacOSNativeCaptureScrollDelta) {
		if let Self::Toolbar { dispatch, .. } = self {
			dispatch.enqueue(MacOSNativeCaptureInputEvent::ToolbarScrollWheel { delta });
		}
	}
}

fn macos_shell_callbacks() -> &'static Mutex<HashMap<usize, MacOSPassiveShellCallback>> {
	static CALLBACKS: OnceLock<Mutex<HashMap<usize, MacOSPassiveShellCallback>>> = OnceLock::new();

	CALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn macos_register_shell_callback(view_key: usize, callback: MacOSPassiveShellCallback) {
	macos_shell_callbacks().lock().expect("shell callback map poisoned").insert(view_key, callback);
}

fn macos_unregister_shell_callback(view_key: usize) {
	macos_shell_callbacks().lock().expect("shell callback map poisoned").remove(&view_key);
}

fn macos_shell_callback(view_key: usize) -> Option<MacOSPassiveShellCallback> {
	macos_shell_callbacks().lock().expect("shell callback map poisoned").get(&view_key).cloned()
}

fn macos_passive_shell_panel_class() -> *const Class {
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

fn macos_passive_shell_view_class() -> *const Class {
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
				macos_passive_shell_view_is_flipped as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(acceptsFirstMouse:),
				macos_passive_shell_view_accepts_first_mouse
					as extern "C" fn(&Object, Sel, *mut Object) -> BOOL,
			);
			decl.add_method(
				objc::sel!(hitTest:),
				macos_passive_shell_view_hit_test
					as extern "C" fn(&Object, Sel, super::MacOSOverlayPoint) -> *mut Object,
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

fn macos_key_focus_shell_panel_class() -> *const Class {
	static CLASS: OnceLock<usize> = OnceLock::new();

	(*CLASS.get_or_init(|| {
		if let Some(class) = Class::get("RsnapKeyFocusCaptureShellPanel") {
			return class as *const Class as usize;
		}

		let superclass = objc::class!(NSWindow);
		let mut decl = ClassDecl::new("RsnapKeyFocusCaptureShellPanel", superclass)
			.expect("key-focus capture shell panel class");

		unsafe {
			decl.add_method(
				objc::sel!(canBecomeMainWindow),
				macos_key_focus_shell_can_become_main_window as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(canBecomeKeyWindow),
				macos_key_focus_shell_can_become_key_window as extern "C" fn(&Object, Sel) -> BOOL,
			);
		}

		decl.register() as *const Class as usize
	})) as *const Class
}

fn macos_key_focus_shell_view_class() -> *const Class {
	static CLASS: OnceLock<usize> = OnceLock::new();

	(*CLASS.get_or_init(|| {
		if let Some(class) = Class::get("RsnapKeyFocusCaptureShellView") {
			return class as *const Class as usize;
		}

		let superclass = objc::class!(NSView);
		let mut decl = ClassDecl::new("RsnapKeyFocusCaptureShellView", superclass)
			.expect("key-focus capture shell view class");

		unsafe {
			decl.add_method(
				objc::sel!(isFlipped),
				macos_passive_shell_view_is_flipped as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(acceptsFirstResponder),
				macos_key_focus_shell_view_accepts_first_responder
					as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(keyDown:),
				macos_key_focus_shell_view_key_down as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(keyUp:),
				macos_key_focus_shell_view_key_up as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(flagsChanged:),
				macos_key_focus_shell_view_flags_changed
					as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(hasMarkedText),
				macos_key_focus_shell_view_has_marked_text as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(markedRange),
				macos_key_focus_shell_view_marked_range
					as extern "C" fn(&Object, Sel) -> MacOSRange,
			);
			decl.add_method(
				objc::sel!(selectedRange),
				macos_key_focus_shell_view_selected_range
					as extern "C" fn(&Object, Sel) -> MacOSRange,
			);
			decl.add_method(
				objc::sel!(setMarkedText:selectedRange:replacementRange:),
				macos_key_focus_shell_view_set_marked_text
					as extern "C" fn(&Object, Sel, *mut Object, MacOSRange, MacOSRange),
			);
			decl.add_method(
				objc::sel!(unmarkText),
				macos_key_focus_shell_view_unmark_text as extern "C" fn(&Object, Sel),
			);
			decl.add_method(
				objc::sel!(validAttributesForMarkedText),
				macos_key_focus_shell_view_valid_attributes_for_marked_text
					as extern "C" fn(&Object, Sel) -> *mut Object,
			);
			decl.add_method(
				objc::sel!(attributedSubstringForProposedRange:actualRange:),
				macos_key_focus_shell_view_attributed_substring_for_proposed_range
					as extern "C" fn(&Object, Sel, MacOSRange, *mut c_void) -> *mut Object,
			);
			decl.add_method(
				objc::sel!(characterIndexForPoint:),
				macos_key_focus_shell_view_character_index_for_point
					as extern "C" fn(&Object, Sel, super::MacOSOverlayPoint) -> usize,
			);
			decl.add_method(
				objc::sel!(firstRectForCharacterRange:actualRange:),
				macos_key_focus_shell_view_first_rect_for_character_range
					as extern "C" fn(&Object, Sel, MacOSRange, *mut c_void) -> MacOSRect,
			);
			decl.add_method(
				objc::sel!(insertText:replacementRange:),
				macos_key_focus_shell_view_insert_text
					as extern "C" fn(&Object, Sel, *mut Object, MacOSRange),
			);
			decl.add_method(
				objc::sel!(doCommandBySelector:),
				macos_key_focus_shell_view_do_command_by_selector
					as extern "C" fn(&Object, Sel, Sel),
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

extern "C" fn macos_key_focus_shell_can_become_main_window(_this: &Object, _cmd: Sel) -> BOOL {
	YES
}

extern "C" fn macos_key_focus_shell_can_become_key_window(_this: &Object, _cmd: Sel) -> BOOL {
	YES
}

extern "C" fn macos_passive_shell_view_is_flipped(_this: &Object, _cmd: Sel) -> BOOL {
	YES
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
	_point: super::MacOSOverlayPoint,
) -> *mut Object {
	this as *const Object as *mut Object
}

extern "C" fn macos_passive_shell_view_draw_rect(this: &Object, _cmd: Sel, dirty_rect: MacOSRect) {
	let _ = this;
	let _ = dirty_rect;
}

extern "C" fn macos_passive_shell_view_reset_cursor_rects(this: &Object, _cmd: Sel) {
	let bounds: NSRect = unsafe { objc::msg_send![this, bounds] };
	let Some(callback) = macos_shell_callback(this as *const Object as usize) else {
		return;
	};
	let cursor = match callback {
		MacOSPassiveShellCallback::Overlay { .. } => {
			super::macos_cursor_object_for_icon(CursorIcon::Crosshair)
		},
		MacOSPassiveShellCallback::Toolbar { .. } | MacOSPassiveShellCallback::KeyFocus { .. } => {
			super::macos_cursor_object_for_icon(CursorIcon::Default)
		},
	};

	if cursor.is_null() {
		return;
	}

	unsafe {
		let _: () = objc::msg_send![this, addCursorRect: bounds cursor: cursor];
	}
}

extern "C" fn macos_key_focus_shell_view_accepts_first_responder(
	_this: &Object,
	_cmd: Sel,
) -> BOOL {
	YES
}

extern "C" fn macos_passive_shell_view_mouse_moved(this: &Object, _cmd: Sel, event: *mut Object) {
	macos_dispatch_shell_pointer_moved(this, event);
}

extern "C" fn macos_passive_shell_view_cursor_update(
	this: &Object,
	_cmd: Sel,
	_event: *mut Object,
) {
	let Some(callback) = macos_shell_callback(this as *const Object as usize) else {
		return;
	};

	tracing::trace!(
		op = "overlay.macos_passive_shell_cursor_update",
		callback = %macos_shell_callback_name(&callback),
		"Passive shell received cursorUpdate."
	);

	match callback {
		MacOSPassiveShellCallback::Overlay { .. } => {
			super::macos_set_cursor_icon(CursorIcon::Crosshair);
		},
		MacOSPassiveShellCallback::Toolbar { .. } => {
			super::macos_set_cursor_icon(CursorIcon::Default);
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
	let Some(callback) = macos_shell_callback(this as *const Object as usize) else {
		return;
	};

	callback.dispatch_mouse_exited(view_key);
}

extern "C" fn macos_passive_shell_view_scroll_wheel(this: &Object, _cmd: Sel, event: *mut Object) {
	let Some(callback) = macos_shell_callback(this as *const Object as usize) else {
		return;
	};
	let Some(delta) = macos_shell_scroll_delta(event) else {
		return;
	};

	callback.dispatch_scroll_wheel(delta);
}

fn macos_key_focus_shell_state(
	this: &Object,
) -> Option<(Arc<Mutex<MacOSKeyFocusShellState>>, MacOSNativeCaptureInputDispatch)> {
	let callback = macos_shell_callback(this as *const Object as usize)?;

	match callback {
		MacOSPassiveShellCallback::KeyFocus { state, dispatch } => Some((state, dispatch)),
		_ => None,
	}
}

fn macos_nsstring_to_string(string: *mut Object) -> String {
	if string.is_null() {
		return String::new();
	}

	unsafe {
		let utf8: *const i8 = objc::msg_send![string, UTF8String];

		if utf8.is_null() {
			return String::new();
		}

		CStr::from_ptr(utf8).to_string_lossy().into_owned()
	}
}

fn macos_text_input_object_to_string(string: *mut Object) -> String {
	if string.is_null() {
		return String::new();
	}

	unsafe {
		let attributed_string_class = objc::class!(NSAttributedString);
		let is_attributed: BOOL = objc::msg_send![string, isKindOfClass: attributed_string_class];
		let string_object = if is_attributed == YES {
			let value: *mut Object = objc::msg_send![string, string];

			value
		} else {
			string
		};

		macos_nsstring_to_string(string_object)
	}
}

fn macos_utf16_offset_to_utf8(text: &str, utf16_offset: usize) -> usize {
	let mut utf16_count = 0;

	for (byte_index, ch) in text.char_indices() {
		if utf16_count >= utf16_offset {
			return byte_index;
		}

		utf16_count += ch.len_utf16();

		if utf16_count >= utf16_offset {
			return byte_index + ch.len_utf8();
		}
	}

	text.len()
}

fn macos_modifier_state_from_event(event: *mut Object) -> ModifiersState {
	if event.is_null() {
		return ModifiersState::empty();
	}

	const SHIFT: u64 = 1 << 17;
	const CONTROL: u64 = 1 << 18;
	const OPTION: u64 = 1 << 19;
	const COMMAND: u64 = 1 << 20;
	unsafe {
		let flags: u64 = objc::msg_send![event, modifierFlags];
		let mut state = ModifiersState::empty();

		state.set(ModifiersState::SHIFT, flags & SHIFT != 0);
		state.set(ModifiersState::CONTROL, flags & CONTROL != 0);
		state.set(ModifiersState::ALT, flags & OPTION != 0);
		state.set(ModifiersState::SUPER, flags & COMMAND != 0);

		state
	}
}

fn macos_key_focus_logical_key(event: *mut Object) -> Key {
	if event.is_null() {
		return Key::Unidentified(NativeKey::MacOS(0));
	}

	unsafe {
		let key_code: u16 = objc::msg_send![event, keyCode];

		match key_code {
			53 => Key::Named(NamedKey::Escape),
			36 | 76 => Key::Named(NamedKey::Enter),
			51 => Key::Named(NamedKey::Backspace),
			49 => Key::Named(NamedKey::Space),
			48 => Key::Named(NamedKey::Tab),
			_ => {
				let string: *mut Object = objc::msg_send![event, charactersIgnoringModifiers];
				let text = macos_nsstring_to_string(string);

				if text.is_empty() {
					Key::Unidentified(NativeKey::MacOS(key_code))
				} else {
					Key::Character(text.into())
				}
			},
		}
	}
}

fn macos_key_focus_text(event: *mut Object, logical_key: &Key) -> Option<String> {
	if event.is_null() {
		return None;
	}

	match logical_key {
		Key::Named(NamedKey::Escape | NamedKey::Backspace | NamedKey::Tab) => None,
		Key::Named(NamedKey::Enter) => Some(String::from("\r")),
		Key::Named(NamedKey::Space) => Some(String::from(" ")),
		_ => unsafe {
			let string: *mut Object = objc::msg_send![event, characters];
			let text = macos_nsstring_to_string(string);

			(!text.is_empty()).then_some(text)
		},
	}
}

fn macos_key_focus_keyboard_input_from_event(
	event: *mut Object,
	state: ElementState,
) -> OverlayKeyboardInputEvent {
	let logical_key = macos_key_focus_logical_key(event);
	let text = macos_key_focus_text(event, &logical_key);
	let repeat = if event.is_null() {
		false
	} else {
		unsafe {
			let repeat: BOOL = objc::msg_send![event, isARepeat];

			repeat == YES
		}
	};

	OverlayKeyboardInputEvent { logical_key, text, state, repeat }
}

fn macos_key_focus_target(this: &Object) -> Option<MacOSKeyFocusShellTarget> {
	let (state, _) = macos_key_focus_shell_state(this)?;
	let Ok(state) = state.lock() else {
		return None;
	};

	state.target
}

fn macos_key_focus_ime_allowed(this: &Object) -> bool {
	macos_key_focus_target(this).is_some_and(|target| target.ime_allowed)
}

fn macos_key_focus_dispatch_keyboard_input(this: &Object, event: *mut Object, state: ElementState) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let overlay_event = macos_key_focus_keyboard_input_from_event(event, state);
	let monitor =
		shell_state.lock().ok().and_then(|state| state.target.and_then(|target| target.monitor));

	dispatch.enqueue(MacOSNativeCaptureInputEvent::KeyboardInput { monitor, event: overlay_event });
}

extern "C" fn macos_key_focus_shell_view_key_down(this: &Object, _cmd: Sel, event: *mut Object) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let ime_allowed = macos_key_focus_ime_allowed(this);

	if let Ok(mut state) = shell_state.lock() {
		state.forward_key_event_to_app = false;
		state.had_ime_input_during_keydown = false;
		state.keyboard_modifiers = macos_modifier_state_from_event(event);
	}

	dispatch.enqueue(MacOSNativeCaptureInputEvent::ModifiersChanged {
		state: macos_modifier_state_from_event(event),
	});

	if ime_allowed {
		unsafe {
			let array: *mut Object = objc::msg_send![objc::class!(NSArray), arrayWithObject: event];
			let _: () = objc::msg_send![this, interpretKeyEvents: array];
		}
	}

	let should_forward = if let Ok(state) = shell_state.lock() {
		!state.had_ime_input_during_keydown || state.forward_key_event_to_app
	} else {
		true
	};

	if should_forward {
		macos_key_focus_dispatch_keyboard_input(this, event, ElementState::Pressed);
	}
}

extern "C" fn macos_key_focus_shell_view_key_up(this: &Object, _cmd: Sel, event: *mut Object) {
	macos_key_focus_dispatch_keyboard_input(this, event, ElementState::Released);
}

extern "C" fn macos_key_focus_shell_view_flags_changed(
	this: &Object,
	_cmd: Sel,
	event: *mut Object,
) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let modifiers = macos_modifier_state_from_event(event);

	if let Ok(mut state) = shell_state.lock() {
		state.keyboard_modifiers = modifiers;
	}

	dispatch.enqueue(MacOSNativeCaptureInputEvent::ModifiersChanged { state: modifiers });
}

extern "C" fn macos_key_focus_shell_view_has_marked_text(this: &Object, _cmd: Sel) -> BOOL {
	let Some((shell_state, _)) = macos_key_focus_shell_state(this) else {
		return NO;
	};
	let Ok(state) = shell_state.lock() else {
		return NO;
	};

	if state.marked_text.is_empty() { NO } else { YES }
}

extern "C" fn macos_key_focus_shell_view_marked_range(this: &Object, _cmd: Sel) -> MacOSRange {
	let Some((shell_state, _)) = macos_key_focus_shell_state(this) else {
		return MacOSRange::from(NSRange::new(usize::MAX, 0));
	};
	let Ok(state) = shell_state.lock() else {
		return MacOSRange::from(NSRange::new(usize::MAX, 0));
	};

	if state.marked_text.is_empty() {
		MacOSRange::from(NSRange::new(usize::MAX, 0))
	} else {
		MacOSRange::from(NSRange::new(0, state.marked_text.encode_utf16().count()))
	}
}

extern "C" fn macos_key_focus_shell_view_selected_range(this: &Object, _cmd: Sel) -> MacOSRange {
	let _ = this;

	MacOSRange::from(NSRange::new(usize::MAX, 0))
}

extern "C" fn macos_key_focus_shell_view_set_marked_text(
	this: &Object,
	_cmd: Sel,
	string: *mut Object,
	selected_range: MacOSRange,
	_replacement_range: MacOSRange,
) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let text = macos_text_input_object_to_string(string);
	let selected_range = NSRange::from(selected_range);
	let cursor_range = if text.is_empty() {
		None
	} else {
		let start = macos_utf16_offset_to_utf8(&text, selected_range.location);
		let end =
			macos_utf16_offset_to_utf8(&text, selected_range.location + selected_range.length);

		Some((start, end))
	};
	let monitor = if let Ok(mut state) = shell_state.lock() {
		state.marked_text = text.clone();
		state.had_ime_input_during_keydown = true;

		state.target.and_then(|target| target.monitor)
	} else {
		None
	};

	tracing::info!(
		op = "overlay.macos_key_focus_shell_set_marked_text",
		monitor_id = ?monitor.map(|target| target.id),
		text_len = text.chars().count(),
		"Received marked text from the native key-focus shell."
	);

	dispatch.enqueue(MacOSNativeCaptureInputEvent::Ime {
		monitor,
		event: Ime::Preedit(text, cursor_range),
	});
}

extern "C" fn macos_key_focus_shell_view_unmark_text(this: &Object, _cmd: Sel) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let monitor = if let Ok(mut state) = shell_state.lock() {
		state.marked_text.clear();

		state.target.and_then(|target| target.monitor)
	} else {
		None
	};

	unsafe {
		let input_context: *mut Object = objc::msg_send![this, inputContext];

		if !input_context.is_null() {
			let _: () = objc::msg_send![input_context, discardMarkedText];
		}
	}

	dispatch.enqueue(MacOSNativeCaptureInputEvent::Ime {
		monitor,
		event: Ime::Preedit(String::new(), None),
	});
}

extern "C" fn macos_key_focus_shell_view_valid_attributes_for_marked_text(
	_this: &Object,
	_cmd: Sel,
) -> *mut Object {
	unsafe { objc::msg_send![objc::class!(NSArray), array] }
}

extern "C" fn macos_key_focus_shell_view_attributed_substring_for_proposed_range(
	_this: &Object,
	_cmd: Sel,
	_range: MacOSRange,
	_actual_range: *mut c_void,
) -> *mut Object {
	ptr::null_mut::<Object>()
}

extern "C" fn macos_key_focus_shell_view_character_index_for_point(
	_this: &Object,
	_cmd: Sel,
	_point: super::MacOSOverlayPoint,
) -> usize {
	0
}

extern "C" fn macos_key_focus_shell_view_first_rect_for_character_range(
	this: &Object,
	_cmd: Sel,
	_range: MacOSRange,
	_actual_range: *mut c_void,
) -> MacOSRect {
	let Some(target) = macos_key_focus_target(this) else {
		return MacOSRect::from(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)));
	};
	let rect = NSRect::new(target.ime_origin, target.ime_size);

	unsafe {
		let ns_window: *mut Object = objc::msg_send![this, window];

		if ns_window.is_null() {
			return rect.into();
		}

		let view_rect: NSRect =
			objc::msg_send![this, convertRect: rect toView: ptr::null_mut::<Object>()];
		let screen_rect: NSRect = objc::msg_send![ns_window, convertRectToScreen: view_rect];

		MacOSRect::from(screen_rect)
	}
}

extern "C" fn macos_key_focus_shell_view_insert_text(
	this: &Object,
	_cmd: Sel,
	string: *mut Object,
	_replacement_range: MacOSRange,
) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let text = macos_text_input_object_to_string(string);

	if text.is_empty() {
		return;
	}

	let monitor = if let Ok(mut state) = shell_state.lock() {
		state.marked_text.clear();

		state.had_ime_input_during_keydown = true;

		state.target.and_then(|target| target.monitor)
	} else {
		None
	};

	tracing::info!(
		op = "overlay.macos_key_focus_shell_insert_text",
		monitor_id = ?monitor.map(|target| target.id),
		text_len = text.chars().count(),
		"Committed text from the native key-focus shell."
	);

	dispatch.enqueue(MacOSNativeCaptureInputEvent::Ime {
		monitor,
		event: Ime::Preedit(String::new(), None),
	});
	dispatch.enqueue(MacOSNativeCaptureInputEvent::Ime { monitor, event: Ime::Commit(text) });
}

extern "C" fn macos_key_focus_shell_view_do_command_by_selector(
	this: &Object,
	_cmd: Sel,
	_selector: Sel,
) {
	let Some((shell_state, _)) = macos_key_focus_shell_state(this) else {
		return;
	};

	if let Ok(mut state) = shell_state.lock() {
		state.forward_key_event_to_app = true;
	}
}

fn macos_dispatch_shell_pointer_moved(this: &Object, event: *mut Object) {
	let Some(callback) = macos_shell_callback(this as *const Object as usize) else {
		return;
	};
	let Some(local_point) = macos_shell_local_point(this, event) else {
		return;
	};

	tracing::trace!(
		op = "overlay.macos_passive_shell_pointer_moved",
		callback = %macos_shell_callback_name(&callback),
		x = local_point.x,
		y = local_point.y,
		"Passive shell received pointer movement."
	);

	match callback {
		MacOSPassiveShellCallback::Overlay { .. } => {
			super::macos_set_cursor_icon(CursorIcon::Crosshair);
		},
		MacOSPassiveShellCallback::Toolbar { .. } => {
			super::macos_set_cursor_icon(CursorIcon::Default);
		},
		MacOSPassiveShellCallback::KeyFocus { .. } => {},
	}

	callback.dispatch_pointer_moved(local_point);
}

fn macos_dispatch_shell_mouse_input(
	this: &Object,
	event: *mut Object,
	button: MouseButton,
	state: ElementState,
) {
	let Some(callback) = macos_shell_callback(this as *const Object as usize) else {
		return;
	};
	let local_point = macos_shell_local_point(this, event);

	match callback {
		MacOSPassiveShellCallback::Overlay { .. } => {
			super::macos_set_cursor_icon(CursorIcon::Crosshair);
		},
		MacOSPassiveShellCallback::Toolbar { .. } => {
			super::macos_set_cursor_icon(CursorIcon::Default);
		},
		MacOSPassiveShellCallback::KeyFocus { .. } => {},
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

fn macos_set_crosshair_cursor() {
	super::macos_set_cursor_icon(CursorIcon::Crosshair);
}

fn macos_shell_callback_name(callback: &MacOSPassiveShellCallback) -> &'static str {
	match callback {
		MacOSPassiveShellCallback::Overlay { .. } => "overlay",
		MacOSPassiveShellCallback::Toolbar { .. } => "toolbar",
		MacOSPassiveShellCallback::KeyFocus { .. } => "key_focus",
	}
}

fn macos_create_passive_overlay_shell_window(
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

fn macos_create_native_capture_root_owner(
	frame: NSRect,
	level: i64,
	collection_behavior: usize,
) -> Result<MacOSNativeCaptureRootOwner, String> {
	let panel_class = macos_passive_shell_panel_class();
	let nonactivating_panel_mask: usize = 1 << 7;
	let backing_buffered: usize = 2;

	unsafe {
		let ns_window_alloc: *mut Object = objc::msg_send![panel_class, alloc];
		let ns_window: *mut Object = objc::msg_send![
			ns_window_alloc,
			initWithContentRect: frame
			styleMask: nonactivating_panel_mask
			backing: backing_buffered
			defer: NO
		];

		if ns_window.is_null() {
			return Err(String::from("Failed to create native capture root owner window"));
		}

		let clear: *mut Object = objc::msg_send![objc::class!(NSColor), clearColor];

		super::macos_configure_nonactivating_capture_window_with_ns_window(ns_window);

		let _: () = objc::msg_send![ns_window, setReleasedWhenClosed: NO];
		let _: () = objc::msg_send![ns_window, setOpaque: NO];
		let _: () = objc::msg_send![ns_window, setHasShadow: NO];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let _: () = objc::msg_send![ns_window, setLevel: level];
		let _: () = objc::msg_send![ns_window, setCollectionBehavior: collection_behavior];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: NO];
		let _: () = objc::msg_send![ns_window, setIgnoresMouseEvents: YES];
		let _: () = objc::msg_send![ns_window, setHidesOnDeactivate: NO];
		let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];

		Ok(MacOSNativeCaptureRootOwner { window_key: ns_window as usize })
	}
}

fn macos_create_passive_toolbar_shell_window(
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

fn macos_create_key_focus_shell_window(
	render_window: &Window,
	dispatch: MacOSNativeCaptureInputDispatch,
) -> Result<MacOSKeyFocusShellWindow, String> {
	let render_ns_window = macos_overlay_window_ns_window(render_window)
		.ok_or_else(|| String::from("Missing macOS render NSWindow for key-focus shell"))?;
	let frame = macos_ns_window_frame(render_ns_window)
		.ok_or_else(|| String::from("Missing macOS render frame for key-focus shell"))?;
	let level = unsafe { macos_ns_window_level(render_ns_window) };
	let collection_behavior = unsafe { macos_ns_window_collection_behavior(render_ns_window) };
	let panel_class = macos_key_focus_shell_panel_class();
	let view_class = macos_key_focus_shell_view_class();
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
			defer: NO
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
		let _: () = objc::msg_send![ns_window, setReleasedWhenClosed: NO];
		let _: () = objc::msg_send![ns_window, setOpaque: NO];
		let _: () = objc::msg_send![ns_window, setHasShadow: NO];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let _: () = objc::msg_send![ns_window, setLevel: level];
		let _: () = objc::msg_send![ns_window, setCollectionBehavior: collection_behavior];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: NO];
		let _: () = objc::msg_send![ns_window, setIgnoresMouseEvents: YES];
		let _: () = objc::msg_send![ns_window, setHidesOnDeactivate: NO];
		let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];

		macos_register_shell_callback(
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
	let panel_class = macos_passive_shell_panel_class();
	let view_class = macos_passive_shell_view_class();
	let nonactivating_panel_mask: usize = 1 << 7;
	let backing_buffered: usize = 2;

	unsafe {
		let ns_window_alloc: *mut Object = objc::msg_send![panel_class, alloc];
		let ns_window: *mut Object = objc::msg_send![
			ns_window_alloc,
			initWithContentRect: frame
			styleMask: nonactivating_panel_mask
			backing: backing_buffered
			defer: NO
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
		let _: () = objc::msg_send![ns_window, setReleasedWhenClosed: NO];
		let _: () = objc::msg_send![ns_window, setOpaque: NO];
		let _: () = objc::msg_send![ns_window, setHasShadow: NO];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let _: () = objc::msg_send![ns_window, setLevel: level];
		let _: () = objc::msg_send![ns_window, setCollectionBehavior: collection_behavior];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: YES];
		let _: () = objc::msg_send![ns_window, setIgnoresMouseEvents: NO];
		let _: () = objc::msg_send![ns_window, setHidesOnDeactivate: NO];
		let _: () = objc::msg_send![ns_window, orderOut: ptr::null_mut::<Object>()];

		macos_register_shell_callback(view as usize, callback);

		Ok(MacOSPassiveShellWindow {
			window_key: ns_window as usize,
			view_key: view as usize,
			tracking_area_key: tracking_area as usize,
			toolbar_state,
		})
	}
}

fn macos_overlay_window_ns_window(window: &Window) -> Option<*mut Object> {
	let ns_view = super::macos_overlay_window_ns_view(window)?;

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

fn macos_union_ns_rect(lhs: NSRect, rhs: NSRect) -> NSRect {
	let min_x = lhs.origin.x.min(rhs.origin.x);
	let min_y = lhs.origin.y.min(rhs.origin.y);
	let max_x = (lhs.origin.x + lhs.size.width).max(rhs.origin.x + rhs.size.width);
	let max_y = (lhs.origin.y + lhs.size.height).max(rhs.origin.y + rhs.size.height);

	NSRect::new(NSPoint::new(min_x, min_y), NSSize::new(max_x - min_x, max_y - min_y))
}
