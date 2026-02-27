use std::{
	collections::HashMap,
	sync::Arc,
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Result, WrapErr};
use device_query::{DeviceQuery, Keycode};
use egui::ClippedPrimitive;
use egui::FullOutput;
use egui::Ui;
use egui::{Align, Color32, CornerRadius, Frame, Layout, Margin, Pos2, Rect, Vec2, ViewportId};
use egui_wgpu::{Renderer, ScreenDescriptor};
use image::{RgbaImage, imageops::FilterType};
#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use wgpu::Adapter;
use wgpu::BindGroup;
use wgpu::BindGroupLayout;
use wgpu::CompositeAlphaMode;
use wgpu::Device;
use wgpu::Queue;
use wgpu::RenderPipeline;
use wgpu::Surface;
use wgpu::SurfaceCapabilities;
use wgpu::SurfaceError;
use wgpu::SurfaceTexture;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::KeyEvent;
use winit::{
	dpi::PhysicalSize,
	event::{ElementState, Modifiers, MouseButton, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
	keyboard::{Key, NamedKey},
	window::{CursorIcon, Theme, WindowId, WindowLevel},
};

use crate::{
	state::{GlobalPoint, MonitorRect, OverlayMode, OverlayState, Rgb},
	worker::{OverlayWorker, WorkerResponse},
};

const HUD_PILL_BODY_FILL_DARK_SRGBA8: [u8; 4] = [28, 28, 32, 156];
const HUD_PILL_BODY_FILL_LIGHT_SRGBA8: [u8; 4] = [232, 236, 243, 176];
const HUD_PILL_BLUR_TINT_ALPHA_DARK: f32 = 0.18;
const HUD_PILL_BLUR_TINT_ALPHA_LIGHT: f32 = 0.22;
const LOUPE_TILE_CORNER_RADIUS_POINTS: f64 = 12.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HudAnchor {
	Cursor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
	#[default]
	System,
	Dark,
	Light,
}

#[derive(Debug)]
pub enum OverlayExit {
	Cancelled,
	PngBytes(Vec<u8>),
	Error(String),
}

#[derive(Debug)]
pub enum OverlayControl {
	Continue,
	Exit(OverlayExit),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AltActivationMode {
	#[default]
	Hold,
	Toggle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HudTheme {
	Dark,
	Light,
}

#[derive(Clone, Debug)]
pub struct OverlayConfig {
	pub hud_anchor: HudAnchor,
	pub show_alt_hint_keycap: bool,
	pub show_hud_blur: bool,
	pub hud_opaque: bool,
	/// 0..=1. Controls HUD background alpha.
	pub hud_opacity: f32,
	/// 0..=1. 0 disables the effect.
	pub hud_fog_amount: f32,
	/// 0..=1. 0 disables the effect.
	pub hud_milk_amount: f32,
	pub alt_activation: AltActivationMode,
	pub loupe_sample_side_px: u32,
	pub theme_mode: ThemeMode,
}
impl Default for OverlayConfig {
	fn default() -> Self {
		Self {
			hud_anchor: HudAnchor::Cursor,
			show_alt_hint_keycap: true,
			show_hud_blur: true,
			hud_opaque: false,
			hud_opacity: 0.35,
			hud_fog_amount: 0.16,
			hud_milk_amount: 0.0,
			alt_activation: AltActivationMode::Hold,
			loupe_sample_side_px: 21,
			theme_mode: ThemeMode::System,
		}
	}
}

#[allow(dead_code)]
pub struct OverlayBuilder {
	config: OverlayConfig,
}
#[allow(dead_code)]
impl OverlayBuilder {
	#[must_use]
	pub fn new() -> Self {
		Self { config: OverlayConfig::default() }
	}

	#[must_use]
	pub fn with_config(mut self, config: OverlayConfig) -> Self {
		self.config = config;

		self
	}

	pub fn run(self) -> Result<OverlayExit> {
		struct Runner {
			session: OverlaySession,
			exit: Option<OverlayExit>,
		}

		impl ApplicationHandler<()> for Runner {
			fn resumed(&mut self, event_loop: &ActiveEventLoop) {
				if let Err(err) = self.session.start(event_loop) {
					self.exit = Some(OverlayExit::Error(err));

					event_loop.exit();
				}
			}

			fn window_event(
				&mut self,
				event_loop: &ActiveEventLoop,
				window_id: WindowId,
				event: WindowEvent,
			) {
				match self.session.handle_window_event(window_id, &event) {
					OverlayControl::Continue => {},
					OverlayControl::Exit(exit) => {
						self.exit = Some(exit);

						event_loop.exit();
					},
				}
			}

			fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
				event_loop.set_control_flow(ControlFlow::Wait);

				if let OverlayControl::Exit(exit) = self.session.about_to_wait() {
					self.exit = Some(exit);

					event_loop.exit();
				}
			}
		}

		let event_loop = EventLoop::new()?;
		let mut runner = Runner { session: OverlaySession::with_config(self.config), exit: None };

		event_loop.run_app(&mut runner)?;

		Ok(runner.exit.unwrap_or(OverlayExit::Cancelled))
	}
}

pub struct OverlaySession {
	config: OverlayConfig,
	worker: Option<OverlayWorker>,
	cursor_device: device_query::DeviceState,
	state: OverlayState,
	cursor_monitor: Option<MonitorRect>,
	windows: HashMap<WindowId, OverlayWindow>,
	hud_window: Option<HudOverlayWindow>,
	loupe_window: Option<HudOverlayWindow>,
	hud_outer_pos: Option<GlobalPoint>,
	hud_inner_size_points: Option<(u32, u32)>,
	loupe_outer_pos: Option<GlobalPoint>,
	loupe_inner_size_points: Option<(u32, u32)>,
	gpu: Option<GpuContext>,
	last_present_at: Instant,
	last_rgb_request_at: Instant,
	rgb_request_interval: Duration,
	last_live_bg_request_at: Instant,
	live_bg_request_interval: Duration,
	last_loupe_request_at: Instant,
	loupe_request_interval: Duration,
	last_alt_press_at: Option<Instant>,
	alt_modifier_down: bool,
	loupe_patch_width_px: u32,
	loupe_patch_height_px: u32,
	pending_freeze_capture: Option<MonitorRect>,
	pending_freeze_capture_armed: bool,
	capture_windows_hidden: bool,
	pending_encode_png: Option<RgbaImage>,
}
impl OverlaySession {
	#[must_use]
	pub fn new() -> Self {
		Self::with_config(OverlayConfig::default())
	}

	#[must_use]
	pub fn with_config(config: OverlayConfig) -> Self {
		let live_bg_request_interval = Duration::from_millis(500);
		let loupe_sample_side_px =
			Self::normalized_loupe_sample_side_px(config.loupe_sample_side_px);

		Self {
			config,
			worker: None,
			cursor_device: device_query::DeviceState::new(),
			state: OverlayState::new(),
			cursor_monitor: None,
			windows: HashMap::new(),
			hud_window: None,
			loupe_window: None,
			hud_outer_pos: None,
			hud_inner_size_points: None,
			loupe_outer_pos: None,
			loupe_inner_size_points: None,
			gpu: None,
			last_present_at: Instant::now(),
			last_rgb_request_at: Instant::now(),
			rgb_request_interval: Duration::from_millis(16),
			last_live_bg_request_at: Instant::now() - live_bg_request_interval,
			live_bg_request_interval,
			last_loupe_request_at: Instant::now(),
			loupe_request_interval: Duration::from_millis(33),
			last_alt_press_at: None,
			alt_modifier_down: false,
			loupe_patch_width_px: loupe_sample_side_px,
			loupe_patch_height_px: loupe_sample_side_px,
			pending_freeze_capture: None,
			pending_freeze_capture_armed: false,
			capture_windows_hidden: false,
			pending_encode_png: None,
		}
	}

	pub fn set_config(&mut self, config: OverlayConfig) {
		let prev = self.config.clone();
		let previous_loupe_patch = self.loupe_patch_width_px;
		let loupe_sample_side = Self::normalized_loupe_sample_side_px(config.loupe_sample_side_px);

		self.config = config;
		self.loupe_patch_width_px = loupe_sample_side;
		self.loupe_patch_height_px = loupe_sample_side;

		let patch_changed = self.loupe_patch_width_px != previous_loupe_patch;

		if patch_changed {
			self.state.loupe = None;
		}
		if !self.is_active() {
			return;
		}

		if let Some(hud_window) = self.hud_window.as_ref() {
			hud_window.window.set_transparent(true);

			#[cfg(target_os = "macos")]
			macos_configure_hud_window(
				hud_window.window.as_ref(),
				self.config.show_hud_blur,
				self.config.hud_fog_amount,
				None,
			);

			#[cfg(not(target_os = "macos"))]
			hud_window.window.set_blur(self.config.show_hud_blur);
		}
		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_transparent(true);

			#[cfg(target_os = "macos")]
			macos_configure_hud_window(
				loupe_window.window.as_ref(),
				self.config.show_hud_blur,
				self.config.hud_fog_amount,
				Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
			);

			#[cfg(not(target_os = "macos"))]
			loupe_window.window.set_blur(self.config.show_hud_blur);
		}

		let prev_fake_blur = prev.show_hud_blur && !cfg!(target_os = "macos");
		let new_fake_blur = self.use_fake_hud_blur();

		if prev_fake_blur != new_fake_blur {
			if new_fake_blur {
				self.last_live_bg_request_at = Instant::now() - self.live_bg_request_interval;

				if matches!(self.state.mode, OverlayMode::Live)
					&& let Some(_cursor) = self.state.cursor
					&& let Some(monitor) = self.active_cursor_monitor()
				{
					self.maybe_request_live_bg(monitor);
				}
			} else {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;
			}
		}
		if patch_changed
			&& matches!(self.state.mode, OverlayMode::Live)
			&& self.state.alt_held
			&& let Some(cursor) = self.state.cursor
			&& let Some(worker) = &self.worker
			&& let Some(monitor) = self.active_cursor_monitor()
		{
			worker.try_sample_loupe(
				monitor,
				cursor,
				self.loupe_patch_width_px,
				self.loupe_patch_height_px,
			);

			self.last_loupe_request_at = Instant::now();
		}

		self.request_redraw_all();
	}

	#[must_use]
	pub fn is_active(&self) -> bool {
		!self.windows.is_empty()
	}

	fn use_fake_hud_blur(&self) -> bool {
		self.config.show_hud_blur && !cfg!(target_os = "macos")
	}

	fn normalized_loupe_sample_side_px(side_px: u32) -> u32 {
		let side_px = side_px.max(3);

		if side_px & 1 == 0 { side_px + 1 } else { side_px }
	}

	pub fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		if self.is_active() {
			return Ok(());
		}

		self.reset_for_start();

		self.worker = Some(OverlayWorker::new(crate::backend::default_capture_backend()));

		let monitors =
			xcap::Monitor::all().map_err(|err| format!("xcap Monitor::all failed: {err:?}"))?;

		if monitors.is_empty() {
			return Err(String::from("No monitors detected"));
		}

		self.gpu = Some(GpuContext::new().map_err(|err| format!("{err:#}"))?);

		self.create_overlay_windows(event_loop, &monitors)?;
		self.create_hud_window(event_loop)?;
		self.create_loupe_window(event_loop)?;
		self.request_redraw_all();
		self.initialize_cursor_state();

		Ok(())
	}

	fn reset_for_start(&mut self) {
		self.hud_inner_size_points = None;
		self.hud_outer_pos = None;
		self.loupe_inner_size_points = None;
		self.loupe_outer_pos = None;
		self.cursor_monitor = None;
		self.state = OverlayState::new();
		self.pending_freeze_capture = None;
		self.pending_freeze_capture_armed = false;
	}

	fn create_overlay_windows(
		&mut self,
		event_loop: &ActiveEventLoop,
		monitors: &[xcap::Monitor],
	) -> Result<(), String> {
		for monitor in monitors {
			let monitor_rect = MonitorRect {
				id: monitor.id().map_err(|err| {
					format!(
						"Failed to read xcap monitor id while creating overlay windows: {err:?}"
					)
				})?,
				origin: GlobalPoint::new(
					monitor.x().map_err(|err| {
						format!(
							"Failed to read monitor x position while creating overlay windows: {err:?}"
						)
					})?,
					monitor.y().map_err(|err| {
						format!(
							"Failed to read monitor y position while creating overlay windows: {err:?}"
						)
					})?,
				),
				width: monitor.width().map_err(|err| {
					format!("Failed to read monitor width while creating overlay windows: {err:?}")
				})?,
				height: monitor.height().map_err(|err| {
					format!("Failed to read monitor height while creating overlay windows: {err:?}")
				})?,
				scale_factor_x1000: {
					let scale_factor = monitor.scale_factor().map_err(|err| {
						format!(
							"Failed to read monitor scale factor while creating overlay windows: {err:?}"
						)
					})?;

					(scale_factor * 1_000.0).round() as u32
				},
			};
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

			window.set_cursor(CursorIcon::Crosshair);

			let _ = window.set_cursor_hittest(true);

			window.request_redraw();
			window.focus_window();

			let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
			let renderer = WindowRenderer::new(gpu, Arc::clone(&window))
				.map_err(|err| format!("Failed to init renderer: {err:#}"))?;

			self.windows
				.insert(window.id(), OverlayWindow { monitor: monitor_rect, window, renderer });
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
		let _ = window.set_cursor_hittest(false);

		window.set_transparent(true);

		#[cfg(target_os = "macos")]
		macos_configure_hud_window(
			window.as_ref(),
			self.config.show_hud_blur,
			self.config.hud_fog_amount,
			None,
		);

		#[cfg(not(target_os = "macos"))]
		window.set_blur(self.config.show_hud_blur);
		window.request_redraw();

		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let renderer = WindowRenderer::new(gpu, Arc::clone(&window))
			.map_err(|err| format!("Failed to init HUD renderer: {err:#}"))?;

		self.hud_window = Some(HudOverlayWindow { window, renderer });

		Ok(())
	}

	fn create_loupe_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		let attrs = winit::window::Window::default_attributes()
			.with_title("rsnap-loupe")
			.with_decorations(false)
			.with_resizable(false)
			.with_transparent(true)
			.with_visible(false)
			.with_window_level(WindowLevel::AlwaysOnTop)
			.with_inner_size(LogicalSize::new(260.0, 260.0));
		let window = event_loop
			.create_window(attrs)
			.map_err(|err| format!("Unable to create loupe window: {err}"))?;
		let window = Arc::new(window);
		let _ = window.set_cursor_hittest(false);

		window.set_transparent(true);

		#[cfg(target_os = "macos")]
		macos_configure_hud_window(
			window.as_ref(),
			self.config.show_hud_blur,
			self.config.hud_fog_amount,
			Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
		);

		#[cfg(not(target_os = "macos"))]
		window.set_blur(self.config.show_hud_blur);

		let gpu = self.gpu.as_ref().ok_or_else(|| String::from("Missing GPU context"))?;
		let renderer = WindowRenderer::new(gpu, Arc::clone(&window))
			.map_err(|err| format!("Failed to init loupe renderer: {err:#}"))?;

		self.loupe_window = Some(HudOverlayWindow { window, renderer });

		Ok(())
	}

	pub fn request_redraw_all(&self) {
		for w in self.windows.values() {
			w.window.request_redraw();
		}

		if let Some(hud) = self.hud_window.as_ref() {
			hud.window.request_redraw();
		}
		if let Some(loupe) = self.loupe_window.as_ref() {
			loupe.window.request_redraw();
		}
	}

	pub fn request_redraw_for_monitor(&self, monitor: MonitorRect) {
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
	}

	pub fn about_to_wait(&mut self) -> OverlayControl {
		self.maybe_request_keepalive_redraw();

		if self.is_active() {
			self.sync_alt_held_from_global_keys();
		}

		self.maybe_tick_live_sampling();
		self.maybe_tick_frozen_cursor_tracking();

		self.drain_worker_responses()
	}

	fn maybe_tick_frozen_cursor_tracking(&mut self) {
		if !self.is_active() || !matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}

		let mouse = self.cursor_device.get_mouse();
		let global = GlobalPoint::new(mouse.coords.0, mouse.coords.1);
		let old_monitor = self.active_cursor_monitor();
		let Some(monitor) = self.monitor_at(global) else {
			return;
		};

		if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
			return;
		}

		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		self.request_redraw_for_monitor(monitor);
	}

	fn maybe_request_keepalive_redraw(&mut self) {
		// Avoid a tight present loop if the OS delivers spurious redraws.
		if self.is_active() && self.last_present_at.elapsed() > Duration::from_secs(30) {
			self.request_redraw_all();
		}
	}

	fn maybe_tick_live_sampling(&mut self) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}

		let Some(worker) = &self.worker else {
			return;
		};

		if self.last_rgb_request_at.elapsed() >= self.rgb_request_interval {
			worker.try_sample_rgb(monitor, cursor);

			self.last_rgb_request_at = Instant::now();
		}
		if self.state.alt_held
			&& self.last_loupe_request_at.elapsed() >= self.loupe_request_interval
		{
			worker.try_sample_loupe(
				monitor,
				cursor,
				self.loupe_patch_width_px,
				self.loupe_patch_height_px,
			);

			self.last_loupe_request_at = Instant::now();
		}
	}

	fn drain_worker_responses(&mut self) -> OverlayControl {
		if self.worker.is_none() {
			return OverlayControl::Continue;
		}

		if let Some(image) = self.pending_encode_png.take()
			&& let Some(worker) = self.worker.as_ref()
			&& let Err(image) = worker.request_encode_png(image)
		{
			self.pending_encode_png = Some(image);
		}

		while let Some(resp) = self.worker.as_ref().and_then(|worker| worker.try_recv()) {
			match resp {
				WorkerResponse::SampledLoupe { monitor, point, rgb, patch } => {
					if matches!(self.state.mode, OverlayMode::Live) {
						self.state.rgb = rgb;
						self.state.loupe =
							patch.map(|patch| crate::state::LoupeSample { center: point, patch });

						let current_monitor = self.active_cursor_monitor();

						if let Some(current_monitor) = current_monitor {
							self.request_redraw_for_monitor(current_monitor);
						}

						if current_monitor != Some(monitor) {
							self.request_redraw_for_monitor(monitor);
						}
					}
				},
				WorkerResponse::SampledRgb { monitor, point, rgb } => {
					if matches!(self.state.mode, OverlayMode::Live) {
						let _ = point;

						self.state.rgb = rgb;

						let current_monitor = self.active_cursor_monitor();

						if let Some(current_monitor) = current_monitor {
							self.request_redraw_for_monitor(current_monitor);
						}

						if current_monitor != Some(monitor) {
							self.request_redraw_for_monitor(monitor);
						}
					}
				},
				WorkerResponse::CapturedFreeze { monitor, image } => {
					if matches!(self.state.mode, OverlayMode::Frozen)
						&& self.state.monitor == Some(monitor)
					{
						self.state.finish_freeze(monitor, image);
						self.restore_capture_windows_visibility();

						if let Some(cursor) = self.state.cursor {
							self.state.rgb =
								frozen_rgb(&self.state.frozen_image, Some(monitor), cursor);
							self.state.loupe = frozen_loupe_patch(
								&self.state.frozen_image,
								Some(monitor),
								cursor,
								self.loupe_patch_width_px,
								self.loupe_patch_height_px,
							)
							.map(|patch| crate::state::LoupeSample { center: cursor, patch });
						}

						self.request_redraw_for_monitor(monitor);
						self.raise_hud_windows();
					} else if matches!(self.state.mode, OverlayMode::Live)
						&& self.use_fake_hud_blur()
						&& self.active_cursor_monitor() == Some(monitor)
					{
						self.state.live_bg_monitor = Some(monitor);
						self.state.live_bg_image = Some(image);
						self.state.live_bg_generation =
							self.state.live_bg_generation.wrapping_add(1);

						self.request_redraw_for_monitor(monitor);
					}
				},
				WorkerResponse::Error(message) => {
					self.restore_capture_windows_visibility();
					self.state.set_error(message);
					self.request_redraw_all();
				},
				WorkerResponse::EncodedPng { png_bytes } => {
					match write_png_bytes_to_clipboard(&png_bytes) {
						Ok(()) => return self.exit(OverlayExit::PngBytes(png_bytes)),
						Err(err) => {
							self.state.set_error(format!("{err:#}"));
							self.request_redraw_all();
						},
					}
				},
			}
		}

		OverlayControl::Continue
	}

	pub fn handle_window_event(
		&mut self,
		window_id: WindowId,
		event: &WindowEvent,
	) -> OverlayControl {
		match event {
			WindowEvent::CloseRequested => self.exit(OverlayExit::Cancelled),
			WindowEvent::Resized(size) => self.handle_resized(window_id, *size),
			WindowEvent::ScaleFactorChanged { .. } => self.handle_scale_factor_changed(window_id),
			WindowEvent::CursorMoved { position, .. } => {
				self.handle_cursor_moved(window_id, *position)
			},
			WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
				self.handle_left_mouse_input(window_id, *state)
			},
			WindowEvent::ThemeChanged(_) => {
				// Keep the HUD palette in sync with system changes when ThemeMode::System is active.
				if let Some(monitor) = self.windows.get(&window_id).map(|w| w.monitor) {
					self.request_redraw_for_monitor(monitor);
				} else {
					self.request_redraw_all();
				}

				OverlayControl::Continue
			},
			WindowEvent::KeyboardInput { event, .. } => self.handle_key_event(event),
			WindowEvent::ModifiersChanged(modifiers) => self.handle_modifiers_changed(modifiers),
			WindowEvent::RedrawRequested => self.handle_redraw_requested(window_id),
			_ => OverlayControl::Continue,
		}
	}

	fn handle_modifiers_changed(&mut self, modifiers: &Modifiers) -> OverlayControl {
		let alt = self.resolve_alt_modifier_state(modifiers.state().alt_key());

		match self.config.alt_activation {
			AltActivationMode::Hold => self.set_alt_held(alt),
			AltActivationMode::Toggle => {
				if alt && !self.alt_modifier_down {
					self.set_alt_held(!self.state.alt_held);
				}
			},
		}

		self.alt_modifier_down = alt;

		if let Some(monitor) = self.active_cursor_monitor() {
			self.request_redraw_for_monitor(monitor);
		} else {
			self.request_redraw_all();
		}

		OverlayControl::Continue
	}

	fn resolve_alt_modifier_state(&mut self, alt: bool) -> bool {
		let transient_alt_release = !alt
			&& self.state.alt_held
			&& self
				.last_alt_press_at
				.is_some_and(|press| press.elapsed() <= Duration::from_millis(120))
			&& self.is_option_key_down();

		if transient_alt_release { true } else { alt }
	}

	fn is_option_key_down(&self) -> bool {
		let keys = self.cursor_device.get_keys();

		keys.contains(&Keycode::LOption)
			|| keys.contains(&Keycode::ROption)
			|| keys.contains(&Keycode::LAlt)
			|| keys.contains(&Keycode::RAlt)
	}

	fn sync_alt_held_from_global_keys(&mut self) {
		if matches!(self.config.alt_activation, AltActivationMode::Hold)
			&& self.state.alt_held
			&& !self.is_option_key_down()
		{
			self.set_alt_held(false);
		}
		if !self.is_option_key_down() {
			self.alt_modifier_down = false;
		}
	}

	fn set_alt_held(&mut self, alt: bool) {
		if self.state.alt_held == alt {
			return;
		}

		self.state.alt_held = alt;

		if alt {
			self.last_alt_press_at = Some(Instant::now());

			if !matches!(self.state.mode, OverlayMode::Live) {
				return;
			}

			let Some(cursor) = self.state.cursor else {
				return;
			};
			let Some(monitor) = self.active_cursor_monitor() else {
				return;
			};
			let visible = self.update_loupe_window_position(monitor);

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(visible);
				loupe_window.window.request_redraw();
			}

			if self.use_fake_hud_blur() {
				self.maybe_request_live_bg(monitor);
			}

			if let Some(worker) = &self.worker {
				worker.try_sample_rgb(monitor, cursor);

				self.last_rgb_request_at = Instant::now();

				worker.try_sample_loupe(
					monitor,
					cursor,
					self.loupe_patch_width_px,
					self.loupe_patch_height_px,
				);

				self.last_loupe_request_at = Instant::now();
			}

			return;
		}

		self.last_alt_press_at = None;
		self.state.loupe = None;
		self.loupe_outer_pos = None;

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
			loupe_window.window.request_redraw();
		}
		if let Some(monitor) = self.active_cursor_monitor() {
			self.request_redraw_for_monitor(monitor);
		}
	}

	fn handle_resized(&mut self, window_id: WindowId, size: PhysicalSize<u32>) -> OverlayControl {
		if let Some(hud_window) = self.hud_window.as_mut()
			&& hud_window.window.id() == window_id
		{
			match hud_window.renderer.resize(size) {
				Ok(()) => {
					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						hud_window.window.as_ref(),
						self.config.show_hud_blur,
						self.config.hud_fog_amount,
						None,
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}
		if let Some(loupe_window) = self.loupe_window.as_mut()
			&& loupe_window.window.id() == window_id
		{
			match loupe_window.renderer.resize(size) {
				Ok(()) => {
					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						loupe_window.window.as_ref(),
						self.config.show_hud_blur,
						self.config.hud_fog_amount,
						Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}

		let Some(overlay_window) = self.windows.get_mut(&window_id) else {
			return OverlayControl::Continue;
		};

		match overlay_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn handle_scale_factor_changed(&mut self, window_id: WindowId) -> OverlayControl {
		if let Some(hud_window) = self.hud_window.as_mut()
			&& hud_window.window.id() == window_id
		{
			let size = hud_window.window.inner_size();

			match hud_window.renderer.resize(size) {
				Ok(()) => {
					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						hud_window.window.as_ref(),
						self.config.show_hud_blur,
						self.config.hud_fog_amount,
						None,
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}
		if let Some(loupe_window) = self.loupe_window.as_mut()
			&& loupe_window.window.id() == window_id
		{
			let size = loupe_window.window.inner_size();

			match loupe_window.renderer.resize(size) {
				Ok(()) => {
					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						loupe_window.window.as_ref(),
						self.config.show_hud_blur,
						self.config.hud_fog_amount,
						Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
					);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}

		let Some(overlay_window) = self.windows.get_mut(&window_id) else {
			return OverlayControl::Continue;
		};
		let size = overlay_window.window.inner_size();

		match overlay_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn handle_cursor_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
	) -> OverlayControl {
		let old_monitor = self.active_cursor_monitor();
		let Some((window_monitor, scale_factor)) =
			self.windows.get(&window_id).map(|w| (w.monitor, w.window.scale_factor()))
		else {
			return OverlayControl::Continue;
		};
		// Prefer the OS/global cursor coordinates for cross-monitor correctness. Window-local cursor
		// events can be in a different coordinate space (logical vs physical), especially across
		// monitors with different scale factors.
		let mouse = self.cursor_device.get_mouse();
		let global_os = GlobalPoint::new(mouse.coords.0, mouse.coords.1);
		let (monitor, global) = if let Some(monitor) = self.monitor_at(global_os) {
			(monitor, global_os)
		} else {
			let local_x = (position.x / scale_factor).round() as i32;
			let local_y = (position.y / scale_factor).round() as i32;
			let global = GlobalPoint::new(
				window_monitor.origin.x + local_x,
				window_monitor.origin.y + local_y,
			);

			(window_monitor, global)
		};

		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);

		if matches!(self.state.mode, OverlayMode::Live) && self.use_fake_hud_blur() {
			if self.state.live_bg_monitor != Some(monitor) {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;
			}

			self.maybe_request_live_bg(monitor);
		}
		if matches!(self.state.mode, OverlayMode::Live)
			&& self.last_rgb_request_at.elapsed() >= self.rgb_request_interval
			&& let Some(worker) = &self.worker
		{
			worker.try_sample_rgb(monitor, global);

			self.last_rgb_request_at = Instant::now();
		}
		if matches!(self.state.mode, OverlayMode::Live)
			&& self.state.alt_held
			&& self.last_loupe_request_at.elapsed() >= self.loupe_request_interval
			&& let Some(worker) = &self.worker
		{
			worker.try_sample_loupe(
				monitor,
				global,
				self.loupe_patch_width_px,
				self.loupe_patch_height_px,
			);

			self.last_loupe_request_at = Instant::now();
		}

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		self.request_redraw_for_monitor(monitor);

		OverlayControl::Continue
	}

	fn handle_left_mouse_input(
		&mut self,
		window_id: WindowId,
		state: ElementState,
	) -> OverlayControl {
		if state != ElementState::Pressed || !matches!(self.state.mode, OverlayMode::Live) {
			return OverlayControl::Continue;
		}

		let Some(monitor) = self.windows.get(&window_id).map(|w| w.monitor) else {
			return OverlayControl::Continue;
		};
		let frozen_rgb = self.state.rgb;
		let frozen_loupe = self.state.loupe.as_ref().map(|loupe| crate::state::LoupeSample {
			center: loupe.center,
			patch: loupe.patch.clone(),
		});

		self.state.clear_error();
		self.state.begin_freeze(monitor);

		self.state.rgb = frozen_rgb;
		self.state.loupe = frozen_loupe;
		self.pending_freeze_capture = Some(monitor);
		self.pending_freeze_capture_armed = false;
		self.capture_windows_hidden = false;

		if self.use_fake_hud_blur()
			&& self.state.live_bg_monitor == Some(monitor)
			&& let Some(image) = self.state.live_bg_image.take()
		{
			self.state.live_bg_monitor = None;

			self.state.finish_freeze(monitor, image);

			self.pending_freeze_capture = None;
			self.pending_freeze_capture_armed = false;

			if let Some(cursor) = self.state.cursor {
				self.update_cursor_state(monitor, cursor);
			}
		} else {
			self.state.live_bg_monitor = None;
			self.state.live_bg_image = None;
			#[cfg(not(target_os = "macos"))]
			{
				self.capture_windows_hidden = true;

				self.hide_capture_windows();
			}
		}

		self.request_redraw_for_monitor(monitor);

		OverlayControl::Continue
	}

	fn handle_key_event(&mut self, event: &KeyEvent) -> OverlayControl {
		if event.state != ElementState::Pressed {
			return OverlayControl::Continue;
		}
		if event.repeat {
			return OverlayControl::Continue;
		}

		match event.logical_key {
			Key::Named(NamedKey::Escape) => self.exit(OverlayExit::Cancelled),
			Key::Named(NamedKey::Tab) => {
				let Some(rgb) = self.state.rgb else {
					return OverlayControl::Continue;
				};
				let hex = rgb.hex_upper();

				match write_text_to_clipboard(&hex) {
					Ok(()) => {},
					Err(err) => {
						self.state.set_error(format!("{err:#}"));
						self.request_redraw_all();
					},
				}

				OverlayControl::Continue
			},
			Key::Named(NamedKey::Space) => {
				if matches!(self.state.mode, OverlayMode::Frozen)
					&& self.state.frozen_image.is_some()
				{
					self.state.set_error("Copying...");

					self.pending_encode_png = self.state.frozen_image.take();

					self.request_redraw_all();
				}

				OverlayControl::Continue
			},
			_ => OverlayControl::Continue,
		}
	}

	fn handle_redraw_requested(&mut self, window_id: WindowId) -> OverlayControl {
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

		self.handle_overlay_window_redraw(window_id)
	}

	fn handle_hud_redraw_requested(&mut self) -> OverlayControl {
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};

		if self.capture_windows_hidden {
			#[cfg(not(target_os = "macos"))]
			if let Some(hud_window) = self.hud_window.as_ref() {
				hud_window.window.set_visible(false);
			}

			self.last_present_at = Instant::now();

			#[cfg(not(target_os = "macos"))]
			return OverlayControl::Continue;
		}

		let monitor =
			self.monitor_for_mode().or_else(|| self.windows.values().next().map(|w| w.monitor));

		if let (Some(monitor), Some(hud_window)) = (monitor, self.hud_window.as_mut()) {
			#[cfg(not(target_os = "macos"))]
			hud_window.window.set_visible(true);

			if let Err(err) = hud_window.renderer.draw(
				gpu,
				&self.state,
				monitor,
				true,
				Some(Pos2::new(-14.0, -14.0)),
				true,
				HudAnchor::Cursor,
				self.config.show_alt_hint_keycap,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.theme_mode,
			) {
				return self.exit(OverlayExit::Error(format!("{err:#}")));
			}
			if let Some(hud_pill) = hud_window.renderer.hud_pill {
				let desired_w = hud_pill.rect.width().ceil().max(1.0) as u32;
				let desired_h = hud_pill.rect.height().ceil().max(1.0) as u32;
				let desired = (desired_w, desired_h);

				if self.hud_inner_size_points != Some(desired) {
					self.hud_inner_size_points = Some(desired);

					let _ = hud_window.window.request_inner_size(LogicalSize::new(
						f64::from(desired_w),
						f64::from(desired_h),
					));

					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						hud_window.window.as_ref(),
						self.config.show_hud_blur,
						self.config.hud_fog_amount,
						None,
					);

					if let Some(cursor) = self.state.cursor {
						self.update_hud_window_position(monitor, cursor);
					}
				}
			}
		}

		self.last_present_at = Instant::now();

		OverlayControl::Continue
	}

	fn handle_loupe_redraw_requested(&mut self) -> OverlayControl {
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};

		if self.capture_windows_hidden {
			#[cfg(not(target_os = "macos"))]
			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(false);
			}

			self.last_present_at = Instant::now();

			#[cfg(not(target_os = "macos"))]
			return OverlayControl::Continue;
		}
		if !self.state.alt_held {
			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(false);
			}

			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		}

		let monitor =
			self.monitor_for_mode().or_else(|| self.windows.values().next().map(|w| w.monitor));
		let Some(monitor) = monitor else {
			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		};
		let Some(cursor) = self.state.cursor else {
			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		};
		let mut needs_reposition = false;

		if let Some(loupe_window) = self.loupe_window.as_mut() {
			#[cfg(not(target_os = "macos"))]
			loupe_window.window.set_visible(true);

			if let Err(err) = loupe_window.renderer.draw_loupe_tile_window(
				gpu,
				&self.state,
				monitor,
				cursor,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_milk_amount,
				self.config.theme_mode,
			) {
				return self.exit(OverlayExit::Error(format!("{err:#}")));
			}
			if let Some(tile_rect) = loupe_window.renderer.loupe_tile {
				let desired_w = tile_rect.max.x.ceil().max(1.0) as u32;
				let desired_h = tile_rect.max.y.ceil().max(1.0) as u32;
				let desired = (desired_w, desired_h);

				if self.loupe_inner_size_points != Some(desired) {
					self.loupe_inner_size_points = Some(desired);

					let _ = loupe_window.window.request_inner_size(LogicalSize::new(
						f64::from(desired_w),
						f64::from(desired_h),
					));

					#[cfg(target_os = "macos")]
					macos_configure_hud_window(
						loupe_window.window.as_ref(),
						self.config.show_hud_blur,
						self.config.hud_fog_amount,
						Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
					);

					needs_reposition = true;
				}
			}
		}

		if needs_reposition {
			let _ = self.update_loupe_window_position(monitor);
		}

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(true);
		}

		self.last_present_at = Instant::now();

		OverlayControl::Continue
	}

	fn handle_overlay_window_redraw(&mut self, window_id: WindowId) -> OverlayControl {
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};
		let overlay_monitor = {
			let Some(overlay_window) = self.windows.get_mut(&window_id) else {
				return OverlayControl::Continue;
			};
			let overlay_monitor = overlay_window.monitor;

			if let Err(err) = overlay_window.renderer.draw(
				gpu,
				&self.state,
				overlay_monitor,
				false,
				None,
				false,
				self.config.hud_anchor,
				self.config.show_alt_hint_keycap,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.theme_mode,
			) {
				return self.exit(OverlayExit::Error(format!("{err:#}")));
			}

			self.last_present_at = Instant::now();

			overlay_monitor
		};

		if self.pending_freeze_capture == Some(overlay_monitor)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor == Some(overlay_monitor)
			&& self.state.frozen_image.is_none()
			&& let Some(worker) = &self.worker
		{
			#[cfg(target_os = "macos")]
			{
				if worker.request_freeze_capture(overlay_monitor) {
					self.pending_freeze_capture = None;
					self.pending_freeze_capture_armed = false;
				} else {
					self.request_redraw_for_monitor(overlay_monitor);
				}
			}
			#[cfg(not(target_os = "macos"))]
			{
				// Capture must happen on a post-hide redraw so the HUD/loupe are not included.
				if self.pending_freeze_capture_armed {
					if worker.request_freeze_capture(overlay_monitor) {
						self.pending_freeze_capture = None;
						self.pending_freeze_capture_armed = false;
					} else {
						self.request_redraw_for_monitor(overlay_monitor);
					}
				} else {
					self.pending_freeze_capture_armed = true;

					self.hide_capture_windows();
					self.request_redraw_for_monitor(overlay_monitor);
				}
			}
		}

		OverlayControl::Continue
	}

	fn exit(&mut self, exit: OverlayExit) -> OverlayControl {
		self.windows.clear();

		self.hud_window = None;
		self.hud_inner_size_points = None;
		self.hud_outer_pos = None;
		self.loupe_window = None;
		self.loupe_inner_size_points = None;
		self.loupe_outer_pos = None;
		self.cursor_monitor = None;
		self.gpu = None;
		self.worker = None;

		OverlayControl::Exit(exit)
	}

	fn initialize_cursor_state(&mut self) {
		let mouse = self.cursor_device.get_mouse();
		let cursor = GlobalPoint::new(mouse.coords.0, mouse.coords.1);
		let Some(monitor) = self.monitor_at(cursor) else {
			self.state.cursor = Some(cursor);
			self.state.rgb = None;
			self.cursor_monitor = None;

			return;
		};

		self.update_cursor_state(monitor, cursor);
		self.update_hud_window_position(monitor, cursor);

		if matches!(self.state.mode, OverlayMode::Live) {
			if self.use_fake_hud_blur() {
				self.maybe_request_live_bg(monitor);
			}

			if let Some(worker) = &self.worker {
				worker.try_sample_rgb(monitor, cursor);

				self.last_rgb_request_at = Instant::now();
			}
		}
	}

	fn maybe_request_live_bg(&mut self, monitor: MonitorRect) {
		if !matches!(self.state.mode, OverlayMode::Live) || !self.use_fake_hud_blur() {
			return;
		}
		if self.state.live_bg_monitor == Some(monitor) && self.state.live_bg_image.is_some() {
			return;
		}

		let force = self.state.alt_held && self.state.live_bg_image.is_none();

		if !force && self.last_live_bg_request_at.elapsed() < self.live_bg_request_interval {
			return;
		}

		let Some(worker) = &self.worker else {
			return;
		};

		if worker.request_freeze_capture(monitor) {
			self.last_live_bg_request_at = Instant::now();
		}
	}

	fn monitor_at(&self, cursor: GlobalPoint) -> Option<MonitorRect> {
		self.windows
			.values()
			.find(|window| window.monitor.contains(cursor))
			.map(|window| window.monitor)
	}

	fn active_cursor_monitor(&self) -> Option<MonitorRect> {
		self.cursor_monitor.or_else(|| self.state.cursor.and_then(|cursor| self.monitor_at(cursor)))
	}

	fn monitor_for_mode(&self) -> Option<MonitorRect> {
		match self.state.mode {
			OverlayMode::Frozen => self.active_cursor_monitor().or(self.state.monitor),
			OverlayMode::Live => self.active_cursor_monitor(),
		}
	}

	fn update_hud_window_position(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		let Some(hud_window) = self.hud_window.as_ref() else {
			return;
		};
		let scale = hud_window.window.scale_factor().max(1.0);
		let size = hud_window.window.inner_size();
		let hud_w_points = ((size.width as f64) / scale).ceil().max(1.0) as i32;
		let hud_h_points = ((size.height as f64) / scale).ceil().max(1.0) as i32;
		let monitor_right = monitor.origin.x.saturating_add_unsigned(monitor.width);
		let monitor_bottom = monitor.origin.y.saturating_add_unsigned(monitor.height);
		let offset_x = 18;
		let offset_y = 18;
		let mut x = cursor.x.saturating_add(offset_x);
		let mut y = cursor.y.saturating_add(offset_y);

		if x.saturating_add(hud_w_points) > monitor_right {
			x = cursor.x.saturating_sub(offset_x.saturating_add(hud_w_points));
		}
		if y.saturating_add(hud_h_points) > monitor_bottom {
			y = cursor.y.saturating_sub(offset_y.saturating_add(hud_h_points));
		}

		x = x.clamp(
			monitor.origin.x,
			monitor_right.saturating_sub(hud_w_points).max(monitor.origin.x),
		);
		y = y.clamp(
			monitor.origin.y,
			monitor_bottom.saturating_sub(hud_h_points).max(monitor.origin.y),
		);

		let desired = GlobalPoint::new(x, y);

		if self.hud_outer_pos == Some(desired) {
			return;
		}

		self.hud_outer_pos = Some(desired);

		hud_window.window.set_outer_position(LogicalPosition::new(x as f64, y as f64));
		hud_window.window.request_redraw();

		if self.state.alt_held {
			let visible = self.update_loupe_window_position(monitor);

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(visible);
				loupe_window.window.request_redraw();
			}
		}
	}

	fn update_loupe_window_position(&mut self, monitor: MonitorRect) -> bool {
		if !self.state.alt_held {
			return false;
		}

		let Some(loupe_window) = self.loupe_window.as_ref() else {
			return false;
		};
		let Some(hud_window) = self.hud_window.as_ref() else {
			return false;
		};
		let Some(hud_outer) = self.hud_outer_pos else {
			return false;
		};
		let hud_scale = hud_window.window.scale_factor().max(1.0);
		let hud_size = hud_window.window.inner_size();
		let hud_h_points = ((hud_size.height as f64) / hud_scale).ceil().max(1.0) as i32;
		let loupe_scale = loupe_window.window.scale_factor().max(1.0);
		let loupe_size = loupe_window.window.inner_size();
		let loupe_w_points = ((loupe_size.width as f64) / loupe_scale).ceil().max(1.0) as i32;
		let loupe_h_points = ((loupe_size.height as f64) / loupe_scale).ceil().max(1.0) as i32;
		let gap = 10;
		let monitor_right = monitor.origin.x.saturating_add_unsigned(monitor.width);
		let monitor_bottom = monitor.origin.y.saturating_add_unsigned(monitor.height);
		let below_y = hud_outer.y.saturating_add(hud_h_points + gap);
		let above_y = hud_outer.y.saturating_sub(gap.saturating_add(loupe_h_points));
		let max_x = monitor_right.saturating_sub(loupe_w_points).max(monitor.origin.x);
		let max_y = monitor_bottom.saturating_sub(loupe_h_points).max(monitor.origin.y);
		let mut x = hud_outer.x;
		let mut y = if below_y.saturating_add(loupe_h_points) <= monitor_bottom {
			below_y
		} else {
			above_y
		};

		x = x.clamp(monitor.origin.x, max_x);
		y = y.clamp(monitor.origin.y, max_y);

		let desired = GlobalPoint::new(x, y);

		if self.loupe_outer_pos == Some(desired) {
			return true;
		}

		self.loupe_outer_pos = Some(desired);

		loupe_window.window.set_outer_position(LogicalPosition::new(x as f64, y as f64));
		loupe_window.window.request_redraw();

		true
	}

	fn update_cursor_state(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		self.cursor_monitor = Some(monitor);
		self.state.cursor = Some(cursor);

		match self.state.mode {
			OverlayMode::Live => {},
			OverlayMode::Frozen => {
				if self.state.frozen_image.is_none() {
					return;
				}

				let frozen_monitor = self.state.monitor;

				self.state.rgb = frozen_rgb(&self.state.frozen_image, frozen_monitor, cursor);
				self.state.loupe = if self.state.alt_held {
					frozen_loupe_patch(
						&self.state.frozen_image,
						frozen_monitor,
						cursor,
						self.loupe_patch_width_px,
						self.loupe_patch_height_px,
					)
					.map(|patch| crate::state::LoupeSample { center: cursor, patch })
				} else {
					None
				};
			},
		}
	}

	#[cfg(not(target_os = "macos"))]
	fn hide_capture_windows(&mut self) {
		self.capture_windows_hidden = true;

		if let Some(hud_window) = &self.hud_window {
			hud_window.window.set_visible(false);
		}
		if let Some(loupe_window) = &self.loupe_window {
			loupe_window.window.set_visible(false);
		}
	}

	fn restore_capture_windows_visibility(&mut self) {
		if !self.capture_windows_hidden {
			return;
		}

		self.capture_windows_hidden = false;

		#[cfg(not(target_os = "macos"))]
		if let Some(hud_window) = &self.hud_window {
			hud_window.window.set_visible(true);
		}
		#[cfg(not(target_os = "macos"))]
		if let Some(loupe_window) = &self.loupe_window {
			loupe_window.window.set_visible(self.state.alt_held);
		}
	}

	fn raise_hud_windows(&self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			#[cfg(target_os = "macos")]
			{
				macos_order_front_regardless(hud_window.window.as_ref());
			}
			#[cfg(not(target_os = "macos"))]
			{
				hud_window.window.focus_window();
			}
		}

		if self.state.alt_held
			&& let Some(loupe_window) = self.loupe_window.as_ref()
		{
			#[cfg(target_os = "macos")]
			{
				macos_order_front_regardless(loupe_window.window.as_ref());
			}
			#[cfg(not(target_os = "macos"))]
			{
				loupe_window.window.focus_window();
			}
		}
	}
}

