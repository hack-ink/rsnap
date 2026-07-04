mod macos_key_focus_shell_runtime;
mod macos_passive_shell_runtime;
mod shell_model;

use std::collections::HashMap;
use std::ptr;
use std::sync::{Arc, Mutex};

use egui::FontId;
use objc::runtime::{BOOL, NO, Object, Sel, YES};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use winit::event::ElementState;
use winit::keyboard::Key;

use self::shell_model::{
	MacOSKeyFocusShellKind, MacOSKeyFocusShellState, MacOSKeyFocusShellTarget,
	MacOSPassiveShellCallback, MacOSPassiveToolbarShellState,
};
use crate::overlay::MacOSFrontmostApplication;
use crate::overlay::{
	GlobalPoint, MacOSNativeCaptureInputDispatch, MacOSNativeCaptureInputEvent, MonitorRect,
	OverlayKeyboardInputEvent, OverlayMode, OverlaySession, Window,
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

	fn sync_overlay_shells(&self, windows: &[MacOSCaptureHostOverlayShell], visible: bool) {
		tracing::trace!(
			op = "overlay.macos_passive_shell_sync_overlay_shells",
			visible,
			window_count = windows.len(),
			"Synced passive overlay shells."
		);

		for overlay_window in windows {
			if let Some(shell) = self.overlay_shells.get(&overlay_window.monitor.id) {
				shell.sync_from_render_window(overlay_window.render_window.as_ref(), visible);
			}
		}

		if visible {
			macos_passive_shell_runtime::macos_set_crosshair_cursor();
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

/// Explicit host-owned macOS capture state derived from the overlay core.
pub struct MacOSCaptureHostSyncState {
	overlay_shells: Vec<MacOSCaptureHostOverlayShell>,
	live_pointer_shell_visible: bool,
	toolbar_shell: Option<MacOSCaptureHostToolbarShell>,
	toolbar_pointer_shell_visible: bool,
	key_focus_target: Option<MacOSCaptureHostKeyFocusTarget>,
	frozen_mode_active: bool,
}

/// App-owned macOS capture host adapter that owns native shell lifecycle,
/// host-routed capture input dispatch, and focus restoration side effects.
pub struct MacOSCaptureHost {
	native_capture_shells: Option<MacOSNativeCaptureShells>,
	native_capture_input_dispatch: MacOSNativeCaptureInputDispatch,
	frontmost_application_before_start: Option<MacOSFrontmostApplication>,
	last_synced_frozen_mode: bool,
}
impl MacOSCaptureHost {
	/// Creates a new host adapter that forwards native capture events to the caller.
	pub fn new(event_sink: Arc<dyn Fn(MacOSNativeCaptureInputEvent) + Send + Sync>) -> Self {
		Self {
			native_capture_shells: None,
			native_capture_input_dispatch: MacOSNativeCaptureInputDispatch { sink: event_sink },
			frontmost_application_before_start: None,
			last_synced_frozen_mode: false,
		}
	}

	/// Captures the currently frontmost application before starting a capture session.
	pub fn begin_session(&mut self) {
		self.frontmost_application_before_start = super::macos_frontmost_application();
		self.last_synced_frozen_mode = false;

		// AppKit only keeps native cursor ownership stable while Rsnap is the active app. Capture
		// the prior frontmost app for teardown restore, then activate Rsnap for the overlay session.
		super::macos_activate_app();

		tracing::info!(
			op = "overlay.frontmost_app_captured",
			target_process_id =
				self.frontmost_application_before_start.map(|target| target.process_id),
			"Captured the pre-capture frontmost application for later restore."
		);
	}

	/// Cancels a session start before any host-owned shells were synchronized.
	pub fn cancel_session_start(&mut self) {
		let target = self.frontmost_application_before_start.take();

		self.restore_frontmost_application_after_exit(target);
	}

	#[doc(hidden)]
	pub fn debug_dispatch_native_capture_input(&self, event: MacOSNativeCaptureInputEvent) {
		self.native_capture_input_dispatch.enqueue(event);
	}

	#[doc(hidden)]
	pub fn debug_dispatch_keyboard_input(
		&self,
		monitor: Option<MonitorRect>,
		logical_key: Key,
		text: Option<&str>,
	) {
		self.native_capture_input_dispatch.enqueue(MacOSNativeCaptureInputEvent::KeyboardInput {
			monitor,
			event: OverlayKeyboardInputEvent {
				logical_key,
				text: text.map(String::from),
				state: ElementState::Pressed,
				repeat: false,
			},
		});
	}

	#[doc(hidden)]
	pub fn debug_last_synced_frozen_mode(&self) -> bool {
		self.last_synced_frozen_mode
	}

	/// Synchronizes native macOS capture shells from explicit host/core state.
	pub fn sync(&mut self, state: MacOSCaptureHostSyncState) -> Result<(), String> {
		if self.native_capture_shells.is_some() {
			self.sync_native_capture_shells(state)?;

			return Ok(());
		}

		let mut overlay_shells = HashMap::with_capacity(state.overlay_shells.len());

		for overlay_window in &state.overlay_shells {
			let shell = macos_create_passive_overlay_shell_window(
				overlay_window.render_window.as_ref(),
				overlay_window.monitor,
				self.native_capture_input_dispatch.clone(),
			)?;

			overlay_shells.insert(overlay_window.monitor.id, shell);
		}

		self.native_capture_shells = Some(MacOSNativeCaptureShells::new(
			overlay_shells,
			self.native_capture_input_dispatch.clone(),
		));

		self.sync_native_capture_shells(state)?;

		Ok(())
	}

	fn sync_native_capture_shells(
		&mut self,
		state: MacOSCaptureHostSyncState,
	) -> Result<(), String> {
		let frozen_mode_active = state.frozen_mode_active;

		for overlay_window in &state.overlay_shells {
			super::macos_set_capture_window_mouse_passthrough(
				overlay_window.render_window.as_ref(),
				state.live_pointer_shell_visible,
			);
		}

		self.maybe_preserve_frontmost_application(&state);

		let Some(shells) = self.native_capture_shells.as_mut() else {
			if let Some(toolbar_shell) = state.toolbar_shell.as_ref() {
				super::macos_set_capture_window_mouse_passthrough(
					toolbar_shell.render_window.as_ref(),
					false,
				);
			}

			return Ok(());
		};

		shells.sync_overlay_shells(&state.overlay_shells, state.live_pointer_shell_visible);

		if let Some(toolbar_shell) = state.toolbar_shell {
			if let Some(placement) = toolbar_shell.placement {
				shells.sync_toolbar_shell(
					toolbar_shell.render_window.as_ref(),
					placement.monitor,
					placement.outer_position,
					state.toolbar_pointer_shell_visible,
				)?;

				super::macos_set_capture_window_mouse_passthrough(
					toolbar_shell.render_window.as_ref(),
					state.toolbar_pointer_shell_visible,
				);
			} else {
				super::macos_set_capture_window_mouse_passthrough(
					toolbar_shell.render_window.as_ref(),
					false,
				);

				shells.clear_toolbar_shell();
			}
		} else {
			shells.clear_toolbar_shell();
		}
		if let Some(key_focus_target) = state.key_focus_target {
			shells.sync_key_focus_shell(
				key_focus_target.render_window.as_ref(),
				key_focus_target.target,
				true,
			)?;
		} else {
			shells.clear_key_focus_shell();
		}

		self.last_synced_frozen_mode = frozen_mode_active;

		Ok(())
	}

	fn destroy_native_capture_shells(&mut self, session: &OverlaySession) {
		for overlay_window in session.windows.values() {
			super::macos_set_capture_window_mouse_passthrough(
				overlay_window.window.as_ref(),
				false,
			);
		}

		if let Some(toolbar_window) = session.toolbar_window.as_ref() {
			super::macos_set_capture_window_mouse_passthrough(
				toolbar_window.window.as_ref(),
				false,
			);
		}

		self.native_capture_shells = None;
		self.last_synced_frozen_mode = false;

		let target = self.frontmost_application_before_start.take();

		self.restore_frontmost_application_after_exit(target);
	}

	fn maybe_preserve_frontmost_application(&mut self, _state: &MacOSCaptureHostSyncState) {
		// Do not hand focus back during an active overlay session. Live crosshair and frozen hover
		// cursors are native AppKit cursors; restoring the previous frontmost app during capture
		// hands visible cursor ownership back to that app and leaves Rsnap showing an arrow until
		// the next direct interaction. The original app is restored when the session exits.
	}

	fn restore_frontmost_application_after_exit(&self, target: Option<MacOSFrontmostApplication>) {
		let Some(target) = target else {
			tracing::info!(
				op = "overlay.frontmost_app_restore_attempted",
				target = "none",
				"Skipped restoring the pre-capture frontmost application because none was recorded."
			);

			return;
		};
		let restored = super::macos_restore_frontmost_application(target);

		tracing::info!(
			op = "overlay.frontmost_app_restore_attempted",
			target_process_id = target.process_id,
			restored,
			"Attempted to restore the pre-capture frontmost application."
		);
	}
}

struct MacOSCaptureHostOverlayShell {
	monitor: MonitorRect,
	render_window: Arc<Window>,
}

struct MacOSCaptureHostToolbarPlacement {
	monitor: MonitorRect,
	outer_position: GlobalPoint,
}

struct MacOSCaptureHostToolbarShell {
	render_window: Arc<Window>,
	placement: Option<MacOSCaptureHostToolbarPlacement>,
}

struct MacOSCaptureHostKeyFocusTarget {
	render_window: Arc<Window>,
	target: MacOSKeyFocusShellTarget,
}

struct MacOSPassiveShellWindow {
	window_key: usize,
	view_key: usize,
	tracking_area_key: usize,
	toolbar_state: Option<Arc<Mutex<MacOSPassiveToolbarShellState>>>,
}
impl MacOSPassiveShellWindow {
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

		if let Some(callback) = self::shell_model::macos_shell_callback(self.view_key) {
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
		self::shell_model::macos_unregister_shell_callback(self.view_key);
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

struct MacOSKeyFocusShellWindow {
	window_key: usize,
	view_key: usize,
	state: Arc<Mutex<MacOSKeyFocusShellState>>,
}
impl MacOSKeyFocusShellWindow {
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

		super::macos_activate_app();

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
		self::shell_model::macos_unregister_shell_callback(self.view_key);

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

impl OverlaySession {
	/// Tears down the app-owned macOS capture host for the current session.
	pub fn teardown_macos_capture_host(&mut self, host: &mut MacOSCaptureHost) {
		host.destroy_native_capture_shells(self);
	}

	/// Re-applies cursor authority after the host has switched native shell visibility or
	/// passthrough ownership.
	pub fn refresh_macos_cursor_after_host_sync(&self) {
		self.apply_macos_cursor_authority();
	}

	/// Builds the explicit macOS host sync state for the current overlay session.
	pub fn take_macos_capture_host_sync_state(&mut self) -> MacOSCaptureHostSyncState {
		let overlay_shells = self
			.windows
			.values()
			.map(|overlay_window| MacOSCaptureHostOverlayShell {
				monitor: overlay_window.monitor,
				render_window: Arc::clone(&overlay_window.window),
			})
			.collect();
		let toolbar_shell = self.toolbar_window.as_ref().map(|toolbar_window| {
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
			let placement = monitor.zip(outer_position).map(|(monitor, outer_position)| {
				MacOSCaptureHostToolbarPlacement { monitor, outer_position }
			});

			MacOSCaptureHostToolbarShell {
				render_window: Arc::clone(&toolbar_window.window),
				placement,
			}
		});

		if self.toolbar_window_visible && self.preserve_frontmost_on_next_toolbar_show {
			self.preserve_frontmost_on_next_toolbar_show = false;
		}

		MacOSCaptureHostSyncState {
			overlay_shells,
			live_pointer_shell_visible: self.should_host_live_pointer_input_in_native_shell(),
			toolbar_shell,
			toolbar_pointer_shell_visible: self.should_host_toolbar_pointer_input_in_native_shell(),
			frozen_mode_active: matches!(self.state.mode, OverlayMode::Frozen),
			key_focus_target: self.native_key_focus_shell_target(),
		}
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

	fn native_key_focus_shell_target(&self) -> Option<MacOSCaptureHostKeyFocusTarget> {
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

			return Some(MacOSCaptureHostKeyFocusTarget { render_window, target });
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

		Some(MacOSCaptureHostKeyFocusTarget {
			render_window: Arc::clone(&overlay_window.window),
			target,
		})
	}
}

extern "C" fn macos_capture_shell_view_is_flipped(_this: &Object, _cmd: Sel) -> BOOL {
	YES
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

		self::shell_model::macos_register_shell_callback(
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

		self::shell_model::macos_register_shell_callback(view as usize, callback);

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
