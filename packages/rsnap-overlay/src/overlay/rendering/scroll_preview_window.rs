use wgpu::SurfaceConfiguration;

#[cfg(target_os = "macos")]
use crate::overlay;
use crate::overlay::rendering::{GpuContext, ScrollPreviewView, WindowRenderer};
use crate::overlay::{
	AcquiredSurfaceFrame, ActiveEventLoop, Align, Arc, CentralPanel, Color32, ColorImage,
	CornerRadius, CurrentSurfaceTexture, FontDefinitions, Frame, FullOutput, HudTheme, Layout,
	LoadOp, LogicalSize, Margin, PhysicalSize, Renderer, Result, RgbaImage,
	SCROLL_PREVIEW_WINDOW_HEIGHT_POINTS, SCROLL_PREVIEW_WINDOW_WIDTH_POINTS, ScreenDescriptor,
	StoreOp, Stroke, Surface, SurfaceFrameSkipReason, TextureHandle, TextureOptions,
	TextureViewDescriptor, Vec2, ViewportId, Visuals, WindowEvent, WindowLevel, WrapErr, eyre,
	image_helpers,
};

pub(in crate::overlay) struct ScrollPreviewWindow {
	pub(in crate::overlay) window: Arc<winit::window::Window>,
	surface: Surface<'static>,
	surface_config: SurfaceConfiguration,
	needs_reconfigure: bool,
	egui_ctx: egui::Context,
	egui_state: egui_winit::State,
	renderer: Renderer,
	preview_image: Option<ScrollPreviewStrip>,
}
impl ScrollPreviewWindow {
	pub(in crate::overlay) fn new(
		event_loop: &ActiveEventLoop,
		gpu: &GpuContext,
	) -> Result<Self, String> {
		let attrs = winit::window::Window::default_attributes()
			.with_title("rsnap-scroll-preview")
			.with_visible(false)
			.with_resizable(false)
			.with_decorations(false)
			.with_transparent(true)
			.with_inner_size(LogicalSize::new(
				SCROLL_PREVIEW_WINDOW_WIDTH_POINTS,
				SCROLL_PREVIEW_WINDOW_HEIGHT_POINTS,
			))
			.with_window_level(WindowLevel::AlwaysOnTop);
		let window = event_loop
			.create_window(attrs)
			.map_err(|err| format!("Unable to create scroll preview window: {err}"))?;
		let window = Arc::new(window);
		let surface = gpu
			.instance
			.create_surface(Arc::clone(&window))
			.map_err(|err| format!("wgpu create_surface failed: {err:#}"))?;
		let caps = surface.get_capabilities(&gpu.adapter);
		let surface_format = WindowRenderer::pick_surface_format(&caps);
		let surface_alpha = WindowRenderer::pick_surface_alpha(&caps);
		let surface_config =
			WindowRenderer::make_surface_config(window.as_ref(), surface_format, surface_alpha);
		let egui_ctx = egui::Context::default();
		let mut fonts = FontDefinitions::default();

		super::configure_egui_fonts(&mut fonts);

		egui_ctx.set_fonts(fonts);

		let egui_state = egui_winit::State::new(
			egui_ctx.clone(),
			ViewportId::ROOT,
			window.as_ref(),
			None,
			None,
			None,
		);
		let renderer = Renderer::new(
			&gpu.device,
			surface_config.format,
			egui_wgpu::RendererOptions {
				msaa_samples: 1,
				depth_stencil_format: None,
				dithering: false,
				predictable_texture_filtering: false,
			},
		);

		surface.configure(&gpu.device, &surface_config);

		let _ = window.set_cursor_hittest(false);

		#[cfg(target_os = "macos")]
		overlay::macos_configure_hud_window(window.as_ref(), false, 0.0, Some(18.0));

		Ok(Self {
			window,
			surface,
			surface_config,
			needs_reconfigure: false,
			egui_ctx,
			egui_state,
			renderer,
			preview_image: None,
		})
	}

	pub(in crate::overlay) fn handle_window_event(&mut self, event: &WindowEvent) {
		match event {
			WindowEvent::Resized(size) => self.resize(*size),
			WindowEvent::ScaleFactorChanged { .. } => self.resize(self.window.inner_size()),
			WindowEvent::ThemeChanged(_) => self.window.request_redraw(),
			_ => {},
		}

		let _ = self.egui_state.on_window_event(&self.window, event);

		self.window.request_redraw();
	}