impl Default for OverlaySession {
	fn default() -> Self {
		Self::new()
	}
}

struct HudOverlayWindow {
	window: Arc<winit::window::Window>,
	renderer: WindowRenderer,
}

struct OverlayWindow {
	monitor: MonitorRect,
	window: Arc<winit::window::Window>,
	renderer: WindowRenderer,
}

struct GpuContext {
	instance: wgpu::Instance,
	adapter: Adapter,
	device: Device,
	queue: Queue,
}
impl GpuContext {
	fn new() -> Result<Self> {
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
		let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::LowPower,
			compatible_surface: None,
			force_fallback_adapter: false,
		}))
		.map_err(|err| eyre::eyre!("Failed to request GPU adapter: {err}"))?;
		let adapter_limits = adapter.limits();
		let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("rsnap-overlay device"),
			required_features: wgpu::Features::empty(),
			// Use the adapter's actual limits. Using `downlevel_defaults()` caps max texture
			// size to 2048, which breaks on common HiDPI displays.
			required_limits: adapter_limits,
			experimental_features: wgpu::ExperimentalFeatures::default(),
			memory_hints: wgpu::MemoryHints::Performance,
			trace: wgpu::Trace::Off,
		}))
		.wrap_err("Failed to create wgpu device")?;

		Ok(Self { instance, adapter, device, queue })
	}
}

