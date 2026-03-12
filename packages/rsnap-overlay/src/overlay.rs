mod hud_helpers;
mod image_helpers;
mod output;
mod scroll_runtime;
mod session_state;
mod window_runtime;

#[cfg(target_os = "macos")]
use std::ffi::c_void;
use std::mem;
use std::panic;
use std::ptr;
use std::slice;
use std::{
	collections::HashMap,
	path::PathBuf,
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Result, WrapErr};
#[cfg(not(target_os = "macos"))]
use device_query::{DeviceQuery, Keycode};
use egui::ClippedPrimitive;
use egui::ColorImage;
use egui::FullOutput;
use egui::Painter;
use egui::TextureHandle;
use egui::TextureId;
use egui::TextureOptions;
use egui::Ui;
use egui::{
	self, Align, Align2, Color32, CornerRadius, Event, FontDefinitions, FontFamily, FontId, Frame,
	Layout, Margin, PointerButton, Pos2, Rect, Vec2,
};
use egui_phosphor::{Variant, regular};
use egui_wgpu::{Renderer, ScreenDescriptor};
use image::{
	RgbaImage,
	imageops::{self, FilterType},
};
#[cfg(target_os = "macos")]
use objc::runtime::{Object, YES};
#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_app_kit::NSScreen;
#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
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
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::KeyEvent;
use winit::{
	dpi::PhysicalSize,
	event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
	event_loop::ActiveEventLoop,
	keyboard::{Key, ModifiersState, NamedKey},
	window::{WindowId, WindowLevel},
};

