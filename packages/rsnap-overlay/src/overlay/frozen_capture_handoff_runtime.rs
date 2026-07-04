use image::RgbaImage;
#[cfg(target_os = "macos")]
use image::imageops;

#[cfg(target_os = "macos")]
use crate::overlay::toolbar_geometry::TOOLBAR_WINDOW_WARMUP_REDRAWS;
use crate::overlay::{
	FrozenCaptureSource, FrozenCaptureWorkerState, GlobalPoint, LiveCaptureInteraction,
	MonitorRect, OverlayMode, OverlaySession, RectPoints, WindowCaptureAlphaMode,
	WindowFreezeCaptureTarget,
};

impl OverlaySession {
	fn prepare_frozen_capture_handoff_state(
		&mut self,
		monitor: MonitorRect,
		window_target: Option<WindowFreezeCaptureTarget>,
	) {
		self.set_frozen_capture_display_pending(
			monitor,
			FrozenCaptureWorkerState::Idle,
			window_target,
		);

		self.freeze_capture_send_full_count = 0;
		self.frozen_window_image = None;
		self.capture_windows_hidden = false;
		self.pending_click_hit_test_request_id = None;
		self.pending_click_hit_test_requested_at = None;

		if !matches!(
			self.live_capture_interaction,
			LiveCaptureInteraction::FrozenFromClick { .. }
				| LiveCaptureInteraction::FrozenFromDrag { .. }
		) {
			self.set_live_capture_interaction(LiveCaptureInteraction::Idle);
		}
	}

	pub(in crate::overlay) fn begin_frozen_capture_with_rect(
		&mut self,
		monitor: MonitorRect,
		rect: Option<RectPoints>,
		window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
	) {
		self.frozen_capture_source = if rect.is_none() {
			FrozenCaptureSource::FullscreenFallback
		} else if window_target.is_some() {
			FrozenCaptureSource::Window
		} else {
			FrozenCaptureSource::DragRegion
		};

		let capture_rect = rect.unwrap_or(RectPoints::new(0, 0, monitor.width, monitor.height));

		self.state.alt_held = false;
		self.loupe_activation_key_down = false;
		self.state.rgb = None;
		self.state.loupe = None;

		self.set_alt_loupe_window_visible(None, false);
		self.state.clear_error();
		self.begin_frozen_transition_timing(monitor, capture_rect, window_target);

		self.state.frozen_capture_rect = Some(capture_rect);
		self.state.frozen_mosaic_preview_rect = None;

		self.reset_frozen_annotation_state();

		self.skip_toolbar_focus_on_next_show = true;
		#[cfg(target_os = "macos")]
		{
			// Keep Rsnap active for the entire overlay session so AppKit continues to honor native
			// crosshair / grab / resize cursors. The pre-capture frontmost app is restored on exit.
			self.preserve_frontmost_on_next_toolbar_show = false;
		}

		tracing::debug!(
			monitor_id = monitor.id,
			origin = ?monitor.origin,
			width_points = monitor.width,
			height_points = monitor.height,
			monitor_scale_factor = monitor.scale_factor(),
			cursor = ?cursor,
			capture_rect = ?capture_rect,
			"Freeze begin."
		);

		self.prepare_toolbar_for_frozen_capture_transition(monitor, capture_rect);
		self.prepare_frozen_capture_handoff_state(monitor, window_target);

		#[cfg(target_os = "macos")]
		if self.begin_frozen_capture_with_rect_macos(monitor, window_target, cursor) {
			return;
		}

		#[cfg(not(target_os = "macos"))]
		self.begin_frozen_capture_with_rect_non_macos(monitor, window_target, cursor);
		// Do not request the first frozen redraw until the session has either committed a preview or
		// started the asynchronous export-authority path. Otherwise the overlay can briefly present
		// an empty black frozen frame before the real preview arrives.
		self.refresh_frozen_helper_windows_for_transition(monitor);
	}

	#[cfg(not(target_os = "macos"))]
	fn begin_frozen_capture_with_rect_non_macos(
		&mut self,
		monitor: MonitorRect,
		window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
	) {
		if self.use_fake_hud_blur()
			&& window_target.is_none()
			&& self.state.live_bg_monitor == Some(monitor)
			&& let Some(image) = self.state.live_bg_image.take()
		{
			self.state.live_bg_monitor = None;

			self.commit_first_frozen_display_handoff(monitor);
			self.state.commit_frozen_final_image(monitor, image);
			self.note_frozen_transition_preview_committed(monitor, "cached_live_background", None);
			self.promote_frozen_capture_display_ready(monitor);
			self.set_frozen_capture_export_ready(monitor);
			self.note_frozen_transition_final_ready(monitor, "cached_live_background", None);

			if let Some(cursor) = cursor {
				self.update_cursor_state(monitor, cursor);
			}

			self.force_apply_pending_toolbar_window_move();

			return;
		}

		self.state.live_bg_monitor = None;
		self.state.live_bg_image = None;
		self.capture_windows_hidden = true;

		self.hide_capture_windows();
	}

	#[cfg(target_os = "macos")]
	pub(in crate::overlay) fn cropped_monitor_frozen_region_image(
		&self,
		monitor: MonitorRect,
		capture_rect_pixels: RectPoints,
	) -> Option<RgbaImage> {
		let export_image = self.state.frozen_export_image.as_ref()?;
		let x = capture_rect_pixels.x.min(export_image.width());
		let y = capture_rect_pixels.y.min(export_image.height());
		let max_width = export_image.width().saturating_sub(x);
		let max_height = export_image.height().saturating_sub(y);
		let width = capture_rect_pixels.width.min(max_width);
		let height = capture_rect_pixels.height.min(max_height);

		if width == 0 || height == 0 {
			tracing::debug!(
				monitor_id = monitor.id,
				capture_rect_pixels = ?capture_rect_pixels,
				export_image_size = ?(export_image.width(), export_image.height()),
				"Scroll capture base-frame crop resolved to an empty region."
			);

			None
		} else {
			Some(imageops::crop_imm(export_image, x, y, width, height).to_image())
		}
	}