struct WindowRenderer {
	window: Arc<winit::window::Window>,
	surface: Surface<'static>,
	surface_config: wgpu::SurfaceConfiguration,
	needs_reconfigure: bool,
	egui_ctx: egui::Context,
	egui_renderer: Renderer,
	bg_sampler: wgpu::Sampler,
	mipgen_pipeline: RenderPipeline,
	mipgen_surface_pipeline: RenderPipeline,
	mipgen_bind_group_layout: BindGroupLayout,
	hud_blur_pipeline: RenderPipeline,
	hud_blur_bind_group_layout: BindGroupLayout,
	hud_blur_uniform: wgpu::Buffer,
	hud_bg: Option<HudBg>,
	hud_bg_generation: u64,
	hud_pill: Option<HudPillGeometry>,
	loupe_tile: Option<Rect>,
	hud_theme: Option<HudTheme>,
}
impl WindowRenderer {
	fn mip_level_count(width: u32, height: u32) -> u32 {
		let max_dim = width.max(height).max(1);

		(32_u32.saturating_sub(max_dim.leading_zeros())).max(1)
	}

	fn create_mipgen_pipeline(
		gpu: &GpuContext,
		format: wgpu::TextureFormat,
	) -> (RenderPipeline, BindGroupLayout) {
		let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rsnap-mipgen shader"),
			source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
				"mipgen.wgsl"
			))),
		});
		let bind_group_layout =
			gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("rsnap-mipgen bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Texture {
							multisampled: false,
							view_dimension: wgpu::TextureViewDimension::D2,
							sample_type: wgpu::TextureSampleType::Float { filterable: true },
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
						count: None,
					},
				],
			});
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen pipeline layout"),
			bind_group_layouts: &[&bind_group_layout],
			push_constant_ranges: &[],
		});
		let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-mipgen pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: wgpu::FrontFace::Ccw,
				cull_mode: None,
				polygon_mode: wgpu::PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			multiview: None,
			cache: None,
		});

		(pipeline, bind_group_layout)
	}

	fn create_mipgen_surface_pipeline(
		gpu: &GpuContext,
		format: wgpu::TextureFormat,
		bind_group_layout: &BindGroupLayout,
	) -> RenderPipeline {
		let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rsnap-mipgen fullscreen shader"),
			source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
				"mipgen.wgsl"
			))),
		});
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline layout"),
			bind_group_layouts: &[bind_group_layout],
			push_constant_ranges: &[],
		});
		let surface_fragment_entry =
			if cfg!(target_os = "macos") { "fs_main_macos_surface" } else { "fs_main" };

		gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: wgpu::FrontFace::Ccw,
				cull_mode: None,
				polygon_mode: wgpu::PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some(surface_fragment_entry),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			multiview: None,
			cache: None,
		})
	}

	fn generate_mipmaps(&self, gpu: &GpuContext, texture: &wgpu::Texture, mip_level_count: u32) {
		if mip_level_count <= 1 {
			return;
		}

		let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("rsnap-mipgen encoder"),
		});

		for level in 1..mip_level_count {
			let src_view = texture.create_view(&wgpu::TextureViewDescriptor {
				label: Some("rsnap-mipgen src view"),
				format: None,
				dimension: None,
				usage: None,
				aspect: wgpu::TextureAspect::All,
				base_mip_level: level - 1,
				mip_level_count: Some(1),
				base_array_layer: 0,
				array_layer_count: Some(1),
			});
			let dst_view = texture.create_view(&wgpu::TextureViewDescriptor {
				label: Some("rsnap-mipgen dst view"),
				format: None,
				dimension: None,
				usage: None,
				aspect: wgpu::TextureAspect::All,
				base_mip_level: level,
				mip_level_count: Some(1),
				base_array_layer: 0,
				array_layer_count: Some(1),
			});
			let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
				label: Some("rsnap-mipgen bind group"),
				layout: &self.mipgen_bind_group_layout,
				entries: &[
					wgpu::BindGroupEntry {
						binding: 0,
						resource: wgpu::BindingResource::TextureView(&src_view),
					},
					wgpu::BindGroupEntry {
						binding: 1,
						resource: wgpu::BindingResource::Sampler(&self.bg_sampler),
					},
				],
			});
			let rpass_desc = wgpu::RenderPassDescriptor {
				label: Some("rsnap-mipgen pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &dst_view,
					depth_slice: None,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
			};
			let mut rpass = encoder.begin_render_pass(&rpass_desc).forget_lifetime();

			rpass.set_pipeline(&self.mipgen_pipeline);
			rpass.set_bind_group(0, &bind_group, &[]);
			rpass.draw(0..3, 0..1);
		}

		gpu.queue.submit(Some(encoder.finish()));
	}
	fn pick_surface_format(caps: &SurfaceCapabilities) -> wgpu::TextureFormat {
		caps.formats
			.iter()
			.copied()
			.find(|f| {
				matches!(
					f,
					wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb
				)
			})
			.or_else(|| caps.formats.iter().copied().find(wgpu::TextureFormat::is_srgb))
			.unwrap_or(caps.formats[0])
	}

	fn pick_surface_alpha(caps: &SurfaceCapabilities) -> CompositeAlphaMode {
		caps.alpha_modes
			.iter()
			.copied()
			.find(|m| matches!(m, wgpu::CompositeAlphaMode::PreMultiplied))
			.or_else(|| {
				caps.alpha_modes
					.iter()
					.copied()
					.find(|m| matches!(m, wgpu::CompositeAlphaMode::PostMultiplied))
			})
			.or_else(|| {
				caps.alpha_modes
					.iter()
					.copied()
					.find(|m| !matches!(m, wgpu::CompositeAlphaMode::Opaque))
			})
			.unwrap_or(caps.alpha_modes[0])
	}

	fn make_surface_config(
		window: &winit::window::Window,
		format: wgpu::TextureFormat,
		alpha_mode: CompositeAlphaMode,
	) -> wgpu::SurfaceConfiguration {
		let size = window.inner_size();

		wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format,
			width: size.width.max(1),
			height: size.height.max(1),
			present_mode: wgpu::PresentMode::Fifo,
			alpha_mode,
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		}
	}

	fn create_bg_sampler(gpu: &GpuContext) -> wgpu::Sampler {
		gpu.device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("rsnap-frozen-bg sampler"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			address_mode_w: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			mipmap_filter: wgpu::FilterMode::Linear,
			..Default::default()
		})
	}

	fn create_hud_blur_pipeline(
		gpu: &GpuContext,
		surface_format: wgpu::TextureFormat,
	) -> (RenderPipeline, BindGroupLayout) {
		let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rsnap-hud-blur shader"),
			source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
				"hud_blur.wgsl"
			))),
		});
		let bind_group_layout =
			gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("rsnap-hud-blur bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Texture {
							multisampled: false,
							view_dimension: wgpu::TextureViewDimension::D2,
							sample_type: wgpu::TextureSampleType::Float { filterable: true },
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 2,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Uniform,
							has_dynamic_offset: false,
							min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<
								HudBlurUniformRaw,
							>() as u64),
						},
						count: None,
					},
				],
			});
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-hud-blur pipeline layout"),
			bind_group_layouts: &[&bind_group_layout],
			push_constant_ranges: &[],
		});
		let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-hud-blur pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: wgpu::FrontFace::Ccw,
				cull_mode: None,
				polygon_mode: wgpu::PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format: surface_format,
					blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			multiview: None,
			cache: None,
		});

		(pipeline, bind_group_layout)
	}

	fn apply_pending_reconfigure(&mut self, gpu: &GpuContext) {
		if self.needs_reconfigure {
			self.reconfigure(gpu);

			self.needs_reconfigure = false;
		}
	}

	fn prepare_egui_input(&mut self, gpu: &GpuContext) -> (PhysicalSize<u32>, f32, egui::RawInput) {
		let size = PhysicalSize::new(self.surface_config.width, self.surface_config.height);
		let pixels_per_point = self.window.scale_factor() as f32;
		let screen_size_points =
			Vec2::new(size.width as f32 / pixels_per_point, size.height as f32 / pixels_per_point);
		let max_texture_side = gpu.device.limits().max_texture_dimension_2d as usize;

		self.egui_ctx.input_mut(|i| i.max_texture_side = max_texture_side);

		let raw_input = egui::RawInput {
			screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen_size_points)),
			focused: true,
			..Default::default()
		};
		let mut raw_input = raw_input;

		raw_input.max_texture_side = Some(max_texture_side);

		if let Some(viewport) = raw_input.viewports.get_mut(&ViewportId::ROOT) {
			viewport.native_pixels_per_point = Some(pixels_per_point);
			viewport.inner_rect = raw_input.screen_rect;
			viewport.focused = Some(true);
		}

		(size, pixels_per_point, raw_input)
	}

	#[allow(clippy::too_many_arguments)]
	fn run_egui(
		&mut self,
		raw_input: egui::RawInput,
		state: &OverlayState,
		monitor: MonitorRect,
		can_draw_hud: bool,
		hud_local_cursor_override: Option<Pos2>,
		hud_compact: bool,
		show_hud_blur: bool,
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		theme: HudTheme,
	) -> (FullOutput, Option<HudPillGeometry>) {
		let hud_data = if can_draw_hud {
			state.cursor.and_then(|cursor| {
				let local_cursor =
					hud_local_cursor_override.or_else(|| global_to_local(cursor, monitor))?;

				Some((cursor, local_cursor))
			})
		} else {
			None
		};
		let mut hud_pill = None;
		let full_output = self.egui_ctx.run(raw_input, |ctx| {
			if let Some((cursor, local_cursor)) = hud_data {
				let _ = show_hud_blur;

				Self::render_hud(
					ctx,
					state,
					monitor,
					cursor,
					local_cursor,
					hud_compact,
					hud_anchor,
					show_alt_hint_keycap,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					theme,
					&mut hud_pill,
				);
			}
		});

		(full_output, hud_pill)
	}

	fn should_draw_hud(state: &OverlayState, monitor: MonitorRect) -> bool {
		if cfg!(target_os = "macos") && matches!(state.mode, OverlayMode::Frozen) {
			return true;
		}

		!matches!(state.mode, OverlayMode::Frozen)
			|| state.monitor != Some(monitor)
			|| state.frozen_image.is_some()
			|| state.error_message.is_some()
	}

	#[allow(clippy::too_many_arguments)]
	fn render_hud(
		ctx: &egui::Context,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		local_cursor: Pos2,
		hud_compact: bool,
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		theme: HudTheme,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let (hud_x, hud_y) = match hud_anchor {
			HudAnchor::Cursor => (local_cursor.x + 14.0, local_cursor.y + 14.0),
		};

		egui::Area::new("hud".into())
			.order(egui::Order::Foreground)
			.fixed_pos(Pos2::new(hud_x, hud_y))
			.show(ctx, |ui| {
				Self::render_hud_frame(
					ui,
					state,
					monitor,
					cursor,
					hud_compact,
					show_alt_hint_keycap,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					theme,
					hud_pill_out,
				);
			});
	}

	#[allow(clippy::too_many_arguments)]
	fn render_hud_frame(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		hud_compact: bool,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		theme: HudTheme,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let pill_radius = 18_u8;
		let opacity = if hud_opaque { 1.0 } else { hud_opacity.clamp(0.0, 1.0) };
		let tint = hud_milk_amount.clamp(0.0, 1.0);
		let tint_strength = match theme {
			HudTheme::Dark => 0.22,
			HudTheme::Light => 0.28,
		};
		let mix = (tint * tint_strength).clamp(0.0, 1.0);
		let mut fill = hud_body_fill_srgba8(theme, false);

		fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
			((f32::from(a) + ((f32::from(b) - f32::from(a)) * t)).round().clamp(0.0, 255.0)) as u8
		}

		fill[0] = lerp_u8(fill[0], 255, mix);
		fill[1] = lerp_u8(fill[1], 255, mix);
		fill[2] = lerp_u8(fill[2], 255, mix);
		fill[3] = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;

		let body_fill = Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3]);
		let outer_stroke_color = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
		};
		let outer_stroke = egui::Stroke::new(1.0, outer_stroke_color);
		let pill_shadow = if hud_compact {
			egui::epaint::Shadow::NONE
		} else {
			egui::epaint::Shadow {
				offset: [0, 0],
				blur: 10,
				spread: 0,
				color: match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 28),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 18),
				},
			}
		};
		let inner = Frame {
			fill: body_fill,
			stroke: outer_stroke,
			shadow: pill_shadow,
			corner_radius: CornerRadius::same(pill_radius),
			inner_margin: Margin::symmetric(12, 8),
			..Frame::default()
		}
		.show(ui, |ui| {
			ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);

			if let Some(err) = &state.error_message {
				let err_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(235, 235, 245, 235),
					HudTheme::Light => Color32::from_rgba_unmultiplied(28, 28, 32, 235),
				};

				ui.label(egui::RichText::new(err).color(err_color).monospace());
			} else {
				Self::render_hud_content(ui, state, monitor, cursor, show_alt_hint_keycap, theme);
			}
		});
		let pill_rect = inner.response.rect;

		*hud_pill_out =
			Some(HudPillGeometry { rect: pill_rect, radius_points: f32::from(pill_radius) });

		if hud_compact {
			return;
		}

		let inner_stroke_color = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
			HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
		};
		let inner_stroke = egui::Stroke::new(1.0, inner_stroke_color);
		let inner_rect = pill_rect.shrink(1.0);

		ui.painter().rect_stroke(
			inner_rect,
			CornerRadius::same(pill_radius.saturating_sub(1)),
			inner_stroke,
			egui::StrokeKind::Inside,
		);

		if !hud_compact {
			Self::render_loupe_tile(
				ui,
				state,
				monitor,
				cursor,
				pill_rect,
				hud_blur_active,
				hud_opaque,
				theme,
			);
		}
	}

	fn render_hud_content(
		ui: &mut Ui,
		state: &OverlayState,
		_monitor: MonitorRect,
		cursor: GlobalPoint,
		show_alt_hint_keycap: bool,
		theme: HudTheme,
	) {
		let (label_color, secondary_color) = match theme {
			HudTheme::Dark => (
				Color32::from_rgba_unmultiplied(235, 235, 245, 235),
				Color32::from_rgba_unmultiplied(235, 235, 245, 150),
			),
			HudTheme::Light => (
				Color32::from_rgba_unmultiplied(28, 28, 32, 235),
				Color32::from_rgba_unmultiplied(28, 28, 32, 160),
			),
		};
		let pos_text = format!("x={}, y={}", cursor.x, cursor.y);
		let (hex_text, rgb_text) = match state.rgb {
			Some(rgb) => (rgb.hex_upper(), format!("RGB({}, {}, {})", rgb.r, rgb.g, rgb.b)),
			None => (String::from("?"), String::from("RGB(?)")),
		};
		let swatch_size = egui::vec2(10.0, 10.0);

		ui.vertical(|ui| {
			ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
				ui.label(egui::RichText::new(pos_text).color(label_color).monospace());
				ui.label(egui::RichText::new("•").color(secondary_color).monospace());

				let (rect, _) = ui.allocate_exact_size(swatch_size, egui::Sense::hover());
				let swatch_color = match state.rgb {
					Some(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
					None => Color32::from_rgba_unmultiplied(255, 255, 255, 26),
				};

				ui.painter().rect_filled(rect, 3.0, swatch_color);
				ui.painter().rect_stroke(
					rect,
					3.0,
					egui::Stroke::new(
						1.0,
						match theme {
							HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 36),
							HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
						},
					),
					egui::StrokeKind::Inside,
				);
				ui.label(egui::RichText::new(hex_text).color(label_color).monospace());
				ui.label(egui::RichText::new(rgb_text).color(secondary_color).monospace());

				if show_alt_hint_keycap {
					let alt_active = state.alt_held;
					let (keycap_fill, keycap_stroke, keycap_text) = match theme {
						HudTheme::Dark if alt_active => (
							Color32::from_rgba_unmultiplied(255, 255, 255, 40),
							egui::Stroke::new(
								1.0,
								Color32::from_rgba_unmultiplied(255, 255, 255, 70),
							),
							label_color,
						),
						HudTheme::Dark => (
							Color32::from_rgba_unmultiplied(255, 255, 255, 18),
							egui::Stroke::new(
								1.0,
								Color32::from_rgba_unmultiplied(255, 255, 255, 30),
							),
							secondary_color,
						),
						HudTheme::Light if alt_active => (
							Color32::from_rgba_unmultiplied(0, 0, 0, 22),
							egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 64)),
							label_color,
						),
						HudTheme::Light => (
							Color32::from_rgba_unmultiplied(0, 0, 0, 12),
							egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 32)),
							secondary_color,
						),
					};

					Frame {
						fill: keycap_fill,
						stroke: keycap_stroke,
						corner_radius: CornerRadius::same(6),
						inner_margin: Margin::symmetric(6, 2),
						..Frame::default()
					}
					.show(ui, |ui| {
						ui.label(egui::RichText::new("Alt").color(keycap_text).monospace());
					});
				}
			});
		});
	}

	#[allow(clippy::too_many_arguments)]
	fn render_loupe_tile(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		pill_rect: Rect,
		hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		let ctx = ui.ctx().clone();

		if !state.alt_held {
			return;
		}

		const CELL: f32 = 10.0;

		let fallback_side_px = 21_u32;
		let (w, h) = state
			.loupe
			.as_ref()
			.map(|loupe| loupe.patch.dimensions())
			.unwrap_or((fallback_side_px, fallback_side_px));
		let side = (w.max(h) as f32) * CELL;
		let tile_padding = Margin::same(10);
		let tile_w = side + (tile_padding.left as f32) + (tile_padding.right as f32);
		let tile_h = side + (tile_padding.top as f32) + (tile_padding.bottom as f32);
		let screen = ctx.content_rect();
		let gap = 10.0;
		let mut x = pill_rect.min.x;

		x = x.clamp(screen.min.x + 6.0, (screen.max.x - tile_w - 6.0).max(screen.min.x + 6.0));

		let below_y = pill_rect.max.y + gap;
		let above_y = pill_rect.min.y - gap - tile_h;
		let mut y = if below_y + tile_h <= screen.max.y { below_y } else { above_y };

		y = y.clamp(screen.min.y + 6.0, (screen.max.y - tile_h - 6.0).max(screen.min.y + 6.0));

		let pos = Pos2::new(x, y);

		egui::Area::new(egui::Id::new("rsnap-loupe-tile"))
			.order(egui::Order::Foreground)
			.fixed_pos(pos)
			.show(&ctx, |ui| {
				let body_fill = hud_body_fill_srgba8(theme, hud_opaque);
				let _ = hud_blur_active;
				let fill = Color32::from_rgba_unmultiplied(
					body_fill[0],
					body_fill[1],
					body_fill[2],
					body_fill[3],
				);
				let outer_stroke_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
				};
				let outer_stroke = egui::Stroke::new(1.0, outer_stroke_color);
				let shadow = egui::epaint::Shadow {
					offset: [0, 0],
					blur: 10,
					spread: 0,
					color: match theme {
						HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 28),
						HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 18),
					},
				};
				let frame = Frame {
					fill,
					stroke: outer_stroke,
					shadow,
					corner_radius: CornerRadius::same(18),
					inner_margin: tile_padding,
					..Frame::default()
				};

				frame.show(ui, |ui| {
					ui.set_min_size(Vec2::new(side, side));

					Self::render_loupe(
						ui,
						state,
						monitor,
						cursor,
						hud_blur_active,
						hud_opaque,
						theme,
					);
				});
			});
	}

	fn render_loupe(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		const CELL: f32 = 10.0;

		let mode = state.mode;

		if matches!(mode, OverlayMode::Live) {
			Self::render_live_loupe(ui, state, CELL, hud_blur_active, hud_opaque, theme);
		} else if matches!(mode, OverlayMode::Frozen)
			&& state.monitor == Some(monitor)
			&& state.frozen_image.is_some()
			&& state.cursor.is_some()
		{
			Self::render_frozen_loupe(
				ui,
				state,
				monitor,
				cursor,
				CELL,
				hud_blur_active,
				hud_opaque,
				theme,
			);
		}
	}

	fn render_live_loupe(
		ui: &mut Ui,
		state: &OverlayState,
		cell: f32,
		_hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		let fallback_side_px = 21_u32;
		let (w, h) = state
			.loupe
			.as_ref()
			.map(|loupe| loupe.patch.dimensions())
			.unwrap_or((fallback_side_px, fallback_side_px));
		let side = (w.max(h) as f32) * cell;
		let (rect, _) = ui.allocate_exact_size(Vec2::new(side, side), egui::Sense::hover());
		let body_fill = hud_body_fill_srgba8(theme, hud_opaque);
		let stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
		let grid_stroke =
			egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 26));

		if let Some(loupe) = state.loupe.as_ref() {
			let _ = loupe.center;

			for y in 0..h {
				for x in 0..w {
					let cell_min =
						Pos2::new(rect.min.x + (x as f32) * cell, rect.min.y + (y as f32) * cell);
					let cell_rect = Rect::from_min_size(cell_min, Vec2::splat(cell));
					let pixel = loupe.patch.get_pixel_checked(x, y).expect("pixel bounds checked");
					let fill = Color32::from_rgb(pixel.0[0], pixel.0[1], pixel.0[2]);

					ui.painter().rect_filled(cell_rect, 0.0, fill);
				}
			}

			let n = w.max(h);

			for i in 0..=n {
				let x = rect.min.x + (i as f32) * cell;
				let y = rect.min.y + (i as f32) * cell;

				ui.painter().line_segment(
					[Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
					grid_stroke,
				);
				ui.painter().line_segment(
					[Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
					grid_stroke,
				);
			}
		} else {
			let placeholder_fill =
				Color32::from_rgba_unmultiplied(body_fill[0], body_fill[1], body_fill[2], 255);

			ui.painter().rect_filled(rect, 3.0, placeholder_fill);
		}

		ui.painter().rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);

		let center_x = (w / 2) as f32;
		let center_y = (h / 2) as f32;
		let center_min = Pos2::new(rect.min.x + center_x * cell, rect.min.y + center_y * cell);
		let center_rect = Rect::from_min_size(center_min, Vec2::splat(cell));

		ui.painter().rect_stroke(
			center_rect,
			0.0,
			egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
			egui::StrokeKind::Inside,
		);
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frozen_loupe(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		cell: f32,
		hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		if state.loupe.is_some() {
			Self::render_live_loupe(ui, state, cell, hud_blur_active, hud_opaque, theme);

			return;
		}

		const LOUPE_RADIUS_PX: i32 = 5;
		const LOUPE_SIDE_PX: i32 = (LOUPE_RADIUS_PX * 2) + 1;

		let side = (LOUPE_SIDE_PX as f32) * cell;
		let (rect, _) = ui.allocate_exact_size(Vec2::new(side, side), egui::Sense::hover());
		let Some(image) = state.frozen_image.as_ref() else {
			return;
		};
		let Some((center_x, center_y)) = monitor.local_u32_pixels(cursor) else {
			return;
		};
		let (width, height) = image.dimensions();
		let width = width as i32;
		let height = height as i32;
		let center_x = center_x as i32;
		let center_y = center_y as i32;
		let stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
		let grid_stroke =
			egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 26));

		for dy in -LOUPE_RADIUS_PX..=LOUPE_RADIUS_PX {
			for dx in -LOUPE_RADIUS_PX..=LOUPE_RADIUS_PX {
				let x = center_x + dx;
				let y = center_y + dy;
				let cell_x = dx + LOUPE_RADIUS_PX;
				let cell_y = dy + LOUPE_RADIUS_PX;
				let cell_min = Pos2::new(
					rect.min.x + (cell_x as f32) * cell,
					rect.min.y + (cell_y as f32) * cell,
				);
				let cell_rect = Rect::from_min_size(cell_min, Vec2::splat(cell));
				let fill = if x < 0 || y < 0 || x >= width || y >= height {
					Color32::from_rgba_unmultiplied(0, 0, 0, 0)
				} else {
					let pixel =
						image.get_pixel_checked(x as u32, y as u32).expect("pixel bounds checked");

					Color32::from_rgb(pixel.0[0], pixel.0[1], pixel.0[2])
				};

				ui.painter().rect_filled(cell_rect, 0.0, fill);
			}
		}
		for i in 0..=LOUPE_SIDE_PX {
			let x = rect.min.x + (i as f32) * cell;
			let y = rect.min.y + (i as f32) * cell;

			ui.painter()
				.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], grid_stroke);
			ui.painter()
				.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], grid_stroke);
		}

		ui.painter().rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);

		let center_min = Pos2::new(
			rect.min.x + (LOUPE_RADIUS_PX as f32) * cell,
			rect.min.y + (LOUPE_RADIUS_PX as f32) * cell,
		);
		let center_rect = Rect::from_min_size(center_min, Vec2::splat(cell));

		ui.painter().rect_stroke(
			center_rect,
			0.0,
			egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
			egui::StrokeKind::Inside,
		);
	}

	fn sync_egui_textures(&mut self, gpu: &GpuContext, full_output: &FullOutput) {
		for (id, image_delta) in &full_output.textures_delta.set {
			self.egui_renderer.update_texture(&gpu.device, &gpu.queue, *id, image_delta);
		}
		for id in &full_output.textures_delta.free {
			self.egui_renderer.free_texture(id);
		}
	}

	fn acquire_frame(&mut self, gpu: &GpuContext) -> Result<SurfaceTexture> {
		match self.surface.get_current_texture() {
			Ok(frame) => Ok(frame),
			Err(SurfaceError::Outdated | SurfaceError::Lost) => {
				self.reconfigure(gpu);

				self.needs_reconfigure = false;

				self.surface
					.get_current_texture()
					.wrap_err("Surface was lost and could not be reacquired")
			},
			Err(err) => Err(err).wrap_err("Failed to acquire surface texture"),
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frame(
		&mut self,
		gpu: &GpuContext,
		draw_frozen_bg: bool,
		hud_blur_active: bool,
		frame: SurfaceTexture,
		paint_jobs: &[ClippedPrimitive],
		screen_descriptor: &ScreenDescriptor,
	) -> Result<()> {
		let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
		let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("rsnap-overlay encoder"),
		});
		let _user_cmds = self.egui_renderer.update_buffers(
			&gpu.device,
			&gpu.queue,
			&mut encoder,
			paint_jobs,
			screen_descriptor,
		);

		{
			let rpass_desc = wgpu::RenderPassDescriptor {
				label: Some("rsnap-overlay renderpass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					depth_slice: None,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
			};
			let mut rpass = encoder.begin_render_pass(&rpass_desc).forget_lifetime();

			if draw_frozen_bg && let Some(bg) = &self.hud_bg {
				rpass.set_pipeline(&self.mipgen_surface_pipeline);
				rpass.set_bind_group(0, &bg.mipgen_bind_group, &[]);
				rpass.draw(0..3, 0..1);
			}
			if hud_blur_active
				&& self.hud_pill.is_some()
				&& let Some(bg) = &self.hud_bg
			{
				if let Some(pill) = self.hud_pill {
					let ppp = screen_descriptor.pixels_per_point;
					let pad_px = (24.0 * ppp).ceil() as i32;
					let surface_w = screen_descriptor.size_in_pixels[0].max(1) as i32;
					let surface_h = screen_descriptor.size_in_pixels[1].max(1) as i32;
					let min_x_bound = (surface_w - 1).max(0);
					let min_y_bound = (surface_h - 1).max(0);
					let min_x =
						((pill.rect.min.x * ppp).floor() as i32 - pad_px).clamp(0, min_x_bound);
					let min_y =
						((pill.rect.min.y * ppp).floor() as i32 - pad_px).clamp(0, min_y_bound);
					let max_x =
						((pill.rect.max.x * ppp).ceil() as i32 + pad_px).clamp(0, surface_w);
					let max_y =
						((pill.rect.max.y * ppp).ceil() as i32 + pad_px).clamp(0, surface_h);
					let w = (max_x - min_x).max(1) as u32;
					let h = (max_y - min_y).max(1) as u32;

					rpass.set_scissor_rect(min_x as u32, min_y as u32, w, h);
				}

				rpass.set_pipeline(&self.hud_blur_pipeline);
				rpass.set_bind_group(0, &bg.hud_blur_bind_group, &[]);
				rpass.draw(0..3, 0..1);
				rpass.set_scissor_rect(
					0,
					0,
					screen_descriptor.size_in_pixels[0].max(1),
					screen_descriptor.size_in_pixels[1].max(1),
				);
			}

			self.egui_renderer.render(&mut rpass, paint_jobs, screen_descriptor);
		}

		gpu.queue.submit(Some(encoder.finish()));
		frame.present();

		Ok(())
	}

	fn new(gpu: &GpuContext, window: Arc<winit::window::Window>) -> Result<Self> {
		let surface = gpu
			.instance
			.create_surface(Arc::clone(&window))
			.wrap_err("wgpu create_surface failed")?;
		let caps = surface.get_capabilities(&gpu.adapter);
		let surface_format = Self::pick_surface_format(&caps);
		let surface_alpha = Self::pick_surface_alpha(&caps);
		let surface_config =
			Self::make_surface_config(window.as_ref(), surface_format, surface_alpha);

		surface.configure(&gpu.device, &surface_config);

		let egui_ctx = egui::Context::default();
		let egui_renderer = Renderer::new(
			&gpu.device,
			surface_format,
			egui_wgpu::RendererOptions {
				msaa_samples: 1,
				depth_stencil_format: None,
				dithering: false,
				predictable_texture_filtering: false,
			},
		);
		let bg_sampler = Self::create_bg_sampler(gpu);
		let (mipgen_pipeline, mipgen_bind_group_layout) =
			Self::create_mipgen_pipeline(gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
		let mipgen_surface_pipeline =
			Self::create_mipgen_surface_pipeline(gpu, surface_format, &mipgen_bind_group_layout);
		let (hud_blur_pipeline, hud_blur_bind_group_layout) =
			Self::create_hud_blur_pipeline(gpu, surface_format);
		let hud_blur_uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("rsnap-hud-blur uniform"),
			size: std::mem::size_of::<HudBlurUniformRaw>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		Ok(Self {
			window,
			surface,
			surface_config,
			needs_reconfigure: false,
			egui_ctx,
			egui_renderer,
			bg_sampler,
			mipgen_pipeline,
			mipgen_surface_pipeline,
			mipgen_bind_group_layout,
			hud_blur_pipeline,
			hud_blur_bind_group_layout,
			hud_blur_uniform,
			hud_bg: None,
			hud_bg_generation: 0,
			hud_pill: None,
			loupe_tile: None,
			hud_theme: None,
		})
	}

	fn resize(&mut self, size: PhysicalSize<u32>) -> Result<()> {
		self.surface_config.width = size.width.max(1);
		self.surface_config.height = size.height.max(1);
		self.needs_reconfigure = true;

		Ok(())
	}

	fn reconfigure(&mut self, gpu: &GpuContext) {
		self.surface.configure(&gpu.device, &self.surface_config);
	}

	fn sync_egui_theme(&mut self, theme: HudTheme) {
		if self.hud_theme == Some(theme) {
			return;
		}

		match theme {
			HudTheme::Dark => self.egui_ctx.set_visuals(egui::Visuals::dark()),
			HudTheme::Light => self.egui_ctx.set_visuals(egui::Visuals::light()),
		}

		self.hud_theme = Some(theme);
	}

	#[allow(clippy::too_many_arguments)]
	fn draw(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		draw_hud: bool,
		hud_local_cursor_override: Option<Pos2>,
		hud_compact: bool,
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		show_hud_blur: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_fog_amount: f32,
		hud_milk_amount: f32,
		theme_mode: ThemeMode,
	) -> Result<()> {
		self.apply_pending_reconfigure(gpu);

		let theme = effective_hud_theme(theme_mode, self.window.theme());

		self.sync_egui_theme(theme);

		let (size, pixels_per_point, raw_input) = self.prepare_egui_input(gpu);
		let can_draw_hud = draw_hud && Self::should_draw_hud(state, monitor);
		let should_sync_bg = matches!(state.mode, OverlayMode::Frozen) || can_draw_hud;

		if should_sync_bg {
			self.sync_hud_bg(gpu, state, monitor)?;
		} else {
			self.hud_bg = None;
			self.hud_bg_generation = match state.mode {
				OverlayMode::Live => state.live_bg_generation,
				OverlayMode::Frozen => state.frozen_generation,
			};
		}

		// `show_hud_blur` is a UX toggle for "glass mode":
		// - On macOS: native compositor blur (the HUD window itself is blurred).
		// - On other platforms: may be implemented by a shader blur (requires `hud_bg`).
		let hud_glass_active = can_draw_hud && show_hud_blur && !hud_opaque;
		let hud_shader_blur_active = hud_glass_active
			&& self.hud_bg.is_some()
			&& match state.mode {
				OverlayMode::Live => state.live_bg_monitor == Some(monitor),
				OverlayMode::Frozen => state.monitor == Some(monitor),
			};
		let (full_output, hud_pill) = self.run_egui(
			raw_input,
			state,
			monitor,
			can_draw_hud,
			hud_local_cursor_override,
			hud_compact,
			show_hud_blur,
			hud_anchor,
			show_alt_hint_keycap,
			hud_glass_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			theme,
		);

		self.hud_pill = hud_pill;

		if hud_shader_blur_active {
			self.update_hud_blur_uniform(
				gpu,
				size,
				pixels_per_point,
				theme,
				hud_fog_amount,
				hud_milk_amount,
			);
		}

		self.sync_egui_textures(gpu, &full_output);

		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
		let screen_descriptor =
			ScreenDescriptor { size_in_pixels: [size.width, size.height], pixels_per_point };
		let frame = self.acquire_frame(gpu)?;
		let draw_frozen_bg = matches!(state.mode, OverlayMode::Frozen)
			&& state.monitor == Some(monitor)
			&& state.frozen_image.is_some();

		self.render_frame(
			gpu,
			draw_frozen_bg,
			hud_shader_blur_active,
			frame,
			&paint_jobs,
			&screen_descriptor,
		)?;

		Ok(())
	}

	#[allow(clippy::too_many_arguments)]
	fn draw_loupe_tile_window(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		show_hud_blur: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		theme_mode: ThemeMode,
	) -> Result<()> {
		self.apply_pending_reconfigure(gpu);

		let theme = effective_hud_theme(theme_mode, self.window.theme());

		self.sync_egui_theme(theme);

		let (size, pixels_per_point, raw_input) = self.prepare_egui_input(gpu);

		self.hud_bg = None;
		self.hud_pill = None;
		self.loupe_tile = None;

		let hud_blur_active = show_hud_blur && !hud_opaque;
		let body_fill = Self::tinted_hud_body_fill(theme, hud_opaque, hud_opacity, hud_milk_amount);
		let (full_output, loupe_tile_rect) = self.run_loupe_tile_egui(
			raw_input,
			state,
			monitor,
			cursor,
			theme,
			hud_blur_active,
			hud_opaque,
			body_fill,
		);

		self.loupe_tile = loupe_tile_rect;

		self.sync_egui_textures(gpu, &full_output);

		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
		let screen_descriptor =
			ScreenDescriptor { size_in_pixels: [size.width, size.height], pixels_per_point };
		let frame = self.acquire_frame(gpu)?;

		self.render_frame(gpu, false, false, frame, &paint_jobs, &screen_descriptor)?;

		Ok(())
	}

	fn tinted_hud_body_fill(
		theme: HudTheme,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
	) -> Color32 {
		let opacity = if hud_opaque { 1.0 } else { hud_opacity.clamp(0.0, 1.0) };
		let tint = hud_milk_amount.clamp(0.0, 1.0);
		let tint_strength = match theme {
			HudTheme::Dark => 0.22,
			HudTheme::Light => 0.28,
		};
		let mix = (tint * tint_strength).clamp(0.0, 1.0);
		let mut fill = hud_body_fill_srgba8(theme, false);

		fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
			((f32::from(a) + ((f32::from(b) - f32::from(a)) * t)).round().clamp(0.0, 255.0)) as u8
		}

		fill[0] = lerp_u8(fill[0], 255, mix);
		fill[1] = lerp_u8(fill[1], 255, mix);
		fill[2] = lerp_u8(fill[2], 255, mix);
		fill[3] = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;

		Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3])
	}

	#[allow(clippy::too_many_arguments)]
	fn run_loupe_tile_egui(
		&mut self,
		raw_input: egui::RawInput,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		body_fill: Color32,
	) -> (FullOutput, Option<Rect>) {
		let mut loupe_tile_rect = None;
		let full_output = self.egui_ctx.run(raw_input, |ctx| {
			if !state.alt_held {
				return;
			}

			const CELL: f32 = 10.0;

			let fallback_side_px = 21_u32;
			let (w, h) = state
				.loupe
				.as_ref()
				.map(|loupe| loupe.patch.dimensions())
				.unwrap_or((fallback_side_px, fallback_side_px));
			let side = (w.max(h) as f32) * CELL;
			let tile_padding = Margin::same(10);
			let outer_stroke_color = match theme {
				HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
				HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
			};
			let outer_stroke = egui::Stroke::new(1.0, outer_stroke_color);
			let shadow = egui::epaint::Shadow {
				offset: [0, 0],
				blur: 10,
				spread: 0,
				color: match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 28),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 18),
				},
			};
			let tile_radius = 12_u8;
			let frame = Frame {
				fill: body_fill,
				stroke: outer_stroke,
				shadow,
				corner_radius: CornerRadius::same(tile_radius),
				inner_margin: tile_padding,
				..Frame::default()
			};
			let pad = 6.0;

			egui::Area::new(egui::Id::new("rsnap-loupe-window"))
				.order(egui::Order::Foreground)
				.fixed_pos(Pos2::new(pad, pad))
				.show(ctx, |ui| {
					let inner = frame.show(ui, |ui| {
						ui.set_min_size(Vec2::new(side, side));

						Self::render_loupe(
							ui,
							state,
							monitor,
							cursor,
							hud_blur_active,
							hud_opaque,
							theme,
						);
					});
					let tile_rect = inner.response.rect;

					loupe_tile_rect = Some(tile_rect);

					let inner_stroke_color = match theme {
						HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
						HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
					};
					let inner_stroke = egui::Stroke::new(1.0, inner_stroke_color);
					let inner_rect = tile_rect.shrink(1.0);

					ui.painter().rect_stroke(
						inner_rect,
						CornerRadius::same(tile_radius.saturating_sub(1)),
						inner_stroke,
						egui::StrokeKind::Inside,
					);
				});
		});

		(full_output, loupe_tile_rect)
	}

	fn update_hud_blur_uniform(
		&mut self,
		gpu: &GpuContext,
		size: PhysicalSize<u32>,
		pixels_per_point: f32,
		theme: HudTheme,
		hud_fog_amount: f32,
		hud_milk_amount: f32,
	) {
		if self.hud_bg.is_none() {
			return;
		}

		let Some(hud_pill) = self.hud_pill else {
			return;
		};
		let max_lod = self.hud_bg.as_ref().map(|bg| bg.max_lod).unwrap_or(0.0);
		let surface_w = size.width as f32;
		let surface_h = size.height as f32;

		if surface_w <= 0.0 || surface_h <= 0.0 {
			return;
		}

		let rect = hud_pill.rect;
		let rect_min_px = [rect.min.x * pixels_per_point, rect.min.y * pixels_per_point];
		let rect_size_px = [rect.width() * pixels_per_point, rect.height() * pixels_per_point];
		let radius_px = hud_pill.radius_points * pixels_per_point;
		let blur_radius_px = (0.9 + (hud_fog_amount.clamp(0.0, 1.0) * 3.2)) * pixels_per_point;
		let edge_softness_px = 1.0 * pixels_per_point;

		fn srgb8_to_linear_f32(x: u8) -> f32 {
			let c = (x as f32) / 255.0;

			if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
		}

		let fill = hud_body_fill_srgba8(theme, false);
		let tint_a = hud_blur_tint_alpha(theme);
		let tint_rgba = [
			srgb8_to_linear_f32(fill[0]),
			srgb8_to_linear_f32(fill[1]),
			srgb8_to_linear_f32(fill[2]),
			tint_a,
		];
		let flip_y = if cfg!(target_os = "macos") { 1.0 } else { 0.0 };
		let effects =
			[hud_fog_amount.clamp(0.0, 1.0), hud_milk_amount.clamp(0.0, 1.0), max_lod, flip_y];
		let u = HudBlurUniformRaw {
			rect_min_size: [rect_min_px[0], rect_min_px[1], rect_size_px[0], rect_size_px[1]],
			radius_blur_soft: [radius_px, blur_radius_px, edge_softness_px, 0.0],
			surface_size_px: [surface_w, surface_h, 0.0, 0.0],
			tint_rgba,
			effects,
		};

		gpu.queue.write_buffer(&self.hud_blur_uniform, 0, u.as_bytes());
	}

	fn sync_hud_bg(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
	) -> Result<()> {
		let (target_generation, target_image) = match state.mode {
			OverlayMode::Live if state.live_bg_monitor == Some(monitor) => {
				(state.live_bg_generation, state.live_bg_image.as_ref())
			},
			OverlayMode::Frozen if state.monitor == Some(monitor) => {
				(state.frozen_generation, state.frozen_image.as_ref())
			},
			OverlayMode::Live => {
				self.hud_bg = None;
				self.hud_bg_generation = state.live_bg_generation;

				return Ok(());
			},
			OverlayMode::Frozen => {
				self.hud_bg = None;
				self.hud_bg_generation = state.frozen_generation;

				return Ok(());
			},
		};

		if self.hud_bg.is_some() && self.hud_bg_generation == target_generation {
			if target_image.is_none() {
				// Keep displaying the already-uploaded background even if image bytes moved.
				return Ok(());
			}

			return Ok(());
		}

		let Some(image) = target_image else {
			// Capture is in progress and no image is available yet.
			self.hud_bg = None;
			self.hud_bg_generation = target_generation;

			return Ok(());
		};

		self.render_frozen_bg_to_texture(gpu, image, target_generation)
	}

	fn render_frozen_bg_to_texture(
		&mut self,
		gpu: &GpuContext,
		image: &RgbaImage,
		target_generation: u64,
	) -> Result<()> {
		let upload_image =
			downscale_for_gpu_upload(image, gpu.device.limits().max_texture_dimension_2d);
		let (width, height) = upload_image.dimensions();
		let max_side = gpu.device.limits().max_texture_dimension_2d;
		let mip_level_count = Self::mip_level_count(width, height).min(10);

		debug_assert!(width <= max_side && height <= max_side);

		let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("rsnap-frozen-bg texture"),
			size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
			mip_level_count,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8UnormSrgb,
			usage: wgpu::TextureUsages::TEXTURE_BINDING
				| wgpu::TextureUsages::COPY_DST
				| wgpu::TextureUsages::RENDER_ATTACHMENT,
			view_formats: &[],
		});
		let upload_bytes = upload_image.as_raw();
		let bytes_per_pixel = 4_usize;
		let unpadded_bytes_per_row = (width as usize) * bytes_per_pixel;
		let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
		let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
		let rgba_padded;
		let rgba_bytes: &[u8] = if padded_bytes_per_row == unpadded_bytes_per_row {
			upload_bytes
		} else {
			let src = upload_bytes;

			rgba_padded =
				pad_rows(src, unpadded_bytes_per_row, padded_bytes_per_row, height as usize);

			&rgba_padded
		};

		gpu.queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &texture,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			rgba_bytes,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(padded_bytes_per_row as u32),
				rows_per_image: Some(height),
			},
			wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
		);
		self.generate_mipmaps(gpu, &texture, mip_level_count);

		let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
		let hud_blur_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("rsnap-hud-blur bind group"),
			layout: &self.hud_blur_bind_group_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&self.bg_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: self.hud_blur_uniform.as_entire_binding(),
				},
			],
		});
		let mipgen_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("rsnap-mipgen fullscreen bind group"),
			layout: &self.mipgen_bind_group_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&self.bg_sampler),
				},
			],
		});
		let max_lod = (mip_level_count.saturating_sub(1)) as f32;

		self.hud_bg = Some(HudBg {
			_texture: texture,
			_view: view,
			hud_blur_bind_group,
			mipgen_bind_group,
			max_lod,
		});
		self.hud_bg_generation = target_generation;

		Ok(())
	}
}

