mod cursor_movement;

use std::time::Instant;

#[cfg(target_os = "macos")]
use winit::keyboard::ModifiersState;

use crate::overlay::live_capture_target::LiveClickCaptureTarget;
use crate::overlay::{
	ElementState, FrozenToolbarTool, GlobalPoint, LiveCaptureInteraction, Modifiers, MonitorRect,
	OverlayControl, OverlayMode, OverlaySession, WindowId,
};

impl OverlaySession {
	pub(super) fn note_window_focus_change(&mut self, window_id: WindowId, focused: bool) {
		if focused {
			self.focused_window_ids.insert(window_id);

			self.pending_focus_loss_cleanup = false;

			return;
		}

		let was_focused = self.focused_window_ids.remove(&window_id);

		if was_focused && self.focused_window_ids.is_empty() {
			self.pending_focus_loss_cleanup = true;
		}
	}

	pub(super) fn handle_modifiers_changed(&mut self, modifiers: &Modifiers) -> OverlayControl {
		self.keyboard_modifiers = modifiers.state();

		OverlayControl::Continue
	}

	#[cfg(target_os = "macos")]
	pub(super) fn handle_modifiers_state_changed(
		&mut self,
		modifiers: ModifiersState,
	) -> OverlayControl {
		self.keyboard_modifiers = modifiers;

		OverlayControl::Continue
	}

	pub(super) fn handle_left_mouse_input(
		&mut self,
		window_id: WindowId,
		state: ElementState,
	) -> OverlayControl {
		let monitor = self
			.windows
			.get(&window_id)
			.map(|w| w.monitor)
			.or_else(|| self.active_cursor_monitor())
			.or(self.state.monitor);
		let Some(monitor) = monitor else {
			return OverlayControl::Continue;
		};

		if matches!(self.state.mode, OverlayMode::Frozen) {
			return self.handle_frozen_left_mouse_input(monitor, state);
		}
		if !matches!(self.state.mode, OverlayMode::Live) {
			return OverlayControl::Continue;
		}
		if self.frozen_display_handoff_pending() {
			return OverlayControl::Continue;
		}

		self.maybe_timeout_pending_click_hit_test(Instant::now());

		match state {
			ElementState::Pressed => {
				let raw_cursor = self.current_device_cursor();
				let (press_monitor, press_global) = if let Some((press_monitor, press_global, _)) =
					self.resolve_live_cursor_point(raw_cursor)
				{
					(press_monitor, press_global)
				} else {
					(monitor, raw_cursor)
				};

				self.handle_live_overlay_left_mouse_input(press_monitor, press_global, state)
			},
			ElementState::Released => {
				let raw_cursor = self.current_device_cursor();
				let release_global = if let Some((_, release_global, _)) =
					self.resolve_live_cursor_point(raw_cursor)
				{
					release_global
				} else {
					raw_cursor
				};

				self.handle_live_overlay_left_mouse_input(monitor, release_global, state)
			},
		}
	}

