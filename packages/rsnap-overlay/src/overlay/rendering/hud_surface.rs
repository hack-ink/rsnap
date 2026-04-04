use egui::RawInput;
use wgpu::{BindGroup, TextureFormat};

#[allow(unused_imports)]
use crate::overlay::rendering::{GpuContext, WindowRenderer, WindowRendererPhaseTimings};
#[allow(unused_imports)]
use crate::overlay::{
	Area, BindingResource, Color32, CornerRadius, Frame, FullOutput, HudDrawConfig, HudTheme, Id,
	Instant, LOUPE_TILE_CORNER_RADIUS_POINTS, Margin, MonitorRect, Order, Origin3d, OverlayMode,
	OverlayState, PhysicalSize, Pos2, Rect, Result, Rgb, RgbaImage, Sampler, StoreOp, Stroke,
	StrokeKind, Texture, TextureAspect, TextureDimension, TextureUsages, TextureView,
	TextureViewDescriptor, ThemeMode, Vec2, WindowRendererPath, hud_helpers, image_helpers, mem,
	ptr, slice,
};

impl WindowRenderer {
	pub(in crate::overlay::rendering) fn trace_frozen_frame_metrics(
		&self,
		state: &OverlayState,
		monitor: MonitorRect,
		size: PhysicalSize<u32>,
		pixels_per_point: f32,
		toolbar_active: bool,
	) {
		if !matches!(state.mode, OverlayMode::Frozen) || state.monitor != Some(monitor) {
			return;
		}

		let screen_size_points =
			Vec2::new(size.width as f32 / pixels_per_point, size.height as f32 / pixels_per_point);

		tracing::trace!(
					window_id = ?self.window.id(),
					monitor_id = monitor.id,
					window_scale_factor = self.window.scale_factor(),
		monitor_scale_factor = monitor.scale_factor(),
					size_in_pixels = ?size,
					pixels_per_point,
					screen_size_points = ?screen_size_points,
					flip_y = false,
					frozen_generation = state.frozen_generation,
					frozen_image_ready = state.frozen_image.is_some(),
					toolbar_active,
					"Frozen frame metrics."
				);
	}

	pub(in crate::overlay::rendering) fn resolve_hud_draw_config(
		state: &OverlayState,
		monitor: MonitorRect,
		draw_hud: bool,
		allow_frozen_surface_bg: bool,
		toolbar_active: bool,
		show_hud_blur: bool,
		hud_opaque: bool,
	) -> HudDrawConfig {
		let can_draw_hud = draw_hud && Self::should_draw_hud(state, monitor);
		let needs_frozen_surface_bg =
			allow_frozen_surface_bg && !draw_hud && matches!(state.mode, OverlayMode::Frozen);
		// `show_hud_blur` is a UX toggle for "glass mode".
		// - On macOS: HUD uses native compositor blur; toolbar uses native HUD windowing, so shader
		//   blur stays tied to monitor-aligned overlay windows.
		// - On non-macOS: HUD and toolbar remain in overlay windows with shader blur paths.
		let hud_glass_active = can_draw_hud && show_hud_blur && !hud_opaque;
		let toolbar_glass_active = toolbar_active && show_hud_blur && !hud_opaque;
		let use_shader_blur_for_hud = !cfg!(target_os = "macos");
		let needs_shader_blur_bg =
			toolbar_glass_active || (hud_glass_active && use_shader_blur_for_hud);

		HudDrawConfig {
			can_draw_hud,
			needs_frozen_surface_bg,
			needs_shader_blur_bg,
			hud_glass_active,
		}
	}

	pub(in crate::overlay::rendering) fn sync_or_clear_hud_bg(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		hud_cfg: HudDrawConfig,
	) -> Result<()> {
		if hud_cfg.needs_frozen_surface_bg || hud_cfg.needs_shader_blur_bg {
			return self.sync_hud_bg(gpu, state, monitor);
		}

		self.hud_bg = None;
		self.hud_bg_generation = match state.mode {
			OverlayMode::Live => state.live_bg_generation,
			OverlayMode::Frozen => state.frozen_generation,
		};

		Ok(())
	}