	pub(in crate::overlay) fn handle_captured_freeze_response(
		&mut self,
		monitor: MonitorRect,
		image: RgbaImage,
		window_image: Option<RgbaImage>,
		captured_window_id: Option<u32>,
	) {
		if self.frozen_capture_monitor() == Some(monitor) && self.frozen_capture_export_pending() {
			let window_capture_target = self.frozen_capture_window_target();
			let had_display_image = self.frozen_display_ready();
			let frozen_preview_image = image;

			self.frozen_window_image = None;

			if self.reject_dirty_window_export_authority(
				monitor,
				window_capture_target,
				window_image.is_some(),
				captured_window_id,
			) {
				self.restore_capture_windows_visibility();

				return;
			}

			let frozen_preview_image = self.apply_window_capture_export_authority(
				monitor,
				had_display_image,
				frozen_preview_image,
				window_capture_target,
				window_image,
				captured_window_id,
			);

			if !had_display_image {
				self.commit_first_frozen_display_handoff(monitor);
				self.state.commit_frozen_display_image(monitor, frozen_preview_image.clone());
				self.promote_frozen_capture_display_ready(monitor);
				self.note_frozen_transition_preview_committed(
					monitor,
					"authoritative_capture",
					None,
				);
			}

			self.state.commit_frozen_export_image(frozen_preview_image.clone());
			self.set_frozen_capture_export_ready(monitor);
			self.note_frozen_transition_final_ready(
				monitor,
				"authoritative_capture",
				captured_window_id,
			);
			#[cfg(target_os = "macos")]
			self.destroy_live_only_aux_windows();
			self.restore_capture_windows_visibility();
			#[cfg(target_os = "macos")]
			self.request_aux_window_creation_if_needed();

			self.toolbar_state.needs_redraw = true;

			#[cfg(target_os = "macos")]
			if self.toolbar_state.visible {
				self.toolbar_window_warmup_redraws_remaining =
					self.toolbar_window_warmup_redraws_remaining.max(TOOLBAR_WINDOW_WARMUP_REDRAWS);
			}

			if let Some(cursor) = self.state.cursor {
				self.update_cursor_state(monitor, cursor);
			}

			self.request_redraw_toolbar_window();
			self.request_redraw_for_monitor(monitor);
			#[cfg(not(target_os = "macos"))]
			self.raise_hud_windows();

			return;
		}
		if self.frozen_capture_worker_inflight() && self.frozen_capture_monitor() == Some(monitor) {
			self.clear_frozen_capture_session_state();
		}
		if matches!(self.state.mode, OverlayMode::Live)
			&& self.use_fake_hud_blur()
			&& self.active_cursor_monitor() == Some(monitor)
		{
			self.state.live_bg_monitor = Some(monitor);
			self.state.live_bg_image = Some(image);
			self.state.live_bg_generation = self.state.live_bg_generation.wrapping_add(1);

			self.request_redraw_for_monitor(monitor);
		}
	}

	fn reject_dirty_window_export_authority(
		&mut self,
		monitor: MonitorRect,
		window_capture_target: Option<WindowFreezeCaptureTarget>,
		window_image_present: bool,
		captured_window_id: Option<u32>,
	) -> bool {
		let Some(target) = window_capture_target else {
			return false;
		};

		if target.monitor != monitor
			|| !matches!(
				self.config.window_capture_alpha_mode,
				WindowCaptureAlphaMode::MatteLight | WindowCaptureAlphaMode::MatteDark
			) || (captured_window_id == Some(target.window_id) && window_image_present)
		{
			return false;
		}

		self.set_frozen_capture_export_failed(monitor);
		self.note_frozen_transition_aborted(
			"Window export authority did not resolve to a clean target window.",
		);
		self.state.set_error("Window capture is unavailable. Please try again.");

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();
		self.request_redraw_for_monitor(monitor);

		true
	}

	fn apply_window_capture_export_authority(
		&mut self,
		monitor: MonitorRect,
		had_display_image: bool,
		base_image: RgbaImage,
		window_capture_target: Option<WindowFreezeCaptureTarget>,
		window_image: Option<RgbaImage>,
		captured_window_id: Option<u32>,
	) -> RgbaImage {
		let Some((target, window_capture_image, window_id)) = window_capture_target
			.zip(window_image)
			.zip(captured_window_id)
			.map(|((target, window_capture_image), window_id)| {
				(target, window_capture_image, window_id)
			})
		else {
			return base_image;
		};

		if target.monitor != monitor || target.window_id != window_id {
			return base_image;
		}

		match self.config.window_capture_alpha_mode {
			WindowCaptureAlphaMode::Background => base_image,
			WindowCaptureAlphaMode::MatteLight | WindowCaptureAlphaMode::MatteDark => {
				let base_image = if had_display_image {
					self.state.frozen_display_image.clone().unwrap_or(base_image)
				} else {
					base_image
				};
				let window_capture_image = Self::compose_window_preview_layer(
					&window_capture_image,
					self.config.window_capture_alpha_mode,
				);
				let preview_image = Self::composite_window_capture_preview(
					base_image,
					&window_capture_image,
					monitor,
					target.rect,
					WindowCaptureAlphaMode::Background,
				);

				self.frozen_window_image = Some(window_capture_image);

				preview_image
			},
		}
	}
}
