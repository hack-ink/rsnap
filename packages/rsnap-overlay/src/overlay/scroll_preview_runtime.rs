use crate::overlay::scroll_preview_geometry::{
	SCROLL_CAPTURE_PREVIEW_WIDTH_PX, SCROLL_PREVIEW_WINDOW_HEIGHT_POINTS,
	SCROLL_PREVIEW_WINDOW_MARGIN_POINTS, SCROLL_PREVIEW_WINDOW_WIDTH_POINTS,
};
use crate::overlay::{
	ElementState, MonitorRect, MouseButton, OverlayControl, OverlayExit, OverlaySession, Pos2,
	Rect, RgbaImage, ScrollPreviewView, Vec2, WindowEvent, WindowId, hud_helpers, scroll_capture,
};
#[cfg(target_os = "macos")]
use crate::overlay::{LogicalPosition, LogicalSize};

impl OverlaySession {
	pub(super) fn sync_scroll_preview_segments(&mut self) {
		let image = self.current_scroll_preview_render_image();

		{
			let Some(preview) = self.scroll_preview_window.as_mut() else {
				return;
			};

			preview.sync_image(image);
			preview.window.request_redraw();
		}

		if let Some(monitor) = self.scroll_capture.monitor.or(self.state.monitor) {
			#[cfg(target_os = "macos")]
			{
				self.position_scroll_preview_window(monitor);
			}

			#[cfg(not(target_os = "macos"))]
			{
				let _ = monitor;
			}
		}
	}

	pub(super) fn refresh_scroll_preview_committed_image(&mut self) {
		self.scroll_capture.preview_committed_image =
			self.scroll_capture.session.as_ref().map(|session| session.export_image().clone());
	}

	pub(super) fn refresh_scroll_preview_display_image(&mut self) {
		let motion_rows_hint = None;

		self.scroll_capture.last_overlay_preview_motion_rows_hint = motion_rows_hint;
		self.scroll_capture.last_overlay_preview_provisional_motion_rows_hint = None;
		self.scroll_capture.last_overlay_preview_existing_candidate_height = None;
		self.scroll_capture.last_overlay_preview_existing_candidate_motion_rows_hint = None;
		self.scroll_capture.last_overlay_preview_ledger_candidate_height = None;
		self.scroll_capture.last_overlay_preview_ledger_candidate_motion_rows_hint = None;
		self.scroll_capture.last_overlay_preview_retained_candidate_height = None;
		self.scroll_capture.last_overlay_preview_retained_candidate_motion_rows_hint = None;
		self.scroll_capture.last_overlay_preview_retained_hint_matches_motion_rows = false;
		self.scroll_capture.last_overlay_preview_fresh_latest_frame_can_drive = false;
		self.scroll_capture.last_overlay_preview_strong_unresolved_registration = false;
		self.scroll_capture.last_overlay_preview_latest_frame_present =
			self.scroll_capture.preview_latest_frame.is_some();
		self.scroll_capture.last_overlay_preview_used_provisional = false;

		if let Some(session) = self.scroll_capture.session.as_mut() {
			self.scroll_capture.preview_committed_image = Some(session.export_image().clone());
			self.scroll_capture.preview_display_image =
				self.scroll_capture.preview_committed_image.clone();

			return;
		}

		self.scroll_capture.preview_display_image =
			self.scroll_capture.preview_committed_image.as_ref().map(|base_preview| {
				scroll_capture::compose_provisional_preview_image(
					base_preview,
					self.scroll_capture.preview_latest_frame.as_ref(),
					motion_rows_hint,
					SCROLL_CAPTURE_PREVIEW_WIDTH_PX,
				)
			});
	}

	pub(super) fn scroll_capture_preview_dimensions(&self) -> Option<[u32; 2]> {
		self.current_scroll_preview_render_image()
			.as_ref()
			.map(|image| [image.width(), image.height()])
	}

	pub(super) fn scroll_preview_display_size_points(&self) -> Option<Vec2> {
		let [width_px, height_px] = self.scroll_capture_preview_dimensions()?;

		if width_px == 0 || height_px == 0 {
			return None;
		}

		let width_points = SCROLL_PREVIEW_WINDOW_WIDTH_POINTS as f32;
		let scale = width_points / width_px as f32;

		Some(Vec2::new(width_points, (height_px as f32 * scale).max(1.0)))
	}

	pub(super) fn current_scroll_preview_render_image(&self) -> Option<RgbaImage> {
		if self.scroll_capture.active {
			return self.current_export_image();
		}

		self.scroll_capture.preview_display_image.clone().or_else(|| self.current_export_image())
	}