#[cfg(all(target_os = "macos", test))]
use self::session_state::InflightScrollCaptureObservation;
use self::session_state::{
	CursorMoveTrace, FrozenToolbarPointerState, FrozenToolbarState, HudDrawConfig,
	LiveSampleApplyResult, ScrollCaptureState, SlowOperationLogger, WindowFreezeCaptureTarget,
};
#[cfg(target_os = "macos")]
use self::session_state::{
	LiveStreamStaleGrace, MacOSHudWindowConfigState, MacOSScrollPixelResidual,
	MacOSScrollWheelEvent,
};
#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::MacLiveFrameStream;
use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome, ScrollSession};
use crate::state::LiveCursorSample;
use crate::{
	state::{
		GlobalPoint, MonitorRect, MonitorRectPoints, OverlayMode, OverlayState, RectPoints,
		WindowHit, WindowListSnapshot,
	},
	worker::{
		CapturedMonitorRegionResult, FreezeCaptureTarget, OverlayWorker, WorkerRequestSendError,
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
#[cfg(target_os = "macos")]
const MACOS_HUD_WINDOW_LEVEL: isize = 26;
#[cfg(target_os = "macos")]
const MACOS_OVERLAY_WINDOW_LEVEL: isize = 25;
const FROZEN_TOOLBAR_BUTTON_SIZE_POINTS: f32 = 24.0;
const FROZEN_TOOLBAR_ITEM_SPACING_POINTS: f32 = 4.0;
const TOOLBAR_MAX_TOOL_COUNT: usize = 9;
const LIVE_EVENT_CURSOR_CACHE_TTL: Duration = Duration::from_millis(120);
const CURSOR_EVENT_TICK_TTL: Duration = Duration::from_millis(24);
const LIVE_HOVER_HIT_TEST_INTERVAL: Duration = Duration::from_millis(60);
const LIVE_WINDOW_LIST_REFRESH_INTERVAL: Duration = Duration::from_millis(120);
const LIVE_PRESENT_INTERVAL_MIN: Duration = Duration::from_nanos(8_333_333);
const HUD_LOUPE_MOVE_INTERVAL_MIN: Duration = LIVE_PRESENT_INTERVAL_MIN;
const CURSOR_POLL_INTERVAL_MIN: Duration = LIVE_PRESENT_INTERVAL_MIN;
const OVERLAY_EVENT_LOOP_STALL_THRESHOLD: Duration = Duration::from_millis(250);
#[cfg(target_os = "macos")]
const SLOW_OP_WARN_CURSOR_LOCATION: Duration = Duration::from_millis(8);
#[cfg(target_os = "macos")]
const SLOW_OP_WARN_HUD_CONFIG: Duration = Duration::from_millis(40);
const SLOW_OP_WARN_OUTER_POSITION: Duration = Duration::from_millis(24);
const SLOW_OP_WARN_RENDER: Duration = Duration::from_millis(24);
const SLOW_OP_WARN_WINDOW_EVENT: Duration = Duration::from_millis(40);
const SLOW_OP_WARN_INTERVAL: Duration = Duration::from_secs(1);
const REDRAW_SUBSTEP_CONTRIBUTION_FLOOR: Duration = Duration::from_millis(4);
const SCROLL_CAPTURE_INPUT_FRESHNESS: Duration = Duration::from_millis(400);
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES: u8 = 3;
const HUD_PILL_INNER_MARGIN_X_POINTS: f32 = 12.0;
const HUD_PILL_INNER_MARGIN_Y_POINTS: f32 = 8.0;
const HUD_PILL_STROKE_WIDTH_POINTS: f32 = 1.0;
const TOOLBAR_EXPANDED_WIDTH_PX: f32 = (TOOLBAR_MAX_TOOL_COUNT as f32)
	* FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
	+ ((TOOLBAR_MAX_TOOL_COUNT as f32) - 1.0) * FROZEN_TOOLBAR_ITEM_SPACING_POINTS
	+ 2.0 * HUD_PILL_INNER_MARGIN_X_POINTS
	+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
const TOOLBAR_EXPANDED_HEIGHT_PX: f32 = FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
	+ 2.0 * HUD_PILL_INNER_MARGIN_Y_POINTS
	+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
const TOOLBAR_CAPTURE_GAP_PX: f32 = 10.0;
const TOOLBAR_SCREEN_MARGIN_PX: f32 = 10.0;
const HUD_PILL_CORNER_RADIUS_POINTS: u8 = 18;
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
const INTERACTIVE_REPAINT_FPS_FLOOR: f32 = 120.0;
const SELECTION_FLOW_PALETTE: [(u8, u8, u8); 3] = [(94, 200, 255), (165, 103, 255), (255, 150, 60)];
const SELECTION_FLOW_FROZEN_ALPHA_SCALE: f32 = 0.70;
const SELECTION_FLOW_FROZEN_INTENSITY: f32 = 1.25;
const WINDOW_CAPTURE_MATTE_LIGHT_RGBA: image::Rgba<u8> = image::Rgba([246, 246, 246, 255]);
const WINDOW_CAPTURE_MATTE_DARK_RGBA: image::Rgba<u8> = image::Rgba([24, 24, 24, 255]);
const SCROLL_PREVIEW_WINDOW_WIDTH_POINTS: f64 = 260.0;
const SCROLL_PREVIEW_WINDOW_HEIGHT_POINTS: f64 = 360.0;
const SCROLL_PREVIEW_WINDOW_MARGIN_POINTS: i32 = 16;
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_SAMPLE_INTERVAL: Duration = Duration::from_nanos(8_333_333);
#[cfg(not(target_os = "macos"))]
const SCROLL_CAPTURE_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE: Duration = Duration::from_millis(180);
const SCROLL_CAPTURE_PREVIEW_WIDTH_PX: u32 = 320;
#[cfg(target_os = "macos")]
const KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: u32 = 0;
#[cfg(target_os = "macos")]
const KCG_EVENT_FLAGS_MASK_ALTERNATE: u64 = 1_u64 << 19;

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
/// Controls how the Alt-triggered loupe interaction is activated.
pub enum AltActivationMode {
	#[default]
	/// Enable the loupe only while Alt is held.
	Hold,
	/// Toggle the loupe on and off with Alt presses.
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
	#[serde(alias = "preserve")]
	/// Preserve the observed screen background behind transparent pixels.
	Background,
	/// Composite transparency against a light matte color.
	MatteLight,
	/// Composite transparency against a dark matte color.
	MatteDark,
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
	Text,
	Mosaic,
	Undo,
	Redo,
	Scroll,
	Copy,
	Save,
}
impl FrozenToolbarTool {
	const fn label(self) -> &'static str {
		match self {
			Self::Pointer => "Pointer",
			Self::Pen => "Pen",
			Self::Text => "Text",
			Self::Mosaic => "Mosaic",
			Self::Undo => "Undo",
			Self::Redo => "Redo",
			Self::Scroll => "Scroll Capture ↓",
			Self::Copy => "Copy",
			Self::Save => "Save",
		}
	}

	const fn icon(self) -> &'static str {
		match self {
			Self::Pointer => regular::CURSOR,
			Self::Pen => regular::PENCIL_SIMPLE,
			Self::Text => regular::TEXT_T,
			Self::Mosaic => regular::CHECKERBOARD,
			Self::Undo => regular::ARROW_COUNTER_CLOCKWISE,
			Self::Redo => regular::ARROW_CLOCKWISE,
			Self::Scroll => "↓",
			Self::Copy => regular::COPY,
			Self::Save => regular::FLOPPY_DISK,
		}
	}

	const fn is_mode_tool(self) -> bool {
		matches!(self, Self::Pointer | Self::Pen | Self::Text | Self::Mosaic)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollCaptureFrameSource {
	Worker {
		request_id: u64,
	},
	#[cfg(target_os = "macos")]
	LiveStream {
		frame_seq: u64,
	},
}
impl ScrollCaptureFrameSource {
	const fn as_str(self) -> &'static str {
		match self {
			Self::Worker { .. } => "worker",
			#[cfg(target_os = "macos")]
			Self::LiveStream { .. } => "live_stream",
		}
	}

	const fn worker_request_id(self) -> Option<u64> {
		match self {
			Self::Worker { request_id } => Some(request_id),
			#[cfg(target_os = "macos")]
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
	FullBorder,
}

#[derive(Clone, Debug)]
/// Runtime configuration applied to a capture overlay session.
pub struct OverlayConfig {
	/// Positions the live HUD relative to the cursor or another anchor point.
	pub hud_anchor: HudAnchor,
	/// Shows the Alt-key hint chip in the live HUD when enabled.
	pub show_alt_hint_keycap: bool,
	/// Enables blur or its platform fallback for HUD windows.
	pub show_hud_blur: bool,
	/// Enables animated particles around the live selection border.
	pub selection_particles: bool,
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
	/// Selects whether Alt must be held or can toggle the loupe.
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
}
impl Default for OverlayConfig {
	fn default() -> Self {
		Self {
			hud_anchor: HudAnchor::Cursor,
			show_alt_hint_keycap: true,
			show_hud_blur: true,
			selection_particles: true,
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
	cursor_monitor: Option<MonitorRect>,
	egui_repaint_deadline: Arc<Mutex<Option<Instant>>>,
	windows: HashMap<WindowId, OverlayWindow>,
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
	toolbar_inner_size_points: Option<(u32, u32)>,
	gpu: Option<GpuContext>,
	last_hud_window_move_at: Instant,
	last_loupe_window_move_at: Instant,
	last_present_at: Instant,
	last_live_cursor_poll_at: Instant,
	last_frozen_cursor_poll_at: Instant,
	window_list_snapshot: Option<Arc<WindowListSnapshot>>,
	last_window_list_refresh_request_at: Instant,
	window_list_refresh_interval: Duration,
	last_live_bg_request_at: Instant,
	live_bg_request_interval: Duration,
	hit_test_send_full_count: u64,
	hit_test_send_disconnected_count: u64,
	hit_test_request_id: u64,
	live_cursor_sample_request_id: u64,
	latest_live_cursor_sample_request_id: Option<u64>,
	applied_live_cursor_sample_request_id: Option<u64>,
	latest_live_cursor_sample_requested_at: Option<Instant>,
	last_idle_live_sample_request_at: Option<Instant>,
	pending_click_hit_test_request_id: Option<u64>,
	last_live_sample_cursor: Option<GlobalPoint>,
	last_event_cursor: Option<(MonitorRect, GlobalPoint)>,
	last_event_cursor_at: Option<Instant>,
	live_sample_stall_started_at: Option<Instant>,
	last_live_sample_stall_log_at: Option<Instant>,
	slow_op_logger: SlowOperationLogger,
	last_alt_press_at: Option<Instant>,
	alt_modifier_down: bool,
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
	pending_freeze_capture_armed: bool,
	pending_window_freeze_capture: Option<WindowFreezeCaptureTarget>,
	inflight_window_freeze_capture: Option<WindowFreezeCaptureTarget>,
	frozen_window_image: Option<RgbaImage>,
	frozen_capture_source: FrozenCaptureSource,
	capture_windows_hidden: bool,
	pending_encode_png: Option<RgbaImage>,
	pending_png_action: Option<PngAction>,
	toolbar_state: FrozenToolbarState,
	toolbar_left_button_down: bool,
	toolbar_left_button_went_down: bool,
	toolbar_left_button_went_up: bool,
	toolbar_pointer_local: Option<Pos2>,
	left_mouse_button_down: bool,
	left_mouse_button_down_monitor: Option<MonitorRect>,
	left_mouse_button_down_global: Option<GlobalPoint>,
	toolbar_window_visible: bool,
	toolbar_window_warmup_redraws_remaining: u8,
	loupe_window_visible: bool,
	loupe_window_warmup_redraws_remaining: u8,
	scroll_capture: ScrollCaptureState,
	#[cfg(target_os = "macos")]
	scroll_frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	response_waker: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl OverlaySession {
	#[must_use]
	pub(crate) fn new() -> Self {
		Self::with_config(OverlayConfig::default())
	}

	#[must_use]
	/// Creates a new overlay session with the provided runtime configuration.
	pub fn with_config(config: OverlayConfig) -> Self {
		let live_bg_request_interval = Duration::from_millis(500);
		let loupe_sample_side_px =
			Self::normalized_loupe_sample_side_px(config.loupe_sample_side_px);
		let window_list_refresh_interval = LIVE_WINDOW_LIST_REFRESH_INTERVAL;
		let now = Instant::now();
		#[cfg(not(target_os = "macos"))]
		let cursor_device = match panic::catch_unwind(device_query::DeviceState::new) {
			Ok(cursor_device) => Some(cursor_device),
			Err(_) => {
				tracing::warn!(
					op = "overlay.cursor_device_unavailable",
					"Falling back to a headless-safe cursor device stub."
				);

				None
			},
		};
		let mut state = OverlayState::new();

		state.loupe_patch_side_px = loupe_sample_side_px;

		Self {
			config,
			worker: None,
			#[cfg(target_os = "macos")]
			live_sample_worker: None,
			#[cfg(target_os = "macos")]
			live_sample_stream: None,
			#[cfg(not(target_os = "macos"))]
			cursor_device,
			state,
			cursor_monitor: None,
			windows: HashMap::new(),
			hud_window: None,
			loupe_window: None,
			toolbar_window: None,
			scroll_preview_window: None,
			#[cfg(target_os = "macos")]
			macos_hud_window_config_cache: HashMap::new(),
			hud_outer_pos: None,
			pending_hud_outer_pos: None,
			hud_inner_size_points: None,
			loupe_outer_pos: None,
			pending_loupe_outer_pos: None,
			loupe_inner_size_points: None,
			toolbar_outer_pos: None,
			toolbar_inner_size_points: None,
			gpu: None,
			last_hud_window_move_at: now,
			last_loupe_window_move_at: now,
			last_present_at: Instant::now(),
			last_live_cursor_poll_at: now - CURSOR_POLL_INTERVAL_MIN,
			last_frozen_cursor_poll_at: now - CURSOR_POLL_INTERVAL_MIN,
			window_list_snapshot: None,
			last_window_list_refresh_request_at: now - window_list_refresh_interval,
			window_list_refresh_interval,
			last_live_bg_request_at: Instant::now() - live_bg_request_interval,
			live_bg_request_interval,
			hit_test_send_full_count: 0,
			hit_test_send_disconnected_count: 0,
			hit_test_request_id: 0,
			live_cursor_sample_request_id: 0,
			latest_live_cursor_sample_request_id: None,
			applied_live_cursor_sample_request_id: None,
			latest_live_cursor_sample_requested_at: None,
			last_idle_live_sample_request_at: None,
			pending_click_hit_test_request_id: None,
			last_live_sample_cursor: None,
			last_event_cursor: None,
			last_event_cursor_at: None,
			live_sample_stall_started_at: None,
			last_live_sample_stall_log_at: None,
			slow_op_logger: SlowOperationLogger::default(),
			last_alt_press_at: None,
			alt_modifier_down: false,
			keyboard_modifiers: ModifiersState::default(),
			event_loop_phase: OverlayEventLoopPhase::Idle,
			event_loop_progress_seq: 0,
			event_loop_last_progress_at: now,
			event_loop_last_progress_window_id: None,
			event_loop_last_progress_monitor_id: None,
			event_loop_last_progress_detail: None,
			event_loop_last_stall_warn_at: None,
			loupe_patch_width_px: loupe_sample_side_px,
			loupe_patch_height_px: loupe_sample_side_px,
			egui_repaint_deadline: Arc::new(Mutex::new(None)),
			pending_freeze_capture: None,
			pending_freeze_capture_armed: false,
			pending_window_freeze_capture: None,
			inflight_window_freeze_capture: None,
			frozen_window_image: None,
			frozen_capture_source: FrozenCaptureSource::None,
			capture_windows_hidden: false,
			pending_encode_png: None,
			pending_png_action: None,
			toolbar_state: FrozenToolbarState::default(),
			toolbar_left_button_down: false,
			toolbar_left_button_went_down: false,
			toolbar_left_button_went_up: false,
			toolbar_pointer_local: None,
			left_mouse_button_down: false,
			left_mouse_button_down_monitor: None,
			left_mouse_button_down_global: None,
			toolbar_window_visible: false,
			toolbar_window_warmup_redraws_remaining: 0,
			loupe_window_visible: false,
			loupe_window_warmup_redraws_remaining: 0,
			scroll_capture: ScrollCaptureState::default(),
			#[cfg(target_os = "macos")]
			scroll_frame_waker: None,
			response_waker: None,
		}
	}

	#[cfg(target_os = "macos")]
	/// Registers a wake callback for macOS live-stream frame notifications.
	pub fn set_scroll_frame_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
		self.scroll_frame_waker = Some(waker);
	}

	/// Registers a wake callback for worker-thread responses.
	pub fn set_response_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
		self.response_waker = Some(waker);
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

	/// Applies updated runtime configuration to an existing session.
	pub fn set_config(&mut self, config: OverlayConfig) {
		let prev = self.config.clone();
		let previous_loupe_patch = self.loupe_patch_width_px;
		let loupe_sample_side = Self::normalized_loupe_sample_side_px(config.loupe_sample_side_px);

		self.config = config;
		self.loupe_patch_width_px = loupe_sample_side;
		self.loupe_patch_height_px = loupe_sample_side;
		self.state.loupe_patch_side_px = loupe_sample_side;

		let patch_changed = self.loupe_patch_width_px != previous_loupe_patch;

		if patch_changed {
			self.state.loupe = None;
		}
		if !self.is_active() {
			return;
		}

		self.configure_hud_windows_for_config();

		let prev_fake_blur = prev.show_hud_blur && !cfg!(target_os = "macos");
		let new_fake_blur = self.use_fake_hud_blur();

		self.handle_fake_hud_blur_toggle(prev_fake_blur, new_fake_blur);

		if patch_changed {
			self.request_loupe_sample_for_patch_change();
		}

		self.request_redraw_all();
	}

	fn configure_hud_windows_for_config(&mut self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			let window = Arc::clone(&hud_window.window);

			self.configure_hud_window_common(window.as_ref(), None);
		}
		if let Some(loupe_window) = self.loupe_window.as_ref() {
			let window = Arc::clone(&loupe_window.window);

			self.configure_hud_window_common(
				window.as_ref(),
				Some(LOUPE_TILE_CORNER_RADIUS_POINTS),
			);
		}
		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			let window = Arc::clone(&toolbar_window.window);

			self.configure_hud_window_common(
				window.as_ref(),
				Some(f64::from(HUD_PILL_CORNER_RADIUS_POINTS)),
			);
		}
	}

	fn configure_hud_window_common(
		&mut self,
		window: &winit::window::Window,
		corner_radius: Option<f64>,
	) {
		window.set_transparent(true);

		#[cfg(not(target_os = "macos"))]
		let _ = corner_radius;

		#[cfg(not(target_os = "macos"))]
		window.set_blur(self.config.show_hud_blur);
		#[cfg(target_os = "macos")]
		self.configure_macos_hud_window_cached(
			window,
			self.macos_hud_window_blur_enabled(),
			self.config.hud_fog_amount,
			corner_radius,
		);
	}

	#[cfg(target_os = "macos")]
	fn configure_macos_hud_window_cached(
		&mut self,
		window: &winit::window::Window,
		blur_enabled: bool,
		blur_amount: f32,
		corner_radius: Option<f64>,
	) {
		let effective_corner_radius = corner_radius.unwrap_or_else(|| {
			let scale = window.scale_factor().max(1.0);
			let size = window.inner_size();

			((size.height as f64) / scale) * 0.5
		});
		let desired =
			MacOSHudWindowConfigState::new(blur_enabled, blur_amount, effective_corner_radius);

		if self
			.macos_hud_window_config_cache
			.get(&window.id())
			.is_some_and(|cached| cached.same(&desired))
		{
			return;
		}

		let started_at = Instant::now();

		macos_configure_hud_window(
			window,
			blur_enabled,
			blur_amount,
			Some(effective_corner_radius),
		);

		let elapsed = started_at.elapsed();

		self.slow_op_logger.warn_if_slow(
			"overlay.macos_hud_window_configure",
			elapsed,
			SLOW_OP_WARN_HUD_CONFIG,
			|| {
				format!(
					"window_id={:?} blur_enabled={} blur_amount={} corner_radius={effective_corner_radius}",
					window.id(),
					blur_enabled,
					blur_amount,
				)
			},
		);

		let _ = self.macos_hud_window_config_cache.insert(window.id(), desired);
	}

	fn handle_fake_hud_blur_toggle(&mut self, prev_fake_blur: bool, new_fake_blur: bool) {
		if prev_fake_blur == new_fake_blur {
			return;
		}
		if new_fake_blur {
			self.last_live_bg_request_at = Instant::now() - self.live_bg_request_interval;

			if matches!(self.state.mode, OverlayMode::Live)
				&& let Some(_cursor) = self.state.cursor
				&& let Some(monitor) = self.active_cursor_monitor()
			{
				self.maybe_request_live_bg(monitor);
			}

			return;
		}

		self.state.live_bg_monitor = None;
		self.state.live_bg_image = None;
	}

	fn request_loupe_sample_for_patch_change(&mut self) {
		let cursor = match self.state.cursor {
			Some(cursor) => cursor,
			None => return,
		};
		let monitor = match self.active_cursor_monitor() {
			Some(monitor) => monitor,
			None => return,
		};
		let _ = self.apply_live_hover_cache_state(monitor, cursor);
		let _ = self.request_live_cursor_sample(monitor, cursor, true);
		let _ = self.request_live_window_list_refresh_if_needed();
	}

	#[must_use]
	pub(crate) fn is_active(&self) -> bool {
		!self.windows.is_empty()
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

	fn live_loupe_uses_hud_window(&self) -> bool {
		cfg!(target_os = "macos") && matches!(self.state.mode, OverlayMode::Live)
	}

	fn live_loupe_renders_in_hud_window(&self) -> bool {
		self.live_loupe_uses_hud_window() && self.state.alt_held
	}

	fn maybe_tick_loupe_window_warmup_redraw(&mut self) {
		if self.loupe_window_warmup_redraws_remaining == 0 {
			return;
		}
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.loupe_window_visible
			|| self.state.frozen_image.is_none()
			|| self.state.monitor.is_none()
		{
			self.loupe_window_warmup_redraws_remaining = 0;

			return;
		}

		self.loupe_window_warmup_redraws_remaining =
			self.loupe_window_warmup_redraws_remaining.saturating_sub(1);

		self.request_redraw_loupe_window();
		self.schedule_egui_repaint_after(self.repaint_interval_for_monitor(self.state.monitor));
	}

	fn maybe_start_loupe_window_warmup_redraw(&mut self) {
		if self.loupe_window_warmup_redraws_remaining > 0 {
			return;
		}
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.state.alt_held
			|| !self.loupe_window_visible
			|| self.state.frozen_image.is_none()
			|| self.state.monitor.is_none()
		{
			return;
		}

		self.loupe_window_warmup_redraws_remaining = LOUPE_WINDOW_WARMUP_REDRAWS;
	}

	fn reset_loupe_window_warmup_redraws(&mut self) {
		self.loupe_window_warmup_redraws_remaining = 0;
	}

	/// Advances periodic session work before the event loop goes idle.
	pub fn about_to_wait(&mut self) -> OverlayControl {
		let now = Instant::now();

		self.maybe_log_event_loop_stall(now);
		self.mark_progress(OverlayEventLoopPhase::AboutToWait);
		self.maybe_request_keepalive_redraw();
		self.maybe_keep_selection_flow_repaint();

		if self.is_active() {
			self.sync_alt_held_from_global_keys();
		}

		self.maybe_keep_frozen_capture_redraw();
		self.maybe_tick_toolbar_window_warmup_redraw();
		self.maybe_tick_loupe_window_warmup_redraw();
		self.maybe_tick_live_cursor_tracking();
		self.maybe_apply_pending_hud_and_loupe_moves();
		self.maybe_tick_live_sampling();
		self.maybe_tick_frozen_cursor_tracking();
		self.maybe_tick_scroll_capture();
		self.maybe_keep_live_cursor_sample_redraw();

		self.drain_worker_responses()
	}

	fn mark_progress(&mut self, phase: OverlayEventLoopPhase) {
		self.mark_progress_with_detail(phase, None);
	}

	fn mark_progress_with_detail(
		&mut self,
		phase: OverlayEventLoopPhase,
		detail: Option<&'static str>,
	) {
		self.event_loop_phase = phase;
		self.event_loop_last_progress_detail = detail;
		self.event_loop_progress_seq = self.event_loop_progress_seq.saturating_add(1);
		self.event_loop_last_progress_at = Instant::now();
	}

	fn maybe_log_event_loop_stall(&mut self, now: Instant) {
		let stall = now.duration_since(self.event_loop_last_progress_at);

		if stall < OVERLAY_EVENT_LOOP_STALL_THRESHOLD {
			return;
		}
		if self
			.event_loop_last_stall_warn_at
			.is_none_or(|last| now.duration_since(last) >= SLOW_OP_WARN_INTERVAL)
		{
			let _ = self.event_loop_last_stall_warn_at.insert(now);

			tracing::warn!(
				op = "overlay.event_loop_stall",
				stall_ms = stall.as_millis(),
				phase = %self.event_loop_phase.as_str(),
				progress_seq = self.event_loop_progress_seq,
				mode = ?self.state.mode,
				window_id = ?self.event_loop_last_progress_window_id,
				monitor_id = ?self.event_loop_last_progress_monitor_id,
				detail = ?self.event_loop_last_progress_detail,
				"Event loop stalled"
			);
		}
	}

	fn window_event_kind(event: &WindowEvent) -> &'static str {
		match event {
			WindowEvent::ActivationTokenDone { .. } => "activation_token_done",
			WindowEvent::CloseRequested => "close_requested",
			WindowEvent::Destroyed => "destroyed",
			WindowEvent::DroppedFile(_) => "dropped_file",
			WindowEvent::HoveredFile(_) => "hovered_file",
			WindowEvent::HoveredFileCancelled => "hovered_file_cancelled",
			WindowEvent::Focused(_) => "focused",
			WindowEvent::Moved(_) => "moved",
			WindowEvent::Resized(_) => "resized",
			WindowEvent::ScaleFactorChanged { .. } => "scale_factor_changed",
			WindowEvent::Ime(_) => "ime",
			WindowEvent::CursorEntered { .. } => "cursor_entered",
			WindowEvent::CursorLeft { .. } => "cursor_left",
			WindowEvent::CursorMoved { .. } => "cursor_moved",
			WindowEvent::MouseWheel { .. } => "mouse_wheel",
			WindowEvent::MouseInput { .. } => "mouse_input",
			WindowEvent::PinchGesture { .. } => "pinch_gesture",
			WindowEvent::PanGesture { .. } => "pan_gesture",
			WindowEvent::DoubleTapGesture { .. } => "double_tap_gesture",
			WindowEvent::RotationGesture { .. } => "rotation_gesture",
			WindowEvent::TouchpadPressure { .. } => "touchpad_pressure",
			WindowEvent::AxisMotion { .. } => "axis_motion",
			WindowEvent::Touch(_) => "touch",
			WindowEvent::ThemeChanged(_) => "theme_changed",
			WindowEvent::KeyboardInput { .. } => "keyboard_input",
			WindowEvent::ModifiersChanged(_) => "modifiers_changed",
			WindowEvent::Occluded(_) => "occluded",
			WindowEvent::RedrawRequested => "redraw_requested",
		}
	}

	fn maybe_keep_live_cursor_sample_redraw(&mut self) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		let Some(latest_request_id) = self.latest_live_cursor_sample_request_id else {
			return;
		};

		if self.applied_live_cursor_sample_request_id == Some(latest_request_id) {
			return;
		}

		self.schedule_egui_repaint_after(
			self.repaint_interval_for_monitor(self.active_cursor_monitor()),
		);
	}

	fn maybe_keep_selection_flow_repaint(&self) {
		if !self.is_active() || !self.config.selection_particles {
			return;
		}

		let keep_repaint = match self.state.mode {
			OverlayMode::Live => self.live_overlay_selection_flow_repaint_active(),
			OverlayMode::Frozen => self.state.frozen_capture_rect.is_some(),
		};

		if keep_repaint {
			let monitor = match self.state.mode {
				OverlayMode::Live => self.active_cursor_monitor(),
				OverlayMode::Frozen => self.state.monitor,
			};
			let repaint_interval = self.selection_flow_repaint_interval(monitor);

			if let Some(monitor) = monitor {
				self.request_redraw_for_monitor(monitor);
			} else {
				self.request_redraw_all();
			}

			self.schedule_egui_repaint_after(repaint_interval);
		}
	}

	fn live_overlay_selection_flow_repaint_active(&self) -> bool {
		self.state.drag_rect.is_some_and(|drag_rect| {
			drag_rect.rect.width as f32 >= LIVE_DRAG_START_THRESHOLD_PX
				&& drag_rect.rect.height as f32 >= LIVE_DRAG_START_THRESHOLD_PX
		})
	}

	fn live_overlay_redraw_needed_for_cursor_update(
		old_monitor: Option<MonitorRect>,
		monitor: MonitorRect,
		previous_drag_rect: Option<MonitorRectPoints>,
		next_drag_rect: Option<MonitorRectPoints>,
	) -> bool {
		old_monitor != Some(monitor) || previous_drag_rect != next_drag_rect
	}

	fn repaint_interval_for_monitor(&self, monitor: Option<MonitorRect>) -> Duration {
		let monitor_fps = monitor
			.and_then(|target| {
				self.windows.values().find_map(|window| {
					(target == window.monitor).then_some(window.refresh_rate_millihertz)
				})
			})
			.flatten()
			.and_then(|hz| {
				let fps = (hz as f32) / 1_000.0;

				if fps.is_finite() && fps > 0.0 { Some(fps) } else { None }
			});
		let fallback_fps = self
			.windows
			.values()
			.filter_map(|window| window.refresh_rate_millihertz)
			.filter_map(|hz| {
				let fps = (hz as f32) / 1_000.0;

				if fps.is_finite() && fps > 0.0 { Some(fps) } else { None }
			})
			.max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
		let fps = monitor_fps.or(fallback_fps).unwrap_or(INTERACTIVE_REPAINT_FPS_FLOOR);
		let fps = fps.max(INTERACTIVE_REPAINT_FPS_FLOOR);

		Duration::from_secs_f32(1.0 / fps)
	}

	fn selection_flow_repaint_interval(&self, monitor: Option<MonitorRect>) -> Duration {
		self.repaint_interval_for_monitor(monitor)
	}

	fn frozen_cursor_tracking_interval(&self, monitor: Option<MonitorRect>) -> Duration {
		self.repaint_interval_for_monitor(monitor)
	}

	fn maybe_apply_pending_hud_and_loupe_moves(&mut self) {
		let now = Instant::now();

		self.maybe_apply_pending_hud_window_move(now);
		self.maybe_apply_pending_loupe_window_move(now);
	}

	fn maybe_apply_pending_hud_window_move(&mut self, now: Instant) {
		let Some(desired) = self.pending_hud_outer_pos else {
			return;
		};
		let elapsed = now.duration_since(self.last_hud_window_move_at);
		let interval = self
			.repaint_interval_for_monitor(self.active_cursor_monitor())
			.max(HUD_LOUPE_MOVE_INTERVAL_MIN);

		if elapsed < interval {
			let delay = interval.saturating_sub(elapsed);

			self.schedule_egui_repaint_after(delay);

			return;
		}

		let Some(hud_window) = self.hud_window.as_ref() else {
			return;
		};
		let started_at = Instant::now();

		hud_window
			.window
			.set_outer_position(LogicalPosition::new(desired.x as f64, desired.y as f64));

		let elapsed = started_at.elapsed();

		self.slow_op_logger.warn_if_slow(
			"overlay.hud_window_set_outer_position",
			elapsed,
			SLOW_OP_WARN_OUTER_POSITION,
			|| format!("window_id={:?} pos=({}, {})", hud_window.window.id(), desired.x, desired.y),
		);

		self.pending_hud_outer_pos = None;
		self.last_hud_window_move_at = now;
	}

	fn maybe_apply_pending_loupe_window_move(&mut self, now: Instant) {
		self.apply_pending_loupe_window_move(now, false);
	}

	fn force_apply_pending_loupe_window_move(&mut self) {
		self.apply_pending_loupe_window_move(Instant::now(), true);
	}

	fn apply_pending_loupe_window_move(&mut self, now: Instant, force: bool) {
		let Some(desired) = self.pending_loupe_outer_pos else {
			return;
		};
		let elapsed = now.duration_since(self.last_loupe_window_move_at);
		let interval = self
			.repaint_interval_for_monitor(self.active_cursor_monitor())
			.max(HUD_LOUPE_MOVE_INTERVAL_MIN);

		if !force && elapsed < interval {
			let delay = interval.saturating_sub(elapsed);

			self.schedule_egui_repaint_after(delay);

			return;
		}

		let Some(loupe_window) = self.loupe_window.as_ref() else {
			return;
		};
		let started_at = Instant::now();

		loupe_window
			.window
			.set_outer_position(LogicalPosition::new(desired.x as f64, desired.y as f64));

		let elapsed = started_at.elapsed();

		self.slow_op_logger.warn_if_slow(
			"overlay.loupe_window_set_outer_position",
			elapsed,
			SLOW_OP_WARN_OUTER_POSITION,
			|| {
				format!(
					"window_id={:?} pos=({}, {})",
					loupe_window.window.id(),
					desired.x,
					desired.y
				)
			},
		);

		self.pending_loupe_outer_pos = None;
		self.last_loupe_window_move_at = now;
	}

	fn schedule_egui_repaint_after(&self, delay: Duration) {
		let deadline = Instant::now() + delay;
		let mut next_repaint =
			self.egui_repaint_deadline.lock().unwrap_or_else(|err| err.into_inner());

		if next_repaint.is_none_or(|next| deadline < next) {
			*next_repaint = Some(deadline);
		}
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

	fn maybe_tick_frozen_cursor_tracking(&mut self) {
		if !self.is_active() || !matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}

		let interval =
			self.frozen_cursor_tracking_interval(self.state.monitor).max(CURSOR_POLL_INTERVAL_MIN);
		let now = Instant::now();

		self.schedule_egui_repaint_after(interval);

		if let Some((monitor, global)) = self.last_fresh_event_cursor() {
			let old_monitor = self.active_cursor_monitor();

			if tracing::enabled!(tracing::Level::TRACE) {
				tracing::trace!(
					mode = "frozen",
					source = DeviceCursorPointSource::EventRecentFallback.as_str(),
					monitor_id = monitor.id,
					"Resolved event cursor for frozen tick."
				);
			}
			if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
				return;
			}

			let previous_drag_rect = self.state.drag_rect;

			self.update_cursor_state(monitor, global);
			self.update_hud_window_position(monitor, global);
			self.update_live_drag_rect(monitor, global);

			if let Some(old_monitor) = old_monitor
				&& old_monitor != monitor
			{
				self.request_redraw_for_monitor(old_monitor);
			}

			if Self::live_overlay_redraw_needed_for_cursor_update(
				old_monitor,
				monitor,
				previous_drag_rect,
				self.state.drag_rect,
			) {
				self.request_redraw_for_monitor(monitor);
			}

			return;
		}

		if now.duration_since(self.last_frozen_cursor_poll_at) < interval {
			return;
		}

		self.last_frozen_cursor_poll_at = now;

		let raw = self.sample_mouse_location();
		let old_monitor = self.active_cursor_monitor();
		let Some((monitor, global, source)) = self.resolve_device_cursor_point(raw) else {
			return;
		};

		if tracing::enabled!(tracing::Level::TRACE) {
			tracing::trace!(
				mode = "frozen",
				source = source.as_str(),
				monitor_id = monitor.id,
				"Resolved device cursor for frozen tick."
			);
		}
		if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
			return;
		}

		let previous_drag_rect = self.state.drag_rect;

		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);
		self.update_live_drag_rect(monitor, global);

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		if Self::live_overlay_redraw_needed_for_cursor_update(
			old_monitor,
			monitor,
			previous_drag_rect,
			self.state.drag_rect,
		) {
			self.request_redraw_for_monitor(monitor);
		}
	}

	fn maybe_tick_live_cursor_tracking(&mut self) {
		if !self.is_active() || !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		let interval = self
			.repaint_interval_for_monitor(self.active_cursor_monitor())
			.max(CURSOR_POLL_INTERVAL_MIN);
		let now = Instant::now();

		// Keep this loop alive even if CursorMoved events are sparse or coalesced.
		self.schedule_egui_repaint_after(interval);

		if let Some((monitor, global)) = self.last_fresh_event_cursor() {
			let old_monitor = self.active_cursor_monitor();

			if tracing::enabled!(tracing::Level::TRACE) {
				tracing::trace!(
					mode = "live",
					source = DeviceCursorPointSource::EventRecentFallback.as_str(),
					monitor_id = monitor.id,
					"Resolved event cursor for live tick."
				);
			}
			if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
				return;
			}

			let previous_drag_rect = self.state.drag_rect;

			self.update_cursor_for_live_move(monitor, global);
			self.update_live_drag_rect(monitor, global);

			if let Some(old_monitor) = old_monitor
				&& old_monitor != monitor
			{
				self.request_redraw_for_monitor(old_monitor);
			}

			if Self::live_overlay_redraw_needed_for_cursor_update(
				old_monitor,
				monitor,
				previous_drag_rect,
				self.state.drag_rect,
			) {
				self.request_redraw_for_monitor(monitor);
			}

			return;
		}

		// If we're already repainting at a higher cadence (for example selection flow), avoid
		// sampling the OS cursor position at that same cadence.
		if now.duration_since(self.last_live_cursor_poll_at) < interval {
			return;
		}

		self.last_live_cursor_poll_at = now;

		let raw = self.sample_mouse_location();
		let old_monitor = self.active_cursor_monitor();
		let Some((monitor, global, source)) = self.resolve_live_cursor_point(raw) else {
			return;
		};

		if tracing::enabled!(tracing::Level::TRACE) {
			tracing::trace!(
				mode = "live",
				source = source.as_str(),
				monitor_id = monitor.id,
				"Resolved device cursor for live tick."
			);
		}
		if self.state.cursor == Some(global) && old_monitor == Some(monitor) {
			return;
		}

		let previous_drag_rect = self.state.drag_rect;

		self.update_cursor_for_live_move(monitor, global);
		self.update_live_drag_rect(monitor, global);

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		if Self::live_overlay_redraw_needed_for_cursor_update(
			old_monitor,
			monitor,
			previous_drag_rect,
			self.state.drag_rect,
		) {
			self.request_redraw_for_monitor(monitor);
		}
	}

	fn maybe_request_keepalive_redraw(&mut self) {
		// Avoid a tight present loop if the OS delivers spurious redraws.
		if self.is_active() && self.last_present_at.elapsed() > Duration::from_secs(30) {
			self.request_redraw_all();
		}
	}

	fn maybe_tick_live_sampling(&mut self) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}
		if self.pending_click_hit_test_request_id.is_some() {
			return;
		}

		let now = Instant::now();
		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};

		if self
			.last_event_cursor_at
			.is_some_and(|at| now.duration_since(at) <= LIVE_HOVER_HIT_TEST_INTERVAL)
		{
			return;
		}
		if self.latest_live_cursor_sample_request_id.is_some()
			&& self.applied_live_cursor_sample_request_id
				!= self.latest_live_cursor_sample_request_id
		{
			return;
		}
		if !self.idle_live_sampling_request_allowed(now, monitor) {
			return;
		}

		self.record_live_sample_stall(cursor, monitor);

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}
		if self.request_live_samples_for_cursor(monitor, cursor) {
			self.last_idle_live_sample_request_at = Some(now);
		}
	}

	#[cfg(test)]
	fn observe_scroll_capture_frame(
		&mut self,
		frame: RgbaImage,
	) -> Option<Result<ScrollObserveOutcome>> {
		self.observe_scroll_capture_frame_at(frame, Instant::now())
	}

	fn observe_scroll_capture_frame_at(
		&mut self,
		frame: RgbaImage,
		observation_at: Instant,
	) -> Option<Result<ScrollObserveOutcome>> {
		self.observe_scroll_capture_frame_with_gate(frame, false, observation_at)
	}

	fn observe_scroll_capture_frame_with_gate(
		&mut self,
		frame: RgbaImage,
		allow_stale_input: bool,
		observation_at: Instant,
	) -> Option<Result<ScrollObserveOutcome>> {
		if let Some(reason) = self.scroll_capture_observation_block_reason_at(observation_at)
			&& !(allow_stale_input && reason == "stale_input")
		{
			return None;
		}

		let direction = self.scroll_capture.input_direction?;
		let result = {
			let Some(session) = self.scroll_capture.session.as_mut() else {
				self.scroll_capture_set_error("Scroll capture session is unavailable.");

				return None;
			};

			match direction {
				ScrollDirection::Down => session.observe_downward_sample(frame),
				ScrollDirection::Up => session.observe_upward_sample(frame),
			}
		};

		Some(result)
	}

	fn sync_scroll_preview_segments(&mut self) {
		if let Some(preview) = self.scroll_preview_window.as_mut() {
			let image = self.scroll_capture.session.as_ref().map(ScrollSession::preview_image);

			preview.sync_image(image);
			preview.window.request_redraw();
		}
	}

	fn scroll_capture_set_error(&mut self, message: impl Into<String>) {
		let message = message.into();

		tracing::warn!(
			op = "scroll_capture.error",
			error = %message,
			"Scroll capture paused on error."
		);

		self.scroll_capture.paused = true;

		self.state.set_error(message);
		self.request_redraw_all();
	}

	fn drain_worker_responses(&mut self) -> OverlayControl {
		#[cfg(target_os = "macos")]
		if self.worker.is_none() && self.live_sample_worker.is_none() {
			return OverlayControl::Continue;
		}
		#[cfg(not(target_os = "macos"))]
		if self.worker.is_none() {
			return OverlayControl::Continue;
		}

		#[cfg(target_os = "macos")]
		while let Some(resp) = self.live_sample_worker.as_ref().and_then(|worker| worker.try_recv())
		{
			let control = self.maybe_tick_worker_response_limiter(resp);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}

		if let Some(image) = self.pending_encode_png.take()
			&& let Some(worker) = self.worker.as_ref()
			&& let Err(image) = worker.request_encode_png(image)
		{
			self.pending_encode_png = Some(image);
		}

		while let Some(resp) =
			self.worker.as_ref().and_then(|worker| worker.try_recv_captured_monitor_region())
		{
			match resp.result {
				CapturedMonitorRegionResult::Image(image) => {
					self.handle_captured_scroll_region(
						resp.monitor,
						resp.rect_px,
						resp.request_id,
						image,
					);
				},
				CapturedMonitorRegionResult::NoNewFrame => {
					self.handle_missing_scroll_region(resp.monitor, resp.rect_px, resp.request_id);
				},
			}
		}
		while let Some(resp) = self.worker.as_ref().and_then(|worker| worker.try_recv()) {
			let control = self.maybe_tick_worker_response_limiter(resp);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}

		OverlayControl::Continue
	}

	fn request_live_samples_for_cursor(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) -> bool {
		if self.pending_click_hit_test_request_id.is_some() {
			return false;
		}

		let is_dragging_window = matches!(self.state.mode, OverlayMode::Live)
			&& self.left_mouse_button_down
			&& self.left_mouse_button_down_monitor == Some(monitor);
		let had_snapshot_update = if is_dragging_window || self.state.alt_held {
			false
		} else {
			self.apply_live_hover_cache_state(monitor, cursor)
		};
		let sample_updated = self.request_live_cursor_sample(monitor, cursor, self.state.alt_held);

		if !is_dragging_window && !self.state.alt_held {
			let _ = self.request_live_window_list_refresh_if_needed();
		}

		let apply = self.live_sample_request_redraw_intent(
			had_snapshot_update,
			sample_updated,
			self.state.alt_held || self.loupe_window_visible,
		);

		if apply.any_changed() {
			self.request_redraw_live_sample_targets(monitor, apply);
		}

		sample_updated
	}

	fn request_live_window_list_refresh_if_needed(&mut self) -> bool {
		let now = Instant::now();
		let needs_refresh = self.window_list_snapshot.as_ref().is_none_or(|snapshot| {
			now.duration_since(snapshot.captured_at) > self.window_list_refresh_interval
				|| self.state.alt_held
		});

		if !needs_refresh
			|| now.duration_since(self.last_window_list_refresh_request_at)
				< self.window_list_refresh_interval
		{
			return false;
		}

		let Some(worker) = self.worker.as_ref() else {
			return false;
		};

		if !worker.request_refresh_window_list() {
			return false;
		}

		self.last_window_list_refresh_request_at = now;

		true
	}

	fn request_live_cursor_sample(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		want_patch: bool,
	) -> bool {
		if !monitor.contains(cursor) {
			return false;
		}

		#[cfg(target_os = "macos")]
		{
			let Some(stream) = self.live_sample_stream.as_ref() else {
				return false;
			};
			let request_id = self.live_cursor_sample_request_id.wrapping_add(1);
			let patch_width_px = if want_patch { self.loupe_patch_width_px } else { 0 };
			let patch_height_px = if want_patch { self.loupe_patch_height_px } else { 0 };
			let Some((x_px, y_px)) = monitor.local_u32_pixels(cursor) else {
				return false;
			};
			let sample = stream.latest_cursor_sample(
				monitor,
				x_px,
				y_px,
				want_patch,
				patch_width_px,
				patch_height_px,
			);

			self.live_cursor_sample_request_id = request_id;
			self.latest_live_cursor_sample_request_id = Some(request_id);
			self.latest_live_cursor_sample_requested_at = Some(Instant::now());

			let Some(sample) = sample else {
				return false;
			};

			self.applied_live_cursor_sample_request_id = Some(request_id);

			if let Some(requested_at) = self.latest_live_cursor_sample_requested_at.take() {
				let sample_latency = requested_at.elapsed();

				if sample_latency >= Duration::from_millis(12) {
					tracing::debug!(
						op = "overlay.live_sample_apply_latency",
						request_id,
						monitor_id = monitor.id,
						point = ?cursor,
						latency_ms = sample_latency.as_millis(),
						alt_held = self.state.alt_held,
						"Live cursor sample apply latency exceeded the target frame budget."
					);
				}
			}

			let apply = self.apply_live_cursor_sample_detail(monitor, cursor, sample);

			if apply.any_changed() {
				self.request_redraw_live_sample_targets(monitor, apply);
			}

			true
		}
		#[cfg(not(target_os = "macos"))]
		{
			if self.latest_live_cursor_sample_request_id.is_some()
				&& self.applied_live_cursor_sample_request_id
					!= self.latest_live_cursor_sample_request_id
			{
				return false;
			}

			let Some(worker) = self.worker.as_ref() else {
				return false;
			};
			let request_id = self.live_cursor_sample_request_id.wrapping_add(1);
			let patch_width_px = if want_patch { self.loupe_patch_width_px } else { 0 };
			let patch_height_px = if want_patch { self.loupe_patch_height_px } else { 0 };

			match worker.request_sample_live_cursor(
				monitor,
				cursor,
				request_id,
				want_patch,
				patch_width_px,
				patch_height_px,
			) {
				Ok(()) => {
					self.live_cursor_sample_request_id = request_id;
					self.latest_live_cursor_sample_request_id = Some(request_id);
					self.latest_live_cursor_sample_requested_at = Some(Instant::now());

					true
				},
				Err(WorkerRequestSendError::Full) => {
					tracing::debug!(
						request_id,
						monitor_id = monitor.id,
						point = ?cursor,
						"Live cursor sample request dropped: worker queue full."
					);

					false
				},
				Err(WorkerRequestSendError::Disconnected) => {
					tracing::debug!(
						request_id,
						monitor_id = monitor.id,
						point = ?cursor,
						"Live cursor sample request dropped: worker queue disconnected."
					);

					false
				},
			}
		}
	}

	fn apply_live_cursor_sample_detail(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		sample: LiveCursorSample,
	) -> LiveSampleApplyResult {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return LiveSampleApplyResult::default();
		}
		if self.active_cursor_monitor() != Some(monitor) {
			return LiveSampleApplyResult::default();
		}

		let is_dragging_window = self.left_mouse_button_down
			&& self.left_mouse_button_down_monitor == Some(monitor)
			&& matches!(self.state.mode, OverlayMode::Live);
		let mut changed = LiveSampleApplyResult::default();

		if is_dragging_window {
			if self.state.hovered_window_rect.is_some() {
				self.state.hovered_window_rect = None;
				changed.overlay_changed = true;
				changed.hud_changed = true;
			}
		} else if self.apply_live_hover_cache_state(monitor, point) {
			changed.overlay_changed = true;
			changed.hud_changed = true;
		}
		if self.state.rgb != sample.rgb && sample.rgb.is_some() {
			self.state.rgb = sample.rgb;
			changed.hud_changed = true;
		}
		if self.state.alt_held {
			let loupe =
				sample.patch.map(|patch| crate::state::LoupeSample { center: point, patch });
			let loupe_changed = match (&self.state.loupe, &loupe) {
				(Some(current), Some(next)) => {
					current.center != next.center || current.patch != next.patch
				},
				(None, None) => false,
				_ => true,
			};

			if loupe_changed {
				self.state.loupe = loupe;
				changed.loupe_changed = true;
			}
		} else if self.state.loupe.is_some() {
			self.state.loupe = None;
			changed.loupe_changed = true;
		}

		changed
	}

	fn apply_live_hover_cache_state(&mut self, monitor: MonitorRect, cursor: GlobalPoint) -> bool {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return false;
		}
		if !monitor.contains(cursor) {
			return false;
		}

		let hovered_window_rect = self
			.hovered_window_hit_from_window_list_snapshot(monitor, cursor)
			.map(|hit| MonitorRectPoints { monitor_id: monitor.id, rect: hit.rect });
		let mut updated = false;

		if self.state.hovered_window_rect != hovered_window_rect {
			self.state.hovered_window_rect = hovered_window_rect;
			updated = true;
		}

		updated
	}

	fn live_sample_request_redraw_intent(
		&self,
		hover_changed: bool,
		_sample_requested: bool,
		_loupe_active: bool,
	) -> LiveSampleApplyResult {
		let mut apply = LiveSampleApplyResult::default();

		if hover_changed {
			apply.overlay_changed = true;
			apply.hud_changed = true;
		}

		apply
	}

	fn idle_live_sampling_interval(&self, monitor: MonitorRect) -> Duration {
		self.repaint_interval_for_monitor(Some(monitor)).max(CURSOR_POLL_INTERVAL_MIN)
	}

	fn idle_live_sampling_request_allowed(&self, now: Instant, monitor: MonitorRect) -> bool {
		self.last_idle_live_sample_request_at.is_none_or(|last_request_at| {
			now.duration_since(last_request_at) >= self.idle_live_sampling_interval(monitor)
		})
	}

	fn hovered_window_hit_from_window_list_snapshot(
		&self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) -> Option<WindowHit> {
		let (local_x, local_y) = monitor.local_u32(cursor)?;
		let window_list_snapshot = self.window_list_snapshot.as_ref()?;

		window_list_snapshot.windows.iter().find_map(|window| {
			let rect = monitor.clip_global_rect_i64(
				window.x,
				window.y,
				window.x.saturating_add(window.width),
				window.y.saturating_add(window.height),
			)?;

			if !rect.contains((local_x, local_y)) {
				return None;
			}

			Some(WindowHit { window_id: window.window_id, rect })
		})
	}

	fn record_live_sample_stall(&mut self, cursor: GlobalPoint, monitor: MonitorRect) {
		let now = Instant::now();

		match self.last_live_sample_cursor {
			Some(last_cursor) if last_cursor == cursor => {
				let stall_started_at = self.live_sample_stall_started_at;

				if self.live_sample_stall_started_at.is_none() {
					self.live_sample_stall_started_at = Some(now);
				} else if stall_started_at
					.is_some_and(|start| now.duration_since(start) >= Duration::from_millis(100))
					&& self.last_live_sample_stall_log_at.is_none_or(|last_log| {
						now.duration_since(last_log) >= Duration::from_millis(250)
					}) {
					let Some(stall_started_at) = self.live_sample_stall_started_at else {
						return;
					};

					tracing::debug!(
						cursor = ?cursor,
						monitor_id = monitor.id,
						stall_duration_ms = now.duration_since(stall_started_at).as_millis(),
						"Live sampling cursor unchanged while sampling ticks continue."
					);

					self.last_live_sample_stall_log_at = Some(now);
				}
			},
			Some(_) => {
				self.live_sample_stall_started_at = None;
				self.last_live_sample_stall_log_at = None;
			},
			None => {
				self.live_sample_stall_started_at = Some(now);
			},
		}

		self.last_live_sample_cursor = Some(cursor);
	}

	fn maybe_tick_worker_response_limiter(&mut self, resp: WorkerResponse) -> OverlayControl {
		match resp {
			WorkerResponse::SampledLiveCursor { monitor, point, request_id, sample } => {
				self.handle_sampled_live_cursor_response(monitor, point, request_id, sample);

				OverlayControl::Continue
			},
			WorkerResponse::RefreshedWindowList { snapshot } => {
				self.handle_refreshed_window_list(snapshot);

				OverlayControl::Continue
			},
			WorkerResponse::HitTestWindow { monitor, point, request_id, hit } => {
				self.handle_hit_test_window_response(monitor, point, request_id, hit);

				OverlayControl::Continue
			},
			WorkerResponse::CapturedFreeze { monitor, image, window_image, captured_window_id } => {
				self.handle_captured_freeze_response(
					monitor,
					image,
					window_image,
					captured_window_id,
				);

				OverlayControl::Continue
			},
			WorkerResponse::Error(message) => {
				self.restore_capture_windows_visibility();
				self.state.set_error(message);
				self.request_redraw_all();

				OverlayControl::Continue
			},
			WorkerResponse::EncodedPng { png_bytes } => self.handle_encoded_png_response(png_bytes),
		}
	}

	fn handle_sampled_live_cursor_response(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		sample: LiveCursorSample,
	) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}
		if self.active_cursor_monitor() != Some(monitor) {
			return;
		}
		if self.latest_live_cursor_sample_request_id != Some(request_id) {
			return;
		}

		self.applied_live_cursor_sample_request_id = Some(request_id);

		if let Some(requested_at) = self.latest_live_cursor_sample_requested_at.take() {
			let sample_latency = requested_at.elapsed();

			if sample_latency >= Duration::from_millis(12) {
				tracing::debug!(
					op = "overlay.live_sample_apply_latency",
					request_id,
					monitor_id = monitor.id,
					point = ?point,
					latency_ms = sample_latency.as_millis(),
					alt_held = self.state.alt_held,
					"Live cursor sample apply latency exceeded the target frame budget."
				);
			}
		}

		let apply = self.apply_live_cursor_sample_detail(monitor, point, sample);

		if apply.any_changed() {
			self.request_redraw_live_sample_targets(monitor, apply);
		}
	}

	fn handle_refreshed_window_list(&mut self, snapshot: Arc<WindowListSnapshot>) {
		self.window_list_snapshot = Some(snapshot);

		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};
		let is_dragging_window = self.left_mouse_button_down
			&& self.left_mouse_button_down_monitor == Some(monitor)
			&& matches!(self.state.mode, OverlayMode::Live);

		if is_dragging_window {
			if self.state.hovered_window_rect.is_some() {
				self.state.hovered_window_rect = None;

				self.request_redraw_live_sample_targets(
					monitor,
					LiveSampleApplyResult {
						overlay_changed: true,
						hud_changed: true,
						loupe_changed: false,
					},
				);
			}

			return;
		}
		if self.apply_live_hover_cache_state(monitor, cursor) {
			self.request_redraw_live_sample_targets(
				monitor,
				LiveSampleApplyResult {
					overlay_changed: true,
					hud_changed: true,
					loupe_changed: false,
				},
			);
		}
	}

	fn handle_hit_test_window_response(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		hit: Option<WindowHit>,
	) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}
		if self.pending_click_hit_test_request_id == Some(request_id) {
			self.pending_click_hit_test_request_id = None;
			self.state.hovered_window_rect = None;

			let capture_rect = hit.map(|window_hit| window_hit.rect);
			let window_target = hit.and_then(|window_hit| {
				window_hit.window_id.map(|window_id| WindowFreezeCaptureTarget {
					monitor,
					window_id,
					rect: window_hit.rect,
				})
			});

			self.begin_frozen_capture_with_rect(monitor, capture_rect, window_target, Some(point));
		}
	}

	fn request_click_capture_hit_test(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		self.request_live_window_list_refresh_if_needed();

		if self.window_list_snapshot.is_none() {
			let request_id = self.hit_test_request_id.wrapping_add(1);
			let Some(worker) = self.worker.as_ref() else {
				self.begin_frozen_capture_with_rect(monitor, None, None, Some(cursor));

				return;
			};

			self.hit_test_request_id = request_id;

			match worker.request_hit_test_window(monitor, cursor, request_id) {
				Ok(()) => {
					self.pending_click_hit_test_request_id = Some(request_id);

					return;
				},
				Err(WorkerRequestSendError::Full) => {
					self.hit_test_send_full_count = self.hit_test_send_full_count.saturating_add(1);

					tracing::debug!(
						request_id,
						monitor_id = monitor.id,
						point = ?cursor,
						full_count = self.hit_test_send_full_count,
						"Hit test request dropped: worker queue full."
					);
				},
				Err(WorkerRequestSendError::Disconnected) => {
					self.hit_test_send_disconnected_count =
						self.hit_test_send_disconnected_count.saturating_add(1);

					tracing::debug!(
						request_id,
						monitor_id = monitor.id,
						point = ?cursor,
						disconnected_count = self.hit_test_send_disconnected_count,
						"Hit test request dropped: worker queue disconnected."
					);
				},
			}
		}

		let capture_hit = self.hovered_window_hit_from_window_list_snapshot(monitor, cursor);
		let capture_rect = capture_hit.map(|window_hit| window_hit.rect);
		let window_target = capture_hit.and_then(|window_hit| {
			window_hit.window_id.map(|window_id| WindowFreezeCaptureTarget {
				monitor,
				window_id,
				rect: window_hit.rect,
			})
		});

		self.begin_frozen_capture_with_rect(monitor, capture_rect, window_target, Some(cursor));
	}

	fn begin_frozen_capture_with_rect(
		&mut self,
		monitor: MonitorRect,
		rect: Option<RectPoints>,
		window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
	) {
		self.state.frozen_capture_is_fullscreen_fallback = rect.is_none();
		self.frozen_capture_source = if rect.is_none() {
			FrozenCaptureSource::FullscreenFallback
		} else if window_target.is_some() {
			FrozenCaptureSource::Window
		} else {
			FrozenCaptureSource::DragRegion
		};

		let capture_rect = rect.unwrap_or(RectPoints::new(0, 0, monitor.width, monitor.height));
		let frozen_rgb = self.state.rgb;
		let frozen_loupe = self.state.loupe.as_ref().map(|loupe| crate::state::LoupeSample {
			center: loupe.center,
			patch: loupe.patch.clone(),
		});

		self.state.clear_error();
		self.state.begin_freeze(monitor);

		self.state.frozen_capture_rect = Some(capture_rect);
		self.state.drag_rect = None;
		self.state.hovered_window_rect = None;

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

		self.toolbar_state.floating_position = None;
		self.toolbar_state.dragging = false;
		self.toolbar_state.needs_redraw = true;
		self.toolbar_state.pill_height_points = None;
		self.toolbar_state.layout_last_screen_size_points = None;
		self.toolbar_state.layout_stable_frames = 0;

		self.sync_scroll_toolbar_state();

		// Spawn the toolbar immediately at the default position (capture aware). This avoids any
		// dependency on egui viewport stabilization or additional input events (mouse move) to
		// finish the initial layout.
		{
			let screen_rect = Rect::from_min_size(
				Pos2::ZERO,
				Vec2::new(monitor.width as f32, monitor.height as f32),
			);
			let capture_rect = Rect::from_min_size(
				Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
				Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
			);
			let toolbar_size = WindowRenderer::frozen_toolbar_size(&self.toolbar_state);
			let default_pos = WindowRenderer::frozen_toolbar_default_pos(
				screen_rect,
				capture_rect,
				toolbar_size,
				self.config.toolbar_placement,
			);

			self.toolbar_state.floating_position = Some(default_pos);

			let _ = self.update_toolbar_outer_position(monitor, default_pos);

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = self.state.frozen_generation,
				toolbar_size_points = ?toolbar_size,
				default_pos = ?default_pos,
				"Frozen toolbar default position preseeded."
			);
		}

		self.request_redraw_toolbar_window();

		self.state.rgb = frozen_rgb;
		self.state.loupe = frozen_loupe;
		self.pending_freeze_capture = Some(monitor);
		self.pending_freeze_capture_armed = false;
		self.pending_window_freeze_capture = window_target;
		self.inflight_window_freeze_capture = None;
		self.frozen_window_image = None;
		self.capture_windows_hidden = false;
		self.pending_click_hit_test_request_id = None;
		self.left_mouse_button_down = false;
		self.left_mouse_button_down_monitor = None;
		self.left_mouse_button_down_global = None;

		self.schedule_egui_repaint_after(self.repaint_interval_for_monitor(Some(monitor)));
		self.request_redraw_for_monitor(monitor);

		if self.use_fake_hud_blur()
			&& window_target.is_none()
			&& self.state.live_bg_monitor == Some(monitor)
			&& let Some(image) = self.state.live_bg_image.take()
		{
			self.state.live_bg_monitor = None;

			self.state.finish_freeze(monitor, image);

			self.pending_freeze_capture = None;
			self.pending_freeze_capture_armed = false;

			if let Some(cursor) = cursor {
				self.update_cursor_state(monitor, cursor);
			}
		} else {
			self.state.live_bg_monitor = None;
			self.state.live_bg_image = None;
			self.capture_windows_hidden = true;

			self.hide_capture_windows();
		}
	}

	fn update_live_drag_rect(&mut self, monitor: MonitorRect, global: GlobalPoint) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			self.state.drag_rect = None;

			return;
		}
		if !self.left_mouse_button_down || self.left_mouse_button_down_monitor != Some(monitor) {
			self.state.drag_rect = None;

			return;
		}

		let Some(start_global) = self.left_mouse_button_down_global else {
			self.state.drag_rect = None;

			return;
		};
		let Some(rect) = monitor.local_rect_from_points(start_global, global) else {
			self.state.drag_rect = None;

			return;
		};

		if rect.is_empty() {
			self.state.drag_rect = None;

			return;
		}

		self.state.drag_rect = Some(MonitorRectPoints { monitor_id: monitor.id, rect });
	}

	fn cropped_frozen_capture_image(&self) -> Option<RgbaImage> {
		if !self.state.frozen_capture_is_fullscreen_fallback
			&& let Some(window_image) = self.frozen_window_image.as_ref()
		{
			match self.config.window_capture_alpha_mode {
				WindowCaptureAlphaMode::Background => {},
				WindowCaptureAlphaMode::MatteLight => {
					return Some(Self::flatten_window_image_with_matte(
						window_image,
						WINDOW_CAPTURE_MATTE_LIGHT_RGBA,
					));
				},
				WindowCaptureAlphaMode::MatteDark => {
					return Some(Self::flatten_window_image_with_matte(
						window_image,
						WINDOW_CAPTURE_MATTE_DARK_RGBA,
					));
				},
			}
		}

		let frozen_image = self.state.frozen_image.as_ref()?;
		let Some(monitor) = self.state.monitor else {
			return Some(frozen_image.clone());
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
			None
		} else {
			Some(imageops::crop_imm(frozen_image, x, y, width, height).to_image())
		}
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

	fn flatten_window_image_with_matte(image: &RgbaImage, matte: image::Rgba<u8>) -> RgbaImage {
		let mut out = RgbaImage::from_pixel(image.width(), image.height(), matte);

		imageops::overlay(&mut out, image, 0, 0);

		out
	}

	fn compose_window_preview_layer(
		window_image: &RgbaImage,
		alpha_mode: WindowCaptureAlphaMode,
	) -> RgbaImage {
		match alpha_mode {
			WindowCaptureAlphaMode::Background => window_image.clone(),
			WindowCaptureAlphaMode::MatteLight => {
				Self::flatten_window_image_with_matte(window_image, WINDOW_CAPTURE_MATTE_LIGHT_RGBA)
			},
			WindowCaptureAlphaMode::MatteDark => {
				Self::flatten_window_image_with_matte(window_image, WINDOW_CAPTURE_MATTE_DARK_RGBA)
			},
		}
	}

	fn composite_window_capture_preview(
		mut monitor_image: RgbaImage,
		window_image: &RgbaImage,
		monitor: MonitorRect,
		capture_rect_points: RectPoints,
		alpha_mode: WindowCaptureAlphaMode,
	) -> RgbaImage {
		let capture_rect_px = monitor.local_rect_to_pixels(capture_rect_points);

		if capture_rect_px.width == 0 || capture_rect_px.height == 0 {
			return monitor_image;
		}

		let window_overlay = if window_image.width() == capture_rect_px.width
			&& window_image.height() == capture_rect_px.height
		{
			window_image.clone()
		} else {
			imageops::resize(
				window_image,
				capture_rect_px.width,
				capture_rect_px.height,
				FilterType::Triangle,
			)
		};
		let preview_layer = Self::compose_window_preview_layer(&window_overlay, alpha_mode);

		imageops::overlay(
			&mut monitor_image,
			&preview_layer,
			i64::from(capture_rect_px.x),
			i64::from(capture_rect_px.y),
		);

		monitor_image
	}

	fn handle_captured_freeze_response(
		&mut self,
		monitor: MonitorRect,
		image: RgbaImage,
		window_image: Option<RgbaImage>,
		captured_window_id: Option<u32>,
	) {
		if matches!(self.state.mode, OverlayMode::Frozen) && self.state.monitor == Some(monitor) {
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
						self.frozen_window_image = Some(window_capture_image);

						if let Some(window_capture_image) = self.frozen_window_image.as_ref() {
							frozen_preview_image = Self::composite_window_capture_preview(
								frozen_preview_image,
								window_capture_image,
								monitor,
								target.rect,
								self.config.window_capture_alpha_mode,
							);
						}
					},
				}
			}

			self.state.finish_freeze(monitor, frozen_preview_image);
			self.restore_capture_windows_visibility();

			self.toolbar_state.needs_redraw = true;

			#[cfg(target_os = "macos")]
			if self.toolbar_state.visible {
				self.toolbar_window_warmup_redraws_remaining =
					self.toolbar_window_warmup_redraws_remaining.max(TOOLBAR_WINDOW_WARMUP_REDRAWS);
			}

			if let Some(cursor) = self.state.cursor {
				self.state.rgb =
					image_helpers::frozen_rgb(&self.state.frozen_image, Some(monitor), cursor);
				self.state.loupe = image_helpers::frozen_loupe_patch(
					&self.state.frozen_image,
					Some(monitor),
					cursor,
					self.loupe_patch_width_px,
					self.loupe_patch_height_px,
				)
				.map(|patch| crate::state::LoupeSample { center: cursor, patch });
			}

			self.maybe_start_loupe_window_warmup_redraw();
			self.request_redraw_for_monitor(monitor);
			self.raise_hud_windows();

			return;
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
		let action = self.pending_png_action.take().unwrap_or(PngAction::Copy);

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

	/// Handles a winit window event for one of the overlay-owned windows.
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

		if self
			.scroll_preview_window
			.as_ref()
			.is_some_and(|preview_window| preview_window.window.id() == window_id)
		{
			return match event {
				WindowEvent::RedrawRequested => self.handle_scroll_preview_redraw_requested(),
				WindowEvent::MouseInput {
					state: ElementState::Pressed,
					button: MouseButton::Right,
					..
				} => self.exit(OverlayExit::Cancelled),
				WindowEvent::KeyboardInput { event, .. } => self.handle_key_event(event),
				WindowEvent::ModifiersChanged(modifiers) => {
					self.handle_modifiers_changed(modifiers)
				},
				_ => self.handle_scroll_preview_window_event(event),
			};
		}

		let toolbar_window_id = self
			.toolbar_window
			.as_ref()
			.is_some_and(|toolbar_window| toolbar_window.window.id() == window_id);
		let control = match event {
			WindowEvent::CloseRequested => self.exit(OverlayExit::Cancelled),
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button: MouseButton::Right,
				..
			} => self.exit(OverlayExit::Cancelled),
			WindowEvent::Resized(size) if toolbar_window_id => {
				self.handle_toolbar_window_resized(*size)
			},
			WindowEvent::Resized(size) => self.handle_resized(window_id, *size),
			WindowEvent::ScaleFactorChanged { .. } if toolbar_window_id => {
				self.handle_toolbar_window_scale_factor_changed(window_id)
			},
			WindowEvent::ScaleFactorChanged { .. } => self.handle_scale_factor_changed(window_id),
			WindowEvent::CursorEntered { .. } if toolbar_window_id => OverlayControl::Continue,
			WindowEvent::CursorLeft { .. } if toolbar_window_id => {
				self.toolbar_pointer_local = None;
				self.toolbar_left_button_down = false;
				self.toolbar_left_button_went_down = false;
				self.toolbar_left_button_went_up = false;
				self.toolbar_state.dragging = false;
				self.toolbar_state.drag_offset = Vec2::ZERO;
				self.toolbar_state.drag_anchor = None;

				#[cfg(target_os = "macos")]
				{
					self.request_redraw_toolbar_window();
				}

				OverlayControl::Continue
			},
			WindowEvent::CursorMoved { position, .. } => {
				if toolbar_window_id {
					self.handle_toolbar_cursor_moved(window_id, *position)
				} else {
					self.handle_cursor_moved(window_id, *position)
				}
			},
			WindowEvent::MouseWheel { delta, .. } if toolbar_window_id => OverlayControl::Continue,
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

	fn handle_toolbar_mouse_input(&mut self, state: ElementState) -> OverlayControl {
		let toolbar_left_button_down = matches!(state, ElementState::Pressed);

		if toolbar_left_button_down == self.toolbar_left_button_down {
			return OverlayControl::Continue;
		}
		if toolbar_left_button_down {
			self.toolbar_left_button_went_down = true;
		} else {
			self.toolbar_left_button_went_up = true;
		}

		self.toolbar_left_button_down = toolbar_left_button_down;

		if !toolbar_left_button_down {
			self.toolbar_state.dragging = false;
			self.toolbar_state.drag_offset = Vec2::ZERO;
			self.toolbar_state.drag_anchor = None;
		} else {
			self.toolbar_state.drag_offset = Vec2::ZERO;
			self.toolbar_state.dragging = false;
			self.toolbar_state.drag_anchor = None;
		}

		#[cfg(target_os = "macos")]
		{
			self.request_redraw_toolbar_window();
		}

		OverlayControl::Continue
	}

	fn reset_toolbar_pointer_state(&mut self) {
		self.toolbar_left_button_down = false;
		self.toolbar_left_button_went_down = false;
		self.toolbar_left_button_went_up = false;
		self.toolbar_pointer_local = None;
		self.toolbar_state.drag_anchor = None;
	}

	fn handle_toolbar_cursor_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
	) -> OverlayControl {
		let Some(toolbar_window) = self.toolbar_window.as_ref() else {
			return OverlayControl::Continue;
		};

		if toolbar_window.window.id() != window_id
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
		{
			return OverlayControl::Continue;
		}

		let scale = toolbar_window.window.scale_factor().max(1.0);
		let cursor_local = Pos2::new((position.x / scale) as f32, (position.y / scale) as f32);

		self.toolbar_pointer_local = Some(cursor_local);

		let monitor = match self.state.monitor.or_else(|| self.active_cursor_monitor()) {
			Some(monitor) => monitor,
			None => return OverlayControl::Continue,
		};
		let global_cursor = self.toolbar_cursor_global_position(toolbar_window, cursor_local);
		let drag_monitor =
			global_cursor.and_then(|cursor| self.monitor_at(cursor)).unwrap_or(monitor);
		let mut mouse_drag = self.toolbar_left_button_down && self.toolbar_state.dragging;

		if self.toolbar_left_button_down && self.toolbar_state.drag_anchor.is_none() {
			self.toolbar_state.drag_anchor = Some(cursor_local);
		}
		if !mouse_drag && let Some(drag_anchor) = self.toolbar_state.drag_anchor {
			let dx = cursor_local.x - drag_anchor.x;
			let dy = cursor_local.y - drag_anchor.y;
			let threshold_sq = TOOLBAR_DRAG_START_THRESHOLD_PX * TOOLBAR_DRAG_START_THRESHOLD_PX;

			if dx * dx + dy * dy >= threshold_sq {
				let toolbar_outer_pos = self.toolbar_outer_pos.or_else(|| {
					self.toolbar_state.floating_position.map(|floating_position| {
						GlobalPoint::new(
							monitor.origin.x.saturating_add(floating_position.x.round() as i32),
							monitor.origin.y.saturating_add(floating_position.y.round() as i32),
						)
					})
				});

				if let (Some(global_cursor), Some(toolbar_outer_pos)) =
					(global_cursor, toolbar_outer_pos)
				{
					self.toolbar_state.drag_offset = Vec2::new(
						global_cursor.x as f32 - toolbar_outer_pos.x as f32,
						global_cursor.y as f32 - toolbar_outer_pos.y as f32,
					);
					self.toolbar_state.dragging = true;
					self.toolbar_state.drag_anchor = None;
					mouse_drag = true;
				}
			}
		}
		if mouse_drag && global_cursor.is_none() {
			mouse_drag = false;
		}
		if mouse_drag && let Some(global_cursor) = global_cursor {
			let desired_global = Pos2::new(
				global_cursor.x as f32 - self.toolbar_state.drag_offset.x,
				global_cursor.y as f32 - self.toolbar_state.drag_offset.y,
			);
			let desired_local = Pos2::new(
				desired_global.x - drag_monitor.origin.x as f32,
				desired_global.y - drag_monitor.origin.y as f32,
			);
			let _ = self.update_toolbar_outer_position(drag_monitor, desired_local);
		}

		self.request_redraw_toolbar_window();

		OverlayControl::Continue
	}

	fn toolbar_cursor_global_position(
		&self,
		toolbar_window: &HudOverlayWindow,
		cursor_local: Pos2,
	) -> Option<GlobalPoint> {
		let toolbar_scale = toolbar_window.window.scale_factor().max(1.0);
		let outer_position = toolbar_window.window.outer_position().ok()?;
		let global_cursor = Pos2::new(
			(outer_position.x as f64 / toolbar_scale) as f32 + cursor_local.x,
			(outer_position.y as f64 / toolbar_scale) as f32 + cursor_local.y,
		);

		Some(GlobalPoint::new(global_cursor.x.round() as i32, global_cursor.y.round() as i32))
	}

	fn handle_toolbar_window_resized(&mut self, size: PhysicalSize<u32>) -> OverlayControl {
		let Some(toolbar_window) = self.toolbar_window.as_mut() else {
			return OverlayControl::Continue;
		};

		match toolbar_window.renderer.resize(size) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn handle_toolbar_window_scale_factor_changed(
		&mut self,
		window_id: WindowId,
	) -> OverlayControl {
		let Some(toolbar_window) = self
			.toolbar_window
			.as_mut()
			.filter(|toolbar_window| toolbar_window.window.id() == window_id)
		else {
			return OverlayControl::Continue;
		};
		let size = toolbar_window.window.inner_size();

		match toolbar_window.renderer.resize(size) {
			Ok(()) => {
				let window = Arc::clone(&toolbar_window.window);

				self.configure_hud_window_common(
					window.as_ref(),
					Some(f64::from(HUD_PILL_CORNER_RADIUS_POINTS)),
				);

				OverlayControl::Continue
			},
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	fn should_hide_toolbar_window(&self, monitor: MonitorRect) -> bool {
		!matches!(self.state.mode, OverlayMode::Frozen)
			|| !self.toolbar_state.visible
			|| self.state.frozen_image.is_none()
			|| self.pending_freeze_capture == Some(monitor)
	}

	fn set_toolbar_window_hidden(&mut self) {
		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			toolbar_window.window.set_visible(false);
		}

		self.toolbar_window_visible = false;
		self.toolbar_window_warmup_redraws_remaining = 0;
		self.last_present_at = Instant::now();
	}

	fn draw_toolbar_window_frame(
		&mut self,
		monitor: MonitorRect,
		toolbar_input: Option<FrozenToolbarPointerState>,
	) -> Result<()> {
		self.sync_scroll_toolbar_state();

		#[cfg(not(target_os = "macos"))]
		{
			let _ = (&monitor, &toolbar_input);
			let Some(toolbar_window) = self.toolbar_window.as_ref() else {
				return Ok(());
			};

			toolbar_window.window.set_visible(false);

			self.last_present_at = Instant::now();

			Ok(())
		}
		#[cfg(target_os = "macos")]
		{
			let should_focus_frozen_keyboard = !self.toolbar_window_visible
				&& matches!(self.state.mode, OverlayMode::Frozen)
				&& !self.scroll_capture.active;
			let Some(gpu) = self.gpu.as_ref() else {
				return Ok(());
			};
			let Some(toolbar_window) = self.toolbar_window.as_ref() else {
				return Ok(());
			};

			toolbar_window.window.set_visible(true);

			if !self.toolbar_window_visible {
				self.toolbar_window_visible = true;
				self.toolbar_window_warmup_redraws_remaining = TOOLBAR_WINDOW_WARMUP_REDRAWS;
			}
			if should_focus_frozen_keyboard {
				self.focus_frozen_keyboard_window();
			}

			let previous_floating_position = self.toolbar_state.floating_position;

			self.toolbar_state.floating_position = Some(Pos2::ZERO);

			let Some(toolbar_window) = self.toolbar_window.as_mut() else {
				return Ok(());
			};
			let draw_result = toolbar_window.renderer.draw(
				gpu,
				&self.state,
				monitor,
				false,
				Some(Pos2::ZERO),
				false,
				HudAnchor::Cursor,
				self.config.toolbar_placement,
				self.config.show_alt_hint_keycap,
				false,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
				self.config.selection_particles,
				self.config.selection_flow_stroke_width_px,
				false,
				false,
				Some(&mut self.toolbar_state),
				toolbar_input,
			);

			self.toolbar_state.floating_position = previous_floating_position;

			draw_result?;

			let desired_inner_size = toolbar_window.renderer.hud_pill.map(|hud_pill| {
				(
					hud_pill.rect.width().ceil().max(1.0) as u32,
					hud_pill.rect.height().ceil().max(1.0) as u32,
				)
			});
			let toolbar_window = Arc::clone(&toolbar_window.window);

			if let Some(desired) = desired_inner_size
				&& self.toolbar_inner_size_points != Some(desired)
			{
				self.toolbar_inner_size_points = Some(desired);

				let _ = toolbar_window.request_inner_size(LogicalSize::new(
					f64::from(desired.0),
					f64::from(desired.1),
				));
			}

			Ok(())
		}
	}

	fn handle_toolbar_window_redraw_requested(&mut self) -> OverlayControl {
		self.event_loop_last_progress_window_id =
			self.toolbar_window.as_ref().map(|toolbar_window| toolbar_window.window.id());
		self.event_loop_last_progress_monitor_id = self.state.monitor.map(|monitor| monitor.id);

		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::ToolbarRedraw);

		let Some(monitor) = self.state.monitor else {
			return OverlayControl::Continue;
		};
		let toolbar_input = self.toolbar_pointer_state(monitor, self.toolbar_pointer_local);
		let should_hide_toolbar_window = self.should_hide_toolbar_window(monitor);

		if should_hide_toolbar_window {
			self.set_toolbar_window_hidden();

			return OverlayControl::Continue;
		}

		if let Err(err) = self.draw_toolbar_window_frame(monitor, toolbar_input) {
			return self.exit(OverlayExit::Error(format!("{err:#}")));
		}

		self.update_scroll_toolbar_default_position(monitor);

		if let Some(toolbar_pos) = self.toolbar_state.floating_position {
			let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);
		}
		if let Some(action) = self.toolbar_state.pending_action.take() {
			let control = self.handle_toolbar_action(action);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}

		self.last_present_at = Instant::now();

		if self.toolbar_state.needs_redraw {
			self.toolbar_state.needs_redraw = false;

			self.request_redraw_toolbar_window();
		}

		OverlayControl::Continue
	}

	fn handle_modifiers_changed(&mut self, modifiers: &winit::event::Modifiers) -> OverlayControl {
		let previous_alt_held = self.state.alt_held;
		let previous_alt_modifier_down = self.alt_modifier_down;

		self.keyboard_modifiers = modifiers.state();

		let alt = self.resolve_alt_modifier_state(self.keyboard_modifiers.alt_key());

		match self.config.alt_activation {
			AltActivationMode::Hold => self.set_alt_held(alt),
			AltActivationMode::Toggle => {
				if alt && !self.alt_modifier_down {
					self.set_alt_held(!self.state.alt_held);
				}
			},
		}

		self.alt_modifier_down = alt;

		if previous_alt_held == self.state.alt_held && previous_alt_modifier_down == alt {
			return OverlayControl::Continue;
		}
		if matches!(self.state.mode, OverlayMode::Live) {
			self.request_redraw_hud_window();

			if !self.live_loupe_uses_hud_window()
				&& (self.state.alt_held || self.loupe_window_visible)
			{
				self.request_redraw_loupe_window();
			}

			return OverlayControl::Continue;
		}

		if let Some(monitor) = self.active_cursor_monitor() {
			self.request_redraw_for_monitor(monitor);
		} else {
			self.request_redraw_all();
		}

		OverlayControl::Continue
	}

	fn resolve_alt_modifier_state(&mut self, alt: bool) -> bool {
		let transient_alt_release = !alt
			&& self.state.alt_held
			&& self
				.last_alt_press_at
				.is_some_and(|press| press.elapsed() <= Duration::from_millis(120))
			&& self.is_option_key_down();

		if transient_alt_release { true } else { alt }
	}

	#[cfg(not(target_os = "macos"))]
	fn is_option_key_down(&self) -> bool {
		let Some(cursor_device) = self.cursor_device.as_ref() else {
			return false;
		};
		let keys = cursor_device.get_keys();

		keys.contains(&Keycode::LOption)
			|| keys.contains(&Keycode::ROption)
			|| keys.contains(&Keycode::LAlt)
			|| keys.contains(&Keycode::RAlt)
	}

	#[cfg(target_os = "macos")]
	fn is_option_key_down(&self) -> bool {
		macos_is_option_key_down()
	}

	#[cfg(not(target_os = "macos"))]
	fn sample_mouse_location(&mut self) -> GlobalPoint {
		let Some(cursor_device) = self.cursor_device.as_ref() else {
			return GlobalPoint::new(0, 0);
		};
		let mouse = cursor_device.get_mouse();

		GlobalPoint::new(mouse.coords.0, mouse.coords.1)
	}

	#[cfg(target_os = "macos")]
	fn sample_mouse_location(&mut self) -> GlobalPoint {
		let started_at = Instant::now();
		let point = macos_mouse_location().unwrap_or(GlobalPoint::new(0, 0));
		let elapsed = started_at.elapsed();

		self.slow_op_logger.warn_if_slow(
			"overlay.macos_cursor_location",
			elapsed,
			SLOW_OP_WARN_CURSOR_LOCATION,
			|| format!("sample point=({}, {})", point.x, point.y),
		);

		point
	}

	fn last_fresh_event_cursor(&self) -> Option<(MonitorRect, GlobalPoint)> {
		self.last_fresh_event_cursor_with_ttl(CURSOR_EVENT_TICK_TTL)
	}

	fn last_fresh_event_cursor_with_ttl(
		&self,
		ttl: Duration,
	) -> Option<(MonitorRect, GlobalPoint)> {
		let event_cursor_at = self.last_event_cursor_at?;
		let event_cursor = self.last_event_cursor?;

		if event_cursor_at.elapsed() > ttl {
			return None;
		}

		Some(event_cursor)
	}

	fn sync_alt_held_from_global_keys(&mut self) {
		if matches!(self.config.alt_activation, AltActivationMode::Hold)
			&& self.state.alt_held
			&& !self.is_option_key_down()
		{
			self.set_alt_held(false);
		}
		if !self.is_option_key_down() {
			self.alt_modifier_down = false;
		}
	}

	fn set_alt_held(&mut self, alt: bool) {
		if self.state.alt_held == alt {
			return;
		}

		self.state.alt_held = alt;

		if !alt {
			self.handle_alt_release();

			return;
		}

		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};

		self.last_alt_press_at = Some(Instant::now());

		self.set_alt_loupe_window_visible(Some(monitor), true);

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}

		match self.state.mode {
			OverlayMode::Live => self.request_live_alt_samples(monitor, cursor),
			OverlayMode::Frozen => self.request_frozen_alt_samples(cursor),
		}
	}

	fn handle_alt_release(&mut self) {
		self.last_alt_press_at = None;
		self.state.loupe = None;
		self.loupe_outer_pos = None;
		self.pending_loupe_outer_pos = None;

		self.set_alt_loupe_window_visible(None, false);

		if matches!(self.state.mode, OverlayMode::Live) {
			self.request_redraw_hud_window();

			return;
		}

		if let Some(monitor) = self.active_cursor_monitor() {
			self.request_redraw_for_monitor(monitor);
		}
	}

	fn set_alt_loupe_window_visible(&mut self, monitor: Option<MonitorRect>, visible: bool) {
		if matches!(self.state.mode, OverlayMode::Live) {
			self.loupe_window_visible = false;

			self.reset_loupe_window_warmup_redraws();

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(false);
			}

			return;
		}
		if visible {
			let Some(monitor) = monitor else {
				return;
			};
			let visible = self.update_loupe_window_position(monitor);
			let was_visible = self.loupe_window_visible;

			self.loupe_window_visible = visible;

			if visible {
				self.force_apply_pending_loupe_window_move();
			}
			if visible {
				if !was_visible {
					self.maybe_start_loupe_window_warmup_redraw();
				}
			} else {
				self.reset_loupe_window_warmup_redraws();
			}

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(visible);
				loupe_window.window.request_redraw();
			}

			return;
		}

		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
			loupe_window.window.request_redraw();
		}
	}

	fn request_live_alt_samples(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		let sample_updated = self.request_live_cursor_sample(monitor, cursor, true);
		let apply = self.live_sample_request_redraw_intent(false, sample_updated, true);

		if apply.any_changed() {
			self.request_redraw_live_sample_targets(monitor, apply);
		}
	}

	fn request_frozen_alt_samples(&mut self, cursor: GlobalPoint) {
		if let (Some(frozen_monitor), Some(_)) =
			(self.state.monitor, self.state.frozen_image.as_ref())
		{
			self.state.loupe = image_helpers::frozen_loupe_patch(
				&self.state.frozen_image,
				Some(frozen_monitor),
				cursor,
				self.loupe_patch_width_px,
				self.loupe_patch_height_px,
			)
			.map(|patch| crate::state::LoupeSample { center: cursor, patch });

			self.request_redraw_for_monitor(frozen_monitor);
		}
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

	fn handle_cursor_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
	) -> OverlayControl {
		let old_monitor = self.active_cursor_monitor();
		let now = Instant::now();
		let Some(overlay_window) = self.windows.get(&window_id) else {
			return self.handle_cursor_moved_without_overlay_window(window_id, old_monitor);
		};
		let window_monitor = overlay_window.monitor;
		let scale_factor = overlay_window.window.scale_factor();
		let window_size = overlay_window.window.inner_size();
		// Clamp to overlay window bounds and map to monitor coordinates.
		let max_local_x = ((window_size.width as f64) / scale_factor).max(1.0) as i32 - 1;
		let max_local_y = ((window_size.height as f64) / scale_factor).max(1.0) as i32 - 1;
		let local_x = (position.x / scale_factor).round() as i32;
		let local_y = (position.y / scale_factor).round() as i32;
		let event_global = GlobalPoint::new(
			window_monitor.origin.x + local_x.clamp(0, max_local_x),
			window_monitor.origin.y + local_y.clamp(0, max_local_y),
		);
		let monitor = window_monitor;
		let global = event_global;
		let source = DeviceCursorPointSource::EventRecentFallback;
		let device_cursor = event_global;

		self.last_event_cursor = Some((monitor, event_global));
		self.last_event_cursor_at = Some(now);

		let old_cursor = self.state.cursor;
		let trace = CursorMoveTrace {
			window_id,
			position,
			old_cursor,
			device_cursor,
			event_global,
			monitor,
			global,
			source,
		};

		self.trace_cursor_moved_with_mapping(trace);
		self.update_cursor_for_live_move(monitor, global);

		let previous_drag_rect = self.state.drag_rect;

		self.update_live_drag_rect(monitor, global);
		self.request_cursor_move_samples(monitor, global);

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		if Self::live_overlay_redraw_needed_for_cursor_update(
			old_monitor,
			monitor,
			previous_drag_rect,
			self.state.drag_rect,
		) {
			self.request_redraw_for_monitor(monitor);
		}

		OverlayControl::Continue
	}

	fn handle_cursor_moved_without_overlay_window(
		&mut self,
		window_id: WindowId,
		old_monitor: Option<MonitorRect>,
	) -> OverlayControl {
		if self.should_ignore_live_auxiliary_cursor_event(window_id) {
			return OverlayControl::Continue;
		}

		let now = Instant::now();
		let raw = self.sample_mouse_location();
		let Some((monitor, global, source)) = self.resolve_device_cursor_point(raw) else {
			return OverlayControl::Continue;
		};
		let old_cursor = self.state.cursor;

		self.last_event_cursor = Some((monitor, global));
		self.last_event_cursor_at = Some(now);

		if tracing::enabled!(tracing::Level::TRACE) {
			tracing::trace!(
				window_id = ?window_id,
				window_known = false,
				old_cursor = ?old_cursor,
				device_cursor = ?global,
				event_cursor = ?global,
				source = source.as_str(),
				"CursorMoved (no overlay window mapping)."
			);
		}

		self.update_cursor_for_live_move(monitor, global);

		let previous_drag_rect = self.state.drag_rect;

		self.update_live_drag_rect(monitor, global);
		self.request_cursor_move_samples(monitor, global);

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		if Self::live_overlay_redraw_needed_for_cursor_update(
			old_monitor,
			monitor,
			previous_drag_rect,
			self.state.drag_rect,
		) {
			self.request_redraw_for_monitor(monitor);
		}

		OverlayControl::Continue
	}

	fn should_ignore_live_auxiliary_cursor_event(&self, window_id: WindowId) -> bool {
		Self::should_ignore_live_auxiliary_cursor_event_for_role(
			self.state.mode,
			self.is_auxiliary_capture_window(window_id),
		)
	}

	fn is_auxiliary_capture_window(&self, window_id: WindowId) -> bool {
		self.hud_window.as_ref().is_some_and(|window| window.window.id() == window_id)
			|| self.loupe_window.as_ref().is_some_and(|window| window.window.id() == window_id)
			|| self.toolbar_window.as_ref().is_some_and(|window| window.window.id() == window_id)
			|| self
				.scroll_preview_window
				.as_ref()
				.is_some_and(|window| window.window.id() == window_id)
	}

	fn should_ignore_live_auxiliary_cursor_event_for_role(
		mode: OverlayMode,
		is_auxiliary_window: bool,
	) -> bool {
		matches!(mode, OverlayMode::Live) && is_auxiliary_window
	}

	fn current_device_cursor(&mut self) -> GlobalPoint {
		self.sample_mouse_location()
	}

	fn trace_cursor_moved_with_mapping(&self, trace: CursorMoveTrace) {
		if !tracing::enabled!(tracing::Level::TRACE) {
			return;
		}

		let delta_x =
			trace.global.x.abs_diff(trace.old_cursor.map_or(trace.global.x, |point| point.x));
		let delta_y =
			trace.global.y.abs_diff(trace.old_cursor.map_or(trace.global.y, |point| point.y));

		tracing::trace!(
			window_id = ?trace.window_id,
			window_known = true,
			window_position = ?trace.position,
			old_cursor = ?trace.old_cursor,
			device_cursor = ?trace.device_cursor,
			event_cursor = ?trace.event_global,
			source = trace.source.as_str(),
			monitor_id = trace.monitor.id,
			cursor_delta_x = delta_x,
			cursor_delta_y = delta_y,
			"CursorMoved coordinate source: {}.",
			trace.source.as_str()
		);
	}

	fn update_cursor_for_live_move(&mut self, monitor: MonitorRect, global: GlobalPoint) {
		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);

		if matches!(self.state.mode, OverlayMode::Live) && self.use_fake_hud_blur() {
			if self.state.live_bg_monitor != Some(monitor) {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;
			}

			self.maybe_request_live_bg(monitor);
		}
	}

	fn request_cursor_move_samples(&mut self, monitor: MonitorRect, global: GlobalPoint) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}
		if self.pending_click_hit_test_request_id.is_some() {
			return;
		}

		let is_dragging_window = matches!(self.state.mode, OverlayMode::Live)
			&& self.left_mouse_button_down
			&& self.left_mouse_button_down_monitor == Some(monitor);
		let had_snapshot_update = if is_dragging_window || self.state.alt_held {
			false
		} else {
			self.apply_live_hover_cache_state(monitor, global)
		};
		let sample_requested =
			self.request_live_cursor_sample(monitor, global, self.state.alt_held);

		if !is_dragging_window && !self.state.alt_held {
			let _ = self.request_live_window_list_refresh_if_needed();
		}

		let apply = self.live_sample_request_redraw_intent(
			had_snapshot_update,
			sample_requested,
			self.state.alt_held || self.loupe_window_visible,
		);

		if apply.any_changed() {
			self.request_redraw_live_sample_targets(monitor, apply);
		}
	}

	fn handle_left_mouse_input(
		&mut self,
		window_id: WindowId,
		state: ElementState,
	) -> OverlayControl {
		let monitor = self
			.windows
			.get(&window_id)
			.map(|w| w.monitor)
			.or_else(|| self.active_cursor_monitor())
			.or(self.state.monitor);
		let Some(monitor) = monitor else {
			return OverlayControl::Continue;
		};

		if matches!(self.state.mode, OverlayMode::Frozen) {
			self.reset_toolbar_pointer_state();
			self.request_redraw_for_monitor(monitor);

			return OverlayControl::Continue;
		}
		if !matches!(self.state.mode, OverlayMode::Live) {
			return OverlayControl::Continue;
		}

		match state {
			ElementState::Pressed => {
				if self.left_mouse_button_down {
					return OverlayControl::Continue;
				}

				let raw_cursor = self.current_device_cursor();
				let Some((press_monitor, press_global, _)) =
					self.resolve_live_cursor_point(raw_cursor)
				else {
					self.left_mouse_button_down = true;
					self.left_mouse_button_down_monitor = Some(monitor);
					self.left_mouse_button_down_global = Some(raw_cursor);
					self.state.drag_rect = None;
					self.state.hovered_window_rect = None;

					self.reset_toolbar_pointer_state();
					self.request_redraw_for_monitor(monitor);

					return OverlayControl::Continue;
				};

				self.left_mouse_button_down = true;
				self.left_mouse_button_down_monitor = Some(press_monitor);
				self.left_mouse_button_down_global = Some(press_global);
				self.state.drag_rect = None;
				self.state.hovered_window_rect = None;

				self.reset_toolbar_pointer_state();
				self.update_cursor_state(press_monitor, press_global);
				self.update_hud_window_position(press_monitor, press_global);
				self.request_redraw_for_monitor(press_monitor);

				OverlayControl::Continue
			},
			ElementState::Released => {
				let Some(start_monitor) = self.left_mouse_button_down_monitor else {
					return OverlayControl::Continue;
				};
				let Some(start_global) = self.left_mouse_button_down_global else {
					self.left_mouse_button_down = false;
					self.left_mouse_button_down_monitor = None;

					return OverlayControl::Continue;
				};
				let raw_cursor = self.current_device_cursor();
				let (release_monitor, release_global) =
					if let Some((release_monitor, release_global, _)) =
						self.resolve_live_cursor_point(raw_cursor)
					{
						(release_monitor, release_global)
					} else {
						(start_monitor, start_global)
					};

				self.left_mouse_button_down = false;
				self.left_mouse_button_down_monitor = None;
				self.left_mouse_button_down_global = None;

				let drag_rect = if start_monitor == release_monitor {
					self.state.drag_rect.take()
				} else {
					None
				};

				if let Some(rect) = drag_rect
					&& start_monitor == release_monitor
					&& rect.monitor_id == release_monitor.id
					&& rect.rect.width as f32 >= LIVE_DRAG_START_THRESHOLD_PX
					&& rect.rect.height as f32 >= LIVE_DRAG_START_THRESHOLD_PX
				{
					self.begin_frozen_capture_with_rect(
						release_monitor,
						Some(rect.rect),
						None,
						Some(release_global),
					);

					return OverlayControl::Continue;
				}

				self.state.drag_rect = None;

				self.request_click_capture_hit_test(release_monitor, release_global);

				OverlayControl::Continue
			},
		}
	}

	fn handle_scroll_mouse_wheel(
		&mut self,
		window_id: WindowId,
		delta: &MouseScrollDelta,
	) -> OverlayControl {
		if !self.scroll_capture.active || self.scroll_capture.paused {
			return OverlayControl::Continue;
		}

		let Some(overlay_monitor) = self.windows.get(&window_id).map(|window| window.monitor)
		else {
			return OverlayControl::Continue;
		};
		let Some(scroll_monitor) = self.scroll_capture.monitor else {
			return OverlayControl::Continue;
		};
		let Some(capture_rect) = self.scroll_capture.capture_rect_pixels else {
			return OverlayControl::Continue;
		};

		if overlay_monitor != scroll_monitor {
			return OverlayControl::Continue;
		}

		let cursor = self.current_device_cursor();
		let cursor_pixels = scroll_monitor.local_u32_pixels(cursor);
		let Some(cursor_pixels) = cursor_pixels else {
			return OverlayControl::Continue;
		};

		if !capture_rect.contains(cursor_pixels) {
			return OverlayControl::Continue;
		}

		self.record_scroll_capture_input_direction_from_overlay_wheel_at(delta, Instant::now());

		#[cfg(target_os = "macos")]
		{
			let target_point = cursor;
			let now = Instant::now();

			self.arm_scroll_overlay_mouse_passthrough_window(now, "overlay_mouse_wheel");

			let forwarded = self.forward_macos_scroll_wheel_event(
				scroll_monitor,
				cursor,
				Some(cursor_pixels),
				capture_rect,
				target_point,
				delta,
			);

			if !forwarded {
				self.disarm_scroll_overlay_mouse_passthrough(now, "wheel_forward_failed");
			}
		}

		OverlayControl::Continue
	}

	#[cfg(target_os = "macos")]
	fn forward_macos_scroll_wheel_event(
		&mut self,
		scroll_monitor: MonitorRect,
		cursor: GlobalPoint,
		cursor_pixels: Option<(u32, u32)>,
		capture_rect: RectPoints,
		target_point: GlobalPoint,
		delta: &MouseScrollDelta,
	) -> bool {
		let normalized = Self::normalize_macos_scroll_wheel_delta(
			delta,
			&mut self.scroll_capture.pixel_delta_residual,
		);

		if normalized.posted_x == 0 && normalized.posted_y == 0 {
			return false;
		}

		if let Err(err) = macos_post_scroll_wheel_event(normalized, target_point) {
			tracing::warn!(
				op = "scroll_capture.wheel_forward_failed",
				monitor_id = scroll_monitor.id,
				cursor = ?cursor,
				cursor_pixels = ?cursor_pixels,
				capture_rect = ?capture_rect,
				target_point = ?target_point,
				raw_delta = ?delta,
				normalized_delta_x = normalized.normalized_x,
				normalized_delta_y = normalized.normalized_y,
				posted_delta_x = normalized.posted_x,
				posted_delta_y = normalized.posted_y,
				pixel_residual_x = normalized.residual.x,
				pixel_residual_y = normalized.residual.y,
				error = %format!("{err:#}"),
				"Failed to forward scroll wheel event."
			);

			self.state.set_error(format!("{err:#}"));
			self.request_redraw_all();

			return false;
		}

		tracing::info!(
			op = "scroll_capture.wheel_forwarded",
			monitor_id = scroll_monitor.id,
			cursor = ?cursor,
			cursor_pixels = ?cursor_pixels,
			capture_rect = ?capture_rect,
			target_point = ?target_point,
			raw_delta = ?delta,
			normalized_delta_x = normalized.normalized_x,
			normalized_delta_y = normalized.normalized_y,
			posted_delta_x = normalized.posted_x,
			posted_delta_y = normalized.posted_y,
			pixel_residual_x = normalized.residual.x,
			pixel_residual_y = normalized.residual.y,
			source_state_id = macos_hid_event_source_state_id(),
			"Forwarded scroll wheel event."
		);

		true
	}

	#[cfg(target_os = "macos")]
	fn normalize_macos_scroll_wheel_delta(
		delta: &MouseScrollDelta,
		residual: &mut MacOSScrollPixelResidual,
	) -> MacOSScrollWheelEvent {
		match delta {
			MouseScrollDelta::LineDelta(x, y) => MacOSScrollWheelEvent {
				units: KCG_SCROLL_EVENT_UNIT_LINE,
				normalized_x: f64::from(*x),
				normalized_y: f64::from(*y),
				posted_x: x.round() as i32,
				posted_y: y.round() as i32,
				residual: *residual,
			},
			MouseScrollDelta::PixelDelta(delta) => {
				let normalized_x = Self::normalize_macos_scroll_pixel_component(delta.x);
				let normalized_y = Self::normalize_macos_scroll_pixel_component(delta.y);
				let accumulated_x = residual.x + normalized_x;
				let accumulated_y = residual.y + normalized_y;
				let posted_x = accumulated_x.trunc() as i32;
				let posted_y = accumulated_y.trunc() as i32;

				*residual = MacOSScrollPixelResidual {
					x: accumulated_x - f64::from(posted_x),
					y: accumulated_y - f64::from(posted_y),
				};

				MacOSScrollWheelEvent {
					units: KCG_SCROLL_EVENT_UNIT_PIXEL,
					normalized_x,
					normalized_y,
					posted_x,
					posted_y,
					residual: *residual,
				}
			},
		}
	}

	#[cfg(target_os = "macos")]
	fn normalize_macos_scroll_pixel_component(value: f64) -> f64 {
		if !value.is_finite() {
			return 0.0;
		}

		let normalized = if value.abs() > MACOS_SCROLL_PIXEL_WRAP_THRESHOLD {
			if value.is_sign_positive() {
				value - MACOS_SCROLL_PIXEL_WRAP_MODULUS
			} else {
				value + MACOS_SCROLL_PIXEL_WRAP_MODULUS
			}
		} else {
			value
		};

		normalized.clamp(-MACOS_SCROLL_PIXEL_DELTA_CLAMP, MACOS_SCROLL_PIXEL_DELTA_CLAMP)
	}

	fn scroll_capture_direction_from_wheel_delta(
		delta: &MouseScrollDelta,
	) -> Option<ScrollDirection> {
		let vertical_delta = match delta {
			MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
			MouseScrollDelta::PixelDelta(delta) => {
				#[cfg(target_os = "macos")]
				{
					Self::normalize_macos_scroll_pixel_component(delta.y)
				}
				#[cfg(not(target_os = "macos"))]
				{
					delta.y
				}
			},
		};

		Self::scroll_capture_direction_from_delta_y(vertical_delta)
	}

	fn scroll_capture_direction_from_delta_y(vertical_delta: f64) -> Option<ScrollDirection> {
		if vertical_delta < 0.0 {
			Some(ScrollDirection::Down)
		} else if vertical_delta > 0.0 {
			Some(ScrollDirection::Up)
		} else {
			None
		}
	}

	fn record_scroll_capture_input_direction_at(
		&mut self,
		direction: ScrollDirection,
		gesture_active: bool,
		at: Instant,
	) {
		self.scroll_capture.input_direction = Some(direction);
		self.scroll_capture.input_direction_at = Some(at);
		self.scroll_capture.input_gesture_active = gesture_active;

		#[cfg(target_os = "macos")]
		self.clear_incompatible_live_stream_stale_grace();
	}

	fn record_scroll_capture_input_direction_from_overlay_wheel_at(
		&mut self,
		delta: &MouseScrollDelta,
		at: Instant,
	) {
		if let Some(direction) = Self::scroll_capture_direction_from_wheel_delta(delta) {
			self.record_scroll_capture_input_direction_at(direction, false, at);
		}
	}

	fn finish_scroll_capture_input_direction_at(&mut self, at: Instant) {
		if self.scroll_capture.input_direction.is_some() {
			self.scroll_capture.input_direction_at = Some(at);
		} else {
			self.scroll_capture.input_direction_at = None;
		}

		self.scroll_capture.input_gesture_active = false;

		#[cfg(target_os = "macos")]
		self.clear_incompatible_live_stream_stale_grace();
	}

	fn apply_scroll_capture_input_delta_y(
		&mut self,
		delta_y: f64,
		gesture_active: bool,
		gesture_ended: bool,
		at: Instant,
	) {
		if let Some(direction) = Self::scroll_capture_direction_from_delta_y(delta_y) {
			self.record_scroll_capture_input_direction_at(direction, gesture_active, at);
		}

		if gesture_ended {
			self.finish_scroll_capture_input_direction_at(at);
		}
	}

	fn apply_external_scroll_input_delta_y(
		&mut self,
		global_x: f64,
		global_y: f64,
		delta_y: f64,
		gesture_active: bool,
		gesture_ended: bool,
		at: Instant,
	) {
		if !self.scroll_capture.active || self.scroll_capture.paused {
			return;
		}

		let Some(scroll_monitor) = self.scroll_capture.monitor else {
			return;
		};
		let Some(capture_rect) = self.scroll_capture.capture_rect_pixels else {
			return;
		};
		let cursor = GlobalPoint::new(global_x.round() as i32, global_y.round() as i32);
		let Some(cursor_pixels) = scroll_monitor.local_u32_pixels(cursor) else {
			return;
		};

		if !capture_rect.contains(cursor_pixels) {
			return;
		}
		#[cfg(target_os = "macos")]
		if delta_y != 0.0 && !gesture_ended {
			self.arm_scroll_overlay_mouse_passthrough_window(
				Instant::now(),
				"external_scroll_input",
			);
		}

		self.apply_scroll_capture_input_delta_y(delta_y, gesture_active, gesture_ended, at);
	}

	fn scroll_capture_input_allows_observation(&self) -> bool {
		self.scroll_capture_observation_block_reason().is_none()
	}

	#[cfg_attr(not(test), allow(dead_code))]
	fn scroll_capture_input_allows_growth(&self) -> bool {
		if self.scroll_capture.input_direction != Some(ScrollDirection::Down) {
			return false;
		}

		self.scroll_capture_input_allows_observation()
	}

	fn scroll_capture_observation_block_reason(&self) -> Option<&'static str> {
		self.scroll_capture_observation_block_reason_at(Instant::now())
	}

	fn scroll_capture_observation_block_reason_at(
		&self,
		observation_at: Instant,
	) -> Option<&'static str> {
		if self.scroll_capture.input_direction.is_none() {
			return Some("missing_direction");
		}
		if self.scroll_capture.input_gesture_active {
			return None;
		}

		let Some(input_direction_at) = self.scroll_capture.input_direction_at else {
			return Some("missing_input_timestamp");
		};

		if observation_at.saturating_duration_since(input_direction_at)
			> SCROLL_CAPTURE_INPUT_FRESHNESS
		{
			return Some("stale_input");
		}

		None
	}

	#[cfg(target_os = "macos")]
	fn scroll_capture_input_age_ms(&self) -> Option<u64> {
		self.scroll_capture_input_age_ms_at(Instant::now())
	}

	fn scroll_capture_input_age_ms_at(&self, observation_at: Instant) -> Option<u64> {
		self.scroll_capture.input_direction_at.map(|input_direction_at| {
			u64::try_from(observation_at.saturating_duration_since(input_direction_at).as_millis())
				.unwrap_or(u64::MAX)
		})
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

		self.toolbar_left_button_went_down = false;
		self.toolbar_left_button_went_up = false;

		let cursor_local = toolbar_cursor_local_override
			.or_else(|| self.state.cursor.and_then(|cursor| global_to_local(cursor, monitor)))?;
		let left_button_down = self.toolbar_left_button_down;

		Some(FrozenToolbarPointerState {
			cursor_local,
			left_button_down,
			left_button_went_down,
			left_button_went_up,
		})
	}

	fn handle_key_event(&mut self, event: &KeyEvent) -> OverlayControl {
		if event.state != ElementState::Pressed {
			return OverlayControl::Continue;
		}
		if event.repeat {
			return OverlayControl::Continue;
		}
		if self.scroll_capture.active {
			return self.handle_scroll_capture_key_event(event);
		}

		match &event.logical_key {
			Key::Named(NamedKey::Escape) => self.exit(OverlayExit::Cancelled),
			Key::Named(NamedKey::Tab) => {
				let Some(rgb) = self.state.rgb else {
					return OverlayControl::Continue;
				};
				let hex = rgb.hex_upper();

				match output::write_text_to_clipboard(&hex) {
					Ok(()) => {},
					Err(err) => {
						self.state.set_error(format!("{err:#}"));
						self.request_redraw_all();
					},
				}

				OverlayControl::Continue
			},
			Key::Character(key_text) if key_text == "h" || key_text == "H" => {
				self.toolbar_state.visible = !self.toolbar_state.visible;

				self.request_redraw_all();

				OverlayControl::Continue
			},
			Key::Character(key_text)
				if key_text.as_str().eq_ignore_ascii_case("s")
					&& self.is_save_shortcut_pressed() =>
			{
				self.begin_png_action(PngAction::Save);

				OverlayControl::Continue
			},
			Key::Character(key_text) if key_text.as_str().eq_ignore_ascii_case("s") => {
				let available = self.scroll_capture_is_available();
				let selection_ready = self.scroll_capture_selection_is_ready();

				tracing::info!(
					op = "scroll_capture.frozen_s_pressed",
					available,
					scroll_capture_active = self.scroll_capture.active,
					selection_ready,
					frozen_capture_source = ?self.frozen_capture_source,
					state_mode = ?self.state.mode,
					"Received `s` while frozen."
				);

				if selection_ready {
					self.start_scroll_capture();
				}

				OverlayControl::Continue
			},
			Key::Named(NamedKey::Space) => {
				self.begin_png_action(PngAction::Copy);

				OverlayControl::Continue
			},
			_ => OverlayControl::Continue,
		}
	}

	fn is_save_shortcut_pressed(&self) -> bool {
		#[cfg(target_os = "macos")]
		{
			self.keyboard_modifiers.super_key()
		}
		#[cfg(not(target_os = "macos"))]
		{
			self.keyboard_modifiers.control_key()
		}
	}

	fn handle_scroll_capture_key_event(&mut self, event: &KeyEvent) -> OverlayControl {
		match &event.logical_key {
			Key::Named(NamedKey::Escape) => self.exit(OverlayExit::Cancelled),
			Key::Named(NamedKey::Space) => {
				self.begin_png_action(PngAction::Copy);

				OverlayControl::Continue
			},
			Key::Character(key_text)
				if key_text.as_str().eq_ignore_ascii_case("s")
					&& self.is_save_shortcut_pressed() =>
			{
				self.begin_png_action(PngAction::Save);

				OverlayControl::Continue
			},
			Key::Character(key_text) if key_text.as_str().eq_ignore_ascii_case("u") => {
				self.undo_scroll_capture_append();

				OverlayControl::Continue
			},
			Key::Character(key_text) if key_text.as_str().eq_ignore_ascii_case("p") => {
				self.toggle_scroll_capture_paused();

				OverlayControl::Continue
			},
			_ => OverlayControl::Continue,
		}
	}

	fn current_export_image(&self) -> Option<RgbaImage> {
		if self.scroll_capture.active {
			return self
				.scroll_capture
				.session
				.as_ref()
				.map(|session| session.export_image().clone());
		}

		self.cropped_frozen_capture_image().or_else(|| self.state.frozen_image.clone())
	}

	fn scroll_capture_selection_is_ready(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor.is_some()
			&& self.state.frozen_capture_rect.is_some()
			&& self.frozen_capture_source == FrozenCaptureSource::DragRegion
	}

	fn scroll_capture_is_available(&mut self) -> bool {
		if !self.scroll_capture_selection_is_ready() {
			return false;
		}

		#[cfg(target_os = "macos")]
		{
			true
		}
		#[cfg(not(target_os = "macos"))]
		{
			false
		}
	}

	#[cfg(target_os = "macos")]
	fn try_prepare_scroll_capture_start(
		&mut self,
	) -> Option<(MonitorRect, RectPoints, RectPoints, RgbaImage)> {
		if !self.scroll_capture_selection_is_ready() {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "selection_not_ready",
				frozen_capture_source = ?self.frozen_capture_source,
				state_mode = ?self.state.mode,
				"Skipped starting scroll capture because the current frozen selection was not eligible."
			);

			self.state
				.set_error(String::from("Scroll capture requires a dragged region selection."));
			self.request_redraw_all();

			return None;
		}

		let Some(monitor) = self.state.monitor else {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "missing_monitor",
				"Skipped starting scroll capture because the frozen monitor was unavailable."
			);

			return None;
		};
		let Some(capture_rect_points) = self.state.frozen_capture_rect else {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "missing_capture_rect",
				monitor_id = monitor.id,
				"Skipped starting scroll capture because the frozen capture rect was unavailable."
			);

			return None;
		};
		let capture_rect_pixels = monitor.local_rect_to_pixels(capture_rect_points);
		let Some(base_frame) =
			self.cropped_monitor_frozen_region_image(monitor, capture_rect_pixels)
		else {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "base_frame_unavailable",
				monitor_id = monitor.id,
				capture_rect_points = ?capture_rect_points,
				capture_rect_pixels = ?capture_rect_pixels,
				"Skipped starting scroll capture because the selected frozen region could not be read."
			);

			self.state
				.set_error(String::from("Scroll capture could not read the selected region."));
			self.request_redraw_all();

			return None;
		};

		Some((monitor, capture_rect_points, capture_rect_pixels, base_frame))
	}

	#[cfg(target_os = "macos")]
	fn build_scroll_capture_state(
		&self,
		monitor: MonitorRect,
		capture_rect_pixels: RectPoints,
		base_frame: RgbaImage,
	) -> Result<ScrollCaptureState> {
		Ok(ScrollCaptureState {
			active: true,
			paused: false,
			monitor: Some(monitor),
			capture_rect_pixels: Some(capture_rect_pixels),
			input_direction: None,
			input_direction_at: None,
			input_gesture_active: false,
			#[cfg(target_os = "macos")]
			overlay_mouse_passthrough_active: false,
			#[cfg(target_os = "macos")]
			overlay_mouse_passthrough_until: None,
			#[cfg(target_os = "macos")]
			external_scroll_input_drain_reader: self
				.scroll_capture
				.external_scroll_input_drain_reader
				.clone(),
			#[cfg(target_os = "macos")]
			last_external_scroll_input_seq: 0,
			#[cfg(target_os = "macos")]
			pixel_delta_residual: MacOSScrollPixelResidual::default(),
			#[cfg(target_os = "macos")]
			live_stream: Some(MacLiveFrameStream::with_waker(self.scroll_frame_waker.clone())),
			#[cfg(target_os = "macos")]
			last_stream_frame_seq: 0,
			#[cfg(target_os = "macos")]
			live_stream_stale_grace: None,
			#[cfg(not(target_os = "macos"))]
			next_sample_at: Some(Instant::now() + SCROLL_CAPTURE_SAMPLE_INTERVAL),
			#[cfg(not(target_os = "macos"))]
			next_request_id: 0,
			inflight_request_id: None,
			#[cfg(target_os = "macos")]
			inflight_request_observation: None,
			session: Some(ScrollSession::new(base_frame, SCROLL_CAPTURE_PREVIEW_WIDTH_PX)?),
		})
	}

	fn sync_scroll_toolbar_state(&mut self) {
		self.toolbar_state.scroll_capture_active = self.scroll_capture.active;
		self.toolbar_state.scroll_capture_available =
			if self.scroll_capture.active { true } else { self.scroll_capture_is_available() };
	}

	fn start_scroll_capture(&mut self) {
		if self.scroll_capture.active {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "already_active",
				"Skipped starting scroll capture because a session is already active."
			);

			return;
		}

		#[cfg(not(target_os = "macos"))]
		{
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "unsupported_platform",
				"Skipped starting scroll capture because the current platform is unsupported."
			);
		}
		#[cfg(target_os = "macos")]
		{
			let Some((monitor, capture_rect_points, capture_rect_pixels, base_frame)) =
				self.try_prepare_scroll_capture_start()
			else {
				return;
			};
			let base_frame_dimensions = base_frame.dimensions();

			self.scroll_capture =
				match self.build_scroll_capture_state(monitor, capture_rect_pixels, base_frame) {
					Ok(scroll_capture) => scroll_capture,
					Err(err) => {
						self.state.set_error(format!("{err:#}"));
						self.request_redraw_all();

						return;
					},
				};

			tracing::info!(
				op = "scroll_capture.start",
				frozen_capture_source = ?self.frozen_capture_source,
				monitor_id = monitor.id,
				monitor_origin = ?monitor.origin,
				monitor_size_points = ?(monitor.width, monitor.height),
				monitor_scale_factor = monitor.scale_factor(),
				capture_rect_points = ?capture_rect_points,
				capture_rect_pixels = ?capture_rect_pixels,
				base_frame_px = ?base_frame_dimensions,
				"Entered scroll-capture mode."
			);

			self.sync_scroll_toolbar_state();
			self.sync_scroll_preview_segments();
			self.position_scroll_preview_window(monitor);
			self.update_scroll_toolbar_default_position(monitor);
			self.set_scroll_overlay_mouse_passthrough(false);
			self.focus_scroll_keyboard_window();

			if let Some(preview) = self.scroll_preview_window.as_ref() {
				preview.window.set_visible(true);
				preview.window.request_redraw();
			}

			let _ = self.try_consume_scroll_stream_frame();

			self.request_redraw_for_monitor(monitor);
		}
	}

	fn toggle_scroll_capture_paused(&mut self) {
		if !self.scroll_capture.active {
			return;
		}

		self.scroll_capture.paused = !self.scroll_capture.paused;

		#[cfg(target_os = "macos")]
		if self.scroll_capture.paused {
			self.disarm_scroll_overlay_mouse_passthrough(Instant::now(), "paused");
		}
		if !self.scroll_capture.paused {
			#[cfg(target_os = "macos")]
			{
				let _ = self.try_consume_scroll_stream_frame();
			}
			#[cfg(not(target_os = "macos"))]
			{
				self.scroll_capture.next_sample_at =
					Some(Instant::now() + SCROLL_CAPTURE_SAMPLE_INTERVAL);
			}
		}

		self.request_redraw_scroll_preview_window();
	}

	fn undo_scroll_capture_append(&mut self) {
		if !self.scroll_capture.active {
			return;
		}

		let Some(session) = self.scroll_capture.session.as_mut() else {
			return;
		};

		if !session.undo_last_append() {
			return;
		}

		self.clear_scroll_capture_inflight_request();

		#[cfg(target_os = "macos")]
		{
			let _ = self.try_consume_scroll_stream_frame();
		}
		#[cfg(not(target_os = "macos"))]
		{
			self.scroll_capture.next_sample_at =
				Some(Instant::now() + SCROLL_CAPTURE_SAMPLE_INTERVAL);
		}

		self.sync_scroll_preview_segments();
	}

	fn begin_png_action(&mut self, action: PngAction) {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}

		let Some(export_image) = self.current_export_image() else {
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

	fn stabilized_live_hud_inner_size(
		mode: OverlayMode,
		previous: Option<(u32, u32)>,
		desired: (u32, u32),
	) -> (u32, u32) {
		if !matches!(mode, OverlayMode::Live) {
			return desired;
		}

		let Some(previous) = previous else {
			return desired;
		};

		(previous.0.max(desired.0), desired.1)
	}

	fn hud_window_content_rect(
		mode: OverlayMode,
		alt_held: bool,
		hud_pill: HudPillGeometry,
		loupe_tile: Option<Rect>,
	) -> Rect {
		if cfg!(target_os = "macos") && matches!(mode, OverlayMode::Live) && alt_held {
			loupe_tile.map(|tile| hud_pill.rect.union(tile)).unwrap_or(hud_pill.rect)
		} else {
			hud_pill.rect
		}
	}

	fn maybe_skip_hud_redraw(&mut self) -> Option<OverlayControl> {
		if self.scroll_capture.active {
			if let Some(hud_window) = self.hud_window.as_ref() {
				hud_window.window.set_visible(false);
			}

			self.last_present_at = Instant::now();

			return Some(OverlayControl::Continue);
		}
		if self.capture_windows_hidden {
			#[cfg(not(target_os = "macos"))]
			if let Some(hud_window) = self.hud_window.as_ref() {
				hud_window.window.set_visible(false);
			}

			self.last_present_at = Instant::now();

			#[cfg(not(target_os = "macos"))]
			return Some(OverlayControl::Continue);
		}

		None
	}

	fn draw_hud_window_frame(&mut self, live_loupe_in_hud: bool) -> Result<HudRedrawSummary> {
		let Some(gpu) = self.gpu.as_ref() else {
			return Err(eyre::eyre!("Missing GPU context"));
		};
		let monitor =
			self.monitor_for_mode().or_else(|| self.windows.values().next().map(|w| w.monitor));
		let mut summary = HudRedrawSummary::default();

		if let (Some(monitor), Some(hud_window)) = (monitor, self.hud_window.as_mut()) {
			summary.redraw_window_id = Some(hud_window.window.id());
			summary.redraw_monitor_id = Some(monitor.id);

			hud_window.window.set_visible(true);

			let draw_started_at = Instant::now();

			hud_window.renderer.draw(
				gpu,
				&self.state,
				monitor,
				true,
				Some(Pos2::new(-14.0, -14.0)),
				!live_loupe_in_hud,
				HudAnchor::Cursor,
				self.config.toolbar_placement,
				self.config.show_alt_hint_keycap,
				self.config.show_hud_blur,
				self.config.hud_opaque,
				self.config.hud_opacity,
				self.config.hud_fog_amount,
				self.config.hud_milk_amount,
				self.config.hud_tint_hue,
				self.config.theme_mode,
				self.config.selection_particles,
				self.config.selection_flow_stroke_width_px,
				true,
				false,
				None,
				None,
			)?;

			summary.renderer_draw_elapsed = Some(draw_started_at.elapsed());

			if let Some(hud_pill) = hud_window.renderer.hud_pill {
				let height_points = hud_pill.rect.height();
				let height_changed = self
					.toolbar_state
					.pill_height_points
					.is_none_or(|prev| (prev - height_points).abs() > 0.1);

				self.toolbar_state.pill_height_points = Some(height_points);

				if height_changed
					&& matches!(self.state.mode, OverlayMode::Frozen)
					&& self.toolbar_state.visible
					&& self.state.monitor == Some(monitor)
				{
					self.toolbar_state.needs_redraw = true;
					summary.request_toolbar_redraw = Some(monitor);
				}

				let combined_rect = Self::hud_window_content_rect(
					self.state.mode,
					live_loupe_in_hud,
					hud_pill,
					hud_window.renderer.loupe_tile,
				);
				let desired_w = combined_rect.width().ceil().max(1.0) as u32;
				let desired_h = combined_rect.height().ceil().max(1.0) as u32;
				let desired = Self::stabilized_live_hud_inner_size(
					self.state.mode,
					self.hud_inner_size_points,
					(desired_w, desired_h),
				);

				if self.hud_inner_size_points != Some(desired) {
					self.hud_inner_size_points = Some(desired);
					summary.resize_target = Some(desired);

					let request_inner_size_started_at = Instant::now();
					let _ = hud_window.window.request_inner_size(LogicalSize::new(
						f64::from(desired.0),
						f64::from(desired.1),
					));

					summary.request_inner_size_elapsed =
						Some(request_inner_size_started_at.elapsed());

					if let Some(cursor) = self.state.cursor {
						let position_update_started_at = Instant::now();

						self.update_hud_window_position(monitor, cursor);

						summary.position_update_elapsed =
							Some(position_update_started_at.elapsed());
					}
				}
			}
		}

		Ok(summary)
	}

	fn log_hud_redraw_metrics(&mut self, redraw_elapsed: Duration, summary: &HudRedrawSummary) {
		if let Some(elapsed) = summary.renderer_draw_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.hud_redraw.renderer_draw",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={:?} monitor_id={:?} toolbar_followup={}",
						summary.redraw_window_id,
						summary.redraw_monitor_id,
						summary.request_toolbar_redraw.is_some()
					)
				},
			);
		}
		if let Some(elapsed) = summary.request_inner_size_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.hud_redraw.request_inner_size",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={:?} monitor_id={:?} desired_size={:?}",
						summary.redraw_window_id, summary.redraw_monitor_id, summary.resize_target
					)
				},
			);
		}
		if let Some(elapsed) = summary.position_update_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.hud_redraw.position_update",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={:?} monitor_id={:?} pending_outer_pos={:?}",
						summary.redraw_window_id,
						summary.redraw_monitor_id,
						self.pending_hud_outer_pos
					)
				},
			);
		}

		self.slow_op_logger.warn_if_slow(
			"overlay.hud_redraw.total",
			redraw_elapsed,
			LIVE_PRESENT_INTERVAL_MIN,
			|| {
				format!(
					"window_id={:?} monitor_id={:?} toolbar_followup={}",
					summary.redraw_window_id,
					summary.redraw_monitor_id,
					summary.request_toolbar_redraw.is_some()
				)
			},
		);
	}

	fn handle_hud_redraw_requested(&mut self) -> OverlayControl {
		let redraw_started_at = Instant::now();
		let live_loupe_in_hud = self.live_loupe_renders_in_hud_window();

		self.event_loop_last_progress_window_id =
			self.hud_window.as_ref().map(|hud_window| hud_window.window.id());
		self.event_loop_last_progress_monitor_id =
			self.monitor_for_mode().map(|monitor| monitor.id);

		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::HudRedraw);

		if let Some(control) = self.maybe_skip_hud_redraw() {
			return control;
		}

		let summary = match self.draw_hud_window_frame(live_loupe_in_hud) {
			Ok(summary) => summary,
			Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
		};

		if let Some(monitor) = summary.request_toolbar_redraw {
			self.request_redraw_for_monitor(monitor);
		}

		let redraw_elapsed = redraw_started_at.elapsed();

		self.log_hud_redraw_metrics(redraw_elapsed, &summary);

		self.last_present_at = Instant::now();

		OverlayControl::Continue
	}

	fn hide_loupe_window(&mut self) {
		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
		}

		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();

		self.last_present_at = Instant::now();
	}

	fn should_skip_loupe_redraw(&self) -> bool {
		self.scroll_capture.active
			|| self.capture_windows_hidden
			|| !self.state.alt_held
			|| matches!(self.state.mode, OverlayMode::Live)
	}

	fn current_loupe_draw_target(&self) -> Option<(MonitorRect, GlobalPoint)> {
		let monitor =
			self.monitor_for_mode().or_else(|| self.windows.values().next().map(|w| w.monitor))?;
		let cursor = self.state.cursor?;

		Some((monitor, cursor))
	}

	fn draw_loupe_window_frame(
		&mut self,
		monitor: MonitorRect,
		_cursor: GlobalPoint,
	) -> Result<bool> {
		let redraw_started_at = Instant::now();
		let Some(loupe_window) = self.loupe_window.as_mut() else {
			return Ok(false);
		};
		let loupe_window_id = loupe_window.window.id();

		#[cfg(not(target_os = "macos"))]
		loupe_window.window.set_visible(true);

		let Some(gpu) = self.gpu.as_ref() else {
			return Err(eyre::eyre!("Missing GPU context"));
		};
		let tile_draw_started_at = Instant::now();

		loupe_window.renderer.draw_loupe_tile_window(
			gpu,
			&self.state,
			monitor,
			self.config.show_hud_blur,
			self.config.hud_opaque,
			self.config.hud_opacity,
			self.config.hud_fog_amount,
			self.config.hud_milk_amount,
			self.config.hud_tint_hue,
			self.config.theme_mode,
		)?;

		let tile_draw_elapsed = tile_draw_started_at.elapsed();
		let mut needs_reposition = false;
		let mut request_inner_size_elapsed = None;
		let mut resize_target = None;

		if let Some(tile_rect) = loupe_window.renderer.loupe_tile {
			let desired_w = tile_rect.max.x.ceil().max(1.0) as u32;
			let desired_h = tile_rect.max.y.ceil().max(1.0) as u32;
			let desired = (desired_w, desired_h);

			if self.loupe_inner_size_points != Some(desired) {
				self.loupe_inner_size_points = Some(desired);
				resize_target = Some(desired);

				let request_inner_size_started_at = Instant::now();
				let _ = loupe_window.window.request_inner_size(LogicalSize::new(
					f64::from(desired_w),
					f64::from(desired_h),
				));

				request_inner_size_elapsed = Some(request_inner_size_started_at.elapsed());
				needs_reposition = true;
			}
		}

		let redraw_elapsed = redraw_started_at.elapsed();

		self.slow_op_logger.warn_if_redraw_substep_slow(
			"overlay.loupe_redraw.tile_draw",
			tile_draw_elapsed,
			redraw_elapsed,
			|| format!("window_id={loupe_window_id:?} monitor_id={}", monitor.id),
		);

		if let Some(elapsed) = request_inner_size_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.loupe_redraw.request_inner_size",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={loupe_window_id:?} monitor_id={} desired_size={resize_target:?}",
						monitor.id
					)
				},
			);
		}

		Ok(needs_reposition)
	}

	fn handle_loupe_redraw_requested(&mut self) -> OverlayControl {
		let redraw_started_at = Instant::now();

		self.event_loop_last_progress_window_id =
			self.loupe_window.as_ref().map(|loupe_window| loupe_window.window.id());
		self.event_loop_last_progress_monitor_id =
			self.monitor_for_mode().map(|monitor| monitor.id);

		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::LoupeRedraw);

		if self.gpu.is_none() {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};
		if self.should_skip_loupe_redraw() {
			self.hide_loupe_window();

			return OverlayControl::Continue;
		}

		let Some((monitor, cursor)) = self.current_loupe_draw_target() else {
			self.last_present_at = Instant::now();

			return OverlayControl::Continue;
		};
		let redraw_window_id =
			self.loupe_window.as_ref().map(|loupe_window| loupe_window.window.id());
		let was_visible = self.loupe_window_visible;
		let needs_reposition = match self.draw_loupe_window_frame(monitor, cursor) {
			Ok(needs_reposition) => needs_reposition,
			Err(err) => return self.exit(OverlayExit::Error(format!("{err:#}"))),
		};
		let mut reposition_elapsed = None;

		if needs_reposition {
			let reposition_started_at = Instant::now();
			let _ = self.update_loupe_window_position(monitor);

			self.force_apply_pending_loupe_window_move();

			reposition_elapsed = Some(reposition_started_at.elapsed());
		}

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(true);
		}

		self.loupe_window_visible = true;

		if !was_visible {
			self.maybe_start_loupe_window_warmup_redraw();
		}

		let redraw_elapsed = redraw_started_at.elapsed();

		if let Some(elapsed) = reposition_elapsed {
			self.slow_op_logger.warn_if_redraw_substep_slow(
				"overlay.loupe_redraw.reposition",
				elapsed,
				redraw_elapsed,
				|| {
					format!(
						"window_id={redraw_window_id:?} monitor_id={} pending_outer_pos={:?}",
						monitor.id, self.pending_loupe_outer_pos
					)
				},
			);
		}

		self.last_present_at = Instant::now();

		OverlayControl::Continue
	}

	fn handle_scroll_preview_window_event(&mut self, event: &WindowEvent) -> OverlayControl {
		let Some(preview_window) = self.scroll_preview_window.as_mut() else {
			return OverlayControl::Continue;
		};

		preview_window.handle_window_event(event);

		OverlayControl::Continue
	}

	fn handle_scroll_preview_redraw_requested(&mut self) -> OverlayControl {
		let Some(preview_window) = self.scroll_preview_window.as_mut() else {
			return OverlayControl::Continue;
		};

		if !self.scroll_capture.active {
			preview_window.window.set_visible(false);

			return OverlayControl::Continue;
		}

		let theme =
			hud_helpers::effective_hud_theme(self.config.theme_mode, preview_window.window.theme());
		let view = ScrollPreviewView { paused: self.scroll_capture.paused, theme };
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};

		match preview_window.draw(gpu, theme, view) {
			Ok(()) => OverlayControl::Continue,
			Err(err) => self.exit(OverlayExit::Error(format!("{err:#}"))),
		}
	}

	#[cfg(target_os = "macos")]
	fn position_scroll_preview_window(&self, monitor: MonitorRect) {
		let Some(preview_window) = self.scroll_preview_window.as_ref() else {
			return;
		};
		let preview_rect = self.scroll_preview_local_rect(monitor);
		let _ = preview_window.window.request_inner_size(LogicalSize::new(
			f64::from(preview_rect.width()),
			f64::from(preview_rect.height()),
		));

		preview_window.window.set_outer_position(LogicalPosition::new(
			f64::from(monitor.origin.x) + f64::from(preview_rect.min.x),
			f64::from(monitor.origin.y) + f64::from(preview_rect.min.y),
		));
	}

	fn scroll_preview_local_rect(&self, monitor: MonitorRect) -> Rect {
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let gap = SCROLL_PREVIEW_WINDOW_MARGIN_POINTS as f32;
		let preview_width = SCROLL_PREVIEW_WINDOW_WIDTH_POINTS as f32;

		if let Some(capture_rect) = self.state.frozen_capture_rect {
			let capture_rect = Rect::from_min_size(
				Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
				Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
			)
			.intersect(screen_rect);
			let preview_height = capture_rect.height().max(1.0);
			let right_x = capture_rect.max.x + gap;
			let left_x = capture_rect.min.x - gap - preview_width;
			let x = if right_x + preview_width <= screen_rect.max.x {
				right_x
			} else if left_x >= screen_rect.min.x {
				left_x
			} else {
				(screen_rect.max.x - preview_width - gap).max(screen_rect.min.x + gap)
			};

			return Rect::from_min_size(
				Pos2::new(x, capture_rect.min.y),
				Vec2::new(preview_width, preview_height),
			);
		}

		let preview_size = if let Some(preview_window) = self.scroll_preview_window.as_ref() {
			let scale = preview_window.window.scale_factor().max(1.0) as f32;
			let size = preview_window.window.inner_size();

			Vec2::new(
				((size.width as f32) / scale).max(preview_width),
				((size.height as f32) / scale).max(SCROLL_PREVIEW_WINDOW_HEIGHT_POINTS as f32),
			)
		} else {
			Vec2::new(preview_width, SCROLL_PREVIEW_WINDOW_HEIGHT_POINTS as f32)
		};
		let min_x = screen_rect.min.x + gap;
		let max_x = (screen_rect.max.x - preview_size.x - gap).max(min_x);
		let min_y = screen_rect.min.y + gap;
		let max_y = (screen_rect.max.y - preview_size.y - gap).max(min_y);
		let y = min_y.min(max_y);
		let pos = Pos2::new(max_x, y);

		Rect::from_min_size(pos, preview_size)
	}

	#[cfg(target_os = "macos")]
	fn set_scroll_overlay_mouse_passthrough(&self, passthrough: bool) {
		for overlay_window in self.windows.values() {
			let _ = overlay_window.window.set_cursor_hittest(!passthrough);
		}
	}

	#[cfg(target_os = "macos")]
	fn set_scroll_overlay_mouse_passthrough_state(
		&mut self,
		now: Instant,
		passthrough: bool,
		reason: &'static str,
	) {
		if self.scroll_capture.overlay_mouse_passthrough_active == passthrough {
			return;
		}

		self.set_scroll_overlay_mouse_passthrough(passthrough);

		self.scroll_capture.overlay_mouse_passthrough_active = passthrough;

		tracing::info!(
			op = if passthrough {
				"scroll_capture.mouse_passthrough_armed"
			} else {
				"scroll_capture.mouse_passthrough_disarmed"
			},
			reason,
			passthrough,
			deadline_in_ms = self.scroll_capture.overlay_mouse_passthrough_until.map(|deadline| {
				u64::try_from(deadline.saturating_duration_since(now).as_millis())
					.unwrap_or(u64::MAX)
			}),
			"Updated scroll-capture mouse passthrough state."
		);
	}

	#[cfg(target_os = "macos")]
	fn arm_scroll_overlay_mouse_passthrough_window(&mut self, now: Instant, reason: &'static str) {
		let deadline = now + SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE;
		let was_active = self.scroll_capture.overlay_mouse_passthrough_active;

		self.scroll_capture.overlay_mouse_passthrough_until = Some(deadline);

		self.set_scroll_overlay_mouse_passthrough_state(now, true, reason);

		if was_active {
			tracing::info!(
				op = "scroll_capture.mouse_passthrough_extended",
				reason,
				deadline_in_ms = u64::try_from(deadline.saturating_duration_since(now).as_millis())
					.unwrap_or(u64::MAX),
				"Extended scroll-capture mouse passthrough window."
			);
		}
	}

	#[cfg(target_os = "macos")]
	fn disarm_scroll_overlay_mouse_passthrough(&mut self, now: Instant, reason: &'static str) {
		self.scroll_capture.overlay_mouse_passthrough_until = None;

		self.set_scroll_overlay_mouse_passthrough_state(now, false, reason);
	}

	#[cfg(target_os = "macos")]
	fn sync_scroll_overlay_mouse_passthrough_window(&mut self, now: Instant) {
		if !self.scroll_capture.overlay_mouse_passthrough_active {
			return;
		}

		let Some(deadline) = self.scroll_capture.overlay_mouse_passthrough_until else {
			self.set_scroll_overlay_mouse_passthrough_state(now, false, "missing_deadline");

			return;
		};

		if deadline <= now {
			self.disarm_scroll_overlay_mouse_passthrough(now, "idle_timeout");
		}
	}

	#[cfg(target_os = "macos")]
	fn focus_frozen_keyboard_window(&self) {
		macos_activate_app();

		let target_window = if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			Some(toolbar_window.window.as_ref())
		} else {
			self.windows
				.values()
				.find(|overlay_window| Some(overlay_window.monitor) == self.state.monitor)
				.map(|overlay_window| overlay_window.window.as_ref())
		};
		let Some(target_window) = target_window else {
			tracing::info!(
				op = "scroll_capture.frozen_focus_requested",
				target = "missing_window",
				state_mode = ?self.state.mode,
				toolbar_window_present = self.toolbar_window.is_some(),
				monitor_id = ?self.state.monitor.map(|monitor| monitor.id),
				"Requested frozen keyboard focus, but no target window was available."
			);

			return;
		};

		tracing::info!(
			op = "scroll_capture.frozen_focus_requested",
			target = if self.toolbar_window.is_some() { "toolbar_window" } else { "overlay_window" },
			state_mode = ?self.state.mode,
			toolbar_window_visible = self.toolbar_window_visible,
			monitor_id = ?self.state.monitor.map(|monitor| monitor.id),
			"Requested frozen keyboard focus."
		);

		macos_make_window_key(target_window);
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

	#[cfg(target_os = "macos")]
	fn focus_scroll_keyboard_window(&self) {
		macos_activate_app();

		let target_window = if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			Some(toolbar_window.window.as_ref())
		} else if let Some(preview_window) = self.scroll_preview_window.as_ref() {
			Some(preview_window.window.as_ref())
		} else {
			self.windows
				.values()
				.find(|overlay_window| Some(overlay_window.monitor) == self.scroll_capture.monitor)
				.map(|overlay_window| overlay_window.window.as_ref())
		};
		let Some(target_window) = target_window else {
			return;
		};

		macos_make_window_key(target_window);
	}

	fn update_scroll_toolbar_default_position(&mut self, monitor: MonitorRect) {
		if !self.scroll_capture.active || self.toolbar_state.dragging {
			return;
		}

		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let preview_rect = self.scroll_preview_local_rect(monitor);
		let toolbar_size = WindowRenderer::frozen_toolbar_size(&self.toolbar_state);
		let toolbar_pos = WindowRenderer::frozen_toolbar_default_pos(
			screen_rect,
			preview_rect,
			toolbar_size,
			self.config.toolbar_placement,
		);

		self.toolbar_state.floating_position = Some(toolbar_pos);

		let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);
	}

	fn handle_overlay_window_redraw(&mut self, window_id: WindowId) -> OverlayControl {
		let Some(overlay_monitor) = self.windows.get(&window_id).map(|overlay| overlay.monitor)
		else {
			return OverlayControl::Continue;
		};

		self.sync_scroll_toolbar_state();

		self.event_loop_last_progress_window_id = Some(window_id);
		self.event_loop_last_progress_monitor_id = Some(overlay_monitor.id);

		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::OverlayRedraw);

		// On macOS the frozen toolbar is now rendered in its own native HUD window; keep this
		// fullscreen overlay free of toolbar UI so shader-backed blur and monitor-aligned offsets
		// do not conflict with native-window positioning.
		let draw_toolbar = !cfg!(target_os = "macos")
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.state.monitor == Some(overlay_monitor)
			&& self.state.frozen_image.is_some()
			&& self.pending_freeze_capture != Some(overlay_monitor);
		let toolbar_input =
			if draw_toolbar { self.toolbar_pointer_state(overlay_monitor, None) } else { None };
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};

		if matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor == Some(overlay_monitor)
		{
			tracing::trace!(
				window_id = ?window_id,
				monitor_id = overlay_monitor.id,
				frozen_generation = self.state.frozen_generation,
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

		let toolbar_state = if draw_toolbar { Some(&mut self.toolbar_state) } else { None };
		let capture_in_progress = self.pending_freeze_capture == Some(overlay_monitor)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor == Some(overlay_monitor)
			&& self.state.frozen_image.is_none();
		let draw_selection_particles =
			(self.config.selection_particles || self.scroll_capture.active) && !capture_in_progress;

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
				draw_selection_particles,
				self.config.selection_flow_stroke_width_px,
				!self.scroll_capture.active,
				self.scroll_capture.active,
				toolbar_state,
				toolbar_input,
			) {
				return self.exit(OverlayExit::Error(format!("{err:#}")));
			}
		}
		self.last_present_at = Instant::now();

		self.handle_capture_and_toolbar_redraw_post(overlay_monitor, draw_toolbar)
	}

	fn handle_capture_and_toolbar_redraw_post(
		&mut self,
		overlay_monitor: MonitorRect,
		draw_toolbar: bool,
	) -> OverlayControl {
		if self.pending_freeze_capture == Some(overlay_monitor)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor == Some(overlay_monitor)
			&& self.state.frozen_image.is_none()
			&& let Some(worker) = &self.worker
		{
			let pending_window_target = self
				.pending_window_freeze_capture
				.filter(|target| target.monitor == overlay_monitor);
			let freeze_target = pending_window_target
				.map_or(FreezeCaptureTarget::Monitor, |target| FreezeCaptureTarget::Window {
					window_id: target.window_id,
				});

			#[cfg(target_os = "macos")]
			{
				if worker.request_freeze_capture(overlay_monitor, freeze_target) {
					self.pending_freeze_capture = None;
					self.pending_freeze_capture_armed = false;
					self.inflight_window_freeze_capture = pending_window_target;
					self.pending_window_freeze_capture = None;
				} else {
					self.request_redraw_for_monitor(overlay_monitor);
				}
			}
			#[cfg(not(target_os = "macos"))]
			{
				// Capture must happen on a post-hide redraw so the HUD/loupe are not included.
				if self.pending_freeze_capture_armed {
					if worker.request_freeze_capture(overlay_monitor, freeze_target) {
						self.pending_freeze_capture = None;
						self.pending_freeze_capture_armed = false;
						self.inflight_window_freeze_capture = pending_window_target;
						self.pending_window_freeze_capture = None;
					} else {
						self.request_redraw_for_monitor(overlay_monitor);
					}
				} else {
					self.pending_freeze_capture_armed = true;

					self.hide_capture_windows();
					self.request_redraw_for_monitor(overlay_monitor);
				}
			}
		}
		if draw_toolbar && let Some(action) = self.toolbar_state.pending_action.take() {
			let control = self.handle_toolbar_action(action);

			if !matches!(control, OverlayControl::Continue) {
				return control;
			}
		}
		if draw_toolbar && self.toolbar_state.needs_redraw {
			self.toolbar_state.needs_redraw = false;

			self.request_redraw_for_monitor(overlay_monitor);
		}

		OverlayControl::Continue
	}

	fn handle_toolbar_action(&mut self, action: FrozenToolbarTool) -> OverlayControl {
		match action {
			FrozenToolbarTool::Copy => {
				self.begin_png_action(PngAction::Copy);

				OverlayControl::Continue
			},
			FrozenToolbarTool::Save => {
				self.begin_png_action(PngAction::Save);

				OverlayControl::Continue
			},
			FrozenToolbarTool::Scroll => {
				self.start_scroll_capture();

				OverlayControl::Continue
			},
			_ => OverlayControl::Continue,
		}
	}

	fn exit(&mut self, exit: OverlayExit) -> OverlayControl {
		#[cfg(target_os = "macos")]
		self.set_scroll_overlay_mouse_passthrough(false);
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
		self.toolbar_window_visible = false;
		self.toolbar_window_warmup_redraws_remaining = 0;
		self.loupe_window_visible = false;
		self.loupe_window_warmup_redraws_remaining = 0;
		self.scroll_capture = ScrollCaptureState::default();
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
		self.toolbar_left_button_down = false;
		self.toolbar_left_button_went_down = false;
		self.toolbar_left_button_went_up = false;
		self.toolbar_pointer_local = None;
		self.pending_encode_png = None;
		self.pending_png_action = None;
		self.keyboard_modifiers = ModifiersState::default();

		OverlayControl::Exit(exit)
	}

	fn initialize_cursor_state(&mut self) {
		let cursor = self.sample_mouse_location();
		let Some(monitor) = self.monitor_at(cursor) else {
			self.state.cursor = Some(cursor);
			self.state.rgb = None;
			self.cursor_monitor = None;

			return;
		};

		self.update_cursor_state(monitor, cursor);
		self.update_hud_window_position(monitor, cursor);

		if matches!(self.state.mode, OverlayMode::Live) {
			if self.use_fake_hud_blur() {
				self.maybe_request_live_bg(monitor);
			}

			self.request_live_samples_for_cursor(monitor, cursor);
		}
	}

	fn maybe_request_live_bg(&mut self, monitor: MonitorRect) {
		if !matches!(self.state.mode, OverlayMode::Live) || !self.use_fake_hud_blur() {
			return;
		}
		if self.state.live_bg_monitor == Some(monitor) && self.state.live_bg_image.is_some() {
			return;
		}

		let force = self.state.alt_held && self.state.live_bg_image.is_none();

		if !force && self.last_live_bg_request_at.elapsed() < self.live_bg_request_interval {
			return;
		}

		let Some(worker) = &self.worker else {
			return;
		};

		if worker.request_freeze_capture(monitor, FreezeCaptureTarget::Monitor) {
			self.last_live_bg_request_at = Instant::now();
		}
	}

	fn monitor_at(&self, cursor: GlobalPoint) -> Option<MonitorRect> {
		self.windows
			.values()
			.find(|window| window.monitor.contains(cursor))
			.map(|window| window.monitor)
	}

	fn resolve_device_cursor_point(
		&self,
		raw: GlobalPoint,
	) -> Option<(MonitorRect, GlobalPoint, DeviceCursorPointSource)> {
		if let Some(monitor) = self.monitor_at(raw) {
			return Some((monitor, raw, DeviceCursorPointSource::DevicePoints));
		}

		for monitor in self.windows.values().map(|window| window.monitor) {
			let sf = f64::from(monitor.scale_factor()).max(1.0);
			let origin_px_x = (monitor.origin.x as f64 * sf).round() as i64;
			let origin_px_y = (monitor.origin.y as f64 * sf).round() as i64;
			let size_px_x = (monitor.width as f64 * sf).round() as i64;
			let size_px_y = (monitor.height as f64 * sf).round() as i64;
			let local_px_x = (raw.x as i64).saturating_sub(origin_px_x);
			let local_px_y = (raw.y as i64).saturating_sub(origin_px_y);

			if local_px_x < 0
				|| local_px_y < 0
				|| local_px_x >= size_px_x
				|| local_px_y >= size_px_y
			{
				continue;
			}

			let local_points_x = (local_px_x as f64 / sf).round() as i64;
			let local_points_y = (local_px_y as f64 / sf).round() as i64;
			let local_points_x = match i32::try_from(local_points_x) {
				Ok(value) => value,
				Err(_) => continue,
			};
			let local_points_y = match i32::try_from(local_points_y) {
				Ok(value) => value,
				Err(_) => continue,
			};
			let candidate = GlobalPoint::new(
				monitor.origin.x.saturating_add(local_points_x),
				monitor.origin.y.saturating_add(local_points_y),
			);

			if monitor.contains(candidate) {
				return Some((monitor, candidate, DeviceCursorPointSource::DevicePixelsFallback));
			}
		}

		None
	}

	fn resolve_live_cursor_point(
		&self,
		raw_device: GlobalPoint,
	) -> Option<(MonitorRect, GlobalPoint, DeviceCursorPointSource)> {
		let Some((device_monitor, device_global, device_source)) =
			self.resolve_device_cursor_point(raw_device)
		else {
			let (monitor, global) = self.last_event_cursor?;
			let event_cursor_at = self.last_event_cursor_at?;

			if event_cursor_at.elapsed() > LIVE_EVENT_CURSOR_CACHE_TTL {
				return None;
			}

			return Some((monitor, global, DeviceCursorPointSource::EventRecentFallback));
		};

		if let (Some(event_cursor_at), Some((event_monitor, event_global))) =
			(self.last_event_cursor_at, self.last_event_cursor)
			&& self.state.cursor == Some(device_global)
			&& event_global != device_global
			&& event_cursor_at.elapsed() <= LIVE_EVENT_CURSOR_CACHE_TTL
		{
			return Some((
				event_monitor,
				event_global,
				DeviceCursorPointSource::EventRecentFallback,
			));
		}

		Some((device_monitor, device_global, device_source))
	}

	fn active_cursor_monitor(&self) -> Option<MonitorRect> {
		self.cursor_monitor.or_else(|| self.state.cursor.and_then(|cursor| self.monitor_at(cursor)))
	}

	fn monitor_for_mode(&self) -> Option<MonitorRect> {
		match self.state.mode {
			OverlayMode::Frozen => self.active_cursor_monitor().or(self.state.monitor),
			OverlayMode::Live => self.active_cursor_monitor(),
		}
	}

	fn update_hud_window_position(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		if matches!(self.state.mode, OverlayMode::Live) && self.state.alt_held {
			let _ = self.update_loupe_window_position(monitor);

			return;
		}

		let Some(hud_window) = self.hud_window.as_ref() else {
			return;
		};
		let scale = hud_window.window.scale_factor().max(1.0);
		let size = hud_window.window.inner_size();
		let hud_w_points = ((size.width as f64) / scale).ceil().max(1.0) as i32;
		let hud_h_points = ((size.height as f64) / scale).ceil().max(1.0) as i32;
		let monitor_right = monitor.origin.x.saturating_add_unsigned(monitor.width);
		let monitor_bottom = monitor.origin.y.saturating_add_unsigned(monitor.height);
		// Keep the HUD far enough from the cursor that even if the OS lags window moves during
		// rapid drags, the cursor is unlikely to "catch up" and overlap the HUD window.
		let offset_x = 48;
		let offset_y = 24;
		let mut x = cursor.x.saturating_add(offset_x);
		let mut y = cursor.y.saturating_add(offset_y);

		if x.saturating_add(hud_w_points) > monitor_right {
			x = cursor.x.saturating_sub(offset_x.saturating_add(hud_w_points));
		}
		if y.saturating_add(hud_h_points) > monitor_bottom {
			y = cursor.y.saturating_sub(offset_y.saturating_add(hud_h_points));
		}

		x = x.clamp(
			monitor.origin.x,
			monitor_right.saturating_sub(hud_w_points).max(monitor.origin.x),
		);
		y = y.clamp(
			monitor.origin.y,
			monitor_bottom.saturating_sub(hud_h_points).max(monitor.origin.y),
		);

		let desired = GlobalPoint::new(x, y);

		if self.hud_outer_pos == Some(desired) {
			if self.state.alt_held {
				let _ = self.update_loupe_window_position(monitor);
			}

			return;
		}

		self.hud_outer_pos = Some(desired);
		self.pending_hud_outer_pos = Some(desired);

		if self.state.alt_held {
			let _ = self.update_loupe_window_position(monitor);
		}
	}

	fn update_loupe_window_position(&mut self, monitor: MonitorRect) -> bool {
		if !self.state.alt_held {
			self.pending_loupe_outer_pos = None;

			return false;
		}

		let Some(loupe_window) = self.loupe_window.as_ref() else {
			return false;
		};
		let loupe_scale = loupe_window.window.scale_factor().max(1.0);
		let loupe_size = loupe_window.window.inner_size();
		let loupe_w_points = ((loupe_size.width as f64) / loupe_scale).ceil().max(1.0) as i32;
		let loupe_h_points = ((loupe_size.height as f64) / loupe_scale).ceil().max(1.0) as i32;
		let monitor_right = monitor.origin.x.saturating_add_unsigned(monitor.width);
		let monitor_bottom = monitor.origin.y.saturating_add_unsigned(monitor.height);
		let max_x = monitor_right.saturating_sub(loupe_w_points).max(monitor.origin.x);
		let max_y = monitor_bottom.saturating_sub(loupe_h_points).max(monitor.origin.y);
		let gap = 10;
		let mut x;
		let mut y;

		if matches!(self.state.mode, OverlayMode::Live) {
			let Some(cursor) = self.state.cursor else {
				return false;
			};

			x = cursor.x.saturating_add(48);
			y = cursor.y.saturating_add(32);

			if x.saturating_add(loupe_w_points) > monitor_right {
				x = cursor.x.saturating_sub(48_i32.saturating_add(loupe_w_points));
			}
			if y.saturating_add(loupe_h_points) > monitor_bottom {
				y = cursor.y.saturating_sub(32_i32.saturating_add(loupe_h_points));
			}
		} else {
			let Some(hud_window) = self.hud_window.as_ref() else {
				return false;
			};
			let Some(hud_outer) = self.hud_outer_pos else {
				return false;
			};
			let hud_scale = hud_window.window.scale_factor().max(1.0);
			let hud_size = hud_window.window.inner_size();
			let hud_h_points = ((hud_size.height as f64) / hud_scale).ceil().max(1.0) as i32;
			let below_y = hud_outer.y.saturating_add(hud_h_points + gap);
			let above_y = hud_outer.y.saturating_sub(gap.saturating_add(loupe_h_points));

			x = hud_outer.x;
			y = if below_y.saturating_add(loupe_h_points) <= monitor_bottom {
				below_y
			} else {
				above_y
			};
		}

		x = x.clamp(monitor.origin.x, max_x);
		y = y.clamp(monitor.origin.y, max_y);

		let desired = GlobalPoint::new(x, y);

		if self.loupe_outer_pos == Some(desired) {
			self.pending_loupe_outer_pos = Some(desired);

			return true;
		}

		self.loupe_outer_pos = Some(desired);
		self.pending_loupe_outer_pos = Some(desired);

		true
	}

	fn update_toolbar_outer_position(&mut self, monitor: MonitorRect, local_pos: Pos2) -> bool {
		let Some(toolbar_window) = self.toolbar_window.as_ref() else {
			return false;
		};
		let toolbar_scale = toolbar_window.window.scale_factor().max(1.0);
		let toolbar_size = if let Some((width, height)) = self.toolbar_inner_size_points {
			Vec2::new(width as f32, height as f32)
		} else {
			let size = toolbar_window.window.inner_size();
			let toolbar_w = ((size.width as f64) / toolbar_scale).ceil().max(1.0) as f32;
			let toolbar_h = ((size.height as f64) / toolbar_scale).ceil().max(1.0) as f32;

			Vec2::new(toolbar_w, toolbar_h)
		};
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let clamped_local_pos = WindowRenderer::clamp_toolbar_position(
			screen_rect,
			toolbar_size,
			local_pos,
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		);
		let desired = GlobalPoint::new(
			monitor.origin.x.saturating_add(clamped_local_pos.x.round() as i32),
			monitor.origin.y.saturating_add(clamped_local_pos.y.round() as i32),
		);

		if self.toolbar_outer_pos == Some(desired) {
			return false;
		}

		self.toolbar_outer_pos = Some(desired);
		self.toolbar_state.floating_position = Some(clamped_local_pos);

		let started_at = Instant::now();

		toolbar_window
			.window
			.set_outer_position(LogicalPosition::new(desired.x as f64, desired.y as f64));
		self.slow_op_logger.warn_if_slow(
			"overlay.toolbar_window_set_outer_position",
			started_at.elapsed(),
			SLOW_OP_WARN_OUTER_POSITION,
			|| {
				format!(
					"window_id={:?} pos=({}, {})",
					toolbar_window.window.id(),
					desired.x,
					desired.y
				)
			},
		);
		toolbar_window.window.request_redraw();

		true
	}

	fn update_cursor_state(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		self.cursor_monitor = Some(monitor);
		self.state.cursor = Some(cursor);

		match self.state.mode {
			OverlayMode::Live => {},
			OverlayMode::Frozen => {
				if self.state.frozen_image.is_none() {
					return;
				}

				let frozen_monitor = self.state.monitor;

				self.state.rgb =
					image_helpers::frozen_rgb(&self.state.frozen_image, frozen_monitor, cursor);
				self.state.loupe = if self.state.alt_held {
					image_helpers::frozen_loupe_patch(
						&self.state.frozen_image,
						frozen_monitor,
						cursor,
						self.loupe_patch_width_px,
						self.loupe_patch_height_px,
					)
					.map(|patch| crate::state::LoupeSample { center: cursor, patch })
				} else {
					None
				};
			},
		}
	}

	#[cfg(target_os = "macos")]
	fn hide_capture_windows(&mut self) {}

	#[cfg(not(target_os = "macos"))]
	fn hide_capture_windows(&mut self) {
		self.capture_windows_hidden = true;

		if let Some(hud_window) = &self.hud_window {
			hud_window.window.set_visible(false);
		}
		if let Some(loupe_window) = &self.loupe_window {
			loupe_window.window.set_visible(false);
		}
	}

	fn restore_capture_windows_visibility(&mut self) {
		if !self.capture_windows_hidden {
			return;
		}

		self.capture_windows_hidden = false;

		#[cfg(not(target_os = "macos"))]
		if let Some(hud_window) = &self.hud_window {
			hud_window.window.set_visible(true);
		}
		#[cfg(not(target_os = "macos"))]
		if let Some(loupe_window) = &self.loupe_window {
			loupe_window.window.set_visible(self.state.alt_held);
		}
	}

	#[cfg(target_os = "macos")]
	fn raise_hud_windows(&self) {}

	#[cfg(not(target_os = "macos"))]
	fn raise_hud_windows(&self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			hud_window.window.focus_window();
		}

		if self.state.alt_held
			&& let Some(loupe_window) = self.loupe_window.as_ref()
		{
			loupe_window.window.focus_window();
		}
	}
}

