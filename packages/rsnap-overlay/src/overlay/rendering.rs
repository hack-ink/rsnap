#![allow(clippy::wildcard_imports)]

use super::*;

mod affordances;
mod hud_rendering;
mod hud_surface;
mod scroll_preview_window;

use self::hud_rendering::LiveLoupeTexture;
use self::hud_surface::{HudBg, HudBlurUniformRaw};
pub(super) use hud_surface::HudPillGeometry;
pub(super) use scroll_preview_window::ScrollPreviewWindow;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FrozenToolbarButtonStyle {
	pub(super) icon_color: Color32,
	pub(super) bg_color: Color32,
	pub(super) border_color: Option<Color32>,
}

pub(super) struct ScrollPreviewView {
	pub(super) paused: bool,
	pub(super) theme: HudTheme,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SelectionFlowGeometryCacheKey {
	rect_min_x_bits: u32,
	rect_min_y_bits: u32,
	rect_max_x_bits: u32,
	rect_max_y_bits: u32,
	corner_radius_bits: u32,
	seam_offset_bits: u32,
	sample_count: usize,
}
impl SelectionFlowGeometryCacheKey {
	const fn new(rect: Rect, corner_radius: f32, seam_offset: f32, sample_count: usize) -> Self {
		Self {
			rect_min_x_bits: rect.min.x.to_bits(),
			rect_min_y_bits: rect.min.y.to_bits(),
			rect_max_x_bits: rect.max.x.to_bits(),
			rect_max_y_bits: rect.max.y.to_bits(),
			corner_radius_bits: corner_radius.to_bits(),
			seam_offset_bits: seam_offset.to_bits(),
			sample_count,
		}
	}
}

#[derive(Debug, Default)]
pub(super) struct SelectionFlowGeometryCache {
	key: Option<SelectionFlowGeometryCacheKey>,
	samples: Vec<(Pos2, f32)>,
	normals: Vec<Vec2>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SelectionDashedBorderCacheKey {
	rect_min_x_bits: u32,
	rect_min_y_bits: u32,
	rect_max_x_bits: u32,
	rect_max_y_bits: u32,
	dash_length_bits: u32,
	gap_length_bits: u32,
}
impl SelectionDashedBorderCacheKey {
	const fn new(rect: Rect, dash_length: f32, gap_length: f32) -> Self {
		Self {
			rect_min_x_bits: rect.min.x.to_bits(),
			rect_min_y_bits: rect.min.y.to_bits(),
			rect_max_x_bits: rect.max.x.to_bits(),
			rect_max_y_bits: rect.max.y.to_bits(),
			dash_length_bits: dash_length.to_bits(),
			gap_length_bits: gap_length.to_bits(),
		}
	}
}

#[derive(Debug, Default)]
pub(super) struct SelectionDashedBorderCache {
	pub(super) key: Option<SelectionDashedBorderCacheKey>,
	pub(super) segments: Vec<[Pos2; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SelectionDashedBorderMetrics {
	pub(super) stroke_width: f32,
	pub(super) dash_length: f32,
	pub(super) gap_length: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SelectionSizeBadgePadding {
	left: f32,
	right: f32,
	top: f32,
	bottom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SelectionSizeBadgeLayout {
	pub(super) text_size: Vec2,
	pub(super) badge_size: Vec2,
	padding: SelectionSizeBadgePadding,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SelectionSizeBadgeTarget {
	pub(super) rect: Rect,
	pub(super) size_points: RectPoints,
}

pub(super) struct HudOverlayWindow {
	pub(super) window: Arc<winit::window::Window>,
	pub(super) renderer: WindowRenderer,
}

#[derive(Debug, Default)]
pub(super) struct HudRedrawSummary {
	pub(super) request_toolbar_redraw: Option<MonitorRect>,
	pub(super) renderer_draw_elapsed: Option<Duration>,
	pub(super) request_inner_size_elapsed: Option<Duration>,
	pub(super) position_update_elapsed: Option<Duration>,
	pub(super) resize_target: Option<(u32, u32)>,
	pub(super) redraw_window_id: Option<WindowId>,
	pub(super) redraw_monitor_id: Option<u32>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StartupLiveRgbPlan {
	pub(super) focus_window: bool,
	pub(super) seed_monitor: Option<MonitorRect>,
}

#[derive(Debug, Default)]
struct WindowRendererPhaseTimings {
	prepare_input: Duration,
	sync_hud_bg: Duration,
	run_egui: Duration,
	update_hud_blur_uniform: Duration,
	sync_egui_textures: Duration,
	tessellate: Duration,
	acquire_frame: Duration,
	render_frame: Duration,
	total: Duration,
}
impl WindowRendererPhaseTimings {
	fn trace(
		&self,
		path: WindowRendererPath,
		window_id: WindowId,
		monitor_id: u32,
		mode: OverlayMode,
		toolbar_active: bool,
		paint_jobs: usize,
	) {
		tracing::trace!(
			op = "overlay.window_renderer_phase_timing",
			path = path.as_str(),
			window_id = ?window_id,
			monitor_id,
			mode = ?mode,
			toolbar_active,
			paint_jobs,
			total_us = self.total.as_micros(),
			prepare_input_us = self.prepare_input.as_micros(),
			sync_hud_bg_us = self.sync_hud_bg.as_micros(),
			run_egui_us = self.run_egui.as_micros(),
			update_hud_blur_uniform_us = self.update_hud_blur_uniform.as_micros(),
			sync_egui_textures_us = self.sync_egui_textures.as_micros(),
			tessellate_us = self.tessellate.as_micros(),
			acquire_frame_us = self.acquire_frame.as_micros(),
			render_frame_us = self.render_frame.as_micros(),
			"Overlay window renderer phase timing."
		);
	}

	fn warn_if_substeps_slow(
		&self,
		slow_op_logger: &mut SlowOperationLogger,
		path: WindowRendererPath,
		window_id: WindowId,
		monitor_id: u32,
		mode: OverlayMode,
		paint_jobs: usize,
	) {
		let context = || {
			format!(
				"path={} window_id={window_id:?} monitor_id={monitor_id} mode={mode:?} paint_jobs={paint_jobs}",
				path.as_str()
			)
		};

		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.prepare_input",
			self.prepare_input,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.sync_hud_bg",
			self.sync_hud_bg,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.run_egui",
			self.run_egui,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.update_hud_blur_uniform",
			self.update_hud_blur_uniform,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.sync_egui_textures",
			self.sync_egui_textures,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.tessellate",
			self.tessellate,
			&context,
		);
	}

	fn warn_phase_if_slow<F>(
		&self,
		slow_op_logger: &mut SlowOperationLogger,
		op: &'static str,
		elapsed: Duration,
		describe: &F,
	) where
		F: Fn() -> String,
	{
		if elapsed.is_zero() {
			return;
		}

		slow_op_logger.warn_if_redraw_substep_slow(op, elapsed, self.total, describe);
	}
}

pub(super) struct OverlayWindow {
	pub(super) monitor: MonitorRect,
	pub(super) window: Arc<winit::window::Window>,
	pub(super) renderer: WindowRenderer,
	pub(super) refresh_rate_millihertz: Option<u32>,
}

pub(super) struct GpuContext {
	instance: wgpu::Instance,
	adapter: Adapter,
	device: Device,
	queue: Queue,
}
impl GpuContext {
	pub(super) fn new() -> Result<Self> {
		let instance = wgpu::Instance::default();
		let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: PowerPreference::LowPower,
			compatible_surface: None,
			force_fallback_adapter: false,
		}))
		.map_err(|err| eyre::eyre!("Failed to request GPU adapter: {err}"))?;
		let adapter_limits = adapter.limits();
		let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("rsnap-overlay device"),
			required_features: Features::empty(),
			// Use the adapter's actual limits. Using `downlevel_defaults()` caps max texture
			// size to 2048, which breaks on common HiDPI displays.
			required_limits: adapter_limits,
			experimental_features: ExperimentalFeatures::default(),
			memory_hints: MemoryHints::Performance,
			trace: Trace::Off,
		}))
		.wrap_err("Failed to create wgpu device")?;

