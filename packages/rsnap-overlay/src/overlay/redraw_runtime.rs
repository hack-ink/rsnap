use std::time::Instant;

use egui::{Pos2, Rect, Vec2};
use winit::window::WindowId;

use crate::overlay::rendering::WindowRenderer;
use crate::overlay::runtime_model::{FrozenCaptureSource, OverlayEventLoopPhase};
use crate::overlay::session_contracts::{OverlayControl, OverlayExit};
use crate::overlay::session_state::{
	FrozenArrowAnnotation, FrozenSpotlightAnnotation, FrozenTextAnnotation,
};
use crate::overlay::{OverlaySession, toolbar_layout_model};
use crate::state::{MonitorRect, OverlayMode};

impl OverlaySession {
	pub(in crate::overlay) fn handle_redraw_requested(
		&mut self,
		window_id: WindowId,
	) -> OverlayControl {
		let now = Instant::now();

		self.event_loop_last_progress_window_id = Some(window_id);
		self.event_loop_last_progress_monitor_id =
			self.windows.get(&window_id).map(|window| window.monitor.id);

		self.maybe_log_event_loop_stall(now);
		self.mark_progress(OverlayEventLoopPhase::RedrawDispatch);

		let control = self.drain_worker_responses();

		if !matches!(control, OverlayControl::Continue) {
			return control;
		}
		if self.hud_window.as_ref().is_some_and(|hud_window| hud_window.window.id() == window_id) {
			return self.handle_hud_redraw_requested();
		}
		if self
			.loupe_window
			.as_ref()
			.is_some_and(|loupe_window| loupe_window.window.id() == window_id)
		{
			return self.handle_loupe_redraw_requested();
		}
		if self
			.scroll_preview_window
			.as_ref()
			.is_some_and(|preview_window| preview_window.window.id() == window_id)
		{
			return self.handle_scroll_preview_redraw_requested();
		}

		self.handle_overlay_window_redraw(window_id)
	}

	pub(in crate::overlay) fn frozen_toolbar_badge_visibility(
		&mut self,
		overlay_monitor: MonitorRect,
		overlay_screen_rect: Rect,
		draw_toolbar: bool,
	) -> bool {
		let toolbar_visible_for_badge = if cfg!(target_os = "macos") {
			!self.should_hide_toolbar_window(overlay_monitor)
		} else {
			draw_toolbar
		};

		#[cfg(target_os = "macos")]
		{
			if !toolbar_visible_for_badge {
				return false;
			}

			let ready = self.advance_frozen_toolbar_readiness_sample(overlay_screen_rect);

			if !ready {
				self.request_redraw_for_monitor(overlay_monitor);
			}

			ready && self.toolbar_window_drawn_once && self.toolbar_badge_slot_ready
		}

		#[cfg(not(target_os = "macos"))]
		{
			toolbar_visible_for_badge && self.frozen_toolbar_ready_for_draw(overlay_screen_rect)
		}
	}

	fn pending_frozen_display_handoff_state(
		&self,
		overlay_monitor: MonitorRect,
	) -> (bool, Option<MonitorRect>) {
		let pending_frozen_display_handoff = self.frozen_display_handoff_pending()
			|| self.frozen_visual_handoff_pending_for_monitor(overlay_monitor);
		let pending_frozen_display_handoff_monitor =
			self.frozen_capture_monitor().filter(|_| pending_frozen_display_handoff);

		(pending_frozen_display_handoff, pending_frozen_display_handoff_monitor)
	}

	pub(in crate::overlay) fn allow_frozen_surface_bg_for_overlay_monitor(
		&self,
		overlay_monitor: MonitorRect,
		scroll_capture_active: bool,
	) -> bool {
		!scroll_capture_active
			&& !self.frozen_display_handoff_pending()
			&& !self.frozen_visual_handoff_pending_for_monitor(overlay_monitor)
	}

	pub(in crate::overlay) fn should_draw_live_surface_bg_for_overlay_monitor(
		&self,
		overlay_monitor: MonitorRect,
	) -> bool {
		matches!(self.state.mode, OverlayMode::Live)
			&& self.state.live_bg_monitor == Some(overlay_monitor)
			&& self.state.live_bg_image.is_some()
	}

	pub(in crate::overlay) fn selection_flow_enabled_for_overlay_draw(&self) -> bool {
		self.config.selection_flow_enabled
	}

	#[cfg(target_os = "macos")]
	pub(in crate::overlay) fn should_refresh_live_surface_bg_for_overlay_monitor(
		&self,
		overlay_monitor: MonitorRect,
	) -> bool {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return false;
		}
		if self.active_cursor_monitor() != Some(overlay_monitor) {
			return false;
		}
		if self.frozen_display_handoff_pending()
			|| self.frozen_visual_handoff_pending_for_monitor(overlay_monitor)
		{
			return true;
		}

