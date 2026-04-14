pub(crate) mod output;
pub(crate) mod replay_support;

mod aux_window_runtime;
mod capture_window_runtime;
mod config_runtime;
mod cursor_context_runtime;
mod cursor_runtime;
mod frozen_arrow_runtime;
mod frozen_brush_runtime;
mod frozen_export_runtime;
mod frozen_mosaic_runtime;
mod frozen_selection_runtime;
mod frozen_spotlight_runtime;
mod frozen_text_runtime;
mod hud_helpers;
mod hud_runtime;
mod image_helpers;
mod input_runtime;
mod rendering;
mod scroll_capture_runtime;
mod scroll_input_runtime;
mod scroll_preview_runtime;
mod scroll_runtime;
mod session_state;
mod toolbar_runtime;
mod trace_recording;
mod window_position_runtime;
mod window_runtime;
mod worker_runtime;

#[cfg(not(target_os = "macos"))]
use std::env;
#[cfg(target_os = "macos")]
use std::ffi::c_void;
use std::mem;
use std::panic;
#[cfg(target_os = "macos")]
use std::process;
use std::ptr;
use std::slice;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;
use std::{
	borrow::Cow,
	cmp::Ordering,
	collections::{HashMap, HashSet},
	path::PathBuf,
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Report, WrapErr};
use egui::FullOutput;
use egui::Mesh;
use egui::Painter;
use egui::TextureHandle;
use egui::TextureId;
use egui::TextureOptions;
use egui::Ui;
use egui::{
	self, Align2, Color32, CornerRadius, Event, FontDefinitions, FontFamily, FontId, Frame, Layout,
	Margin, PointerButton, Pos2, Rect, Vec2,
};
use egui::{Align, ColorImage};
use egui::{
	Area, CentralPanel, ClippedPrimitive, Id, LayerId, Order, RichText, Sense, Shape, Stroke,
	StrokeKind, UiBuilder, ViewportId, Visuals,
};
use egui_phosphor::{Variant, regular};
use egui_wgpu::{Renderer, ScreenDescriptor};
use image::RgbaImage;
#[cfg(target_os = "macos")]
use image::imageops;
#[cfg(target_os = "macos")]
use objc::declare::ClassDecl;
#[cfg(target_os = "macos")]
use objc::runtime::Sel;
#[cfg(target_os = "macos")]
use objc::runtime::{BOOL, Class, Object, YES};
#[cfg(target_os = "macos")]
use objc::{Encode, Encoding};
#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_app_kit::NSScreen;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSPoint, NSRect, NSSize};
#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use wgpu::Adapter;
use wgpu::AddressMode;
use wgpu::BindGroupLayout;
use wgpu::BindingResource;
use wgpu::BindingType;
use wgpu::BlendState;
use wgpu::Buffer;
use wgpu::BufferBindingType;
use wgpu::BufferSize;
use wgpu::BufferUsages;
use wgpu::ColorWrites;
use wgpu::CompositeAlphaMode;
use wgpu::CurrentSurfaceTexture;
use wgpu::Device;
use wgpu::ExperimentalFeatures;
use wgpu::Features;
use wgpu::FilterMode;
use wgpu::FrontFace;
use wgpu::LoadOp;
use wgpu::MemoryHints;
use wgpu::MipmapFilterMode;
use wgpu::MultisampleState;
use wgpu::Origin3d;
use wgpu::PipelineCompilationOptions;
use wgpu::PolygonMode;
use wgpu::PowerPreference;
use wgpu::PresentMode;
use wgpu::PrimitiveTopology;
use wgpu::Queue;
use wgpu::RenderPipeline;
use wgpu::Sampler;
use wgpu::SamplerBindingType;
use wgpu::ShaderSource;
use wgpu::ShaderStages;
use wgpu::StoreOp;
use wgpu::Surface;
use wgpu::SurfaceCapabilities;
use wgpu::SurfaceTexture;
use wgpu::Texture;
use wgpu::TextureAspect;
use wgpu::TextureDimension;
use wgpu::TextureSampleType;
use wgpu::TextureUsages;
use wgpu::TextureView;
use wgpu::TextureViewDescriptor;
use wgpu::TextureViewDimension;
use wgpu::Trace;
use wgpu::{self};
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::KeyEvent;
use winit::event::Modifiers;
#[cfg(target_os = "macos")]
use winit::window::Window;
use winit::{
	dpi::PhysicalSize,
	event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
	event_loop::ActiveEventLoop,
	keyboard::{Key, ModifiersState, NamedKey},
	window::{CursorIcon, WindowId, WindowLevel},
};

use self::frozen_text_runtime::{FrozenTextInputSource, FrozenTextRecentInput};
#[cfg(target_os = "macos")]
use self::rendering::StartupLiveRgbPlan;
use self::rendering::{
	GpuContext, HudOverlayWindow, HudPillGeometry, HudRedrawSummary, OverlayWindow,
	ScrollPreviewView, ScrollPreviewWindow, WindowRenderer,
};
#[cfg(test)]
use self::rendering::{
	SelectionDashedBorderCache, SelectionFlowGeometryCache, SelectionSizeBadgeTarget,
};
#[cfg(all(target_os = "macos", test))]
use self::session_state::InflightScrollCaptureObservation;
use self::session_state::{
	ActiveFrozenBrushStroke, CursorMoveTrace, FrozenAnnotationColor, FrozenArrowAnnotation,
	FrozenArrowDragState, FrozenBrushModelState, FrozenBrushState, FrozenBrushStroke,
	FrozenMosaicDragState, FrozenSelectionDragCursorMoveTiming, FrozenSelectionDragState,
	FrozenSpotlightAnnotation, FrozenSpotlightDragState, FrozenTextAnnotation, FrozenTextEditState,
	FrozenTextStyle, FrozenToolbarPointerState, FrozenToolbarState, HudDrawConfig,
	LiveSampleApplyResult, ScrollCaptureState, SlowOperationLogger, WindowFreezeCaptureTarget,
};
#[cfg(target_os = "macos")]
use self::session_state::{
	LiveStreamStaleGrace, MacOSHudWindowConfigState, MacOSScrollPixelResidual,
	MacOSScrollWheelEvent,
};
#[cfg(target_os = "macos")]
use self::trace_recording::ScrollCaptureTraceInputRecord;
use self::trace_recording::{
	ScrollCaptureTraceFrameRecord, ScrollCaptureTraceRecorder, ScrollCaptureTraceSessionSnapshot,
};
#[cfg(target_os = "macos")]
use crate::deferred_text_recognition::DeferredTextRecognitionRequest;
#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::{
	CursorSampleRequest, MacLiveFrameStream, STREAM_REGION_FRAME_MAX_AGE,
};
use crate::scroll_capture::{self, ScrollDirection, ScrollObserveOutcome, ScrollSession};
use crate::state::LiveCursorSample;
#[cfg(target_os = "macos")]
use crate::state::MonitorImageSnapshot;
use crate::worker::CapturedMonitorRegionResult;
use crate::{
	state::{
		GlobalPoint, MonitorRect, MonitorRectPoints, OverlayMode, OverlayState, RectPoints, Rgb,
		WindowHit, WindowListSnapshot,
	},
	worker::{
		FreezeCaptureTarget, OverlayWorker, WorkerErrorSource, WorkerRequestSendError,
		WorkerResponse,
	},
};

#[cfg(target_os = "macos")]
macro_rules! sel {
	($($tt:tt)*) => {
		objc::sel!($($tt)*)
	};
}

#[cfg(target_os = "macos")]
macro_rules! sel_impl {
	($($tt:tt)*) => {
		objc::sel_impl!($($tt)*)
	};
}

#[cfg(target_os = "macos")]
type CFTypeRef = *const c_void;

#[cfg(target_os = "macos")]
type CGEventRef = *mut c_void;

#[cfg(target_os = "macos")]
type ExternalScrollInputEvent = (u64, Instant, f64, f64, f64, bool, bool);

#[cfg(target_os = "macos")]
type ExternalScrollInputDrainReader =
	Arc<dyn Fn(u64, Instant) -> Vec<ExternalScrollInputEvent> + Send + Sync>;

#[cfg(target_os = "macos")]
type ScrollCaptureStartGuard = Arc<dyn Fn() -> color_eyre::eyre::Result<bool> + Send + Sync>;

#[cfg(target_os = "macos")]
type ScrollCaptureStartingHook = Arc<dyn Fn() -> color_eyre::eyre::Result<()> + Send + Sync>;

#[cfg(target_os = "macos")]
type ScrollCaptureStartedHook = Arc<dyn Fn() + Send + Sync>;

type Result<T, E = Report> = std::result::Result<T, E>;

pub(crate) const CAPTURE_WINDOW_CONTENT_PROTECTION_ENABLED: bool = false;

#[cfg(target_os = "macos")]
const KCG_HID_EVENT_TAP: u32 = 0;
#[cfg(target_os = "macos")]
const KCG_SCROLL_EVENT_UNIT_PIXEL: u32 = 0;
#[cfg(target_os = "macos")]
const KCG_SCROLL_EVENT_UNIT_LINE: u32 = 1;
#[cfg(target_os = "macos")]
const MACOS_SCROLL_PIXEL_WRAP_MODULUS: f64 = 4_294_967_296.0;
#[cfg(target_os = "macos")]
const MACOS_SCROLL_PIXEL_WRAP_THRESHOLD: f64 = 1_000_000.0;
#[cfg(target_os = "macos")]
const MACOS_SCROLL_PIXEL_DELTA_CLAMP: f64 = 240.0;
const HUD_PILL_BODY_FILL_DARK_SRGBA8: [u8; 4] = [28, 28, 32, 156];
const HUD_PILL_BODY_FILL_LIGHT_SRGBA8: [u8; 4] = [232, 236, 243, 176];
const HUD_PILL_BLUR_TINT_ALPHA_DARK: f32 = 0.18;
const HUD_PILL_BLUR_TINT_ALPHA_LIGHT: f32 = 0.22;
const LOUPE_TILE_CORNER_RADIUS_POINTS: f64 = 12.0;
const HUD_LOUPE_STRIP_GAP_POINTS: i32 = 8;
#[cfg(target_os = "macos")]
const MACOS_HUD_WINDOW_LEVEL: isize = 26;
#[cfg(target_os = "macos")]
const MACOS_OVERLAY_WINDOW_LEVEL: isize = 25;
const FROZEN_TOOLBAR_BUTTON_SIZE_POINTS: f32 = 24.0;
const FROZEN_TOOLBAR_ITEM_SPACING_POINTS: f32 = 4.0;
const TOOLBAR_PILL_INNER_MARGIN_Y_POINTS: f32 = 6.0;
const LIVE_EVENT_CURSOR_CACHE_TTL: Duration = Duration::from_millis(120);
const CURSOR_EVENT_TICK_TTL: Duration = Duration::from_millis(24);
const LIVE_HOVER_HIT_TEST_INTERVAL: Duration = Duration::from_millis(60);
const LIVE_WINDOW_LIST_REFRESH_INTERVAL: Duration = Duration::from_millis(120);
#[cfg(target_os = "macos")]
const POST_HIDE_LIVE_SNAPSHOT_GRACE: Duration = Duration::from_millis(12);
const LIVE_PRESENT_INTERVAL_MIN: Duration = Duration::from_nanos(8_333_333);
const HUD_LOUPE_MOVE_INTERVAL_MIN: Duration = LIVE_PRESENT_INTERVAL_MIN;
const CURSOR_POLL_INTERVAL_MIN: Duration = LIVE_PRESENT_INTERVAL_MIN;
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_STREAM_EVENT_FALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(40);
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_STREAM_POLL_INTERVAL: Duration = Duration::from_millis(8);
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_STREAM_BACKLOG_MAX_FRAMES: usize = 12;
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW: Duration =
	Duration::from_millis(320);
const OVERLAY_EVENT_LOOP_STALL_THRESHOLD: Duration = Duration::from_millis(250);
#[cfg(target_os = "macos")]
const SLOW_OP_WARN_CURSOR_LOCATION: Duration = Duration::from_millis(8);
#[cfg(target_os = "macos")]
const SLOW_OP_WARN_HUD_CONFIG: Duration = Duration::from_millis(40);
const SLOW_OP_WARN_OUTER_POSITION: Duration = Duration::from_millis(24);
const SLOW_OP_WARN_RENDER: Duration = Duration::from_millis(24);
const SLOW_OP_WARN_WINDOW_EVENT: Duration = Duration::from_millis(40);
const SLOW_OP_WARN_INTERVAL: Duration = Duration::from_secs(1);
const OCCLUDED_FRAME_REDRAW_RETRY_WINDOW: Duration = Duration::from_secs(2);
const REDRAW_SUBSTEP_CONTRIBUTION_FLOOR: Duration = Duration::from_millis(4);
// macOS trackpad/wheel sequences can keep delivering usable follow-up frames after the
// initiating input event. Keep the observation window wide enough for the capture pipeline
// to pair those frames before declaring the input stale.
const SCROLL_CAPTURE_INPUT_FRESHNESS: Duration = Duration::from_millis(600);
const SCROLL_CAPTURE_INPUT_MOTION_PRIOR_ROWS_MAX: f64 = 4_096.0;
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES: u8 = 5;
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_DUPLICATE_STREAM_STALL_THRESHOLD: u8 = 3;
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_DUPLICATE_STREAM_REFRESH_INTERVAL: Duration = Duration::from_millis(80);
const HUD_PILL_INNER_MARGIN_X_POINTS: f32 = 12.0;
const HUD_PILL_STROKE_WIDTH_POINTS: f32 = 1.0;
const TOOLBAR_EXPANDED_HEIGHT_PX: f32 = FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
	+ 2.0 * TOOLBAR_PILL_INNER_MARGIN_Y_POINTS
	+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
const TOOLBAR_CAPTURE_GAP_PX: f32 = 10.0;
const TOOLBAR_SCREEN_MARGIN_PX: f32 = 10.0;
const TOOLBAR_DEFAULT_SLOT_POSITION_EPSILON_POINTS: f32 = 1.0;
const HUD_PILL_CORNER_RADIUS_POINTS: u8 = 18;
const FROZEN_TEXT_FONT_SIZE_POINTS: f32 = 16.0;
const FROZEN_TEXT_FONT_SIZE_MIN_POINTS: f32 = 12.0;
const FROZEN_TEXT_FONT_SIZE_MAX_POINTS: f32 = 72.0;
const FROZEN_TEXT_PREVIEW_PLACEHOLDER: &str = "Type";
const FROZEN_TEXT_CARET_BLINK_PERIOD_SECS: f64 = 1.0;
const FROZEN_TEXT_CARET_REPAINT_INTERVAL: Duration = Duration::from_millis(250);
const SELECTION_SIZE_BADGE_FONT_SIZE_POINTS: f32 = 13.0;
const SELECTION_SIZE_BADGE_TEXT_OUTSET_POINTS: f32 = 2.0;
const SELECTION_SIZE_BADGE_OUTLINE_OFFSET_PX: f32 = 1.0;
const SELECTION_SIZE_BADGE_NEAR_SHADOW_OFFSET_PX: f32 = 1.0;
const SELECTION_SIZE_BADGE_FAR_SHADOW_OFFSET_PX: f32 = 2.0;
const SELECTION_SIZE_BADGE_GAP_PX: f32 = 8.0;
const SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX: f32 = 8.0;
const SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX: f32 = 8.0;
const TOOLBAR_DRAG_START_THRESHOLD_PX: f32 = 6.0;
#[cfg(target_os = "macos")]
const TOOLBAR_WINDOW_WARMUP_REDRAWS: u8 = 30;
const LOUPE_WINDOW_WARMUP_REDRAWS: u8 = 30;
const LIVE_DRAG_START_THRESHOLD_PX: f32 = 6.0;
const SELECTION_FLOW_CORNER_RADIUS_PX: f32 = 9.0;
const SELECTION_FLOW_MIN_SEGMENTS: usize = 160;
const SELECTION_FLOW_MAX_SEGMENTS: usize = 1_536;
const SELECTION_FLOW_SAMPLE_STEP_PX: f32 = 3.2;
const SELECTION_FLOW_SPEED: f32 = 0.24;
const SELECTION_FLOW_CORE_WIDTH_PX: f32 = 2.4;
const SELECTION_FLOW_CORE_FLOW_WIDTH: f32 = 0.06;
const SELECTION_FLOW_FLOW_BOOST: f32 = 2.8;
const INTERACTIVE_REPAINT_FPS_CAP: f32 = 120.0;
const SELECTION_FLOW_PALETTE: [(u8, u8, u8); 3] =
	[(196, 226, 255), (228, 198, 255), (176, 244, 224)];
const SELECTION_FLOW_LIGHT_PALETTE: [(u8, u8, u8); 3] =
	[(196, 226, 255), (228, 198, 255), (176, 244, 224)];
const LIVE_DRAG_SELECTION_SCRIM_ALPHA_LIGHT: u8 = 96;
const LIVE_DRAG_SELECTION_SCRIM_ALPHA_DARK: u8 = 148;
const FROZEN_SELECTION_SCRIM_ALPHA_LIGHT: u8 = 224;
const FROZEN_SELECTION_SCRIM_ALPHA_DARK: u8 = 208;
const FROZEN_SELECTION_DASHED_BORDER_WIDTH_PX: f32 = 1.55;
const SELECTION_DASHED_BORDER_WIDTH_PX: f32 = 3.1;
const SELECTION_DASHED_BORDER_DASH_LENGTH_PX: f32 = 12.0;
const SELECTION_DASHED_BORDER_GAP_LENGTH_PX: f32 = 7.8;
const SELECTION_DASHED_BORDER_ALPHA: u8 = 248;
const FROZEN_SELECTION_RESIZE_HANDLE_HIT_SIZE_POINTS: f32 = 24.0;
const FROZEN_SELECTION_RESIZE_HANDLE_HIT_OFFSET_POINTS: f32 = 4.0;
const FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS: f32 = 8.0;
const FROZEN_SELECTION_RESIZE_HANDLE_CORNER_KEEPOUT_POINTS: f32 = 4.25;
const FROZEN_SELECTION_RESIZE_HANDLE_OUTER_RADIUS_POINTS: f32 = 4.25;
const FROZEN_SELECTION_RESIZE_HANDLE_CENTER_DOT_RADIUS_POINTS: f32 = 1.15;
const FROZEN_SELECTION_RESIZE_HANDLE_STROKE_WIDTH_POINTS: f32 = 1.3;
const FROZEN_MOSAIC_BLOCK_SIZE_PX: u32 = 12;
const FROZEN_EDIT_HISTORY_LIMIT: usize = 24;
const FROZEN_BRUSH_STROKE_WIDTH_POINTS: f32 = 3.5;
const FROZEN_BRUSH_STROKE_WIDTH_MIN_POINTS: f32 = 1.0;
const FROZEN_BRUSH_STROKE_WIDTH_MAX_POINTS: f32 = 24.0;
const FROZEN_BRUSH_POINT_SPACING_MIN_POINTS: f32 = 0.25;
const FROZEN_BRUSH_PREVIEW_POINT_SPACING_MIN_POINTS: f32 = 0.1;
const FROZEN_BRUSH_MODELED_POINT_SPACING_MIN_POINTS: f32 = 0.25;
const FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MIN: f32 = 0.12;
const FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MAX: f32 = 0.96;
const FROZEN_BRUSH_MODEL_SPEED_FLOOR_POINTS_PER_SECOND: f32 = 12.0;
const FROZEN_BRUSH_MODEL_SPEED_CEILING_POINTS_PER_SECOND: f32 = 1_200.0;
const FROZEN_BRUSH_MODEL_OUTPUT_RATE_HZ: f32 = 180.0;
const FROZEN_BRUSH_MODEL_TIMESTEP_SECONDS: f32 = 1.0 / FROZEN_BRUSH_MODEL_OUTPUT_RATE_HZ;
const FROZEN_BRUSH_MODEL_SPRING_CONSTANT: f32 = 540.0;
const FROZEN_BRUSH_MODEL_DRAG_CONSTANT: f32 = 42.0;
#[cfg(test)]
const FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS: f32 = 1.0 / 120.0;
const FROZEN_BRUSH_MODEL_CURVE_TURN_RADIANS: f32 = 0.2;
const FROZEN_BRUSH_MODEL_CURVE_AMPLITUDE_POINTS: f32 = FROZEN_BRUSH_STROKE_WIDTH_POINTS * 0.08;
const FROZEN_BRUSH_MODEL_CURVE_RESPONSE_BOOST: f32 = 0.34;
const FROZEN_BRUSH_MODEL_FEATURE_TURN_RADIANS: f32 = 0.78;
const FROZEN_BRUSH_MODEL_SHARP_TURN_RADIANS: f32 = 1.45;
const FROZEN_BRUSH_MODEL_FEATURE_AMPLITUDE_POINTS: f32 = FROZEN_BRUSH_STROKE_WIDTH_POINTS * 0.22;
const FROZEN_BRUSH_STREAMLINE_RESPONSE_MIN: f32 = 0.18;
const FROZEN_BRUSH_STREAMLINE_RESPONSE_MAX: f32 = 0.78;
const FROZEN_BRUSH_STREAMLINE_DISTANCE_CEILING_POINTS: f32 = 6.0;
const FROZEN_BRUSH_PREVIEW_ROUNDING_PASSES: usize = 1;
const FROZEN_BRUSH_COMMIT_ROUNDING_PASSES: usize = 2;
const FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS: f32 = 0.25;
const WINDOW_CAPTURE_MATTE_LIGHT_RGBA: image::Rgba<u8> = image::Rgba([246, 246, 246, 255]);
const WINDOW_CAPTURE_MATTE_DARK_RGBA: image::Rgba<u8> = image::Rgba([24, 24, 24, 255]);
const SCROLL_PREVIEW_WINDOW_WIDTH_POINTS: f64 = 260.0;
const SCROLL_PREVIEW_WINDOW_HEIGHT_POINTS: f64 = 360.0;
const SCROLL_PREVIEW_WINDOW_MARGIN_POINTS: i32 = 16;
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
#[cfg(not(target_os = "macos"))]
const SCROLL_CAPTURE_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_DUPLICATE_WORKER_FRAME_RETRY_INTERVAL: Duration = Duration::from_millis(60);
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE: Duration = Duration::from_millis(180);
const SCROLL_CAPTURE_PREVIEW_WIDTH_PX: u32 = 320;
#[cfg(target_os = "macos")]
const KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: u32 = 0;