	pub(in crate::overlay::rendering) fn hud_shader_blur_active(
		&self,
		state: &OverlayState,
		monitor: MonitorRect,
		hud_cfg: HudDrawConfig,
	) -> bool {
		hud_cfg.needs_shader_blur_bg
			&& self.hud_bg.is_some()
			&& match state.mode {
				OverlayMode::Live => state.live_bg_monitor == Some(monitor),
				OverlayMode::Frozen => state.monitor == Some(monitor),
			}
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn draw_loupe_tile_window(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		show_hud_blur: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_fog_amount: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme_mode: ThemeMode,
	) -> Result<()> {
		let draw_started_at = Instant::now();
		let mut phase_timings = WindowRendererPhaseTimings::default();
		let (theme, size, pixels_per_point, raw_input) =
			self.prepare_window_renderer_input(gpu, monitor, None, theme_mode, &mut phase_timings);

		self.loupe_tile = None;

		let shader_blur_active = !cfg!(target_os = "macos")
			&& matches!(state.mode, OverlayMode::Frozen)
			&& show_hud_blur
			&& !hud_opaque;
		let hud_cfg = HudDrawConfig {
			can_draw_hud: false,
			needs_frozen_surface_bg: false,
			needs_shader_blur_bg: shader_blur_active,
			hud_glass_active: shader_blur_active,
		};
		let sync_hud_bg_started_at = Instant::now();

		self.sync_or_clear_hud_bg(gpu, state, monitor, hud_cfg)?;

		phase_timings.sync_hud_bg = sync_hud_bg_started_at.elapsed();

		let hud_shader_blur_active = self.hud_shader_blur_active(state, monitor, hud_cfg);
		let hud_blur_active = show_hud_blur && !hud_opaque;
		let body_fill = Self::tinted_hud_body_fill(
			theme,
			hud_blur_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
		);
		let run_loupe_tile_egui_started_at = Instant::now();
		let (full_output, loupe_tile_rect) = self.run_loupe_tile_egui(
			raw_input,
			state,
			theme,
			hud_blur_active,
			hud_opaque,
			body_fill,
		);

		phase_timings.run_egui = run_loupe_tile_egui_started_at.elapsed();
		self.loupe_tile = loupe_tile_rect;

		if hud_shader_blur_active {
			self.hud_pill = loupe_tile_rect.map(|rect| HudPillGeometry {
				rect,
				radius_points: LOUPE_TILE_CORNER_RADIUS_POINTS as f32,
			});

			if self.hud_pill.is_some() {
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
			}
		} else {
			self.hud_pill = None;
		}

		let sync_egui_textures_started_at = Instant::now();

		self.sync_egui_textures(gpu, &full_output);

		phase_timings.sync_egui_textures = sync_egui_textures_started_at.elapsed();

		let tessellate_started_at = Instant::now();
		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);

		phase_timings.tessellate = tessellate_started_at.elapsed();

		self.finish_window_renderer_draw(
			gpu,
			state,
			WindowRendererPath::LoupeTile,
			monitor,
			size,
			pixels_per_point,
			draw_started_at,
			&mut phase_timings,
			paint_jobs,
			false,
			hud_shader_blur_active,
			false,
		)
	}

	pub(in crate::overlay) fn tinted_hud_body_fill(
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
	) -> Color32 {
		let mut opacity = if hud_opaque { 1.0 } else { hud_opacity.clamp(0.0, 1.0) };

		if hud_blur_active {
			opacity = opacity.max(hud_helpers::hud_blur_tint_alpha(theme));
		}

		let tint = hud_milk_amount.clamp(0.0, 1.0);
		let mut fill = hud_helpers::hud_body_fill_srgba8(theme, false);
		let tint_hue = hud_tint_hue.clamp(0.0, 1.0);
		let tint_saturation = 1.0;
		let (_, _, base_lightness) = hud_helpers::rgb_to_hsl(Rgb::new(fill[0], fill[1], fill[2]));
		let tinted_target = hud_helpers::hsl_to_rgb(tint_hue, tint_saturation, base_lightness);

		fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
			((f32::from(a) + ((f32::from(b) - f32::from(a)) * t)).round().clamp(0.0, 255.0)) as u8
		}