		Ok(Self { instance, adapter, device, queue })
	}
}

pub(super) struct WindowRenderer {
	window: Arc<winit::window::Window>,
	surface: Surface<'static>,
	surface_config: wgpu::SurfaceConfiguration,
	needs_reconfigure: bool,
	egui_ctx: egui::Context,
	egui_renderer: Renderer,
	bg_sampler: Sampler,
	mipgen_pipeline: RenderPipeline,
	mipgen_surface_pipeline: RenderPipeline,
	mipgen_bind_group_layout: BindGroupLayout,
	hud_blur_pipeline: RenderPipeline,
	hud_blur_bind_group_layout: BindGroupLayout,
	hud_blur_uniform: Buffer,
	hud_bg: Option<HudBg>,
	hud_bg_generation: u64,
	pub(super) hud_pill: Option<HudPillGeometry>,
	pub(super) loupe_tile: Option<Rect>,
	live_loupe_texture: Option<LiveLoupeTexture>,
	hud_theme: Option<HudTheme>,
	egui_start_time: Instant,
	egui_last_frame_time: Instant,
	selection_flow_cache: SelectionFlowGeometryCache,
	selection_dashed_border_cache: SelectionDashedBorderCache,
	slow_op_logger: SlowOperationLogger,
	occluded_redraw_retry_until: Option<Instant>,
}
impl WindowRenderer {
	fn note_successful_frame_presented(&mut self) {
		self.occluded_redraw_retry_until = None;
	}

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
			source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("../mipgen.wgsl"))),
		});
		let bind_group_layout =
			gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("rsnap-mipgen bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: ShaderStages::FRAGMENT,
						ty: BindingType::Texture {
							multisampled: false,
							view_dimension: TextureViewDimension::D2,
							sample_type: TextureSampleType::Float { filterable: true },
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: ShaderStages::FRAGMENT,
						ty: BindingType::Sampler(SamplerBindingType::Filtering),
						count: None,
					},
				],
			});
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen pipeline layout"),
			bind_group_layouts: &[Some(&bind_group_layout)],
			immediate_size: 0,
		});
		let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-mipgen pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: wgpu::PrimitiveState {
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
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
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
		format: wgpu::TextureFormat,
		bind_group_layout: &BindGroupLayout,
	) -> RenderPipeline {
		let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rsnap-mipgen fullscreen shader"),
			source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("../mipgen.wgsl"))),
		});
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline layout"),
			bind_group_layouts: &[Some(bind_group_layout)],
			immediate_size: 0,
		});

		gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: wgpu::PrimitiveState {
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
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: None,
					write_mask: ColorWrites::ALL,
				})],
			}),
			multiview_mask: None,
			cache: None,
		})
	}

	fn generate_mipmaps(&self, gpu: &GpuContext, texture: &Texture, mip_level_count: u32) {
		if mip_level_count <= 1 {
			return;
		}

		let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
			let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
				label: Some("rsnap-mipgen bind group"),
				layout: &self.mipgen_bind_group_layout,
				entries: &[
					wgpu::BindGroupEntry {
						binding: 0,
						resource: BindingResource::TextureView(&src_view),
					},
					wgpu::BindGroupEntry {
						binding: 1,
						resource: BindingResource::Sampler(&self.bg_sampler),
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
						load: LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }),
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
		gpu.device.create_sampler(&wgpu::SamplerDescriptor {
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
		surface_format: wgpu::TextureFormat,
	) -> (RenderPipeline, BindGroupLayout) {
		let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("rsnap-hud-blur shader"),
			source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("../hud_blur.wgsl"))),
		});
		let bind_group_layout =
			gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("rsnap-hud-blur bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: ShaderStages::FRAGMENT,
						ty: BindingType::Texture {
							multisampled: false,
							view_dimension: TextureViewDimension::D2,
							sample_type: TextureSampleType::Float { filterable: true },
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: ShaderStages::FRAGMENT,
						ty: BindingType::Sampler(SamplerBindingType::Filtering),
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
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
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-hud-blur pipeline layout"),
			bind_group_layouts: &[Some(&bind_group_layout)],
			immediate_size: 0,
		});
		let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-hud-blur pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				buffers: &[],
			},
			primitive: wgpu::PrimitiveState {
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
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				compilation_options: PipelineCompilationOptions::default(),
				targets: &[Some(wgpu::ColorTargetState {
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

	fn apply_pending_reconfigure(&mut self, gpu: &GpuContext) {
		if self.needs_reconfigure {
			self.reconfigure(gpu);

			self.needs_reconfigure = false;
		}
	}

	fn prepare_egui_input(
		&mut self,
		gpu: &GpuContext,
		pointer_state: Option<FrozenToolbarPointerState>,
		pixels_per_point_override: Option<f32>,
	) -> (PhysicalSize<u32>, f32, egui::RawInput) {
		// egui animations depend on a monotonic time base. Without this, animation state can appear
		// to "snap" only after an input event (e.g. CursorMoved) triggers a new frame.
		let now = Instant::now();
		let elapsed = now.duration_since(self.egui_start_time).as_secs_f64().max(0.0);
		let predicted_dt =
			now.duration_since(self.egui_last_frame_time).as_secs_f32().clamp(0.0, 0.5);

		self.egui_last_frame_time = now;

		// Keep the wgpu surface configuration in sync with the OS-reported window size.
		//
		// On macOS we can observe transient mismatches where `surface_config` is smaller than the
		// actual window size (e.g. right after entering Frozen mode), which causes egui to build
		// a smaller `screen_rect` and results in UI elements appearing clipped/offset until a
		// later redraw or input event triggers a resize/reconfigure.
		let actual_size = self.window.inner_size();
		let desired_w = actual_size.width.max(1);
		let desired_h = actual_size.height.max(1);

		if self.surface_config.width != desired_w || self.surface_config.height != desired_h {
			tracing::debug!(
				window_id = ?self.window.id(),
				actual_size_px = ?actual_size,
				old_surface_px = ?(self.surface_config.width, self.surface_config.height),
				new_surface_px = ?(desired_w, desired_h),
				window_scale_factor = self.window.scale_factor(),
				pixels_per_point_override,
				"Reconfiguring wgpu surface to match window."
			);

			self.surface_config.width = desired_w;
			self.surface_config.height = desired_h;
			self.needs_reconfigure = false;

			self.reconfigure(gpu);
		}

		let size = PhysicalSize::new(self.surface_config.width, self.surface_config.height);
		let pixels_per_point = pixels_per_point_override
			.filter(|v| *v > 0.0)
			.unwrap_or_else(|| self.window.scale_factor() as f32);
		let screen_size_points =
			Vec2::new(size.width as f32 / pixels_per_point, size.height as f32 / pixels_per_point);
		let max_texture_side = gpu.device.limits().max_texture_dimension_2d as usize;

		self.egui_ctx.input_mut(|i| i.max_texture_side = max_texture_side);

		let mut raw_input = egui::RawInput {
			screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen_size_points)),
			focused: true,
			time: Some(elapsed),
			predicted_dt,
			..Default::default()
		};
		let mut events = Vec::new();

		raw_input.max_texture_side = Some(max_texture_side);

		if let Some(pointer) = pointer_state {
			events.push(Event::PointerMoved(pointer.cursor_local));

			if pointer.left_button_went_down {
				events.push(Event::PointerButton {
					pos: pointer.cursor_local,
					button: PointerButton::Primary,
					pressed: true,
					modifiers: egui::Modifiers::default(),
				});
			}
			if pointer.left_button_went_up {
				events.push(Event::PointerButton {
					pos: pointer.cursor_local,
					button: PointerButton::Primary,
					pressed: false,
					modifiers: egui::Modifiers::default(),
				});
			}
		}

		if !events.is_empty() {
			raw_input.events = events;
		}

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
		toolbar_placement: ToolbarPlacement,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme: HudTheme,
		selection_flow_enabled: bool,
		selection_flow_stroke_width_px: f32,
		needs_frozen_surface_bg: bool,
		show_frozen_capture_affordance: bool,
		frozen_capture_is_fullscreen_fallback: bool,
		frozen_toolbar_reserved_rect: Option<Rect>,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
		mut toolbar_state: Option<&mut FrozenToolbarState>,
		toolbar_pointer: Option<FrozenToolbarPointerState>,
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
		let mut _show_selection_affordance = false;
		let egui_ctx = self.egui_ctx.clone();
		let full_output = egui_ctx.run_ui(raw_input, |ui| {
			let ctx = ui.ctx();

			Self::render_frozen_toolbar_ui(
				ui.ctx(),
				state,
				monitor,
				theme,
				toolbar_placement,
				hud_blur_active,
				hud_opaque,
				hud_opacity,
				hud_milk_amount,
				hud_tint_hue,
				toolbar_state.as_deref_mut(),
				toolbar_pointer,
				&mut hud_pill,
			);

			if let Some((cursor, local_cursor)) = hud_data {
				let _ = show_hud_blur;

				self.render_hud(
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
					hud_tint_hue,
					theme,
					&mut hud_pill,
				);
			}

			if matches!(state.mode, OverlayMode::Live) && !can_draw_hud {
				let screen_rect = ctx.input(|i| i.viewport_rect());
				let layer = LayerId::new(
					Order::Foreground,
					Id::new(format!("live-capture-{}", monitor.id)),
				);
				let painter = ctx.layer_painter(layer);

				_show_selection_affordance |= Self::render_live_capture_affordances(
					ctx,
					&painter,
					state,
					monitor,
					screen_rect,
					theme,
					selection_flow_enabled,
					selection_flow_stroke_width_px,
					selection_flow_geometry_cache,
				);
			}
			if matches!(state.mode, OverlayMode::Frozen)
				&& (needs_frozen_surface_bg || show_frozen_capture_affordance)
				&& state.monitor == Some(monitor)
				&& state.frozen_capture_rect.is_some()
			{
				let screen_rect = ctx.input(|i| i.viewport_rect());

				_show_selection_affordance |= Self::render_frozen_capture_affordance(
					ctx,
					state,
					monitor,
					screen_rect,
					theme,
					frozen_toolbar_reserved_rect,
					frozen_capture_is_fullscreen_fallback,
					selection_flow_enabled,
					selection_flow_stroke_width_px,
					selection_flow_geometry_cache,
					selection_dashed_border_cache,
				);
			}
		});

		(full_output, hud_pill)
	}

	fn sync_egui_textures(&mut self, gpu: &GpuContext, full_output: &FullOutput) {
		for (id, image_delta) in &full_output.textures_delta.set {
			self.egui_renderer.update_texture(&gpu.device, &gpu.queue, *id, image_delta);
		}
		for id in &full_output.textures_delta.free {
			self.egui_renderer.free_texture(id);
		}
	}

	fn acquire_frame(&mut self, gpu: &GpuContext) -> Result<AcquiredSurfaceFrame> {
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
	fn render_frame(
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

	pub(super) fn new(
		gpu: &GpuContext,
		window: Arc<winit::window::Window>,
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

		let egui_ctx = egui::Context::default();
		let mut fonts = FontDefinitions::default();

		egui_phosphor::add_to_fonts(&mut fonts, Variant::Regular);

		let phosphor_fill = String::from("phosphor-fill");
		let proportional_fallback =
			fonts.families.get(&FontFamily::Proportional).and_then(|names| names.first()).cloned();

		fonts.font_data.insert(phosphor_fill.clone(), Variant::Fill.font_data().into());

		{
			let family =
				fonts.families.entry(FontFamily::Name(phosphor_fill.clone().into())).or_default();

			family.insert(0, phosphor_fill.clone());

			if let Some(fallback) = proportional_fallback
				&& !family.contains(&fallback)
			{
				family.push(fallback);
			}
		}

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

	pub(super) fn resize(&mut self, size: PhysicalSize<u32>) -> Result<()> {
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
			HudTheme::Dark => self.egui_ctx.set_visuals(Visuals::dark()),
			HudTheme::Light => self.egui_ctx.set_visuals(Visuals::light()),
		}

		self.hud_theme = Some(theme);
	}

	fn prepare_window_renderer_input(
		&mut self,
		gpu: &GpuContext,
		monitor: MonitorRect,
		toolbar_pointer: Option<FrozenToolbarPointerState>,
		theme_mode: ThemeMode,
		phase_timings: &mut WindowRendererPhaseTimings,
	) -> (HudTheme, PhysicalSize<u32>, f32, egui::RawInput) {
		self.apply_pending_reconfigure(gpu);

		let theme = hud_helpers::effective_hud_theme(theme_mode, self.window.theme());

		self.sync_egui_theme(theme);

		let prepare_input_started_at = Instant::now();
		let (size, pixels_per_point, raw_input) =
			self.prepare_egui_input(gpu, toolbar_pointer, Some(monitor.scale_factor()));

		phase_timings.prepare_input = prepare_input_started_at.elapsed();

		(theme, size, pixels_per_point, raw_input)
	}

	#[allow(clippy::too_many_arguments)]
	fn maybe_update_hud_blur_uniform(
		&mut self,
		gpu: &GpuContext,
		size: PhysicalSize<u32>,
		pixels_per_point: f32,
		theme: HudTheme,
		hud_shader_blur_active: bool,
		hud_fog_amount: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		phase_timings: &mut WindowRendererPhaseTimings,
	) {
		if !hud_shader_blur_active {
			return;
		}

		let update_hud_blur_uniform_started_at = Instant::now();

		self.update_hud_blur_uniform(
			gpu,
			size,
			pixels_per_point,
			theme,
			hud_fog_amount,
			hud_milk_amount,
			hud_tint_hue,
		);

		phase_timings.update_hud_blur_uniform = update_hud_blur_uniform_started_at.elapsed();
	}

	#[allow(clippy::too_many_arguments)]
	fn finish_window_renderer_draw(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		path: WindowRendererPath,
		monitor: MonitorRect,
		size: PhysicalSize<u32>,
		pixels_per_point: f32,
		draw_started_at: Instant,
		phase_timings: &mut WindowRendererPhaseTimings,
		paint_jobs: Vec<ClippedPrimitive>,
		draw_frozen_bg: bool,
		hud_shader_blur_active: bool,
		toolbar_active: bool,
	) -> Result<()> {
		let screen_descriptor =
			ScreenDescriptor { size_in_pixels: [size.width, size.height], pixels_per_point };
		let acquire_frame_started_at = Instant::now();
		let frame = self.acquire_frame(gpu)?;

		phase_timings.acquire_frame = acquire_frame_started_at.elapsed();

		let frame = match frame {
			AcquiredSurfaceFrame::Ready(frame) => frame,
			AcquiredSurfaceFrame::Skipped(reason) => {
				phase_timings.total = draw_started_at.elapsed();

				phase_timings.warn_if_substeps_slow(
					&mut self.slow_op_logger,
					path,
					self.window.id(),
					monitor.id,
					state.mode,
					paint_jobs.len(),
				);
				phase_timings.trace(
					path,
					self.window.id(),
					monitor.id,
					state.mode,
					toolbar_active,
					paint_jobs.len(),
				);

				tracing::trace!(
					path = path.as_str(),
					window_id = ?self.window.id(),
					monitor_id = monitor.id,
					reason = reason.as_str(),
					"Skipped overlay window frame acquisition."
				);

				if should_request_overlay_redraw_after_surface_skip(
					reason,
					Instant::now(),
					&mut self.occluded_redraw_retry_until,
				) {
					self.window.request_redraw();
				}

				return Ok(());
			},
		};
		let render_frame_started_at = Instant::now();

		self.render_frame(
			gpu,
			draw_frozen_bg,
			hud_shader_blur_active,
			frame,
			&paint_jobs,
			&screen_descriptor,
		)?;
		self.note_successful_frame_presented();

		phase_timings.render_frame = render_frame_started_at.elapsed();
		phase_timings.total = draw_started_at.elapsed();

		phase_timings.warn_if_substeps_slow(
			&mut self.slow_op_logger,
			path,
			self.window.id(),
			monitor.id,
			state.mode,
			paint_jobs.len(),
		);
		phase_timings.trace(
			path,
			self.window.id(),
			monitor.id,
			state.mode,
			toolbar_active,
			paint_jobs.len(),
		);

		Ok(())
	}

	#[allow(clippy::too_many_arguments)]
	pub(super) fn draw(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		draw_hud: bool,
		hud_local_cursor_override: Option<Pos2>,
		hud_compact: bool,
		hud_anchor: HudAnchor,
		toolbar_placement: ToolbarPlacement,
		show_alt_hint_keycap: bool,
		show_hud_blur: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_fog_amount: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme_mode: ThemeMode,
		selection_flow_enabled: bool,
		selection_flow_stroke_width_px: f32,
		allow_frozen_surface_bg: bool,
		show_frozen_capture_affordance: bool,
		frozen_capture_is_fullscreen_fallback: bool,
		frozen_toolbar_reserved_rect: Option<Rect>,
		toolbar_state: Option<&mut FrozenToolbarState>,
		toolbar_pointer: Option<FrozenToolbarPointerState>,
	) -> Result<()> {
		let draw_started_at = Instant::now();
		let mut phase_timings = WindowRendererPhaseTimings::default();
		let (theme, size, pixels_per_point, raw_input) = self.prepare_window_renderer_input(
			gpu,
			monitor,
			toolbar_pointer,
			theme_mode,
			&mut phase_timings,
		);
		let toolbar_active = toolbar_state.is_some();

		self.trace_frozen_frame_metrics(state, monitor, size, pixels_per_point, toolbar_active);

		self.loupe_tile = None;

		let hud_cfg = Self::resolve_hud_draw_config(
			state,
			monitor,
			draw_hud,
			allow_frozen_surface_bg,
			toolbar_active,
			show_hud_blur,
			hud_opaque,
		);
		let sync_hud_bg_started_at = Instant::now();

		self.sync_or_clear_hud_bg(gpu, state, monitor, hud_cfg)?;

		phase_timings.sync_hud_bg = sync_hud_bg_started_at.elapsed();

		let hud_shader_blur_active = self.hud_shader_blur_active(state, monitor, hud_cfg);
		let mut selection_flow_cache = mem::take(&mut self.selection_flow_cache);
		let mut selection_dashed_border_cache = mem::take(&mut self.selection_dashed_border_cache);
		let run_egui_started_at = Instant::now();
		let (full_output, hud_pill) = self.run_egui(
			raw_input,
			state,
			monitor,
			hud_cfg.can_draw_hud,
			hud_local_cursor_override,
			hud_compact,
			show_hud_blur,
			hud_anchor,
			toolbar_placement,
			show_alt_hint_keycap,
			hud_cfg.hud_glass_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
			theme,
			selection_flow_enabled,
			selection_flow_stroke_width_px,
			hud_cfg.needs_frozen_surface_bg,
			show_frozen_capture_affordance,
			frozen_capture_is_fullscreen_fallback,
			frozen_toolbar_reserved_rect,
			&mut selection_flow_cache,
			&mut selection_dashed_border_cache,
			toolbar_state,
			toolbar_pointer,
		);

		phase_timings.run_egui = run_egui_started_at.elapsed();
		self.selection_flow_cache = selection_flow_cache;
		self.selection_dashed_border_cache = selection_dashed_border_cache;
		self.hud_pill = hud_pill;

		self.maybe_update_hud_blur_uniform(
			gpu,
			size,
			pixels_per_point,
			theme,
			hud_shader_blur_active,
			hud_fog_amount,
			hud_milk_amount,
			hud_tint_hue,
			&mut phase_timings,
		);

		let sync_egui_textures_started_at = Instant::now();

		self.sync_egui_textures(gpu, &full_output);

		phase_timings.sync_egui_textures = sync_egui_textures_started_at.elapsed();

		let tessellate_started_at = Instant::now();
		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);

		phase_timings.tessellate = tessellate_started_at.elapsed();

		let draw_frozen_bg = hud_cfg.needs_frozen_surface_bg
			&& state.monitor == Some(monitor)
			&& state.frozen_image.is_some();

		self.finish_window_renderer_draw(
			gpu,
			state,
			WindowRendererPath::Overlay,
			monitor,
			size,
			pixels_per_point,
			draw_started_at,
			&mut phase_timings,
			paint_jobs,
			draw_frozen_bg,
			hud_shader_blur_active,
			toolbar_active,
		)
	}
}
