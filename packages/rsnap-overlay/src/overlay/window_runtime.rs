use std::sync::Arc;
#[cfg(not(target_os = "macos"))]
use std::time::Duration;
use std::time::Instant;

#[cfg(target_os = "macos")]
use objc2_foundation::NSArray;
use winit::window::Window;

use crate::backend;
#[cfg(target_os = "macos")]
use crate::overlay::{self, MacLiveFrameStream, MainThreadMarker, NSScreen};
use crate::overlay::{
	ActiveEventLoop, GlobalPoint, GpuContext, HUD_PILL_CORNER_RADIUS_POINTS, HudOverlayWindow,
	LOUPE_TILE_CORNER_RADIUS_POINTS, LiveSampleApplyResult, LogicalPosition, LogicalSize,
	MonitorRect, OverlayMode, OverlaySession, OverlayWindow, OverlayWorker, Result,
	ScrollPreviewWindow, TOOLBAR_EXPANDED_HEIGHT_PX, TOOLBAR_EXPANDED_WIDTH_PX, WindowLevel,
	WindowRenderer, hud_helpers,
};

impl OverlaySession {
	/// Starts the overlay session and creates the required capture windows.
	pub fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let startup_started_at = Instant::now();

		if self.is_active() {
			return Ok(());
		}

		let reset_started_at = Instant::now();

		self.reset_for_start();

		let reset_ms = reset_started_at.elapsed().as_millis();
		let worker_setup_ms = self.setup_startup_worker();
		let monitor_enum_started_at = Instant::now();
		let monitors = self.available_overlay_monitors()?;
		let monitor_enum_ms = monitor_enum_started_at.elapsed().as_millis();

		if monitors.is_empty() {
			return Err(String::from("No monitors detected"));
		}

		let startup_cursor = self.sample_mouse_location();
		let startup_monitor = Self::monitor_for_cursor_in_rects(&monitors, startup_cursor);
		let gpu_init_started_at = Instant::now();

		self.gpu = Some(GpuContext::new().map_err(|err| format!("{err:#}"))?);

		let gpu_init_ms = gpu_init_started_at.elapsed().as_millis();
		let window_creation = self.create_startup_windows(event_loop, &monitors)?;
		let prime_cursor_started_at = Instant::now();

		self.prime_startup_cursor_context(startup_cursor, startup_monitor);

		let prime_cursor_ms = prime_cursor_started_at.elapsed().as_millis();
		let startup_seed_ms = self.seed_startup_live_cursor(startup_monitor, startup_cursor);
		let initialize_cursor_started_at = Instant::now();

		self.initialize_cursor_state_for_cursor(startup_cursor, startup_monitor);

		let initialize_cursor_ms = initialize_cursor_started_at.elapsed().as_millis();
		let request_redraw_started_at = Instant::now();

		self.request_redraw_all();

		let request_redraw_ms = request_redraw_started_at.elapsed().as_millis();

		tracing::info!(
			op = "overlay.start_phase_timing",
			mode = ?self.state.mode,
			monitor_count = monitors.len(),
			window_count = self.windows.len(),
			startup_monitor_id = ?startup_monitor.map(|monitor| monitor.id),
			reset_ms,
			worker_setup_ms,
			monitor_enum_ms,
			gpu_init_ms,
			overlay_windows_ms = window_creation.overlay_windows_ms,
			hud_window_ms = window_creation.hud_window_ms,
			loupe_window_ms = window_creation.loupe_window_ms,
			toolbar_window_ms = window_creation.toolbar_window_ms,
			scroll_preview_window_ms = window_creation.scroll_preview_window_ms,
			startup_aux_windows_deferred = cfg!(target_os = "macos"),
			prime_cursor_ms,
			startup_seed_ms,
			initialize_cursor_ms,
			request_redraw_ms,
			total_ms = startup_started_at.elapsed().as_millis(),
			"Overlay start phase timing."
		);

