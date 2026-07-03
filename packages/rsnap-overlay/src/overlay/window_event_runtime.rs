use std::sync::Arc;
use std::time::Instant;

use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::window::WindowId;

use crate::overlay::hud_geometry::LOUPE_TILE_CORNER_RADIUS_POINTS;
use crate::overlay::runtime_model::OverlayEventLoopPhase;
use crate::overlay::runtime_timing::SLOW_OP_WARN_WINDOW_EVENT;
use crate::overlay::{OverlayControl, OverlayExit, OverlayMode, OverlaySession};

impl OverlaySession {
	pub(in crate::overlay) fn maybe_stop_frozen_selection_drag_for_mouse_input(
		&mut self,
		state: ElementState,
		button: MouseButton,
	) {
		if state == ElementState::Released && button == MouseButton::Left {
			self.commit_frozen_arrow_drag();
			self.commit_frozen_spotlight_drag();
			self.commit_frozen_mosaic_drag();

			let _ = self.finish_frozen_brush_stroke();

			self.stop_frozen_selection_drag();
			self.sync_overlay_cursor_icons();
		}
	}

	fn inline_toolbar_size_wheel_active(&self, toolbar_window_id: bool) -> bool {
		!toolbar_window_id
			&& !cfg!(target_os = "macos")
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.toolbar_state.annotation_size_control_hovered
	}

