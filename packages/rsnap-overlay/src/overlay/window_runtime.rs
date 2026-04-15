use std::collections::HashMap;
use std::mem;
use std::sync::{Arc, Mutex};
#[cfg(not(target_os = "macos"))]
use std::time::Duration;
use std::time::Instant;

#[cfg(target_os = "macos")]
use objc2_foundation::NSArray;
use winit::window::{Window, WindowId};

use crate::backend;
use crate::overlay;
#[cfg(target_os = "macos")]
use crate::overlay::MacOSHudWindowConfigState;
use crate::overlay::{
	ActiveEventLoop, GlobalPoint, GpuContext, HudOverlayWindow, LOUPE_TILE_CORNER_RADIUS_POINTS,
	LiveSampleApplyResult, LogicalPosition, LogicalSize, MonitorRect, OverlayMode, OverlaySession,
	OverlayWindow, OverlayWorker, Result, ScrollPreviewWindow, TOOLBAR_EXPANDED_HEIGHT_PX,
	WindowLevel, WindowRenderer, hud_helpers,
};
#[cfg(target_os = "macos")]
use crate::overlay::{MacLiveFrameStream, MainThreadMarker, NSScreen};

impl OverlaySession {
	/// Starts the overlay session and creates the required capture windows.
	pub fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let startup_started_at = Instant::now();

		if self.is_active() {
			return Ok(());
		}

		let reset_started_at = Instant::now();

		self.reset_for_start();
		#[cfg(target_os = "macos")]
		self.capture_frontmost_application_for_exit_restore();

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
		let startup_stream_prime_started_at = Instant::now();

		self.prime_startup_live_stream_nonblocking(startup_monitor);

		let startup_stream_prime_ms = startup_stream_prime_started_at.elapsed().as_millis();
		let gpu_init_started_at = Instant::now();

		if self.gpu.is_none() {
			self.gpu = Some(GpuContext::new().map_err(|err| format!("{err:#}"))?);
		}

		let gpu_init_ms = gpu_init_started_at.elapsed().as_millis();
		#[cfg(target_os = "macos")]
		let reused_prewarmed_windows = self.has_matching_prewarmed_startup_resources(&monitors);
		let window_creation = self.create_startup_windows(event_loop, &monitors)?;

		#[cfg(target_os = "macos")]
		if !reused_prewarmed_windows {
			self.refresh_startup_live_stream_after_window_creation(startup_monitor);
		}

		let prime_cursor_started_at = Instant::now();

		self.prime_startup_cursor_context(startup_cursor, startup_monitor);

		let prime_cursor_ms = prime_cursor_started_at.elapsed().as_millis();
		let startup_seed_ms = self.seed_startup_live_cursor(startup_monitor, startup_cursor);
		let initialize_cursor_started_at = Instant::now();

		self.initialize_cursor_state_for_cursor(startup_cursor, startup_monitor);

		let initialize_cursor_ms = initialize_cursor_started_at.elapsed().as_millis();
		let request_redraw_started_at = Instant::now();

		self.session_active = true;

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
			startup_stream_prime_ms,
			gpu_init_ms,
			overlay_windows_ms = window_creation.overlay_windows_ms,
			hud_window_ms = window_creation.hud_window_ms,
			loupe_window_ms = window_creation.loupe_window_ms,
			toolbar_window_ms = window_creation.toolbar_window_ms,
			scroll_preview_window_ms = window_creation.scroll_preview_window_ms,
			startup_windows_source = window_creation.startup_windows_source,
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

