use egui::Context;
use egui::Modifiers;

use crate::overlay::coordinate_geometry;
use crate::overlay::rendering::{
	GpuContext, HudPillGeometry, SelectionDashedBorderCache, SelectionFlowGeometryCache,
	WindowRenderer, WindowRendererPhaseTimings,
};
use crate::overlay::runtime_timing;
use crate::overlay::{
	AcquiredSurfaceFrame, ClippedPrimitive, Duration, Event, FrozenArrowAnnotation,
	FrozenBrushState, FrozenCaptureSource, FrozenEditKind, FrozenSpotlightAnnotation,
	FrozenTextAnnotation, FrozenTextEditState, FrozenTextStyle, FrozenToolbarPointerState,
	FrozenToolbarState, FullOutput, HudAnchor, HudDrawConfig, HudTheme, Id, Instant, LayerId,
	MonitorRect, Order, OverlayMode, OverlayState, PhysicalSize, PointerButton, Pos2, Rect,
	RectPoints, Result, ScreenDescriptor, ThemeMode, ToolbarPlacement, Vec2, ViewportId, Visuals,
	WindowRendererPath, hud_helpers, mem,
};

impl WindowRenderer {
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
					modifiers: Modifiers::default(),
				});
			}
			if pointer.left_button_went_up {
				events.push(Event::PointerButton {
					pos: pointer.cursor_local,
					button: PointerButton::Primary,
					pressed: false,
					modifiers: Modifiers::default(),
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
	#[allow(clippy::too_many_lines)]
	fn run_egui(
		&mut self,
		raw_input: egui::RawInput,
		state: &OverlayState,
		monitor: MonitorRect,
		pending_frozen_display_handoff_monitor: Option<MonitorRect>,
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
		pending_frozen_display_handoff: bool,
		needs_surface_bg: bool,
		show_frozen_capture_affordance: bool,
		frozen_selection_resize_handles_enabled: bool,
		frozen_capture_source: FrozenCaptureSource,
		frozen_capture_is_fullscreen_fallback: bool,
		frozen_toolbar_reserved_rect: Option<Rect>,
		frozen_edit_history: &[FrozenEditKind],
		frozen_brush_state: Option<&FrozenBrushState>,
		frozen_arrow_annotations: &[FrozenArrowAnnotation],
		frozen_arrow_preview: Option<&FrozenArrowAnnotation>,
		frozen_spotlight_annotations: &[FrozenSpotlightAnnotation],
		frozen_spotlight_preview_rect: Option<RectPoints>,
		frozen_text_annotations: &[FrozenTextAnnotation],
		frozen_text_edit: Option<&FrozenTextEditState>,
		frozen_text_style: FrozenTextStyle,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
		mut toolbar_state: Option<&mut FrozenToolbarState>,
		toolbar_pointer: Option<FrozenToolbarPointerState>,
	) -> (FullOutput, Option<HudPillGeometry>) {
		let hud_data = if can_draw_hud {
			state.cursor.and_then(|cursor| {
				let local_cursor = hud_local_cursor_override
					.or_else(|| coordinate_geometry::global_to_local(cursor, monitor))?;

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

			if (matches!(state.mode, OverlayMode::Live) || pending_frozen_display_handoff)
				&& !can_draw_hud
			{
				_show_selection_affordance |= Self::render_live_or_pending_capture_affordances(
					ctx,
					state,
					monitor,
					pending_frozen_display_handoff_monitor,
					theme,
					selection_flow_enabled,
					selection_flow_stroke_width_px,
					pending_frozen_display_handoff,
					frozen_capture_source,
					selection_flow_geometry_cache,
					selection_dashed_border_cache,
				);
			}
			if matches!(state.mode, OverlayMode::Frozen)
				&& !pending_frozen_display_handoff
				&& (needs_surface_bg || show_frozen_capture_affordance)
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
					frozen_selection_resize_handles_enabled,
					frozen_capture_source,
					frozen_toolbar_reserved_rect,
					frozen_edit_history,
					frozen_brush_state,
					frozen_arrow_annotations,
					frozen_arrow_preview,
					frozen_spotlight_annotations,
					frozen_spotlight_preview_rect,
					frozen_text_annotations,
					frozen_text_edit,
					frozen_text_style,
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

	#[allow(clippy::too_many_arguments)]
	fn render_live_or_pending_capture_affordances(
		ctx: &Context,
		state: &OverlayState,
		monitor: MonitorRect,
		pending_frozen_display_handoff_monitor: Option<MonitorRect>,
		theme: HudTheme,
		selection_flow_enabled: bool,
		selection_flow_stroke_width_px: f32,
		pending_frozen_display_handoff: bool,
		frozen_capture_source: FrozenCaptureSource,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		selection_dashed_border_cache: &mut SelectionDashedBorderCache,
	) -> bool {
		let screen_rect = ctx.input(|i| i.viewport_rect());
		let layer =
			LayerId::new(Order::Foreground, Id::new(format!("live-capture-{}", monitor.id)));
		let painter = ctx.layer_painter(layer);

		if pending_frozen_display_handoff {
			Self::render_pending_frozen_display_handoff_affordance(
				ctx,
				&painter,
				state,
				monitor,
				pending_frozen_display_handoff_monitor,
				screen_rect,
				theme,
				selection_flow_enabled,
				selection_flow_stroke_width_px,
				frozen_capture_source,
				selection_flow_geometry_cache,
				selection_dashed_border_cache,
			)
		} else {
			Self::render_live_capture_affordances(
				ctx,
				&painter,
				state,
				monitor,
				screen_rect,
				theme,
				selection_flow_enabled,
				selection_flow_stroke_width_px,
				selection_flow_geometry_cache,
				selection_dashed_border_cache,
			)
		}
	}

	fn sync_hud_bg_with_timing(
		&mut self,
		gpu: &GpuContext,
		state: &OverlayState,
		monitor: MonitorRect,
		hud_cfg: HudDrawConfig,
	) -> Result<Duration> {
		let sync_hud_bg_started_at = Instant::now();

		self.sync_or_clear_hud_bg(gpu, state, monitor, hud_cfg)?;

		Ok(sync_hud_bg_started_at.elapsed())
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

	pub(in crate::overlay::rendering) fn prepare_window_renderer_input(
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
	pub(in crate::overlay::rendering) fn maybe_update_hud_blur_uniform(
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
	pub(in crate::overlay::rendering) fn finish_window_renderer_draw(
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

				if runtime_timing::should_request_overlay_redraw_after_surface_skip(
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
	pub(in crate::overlay) fn draw(
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
		allow_live_surface_bg: bool,
		pending_frozen_display_handoff: bool,
		pending_frozen_display_handoff_monitor: Option<MonitorRect>,
		show_frozen_capture_affordance: bool,
		frozen_selection_resize_handles_enabled: bool,
		frozen_capture_source: FrozenCaptureSource,
		frozen_capture_is_fullscreen_fallback: bool,
		frozen_toolbar_reserved_rect: Option<Rect>,
		frozen_edit_history: &[FrozenEditKind],
		frozen_brush_state: Option<&FrozenBrushState>,
		frozen_arrow_annotations: &[FrozenArrowAnnotation],
		frozen_arrow_preview: Option<&FrozenArrowAnnotation>,
		frozen_spotlight_annotations: &[FrozenSpotlightAnnotation],
		frozen_spotlight_preview_rect: Option<RectPoints>,
		frozen_text_annotations: &[FrozenTextAnnotation],
		frozen_text_edit: Option<&FrozenTextEditState>,
		frozen_text_style: FrozenTextStyle,
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
			allow_live_surface_bg,
			toolbar_active,
			show_hud_blur,
			hud_opaque,
		);

		phase_timings.sync_hud_bg = self.sync_hud_bg_with_timing(gpu, state, monitor, hud_cfg)?;

		let hud_shader_blur_active = self.hud_shader_blur_active(state, monitor, hud_cfg);
		let mut selection_flow_cache = mem::take(&mut self.selection_flow_cache);
		let mut selection_dashed_border_cache = mem::take(&mut self.selection_dashed_border_cache);
		let run_egui_started_at = Instant::now();
		let (full_output, hud_pill) = self.run_egui(
			raw_input,
			state,
			monitor,
			pending_frozen_display_handoff_monitor,
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
			pending_frozen_display_handoff,
			hud_cfg.needs_surface_bg,
			show_frozen_capture_affordance,
			frozen_selection_resize_handles_enabled,
			frozen_capture_source,
			frozen_capture_is_fullscreen_fallback,
			frozen_toolbar_reserved_rect,
			frozen_edit_history,
			frozen_brush_state,
			frozen_arrow_annotations,
			frozen_arrow_preview,
			frozen_spotlight_annotations,
			frozen_spotlight_preview_rect,
			frozen_text_annotations,
			frozen_text_edit,
			frozen_text_style,
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

		phase_timings.sync_egui_textures = self.sync_egui_textures_with_timing(gpu, &full_output);

		let tessellate_started_at = Instant::now();
		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);

		phase_timings.tessellate = tessellate_started_at.elapsed();

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
			Self::should_draw_surface_bg(state, monitor, hud_cfg.needs_surface_bg),
			hud_shader_blur_active,
			toolbar_active,
		)
	}

	fn should_draw_surface_bg(
		state: &OverlayState,
		monitor: MonitorRect,
		needs_surface_bg: bool,
	) -> bool {
		needs_surface_bg
			&& match state.mode {
				OverlayMode::Live => {
					state.live_bg_monitor == Some(monitor) && state.live_bg_image.is_some()
				},
				OverlayMode::Frozen => {
					state.monitor == Some(monitor) && state.frozen_display_surface_image().is_some()
				},
			}
	}
}
