use egui::{Pos2, Rect, Vec2};

use crate::overlay::rendering::WindowRenderer;
use crate::overlay::runtime_model::{FrozenToolbarTool, PngAction};
use crate::overlay::session_contracts::OverlayControl;
#[cfg(not(target_os = "macos"))]
use crate::overlay::session_state::FrozenCaptureWorkerState;
use crate::overlay::session_state::FrozenToolbarPointerState;
use crate::overlay::{OverlaySession, coordinate_geometry, toolbar_layout_model};
use crate::state::{MonitorRect, OverlayMode, RectPoints};
use crate::worker::FreezeCaptureTarget;

impl OverlaySession {
	pub(in crate::overlay) fn seed_frozen_toolbar_default_position(
		&mut self,
		monitor: MonitorRect,
		capture_rect: RectPoints,
	) {
		let default_pos =
			self.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);

		self.toolbar_state.default_slot_position = Some(default_pos);
		self.toolbar_state.floating_position = Some(default_pos);

		let _ = self.update_toolbar_outer_position(monitor, default_pos);

		tracing::debug!(
			monitor_id = monitor.id,
			frozen_generation = self.state.frozen_generation,
			toolbar_primary_size_points =
				?WindowRenderer::frozen_toolbar_primary_size(&self.toolbar_state),
			toolbar_size_points =
				?WindowRenderer::frozen_toolbar_size(&self.toolbar_state),
			default_pos = ?default_pos,
			"Frozen toolbar default position preseeded."
		);
	}

	pub(in crate::overlay) fn frozen_toolbar_default_position_for_capture_rect(
		&self,
		monitor: MonitorRect,
		capture_rect_points: RectPoints,
	) -> Pos2 {
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let capture_rect = Rect::from_min_size(
			Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
			Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
		);
		let toolbar_primary_size = WindowRenderer::frozen_toolbar_primary_size(&self.toolbar_state);
		let toolbar_positioning_size = self.toolbar_positioning_size();

		WindowRenderer::frozen_toolbar_default_window_pos(
			screen_rect,
			capture_rect,
			toolbar_primary_size,
			toolbar_positioning_size,
			self.config.toolbar_placement,
		)
	}

	pub(in crate::overlay) fn sync_frozen_annotation_style_capsule_placement(
		&mut self,
		monitor: MonitorRect,
	) {
		let Some(toolbar_pos) =
			self.toolbar_state.floating_position.or(self.toolbar_state.default_slot_position)
		else {
			return;
		};
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));

		WindowRenderer::sync_frozen_annotation_style_capsule_placement(
			&mut self.toolbar_state,
			screen_rect,
			toolbar_pos,
		);
	}

	pub(in crate::overlay) fn refresh_frozen_helper_windows_for_transition(
		&mut self,
		monitor: MonitorRect,
	) {
		self.force_apply_pending_toolbar_window_move();
		self.schedule_egui_repaint_after(self.repaint_interval_for_monitor(Some(monitor)));
		self.request_redraw_for_monitor(monitor);
		self.request_redraw_toolbar_window();
	}

	pub(in crate::overlay) fn prepare_toolbar_for_frozen_capture_transition(
		&mut self,
		monitor: MonitorRect,
		capture_rect: RectPoints,
	) {
		self.toolbar_window_drawn_once = false;
		self.toolbar_badge_slot_ready = false;
		self.toolbar_state.floating_position = None;
		self.toolbar_state.default_slot_position = None;
		self.toolbar_state.dragging = false;
		self.toolbar_state.needs_redraw = true;
		self.toolbar_state.pill_height_points = None;
		self.toolbar_state.layout_last_screen_size_points = None;
		self.toolbar_state.layout_stable_frames = 0;

		self.reset_frozen_text_state();
		self.sync_frozen_toolbar_state();
		// Spawn the toolbar immediately at the default position (capture aware). This avoids any
		// dependency on egui viewport stabilization or additional input events (mouse move) to
		// finish the initial layout.
		self.seed_frozen_toolbar_default_position(monitor, capture_rect);
		self.request_redraw_toolbar_window();
	}

	pub(in crate::overlay) fn toolbar_pointer_state(
		&mut self,
		monitor: MonitorRect,
		toolbar_cursor_local_override: Option<Pos2>,
	) -> Option<FrozenToolbarPointerState> {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return None;
		}
		if !self.toolbar_state.visible {
			return None;
		}
		if self.state.monitor != Some(monitor) {
			return None;
		}
		if toolbar_cursor_local_override.is_none() && self.active_cursor_monitor() != Some(monitor)
		{
			return None;
		}

		let left_button_went_down = self.toolbar_left_button_went_down;
		let left_button_went_up = self.toolbar_left_button_went_up;
		#[cfg(not(target_os = "macos"))]
		let left_button_down = self.toolbar_left_button_down;

		self.toolbar_left_button_went_down = false;
		self.toolbar_left_button_went_up = false;

		let cursor_local = toolbar_cursor_local_override.or_else(|| {
			self.state
				.cursor
				.and_then(|cursor| coordinate_geometry::global_to_local(cursor, monitor))
		})?;

		Some(FrozenToolbarPointerState {
			cursor_local,
			#[cfg(not(target_os = "macos"))]
			left_button_down,
			left_button_went_down,
			left_button_went_up,
		})
	}

	pub(in crate::overlay) fn sync_frozen_toolbar_state(&mut self) {
		self.toolbar_state.auto_center_available = self.frozen_auto_center_available();
		self.toolbar_state.undo_available = self.frozen_undo_available();
		self.toolbar_state.redo_available = self.frozen_redo_available();
		self.toolbar_state.scroll_capture_active = self.scroll_capture.active;
		// Keep drag-region toolbar geometry stable across the authoritative frozen-capture handoff:
		// show the Scroll slot immediately, but keep it disabled until final_capture_ready flips.
		self.toolbar_state.scroll_capture_available = self.toolbar_scroll_capture_slot_available();
		self.toolbar_state.final_capture_ready = self.frozen_final_capture_ready();
	}

	pub(in crate::overlay) fn maybe_recenter_frozen_toolbar_default_slot(
		&mut self,
		monitor: MonitorRect,
	) -> bool {
		if !matches!(self.state.mode, OverlayMode::Frozen) || self.state.monitor != Some(monitor) {
			return false;
		}
		if self.scroll_capture.active || self.toolbar_state.dragging {
			return false;
		}

		let Some(capture_rect) = self.state.frozen_capture_rect else {
			return false;
		};
		let Some(toolbar_pos) = self.toolbar_state.floating_position else {
			return false;
		};
		let Some(previous_default_pos) = self.toolbar_state.default_slot_position else {
			return false;
		};
		let current_default_pos =
			self.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);

		self.toolbar_state.default_slot_position = Some(current_default_pos);

		if toolbar_layout_model::frozen_toolbar_matches_default_slot(
			toolbar_pos,
			previous_default_pos,
		) {
			self.toolbar_state.floating_position = Some(current_default_pos);

			self.sync_frozen_annotation_style_capsule_placement(monitor);

			return !toolbar_layout_model::frozen_toolbar_matches_default_slot(
				toolbar_pos,
				current_default_pos,
			);
		}

		self.sync_frozen_annotation_style_capsule_placement(monitor);

		false
	}

	pub(in crate::overlay) fn handle_capture_and_toolbar_redraw_post(
		&mut self,
		overlay_monitor: MonitorRect,
		draw_toolbar: bool,
	) -> OverlayControl {
		if self.should_dispatch_pending_freeze_capture(overlay_monitor) {
			let pending_window_target =
				self.pending_window_freeze_capture_for_monitor(overlay_monitor);
			let freeze_target = pending_window_target
				.map_or(FreezeCaptureTarget::Monitor, |target| FreezeCaptureTarget::Window {
					window_id: target.window_id,
				});
			#[cfg(target_os = "macos")]
			let _ = (&freeze_target, &pending_window_target, &overlay_monitor);

			#[cfg(not(target_os = "macos"))]
			{
				// Capture must happen on a post-hide redraw so the HUD/loupe are not included.
				if self.frozen_capture_worker_armed() {
					let Some(worker) = &self.worker else {
						self.abort_pending_freeze_capture("Capture worker is unavailable.");

						return OverlayControl::Continue;
					};

					match worker.request_freeze_capture(overlay_monitor, freeze_target) {
						Ok(()) => {
							self.note_freeze_capture_request_started(
								overlay_monitor,
								pending_window_target,
							);
						},
						Err(err) => {
							self.handle_freeze_capture_request_send_error(overlay_monitor, err);
						},
					}
				} else {
					self.freeze_capture_send_full_count = 0;

					self.set_frozen_capture_worker_state(FrozenCaptureWorkerState::Armed);
					#[cfg(not(target_os = "macos"))]
					self.hide_capture_windows();
					self.request_redraw_for_monitor(overlay_monitor);
				}
			}
		}
		if draw_toolbar && self.sync_frozen_text_edit_for_selected_tool() {
			self.request_redraw_for_monitor(overlay_monitor);
		}
		if draw_toolbar && let Some(action) = self.toolbar_state.pending_action.take() {
			let control = self.handle_toolbar_action(action);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}
		if draw_toolbar && self.toolbar_state.needs_redraw {
			self.toolbar_state.needs_redraw = false;

			self.refresh_frozen_text_ime_cursor_area_for_text_style_change(overlay_monitor);
			self.request_redraw_for_monitor(overlay_monitor);
		}

		OverlayControl::Continue
	}

	pub(in crate::overlay) fn handle_toolbar_action(
		&mut self,
		action: FrozenToolbarTool,
	) -> OverlayControl {
		if self.frozen_text_edit.is_some() {
			let _ = self.finish_frozen_text_editing(true);
		}

		match action {
			FrozenToolbarTool::Undo => {
				let _ = self.perform_frozen_undo();

				OverlayControl::Continue
			},
			FrozenToolbarTool::Redo => {
				let _ = self.perform_frozen_redo();

				OverlayControl::Continue
			},
			FrozenToolbarTool::AutoCenter => {
				self.auto_center_frozen_capture_rect();

				OverlayControl::Continue
			},
			FrozenToolbarTool::Copy => {
				self.begin_png_action(PngAction::Copy);

				OverlayControl::Continue
			},
			FrozenToolbarTool::Save => {
				self.begin_png_action(PngAction::Save);

				OverlayControl::Continue
			},
			FrozenToolbarTool::Scroll => self.start_scroll_capture(),
			#[cfg(target_os = "macos")]
			FrozenToolbarTool::Ocr => self.begin_ocr_action(),
			_ => OverlayControl::Continue,
		}
	}
}