	pub(super) fn handle_live_overlay_left_mouse_input(
		&mut self,
		monitor: MonitorRect,
		global: GlobalPoint,
		state: ElementState,
	) -> OverlayControl {
		if matches!(self.state.mode, OverlayMode::Frozen) {
			self.last_event_cursor = Some((monitor, global));
			self.last_event_cursor_at = Some(Instant::now());

			self.update_cursor_state(monitor, global);

			return self.handle_frozen_left_mouse_input(monitor, state);
		}
		if !matches!(self.state.mode, OverlayMode::Live) {
			return OverlayControl::Continue;
		}
		if self.frozen_display_handoff_pending() {
			return OverlayControl::Continue;
		}

		self.maybe_timeout_pending_click_hit_test(Instant::now());

		match state {
			ElementState::Pressed => {
				if self.live_capture_interaction_is_press_pending()
					|| self.live_capture_interaction_is_dragging()
				{
					return OverlayControl::Continue;
				}

				self.last_event_cursor = Some((monitor, global));
				self.last_event_cursor_at = Some(Instant::now());

				self.update_cursor_state(monitor, global);
				self.update_hud_window_position(monitor, global);
				self.begin_live_capture_press(monitor, global);

				if matches!(
					self.live_capture_interaction,
					LiveCaptureInteraction::PressPending { click_target: None, .. }
				) {
					self.request_click_capture_hit_test(monitor, global);
				}

				self.reset_toolbar_pointer_state();
				self.request_redraw_for_monitor(monitor);

				OverlayControl::Continue
			},
			ElementState::Released => {
				match self.live_capture_interaction {
					LiveCaptureInteraction::PressPending {
						monitor: press_monitor,
						press_global,
						click_target,
						..
					} => {
						if let Some(target) = click_target {
							self.begin_frozen_capture_from_click(press_monitor, target, global);
						} else if self.pending_click_hit_test_request_id.is_some() {
							self.set_live_capture_interaction(
								LiveCaptureInteraction::PressPending {
									monitor: press_monitor,
									press_global,
									click_target: None,
									release_global: Some(global),
									released: true,
								},
							);
						} else {
							self.begin_frozen_capture_from_click(
								press_monitor,
								LiveClickCaptureTarget::fullscreen_fallback(),
								global,
							);
						}
					},
					LiveCaptureInteraction::DraggingSelection { monitor, .. } => {
						if let Some(drag_rect) =
							self.state.drag_rect.filter(|rect| rect.monitor_id == monitor.id)
						{
							self.begin_frozen_capture_from_drag(monitor, drag_rect.rect, global);
						} else {
							self.set_live_capture_interaction(LiveCaptureInteraction::Idle);
						}
					},
					_ => {},
				}

				OverlayControl::Continue
			},
		}
	}

	pub(super) fn handle_frozen_left_mouse_input(
		&mut self,
		monitor: MonitorRect,
		state: ElementState,
	) -> OverlayControl {
		self.reset_toolbar_pointer_state();

		if self.frozen_text_tool_active() {
			match state {
				ElementState::Pressed => {
					let cursor = self.current_device_cursor();
					let started_drag = self.begin_frozen_text_edit_drag_at(monitor, cursor);

					if !started_drag {
						let started = self.begin_frozen_text_edit_at(monitor, cursor);

						if !started {
							let _ = self.finish_frozen_text_editing(true);
						}
					}

					self.sync_overlay_cursor_icons();
				},
				ElementState::Released => {
					let stopped_drag = self.stop_frozen_text_edit_drag();

					if stopped_drag {
						self.sync_overlay_cursor_icons();
					}
				},
			}

			self.request_redraw_for_monitor(monitor);

			return OverlayControl::Continue;
		}
		if self.frozen_text_edit.is_some() {
			let _ = self.finish_frozen_text_editing(true);
		}

		match state {
			ElementState::Pressed => {
				let cursor = self.current_frozen_interaction_cursor();

				match self.toolbar_state.selected_tool {
					FrozenToolbarTool::Pen => {
						let _ = self.begin_frozen_brush_stroke(cursor);
					},
					FrozenToolbarTool::Arrow => {
						let _ = self.begin_frozen_arrow_drag(cursor);
					},
					FrozenToolbarTool::Spotlight => {
						let _ = self.begin_frozen_spotlight_drag(cursor);
					},
					FrozenToolbarTool::Mosaic => {
						let _ = self.begin_frozen_mosaic_drag(cursor);
					},
					_ => {
						let _ = self.begin_frozen_selection_drag(cursor);
					},
				}

				self.sync_overlay_cursor_icons();
			},
			ElementState::Released => {
				let _ = self.commit_frozen_arrow_drag();
				let _ = self.commit_frozen_spotlight_drag();
				let _ = self.commit_frozen_mosaic_drag();
				let _ = self.finish_frozen_brush_stroke();

				self.stop_frozen_selection_drag();
				self.sync_overlay_cursor_icons();
			},
		}

		self.request_redraw_for_monitor(monitor);

		OverlayControl::Continue
	}
}