	/// Pre-creates the GPU context plus hidden overlay/HUD windows so the first capture can
	/// reuse them instead of paying the full cold-start cost on demand.
	pub fn prewarm(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		if self.is_active() || self.has_prewarmed_startup_resources() {
			return Ok(());
		}

		let prewarm_started_at = Instant::now();
		let monitor_enum_started_at = Instant::now();
		let monitors = self.available_overlay_monitors()?;
		let monitor_enum_ms = monitor_enum_started_at.elapsed().as_millis();

		if monitors.is_empty() {
			return Err(String::from("No monitors detected"));
		}

		let gpu_init_started_at = Instant::now();

		if self.gpu.is_none() {
			self.gpu = Some(GpuContext::new().map_err(|err| format!("{err:#}"))?);
		}

		let gpu_init_ms = gpu_init_started_at.elapsed().as_millis();

		if !self.windows.is_empty() || self.hud_window.is_some() {
			self.discard_prewarmed_startup_resources();
		}

		let overlay_windows_started_at = Instant::now();

		self.create_overlay_windows(event_loop, &monitors, false)?;

		let overlay_windows_ms = overlay_windows_started_at.elapsed().as_millis();
		let hud_window_started_at = Instant::now();

		self.create_hud_window(event_loop)?;

		let hud_window_ms = hud_window_started_at.elapsed().as_millis();

		tracing::info!(
			op = "overlay.prewarm_phase_timing",
			monitor_count = monitors.len(),
			window_count = self.windows.len(),
			monitor_enum_ms,
			gpu_init_ms,
			overlay_windows_ms,
			hud_window_ms,
			total_ms = prewarm_started_at.elapsed().as_millis(),
			"Overlay startup resources prewarmed."
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
		let reused_prewarmed_windows = self.has_matching_prewarmed_startup_resources(monitors);

		if reused_prewarmed_windows {
			self.activate_prewarmed_overlay_windows();
		} else {
			self.discard_prewarmed_startup_resources();
			self.create_overlay_windows(event_loop, monitors, true)?;
		}

		let overlay_windows_ms = overlay_windows_started_at.elapsed().as_millis();
		let hud_window_started_at = Instant::now();

		if reused_prewarmed_windows {
			self.configure_hud_windows_for_config();
		} else {
			self.create_hud_window(event_loop)?;
		}

		let hud_window_ms = hud_window_started_at.elapsed().as_millis();
		let startup_windows_source = if reused_prewarmed_windows { "prewarmed" } else { "fresh" };

		#[cfg(target_os = "macos")]
		{
			self.startup_aux_window_creation_pending = false;
			self.startup_aux_window_creation_scheduled = false;

			Ok(StartupWindowCreationMetrics {
				overlay_windows_ms,
				hud_window_ms,
				loupe_window_ms: 0,
				toolbar_window_ms: 0,
				scroll_preview_window_ms: 0,
				startup_windows_source,
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
				startup_windows_source,
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
	pub(super) fn refresh_startup_live_stream_after_window_creation(
		&mut self,
		startup_monitor: Option<MonitorRect>,
	) {
		self.live_sample_stream = Some(MacLiveFrameStream::with_self_capture_exception_window_ids(
			self.config.self_capture_exception_window_ids.clone(),
		));

		self.prime_startup_live_stream_nonblocking(startup_monitor);
	}

	#[cfg(target_os = "macos")]
	/// Completes creation of non-critical auxiliary windows after the first overlay frame.
	pub fn finish_startup_aux_window_creation(
		&mut self,
		event_loop: &ActiveEventLoop,
	) -> Result<(), String> {
		if !self.startup_aux_window_creation_pending && !self.aux_window_creation_needed() {
			return Ok(());
		}

		self.startup_aux_window_creation_scheduled = false;

		let mut created_aux_windows = false;

		if self.loupe_window.is_none() && self.loupe_window_needed() {
			self.create_loupe_window(event_loop)?;

			created_aux_windows = true;
		}
		if self.toolbar_window.is_none() && self.toolbar_window_needed() {
			self.create_toolbar_window(event_loop)?;

			created_aux_windows = true;
		}
		if self.scroll_preview_window.is_none() && self.scroll_preview_window_needed() {
			self.create_scroll_preview_window(event_loop)?;

			created_aux_windows = true;
		}

		self.complete_startup_aux_window_creation(created_aux_windows);

		if created_aux_windows
			&& let Some(monitor) = self.scroll_capture.monitor
			&& self.rebuild_active_scroll_capture_live_stream()
			&& let Some(live_stream) = self.scroll_capture.live_stream.as_ref()
		{
			live_stream.prime_monitor_nonblocking(monitor);
		}
		if self.loupe_window_needed() {
			self.set_alt_loupe_window_visible(self.active_cursor_monitor(), true);
		}
		if self.toolbar_window_needed() {
			self.request_redraw_toolbar_window();
		}
		if self.scroll_preview_window_needed() {
			if let Some(monitor) = self.scroll_capture.monitor {
				self.position_scroll_preview_window(monitor);
			}

			self.request_redraw_scroll_preview_window();
		}

		Ok(())
	}

	#[cfg(target_os = "macos")]
	fn loupe_window_needed(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Live)
			&& self.state.alt_held
			&& !self.live_loupe_uses_hud_window()
	}

	#[cfg(target_os = "macos")]
	fn toolbar_window_needed(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.frozen_preview_visible()
	}

	#[cfg(target_os = "macos")]
	fn scroll_preview_window_needed(&self) -> bool {
		self.scroll_capture.active
	}

	#[cfg(target_os = "macos")]
	fn aux_window_creation_needed(&self) -> bool {
		(self.loupe_window.is_none() && self.loupe_window_needed())
			|| (self.toolbar_window.is_none() && self.toolbar_window_needed())
			|| (self.scroll_preview_window.is_none() && self.scroll_preview_window_needed())
	}

	#[cfg(target_os = "macos")]
	pub(super) fn request_aux_window_creation_if_needed(&mut self) {
		if !self.aux_window_creation_needed() {
			return;
		}

		self.startup_aux_window_creation_pending = true;

		self.maybe_schedule_startup_aux_window_creation();
	}

	#[cfg(target_os = "macos")]
	pub(super) fn complete_startup_aux_window_creation(&mut self, created_aux_windows: bool) {
		self.startup_aux_window_creation_pending = false;

		if created_aux_windows {
			if self.latest_live_cursor_sample_request_id.is_some() {
				// If startup already primed live sampling, keep the existing stream alive
				// and defer the narrow ScreenCaptureKit upgrade until an auxiliary window
				// is actually shown.
				self.pending_startup_aux_live_stream_filter_upgrade = true;
			} else {
				// Delay the first live-stream ensure until after the aux windows exist so
				// startup can begin with the full self-capture exclusion set.
				self.kick_startup_live_sampling();
			}
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn maybe_apply_pending_startup_aux_live_stream_filter_upgrade(
		&mut self,
		monitor: MonitorRect,
	) {
		if !self.pending_startup_aux_live_stream_filter_upgrade {
			return;
		}

		let Some(stream) = self.live_sample_stream.as_ref() else {
			return;
		};

		if stream.upgrade_monitor_nonblocking(monitor) {
			self.pending_startup_aux_live_stream_filter_upgrade = false;
		}
	}

	#[cfg(not(target_os = "macos"))]
	pub(super) fn maybe_apply_pending_startup_aux_live_stream_filter_upgrade(
		&mut self,
		monitor: MonitorRect,
	) {
		let _ = monitor;
	}

	pub(super) fn reset_for_start(&mut self) {
		#[cfg(target_os = "macos")]
		self.set_scroll_overlay_mouse_passthrough(false);

		let config = self.config.clone();
		let prewarmed_startup_resources = self.take_prewarmed_startup_resources();
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

		self.restore_prewarmed_startup_resources(prewarmed_startup_resources);

		self.response_waker = response_waker;
		#[cfg(target_os = "macos")]
		{
			self.scroll_frame_waker = scroll_frame_waker;
			self.scroll_capture_start_guard = scroll_capture_start_guard;
			self.scroll_capture_starting_hook = scroll_capture_starting_hook;
			self.scroll_capture_started_hook = scroll_capture_started_hook;
			self.startup_aux_window_waker = startup_aux_window_waker;
			self.pending_startup_aux_live_stream_filter_upgrade = false;
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
		visible: bool,
	) -> Result<(), String> {
		for monitor in monitors {
			let monitor_rect = *monitor;
			let mut attrs = Window::default_attributes()
				.with_title("rsnap-overlay")
				.with_decorations(false)
				.with_resizable(false)
				.with_content_protected(overlay::CAPTURE_WINDOW_CONTENT_PROTECTION_ENABLED)
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

			if !visible {
				attrs = attrs.with_visible(false);
			}

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

			#[cfg(target_os = "macos")]
			let cursor_rects = overlay::macos_install_overlay_cursor_rect_support(window.as_ref())?;
			let refresh_rate_millihertz =
				window.current_monitor().and_then(|monitor| monitor.refresh_rate_millihertz());

			if visible {
				window.request_redraw();
				#[cfg(not(target_os = "macos"))]
				window.focus_window();
			}

			let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
			let renderer = WindowRenderer::new(
				gpu,
				Arc::clone(&window),
				Arc::clone(&self.egui_repaint_deadline),
			)
			.map_err(|err| format!("Failed to init renderer: {err:#}"))?;

			self.windows.insert(
				window.id(),
				OverlayWindow {
					monitor: monitor_rect,
					#[cfg(target_os = "macos")]
					cursor_rects,
					window,
					renderer,
					refresh_rate_millihertz,
				},
			);
		}

		Ok(())
	}

	fn activate_prewarmed_overlay_windows(&self) {
		for window in self.windows.values() {
			window.window.set_visible(true);
		}
	}

	fn has_matching_prewarmed_startup_resources(&self, monitors: &[MonitorRect]) -> bool {
		self.has_prewarmed_startup_resources()
			&& self.windows.len() == monitors.len()
			&& monitors
				.iter()
				.all(|monitor| self.windows.values().any(|window| window.monitor == *monitor))
	}

	fn discard_prewarmed_startup_resources(&mut self) {
		self.windows.clear();

		self.hud_window = None;
		self.hud_inner_size_points = None;
		self.hud_outer_pos = None;
		self.pending_hud_outer_pos = None;

		#[cfg(target_os = "macos")]
		self.macos_hud_window_config_cache.clear();
	}

	fn take_prewarmed_startup_resources(&mut self) -> Option<PrewarmedStartupResources> {
		if !self.has_prewarmed_startup_resources() {
			return None;
		}

		Some(PrewarmedStartupResources {
			egui_repaint_deadline: Arc::clone(&self.egui_repaint_deadline),
			gpu: self.gpu.take(),
			windows: mem::take(&mut self.windows),
			hud_window: self.hud_window.take(),
			#[cfg(target_os = "macos")]
			macos_hud_window_config_cache: mem::take(&mut self.macos_hud_window_config_cache),
		})
	}

	fn restore_prewarmed_startup_resources(
		&mut self,
		resources: Option<PrewarmedStartupResources>,
	) {
		let Some(resources) = resources else {
			return;
		};

		self.egui_repaint_deadline = resources.egui_repaint_deadline;
		self.gpu = resources.gpu;
		self.windows = resources.windows;
		self.hud_window = resources.hud_window;
		#[cfg(target_os = "macos")]
		{
			self.macos_hud_window_config_cache = resources.macos_hud_window_config_cache;
		}
	}

	fn create_hud_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let attrs = Window::default_attributes()
			.with_title("rsnap-hud")
			.with_decorations(false)
			.with_resizable(false)
			.with_content_protected(overlay::CAPTURE_WINDOW_CONTENT_PROTECTION_ENABLED)
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
			.with_content_protected(overlay::CAPTURE_WINDOW_CONTENT_PROTECTION_ENABLED)
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
		let startup_size = super::frozen_toolbar_window_startup_size_points();
		let attrs = Window::default_attributes()
			.with_title("rsnap-toolbar")
			.with_decorations(false)
			.with_resizable(false)
			.with_content_protected(overlay::CAPTURE_WINDOW_CONTENT_PROTECTION_ENABLED)
			.with_inner_size(LogicalSize::new(
				startup_size.x as f64,
				f64::from(startup_size.y.max(TOOLBAR_EXPANDED_HEIGHT_PX)),
			))
			.with_transparent(true)
			.with_visible(false)
			.with_window_level(WindowLevel::AlwaysOnTop);
		let window = event_loop
			.create_window(attrs)
			.map_err(|err| format!("Unable to create toolbar window: {err}"))?;
		let window = Arc::new(window);
		#[cfg(target_os = "macos")]
		let _ = window.set_cursor_hittest(false);
		#[cfg(not(target_os = "macos"))]
		let _ = window.set_cursor_hittest(false);

		window.set_transparent(true);
		self.configure_hud_window_common(
			window.as_ref(),
			Some(overlay::frozen_toolbar_corner_radius_points(
				WindowRenderer::frozen_toolbar_primary_size(&self.toolbar_state).y,
			)),
		);
		window.request_redraw();

		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let renderer =
			WindowRenderer::new(gpu, Arc::clone(&window), Arc::clone(&self.egui_repaint_deadline))
				.map_err(|err| format!("Failed to init toolbar renderer: {err:#}"))?;

		#[cfg(target_os = "macos")]
		{
			self.toolbar_inner_size_points = Some((
				startup_size.x.ceil().max(1.0) as u32,
				startup_size.y.ceil().max(1.0) as u32,
			));
			self.toolbar_window_cursor_hittest_enabled = false;
		}
		#[cfg(not(target_os = "macos"))]
		{
			self.toolbar_inner_size_points = None;
		}
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

		let hide_auxiliary_windows = self.frozen_selection_drag_hides_auxiliary_windows();
		let hide_live_drag_auxiliary_windows = self.live_drag_hides_auxiliary_windows();
		let request_hud_window = !hide_auxiliary_windows
			&& !hide_live_drag_auxiliary_windows
			&& self.hud_window.is_some();
		let request_loupe_window = !hide_auxiliary_windows
			&& !hide_live_drag_auxiliary_windows
			&& self.loupe_window.is_some();
		let request_toolbar_window = !hide_auxiliary_windows
			&& cfg!(target_os = "macos")
			&& self.frozen_display_ready_for_monitor(monitor)
			&& self.toolbar_state.visible;
		let request_scroll_preview_window =
			!hide_auxiliary_windows && self.scroll_preview_window.is_some();

		if tracing::enabled!(tracing::Level::TRACE)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.frozen_selection_drag.active
			&& self.state.monitor == Some(monitor)
		{
			let overlay_windows =
				self.windows.values().filter(|window| window.monitor == monitor).count();

			tracing::trace!(
				op = "overlay.frozen_selection_drag.redraw_fanout",
				monitor_id = monitor.id,
				overlay_window_count = overlay_windows,
				request_hud_window,
				request_loupe_window,
				request_toolbar_window,
				request_scroll_preview_window,
				hide_auxiliary_windows,
				scroll_capture_active = self.scroll_capture.active,
				alt_held = self.state.alt_held,
				"Requested redraw fan-out for frozen selection drag."
			);
		}
		if hide_auxiliary_windows {
			return;
		}
		if request_hud_window && let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
		if request_loupe_window && let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}
		// macOS uses a native toolbar popup window with compositor blur; keep shader-viewport
		// toolbar redraw on the fullscreen overlay path disabled for this platform.
		// Future direction: if toolbar styling moves off native blur, add a dedicated capture
		// pass feeding a toolbar-local shader-blur texture.
		if request_toolbar_window {
			self.request_redraw_toolbar_window();
		}
		if request_scroll_preview_window {
			self.request_redraw_scroll_preview_window();
		}
	}

	pub(super) fn request_redraw_hud_window(&self) {
		if self.frozen_selection_drag_hides_auxiliary_windows()
			|| self.live_drag_hides_auxiliary_windows()
		{
			return;
		}

		if let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
	}

	pub(super) fn request_redraw_toolbar_window(&self) {
		if self.frozen_selection_drag_hides_auxiliary_windows() {
			return;
		}

		if let Some(toolbar) = self.toolbar_window.as_ref() {
			toolbar.window.request_redraw();
		}
	}

	pub(super) fn request_redraw_loupe_window(&self) {
		if self.frozen_selection_drag_hides_auxiliary_windows()
			|| self.live_drag_hides_auxiliary_windows()
		{
			return;
		}

		if let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}
	}

	pub(super) fn request_redraw_scroll_preview_window(&self) {
		if self.frozen_selection_drag_hides_auxiliary_windows() {
			return;
		}

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
	startup_windows_source: &'static str,
}

struct PrewarmedStartupResources {
	egui_repaint_deadline: Arc<Mutex<Option<Instant>>>,
	gpu: Option<GpuContext>,
	windows: HashMap<WindowId, OverlayWindow>,
	hud_window: Option<HudOverlayWindow>,
	#[cfg(target_os = "macos")]
	macos_hud_window_config_cache: HashMap<WindowId, MacOSHudWindowConfigState>,
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
