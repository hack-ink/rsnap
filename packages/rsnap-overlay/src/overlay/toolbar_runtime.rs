mod pointer_motion;

#[cfg(target_os = "macos")]
use crate::overlay::Pos2;
#[cfg(target_os = "macos")]
use crate::overlay::toolbar_geometry::TOOLBAR_WINDOW_WARMUP_REDRAWS;
use crate::overlay::toolbar_layout_model;
use crate::overlay::{
	Arc, Duration, FrozenToolbarPointerState, GlobalPoint, Instant, MonitorRect, OverlayControl,
	OverlayEventLoopPhase, OverlayExit, OverlayMode, OverlaySession, PhysicalPosition,
	PhysicalSize, Result, WindowId, WindowRenderer,
};
#[cfg(target_os = "macos")]
use crate::overlay::{FrozenCaptureSource, HudAnchor};

impl OverlaySession {
	pub(super) fn handle_toolbar_window_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<i32>,
	) -> OverlayControl {
		let Some((toolbar_window_id, toolbar_scale)) =
			self.toolbar_window.as_ref().map(|toolbar_window| {
				(toolbar_window.window.id(), toolbar_window.window.scale_factor().max(1.0))
			})
		else {
			return OverlayControl::Continue;
		};

		if toolbar_window_id != window_id
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
		{
			return OverlayControl::Continue;
		}

		let Some(monitor) = self.state.monitor.or_else(|| self.active_cursor_monitor()) else {
			return OverlayControl::Continue;
		};
		let outer_position = GlobalPoint::new(
			(f64::from(position.x) / toolbar_scale).round() as i32,
			(f64::from(position.y) / toolbar_scale).round() as i32,
		);
		let changed = self.sync_toolbar_outer_position_from_window(monitor, outer_position);

		#[cfg(target_os = "macos")]
		self.sync_toolbar_window_cursor_hittest(super::macos_mouse_location());

		if self.pending_toolbar_outer_pos.is_some() {
			self.force_apply_pending_toolbar_window_move();
		} else {
			self.last_toolbar_window_move_at = Instant::now();
		}
		if changed {
			self.request_redraw_toolbar_window();
		}

		OverlayControl::Continue
	}

	pub(super) fn handle_toolbar_window_resized(
		&mut self,
		size: PhysicalSize<u32>,
	) -> OverlayControl {
		let Some(toolbar_window) = self.toolbar_window.as_mut() else {
			return OverlayControl::Continue;
		};

		match toolbar_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	pub(super) fn handle_toolbar_window_scale_factor_changed(
		&mut self,
		window_id: WindowId,
	) -> OverlayControl {
		let Some(toolbar_window) = self
			.toolbar_window
			.as_mut()
			.filter(|toolbar_window| toolbar_window.window.id() == window_id)
		else {
			return OverlayControl::Continue;
		};
		let size = toolbar_window.window.inner_size();

		match toolbar_window.renderer.resize(size) {
			Ok(()) => {
				let window = Arc::clone(&toolbar_window.window);
				let toolbar_height_points =
					WindowRenderer::frozen_toolbar_primary_size(&self.toolbar_state).y;

				self.configure_hud_window_common(
					window.as_ref(),
					Some(toolbar_layout_model::frozen_toolbar_corner_radius_points(
						toolbar_height_points,
					)),
				);

				OverlayControl::Continue
			},
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	pub(super) fn should_hide_toolbar_window(&self, monitor: MonitorRect) -> bool {
		self.frozen_selection_drag_hides_auxiliary_windows()
			|| !self.frozen_display_ready_for_monitor(monitor)
			|| !self.toolbar_state.visible
	}

	#[cfg(test)]
	pub(super) fn should_focus_frozen_toolbar_window_on_show(&self) -> bool {
		false
	}

	pub(super) fn set_toolbar_window_hidden(&mut self) {
		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			#[cfg(target_os = "macos")]
			let _ = toolbar_window.window.set_cursor_hittest(false);

			toolbar_window.window.set_visible(false);
		}

		self.toolbar_window_visible = false;
		self.toolbar_window_drawn_once = false;
		self.toolbar_badge_slot_ready = false;
		#[cfg(target_os = "macos")]
		{
			self.toolbar_window_cursor_hittest_enabled = false;
		}
		self.toolbar_window_warmup_redraws_remaining = 0;
		self.last_present_at = Instant::now();
	}

	pub(super) fn draw_toolbar_window_frame(
		&mut self,
		monitor: MonitorRect,
		toolbar_input: Option<FrozenToolbarPointerState>,
	) -> Result<()> {
		self.sync_frozen_toolbar_state();

		if self.maybe_recenter_frozen_toolbar_default_slot(monitor) {
			self.request_redraw_for_monitor(monitor);
		}

		#[cfg(not(target_os = "macos"))]
		{
			let _ = (&monitor, &toolbar_input);
			let Some(toolbar_window) = self.toolbar_window.as_ref() else {
				return Ok(());
			};

			toolbar_window.window.set_visible(false);

			self.last_present_at = Instant::now();

			Ok(())
		}
		#[cfg(target_os = "macos")]
		{
			let Some(toolbar_became_visible) = self.prepare_toolbar_window_for_draw(monitor) else {
				return Ok(());
			};

			self.sync_frozen_annotation_style_capsule_placement(monitor);

			let Some(gpu) = self.gpu.as_ref() else {
				return Ok(());
			};
			let previous_floating_position = self.toolbar_state.floating_position;
			let frozen_arrow_preview = self.active_frozen_arrow_preview();

			self.toolbar_state.floating_position =
				Some(toolbar_layout_model::frozen_toolbar_window_primary_origin());

			let Some(toolbar_window) = self.toolbar_window.as_mut() else {
				return Ok(());
			};
			let draw_result = toolbar_window.renderer.draw(
				gpu,
				&self.state,
				monitor,
				false,
				Some(Pos2::ZERO),
				false,
				HudAnchor::Cursor,
				self.config.toolbar_placement,
				self.config.show_alt_hint_keycap,
				false,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
				self.config.selection_flow_enabled,
				self.config.selection_flow_stroke_width_px,
				false,
				false,
				false,
				None,
				false,
				false,
				self.frozen_capture_source,
				self.frozen_capture_source == FrozenCaptureSource::FullscreenFallback,
				None,
				&[],
				None,
				&self.frozen_arrow_annotations,
				frozen_arrow_preview.as_ref(),
				&self.frozen_spotlight_annotations,
				self.frozen_spotlight_preview_rect,
				&self.frozen_text_annotations,
				self.frozen_text_edit.as_ref(),
				self.toolbar_state.text_style,
				Some(&mut self.toolbar_state),
				toolbar_input,
			);

			self.toolbar_state.floating_position = previous_floating_position;

			draw_result?;

			let first_toolbar_draw = !self.toolbar_window_drawn_once;

			self.toolbar_window_drawn_once = true;

			if first_toolbar_draw {
				self.note_frozen_transition_toolbar_first_draw(monitor);
			}
			if toolbar_became_visible {
				self.note_frozen_transition_toolbar_visible(monitor);
			}

			Ok(())
		}
	}

	#[cfg(target_os = "macos")]
	fn prepare_toolbar_window_for_draw(&mut self, monitor: MonitorRect) -> Option<bool> {
		if !self.toolbar_window_visible {
			self.maybe_apply_pending_startup_aux_live_stream_filter_upgrade(monitor);
		}

		let toolbar_window = self.toolbar_window.as_ref()?;

		toolbar_window.window.set_visible(true);

		let mut toolbar_became_visible = false;

		if !self.toolbar_window_visible {
			self.toolbar_window_visible = true;
			self.skip_toolbar_focus_on_next_show = false;
			self.toolbar_window_warmup_redraws_remaining = TOOLBAR_WINDOW_WARMUP_REDRAWS;
			toolbar_became_visible = true;
		}

		Some(toolbar_became_visible)
	}
	pub(super) fn handle_toolbar_window_redraw_requested(&mut self) -> OverlayControl {
		let redraw_started_at = Instant::now();

		self.event_loop_last_progress_window_id =
			self.toolbar_window.as_ref().map(|toolbar_window| toolbar_window.window.id());
		self.event_loop_last_progress_monitor_id = self.state.monitor.map(|monitor| monitor.id);

		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::ToolbarRedraw);

		let Some(monitor) = self.state.monitor else {
			return OverlayControl::Continue;
		};
		let toolbar_input = self.toolbar_pointer_state(monitor, self.toolbar_pointer_local);
		let should_hide_toolbar_window = self.should_hide_toolbar_window(monitor);

		if should_hide_toolbar_window {
			self.set_toolbar_window_hidden();

			return OverlayControl::Continue;
		}

		let draw_frame_started_at = Instant::now();

		if let Err(err) = self.draw_toolbar_window_frame(monitor, toolbar_input) {
			return self.exit(OverlayExit::Error(format!("{err:#}")));
		}

		if self.sync_frozen_text_edit_for_selected_tool() {
			self.request_redraw_for_monitor(monitor);
		}

		self.sync_overlay_cursor_icons();

		let draw_frame_elapsed = draw_frame_started_at.elapsed();

		self.update_scroll_toolbar_default_position(monitor);

		let position_update_elapsed = self.update_toolbar_position_after_redraw(monitor);

		#[cfg(target_os = "macos")]
		self.sync_toolbar_window_cursor_hittest(super::macos_mouse_location());

		if let Some(action) = self.toolbar_state.pending_action.take() {
			let control = self.handle_toolbar_action(action);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}

		self.last_present_at = Instant::now();

		if self.toolbar_state.needs_redraw {
			self.toolbar_state.needs_redraw = false;

			self.refresh_frozen_text_ime_cursor_area_for_text_style_change(monitor);
			self.request_redraw_for_monitor(monitor);
			self.request_redraw_toolbar_window();
		}

		self.log_toolbar_redraw_phase_timing(
			monitor,
			redraw_started_at,
			draw_frame_elapsed,
			position_update_elapsed,
			should_hide_toolbar_window,
		);

		OverlayControl::Continue
	}

	fn update_toolbar_position_after_redraw(&mut self, monitor: MonitorRect) -> Option<Duration> {
		let toolbar_pos = self.toolbar_state.floating_position?;
		let position_update_started_at = Instant::now();
		let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);

		self.force_apply_pending_toolbar_window_move();

		Some(position_update_started_at.elapsed())
	}

	#[cfg(target_os = "macos")]
	pub(super) fn sync_toolbar_window_cursor_hittest(
		&mut self,
		current_cursor: Option<GlobalPoint>,
	) {
		if self.should_host_toolbar_pointer_input_in_native_shell() {
			self.toolbar_window_cursor_hittest_enabled = false;

			if let Some(toolbar_window) = self.toolbar_window.as_ref() {
				let _ = toolbar_window.window.set_cursor_hittest(false);
			}

			return;
		}

		let enabled = self.toolbar_window_cursor_hittest_should_be_enabled(current_cursor);
		let Some(toolbar_window) = self.toolbar_window.as_ref() else {
			self.toolbar_window_cursor_hittest_enabled = false;

			return;
		};

		if enabled == self.toolbar_window_cursor_hittest_enabled {
			return;
		}

		let _ = toolbar_window.window.set_cursor_hittest(enabled);

		self.toolbar_window_cursor_hittest_enabled = enabled;
	}

	#[cfg(target_os = "macos")]
	fn toolbar_window_cursor_hittest_should_be_enabled(
		&self,
		current_cursor: Option<GlobalPoint>,
	) -> bool {
		if !self.toolbar_window_visible
			|| !self.frozen_display_ready()
			|| !self.toolbar_state.visible
		{
			return false;
		}

		let Some(monitor) = self.state.monitor.or_else(|| self.active_cursor_monitor()) else {
			return false;
		};
		let Some(cursor_global) = current_cursor.or(self.state.cursor) else {
			return false;
		};
		let window_toolbar_outer_pos =
			self.toolbar_window.as_ref().and_then(Self::toolbar_window_outer_position);
		let toolbar_outer_pos = window_toolbar_outer_pos
			.or(self.pending_toolbar_outer_pos)
			.or(self.toolbar_outer_pos)
			.or_else(|| {
				#[cfg(target_os = "macos")]
				{
					self.toolbar_state.floating_position.map(|floating_position| {
						self.toolbar_outer_position_from_primary_anchor(monitor, floating_position)
					})
				}
				#[cfg(not(target_os = "macos"))]
				{
					self.toolbar_state.floating_position.map(|floating_position| {
						GlobalPoint::new(
							monitor.origin.x.saturating_add(floating_position.x.round() as i32),
							monitor.origin.y.saturating_add(floating_position.y.round() as i32),
						)
					})
				}
			});
		let Some(toolbar_outer_pos) = toolbar_outer_pos else {
			return false;
		};
		let cursor_local =
			Self::toolbar_cursor_local_position_from_outer(toolbar_outer_pos, cursor_global);
		#[cfg(target_os = "macos")]
		let toolbar_primary_origin = toolbar_layout_model::frozen_toolbar_window_primary_origin();
		#[cfg(not(target_os = "macos"))]
		let toolbar_primary_origin = Pos2::ZERO;

		WindowRenderer::frozen_toolbar_visible_capsules_contain(
			&self.toolbar_state,
			toolbar_primary_origin,
			cursor_local,
		)
	}

	fn log_toolbar_redraw_phase_timing(
		&self,
		monitor: MonitorRect,
		redraw_started_at: Instant,
		draw_frame_elapsed: Duration,
		position_update_elapsed: Option<Duration>,
		should_hide_toolbar_window: bool,
	) {
		if !tracing::enabled!(tracing::Level::TRACE)
			|| !matches!(self.state.mode, OverlayMode::Frozen)
		{
			return;
		}

		tracing::trace!(
			op = "overlay.toolbar_redraw_phase_timing",
			monitor_id = monitor.id,
			total_us = redraw_started_at.elapsed().as_micros(),
			draw_frame_us = draw_frame_elapsed.as_micros(),
			position_update_us = position_update_elapsed.map_or(0, |elapsed| elapsed.as_micros()),
			hidden = should_hide_toolbar_window,
			frozen_selection_drag_active = self.frozen_selection_drag.active,
			"Toolbar redraw phase timing."
		);
	}
}