		Ok(())
	}

	fn setup_startup_worker(&mut self) -> u128 {
		let worker_setup_started_at = Instant::now();

		self.worker = Some(OverlayWorker::new(
			backend::default_capture_backend_with_self_capture_exception_window_ids(
				self.config.self_capture_exception_window_ids.clone(),
			),
			self.response_waker.clone(),
		));
		#[cfg(target_os = "macos")]
		{
			self.live_sample_stream =
				Some(MacLiveFrameStream::with_self_capture_exception_window_ids(
					self.config.self_capture_exception_window_ids.clone(),
				));
		}

		worker_setup_started_at.elapsed().as_millis()
	}

	fn create_startup_windows(
		&mut self,
		event_loop: &ActiveEventLoop,
		monitors: &[MonitorRect],
	) -> Result<StartupWindowCreationMetrics, String> {
		let overlay_windows_started_at = Instant::now();

		self.create_overlay_windows(event_loop, monitors)?;

		let overlay_windows_ms = overlay_windows_started_at.elapsed().as_millis();
		let hud_window_started_at = Instant::now();

		self.create_hud_window(event_loop)?;

		let hud_window_ms = hud_window_started_at.elapsed().as_millis();

		#[cfg(target_os = "macos")]
		{
			self.startup_aux_window_creation_pending = true;
			self.startup_aux_window_creation_scheduled = false;

			Ok(StartupWindowCreationMetrics {
				overlay_windows_ms,
				hud_window_ms,
				loupe_window_ms: 0,
				toolbar_window_ms: 0,
				scroll_preview_window_ms: 0,
			})
		}
		#[cfg(not(target_os = "macos"))]
		{
			let loupe_window_started_at = Instant::now();

			self.create_loupe_window(event_loop)?;

			let loupe_window_ms = loupe_window_started_at.elapsed().as_millis();
			let toolbar_window_started_at = Instant::now();

			self.create_toolbar_window(event_loop)?;

			let toolbar_window_ms = toolbar_window_started_at.elapsed().as_millis();
			let scroll_preview_window_started_at = Instant::now();

			self.create_scroll_preview_window(event_loop)?;

			let scroll_preview_window_ms = scroll_preview_window_started_at.elapsed().as_millis();

			Ok(StartupWindowCreationMetrics {
				overlay_windows_ms,
				hud_window_ms,
				loupe_window_ms,
				toolbar_window_ms,
				scroll_preview_window_ms,
			})
		}
	}

	fn seed_startup_live_cursor(
		&mut self,
		startup_monitor: Option<MonitorRect>,
		startup_cursor: GlobalPoint,
	) -> u128 {
		#[cfg(target_os = "macos")]
		{
			let startup_seed_started_at = Instant::now();
			let startup_live_rgb_plan = Self::startup_live_rgb_plan(startup_monitor);

			if startup_live_rgb_plan.focus_window {
				self.focus_live_capture_window();
			}

			if let Some(monitor) = startup_live_rgb_plan.seed_monitor {
				self.seed_startup_live_cursor_rgb(monitor, startup_cursor);
			}

			startup_seed_started_at.elapsed().as_millis()
		}
		#[cfg(not(target_os = "macos"))]
		{
			let _ = startup_monitor;
			let _ = startup_cursor;

			Duration::ZERO.as_millis()
		}
	}

	#[cfg(target_os = "macos")]
	/// Completes creation of non-critical auxiliary windows after the first overlay frame.
	pub fn finish_startup_aux_window_creation(
		&mut self,
		event_loop: &ActiveEventLoop,
	) -> Result<(), String> {
		if !self.startup_aux_window_creation_pending {
			return Ok(());
		}

		self.startup_aux_window_creation_scheduled = false;

		let mut created_aux_windows = false;

		if self.loupe_window.is_none() {
			self.create_loupe_window(event_loop)?;

			created_aux_windows = true;
		}
		if self.toolbar_window.is_none() {
			self.create_toolbar_window(event_loop)?;

			created_aux_windows = true;
		}
		if self.scroll_preview_window.is_none() {
			self.create_scroll_preview_window(event_loop)?;

			created_aux_windows = true;
		}

		self.complete_startup_aux_window_creation(created_aux_windows);

		if self.state.alt_held {
			self.set_alt_loupe_window_visible(self.active_cursor_monitor(), true);
		}
		if self.toolbar_state.visible {
			self.request_redraw_toolbar_window();
		}
		if self.scroll_capture.active {
			self.request_redraw_scroll_preview_window();
		}

		Ok(())
	}

	#[cfg(target_os = "macos")]
	pub(super) fn complete_startup_aux_window_creation(&mut self, created_aux_windows: bool) {
		self.startup_aux_window_creation_pending = false;

		if created_aux_windows {
			// When ScreenCaptureKit falls back to excluding only currently shareable
			// rsnap windows, deferred aux windows must exist before we rebuild the
			// active filters or they can remain visible in the live stream.
			self.apply_self_capture_exception_window_ids_to_active_streams();
		}
	}

	pub(super) fn reset_for_start(&mut self) {
		#[cfg(target_os = "macos")]
		self.set_scroll_overlay_mouse_passthrough(false);

		let config = self.config.clone();
		let response_waker = self.response_waker.clone();
		#[cfg(target_os = "macos")]
		let scroll_frame_waker = self.scroll_frame_waker.clone();
		#[cfg(target_os = "macos")]
		let scroll_capture_start_guard = self.scroll_capture_start_guard.clone();
		#[cfg(target_os = "macos")]
		let scroll_capture_starting_hook = self.scroll_capture_starting_hook.clone();
		#[cfg(target_os = "macos")]
		let scroll_capture_started_hook = self.scroll_capture_started_hook.clone();
		#[cfg(target_os = "macos")]
		let startup_aux_window_waker = self.startup_aux_window_waker.clone();
		#[cfg(target_os = "macos")]
		let external_scroll_input_drain_reader =
			self.scroll_capture.external_scroll_input_drain_reader.clone();

		*self = Self::with_config(config);
		self.response_waker = response_waker;
		#[cfg(target_os = "macos")]
		{
			self.scroll_frame_waker = scroll_frame_waker;
			self.scroll_capture_start_guard = scroll_capture_start_guard;
			self.scroll_capture_starting_hook = scroll_capture_starting_hook;
			self.scroll_capture_started_hook = scroll_capture_started_hook;
			self.startup_aux_window_waker = startup_aux_window_waker;
			self.scroll_capture.external_scroll_input_drain_reader =
				external_scroll_input_drain_reader;
		}
	}

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
		let main_display_height = Self::main_display_height_points_from_screens(&screens)
			.ok_or_else(|| String::from("Overlay startup could not determine the main display."))?;
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
					Self::window_server_top_from_appkit_bounds(
						frame.origin.y.round() as i64,
						height.into(),
						main_display_height,
					) as i32,
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

	#[cfg(any(test, target_os = "macos"))]
	fn window_server_top_from_appkit_bounds(
		appkit_origin_y: i64,
		frame_height: i64,
		main_display_height: i64,
	) -> i64 {
		// AppKit screen coordinates are rooted at the main display's lower-left corner, while
		// Quartz global display coordinates are rooted at the main display's upper-left corner.
		// Secondary-display offsets are already encoded in `appkit_origin_y`, so only the main
		// display height participates in the y-axis flip into WindowServer space.
		main_display_height - (appkit_origin_y + frame_height)
	}

	#[cfg(target_os = "macos")]
	fn main_display_height_points_from_screens(screens: &NSArray<NSScreen>) -> Option<i64> {
		screens
			.iter()
			.find_map(|screen| {
				let frame = screen.frame();
				let origin_x = frame.origin.x.round() as i64;
				let origin_y = frame.origin.y.round() as i64;

				(origin_x == 0 && origin_y == 0).then(|| frame.size.height.round() as i64)
			})
			.or_else(|| {
				screens.iter().next().map(|screen| screen.frame().size.height.round() as i64)
			})
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
			let attrs = Window::default_attributes()
				.with_title("rsnap-overlay")
				.with_decorations(false)
				.with_resizable(false)
				.with_transparent(true)
				.with_window_level(WindowLevel::AlwaysOnTop)
				.with_inner_size(LogicalSize::new(
					monitor_rect.width as f64,
					monitor_rect.height as f64,
				))
				// On macOS, winit window positions use top-left desktop coordinates and flip
				// back into AppKit space internally, so the WindowServer-space monitor origin
				// remains the correct placement/input coordinate system here.
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
		let attrs = Window::default_attributes()
			.with_title("rsnap-hud")
			.with_decorations(false)
			.with_resizable(false)
			.with_transparent(true)
			.with_visible(false)
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
		let attrs = Window::default_attributes()
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
		let attrs = Window::default_attributes()
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

struct StartupWindowCreationMetrics {
	overlay_windows_ms: u128,
	hud_window_ms: u128,
	loupe_window_ms: u128,
	toolbar_window_ms: u128,
	scroll_preview_window_ms: u128,
}

#[cfg(test)]
mod tests {
	use crate::overlay::OverlaySession;

	#[test]
	fn window_server_top_from_appkit_bounds_maps_display_above_main_into_negative_y() {
		assert_eq!(OverlaySession::window_server_top_from_appkit_bounds(1_120, 360, 900), -580);
	}

	#[test]
	fn window_server_top_from_appkit_bounds_maps_display_below_main_below_main_display_height() {
		assert_eq!(OverlaySession::window_server_top_from_appkit_bounds(-760, 360, 900), 1_300);
	}
}