struct HudBg {
	_texture: wgpu::Texture,
	_view: wgpu::TextureView,
	hud_blur_bind_group: BindGroup,
	mipgen_bind_group: BindGroup,
	max_lod: f32,
}

#[derive(Clone, Copy, Debug)]
struct HudPillGeometry {
	rect: Rect,
	radius_points: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct HudBlurUniformRaw {
	rect_min_size: [f32; 4],
	radius_blur_soft: [f32; 4],
	surface_size_px: [f32; 4],
	tint_rgba: [f32; 4],
	effects: [f32; 4],
}
impl HudBlurUniformRaw {
	fn as_bytes(&self) -> &[u8] {
		unsafe {
			std::slice::from_raw_parts(
				std::ptr::from_ref(self).cast::<u8>(),
				std::mem::size_of::<Self>(),
			)
		}
	}
}

#[cfg(target_os = "macos")]
fn macos_order_front_regardless(window: &winit::window::Window) {
	use objc::runtime::Object;

	use objc::{msg_send, sel, sel_impl};

	let Ok(handle) = window.window_handle() else {
		return;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return;
	};
	let ns_view = appkit.ns_view.as_ptr() as *mut Object;

	if ns_view.is_null() {
		return;
	}

	unsafe {
		let ns_window: *mut Object = msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		let _: () = msg_send![ns_window, orderFrontRegardless];
	}
}

fn effective_hud_theme(mode: ThemeMode, window_theme: Option<Theme>) -> HudTheme {
	match mode {
		ThemeMode::System => match window_theme.unwrap_or(Theme::Dark) {
			Theme::Dark => HudTheme::Dark,
			Theme::Light => HudTheme::Light,
		},
		ThemeMode::Dark => HudTheme::Dark,
		ThemeMode::Light => HudTheme::Light,
	}
}

fn hud_body_fill_srgba8(theme: HudTheme, opaque: bool) -> [u8; 4] {
	let mut c = if matches!(theme, HudTheme::Light) {
		HUD_PILL_BODY_FILL_LIGHT_SRGBA8
	} else {
		HUD_PILL_BODY_FILL_DARK_SRGBA8
	};

	if opaque {
		c[3] = 255;
	}

	c
}

fn hud_blur_tint_alpha(theme: HudTheme) -> f32 {
	if matches!(theme, HudTheme::Light) {
		HUD_PILL_BLUR_TINT_ALPHA_LIGHT
	} else {
		HUD_PILL_BLUR_TINT_ALPHA_DARK
	}
}

fn frozen_rgb(
	image: &Option<RgbaImage>,
	monitor: Option<MonitorRect>,
	point: GlobalPoint,
) -> Option<Rgb> {
	let Some(image) = image else {
		return None;
	};
	let monitor = monitor?;
	let (x, y) = monitor.local_u32_pixels(point)?;
	let pixel = image.get_pixel_checked(x, y)?;

	Some(Rgb::new(pixel.0[0], pixel.0[1], pixel.0[2]))
}

fn frozen_loupe_patch(
	image: &Option<RgbaImage>,
	monitor: Option<MonitorRect>,
	point: GlobalPoint,
	width_px: u32,
	height_px: u32,
) -> Option<RgbaImage> {
	let Some(image) = image else {
		return None;
	};
	let monitor = monitor?;
	let (center_x, center_y) = monitor.local_u32_pixels(point)?;
	let mut out = RgbaImage::new(width_px.max(1), height_px.max(1));
	let out_width = out.width() as i32;
	let out_height = out.height() as i32;
	let half_width = out_width / 2;
	let half_height = out_height / 2;
	let center_x = center_x as i32;
	let center_y = center_y as i32;
	let image_width = image.width() as i32;
	let image_height = image.height() as i32;

	for out_y in 0..out.height() {
		for out_x in 0..out.width() {
			let image_x = center_x + (out_x as i32) - half_width;
			let image_y = center_y + (out_y as i32) - half_height;
			let color = if image_x >= 0
				&& image_y >= 0
				&& image_x < image_width
				&& image_y < image_height
			{
				*image.get_pixel(image_x as u32, image_y as u32)
			} else {
				image::Rgba([0, 0, 0, 0])
			};

			out.put_pixel(out_x, out_y, color);
		}
	}

	Some(out)
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn write_png_bytes_to_clipboard(png_bytes: &[u8]) -> Result<()> {
	use std::ffi::CString;

	use objc::runtime::{BOOL, Object, YES};

	use objc::{class, msg_send, sel, sel_impl};

	let pasteboard_type = CString::new("public.png").wrap_err("Invalid NSPasteboard type")?;

	unsafe {
		let data: *mut Object =
			msg_send![class!(NSData), dataWithBytes: png_bytes.as_ptr() length: png_bytes.len()];
		let pasteboard: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
		let _: i64 = msg_send![pasteboard, clearContents];
		let ty: *mut Object =
			msg_send![class!(NSString), stringWithUTF8String: pasteboard_type.as_ptr()];
		let ok: BOOL = msg_send![pasteboard, setData: data forType: ty];

		if ok != YES {
			return Err(eyre::eyre!("NSPasteboard setData:forType failed"));
		}
	}

	Ok(())
}

#[cfg(not(target_os = "macos"))]
fn write_png_bytes_to_clipboard(png_bytes: &[u8]) -> Result<()> {
	use arboard::{Clipboard, ImageData};

	let image = image::load_from_memory(png_bytes).wrap_err("Failed to decode PNG bytes")?;
	let rgba = image.to_rgba8();
	let (width, height) = rgba.dimensions();
	let mut clipboard = Clipboard::new().wrap_err("Failed to initialize clipboard")?;

	clipboard
		.set_image(ImageData {
			width: width as usize,
			height: height as usize,
			bytes: std::borrow::Cow::Owned(rgba.into_raw()),
		})
		.wrap_err("Failed to write image to clipboard")?;

	Ok(())
}

fn write_text_to_clipboard(text: &str) -> Result<()> {
	use arboard::Clipboard;

	let mut clipboard = Clipboard::new().wrap_err("Failed to initialize clipboard")?;

	clipboard.set_text(text.to_string()).wrap_err("Failed to write text to clipboard")?;

	Ok(())
}

fn pad_rows(src: &[u8], src_row_bytes: usize, dst_row_bytes: usize, rows: usize) -> Vec<u8> {
	debug_assert!(dst_row_bytes >= src_row_bytes);

	let mut out = vec![0_u8; dst_row_bytes * rows];

	for y in 0..rows {
		let src_i = y * src_row_bytes;
		let dst_i = y * dst_row_bytes;

		out[dst_i..dst_i + src_row_bytes].copy_from_slice(&src[src_i..src_i + src_row_bytes]);
	}

	out
}

fn downscale_for_gpu_upload(image: &RgbaImage, max_side: u32) -> std::borrow::Cow<'_, RgbaImage> {
	if image.width() <= max_side && image.height() <= max_side {
		return std::borrow::Cow::Borrowed(image);
	}

	let longest_side = image.width().max(image.height()) as f32;
	let scale = (max_side as f32) / longest_side;
	let width = ((image.width() as f32) * scale).round().max(1.0) as u32;
	let height = ((image.height() as f32) * scale).round().max(1.0) as u32;

	std::borrow::Cow::Owned(image::imageops::resize(
		image,
		width.min(max_side),
		height.min(max_side),
		FilterType::Triangle,
	))
}

fn global_to_local(cursor: GlobalPoint, monitor: MonitorRect) -> Option<Pos2> {
	let (x, y) = monitor.local_u32(cursor)?;

	Some(Pos2::new(x as f32, y as f32))
}

#[cfg(target_os = "macos")]
fn macos_configure_hud_window(
	window: &winit::window::Window,
	blur_enabled: bool,
	blur_amount: f32,
	corner_radius_points: Option<f64>,
) {
	use objc::runtime::{Object, YES};

	use objc::{class, msg_send, sel, sel_impl};

	let Ok(handle) = window.window_handle() else {
		return;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return;
	};
	let ns_view = appkit.ns_view.as_ptr().cast::<Object>();

	unsafe {
		let ns_window: *mut Object = msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		// winit exposes blur as a boolean. We also set an explicit radius so we can drive it from
		// settings (this uses the same private CGS API that winit uses internally).
		{
			use std::ffi::c_void;

			#[link(name = "CoreGraphics", kind = "framework")]
			unsafe extern "C" {
				fn CGSMainConnectionID() -> *mut c_void;

				fn CGSSetWindowBackgroundBlurRadius(
					connection_id: *mut c_void,
					window_id: isize,
					radius: i64,
				) -> i32;
			}

			let amount = blur_amount.clamp(0.0, 1.0);
			let radius = if blur_enabled {
				// Map the slider linearly (0..=1) to the native blur radius.
				// Keep the upper bound conservative; CGS blur radius gets strong quickly.
				let max_radius = 12.0;

				(amount * max_radius).round().clamp(0.0, 200.0) as i64
			} else {
				0
			};
			let window_number: isize = msg_send![ns_window, windowNumber];
			let _ = CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), window_number, radius);
		}

		let _: () = msg_send![ns_window, setOpaque: false];
		let _: () = msg_send![ns_window, setHasShadow: false];
		let sharing_type_none = 0_u64;
		let _: () = msg_send![ns_window, setSharingType: sharing_type_none];
		let clear: *mut Object = msg_send![class!(NSColor), clearColor];
		let _: () = msg_send![ns_window, setBackgroundColor: clear];
		let content_view: *mut Object = msg_send![ns_window, contentView];

		if content_view.is_null() {
			return;
		}

		let _: () = msg_send![content_view, setWantsLayer: YES];
		let layer: *mut Object = msg_send![content_view, layer];

		if layer.is_null() {
			return;
		}

		// Round the window itself so native blur doesn't show a rectangular boundary.
		let scale = window.scale_factor().max(1.0);
		let size = window.inner_size();
		let height_points = (size.height as f64) / scale;
		let radius = corner_radius_points.unwrap_or(height_points * 0.5);
		let _: () = msg_send![layer, setCornerRadius: radius];
		let _: () = msg_send![layer, setMasksToBounds: YES];
	}
}
