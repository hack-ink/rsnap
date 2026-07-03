#[cfg(target_os = "macos")]
use crate::overlay::toolbar_layout_model;
use crate::overlay::{
	ElementState, GlobalPoint, MouseScrollDelta, OverlayControl, OverlayMode, OverlaySession, Pos2,
	Vec2, WindowRenderer,
};

impl OverlaySession {
	pub(super) fn handle_toolbar_mouse_input(&mut self, state: ElementState) -> OverlayControl {
		let toolbar_left_button_down = matches!(state, ElementState::Pressed);

		if toolbar_left_button_down == self.toolbar_left_button_down {
			return OverlayControl::Continue;
		}
		if toolbar_left_button_down {
			self.toolbar_left_button_went_down = true;
		} else {
			self.toolbar_left_button_went_up = true;
		}

		self.toolbar_left_button_down = toolbar_left_button_down;

		if !toolbar_left_button_down {
			if self.toolbar_state.dragging
				&& let Some(monitor) = self.state.monitor.or_else(|| self.active_cursor_monitor())
				&& let Some(toolbar_window) = self.toolbar_window.as_ref()
				&& let Some(outer_position) = Self::toolbar_window_outer_position(toolbar_window)
			{
				let _ = self.sync_toolbar_outer_position_from_window(monitor, outer_position);

				self.force_apply_pending_toolbar_window_move();
			}

			let _ = self.commit_frozen_arrow_drag();
			let _ = self.commit_frozen_spotlight_drag();
			let _ = self.commit_frozen_mosaic_drag();
			let _ = self.finish_frozen_brush_stroke();

			self.stop_frozen_selection_drag();

			let _ = self.stop_frozen_text_edit_drag();

			self.toolbar_state.dragging = false;
			self.toolbar_state.drag_start_eligible = false;
			self.toolbar_state.drag_offset = Vec2::ZERO;
			self.toolbar_state.drag_anchor = None;
		} else {
			self.toolbar_state.drag_offset = Vec2::ZERO;
			self.toolbar_state.dragging = false;
			self.toolbar_state.drag_anchor = None;

			let current_cursor_local = self.current_toolbar_cursor_local();

			self.toolbar_state.drag_start_eligible =
				self.resolve_toolbar_drag_start_eligibility(current_cursor_local);
		}

		#[cfg(target_os = "macos")]
		{
			self.request_redraw_toolbar_window();

			if !toolbar_left_button_down && self.frozen_text_edit.is_some() {
				self.focus_frozen_text_input_window(self.state.monitor);
			}
		}

		OverlayControl::Continue
	}

	pub(super) fn handle_toolbar_mouse_wheel(
		&mut self,
		delta: &MouseScrollDelta,
	) -> OverlayControl {
		if !matches!(self.state.mode, OverlayMode::Frozen) || !self.toolbar_state.visible {
			return OverlayControl::Continue;
		}
		if !self.toolbar_state.apply_annotation_size_wheel_delta(delta) {
			return OverlayControl::Continue;
		}

		self.toolbar_state.needs_redraw = true;

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		#[cfg(target_os = "macos")]
		{
			self.request_redraw_toolbar_window();
		}

		OverlayControl::Continue
	}

	pub(super) fn reset_toolbar_pointer_state(&mut self) {
		self.toolbar_left_button_down = false;
		self.toolbar_left_button_went_down = false;
		self.toolbar_left_button_went_up = false;
		self.toolbar_pointer_local = None;
		self.toolbar_state.annotation_size_control_hovered = false;
		self.toolbar_state.annotation_size_wheel_accumulator = 0.0;
		self.toolbar_state.drag_start_eligible = false;
		self.toolbar_state.drag_anchor = None;
	}

	pub(super) fn resolve_toolbar_drag_start_eligibility(
		&self,
		current_cursor_local: Option<Pos2>,
	) -> bool {
		current_cursor_local
			.or(self.toolbar_pointer_local)
			.is_some_and(|cursor_local| self.toolbar_primary_rect_contains(cursor_local))
	}

	fn current_toolbar_cursor_local(&mut self) -> Option<Pos2> {
		let toolbar_window = self.toolbar_window.as_ref()?;
		let outer_position = Self::toolbar_window_outer_position(toolbar_window)?;
		#[cfg(target_os = "macos")]
		let cursor_global = super::macos_mouse_location()?;
		#[cfg(not(target_os = "macos"))]
		let cursor_global = self.sample_mouse_location();

		Self::toolbar_cursor_local_from_sampled_global(outer_position, Some(cursor_global))
	}

	fn toolbar_primary_rect_contains(&self, cursor_local: Pos2) -> bool {
		#[cfg(target_os = "macos")]
		let toolbar_primary_origin = toolbar_layout_model::frozen_toolbar_window_primary_origin();
		#[cfg(not(target_os = "macos"))]
		let toolbar_primary_origin = Pos2::ZERO;

		WindowRenderer::frozen_toolbar_primary_rect(&self.toolbar_state, toolbar_primary_origin)
			.contains(cursor_local)
	}

	pub(super) fn toolbar_cursor_local_position_from_outer(
		outer_position: GlobalPoint,
		global_cursor: GlobalPoint,
	) -> Pos2 {
		Pos2::new(
			global_cursor.x as f32 - outer_position.x as f32,
			global_cursor.y as f32 - outer_position.y as f32,
		)
	}

	pub(super) fn toolbar_cursor_local_from_sampled_global(
		outer_position: GlobalPoint,
		sampled_cursor: Option<GlobalPoint>,
	) -> Option<Pos2> {
		sampled_cursor.map(|global_cursor| {
			Self::toolbar_cursor_local_position_from_outer(outer_position, global_cursor)
		})
	}
}