#[cfg(target_os = "macos")]
pub(super) struct MacOSOverlayCursorRectSupport {
	view_key: usize,
}
#[cfg(target_os = "macos")]
impl MacOSOverlayCursorRectSupport {
	const fn new(view_key: usize) -> Self {
		Self { view_key }
	}

	fn sync_cursor_rects(&self, window: &Window, rects: &[OverlayCursorRect]) {
		macos_resize_overlay_cursor_view(window, self.view_key);

		if macos_set_overlay_view_cursor_rects(self.view_key, rects) {
			macos_invalidate_overlay_cursor_rects(self.view_key);
		}

		macos_apply_overlay_cursor_for_current_pointer(self.view_key);
	}
}

#[cfg(target_os = "macos")]
impl Drop for MacOSOverlayCursorRectSupport {
	fn drop(&mut self) {
		let rects = macos_overlay_view_cursor_rects();

		match rects.lock() {
			Ok(mut guard) => {
				guard.remove(&self.view_key);
			},
			Err(poisoned) => {
				poisoned.into_inner().remove(&self.view_key);
			},
		}

		macos_remove_overlay_cursor_view(self.view_key);
	}
}

#[derive(Clone, Debug)]
/// Runtime configuration applied to a capture overlay session.
pub struct OverlayConfig {
	/// Positions the live HUD relative to the cursor or another anchor point.
	pub hud_anchor: HudAnchor,
	/// Shows the Tab-key hint chip in the live HUD when enabled.
	pub show_alt_hint_keycap: bool,
	/// Enables blur or its platform fallback for HUD windows.
	pub show_hud_blur: bool,
	/// Enables the animated flow ring drawn around live auto-detected windows.
	pub selection_flow_enabled: bool,
	/// Sets the core stroke width used for the animated selection border.
	pub selection_flow_stroke_width_px: f32,
	/// Forces an opaque HUD background instead of glass styling.
	pub hud_opaque: bool,
	/// 0..=1. Controls HUD background alpha.
	pub hud_opacity: f32,
	/// 0..=1. 0 disables the effect.
	pub hud_fog_amount: f32,
	/// 0..=1. 0 disables the effect.
	pub hud_milk_amount: f32,
	/// Hue value for tint, 0..=1.
	pub hud_tint_hue: f32,
	/// Selects whether Tab must be held or can toggle the loupe.
	pub alt_activation: AltActivationMode,
	/// Chooses where the frozen toolbar is placed.
	pub toolbar_placement: ToolbarPlacement,
	/// Sets the loupe sample size in source pixels.
	pub loupe_sample_side_px: u32,
	/// Requests the light, dark, or system theme.
	pub theme_mode: ThemeMode,
	/// Chooses the destination directory for saved captures.
	pub output_dir: PathBuf,
	/// Sets the filename prefix used for saved captures.
	pub output_filename_prefix: String,
	/// Selects the disk naming strategy for saved captures.
	pub output_naming: OutputNaming,
	/// Selects how transparent window captures are flattened.
	pub window_capture_alpha_mode: WindowCaptureAlphaMode,
	/// Current-process windows that should remain capturable while the rest of rsnap stays excluded.
	pub self_capture_exception_window_ids: Vec<u32>,
}
impl Default for OverlayConfig {
	fn default() -> Self {
		Self {
			hud_anchor: HudAnchor::Cursor,
			show_alt_hint_keycap: true,
			show_hud_blur: true,
			selection_flow_enabled: true,
			selection_flow_stroke_width_px: SELECTION_FLOW_CORE_WIDTH_PX,
			hud_opaque: false,
			hud_opacity: 0.35,
			hud_fog_amount: 0.16,
			hud_milk_amount: 0.0,
			hud_tint_hue: 0.585,
			alt_activation: AltActivationMode::Hold,
			toolbar_placement: ToolbarPlacement::Bottom,
			loupe_sample_side_px: 21,
			theme_mode: ThemeMode::System,
			output_dir: PathBuf::from("."),
			output_filename_prefix: String::from("rsnap"),
			output_naming: OutputNaming::Timestamp,
			window_capture_alpha_mode: WindowCaptureAlphaMode::Background,
			self_capture_exception_window_ids: Vec::new(),
		}
	}
}

/// Stateful overlay controller that drives capture windows and session output.
pub struct OverlaySession {
	config: OverlayConfig,
	worker: Option<OverlayWorker>,
	#[cfg(target_os = "macos")]
	live_sample_worker: Option<OverlayWorker>,
	#[cfg(target_os = "macos")]
	live_sample_stream: Option<MacLiveFrameStream>,
	#[cfg(not(target_os = "macos"))]
	cursor_device: Option<device_query::DeviceState>,
	state: OverlayState,
	session_active: bool,
	cursor_monitor: Option<MonitorRect>,
	egui_repaint_deadline: Arc<Mutex<Option<Instant>>>,
	windows: HashMap<WindowId, OverlayWindow>,
	focused_window_ids: HashSet<WindowId>,
	pending_focus_loss_cleanup: bool,
	hud_window: Option<HudOverlayWindow>,
	loupe_window: Option<HudOverlayWindow>,
	toolbar_window: Option<HudOverlayWindow>,
	scroll_preview_window: Option<ScrollPreviewWindow>,
	#[cfg(target_os = "macos")]
	macos_hud_window_config_cache: HashMap<WindowId, MacOSHudWindowConfigState>,
	hud_outer_pos: Option<GlobalPoint>,
	pending_hud_outer_pos: Option<GlobalPoint>,
	hud_inner_size_points: Option<(u32, u32)>,
	loupe_outer_pos: Option<GlobalPoint>,
	pending_loupe_outer_pos: Option<GlobalPoint>,
	loupe_inner_size_points: Option<(u32, u32)>,
	toolbar_outer_pos: Option<GlobalPoint>,
	pending_toolbar_outer_pos: Option<GlobalPoint>,
	toolbar_inner_size_points: Option<(u32, u32)>,
	gpu: Option<GpuContext>,
	last_hud_window_move_at: Instant,
	last_loupe_window_move_at: Instant,
	last_toolbar_window_move_at: Instant,
	last_present_at: Instant,
	last_live_cursor_poll_at: Instant,
	last_frozen_cursor_poll_at: Instant,
	window_list_snapshot: Option<Arc<WindowListSnapshot>>,
	last_window_list_refresh_request_at: Instant,
	window_list_refresh_interval: Duration,
	last_live_bg_request_at: Instant,
	live_bg_request_interval: Duration,
	freeze_capture_send_full_count: u64,
	hit_test_send_full_count: u64,
	hit_test_send_disconnected_count: u64,
	hit_test_request_id: u64,
	live_cursor_sample_request_id: u64,
	latest_live_cursor_sample_request_id: Option<u64>,
	applied_live_cursor_sample_request_id: Option<u64>,
	latest_live_cursor_sample_requested_at: Option<Instant>,
	last_idle_live_sample_request_at: Option<Instant>,
	pending_click_hit_test_request_id: Option<u64>,
	#[cfg(target_os = "macos")]
	window_list_refresh_inflight: bool,
	#[cfg(target_os = "macos")]
	drop_next_window_list_refresh_snapshot: bool,
	last_live_sample_cursor: Option<GlobalPoint>,
	last_event_cursor: Option<(MonitorRect, GlobalPoint)>,
	last_event_cursor_at: Option<Instant>,
	live_sample_stall_started_at: Option<Instant>,
	last_live_sample_stall_log_at: Option<Instant>,
	slow_op_logger: SlowOperationLogger,
	loupe_activation_key_down: bool,
	keyboard_modifiers: ModifiersState,
	event_loop_phase: OverlayEventLoopPhase,
	event_loop_progress_seq: u64,
	event_loop_last_progress_at: Instant,
	event_loop_last_progress_window_id: Option<WindowId>,
	event_loop_last_progress_monitor_id: Option<u32>,
	event_loop_last_progress_detail: Option<&'static str>,
	event_loop_last_stall_warn_at: Option<Instant>,
	loupe_patch_width_px: u32,
	loupe_patch_height_px: u32,
	pending_freeze_capture: Option<MonitorRect>,
	inflight_freeze_capture: Option<MonitorRect>,
	pending_freeze_capture_armed: bool,
	#[cfg(target_os = "macos")]
	pending_freeze_capture_windows_hidden_at: Option<Instant>,
	#[cfg(target_os = "macos")]
	pending_freeze_capture_hidden_after_stream_generation: Option<u64>,
	authoritative_frozen_capture_ready: bool,
	frozen_transition_started_at: Option<Instant>,
	frozen_transition_preview_committed_at: Option<Instant>,
	frozen_transition_preview_source: Option<&'static str>,
	frozen_transition_final_ready_at: Option<Instant>,
	frozen_transition_toolbar_visible_at: Option<Instant>,
	frozen_transition_target_window_id: Option<u32>,
	pending_window_freeze_capture: Option<WindowFreezeCaptureTarget>,
	inflight_window_freeze_capture: Option<WindowFreezeCaptureTarget>,
	frozen_window_image: Option<RgbaImage>,
	frozen_capture_source: FrozenCaptureSource,
	capture_windows_hidden: bool,
	#[cfg(target_os = "macos")]
	next_ocr_request_id: u64,
	pending_encode_png: Option<RgbaImage>,
	pending_png_action: Option<PngAction>,
	#[cfg(target_os = "macos")]
	png_encode_inflight: bool,
	#[cfg(target_os = "macos")]
	pending_self_capture_exception_window_ids_worker_refresh: bool,
	frozen_text_annotations: Vec<FrozenTextAnnotation>,
	frozen_text_redo_annotations: Vec<FrozenTextAnnotation>,
	frozen_arrow_annotations: Vec<FrozenArrowAnnotation>,
	frozen_arrow_redo_annotations: Vec<FrozenArrowAnnotation>,
	frozen_spotlight_annotations: Vec<FrozenSpotlightAnnotation>,
	frozen_spotlight_redo_annotations: Vec<FrozenSpotlightAnnotation>,
	frozen_text_edit: Option<FrozenTextEditState>,
	frozen_text_input_generation: u64,
	frozen_text_recent_input: Option<FrozenTextRecentInput>,
	toolbar_state: FrozenToolbarState,
	toolbar_left_button_down: bool,
	toolbar_left_button_went_down: bool,
	toolbar_left_button_went_up: bool,
	toolbar_pointer_local: Option<Pos2>,
	#[cfg(target_os = "macos")]
	toolbar_window_cursor_hittest_enabled: bool,
	live_capture_interaction: LiveCaptureInteraction,
	frozen_brush: FrozenBrushState,
	frozen_arrow_drag: FrozenArrowDragState,
	frozen_selection_drag: FrozenSelectionDragState,
	frozen_mosaic_drag: FrozenMosaicDragState,
	frozen_spotlight_drag: FrozenSpotlightDragState,
	frozen_spotlight_preview_rect: Option<RectPoints>,
	frozen_edit_undo_stack: Vec<FrozenEditKind>,
	frozen_edit_redo_stack: Vec<FrozenEditKind>,
	frozen_mosaic_undo_stack: Vec<FrozenMosaicEdit>,
	frozen_mosaic_redo_stack: Vec<FrozenMosaicEdit>,
	hud_window_visible: bool,
	toolbar_window_visible: bool,
	skip_toolbar_focus_on_next_show: bool,
	#[cfg(target_os = "macos")]
	preserve_frontmost_on_next_toolbar_show: bool,
	toolbar_window_warmup_redraws_remaining: u8,
	loupe_window_visible: bool,
	loupe_window_warmup_redraws_remaining: u8,
	scroll_capture: ScrollCaptureState,
	#[cfg(target_os = "macos")]
	scroll_frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	#[cfg(target_os = "macos")]
	scroll_capture_start_guard: Option<ScrollCaptureStartGuard>,
	#[cfg(target_os = "macos")]
	scroll_capture_starting_hook: Option<ScrollCaptureStartingHook>,
	#[cfg(target_os = "macos")]
	scroll_capture_started_hook: Option<ScrollCaptureStartedHook>,
	#[cfg(target_os = "macos")]
	startup_aux_window_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	#[cfg(target_os = "macos")]
	startup_aux_window_creation_pending: bool,
	#[cfg(target_os = "macos")]
	startup_aux_window_creation_scheduled: bool,
	#[cfg(target_os = "macos")]
	pending_startup_aux_live_stream_filter_upgrade: bool,
	#[cfg(target_os = "macos")]
	frontmost_application_before_start: Option<MacOSFrontmostApplication>,
	response_waker: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl OverlaySession {
	#[cfg(not(target_os = "macos"))]
	fn try_create_cursor_device() -> Option<device_query::DeviceState> {
		let has_display =
			env::var_os("DISPLAY").is_some() || env::var_os("WAYLAND_DISPLAY").is_some();

		if !has_display {
			tracing::warn!(
				op = "overlay.cursor_device_unavailable",
				"Skipping cursor-device initialization because no display server is available."
			);

			return None;
		}

		match panic::catch_unwind(device_query::DeviceState::new) {
			Ok(cursor_device) => Some(cursor_device),
			Err(_) => {
				tracing::warn!(
					op = "overlay.cursor_device_unavailable",
					"Falling back to a headless-safe cursor device stub."
				);

				None
			},
		}
	}

	#[must_use]
	pub(crate) fn new() -> Self {
		Self::with_config(OverlayConfig::default())
	}

	fn initial_timing() -> (Duration, Duration, Instant) {
		(Duration::from_millis(500), LIVE_WINDOW_LIST_REFRESH_INTERVAL, Instant::now())
	}

	#[must_use]
	/// Creates a new overlay session with the provided runtime configuration.
	pub fn with_config(config: OverlayConfig) -> Self {
		let runtime = Self::initial_session_runtime(&config);
		#[cfg(not(target_os = "macos"))]
		let cursor_device = Self::try_create_cursor_device();

		Self::build_with_config(
			config,
			runtime,
			#[cfg(not(target_os = "macos"))]
			cursor_device,
		)
	}

	fn build_with_config(
		config: OverlayConfig,
		runtime: InitialSessionRuntime,
		#[cfg(not(target_os = "macos"))] cursor_device: Option<device_query::DeviceState>,
	) -> Self {
		let mut session = Self::build_base_session(
			config,
			#[cfg(not(target_os = "macos"))]
			cursor_device,
		);

		session.apply_initial_session_runtime(runtime);

		session
	}

	fn build_base_session(
		config: OverlayConfig,
		#[cfg(not(target_os = "macos"))] cursor_device: Option<device_query::DeviceState>,
	) -> Self {
		let now = Instant::now();

		Self {
			config,
			#[cfg(not(target_os = "macos"))]
			cursor_device,
			state: OverlayState::new(),
			last_hud_window_move_at: now,
			last_loupe_window_move_at: now,
			last_present_at: now,
			last_live_cursor_poll_at: now - CURSOR_POLL_INTERVAL_MIN,
			last_frozen_cursor_poll_at: now - CURSOR_POLL_INTERVAL_MIN,
			last_window_list_refresh_request_at: now,
			window_list_refresh_interval: Duration::ZERO,
			last_live_bg_request_at: now,
			live_bg_request_interval: Duration::ZERO,
			event_loop_last_progress_at: now,
			loupe_patch_width_px: 0,
			loupe_patch_height_px: 0,
			..Self::build_base_session_defaults()
		}
	}

	#[allow(clippy::too_many_lines)]
	#[rustfmt::skip]
	fn build_base_session_defaults() -> Self {
		Self {
			config: OverlayConfig::default(),
			worker: None,
			#[cfg(target_os = "macos")]
			live_sample_worker: None,
			#[cfg(target_os = "macos")]
			live_sample_stream: None,
			#[cfg(not(target_os = "macos"))]
			cursor_device: None,
			state: OverlayState::new(),
			session_active: false,
			cursor_monitor: None,
			windows: HashMap::new(),
			focused_window_ids: HashSet::new(),
			pending_focus_loss_cleanup: false,
			hud_window: None, loupe_window: None, toolbar_window: None, scroll_preview_window: None,
			#[cfg(target_os = "macos")]
			macos_hud_window_config_cache: HashMap::new(),
			hud_outer_pos: None, pending_hud_outer_pos: None, hud_inner_size_points: None,
			loupe_outer_pos: None, pending_loupe_outer_pos: None, loupe_inner_size_points: None,
			toolbar_outer_pos: None, pending_toolbar_outer_pos: None, toolbar_inner_size_points: None,
			gpu: None,
			last_hud_window_move_at: Instant::now(),
			last_loupe_window_move_at: Instant::now(),
			last_toolbar_window_move_at: Instant::now(),
			last_present_at: Instant::now(),
			last_live_cursor_poll_at: Instant::now(),
			last_frozen_cursor_poll_at: Instant::now(),
			window_list_snapshot: None,
			last_window_list_refresh_request_at: Instant::now(),
			window_list_refresh_interval: Duration::ZERO,
			last_live_bg_request_at: Instant::now(),
			live_bg_request_interval: Duration::ZERO,
			freeze_capture_send_full_count: 0,
			hit_test_send_full_count: 0,
			hit_test_send_disconnected_count: 0,
			hit_test_request_id: 0,
			live_cursor_sample_request_id: 0,
			latest_live_cursor_sample_request_id: None,
			applied_live_cursor_sample_request_id: None,
			latest_live_cursor_sample_requested_at: None,
			last_idle_live_sample_request_at: None,
			pending_click_hit_test_request_id: None,
			#[cfg(target_os = "macos")]
			window_list_refresh_inflight: false,
			#[cfg(target_os = "macos")]
			drop_next_window_list_refresh_snapshot: false,
			last_live_sample_cursor: None, last_event_cursor: None, last_event_cursor_at: None,
			live_sample_stall_started_at: None,
			last_live_sample_stall_log_at: None,
			slow_op_logger: SlowOperationLogger::default(),
			loupe_activation_key_down: false,
			keyboard_modifiers: ModifiersState::default(),
			event_loop_phase: OverlayEventLoopPhase::Idle,
			event_loop_progress_seq: 0,
			event_loop_last_progress_at: Instant::now(),
			event_loop_last_progress_window_id: None, event_loop_last_progress_monitor_id: None,
			event_loop_last_progress_detail: None,
			event_loop_last_stall_warn_at: None,
			loupe_patch_width_px: 0,
			loupe_patch_height_px: 0,
			egui_repaint_deadline: Arc::new(Mutex::new(None)),
			pending_freeze_capture: None, inflight_freeze_capture: None, pending_freeze_capture_armed: false,
			#[cfg(target_os = "macos")] pending_freeze_capture_windows_hidden_at: None, #[cfg(target_os = "macos")] pending_freeze_capture_hidden_after_stream_generation: None,
			authoritative_frozen_capture_ready: false, frozen_transition_started_at: None, frozen_transition_preview_committed_at: None,
			frozen_transition_preview_source: None, frozen_transition_final_ready_at: None,
			frozen_transition_toolbar_visible_at: None, frozen_transition_target_window_id: None,
			pending_window_freeze_capture: None, inflight_window_freeze_capture: None, frozen_window_image: None,
			frozen_capture_source: FrozenCaptureSource::None,
			capture_windows_hidden: false,
			#[cfg(target_os = "macos")]
			next_ocr_request_id: 0,
			pending_encode_png: None,
			pending_png_action: None,
			#[cfg(target_os = "macos")]
			png_encode_inflight: false,
			#[cfg(target_os = "macos")]
			pending_self_capture_exception_window_ids_worker_refresh: false,
			frozen_text_annotations: Vec::new(), frozen_text_redo_annotations: Vec::new(),
			frozen_arrow_annotations: Vec::new(), frozen_arrow_redo_annotations: Vec::new(),
			frozen_spotlight_annotations: Vec::new(), frozen_spotlight_redo_annotations: Vec::new(),
			frozen_text_edit: None, frozen_text_input_generation: 0, frozen_text_recent_input: None, toolbar_state: FrozenToolbarState::default(),
			toolbar_left_button_down: false, toolbar_left_button_went_down: false, toolbar_left_button_went_up: false,
			toolbar_pointer_local: None,
			#[cfg(target_os = "macos")]
			toolbar_window_cursor_hittest_enabled: false,
			live_capture_interaction: LiveCaptureInteraction::Idle,
			frozen_brush: FrozenBrushState::default(), frozen_arrow_drag: FrozenArrowDragState::default(),
			frozen_selection_drag: FrozenSelectionDragState::default(),
			frozen_mosaic_drag: FrozenMosaicDragState::default(), frozen_spotlight_drag: FrozenSpotlightDragState::default(),
			frozen_spotlight_preview_rect: None, frozen_edit_undo_stack: Vec::new(),
			frozen_edit_redo_stack: Vec::new(), frozen_mosaic_undo_stack: Vec::new(), frozen_mosaic_redo_stack: Vec::new(),
			hud_window_visible: false, toolbar_window_visible: false, skip_toolbar_focus_on_next_show: false,
			#[cfg(target_os = "macos")]
			preserve_frontmost_on_next_toolbar_show: false,
			toolbar_window_warmup_redraws_remaining: 0, loupe_window_visible: false, loupe_window_warmup_redraws_remaining: 0,
			scroll_capture: ScrollCaptureState::default(),
			#[cfg(target_os = "macos")]
			scroll_frame_waker: None,
			#[cfg(target_os = "macos")]
			scroll_capture_start_guard: None,
			#[cfg(target_os = "macos")]
			scroll_capture_starting_hook: None,
			#[cfg(target_os = "macos")]
			scroll_capture_started_hook: None,
			#[cfg(target_os = "macos")]
			startup_aux_window_waker: None,
			#[cfg(target_os = "macos")]
			startup_aux_window_creation_pending: false,
			#[cfg(target_os = "macos")]
			startup_aux_window_creation_scheduled: false,
			#[cfg(target_os = "macos")]
			pending_startup_aux_live_stream_filter_upgrade: false,
			#[cfg(target_os = "macos")]
			frontmost_application_before_start: None,
			response_waker: None,
		}
	}

	fn apply_initial_session_runtime(&mut self, runtime: InitialSessionRuntime) {
		self.state = runtime.state;
		self.last_hud_window_move_at = runtime.now;
		self.last_loupe_window_move_at = runtime.now;
		self.last_toolbar_window_move_at = runtime.now;
		self.last_present_at = runtime.now;
		self.last_live_cursor_poll_at = runtime.now - CURSOR_POLL_INTERVAL_MIN;
		self.last_frozen_cursor_poll_at = runtime.now - CURSOR_POLL_INTERVAL_MIN;
		self.last_window_list_refresh_request_at =
			runtime.now - runtime.window_list_refresh_interval;
		self.window_list_refresh_interval = runtime.window_list_refresh_interval;
		self.last_live_bg_request_at = runtime.now - runtime.live_bg_request_interval;
		self.live_bg_request_interval = runtime.live_bg_request_interval;
		self.event_loop_last_progress_at = runtime.now;
		self.loupe_patch_width_px = runtime.loupe_sample_side_px;
		self.loupe_patch_height_px = runtime.loupe_sample_side_px;
	}

	fn overlay_state_with_loupe_patch(loupe_sample_side_px: u32) -> OverlayState {
		let mut state = OverlayState::new();

		state.reset_for_start(loupe_sample_side_px);

		state
	}

	fn overlay_state_with_config(config: &OverlayConfig) -> (u32, OverlayState) {
		let loupe_sample_side_px =
			Self::normalized_loupe_sample_side_px(config.loupe_sample_side_px);

		(loupe_sample_side_px, Self::overlay_state_with_loupe_patch(loupe_sample_side_px))
	}

	fn note_startup_overlay_frame_presented(&mut self) {
		#[cfg(target_os = "macos")]
		self.maybe_schedule_startup_aux_window_creation();
	}

	#[cfg(target_os = "macos")]
	fn capture_frontmost_application_for_exit_restore(&mut self) {
		self.frontmost_application_before_start = macos_frontmost_application();

		tracing::info!(
			op = "overlay.frontmost_app_captured",
			target_process_id =
				self.frontmost_application_before_start.map(|target| target.process_id),
			"Captured the pre-capture frontmost application for later restore."
		);
	}

	#[cfg(target_os = "macos")]
	fn restore_frontmost_application_after_exit(&self, target: Option<MacOSFrontmostApplication>) {
		let Some(target) = target else {
			tracing::info!(
				op = "overlay.frontmost_app_restore_attempted",
				target = "none",
				"Skipped restoring the pre-capture frontmost application because none was recorded."
			);

			return;
		};
		let restored = macos_restore_frontmost_application(target);

		tracing::info!(
			op = "overlay.frontmost_app_restore_attempted",
			target_process_id = target.process_id,
			restored,
			"Attempted to restore the pre-capture frontmost application."
		);
	}

	#[cfg(target_os = "macos")]
	fn restore_recorded_frontmost_application_for_focus_preservation(&self, reason: &'static str) {
		let Some(target) = self.frontmost_application_before_start else {
			return;
		};
		let restored = macos_restore_frontmost_application(target);

		tracing::info!(
			op = "overlay.frontmost_app_focus_preservation_attempted",
			target_process_id = target.process_id,
			reason,
			restored,
			"Attempted to preserve the pre-capture frontmost application during overlay interaction."
		);
	}

	#[cfg(target_os = "macos")]
	/// Registers a wake callback for macOS live-stream frame notifications.
	pub fn set_scroll_frame_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
		self.scroll_frame_waker = Some(waker);
	}