	pub(in crate::overlay) fn sync_image(&mut self, image: Option<RgbaImage>) {
		let Some(image) = image else {
			self.preview_image = None;

			return;
		};
		let preview_image = image_helpers::resize_scroll_preview_segment(&image);
		let pixel_size = [preview_image.width() as usize, preview_image.height() as usize];
		let rgba = preview_image.as_raw().clone();
		let color_image = ColorImage::from_rgba_unmultiplied(pixel_size, &rgba);
		let ppp = self.window.scale_factor() as f32;
		let size_points =
			Vec2::new(preview_image.width() as f32 / ppp, preview_image.height() as f32 / ppp);

		match self.preview_image.as_mut() {
			Some(strip) if strip.pixel_size == pixel_size => {
				strip.texture.set(color_image, TextureOptions::LINEAR);

				strip.pixel_size = pixel_size;
				strip.rgba = rgba;
				strip.size_points = size_points;
			},
			_ => {
				let texture = self.egui_ctx.load_texture(
					String::from("scroll-preview-image"),
					color_image,
					TextureOptions::LINEAR,
				);

				self.preview_image =
					Some(ScrollPreviewStrip { texture, pixel_size, rgba, size_points });
			},
		}
	}

	fn render_preview_ui(&mut self, view: ScrollPreviewView) -> FullOutput {
		let raw_input = self.egui_state.take_egui_input(&self.window);

		self.egui_ctx.run_ui(raw_input, |ui| {
			CentralPanel::default().frame(Frame::new().fill(Color32::TRANSPARENT)).show_inside(
				ui,
				|ui| {
					let _ = view.paused;
					let tile_fill = match view.theme {
						HudTheme::Dark => Color32::from_rgba_unmultiplied(20, 22, 27, 228),
						HudTheme::Light => Color32::from_rgba_unmultiplied(244, 246, 249, 236),
					};
					let tile_stroke = match view.theme {
						HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 18),
						HudTheme::Light => Color32::from_rgba_unmultiplied(30, 36, 44, 22),
					};
					let tile_frame = Frame::new()
						.fill(tile_fill)
						.stroke(Stroke::new(1.0, tile_stroke))
						.corner_radius(CornerRadius::same(18))
						.inner_margin(Margin::symmetric(14, 14));

					tile_frame.show(ui, |ui| {
						ui.set_min_size(ui.available_size());

						if let Some(preview_image) = self.preview_image.as_ref() {
							let available = ui.available_size();
							let scale =
								(available.x / preview_image.size_points.x).clamp(0.05, 1.0);
							let draw_size = preview_image.size_points * scale;

							ui.with_layout(Layout::top_down(Align::Center), |ui| {
								ui.image((preview_image.texture.id(), draw_size));
							});
						} else {
							ui.allocate_space(ui.available_size());
						}
					});
				},
			);
		})
	}

	fn render_preview_frame(&mut self, gpu: &GpuContext, full_output: FullOutput) -> Result<()> {
		self.egui_state.handle_platform_output(&self.window, full_output.platform_output);

		for (id, delta) in &full_output.textures_delta.set {
			self.renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
		}
		for id in &full_output.textures_delta.free {
			self.renderer.free_texture(id);
		}

		let pixels_per_point = self.window.scale_factor() as f32;
		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
		let size = self.window.inner_size();
		let screen_descriptor = ScreenDescriptor {
			size_in_pixels: [size.width.max(1), size.height.max(1)],
			pixels_per_point,
		};
		let frame = match self.acquire_frame(gpu)? {
			AcquiredSurfaceFrame::Ready(frame) => frame,
			AcquiredSurfaceFrame::Skipped(reason) => {
				tracing::trace!(
					window_id = ?self.window.id(),
					reason = reason.as_str(),
					"Skipped scroll preview frame acquisition."
				);

				if reason.should_request_redraw() {
					self.window.request_redraw();
				}

				return Ok(());
			},
		};
		let view = frame.texture.create_view(&TextureViewDescriptor::default());
		let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("rsnap-scroll-preview encoder"),
		});
		let _ = self.renderer.update_buffers(
			&gpu.device,
			&gpu.queue,
			&mut encoder,
			&paint_jobs,
			&screen_descriptor,
		);

		{
			let rpass_desc = wgpu::RenderPassDescriptor {
				label: Some("rsnap-scroll-preview rpass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					depth_slice: None,
					resolve_target: None,
					ops: wgpu::Operations {
						load: LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
						store: StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			};
			let mut rpass = encoder.begin_render_pass(&rpass_desc).forget_lifetime();

			self.renderer.render(&mut rpass, &paint_jobs, &screen_descriptor);
		}

		gpu.queue.submit(Some(encoder.finish()));
		frame.present();

		Ok(())
	}

	pub(in crate::overlay) fn draw(
		&mut self,
		gpu: &GpuContext,
		theme: HudTheme,
		view: ScrollPreviewView,
	) -> Result<()> {
		self.sync_surface_to_window(gpu);

		if self.needs_reconfigure {
			self.reconfigure_surface(gpu);
		}

		match theme {
			HudTheme::Dark => self.egui_ctx.set_visuals(Visuals::dark()),
			HudTheme::Light => self.egui_ctx.set_visuals(Visuals::light()),
		}

		let full_output = self.render_preview_ui(view);

		self.render_preview_frame(gpu, full_output)
	}

	fn acquire_frame(&mut self, gpu: &GpuContext) -> Result<AcquiredSurfaceFrame> {
		for attempt in 0..2 {
			match self.surface.get_current_texture() {
				CurrentSurfaceTexture::Success(frame) => {
					return Ok(AcquiredSurfaceFrame::Ready(frame));
				},
				CurrentSurfaceTexture::Suboptimal(frame) => {
					self.needs_reconfigure = true;

					return Ok(AcquiredSurfaceFrame::Ready(frame));
				},
				CurrentSurfaceTexture::Outdated if attempt == 0 => {
					self.reconfigure_surface(gpu);
				},
				CurrentSurfaceTexture::Lost if attempt == 0 => {
					self.recreate_surface(gpu).wrap_err("recreate scroll preview surface")?;
				},
				CurrentSurfaceTexture::Outdated => {
					return Err(eyre::eyre!(
						"scroll preview get_current_texture stayed outdated after reconfigure"
					));
				},
				CurrentSurfaceTexture::Lost => {
					return Err(eyre::eyre!(
						"scroll preview get_current_texture stayed lost after recreate"
					));
				},
				CurrentSurfaceTexture::Timeout => {
					return Ok(AcquiredSurfaceFrame::Skipped(SurfaceFrameSkipReason::Timeout));
				},
				CurrentSurfaceTexture::Occluded => {
					return Ok(AcquiredSurfaceFrame::Skipped(SurfaceFrameSkipReason::Occluded));
				},
				CurrentSurfaceTexture::Validation => {
					return Err(eyre::eyre!("scroll preview get_current_texture hit validation"));
				},
			}
		}

		unreachable!("surface acquisition attempts are bounded")
	}

	fn recreate_surface(&mut self, gpu: &GpuContext) -> Result<()> {
		let surface = gpu
			.instance
			.create_surface(Arc::clone(&self.window))
			.wrap_err("create scroll preview surface")?;

		self.surface = surface;

		self.reconfigure_surface(gpu);

		Ok(())
	}

	fn reconfigure_surface(&mut self, gpu: &GpuContext) {
		self.surface.configure(&gpu.device, &self.surface_config);

		self.needs_reconfigure = false;
	}

	fn sync_surface_to_window(&mut self, gpu: &GpuContext) {
		let actual_size = self.window.inner_size();
		let desired_w = actual_size.width.max(1);
		let desired_h = actual_size.height.max(1);

		if self.surface_config.width == desired_w && self.surface_config.height == desired_h {
			return;
		}

		tracing::debug!(
			window_id = ?self.window.id(),
			actual_size_px = ?actual_size,
			old_surface_px = ?(self.surface_config.width, self.surface_config.height),
			new_surface_px = ?(desired_w, desired_h),
			window_scale_factor = self.window.scale_factor(),
			"Reconfiguring scroll preview surface to match window."
		);

		self.surface_config.width = desired_w;
		self.surface_config.height = desired_h;
		self.needs_reconfigure = false;

		self.reconfigure_surface(gpu);
	}

	pub(in crate::overlay) fn resize(&mut self, size: PhysicalSize<u32>) {
		self.surface_config.width = size.width.max(1);
		self.surface_config.height = size.height.max(1);
		self.needs_reconfigure = true;
	}
}

struct ScrollPreviewStrip {
	texture: TextureHandle,
	pixel_size: [usize; 2],
	rgba: Vec<u8>,
	size_points: Vec2,
}