impl Default for OverlaySession {
	fn default() -> Self {
		Self::new()
	}
}

struct ScrollPreviewStrip {
	texture: TextureHandle,
	size_points: Vec2,
}

struct LiveLoupeTexture {
	texture: TextureHandle,
	patch_size_px: [usize; 2],
	rgba: Vec<u8>,
}

struct ScrollPreviewWindow {
	window: Arc<winit::window::Window>,
	surface: Surface<'static>,
	surface_config: wgpu::SurfaceConfiguration,
	needs_reconfigure: bool,
	egui_ctx: egui::Context,
	egui_state: egui_winit::State,
	renderer: Renderer,
	preview_image: Option<ScrollPreviewStrip>,
}
impl ScrollPreviewWindow {
	fn new(event_loop: &ActiveEventLoop, gpu: &GpuContext) -> Result<Self, String> {
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

		egui_phosphor::add_to_fonts(&mut fonts, Variant::Regular);

		egui_ctx.set_fonts(fonts);

		let egui_state = egui_winit::State::new(
			egui_ctx.clone(),
			egui::ViewportId::ROOT,
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
		macos_configure_hud_window(window.as_ref(), false, 0.0, Some(18.0));

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

	fn handle_window_event(&mut self, event: &WindowEvent) {
		match event {
			WindowEvent::Resized(size) => self.resize(*size),
			WindowEvent::ScaleFactorChanged { .. } => self.resize(self.window.inner_size()),
			WindowEvent::ThemeChanged(_) => self.window.request_redraw(),
			_ => {},
		}

		let _ = self.egui_state.on_window_event(&self.window, event);

		self.window.request_redraw();
	}

	fn sync_image(&mut self, image: Option<&RgbaImage>) {
		self.preview_image = image.map(|image| {
			let preview_image = image_helpers::resize_scroll_preview_segment(image);
			let color_image = ColorImage::from_rgba_unmultiplied(
				[preview_image.width() as usize, preview_image.height() as usize],
				preview_image.as_raw(),
			);
			let texture = self.egui_ctx.load_texture(
				String::from("scroll-preview-image"),
				color_image,
				TextureOptions::LINEAR,
			);
			let ppp = self.window.scale_factor() as f32;
			let size_points =
				Vec2::new(preview_image.width() as f32 / ppp, preview_image.height() as f32 / ppp);

			ScrollPreviewStrip { texture, size_points }
		});
	}

	fn render_preview_ui(&mut self, view: ScrollPreviewView) -> FullOutput {
		let raw_input = self.egui_state.take_egui_input(&self.window);

		self.egui_ctx.run(raw_input, |ctx| {
			egui::CentralPanel::default().frame(Frame::new().fill(Color32::TRANSPARENT)).show(
				ctx,
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
						.stroke(egui::Stroke::new(1.0, tile_stroke))
						.corner_radius(CornerRadius::same(18))
						.inner_margin(Margin::symmetric(14, 14));

					tile_frame.show(ui, |ui| {
						ui.set_min_size(ui.available_size());

						if let Some(preview_image) = self.preview_image.as_ref() {
							let available = ui.available_size();
							let scale = (available.x / preview_image.size_points.x)
								.min(available.y / preview_image.size_points.y)
								.clamp(0.05, 1.0);
							let draw_size = preview_image.size_points * scale;

							ui.with_layout(
								Layout::centered_and_justified(egui::Direction::TopDown),
								|ui| {
									ui.image((preview_image.texture.id(), draw_size));
								},
							);
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
		let frame = self.acquire_frame(gpu)?;
		let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
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
						load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
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

		gpu.queue.submit(Some(encoder.finish()));
		frame.present();

		Ok(())
	}

	fn draw(&mut self, gpu: &GpuContext, theme: HudTheme, view: ScrollPreviewView) -> Result<()> {
		self.sync_surface_to_window(gpu);

		if self.needs_reconfigure {
			self.reconfigure_surface(gpu);
		}

		match theme {
			HudTheme::Dark => self.egui_ctx.set_visuals(egui::Visuals::dark()),
			HudTheme::Light => self.egui_ctx.set_visuals(egui::Visuals::light()),
		}

		let full_output = self.render_preview_ui(view);

		self.render_preview_frame(gpu, full_output)
	}

	fn acquire_frame(&mut self, gpu: &GpuContext) -> Result<SurfaceTexture> {
		match self.surface.get_current_texture() {
			Ok(frame) => Ok(frame),
			Err(SurfaceError::Outdated) => {
				self.reconfigure_surface(gpu);

				self.surface.get_current_texture().wrap_err("get_current_texture after reconfigure")
			},
			Err(SurfaceError::Lost) => {
				self.recreate_surface(gpu).wrap_err("recreate scroll preview surface")?;

				self.surface.get_current_texture().wrap_err("get_current_texture after recreate")
			},
			Err(err) => Err(eyre::eyre!("scroll preview get_current_texture failed: {err:?}")),
		}
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

	fn resize(&mut self, size: PhysicalSize<u32>) {
		self.surface_config.width = size.width.max(1);
		self.surface_config.height = size.height.max(1);
		self.needs_reconfigure = true;
	}
}

struct ScrollPreviewView {
	paused: bool,
	theme: HudTheme,
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
struct SelectionFlowGeometryCache {
	key: Option<SelectionFlowGeometryCacheKey>,
	samples: Vec<(Pos2, f32)>,
	normals: Vec<Vec2>,
}

struct HudOverlayWindow {
	window: Arc<winit::window::Window>,
	renderer: WindowRenderer,
}

#[derive(Debug, Default)]
struct HudRedrawSummary {
	request_toolbar_redraw: Option<MonitorRect>,
	renderer_draw_elapsed: Option<Duration>,
	request_inner_size_elapsed: Option<Duration>,
	position_update_elapsed: Option<Duration>,
	resize_target: Option<(u32, u32)>,
	redraw_window_id: Option<WindowId>,
	redraw_monitor_id: Option<u32>,
}

struct OverlayWindow {
	monitor: MonitorRect,
	window: Arc<winit::window::Window>,
	renderer: WindowRenderer,
	refresh_rate_millihertz: Option<u32>,
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
		.map_err(|err| eyre::eyre!("Failed to request GPU adapter: {err}"))?;
		let adapter_limits = adapter.limits();
		let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("rsnap-overlay device"),
			required_features: wgpu::Features::empty(),
			// Use the adapter's actual limits. Using `downlevel_defaults()` caps max texture
			// size to 2048, which breaks on common HiDPI displays.
			required_limits: adapter_limits,
			experimental_features: wgpu::ExperimentalFeatures::default(),
			memory_hints: wgpu::MemoryHints::Performance,
			trace: wgpu::Trace::Off,
		}))
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
	mipgen_pipeline: RenderPipeline,
	mipgen_surface_pipeline: RenderPipeline,
	mipgen_bind_group_layout: BindGroupLayout,
	hud_blur_pipeline: RenderPipeline,
	hud_blur_bind_group_layout: BindGroupLayout,
	hud_blur_uniform: wgpu::Buffer,
	hud_bg: Option<HudBg>,
	hud_bg_generation: u64,
	hud_pill: Option<HudPillGeometry>,
	loupe_tile: Option<Rect>,
	live_loupe_texture: Option<LiveLoupeTexture>,
	hud_theme: Option<HudTheme>,
	egui_start_time: Instant,
	egui_last_frame_time: Instant,
	selection_flow_cache: SelectionFlowGeometryCache,
	slow_op_logger: SlowOperationLogger,
}
impl WindowRenderer {
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
			source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
				"mipgen.wgsl"
			))),
		});
		let bind_group_layout =
			gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("rsnap-mipgen bgl"),
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
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen pipeline layout"),
			bind_group_layouts: &[&bind_group_layout],
			push_constant_ranges: &[],
		});
		let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-mipgen pipeline"),
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
					format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			multiview: None,
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
			source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
				"mipgen.wgsl"
			))),
		});
		let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline layout"),
			bind_group_layouts: &[bind_group_layout],
			push_constant_ranges: &[],
		});

		gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("rsnap-mipgen fullscreen pipeline"),
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
					format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			multiview: None,
			cache: None,
		})
	}

	fn generate_mipmaps(&self, gpu: &GpuContext, texture: &wgpu::Texture, mip_level_count: u32) {
		if mip_level_count <= 1 {
			return;
		}

		let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
			label: Some("rsnap-mipgen encoder"),
		});

		for level in 1..mip_level_count {
			let src_view = texture.create_view(&wgpu::TextureViewDescriptor {
				label: Some("rsnap-mipgen src view"),
				format: None,
				dimension: None,
				usage: None,
				aspect: wgpu::TextureAspect::All,
				base_mip_level: level - 1,
				mip_level_count: Some(1),
				base_array_layer: 0,
				array_layer_count: Some(1),
			});
			let dst_view = texture.create_view(&wgpu::TextureViewDescriptor {
				label: Some("rsnap-mipgen dst view"),
				format: None,
				dimension: None,
				usage: None,
				aspect: wgpu::TextureAspect::All,
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
						resource: wgpu::BindingResource::TextureView(&src_view),
					},
					wgpu::BindGroupEntry {
						binding: 1,
						resource: wgpu::BindingResource::Sampler(&self.bg_sampler),
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
						load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
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
			mipmap_filter: wgpu::FilterMode::Linear,
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
							min_binding_size: wgpu::BufferSize::new(
								mem::size_of::<HudBlurUniformRaw>() as u64,
							),
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

		if let Some(viewport) = raw_input.viewports.get_mut(&egui::ViewportId::ROOT) {
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
		selection_particles: bool,
		selection_flow_stroke_width_px: f32,
		needs_frozen_surface_bg: bool,
		show_frozen_capture_affordance: bool,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
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
		let mut _show_selection_particles = false;
		let egui_ctx = self.egui_ctx.clone();
		let full_output = egui_ctx.run(raw_input, |ctx| {
			Self::render_frozen_toolbar_ui(
				ctx,
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

			if selection_particles && matches!(state.mode, OverlayMode::Live) && !can_draw_hud {
				let screen_rect = ctx.input(|i| i.viewport_rect());
				let layer = egui::LayerId::new(
					egui::Order::Foreground,
					egui::Id::new(format!("live-capture-{}", monitor.id)),
				);
				let painter = ctx.layer_painter(layer);

				_show_selection_particles |= Self::render_live_capture_affordances(
					ctx,
					&painter,
					state,
					monitor,
					screen_rect,
					theme,
					selection_flow_stroke_width_px,
					selection_flow_geometry_cache,
				);
			}
			if selection_particles
				&& matches!(state.mode, OverlayMode::Frozen)
				&& (needs_frozen_surface_bg || show_frozen_capture_affordance)
				&& state.monitor == Some(monitor)
				&& state.frozen_capture_rect.is_some()
			{
				let screen_rect = ctx.input(|i| i.viewport_rect());

				_show_selection_particles |= Self::render_frozen_pending_affordance(
					ctx,
					state,
					monitor,
					screen_rect,
					theme,
					selection_flow_stroke_width_px,
					selection_flow_geometry_cache,
				);
			}
		});

		(full_output, hud_pill)
	}

	#[allow(clippy::too_many_arguments)]
	fn render_live_capture_affordances(
		ctx: &egui::Context,
		painter: &Painter,
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		theme: HudTheme,
		selection_flow_stroke_width_px: f32,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
	) -> bool {
		let mut has_rect = false;

		if !matches!(state.mode, OverlayMode::Live) {
			return false;
		}

		if let Some(hovered_window) = state.hovered_window_rect
			&& hovered_window.monitor_id == monitor.id
		{
			let rect = Rect::from_min_size(
				Pos2::new(hovered_window.rect.x as f32, hovered_window.rect.y as f32),
				Vec2::new(hovered_window.rect.width as f32, hovered_window.rect.height as f32),
			);
			let rect = rect.intersect(screen_rect);

			if rect.width() >= LIVE_DRAG_START_THRESHOLD_PX
				&& rect.height() >= LIVE_DRAG_START_THRESHOLD_PX
			{
				Self::render_selection_flow_ring(
					painter,
					rect,
					ctx,
					theme,
					SelectionFlowStyle::Band,
					selection_flow_stroke_width_px,
					selection_flow_geometry_cache,
				);

				has_rect = true;
			}
		}
		if let Some(drag_rect) = state.drag_rect
			&& drag_rect.monitor_id == monitor.id
			&& drag_rect.rect.width as f32 >= LIVE_DRAG_START_THRESHOLD_PX
			&& drag_rect.rect.height as f32 >= LIVE_DRAG_START_THRESHOLD_PX
		{
			let rect = Rect::from_min_size(
				Pos2::new(drag_rect.rect.x as f32, drag_rect.rect.y as f32),
				Vec2::new(drag_rect.rect.width as f32, drag_rect.rect.height as f32),
			);
			let rect = rect.intersect(screen_rect);

			Self::render_selection_flow_ring(
				painter,
				rect,
				ctx,
				theme,
				SelectionFlowStyle::FullBorder,
				selection_flow_stroke_width_px,
				selection_flow_geometry_cache,
			);

			has_rect = true;
		}

		let has_hovered_window_for_this_monitor =
			state.hovered_window_rect.is_some_and(|hovered| hovered.monitor_id == monitor.id);
		let has_drag_rect_for_this_monitor =
			state.drag_rect.is_some_and(|drag_rect| drag_rect.monitor_id == monitor.id);
		let cursor_on_monitor = state.cursor.is_some_and(|cursor| monitor.contains(cursor));
		let primary_not_down = !ctx.input(|i| i.pointer.primary_down());

		if !has_hovered_window_for_this_monitor
			&& !has_drag_rect_for_this_monitor
			&& cursor_on_monitor
			&& primary_not_down
		{
			Self::render_selection_flow_ring(
				painter,
				screen_rect,
				ctx,
				theme,
				SelectionFlowStyle::Band,
				selection_flow_stroke_width_px,
				selection_flow_geometry_cache,
			);

			has_rect = true;
		}

		has_rect
	}

	fn render_frozen_pending_affordance(
		ctx: &egui::Context,
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
		theme: HudTheme,
		selection_flow_stroke_width_px: f32,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
	) -> bool {
		let Some(capture_rect) = state.frozen_capture_rect else {
			return false;
		};
		let capture_width = capture_rect.width as f32;
		let capture_height = capture_rect.height as f32;

		if capture_width < LIVE_DRAG_START_THRESHOLD_PX
			|| capture_height < LIVE_DRAG_START_THRESHOLD_PX
		{
			return false;
		}

		let layer = egui::LayerId::new(
			egui::Order::Foreground,
			egui::Id::new(format!("frozen-pending-{}", monitor.id)),
		);
		let painter = ctx.layer_painter(layer);
		let rect = Rect::from_min_size(
			Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
			Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
		)
		.intersect(screen_rect);

		if rect.width() < LIVE_DRAG_START_THRESHOLD_PX
			|| rect.height() < LIVE_DRAG_START_THRESHOLD_PX
		{
			return false;
		}

		Self::render_selection_flow_ring(
			&painter,
			rect,
			ctx,
			theme,
			if state.frozen_capture_is_fullscreen_fallback {
				SelectionFlowStyle::Band
			} else {
				SelectionFlowStyle::FullBorder
			},
			selection_flow_stroke_width_px,
			selection_flow_geometry_cache,
		);

		true
	}

	fn render_selection_flow_ring(
		painter: &Painter,
		rect: Rect,
		ctx: &egui::Context,
		theme: HudTheme,
		style: SelectionFlowStyle,
		selection_flow_stroke_width_px: f32,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
	) {
		if rect.width() < LIVE_DRAG_START_THRESHOLD_PX
			|| rect.height() < LIVE_DRAG_START_THRESHOLD_PX
		{
			return;
		}

		let corner_radius = SELECTION_FLOW_CORNER_RADIUS_PX
			.min(rect.width() / 2.0 - 0.25)
			.min(rect.height() / 2.0 - 0.25)
			.max(0.0);
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);
		let time = ctx.input(|i| i.time) as f32;
		let sample_count = Self::selection_flow_sample_count(perimeter);
		let seam_offset = if rect.width() > corner_radius * 2.0 {
			(rect.width() - corner_radius * 2.0) * 0.5
		} else {
			0.0
		};
		let (samples, normals) = Self::selection_flow_cached_geometry(
			selection_flow_geometry_cache,
			rect,
			corner_radius,
			sample_count,
			seam_offset,
		);
		let base_alpha_scale = match theme {
			HudTheme::Light => 0.86,
			HudTheme::Dark => 1.0,
		};
		let stroke_width = selection_flow_stroke_width_px.clamp(1.0, 8.0);

		if samples.is_empty() {
			return;
		}

		let flow_time = time * SELECTION_FLOW_SPEED;
		let phase = flow_time * 1.28 + 0.72;

		match style {
			SelectionFlowStyle::Band => Self::selection_flow_draw_layer(
				painter,
				samples,
				normals,
				stroke_width,
				base_alpha_scale * 0.52,
				phase,
				SELECTION_FLOW_CORE_FLOW_WIDTH,
				theme,
			),
			SelectionFlowStyle::FullBorder => Self::selection_flow_draw_layer_full_border(
				painter,
				samples,
				normals,
				stroke_width,
				base_alpha_scale * SELECTION_FLOW_FROZEN_ALPHA_SCALE,
				phase,
				SELECTION_FLOW_FROZEN_INTENSITY,
				theme,
			),
		}
	}

	fn selection_flow_cached_geometry(
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		rect: Rect,
		corner_radius: f32,
		sample_count: usize,
		seam_offset: f32,
	) -> (&[(Pos2, f32)], &[Vec2]) {
		let key =
			SelectionFlowGeometryCacheKey::new(rect, corner_radius, seam_offset, sample_count);

		if selection_flow_geometry_cache.key == Some(key)
			&& !selection_flow_geometry_cache.samples.is_empty()
		{
			return (
				&selection_flow_geometry_cache.samples,
				&selection_flow_geometry_cache.normals,
			);
		}

		let samples =
			Self::selection_flow_path_samples(rect, corner_radius, sample_count, seam_offset);
		let normals = Self::selection_flow_compute_normals(&samples);

		selection_flow_geometry_cache.key = Some(key);
		selection_flow_geometry_cache.samples = samples;
		selection_flow_geometry_cache.normals = normals;

		(&selection_flow_geometry_cache.samples, &selection_flow_geometry_cache.normals)
	}

	fn selection_flow_compute_normals(samples: &[(Pos2, f32)]) -> Vec<Vec2> {
		let n = samples.len();

		if n == 0 {
			return Vec::new();
		}

		let mut normals = Vec::with_capacity(n);
		let mut first_non_zero = None;

		for i in 0..n {
			let (current_point, _) = samples[i];
			let (prev_point, _) = samples[(i + n - 1) % n];
			let (next_point, _) = samples[(i + 1) % n];
			let prev_tangent = current_point - prev_point;
			let next_tangent = next_point - current_point;
			let mut normal = Vec2::ZERO;

			if prev_tangent.length_sq() > f32::EPSILON {
				let prev_len = prev_tangent.length();

				normal += Vec2::new(-prev_tangent.y / prev_len, prev_tangent.x / prev_len);
			}
			if next_tangent.length_sq() > f32::EPSILON {
				let next_len = next_tangent.length();

				normal += Vec2::new(-next_tangent.y / next_len, next_tangent.x / next_len);
			}
			if normal.length_sq() <= f32::EPSILON {
				if next_tangent.length_sq() > f32::EPSILON {
					let next_len = next_tangent.length();

					normal = Vec2::new(-next_tangent.y / next_len, next_tangent.x / next_len);
				} else if prev_tangent.length_sq() > f32::EPSILON {
					let prev_len = prev_tangent.length();

					normal = Vec2::new(-prev_tangent.y / prev_len, prev_tangent.x / prev_len);
				}
			}

			let normal = if normal.length_sq() > f32::EPSILON {
				let normalized = normal / normal.length();

				if first_non_zero.is_none() && normalized.length_sq() > f32::EPSILON {
					first_non_zero = Some(i);
				}

				normalized
			} else {
				Vec2::ZERO
			};

			normals.push(normal);
		}

		if let Some(first_idx) = first_non_zero {
			let mut previous = normals[first_idx];

			for normal in normals.iter_mut().skip(first_idx + 1) {
				if normal.length_sq() > f32::EPSILON && normal.dot(previous) < 0.0 {
					*normal = -*normal;
				}
				if normal.length_sq() > f32::EPSILON {
					previous = *normal;
				}
			}
			for normal in normals.iter_mut().take(first_idx).rev() {
				if normal.length_sq() > f32::EPSILON && normal.dot(previous) < 0.0 {
					*normal = -*normal;
				}
				if normal.length_sq() > f32::EPSILON {
					previous = *normal;
				}
			}

			if normals[first_idx].length_sq() > f32::EPSILON
				&& normals[(first_idx + n - 1) % n].length_sq() > f32::EPSILON
				&& normals[first_idx].dot(normals[(first_idx + n - 1) % n]) < 0.0
			{
				for normal in &mut normals {
					*normal = -*normal;
				}
			}
		}

		normals
	}

	#[allow(clippy::too_many_arguments)]
	fn selection_flow_draw_layer(
		painter: &Painter,
		samples: &[(Pos2, f32)],
		normals: &[Vec2],
		line_width: f32,
		alpha_scale: f32,
		phase: f32,
		flow_band_width: f32,
		theme: HudTheme,
	) {
		if samples.is_empty() || normals.is_empty() || samples.len() != normals.len() {
			return;
		}

		let half = (line_width * 0.5).max(0.1);
		let n = samples.len();
		let mut mesh = egui::epaint::Mesh::default();

		for i in 0..n {
			let (current_point, t) = samples[i];
			let movement = Self::selection_flow_flow_band(t, phase, flow_band_width);
			let intensity = SELECTION_FLOW_FLOW_BOOST * movement;
			let color = Self::selection_flow_color(t + phase, theme, alpha_scale, intensity);
			let normal = normals[i] * half;

			mesh.colored_vertex(current_point + normal, color);
			mesh.colored_vertex(current_point - normal, color);
		}
		for i in 0..n {
			let i0 = (i * 2) as u32;
			let i1 = ((i * 2) + 1) as u32;
			let n0 = (((i + 1) % n) * 2) as u32;
			let n1 = (((i + 1) % n) * 2 + 1) as u32;

			mesh.add_triangle(i0, i1, n0);
			mesh.add_triangle(i1, n1, n0);
		}

		painter.add(egui::Shape::Mesh(mesh.into()));
	}

	#[allow(clippy::too_many_arguments)]
	fn selection_flow_draw_layer_full_border(
		painter: &Painter,
		samples: &[(Pos2, f32)],
		normals: &[Vec2],
		line_width: f32,
		alpha_scale: f32,
		phase: f32,
		intensity: f32,
		theme: HudTheme,
	) {
		if samples.is_empty() || normals.is_empty() || samples.len() != normals.len() {
			return;
		}

		let half = (line_width * 0.5).max(0.1);
		let n = samples.len();
		let mut mesh = egui::epaint::Mesh::default();

		for i in 0..n {
			let (current_point, t) = samples[i];
			let color = Self::selection_flow_color(t + phase, theme, alpha_scale, intensity);
			let normal = normals[i] * half;

			mesh.colored_vertex(current_point + normal, color);
			mesh.colored_vertex(current_point - normal, color);
		}
		for i in 0..n {
			let i0 = (i * 2) as u32;
			let i1 = ((i * 2) + 1) as u32;
			let n0 = (((i + 1) % n) * 2) as u32;
			let n1 = (((i + 1) % n) * 2 + 1) as u32;

			mesh.add_triangle(i0, i1, n0);
			mesh.add_triangle(i1, n1, n0);
		}

		painter.add(egui::Shape::Mesh(mesh.into()));
	}

	fn selection_flow_flow_band(progress: f32, phase: f32, band_width: f32) -> f32 {
		let width = band_width.clamp(0.001, 0.5);
		let distance = (progress - phase).rem_euclid(1.0);
		let distance = distance.min(1.0 - distance);
		let normalized = (distance / width).min(1.0);

		(1.0 - normalized).powf(2.0)
	}

	fn selection_flow_sample_count(perimeter: f32) -> usize {
		if perimeter <= 0.0 || !perimeter.is_finite() {
			return SELECTION_FLOW_MIN_SEGMENTS;
		}

		let by_step = (perimeter / SELECTION_FLOW_SAMPLE_STEP_PX).ceil() as usize;

		by_step.clamp(SELECTION_FLOW_MIN_SEGMENTS, SELECTION_FLOW_MAX_SEGMENTS)
	}

	fn selection_flow_path_samples(
		rect: Rect,
		corner_radius: f32,
		sample_count: usize,
		start_offset: f32,
	) -> Vec<(Pos2, f32)> {
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);

		if perimeter <= 0.0 {
			return Vec::new();
		}

		let start = (start_offset / perimeter).rem_euclid(1.0);

		(0..sample_count)
			.map(|index| {
				let t = (index as f32 + 0.5) / sample_count as f32;
				let progress = (t + start).rem_euclid(1.0);

				(
					Self::selection_flow_sample_at_distance(
						rect,
						corner_radius,
						perimeter * progress,
					),
					t,
				)
			})
			.collect()
	}

	fn selection_flow_sample_at_distance(rect: Rect, corner_radius: f32, distance: f32) -> Pos2 {
		if corner_radius <= f32::EPSILON {
			let perimeter = Self::selection_flow_perimeter(rect, 0.0);
			let keep = distance.rem_euclid(perimeter);
			let edge_top = rect.width();
			let edge_right = rect.height();

			if keep < edge_top {
				return Pos2::new(rect.min.x + keep, rect.min.y);
			}
			if keep < edge_top + edge_right {
				return Pos2::new(rect.max.x, rect.min.y + (keep - edge_top));
			}
			if keep < edge_top * 2.0 + edge_right {
				return Pos2::new(rect.max.x - (keep - edge_top - edge_right), rect.max.y);
			}

			return Pos2::new(rect.min.x, rect.max.y - (keep - edge_top * 2.0 - edge_right));
		}

		let x0 = rect.min.x;
		let x1 = rect.max.x;
		let y0 = rect.min.y;
		let y1 = rect.max.y;
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);
		let remain = distance.rem_euclid(perimeter);
		let edge_top_len = (rect.width() - corner_radius * 2.0).max(0.0);
		let edge_right_len = (rect.height() - corner_radius * 2.0).max(0.0);
		let corner_len = std::f32::consts::FRAC_PI_2 * corner_radius;

		if remain < edge_top_len {
			return Pos2::new(x0 + corner_radius + remain, y0);
		}

		let mut offset = remain - edge_top_len;

		if offset < corner_len {
			let angle = -std::f32::consts::FRAC_PI_2 + offset / corner_radius;

			return Pos2::new(
				x1 - corner_radius + corner_radius * angle.cos(),
				y0 + corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_right_len {
			return Pos2::new(x1, y0 + corner_radius + offset);
		}

		offset -= edge_right_len;

		if offset < corner_len {
			let angle = offset / corner_radius;

			return Pos2::new(
				x1 - corner_radius + corner_radius * angle.cos(),
				y1 - corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_top_len {
			return Pos2::new(x1 - corner_radius - offset, y1);
		}

		offset -= edge_top_len;

		if offset < corner_len {
			let angle = std::f32::consts::FRAC_PI_2 + offset / corner_radius;

			return Pos2::new(
				x0 + corner_radius + corner_radius * angle.cos(),
				y1 - corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_right_len {
			return Pos2::new(x0, y1 - corner_radius - offset);
		}

		offset -= edge_right_len;

		if offset < corner_len {
			let angle = std::f32::consts::PI + offset / corner_radius;

			return Pos2::new(
				x0 + corner_radius + corner_radius * angle.cos(),
				y0 + corner_radius + corner_radius * angle.sin(),
			);
		}

		Pos2::new(x0 + corner_radius, y0)
	}

	fn selection_flow_perimeter(rect: Rect, corner_radius: f32) -> f32 {
		let edge_top_len = (rect.width() - corner_radius * 2.0).max(0.0);
		let edge_right_len = (rect.height() - corner_radius * 2.0).max(0.0);
		let corner_len = std::f32::consts::FRAC_PI_2 * corner_radius;

		2.0 * (edge_top_len + edge_right_len) + 4.0 * corner_len
	}

	fn selection_flow_color(
		progress: f32,
		theme: HudTheme,
		alpha_scale: f32,
		intensity: f32,
	) -> Color32 {
		let palette = SELECTION_FLOW_PALETTE;
		let normalized = progress.rem_euclid(1.0);
		let band_position = normalized * palette.len() as f32;
		let band = band_position.floor() as usize % palette.len();
		let local = band_position - band as f32;
		let (r0, g0, b0) = palette[band];
		let (r1, g1, b1) = palette[(band + 1) % palette.len()];
		let blend = |a: u8, b: u8, ratio: f32| -> u8 {
			(a as f32 + (b as f32 - a as f32) * ratio).clamp(0.0, 255.0).round() as u8
		};
		let theme_alpha = match theme {
			HudTheme::Dark => 1.0,
			HudTheme::Light => 0.82,
		};
		let alpha = (255.0 * alpha_scale * intensity * theme_alpha).clamp(0.0, 255.0);

		Color32::from_rgba_unmultiplied(
			blend(r0, r1, local),
			blend(g0, g1, local),
			blend(b0, b1, local),
			alpha as u8,
		)
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frozen_toolbar_ui(
		ctx: &egui::Context,
		state: &OverlayState,
		monitor: MonitorRect,
		theme: HudTheme,
		toolbar_placement: ToolbarPlacement,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		toolbar_state: Option<&mut FrozenToolbarState>,
		pointer_state: Option<FrozenToolbarPointerState>,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let Some(toolbar_state) = toolbar_state else {
			return;
		};

		if !matches!(state.mode, OverlayMode::Frozen) || !toolbar_state.visible {
			return;
		}
		if state.monitor != Some(monitor) {
			return;
		}

		let (cursor, left_button_down) = if let Some(pointer_state) = pointer_state {
			(pointer_state.cursor_local, pointer_state.left_button_down)
		} else {
			toolbar_state.dragging = false;

			(Pos2::new(-1.0, -1.0), false)
		};
		let toolbar_size = Self::frozen_toolbar_size(toolbar_state);
		let screen_rect = ctx.input(|i| i.viewport_rect());
		let capture_rect = Self::frozen_toolbar_capture_rect(state, monitor, screen_rect);
		let Some(toolbar_pos) = Self::resolve_frozen_toolbar_birth(
			ctx,
			state,
			monitor,
			toolbar_state,
			screen_rect,
			capture_rect,
			toolbar_size,
			toolbar_placement,
		) else {
			return;
		};

		Self::draw_frozen_toolbar(
			ctx,
			toolbar_state,
			monitor,
			screen_rect,
			toolbar_pos,
			toolbar_size,
			theme,
			hud_blur_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
			cursor,
			left_button_down,
			hud_pill_out,
		);
	}

	fn frozen_toolbar_tools(toolbar_state: &FrozenToolbarState) -> &'static [FrozenToolbarTool] {
		const TOOLS_SCROLL_MODE: [FrozenToolbarTool; 2] =
			[FrozenToolbarTool::Copy, FrozenToolbarTool::Save];
		const TOOLS_WITH_SCROLL: [FrozenToolbarTool; 9] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Scroll,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];
		const TOOLS_WITHOUT_SCROLL: [FrozenToolbarTool; 8] = [
			FrozenToolbarTool::Pointer,
			FrozenToolbarTool::Pen,
			FrozenToolbarTool::Text,
			FrozenToolbarTool::Mosaic,
			FrozenToolbarTool::Undo,
			FrozenToolbarTool::Redo,
			FrozenToolbarTool::Copy,
			FrozenToolbarTool::Save,
		];

		if toolbar_state.scroll_capture_active {
			&TOOLS_SCROLL_MODE
		} else if toolbar_state.scroll_capture_available {
			&TOOLS_WITH_SCROLL
		} else {
			&TOOLS_WITHOUT_SCROLL
		}
	}

	fn frozen_toolbar_size(toolbar_state: &FrozenToolbarState) -> Vec2 {
		let tool_count = Self::frozen_toolbar_tools(toolbar_state).len() as f32;
		let spacing_count = (tool_count - 1.0).max(0.0);
		let width = tool_count * FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
			+ spacing_count * FROZEN_TOOLBAR_ITEM_SPACING_POINTS
			+ 2.0 * HUD_PILL_INNER_MARGIN_X_POINTS
			+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
		let height = toolbar_state.pill_height_points.unwrap_or(TOOLBAR_EXPANDED_HEIGHT_PX);

		Vec2::new(width, height)
	}

	#[allow(clippy::too_many_arguments)]
	fn resolve_frozen_toolbar_birth(
		ctx: &egui::Context,
		state: &OverlayState,
		monitor: MonitorRect,
		toolbar_state: &mut FrozenToolbarState,
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Option<Pos2> {
		if let Some(pos) = toolbar_state.floating_position {
			return Some(pos);
		}

		let screen_size_points = screen_rect.size();

		tracing::trace!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			screen_rect = ?screen_rect,
			screen_size_points = ?screen_size_points,
			pixels_per_point = ctx.pixels_per_point(),
			last_screen_size_points = ?toolbar_state.layout_last_screen_size_points,
			stable_frames = toolbar_state.layout_stable_frames,
			"Frozen toolbar birth attempt."
		);

		let needs_new_sample = match toolbar_state.layout_last_screen_size_points {
			None => true,
			Some(last) => {
				let dx = (last.x - screen_size_points.x).abs();
				let dy = (last.y - screen_size_points.y).abs();

				dx > 0.5 || dy > 0.5
			},
		};

		if needs_new_sample {
			toolbar_state.layout_last_screen_size_points = Some(screen_size_points);
			toolbar_state.layout_stable_frames = 0;
			toolbar_state.needs_redraw = true;

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = state.frozen_generation,
				new_screen_size_points = ?screen_size_points,
				"Frozen toolbar waiting for stable screen rect (new sample)."
			);

			ctx.request_repaint();

			return None;
		}
		if toolbar_state.layout_stable_frames < 1 {
			toolbar_state.layout_stable_frames =
				toolbar_state.layout_stable_frames.saturating_add(1);
			toolbar_state.needs_redraw = true;

			tracing::debug!(
				monitor_id = monitor.id,
				frozen_generation = state.frozen_generation,
				screen_size_points = ?screen_size_points,
				stable_frames = toolbar_state.layout_stable_frames,
				"Frozen toolbar waiting for stable screen rect (confirm)."
			);

			ctx.request_repaint();

			return None;
		}

		let default_pos = Self::frozen_toolbar_default_pos(
			screen_rect,
			capture_rect,
			toolbar_size,
			toolbar_placement,
		);

		tracing::debug!(
			monitor_id = monitor.id,
			frozen_generation = state.frozen_generation,
			toolbar_size_points = ?toolbar_size,
			default_pos = ?default_pos,
			"Frozen toolbar birth resolved."
		);

		toolbar_state.floating_position = Some(default_pos);

		Some(default_pos)
	}

	fn frozen_toolbar_capture_rect(
		state: &OverlayState,
		monitor: MonitorRect,
		screen_rect: Rect,
	) -> Rect {
		let Some(capture_rect) = state.frozen_capture_rect else {
			return screen_rect;
		};
		let Some(frozen_monitor) = state.monitor else {
			return screen_rect;
		};

		if frozen_monitor != monitor {
			return screen_rect;
		}

		let capture_rect = Rect::from_min_size(
			Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
			Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
		);

		capture_rect.intersect(screen_rect)
	}

	fn frozen_toolbar_default_pos(
		screen_rect: Rect,
		capture_rect: Rect,
		toolbar_size: Vec2,
		toolbar_placement: ToolbarPlacement,
	) -> Pos2 {
		let y = match toolbar_placement {
			ToolbarPlacement::Bottom => {
				let below_y = capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX;
				let within_screen =
					below_y + toolbar_size.y + TOOLBAR_SCREEN_MARGIN_PX <= screen_rect.max.y;

				if within_screen {
					below_y
				} else {
					capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y
				}
			},
			ToolbarPlacement::Top => {
				let above_y = capture_rect.min.y - TOOLBAR_CAPTURE_GAP_PX - toolbar_size.y;
				let within_screen = above_y >= screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;

				if within_screen { above_y } else { capture_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX }
			},
		};
		let min_x = screen_rect.min.x + TOOLBAR_SCREEN_MARGIN_PX;
		let min_y = screen_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX;
		let max_x = (screen_rect.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(min_x);
		let max_y = (screen_rect.max.y - toolbar_size.y - TOOLBAR_SCREEN_MARGIN_PX).max(min_y);
		let x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(min_x, max_x);
		let y = y.max(min_y).min(max_y);

		Pos2::new(x, y)
	}

	#[allow(clippy::too_many_arguments)]
	fn draw_frozen_toolbar(
		ctx: &egui::Context,
		toolbar_state: &mut FrozenToolbarState,
		monitor: MonitorRect,
		screen_rect: Rect,
		toolbar_pos: Pos2,
		toolbar_size: Vec2,
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		cursor: Pos2,
		left_button_down: bool,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		egui::Area::new(egui::Id::new(format!("frozen-toolbar-{}", monitor.id)))
			.order(egui::Order::Foreground)
			.fixed_pos(toolbar_pos)
			.show(ctx, |ui| {
				let (rect, response) =
					ui.allocate_exact_size(toolbar_size, egui::Sense::click_and_drag());
				let body_fill = Self::tinted_hud_body_fill(
					theme,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					hud_tint_hue,
				);
				let toolbar_frame =
					Self::hud_pill_frame(theme, hud_opaque, hud_opacity, body_fill, false);

				if response.drag_started() {
					toolbar_state.dragging = true;
					toolbar_state.floating_position = Some(toolbar_pos);
					toolbar_state.drag_offset = cursor - toolbar_pos;
				}
				if toolbar_state.dragging && left_button_down {
					let desired_pos = cursor - toolbar_state.drag_offset;

					toolbar_state.floating_position = Some(Self::clamp_toolbar_position(
						screen_rect,
						toolbar_size,
						desired_pos,
						TOOLBAR_SCREEN_MARGIN_PX,
						TOOLBAR_SCREEN_MARGIN_PX,
					));
				} else if toolbar_state.dragging {
					toolbar_state.dragging = false;
				}

				// Draw the capsule ourselves at the exact allocated rect. This keeps the visible pill
				// and the blur rect perfectly aligned (no shrink-to-content surprises on first frame).
				ui.painter().rect_filled(
					rect,
					f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
					toolbar_frame.fill,
				);
				ui.painter().rect_stroke(
					rect.shrink(0.5),
					CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS),
					toolbar_frame.stroke,
					egui::StrokeKind::Inside,
				);

				let inner_stroke_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
					HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
				};
				let inner_stroke = egui::Stroke::new(1.0, inner_stroke_color);
				let inner_rect = rect.shrink(1.0);

				ui.painter().rect_stroke(
					inner_rect,
					CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS.saturating_sub(1)),
					inner_stroke,
					egui::StrokeKind::Inside,
				);

				let inner_rect = rect.shrink2(egui::vec2(
					HUD_PILL_INNER_MARGIN_X_POINTS,
					HUD_PILL_INNER_MARGIN_Y_POINTS,
				));
				let _ = ui.scope_builder(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
					ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
						ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);

						Self::render_frozen_toolbar_controls(ui, toolbar_state, theme);
					});
				});

				*hud_pill_out = Some(HudPillGeometry {
					rect,
					radius_points: f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
				});
			});
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frozen_toolbar_controls(
		ui: &mut Ui,
		toolbar_state: &mut FrozenToolbarState,
		theme: HudTheme,
	) {
		if toolbar_state.selected_tool == FrozenToolbarTool::Scroll {
			toolbar_state.selected_tool = FrozenToolbarTool::Pointer;
		}

		let tools = Self::frozen_toolbar_tools(toolbar_state);
		let button_size = FROZEN_TOOLBAR_BUTTON_SIZE_POINTS;
		let button_font_size = 18.0;
		let item_spacing = FROZEN_TOOLBAR_ITEM_SPACING_POINTS;
		let hit_area_inset = 5.0;
		let (normal_color, hover_color, selected_color, hover_bg, selected_bg, selected_border) =
			Self::frozen_toolbar_colors(theme);

		ui.horizontal_centered(|ui| {
			ui.spacing_mut().item_spacing.x = item_spacing;

			for tool in tools {
				let is_mode_tool = tool.is_mode_tool();
				let response =
					ui.allocate_response(Vec2::new(button_size, button_size), egui::Sense::click());
				let hovered = response.hovered();
				let response = response.on_hover_text(tool.label());
				let hover_anim: f32 = if hovered { 1.0 } else { 0.0 };

				if response.clicked() {
					let tool = *tool;

					if is_mode_tool {
						toolbar_state.selected_tool = tool;
					} else {
						toolbar_state.pending_action = Some(tool);
					}

					toolbar_state.needs_redraw = true;
				}

				let selected = is_mode_tool && *tool == toolbar_state.selected_tool;
				let selected_anim: f32 = if selected { 1.0 } else { 0.0 };
				let glow = hover_anim.max(selected_anim);
				let icon_font = if selected {
					FontFamily::Name("phosphor-fill".into())
				} else {
					FontFamily::Proportional
				};
				let mut icon_color = normal_color;
				let mut bg_color = Color32::from_rgba_unmultiplied(255, 255, 255, 0);
				let mut border_alpha = 0.0;

				if selected_anim > 0.0 {
					icon_color = Self::blend_color(icon_color, selected_color, selected_anim);
					bg_color = Self::blend_color(bg_color, selected_bg, selected_anim);
					border_alpha = selected_anim;
				}
				if hover_anim > 0.0 {
					icon_color = Self::blend_color(icon_color, hover_color, hover_anim);
					bg_color =
						Self::blend_color(bg_color, hover_bg, hover_anim * (1.0 - selected_anim));
				}
				if glow > 0.0 {
					let bg_rect = response.rect.shrink(hit_area_inset);

					ui.painter().rect_filled(bg_rect, 8.0, bg_color);
				}
				if border_alpha > 0.0 {
					let selected_border = Color32::from_rgba_unmultiplied(
						selected_border.r(),
						selected_border.g(),
						selected_border.b(),
						(selected_border.a() as f32 * border_alpha).round() as u8,
					);

					ui.painter().rect_stroke(
						response.rect.shrink(hit_area_inset),
						8.0,
						egui::Stroke::new(1.0, selected_border),
						egui::StrokeKind::Inside,
					);
				}

				ui.painter().text(
					response.rect.center(),
					Align2::CENTER_CENTER,
					tool.icon(),
					FontId::new(button_font_size, icon_font),
					icon_color,
				);
			}
		});
	}

	fn frozen_toolbar_colors(
		theme: HudTheme,
	) -> (Color32, Color32, Color32, Color32, Color32, Color32) {
		let (normal_color, hover_color, selected_color) = match theme {
			HudTheme::Dark => (
				Color32::from_rgba_unmultiplied(255, 255, 255, 160),
				Color32::from_rgba_unmultiplied(255, 255, 255, 222),
				Color32::from_rgba_unmultiplied(255, 255, 255, 255),
			),
			HudTheme::Light => (
				Color32::from_rgba_unmultiplied(28, 28, 32, 182),
				Color32::from_rgba_unmultiplied(28, 28, 32, 220),
				Color32::from_rgba_unmultiplied(28, 28, 32, 255),
			),
		};
		let hover_bg = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 20),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 20),
		};
		let selected_bg = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 28),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 24),
		};
		let selected_border = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 82),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 72),
		};

		(normal_color, hover_color, selected_color, hover_bg, selected_bg, selected_border)
	}

	fn blend_color(a: Color32, b: Color32, t: f32) -> Color32 {
		let t = t.clamp(0.0, 1.0);
		let u = 1.0 - t;

		Color32::from_rgba_unmultiplied(
			((f32::from(a.r()) * u + f32::from(b.r()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.g()) * u + f32::from(b.g()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.b()) * u + f32::from(b.b()) * t).round().clamp(0.0, 255.0)) as u8,
			((f32::from(a.a()) * u + f32::from(b.a()) * t).round().clamp(0.0, 255.0)) as u8,
		)
	}

	fn clamp_toolbar_position(
		screen_rect: Rect,
		toolbar_size: Vec2,
		cursor: Pos2,
		side_margin: f32,
		top_margin: f32,
	) -> Pos2 {
		let min_x = screen_rect.min.x + side_margin;
		let min_y = screen_rect.min.y + top_margin;
		let max_x = (screen_rect.max.x - toolbar_size.x - side_margin).max(min_x);
		let max_y = (screen_rect.max.y - toolbar_size.y - top_margin * 0.5).max(min_y);

		Pos2::new(cursor.x.clamp(min_x, max_x.max(min_x)), cursor.y.clamp(min_y, max_y.max(min_y)))
	}

	fn should_draw_hud(state: &OverlayState, monitor: MonitorRect) -> bool {
		if cfg!(target_os = "macos") && matches!(state.mode, OverlayMode::Frozen) {
			return true;
		}

		!matches!(state.mode, OverlayMode::Frozen)
			|| state.monitor != Some(monitor)
			|| state.frozen_image.is_some()
			|| state.error_message.is_some()
	}

	#[allow(clippy::too_many_arguments)]
	fn render_hud(
		&mut self,
		ctx: &egui::Context,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		local_cursor: Pos2,
		hud_compact: bool,
		hud_anchor: HudAnchor,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme: HudTheme,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let (hud_x, hud_y) = match hud_anchor {
			HudAnchor::Cursor => (local_cursor.x + 14.0, local_cursor.y + 14.0),
		};

		egui::Area::new("hud".into())
			.order(egui::Order::Foreground)
			.fixed_pos(Pos2::new(hud_x, hud_y))
			.show(ctx, |ui| {
				self.render_hud_frame(
					ui,
					state,
					monitor,
					cursor,
					hud_compact,
					show_alt_hint_keycap,
					hud_blur_active,
					hud_opaque,
					hud_opacity,
					hud_milk_amount,
					hud_tint_hue,
					theme,
					hud_pill_out,
				);
			});
	}

	#[allow(clippy::too_many_arguments)]
	fn render_hud_frame(
		&mut self,
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		hud_compact: bool,
		show_alt_hint_keycap: bool,
		hud_blur_active: bool,
		hud_opaque: bool,
		hud_opacity: f32,
		hud_milk_amount: f32,
		hud_tint_hue: f32,
		theme: HudTheme,
		hud_pill_out: &mut Option<HudPillGeometry>,
	) {
		let body_fill = Self::tinted_hud_body_fill(
			theme,
			hud_blur_active,
			hud_opaque,
			hud_opacity,
			hud_milk_amount,
			hud_tint_hue,
		);
		let pill_frame =
			Self::hud_pill_frame(theme, hud_opaque, hud_opacity, body_fill, !hud_compact);
		let inner = pill_frame.show(ui, |ui| {
			ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);

			if let Some(err) = &state.error_message {
				let err_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(235, 235, 245, 235),
					HudTheme::Light => Color32::from_rgba_unmultiplied(28, 28, 32, 235),
				};

				ui.label(egui::RichText::new(err).color(err_color).monospace());
			} else {
				Self::render_hud_content(ui, state, monitor, cursor, show_alt_hint_keycap, theme);
			}
		});
		let pill_rect = inner.response.rect;

		*hud_pill_out = Some(HudPillGeometry {
			rect: pill_rect,
			radius_points: f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
		});

		if hud_compact {
			return;
		}

		let inner_stroke_color = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
			HudTheme::Light => Color32::from_rgba_unmultiplied(255, 255, 255, 140),
		};
		let inner_stroke = egui::Stroke::new(1.0, inner_stroke_color);
		let inner_rect = pill_rect.shrink(1.0);

		ui.painter().rect_stroke(
			inner_rect,
			CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS.saturating_sub(1)),
			inner_stroke,
			egui::StrokeKind::Inside,
		);

		if !hud_compact {
			self.render_loupe_tile(
				ui,
				state,
				pill_rect,
				hud_blur_active,
				hud_opaque,
				body_fill,
				theme,
			);
		}
	}

	fn hud_pill_frame(
		theme: HudTheme,
		_hud_opaque: bool,
		_hud_opacity: f32,
		body_fill: Color32,
		with_shadow: bool,
	) -> Frame {
		let outer_stroke_color = match theme {
			HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
			HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
		};
		let pill_shadow = if with_shadow {
			egui::epaint::Shadow {
				offset: [0, 0],
				blur: 10,
				spread: 0,
				color: match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 28),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 18),
				},
			}
		} else {
			egui::epaint::Shadow::NONE
		};

		Frame {
			fill: body_fill,
			stroke: egui::Stroke::new(1.0, outer_stroke_color),
			shadow: pill_shadow,
			corner_radius: CornerRadius::same(HUD_PILL_CORNER_RADIUS_POINTS),
			inner_margin: Margin::symmetric(12, 8),
			..Frame::default()
		}
	}

	fn render_hud_content(
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		show_alt_hint_keycap: bool,
		theme: HudTheme,
	) {
		let (label_color, secondary_color) = match theme {
			HudTheme::Dark => (
				Color32::from_rgba_unmultiplied(235, 235, 245, 235),
				Color32::from_rgba_unmultiplied(235, 235, 245, 150),
			),
			HudTheme::Light => (
				Color32::from_rgba_unmultiplied(28, 28, 32, 235),
				Color32::from_rgba_unmultiplied(28, 28, 32, 160),
			),
		};
		let pos_text = hud_helpers::format_live_hud_position_text(monitor, cursor);
		let (hex_text, rgb_text) = hud_helpers::format_live_hud_rgb_text(state.rgb);
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
					egui::Stroke::new(
						1.0,
						match theme {
							HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 36),
							HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
						},
					),
					egui::StrokeKind::Inside,
				);
				ui.label(egui::RichText::new(hex_text).color(label_color).monospace());
				ui.label(egui::RichText::new(rgb_text).color(secondary_color).monospace());

				if show_alt_hint_keycap {
					let alt_active = state.alt_held;
					let (keycap_fill, keycap_stroke, keycap_text) = match theme {
						HudTheme::Dark if alt_active => (
							Color32::from_rgba_unmultiplied(255, 255, 255, 40),
							egui::Stroke::new(
								1.0,
								Color32::from_rgba_unmultiplied(255, 255, 255, 70),
							),
							label_color,
						),
						HudTheme::Dark => (
							Color32::from_rgba_unmultiplied(255, 255, 255, 18),
							egui::Stroke::new(
								1.0,
								Color32::from_rgba_unmultiplied(255, 255, 255, 30),
							),
							secondary_color,
						),
						HudTheme::Light if alt_active => (
							Color32::from_rgba_unmultiplied(0, 0, 0, 22),
							egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 64)),
							label_color,
						),
						HudTheme::Light => (
							Color32::from_rgba_unmultiplied(0, 0, 0, 12),
							egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 32)),
							secondary_color,
						),
					};

					Frame {
						fill: keycap_fill,
						stroke: keycap_stroke,
						corner_radius: CornerRadius::same(6),
						inner_margin: Margin::symmetric(6, 2),
						..Frame::default()
					}
					.show(ui, |ui| {
						ui.label(egui::RichText::new("Alt").color(keycap_text).monospace());
					});
				}
			});
		});
	}

	#[allow(clippy::too_many_arguments)]
	fn render_loupe_tile(
		&mut self,
		ui: &mut Ui,
		state: &OverlayState,
		pill_rect: Rect,
		hud_blur_active: bool,
		hud_opaque: bool,
		body_fill: Color32,
		theme: HudTheme,
	) {
		let ctx = ui.ctx().clone();

		self.loupe_tile = None;

		if !state.alt_held {
			return;
		}

		const CELL: f32 = 10.0;

		let side = hud_helpers::stable_live_loupe_side_points(state, CELL);
		let tile_padding = Margin::same(10);
		let tile_w = side + (tile_padding.left as f32) + (tile_padding.right as f32);
		let tile_h = side + (tile_padding.top as f32) + (tile_padding.bottom as f32);
		let screen = ctx.content_rect();
		let gap = 10.0;
		let mut x = pill_rect.min.x;

		x = x.clamp(screen.min.x + 6.0, (screen.max.x - tile_w - 6.0).max(screen.min.x + 6.0));

		let below_y = pill_rect.max.y + gap;
		let above_y = pill_rect.min.y - gap - tile_h;
		let mut y = if below_y + tile_h <= screen.max.y { below_y } else { above_y };

		y = y.clamp(screen.min.y + 6.0, (screen.max.y - tile_h - 6.0).max(screen.min.y + 6.0));

		let pos = Pos2::new(x, y);
		let tile = egui::Area::new(egui::Id::new("rsnap-loupe-tile"))
			.order(egui::Order::Foreground)
			.fixed_pos(pos)
			.show(&ctx, |ui| {
				let _ = hud_blur_active;
				let fill = body_fill;
				let outer_stroke_color = match theme {
					HudTheme::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 40),
					HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 44),
				};
				let outer_stroke = egui::Stroke::new(1.0, outer_stroke_color);
				let shadow = egui::epaint::Shadow {
					offset: [0, 0],
					blur: 10,
					spread: 0,
					color: match theme {
						HudTheme::Dark => Color32::from_rgba_unmultiplied(0, 0, 0, 28),
						HudTheme::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 18),
					},
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
					self.render_loupe(ui, state, hud_blur_active, hud_opaque, theme);
				});
			});

		self.loupe_tile = Some(tile.response.rect);
	}

	fn render_loupe(
		&mut self,
		ui: &mut Ui,
		state: &OverlayState,
		hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		const CELL: f32 = 10.0;

		let mode = state.mode;

		if matches!(mode, OverlayMode::Live) {
			self.render_live_loupe(ui, state, CELL, hud_blur_active, hud_opaque, theme);
		} else if matches!(mode, OverlayMode::Frozen)
			&& (state.frozen_image.is_some() || state.loupe.is_some())
		{
			let Some(monitor) = state.monitor else {
				return;
			};
			let Some(cursor) = state.cursor else {
				return;
			};

			self.render_frozen_loupe(
				ui,
				state,
				monitor,
				cursor,
				CELL,
				hud_blur_active,
				hud_opaque,
				theme,
			);
		}
	}

	fn sync_live_loupe_texture(
		&mut self,
		loupe: Option<&crate::state::LoupeSample>,
	) -> Option<TextureId> {
		let Some(loupe) = loupe else {
			self.live_loupe_texture = None;

			return None;
		};
		let patch_size_px = [loupe.patch.width() as usize, loupe.patch.height() as usize];
		let patch_rgba = loupe.patch.as_raw();

		match self.live_loupe_texture.as_mut() {
			Some(cached) if cached.patch_size_px == patch_size_px => {
				if cached.rgba != *patch_rgba {
					let color_image = ColorImage::from_rgba_unmultiplied(
						[patch_size_px[0], patch_size_px[1]],
						patch_rgba,
					);

					cached.texture.set(color_image, TextureOptions::NEAREST);
					cached.rgba.clone_from(patch_rgba);
				}
			},
			_ => {
				let color_image = ColorImage::from_rgba_unmultiplied(
					[patch_size_px[0], patch_size_px[1]],
					patch_rgba,
				);
				let texture = self.egui_ctx.load_texture(
					String::from("live-loupe-image"),
					color_image,
					TextureOptions::NEAREST,
				);

				self.live_loupe_texture =
					Some(LiveLoupeTexture { texture, patch_size_px, rgba: patch_rgba.clone() });
			},
		}

		self.live_loupe_texture.as_ref().map(|cached| cached.texture.id())
	}

	fn render_live_loupe(
		&mut self,
		ui: &mut Ui,
		state: &OverlayState,
		cell: f32,
		_hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		let fallback_side_px = state.loupe_patch_side_px.max(1);
		let (w, h) = state
			.loupe
			.as_ref()
			.map(|loupe| loupe.patch.dimensions())
			.unwrap_or((fallback_side_px, fallback_side_px));
		let side = hud_helpers::stable_live_loupe_side_points(state, cell);
		let (rect, _) = ui.allocate_exact_size(Vec2::new(side, side), egui::Sense::hover());
		let body_fill = hud_helpers::hud_body_fill_srgba8(theme, hud_opaque);
		let stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
		let placeholder_fill =
			Color32::from_rgba_unmultiplied(body_fill[0], body_fill[1], body_fill[2], 255);
		let image_rect =
			Rect::from_center_size(rect.center(), Vec2::new((w as f32) * cell, (h as f32) * cell));

		if let Some(texture_id) = self.sync_live_loupe_texture(state.loupe.as_ref()) {
			ui.painter().rect_filled(rect, 3.0, placeholder_fill);
			ui.painter().image(
				texture_id,
				image_rect,
				Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
				Color32::WHITE,
			);
		} else {
			ui.painter().rect_filled(rect, 3.0, placeholder_fill);
		}

		ui.painter().rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);

		let center_x = (w / 2) as f32;
		let center_y = (h / 2) as f32;
		let center_min =
			Pos2::new(image_rect.min.x + center_x * cell, image_rect.min.y + center_y * cell);
		let center_rect = Rect::from_min_size(center_min, Vec2::splat(cell));

		ui.painter().rect_stroke(
			center_rect,
			0.0,
			egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
			egui::StrokeKind::Inside,
		);
	}

	#[allow(clippy::too_many_arguments)]
	fn render_frozen_loupe(
		&mut self,
		ui: &mut Ui,
		state: &OverlayState,
		monitor: MonitorRect,
		cursor: GlobalPoint,
		cell: f32,
		hud_blur_active: bool,
		hud_opaque: bool,
		theme: HudTheme,
	) {
		if state.loupe.is_some() {
			self.render_live_loupe(ui, state, cell, hud_blur_active, hud_opaque, theme);

			return;
		}

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
		let started_at = Instant::now();
		let frame = match self.surface.get_current_texture() {
			Ok(frame) => Ok(frame),
			Err(SurfaceError::Outdated | SurfaceError::Lost) => {
				self.reconfigure(gpu);

				self.needs_reconfigure = false;

				self.surface
					.get_current_texture()
					.wrap_err("Surface was lost and could not be reacquired")
			},
			Err(err) => Err(err).wrap_err("Failed to acquire surface texture"),
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
					depth_slice: None,
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

	fn new(
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
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
			slow_op_logger: SlowOperationLogger::default(),
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

	fn sync_egui_theme(&mut self, theme: HudTheme) {
		if self.hud_theme == Some(theme) {
			return;
		}

		match theme {
			HudTheme::Dark => self.egui_ctx.set_visuals(egui::Visuals::dark()),
			HudTheme::Light => self.egui_ctx.set_visuals(egui::Visuals::light()),
		}

		self.hud_theme = Some(theme);
	}

	#[allow(clippy::too_many_arguments)]
	fn draw(
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
		selection_particles: bool,
		selection_flow_stroke_width_px: f32,
		allow_frozen_surface_bg: bool,
		show_frozen_capture_affordance: bool,
		toolbar_state: Option<&mut FrozenToolbarState>,
		toolbar_pointer: Option<FrozenToolbarPointerState>,
	) -> Result<()> {
		self.apply_pending_reconfigure(gpu);

		let theme = hud_helpers::effective_hud_theme(theme_mode, self.window.theme());

		self.sync_egui_theme(theme);

		let (size, pixels_per_point, raw_input) =
			self.prepare_egui_input(gpu, toolbar_pointer, Some(monitor.scale_factor()));
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

		self.sync_or_clear_hud_bg(gpu, state, monitor, hud_cfg)?;

		let hud_shader_blur_active = self.hud_shader_blur_active(state, monitor, hud_cfg);
		let mut selection_flow_cache = mem::take(&mut self.selection_flow_cache);
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
			selection_particles,
			selection_flow_stroke_width_px,
			hud_cfg.needs_frozen_surface_bg,
			show_frozen_capture_affordance,
			&mut selection_flow_cache,
			toolbar_state,
			toolbar_pointer,
		);

		self.selection_flow_cache = selection_flow_cache;
		self.hud_pill = hud_pill;

		if hud_shader_blur_active {
			self.update_hud_blur_uniform(
				gpu,
				size,
				pixels_per_point,
				theme,
				hud_fog_amount,
				hud_milk_amount,
				hud_tint_hue,
			);
		}

		self.sync_egui_textures(gpu, &full_output);

		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
		let screen_descriptor =
			ScreenDescriptor { size_in_pixels: [size.width, size.height], pixels_per_point };
		let frame = self.acquire_frame(gpu)?;
		let draw_frozen_bg = hud_cfg.needs_frozen_surface_bg
			&& state.monitor == Some(monitor)
			&& state.frozen_image.is_some();

		self.render_frame(
			gpu,
			draw_frozen_bg,
			hud_shader_blur_active,
			frame,
			&paint_jobs,
			&screen_descriptor,
		)?;

		Ok(())
	}

	fn trace_frozen_frame_metrics(
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

	fn resolve_hud_draw_config(
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

	fn sync_or_clear_hud_bg(
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

	fn hud_shader_blur_active(
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
	fn draw_loupe_tile_window(
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
		self.apply_pending_reconfigure(gpu);

		let theme = hud_helpers::effective_hud_theme(theme_mode, self.window.theme());

		self.sync_egui_theme(theme);

		let (size, pixels_per_point, raw_input) =
			self.prepare_egui_input(gpu, None, Some(monitor.scale_factor()));

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

		self.sync_or_clear_hud_bg(gpu, state, monitor, hud_cfg)?;

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
		let (full_output, loupe_tile_rect) = self.run_loupe_tile_egui(
			raw_input,
			state,
			theme,
			hud_blur_active,
			hud_opaque,
			body_fill,
		);

		self.loupe_tile = loupe_tile_rect;

		if hud_shader_blur_active {
			self.hud_pill = loupe_tile_rect.map(|rect| HudPillGeometry {
				rect,
				radius_points: LOUPE_TILE_CORNER_RADIUS_POINTS as f32,
			});

			if self.hud_pill.is_some() {
				self.update_hud_blur_uniform(
					gpu,
					size,
					pixels_per_point,
					theme,
					hud_fog_amount,
					hud_milk_amount,
					hud_tint_hue,
				);
			}
		} else {
			self.hud_pill = None;
		}

		self.sync_egui_textures(gpu, &full_output);

		let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
		let screen_descriptor =
			ScreenDescriptor { size_in_pixels: [size.width, size.height], pixels_per_point };
		let frame = self.acquire_frame(gpu)?;

		self.render_frame(
			gpu,
			false,
			hud_shader_blur_active,
			frame,
			&paint_jobs,
			&screen_descriptor,
		)?;

		Ok(())
	}

	fn tinted_hud_body_fill(
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
		let (_, _, base_lightness) =
			hud_helpers::rgb_to_hsl(crate::state::Rgb::new(fill[0], fill[1], fill[2]));
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
	fn run_loupe_tile_egui(
		&mut self,
		raw_input: egui::RawInput,
		state: &OverlayState,
		theme: HudTheme,
		hud_blur_active: bool,
		hud_opaque: bool,
		body_fill: Color32,
	) -> (FullOutput, Option<Rect>) {
		let mut loupe_tile_rect = None;
		let egui_ctx = self.egui_ctx.clone();
		let full_output = egui_ctx.run(raw_input, |ctx| {
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
			let outer_stroke = egui::Stroke::new(1.0, outer_stroke_color);
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

			egui::Area::new(egui::Id::new("rsnap-loupe-window"))
				.order(egui::Order::Foreground)
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
					let inner_stroke = egui::Stroke::new(1.0, inner_stroke_color);
					let inner_rect = tile_rect.shrink(1.0);

					ui.painter().rect_stroke(
						inner_rect,
						CornerRadius::same(tile_radius.saturating_sub(1)),
						inner_stroke,
						egui::StrokeKind::Inside,
					);
				});
		});

		(full_output, loupe_tile_rect)
	}

	#[allow(clippy::too_many_arguments)]
	fn update_hud_blur_uniform(
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

		let rect_min_px =
			[hud_pill.rect.min.x * pixels_per_point, hud_pill.rect.min.y * pixels_per_point];
		let rect_size_px =
			[hud_pill.rect.width() * pixels_per_point, hud_pill.rect.height() * pixels_per_point];
		let rect_min_size = [rect_min_px[0], rect_min_px[1], rect_size_px[0], rect_size_px[1]];
		let max_lod = self.hud_bg.as_ref().map(|bg| bg.max_lod).unwrap_or(0.0);
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

	fn render_frozen_bg_to_texture(
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
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8UnormSrgb,
			usage: wgpu::TextureUsages::TEXTURE_BINDING
				| wgpu::TextureUsages::COPY_DST
				| wgpu::TextureUsages::RENDER_ATTACHMENT,
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
		self.generate_mipmaps(gpu, &texture, mip_level_count);

		let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
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
		let mipgen_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("rsnap-mipgen fullscreen bind group"),
			layout: &self.mipgen_bind_group_layout,
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

struct HudBg {
	_texture: wgpu::Texture,
	_view: wgpu::TextureView,
	hud_blur_bind_group: BindGroup,
	mipgen_bind_group: BindGroup,
	max_lod: f32,
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
	effects: [f32; 4],
}
impl HudBlurUniformRaw {
	fn as_bytes(&self) -> &[u8] {
		unsafe { slice::from_raw_parts(ptr::from_ref(self).cast::<u8>(), mem::size_of::<Self>()) }
	}
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct MacOSCGPoint {
	x: f64,
	y: f64,
}

#[cfg(target_os = "macos")]
fn macos_is_option_key_down() -> bool {
	let flags = unsafe { CGEventSourceFlagsState(macos_hid_event_source_state_id()) };

	flags & KCG_EVENT_FLAGS_MASK_ALTERNATE != 0
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
	fn CGEventSourceFlagsState(source_state_id: u32) -> u64;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	fn CFRelease(obj: CFTypeRef);
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
fn macos_make_window_key(window: &winit::window::Window) {
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
) -> Result<()> {
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
fn macos_configure_overlay_window_mouse_moved_events(window: &winit::window::Window) {
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

		let _: () = objc::msg_send![ns_window, setOpaque: false];
		let _: () = objc::msg_send![ns_window, setHasShadow: false];
		let sharing_type_none = 0_u64;
		let _: () = objc::msg_send![ns_window, setSharingType: sharing_type_none];
		let clear: *mut Object = objc::msg_send![objc::class!(NSColor), clearColor];
		let _: () = objc::msg_send![ns_window, setBackgroundColor: clear];
		let _: () = objc::msg_send![ns_window, setLevel: MACOS_OVERLAY_WINDOW_LEVEL];
		let _: () = objc::msg_send![ns_window, setAcceptsMouseMovedEvents: YES];
	}
}

#[cfg(target_os = "macos")]
fn macos_configure_hud_window(
	window: &winit::window::Window,
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
		let sharing_type_none = 0_u64;
		let _: () = objc::msg_send![ns_window, setSharingType: sharing_type_none];
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

#[cfg(test)]
mod tests {
	#[cfg(target_os = "macos")]
	use std::sync::Arc;
	#[cfg(target_os = "macos")]
	use std::time::Duration;
	use std::time::Instant;

	use image::{Rgba, RgbaImage};
	#[cfg(target_os = "macos")]
	use winit::dpi::PhysicalPosition;
	use winit::event::MouseScrollDelta;

	#[cfg(target_os = "macos")]
	use crate::live_frame_stream_macos::MacLiveFrameStream;
	#[cfg(not(target_os = "macos"))]
	use crate::overlay::FrozenCaptureSource;
	use crate::overlay::{
		FrozenToolbarState, FrozenToolbarTool, HudTheme, OverlaySession, Pos2, Rect,
		TOOLBAR_CAPTURE_GAP_PX, TOOLBAR_SCREEN_MARGIN_PX, ToolbarPlacement, Vec2, WindowRenderer,
		hud_helpers,
	};
	#[cfg(target_os = "macos")]
	use crate::overlay::{
		HUD_PILL_CORNER_RADIUS_POINTS, HudPillGeometry, InflightScrollCaptureObservation,
		KCG_SCROLL_EVENT_UNIT_PIXEL, LiveSampleApplyResult, LiveStreamStaleGrace,
		MacOSScrollPixelResidual, SCROLL_CAPTURE_INPUT_FRESHNESS,
		SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES, SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE,
		ScrollCaptureFrameSource,
	};
	use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome, ScrollSession};
	#[cfg(target_os = "macos")]
	use crate::state::LiveCursorSample;
	use crate::state::{
		GlobalPoint, LoupeSample, MonitorRect, MonitorRectPoints, OverlayMode, RectPoints, Rgb,
	};

	fn make_scroll_capture_test_image(width: u32, rows: &[[u8; 4]]) -> image::RgbaImage {
		let mut image = image::RgbaImage::new(width, rows.len() as u32);

		for (y, row) in rows.iter().enumerate() {
			for x in 0..width {
				image.put_pixel(x, y as u32, Rgba(*row));
			}
		}

		image
	}

	fn make_scroll_capture_window(
		document: &[[u8; 4]],
		width: u32,
		start_row: usize,
		window_rows: usize,
	) -> image::RgbaImage {
		make_scroll_capture_test_image(width, &document[start_row..start_row + window_rows])
	}

	fn set_scroll_capture_input(session: &mut OverlaySession, direction: ScrollDirection) {
		session.scroll_capture.input_direction = Some(direction);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;
	}

	fn observe_scroll_capture_frame(
		session: &mut OverlaySession,
		frame: image::RgbaImage,
	) -> Option<ScrollObserveOutcome> {
		session.observe_scroll_capture_frame(frame).transpose().unwrap()
	}

	fn scroll_capture_export_height(session: &OverlaySession) -> u32 {
		session.scroll_capture.session.as_ref().unwrap().export_image().height()
	}

	#[test]
	fn frozen_toolbar_default_position_fits_below_capture_rect() {
		let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
		let capture_rect = Rect::from_min_size(Pos2::new(50.0, 100.0), Vec2::new(300.0, 200.0));
		let toolbar_size = Vec2::new(460.0, 54.0);
		let pos = WindowRenderer::frozen_toolbar_default_pos(
			monitor,
			capture_rect,
			toolbar_size,
			ToolbarPlacement::Bottom,
		);
		let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
			TOOLBAR_SCREEN_MARGIN_PX,
			(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX)
				.max(TOOLBAR_SCREEN_MARGIN_PX),
		);

		assert!((pos.x - expected_x).abs() < f32::EPSILON);
		assert_eq!(pos.y, capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX);
	}

	#[test]
	fn frozen_toolbar_default_position_falls_inside_when_no_space_below_capture_rect() {
		let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 600.0));
		let toolbar_size = Vec2::new(460.0, 54.0);
		let capture_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 560.0));
		let pos = WindowRenderer::frozen_toolbar_default_pos(
			monitor,
			capture_rect,
			toolbar_size,
			ToolbarPlacement::Bottom,
		);
		let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
			TOOLBAR_SCREEN_MARGIN_PX,
			(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX)
				.max(TOOLBAR_SCREEN_MARGIN_PX),
		);
		let expected_y = capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y;

		assert_eq!(pos.x, expected_x);
		assert_eq!(pos.y, capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y);
		assert_eq!(pos.y, expected_y);
	}

	#[test]
	fn frozen_toolbar_top_default_position_fits_above_capture_rect() {
		let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
		let capture_rect = Rect::from_min_size(Pos2::new(50.0, 180.0), Vec2::new(300.0, 200.0));
		let toolbar_size = Vec2::new(460.0, 54.0);
		let pos = WindowRenderer::frozen_toolbar_default_pos(
			monitor,
			capture_rect,
			toolbar_size,
			ToolbarPlacement::Top,
		);
		let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
			TOOLBAR_SCREEN_MARGIN_PX,
			(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX)
				.max(TOOLBAR_SCREEN_MARGIN_PX),
		);

		assert_eq!(pos.x, expected_x);
		assert_eq!(pos.y, capture_rect.min.y - TOOLBAR_CAPTURE_GAP_PX - toolbar_size.y);
	}

	#[test]
	fn frozen_toolbar_top_default_position_falls_inside_when_no_space_above_capture_rect() {
		let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 600.0));
		let capture_rect = Rect::from_min_size(Pos2::new(0.0, 20.0), Vec2::new(500.0, 400.0));
		let toolbar_size = Vec2::new(460.0, 54.0);
		let pos = WindowRenderer::frozen_toolbar_default_pos(
			monitor,
			capture_rect,
			toolbar_size,
			ToolbarPlacement::Top,
		);
		let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
			TOOLBAR_SCREEN_MARGIN_PX,
			(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX)
				.max(TOOLBAR_SCREEN_MARGIN_PX),
		);

		assert_eq!(pos.x, expected_x);
		assert_eq!(pos.y, capture_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX);
	}

	#[test]
	fn scroll_toolbar_compacts_to_two_buttons() {
		let frozen_toolbar_size =
			WindowRenderer::frozen_toolbar_size(&FrozenToolbarState::default());
		let scroll_toolbar_size = WindowRenderer::frozen_toolbar_size(&FrozenToolbarState {
			scroll_capture_active: true,
			..FrozenToolbarState::default()
		});

		assert!(scroll_toolbar_size.x < frozen_toolbar_size.x);
		assert_eq!(scroll_toolbar_size.y, frozen_toolbar_size.y);
	}

	#[test]
	fn scroll_preview_prefers_right_side_when_space_exists() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_400,
			height: 900,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.state.frozen_capture_rect = Some(RectPoints::new(120, 160, 400, 320));

		let preview = session.scroll_preview_local_rect(monitor);

		assert_eq!(preview.min.y, 160.0);
		assert_eq!(preview.height(), 320.0);
		assert!(preview.min.x >= 120.0 + 400.0);
	}

	#[test]
	fn scroll_preview_falls_back_to_left_when_right_side_is_tight() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 900,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.state.frozen_capture_rect = Some(RectPoints::new(760, 180, 200, 260));

		let preview = session.scroll_preview_local_rect(monitor);

		assert_eq!(preview.min.y, 180.0);
		assert_eq!(preview.height(), 260.0);
		assert!(preview.max.x <= 760.0);
	}

	#[cfg(not(target_os = "macos"))]
	#[test]
	fn scroll_capture_is_unavailable_on_non_macos_even_with_drag_selection() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.state.mode = OverlayMode::Frozen;
		session.state.monitor = Some(monitor);
		session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 200, 240));
		session.frozen_capture_source = FrozenCaptureSource::DragRegion;

		assert!(!session.scroll_capture_is_available());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn reset_for_start_preserves_external_scroll_input_drain_reader() {
		let mut session = OverlaySession::default();

		session.set_external_scroll_input_drain_reader(Arc::new(|_, _| {
			vec![(1, Instant::now(), 10.0, 20.0, -4.0, true, false)]
		}));
		session.reset_for_start();

		assert!(session.scroll_capture.external_scroll_input_drain_reader.is_some());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn apply_live_cursor_sample_updates_rgb_and_loupe_state() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let cursor = GlobalPoint::new(120, 180);
		let patch = image::RgbaImage::from_pixel(3, 3, Rgba([10, 20, 30, 255]));
		let mut session = OverlaySession::new();

		session.cursor_monitor = Some(monitor);
		session.state.cursor = Some(cursor);
		session.state.alt_held = true;

		assert!(
			session
				.apply_live_cursor_sample_detail(
					monitor,
					cursor,
					LiveCursorSample {
						rgb: Some(Rgb::new(10, 20, 30)),
						patch: Some(patch.clone()),
					},
				)
				.any_changed()
		);
		assert_eq!(session.state.rgb, Some(Rgb::new(10, 20, 30)));
		assert_eq!(session.state.loupe.as_ref().map(|loupe| loupe.center), Some(cursor));
		assert_eq!(
			session.state.loupe.as_ref().map(|loupe| loupe.patch.dimensions()),
			Some(patch.dimensions())
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn apply_live_cursor_sample_detail_keeps_overlay_redraw_narrow_for_rgb_and_loupe_updates() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let cursor = GlobalPoint::new(120, 180);
		let patch = image::RgbaImage::from_pixel(3, 3, Rgba([10, 20, 30, 255]));
		let mut session = OverlaySession::new();

		session.cursor_monitor = Some(monitor);
		session.state.cursor = Some(cursor);
		session.state.alt_held = true;

		let apply = session.apply_live_cursor_sample_detail(
			monitor,
			cursor,
			LiveCursorSample { rgb: Some(Rgb::new(10, 20, 30)), patch: Some(patch) },
		);

		assert_eq!(
			apply,
			LiveSampleApplyResult {
				overlay_changed: false,
				hud_changed: true,
				loupe_changed: true,
			}
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn live_sample_request_redraw_intent_only_redraws_immediate_hover_changes() {
		let session = OverlaySession::new();

		assert_eq!(
			session.live_sample_request_redraw_intent(false, true, true),
			LiveSampleApplyResult::default()
		);
		assert_eq!(
			session.live_sample_request_redraw_intent(true, true, true),
			LiveSampleApplyResult {
				overlay_changed: true,
				hud_changed: true,
				loupe_changed: false,
			}
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn live_loupe_renders_in_hud_window_only_for_live_alt() {
		let mut session = OverlaySession::new();

		session.state.mode = OverlayMode::Frozen;

		assert!(!session.live_loupe_uses_hud_window());
		assert!(!session.live_loupe_renders_in_hud_window());

		session.state.mode = OverlayMode::Live;

		assert!(session.live_loupe_uses_hud_window());
		assert!(!session.live_loupe_renders_in_hud_window());

		session.state.alt_held = true;

		assert!(session.live_loupe_renders_in_hud_window());

		session.state.mode = OverlayMode::Frozen;

		assert!(!session.live_loupe_uses_hud_window());
		assert!(!session.live_loupe_renders_in_hud_window());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn hud_window_content_rect_includes_live_loupe_tile_bounds() {
		let hud_pill = HudPillGeometry {
			rect: Rect::from_min_max(Pos2::new(14.0, 14.0), Pos2::new(200.0, 58.0)),
			radius_points: f32::from(HUD_PILL_CORNER_RADIUS_POINTS),
		};
		let loupe_tile = Rect::from_min_max(Pos2::new(14.0, 68.0), Pos2::new(246.0, 300.0));
		let live_rect = OverlaySession::hud_window_content_rect(
			OverlayMode::Live,
			true,
			hud_pill,
			Some(loupe_tile),
		);

		assert_eq!(live_rect.min, Pos2::new(14.0, 14.0));
		assert_eq!(live_rect.max, Pos2::new(246.0, 300.0));

		let frozen_rect = OverlaySession::hud_window_content_rect(
			OverlayMode::Frozen,
			true,
			hud_pill,
			Some(loupe_tile),
		);

		assert_eq!(frozen_rect, hud_pill.rect);
	}

	#[test]
	fn live_overlay_selection_flow_repaint_active_requires_drag_rect() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.state.mode = OverlayMode::Live;
		session.cursor_monitor = Some(monitor);
		session.state.cursor = Some(GlobalPoint::new(120, 180));

		assert!(!session.live_overlay_selection_flow_repaint_active());

		session.state.hovered_window_rect = Some(MonitorRectPoints {
			monitor_id: monitor.id,
			rect: RectPoints::new(100, 120, 240, 320),
		});

		assert!(!session.live_overlay_selection_flow_repaint_active());

		session.state.drag_rect = Some(MonitorRectPoints {
			monitor_id: monitor.id,
			rect: RectPoints::new(100, 120, 240, 320),
		});

		assert!(session.live_overlay_selection_flow_repaint_active());
	}

	#[test]
	fn live_overlay_redraw_needed_for_cursor_update_only_for_monitor_or_drag_changes() {
		let monitor_a = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let monitor_b = MonitorRect {
			id: 2,
			origin: GlobalPoint::new(1_000, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let drag = Some(MonitorRectPoints {
			monitor_id: monitor_a.id,
			rect: RectPoints::new(100, 120, 240, 320),
		});

		assert!(!OverlaySession::live_overlay_redraw_needed_for_cursor_update(
			Some(monitor_a),
			monitor_a,
			None,
			None,
		));
		assert!(OverlaySession::live_overlay_redraw_needed_for_cursor_update(
			Some(monitor_a),
			monitor_a,
			None,
			drag,
		));
		assert!(OverlaySession::live_overlay_redraw_needed_for_cursor_update(
			Some(monitor_a),
			monitor_b,
			None,
			None,
		));
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn apply_live_cursor_sample_clears_existing_loupe_when_alt_is_released() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let cursor = GlobalPoint::new(120, 180);
		let mut session = OverlaySession::new();

		session.cursor_monitor = Some(monitor);
		session.state.cursor = Some(cursor);
		session.state.alt_held = true;

		let _ = session.apply_live_cursor_sample_detail(
			monitor,
			cursor,
			LiveCursorSample {
				rgb: Some(Rgb::new(10, 20, 30)),
				patch: Some(image::RgbaImage::from_pixel(3, 3, Rgba([10, 20, 30, 255]))),
			},
		);

		session.state.alt_held = false;

		assert!(
			session
				.apply_live_cursor_sample_detail(
					monitor,
					cursor,
					LiveCursorSample { rgb: None, patch: None },
				)
				.any_changed()
		);
		assert!(session.state.loupe.is_none());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn stabilized_live_hud_inner_size_keeps_live_width_from_shrinking() {
		let mut session = OverlaySession::new();

		session.state.mode = OverlayMode::Live;
		session.hud_inner_size_points = Some((826, 44));

		assert_eq!(
			OverlaySession::stabilized_live_hud_inner_size(
				OverlayMode::Live,
				session.hud_inner_size_points,
				(810, 44),
			),
			(826, 44)
		);
		assert_eq!(
			OverlaySession::stabilized_live_hud_inner_size(
				OverlayMode::Live,
				session.hud_inner_size_points,
				(780, 44),
			),
			(826, 44)
		);

		session.state.mode = OverlayMode::Frozen;

		assert_eq!(
			OverlaySession::stabilized_live_hud_inner_size(
				OverlayMode::Frozen,
				session.hud_inner_size_points,
				(810, 44),
			),
			(810, 44)
		);
	}

	#[test]
	fn live_hud_position_text_uses_stable_monitor_width() {
		let monitor = MonitorRect {
			id: 5,
			origin: GlobalPoint::new(0, 0),
			width: 3_008,
			height: 1_692,
			scale_factor_x1000: 2_000,
		};
		let short = hud_helpers::format_live_hud_position_text(monitor, GlobalPoint::new(842, 846));
		let long =
			hud_helpers::format_live_hud_position_text(monitor, GlobalPoint::new(1_504, 1_320));

		assert_eq!(short.len(), long.len());
		assert_eq!(short, "x= 842, y= 846");
		assert_eq!(long, "x=1504, y=1320");
	}

	#[test]
	fn live_hud_rgb_text_uses_fixed_width_placeholders() {
		let (missing_hex, missing_rgb) = hud_helpers::format_live_hud_rgb_text(None);
		let (hex, rgb) = hud_helpers::format_live_hud_rgb_text(Some(Rgb::new(7, 128, 255)));

		assert_eq!(missing_hex.len(), hex.len());
		assert_eq!(missing_rgb.len(), rgb.len());
		assert_eq!(missing_hex, "#??????");
		assert_eq!(missing_rgb, "RGB(???, ???, ???)");
		assert_eq!(rgb, "RGB(  7, 128, 255)");
	}

	#[test]
	fn stable_live_loupe_side_prefers_configured_patch_side() {
		let mut state = crate::state::OverlayState::new();

		state.loupe_patch_side_px = 21;
		state.loupe = Some(LoupeSample {
			center: GlobalPoint::new(100, 120),
			patch: RgbaImage::from_pixel(17, 19, image::Rgba([0, 0, 0, 255])),
		});

		assert_eq!(hud_helpers::stable_live_loupe_side_px(&state), 21);
	}

	#[test]
	fn stable_live_loupe_side_ignores_larger_runtime_patch() {
		let mut state = crate::state::OverlayState::new();

		state.loupe_patch_side_px = 21;
		state.loupe = Some(LoupeSample {
			center: GlobalPoint::new(100, 120),
			patch: RgbaImage::from_pixel(25, 25, image::Rgba([0, 0, 0, 255])),
		});

		assert_eq!(hud_helpers::stable_live_loupe_side_px(&state), 21);
	}

	#[test]
	fn stable_live_loupe_window_inner_size_matches_runtime_target() {
		assert_eq!(hud_helpers::stable_live_loupe_window_inner_size_points(21), (232, 232));
		assert_eq!(hud_helpers::stable_live_loupe_window_inner_size_points(1), (32, 32));
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn drain_external_scroll_input_events_through_advances_last_seen_seq() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let start = Instant::now();
		let events = Arc::new([
			(1, start, 150.0, 160.0, -4.0, true, false),
			(2, start + Duration::from_millis(2), 150.0, 160.0, 4.0, false, true),
		]);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));
		session.set_external_scroll_input_drain_reader(Arc::new({
			let events = Arc::clone(&events);

			move |after_seq, through| {
				events
					.iter()
					.copied()
					.filter(|event| event.0 > after_seq && event.1 <= through)
					.collect()
			}
		}));

		session.drain_external_scroll_input_events_through(start);

		assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
		assert!(session.scroll_capture.input_gesture_active);
		assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);

		session.drain_external_scroll_input_events_through(start);

		assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);

		session.drain_external_scroll_input_events_through(start + Duration::from_millis(2));

		assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Up));
		assert!(!session.scroll_capture.input_gesture_active);
		assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 2);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn drain_external_scroll_input_events_through_uses_pairing_time_for_freshness() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let through = Instant::now();
		let recorded_at = through - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50);
		let events = Arc::new([(1, recorded_at, 150.0, 160.0, -4.0, false, false)]);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));
		session.set_external_scroll_input_drain_reader(Arc::new({
			let events = Arc::clone(&events);

			move |after_seq, paired_through| {
				events
					.iter()
					.copied()
					.filter(|event| event.0 > after_seq && event.1 <= paired_through)
					.collect()
			}
		}));

		session.drain_external_scroll_input_events_through(through);

		assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
		assert_eq!(session.scroll_capture.input_direction_at, Some(through));
		assert_eq!(session.scroll_capture_observation_block_reason(), None);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn replayed_stream_input_uses_frame_time_for_stale_gate_without_global_relaxation() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
		];
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let capture_rect = RectPoints::new(100, 120, 200, 240);
		let through = Instant::now() - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50);
		let recorded_at = through - Duration::from_millis(12);
		let events = Arc::new([(1, recorded_at, 150.0, 160.0, -4.0, false, false)]);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(capture_rect);
		session.scroll_capture.session =
			Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
		session.set_external_scroll_input_drain_reader(Arc::new({
			let events = Arc::clone(&events);

			move |after_seq, paired_through| {
				events
					.iter()
					.copied()
					.filter(|event| event.0 > after_seq && event.1 <= paired_through)
					.collect()
			}
		}));

		session.drain_external_scroll_input_events_through(through);

		assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
		assert_eq!(session.scroll_capture.input_direction_at, Some(through));
		assert_eq!(session.scroll_capture_observation_block_reason(), Some("stale_input"));
		assert_eq!(session.scroll_capture_observation_block_reason_at(through), None);
		assert_eq!(
			session
				.observe_scroll_capture_frame_at(
					make_scroll_capture_window(&document, 3, 1, 5),
					through,
				)
				.transpose()
				.unwrap(),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn replayed_downward_input_allows_bounded_stale_live_stream_frame() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
		];
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let capture_rect = RectPoints::new(100, 120, 200, 240);
		let through = Instant::now();
		let events =
			Arc::new([(7, through - Duration::from_millis(10), 150.0, 160.0, -4.0, false, false)]);
		let stale_at = through + SCROLL_CAPTURE_INPUT_FRESHNESS + Duration::from_millis(1);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(capture_rect);
		session.scroll_capture.session =
			Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
		session.set_external_scroll_input_drain_reader(Arc::new({
			let events = Arc::clone(&events);

			move |after_seq, paired_through| {
				events
					.iter()
					.copied()
					.filter(|event| event.0 > after_seq && event.1 <= paired_through)
					.collect()
			}
		}));

		session.drain_external_scroll_input_events_through(through);

		assert_eq!(
			session.scroll_capture.live_stream_stale_grace,
			Some(LiveStreamStaleGrace {
				external_input_seq: 7,
				input_direction: ScrollDirection::Down,
				remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
			})
		);
		assert_eq!(
			session
				.observe_scroll_capture_frame_at(
					make_scroll_capture_window(&document, 3, 1, 5),
					stale_at,
				)
				.transpose()
				.unwrap(),
			None
		);

		session.handle_scroll_capture_frame(
			make_scroll_capture_window(&document, 3, 1, 5),
			ScrollCaptureFrameSource::LiveStream { frame_seq: 143 },
			false,
			stale_at,
		);

		assert_eq!(scroll_capture_export_height(&session), 6);
		assert_eq!(
			session.scroll_capture.live_stream_stale_grace,
			Some(LiveStreamStaleGrace {
				external_input_seq: 7,
				input_direction: ScrollDirection::Down,
				remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES - 1,
			})
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn stale_live_stream_grace_survives_same_direction_overlay_wheel_update() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
		];
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let capture_rect = RectPoints::new(100, 120, 200, 240);
		let through = Instant::now();
		let wheel_at = through + Duration::from_millis(10);
		let events =
			Arc::new([(7, through - Duration::from_millis(10), 150.0, 160.0, -4.0, false, false)]);
		let stale_at = wheel_at + SCROLL_CAPTURE_INPUT_FRESHNESS + Duration::from_millis(1);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(capture_rect);
		session.scroll_capture.session =
			Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
		session.set_external_scroll_input_drain_reader(Arc::new({
			let events = Arc::clone(&events);

			move |after_seq, paired_through| {
				events
					.iter()
					.copied()
					.filter(|event| event.0 > after_seq && event.1 <= paired_through)
					.collect()
			}
		}));

		session.drain_external_scroll_input_events_through(through);
		session.record_scroll_capture_input_direction_from_overlay_wheel_at(
			&MouseScrollDelta::LineDelta(0.0, -1.0),
			wheel_at,
		);

		assert_eq!(session.scroll_capture.input_direction_at, Some(wheel_at));
		assert_eq!(
			session.scroll_capture.live_stream_stale_grace,
			Some(LiveStreamStaleGrace {
				external_input_seq: 7,
				input_direction: ScrollDirection::Down,
				remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
			})
		);
		assert_eq!(
			session
				.observe_scroll_capture_frame_at(
					make_scroll_capture_window(&document, 3, 1, 5),
					stale_at,
				)
				.transpose()
				.unwrap(),
			None
		);

		session.handle_scroll_capture_frame(
			make_scroll_capture_window(&document, 3, 1, 5),
			ScrollCaptureFrameSource::LiveStream { frame_seq: 143 },
			false,
			stale_at,
		);

		assert_eq!(scroll_capture_export_height(&session), 6);
		assert_eq!(
			session.scroll_capture.live_stream_stale_grace,
			Some(LiveStreamStaleGrace {
				external_input_seq: 7,
				input_direction: ScrollDirection::Down,
				remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES - 1,
			})
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn live_stream_stale_grace_is_consumed_and_superseded() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
		];
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let capture_rect = RectPoints::new(100, 120, 200, 240);
		let stale_at = Instant::now() - Duration::from_millis(1);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(capture_rect);
		session.scroll_capture.session =
			Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
		session.scroll_capture.last_external_scroll_input_seq = 7;
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at =
			Some(stale_at - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(1));
		session.scroll_capture.input_gesture_active = false;
		session.scroll_capture.live_stream_stale_grace = Some(LiveStreamStaleGrace {
			external_input_seq: 7,
			input_direction: ScrollDirection::Down,
			remaining_stale_frames: 1,
		});

		session.handle_scroll_capture_frame(
			make_scroll_capture_window(&document, 3, 1, 5),
			ScrollCaptureFrameSource::LiveStream { frame_seq: 143 },
			false,
			stale_at,
		);

		assert_eq!(scroll_capture_export_height(&session), 6);
		assert_eq!(session.scroll_capture.live_stream_stale_grace, None);

		let height_after_first_stale = scroll_capture_export_height(&session);

		session.handle_scroll_capture_frame(
			make_scroll_capture_window(&document, 3, 2, 5),
			ScrollCaptureFrameSource::LiveStream { frame_seq: 144 },
			false,
			stale_at,
		);

		assert_eq!(scroll_capture_export_height(&session), height_after_first_stale);

		session.scroll_capture.last_external_scroll_input_seq = 8;
		session.scroll_capture.input_direction = Some(ScrollDirection::Up);
		session.scroll_capture.input_direction_at =
			Some(stale_at - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(1));
		session.scroll_capture.input_gesture_active = false;
		session.scroll_capture.live_stream_stale_grace = Some(LiveStreamStaleGrace {
			external_input_seq: 7,
			input_direction: ScrollDirection::Down,
			remaining_stale_frames: 1,
		});

		session.handle_scroll_capture_frame(
			make_scroll_capture_window(&document, 3, 1, 5),
			ScrollCaptureFrameSource::LiveStream { frame_seq: 145 },
			false,
			stale_at,
		);

		assert_eq!(scroll_capture_export_height(&session), height_after_first_stale);
		assert_eq!(session.scroll_capture.live_stream_stale_grace, None);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn wrapped_pixel_delta_normalizes_back_to_signed_values() {
		assert_eq!(OverlaySession::normalize_macos_scroll_pixel_component(4_294_967_294.0), -2.0);
		assert_eq!(OverlaySession::normalize_macos_scroll_pixel_component(4_294_967_290.0), -6.0);
	}

	#[test]
	fn negative_vertical_wheel_delta_maps_to_downward_scroll_capture() {
		assert_eq!(
			OverlaySession::scroll_capture_direction_from_wheel_delta(
				&MouseScrollDelta::LineDelta(0.0, -1.0)
			),
			Some(ScrollDirection::Down)
		);
	}

	#[test]
	fn positive_vertical_wheel_delta_maps_to_upward_scroll_capture() {
		assert_eq!(
			OverlaySession::scroll_capture_direction_from_wheel_delta(
				&MouseScrollDelta::LineDelta(0.0, 1.0)
			),
			Some(ScrollDirection::Up)
		);
	}

	#[test]
	fn external_scroll_input_inside_capture_rect_updates_direction() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));

		session.handle_external_scroll_input_delta_y(150.0, 160.0, -4.0, true, false);

		assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
		assert!(session.scroll_capture.input_direction_at.is_some());
		assert!(session.scroll_capture.input_gesture_active);
	}

	#[test]
	fn external_scroll_input_outside_capture_rect_is_ignored() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));

		session.handle_external_scroll_input_delta_y(50.0, 50.0, -4.0, true, false);

		assert_eq!(session.scroll_capture.input_direction, None);
		assert!(session.scroll_capture.input_direction_at.is_none());
	}

	#[test]
	fn external_scroll_input_terminal_event_preserves_last_direction_for_freshness() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;

		session.handle_external_scroll_input_delta_y(150.0, 160.0, 0.0, false, true);

		assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
		assert!(session.scroll_capture.input_direction_at.is_some());
		assert!(!session.scroll_capture.input_gesture_active);
		assert!(session.scroll_capture_input_allows_growth());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn scroll_overlay_mouse_passthrough_window_arms_and_expires() {
		let now = Instant::now();
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;

		session.arm_scroll_overlay_mouse_passthrough_window(now, "test");

		assert!(session.scroll_capture.overlay_mouse_passthrough_active);
		assert_eq!(
			session.scroll_capture.overlay_mouse_passthrough_until,
			Some(now + SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE)
		);

		session.sync_scroll_overlay_mouse_passthrough_window(
			now + SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE / 2,
		);

		assert!(session.scroll_capture.overlay_mouse_passthrough_active);

		session.sync_scroll_overlay_mouse_passthrough_window(
			now + SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE + Duration::from_millis(1),
		);

		assert!(!session.scroll_capture.overlay_mouse_passthrough_active);
		assert!(session.scroll_capture.overlay_mouse_passthrough_until.is_none());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn external_scroll_input_extends_passthrough_window_inside_capture_rect() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let earlier = Instant::now() - Duration::from_millis(20);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));

		session.arm_scroll_overlay_mouse_passthrough_window(earlier, "test");

		let first_deadline = session.scroll_capture.overlay_mouse_passthrough_until;

		session.handle_external_scroll_input_delta_y(150.0, 160.0, -4.0, true, false);

		assert!(session.scroll_capture.overlay_mouse_passthrough_active);
		assert!(session.scroll_capture.overlay_mouse_passthrough_until > first_deadline);
	}

	#[test]
	fn terminal_downward_scroll_event_sets_direction_before_finishing() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));

		session.handle_external_scroll_input_delta_y(150.0, 160.0, -4.0, false, true);

		assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
		assert!(session.scroll_capture.input_direction_at.is_some());
		assert!(!session.scroll_capture.input_gesture_active);
		assert!(session.scroll_capture_input_allows_growth());
	}

	#[test]
	fn terminal_upward_scroll_event_does_not_allow_growth() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));

		session.handle_external_scroll_input_delta_y(150.0, 160.0, 4.0, false, true);

		assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Up));
		assert!(session.scroll_capture.input_direction_at.is_some());
		assert!(!session.scroll_capture.input_gesture_active);
		assert!(!session.scroll_capture_input_allows_growth());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn overlay_wheel_fallback_records_direction_with_drain_reader_present() {
		let observed_at = Instant::now();
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;

		session.set_external_scroll_input_drain_reader(Arc::new(|_, _| Vec::new()));
		session.record_scroll_capture_input_direction_from_overlay_wheel_at(
			&MouseScrollDelta::LineDelta(0.0, -1.0),
			observed_at,
		);

		assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
		assert_eq!(session.scroll_capture.input_direction_at, Some(observed_at));
		assert!(!session.scroll_capture.input_gesture_active);
	}

	#[test]
	fn missing_scroll_direction_does_not_allow_growth() {
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;

		assert!(!session.scroll_capture_input_allows_growth());
	}

	#[test]
	fn fresh_upward_direction_still_allows_observation() {
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.input_direction = Some(ScrollDirection::Up);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;

		assert!(session.scroll_capture_input_allows_observation());
		assert!(!session.scroll_capture_input_allows_growth());
	}

	#[test]
	fn fresh_downward_direction_allows_growth_without_active_gesture() {
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = false;

		assert!(session.scroll_capture_input_allows_growth());
	}

	#[test]
	fn upward_direction_never_allows_growth() {
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.input_direction = Some(ScrollDirection::Up);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;

		assert!(!session.scroll_capture_input_allows_growth());
	}

	#[test]
	fn upward_rewind_frame_is_observed_before_resume_frontier_growth() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.session =
			Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());

		set_scroll_capture_input(&mut session, ScrollDirection::Down);

		assert_eq!(
			observe_scroll_capture_frame(
				&mut session,
				make_scroll_capture_window(&document, 3, 1, 5),
			),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);
		assert_eq!(
			observe_scroll_capture_frame(
				&mut session,
				make_scroll_capture_window(&document, 3, 2, 5),
			),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);

		let height_after_second_append = scroll_capture_export_height(&session);

		set_scroll_capture_input(&mut session, ScrollDirection::Up);

		assert!(matches!(
			observe_scroll_capture_frame(
				&mut session,
				make_scroll_capture_window(&document, 3, 0, 5),
			),
			Some(
				ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
					| ScrollObserveOutcome::PreviewUpdated
			)
		));
		assert_eq!(scroll_capture_export_height(&session), height_after_second_append);

		set_scroll_capture_input(&mut session, ScrollDirection::Down);

		assert!(matches!(
			observe_scroll_capture_frame(
				&mut session,
				make_scroll_capture_window(&document, 3, 2, 5),
			),
			Some(ScrollObserveOutcome::NoChange) | Some(ScrollObserveOutcome::PreviewUpdated)
		));
		assert_eq!(scroll_capture_export_height(&session), height_after_second_append);

		set_scroll_capture_input(&mut session, ScrollDirection::Up);

		assert!(matches!(
			observe_scroll_capture_frame(
				&mut session,
				make_scroll_capture_window(&document, 3, 1, 5),
			),
			Some(
				ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
					| ScrollObserveOutcome::PreviewUpdated
					| ScrollObserveOutcome::NoChange
			)
		));
		assert_eq!(scroll_capture_export_height(&session), height_after_second_append);

		set_scroll_capture_input(&mut session, ScrollDirection::Down);

		assert!(matches!(
			observe_scroll_capture_frame(
				&mut session,
				make_scroll_capture_window(&document, 3, 2, 5),
			),
			Some(ScrollObserveOutcome::NoChange) | Some(ScrollObserveOutcome::PreviewUpdated)
		));
		assert_eq!(scroll_capture_export_height(&session), height_after_second_append);
		assert_eq!(
			observe_scroll_capture_frame(
				&mut session,
				make_scroll_capture_window(&document, 3, 3, 5),
			),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn stale_latched_worker_input_rewinds_without_ax_position() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let capture_rect = RectPoints::new(100, 120, 200, 240);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(capture_rect);
		session.scroll_capture.session =
			Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;

		assert_eq!(
			session
				.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 1, 5))
				.transpose()
				.unwrap(),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);
		assert_eq!(
			session
				.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 2, 5))
				.transpose()
				.unwrap(),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);

		let height_after_second_append =
			session.scroll_capture.session.as_ref().unwrap().export_image().height();

		session.scroll_capture.input_direction = Some(ScrollDirection::Up);
		session.scroll_capture.input_direction_at =
			Some(Instant::now() - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50));
		session.scroll_capture.input_gesture_active = false;
		session.scroll_capture.last_external_scroll_input_seq = 7;
		session.scroll_capture.inflight_request_id = Some(41);
		session.scroll_capture.inflight_request_observation =
			Some(InflightScrollCaptureObservation {
				input_direction: Some(ScrollDirection::Up),
				was_observable: true,
				external_input_seq: 7,
			});

		session.handle_captured_scroll_region(
			monitor,
			capture_rect,
			41,
			make_scroll_capture_window(&document, 3, 1, 5),
		);

		assert_eq!(session.scroll_capture.inflight_request_id, None);
		assert_eq!(session.scroll_capture.inflight_request_observation, None);

		let scroll_session_debug =
			format!("{:?}", session.scroll_capture.session.as_ref().unwrap());

		assert!(
			scroll_session_debug.contains("resume_frontier_top_y: Some(2)"),
			"{scroll_session_debug}"
		);
		assert!(
			scroll_session_debug.contains("observed_viewport_top_y: 1"),
			"{scroll_session_debug}"
		);
		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().export_image().height(),
			height_after_second_append
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn newer_input_supersedes_latched_worker_observation_context() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let capture_rect = RectPoints::new(100, 120, 200, 240);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(capture_rect);
		session.scroll_capture.session =
			Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;

		assert_eq!(
			session
				.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 1, 5))
				.transpose()
				.unwrap(),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);
		assert_eq!(
			session
				.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 2, 5))
				.transpose()
				.unwrap(),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);

		let height_after_second_append =
			session.scroll_capture.session.as_ref().unwrap().export_image().height();

		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at =
			Some(Instant::now() - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50));
		session.scroll_capture.input_gesture_active = false;
		session.scroll_capture.last_external_scroll_input_seq = 8;
		session.scroll_capture.inflight_request_id = Some(41);
		session.scroll_capture.inflight_request_observation =
			Some(InflightScrollCaptureObservation {
				input_direction: Some(ScrollDirection::Up),
				was_observable: true,
				external_input_seq: 7,
			});

		session.handle_captured_scroll_region(
			monitor,
			capture_rect,
			41,
			make_scroll_capture_window(&document, 3, 1, 5),
		);

		assert_eq!(session.scroll_capture.inflight_request_id, None);
		assert_eq!(session.scroll_capture.inflight_request_observation, None);

		let scroll_session_debug =
			format!("{:?}", session.scroll_capture.session.as_ref().unwrap());

		assert!(scroll_session_debug.contains("resume_frontier_top_y: None"));
		assert!(scroll_session_debug.contains("current_viewport_top_y: 2"));
		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().export_image().height(),
			height_after_second_append
		);

		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;

		assert_eq!(
			session
				.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 3, 5))
				.transpose()
				.unwrap(),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn missing_worker_scroll_frame_clears_inflight_without_mutating_session() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let capture_rect = RectPoints::new(100, 120, 200, 240);
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(capture_rect);
		session.scroll_capture.session =
			Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;
		session.scroll_capture.last_external_scroll_input_seq = 11;
		session.scroll_capture.inflight_request_id = Some(41);
		session.scroll_capture.inflight_request_observation =
			Some(InflightScrollCaptureObservation {
				input_direction: Some(ScrollDirection::Down),
				was_observable: true,
				external_input_seq: 11,
			});

		let scroll_session_before =
			format!("{:?}", session.scroll_capture.session.as_ref().unwrap());

		session.handle_missing_scroll_region(monitor, capture_rect, 41);

		assert_eq!(session.scroll_capture.inflight_request_id, None);
		assert_eq!(session.scroll_capture.inflight_request_observation, None);
		assert_eq!(
			format!("{:?}", session.scroll_capture.session.as_ref().unwrap()),
			scroll_session_before
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn maybe_tick_scroll_capture_stays_on_stream_path_without_worker_fallback() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		};
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor);
		session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));
		session.scroll_capture.live_stream = Some(MacLiveFrameStream::new());

		session.maybe_tick_scroll_capture();

		assert!(!session.scroll_capture.paused);
		assert!(session.state.error_message.is_none());
		assert_eq!(session.scroll_capture.inflight_request_id, None);
	}

	#[test]
	fn upward_input_with_lower_frame_never_appends_growth() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
		];
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.session =
			Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;

		assert_eq!(
			session
				.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 1, 5))
				.transpose()
				.unwrap(),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);

		let height_after_first_append =
			session.scroll_capture.session.as_ref().unwrap().export_image().height();

		session.scroll_capture.input_direction = Some(ScrollDirection::Up);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;

		assert!(matches!(
			session
				.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 2, 5))
				.transpose()
				.unwrap(),
			Some(
				ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
					| ScrollObserveOutcome::PreviewUpdated
					| ScrollObserveOutcome::NoChange
			)
		));
		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().export_image().height(),
			height_after_first_append
		);
		assert!(matches!(
			session
				.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 2, 5))
				.transpose()
				.unwrap(),
			Some(ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange)
		));

		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;

		assert_eq!(
			session
				.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 2, 5))
				.transpose()
				.unwrap(),
			Some(ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: 1,
			})
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn positive_pixel_delta_maps_to_upward_scroll_capture() {
		assert_eq!(
			OverlaySession::scroll_capture_direction_from_wheel_delta(
				&MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 2.0))
			),
			Some(ScrollDirection::Up)
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn macos_scroll_wheel_events_use_hid_system_source_state() {
		assert_eq!(
			super::macos_hid_event_source_state_id(),
			super::KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn pixel_delta_residuals_accumulate_until_whole_pixels_emit() {
		let mut residual = MacOSScrollPixelResidual::default();
		let first = OverlaySession::normalize_macos_scroll_wheel_delta(
			&MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.4, -0.4)),
			&mut residual,
		);
		let second = OverlaySession::normalize_macos_scroll_wheel_delta(
			&MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.7, -0.8)),
			&mut residual,
		);

		assert_eq!(first.units, KCG_SCROLL_EVENT_UNIT_PIXEL);
		assert_eq!(first.posted_x, 0);
		assert_eq!(first.posted_y, 0);
		assert!((first.residual.x - 0.4).abs() < f64::EPSILON);
		assert!((first.residual.y + 0.4).abs() < f64::EPSILON);
		assert_eq!(second.posted_x, 1);
		assert_eq!(second.posted_y, -1);
		assert!((second.residual.x - 0.1).abs() < 1e-9);
		assert!((second.residual.y + 0.2).abs() < 1e-9);
	}

	#[test]
	fn frozen_toolbar_mode_tools_are_identifiable() {
		assert!(FrozenToolbarTool::Pointer.is_mode_tool());
		assert!(FrozenToolbarTool::Pen.is_mode_tool());
		assert!(FrozenToolbarTool::Text.is_mode_tool());
		assert!(FrozenToolbarTool::Mosaic.is_mode_tool());
	}

	#[test]
	fn frozen_toolbar_action_tools_are_not_mode_tools() {
		assert!(!FrozenToolbarTool::Undo.is_mode_tool());
		assert!(!FrozenToolbarTool::Redo.is_mode_tool());
		assert!(!FrozenToolbarTool::Scroll.is_mode_tool());
		assert!(!FrozenToolbarTool::Copy.is_mode_tool());
		assert!(!FrozenToolbarTool::Save.is_mode_tool());
	}

	#[test]
	fn tinted_hud_body_fill_amount_zero_keeps_base_fill() {
		for theme in [HudTheme::Dark, HudTheme::Light] {
			let base_fill = hud_helpers::hud_body_fill_srgba8(theme, false);
			let no_tint =
				WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 0.0, 0.585);

			assert_eq!(no_tint.r(), base_fill[0]);
			assert_eq!(no_tint.g(), base_fill[1]);
			assert_eq!(no_tint.b(), base_fill[2]);
			assert_eq!(no_tint.a(), 255);
		}
	}

	#[test]
	fn tinted_hud_body_fill_100pct_tint_is_visibly_blue() {
		let dark_min_delta: u16 = 57;
		let light_min_delta: u16 = 24;
		let sky_tint = 0.585;

		for theme in [HudTheme::Dark, HudTheme::Light] {
			let base_fill =
				WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 0.0, sky_tint);
			let tinted_fill =
				WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 1.0, sky_tint);
			let rgb_delta = u16::from(base_fill.r()).abs_diff(u16::from(tinted_fill.r()))
				+ u16::from(base_fill.g()).abs_diff(u16::from(tinted_fill.g()))
				+ u16::from(base_fill.b()).abs_diff(u16::from(tinted_fill.b()));
			let min_delta =
				if matches!(theme, HudTheme::Dark) { dark_min_delta } else { light_min_delta };

			assert!(
				rgb_delta >= min_delta,
				"expected minimum tint delta >= {min_delta}, got {rgb_delta}"
			);
		}
	}

	#[test]
	fn tinted_hud_body_fill_preserves_alpha() {
		for theme in [HudTheme::Dark, HudTheme::Light] {
			let tint_hue = 0.585;
			let opaque =
				WindowRenderer::tinted_hud_body_fill(theme, false, true, 0.25, 1.0, tint_hue);
			let translucent =
				WindowRenderer::tinted_hud_body_fill(theme, false, false, 0.33, 1.0, tint_hue);

			assert_eq!(opaque.a(), 255);
			assert_eq!(translucent.a(), (0.33_f32 * 255.0).round().clamp(0.0, 255.0) as u8);
		}
	}

	#[test]
	fn tinted_hud_body_fill_blur_active_enforces_min_opacity() {
		for theme in [HudTheme::Dark, HudTheme::Light] {
			let tint_hue = 0.585;
			let fill = WindowRenderer::tinted_hud_body_fill(theme, true, false, 0.0, 0.0, tint_hue);
			let expected =
				(hud_helpers::hud_blur_tint_alpha(theme) * 255.0).round().clamp(0.0, 255.0) as u8;

			assert_eq!(fill.a(), expected);
		}
	}

	#[test]
	fn frozen_toolbar_clamps_floating_position() {
		let monitor = Rect::from_min_size(Pos2::new(-200.0, -100.0), Vec2::new(500.0, 400.0));
		let toolbar_size = Vec2::new(220.0, 42.0);
		let clamped = WindowRenderer::clamp_toolbar_position(
			monitor,
			toolbar_size,
			Pos2::new(-400.0, -240.0),
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		);

		assert_eq!(clamped.x, monitor.min.x + TOOLBAR_SCREEN_MARGIN_PX);
		assert_eq!(clamped.y, monitor.min.y + TOOLBAR_SCREEN_MARGIN_PX);
	}
}