	#[cfg(target_os = "macos")]
	/// Returns whether the host should keep the global Escape hotkey registered
	/// for the current overlay mode.
	#[must_use]
	pub fn wants_global_cancel_hotkey(&self) -> bool {
		self.session_active
	}

	#[cfg(target_os = "macos")]
	/// Returns whether the host should keep the global Tab hotkey registered
	/// for the current overlay mode.
	#[must_use]
	pub fn wants_global_loupe_hotkey(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Live)
	}

	#[cfg(target_os = "macos")]
	/// Registers a wake callback that creates non-critical startup windows after first paint.
	pub fn set_startup_aux_window_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
		self.startup_aux_window_waker = Some(waker);
	}

	#[cfg(target_os = "macos")]
	/// Supplies a host-owned guard that must approve scroll capture before it can start.
	/// Return `Ok(false)` to reject the attempt without surfacing a HUD error.
	pub fn set_scroll_capture_start_guard(&mut self, guard: ScrollCaptureStartGuard) {
		self.scroll_capture_start_guard = Some(guard);
	}

	#[cfg(target_os = "macos")]
	/// Registers a host-owned callback that fires after preflight succeeds but before
	/// scroll capture becomes active.
	pub fn set_scroll_capture_starting_hook(&mut self, hook: ScrollCaptureStartingHook) {
		self.scroll_capture_starting_hook = Some(hook);
	}

	#[cfg(target_os = "macos")]
	/// Registers a host-owned callback that fires only after scroll capture actually starts.
	pub fn set_scroll_capture_started_hook(&mut self, hook: ScrollCaptureStartedHook) {
		self.scroll_capture_started_hook = Some(hook);
	}

	/// Registers a wake callback for worker-thread responses.
	pub fn set_response_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
		self.response_waker = Some(waker);
	}

	#[cfg(target_os = "macos")]
	fn maybe_schedule_startup_aux_window_creation(&mut self) {
		if !self.startup_aux_window_creation_pending || self.startup_aux_window_creation_scheduled {
			return;
		}

		let Some(waker) = self.startup_aux_window_waker.as_ref().cloned() else {
			return;
		};

		self.startup_aux_window_creation_scheduled = true;

		waker();
	}

	#[cfg(target_os = "macos")]
	/// Supplies a reader that replays recorded external scroll input into the session.
	pub fn set_external_scroll_input_drain_reader(
		&mut self,
		reader: ExternalScrollInputDrainReader,
	) {
		self.scroll_capture.external_scroll_input_drain_reader = Some(reader);
	}

	/// Replays a single external scroll-input delta into the active scroll-capture session.
	pub fn handle_external_scroll_input_delta_y(
		&mut self,
		global_x: f64,
		global_y: f64,
		delta_y: f64,
		gesture_active: bool,
		gesture_ended: bool,
	) {
		self.apply_external_scroll_input_delta_y(
			global_x,
			global_y,
			delta_y,
			gesture_active,
			gesture_ended,
			Instant::now(),
		);
	}

	#[must_use]
	pub(crate) fn is_active(&self) -> bool {
		self.session_active
	}

	fn has_prewarmed_startup_resources(&self) -> bool {
		!self.session_active
			&& self.gpu.is_some()
			&& !self.windows.is_empty()
			&& self.hud_window.is_some()
	}

	fn use_fake_hud_blur(&self) -> bool {
		self.config.show_hud_blur && !cfg!(target_os = "macos")
	}

	#[cfg(target_os = "macos")]
	fn macos_hud_window_blur_enabled(&self) -> bool {
		self.config.show_hud_blur
	}

	fn normalized_loupe_sample_side_px(side_px: u32) -> u32 {
		let side_px = side_px.max(3);

		if side_px & 1 == 0 { side_px + 1 } else { side_px }
	}

	fn maybe_keep_frozen_capture_redraw(&self) {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}
		if self.state.frozen_image.is_some() {
			return;
		}

		// Keep producing redraw events while the frozen background is being captured.
		// On some platforms the worker response won't wake the winit event loop, so we
		// must ensure `handle_overlay_window_redraw` + `drain_worker_responses` keep
		// running even with no input events.
		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		} else {
			self.request_redraw_all();
		}

		self.schedule_egui_repaint_after(self.repaint_interval_for_monitor(self.state.monitor));
	}

	fn frozen_preview_visible(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen) && self.state.frozen_image.is_some()
	}

	fn maybe_tick_toolbar_window_warmup_redraw(&mut self) {
		if self.toolbar_window_warmup_redraws_remaining == 0 {
			return;
		}

		#[cfg(not(target_os = "macos"))]
		{
			self.toolbar_window_warmup_redraws_remaining = 0;
		}
		#[cfg(target_os = "macos")]
		{
			if !matches!(self.state.mode, OverlayMode::Frozen)
				|| !self.toolbar_state.visible
				|| self.state.frozen_image.is_none()
				|| self.state.monitor.is_none()
			{
				self.toolbar_window_warmup_redraws_remaining = 0;

				return;
			}

			self.toolbar_window_warmup_redraws_remaining =
				self.toolbar_window_warmup_redraws_remaining.saturating_sub(1);

			self.request_redraw_toolbar_window();
			self.schedule_egui_repaint_after(self.repaint_interval_for_monitor(self.state.monitor));
		}
	}

	fn pending_freeze_capture_matches(&self, monitor: MonitorRect) -> bool {
		self.pending_freeze_capture == Some(monitor)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor == Some(monitor)
	}

	#[cfg(target_os = "macos")]
	fn should_dispatch_pending_freeze_capture(&self, monitor: MonitorRect) -> bool {
		self.pending_freeze_capture_matches(monitor)
	}

	#[cfg(not(target_os = "macos"))]
	fn should_dispatch_pending_freeze_capture(&self, monitor: MonitorRect) -> bool {
		self.pending_freeze_capture_matches(monitor) && self.state.frozen_image.is_none()
	}

	fn frozen_final_capture_ready(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& self.authoritative_frozen_capture_ready
			&& self.state.frozen_image.is_some()
			&& self.pending_freeze_capture.is_none()
			&& self.inflight_freeze_capture.is_none()
	}

	#[cfg(target_os = "macos")]
	fn pending_window_freeze_capture_for_monitor(
		&self,
		monitor: MonitorRect,
	) -> Option<WindowFreezeCaptureTarget> {
		self.pending_window_freeze_capture.filter(|target| target.monitor == monitor)
	}

	fn frozen_transition_elapsed_ms_since(
		&self,
		started_at: Option<Instant>,
		now: Instant,
	) -> Option<u128> {
		started_at
			.and_then(|started_at| now.checked_duration_since(started_at))
			.map(|elapsed| elapsed.as_millis())
	}

	fn reset_frozen_transition_timing(&mut self) {
		self.frozen_transition_started_at = None;
		self.frozen_transition_preview_committed_at = None;
		self.frozen_transition_preview_source = None;
		self.frozen_transition_final_ready_at = None;
		self.frozen_transition_toolbar_visible_at = None;
		self.frozen_transition_target_window_id = None;
	}

	fn log_frozen_transition_timing_info(&self, event: FrozenTransitionTimingInfo) {
		let now = Instant::now();

		tracing::info!(
			target: "rsnap",
			op = event.op,
			monitor_id = event.monitor.map(|monitor| monitor.id),
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition_target_window_id,
			captured_window_id = event.captured_window_id,
				source = event.source,
				preview_source = self.frozen_transition_preview_source,
				reason = event.reason,
				snapshot_age_ms = event.snapshot_age_ms,
				grace_ms = event.grace_ms,
				capture_windows_hidden = self.capture_windows_hidden,
			since_begin_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_started_at, now),
			since_preview_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_preview_committed_at, now),
			since_final_ready_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_final_ready_at, now),
			"{}",
			event.message
		);
	}

	fn begin_frozen_transition_timing(
		&mut self,
		monitor: MonitorRect,
		capture_rect: RectPoints,
		window_target: Option<WindowFreezeCaptureTarget>,
	) {
		let now = Instant::now();

		self.reset_frozen_transition_timing();

		self.frozen_transition_started_at = Some(now);
		self.frozen_transition_target_window_id = window_target.map(|target| target.window_id);

		tracing::debug!(
			op = "overlay.freeze_transition_begin",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			capture_rect = ?capture_rect,
			target_window_id = self.frozen_transition_target_window_id,
			"Frozen transition started."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_begin",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition started.",
		});
	}

	#[cfg(target_os = "macos")]
	fn note_frozen_transition_preview_deferred(
		&self,
		monitor: MonitorRect,
		reason: &'static str,
		snapshot_age_ms: Option<u128>,
	) {
		let now = Instant::now();

		tracing::debug!(
			op = "overlay.freeze_transition_preview_deferred",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition_target_window_id,
			reason,
			snapshot_age_ms,
			since_begin_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_started_at, now),
			"Frozen transition preview is deferred while capture settles."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_preview_deferred",
			monitor: Some(monitor),
			reason: Some(reason),
			source: None,
			snapshot_age_ms,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition preview is deferred while capture settles.",
		});
	}

	fn note_frozen_transition_preview_committed(
		&mut self,
		monitor: MonitorRect,
		source: &'static str,
		snapshot_age_ms: Option<u128>,
	) {
		if self.frozen_transition_preview_committed_at.is_some() {
			return;
		}

		let now = Instant::now();

		self.frozen_transition_preview_committed_at = Some(now);
		self.frozen_transition_preview_source = Some(source);

		tracing::debug!(
			op = "overlay.freeze_transition_preview_committed",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition_target_window_id,
			source,
			snapshot_age_ms,
			since_begin_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_started_at, now),
			"Frozen transition preview became visible."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_preview_committed",
			monitor: Some(monitor),
			reason: None,
			source: Some(source),
			snapshot_age_ms,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition preview became visible.",
		});
	}

	fn note_frozen_transition_worker_requested(
		&mut self,
		monitor: MonitorRect,
		pending_window_target: Option<WindowFreezeCaptureTarget>,
	) {
		let now = Instant::now();

		if self.frozen_transition_target_window_id.is_none() {
			self.frozen_transition_target_window_id =
				pending_window_target.map(|target| target.window_id);
		}

		tracing::debug!(
			op = "overlay.freeze_transition_worker_requested",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition_target_window_id,
			capture_windows_hidden = self.capture_windows_hidden,
			since_begin_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_started_at, now),
			since_preview_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_preview_committed_at, now),
			"Authoritative frozen capture was requested from the worker."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_worker_requested",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Authoritative frozen capture was requested from the worker.",
		});
	}

	fn note_frozen_transition_final_ready(
		&mut self,
		monitor: MonitorRect,
		source: &'static str,
		captured_window_id: Option<u32>,
	) {
		if self.frozen_transition_final_ready_at.is_some() {
			return;
		}

		let now = Instant::now();

		self.frozen_transition_final_ready_at = Some(now);

		tracing::debug!(
			op = "overlay.freeze_transition_final_ready",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition_target_window_id,
			captured_window_id,
			source,
			preview_source = self.frozen_transition_preview_source,
			since_begin_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_started_at, now),
			since_preview_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_preview_committed_at, now),
			"Frozen transition final capture is ready."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_final_ready",
			monitor: Some(monitor),
			reason: None,
			source: Some(source),
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id,
			message: "Frozen transition final capture is ready.",
		});
	}

	#[cfg(target_os = "macos")]
	fn note_frozen_transition_toolbar_visible(&mut self, monitor: MonitorRect) {
		if self.frozen_transition_toolbar_visible_at.is_some() {
			return;
		}

		let now = Instant::now();

		self.frozen_transition_toolbar_visible_at = Some(now);

		tracing::debug!(
			op = "overlay.freeze_transition_toolbar_visible",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition_target_window_id,
			preview_source = self.frozen_transition_preview_source,
			since_begin_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_started_at, now),
			since_preview_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_preview_committed_at, now),
			since_final_ready_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_final_ready_at, now),
			"Frozen transition toolbar became visible."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_toolbar_visible",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition toolbar became visible.",
		});
	}

	#[cfg(target_os = "macos")]
	fn note_frozen_transition_authoritative_handoff_armed(&self, monitor: MonitorRect) {
		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_authoritative_handoff_armed",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition armed authoritative capture fallback.",
		});
	}

	fn note_frozen_transition_aborted(&self, message: &str) {
		let now = Instant::now();

		tracing::debug!(
			op = "overlay.freeze_transition_aborted",
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition_target_window_id,
			preview_source = self.frozen_transition_preview_source,
			message,
			since_begin_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_started_at, now),
			since_preview_ms =
				self.frozen_transition_elapsed_ms_since(self.frozen_transition_preview_committed_at, now),
			"Frozen transition was aborted before completion."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_aborted",
			monitor: None,
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition was aborted before completion.",
		});
	}

	#[cfg(target_os = "macos")]
	fn commit_frozen_preview(
		&mut self,
		monitor: MonitorRect,
		image: RgbaImage,
		cursor: Option<GlobalPoint>,
	) {
		self.state.finish_freeze(monitor, image);

		if let Some(cursor) = cursor {
			self.update_cursor_state(monitor, cursor);
		}
	}

	#[cfg(target_os = "macos")]
	fn snapshot_can_finish_frozen_capture(
		&self,
		window_target: Option<WindowFreezeCaptureTarget>,
	) -> bool {
		window_target.is_none()
			|| self.config.window_capture_alpha_mode == WindowCaptureAlphaMode::Background
	}

	#[cfg(target_os = "macos")]
	fn usable_frozen_capture_snapshot(
		&self,
		monitor: MonitorRect,
		snapshot: Option<Arc<MonitorImageSnapshot>>,
	) -> Option<(Arc<MonitorImageSnapshot>, u128)> {
		let snapshot = snapshot.filter(|snapshot| snapshot.monitor == monitor)?;
		let snapshot_age = snapshot.captured_at.elapsed();

		if snapshot_age > STREAM_REGION_FRAME_MAX_AGE {
			return None;
		}

		Some((snapshot, snapshot_age.as_millis()))
	}

	#[cfg(target_os = "macos")]
	fn maybe_finish_frozen_capture_from_snapshot(
		&mut self,
		monitor: MonitorRect,
		window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
		snapshot: Option<Arc<MonitorImageSnapshot>>,
		source: &'static str,
	) -> bool {
		if !self.snapshot_can_finish_frozen_capture(window_target) {
			return false;
		}

		let Some((snapshot, snapshot_age_ms)) =
			self.usable_frozen_capture_snapshot(monitor, snapshot)
		else {
			return false;
		};
		let snapshot_image = snapshot.image.as_ref().clone();
		let restore_hidden_capture_windows = self.capture_windows_hidden;

		self.commit_frozen_preview(monitor, snapshot_image, cursor);
		self.note_frozen_transition_preview_committed(monitor, source, Some(snapshot_age_ms));

		self.pending_freeze_capture = None;
		self.inflight_freeze_capture = None;
		self.pending_freeze_capture_armed = false;
		self.pending_freeze_capture_windows_hidden_at = None;
		self.pending_freeze_capture_hidden_after_stream_generation = None;
		self.pending_window_freeze_capture = None;
		self.inflight_window_freeze_capture = None;
		self.authoritative_frozen_capture_ready = true;

		self.note_frozen_transition_final_ready(
			monitor,
			source,
			window_target.map(|target| target.window_id),
		);

		self.freeze_capture_send_full_count = 0;
		self.frozen_window_image = None;
		self.toolbar_state.needs_redraw = true;

		if restore_hidden_capture_windows {
			self.destroy_live_only_aux_windows();
			self.restore_capture_windows_visibility();
		} else {
			self.capture_windows_hidden = false;
		}

		self.sync_frozen_toolbar_state();
		self.request_redraw_for_monitor(monitor);
		#[cfg(target_os = "macos")]
		{
			self.request_aux_window_creation_if_needed();
			self.request_redraw_toolbar_window();
		}

		true
	}

	#[cfg(all(test, target_os = "macos"))]
	fn maybe_seed_frozen_capture_preview_from_snapshot(
		&mut self,
		monitor: MonitorRect,
		cursor: Option<GlobalPoint>,
		snapshot: Option<Arc<MonitorImageSnapshot>>,
		source: &'static str,
	) -> bool {
		let Some((snapshot, snapshot_age_ms)) =
			self.usable_frozen_capture_snapshot(monitor, snapshot)
		else {
			return false;
		};
		let snapshot_image = snapshot.image.as_ref().clone();

		self.commit_frozen_preview(monitor, snapshot_image, cursor);
		self.note_frozen_transition_preview_committed(monitor, source, Some(snapshot_age_ms));

		self.toolbar_state.needs_redraw = true;

		self.sync_frozen_toolbar_state();
		self.request_redraw_for_monitor(monitor);
		self.request_aux_window_creation_if_needed();
		self.request_redraw_toolbar_window();

		true
	}

	#[cfg(target_os = "macos")]
	fn maybe_finish_pending_frozen_capture_from_hidden_live_stream_snapshot(
		&mut self,
		monitor: MonitorRect,
	) -> bool {
		let Some(hidden_at) = self.pending_freeze_capture_windows_hidden_at else {
			return false;
		};
		let Some(hidden_after_stream_generation) =
			self.pending_freeze_capture_hidden_after_stream_generation
		else {
			return false;
		};

		if !self.pending_freeze_capture_matches(monitor)
			|| self.authoritative_frozen_capture_ready
			|| self.inflight_freeze_capture.is_some()
			|| !self.capture_windows_hidden
		{
			return false;
		}

		let window_target = self.pending_window_freeze_capture_for_monitor(monitor);

		if !self.snapshot_can_finish_frozen_capture(window_target) {
			return false;
		}

		let Some(stream) = self.live_sample_stream.as_ref() else {
			return false;
		};
		let Some(snapshot) = stream.peek_latest_rgba_snapshot(monitor) else {
			return false;
		};

		if snapshot.captured_at < hidden_at
			|| snapshot.stream_generation <= hidden_after_stream_generation
		{
			return false;
		}

		self.maybe_finish_frozen_capture_from_snapshot(
			monitor,
			window_target,
			self.state.cursor,
			Some(snapshot),
			"live_stream_snapshot_after_hide",
		)
	}

	#[cfg(target_os = "macos")]
	fn should_wait_for_hidden_live_snapshot_before_authoritative_dispatch(
		&self,
		monitor: MonitorRect,
	) -> bool {
		let Some(hidden_at) = self.pending_freeze_capture_windows_hidden_at else {
			return false;
		};
		let Some(hidden_after_stream_generation) =
			self.pending_freeze_capture_hidden_after_stream_generation
		else {
			return false;
		};

		if hidden_at.elapsed() >= POST_HIDE_LIVE_SNAPSHOT_GRACE {
			return false;
		}

		let window_target = self.pending_window_freeze_capture_for_monitor(monitor);

		if !self.snapshot_can_finish_frozen_capture(window_target) {
			return false;
		}

		let Some(stream) = self.live_sample_stream.as_ref() else {
			return false;
		};

		stream.peek_latest_rgba_snapshot(monitor).is_none_or(|snapshot| {
			snapshot.captured_at < hidden_at
				|| snapshot.stream_generation <= hidden_after_stream_generation
		})
	}

	fn seed_frozen_toolbar_default_position(
		&mut self,
		monitor: MonitorRect,
		capture_rect: RectPoints,
	) {
		let default_pos =
			self.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);

		self.toolbar_state.default_slot_position = Some(default_pos);
		self.toolbar_state.floating_position = Some(default_pos);

		let _ = self.update_toolbar_outer_position(monitor, default_pos);

		tracing::debug!(
			monitor_id = monitor.id,
			frozen_generation = self.state.frozen_generation,
			toolbar_primary_size_points =
				?WindowRenderer::frozen_toolbar_primary_size(&self.toolbar_state),
			toolbar_size_points =
				?WindowRenderer::frozen_toolbar_size(&self.toolbar_state),
			default_pos = ?default_pos,
			"Frozen toolbar default position preseeded."
		);
	}

	fn frozen_toolbar_default_position_for_capture_rect(
		&self,
		monitor: MonitorRect,
		capture_rect_points: RectPoints,
	) -> Pos2 {
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let capture_rect = Rect::from_min_size(
			Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
			Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
		);
		let toolbar_primary_size = WindowRenderer::frozen_toolbar_primary_size(&self.toolbar_state);
		let toolbar_positioning_size = self.toolbar_positioning_size();

		WindowRenderer::frozen_toolbar_default_window_pos(
			screen_rect,
			capture_rect,
			toolbar_primary_size,
			toolbar_positioning_size,
			self.config.toolbar_placement,
		)
	}

	fn sync_frozen_annotation_style_capsule_placement(&mut self, monitor: MonitorRect) {
		let Some(toolbar_pos) =
			self.toolbar_state.floating_position.or(self.toolbar_state.default_slot_position)
		else {
			return;
		};
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));

		WindowRenderer::sync_frozen_annotation_style_capsule_placement(
			&mut self.toolbar_state,
			screen_rect,
			toolbar_pos,
		);
	}

	fn initial_session_runtime(config: &OverlayConfig) -> InitialSessionRuntime {
		let (live_bg_request_interval, window_list_refresh_interval, now) = Self::initial_timing();
		let (loupe_sample_side_px, state) = Self::overlay_state_with_config(config);

		InitialSessionRuntime {
			live_bg_request_interval,
			window_list_refresh_interval,
			now,
			loupe_sample_side_px,
			state,
		}
	}

	fn refresh_frozen_helper_windows_for_transition(&mut self, monitor: MonitorRect) {
		self.force_apply_pending_toolbar_window_move();
		self.schedule_egui_repaint_after(self.repaint_interval_for_monitor(Some(monitor)));
		self.request_redraw_for_monitor(monitor);
		self.request_redraw_toolbar_window();
	}

	fn prepare_toolbar_for_frozen_capture_transition(
		&mut self,
		monitor: MonitorRect,
		capture_rect: RectPoints,
	) {
		self.toolbar_state.floating_position = None;
		self.toolbar_state.default_slot_position = None;
		self.toolbar_state.dragging = false;
		self.toolbar_state.needs_redraw = true;
		self.toolbar_state.pill_height_points = None;
		self.toolbar_state.layout_last_screen_size_points = None;
		self.toolbar_state.layout_stable_frames = 0;

		self.reset_frozen_text_state();
		self.sync_frozen_toolbar_state();
		// Spawn the toolbar immediately at the default position (capture aware). This avoids any
		// dependency on egui viewport stabilization or additional input events (mouse move) to
		// finish the initial layout.
		self.seed_frozen_toolbar_default_position(monitor, capture_rect);
		self.request_redraw_toolbar_window();
	}

	fn reset_frozen_annotation_state(&mut self) {
		self.frozen_brush = FrozenBrushState::default();

		self.frozen_arrow_annotations.clear();
		self.frozen_arrow_redo_annotations.clear();
		self.frozen_spotlight_annotations.clear();
		self.frozen_spotlight_redo_annotations.clear();

		self.frozen_arrow_drag = FrozenArrowDragState::default();
		self.frozen_selection_drag = FrozenSelectionDragState::default();
		self.frozen_mosaic_drag = FrozenMosaicDragState::default();
		self.frozen_spotlight_drag = FrozenSpotlightDragState::default();
		self.frozen_spotlight_preview_rect = None;

		self.frozen_edit_undo_stack.clear();
		self.frozen_edit_redo_stack.clear();
		self.frozen_mosaic_undo_stack.clear();
		self.frozen_mosaic_redo_stack.clear();
	}

	fn prepare_frozen_capture_handoff_state(
		&mut self,
		monitor: MonitorRect,
		window_target: Option<WindowFreezeCaptureTarget>,
	) {
		self.pending_freeze_capture = Some(monitor);
		self.pending_freeze_capture_armed = false;
		#[cfg(target_os = "macos")]
		{
			self.pending_freeze_capture_windows_hidden_at = None;
			self.pending_freeze_capture_hidden_after_stream_generation = None;
		}
		self.inflight_freeze_capture = None;
		self.authoritative_frozen_capture_ready = false;
		self.freeze_capture_send_full_count = 0;
		self.pending_window_freeze_capture = window_target;
		self.inflight_window_freeze_capture = None;
		self.frozen_window_image = None;
		self.capture_windows_hidden = false;
		self.pending_click_hit_test_request_id = None;

		if !matches!(
			self.live_capture_interaction,
			LiveCaptureInteraction::FrozenFromClick { .. }
				| LiveCaptureInteraction::FrozenFromDrag { .. }
		) {
			self.set_live_capture_interaction(LiveCaptureInteraction::Idle);
		}
	}

	fn begin_frozen_capture_with_rect(
		&mut self,
		monitor: MonitorRect,
		rect: Option<RectPoints>,
		window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
	) {
		self.frozen_capture_source = if rect.is_none() {
			FrozenCaptureSource::FullscreenFallback
		} else if window_target.is_some() {
			FrozenCaptureSource::Window
		} else {
			FrozenCaptureSource::DragRegion
		};

		let capture_rect = rect.unwrap_or(RectPoints::new(0, 0, monitor.width, monitor.height));

		self.state.alt_held = false;
		self.loupe_activation_key_down = false;
		self.state.rgb = None;
		self.state.loupe = None;

		self.set_alt_loupe_window_visible(None, false);
		self.state.clear_error();
		self.state.begin_freeze(monitor);
		self.begin_frozen_transition_timing(monitor, capture_rect, window_target);

		self.state.frozen_capture_rect = Some(capture_rect);
		self.state.frozen_mosaic_preview_rect = None;
		self.state.drag_rect = None;
		self.state.hovered_window_rect = None;

		self.reset_frozen_annotation_state();

		self.skip_toolbar_focus_on_next_show = true;
		#[cfg(target_os = "macos")]
		{
			self.preserve_frontmost_on_next_toolbar_show = true;
		}

		tracing::debug!(
			monitor_id = monitor.id,
			origin = ?monitor.origin,
			width_points = monitor.width,
			height_points = monitor.height,
			monitor_scale_factor = monitor.scale_factor(),
			cursor = ?cursor,
			capture_rect = ?capture_rect,
			"Freeze begin."
		);

		self.prepare_toolbar_for_frozen_capture_transition(monitor, capture_rect);
		self.prepare_frozen_capture_handoff_state(monitor, window_target);
		#[cfg(target_os = "macos")]
		self.restore_recorded_frontmost_application_for_focus_preservation("begin_frozen_capture");

		#[cfg(target_os = "macos")]
		if self.begin_frozen_capture_with_rect_macos(monitor, window_target, cursor) {
			return;
		}

		#[cfg(not(target_os = "macos"))]
		self.begin_frozen_capture_with_rect_non_macos(monitor, window_target, cursor);
		// Do not request the first frozen redraw until the session either has a preview image or has
		// committed to hiding capture windows for the authoritative handoff. Otherwise the overlay can
		// briefly present an empty black frozen frame before the real preview arrives.
		self.refresh_frozen_helper_windows_for_transition(monitor);
	}

	#[cfg(target_os = "macos")]
	fn begin_frozen_capture_with_rect_macos(
		&mut self,
		monitor: MonitorRect,
		_window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
	) -> bool {
		if let Some(cursor) = cursor {
			self.update_cursor_state(monitor, cursor);
		}

		self.state.live_bg_monitor = None;
		self.state.live_bg_image = None;
		self.capture_windows_hidden = true;
		self.pending_freeze_capture_armed = true;

		self.note_frozen_transition_preview_deferred(
			monitor,
			"waiting_for_hidden_live_snapshot_or_authoritative_capture",
			None,
		);
		self.hide_capture_windows();

		self.pending_freeze_capture_hidden_after_stream_generation =
			self.live_sample_stream.as_ref().and_then(|stream| {
				let (after_frame_seq, hidden_after_stream_generation) = stream
					.latest_frame_frontier_for_monitor(monitor)
					.map_or((0, 0), |(frame_seq, stream_generation)| {
						(frame_seq, stream_generation)
					});

				stream
					.refresh_monitor_nonblocking_if_stale(monitor, after_frame_seq, true)
					.then_some(hidden_after_stream_generation)
			});
		self.pending_freeze_capture_windows_hidden_at = Some(Instant::now());

		self.note_frozen_transition_authoritative_handoff_armed(monitor);

		false
	}

	#[cfg(not(target_os = "macos"))]
	fn begin_frozen_capture_with_rect_non_macos(
		&mut self,
		monitor: MonitorRect,
		window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
	) {
		if self.use_fake_hud_blur()
			&& window_target.is_none()
			&& self.state.live_bg_monitor == Some(monitor)
			&& let Some(image) = self.state.live_bg_image.take()
		{
			self.state.live_bg_monitor = None;

			self.state.finish_freeze(monitor, image);
			self.note_frozen_transition_preview_committed(monitor, "cached_live_background", None);

			self.pending_freeze_capture = None;
			self.pending_freeze_capture_armed = false;
			self.authoritative_frozen_capture_ready = true;

			self.note_frozen_transition_final_ready(monitor, "cached_live_background", None);

			if let Some(cursor) = cursor {
				self.update_cursor_state(monitor, cursor);
			}

			self.force_apply_pending_toolbar_window_move();

			return;
		}

		self.state.live_bg_monitor = None;
		self.state.live_bg_image = None;
		self.capture_windows_hidden = true;

		self.hide_capture_windows();
	}

	#[cfg(target_os = "macos")]
	fn cropped_monitor_frozen_region_image(
		&self,
		monitor: MonitorRect,
		capture_rect_pixels: RectPoints,
	) -> Option<RgbaImage> {
		let frozen_image = self.state.frozen_image.as_ref()?;
		let x = capture_rect_pixels.x.min(frozen_image.width());
		let y = capture_rect_pixels.y.min(frozen_image.height());
		let max_width = frozen_image.width().saturating_sub(x);
		let max_height = frozen_image.height().saturating_sub(y);
		let width = capture_rect_pixels.width.min(max_width);
		let height = capture_rect_pixels.height.min(max_height);

		if width == 0 || height == 0 {
			tracing::debug!(
				monitor_id = monitor.id,
				capture_rect_pixels = ?capture_rect_pixels,
				frozen_image_size = ?(frozen_image.width(), frozen_image.height()),
				"Scroll capture base-frame crop resolved to an empty region."
			);

			None
		} else {
			Some(imageops::crop_imm(frozen_image, x, y, width, height).to_image())
		}
	}

	fn note_frozen_image_mutated(&mut self, monitor: MonitorRect) {
		self.state.frozen_generation = self.state.frozen_generation.wrapping_add(1);

		self.sync_frozen_toolbar_state();
		self.request_redraw_for_monitor(monitor);
		self.request_redraw_toolbar_window();
	}

	fn clear_frozen_redo_history(&mut self) {
		self.frozen_edit_redo_stack.clear();
		self.frozen_brush.redo_strokes.clear();
		self.frozen_mosaic_redo_stack.clear();
		self.frozen_text_redo_annotations.clear();
		self.frozen_arrow_redo_annotations.clear();
		self.frozen_spotlight_redo_annotations.clear();
	}

	fn discard_evicted_frozen_edit_payload(&mut self, edit_kind: FrozenEditKind) {
		match edit_kind {
			FrozenEditKind::BrushStroke => {
				if !self.frozen_brush.committed_strokes.is_empty() {
					self.frozen_brush.committed_strokes.remove(0);
				}
			},
			FrozenEditKind::MosaicEdit => {
				if !self.frozen_mosaic_undo_stack.is_empty() {
					self.frozen_mosaic_undo_stack.remove(0);
				}
			},
			FrozenEditKind::TextAnnotation => {
				if !self.frozen_text_annotations.is_empty() {
					self.frozen_text_annotations.remove(0);
				}
			},
			FrozenEditKind::ArrowAnnotation => {
				if !self.frozen_arrow_annotations.is_empty() {
					self.frozen_arrow_annotations.remove(0);
				}
			},
			FrozenEditKind::SpotlightAnnotation => {
				if !self.frozen_spotlight_annotations.is_empty() {
					self.frozen_spotlight_annotations.remove(0);
				}
			},
		}
	}

	fn push_frozen_edit_to_undo_history(&mut self, edit_kind: FrozenEditKind) {
		self.frozen_edit_undo_stack.push(edit_kind);

		if self.frozen_edit_undo_stack.len() > FROZEN_EDIT_HISTORY_LIMIT {
			let evicted = self.frozen_edit_undo_stack.remove(0);

			self.discard_evicted_frozen_edit_payload(evicted);
		}

		self.clear_frozen_redo_history();
	}

	fn frozen_undo_available(&self) -> bool {
		!self.frozen_edit_undo_stack.is_empty()
	}

	fn frozen_redo_available(&self) -> bool {
		!self.frozen_edit_redo_stack.is_empty()
	}

	fn perform_frozen_undo(&mut self) -> bool {
		let Some(edit_kind) = self.frozen_edit_undo_stack.pop() else {
			return false;
		};
		let undone = match edit_kind {
			FrozenEditKind::BrushStroke => self.undo_frozen_brush_stroke(),
			FrozenEditKind::MosaicEdit => self.undo_frozen_mosaic_edit(),
			FrozenEditKind::TextAnnotation => self.undo_frozen_text_annotation(),
			FrozenEditKind::ArrowAnnotation => self.undo_frozen_arrow_annotation(),
			FrozenEditKind::SpotlightAnnotation => self.undo_frozen_spotlight_annotation(),
		};

		if undone {
			self.frozen_edit_redo_stack.push(edit_kind);
		} else {
			self.frozen_edit_undo_stack.push(edit_kind);
		}

		self.sync_frozen_toolbar_state();

		undone
	}

	fn perform_frozen_redo(&mut self) -> bool {
		let Some(edit_kind) = self.frozen_edit_redo_stack.pop() else {
			return false;
		};
		let redone = match edit_kind {
			FrozenEditKind::BrushStroke => self.redo_frozen_brush_stroke(),
			FrozenEditKind::MosaicEdit => self.redo_frozen_mosaic_edit(),
			FrozenEditKind::TextAnnotation => self.redo_frozen_text_annotation(),
			FrozenEditKind::ArrowAnnotation => self.redo_frozen_arrow_annotation(),
			FrozenEditKind::SpotlightAnnotation => self.redo_frozen_spotlight_annotation(),
		};

		if redone {
			self.frozen_edit_undo_stack.push(edit_kind);
		} else {
			self.frozen_edit_redo_stack.push(edit_kind);
		}

		self.sync_frozen_toolbar_state();

		redone
	}

	fn handle_captured_freeze_response(
		&mut self,
		monitor: MonitorRect,
		image: RgbaImage,
		window_image: Option<RgbaImage>,
		captured_window_id: Option<u32>,
	) {
		if matches!(self.state.mode, OverlayMode::Frozen) && self.state.monitor == Some(monitor) {
			self.inflight_freeze_capture = None;
			self.authoritative_frozen_capture_ready = true;

			let window_capture_target = self.inflight_window_freeze_capture.take();
			let mut frozen_preview_image = image;

			self.pending_window_freeze_capture = None;
			self.frozen_window_image = None;

			if let (Some(target), Some(window_capture_image), Some(window_id)) =
				(window_capture_target, window_image, captured_window_id)
				&& target.monitor == monitor
				&& target.window_id == window_id
			{
				match self.config.window_capture_alpha_mode {
					WindowCaptureAlphaMode::Background => {},
					WindowCaptureAlphaMode::MatteLight | WindowCaptureAlphaMode::MatteDark => {
						let window_capture_image = Self::compose_window_preview_layer(
							&window_capture_image,
							self.config.window_capture_alpha_mode,
						);

						frozen_preview_image = Self::composite_window_capture_preview(
							frozen_preview_image,
							&window_capture_image,
							monitor,
							target.rect,
							WindowCaptureAlphaMode::Background,
						);
						self.frozen_window_image = Some(window_capture_image);
					},
				}
			}

			self.state.finish_freeze(monitor, frozen_preview_image);
			self.note_frozen_transition_preview_committed(monitor, "authoritative_capture", None);
			self.note_frozen_transition_final_ready(
				monitor,
				"authoritative_capture",
				captured_window_id,
			);
			#[cfg(target_os = "macos")]
			self.destroy_live_only_aux_windows();
			self.restore_capture_windows_visibility();
			#[cfg(target_os = "macos")]
			self.request_aux_window_creation_if_needed();

			self.toolbar_state.needs_redraw = true;

			#[cfg(target_os = "macos")]
			if self.toolbar_state.visible {
				self.toolbar_window_warmup_redraws_remaining =
					self.toolbar_window_warmup_redraws_remaining.max(TOOLBAR_WINDOW_WARMUP_REDRAWS);
			}

			if let Some(cursor) = self.state.cursor {
				self.update_cursor_state(monitor, cursor);
			}

			self.request_redraw_toolbar_window();
			self.request_redraw_for_monitor(monitor);
			#[cfg(not(target_os = "macos"))]
			self.raise_hud_windows();

			return;
		}
		if self.inflight_freeze_capture == Some(monitor) {
			self.inflight_freeze_capture = None;
		}
		if self.inflight_window_freeze_capture.is_some_and(|inflight| inflight.monitor == monitor) {
			self.inflight_window_freeze_capture = None;
			self.pending_window_freeze_capture = None;
		}
		if matches!(self.state.mode, OverlayMode::Live)
			&& self.use_fake_hud_blur()
			&& self.active_cursor_monitor() == Some(monitor)
		{
			self.state.live_bg_monitor = Some(monitor);
			self.state.live_bg_image = Some(image);
			self.state.live_bg_generation = self.state.live_bg_generation.wrapping_add(1);

			self.request_redraw_for_monitor(monitor);
		}
	}

	fn handle_encoded_png_response(&mut self, png_bytes: Vec<u8>) -> OverlayControl {
		let Some(action) = self.pending_png_action.take() else {
			return OverlayControl::Continue;
		};

		match action {
			PngAction::Copy => match output::write_png_bytes_to_clipboard(&png_bytes) {
				Ok(()) => self.exit(OverlayExit::PngBytes(png_bytes)),
				Err(err) => {
					self.state.set_error(format!("{err:#}"));
					self.request_redraw_all();

					OverlayControl::Continue
				},
			},
			PngAction::Save => {
				match output::save_png_bytes_to_configured_dir(&png_bytes, &self.config) {
					Ok(path) => self.exit(OverlayExit::Saved(path)),
					Err(err) => {
						self.state.set_error(format!("{err:#}"));
						self.request_redraw_all();

						OverlayControl::Continue
					},
				}
			},
		}
	}

	#[cfg(target_os = "macos")]
	fn next_ocr_request_id(&mut self) -> u64 {
		let request_id = self.next_ocr_request_id;

		self.next_ocr_request_id = self.next_ocr_request_id.wrapping_add(1);

		request_id
	}

	#[cfg(target_os = "macos")]
	fn maybe_request_redraw_for_pending_output(&mut self) {
		if self.pending_encode_png.is_some() {
			self.request_redraw_all();
		}
	}

	fn maybe_stop_frozen_selection_drag_for_mouse_input(
		&mut self,
		state: ElementState,
		button: MouseButton,
	) {
		if state == ElementState::Released && button == MouseButton::Left {
			self.commit_frozen_arrow_drag();
			self.commit_frozen_spotlight_drag();
			self.commit_frozen_mosaic_drag();

			let _ = self.finish_frozen_brush_stroke();

			self.stop_frozen_selection_drag();
			self.sync_overlay_cursor_icons();
		}
	}

	fn inline_toolbar_size_wheel_active(&self, toolbar_window_id: bool) -> bool {
		!toolbar_window_id
			&& !cfg!(target_os = "macos")
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.toolbar_state.annotation_size_control_hovered
	}

	/// Handles a winit window event for one of the overlay-owned windows.
	#[allow(clippy::too_many_lines)]
	pub fn handle_window_event(
		&mut self,
		window_id: WindowId,
		event: &WindowEvent,
	) -> OverlayControl {
		let started_at = Instant::now();
		let kind = Self::window_event_kind(event);
		let now = Instant::now();

		self.event_loop_last_progress_window_id = Some(window_id);
		self.event_loop_last_progress_monitor_id =
			self.windows.get(&window_id).map(|window| window.monitor.id);

		self.maybe_log_event_loop_stall(now);
		self.mark_progress_with_detail(OverlayEventLoopPhase::WindowEvent, Some(kind));

		match event {
			WindowEvent::MouseInput { state, button, .. } => {
				self.maybe_stop_frozen_selection_drag_for_mouse_input(*state, *button);
			},
			WindowEvent::Focused(focused) => {
				self.note_window_focus_change(window_id, *focused);
			},
			_ => {},
		}

		if let Some(control) = self.handle_scroll_preview_event(window_id, event) {
			return control;
		}

		let toolbar_window_id = self
			.toolbar_window
			.as_ref()
			.is_some_and(|toolbar_window| toolbar_window.window.id() == window_id);
		let inline_toolbar_size_wheel_active =
			self.inline_toolbar_size_wheel_active(toolbar_window_id);
		let control = match event {
			WindowEvent::CloseRequested => self.cancel_overlay("window_close_requested"),
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button: MouseButton::Right,
				..
			} => self.cancel_overlay("window_right_click"),
			WindowEvent::Resized(size) if toolbar_window_id => {
				self.handle_toolbar_window_resized(*size)
			},
			WindowEvent::Moved(position) if toolbar_window_id => {
				self.handle_toolbar_window_moved(window_id, *position)
			},
			WindowEvent::Resized(size) => self.handle_resized(window_id, *size),
			WindowEvent::ScaleFactorChanged { .. } if toolbar_window_id => {
				self.handle_toolbar_window_scale_factor_changed(window_id)
			},
			WindowEvent::ScaleFactorChanged { .. } => self.handle_scale_factor_changed(window_id),
			WindowEvent::CursorEntered { .. } if toolbar_window_id => OverlayControl::Continue,
			WindowEvent::CursorLeft { .. } if toolbar_window_id => {
				self.handle_toolbar_cursor_left()
			},
			WindowEvent::CursorMoved { position, .. } => {
				if toolbar_window_id {
					self.handle_toolbar_cursor_moved(window_id, *position)
				} else {
					self.handle_cursor_moved(window_id, *position)
				}
			},
			WindowEvent::Ime(event) => self.handle_ime_event(window_id, event),
			WindowEvent::MouseWheel { delta, .. } if toolbar_window_id => {
				self.handle_toolbar_mouse_wheel(delta)
			},
			WindowEvent::MouseWheel { delta, .. } if inline_toolbar_size_wheel_active => {
				self.handle_toolbar_mouse_wheel(delta)
			},
			WindowEvent::MouseWheel { delta, .. } => {
				self.handle_scroll_mouse_wheel(window_id, delta)
			},
			WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
				if toolbar_window_id {
					self.handle_toolbar_mouse_input(*state)
				} else {
					self.handle_left_mouse_input(window_id, *state)
				}
			},
			WindowEvent::RedrawRequested if toolbar_window_id => {
				self.handle_toolbar_window_redraw_requested()
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
		};

		self.slow_op_logger.warn_if_slow(
			"overlay.window_event",
			started_at.elapsed(),
			SLOW_OP_WARN_WINDOW_EVENT,
			|| format!("kind={kind} window_id={window_id:?} toolbar_window={toolbar_window_id}"),
		);

		control
	}

	fn handle_resized(&mut self, window_id: WindowId, size: PhysicalSize<u32>) -> OverlayControl {
		let window_scale_factor = self
			.windows
			.get(&window_id)
			.map(|w| w.window.scale_factor())
			.or_else(|| self.hud_window.as_ref().map(|w| w.window.scale_factor()))
			.or_else(|| self.loupe_window.as_ref().map(|w| w.window.scale_factor()));

		tracing::trace!(?window_id, ?size, ?window_scale_factor, "WindowEvent::Resized");

		if let Some(hud_window) = self.hud_window.as_mut()
			&& hud_window.window.id() == window_id
		{
			let window = Arc::clone(&hud_window.window);

			match hud_window.renderer.resize(size) {
				Ok(()) => {
					self.configure_hud_window_common(window.as_ref(), None);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}
		if let Some(loupe_window) = self.loupe_window.as_mut()
			&& loupe_window.window.id() == window_id
		{
			let window = Arc::clone(&loupe_window.window);

			match loupe_window.renderer.resize(size) {
				Ok(()) => {
					self.configure_hud_window_common(
						window.as_ref(),
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
		let window_scale_factor = self
			.windows
			.get(&window_id)
			.map(|w| w.window.scale_factor())
			.or_else(|| self.hud_window.as_ref().map(|w| w.window.scale_factor()))
			.or_else(|| self.loupe_window.as_ref().map(|w| w.window.scale_factor()));

		tracing::trace!(?window_id, ?window_scale_factor, "WindowEvent::ScaleFactorChanged");

		if let Some(hud_window) = self.hud_window.as_mut()
			&& hud_window.window.id() == window_id
		{
			let size = hud_window.window.inner_size();
			let window = Arc::clone(&hud_window.window);

			match hud_window.renderer.resize(size) {
				Ok(()) => {
					self.configure_hud_window_common(window.as_ref(), None);

					return OverlayControl::Continue;
				},
				Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
			}
		}
		if let Some(loupe_window) = self.loupe_window.as_mut()
			&& loupe_window.window.id() == window_id
		{
			let size = loupe_window.window.inner_size();
			let window = Arc::clone(&loupe_window.window);

			match loupe_window.renderer.resize(size) {
				Ok(()) => {
					self.configure_hud_window_common(
						window.as_ref(),
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

	fn toolbar_pointer_state(
		&mut self,
		monitor: MonitorRect,
		toolbar_cursor_local_override: Option<Pos2>,
	) -> Option<FrozenToolbarPointerState> {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return None;
		}
		if !self.toolbar_state.visible {
			return None;
		}
		if self.state.monitor != Some(monitor) {
			return None;
		}
		if toolbar_cursor_local_override.is_none() && self.active_cursor_monitor() != Some(monitor)
		{
			return None;
		}

		let left_button_went_down = self.toolbar_left_button_went_down;
		let left_button_went_up = self.toolbar_left_button_went_up;
		#[cfg(not(target_os = "macos"))]
		let left_button_down = self.toolbar_left_button_down;

		self.toolbar_left_button_went_down = false;
		self.toolbar_left_button_went_up = false;

		let cursor_local = toolbar_cursor_local_override
			.or_else(|| self.state.cursor.and_then(|cursor| global_to_local(cursor, monitor)))?;

		Some(FrozenToolbarPointerState {
			cursor_local,
			#[cfg(not(target_os = "macos"))]
			left_button_down,
			left_button_went_down,
			left_button_went_up,
		})
	}

	#[cfg(target_os = "macos")]
	fn current_deferred_text_recognition_request(
		&mut self,
		request_id: u64,
	) -> Option<DeferredTextRecognitionRequest> {
		let requested_at = Instant::now();

		if self.scroll_capture.active {
			let image = self.scroll_capture.session.as_ref()?.export_image().clone();

			return Some(DeferredTextRecognitionRequest::prepared(request_id, requested_at, image));
		}
		if self.frozen_capture_source == FrozenCaptureSource::Window {
			match self.config.window_capture_alpha_mode {
				WindowCaptureAlphaMode::Background => {},
				WindowCaptureAlphaMode::MatteLight => {
					if let Some(window_image) = self.frozen_window_image.take() {
						return Some(DeferredTextRecognitionRequest::prepared(
							request_id,
							requested_at,
							window_image,
						));
					}
				},
				WindowCaptureAlphaMode::MatteDark => {
					if let Some(window_image) = self.frozen_window_image.take() {
						return Some(DeferredTextRecognitionRequest::prepared(
							request_id,
							requested_at,
							window_image,
						));
					}
				},
			}
		}

		let crop_rect = self.deferred_text_recognition_crop_rect_pixels()?;
		let frozen_image = self.state.frozen_image.take()?;

		Some(DeferredTextRecognitionRequest::frozen_crop(
			request_id,
			requested_at,
			frozen_image,
			crop_rect,
		))
	}

	#[cfg(target_os = "macos")]
	fn deferred_text_recognition_crop_rect_pixels(&self) -> Option<Option<RectPoints>> {
		let frozen_image = self.state.frozen_image.as_ref()?;
		let Some(monitor) = self.state.monitor else {
			return Some(None);
		};
		let capture_rect = self
			.state
			.frozen_capture_rect
			.unwrap_or_else(|| RectPoints::new(0, 0, monitor.width, monitor.height));
		let capture_rect = monitor.local_rect_to_pixels(capture_rect);
		let x = capture_rect.x.min(frozen_image.width());
		let y = capture_rect.y.min(frozen_image.height());
		let max_width = frozen_image.width().saturating_sub(x);
		let max_height = frozen_image.height().saturating_sub(y);
		let width = capture_rect.width.min(max_width);
		let height = capture_rect.height.min(max_height);

		if width == 0 || height == 0 {
			return None;
		}
		if x == 0 && y == 0 && width == frozen_image.width() && height == frozen_image.height() {
			return Some(None);
		}

		Some(Some(RectPoints::new(x, y, width, height)))
	}

	fn sync_frozen_toolbar_state(&mut self) {
		self.toolbar_state.auto_center_available = self.frozen_auto_center_available();
		self.toolbar_state.undo_available = self.frozen_undo_available();
		self.toolbar_state.redo_available = self.frozen_redo_available();
		self.toolbar_state.scroll_capture_active = self.scroll_capture.active;
		// Keep drag-region toolbar geometry stable across the authoritative frozen-capture handoff:
		// show the Scroll slot immediately, but keep it disabled until final_capture_ready flips.
		self.toolbar_state.scroll_capture_available = self.toolbar_scroll_capture_slot_available();
		self.toolbar_state.final_capture_ready = self.frozen_final_capture_ready();
	}

	fn begin_png_action(&mut self, action: PngAction) {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}
		if !self.frozen_final_capture_ready() {
			self.state.set_error("Preparing capture...");
			self.request_redraw_all();

			return;
		}

		self.prepare_active_scroll_capture_output();

		let image = if self.scroll_capture.active {
			self.current_scroll_preview_render_image()
		} else {
			self.current_export_image()
		};
		let Some(export_image) = image else {
			return;
		};

		self.pending_png_action = Some(action);

		match action {
			PngAction::Copy => self.state.set_error("Copying..."),
			PngAction::Save => self.state.set_error("Saving..."),
		}

		self.pending_encode_png = Some(export_image);

		self.request_redraw_all();
	}

	#[cfg(target_os = "macos")]
	fn begin_ocr_action(&mut self) -> OverlayControl {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return OverlayControl::Continue;
		}
		if !self.frozen_final_capture_ready() {
			self.state.set_error("Preparing capture...");
			self.request_redraw_all();

			return OverlayControl::Continue;
		}

		self.prepare_active_scroll_capture_output();

		let request_id = self.next_ocr_request_id();
		let Some(request) = self.current_deferred_text_recognition_request(request_id) else {
			return OverlayControl::Continue;
		};
		let (image_width_px, image_height_px) = request.image_dimensions();

		self.pending_png_action = None;
		self.pending_encode_png = None;

		self.state.clear_error();

		tracing::info!(
			target: "rsnap",
			op = "overlay.ocr_request_started",
			request_id,
			image_width_px,
			image_height_px,
			image_pixels = u64::from(image_width_px) * u64::from(image_height_px),
			scroll_capture_active = self.scroll_capture.active,
			"Queued OCR request."
		);

		self.exit(OverlayExit::DeferredTextRecognition(request))
	}

	fn handle_redraw_requested(&mut self, window_id: WindowId) -> OverlayControl {
		let now = Instant::now();

		self.event_loop_last_progress_window_id = Some(window_id);
		self.event_loop_last_progress_monitor_id =
			self.windows.get(&window_id).map(|window| window.monitor.id);

		self.maybe_log_event_loop_stall(now);
		self.mark_progress(OverlayEventLoopPhase::RedrawDispatch);

		let control = self.drain_worker_responses();

		if !matches!(control, OverlayControl::Continue) {
			return control;
		}
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
		if self
			.scroll_preview_window
			.as_ref()
			.is_some_and(|preview_window| preview_window.window.id() == window_id)
		{
			return self.handle_scroll_preview_redraw_requested();
		}

		self.handle_overlay_window_redraw(window_id)
	}

	#[cfg(target_os = "macos")]
	fn focus_live_capture_window(&self) {
		macos_activate_app();

		let target_window = self
			.active_cursor_monitor()
			.and_then(|monitor| {
				self.windows.values().find(|overlay_window| overlay_window.monitor == monitor)
			})
			.or_else(|| self.windows.values().next())
			.map(|overlay_window| overlay_window.window.as_ref());
		let Some(target_window) = target_window else {
			tracing::info!(
				op = "overlay.live_focus_requested",
				target = "missing_window",
				window_count = self.windows.len(),
				"Requested live capture focus, but no overlay window was available."
			);

			return;
		};

		tracing::info!(
			op = "overlay.live_focus_requested",
			target = "overlay_window",
			window_count = self.windows.len(),
			cursor_monitor_id = ?self.active_cursor_monitor().map(|monitor| monitor.id),
			"Requested live capture focus."
		);

		macos_make_window_key(target_window);
	}

	fn maybe_recenter_frozen_toolbar_default_slot(&mut self, monitor: MonitorRect) -> bool {
		if !matches!(self.state.mode, OverlayMode::Frozen) || self.state.monitor != Some(monitor) {
			return false;
		}
		if self.scroll_capture.active || self.toolbar_state.dragging {
			return false;
		}

		let Some(capture_rect) = self.state.frozen_capture_rect else {
			return false;
		};
		let Some(toolbar_pos) = self.toolbar_state.floating_position else {
			return false;
		};
		let Some(previous_default_pos) = self.toolbar_state.default_slot_position else {
			return false;
		};
		let current_default_pos =
			self.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);

		self.toolbar_state.default_slot_position = Some(current_default_pos);

		if frozen_toolbar_matches_default_slot(toolbar_pos, previous_default_pos) {
			self.toolbar_state.floating_position = Some(current_default_pos);

			self.sync_frozen_annotation_style_capsule_placement(monitor);

			return !frozen_toolbar_matches_default_slot(toolbar_pos, current_default_pos);
		}

		self.sync_frozen_annotation_style_capsule_placement(monitor);

		false
	}

	fn frozen_toolbar_badge_visibility(
		&mut self,
		overlay_monitor: MonitorRect,
		overlay_screen_rect: Rect,
		draw_toolbar: bool,
	) -> bool {
		let toolbar_visible_for_badge = if cfg!(target_os = "macos") {
			!self.should_hide_toolbar_window(overlay_monitor)
		} else {
			draw_toolbar
		};

		#[cfg(target_os = "macos")]
		{
			if !toolbar_visible_for_badge {
				return false;
			}

			let ready = self.advance_frozen_toolbar_readiness_sample(overlay_screen_rect);

			if !ready {
				self.request_redraw_for_monitor(overlay_monitor);
			}

			ready
		}

		#[cfg(not(target_os = "macos"))]
		{
			toolbar_visible_for_badge && self.frozen_toolbar_ready_for_draw(overlay_screen_rect)
		}
	}

	fn handle_overlay_window_redraw(&mut self, window_id: WindowId) -> OverlayControl {
		let Some(overlay_monitor) = self.windows.get(&window_id).map(|overlay| overlay.monitor)
		else {
			return OverlayControl::Continue;
		};

		self.sync_overlay_cursor_icons();
		self.sync_frozen_toolbar_state();

		self.event_loop_last_progress_window_id = Some(window_id);
		self.event_loop_last_progress_monitor_id = Some(overlay_monitor.id);

		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::OverlayRedraw);

		let overlay_screen_rect = self.overlay_window_screen_rect(window_id, overlay_monitor);
		#[cfg(target_os = "macos")]
		let draw_toolbar = false;
		#[cfg(not(target_os = "macos"))]
		let draw_toolbar = matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.state.monitor == Some(overlay_monitor)
			&& self.frozen_preview_visible();
		#[cfg(not(target_os = "macos"))]
		let toolbar_input =
			if draw_toolbar { self.toolbar_pointer_state(overlay_monitor, None) } else { None };
		#[cfg(target_os = "macos")]
		let toolbar_input = None;

		self.log_frozen_overlay_redraw_trace(window_id, overlay_monitor, draw_toolbar);

		let toolbar_ready_for_badge = self.frozen_toolbar_badge_visibility(
			overlay_monitor,
			overlay_screen_rect,
			draw_toolbar,
		);
		let frozen_toolbar_reserved_rect = self.frozen_size_badge_toolbar_reserved_rect(
			overlay_monitor,
			overlay_screen_rect,
			toolbar_ready_for_badge,
		);
		let frozen_selection_resize_handles_enabled = self.frozen_selection_drag_target().is_some();
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};
		let scroll_capture_active = self.scroll_capture.active;
		let frozen_text_style = self.toolbar_state.text_style;
		let visible_frozen_text_annotations: &[FrozenTextAnnotation] =
			if scroll_capture_active { &[] } else { &self.frozen_text_annotations };
		let visible_frozen_arrow_annotations: &[FrozenArrowAnnotation] =
			if scroll_capture_active { &[] } else { &self.frozen_arrow_annotations };
		let visible_frozen_spotlight_annotations: &[FrozenSpotlightAnnotation] =
			if scroll_capture_active { &[] } else { &self.frozen_spotlight_annotations };
		let visible_frozen_text_edit =
			if scroll_capture_active { None } else { self.frozen_text_edit.as_ref() };
		let visible_frozen_arrow_preview =
			if scroll_capture_active { None } else { self.active_frozen_arrow_preview() };
		let visible_frozen_spotlight_preview_rect =
			if scroll_capture_active { None } else { self.frozen_spotlight_preview_rect };
		let toolbar_state = if draw_toolbar { Some(&mut self.toolbar_state) } else { None };

		{
			let Some(overlay_window) = self.windows.get_mut(&window_id) else {
				return OverlayControl::Continue;
			};

			if let Err(err) = overlay_window.renderer.draw(
				gpu,
				&self.state,
				overlay_monitor,
				false,
				None,
				false,
				self.config.hud_anchor,
				self.config.toolbar_placement,
				self.config.show_alt_hint_keycap,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
				self.config.selection_flow_enabled,
				self.config.selection_flow_stroke_width_px,
				!scroll_capture_active,
				scroll_capture_active,
				frozen_selection_resize_handles_enabled,
				self.frozen_capture_source,
				self.frozen_capture_source == FrozenCaptureSource::FullscreenFallback,
				frozen_toolbar_reserved_rect,
				&self.frozen_edit_undo_stack,
				(!scroll_capture_active).then_some(&self.frozen_brush),
				visible_frozen_arrow_annotations,
				visible_frozen_arrow_preview.as_ref(),
				visible_frozen_spotlight_annotations,
				visible_frozen_spotlight_preview_rect,
				visible_frozen_text_annotations,
				visible_frozen_text_edit,
				frozen_text_style,
				toolbar_state,
				toolbar_input,
			) {
				return self.exit(OverlayExit::Error(format!("{err:#}")));
			}
		}
		self.last_present_at = Instant::now();

		self.note_startup_overlay_frame_presented();

		self.handle_capture_and_toolbar_redraw_post(overlay_monitor, draw_toolbar)
	}

	fn log_frozen_overlay_redraw_trace(
		&self,
		window_id: WindowId,
		overlay_monitor: MonitorRect,
		draw_toolbar: bool,
	) {
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.state.monitor != Some(overlay_monitor)
		{
			return;
		}

		tracing::trace!(
			window_id = ?window_id,
			monitor_id = overlay_monitor.id,
			frozen_generation = self.state.frozen_generation,
			final_capture_ready = self.authoritative_frozen_capture_ready,
			frozen_image_ready = self.state.frozen_image.is_some(),
			pending_freeze_capture = self.pending_freeze_capture.map(|m| m.id),
			draw_toolbar,
			toolbar_visible = self.toolbar_state.visible,
			toolbar_floating_position = ?self.toolbar_state.floating_position,
			toolbar_stable_frames = self.toolbar_state.layout_stable_frames,
			toolbar_last_screen_size_points = ?self.toolbar_state.layout_last_screen_size_points,
			"Overlay redraw (Frozen)."
		);
	}

	fn overlay_window_screen_rect(&self, window_id: WindowId, monitor: MonitorRect) -> Rect {
		let fallback_size = Vec2::new(monitor.width as f32, monitor.height as f32);

		self.windows
			.get(&window_id)
			.map(|overlay_window| {
				let scale_factor = overlay_window.window.scale_factor().max(1.0) as f32;
				let size = overlay_window.window.inner_size();
				let size_points = if size.width == 0 || size.height == 0 {
					fallback_size
				} else {
					Vec2::new(
						(size.width as f32 / scale_factor).max(1.0),
						(size.height as f32 / scale_factor).max(1.0),
					)
				};

				Rect::from_min_size(Pos2::ZERO, size_points)
			})
			.unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, fallback_size))
	}

	#[cfg(any(target_os = "macos", test))]
	fn advance_frozen_toolbar_readiness_sample(&mut self, screen_rect: Rect) -> bool {
		advance_frozen_toolbar_readiness_sample_state(&mut self.toolbar_state, screen_rect)
	}

	#[cfg(any(not(target_os = "macos"), test))]
	fn frozen_toolbar_ready_for_draw(&self, screen_rect: Rect) -> bool {
		let screen_size_points = screen_rect.size();
		let needs_new_sample = frozen_toolbar_needs_new_sample(
			self.toolbar_state.layout_last_screen_size_points,
			screen_size_points,
		);

		!needs_new_sample && self.toolbar_state.layout_stable_frames >= 1
	}

	fn frozen_size_badge_toolbar_reserved_rect(
		&self,
		monitor: MonitorRect,
		screen_rect: Rect,
		toolbar_ready: bool,
	) -> Option<Rect> {
		if !toolbar_ready
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.state.monitor != Some(monitor)
		{
			return None;
		}

		WindowRenderer::frozen_toolbar_reserved_rect(
			&self.state,
			monitor,
			screen_rect,
			self.config.toolbar_placement,
			&self.toolbar_state,
		)
	}

	fn handle_capture_and_toolbar_redraw_post(
		&mut self,
		overlay_monitor: MonitorRect,
		draw_toolbar: bool,
	) -> OverlayControl {
		if self.should_dispatch_pending_freeze_capture(overlay_monitor) {
			let pending_window_target = self
				.pending_window_freeze_capture
				.filter(|target| target.monitor == overlay_monitor);
			let freeze_target = pending_window_target
				.map_or(FreezeCaptureTarget::Monitor, |target| FreezeCaptureTarget::Window {
					window_id: target.window_id,
				});
			#[cfg(target_os = "macos")]
			let _ = (&freeze_target, &pending_window_target, &overlay_monitor);

			#[cfg(not(target_os = "macos"))]
			{
				// Capture must happen on a post-hide redraw so the HUD/loupe are not included.
				if self.pending_freeze_capture_armed {
					let Some(worker) = &self.worker else {
						self.abort_pending_freeze_capture("Capture worker is unavailable.");

						return OverlayControl::Continue;
					};

					match worker.request_freeze_capture(overlay_monitor, freeze_target) {
						Ok(()) => {
							self.note_freeze_capture_request_started(
								overlay_monitor,
								pending_window_target,
							);
						},
						Err(err) => {
							self.handle_freeze_capture_request_send_error(overlay_monitor, err);
						},
					}
				} else {
					self.freeze_capture_send_full_count = 0;
					self.pending_freeze_capture_armed = true;

					#[cfg(not(target_os = "macos"))]
					self.hide_capture_windows();
					self.request_redraw_for_monitor(overlay_monitor);
				}
			}
		}
		if draw_toolbar && self.sync_frozen_text_edit_for_selected_tool() {
			self.request_redraw_for_monitor(overlay_monitor);
		}
		if draw_toolbar && let Some(action) = self.toolbar_state.pending_action.take() {
			let control = self.handle_toolbar_action(action);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}
		if draw_toolbar && self.toolbar_state.needs_redraw {
			self.toolbar_state.needs_redraw = false;

			self.refresh_frozen_text_ime_cursor_area_for_text_style_change(overlay_monitor);
			self.request_redraw_for_monitor(overlay_monitor);
		}

		OverlayControl::Continue
	}

	fn handle_toolbar_action(&mut self, action: FrozenToolbarTool) -> OverlayControl {
		if self.frozen_text_edit.is_some() {
			let _ = self.finish_frozen_text_editing(true);
		}

		match action {
			FrozenToolbarTool::Undo => {
				let _ = self.perform_frozen_undo();

				OverlayControl::Continue
			},
			FrozenToolbarTool::Redo => {
				let _ = self.perform_frozen_redo();

				OverlayControl::Continue
			},
			FrozenToolbarTool::AutoCenter => {
				self.auto_center_frozen_capture_rect();

				OverlayControl::Continue
			},
			FrozenToolbarTool::Copy => {
				self.begin_png_action(PngAction::Copy);

				OverlayControl::Continue
			},
			FrozenToolbarTool::Save => {
				self.begin_png_action(PngAction::Save);

				OverlayControl::Continue
			},
			FrozenToolbarTool::Scroll => self.start_scroll_capture(),
			#[cfg(target_os = "macos")]
			FrozenToolbarTool::Ocr => self.begin_ocr_action(),
			_ => OverlayControl::Continue,
		}
	}

	fn cancel_overlay(&mut self, reason: &'static str) -> OverlayControl {
		tracing::info!(
			op = "overlay.cancel_requested",
			reason,
			mode = ?self.state.mode,
			scroll_capture_active = self.scroll_capture.active,
			last_event_phase = %self.event_loop_phase.as_str(),
			last_event_window_id = ?self.event_loop_last_progress_window_id,
			last_event_monitor_id = ?self.event_loop_last_progress_monitor_id,
			last_event_detail = ?self.event_loop_last_progress_detail,
			"Overlay cancellation was requested."
		);

		self.exit(OverlayExit::Cancelled)
	}

	fn exit(&mut self, exit: OverlayExit) -> OverlayControl {
		let exit_metadata = Self::exit_metadata(&exit);

		self.log_exit_begin(&exit_metadata);
		self.finalize_scroll_capture_for_exit();

		#[cfg(target_os = "macos")]
		let frontmost_application_before_start = self.frontmost_application_before_start.take();

		self.reset_runtime_for_exit();
		#[cfg(target_os = "macos")]
		self.restore_frontmost_application_after_exit(frontmost_application_before_start);
		self.log_exit_end(&exit_metadata);

		OverlayControl::Exit(exit)
	}

	fn exit_metadata(exit: &OverlayExit) -> OverlayExitMetadata<'_> {
		match exit {
			OverlayExit::Cancelled => OverlayExitMetadata::new("cancelled"),
			OverlayExit::PngBytes(png_bytes) => {
				OverlayExitMetadata::new("png_bytes").with_png_bytes_len(png_bytes.len())
			},
			OverlayExit::TextCopied(_) => OverlayExitMetadata::new("text_copied"),
			#[cfg(target_os = "macos")]
			OverlayExit::DeferredTextRecognition(request) => {
				OverlayExitMetadata::new("deferred_text_recognition")
					.with_ocr_request_id(request.request_id)
			},
			OverlayExit::Saved(path) => {
				OverlayExitMetadata::new("saved").with_saved_path(path.display().to_string())
			},
			OverlayExit::Error(message) => {
				OverlayExitMetadata::new("error").with_error_message(message.as_str())
			},
		}
	}

	fn log_exit_begin(&self, exit_metadata: &OverlayExitMetadata<'_>) {
		#[cfg(target_os = "macos")]
		let scroll_capture_has_live_stream = self.scroll_capture.live_stream.is_some();
		#[cfg(not(target_os = "macos"))]
		let scroll_capture_has_live_stream = false;
		#[cfg(target_os = "macos")]
		let live_sample_stream_present = self.live_sample_stream.is_some();
		#[cfg(not(target_os = "macos"))]
		let live_sample_stream_present = false;

		tracing::info!(
			op = "overlay.exit_begin",
			exit_kind = exit_metadata.exit_kind,
			png_bytes_len = exit_metadata.png_bytes_len,
			saved_path = exit_metadata.saved_path,
			error_message = exit_metadata.error_message,
			ocr_request_id = exit_metadata.ocr_request_id,
			scroll_capture_active = self.scroll_capture.active,
			scroll_capture_has_live_stream,
			live_sample_stream_present,
			last_event_phase = %self.event_loop_phase.as_str(),
			last_event_window_id = ?self.event_loop_last_progress_window_id,
			last_event_monitor_id = ?self.event_loop_last_progress_monitor_id,
			last_event_detail = ?self.event_loop_last_progress_detail,
			"Beginning overlay exit cleanup."
		);
	}

	fn reset_runtime_for_exit(&mut self) {
		#[cfg(target_os = "macos")]
		self.set_scroll_overlay_mouse_passthrough(false);

		self.session_active = false;

		self.windows.clear();

		self.hud_window = None;
		self.hud_inner_size_points = None;
		self.hud_outer_pos = None;
		self.pending_hud_outer_pos = None;
		self.loupe_window = None;
		self.loupe_inner_size_points = None;
		self.loupe_outer_pos = None;
		self.pending_loupe_outer_pos = None;
		self.toolbar_window = None;
		self.scroll_preview_window = None;
		self.toolbar_inner_size_points = None;
		self.toolbar_outer_pos = None;
		self.pending_toolbar_outer_pos = None;
		self.hud_window_visible = false;
		self.toolbar_window_visible = false;
		#[cfg(target_os = "macos")]
		{
			self.toolbar_window_cursor_hittest_enabled = false;
			self.preserve_frontmost_on_next_toolbar_show = false;
		}
		self.skip_toolbar_focus_on_next_show = false;
		self.toolbar_window_warmup_redraws_remaining = 0;
		self.loupe_window_visible = false;
		self.loupe_window_warmup_redraws_remaining = 0;
		self.scroll_capture = ScrollCaptureState::default();
		#[cfg(target_os = "macos")]
		{
			self.frontmost_application_before_start = None;
		}
		self.frozen_capture_source = FrozenCaptureSource::None;
		self.cursor_monitor = None;
		self.gpu = None;
		self.worker = None;
		#[cfg(target_os = "macos")]
		{
			self.live_sample_worker = None;
			self.live_sample_stream = None;
		}
		self.event_loop_phase = OverlayEventLoopPhase::Idle;
		self.event_loop_progress_seq = 0;
		self.event_loop_last_progress_at = Instant::now();
		self.event_loop_last_progress_window_id = None;
		self.event_loop_last_progress_monitor_id = None;
		self.event_loop_last_progress_detail = None;
		self.event_loop_last_stall_warn_at = None;

		#[cfg(target_os = "macos")]
		self.macos_hud_window_config_cache.clear();

		self.toolbar_left_button_down = false;
		self.toolbar_left_button_went_down = false;
		self.toolbar_left_button_went_up = false;
		self.toolbar_pointer_local = None;

		self.frozen_text_annotations.clear();
		self.frozen_text_redo_annotations.clear();

		self.frozen_text_edit = None;
		self.frozen_text_recent_input = None;

		self.reset_frozen_transition_timing();
		self.sync_text_input_ime_state();
		self.stop_frozen_selection_drag();
		self.stop_frozen_mosaic_drag();
		self.frozen_edit_undo_stack.clear();
		self.frozen_edit_redo_stack.clear();
		self.frozen_mosaic_undo_stack.clear();
		self.frozen_mosaic_redo_stack.clear();
		self.clear_pending_output_actions();
	}

	fn log_exit_end(&self, exit_metadata: &OverlayExitMetadata<'_>) {
		tracing::info!(
			op = "overlay.exit_end",
			exit_kind = exit_metadata.exit_kind,
			png_bytes_len = exit_metadata.png_bytes_len,
			saved_path = exit_metadata.saved_path,
			error_message = exit_metadata.error_message,
			ocr_request_id = exit_metadata.ocr_request_id,
			"Finished overlay exit cleanup."
		);
	}

	fn clear_pending_output_actions(&mut self) {
		self.pending_encode_png = None;
		self.pending_png_action = None;
		#[cfg(target_os = "macos")]
		{
			self.png_encode_inflight = false;
		}

		self.focused_window_ids.clear();

		self.pending_focus_loss_cleanup = false;
		self.loupe_activation_key_down = false;
		self.keyboard_modifiers = ModifiersState::default();
	}
}

impl Default for OverlaySession {
	fn default() -> Self {
		Self::new()
	}
}

struct FrozenTransitionTimingInfo {
	op: &'static str,
	monitor: Option<MonitorRect>,
	reason: Option<&'static str>,
	source: Option<&'static str>,
	snapshot_age_ms: Option<u128>,
	grace_ms: Option<u128>,
	captured_window_id: Option<u32>,
	message: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LiveClickCaptureTarget {
	capture_rect: Option<RectPoints>,
	window_target: Option<WindowFreezeCaptureTarget>,
}
impl LiveClickCaptureTarget {
	fn fullscreen_fallback() -> Self {
		Self { capture_rect: None, window_target: None }
	}

	fn from_window_hit(monitor: MonitorRect, hit: WindowHit) -> Self {
		Self {
			capture_rect: Some(hit.rect),
			window_target: hit.window_id.map(|window_id| WindowFreezeCaptureTarget {
				monitor,
				window_id,
				rect: hit.rect,
			}),
		}
	}
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MacOSFrontmostApplication {
	process_id: i32,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq)]
struct OverlayCursorRect {
	rect: Rect,
	icon: CursorIcon,
}
#[cfg(target_os = "macos")]
impl OverlayCursorRect {
	const fn new(rect: Rect, icon: CursorIcon) -> Self {
		Self { rect, icon }
	}
}

#[derive(Clone, Debug)]
struct FrozenImagePatch {
	rect: RectPoints,
	before: RgbaImage,
	after: RgbaImage,
}

#[derive(Clone, Debug)]
struct FrozenMosaicEdit {
	preview_patch: FrozenImagePatch,
	window_patch: Option<FrozenImagePatch>,
}

#[derive(Clone, Copy, Debug)]
struct FrozenExportTransform {
	capture_rect: RectPoints,
	scale_x: f32,
	scale_y: f32,
}
impl FrozenExportTransform {
	fn new(capture_rect: RectPoints, export_width: u32, export_height: u32) -> Option<Self> {
		if capture_rect.width == 0
			|| capture_rect.height == 0
			|| export_width == 0
			|| export_height == 0
		{
			return None;
		}

		Some(Self {
			capture_rect,
			scale_x: export_width as f32 / capture_rect.width as f32,
			scale_y: export_height as f32 / capture_rect.height as f32,
		})
	}

	fn point_to_pixels(self, point: Pos2) -> Pos2 {
		Pos2::new(
			(point.x - self.capture_rect.x as f32) * self.scale_x,
			(point.y - self.capture_rect.y as f32) * self.scale_y,
		)
	}

	fn scalar_scale(self) -> f32 {
		(self.scale_x + self.scale_y) * 0.5
	}
}

#[derive(Debug)]
struct OverlayExitMetadata<'a> {
	exit_kind: &'static str,
	png_bytes_len: Option<usize>,
	saved_path: Option<String>,
	error_message: Option<&'a str>,
	ocr_request_id: Option<u64>,
}
impl<'a> OverlayExitMetadata<'a> {
	fn new(exit_kind: &'static str) -> Self {
		Self {
			exit_kind,
			png_bytes_len: None,
			saved_path: None,
			error_message: None,
			ocr_request_id: None,
		}
	}

	fn with_png_bytes_len(mut self, png_bytes_len: usize) -> Self {
		self.png_bytes_len = Some(png_bytes_len);

		self
	}

	fn with_saved_path(mut self, saved_path: String) -> Self {
		self.saved_path = Some(saved_path);

		self
	}

	fn with_error_message(mut self, error_message: &'a str) -> Self {
		self.error_message = Some(error_message);

		self
	}

	#[cfg(target_os = "macos")]
	fn with_ocr_request_id(mut self, ocr_request_id: u64) -> Self {
		self.ocr_request_id = Some(ocr_request_id);

		self
	}
}

struct InitialSessionRuntime {
	live_bg_request_interval: Duration,
	window_list_refresh_interval: Duration,
	now: Instant,
	loupe_sample_side_px: u32,
	state: OverlayState,
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct MacOSCGPoint {
	x: f64,
	y: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MacOSOverlayPoint {
	x: f64,
	y: f64,
}
#[cfg(target_os = "macos")]
unsafe impl Encode for MacOSOverlayPoint {
	fn encode() -> Encoding {
		unsafe { Encoding::from_str("{CGPoint=dd}") }
	}
}

#[derive(Clone, Copy, Debug)]
struct FrozenArrowGeometry {
	shaft_end: Pos2,
	tip: Pos2,
	head_left: Pos2,
	head_right: Pos2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Selects how the live HUD should be positioned.
pub enum HudAnchor {
	/// Pin the HUD cluster to the current cursor position.
	Cursor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
/// Chooses the requested HUD and chrome theme.
pub enum ThemeMode {
	#[default]
	/// Follow the host window or operating-system theme.
	System,
	/// Force the dark theme variant.
	Dark,
	/// Force the light theme variant.
	Light,
}

#[derive(Debug)]
/// Describes how an overlay session finished.
pub enum OverlayExit {
	/// The user cancelled the session without producing output.
	Cancelled,
	/// The session completed by copying PNG bytes to the caller.
	PngBytes(Vec<u8>),
	/// The session completed by copying recognized text to the clipboard.
	TextCopied(usize),
	/// The session completed by handing OCR work to a background task.
	#[cfg(target_os = "macos")]
	DeferredTextRecognition(DeferredTextRecognitionRequest),
	/// The session completed by saving a file to disk.
	Saved(PathBuf),
	/// The session failed with a user-visible error message.
	Error(String),
}

#[derive(Debug)]
/// Signals whether the caller should keep driving the overlay event loop.
pub enum OverlayControl {
	/// Keep the session alive and continue processing events.
	Continue,
	/// Exit the session with the provided terminal outcome.
	Exit(OverlayExit),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
/// Controls how the Tab-triggered loupe interaction is activated.
pub enum AltActivationMode {
	#[default]
	/// Enable the loupe only while Tab is held.
	Hold,
	/// Toggle the loupe on and off with Tab presses.
	Toggle,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
/// Chooses where the frozen toolbar is anchored relative to the capture.
pub enum ToolbarPlacement {
	/// Render the toolbar above the frozen capture.
	Top,
	#[default]
	/// Render the toolbar below the frozen capture.
	Bottom,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
/// Selects how saved captures are named on disk.
pub enum OutputNaming {
	#[default]
	/// Use the current Unix timestamp in milliseconds.
	Timestamp,
	/// Use a zero-padded incrementing sequence number.
	Sequence,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
/// Controls how transparent window captures are composited before export.
pub enum WindowCaptureAlphaMode {
	#[default]
	/// Preserve the observed screen background behind transparent pixels.
	Background,
	/// Composite transparency against a light matte color.
	MatteLight,
	/// Composite transparency against a dark matte color.
	MatteDark,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Single source of truth for live capture entry.
///
/// State flow:
/// `Idle` -> `HoverWindow` -> `PressPending` -> `DraggingSelection` -> `FrozenFromDrag`
/// `Idle` -> `HoverWindow` -> `PressPending` -> `FrozenFromClick`
///
/// Hover and drag visuals are derived from this state instead of being coordinated through
/// separate button, hover, and drag flags.
enum LiveCaptureInteraction {
	#[default]
	Idle,
	HoverWindow {
		monitor: MonitorRect,
		target: LiveClickCaptureTarget,
	},
	PressPending {
		monitor: MonitorRect,
		press_global: GlobalPoint,
		click_target: Option<LiveClickCaptureTarget>,
		release_global: Option<GlobalPoint>,
		released: bool,
	},
	DraggingSelection {
		monitor: MonitorRect,
		press_global: GlobalPoint,
		current_global: GlobalPoint,
	},
	FrozenFromClick {
		monitor: MonitorRect,
		target: LiveClickCaptureTarget,
	},
	FrozenFromDrag {
		monitor: MonitorRect,
		capture_rect: RectPoints,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OverlayEventLoopPhase {
	Idle,
	WindowEvent,
	AboutToWait,
	RedrawDispatch,
	HudRedraw,
	LoupeRedraw,
	ToolbarRedraw,
	OverlayRedraw,
}
impl OverlayEventLoopPhase {
	const fn as_str(self) -> &'static str {
		match self {
			Self::Idle => "idle",
			Self::WindowEvent => "window_event",
			Self::AboutToWait => "about_to_wait",
			Self::RedrawDispatch => "redraw_dispatch",
			Self::HudRedraw => "hud_redraw",
			Self::LoupeRedraw => "loupe_redraw",
			Self::ToolbarRedraw => "toolbar_redraw",
			Self::OverlayRedraw => "overlay_window_redraw",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HudTheme {
	Dark,
	Light,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrozenToolbarTool {
	Pointer,
	Pen,
	Arrow,
	Text,
	Mosaic,
	Spotlight,
	Undo,
	Redo,
	AutoCenter,
	Scroll,
	#[cfg(target_os = "macos")]
	Ocr,
	Copy,
	Save,
}
impl FrozenToolbarTool {
	const fn label(self) -> &'static str {
		match self {
			Self::Pointer => "Pointer",
			Self::Pen => "Pen",
			Self::Arrow => "Arrow",
			Self::Text => "Text",
			Self::Mosaic => "Mosaic",
			Self::Spotlight => "Spotlight",
			Self::Undo => "Undo",
			Self::Redo => "Redo",
			Self::AutoCenter => "Auto-center (C)",
			Self::Scroll => "Scroll Capture",
			#[cfg(target_os = "macos")]
			Self::Ocr => "Recognize Text",
			Self::Copy => "Copy",
			Self::Save => "Save",
		}
	}

	const fn icon(self) -> &'static str {
		match self {
			Self::Pointer => regular::CURSOR,
			Self::Pen => regular::PENCIL_SIMPLE,
			Self::Arrow => regular::ARROW_UP_RIGHT,
			Self::Text => regular::TEXT_T,
			Self::Mosaic => regular::CHECKERBOARD,
			Self::Spotlight => regular::FRAME_CORNERS,
			Self::Undo => regular::ARROW_COUNTER_CLOCKWISE,
			Self::Redo => regular::ARROW_CLOCKWISE,
			Self::AutoCenter => regular::ARROWS_IN_CARDINAL,
			Self::Scroll => regular::ARROWS_DOWN_UP,
			#[cfg(target_os = "macos")]
			Self::Ocr => regular::FILE_TEXT,
			Self::Copy => regular::COPY,
			Self::Save => regular::FLOPPY_DISK,
		}
	}

	const fn is_mode_tool(self) -> bool {
		matches!(
			self,
			Self::Pointer | Self::Pen | Self::Arrow | Self::Text | Self::Mosaic | Self::Spotlight
		)
	}

	const fn requires_final_capture(self) -> bool {
		match self {
			Self::Pointer
			| Self::Pen
			| Self::Arrow
			| Self::Text
			| Self::AutoCenter
			| Self::Spotlight => false,
			Self::Mosaic | Self::Undo | Self::Redo => true,
			Self::Scroll | Self::Copy | Self::Save => true,
			#[cfg(target_os = "macos")]
			Self::Ocr => true,
		}
	}

	fn is_available(self, toolbar_state: &FrozenToolbarState) -> bool {
		match self {
			Self::Undo => toolbar_state.undo_available,
			Self::Redo => toolbar_state.redo_available,
			_ => true,
		}
	}

	fn unavailable_label(self, toolbar_state: &FrozenToolbarState) -> &'static str {
		if self.requires_final_capture() && !toolbar_state.final_capture_ready {
			return "Preparing capture...";
		}

		match self {
			Self::Undo => "Nothing to undo",
			Self::Redo => "Nothing to redo",
			_ => "Preparing capture...",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollCaptureFrameSource {
	Worker { request_id: u64 },
	LiveStream { frame_seq: u64 },
}
impl ScrollCaptureFrameSource {
	const fn as_str(self) -> &'static str {
		match self {
			Self::Worker { .. } => "worker",
			Self::LiveStream { .. } => "live_stream",
		}
	}

	const fn worker_request_id(self) -> Option<u64> {
		match self {
			Self::Worker { request_id } => Some(request_id),
			Self::LiveStream { .. } => None,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PngAction {
	Copy,
	Save,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum FrozenCaptureSource {
	#[default]
	None,
	DragRegion,
	Window,
	FullscreenFallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrozenSelectionCorner {
	TopLeft,
	TopRight,
	BottomLeft,
	BottomRight,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum FrozenSelectionInteractionKind {
	#[default]
	Move,
	Resize(FrozenSelectionCorner),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeviceCursorPointSource {
	DevicePoints,
	DevicePixelsFallback,
	EventRecentFallback,
}
impl DeviceCursorPointSource {
	const fn as_str(self) -> &'static str {
		match self {
			Self::DevicePoints => "device_points",
			Self::DevicePixelsFallback => "device_pixels_fallback",
			Self::EventRecentFallback => "event_recent_fallback",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectionFlowStyle {
	Band,
}

#[derive(Clone, Copy, Debug)]
enum WindowRendererPath {
	Overlay,
	LoupeTile,
}
impl WindowRendererPath {
	const fn as_str(self) -> &'static str {
		match self {
			Self::Overlay => "overlay",
			Self::LoupeTile => "loupe_tile",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SurfaceFrameSkipReason {
	Timeout,
	Occluded,
}
impl SurfaceFrameSkipReason {
	const fn as_str(self) -> &'static str {
		match self {
			Self::Timeout => "timeout",
			Self::Occluded => "occluded",
		}
	}

	const fn should_request_redraw(self) -> bool {
		matches!(self, Self::Timeout)
	}
}

enum AcquiredSurfaceFrame {
	Ready(SurfaceTexture),
	Skipped(SurfaceFrameSkipReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrozenEditKind {
	BrushStroke,
	MosaicEdit,
	TextAnnotation,
	ArrowAnnotation,
	SpotlightAnnotation,
}

#[derive(Clone, Copy, Debug)]
enum FrozenCommittedOverlay<'a> {
	Brush(&'a FrozenBrushStroke),
	Text(&'a FrozenTextAnnotation),
	Arrow(&'a FrozenArrowAnnotation),
}

pub(super) fn frozen_toolbar_corner_radius_u8(toolbar_height_points: f32) -> u8 {
	if toolbar_height_points <= TOOLBAR_EXPANDED_HEIGHT_PX + 0.5 {
		(toolbar_height_points * 0.5).round().clamp(1.0, f32::from(u8::MAX)) as u8
	} else {
		HUD_PILL_CORNER_RADIUS_POINTS
	}
}

pub(super) fn frozen_toolbar_corner_radius_points(toolbar_height_points: f32) -> f64 {
	f64::from(frozen_toolbar_corner_radius_u8(toolbar_height_points))
}

#[cfg(target_os = "macos")]
pub(in crate::overlay) fn frozen_toolbar_window_primary_origin() -> Pos2 {
	Pos2::new(0.0, WindowRenderer::frozen_toolbar_window_top_padding_points())
}

fn frozen_toolbar_window_startup_size_points() -> Vec2 {
	[
		FrozenToolbarState::default(),
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Pen,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Arrow,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Text,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState { auto_center_available: true, ..FrozenToolbarState::default() },
		FrozenToolbarState { scroll_capture_available: true, ..FrozenToolbarState::default() },
		FrozenToolbarState {
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Pen,
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Arrow,
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Text,
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			scroll_capture_active: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
	]
	.into_iter()
	.map(|toolbar_state| WindowRenderer::frozen_toolbar_size(&toolbar_state))
	.fold(Vec2::new(0.0, TOOLBAR_EXPANDED_HEIGHT_PX), |max_size, size| {
		Vec2::new(max_size.x.max(size.x), max_size.y.max(size.y))
	}) + Vec2::new(0.0, WindowRenderer::frozen_toolbar_window_top_padding_points())
}

#[cfg(target_os = "macos")]
fn overlay_cursor_rect_icon_at_point(
	rects: &[OverlayCursorRect],
	point: Pos2,
) -> Option<CursorIcon> {
	rects.iter().find(|entry| entry.rect.contains(point)).map(|entry| entry.icon)
}

#[cfg(target_os = "macos")]
fn sort_unique_axis_values(values: &mut Vec<f32>) {
	values.sort_by(f32::total_cmp);
	values.dedup_by(|a, b| (*a - *b).abs() <= f32::EPSILON);
}

#[cfg(target_os = "macos")]
fn trim_rect_min_edge(min: f32, max: f32) -> f32 {
	let trimmed = min.next_up();

	if trimmed < max { trimmed } else { max }
}

#[cfg(target_os = "macos")]
fn trim_rect_max_edge(max: f32, min: f32) -> f32 {
	let trimmed = max.next_down();

	if trimmed > min { trimmed } else { min }
}

fn should_request_overlay_redraw_after_surface_skip(
	reason: SurfaceFrameSkipReason,
	now: Instant,
	occluded_redraw_retry_until: &mut Option<Instant>,
) -> bool {
	match reason {
		SurfaceFrameSkipReason::Timeout => true,
		SurfaceFrameSkipReason::Occluded => match occluded_redraw_retry_until {
			Some(deadline) if now >= *deadline => {
				*occluded_redraw_retry_until = None;

				false
			},
			Some(_) => true,
			None => {
				*occluded_redraw_retry_until = Some(now + OCCLUDED_FRAME_REDRAW_RETRY_WINDOW);

				true
			},
		},
	}
}

fn frozen_toolbar_needs_new_sample(
	last_screen_size_points: Option<Vec2>,
	screen_size_points: Vec2,
) -> bool {
	match last_screen_size_points {
		None => true,
		Some(last) => {
			let dx = (last.x - screen_size_points.x).abs();
			let dy = (last.y - screen_size_points.y).abs();

			dx > 0.5 || dy > 0.5
		},
	}
}

fn advance_frozen_toolbar_readiness_sample_state(
	toolbar_state: &mut FrozenToolbarState,
	screen_rect: Rect,
) -> bool {
	let screen_size_points = screen_rect.size();

	if frozen_toolbar_needs_new_sample(
		toolbar_state.layout_last_screen_size_points,
		screen_size_points,
	) {
		toolbar_state.layout_last_screen_size_points = Some(screen_size_points);
		toolbar_state.layout_stable_frames = 0;

		return false;
	}
	if toolbar_state.layout_stable_frames < 1 {
		toolbar_state.layout_stable_frames = toolbar_state.layout_stable_frames.saturating_add(1);

		return false;
	}

	true
}

fn frozen_toolbar_matches_default_slot(toolbar_pos: Pos2, default_pos: Pos2) -> bool {
	let dx = (toolbar_pos.x - default_pos.x).abs();
	let dy = (toolbar_pos.y - default_pos.y).abs();

	dx <= TOOLBAR_DEFAULT_SLOT_POSITION_EPSILON_POINTS
		&& dy <= TOOLBAR_DEFAULT_SLOT_POSITION_EPSILON_POINTS
}

#[cfg(target_os = "macos")]
fn macos_hid_event_source_state_id() -> u32 {
	KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE
}

fn global_to_local(cursor: GlobalPoint, monitor: MonitorRect) -> Option<Pos2> {
	let (x, y) = monitor.local_u32(cursor)?;

	Some(Pos2::new(x as f32, y as f32))
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
	fn CGEventGetLocation(event: CGEventRef) -> MacOSCGPoint;
	fn CGEventCreate(source: *const c_void) -> CGEventRef;
	fn CGEventSourceCreate(source_state_id: u32) -> CFTypeRef;
	fn CGEventCreateScrollWheelEvent2(
		source: *const c_void,
		units: u32,
		wheel_count: u32,
		wheel1: i32,
		wheel2: i32,
		wheel3: i32,
	) -> CGEventRef;
	fn CGEventPost(tap_location: u32, event: CGEventRef);
	fn CGEventSetLocation(event: CGEventRef, location: MacOSCGPoint);
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	fn CFRelease(obj: CFTypeRef);
}

#[cfg(target_os = "macos")]
fn macos_install_overlay_cursor_rect_support(
	window: &Window,
) -> std::result::Result<MacOSOverlayCursorRectSupport, String> {
	let _ = MainThreadMarker::new().ok_or_else(|| {
		String::from("Installing macOS overlay cursor rect support requires the main thread.")
	})?;
	let Some(host_view) = macos_overlay_window_ns_view(window) else {
		return Err(String::from("Overlay cursor rect support requires an AppKit window handle."));
	};
	let bounds: NSRect = unsafe { objc::msg_send![host_view, bounds] };
	let overlay_class = macos_overlay_cursor_view_class();
	let overlay_view: *mut Object = unsafe {
		let overlay_view: *mut Object = objc::msg_send![overlay_class, alloc];

		objc::msg_send![overlay_view, initWithFrame: bounds]
	};

	if overlay_view.is_null() {
		return Err(String::from("Failed to create macOS overlay cursor view."));
	}

	unsafe {
		const NS_VIEW_WIDTH_SIZABLE: usize = 2;
		const NS_VIEW_HEIGHT_SIZABLE: usize = 16;

		let _: () = objc::msg_send![overlay_view, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
		let _: () = objc::msg_send![host_view, addSubview: overlay_view];
		let _: () = objc::msg_send![overlay_view, release];
	}

	Ok(MacOSOverlayCursorRectSupport::new(overlay_view as usize))
}

#[cfg(target_os = "macos")]
fn macos_mouse_location() -> Option<GlobalPoint> {
	let event = unsafe { CGEventCreate(ptr::null()) };

	if event.is_null() {
		return None;
	}

	let point = unsafe { CGEventGetLocation(event) };

	unsafe { CFRelease(event) };

	Some(GlobalPoint::new(point.x as i32, point.y as i32))
}

#[cfg(target_os = "macos")]
fn macos_overlay_view_cursor_rects() -> &'static Mutex<HashMap<usize, Vec<OverlayCursorRect>>> {
	static RECTS: OnceLock<Mutex<HashMap<usize, Vec<OverlayCursorRect>>>> = OnceLock::new();

	RECTS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "macos")]
fn macos_set_overlay_view_cursor_rects(view_key: usize, rects: &[OverlayCursorRect]) -> bool {
	let rects_by_view = macos_overlay_view_cursor_rects();

	match rects_by_view.lock() {
		Ok(mut guard) => {
			let unchanged =
				guard.get(&view_key).is_some_and(|existing| existing.as_slice() == rects);

			if unchanged || (rects.is_empty() && !guard.contains_key(&view_key)) {
				return false;
			}
			if rects.is_empty() {
				guard.remove(&view_key);
			} else {
				guard.insert(view_key, rects.to_vec());
			}
		},
		Err(poisoned) => {
			let mut guard = poisoned.into_inner();
			let unchanged =
				guard.get(&view_key).is_some_and(|existing| existing.as_slice() == rects);

			if unchanged || (rects.is_empty() && !guard.contains_key(&view_key)) {
				return false;
			}
			if rects.is_empty() {
				guard.remove(&view_key);
			} else {
				guard.insert(view_key, rects.to_vec());
			}
		},
	}

	true
}

#[cfg(target_os = "macos")]
fn macos_overlay_view_cursor_rect_entries(view_key: usize) -> Option<Vec<OverlayCursorRect>> {
	let rects = macos_overlay_view_cursor_rects();

	match rects.lock() {
		Ok(guard) => guard.get(&view_key).cloned(),
		Err(poisoned) => poisoned.into_inner().get(&view_key).cloned(),
	}
}

#[cfg(target_os = "macos")]
fn macos_cursor_object_for_icon(icon: CursorIcon) -> *mut Object {
	let cursor_class = objc::class!(NSCursor);

	match icon {
		CursorIcon::Crosshair => unsafe { objc::msg_send![cursor_class, crosshairCursor] },
		CursorIcon::Grab => unsafe { objc::msg_send![cursor_class, openHandCursor] },
		CursorIcon::Grabbing => unsafe { objc::msg_send![cursor_class, closedHandCursor] },
		CursorIcon::NeswResize => unsafe {
			let responds: bool = objc::msg_send![cursor_class, respondsToSelector: objc::sel!(_windowResizeNorthEastSouthWestCursor)];

			if responds {
				objc::msg_send![cursor_class, performSelector: objc::sel!(_windowResizeNorthEastSouthWestCursor)]
			} else {
				objc::msg_send![cursor_class, arrowCursor]
			}
		},
		CursorIcon::NwseResize => unsafe {
			let responds: bool = objc::msg_send![cursor_class, respondsToSelector: objc::sel!(_windowResizeNorthWestSouthEastCursor)];

			if responds {
				objc::msg_send![cursor_class, performSelector: objc::sel!(_windowResizeNorthWestSouthEastCursor)]
			} else {
				objc::msg_send![cursor_class, arrowCursor]
			}
		},
		_ => unsafe { objc::msg_send![cursor_class, arrowCursor] },
	}
}

#[cfg(target_os = "macos")]
extern "C" fn macos_overlay_cursor_view_is_flipped(_this: &Object, _cmd: Sel) -> BOOL {
	let _ = _cmd;

	YES
}

#[cfg(target_os = "macos")]
extern "C" fn macos_overlay_cursor_view_hit_test(
	_this: &Object,
	_cmd: Sel,
	_point: MacOSOverlayPoint,
) -> *mut Object {
	let _ = (_cmd, _point);

	ptr::null_mut()
}

#[cfg(target_os = "macos")]
extern "C" fn macos_overlay_cursor_view_reset_cursor_rects(this: &Object, _cmd: Sel) {
	let _ = _cmd;
	let view_key = (this as *const Object) as usize;
	let Some(entries) = macos_overlay_view_cursor_rect_entries(view_key) else {
		return;
	};

	for entry in entries {
		let cursor = macos_cursor_object_for_icon(entry.icon);

		if cursor.is_null() {
			continue;
		}

		let rect = NSRect::new(
			NSPoint::new(f64::from(entry.rect.min.x), f64::from(entry.rect.min.y)),
			NSSize::new(f64::from(entry.rect.width()), f64::from(entry.rect.height())),
		);

		unsafe {
			let _: () = objc::msg_send![this, addCursorRect: rect cursor: cursor];
		}
	}
}

#[cfg(target_os = "macos")]
fn macos_overlay_cursor_view_class() -> *const Class {
	static CLASS: OnceLock<usize> = OnceLock::new();

	(*CLASS.get_or_init(|| {
		if let Some(class) = Class::get("RsnapOverlayCursorView") {
			return class as *const Class as usize;
		}

		let superclass = objc::class!(NSView);
		let mut decl = ClassDecl::new("RsnapOverlayCursorView", superclass)
			.expect("cursor overlay view class");

		unsafe {
			decl.add_method(
				objc::sel!(isFlipped),
				macos_overlay_cursor_view_is_flipped as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(hitTest:),
				macos_overlay_cursor_view_hit_test
					as extern "C" fn(&Object, Sel, MacOSOverlayPoint) -> *mut Object,
			);
			decl.add_method(
				objc::sel!(resetCursorRects),
				macos_overlay_cursor_view_reset_cursor_rects as extern "C" fn(&Object, Sel),
			);
		}

		decl.register() as *const Class as usize
	})) as *const Class
}

#[cfg(target_os = "macos")]
fn macos_overlay_window_ns_view(window: &Window) -> Option<*mut Object> {
	let Ok(handle) = window.window_handle() else {
		return None;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return None;
	};

	Some(appkit.ns_view.as_ptr().cast::<Object>())
}

#[cfg(target_os = "macos")]
fn macos_resize_overlay_cursor_view(window: &Window, overlay_view_key: usize) {
	let Some(ns_view) = macos_overlay_window_ns_view(window) else {
		return;
	};
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return;
	}

	let bounds: NSRect = unsafe { objc::msg_send![ns_view, bounds] };

	unsafe {
		let _: () = objc::msg_send![overlay_view, setFrame: bounds];
	}
}

#[cfg(target_os = "macos")]
fn macos_invalidate_overlay_cursor_rects(overlay_view_key: usize) {
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return;
	}

	unsafe {
		let ns_window: *mut Object = objc::msg_send![overlay_view, window];

		if ns_window.is_null() {
			return;
		}

		let _: () = objc::msg_send![ns_window, invalidateCursorRectsForView: overlay_view];
	}
}

#[cfg(target_os = "macos")]
fn macos_overlay_view_current_local_point(overlay_view_key: usize) -> Option<Pos2> {
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return None;
	}

	unsafe {
		let ns_window: *mut Object = objc::msg_send![overlay_view, window];

		if ns_window.is_null() {
			return None;
		}

		let window_point: NSPoint = objc::msg_send![ns_window, mouseLocationOutsideOfEventStream];
		let local_point: NSPoint = objc::msg_send![overlay_view, convertPoint: window_point fromView: ptr::null_mut::<Object>()];

		Some(Pos2::new(local_point.x as f32, local_point.y as f32))
	}
}

#[cfg(target_os = "macos")]
fn macos_overlay_view_bounds(overlay_view_key: usize) -> Option<Rect> {
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return None;
	}

	unsafe {
		let bounds: NSRect = objc::msg_send![overlay_view, bounds];

		Some(Rect::from_min_size(
			Pos2::new(bounds.origin.x as f32, bounds.origin.y as f32),
			Vec2::new(bounds.size.width as f32, bounds.size.height as f32),
		))
	}
}

#[cfg(target_os = "macos")]
fn macos_cursor_icon_for_current_pointer(
	entries: Option<&[OverlayCursorRect]>,
	local_point: Option<Pos2>,
	overlay_bounds: Option<Rect>,
) -> Option<CursorIcon> {
	let local_point = local_point?;
	let overlay_bounds = overlay_bounds?;

	if !overlay_bounds.contains(local_point) {
		return None;
	}

	Some(match entries {
		Some(entries) => {
			overlay_cursor_rect_icon_at_point(entries, local_point).unwrap_or(CursorIcon::Default)
		},
		None => CursorIcon::Default,
	})
}

#[cfg(target_os = "macos")]
fn macos_apply_overlay_cursor_for_current_pointer(overlay_view_key: usize) {
	let entries = macos_overlay_view_cursor_rect_entries(overlay_view_key);
	let local_point = macos_overlay_view_current_local_point(overlay_view_key);
	let overlay_bounds = macos_overlay_view_bounds(overlay_view_key);
	let Some(icon) =
		macos_cursor_icon_for_current_pointer(entries.as_deref(), local_point, overlay_bounds)
	else {
		return;
	};
	let cursor = macos_cursor_object_for_icon(icon);

	if cursor.is_null() {
		return;
	}

	unsafe {
		let _: () = objc::msg_send![cursor, set];
	}
}

#[cfg(target_os = "macos")]
fn macos_remove_overlay_cursor_view(overlay_view_key: usize) {
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return;
	}

	unsafe {
		let superview: *mut Object = objc::msg_send![overlay_view, superview];

		if !superview.is_null() {
			let _: () = objc::msg_send![overlay_view, removeFromSuperview];
		}
	}
}

#[cfg(target_os = "macos")]
fn macos_activate_app() {
	unsafe {
		let app: *mut Object = objc::msg_send![objc::class!(NSApplication), sharedApplication];

		if app.is_null() {
			return;
		}

		let _: () = objc::msg_send![app, activateIgnoringOtherApps: YES];
	}
}

#[cfg(target_os = "macos")]
fn macos_frontmost_application() -> Option<MacOSFrontmostApplication> {
	unsafe {
		let workspace: *mut Object = objc::msg_send![objc::class!(NSWorkspace), sharedWorkspace];

		if workspace.is_null() {
			return None;
		}

		let app: *mut Object = objc::msg_send![workspace, frontmostApplication];

		if app.is_null() {
			return None;
		}

		let process_id: i32 = objc::msg_send![app, processIdentifier];

		(process_id > 0).then_some(MacOSFrontmostApplication { process_id })
	}
}

#[cfg(target_os = "macos")]
fn macos_restore_frontmost_application(target: MacOSFrontmostApplication) -> bool {
	if target.process_id == process::id() as i32 {
		macos_activate_app();

		return true;
	}

	unsafe {
		let running_application_class = objc::class!(NSRunningApplication);
		let app: *mut Object = objc::msg_send![
			running_application_class,
			runningApplicationWithProcessIdentifier: target.process_id
		];

		if app.is_null() {
			return false;
		}

		let options: usize = 1 << 1;
		let activated: BOOL = objc::msg_send![app, activateWithOptions: options];

		activated == YES
	}
}

#[cfg(target_os = "macos")]
fn macos_make_window_key(window: &Window) {
	let Ok(handle) = window.window_handle() else {
		return;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return;
	};
	let ns_view = appkit.ns_view.as_ptr().cast::<Object>();

	unsafe {
		let ns_window: *mut Object = objc::msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		let nil: *mut Object = ptr::null_mut();
		let _: () = objc::msg_send![ns_window, makeKeyAndOrderFront: nil];
	}

	window.focus_window();
}

#[cfg(target_os = "macos")]
fn macos_post_scroll_wheel_event(
	delta: MacOSScrollWheelEvent,
	target_point: GlobalPoint,
) -> color_eyre::eyre::Result<()> {
	let units = delta.units;
	let wheel1 = delta.posted_y;
	let wheel2 = delta.posted_x;

	if wheel1 == 0 && wheel2 == 0 {
		return Ok(());
	}

	let source = unsafe { CGEventSourceCreate(macos_hid_event_source_state_id()) };

	if source.is_null() {
		return Err(eyre::eyre!("failed to create macOS scroll wheel event source"));
	}

	let wheel_count = if wheel2 != 0 { 2 } else { 1 };
	let event =
		unsafe { CGEventCreateScrollWheelEvent2(source, units, wheel_count, wheel1, wheel2, 0) };

	if event.is_null() {
		unsafe {
			CFRelease(source);
		}

		return Err(eyre::eyre!("failed to create macOS scroll wheel event"));
	}

	unsafe {
		CGEventSetLocation(
			event,
			MacOSCGPoint { x: f64::from(target_point.x), y: f64::from(target_point.y) },
		);
		CGEventPost(KCG_HID_EVENT_TAP, event);
		CFRelease(event);
		CFRelease(source);
	}

	Ok(())
}

#[cfg(target_os = "macos")]
fn macos_configure_overlay_window_mouse_moved_events(window: &Window) {
	let Ok(handle) = window.window_handle() else {
		return;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return;
	};
	let ns_view = appkit.ns_view.as_ptr().cast::<Object>();

	unsafe {
		let ns_window: *mut Object = objc::msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		macos_configure_nonactivating_capture_window_with_ns_window(ns_window);

		let _: () = objc::msg_send![ns_window, setOpaque: false];
		let _: () = objc::msg_send![ns_window, setHasShadow: false];
		let clear: *mut Object = objc::msg_send![objc::class!(NSColor), clearColor];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let _: () = objc::msg_send![ns_window, setLevel: MACOS_OVERLAY_WINDOW_LEVEL];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: YES];
	}
}

#[cfg(target_os = "macos")]
fn macos_configure_hud_window(
	window: &Window,
	blur_enabled: bool,
	blur_amount: f32,
	corner_radius_points: Option<f64>,
) {
	let Ok(handle) = window.window_handle() else {
		return;
	};
	let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
		return;
	};
	let ns_view = appkit.ns_view.as_ptr().cast::<Object>();

	unsafe {
		let ns_window: *mut Object = objc::msg_send![ns_view, window];

		if ns_window.is_null() {
			return;
		}

		macos_configure_nonactivating_capture_window_with_ns_window(ns_window);

		// winit exposes blur as a boolean. We also set an explicit radius so we can drive it from
		// settings (this uses the same private CGS API that winit uses internally).
		{
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
			let window_number: isize = objc::msg_send![ns_window, windowNumber];
			let _ = CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), window_number, radius);
		}

		let _: () = objc::msg_send![ns_window, setOpaque: false];
		let _: () = objc::msg_send![ns_window, setHasShadow: false];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: YES];
		let _: () = objc::msg_send![ns_window, setLevel: MACOS_HUD_WINDOW_LEVEL];
		let clear: *mut Object = objc::msg_send![objc::class!(NSColor), clearColor];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let content_view: *mut Object = objc::msg_send![ns_window, contentView];

		if content_view.is_null() {
			return;
		}

		let _: () = objc::msg_send![content_view, setWantsLayer: YES];
		let layer: *mut Object = objc::msg_send![content_view, layer];

		if layer.is_null() {
			return;
		}

		// Round the window itself so native blur doesn't show a rectangular boundary.
		let scale = window.scale_factor().max(1.0);
		let size = window.inner_size();
		let height_points = (size.height as f64) / scale;
		let radius = corner_radius_points.unwrap_or(height_points * 0.5);
		let _: () = objc::msg_send![layer, setCornerRadius: radius];
		let _: () = objc::msg_send![layer, setMasksToBounds: YES];
	}
}

#[cfg(target_os = "macos")]
fn macos_configure_nonactivating_capture_window_with_ns_window(ns_window: *mut Object) {
	if ns_window.is_null() {
		return;
	}

	unsafe {
		let style_mask: usize = objc::msg_send![ns_window, styleMask];
		let nonactivating_panel_mask: usize = 1 << 7;
		let _: () = objc::msg_send![ns_window, setStyleMask: style_mask | nonactivating_panel_mask];
		let _: () = objc::msg_send![ns_window, setHidesOnDeactivate: false];
	}
}

#[cfg(test)]
mod tests;
