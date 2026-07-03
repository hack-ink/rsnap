use std::time::Duration;
use std::time::Instant;

use egui::Context;
use egui::FullOutput;
use egui_wgpu::RendererOptions;
use wgpu::BindGroupDescriptor;
use wgpu::BindGroupEntry;
use wgpu::BindGroupLayoutDescriptor;
use wgpu::BindGroupLayoutEntry;
use wgpu::BufferDescriptor;
use wgpu::BufferUsages;
use wgpu::Color;
use wgpu::ColorTargetState;
use wgpu::CommandEncoderDescriptor;
use wgpu::FragmentState;
use wgpu::LoadOp;
use wgpu::Operations;
use wgpu::PipelineLayoutDescriptor;
use wgpu::PrimitiveState;
use wgpu::RenderPassColorAttachment;
use wgpu::RenderPassDescriptor;
use wgpu::RenderPipelineDescriptor;
use wgpu::SamplerDescriptor;
use wgpu::ShaderModuleDescriptor;
use wgpu::TextureFormat;
use wgpu::VertexState;
use winit::window::Window;

use crate::overlay::rendering::hud_surface::HudBlurUniformRaw;
use crate::overlay::rendering::{
	self, GpuContext, SelectionDashedBorderCache, SelectionFlowGeometryCache, WindowRenderer,
};
use crate::overlay::runtime_model::SurfaceFrameSkipReason;
use crate::overlay::runtime_timing::SLOW_OP_WARN_RENDER;
use crate::overlay::{
	AcquiredSurfaceFrame, AddressMode, Arc, BindGroupLayout, BindingResource, BindingType,
	BlendState, BufferBindingType, BufferSize, ClippedPrimitive, ColorWrites, CompositeAlphaMode,
	Cow, CurrentSurfaceTexture, FilterMode, FontDefinitions, FrontFace, MipmapFilterMode,
	MultisampleState, Mutex, PhysicalSize, PipelineCompilationOptions, PolygonMode, PresentMode,
	PrimitiveTopology, RenderPipeline, Renderer, Result, Sampler, SamplerBindingType,
	ScreenDescriptor, ShaderSource, ShaderStages, SlowOperationLogger, StoreOp,
	SurfaceCapabilities, SurfaceTexture, Texture, TextureAspect, TextureSampleType, TextureUsages,
	TextureViewDescriptor, TextureViewDimension, WrapErr, eyre, mem,
};

impl WindowRenderer {
	pub(in crate::overlay) fn new(
		gpu: &GpuContext,
		window: Arc<Window>,
		egui_repaint_deadline: Arc<Mutex<Option<Instant>>>,
	) -> Result<Self> {
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

		let egui_ctx = Context::default();
		let mut fonts = FontDefinitions::default();

		rendering::configure_egui_fonts(&mut fonts);

		egui_ctx.set_fonts(fonts);

		let repaint_deadline = Arc::clone(&egui_repaint_deadline);

		egui_ctx.set_request_repaint_callback(move |info| {
			let deadline = Instant::now() + info.delay;
			let mut next_repaint = repaint_deadline.lock().unwrap_or_else(|err| err.into_inner());
			let needs_update = next_repaint.is_none_or(|previous| deadline < previous);

			if needs_update {
				*next_repaint = Some(deadline);
			}
		});

		let egui_renderer = Renderer::new(
			&gpu.device,
			surface_format,
			RendererOptions {
				msaa_samples: 1,
				depth_stencil_format: None,
				dithering: false,
				predictable_texture_filtering: false,
			},
		);
		let bg_sampler = Self::create_bg_sampler(gpu);
		let (mipgen_pipeline, mipgen_bind_group_layout) =
			Self::create_mipgen_pipeline(gpu, TextureFormat::Rgba8UnormSrgb);
		let mipgen_surface_pipeline =
			Self::create_mipgen_surface_pipeline(gpu, surface_format, &mipgen_bind_group_layout);
		let (hud_blur_pipeline, hud_blur_bind_group_layout) =
			Self::create_hud_blur_pipeline(gpu, surface_format);
		let hud_blur_uniform = gpu.device.create_buffer(&BufferDescriptor {
			label: Some("rsnap-hud-blur uniform"),
			size: mem::size_of::<HudBlurUniformRaw>() as u64,
			usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let now = Instant::now();

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
			live_loupe_texture: None,
			hud_theme: None,
			egui_start_time: now,
			egui_last_frame_time: now,
			selection_flow_cache: SelectionFlowGeometryCache::default(),
			selection_dashed_border_cache: SelectionDashedBorderCache::default(),
			slow_op_logger: SlowOperationLogger::default(),
			occluded_redraw_retry_until: None,
		})
	}

