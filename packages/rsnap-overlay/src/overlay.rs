use std::{
	borrow::Cow,
	collections::HashMap,
	sync::Arc,
	time::{Duration, Instant},
};

use color_eyre::eyre::{Result, WrapErr};
use device_query::DeviceQuery;
use egui::{Color32, CornerRadius, Frame, Margin, Pos2, Rect, Vec2, ViewportId};
use egui_wgpu::{Renderer, ScreenDescriptor};
use image::RgbaImage;
use wgpu::SurfaceError;
use winit::{
	dpi::{LogicalPosition, LogicalSize, PhysicalSize},
	event::{ElementState, MouseButton, WindowEvent},
	event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
	keyboard::{Key, NamedKey},
	window::{CursorIcon, WindowId, WindowLevel},
};

use crate::{
	backend::default_capture_backend,
	png::rgba_image_to_png_bytes,
	state::{GlobalPoint, MonitorRect, OverlayMode, OverlayState, Rgb},
	worker::{OverlayWorker, WorkerResponse},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HudAnchor {
	Cursor,
}

#[derive(Clone, Debug)]
pub struct OverlayConfig {
	pub hud_anchor: HudAnchor,
}

impl Default for OverlayConfig {
	fn default() -> Self {
		Self { hud_anchor: HudAnchor::Cursor }
	}
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

		impl winit::application::ApplicationHandler<()> for Runner {
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
	pending_freeze_capture: Option<MonitorRect>,
}

impl OverlaySession {
	#[must_use]
	pub fn new() -> Self {
		Self::with_config(OverlayConfig::default())
	}

	#[must_use]
	pub fn with_config(config: OverlayConfig) -> Self {
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
			pending_freeze_capture: None,
		}
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
		self.worker = Some(OverlayWorker::new(default_capture_backend()));
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
				scale_factor_x1000: (monitor.scale_factor() * 1000.0).round() as u32,
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
			while let Some(resp) = worker.try_recv() {
				match resp {
					WorkerResponse::SampledRgb { monitor, point, rgb } => {
						if matches!(self.state.mode, OverlayMode::Live)
							&& self.state.cursor == Some(point)
						{
							self.state.rgb = rgb;
							self.request_redraw_for_monitor(monitor);
						}
					},
					WorkerResponse::CapturedFreeze { monitor, image } => {
						if matches!(self.state.mode, OverlayMode::Frozen)
							&& self.state.monitor == Some(monitor)
						{
							self.state.finish_freeze(monitor, image);
							self.request_redraw_for_monitor(monitor);
						}
					},
					WorkerResponse::Error(message) => {
						self.state.set_error(message);
						self.request_redraw_all();
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
			WindowEvent::CloseRequested => return self.exit(OverlayExit::Cancelled),
			WindowEvent::Resized(size) => {
				let Some(overlay_window) = self.windows.get_mut(&window_id) else {
					return OverlayControl::Continue;
				};
				if let Err(err) = overlay_window.renderer.resize(*size) {
					return self.exit(OverlayExit::Error(format!("{err:#}")));
				}
			},
			WindowEvent::ScaleFactorChanged { .. } => {
				let Some(overlay_window) = self.windows.get_mut(&window_id) else {
					return OverlayControl::Continue;
				};
				let size = overlay_window.window.inner_size();
				if let Err(err) = overlay_window.renderer.resize(size) {
					return self.exit(OverlayExit::Error(format!("{err:#}")));
				}
			},
			WindowEvent::CursorMoved { position, .. } => {
				let Some((monitor, sf)) =
					self.windows.get(&window_id).map(|w| (w.monitor, w.window.scale_factor()))
				else {
					return OverlayControl::Continue;
				};
				let local_x = (position.x / sf).round() as i32;
				let local_y = (position.y / sf).round() as i32;
				let global =
					GlobalPoint::new(monitor.origin.x + local_x, monitor.origin.y + local_y);
				self.update_cursor_state(monitor, global);

				if matches!(self.state.mode, OverlayMode::Live)
					&& self.last_rgb_request_at.elapsed() >= self.rgb_request_interval
					&& let Some(worker) = &self.worker
				{
					worker.try_sample_rgb(monitor, global);
					self.last_rgb_request_at = Instant::now();
				}

				self.request_redraw_for_monitor(monitor);
			},
			WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
				if *state == ElementState::Pressed && matches!(self.state.mode, OverlayMode::Live) {
					let Some(monitor) = self.windows.get(&window_id).map(|w| w.monitor) else {
						return OverlayControl::Continue;
					};
					self.state.clear_error();
					self.state.begin_freeze(monitor);
					self.pending_freeze_capture = Some(monitor);
					self.request_redraw_for_monitor(monitor);
				}
			},
			WindowEvent::KeyboardInput { event, .. } => {
				if event.state != ElementState::Pressed {
					return OverlayControl::Continue;
				}
				match event.logical_key {
					Key::Named(NamedKey::Escape) => return self.exit(OverlayExit::Cancelled),
					Key::Named(NamedKey::Space) => {
						if matches!(self.state.mode, OverlayMode::Frozen)
							&& let Some(image) = &self.state.frozen_image
						{
							match rgba_image_to_png_bytes(image) {
								Ok(png_bytes) => {
									if let Err(err) = write_png_bytes_to_clipboard(&png_bytes) {
										self.state.set_error(format!("{err:#}"));
										self.request_redraw_all();
										return OverlayControl::Continue;
									}
									return self.exit(OverlayExit::PngBytes(png_bytes));
								},
								Err(err) => {
									self.state.set_error(format!("{err:#}"));
									self.request_redraw_all();
									return OverlayControl::Continue;
								},
							}
						}
					},
					_ => {},
				}
			},
			WindowEvent::RedrawRequested => {
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
			},
			_ => {},
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
			OverlayMode::Live => {
				self.state.rgb = None;
			},
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
			return Err(color_eyre::eyre::eyre!("NSPasteboard setData:forType failed"));
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

struct OverlayWindow {
	monitor: MonitorRect,
	window: Arc<winit::window::Window>,
	renderer: WindowRenderer,
}

struct GpuContext {
	instance: wgpu::Instance,
	adapter: wgpu::Adapter,
	device: wgpu::Device,
	queue: wgpu::Queue,
}

impl GpuContext {
	fn new() -> Result<Self> {
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
		let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::LowPower,
			compatible_surface: None,
			force_fallback_adapter: false,
		}))
		.ok_or_else(|| color_eyre::eyre::eyre!("No suitable GPU adapters found"))?;

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
	surface: wgpu::Surface<'static>,
	surface_config: wgpu::SurfaceConfiguration,
	needs_reconfigure: bool,
	egui_ctx: egui::Context,
	egui_renderer: Renderer,
	bg_pipeline: wgpu::RenderPipeline,
	bg_bind_group_layout: wgpu::BindGroupLayout,
	bg_sampler: wgpu::Sampler,
	frozen_bg: Option<FrozenBg>,
	frozen_bg_generation: u64,
}

struct FrozenBg {
	_texture: wgpu::Texture,
	_view: wgpu::TextureView,
	bind_group: wgpu::BindGroup,
}

impl WindowRenderer {
	fn new(gpu: &GpuContext, window: Arc<winit::window::Window>) -> Result<Self> {
		let surface = gpu
			.instance
			.create_surface(Arc::clone(&window))
			.wrap_err("wgpu create_surface failed")?;

		let caps = surface.get_capabilities(&gpu.adapter);
		let surface_format = caps
			.formats
			.iter()
			.copied()
			.find(|f| {
				matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm)
			})
			.or_else(|| caps.formats.iter().copied().find(|f| !f.is_srgb()))
			.unwrap_or(caps.formats[0]);

		let surface_alpha = caps
			.alpha_modes
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
			.unwrap_or(caps.alpha_modes[0]);

		let size = window.inner_size();
		let surface_config = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format: surface_format,
			width: size.width.max(1),
			height: size.height.max(1),
			present_mode: wgpu::PresentMode::Fifo,
			alpha_mode: surface_alpha,
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		};
		surface.configure(&gpu.device, &surface_config);

		let egui_ctx = egui::Context::default();
		let egui_renderer = Renderer::new(&gpu.device, surface_format, None, 1, false);

		let bg_shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rsnap-frozen-bg shader"),
			source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("overlay_bg.wgsl"))),
		});

		let bg_bind_group_layout =
			gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("rsnap-frozen-bg bgl"),
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

		let bg_pipeline_layout =
			gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("rsnap-frozen-bg pipeline layout"),
				bind_group_layouts: &[&bg_bind_group_layout],
				push_constant_ranges: &[],
			});

		let bg_pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-frozen-bg pipeline"),
			layout: Some(&bg_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &bg_shader,
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
				module: &bg_shader,
				entry_point: Some("fs_main"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format: surface_format,
					blend: Some(wgpu::BlendState::REPLACE),
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			multiview: None,
			cache: None,
		});

		let bg_sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("rsnap-frozen-bg sampler"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			address_mode_w: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			mipmap_filter: wgpu::FilterMode::Nearest,
			..Default::default()
		});

		Ok(Self {
			window,
			surface,
			surface_config,
			needs_reconfigure: false,
			egui_ctx,
			egui_renderer,
			bg_pipeline,
			bg_bind_group_layout,
			bg_sampler,
			frozen_bg: None,
			frozen_bg_generation: 0,
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
	) -> Result<()> {
		if self.needs_reconfigure {
			self.reconfigure(gpu);
			self.needs_reconfigure = false;
		}

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

		self.sync_frozen_bg(gpu, state, monitor)?;

		let full_output = self.egui_ctx.run(raw_input, |ctx| {
			if matches!(state.mode, OverlayMode::Frozen)
				&& state.monitor == Some(monitor)
				&& state.frozen_image.is_none()
			{
				return;
			}

			let Some(cursor) = state.cursor else {
				return;
			};

			let Some(local_cursor) = global_to_local(cursor, monitor) else {
				return;
			};

			let (hud_x, hud_y) = match hud_anchor {
				HudAnchor::Cursor => (local_cursor.x + 14.0, local_cursor.y + 14.0),
			};

			let mut lines = Vec::new();
			if let Some(err) = &state.error_message {
				lines.push(err.clone());
			} else {
				lines.push(format!("x={}, y={}", cursor.x, cursor.y));
				lines.push(match state.rgb {
					Some(rgb) => format!("rgb({}, {}, {})", rgb.r, rgb.g, rgb.b),
					None => String::from("rgb(?, ?, ?)"),
				});
				lines.push(match state.mode {
					OverlayMode::Live => String::from("Click to freeze"),
					OverlayMode::Frozen => String::from("Space to copy PNG"),
				});
			}

			egui::Area::new("hud".into())
				.order(egui::Order::Foreground)
				.fixed_pos(Pos2::new(hud_x, hud_y))
				.show(ctx, |ui| {
					Frame {
						fill: Color32::from_rgba_unmultiplied(0, 0, 0, 200),
						corner_radius: CornerRadius::same(6),
						inner_margin: Margin::same(8),
						..Frame::default()
					}
					.show(ui, |ui| {
						ui.style_mut().visuals.override_text_color = Some(Color32::WHITE);
						ui.set_min_width(180.0);
						for line in lines {
							ui.label(line);
						}
					});
				});
		});

		for (id, image_delta) in &full_output.textures_delta.set {
			self.egui_renderer.update_texture(&gpu.device, &gpu.queue, *id, image_delta);
		}
		for id in &full_output.textures_delta.free {
			self.egui_renderer.free_texture(id);
		}

		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
		let screen_descriptor =
			ScreenDescriptor { size_in_pixels: [size.width, size.height], pixels_per_point };

		let frame = match self.surface.get_current_texture() {
			Ok(frame) => frame,
			Err(SurfaceError::Outdated | SurfaceError::Lost) => {
				self.reconfigure(gpu);
				self.needs_reconfigure = false;
				self.surface
					.get_current_texture()
					.wrap_err("Surface was lost and could not be reacquired")?
			},
			Err(err) => return Err(err).wrap_err("Failed to acquire surface texture"),
		};

		let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

		let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("rsnap-overlay encoder"),
		});

		let _user_cmds = self.egui_renderer.update_buffers(
			&gpu.device,
			&gpu.queue,
			&mut encoder,
			&paint_jobs,
			&screen_descriptor,
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
			if matches!(state.mode, OverlayMode::Frozen)
				&& state.monitor == Some(monitor)
				&& let Some(bg) = &self.frozen_bg
			{
				rpass.set_pipeline(&self.bg_pipeline);
				rpass.set_bind_group(0, &bg.bind_group, &[]);
				rpass.draw(0..3, 0..1);
			}
			self.egui_renderer.render(&mut rpass, &paint_jobs, &screen_descriptor);
		}

		gpu.queue.submit(Some(encoder.finish()));
		frame.present();

		Ok(())
	}

	fn sync_frozen_bg(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
	) -> Result<()> {
		let should_have_bg = matches!(state.mode, OverlayMode::Frozen)
			&& state.monitor == Some(monitor)
			&& state.frozen_image.is_some();

		if !should_have_bg {
			self.frozen_bg = None;
			self.frozen_bg_generation = state.frozen_generation;
			return Ok(());
		}

		if self.frozen_bg.is_some() && self.frozen_bg_generation == state.frozen_generation {
			return Ok(());
		}

		let Some(image) = state.frozen_image.as_ref() else {
			self.frozen_bg = None;
			self.frozen_bg_generation = state.frozen_generation;
			return Ok(());
		};

		let (width, height) = image.dimensions();
		let max_side = gpu.device.limits().max_texture_dimension_2d;
		if width > max_side || height > max_side {
			return Err(color_eyre::eyre::eyre!(
				"Frozen background image is too large for this GPU: {width}x{height} (max {max_side})"
			));
		}

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

		let bytes_per_pixel = 4usize;
		let unpadded_bytes_per_row = (width as usize) * bytes_per_pixel;
		let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
		let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

		let rgba_padded;
		let rgba_bytes: &[u8] = if padded_bytes_per_row == unpadded_bytes_per_row {
			image.as_raw()
		} else {
			let src = image.as_raw();
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

		let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("rsnap-frozen-bg bind group"),
			layout: &self.bg_bind_group_layout,
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

		self.frozen_bg = Some(FrozenBg { _texture: texture, _view: view, bind_group });
		self.frozen_bg_generation = state.frozen_generation;
		Ok(())
	}
}

fn pad_rows(src: &[u8], src_row_bytes: usize, dst_row_bytes: usize, rows: usize) -> Vec<u8> {
	debug_assert!(dst_row_bytes >= src_row_bytes);
	let mut out = vec![0u8; dst_row_bytes * rows];
	for y in 0..rows {
		let src_i = y * src_row_bytes;
		let dst_i = y * dst_row_bytes;
		out[dst_i..dst_i + src_row_bytes].copy_from_slice(&src[src_i..src_i + src_row_bytes]);
	}
	out
}

fn global_to_local(cursor: GlobalPoint, monitor: MonitorRect) -> Option<Pos2> {
	let (x, y) = monitor.local_u32(cursor)?;
	Some(Pos2::new(x as f32, y as f32))
}
