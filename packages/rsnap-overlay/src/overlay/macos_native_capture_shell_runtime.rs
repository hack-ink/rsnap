mod macos_key_focus_shell_runtime;
mod macos_passive_shell_runtime;
mod shell_model;
mod shell_windows;

use std::collections::HashMap;
use std::sync::Arc;

use egui::FontId;
use objc::runtime::{BOOL, Object, Sel, YES};
use objc2_foundation::{NSPoint, NSSize};
use winit::event::ElementState;
use winit::keyboard::Key;

use self::shell_model::{MacOSKeyFocusShellKind, MacOSKeyFocusShellTarget};
use self::shell_windows::{MacOSKeyFocusShellWindow, MacOSPassiveShellWindow};
use crate::overlay::MacOSFrontmostApplication;
use crate::overlay::{
	GlobalPoint, MacOSNativeCaptureInputDispatch, MacOSNativeCaptureInputEvent, MonitorRect,
	OverlayKeyboardInputEvent, OverlayMode, OverlaySession, Window,
};

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
			self.toolbar_shell = Some(shell_windows::macos_create_passive_toolbar_shell_window(
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
			self.key_focus_shell = Some(shell_windows::macos_create_key_focus_shell_window(
				render_window,
				self.dispatch.clone(),
			)?);
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
			let shell = shell_windows::macos_create_passive_overlay_shell_window(
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
