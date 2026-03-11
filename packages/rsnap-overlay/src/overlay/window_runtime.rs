use std::{sync::Arc, time::Instant};

use crate::backend;
use crate::overlay::{self, ActiveEventLoop, FrozenCaptureSource, FrozenToolbarState, GlobalPoint, GpuContext, HUD_PILL_CORNER_RADIUS_POINTS, HudOverlayWindow, LOUPE_TILE_CORNER_RADIUS_POINTS, LiveSampleApplyResult, LogicalPosition, LogicalSize, MacLiveFrameStream, MainThreadMarker, MonitorRect, NSScreen, OverlayEventLoopPhase, OverlayMode, OverlaySession, OverlayWindow, OverlayWorker, Result, ScrollCaptureState, ScrollPreviewWindow, SlowOperationLogger, TOOLBAR_EXPANDED_HEIGHT_PX, TOOLBAR_EXPANDED_WIDTH_PX, WindowLevel, WindowRenderer, hud_helpers};

impl OverlaySession {
	pub fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		if self.is_active() {
			return Ok(());
		}

		self.reset_for_start();

		self.worker = Some(OverlayWorker::new(
			backend::default_capture_backend(),
			self.response_waker.clone(),
		));
		#[cfg(target_os = "macos")]
		{
			self.live_sample_stream = Some(MacLiveFrameStream::new());
		}

		let monitors = self.available_overlay_monitors()?;

		if monitors.is_empty() {
			return Err(String::from("No monitors detected"));
		}

		self.gpu = Some(GpuContext::new().map_err(|err| format!("{err:#}"))?);

		self.create_overlay_windows(event_loop, &monitors)?;
		self.create_hud_window(event_loop)?;
		self.create_loupe_window(event_loop)?;
		self.create_toolbar_window(event_loop)?;
		self.create_scroll_preview_window(event_loop)?;
		self.initialize_cursor_state();
		#[cfg(target_os = "macos")]
		self.focus_live_capture_window();
		self.request_redraw_all();

