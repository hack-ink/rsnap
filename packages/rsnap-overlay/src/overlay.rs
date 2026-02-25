use std::{
	collections::HashMap,
	sync::Arc,
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Result, WrapErr};
use device_query::DeviceQuery;
use egui::ClippedPrimitive;
use egui::FullOutput;
use egui::Ui;
use egui::{Align, Color32, CornerRadius, Frame, Layout, Margin, Pos2, Rect, Vec2, ViewportId};
use egui_wgpu::{Renderer, ScreenDescriptor};
use image::{RgbaImage, imageops::FilterType};
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
use winit::dpi::PhysicalPosition;
use winit::event::KeyEvent;
use winit::{
	dpi::{LogicalPosition, LogicalSize, PhysicalSize},
	event::{ElementState, Modifiers, MouseButton, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
	keyboard::{Key, NamedKey},
	window::{CursorIcon, WindowId, WindowLevel},
};

use crate::{
	state::{GlobalPoint, MonitorRect, OverlayMode, OverlayState, Rgb},
	worker::{OverlayWorker, WorkerResponse},
};

const HUD_PILL_BODY_FILL_SRGBA8: [u8; 4] = [28, 28, 32, 156];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HudAnchor {
	Cursor,
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

#[derive(Clone, Debug)]
pub struct OverlayConfig {
	pub hud_anchor: HudAnchor,
	pub show_alt_hint_keycap: bool,
	pub show_hud_blur: bool,
}
impl Default for OverlayConfig {
	fn default() -> Self {
		Self { hud_anchor: HudAnchor::Cursor, show_alt_hint_keycap: true, show_hud_blur: true }
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
	windows: HashMap<WindowId, OverlayWindow>,
	gpu: Option<GpuContext>,
	last_present_at: Instant,
	last_rgb_request_at: Instant,
	rgb_request_interval: Duration,
	last_live_bg_request_at: Instant,
	live_bg_request_interval: Duration,
	last_loupe_request_at: Instant,
	loupe_request_interval: Duration,
	loupe_patch_width_px: u32,
	loupe_patch_height_px: u32,
	pending_freeze_capture: Option<MonitorRect>,
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

		Self {
			config,
			worker: None,
			cursor_device: device_query::DeviceState::new(),
			state: OverlayState::new(),
			windows: HashMap::new(),
			gpu: None,
			last_present_at: Instant::now(),
			last_rgb_request_at: Instant::now(),
			rgb_request_interval: Duration::from_millis(16),
			last_live_bg_request_at: Instant::now() - live_bg_request_interval,
			live_bg_request_interval,
			last_loupe_request_at: Instant::now(),
			loupe_request_interval: Duration::from_millis(33),
			loupe_patch_width_px: 21,
			loupe_patch_height_px: 21,
			pending_freeze_capture: None,
			pending_encode_png: None,
		}
	}

	pub fn set_config(&mut self, config: OverlayConfig) {
		let prev_show_hud_blur = self.config.show_hud_blur;

		self.config = config;

		if !self.is_active() {
			return;
		}
		if prev_show_hud_blur != self.config.show_hud_blur {
			if self.config.show_hud_blur {
				self.last_live_bg_request_at = Instant::now() - self.live_bg_request_interval;

				if matches!(self.state.mode, OverlayMode::Live)
					&& let Some(cursor) = self.state.cursor
					&& let Some(monitor) = self.monitor_at(cursor)
				{
					self.maybe_request_live_bg(monitor);
				}
			} else {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;
			}
		}

		self.request_redraw_all();
	}

	#[must_use]
	pub fn is_active(&self) -> bool {
		!self.windows.is_empty()
	}

	pub fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
		if self.is_active() {
			return Ok(());
		}

		self.state = OverlayState::new();
		self.worker = Some(OverlayWorker::new(crate::backend::default_capture_backend()));

		let monitors =
			xcap::Monitor::all().map_err(|err| format!("xcap Monitor::all failed: {err:?}"))?;

		if monitors.is_empty() {
			return Err(String::from("No monitors detected"));
		}

		let gpu = GpuContext::new().map_err(|err| format!("{err:#}"))?;

		self.gpu = Some(gpu);

		for monitor in monitors {
			let monitor_rect = MonitorRect {
				id: monitor.id(),
				origin: GlobalPoint::new(monitor.x(), monitor.y()),
				width: monitor.width(),
				height: monitor.height(),
				scale_factor_x1000: (monitor.scale_factor() * 1_000.0).round() as u32,
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

		self.request_redraw_all();
		self.initialize_cursor_state();

		Ok(())
	}

	pub fn request_redraw_all(&self) {
		for w in self.windows.values() {
			w.window.request_redraw();
		}
	}

	pub fn request_redraw_for_monitor(&self, monitor: MonitorRect) {
		for w in self.windows.values() {
			if w.monitor == monitor {
				w.window.request_redraw();
			}
		}
	}

	pub fn about_to_wait(&mut self) -> OverlayControl {
		// Avoid a tight present loop if the OS delivers spurious redraws.
		if self.is_active() && self.last_present_at.elapsed() > Duration::from_secs(30) {
			self.request_redraw_all();
		}

		if let Some(worker) = &self.worker {
			if let Some(image) = self.pending_encode_png.take()
				&& let Err(image) = worker.request_encode_png(image)
			{
				self.pending_encode_png = Some(image);
			}

			while let Some(resp) = worker.try_recv() {
				match resp {
					WorkerResponse::SampledLoupe { monitor, point, rgb, patch } => {
						if matches!(self.state.mode, OverlayMode::Live) {
							self.state.rgb = rgb;
							self.state.loupe = patch
								.map(|patch| crate::state::LoupeSample { center: point, patch });

							let current_monitor =
								self.state.cursor.and_then(|cursor| self.monitor_at(cursor));

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

							let current_monitor =
								self.state.cursor.and_then(|cursor| self.monitor_at(cursor));

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
							self.request_redraw_for_monitor(monitor);
						} else if matches!(self.state.mode, OverlayMode::Live)
							&& self.config.show_hud_blur
							&& self.state.cursor.and_then(|cursor| self.monitor_at(cursor))
								== Some(monitor)
						{
							self.state.live_bg_monitor = Some(monitor);
							self.state.live_bg_image = Some(image);
							self.state.live_bg_generation =
								self.state.live_bg_generation.wrapping_add(1);

							self.request_redraw_for_monitor(monitor);
						}
					},
					WorkerResponse::Error(message) => {
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
			WindowEvent::KeyboardInput { event, .. } => self.handle_key_event(event),
			WindowEvent::ModifiersChanged(modifiers) => self.handle_modifiers_changed(modifiers),
			WindowEvent::RedrawRequested => self.handle_redraw_requested(window_id),
			_ => OverlayControl::Continue,
		}
	}

	fn handle_modifiers_changed(&mut self, modifiers: &Modifiers) -> OverlayControl {
		let alt = modifiers.state().alt_key();

		if self.state.alt_held == alt {
			return OverlayControl::Continue;
		}

		self.state.alt_held = alt;

		if !alt {
			self.state.loupe = None;
		} else if matches!(self.state.mode, OverlayMode::Live)
			&& let Some(cursor) = self.state.cursor
			&& let Some(monitor) = self.monitor_at(cursor)
			&& let Some(worker) = &self.worker
		{
			worker.try_sample_loupe(
				monitor,
				cursor,
				self.loupe_patch_width_px,
				self.loupe_patch_height_px,
			);

			self.last_loupe_request_at = Instant::now();
		}

		if let Some(monitor) = self.state.cursor.and_then(|cursor| self.monitor_at(cursor)) {
			self.request_redraw_for_monitor(monitor);
		} else {
			self.request_redraw_all();
		}

		OverlayControl::Continue
	}

	fn handle_resized(&mut self, window_id: WindowId, size: PhysicalSize<u32>) -> OverlayControl {
		let Some(overlay_window) = self.windows.get_mut(&window_id) else {
			return OverlayControl::Continue;
		};

		match overlay_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn handle_scale_factor_changed(&mut self, window_id: WindowId) -> OverlayControl {
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
		let old_monitor = self.state.cursor.and_then(|cursor| self.monitor_at(cursor));
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

		if matches!(self.state.mode, OverlayMode::Live) && self.config.show_hud_blur {
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

		self.state.clear_error();
		self.state.begin_freeze(monitor);

		if self.config.show_hud_blur
			&& self.state.live_bg_monitor == Some(monitor)
			&& let Some(image) = self.state.live_bg_image.take()
		{
			self.state.live_bg_monitor = None;

			self.state.finish_freeze(monitor, image);

			self.pending_freeze_capture = None;
		} else {
			self.pending_freeze_capture = Some(monitor);
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
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};
		let Some(overlay_window) = self.windows.get_mut(&window_id) else {
			return OverlayControl::Continue;
		};

		if let Err(err) = overlay_window.renderer.draw(
			gpu,
			&self.state,
			overlay_window.monitor,
			self.config.hud_anchor,
			self.config.show_alt_hint_keycap,
			self.config.show_hud_blur,
		) {
			return self.exit(OverlayExit::Error(format!("{err:#}")));
		}

		self.last_present_at = Instant::now();

		if self.pending_freeze_capture == Some(overlay_window.monitor)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor == Some(overlay_window.monitor)
			&& self.state.frozen_image.is_none()
			&& let Some(worker) = &self.worker
			&& worker.request_freeze_capture(overlay_window.monitor)
		{
			self.pending_freeze_capture = None;
		}

		OverlayControl::Continue
	}

	fn exit(&mut self, exit: OverlayExit) -> OverlayControl {
		self.windows.clear();

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

			return;
		};

		self.update_cursor_state(monitor, cursor);

		if matches!(self.state.mode, OverlayMode::Live) {
			if self.config.show_hud_blur {
				self.maybe_request_live_bg(monitor);
			}

			if let Some(worker) = &self.worker {
				worker.try_sample_rgb(monitor, cursor);

				self.last_rgb_request_at = Instant::now();
			}
		}
	}

	fn maybe_request_live_bg(&mut self, monitor: MonitorRect) {
		if !matches!(self.state.mode, OverlayMode::Live) || !self.config.show_hud_blur {
			return;
		}
		if self.state.live_bg_monitor == Some(monitor) && self.state.live_bg_image.is_some() {
			return;
		}
		if self.last_live_bg_request_at.elapsed() < self.live_bg_request_interval {
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

	fn update_cursor_state(&mut self, _monitor: MonitorRect, cursor: GlobalPoint) {
		self.state.cursor = Some(cursor);

		match self.state.mode {
			OverlayMode::Live => {},
			OverlayMode::Frozen => {
				let frozen_monitor = self.state.monitor;

				self.state.rgb = frozen_rgb(&self.state.frozen_image, frozen_monitor, cursor);
			},
		}
	}
}

impl Default for OverlaySession {
	fn default() -> Self {
		Self::new()
	}
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
		.ok_or_else(|| eyre::eyre!("No suitable GPU adapters found"))?;
		let adapter_limits = adapter.limits();
		let (device, queue) = pollster::block_on(adapter.request_device(
			&wgpu::DeviceDescriptor {
				label: Some("rsnap-overlay device"),
				required_features: wgpu::Features::empty(),
				// Use the adapter's actual limits. Using `downlevel_defaults()` caps max texture
				// size to 2048, which breaks on common HiDPI displays.
				required_limits: adapter_limits,
				memory_hints: wgpu::MemoryHints::Performance,
			},
			None,
		))
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
	hud_blur_pipeline: RenderPipeline,
	hud_blur_bind_group_layout: BindGroupLayout,
	hud_blur_uniform: wgpu::Buffer,
	hud_bg: Option<HudBg>,
	hud_bg_generation: u64,
	hud_pill: Option<HudPillGeometry>,
}
impl WindowRenderer {
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
			mipmap_filter: wgpu::FilterMode::Nearest,
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

	fn run_egui(
		&mut self,
		raw_input: egui::RawInput,
		state: &OverlayState,
		monitor: MonitorRect,
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
	) -> (FullOutput, Option<HudPillGeometry>) {
		let can_draw = Self::should_draw_hud(state, monitor);
		let hud_data = if can_draw {
			state.cursor.and_then(|cursor| {
				global_to_local(cursor, monitor).map(|local_cursor| (cursor, local_cursor))
			})
		} else {
			None
		};
		let mut hud_pill = None;
		let full_output = self.egui_ctx.run(raw_input, |ctx| {
			if let Some((cursor, local_cursor)) = hud_data {
				Self::render_hud(
					ctx,
					state,
					monitor,
					cursor,
					local_cursor,
					hud_anchor,
					show_alt_hint_keycap,
					hud_blur_active,
					&mut hud_pill,
				);
			}
		});

		(full_output, hud_pill)
	}

	fn should_draw_hud(state: &OverlayState, monitor: MonitorRect) -> bool {
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
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
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
					show_alt_hint_keycap,
					hud_blur_active,
					hud_pill_out,
				);
			});
	}

	fn render_hud_frame(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let pill_radius = 18_u8;
		let body_fill = Color32::from_rgba_unmultiplied(
			HUD_PILL_BODY_FILL_SRGBA8[0],
			HUD_PILL_BODY_FILL_SRGBA8[1],
			HUD_PILL_BODY_FILL_SRGBA8[2],
			HUD_PILL_BODY_FILL_SRGBA8[3],
		);
		let outer_stroke =
			egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 40));
		let pill_shadow = egui::epaint::Shadow {
			offset: [0, 0],
			blur: 10,
			spread: 0,
			color: Color32::from_rgba_unmultiplied(0, 0, 0, 28),
		};
		let inner = Frame {
			fill: if hud_blur_active { Color32::TRANSPARENT } else { body_fill },
			stroke: outer_stroke,
			shadow: pill_shadow,
			corner_radius: CornerRadius::same(pill_radius),
			inner_margin: Margin::symmetric(12, 8),
			..Frame::default()
		}
		.show(ui, |ui| {
			ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);

			if let Some(err) = &state.error_message {
				ui.label(
					egui::RichText::new(err)
						.color(Color32::from_rgba_unmultiplied(235, 235, 245, 235))
						.monospace(),
				);
			} else {
				Self::render_hud_content(ui, state, monitor, cursor, show_alt_hint_keycap);
			}
		});
		let pill_rect = inner.response.rect;

		*hud_pill_out =
			Some(HudPillGeometry { rect: pill_rect, radius_points: f32::from(pill_radius) });

		let inner_stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 44));
		let inner_rect = pill_rect.shrink(1.0);

		ui.painter().rect_stroke(
			inner_rect,
			CornerRadius::same(pill_radius.saturating_sub(1)),
			inner_stroke,
			egui::StrokeKind::Inside,
		);

		Self::render_loupe_tile(ui, state, monitor, cursor, pill_rect);
	}

	fn render_hud_content(
		ui: &mut Ui,
		state: &OverlayState,
		_monitor: MonitorRect,
		cursor: GlobalPoint,
		show_alt_hint_keycap: bool,
	) {
		let label_color = Color32::from_rgba_unmultiplied(235, 235, 245, 235);
		let secondary_color = Color32::from_rgba_unmultiplied(235, 235, 245, 150);
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
					egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 36)),
					egui::StrokeKind::Inside,
				);
				ui.label(egui::RichText::new(hex_text).color(label_color).monospace());
				ui.label(egui::RichText::new(rgb_text).color(secondary_color).monospace());

				if show_alt_hint_keycap {
					let keycap_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 18);
					let keycap_stroke =
						egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 30));

					Frame {
						fill: keycap_fill,
						stroke: keycap_stroke,
						corner_radius: CornerRadius::same(6),
						inner_margin: Margin::symmetric(6, 2),
						..Frame::default()
					}
					.show(ui, |ui| {
						ui.label(egui::RichText::new("Alt").color(secondary_color).monospace());
					});
				}
			});
		});
	}

	fn render_loupe_tile(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		pill_rect: Rect,
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
		let screen = ctx.screen_rect();
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
				let fill = Color32::from_rgba_unmultiplied(28, 28, 32, 156);
				let outer_stroke =
					egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 40));
				let shadow = egui::epaint::Shadow {
					offset: [0, 0],
					blur: 10,
					spread: 0,
					color: Color32::from_rgba_unmultiplied(0, 0, 0, 28),
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

					Self::render_loupe(ui, state, monitor, cursor);
				});
			});
	}

	fn render_loupe(ui: &mut Ui, state: &OverlayState, monitor: MonitorRect, cursor: GlobalPoint) {
		const CELL: f32 = 10.0;

		let mode = state.mode;

		if matches!(mode, OverlayMode::Live) {
			Self::render_live_loupe(ui, state, CELL);
		} else if matches!(mode, OverlayMode::Frozen)
			&& state.monitor == Some(monitor)
			&& state.frozen_image.is_some()
			&& state.cursor.is_some()
		{
			Self::render_frozen_loupe(ui, state, monitor, cursor, CELL);
		}
	}

	fn render_live_loupe(ui: &mut Ui, state: &OverlayState, cell: f32) {
		let fallback_side_px = 21_u32;
		let (w, h) = state
			.loupe
			.as_ref()
			.map(|loupe| loupe.patch.dimensions())
			.unwrap_or((fallback_side_px, fallback_side_px));
		let side = (w.max(h) as f32) * cell;
		let (rect, _) = ui.allocate_exact_size(Vec2::new(side, side), egui::Sense::hover());
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
					let fill = Color32::from_rgba_unmultiplied(
						pixel.0[0], pixel.0[1], pixel.0[2], pixel.0[3],
					);

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
			ui.painter().rect_filled(rect, 3.0, Color32::from_rgba_unmultiplied(255, 255, 255, 10));
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

	fn render_frozen_loupe(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		cell: f32,
	) {
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

			if hud_blur_active
				&& self.hud_pill.is_some()
				&& let Some(bg) = &self.hud_bg
			{
				rpass.set_pipeline(&self.hud_blur_pipeline);
				rpass.set_bind_group(0, &bg.hud_blur_bind_group, &[]);
				rpass.draw(0..3, 0..1);
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
		let egui_renderer = Renderer::new(&gpu.device, surface_format, None, 1, false);
		let bg_sampler = Self::create_bg_sampler(gpu);
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
			hud_blur_pipeline,
			hud_blur_bind_group_layout,
			hud_blur_uniform,
			hud_bg: None,
			hud_bg_generation: 0,
			hud_pill: None,
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

	fn draw(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		show_hud_blur: bool,
	) -> Result<()> {
		self.apply_pending_reconfigure(gpu);

		let (size, pixels_per_point, raw_input) = self.prepare_egui_input(gpu);

		self.sync_hud_bg(gpu, state, monitor)?;

		let hud_blur_active = show_hud_blur
			&& self.hud_bg.is_some()
			&& match state.mode {
				OverlayMode::Live => state.live_bg_monitor == Some(monitor),
				OverlayMode::Frozen => state.monitor == Some(monitor),
			};
		let (full_output, hud_pill) = self.run_egui(
			raw_input,
			state,
			monitor,
			hud_anchor,
			show_alt_hint_keycap,
			hud_blur_active,
		);

		self.hud_pill = hud_pill;

		if hud_blur_active && self.hud_bg.is_some() {
			self.update_hud_blur_uniform(gpu, size, pixels_per_point);
		}

		self.sync_egui_textures(gpu, &full_output);

		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
		let screen_descriptor =
			ScreenDescriptor { size_in_pixels: [size.width, size.height], pixels_per_point };
		let frame = self.acquire_frame(gpu)?;

		self.render_frame(gpu, hud_blur_active, frame, &paint_jobs, &screen_descriptor)?;

		Ok(())
	}

	fn update_hud_blur_uniform(
		&mut self,
		gpu: &GpuContext,
		size: PhysicalSize<u32>,
		pixels_per_point: f32,
	) {
		if self.hud_bg.is_none() {
			return;
		}

		let Some(hud_pill) = self.hud_pill else {
			return;
		};
		let surface_w = size.width as f32;
		let surface_h = size.height as f32;

		if surface_w <= 0.0 || surface_h <= 0.0 {
			return;
		}

		let rect = hud_pill.rect;
		let rect_min_px = [rect.min.x * pixels_per_point, rect.min.y * pixels_per_point];
		let rect_size_px = [rect.width() * pixels_per_point, rect.height() * pixels_per_point];
		let radius_px = hud_pill.radius_points * pixels_per_point;
		let blur_radius_px = 5.5 * pixels_per_point;
		let edge_softness_px = 1.0 * pixels_per_point;

		fn srgb8_to_linear_f32(x: u8) -> f32 {
			let c = (x as f32) / 255.0;

			if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
		}

		let tint_a = (HUD_PILL_BODY_FILL_SRGBA8[3] as f32) / 255.0;
		let tint_rgba = [
			srgb8_to_linear_f32(HUD_PILL_BODY_FILL_SRGBA8[0]),
			srgb8_to_linear_f32(HUD_PILL_BODY_FILL_SRGBA8[1]),
			srgb8_to_linear_f32(HUD_PILL_BODY_FILL_SRGBA8[2]),
			tint_a,
		];
		let u = HudBlurUniformRaw {
			rect_min_size: [rect_min_px[0], rect_min_px[1], rect_size_px[0], rect_size_px[1]],
			radius_blur_soft: [radius_px, blur_radius_px, edge_softness_px, 0.0],
			surface_size_px: [surface_w, surface_h, 0.0, 0.0],
			tint_rgba,
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
				// Keep displaying the already-uploaded background even if the image bytes have
				// been moved elsewhere (e.g. to encode PNG on a worker thread).
				return Ok(());
			}

			return Ok(());
		}

		let Some(image) = target_image else {
			// We don't have an image yet for this generation (capture in progress).
			self.hud_bg = None;
			self.hud_bg_generation = target_generation;

			return Ok(());
		};
		let upload_image =
			downscale_for_gpu_upload(image, gpu.device.limits().max_texture_dimension_2d);
		let (width, height) = upload_image.dimensions();
		let max_side = gpu.device.limits().max_texture_dimension_2d;

		debug_assert!(width <= max_side && height <= max_side);

		let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("rsnap-frozen-bg texture"),
			size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8UnormSrgb,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
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

		self.hud_bg = Some(HudBg { _texture: texture, _view: view, hud_blur_bind_group });
		self.hud_bg_generation = target_generation;

		Ok(())
	}
}

struct HudBg {
	_texture: wgpu::Texture,
	_view: wgpu::TextureView,
	hud_blur_bind_group: BindGroup,
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
