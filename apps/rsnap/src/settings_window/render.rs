use std::time::Duration;

use color_eyre::eyre::{self, Result, WrapErr};
use egui_wgpu::ScreenDescriptor;
use wgpu::SurfaceTexture;
use wgpu::TextureFormat;
use wgpu::{Adapter, CompositeAlphaMode, Device, Queue, Surface, SurfaceCapabilities};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::settings::AppSettings;
use crate::settings_window::SettingsWindow;

impl SettingsWindow {
	pub fn draw(&mut self, settings: &mut AppSettings) -> Result<bool> {
		if self.last_redraw.elapsed().as_millis() > 1_500 {
			self.window.request_redraw();
		}

		self.last_redraw = std::time::Instant::now();

		let raw_input = self.egui_state.take_egui_input(&self.window);
		let mut settings_changed = false;
		let egui_ctx = self.egui_ctx.clone();
		let full_output = egui_ctx.run(raw_input, |ctx| {
			settings_changed = self.ui(ctx, settings);
		});

		if let Some(repaint_delay) = full_output
			.viewport_output
			.get(&egui::ViewportId::ROOT)
			.map(|viewport_output| viewport_output.repaint_delay)
			&& repaint_delay < Duration::from_secs(1)
			&& repaint_delay != Duration::MAX
		{
			self.window.request_redraw();
		}

		self.egui_state.handle_platform_output(&self.window, full_output.platform_output);

		for (id, delta) in &full_output.textures_delta.set {
			self.renderer.update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
		}
		for id in &full_output.textures_delta.free {
			self.renderer.free_texture(id);
		}

		let paint_jobs =
			self.egui_ctx.tessellate(full_output.shapes, self.window.scale_factor() as f32);
		let size = self.window.inner_size();
		let screen_descriptor = ScreenDescriptor {
			size_in_pixels: [size.width.max(1), size.height.max(1)],
			pixels_per_point: self.window.scale_factor() as f32,
		};
		let frame = self.acquire_frame()?;
		let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
		let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("rsnap-settings encoder"),
		});

		self.renderer.update_buffers(
			&self.gpu.device,
			&self.gpu.queue,
			&mut encoder,
			&paint_jobs,
			&screen_descriptor,
		);

		{
			let panel_fill = self.egui_ctx.style().visuals.panel_fill;
			let clear = wgpu::Color {
				r: f64::from(panel_fill.r()) / 255.0,
				g: f64::from(panel_fill.g()) / 255.0,
				b: f64::from(panel_fill.b()) / 255.0,
				a: f64::from(panel_fill.a()) / 255.0,
			};
			let rpass_desc = wgpu::RenderPassDescriptor {
				label: Some("rsnap-settings rpass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					depth_slice: None,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(clear),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
			};
			let mut rpass = encoder.begin_render_pass(&rpass_desc).forget_lifetime();

			self.renderer.render(&mut rpass, &paint_jobs, &screen_descriptor);
		}

		self.gpu.queue.submit(Some(encoder.finish()));
		frame.present();

		Ok(settings_changed)
	}

	fn acquire_frame(&mut self) -> Result<SurfaceTexture> {
		match self.surface.get_current_texture() {
			Ok(frame) => Ok(frame),
			Err(wgpu::SurfaceError::Outdated) => {
				self.reconfigure_surface();

				self.surface.get_current_texture().wrap_err("get_current_texture after reconfigure")
			},
			Err(wgpu::SurfaceError::Lost) => {
				self.recreate_surface().wrap_err("recreate surface")?;

				self.surface.get_current_texture().wrap_err("get_current_texture after recreate")
			},
			Err(err) => Err(eyre::eyre!("get_current_texture failed: {err:?}")),
		}
	}

	fn recreate_surface(&mut self) -> Result<()> {
		let surface = self
			.gpu
			.instance
			.create_surface(std::sync::Arc::clone(&self.window))
			.wrap_err("create_surface")?;

		self.surface = surface;

		self.reconfigure_surface();

		Ok(())
	}

	fn reconfigure_surface(&mut self) {
		let caps = self.surface.get_capabilities(&self.gpu.adapter);

		self.surface_config.present_mode = caps.present_modes[0];
		self.surface_config.alpha_mode = pick_surface_alpha(&caps);

		self.surface.configure(&self.gpu.device, &self.surface_config);
	}

	pub(super) fn resize(&mut self, size: PhysicalSize<u32>) {
		self.surface_config.width = size.width.max(1);
		self.surface_config.height = size.height.max(1);

		self.reconfigure_surface();
	}
}

pub(super) struct GpuContext {
	instance: wgpu::Instance,
	adapter: Adapter,
	pub(super) device: Device,
	queue: Queue,
}
impl GpuContext {
	pub(super) fn new_with_surface(
		window: std::sync::Arc<Window>,
	) -> Result<(Self, Surface<'static>, wgpu::SurfaceConfiguration)> {
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
		let surface =
			instance.create_surface(std::sync::Arc::clone(&window)).wrap_err("create_surface")?;
		let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::LowPower,
			compatible_surface: Some(&surface),
			force_fallback_adapter: false,
		}))
		.map_err(|err| eyre::eyre!("Failed to request GPU adapter: {err}"))?;
		let limits = adapter.limits();
		let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("rsnap-settings device"),
			required_features: wgpu::Features::empty(),
			required_limits: limits,
			experimental_features: wgpu::ExperimentalFeatures::default(),
			memory_hints: wgpu::MemoryHints::Performance,
			trace: wgpu::Trace::Off,
		}))
		.wrap_err("request_device")?;
		let caps = surface.get_capabilities(&adapter);
		let format = pick_surface_format(&caps);
		let alpha = pick_surface_alpha(&caps);
		let size = window.inner_size();
		let surface_config = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format,
			width: size.width.max(1),
			height: size.height.max(1),
			present_mode: caps.present_modes[0],
			alpha_mode: alpha,
			view_formats: vec![format],
			desired_maximum_frame_latency: 2,
		};

		surface.configure(&device, &surface_config);

		Ok((Self { instance, adapter, device, queue }, surface, surface_config))
	}
}

fn pick_surface_format(caps: &SurfaceCapabilities) -> TextureFormat {
	caps.formats
		.iter()
		.copied()
		.find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
		.or_else(|| {
			caps.formats.iter().copied().find(|f| {
				matches!(
					f,
					wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb
				)
			})
		})
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