		Ok(())
	}

	pub(super) fn reset_for_start(&mut self) {
		let now = Instant::now();

		#[cfg(target_os = "macos")]
		self.set_scroll_overlay_mouse_passthrough(false);

		self.hud_inner_size_points = None;
		self.hud_outer_pos = None;
		self.pending_hud_outer_pos = None;
		self.loupe_inner_size_points = None;
		self.loupe_outer_pos = None;
		self.pending_loupe_outer_pos = None;
		self.toolbar_inner_size_points = None;
		self.toolbar_outer_pos = None;
		self.scroll_preview_window = None;
		self.cursor_monitor = None;
		#[cfg(target_os = "macos")]
		{
			self.live_sample_worker = None;
			self.live_sample_stream = None;
		}

		self.state.reset_for_start(self.loupe_patch_width_px);

		self.pending_freeze_capture = None;
		self.pending_freeze_capture_armed = false;
		self.pending_window_freeze_capture = None;
		self.inflight_window_freeze_capture = None;
		self.frozen_window_image = None;
		self.frozen_capture_source = FrozenCaptureSource::None;
		self.hit_test_send_full_count = 0;
		self.hit_test_send_disconnected_count = 0;
		self.live_cursor_sample_request_id = 0;
		self.latest_live_cursor_sample_request_id = None;
		self.applied_live_cursor_sample_request_id = None;
		self.latest_live_cursor_sample_requested_at = None;
		self.last_idle_live_sample_request_at = None;
		self.pending_click_hit_test_request_id = None;
		self.last_event_cursor = None;
		self.last_event_cursor_at = None;
		self.last_live_sample_cursor = None;
		self.live_sample_stall_started_at = None;
		self.last_live_sample_stall_log_at = None;
		self.slow_op_logger = SlowOperationLogger::default();
		self.last_hud_window_move_at = now;
		self.last_loupe_window_move_at = now;
		self.event_loop_phase = OverlayEventLoopPhase::Idle;
		self.event_loop_progress_seq = 0;
		self.event_loop_last_progress_at = now;
		self.event_loop_last_progress_window_id = None;
		self.event_loop_last_progress_monitor_id = None;
		self.event_loop_last_progress_detail = None;
		self.event_loop_last_stall_warn_at = None;

		self.clear_macos_hud_window_config_cache();

		self.window_list_snapshot = None;
		self.last_window_list_refresh_request_at = now - self.window_list_refresh_interval;
		self.toolbar_state = FrozenToolbarState::default();
		self.toolbar_left_button_down = false;
		self.toolbar_left_button_went_down = false;
		self.toolbar_left_button_went_up = false;
		self.toolbar_pointer_local = None;
		self.loupe_window_visible = false;
		self.loupe_window_warmup_redraws_remaining = 0;

		#[cfg(target_os = "macos")]
		let external_scroll_input_drain_reader =
			self.scroll_capture.external_scroll_input_drain_reader.clone();

		self.scroll_capture = ScrollCaptureState::default();
		#[cfg(target_os = "macos")]
		{
			self.scroll_capture.external_scroll_input_drain_reader =
				external_scroll_input_drain_reader;
		}
	}

	#[cfg(target_os = "macos")]
	fn clear_macos_hud_window_config_cache(&mut self) {
		self.macos_hud_window_config_cache.clear();
	}

	#[cfg(not(target_os = "macos"))]
	fn clear_macos_hud_window_config_cache(&mut self) {}

	fn available_overlay_monitors(&self) -> Result<Vec<MonitorRect>, String> {
		#[cfg(target_os = "macos")]
		{
			Self::macos_monitor_rects()
		}

		#[cfg(not(target_os = "macos"))]
		{
			let monitors =
				xcap::Monitor::all().map_err(|err| format!("xcap Monitor::all failed: {err:?}"))?;
			let mut monitor_rects = Vec::with_capacity(monitors.len());

			for monitor in &monitors {
				monitor_rects.push(Self::monitor_rect_from_xcap_monitor(monitor)?);
			}

			Ok(monitor_rects)
		}
	}

	#[cfg(target_os = "macos")]
	fn macos_monitor_rects() -> Result<Vec<MonitorRect>, String> {
		let mtm = MainThreadMarker::new()
			.ok_or_else(|| String::from("Overlay startup requires the macOS main thread."))?;
		let screens = NSScreen::screens(mtm);
		let mut monitor_rects = Vec::with_capacity(screens.len());

		for screen in screens.iter() {
			let frame = screen.frame();
			let width = frame.size.width.round().max(0.0) as u32;
			let height = frame.size.height.round().max(0.0) as u32;

			if width == 0 || height == 0 {
				continue;
			}

			let scale_factor_x1000 =
				(screen.backingScaleFactor() * 1_000.0).round().max(1.0) as u32;
			let monitor_rect = MonitorRect {
				id: screen.CGDirectDisplayID(),
				origin: GlobalPoint::new(
					frame.origin.x.round() as i32,
					frame.origin.y.round() as i32,
				),
				width,
				height,
				scale_factor_x1000,
			};

			if monitor_rect.id == 0 {
				continue;
			}

			monitor_rects.push(monitor_rect);
		}

		Ok(monitor_rects)
	}

	#[cfg(not(target_os = "macos"))]
	fn monitor_rect_from_xcap_monitor(monitor: &xcap::Monitor) -> Result<MonitorRect, String> {
		Ok(MonitorRect {
			id: monitor.id().map_err(|err| {
				format!(
					"Failed to read xcap monitor id while enumerating overlay monitors: {err:?}"
				)
			})?,
			origin: GlobalPoint::new(
				monitor.x().map_err(|err| {
					format!(
						"Failed to read xcap monitor x position while enumerating overlay monitors: {err:?}"
					)
				})?,
				monitor.y().map_err(|err| {
					format!(
						"Failed to read xcap monitor y position while enumerating overlay monitors: {err:?}"
					)
				})?,
			),
			width: monitor.width().map_err(|err| {
				format!(
					"Failed to read xcap monitor width while enumerating overlay monitors: {err:?}"
				)
			})?,
			height: monitor.height().map_err(|err| {
				format!(
					"Failed to read xcap monitor height while enumerating overlay monitors: {err:?}"
				)
			})?,
			scale_factor_x1000: {
				let scale_factor = monitor.scale_factor().map_err(|err| {
					format!(
						"Failed to read xcap monitor scale factor while enumerating overlay monitors: {err:?}"
					)
				})?;

				(scale_factor * 1_000.0).round() as u32
			},
		})
	}

	fn create_overlay_windows(
		&mut self,
		event_loop: &ActiveEventLoop,
		monitors: &[MonitorRect],
	) -> Result<(), String> {
		for monitor in monitors {
			let monitor_rect = *monitor;
			let attrs = winit::window::Window::default_attributes()
				.with_title("rsnap-overlay")
				.with_decorations(false)
				.with_resizable(false)
				.with_transparent(true)
				.with_window_level(WindowLevel::AlwaysOnTop)
				.with_inner_size(LogicalSize::new(
					monitor_rect.width as f64,
					monitor_rect.height as f64,
				))
				.with_position(LogicalPosition::new(
					monitor_rect.origin.x as f64,
					monitor_rect.origin.y as f64,
				));
			let window = event_loop
				.create_window(attrs)
				.map_err(|err| format!("Unable to create overlay window: {err}"))?;
			let window = Arc::new(window);
			let scale_factor = monitor_rect.scale_factor();
			let inner_size = window.inner_size();

			tracing::debug!(
				monitor_id = monitor_rect.id,
				origin = ?monitor_rect.origin,
				width_points = monitor_rect.width,
				height_points = monitor_rect.height,
				monitor_scale_factor = scale_factor,
				window_scale_factor = window.scale_factor(),
				window_inner_size_px = ?inner_size,
				"Overlay window created."
			);

			let _ = window.set_cursor_hittest(true);

			#[cfg(target_os = "macos")]
			overlay::macos_configure_overlay_window_mouse_moved_events(window.as_ref());

			let refresh_rate_millihertz =
				window.current_monitor().and_then(|monitor| monitor.refresh_rate_millihertz());

			window.request_redraw();
			window.focus_window();

			let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
			let renderer = WindowRenderer::new(
				gpu,
				Arc::clone(&window),
				Arc::clone(&self.egui_repaint_deadline),
			)
			.map_err(|err| format!("Failed to init renderer: {err:#}"))?;

			self.windows.insert(
				window.id(),
				OverlayWindow { monitor: monitor_rect, window, renderer, refresh_rate_millihertz },
			);
		}

		Ok(())
	}

	fn create_hud_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let attrs = winit::window::Window::default_attributes()
			.with_title("rsnap-hud")
			.with_decorations(false)
			.with_resizable(false)
			.with_transparent(true)
			.with_window_level(WindowLevel::AlwaysOnTop)
			.with_inner_size(LogicalSize::new(460.0, 52.0));
		let window = event_loop
			.create_window(attrs)
			.map_err(|err| format!("Unable to create HUD window: {err}"))?;
		let window = Arc::new(window);
		#[cfg(target_os = "macos")]
		let _ = window.set_cursor_hittest(false);
		#[cfg(not(target_os = "macos"))]
		let _ = window.set_cursor_hittest(false);

		window.set_transparent(true);
		self.configure_hud_window_common(window.as_ref(), None);

		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let renderer =
			WindowRenderer::new(gpu, Arc::clone(&window), Arc::clone(&self.egui_repaint_deadline))
				.map_err(|err| format!("Failed to init HUD renderer: {err:#}"))?;

		self.hud_window = Some(HudOverlayWindow { window, renderer });

		Ok(())
	}

	fn create_loupe_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let desired_inner_size =
			hud_helpers::stable_live_loupe_window_inner_size_points(self.state.loupe_patch_side_px);
		let attrs = winit::window::Window::default_attributes()
			.with_title("rsnap-loupe")
			.with_decorations(false)
			.with_resizable(false)
			.with_transparent(true)
			.with_visible(false)
			.with_window_level(WindowLevel::AlwaysOnTop)
			.with_inner_size(LogicalSize::new(
				f64::from(desired_inner_size.0),
				f64::from(desired_inner_size.1),
			));
		let window = event_loop
			.create_window(attrs)
			.map_err(|err| format!("Unable to create loupe window: {err}"))?;
		let window = Arc::new(window);
		#[cfg(target_os = "macos")]
		let _ = window.set_cursor_hittest(false);
		#[cfg(not(target_os = "macos"))]
		let _ = window.set_cursor_hittest(false);

		window.set_transparent(true);
		self.configure_hud_window_common(window.as_ref(), Some(LOUPE_TILE_CORNER_RADIUS_POINTS));

		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let renderer =
			WindowRenderer::new(gpu, Arc::clone(&window), Arc::clone(&self.egui_repaint_deadline))
				.map_err(|err| format!("Failed to init loupe renderer: {err:#}"))?;

		self.loupe_inner_size_points = Some(desired_inner_size);
		self.loupe_window = Some(HudOverlayWindow { window, renderer });

		Ok(())
	}

	fn create_toolbar_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let attrs = winit::window::Window::default_attributes()
			.with_title("rsnap-toolbar")
			.with_decorations(false)
			.with_resizable(false)
			.with_inner_size(LogicalSize::new(
				TOOLBAR_EXPANDED_WIDTH_PX as f64,
				TOOLBAR_EXPANDED_HEIGHT_PX as f64,
			))
			.with_transparent(true)
			.with_visible(false)
			.with_window_level(WindowLevel::AlwaysOnTop);
		let window = event_loop
			.create_window(attrs)
			.map_err(|err| format!("Unable to create toolbar window: {err}"))?;
		let window = Arc::new(window);
		#[cfg(target_os = "macos")]
		let _ = window.set_cursor_hittest(true);
		#[cfg(not(target_os = "macos"))]
		let _ = window.set_cursor_hittest(false);

		window.set_transparent(true);
		self.configure_hud_window_common(
			window.as_ref(),
			Some(f64::from(HUD_PILL_CORNER_RADIUS_POINTS)),
		);
		window.request_redraw();

		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let renderer =
			WindowRenderer::new(gpu, Arc::clone(&window), Arc::clone(&self.egui_repaint_deadline))
				.map_err(|err| format!("Failed to init toolbar renderer: {err:#}"))?;

		self.toolbar_window = Some(HudOverlayWindow { window, renderer });

		Ok(())
	}

	fn create_scroll_preview_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let window = ScrollPreviewWindow::new(event_loop, gpu)?;

		self.scroll_preview_window = Some(window);

		Ok(())
	}

	pub(super) fn request_redraw_all(&self) {
		for w in self.windows.values() {
			w.window.request_redraw();
		}

		if let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
		if let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}
		if let Some(toolbar) = self.toolbar_window.as_ref() {
			toolbar.window.request_redraw();
		}
		if let Some(preview) = self.scroll_preview_window.as_ref() {
			preview.window.request_redraw();
		}
	}

	pub(super) fn request_redraw_for_monitor(&self, monitor: MonitorRect) {
		for w in self.windows.values() {
			if w.monitor == monitor {
				w.window.request_redraw();
			}
		}

		if let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
		if let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}

		// macOS uses a native toolbar popup window with compositor blur; keep shader-viewport
		// toolbar redraw on the fullscreen overlay path disabled for this platform.
		// Future direction: if toolbar styling moves off native blur, add a dedicated capture
		// pass feeding a toolbar-local shader-blur texture.
		if cfg!(target_os = "macos")
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.state.monitor == Some(monitor)
			&& self.state.frozen_image.is_some()
			&& self.pending_freeze_capture != Some(monitor)
		{
			self.request_redraw_toolbar_window();
		}

		self.request_redraw_scroll_preview_window();
	}

	pub(super) fn request_redraw_hud_window(&self) {
		if let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
	}

	pub(super) fn request_redraw_toolbar_window(&self) {
		if let Some(toolbar) = self.toolbar_window.as_ref() {
			toolbar.window.request_redraw();
		}
	}

	pub(super) fn request_redraw_loupe_window(&self) {
		if let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}
	}

	pub(super) fn request_redraw_scroll_preview_window(&self) {
		if let Some(preview) = self.scroll_preview_window.as_ref() {
			preview.window.request_redraw();
		}
	}

	pub(super) fn request_redraw_live_sample_targets(
		&self,
		monitor: MonitorRect,
		apply: LiveSampleApplyResult,
	) {
		if apply.overlay_changed {
			for window in self.windows.values() {
				if window.monitor == monitor {
					window.window.request_redraw();
				}
			}
		}
		if apply.hud_changed {
			self.request_redraw_hud_window();
		}
		if apply.loupe_changed {
			if self.live_loupe_uses_hud_window() {
				self.request_redraw_hud_window();
			} else {
				self.request_redraw_loupe_window();
			}
		}
	}
}