		self.state.live_bg_monitor != Some(overlay_monitor) || self.state.live_bg_image.is_none()
	}

	fn mark_overlay_window_redraw_progress(
		&mut self,
		window_id: WindowId,
		overlay_monitor: MonitorRect,
	) {
		self.sync_overlay_cursor_icons();
		self.sync_frozen_toolbar_state();

		self.event_loop_last_progress_window_id = Some(window_id);
		self.event_loop_last_progress_monitor_id = Some(overlay_monitor.id);
	}

	#[cfg(target_os = "macos")]
	fn maybe_sync_live_surface_bg_for_overlay_redraw(&mut self, overlay_monitor: MonitorRect) {
		if self.should_refresh_live_surface_bg_for_overlay_monitor(overlay_monitor) {
			self.sync_live_surface_bg_from_stream(overlay_monitor);
		}
	}

	#[cfg(not(target_os = "macos"))]
	fn maybe_sync_live_surface_bg_for_overlay_redraw(&mut self, _overlay_monitor: MonitorRect) {}

	fn finish_overlay_window_redraw(
		&mut self,
		overlay_monitor: MonitorRect,
		draw_toolbar: bool,
	) -> OverlayControl {
		self.maybe_arm_frozen_toolbar_badge_slot_after_overlay_draw(overlay_monitor);

		self.last_present_at = Instant::now();

		self.note_startup_overlay_frame_presented();

		self.handle_capture_and_toolbar_redraw_post(overlay_monitor, draw_toolbar)
	}

	fn handle_overlay_window_redraw(&mut self, window_id: WindowId) -> OverlayControl {
		let Some(overlay_monitor) = self.windows.get(&window_id).map(|overlay| overlay.monitor)
		else {
			return OverlayControl::Continue;
		};

		self.mark_overlay_window_redraw_progress(window_id, overlay_monitor);
		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::OverlayRedraw);
		self.maybe_sync_live_surface_bg_for_overlay_redraw(overlay_monitor);

		let overlay_screen_rect = self.overlay_window_screen_rect(window_id, overlay_monitor);
		#[cfg(target_os = "macos")]
		let draw_toolbar = false;
		#[cfg(not(target_os = "macos"))]
		let draw_toolbar = matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.state.monitor == Some(overlay_monitor)
			&& self.frozen_preview_visible();
		#[cfg(not(target_os = "macos"))]
		let toolbar_input =
			if draw_toolbar { self.toolbar_pointer_state(overlay_monitor, None) } else { None };
		#[cfg(target_os = "macos")]
		let toolbar_input = None;

		self.log_frozen_overlay_redraw_trace(window_id, overlay_monitor, draw_toolbar);

		let toolbar_ready_for_badge = self.frozen_toolbar_badge_visibility(
			overlay_monitor,
			overlay_screen_rect,
			draw_toolbar,
		);
		let frozen_toolbar_reserved_rect = self.frozen_size_badge_toolbar_reserved_rect(
			overlay_monitor,
			overlay_screen_rect,
			toolbar_ready_for_badge,
		);
		let frozen_selection_resize_handles_enabled = self.frozen_selection_drag_target().is_some();
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};
		let (scroll_capture_active, frozen_text_style) =
			(self.scroll_capture.active, self.toolbar_state.text_style);
		let visible_frozen_text_annotations: &[FrozenTextAnnotation] =
			if scroll_capture_active { &[] } else { &self.frozen_text_annotations };
		let visible_frozen_arrow_annotations: &[FrozenArrowAnnotation] =
			if scroll_capture_active { &[] } else { &self.frozen_arrow_annotations };
		let visible_frozen_spotlight_annotations: &[FrozenSpotlightAnnotation] =
			if scroll_capture_active { &[] } else { &self.frozen_spotlight_annotations };
		let visible_frozen_text_edit =
			if scroll_capture_active { None } else { self.frozen_text_edit.as_ref() };
		let visible_frozen_arrow_preview =
			if scroll_capture_active { None } else { self.active_frozen_arrow_preview() };
		let visible_frozen_spotlight_preview_rect =
			if scroll_capture_active { None } else { self.frozen_spotlight_preview_rect };
		let (pending_frozen_display_handoff, pending_frozen_display_handoff_monitor) =
			self.pending_frozen_display_handoff_state(overlay_monitor);
		let allow_frozen_surface_bg = self
			.allow_frozen_surface_bg_for_overlay_monitor(overlay_monitor, scroll_capture_active);
		let allow_live_surface_bg =
			self.should_draw_live_surface_bg_for_overlay_monitor(overlay_monitor);
		let selection_flow_enabled = self.selection_flow_enabled_for_overlay_draw();
		let toolbar_state = if draw_toolbar { Some(&mut self.toolbar_state) } else { None };

		{
			let Some(overlay_window) = self.windows.get_mut(&window_id) else {
				return OverlayControl::Continue;
			};

			if let Err(err) = overlay_window.renderer.draw(
				gpu,
				&self.state,
				overlay_monitor,
				false,
				None,
				false,
				self.config.hud_anchor,
				self.config.toolbar_placement,
				self.config.show_alt_hint_keycap,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
				selection_flow_enabled,
				self.config.selection_flow_stroke_width_px,
				allow_frozen_surface_bg,
				allow_live_surface_bg,
				pending_frozen_display_handoff,
				pending_frozen_display_handoff_monitor,
				scroll_capture_active,
				frozen_selection_resize_handles_enabled,
				self.frozen_capture_source,
				self.frozen_capture_source == FrozenCaptureSource::FullscreenFallback,
				frozen_toolbar_reserved_rect,
				&self.frozen_edit_undo_stack,
				(!scroll_capture_active).then_some(&self.frozen_brush),
				visible_frozen_arrow_annotations,
				visible_frozen_arrow_preview.as_ref(),
				visible_frozen_spotlight_annotations,
				visible_frozen_spotlight_preview_rect,
				visible_frozen_text_annotations,
				visible_frozen_text_edit,
				frozen_text_style,
				toolbar_state,
				toolbar_input,
			) {
				return self.exit(OverlayExit::Error(format!("{err:#}")));
			}
		}

		self.finish_overlay_window_redraw(overlay_monitor, draw_toolbar)
	}

	#[cfg(target_os = "macos")]
	fn maybe_arm_frozen_toolbar_badge_slot_after_overlay_draw(
		&mut self,
		overlay_monitor: MonitorRect,
	) {
		if self.toolbar_badge_slot_ready
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.state.monitor != Some(overlay_monitor)
			|| !self.toolbar_window_visible
			|| !self.toolbar_window_drawn_once
		{
			return;
		}

		self.toolbar_badge_slot_ready = true;

		self.note_frozen_transition_badge_slot_armed(overlay_monitor);
		self.request_redraw_for_monitor(overlay_monitor);
	}

	#[cfg(not(target_os = "macos"))]
	fn maybe_arm_frozen_toolbar_badge_slot_after_overlay_draw(
		&mut self,
		_overlay_monitor: MonitorRect,
	) {
	}

	fn log_frozen_overlay_redraw_trace(
		&self,
		window_id: WindowId,
		overlay_monitor: MonitorRect,
		draw_toolbar: bool,
	) {
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.state.monitor != Some(overlay_monitor)
		{
			return;
		}

		tracing::trace!(
			window_id = ?window_id,
			monitor_id = overlay_monitor.id,
			frozen_generation = self.state.frozen_generation,
			final_capture_ready = self.frozen_final_capture_ready(),
			frozen_image_ready = self.frozen_display_ready(),
			frozen_capture_session_state = ?self.frozen_capture_session_state,
			pending_freeze_capture = self
				.frozen_capture_monitor()
				.filter(|_| self.frozen_capture_export_pending())
				.map(|m| m.id),
			draw_toolbar,
			toolbar_visible = self.toolbar_state.visible,
			toolbar_floating_position = ?self.toolbar_state.floating_position,
			toolbar_stable_frames = self.toolbar_state.layout_stable_frames,
			toolbar_last_screen_size_points = ?self.toolbar_state.layout_last_screen_size_points,
			"Overlay redraw (Frozen)."
		);
	}

	fn overlay_window_screen_rect(&self, window_id: WindowId, monitor: MonitorRect) -> Rect {
		let fallback_size = Vec2::new(monitor.width as f32, monitor.height as f32);

		self.windows
			.get(&window_id)
			.map(|overlay_window| {
				let scale_factor = overlay_window.window.scale_factor().max(1.0) as f32;
				let size = overlay_window.window.inner_size();
				let size_points = if size.width == 0 || size.height == 0 {
					fallback_size
				} else {
					Vec2::new(
						(size.width as f32 / scale_factor).max(1.0),
						(size.height as f32 / scale_factor).max(1.0),
					)
				};

				Rect::from_min_size(Pos2::ZERO, size_points)
			})
			.unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, fallback_size))
	}

	#[cfg(any(target_os = "macos", test))]
	pub(in crate::overlay) fn advance_frozen_toolbar_readiness_sample(
		&mut self,
		screen_rect: Rect,
	) -> bool {
		toolbar_layout_model::advance_frozen_toolbar_readiness_sample_state(
			&mut self.toolbar_state,
			screen_rect,
		)
	}

	#[cfg(any(not(target_os = "macos"), test))]
	pub(in crate::overlay) fn frozen_toolbar_ready_for_draw(&self, screen_rect: Rect) -> bool {
		let screen_size_points = screen_rect.size();
		let needs_new_sample = toolbar_layout_model::frozen_toolbar_needs_new_sample(
			self.toolbar_state.layout_last_screen_size_points,
			screen_size_points,
		);

		!needs_new_sample && self.toolbar_state.layout_stable_frames >= 1
	}

	pub(in crate::overlay) fn frozen_size_badge_toolbar_reserved_rect(
		&self,
		monitor: MonitorRect,
		screen_rect: Rect,
		toolbar_ready: bool,
	) -> Option<Rect> {
		if !toolbar_ready
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.state.monitor != Some(monitor)
		{
			return None;
		}

		WindowRenderer::frozen_toolbar_reserved_rect(
			&self.state,
			monitor,
			screen_rect,
			self.config.toolbar_placement,
			&self.toolbar_state,
		)
	}
}