		fill[0] = lerp_u8(fill[0], tinted_target.r, tint);
		fill[1] = lerp_u8(fill[1], tinted_target.g, tint);
		fill[2] = lerp_u8(fill[2], tinted_target.b, tint);
		fill[3] = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;

		Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3])
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay::rendering) fn run_loupe_tile_egui(
		&mut self,
		raw_input: RawInput,
		state: &OverlayState,
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		body_fill: Color32,
	) -> (FullOutput, Option<Rect>) {
		let mut loupe_tile_rect = None;
		let egui_ctx = self.egui_ctx.clone();
		let full_output = egui_ctx.run_ui(raw_input, |ui| {
			let ctx = ui.ctx();

			if !state.alt_held {
				return;
			}

			const CELL: f32 = 10.0;

			let side = hud_helpers::stable_live_loupe_side_points(state, CELL);
			let tile_padding = Margin::same(10);
			let outer_stroke_color = match theme {
				HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
				HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
			};
			let outer_stroke = Stroke::new(1.0, outer_stroke_color);
			let shadow = egui::epaint::Shadow {
				offset: [0, 0],
				blur: 10,
				spread: 0,
				color: match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 28),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 18),
				},
			};
			let tile_radius = LOUPE_TILE_CORNER_RADIUS_POINTS as u8;
			let frame = Frame {
				fill: body_fill,
				stroke: outer_stroke,
				shadow,
				corner_radius: CornerRadius::same(tile_radius),
				inner_margin: tile_padding,
				..Frame::default()
			};
			let pad = 6.0;

			Area::new(Id::new("rsnap-loupe-window"))
				.order(Order::Foreground)
				.fixed_pos(Pos2::new(pad, pad))
				.show(ctx, |ui| {
					let inner = frame.show(ui, |ui| {
						ui.set_min_size(Vec2::new(side, side));
						self.render_loupe(ui, state, hud_blur_active, hud_opaque, theme);
					});
					let tile_rect = inner.response.rect;

					loupe_tile_rect = Some(tile_rect);

					let inner_stroke_color = match theme {
						HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
						HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
					};
					let inner_stroke = Stroke::new(1.0, inner_stroke_color);
					let inner_rect = tile_rect.shrink(1.0);

					ui.painter().rect_stroke(
						inner_rect,
						CornerRadius::same(tile_radius.saturating_sub(1)),
						inner_stroke,
						StrokeKind::Inside,
					);
				});
		});

		(full_output, loupe_tile_rect)
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay::rendering) fn update_hud_blur_uniform(
		&mut self,
		gpu: &GpuContext,
		size: PhysicalSize<u32>,
		pixels_per_point: f32,
		theme: HudTheme,
		hud_fog_amount: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
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

		let max_lod = self.hud_bg.as_ref().map(|bg| bg.max_lod).unwrap_or(0.0);
		let rect_min_px =
			[hud_pill.rect.min.x * pixels_per_point, hud_pill.rect.min.y * pixels_per_point];
		let rect_size_px =
			[hud_pill.rect.width() * pixels_per_point, hud_pill.rect.height() * pixels_per_point];
		let rect_min_size = [rect_min_px[0], rect_min_px[1], rect_size_px[0], rect_size_px[1]];
		let tint =
			Self::tinted_hud_body_fill(theme, false, false, 1.0, hud_milk_amount, hud_tint_hue);
		let tint_rgba = [
			hud_helpers::srgb8_to_linear_f32(tint[0]),
			hud_helpers::srgb8_to_linear_f32(tint[1]),
			hud_helpers::srgb8_to_linear_f32(tint[2]),
			hud_helpers::hud_blur_tint_alpha(theme),
		];
		let effects =
			[hud_fog_amount.clamp(0.0, 1.0), hud_milk_amount.clamp(0.0, 1.0), max_lod, 0.0];
		let u = HudBlurUniformRaw {
			rect_min_size,
			radius_blur_soft: [
				hud_pill.radius_points * pixels_per_point,
				(0.9 + (hud_fog_amount.clamp(0.0, 1.0) * 3.2)) * pixels_per_point,
				1.0 * pixels_per_point,
				0.0,
			],
			surface_size_px: [surface_w, surface_h, 0.0, 0.0],
			tint_rgba,
			effects,
		};

		gpu.queue.write_buffer(&self.hud_blur_uniform, 0, u.as_bytes());
	}

	pub(in crate::overlay::rendering) fn sync_hud_bg(
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

	pub(in crate::overlay::rendering) fn render_frozen_bg_to_texture(
		&mut self,
		gpu: &GpuContext,
		image: &RgbaImage,
		target_generation: u64,
	) -> Result<()> {
		let upload_image = image_helpers::downscale_for_gpu_upload(
			image,
			gpu.device.limits().max_texture_dimension_2d,
		);
		let (width, height) = upload_image.dimensions();
		let max_side = gpu.device.limits().max_texture_dimension_2d;
		let mip_level_count = Self::mip_level_count(width, height).min(10);

		debug_assert!(width <= max_side && height <= max_side);

		let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("rsnap-frozen-bg texture"),
			size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
			mip_level_count,
			sample_count: 1,
			dimension: TextureDimension::D2,
			format: TextureFormat::Rgba8UnormSrgb,
			usage: TextureUsages::TEXTURE_BINDING
				| TextureUsages::COPY_DST
				| TextureUsages::RENDER_ATTACHMENT,
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

			rgba_padded = image_helpers::pad_rows(
				src,
				unpadded_bytes_per_row,
				padded_bytes_per_row,
				height as usize,
			);

			&rgba_padded
		};

		gpu.queue.write_texture(
			wgpu::TexelCopyTextureInfo {
				texture: &texture,
				mip_level: 0,
				origin: Origin3d::ZERO,
				aspect: TextureAspect::All,
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

		let view = texture.create_view(&TextureViewDescriptor::default());
		let hud_blur_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("rsnap-hud-blur bind group"),
			layout: &self.hud_blur_bind_group_layout,
			entries: &[
				wgpu::BindGroupEntry { binding: 0, resource: BindingResource::TextureView(&view) },
				wgpu::BindGroupEntry {
					binding: 1,
					resource: BindingResource::Sampler(&self.bg_sampler),
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
				wgpu::BindGroupEntry { binding: 0, resource: BindingResource::TextureView(&view) },
				wgpu::BindGroupEntry {
					binding: 1,
					resource: BindingResource::Sampler(&self.bg_sampler),
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

pub(super) struct HudBg {
	_texture: Texture,
	_view: TextureView,
	pub(super) hud_blur_bind_group: BindGroup,
	pub(super) mipgen_bind_group: BindGroup,
	max_lod: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(super) struct HudBlurUniformRaw {
	rect_min_size: [f32; 4],
	radius_blur_soft: [f32; 4],
	surface_size_px: [f32; 4],
	tint_rgba: [f32; 4],
	effects: [f32; 4],
}
impl HudBlurUniformRaw {
	fn as_bytes(&self) -> &[u8] {
		unsafe { slice::from_raw_parts(ptr::from_ref(self).cast::<u8>(), mem::size_of::<Self>()) }
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::overlay) struct HudPillGeometry {
	pub(in crate::overlay) rect: Rect,
	pub(in crate::overlay) radius_points: f32,
}