	pub(super) fn handle_scroll_preview_event(
		&mut self,
		window_id: WindowId,
		event: &WindowEvent,
	) -> Option<OverlayControl> {
		if self
			.scroll_preview_window
			.as_ref()
			.is_none_or(|preview_window| preview_window.window.id() != window_id)
		{
			return None;
		}

		Some(match event {
			WindowEvent::RedrawRequested => self.handle_scroll_preview_redraw_requested(),
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button: MouseButton::Right,
				..
			} => self.cancel_overlay("scroll_preview_right_click"),
			WindowEvent::KeyboardInput { event, .. } => self.handle_key_event(event),
			WindowEvent::ModifiersChanged(modifiers) => self.handle_modifiers_changed(modifiers),
			_ => self.handle_scroll_preview_window_event(event),
		})
	}

	pub(super) fn handle_scroll_preview_window_event(
		&mut self,
		event: &WindowEvent,
	) -> OverlayControl {
		let Some(preview_window) = self.scroll_preview_window.as_mut() else {
			return OverlayControl::Continue;
		};

		preview_window.handle_window_event(event);

		OverlayControl::Continue
	}

	pub(super) fn handle_scroll_preview_redraw_requested(&mut self) -> OverlayControl {
		let should_hide_preview = self.should_hide_scroll_preview_window();
		let Some(preview_window) = self.scroll_preview_window.as_mut() else {
			return OverlayControl::Continue;
		};

		if should_hide_preview {
			preview_window.window.set_visible(false);

			return OverlayControl::Continue;
		}

		preview_window.window.set_visible(true);

		let theme =
			hud_helpers::effective_hud_theme(self.config.theme_mode, preview_window.window.theme());
		let view = ScrollPreviewView { paused: self.scroll_capture.paused, theme };
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};

		match preview_window.draw(gpu, theme, view) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	pub(super) fn should_hide_scroll_preview_window(&self) -> bool {
		self.frozen_selection_drag_hides_auxiliary_windows() || !self.scroll_capture.active
	}

	#[cfg(target_os = "macos")]
	pub(super) fn position_scroll_preview_window(&self, monitor: MonitorRect) {
		let Some(preview_window) = self.scroll_preview_window.as_ref() else {
			return;
		};
		let preview_rect = self.scroll_preview_local_rect(monitor);
		let current_size = preview_window.window.inner_size();
		let desired_width = preview_rect.width().round().max(1.0) as u32;
		let desired_height = preview_rect.height().round().max(1.0) as u32;

		if current_size.width != desired_width || current_size.height != desired_height {
			let _ = preview_window.window.request_inner_size(LogicalSize::new(
				f64::from(desired_width),
				f64::from(desired_height),
			));
		}

		preview_window.window.set_outer_position(LogicalPosition::new(
			f64::from(monitor.origin.x) + f64::from(preview_rect.min.x),
			f64::from(monitor.origin.y) + f64::from(preview_rect.min.y),
		));
	}

	pub(super) fn scroll_preview_local_rect(&self, monitor: MonitorRect) -> Rect {
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let gap = SCROLL_PREVIEW_WINDOW_MARGIN_POINTS as f32;
		let preview_width = SCROLL_PREVIEW_WINDOW_WIDTH_POINTS as f32;

		if let Some(capture_rect) = self.state.frozen_capture_rect {
			let capture_rect = Rect::from_min_size(
				Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
				Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
			)
			.intersect(screen_rect);
			let preview_size = self
				.scroll_preview_display_size_points()
				.unwrap_or(Vec2::new(preview_width, capture_rect.height().max(1.0)));
			let preview_width = preview_size.x.max(preview_width);
			let max_preview_height = (screen_rect.max.y - capture_rect.min.y - gap).max(1.0);
			let preview_height = preview_size.y.min(max_preview_height).max(1.0);
			let right_x = capture_rect.max.x + gap;
			let left_x = capture_rect.min.x - gap - preview_width;
			let x = if right_x + preview_width <= screen_rect.max.x {
				right_x
			} else if left_x >= screen_rect.min.x {
				left_x
			} else {
				(screen_rect.max.x - preview_width - gap).max(screen_rect.min.x + gap)
			};

			return Rect::from_min_size(
				Pos2::new(x, capture_rect.min.y),
				Vec2::new(preview_width, preview_height),
			);
		}

		let preview_size = if let Some(preview_window) = self.scroll_preview_window.as_ref() {
			let scale = preview_window.window.scale_factor().max(1.0) as f32;
			let size = preview_window.window.inner_size();

			Vec2::new(
				((size.width as f32) / scale).max(preview_width),
				((size.height as f32) / scale).max(SCROLL_PREVIEW_WINDOW_HEIGHT_POINTS as f32),
			)
		} else {
			Vec2::new(preview_width, SCROLL_PREVIEW_WINDOW_HEIGHT_POINTS as f32)
		};
		let min_x = screen_rect.min.x + gap;
		let max_x = (screen_rect.max.x - preview_size.x - gap).max(min_x);
		let min_y = screen_rect.min.y + gap;
		let max_y = (screen_rect.max.y - preview_size.y - gap).max(min_y);
		let y = min_y.min(max_y);
		let pos = Pos2::new(max_x, y);

		Rect::from_min_size(pos, preview_size)
	}
}
