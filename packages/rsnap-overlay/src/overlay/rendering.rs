mod affordances;
mod frame_pipeline;
mod hud_rendering;
mod hud_surface;
mod scroll_preview_window;
mod selection_geometry;
mod timing;
mod window_surface;

#[cfg(test)]
pub(super) use self::affordances::size_badge::{
	SELECTION_SIZE_BADGE_GAP_PX, SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX,
	SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX,
};
pub(super) use self::{hud_surface::HudPillGeometry, scroll_preview_window::ScrollPreviewWindow};
pub(super) use self::{
	selection_geometry::{
		SelectionDashedBorderCache, SelectionDashedBorderCacheKey, SelectionDashedBorderMetrics,
		SelectionFlowGeometryCache, SelectionFlowGeometryCacheKey, SelectionSizeBadgeLayout,
		SelectionSizeBadgePadding, SelectionSizeBadgeTarget,
	},
	timing::WindowRendererPhaseTimings,
};

use egui::Context;
use wgpu::DeviceDescriptor;
use wgpu::RequestAdapterOptions;
use wgpu::SurfaceConfiguration;
use winit::window::Window;

use self::hud_rendering::LiveLoupeTexture;
use self::hud_surface::HudBg;
#[cfg(target_os = "macos")]
use crate::overlay::macos_cursor_runtime::MacOSOverlayCursorRectSupport;
use crate::overlay::{
	Adapter, Arc, BindGroupLayout, Buffer, Color32, Device, Duration, ExperimentalFeatures,
	Features, FontDefinitions, FontFamily, HudTheme, Instant, MemoryHints, MonitorRect,
	PowerPreference, Queue, Rect, RenderPipeline, Renderer, Result, Sampler, SlowOperationLogger,
	Surface, Trace, Variant, WindowId, WrapErr, eyre,
};
use crate::system_fonts;

pub(in crate::overlay) const FROZEN_TEXT_CARET_BLINK_PERIOD_SECS: f64 = 1.0;

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

pub(super) struct HudOverlayWindow {
	pub(super) window: Arc<Window>,
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

pub(super) struct OverlayWindow {
	pub(super) monitor: MonitorRect,
	#[cfg(target_os = "macos")]
	// Drop cursor rect support before releasing the backing window.
	pub(super) cursor_rects: MacOSOverlayCursorRectSupport,
	pub(super) window: Arc<Window>,
	pub(super) renderer: WindowRenderer,
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
		let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
			power_preference: PowerPreference::LowPower,
			compatible_surface: None,
			force_fallback_adapter: false,
		}))
		.map_err(|err| eyre::eyre!("Failed to request GPU adapter: {err}"))?;
		let adapter_limits = adapter.limits();
		let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
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
	window: Arc<Window>,
	surface: Surface<'static>,
	surface_config: SurfaceConfiguration,
	needs_reconfigure: bool,
	egui_ctx: Context,
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

pub(super) fn configure_egui_fonts(fonts: &mut FontDefinitions) {
	egui_phosphor::add_to_fonts(fonts, Variant::Regular);

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

	system_fonts::configure_text_font_fallbacks(fonts);
}