	pub(in crate::overlay) fn resize(&mut self, size: PhysicalSize<u32>) -> Result<()> {
		self.surface_config.width = size.width.max(1);
		self.surface_config.height = size.height.max(1);
		self.needs_reconfigure = true;

		Ok(())
	}

	pub(in crate::overlay::rendering) fn note_successful_frame_presented(&mut self) {
		self.occluded_redraw_retry_until = None;
	}

	pub(in crate::overlay::rendering) fn mip_level_count(width: u32, height: u32) -> u32 {
		let max_dim = width.max(height).max(1);

		(32_u32.saturating_sub(max_dim.leading_zeros())).max(1)
	}

	fn create_mipgen_pipeline(
		gpu: &GpuContext,
		format: TextureFormat,
	) -> (RenderPipeline, BindGroupLayout) {
		let shader = gpu.device.create_shader_module(ShaderModuleDescriptor {
			label: Some("rsnap-mipgen shader"),
			source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("../../mipgen.wgsl"))),
		});
		let bind_group_layout = gpu.device.create_bind_group_layout(&BindGroupLayoutDescriptor {
			label: Some("rsnap-mipgen bgl"),
			entries: &[
				BindGroupLayoutEntry {
					binding: 0,
					visibility: ShaderStages::FRAGMENT,
					ty: BindingType::Texture {
						multisampled: false,
						view_dimension: TextureViewDimension::D2,
						sample_type: TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				BindGroupLayoutEntry {
					binding: 1,
					visibility: ShaderStages::FRAGMENT,
					ty: BindingType::Sampler(SamplerBindingType::Filtering),
					count: None,
				},
			],
		});
		let pipeline_layout = gpu.device.create_pipeline_layout(&PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen pipeline layout"),
			bind_group_layouts: &[Some(&bind_group_layout)],
			immediate_size: 0,
		});
		let pipeline = gpu.device.create_render_pipeline(&RenderPipelineDescriptor {
			label: Some("rsnap-mipgen pipeline"),
			layout: Some(&pipeline_layout),
			vertex: VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: PrimitiveState {
				topology: PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: FrontFace::Ccw,
				cull_mode: None,
				polygon_mode: PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: MultisampleState::default(),
			fragment: Some(FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				targets: &[Some(ColorTargetState {
					format,
					blend: None,
					write_mask: ColorWrites::ALL,
				})],
			}),
			multiview_mask: None,
			cache: None,
		});

		(pipeline, bind_group_layout)
	}

	fn create_mipgen_surface_pipeline(
		gpu: &GpuContext,
		format: TextureFormat,
		bind_group_layout: &BindGroupLayout,
	) -> RenderPipeline {
		let shader = gpu.device.create_shader_module(ShaderModuleDescriptor {
			label: Some("rsnap-mipgen fullscreen shader"),
			source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("../../mipgen.wgsl"))),
		});
		let pipeline_layout = gpu.device.create_pipeline_layout(&PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline layout"),
			bind_group_layouts: &[Some(bind_group_layout)],
			immediate_size: 0,
		});

		gpu.device.create_render_pipeline(&RenderPipelineDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline"),
			layout: Some(&pipeline_layout),
			vertex: VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: PrimitiveState {
				topology: PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: FrontFace::Ccw,
				cull_mode: None,
				polygon_mode: PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: MultisampleState::default(),
			fragment: Some(FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				targets: &[Some(ColorTargetState {
					format,
					blend: None,
					write_mask: ColorWrites::ALL,
				})],
			}),
			multiview_mask: None,
			cache: None,
		})
	}

	pub(in crate::overlay::rendering) fn generate_mipmaps(
		&self,
		gpu: &GpuContext,
		texture: &Texture,
		mip_level_count: u32,
	) {
		if mip_level_count <= 1 {
			return;
		}

		let mut encoder = gpu.device.create_command_encoder(&CommandEncoderDescriptor {
			label: Some("rsnap-mipgen encoder"),
		});

		for level in 1..mip_level_count {
			let src_view = texture.create_view(&TextureViewDescriptor {
				label: Some("rsnap-mipgen src view"),
				format: None,
				dimension: None,
				usage: None,
				aspect: TextureAspect::All,
				base_mip_level: level - 1,
				mip_level_count: Some(1),
				base_array_layer: 0,
				array_layer_count: Some(1),
			});
			let dst_view = texture.create_view(&TextureViewDescriptor {
				label: Some("rsnap-mipgen dst view"),
				format: None,
				dimension: None,
				usage: None,
				aspect: TextureAspect::All,
				base_mip_level: level,
				mip_level_count: Some(1),
				base_array_layer: 0,
				array_layer_count: Some(1),
			});
			let bind_group = gpu.device.create_bind_group(&BindGroupDescriptor {
				label: Some("rsnap-mipgen bind group"),
				layout: &self.mipgen_bind_group_layout,
				entries: &[
					BindGroupEntry {
						binding: 0,
						resource: BindingResource::TextureView(&src_view),
					},
					BindGroupEntry {
						binding: 1,
						resource: BindingResource::Sampler(&self.bg_sampler),
					},
				],
			});
			let rpass_desc = RenderPassDescriptor {
				label: Some("rsnap-mipgen pass"),
				color_attachments: &[Some(RenderPassColorAttachment {
					view: &dst_view,
					depth_slice: None,
					resolve_target: None,
					ops: Operations {
						load: LoadOp::Clear(Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }),
						store: StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			};
			let mut rpass = encoder.begin_render_pass(&rpass_desc).forget_lifetime();

			rpass.set_pipeline(&self.mipgen_pipeline);
			rpass.set_bind_group(0, &bind_group, &[]);
			rpass.draw(0..3, 0..1);
		}

		gpu.queue.submit(Some(encoder.finish()));
	}

	pub(in crate::overlay::rendering) fn pick_surface_format(
		caps: &SurfaceCapabilities,
	) -> TextureFormat {
		caps.formats
			.iter()
			.copied()
			.find(|f| matches!(f, TextureFormat::Bgra8UnormSrgb | TextureFormat::Rgba8UnormSrgb))
			.or_else(|| caps.formats.iter().copied().find(TextureFormat::is_srgb))
			.unwrap_or(caps.formats[0])
	}

	pub(in crate::overlay::rendering) fn pick_surface_alpha(
		caps: &SurfaceCapabilities,
	) -> CompositeAlphaMode {
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

	pub(in crate::overlay::rendering) fn make_surface_config(
		window: &Window,
		format: TextureFormat,
		alpha_mode: CompositeAlphaMode,
	) -> wgpu::SurfaceConfiguration {
		let size = window.inner_size();

		wgpu::SurfaceConfiguration {
			usage: TextureUsages::RENDER_ATTACHMENT,
			format,
			width: size.width.max(1),
			height: size.height.max(1),
			present_mode: PresentMode::Fifo,
			alpha_mode,
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		}
	}

	fn create_bg_sampler(gpu: &GpuContext) -> Sampler {
		gpu.device.create_sampler(&SamplerDescriptor {
			label: Some("rsnap-frozen-bg sampler"),
			address_mode_u: AddressMode::ClampToEdge,
			address_mode_v: AddressMode::ClampToEdge,
			address_mode_w: AddressMode::ClampToEdge,
			mag_filter: FilterMode::Linear,
			min_filter: FilterMode::Linear,
			mipmap_filter: MipmapFilterMode::Linear,
			..Default::default()
		})
	}

	fn create_hud_blur_pipeline(
		gpu: &GpuContext,
		surface_format: TextureFormat,
	) -> (RenderPipeline, BindGroupLayout) {
		let shader = gpu.device.create_shader_module(ShaderModuleDescriptor {
			label: Some("rsnap-hud-blur shader"),
			source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("../../hud_blur.wgsl"))),
		});
		let bind_group_layout = gpu.device.create_bind_group_layout(&BindGroupLayoutDescriptor {
			label: Some("rsnap-hud-blur bgl"),
			entries: &[
				BindGroupLayoutEntry {
					binding: 0,
					visibility: ShaderStages::FRAGMENT,
					ty: BindingType::Texture {
						multisampled: false,
						view_dimension: TextureViewDimension::D2,
						sample_type: TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				BindGroupLayoutEntry {
					binding: 1,
					visibility: ShaderStages::FRAGMENT,
					ty: BindingType::Sampler(SamplerBindingType::Filtering),
					count: None,
				},
				BindGroupLayoutEntry {
					binding: 2,
					visibility: ShaderStages::FRAGMENT,
					ty: BindingType::Buffer {
						ty: BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: BufferSize::new(
							mem::size_of::<HudBlurUniformRaw>() as u64
						),
					},
					count: None,
				},
			],
		});
		let pipeline_layout = gpu.device.create_pipeline_layout(&PipelineLayoutDescriptor {
			label: Some("rsnap-hud-blur pipeline layout"),
			bind_group_layouts: &[Some(&bind_group_layout)],
			immediate_size: 0,
		});
		let pipeline = gpu.device.create_render_pipeline(&RenderPipelineDescriptor {
			label: Some("rsnap-hud-blur pipeline"),
			layout: Some(&pipeline_layout),
			vertex: VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: PrimitiveState {
				topology: PrimitiveTopology::TriangleList,
				strip_index_format: None,
				front_face: FrontFace::Ccw,
				cull_mode: None,
				polygon_mode: PolygonMode::Fill,
				unclipped_depth: false,
				conservative: false,
			},
			depth_stencil: None,
			multisample: MultisampleState::default(),
			fragment: Some(FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				targets: &[Some(ColorTargetState {
					format: surface_format,
					blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
					write_mask: ColorWrites::ALL,
				})],
			}),
			multiview_mask: None,
			cache: None,
		});

		(pipeline, bind_group_layout)
	}

	pub(in crate::overlay::rendering) fn apply_pending_reconfigure(&mut self, gpu: &GpuContext) {
		if self.needs_reconfigure {
			self.reconfigure(gpu);

			self.needs_reconfigure = false;
		}
	}

	pub(in crate::overlay::rendering) fn sync_egui_textures(
		&mut self,
		gpu: &GpuContext,
		full_output: &FullOutput,
	) {
		for (id, image_delta) in &full_output.textures_delta.set {
			self.egui_renderer.update_texture(&gpu.device, &gpu.queue, *id, image_delta);
		}
		for id in &full_output.textures_delta.free {
			self.egui_renderer.free_texture(id);
		}
	}

	pub(in crate::overlay::rendering) fn sync_egui_textures_with_timing(
		&mut self,
		gpu: &GpuContext,
		full_output: &FullOutput,
	) -> Duration {
		let sync_egui_textures_started_at = Instant::now();

		self.sync_egui_textures(gpu, full_output);

		sync_egui_textures_started_at.elapsed()
	}

	pub(in crate::overlay::rendering) fn acquire_frame(
		&mut self,
		gpu: &GpuContext,
	) -> Result<AcquiredSurfaceFrame> {
		let started_at = Instant::now();
		let frame = {
			let mut acquired = None;

			for attempt in 0..2 {
				match self.surface.get_current_texture() {
					CurrentSurfaceTexture::Success(frame) => {
						acquired = Some(Ok(AcquiredSurfaceFrame::Ready(frame)));

						break;
					},
					CurrentSurfaceTexture::Suboptimal(frame) => {
						self.needs_reconfigure = true;
						acquired = Some(Ok(AcquiredSurfaceFrame::Ready(frame)));

						break;
					},
					CurrentSurfaceTexture::Outdated if attempt == 0 => {
						self.reconfigure(gpu);

						self.needs_reconfigure = false;
					},
					CurrentSurfaceTexture::Lost if attempt == 0 => {
						let surface = gpu
							.instance
							.create_surface(Arc::clone(&self.window))
							.wrap_err("Failed to recreate lost surface")?;

						self.surface = surface;

						self.reconfigure(gpu);

						self.needs_reconfigure = false;
					},
					CurrentSurfaceTexture::Outdated => {
						acquired = Some(Err(eyre::eyre!(
							"Failed to acquire surface texture after reconfigure: surface stayed outdated"
						)));

						break;
					},
					CurrentSurfaceTexture::Lost => {
						acquired = Some(Err(eyre::eyre!(
							"Failed to acquire surface texture after recreate: surface stayed lost"
						)));

						break;
					},
					CurrentSurfaceTexture::Timeout => {
						acquired = Some(Ok(AcquiredSurfaceFrame::Skipped(
							SurfaceFrameSkipReason::Timeout,
						)));

						break;
					},
					CurrentSurfaceTexture::Occluded => {
						acquired = Some(Ok(AcquiredSurfaceFrame::Skipped(
							SurfaceFrameSkipReason::Occluded,
						)));

						break;
					},
					CurrentSurfaceTexture::Validation => {
						acquired = Some(Err(eyre::eyre!(
							"Failed to acquire surface texture: validation error"
						)));

						break;
					},
				}
			}

			acquired.unwrap_or_else(|| {
				Err(eyre::eyre!(
					"Failed to acquire surface texture: bounded retries exhausted unexpectedly"
				))
			})
		};
		let elapsed = started_at.elapsed();

		self.slow_op_logger.warn_if_slow(
			"overlay.window_renderer_acquire_frame",
			elapsed,
			SLOW_OP_WARN_RENDER,
			|| format!("needs_reconfigure={}", self.needs_reconfigure),
		);

		frame
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay::rendering) fn render_frame(
		&mut self,
		gpu: &GpuContext,
		draw_frozen_bg: bool,
		hud_blur_active: bool,
		frame: SurfaceTexture,
		paint_jobs: &[ClippedPrimitive],
		screen_descriptor: &ScreenDescriptor,
	) -> Result<()> {
		let started_at = Instant::now();
		let view = frame.texture.create_view(&TextureViewDescriptor::default());
		let mut encoder = gpu.device.create_command_encoder(&CommandEncoderDescriptor {
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
			let rpass_desc = RenderPassDescriptor {
				label: Some("rsnap-overlay renderpass"),
				color_attachments: &[Some(RenderPassColorAttachment {
					view: &view,
					depth_slice: None,
					resolve_target: None,
					ops: Operations {
						load: LoadOp::Clear(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
						store: StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
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
		self.slow_op_logger.warn_if_slow(
			"overlay.window_renderer_render_frame",
			started_at.elapsed(),
			SLOW_OP_WARN_RENDER,
			|| {
				format!(
					"draw_frozen_bg={} hud_blur_active={} paint_jobs={}",
					draw_frozen_bg,
					hud_blur_active,
					paint_jobs.len()
				)
			},
		);

		Ok(())
	}

	pub(in crate::overlay::rendering) fn reconfigure(&mut self, gpu: &GpuContext) {
		self.surface.configure(&gpu.device, &self.surface_config);
	}
}