	/// Handles a winit window event for one of the overlay-owned windows.
	#[allow(clippy::too_many_lines)]
	pub fn handle_window_event(
		&mut self,
		window_id: WindowId,
		event: &WindowEvent,
	) -> OverlayControl {
		let started_at = Instant::now();
		let kind = Self::window_event_kind(event);
		let now = Instant::now();

		self.event_loop_last_progress_window_id = Some(window_id);
		self.event_loop_last_progress_monitor_id =
			self.windows.get(&window_id).map(|window| window.monitor.id);

		self.maybe_log_event_loop_stall(now);
		self.mark_progress_with_detail(OverlayEventLoopPhase::WindowEvent, Some(kind));

		match event {
			WindowEvent::MouseInput { state, button, .. } => {
				self.maybe_stop_frozen_selection_drag_for_mouse_input(*state, *button);
			},
			WindowEvent::Focused(focused) => {
				self.note_window_focus_change(window_id, *focused);
			},
			_ => {},
		}

		if let Some(control) = self.handle_scroll_preview_event(window_id, event) {
			return control;
		}

		let toolbar_window_id = self
			.toolbar_window
			.as_ref()
			.is_some_and(|toolbar_window| toolbar_window.window.id() == window_id);
		let inline_toolbar_size_wheel_active =
			self.inline_toolbar_size_wheel_active(toolbar_window_id);
		let control = match event {
			WindowEvent::CloseRequested => self.cancel_overlay("window_close_requested"),
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button: MouseButton::Right,
				..
			} => self.cancel_overlay("window_right_click"),
			WindowEvent::Resized(size) if toolbar_window_id => {
				self.handle_toolbar_window_resized(*size)
			},
			WindowEvent::Moved(position) if toolbar_window_id => {
				self.handle_toolbar_window_moved(window_id, *position)
			},
			WindowEvent::Resized(size) => self.handle_resized(window_id, *size),
			WindowEvent::ScaleFactorChanged { .. } if toolbar_window_id => {
				self.handle_toolbar_window_scale_factor_changed(window_id)
			},
			WindowEvent::ScaleFactorChanged { .. } => self.handle_scale_factor_changed(window_id),
			WindowEvent::CursorEntered { .. } if toolbar_window_id => OverlayControl::Continue,
			WindowEvent::CursorLeft { .. } if toolbar_window_id => {
				self.handle_toolbar_cursor_left()
			},
			WindowEvent::CursorMoved { position, .. } => {
				if toolbar_window_id {
					self.handle_toolbar_cursor_moved(window_id, *position)
				} else {
					self.handle_cursor_moved(window_id, *position)
				}
			},
			#[cfg(target_os = "macos")]
			WindowEvent::Ime(_) => OverlayControl::Continue,
			#[cfg(not(target_os = "macos"))]
			WindowEvent::Ime(event) => self.handle_ime_event(window_id, event),
			WindowEvent::MouseWheel { delta, .. } if toolbar_window_id => {
				self.handle_toolbar_mouse_wheel(delta)
			},
			WindowEvent::MouseWheel { delta, .. } if inline_toolbar_size_wheel_active => {
				self.handle_toolbar_mouse_wheel(delta)
			},
			WindowEvent::MouseWheel { delta, .. } => {
				self.handle_scroll_mouse_wheel(window_id, delta)
			},
			WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
				if toolbar_window_id {
					self.handle_toolbar_mouse_input(*state)
				} else {
					self.handle_left_mouse_input(window_id, *state)
				}
			},
			WindowEvent::RedrawRequested if toolbar_window_id => {
				self.handle_toolbar_window_redraw_requested()
			},
			WindowEvent::ThemeChanged(_) => {
				// Keep the HUD palette in sync with system changes when ThemeMode::System is active.
				if let Some(monitor) = self.windows.get(&window_id).map(|w| w.monitor) {
					self.request_redraw_for_monitor(monitor);
				} else {
					self.request_redraw_all();
				}

				OverlayControl::Continue
			},
			#[cfg(target_os = "macos")]
			WindowEvent::KeyboardInput { .. } => OverlayControl::Continue,
			#[cfg(not(target_os = "macos"))]
			WindowEvent::KeyboardInput { event, .. } => self.handle_key_event(event),
			#[cfg(target_os = "macos")]
			WindowEvent::ModifiersChanged(_) => OverlayControl::Continue,
			#[cfg(not(target_os = "macos"))]
			WindowEvent::ModifiersChanged(modifiers) => self.handle_modifiers_changed(modifiers),
			WindowEvent::RedrawRequested => self.handle_redraw_requested(window_id),
			_ => OverlayControl::Continue,
		};

		self.slow_op_logger.warn_if_slow(
			"overlay.window_event",
			started_at.elapsed(),
			SLOW_OP_WARN_WINDOW_EVENT,
			|| format!("kind={kind} window_id={window_id:?} toolbar_window={toolbar_window_id}"),
		);

		control
	}

	fn handle_resized(&mut self, window_id: WindowId, size: PhysicalSize<u32>) -> OverlayControl {
		let window_scale_factor = self
			.windows
			.get(&window_id)
			.map(|w| w.window.scale_factor())
			.or_else(|| self.hud_window.as_ref().map(|w| w.window.scale_factor()))
			.or_else(|| self.loupe_window.as_ref().map(|w| w.window.scale_factor()));

		tracing::trace!(?window_id, ?size, ?window_scale_factor, "WindowEvent::Resized");

		if let Some(hud_window) = self.hud_window.as_mut()
			&& hud_window.window.id() == window_id
		{
			let window = Arc::clone(&hud_window.window);

			match hud_window.renderer.resize(size) {
				Ok(()) => {
					self.configure_hud_window_common(window.as_ref(), None);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}
		if let Some(loupe_window) = self.loupe_window.as_mut()
			&& loupe_window.window.id() == window_id
		{
			let window = Arc::clone(&loupe_window.window);

			match loupe_window.renderer.resize(size) {
				Ok(()) => {
					self.configure_hud_window_common(
						window.as_ref(),
						Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}

		let Some(overlay_window) = self.windows.get_mut(&window_id) else {
			return OverlayControl::Continue;
		};

		match overlay_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn handle_scale_factor_changed(&mut self, window_id: WindowId) -> OverlayControl {
		let window_scale_factor = self
			.windows
			.get(&window_id)
			.map(|w| w.window.scale_factor())
			.or_else(|| self.hud_window.as_ref().map(|w| w.window.scale_factor()))
			.or_else(|| self.loupe_window.as_ref().map(|w| w.window.scale_factor()));

		tracing::trace!(?window_id, ?window_scale_factor, "WindowEvent::ScaleFactorChanged");

		if let Some(hud_window) = self.hud_window.as_mut()
			&& hud_window.window.id() == window_id
		{
			let size = hud_window.window.inner_size();
			let window = Arc::clone(&hud_window.window);

			match hud_window.renderer.resize(size) {
				Ok(()) => {
					self.configure_hud_window_common(window.as_ref(), None);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}
		if let Some(loupe_window) = self.loupe_window.as_mut()
			&& loupe_window.window.id() == window_id
		{
			let size = loupe_window.window.inner_size();
			let window = Arc::clone(&loupe_window.window);

			match loupe_window.renderer.resize(size) {
				Ok(()) => {
					self.configure_hud_window_common(
						window.as_ref(),
						Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}

		let Some(overlay_window) = self.windows.get_mut(&window_id) else {
			return OverlayControl::Continue;
		};
		let size = overlay_window.window.inner_size();

		match overlay_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}
}
