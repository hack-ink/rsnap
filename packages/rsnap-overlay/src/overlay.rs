pub(crate) mod output;
pub(crate) mod replay_support;

mod aux_window_runtime;
mod capture_window_runtime;
mod config_runtime;
mod cursor_context_runtime;
mod cursor_runtime;
mod hud_helpers;
mod hud_runtime;
mod image_helpers;
mod rendering;
mod scroll_preview_runtime;
mod scroll_runtime;
mod session_state;
mod toolbar_runtime;
mod trace_recording;
mod window_position_runtime;
mod window_runtime;
mod worker_runtime;

#[cfg(target_os = "macos")]
use std::collections::VecDeque;
#[cfg(not(target_os = "macos"))]
use std::env;
#[cfg(target_os = "macos")]
use std::ffi::c_void;
use std::mem;
use std::panic;
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
#[cfg(not(target_os = "macos"))]
use device_query::DeviceQuery;
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
use image::{
	RgbaImage,
	imageops::{self, FilterType},
};
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

#[cfg(target_os = "macos")]
use self::rendering::StartupLiveRgbPlan;
use self::rendering::{
	GpuContext, HudOverlayWindow, HudPillGeometry, HudRedrawSummary, OverlayWindow,
	ScrollPreviewView, ScrollPreviewWindow, WindowRenderer,
};
#[cfg(test)]
use self::rendering::{
	SelectionDashedBorderCache, SelectionDashedBorderMetrics, SelectionFlowGeometryCache,
	SelectionSizeBadgeTarget,
};
#[cfg(all(target_os = "macos", test))]
use self::session_state::InflightScrollCaptureObservation;
use self::session_state::{
	ActiveFrozenBrushStroke, CursorMoveTrace, FrozenBrushModelState, FrozenBrushState,
	FrozenBrushStroke, FrozenMosaicDragState, FrozenSelectionDragCursorMoveTiming,
	FrozenSelectionDragState, FrozenTextAnnotation, FrozenTextColor, FrozenTextEditState,
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
use crate::live_frame_stream_macos::{CursorSampleRequest, MacLiveFrameStream};
use crate::scroll_capture::{self, ScrollDirection, ScrollObserveOutcome, ScrollSession};
use crate::state::LiveCursorSample;
use crate::text_rendering::{self, RasterTextAnnotation};
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
const TOOLBAR_MAX_TOOL_COUNT: usize = 10;
const LIVE_EVENT_CURSOR_CACHE_TTL: Duration = Duration::from_millis(120);
const CURSOR_EVENT_TICK_TTL: Duration = Duration::from_millis(24);
const LIVE_HOVER_HIT_TEST_INTERVAL: Duration = Duration::from_millis(60);
const LIVE_WINDOW_LIST_REFRESH_INTERVAL: Duration = Duration::from_millis(120);
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
const TOOLBAR_DEFAULT_SLOT_POSITION_EPSILON_POINTS: f32 = 1.0;
const HUD_PILL_CORNER_RADIUS_POINTS: u8 = 18;
const FROZEN_TEXT_FONT_SIZE_POINTS: f32 = 16.0;
const FROZEN_TEXT_FONT_SIZE_PRESETS: [f32; 4] = [16.0, 22.0, 30.0, 40.0];
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
const FROZEN_BRUSH_COLOR_RGBA: [u8; 4] = [255, 69, 58, 255];
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
			Self::Text => "Text",
			Self::Mosaic => "Mosaic",
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
			Self::Text => regular::TEXT_T,
			Self::Mosaic => regular::CHECKERBOARD,
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
		matches!(self, Self::Pointer | Self::Pen | Self::Text | Self::Mosaic)
	}

	const fn requires_final_capture(self) -> bool {
		match self {
			Self::Pointer | Self::Pen | Self::Text | Self::AutoCenter => false,
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
	authoritative_frozen_capture_ready: bool,
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
	frozen_text_edit: Option<FrozenTextEditState>,
	frozen_text_input_generation: u64,
	frozen_text_recent_input: Option<FrozenTextRecentInput>,
	toolbar_state: FrozenToolbarState,
	toolbar_left_button_down: bool,
	toolbar_left_button_went_down: bool,
	toolbar_left_button_went_up: bool,
	toolbar_pointer_local: Option<Pos2>,
	left_mouse_button_down: bool,
	left_mouse_button_down_monitor: Option<MonitorRect>,
	left_mouse_button_down_global: Option<GlobalPoint>,
	frozen_brush: FrozenBrushState,
	frozen_selection_drag: FrozenSelectionDragState,
	frozen_mosaic_drag: FrozenMosaicDragState,
	frozen_edit_undo_stack: Vec<FrozenEditKind>,
	frozen_edit_redo_stack: Vec<FrozenEditKind>,
	frozen_mosaic_undo_stack: Vec<FrozenMosaicEdit>,
	frozen_mosaic_redo_stack: Vec<FrozenMosaicEdit>,
	hud_window_visible: bool,
	toolbar_window_visible: bool,
	skip_toolbar_focus_on_next_show: bool,
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
			toolbar_outer_pos: None, pending_toolbar_outer_pos: None,
			toolbar_inner_size_points: None,
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
			authoritative_frozen_capture_ready: false,
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
			frozen_text_annotations: Vec::new(),
			frozen_text_redo_annotations: Vec::new(),
			frozen_text_edit: None,
			frozen_text_input_generation: 0,
			frozen_text_recent_input: None,
			toolbar_state: FrozenToolbarState::default(),
			toolbar_left_button_down: false, toolbar_left_button_went_down: false, toolbar_left_button_went_up: false,
			toolbar_pointer_local: None,
			left_mouse_button_down: false, left_mouse_button_down_monitor: None, left_mouse_button_down_global: None,
			frozen_brush: FrozenBrushState::default(),
			frozen_selection_drag: FrozenSelectionDragState::default(),
			frozen_mosaic_drag: FrozenMosaicDragState::default(),
			frozen_edit_undo_stack: Vec::new(),
			frozen_edit_redo_stack: Vec::new(),
			frozen_mosaic_undo_stack: Vec::new(),
			frozen_mosaic_redo_stack: Vec::new(),
			hud_window_visible: false, toolbar_window_visible: false,
			skip_toolbar_focus_on_next_show: false, toolbar_window_warmup_redraws_remaining: 0,
			loupe_window_visible: false,
			loupe_window_warmup_redraws_remaining: 0,
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
	/// Registers a wake callback for macOS live-stream frame notifications.
	pub fn set_scroll_frame_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
		self.scroll_frame_waker = Some(waker);
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

	#[cfg(test)]
	fn observe_scroll_capture_frame(
		&mut self,
		frame: RgbaImage,
	) -> Option<Result<ScrollObserveOutcome>> {
		self.observe_scroll_capture_frame_at(frame, Instant::now())
	}

	#[cfg(test)]
	fn observe_scroll_capture_frame_at(
		&mut self,
		frame: RgbaImage,
		observation_at: Instant,
	) -> Option<Result<ScrollObserveOutcome>> {
		self.observe_scroll_capture_frame_with_gate(frame, false, observation_at, false)
	}

	fn observe_scroll_capture_frame_with_gate(
		&mut self,
		frame: RgbaImage,
		allow_stale_input: bool,
		observation_at: Instant,
		allow_post_stall_burst_search: bool,
	) -> Option<Result<ScrollObserveOutcome>> {
		let prior_block_reason = self.scroll_capture_observation_block_reason_at(observation_at);
		#[cfg(target_os = "macos")]
		let consumed_live_stream_stale_grace = !allow_stale_input
			&& prior_block_reason == Some("stale_input")
			&& self.consume_live_stream_stale_grace_if_current();
		#[cfg(not(target_os = "macos"))]
		let consumed_live_stream_stale_grace = false;
		let allow_gate_bypass = allow_stale_input || consumed_live_stream_stale_grace;
		let motion_rows_hint = self.scroll_capture_commit_motion_rows_hint_at(observation_at);

		if !allow_gate_bypass && prior_block_reason.is_some() {
			return Some(Ok(ScrollObserveOutcome::NoChange));
		}

		let result = {
			let Some(session) = self.scroll_capture.session.as_mut() else {
				self.scroll_capture_set_error("Scroll capture session is unavailable.");

				return None;
			};

			session.observe_downward_sample_with_motion_hint_and_burst(
				frame,
				motion_rows_hint,
				allow_post_stall_burst_search,
			)
		};

		if let Ok(outcome) = &result {
			self.consume_scroll_capture_downward_motion_rows_for_outcome(outcome);
		}

		Some(result)
	}

	fn scroll_capture_commit_motion_rows_hint_at(&self, observation_at: Instant) -> Option<u32> {
		if self.scroll_capture.input_direction != Some(ScrollDirection::Down) {
			return None;
		}

		let input_direction_at = self.scroll_capture.input_direction_at?;

		if !self.scroll_capture.input_gesture_active
			&& observation_at.saturating_duration_since(input_direction_at)
				> SCROLL_CAPTURE_INPUT_FRESHNESS
		{
			return None;
		}
		if !self.scroll_capture.downward_motion_rows_pending.is_finite()
			|| self.scroll_capture.downward_motion_rows_pending <= 0.0
		{
			return None;
		}

		Some(self.scroll_capture.downward_motion_rows_pending.ceil() as u32)
	}

	fn scroll_capture_set_error(&mut self, message: impl Into<String>) {
		let message = message.into();

		tracing::warn!(
			op = "scroll_capture.error",
			error = %message,
			"Scroll capture paused on error."
		);

		if let Some(trace_recorder) = self.scroll_capture.trace_recorder.as_mut() {
			trace_recorder.record_error(&message);
		}

		self.scroll_capture.paused = true;

		self.state.set_error(message);
		self.request_redraw_all();
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

	fn should_force_pending_hud_and_loupe_moves(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor.is_some()
			&& !self.frozen_final_capture_ready()
	}

	#[cfg(target_os = "macos")]
	fn try_latest_live_freeze_preview(&mut self, monitor: MonitorRect) -> Option<RgbaImage> {
		if self.state.live_bg_monitor == Some(monitor)
			&& let Some(image) = self.state.live_bg_image.take()
		{
			return Some(image);
		}

		self.live_sample_stream
			.as_ref()
			.and_then(|stream| stream.peek_latest_rgba_snapshot(monitor))
			.map(|snapshot| snapshot.image.as_ref().clone())
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
			self.update_hud_window_position(monitor, cursor);
		}
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
		let toolbar_size = WindowRenderer::frozen_toolbar_size(&self.toolbar_state);

		WindowRenderer::frozen_toolbar_default_pos(
			screen_rect,
			capture_rect,
			toolbar_size,
			self.config.toolbar_placement,
		)
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

	fn refresh_frozen_helper_windows_for_transition(
		&mut self,
		monitor: MonitorRect,
		cursor: Option<GlobalPoint>,
	) {
		if let Some(cursor) = cursor {
			self.update_hud_window_position(monitor, cursor);
		}

		if self.should_force_pending_hud_and_loupe_moves() {
			self.force_apply_pending_hud_and_loupe_moves();
		}

		self.schedule_egui_repaint_after(self.repaint_interval_for_monitor(Some(monitor)));
		self.request_redraw_for_monitor(monitor);
		self.request_redraw_hud_window();

		if self.state.alt_held || self.loupe_window_visible {
			self.request_redraw_loupe_window();
		}
	}

	fn reset_frozen_text_state(&mut self) {
		self.frozen_text_annotations.clear();
		self.frozen_text_redo_annotations.clear();

		self.frozen_text_edit = None;

		self.sync_text_input_ime_state();
	}

	fn reset_frozen_annotation_state(&mut self) {
		self.frozen_brush = FrozenBrushState::default();
		self.frozen_selection_drag = FrozenSelectionDragState::default();
		self.frozen_mosaic_drag = FrozenMosaicDragState::default();

		self.frozen_edit_undo_stack.clear();
		self.frozen_edit_redo_stack.clear();
		self.frozen_mosaic_undo_stack.clear();
		self.frozen_mosaic_redo_stack.clear();
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
		let frozen_rgb = self.state.rgb;
		let frozen_loupe = self.state.loupe.as_ref().map(|loupe| crate::state::LoupeSample {
			center: loupe.center,
			patch: loupe.patch.clone(),
		});

		self.state.clear_error();
		self.state.begin_freeze(monitor);

		self.state.frozen_capture_rect = Some(capture_rect);
		self.state.frozen_mosaic_preview_rect = None;
		self.state.drag_rect = None;
		self.state.hovered_window_rect = None;

		self.reset_frozen_annotation_state();

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

		self.state.rgb = frozen_rgb;
		self.state.loupe = frozen_loupe;
		self.pending_freeze_capture = Some(monitor);
		self.pending_freeze_capture_armed = false;
		self.inflight_freeze_capture = None;
		self.authoritative_frozen_capture_ready = false;
		self.pending_window_freeze_capture = window_target;
		self.inflight_window_freeze_capture = None;
		self.frozen_window_image = None;
		self.capture_windows_hidden = false;
		self.pending_click_hit_test_request_id = None;
		self.left_mouse_button_down = false;
		self.left_mouse_button_down_monitor = None;
		self.left_mouse_button_down_global = None;

		self.refresh_frozen_helper_windows_for_transition(monitor, cursor);

		#[cfg(target_os = "macos")]
		{
			if let Some(image) = self.try_latest_live_freeze_preview(monitor) {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;

				self.commit_frozen_preview(monitor, image, cursor);
				self.force_apply_pending_hud_and_loupe_moves();
			} else {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;
				self.capture_windows_hidden = true;
			}
		}
		#[cfg(not(target_os = "macos"))]
		{
			if self.use_fake_hud_blur()
				&& window_target.is_none()
				&& self.state.live_bg_monitor == Some(monitor)
				&& let Some(image) = self.state.live_bg_image.take()
			{
				self.state.live_bg_monitor = None;

				self.state.finish_freeze(monitor, image);

				self.pending_freeze_capture = None;
				self.pending_freeze_capture_armed = false;
				self.authoritative_frozen_capture_ready = true;

				if let Some(cursor) = cursor {
					self.update_cursor_state(monitor, cursor);
					self.update_hud_window_position(monitor, cursor);
				}

				if self.should_force_pending_hud_and_loupe_moves() {
					self.force_apply_pending_hud_and_loupe_moves();
				}
			} else {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;
				self.capture_windows_hidden = true;

				self.hide_capture_windows();
			}
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

	fn frozen_capture_rect_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.frozen_capture_source != FrozenCaptureSource::DragRegion
			|| self.scroll_capture.active
			|| self.state.frozen_image.is_none()
		{
			return None;
		}

		let monitor = self.state.monitor?;
		let capture_rect = self.state.frozen_capture_rect?;

		if capture_rect.is_empty() {
			return None;
		}

		Some((monitor, capture_rect))
	}

	fn frozen_selection_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		(self.toolbar_state.selected_tool == FrozenToolbarTool::Pointer)
			.then(|| self.frozen_capture_rect_drag_target())
			.flatten()
	}

	fn frozen_capture_rect_for_monitor(&self, monitor: MonitorRect) -> Option<RectPoints> {
		self.state
			.frozen_capture_rect
			.or_else(|| Some(RectPoints::new(0, 0, monitor.width, monitor.height)))
			.filter(|capture_rect| !capture_rect.is_empty())
	}

	fn frozen_brush_capture_target(&self) -> Option<(MonitorRect, RectPoints)> {
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.scroll_capture.active
			|| self.state.frozen_image.is_none()
		{
			return None;
		}

		let monitor = self.state.monitor?;
		let capture_rect = self.frozen_capture_rect_for_monitor(monitor)?;

		Some((monitor, capture_rect))
	}

	fn frozen_auto_center_available(&self) -> bool {
		self.frozen_capture_rect_drag_target().is_some()
	}

	fn frozen_mosaic_drag_target(&self) -> Option<(MonitorRect, RectPoints)> {
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.scroll_capture.active
			|| self.state.frozen_image.is_none()
			|| !self.frozen_final_capture_ready()
			|| self.toolbar_state.selected_tool != FrozenToolbarTool::Mosaic
		{
			return None;
		}

		let monitor = self.state.monitor?;
		let capture_rect = self.state.frozen_capture_rect?;

		if capture_rect.is_empty() {
			return None;
		}

		Some((monitor, capture_rect))
	}

	fn begin_frozen_selection_drag(&mut self, global: GlobalPoint) -> bool {
		let Some((monitor, capture_rect)) = self.frozen_selection_drag_target() else {
			return false;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(global) else {
			return false;
		};
		let Some(interaction) =
			Self::frozen_selection_interaction_kind(capture_rect, cursor_x, cursor_y)
		else {
			return false;
		};

		self.frozen_selection_drag = FrozenSelectionDragState {
			active: true,
			interaction,
			anchor_rect: capture_rect,
			pointer_offset_x: cursor_x.saturating_sub(capture_rect.x),
			pointer_offset_y: cursor_y.saturating_sub(capture_rect.y),
			press_cursor_x: cursor_x,
			press_cursor_y: cursor_y,
		};

		self.hide_auxiliary_windows_for_frozen_selection_drag();

		true
	}

	fn begin_frozen_mosaic_drag(&mut self, global: GlobalPoint) -> bool {
		let Some((monitor, capture_rect)) = self.frozen_mosaic_drag_target() else {
			return false;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(global) else {
			return false;
		};

		if !capture_rect.contains((cursor_x, cursor_y)) {
			return false;
		}

		self.frozen_mosaic_drag =
			FrozenMosaicDragState { active: true, anchor_x: cursor_x, anchor_y: cursor_y };
		self.state.frozen_mosaic_preview_rect = Some(RectPoints::new(cursor_x, cursor_y, 1, 1));

		true
	}

	fn frozen_selection_interaction_kind(
		capture_rect: RectPoints,
		cursor_x: u32,
		cursor_y: u32,
	) -> Option<FrozenSelectionInteractionKind> {
		if let Some(corner) = WindowRenderer::frozen_selection_resize_hit_test(
			capture_rect,
			Pos2::new(cursor_x as f32, cursor_y as f32),
		) {
			return Some(FrozenSelectionInteractionKind::Resize(corner));
		}

		capture_rect.contains((cursor_x, cursor_y)).then_some(FrozenSelectionInteractionKind::Move)
	}

	fn frozen_selection_resize_cursor_icon(corner: FrozenSelectionCorner) -> CursorIcon {
		match corner {
			FrozenSelectionCorner::TopLeft | FrozenSelectionCorner::BottomRight => {
				CursorIcon::NwseResize
			},
			FrozenSelectionCorner::TopRight | FrozenSelectionCorner::BottomLeft => {
				CursorIcon::NeswResize
			},
		}
	}

	#[cfg_attr(target_os = "macos", allow(dead_code))]
	fn frozen_selection_cursor_icon_for_monitor(&self, monitor: MonitorRect) -> CursorIcon {
		if let Some((target_monitor, capture_rect)) = self.frozen_mosaic_drag_target() {
			if target_monitor != monitor {
				return CursorIcon::Default;
			}
			if self.frozen_mosaic_drag.active {
				return CursorIcon::Crosshair;
			}

			let Some(cursor) = self.state.cursor else {
				return CursorIcon::Default;
			};
			let Some((cursor_x, cursor_y)) = monitor.local_u32(cursor) else {
				return CursorIcon::Default;
			};

			return if capture_rect.contains((cursor_x, cursor_y)) {
				CursorIcon::Crosshair
			} else {
				CursorIcon::Default
			};
		}

		if self.toolbar_state.selected_tool == FrozenToolbarTool::Pen
			&& let Some((target_monitor, capture_rect)) = self.frozen_brush_capture_target()
		{
			if target_monitor != monitor {
				return CursorIcon::Default;
			}
			if self.frozen_brush.active_stroke.is_some() {
				return CursorIcon::Crosshair;
			}

			let Some(cursor) = self.state.cursor else {
				return CursorIcon::Default;
			};
			let Some((cursor_x, cursor_y)) = monitor.local_u32(cursor) else {
				return CursorIcon::Default;
			};

			return if capture_rect.contains((cursor_x, cursor_y)) {
				CursorIcon::Crosshair
			} else {
				CursorIcon::Default
			};
		}

		let Some((target_monitor, capture_rect)) = self.frozen_selection_drag_target() else {
			return CursorIcon::Default;
		};

		if target_monitor != monitor {
			return CursorIcon::Default;
		}
		if self.frozen_selection_drag.active {
			return match self.frozen_selection_drag.interaction {
				FrozenSelectionInteractionKind::Resize(corner) => {
					Self::frozen_selection_resize_cursor_icon(corner)
				},
				FrozenSelectionInteractionKind::Move => CursorIcon::Grabbing,
			};
		}

		let Some(cursor) = self.state.cursor else {
			return CursorIcon::Default;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(cursor) else {
			return CursorIcon::Default;
		};
		let cursor_local = Pos2::new(cursor_x as f32, cursor_y as f32);

		if let Some(hit_rect) = self.frozen_text_edit_hit_rect_for_monitor(monitor)
			&& hit_rect.contains(cursor_local)
		{
			return if self.frozen_text_edit.as_ref().is_some_and(|edit| edit.dragging) {
				CursorIcon::Grabbing
			} else {
				CursorIcon::Grab
			};
		}

		if self.frozen_text_tool_active() && capture_rect.contains((cursor_x, cursor_y)) {
			return CursorIcon::Text;
		}

		match Self::frozen_selection_interaction_kind(capture_rect, cursor_x, cursor_y) {
			Some(FrozenSelectionInteractionKind::Resize(corner)) => {
				Self::frozen_selection_resize_cursor_icon(corner)
			},
			Some(FrozenSelectionInteractionKind::Move) => CursorIcon::Grab,
			None => CursorIcon::Default,
		}
	}

	#[cfg_attr(target_os = "macos", allow(dead_code))]
	fn overlay_cursor_icon_for_monitor(&self, monitor: MonitorRect) -> CursorIcon {
		match self.state.mode {
			OverlayMode::Frozen => self.frozen_selection_cursor_icon_for_monitor(monitor),
			OverlayMode::Live => CursorIcon::Default,
		}
	}

	#[cfg(target_os = "macos")]
	fn frozen_selection_cursor_rects_for_monitor(
		&self,
		monitor: MonitorRect,
	) -> Vec<OverlayCursorRect> {
		if !matches!(self.state.mode, OverlayMode::Frozen) {
			return Vec::new();
		}

		let overlay_bounds =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));

		if let Some((target_monitor, capture_rect)) = self.frozen_mosaic_drag_target() {
			if target_monitor != monitor {
				return Vec::new();
			}
			if self.frozen_mosaic_drag.active {
				return vec![OverlayCursorRect::new(overlay_bounds, CursorIcon::Crosshair)];
			}

			let capture_rect = Rect::from_min_size(
				Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
				Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
			)
			.intersect(overlay_bounds);

			return (capture_rect.width() > 0.0 && capture_rect.height() > 0.0)
				.then_some(vec![OverlayCursorRect::new(capture_rect, CursorIcon::Crosshair)])
				.unwrap_or_default();
		}

		let Some((target_monitor, capture_rect)) = self.frozen_selection_drag_target() else {
			return Vec::new();
		};

		if target_monitor != monitor {
			return Vec::new();
		}
		if self.frozen_selection_drag.active {
			return match self.frozen_selection_drag.interaction {
				FrozenSelectionInteractionKind::Resize(corner) => vec![OverlayCursorRect::new(
					overlay_bounds,
					Self::frozen_selection_resize_cursor_icon(corner),
				)],
				FrozenSelectionInteractionKind::Move => {
					vec![OverlayCursorRect::new(overlay_bounds, CursorIcon::Grabbing)]
				},
			};
		}

		Self::frozen_selection_hover_cursor_rects(capture_rect)
			.into_iter()
			.filter_map(|cursor_rect| {
				let rect = cursor_rect.rect.intersect(overlay_bounds);

				(rect.width() > 0.0 && rect.height() > 0.0)
					.then_some(OverlayCursorRect::new(rect, cursor_rect.icon))
			})
			.collect()
	}

	#[cfg(target_os = "macos")]
	fn frozen_selection_hover_cursor_rects(capture_rect: RectPoints) -> Vec<OverlayCursorRect> {
		let selection_rect = Rect::from_min_size(
			Pos2::new(capture_rect.x as f32, capture_rect.y as f32),
			Vec2::new(capture_rect.width as f32, capture_rect.height as f32),
		);
		let interior_reach_x = (selection_rect.width() * 0.35)
			.min(FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS);
		let interior_reach_y = (selection_rect.height() * 0.35)
			.min(FROZEN_SELECTION_RESIZE_HANDLE_INTERIOR_REACH_MAX_POINTS);
		let mut x_edges = vec![
			selection_rect.min.x,
			selection_rect.min.x + interior_reach_x,
			selection_rect.center().x,
			selection_rect.max.x - interior_reach_x,
			selection_rect.max.x,
		];
		let mut y_edges = vec![
			selection_rect.min.y,
			selection_rect.min.y + interior_reach_y,
			selection_rect.center().y,
			selection_rect.max.y - interior_reach_y,
			selection_rect.max.y,
		];

		for handle in WindowRenderer::frozen_selection_resize_handles(capture_rect) {
			x_edges.push(handle.hit_rect.min.x);
			x_edges.push(handle.hit_rect.max.x);
			y_edges.push(handle.hit_rect.min.y);
			y_edges.push(handle.hit_rect.max.y);
		}

		sort_unique_axis_values(&mut x_edges);
		sort_unique_axis_values(&mut y_edges);

		let mut rects = Vec::new();

		for x_pair in x_edges.windows(2) {
			let [min_x, max_x] = [x_pair[0], x_pair[1]];

			if max_x <= min_x {
				continue;
			}

			for y_pair in y_edges.windows(2) {
				let [min_y, max_y] = [y_pair[0], y_pair[1]];

				if max_y <= min_y {
					continue;
				}

				let rect = Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y));
				let point = rect.center();
				let Some(interaction) = Self::frozen_selection_interaction_kind(
					capture_rect,
					point.x as u32,
					point.y as u32,
				) else {
					continue;
				};
				let FrozenSelectionInteractionKind::Resize(corner) = interaction else {
					rects.push(OverlayCursorRect::new(rect, CursorIcon::Grab));

					continue;
				};
				let icon = Self::frozen_selection_resize_cursor_icon(corner);
				let mut adjusted_min = rect.min;
				let mut adjusted_max = rect.max;

				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(rect.min.x, point.y),
				) != Some(corner)
				{
					adjusted_min.x = trim_rect_min_edge(adjusted_min.x, adjusted_max.x);
				}
				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(rect.max.x, point.y),
				) != Some(corner)
				{
					adjusted_max.x = trim_rect_max_edge(adjusted_max.x, adjusted_min.x);
				}
				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(point.x, rect.min.y),
				) != Some(corner)
				{
					adjusted_min.y = trim_rect_min_edge(adjusted_min.y, adjusted_max.y);
				}
				if WindowRenderer::frozen_selection_resize_hit_test(
					capture_rect,
					Pos2::new(point.x, rect.max.y),
				) != Some(corner)
				{
					adjusted_max.y = trim_rect_max_edge(adjusted_max.y, adjusted_min.y);
				}
				if adjusted_max.x <= adjusted_min.x || adjusted_max.y <= adjusted_min.y {
					continue;
				}

				rects.push(OverlayCursorRect::new(
					Rect::from_min_max(adjusted_min, adjusted_max),
					icon,
				));
			}
		}

		rects
	}

	fn sync_overlay_cursor_icons(&self) {
		for overlay_window in self.windows.values() {
			#[cfg(not(target_os = "macos"))]
			let icon = self.overlay_cursor_icon_for_monitor(overlay_window.monitor);

			#[cfg(not(target_os = "macos"))]
			overlay_window.window.set_cursor(icon);
			#[cfg(target_os = "macos")]
			overlay_window.cursor_rects.sync_cursor_rects(
				overlay_window.window.as_ref(),
				&self.frozen_selection_cursor_rects_for_monitor(overlay_window.monitor),
			);
		}
	}

	pub(super) fn frozen_selection_drag_hides_auxiliary_windows(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen) && self.frozen_selection_drag.active
	}

	fn hide_auxiliary_windows_for_frozen_selection_drag(&mut self) {
		if let Some(hud_window) = self.hud_window.as_ref() {
			hud_window.window.set_visible(false);
		}

		self.hud_window_visible = false;

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
		}

		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();

		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			toolbar_window.window.set_visible(false);
		}

		self.skip_toolbar_focus_on_next_show = true;
		self.toolbar_window_visible = false;
		self.toolbar_window_warmup_redraws_remaining = 0;

		if let Some(preview_window) = self.scroll_preview_window.as_ref() {
			preview_window.window.set_visible(false);
		}

		self.last_present_at = Instant::now();
	}

	fn stop_frozen_selection_drag(&mut self) {
		let was_active = self.frozen_selection_drag.active;

		self.frozen_selection_drag = FrozenSelectionDragState::default();

		if was_active && let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}
	}

	fn stop_frozen_mosaic_drag(&mut self) {
		self.frozen_mosaic_drag = FrozenMosaicDragState::default();
		self.state.frozen_mosaic_preview_rect = None;
	}

	fn begin_frozen_brush_stroke(&mut self, global: GlobalPoint) -> bool {
		if self.toolbar_state.selected_tool != FrozenToolbarTool::Pen {
			return false;
		}
		if self.frozen_brush.active_stroke.is_some() {
			return false;
		}

		let Some((monitor, capture_rect)) = self.frozen_brush_capture_target() else {
			return false;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(global) else {
			return false;
		};

		if !capture_rect.contains((cursor_x, cursor_y)) {
			return false;
		}

		let point = Pos2::new(cursor_x as f32, cursor_y as f32);
		let sampled_at = Instant::now();

		self.frozen_brush.active_stroke =
			Some(Self::new_active_frozen_brush_stroke(point, sampled_at));

		self.request_redraw_for_monitor(monitor);

		true
	}

	fn update_frozen_brush_stroke(&mut self, global: GlobalPoint) -> bool {
		let Some((monitor, capture_rect)) = self.frozen_brush_capture_target() else {
			return false;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(global) else {
			return false;
		};
		let Some(active_stroke) = self.frozen_brush.active_stroke.as_mut() else {
			return false;
		};
		let point = Self::clamped_point_in_capture_rect(capture_rect, cursor_x, cursor_y);
		let sampled_at = Instant::now();

		if let Some(previous) = active_stroke.raw_points.last().copied()
			&& previous.distance(point) < FROZEN_BRUSH_POINT_SPACING_MIN_POINTS
		{
			return false;
		}

		Self::append_frozen_brush_raw_sample(active_stroke, point, sampled_at);

		true
	}

	fn finish_frozen_brush_stroke(&mut self) -> bool {
		let Some(stroke) = self.frozen_brush.active_stroke.take() else {
			return false;
		};

		if stroke.points.is_empty() {
			return false;
		}

		self.frozen_brush
			.committed_strokes
			.push(FrozenBrushStroke { points: Self::finished_frozen_brush_points(&stroke) });
		self.push_frozen_edit_to_undo_history(FrozenEditKind::BrushStroke);
		self.sync_frozen_toolbar_state();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	fn undo_frozen_brush_stroke(&mut self) -> bool {
		let Some(stroke) = self.frozen_brush.committed_strokes.pop() else {
			return false;
		};

		self.frozen_brush.redo_strokes.push(stroke);
		self.sync_frozen_toolbar_state();

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	fn redo_frozen_brush_stroke(&mut self) -> bool {
		let Some(stroke) = self.frozen_brush.redo_strokes.pop() else {
			return false;
		};

		self.frozen_brush.committed_strokes.push(stroke);
		self.sync_frozen_toolbar_state();

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	fn clamped_point_in_capture_rect(
		capture_rect: RectPoints,
		cursor_x: u32,
		cursor_y: u32,
	) -> Pos2 {
		let max_x = capture_rect.x.saturating_add(capture_rect.width.saturating_sub(1));
		let max_y = capture_rect.y.saturating_add(capture_rect.height.saturating_sub(1));

		Pos2::new(
			cursor_x.clamp(capture_rect.x, max_x) as f32,
			cursor_y.clamp(capture_rect.y, max_y) as f32,
		)
	}

	fn new_active_frozen_brush_stroke(point: Pos2, sampled_at: Instant) -> ActiveFrozenBrushStroke {
		ActiveFrozenBrushStroke {
			raw_points: vec![point],
			points: vec![point],
			model_state: FrozenBrushModelState {
				filtered_input_point: point,
				modeled_point: point,
				modeled_velocity: Vec2::ZERO,
				modeled_elapsed_seconds: 0.0,
			},
			started_at: sampled_at,
			last_sample_at: sampled_at,
		}
	}

	fn append_frozen_brush_raw_sample(
		active_stroke: &mut ActiveFrozenBrushStroke,
		point: Pos2,
		sampled_at: Instant,
	) {
		let delta_seconds = sampled_at
			.saturating_duration_since(active_stroke.last_sample_at)
			.as_secs_f32()
			.max(1.0 / 240.0);

		active_stroke.raw_points.push(point);

		let snap_boost = Self::frozen_brush_feature_snap_boost(&active_stroke.raw_points);
		let response = Self::frozen_brush_input_response_with_feature_boost(
			&active_stroke.raw_points,
			delta_seconds,
			Self::frozen_brush_response_floor_boost(&active_stroke.raw_points, snap_boost),
		);

		active_stroke.model_state.filtered_input_point = Pos2::new(
			active_stroke.model_state.filtered_input_point.x
				+ ((point.x - active_stroke.model_state.filtered_input_point.x) * response),
			active_stroke.model_state.filtered_input_point.y
				+ ((point.y - active_stroke.model_state.filtered_input_point.y) * response),
		);

		let elapsed_seconds =
			sampled_at.saturating_duration_since(active_stroke.started_at).as_secs_f32();

		Self::advance_frozen_brush_model(
			&mut active_stroke.points,
			&mut active_stroke.model_state,
			elapsed_seconds,
		);

		if snap_boost >= 1.0 {
			active_stroke.model_state.modeled_point = Pos2::new(
				active_stroke.model_state.modeled_point.x
					+ (point.x - active_stroke.model_state.modeled_point.x),
				active_stroke.model_state.modeled_point.y
					+ (point.y - active_stroke.model_state.modeled_point.y),
			);
			active_stroke.model_state.modeled_velocity *= 0.35;

			Self::push_modeled_frozen_brush_point(
				&mut active_stroke.points,
				active_stroke.model_state.modeled_point,
			);
		} else if snap_boost > 0.0 {
			active_stroke.model_state.modeled_point = Pos2::new(
				active_stroke.model_state.modeled_point.x
					+ ((point.x - active_stroke.model_state.modeled_point.x) * 0.24),
				active_stroke.model_state.modeled_point.y
					+ ((point.y - active_stroke.model_state.modeled_point.y) * 0.24),
			);
			active_stroke.model_state.modeled_velocity *= 0.82;

			Self::push_modeled_frozen_brush_point(
				&mut active_stroke.points,
				active_stroke.model_state.modeled_point,
			);
		}

		active_stroke.last_sample_at = sampled_at;
	}

	#[cfg(test)]
	fn frozen_brush_input_response(points: &[Pos2], delta_seconds: f32) -> f32 {
		let snap_boost = Self::frozen_brush_feature_snap_boost(points);

		Self::frozen_brush_input_response_with_feature_boost(
			points,
			delta_seconds,
			Self::frozen_brush_response_floor_boost(points, snap_boost),
		)
	}

	fn frozen_brush_input_response_with_feature_boost(
		points: &[Pos2],
		delta_seconds: f32,
		feature_boost: f32,
	) -> f32 {
		let speed_points_per_second = points
			.windows(2)
			.last()
			.map_or(0.0, |window| window[0].distance(window[1]) / delta_seconds);
		let normalized_speed = ((speed_points_per_second
			- FROZEN_BRUSH_MODEL_SPEED_FLOOR_POINTS_PER_SECOND)
			/ (FROZEN_BRUSH_MODEL_SPEED_CEILING_POINTS_PER_SECOND
				- FROZEN_BRUSH_MODEL_SPEED_FLOOR_POINTS_PER_SECOND))
			.clamp(0.0, 1.0);
		let base_response = FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MIN
			+ ((FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MAX - FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MIN)
				* normalized_speed);

		base_response.max(feature_boost)
	}

	fn frozen_brush_response_floor_boost(points: &[Pos2], snap_boost: f32) -> f32 {
		Self::frozen_brush_curve_response_boost(points).max(snap_boost)
	}

	fn frozen_brush_preview_rounding_passes(points: &[Pos2]) -> usize {
		if Self::frozen_brush_has_sustained_curve_context(points) {
			FROZEN_BRUSH_PREVIEW_ROUNDING_PASSES + 1
		} else {
			FROZEN_BRUSH_PREVIEW_ROUNDING_PASSES
		}
	}

	fn frozen_brush_curve_response_boost(points: &[Pos2]) -> f32 {
		if Self::frozen_brush_has_sustained_curve_context(points) {
			FROZEN_BRUSH_MODEL_CURVE_RESPONSE_BOOST
		} else {
			0.0
		}
	}

	fn frozen_brush_feature_snap_boost(points: &[Pos2]) -> f32 {
		let len = points.len();

		if len < 3 {
			return 0.0;
		}

		let previous = points[len - 2];
		let current = points[len - 1];
		let anchor = points[len - 3];
		let current_turn_angle = Self::frozen_brush_turn_angle(anchor, previous, current);
		let current_amplitude =
			Self::frozen_brush_point_to_segment_distance(previous, anchor, current);

		if current_turn_angle < FROZEN_BRUSH_MODEL_FEATURE_TURN_RADIANS
			|| current_amplitude < FROZEN_BRUSH_MODEL_FEATURE_AMPLITUDE_POINTS
		{
			return 0.0;
		}
		if Self::frozen_brush_has_sustained_curve_context(points) {
			return 0.0;
		}
		if current_turn_angle >= FROZEN_BRUSH_MODEL_SHARP_TURN_RADIANS {
			return 1.0;
		}
		if len < 4 {
			return 0.0;
		}

		let support = points[len - 4];
		let previous_turn = Self::frozen_brush_signed_turn(support, anchor, previous);
		let current_turn = Self::frozen_brush_signed_turn(anchor, previous, current);
		let previous_turn_angle = Self::frozen_brush_turn_angle(support, anchor, previous);
		let previous_amplitude =
			Self::frozen_brush_point_to_segment_distance(anchor, support, previous);

		if previous_turn * current_turn < 0.0
			&& previous_turn_angle >= FROZEN_BRUSH_MODEL_FEATURE_TURN_RADIANS
			&& previous_amplitude >= FROZEN_BRUSH_MODEL_FEATURE_AMPLITUDE_POINTS
		{
			return 0.94;
		}

		0.0
	}

	fn frozen_brush_has_sustained_curve_context(points: &[Pos2]) -> bool {
		let len = points.len();

		if len < 5 {
			return false;
		}

		let recent_points = &points[len - 5..];
		let mut turn_sign: f32 = 0.0;

		for window in recent_points.windows(3) {
			let signed_turn = Self::frozen_brush_signed_turn(window[0], window[1], window[2]);
			let turn_angle = Self::frozen_brush_turn_angle(window[0], window[1], window[2]);
			let amplitude =
				Self::frozen_brush_point_to_segment_distance(window[1], window[0], window[2]);

			if turn_angle < FROZEN_BRUSH_MODEL_CURVE_TURN_RADIANS
				|| amplitude < FROZEN_BRUSH_MODEL_CURVE_AMPLITUDE_POINTS
			{
				return false;
			}
			if turn_sign.abs() <= f32::EPSILON {
				turn_sign = signed_turn.signum();
			} else if signed_turn.signum() != turn_sign {
				return false;
			}
		}

		true
	}

	fn advance_frozen_brush_model(
		points: &mut Vec<Pos2>,
		model_state: &mut FrozenBrushModelState,
		target_elapsed_seconds: f32,
	) {
		if target_elapsed_seconds <= model_state.modeled_elapsed_seconds {
			return;
		}

		while model_state.modeled_elapsed_seconds + FROZEN_BRUSH_MODEL_TIMESTEP_SECONDS
			< target_elapsed_seconds
		{
			Self::step_frozen_brush_model(points, model_state, FROZEN_BRUSH_MODEL_TIMESTEP_SECONDS);
		}

		let remainder = target_elapsed_seconds - model_state.modeled_elapsed_seconds;

		if remainder > f32::EPSILON {
			Self::step_frozen_brush_model(points, model_state, remainder);
		}
	}

	fn step_frozen_brush_model(
		points: &mut Vec<Pos2>,
		model_state: &mut FrozenBrushModelState,
		delta_seconds: f32,
	) {
		let displacement = model_state.filtered_input_point - model_state.modeled_point;
		let acceleration = (displacement * FROZEN_BRUSH_MODEL_SPRING_CONSTANT)
			- (model_state.modeled_velocity * FROZEN_BRUSH_MODEL_DRAG_CONSTANT);

		model_state.modeled_velocity += acceleration * delta_seconds;
		model_state.modeled_point += model_state.modeled_velocity * delta_seconds;
		model_state.modeled_elapsed_seconds += delta_seconds;

		Self::push_modeled_frozen_brush_point(points, model_state.modeled_point);
	}

	fn finished_frozen_brush_points(stroke: &ActiveFrozenBrushStroke) -> Vec<Pos2> {
		let source_points =
			if stroke.points.len() >= 2 { &stroke.points } else { &stroke.raw_points };

		Self::processed_frozen_brush_points(
			source_points,
			stroke.raw_points[0],
			stroke.raw_points[stroke.raw_points.len().saturating_sub(1)],
			FROZEN_BRUSH_COMMIT_ROUNDING_PASSES,
			true,
		)
	}

	fn rendered_frozen_brush_points(points: &[Pos2], sample_step: f32) -> Vec<Pos2> {
		match points {
			[] => Vec::new(),
			[first] => vec![*first],
			[first, second] => {
				let sample_step = sample_step.max(0.1);
				let mut rendered = vec![*first];

				Self::append_frozen_brush_linear_segment(
					&mut rendered,
					*first,
					*second,
					sample_step,
				);

				rendered
			},
			_ => {
				let sample_step = sample_step.max(0.1);
				let mut rendered = Vec::with_capacity(points.len() * 6);

				rendered.push(points[0]);

				for index in 0..points.len().saturating_sub(1) {
					let previous = if index == 0 { points[0] } else { points[index - 1] };
					let start = points[index];
					let end = points[index + 1];
					let next = points
						.get(index + 2)
						.copied()
						.unwrap_or(points[points.len().saturating_sub(1)]);

					Self::append_frozen_brush_curve_segment(
						&mut rendered,
						previous,
						start,
						end,
						next,
						sample_step,
					);
				}

				rendered
			},
		}
	}

	fn active_frozen_brush_display_points(active_stroke: &ActiveFrozenBrushStroke) -> Vec<Pos2> {
		let source_points = if active_stroke.points.len() >= 2 {
			&active_stroke.points
		} else {
			&active_stroke.raw_points
		};
		let rounding_passes = Self::frozen_brush_preview_rounding_passes(&active_stroke.raw_points);

		Self::processed_frozen_brush_points(
			source_points,
			active_stroke.raw_points[0],
			active_stroke.raw_points[active_stroke.raw_points.len().saturating_sub(1)],
			rounding_passes,
			false,
		)
	}

	fn preview_frozen_brush_points(active_stroke: &ActiveFrozenBrushStroke) -> Vec<Pos2> {
		Self::active_frozen_brush_display_points(active_stroke)
	}

	fn processed_frozen_brush_points(
		source_points: &[Pos2],
		start_point: Pos2,
		end_point: Pos2,
		rounding_passes: usize,
		streamline: bool,
	) -> Vec<Pos2> {
		match source_points {
			[] => Vec::new(),
			[_] | [_, _] => vec![start_point, end_point],
			_ => {
				let streamlined = if streamline {
					Self::streamlined_frozen_brush_points(source_points)
				} else {
					source_points.to_vec()
				};
				let mut rounded =
					Self::rounded_open_frozen_brush_points(&streamlined, rounding_passes);

				if rounded.len() < 2 {
					return vec![start_point, end_point];
				}

				rounded[0] = start_point;

				if let Some(last) = rounded.last_mut() {
					*last = end_point;
				}

				rounded
			},
		}
	}

	fn streamlined_frozen_brush_points(raw_points: &[Pos2]) -> Vec<Pos2> {
		let Some(first) = raw_points.first().copied() else {
			return Vec::new();
		};
		let mut streamlined = vec![first];
		let mut filtered = first;

		for window in raw_points.windows(2) {
			let previous_raw = window[0];
			let current_raw = window[1];
			let normalized_distance = ((previous_raw.distance(current_raw)
				- FROZEN_BRUSH_POINT_SPACING_MIN_POINTS)
				/ (FROZEN_BRUSH_STREAMLINE_DISTANCE_CEILING_POINTS
					- FROZEN_BRUSH_POINT_SPACING_MIN_POINTS))
				.clamp(0.0, 1.0);
			let response = FROZEN_BRUSH_STREAMLINE_RESPONSE_MIN
				+ ((FROZEN_BRUSH_STREAMLINE_RESPONSE_MAX - FROZEN_BRUSH_STREAMLINE_RESPONSE_MIN)
					* normalized_distance);

			filtered = Pos2::new(
				filtered.x + ((current_raw.x - filtered.x) * response),
				filtered.y + ((current_raw.y - filtered.y) * response),
			);

			Self::push_processed_frozen_brush_point(&mut streamlined, filtered);
		}

		streamlined[0] = raw_points[0];

		let last_raw = raw_points[raw_points.len().saturating_sub(1)];

		if let Some(last) = streamlined.last_mut() {
			*last = last_raw;
		}

		streamlined
	}

	fn rounded_open_frozen_brush_points(points: &[Pos2], passes: usize) -> Vec<Pos2> {
		if points.len() <= 2 || passes == 0 {
			return points.to_vec();
		}

		let mut rounded = points.to_vec();

		for _ in 0..passes {
			if rounded.len() <= 2 {
				break;
			}

			let mut next = Vec::with_capacity((rounded.len() * 2).saturating_sub(2));

			next.push(rounded[0]);

			for window in rounded.windows(2) {
				let start = window[0];
				let end = window[1];
				let quarter =
					Pos2::new((start.x * 0.75) + (end.x * 0.25), (start.y * 0.75) + (end.y * 0.25));
				let three_quarters =
					Pos2::new((start.x * 0.25) + (end.x * 0.75), (start.y * 0.25) + (end.y * 0.75));

				Self::push_processed_frozen_brush_point(&mut next, quarter);
				Self::push_processed_frozen_brush_point(&mut next, three_quarters);
			}

			let last = rounded[rounded.len().saturating_sub(1)];

			if next.last().is_none_or(|point| {
				point.distance(last) > FROZEN_BRUSH_PREVIEW_POINT_SPACING_MIN_POINTS
			}) {
				next.push(last);
			} else if let Some(last_point) = next.last_mut() {
				*last_point = last;
			}

			rounded = next;
		}

		rounded
	}

	fn append_frozen_brush_curve_segment(
		points: &mut Vec<Pos2>,
		previous: Pos2,
		start: Pos2,
		end: Pos2,
		next: Pos2,
		sample_step: f32,
	) {
		let approximate_length = start.distance(end);
		let steps = ((approximate_length / sample_step).ceil().max(1.0)) as usize;

		for step in 1..=steps {
			let t = step as f32 / steps as f32;
			let point = Self::catmull_rom_frozen_brush_point(previous, start, end, next, t);

			Self::push_frozen_brush_sample_point(points, point);
		}
	}

	fn append_frozen_brush_linear_segment(
		points: &mut Vec<Pos2>,
		start: Pos2,
		end: Pos2,
		sample_step: f32,
	) {
		let approximate_length = start.distance(end);
		let steps = ((approximate_length / sample_step).ceil().max(1.0)) as usize;

		for step in 1..=steps {
			let t = step as f32 / steps as f32;
			let point = Pos2::new(start.x + (end.x - start.x) * t, start.y + (end.y - start.y) * t);

			Self::push_frozen_brush_sample_point(points, point);
		}
	}

	fn catmull_rom_frozen_brush_point(
		previous: Pos2,
		start: Pos2,
		end: Pos2,
		next: Pos2,
		t: f32,
	) -> Pos2 {
		const CENTRIPETAL_ALPHA: f32 = 0.5;
		const MIN_PARAMETER_STEP: f32 = 1.0e-3;

		let t0 = 0.0;
		let t1 = t0 + previous.distance(start).powf(CENTRIPETAL_ALPHA).max(MIN_PARAMETER_STEP);
		let t2 = t1 + start.distance(end).powf(CENTRIPETAL_ALPHA).max(MIN_PARAMETER_STEP);
		let t3 = t2 + end.distance(next).powf(CENTRIPETAL_ALPHA).max(MIN_PARAMETER_STEP);
		let sample_t = t1 + ((t2 - t1) * t.clamp(0.0, 1.0));
		let a1 = Self::centripetal_catmull_rom_lerp(previous, start, t0, t1, sample_t);
		let a2 = Self::centripetal_catmull_rom_lerp(start, end, t1, t2, sample_t);
		let a3 = Self::centripetal_catmull_rom_lerp(end, next, t2, t3, sample_t);
		let b1 = Self::centripetal_catmull_rom_lerp(a1, a2, t0, t2, sample_t);
		let b2 = Self::centripetal_catmull_rom_lerp(a2, a3, t1, t3, sample_t);

		Self::centripetal_catmull_rom_lerp(b1, b2, t1, t2, sample_t)
	}

	fn centripetal_catmull_rom_lerp(
		start: Pos2,
		end: Pos2,
		start_t: f32,
		end_t: f32,
		sample_t: f32,
	) -> Pos2 {
		let span = (end_t - start_t).max(f32::EPSILON);
		let start_weight = (end_t - sample_t) / span;
		let end_weight = (sample_t - start_t) / span;

		Pos2::new(
			(start.x * start_weight) + (end.x * end_weight),
			(start.y * start_weight) + (end.y * end_weight),
		)
	}

	fn push_frozen_brush_sample_point(points: &mut Vec<Pos2>, point: Pos2) {
		if points.last().is_none_or(|previous| previous.distance(point) > f32::EPSILON) {
			points.push(point);
		}
	}

	fn push_modeled_frozen_brush_point(points: &mut Vec<Pos2>, point: Pos2) {
		let Some(previous) = points.last().copied() else {
			points.push(point);

			return;
		};

		if previous.distance(point) < f32::EPSILON {
			return;
		}
		if previous.distance(point) < FROZEN_BRUSH_MODELED_POINT_SPACING_MIN_POINTS
			&& points.len() > 1
		{
			if let Some(last) = points.last_mut() {
				*last = point;
			}

			return;
		}

		points.push(point);
	}

	fn push_processed_frozen_brush_point(points: &mut Vec<Pos2>, point: Pos2) {
		let Some(previous) = points.last().copied() else {
			points.push(point);

			return;
		};

		if previous.distance(point) < FROZEN_BRUSH_PREVIEW_POINT_SPACING_MIN_POINTS {
			if let Some(last) = points.last_mut() {
				*last = point;
			}

			return;
		}

		points.push(point);
	}

	#[cfg(test)]
	fn corrected_frozen_brush_points(points: &[Pos2]) -> Vec<Pos2> {
		if points.len() <= 2 {
			return points.to_vec();
		}

		let started_at = Instant::now();
		let mut stroke = Self::new_active_frozen_brush_stroke(points[0], started_at);

		for (index, point) in points.iter().copied().enumerate().skip(1) {
			let sampled_at = started_at
				+ Duration::from_secs_f32(
					index as f32 * FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS,
				);

			Self::append_frozen_brush_raw_sample(&mut stroke, point, sampled_at);
		}

		Self::finished_frozen_brush_points(&stroke)
	}

	fn frozen_brush_point_to_segment_distance(point: Pos2, start: Pos2, end: Pos2) -> f32 {
		let segment_x = end.x - start.x;
		let segment_y = end.y - start.y;
		let segment_length_sq = (segment_x * segment_x) + (segment_y * segment_y);

		if segment_length_sq <= f32::EPSILON {
			return point.distance(start);
		}

		let t = (((point.x - start.x) * segment_x) + ((point.y - start.y) * segment_y))
			/ segment_length_sq;
		let t = t.clamp(0.0, 1.0);
		let projection = Pos2::new(start.x + (segment_x * t), start.y + (segment_y * t));

		point.distance(projection)
	}

	fn frozen_brush_turn_angle(previous: Pos2, current: Pos2, next: Pos2) -> f32 {
		let first = current - previous;
		let second = next - current;
		let first_length = first.length();
		let second_length = second.length();

		if first_length <= f32::EPSILON || second_length <= f32::EPSILON {
			return 0.0;
		}

		let cosine = (first.dot(second) / (first_length * second_length)).clamp(-1.0, 1.0);

		cosine.acos()
	}

	fn frozen_brush_signed_turn(previous: Pos2, current: Pos2, next: Pos2) -> f32 {
		let first = current - previous;
		let second = next - current;

		(first.x * second.y) - (first.y * second.x)
	}

	fn update_frozen_selection_drag_rect(&mut self, global: GlobalPoint) -> bool {
		if !self.frozen_selection_drag.active {
			return false;
		}

		let Some((monitor, _capture_rect)) = self.frozen_selection_drag_target() else {
			self.stop_frozen_selection_drag();

			return false;
		};
		let anchor_rect = self.frozen_selection_drag.anchor_rect;
		let next_rect = match self.frozen_selection_drag.interaction {
			FrozenSelectionInteractionKind::Move => {
				let (cursor_x, cursor_y) = Self::clamped_local_point_in_monitor(monitor, global);
				let desired_x =
					i64::from(cursor_x) - i64::from(self.frozen_selection_drag.pointer_offset_x);
				let desired_y =
					i64::from(cursor_y) - i64::from(self.frozen_selection_drag.pointer_offset_y);

				Self::clamp_frozen_capture_rect_to_monitor(
					monitor,
					anchor_rect.width,
					anchor_rect.height,
					desired_x,
					desired_y,
				)
			},
			FrozenSelectionInteractionKind::Resize(corner) => {
				let (cursor_x, cursor_y) = Self::local_point_in_monitor_space(monitor, global);

				Self::resize_frozen_capture_rect_from_corner(
					monitor,
					anchor_rect,
					corner,
					self.frozen_selection_drag.press_cursor_x,
					self.frozen_selection_drag.press_cursor_y,
					cursor_x,
					cursor_y,
				)
			},
		};

		self.apply_frozen_capture_rect_update(monitor, next_rect)
	}

	fn update_frozen_mosaic_drag_rect(&mut self, global: GlobalPoint) -> bool {
		if !self.frozen_mosaic_drag.active {
			return false;
		}

		let Some((monitor, capture_rect)) = self.frozen_mosaic_drag_target() else {
			self.stop_frozen_mosaic_drag();

			return false;
		};
		let (cursor_x, cursor_y) = Self::clamped_local_point_in_rect(monitor, capture_rect, global);
		let next_rect = Self::rect_from_drag_points(
			self.frozen_mosaic_drag.anchor_x,
			self.frozen_mosaic_drag.anchor_y,
			cursor_x,
			cursor_y,
		);

		if self.state.frozen_mosaic_preview_rect == Some(next_rect) {
			return false;
		}

		self.state.frozen_mosaic_preview_rect = Some(next_rect);

		self.request_redraw_for_monitor(monitor);

		true
	}

	fn clamped_local_point_in_monitor(monitor: MonitorRect, global: GlobalPoint) -> (u32, u32) {
		let max_x = i64::from(monitor.width.saturating_sub(1));
		let max_y = i64::from(monitor.height.saturating_sub(1));
		let local_x = (i64::from(global.x) - i64::from(monitor.origin.x)).clamp(0, max_x) as u32;
		let local_y = (i64::from(global.y) - i64::from(monitor.origin.y)).clamp(0, max_y) as u32;

		(local_x, local_y)
	}

	fn clamped_local_point_in_rect(
		monitor: MonitorRect,
		capture_rect: RectPoints,
		global: GlobalPoint,
	) -> (u32, u32) {
		let (local_x, local_y) = Self::clamped_local_point_in_monitor(monitor, global);
		let max_x = capture_rect.x.saturating_add(capture_rect.width.saturating_sub(1));
		let max_y = capture_rect.y.saturating_add(capture_rect.height.saturating_sub(1));

		(local_x.clamp(capture_rect.x, max_x), local_y.clamp(capture_rect.y, max_y))
	}

	fn rect_from_drag_points(
		anchor_x: u32,
		anchor_y: u32,
		cursor_x: u32,
		cursor_y: u32,
	) -> RectPoints {
		let left = anchor_x.min(cursor_x);
		let top = anchor_y.min(cursor_y);
		let right = anchor_x.max(cursor_x).saturating_add(1);
		let bottom = anchor_y.max(cursor_y).saturating_add(1);

		RectPoints::new(left, top, right.saturating_sub(left), bottom.saturating_sub(top))
	}

	fn local_point_in_monitor_space(monitor: MonitorRect, global: GlobalPoint) -> (i64, i64) {
		(
			i64::from(global.x) - i64::from(monitor.origin.x),
			i64::from(global.y) - i64::from(monitor.origin.y),
		)
	}

	fn clamp_frozen_capture_rect_to_monitor(
		monitor: MonitorRect,
		width: u32,
		height: u32,
		desired_x: i64,
		desired_y: i64,
	) -> RectPoints {
		let max_x = i64::from(monitor.width.saturating_sub(width));
		let max_y = i64::from(monitor.height.saturating_sub(height));
		let x = desired_x.clamp(0, max_x) as u32;
		let y = desired_y.clamp(0, max_y) as u32;

		RectPoints::new(x, y, width, height)
	}

	fn resize_frozen_capture_rect_from_corner(
		monitor: MonitorRect,
		anchor_rect: RectPoints,
		corner: FrozenSelectionCorner,
		press_cursor_x: u32,
		press_cursor_y: u32,
		cursor_x: i64,
		cursor_y: i64,
	) -> RectPoints {
		let left = i64::from(anchor_rect.x);
		let top = i64::from(anchor_rect.y);
		let right = i64::from(anchor_rect.x.saturating_add(anchor_rect.width));
		let bottom = i64::from(anchor_rect.y.saturating_add(anchor_rect.height));
		let delta_x = cursor_x - i64::from(press_cursor_x);
		let delta_y = cursor_y - i64::from(press_cursor_y);
		let monitor_width = i64::from(monitor.width);
		let monitor_height = i64::from(monitor.height);

		match corner {
			FrozenSelectionCorner::TopLeft => {
				let next_left = (left + delta_x).clamp(0, right.saturating_sub(1)) as u32;
				let next_top = (top + delta_y).clamp(0, bottom.saturating_sub(1)) as u32;

				RectPoints::new(
					next_left,
					next_top,
					(right as u32).saturating_sub(next_left),
					(bottom as u32).saturating_sub(next_top),
				)
			},
			FrozenSelectionCorner::TopRight => {
				let next_right =
					(right + delta_x).clamp(left.saturating_add(1), monitor_width) as u32;
				let next_top = (top + delta_y).clamp(0, bottom.saturating_sub(1)) as u32;

				RectPoints::new(
					left as u32,
					next_top,
					next_right.saturating_sub(left as u32),
					(bottom as u32).saturating_sub(next_top),
				)
			},
			FrozenSelectionCorner::BottomLeft => {
				let next_left = (left + delta_x).clamp(0, right.saturating_sub(1)) as u32;
				let next_bottom =
					(bottom + delta_y).clamp(top.saturating_add(1), monitor_height) as u32;

				RectPoints::new(
					next_left,
					top as u32,
					(right as u32).saturating_sub(next_left),
					next_bottom.saturating_sub(top as u32),
				)
			},
			FrozenSelectionCorner::BottomRight => {
				let next_right =
					(right + delta_x).clamp(left.saturating_add(1), monitor_width) as u32;
				let next_bottom =
					(bottom + delta_y).clamp(top.saturating_add(1), monitor_height) as u32;

				RectPoints::new(
					left as u32,
					top as u32,
					next_right.saturating_sub(left as u32),
					next_bottom.saturating_sub(top as u32),
				)
			},
		}
	}

	fn apply_frozen_capture_rect_update(
		&mut self,
		monitor: MonitorRect,
		next_rect: RectPoints,
	) -> bool {
		if self.state.frozen_capture_rect == Some(next_rect) {
			return false;
		}

		self.state.frozen_capture_rect = Some(next_rect);

		let toolbar_default_pos =
			self.frozen_toolbar_default_position_for_capture_rect(monitor, next_rect);
		let toolbar_pos = match (
			self.toolbar_state.floating_position,
			self.toolbar_state.default_slot_position,
		) {
			(Some(floating_pos), Some(default_pos))
				if !frozen_toolbar_matches_default_slot(floating_pos, default_pos) =>
			{
				floating_pos
			},
			_ => toolbar_default_pos,
		};

		self.toolbar_state.default_slot_position = Some(toolbar_default_pos);
		self.toolbar_state.floating_position = Some(toolbar_pos);

		let should_trace_frozen_selection_drag_timing =
			self.should_trace_frozen_selection_drag_timing();
		let toolbar_position_elapsed = if should_trace_frozen_selection_drag_timing {
			let toolbar_position_started_at = Instant::now();
			let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);

			Some(toolbar_position_started_at.elapsed())
		} else {
			let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);

			None
		};
		let redraw_request_elapsed = if should_trace_frozen_selection_drag_timing {
			let redraw_request_started_at = Instant::now();

			self.request_redraw_for_monitor(monitor);
			self.request_redraw_toolbar_window();

			if self.scroll_capture.active {
				self.request_redraw_scroll_preview_window();
			}

			Some(redraw_request_started_at.elapsed())
		} else {
			self.request_redraw_for_monitor(monitor);
			self.request_redraw_toolbar_window();

			if self.scroll_capture.active {
				self.request_redraw_scroll_preview_window();
			}

			None
		};

		if should_trace_frozen_selection_drag_timing {
			tracing::trace!(
				op = "overlay.frozen_selection_drag.rect_update",
				monitor_id = monitor.id,
				rect_x = next_rect.x,
				rect_y = next_rect.y,
				rect_width = next_rect.width,
				rect_height = next_rect.height,
				toolbar_position_us =
					toolbar_position_elapsed.map_or(0, |elapsed| elapsed.as_micros()),
				redraw_request_us = redraw_request_elapsed.map_or(0, |elapsed| elapsed.as_micros()),
				scroll_capture_active = self.scroll_capture.active,
				toolbar_outer_pos = ?self.toolbar_outer_pos,
				toolbar_floating_position = ?self.toolbar_state.floating_position,
				"Applied frozen selection rect update."
			);
		}

		true
	}

	fn auto_center_frozen_capture_rect(&mut self) -> bool {
		let Some((monitor, capture_rect)) = self.frozen_capture_rect_drag_target() else {
			return false;
		};
		let Some(capture_image) = self.cropped_frozen_capture_image() else {
			return false;
		};
		let Some(content_bounds) = Self::detect_auto_center_content_bounds(&capture_image) else {
			return false;
		};
		let delta_x_points = Self::auto_center_shift_points(
			content_bounds.x,
			content_bounds.width,
			capture_image.width(),
			capture_rect.width,
		);
		let delta_y_points = Self::auto_center_shift_points(
			content_bounds.y,
			content_bounds.height,
			capture_image.height(),
			capture_rect.height,
		);
		let next_rect = Self::clamp_frozen_capture_rect_to_monitor(
			monitor,
			capture_rect.width,
			capture_rect.height,
			i64::from(capture_rect.x) + delta_x_points,
			i64::from(capture_rect.y) + delta_y_points,
		);

		self.apply_frozen_capture_rect_update(monitor, next_rect)
	}

	fn auto_center_shift_points(
		content_origin_px: u32,
		content_size_px: u32,
		crop_size_px: u32,
		capture_size_points: u32,
	) -> i64 {
		if crop_size_px == 0 || capture_size_points == 0 {
			return 0;
		}

		let content_center_px = content_origin_px as f32 + (content_size_px as f32 * 0.5);
		let crop_center_px = crop_size_px as f32 * 0.5;
		let delta_px = content_center_px - crop_center_px;

		((delta_px * capture_size_points as f32) / crop_size_px as f32).round() as i64
	}

	fn detect_auto_center_content_bounds(image: &RgbaImage) -> Option<RectPoints> {
		let width = image.width();
		let height = image.height();

		if width < 2 || height < 2 {
			return None;
		}

		let edge_strip = Self::auto_center_edge_strip_extent(width.min(height));
		let top_mean = Self::region_rgb_mean(image, 0, width, 0, edge_strip)?;
		let bottom_mean =
			Self::region_rgb_mean(image, 0, width, height.saturating_sub(edge_strip), height)?;
		let left_mean = Self::region_rgb_mean(image, 0, edge_strip, 0, height)?;
		let right_mean =
			Self::region_rgb_mean(image, width.saturating_sub(edge_strip), width, 0, height)?;
		let threshold = {
			let edge_noise = [
				Self::region_rgb_mean_distance(image, 0, width, 0, edge_strip, top_mean),
				Self::region_rgb_mean_distance(
					image,
					0,
					width,
					height.saturating_sub(edge_strip),
					height,
					bottom_mean,
				),
				Self::region_rgb_mean_distance(image, 0, edge_strip, 0, height, left_mean),
				Self::region_rgb_mean_distance(
					image,
					width.saturating_sub(edge_strip),
					width,
					0,
					height,
					right_mean,
				),
			]
			.into_iter()
			.fold(0.0, f32::max);

			(edge_noise * 3.0).round().clamp(24.0, 96.0) as u32
		};
		let min_salient_per_row = (width / 64).max(1) as usize;
		let min_salient_per_column = (height / 64).max(1) as usize;
		let mut row_counts = vec![0_usize; height as usize];
		let mut column_counts = vec![0_usize; width as usize];

		for (x, y, pixel) in image.enumerate_pixels() {
			let salient_distance = [
				Self::rgb_distance_to_mean(pixel, top_mean),
				Self::rgb_distance_to_mean(pixel, bottom_mean),
				Self::rgb_distance_to_mean(pixel, left_mean),
				Self::rgb_distance_to_mean(pixel, right_mean),
			]
			.into_iter()
			.min()
			.unwrap_or(0);

			if salient_distance < threshold {
				continue;
			}

			row_counts[y as usize] += 1;
			column_counts[x as usize] += 1;
		}

		let top = row_counts.iter().position(|count| *count >= min_salient_per_row)?;
		let bottom = row_counts.iter().rposition(|count| *count >= min_salient_per_row)?;
		let left = column_counts.iter().position(|count| *count >= min_salient_per_column)?;
		let right = column_counts.iter().rposition(|count| *count >= min_salient_per_column)?;

		if left > right || top > bottom {
			return None;
		}

		let bounds = RectPoints::new(
			left as u32,
			top as u32,
			(right - left + 1) as u32,
			(bottom - top + 1) as u32,
		);
		let fills_crop_width = bounds.width.saturating_mul(100) >= width.saturating_mul(92);
		let fills_crop_height = bounds.height.saturating_mul(100) >= height.saturating_mul(92);

		if fills_crop_width && fills_crop_height {
			return None;
		}

		Some(bounds)
	}

	fn auto_center_edge_strip_extent(length: u32) -> u32 {
		((length as f32) * 0.08).round().clamp(1.0, 24.0) as u32
	}

	fn region_rgb_mean(image: &RgbaImage, x0: u32, x1: u32, y0: u32, y1: u32) -> Option<[f32; 3]> {
		if x0 >= x1 || y0 >= y1 {
			return None;
		}

		let mut r_total = 0_u64;
		let mut g_total = 0_u64;
		let mut b_total = 0_u64;
		let mut sample_count = 0_u64;

		for y in y0..y1 {
			for x in x0..x1 {
				let pixel = image.get_pixel(x, y);

				r_total += u64::from(pixel[0]);
				g_total += u64::from(pixel[1]);
				b_total += u64::from(pixel[2]);
				sample_count += 1;
			}
		}

		if sample_count == 0 {
			return None;
		}

		Some([
			r_total as f32 / sample_count as f32,
			g_total as f32 / sample_count as f32,
			b_total as f32 / sample_count as f32,
		])
	}

	fn region_rgb_mean_distance(
		image: &RgbaImage,
		x0: u32,
		x1: u32,
		y0: u32,
		y1: u32,
		mean: [f32; 3],
	) -> f32 {
		if x0 >= x1 || y0 >= y1 {
			return 0.0;
		}

		let mut total_distance = 0_u64;
		let mut sample_count = 0_u64;

		for y in y0..y1 {
			for x in x0..x1 {
				total_distance +=
					u64::from(Self::rgb_distance_to_mean(image.get_pixel(x, y), mean));
				sample_count += 1;
			}
		}

		if sample_count == 0 { 0.0 } else { total_distance as f32 / sample_count as f32 }
	}

	fn rgb_distance_to_mean(pixel: &image::Rgba<u8>, mean: [f32; 3]) -> u32 {
		(pixel[0] as f32 - mean[0]).abs().round() as u32
			+ (pixel[1] as f32 - mean[1]).abs().round() as u32
			+ (pixel[2] as f32 - mean[2]).abs().round() as u32
	}

	fn cropped_frozen_capture_image(&self) -> Option<RgbaImage> {
		if self.frozen_capture_source != FrozenCaptureSource::FullscreenFallback
			&& let Some(window_image) = self.frozen_window_image.as_ref()
		{
			match self.config.window_capture_alpha_mode {
				WindowCaptureAlphaMode::Background => {},
				WindowCaptureAlphaMode::MatteLight => {
					return Some(window_image.clone());
				},
				WindowCaptureAlphaMode::MatteDark => {
					return Some(window_image.clone());
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

	fn render_frozen_committed_overlays_into_image(&self, image: &mut RgbaImage) {
		if self.scroll_capture.active {
			return;
		}

		let Some(export_transform) = self.frozen_export_transform_for_image(image) else {
			return;
		};

		Self::for_each_frozen_committed_overlay(
			&self.frozen_edit_undo_stack,
			&self.frozen_brush.committed_strokes,
			&self.frozen_text_annotations,
			|overlay| match overlay {
				FrozenCommittedOverlay::Brush(stroke) => {
					Self::rasterize_frozen_brush_points_into_image(
						image,
						export_transform,
						&stroke.points,
					);
				},
				FrozenCommittedOverlay::Text(annotation) => {
					Self::render_frozen_text_annotation_into_image(
						image,
						export_transform,
						annotation,
					);
				},
			},
		);

		if let Some(active_stroke) = &self.frozen_brush.active_stroke {
			let display_points = Self::active_frozen_brush_display_points(active_stroke);

			Self::rasterize_frozen_brush_points_into_image(
				image,
				export_transform,
				&display_points,
			);
		}
	}

	fn rasterize_frozen_brush_points_into_image(
		export_image: &mut RgbaImage,
		export_transform: FrozenExportTransform,
		points: &[Pos2],
	) {
		if export_image.width() == 0 || export_image.height() == 0 {
			return;
		}

		let radius =
			(FROZEN_BRUSH_STROKE_WIDTH_POINTS * export_transform.scalar_scale() * 0.5).max(1.0);
		let color = image::Rgba(FROZEN_BRUSH_COLOR_RGBA);
		let mut coverage_mask =
			vec![0_u8; export_image.width() as usize * export_image.height() as usize];

		Self::rasterize_frozen_brush_points(
			&mut coverage_mask,
			export_image.width(),
			export_image.height(),
			points,
			export_transform,
			radius,
		);
		Self::blend_frozen_brush_coverage_mask(export_image, &coverage_mask, color);
	}

	fn for_each_frozen_committed_overlay(
		edit_history: &[FrozenEditKind],
		brush_strokes: &[FrozenBrushStroke],
		text_annotations: &[FrozenTextAnnotation],
		mut f: impl FnMut(FrozenCommittedOverlay<'_>),
	) {
		let mut brush_index = 0;
		let mut text_index = 0;

		for edit_kind in edit_history {
			match edit_kind {
				FrozenEditKind::BrushStroke => {
					let Some(stroke) = brush_strokes.get(brush_index) else {
						continue;
					};

					brush_index += 1;

					f(FrozenCommittedOverlay::Brush(stroke));
				},
				FrozenEditKind::TextAnnotation => {
					let Some(annotation) = text_annotations.get(text_index) else {
						continue;
					};

					text_index += 1;

					f(FrozenCommittedOverlay::Text(annotation));
				},
				FrozenEditKind::MosaicEdit => {},
			}
		}
	}

	fn rasterize_frozen_brush_points(
		coverage_mask: &mut [u8],
		export_width: u32,
		export_height: u32,
		points: &[Pos2],
		export_transform: FrozenExportTransform,
		radius: f32,
	) {
		let rendered_points =
			Self::rendered_frozen_brush_points(points, FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS);
		let Some(first) = rendered_points.first().copied() else {
			return;
		};
		let mut previous = export_transform.point_to_pixels(first);

		Self::rasterize_frozen_brush_circle(
			coverage_mask,
			export_width,
			export_height,
			previous,
			radius,
		);

		for point in rendered_points.into_iter().skip(1) {
			let current = export_transform.point_to_pixels(point);

			Self::rasterize_frozen_brush_segment(
				coverage_mask,
				export_width,
				export_height,
				previous,
				current,
				radius,
			);

			previous = current;
		}
	}

	fn frozen_export_capture_rect(&self) -> Option<RectPoints> {
		self.state.frozen_capture_rect.or_else(|| {
			self.state.monitor.map(|monitor| RectPoints::new(0, 0, monitor.width, monitor.height))
		})
	}

	fn frozen_export_transform_for_image(
		&self,
		image: &RgbaImage,
	) -> Option<FrozenExportTransform> {
		FrozenExportTransform::new(
			self.frozen_export_capture_rect()?,
			image.width(),
			image.height(),
		)
	}

	fn rasterize_frozen_brush_segment(
		coverage_mask: &mut [u8],
		export_width: u32,
		export_height: u32,
		start: Pos2,
		end: Pos2,
		radius: f32,
	) {
		let delta = end - start;
		let delta_len_sq = delta.length_sq();

		if delta_len_sq <= f32::EPSILON {
			Self::rasterize_frozen_brush_circle(
				coverage_mask,
				export_width,
				export_height,
				start,
				radius,
			);

			return;
		}

		let min_x = ((start.x.min(end.x) - radius - 0.5).floor().max(0.0)) as u32;
		let min_y = ((start.y.min(end.y) - radius - 0.5).floor().max(0.0)) as u32;
		let max_x = ((start.x.max(end.x) + radius + 0.5)
			.ceil()
			.min((export_width.saturating_sub(1)) as f32)) as u32;
		let max_y = ((start.y.max(end.y) + radius + 0.5)
			.ceil()
			.min((export_height.saturating_sub(1)) as f32)) as u32;

		for y in min_y..=max_y {
			for x in min_x..=max_x {
				let sample = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
				let projection = ((sample - start).dot(delta) / delta_len_sq).clamp(0.0, 1.0);
				let nearest = start + delta * projection;
				let coverage = Self::frozen_brush_coverage(sample.distance(nearest), radius);

				Self::update_frozen_brush_coverage_mask(
					coverage_mask,
					export_width,
					x,
					y,
					coverage,
				);
			}
		}
	}

	fn rasterize_frozen_brush_circle(
		coverage_mask: &mut [u8],
		export_width: u32,
		export_height: u32,
		center: Pos2,
		radius: f32,
	) {
		if export_width == 0 || export_height == 0 {
			return;
		}

		let min_x = ((center.x - radius - 0.5).floor().max(0.0)) as u32;
		let min_y = ((center.y - radius - 0.5).floor().max(0.0)) as u32;
		let max_x =
			(center.x + radius + 0.5).ceil().min((export_width.saturating_sub(1)) as f32) as u32;
		let max_y =
			(center.y + radius + 0.5).ceil().min((export_height.saturating_sub(1)) as f32) as u32;

		if min_x > max_x || min_y > max_y {
			return;
		}

		for y in min_y..=max_y {
			for x in min_x..=max_x {
				let sample = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
				let coverage = Self::frozen_brush_coverage(sample.distance(center), radius);

				Self::update_frozen_brush_coverage_mask(
					coverage_mask,
					export_width,
					x,
					y,
					coverage,
				);
			}
		}
	}

	fn frozen_brush_coverage(distance: f32, radius: f32) -> u8 {
		((radius + 0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u8
	}

	fn update_frozen_brush_coverage_mask(
		coverage_mask: &mut [u8],
		export_width: u32,
		x: u32,
		y: u32,
		coverage: u8,
	) {
		if coverage == 0 {
			return;
		}

		let index = y as usize * export_width as usize + x as usize;

		coverage_mask[index] = coverage_mask[index].max(coverage);
	}

	fn blend_frozen_brush_coverage_mask(
		export_image: &mut RgbaImage,
		coverage_mask: &[u8],
		color: image::Rgba<u8>,
	) {
		let source_alpha = color[3] as f32 / 255.0;

		for (index, pixel) in export_image.pixels_mut().enumerate() {
			let mask_alpha = coverage_mask[index];

			if mask_alpha == 0 {
				continue;
			}

			let src_a = (mask_alpha as f32 / 255.0) * source_alpha;
			let dst_a = pixel[3] as f32 / 255.0;
			let out_a = src_a + dst_a * (1.0 - src_a);

			if out_a <= f32::EPSILON {
				continue;
			}

			for channel in 0..3 {
				let src = color[channel] as f32 / 255.0;
				let dst = pixel[channel] as f32 / 255.0;
				let out = (src * src_a + dst * dst_a * (1.0 - src_a)) / out_a;

				pixel[channel] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
			}

			pixel[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
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

	fn intersect_rect_points(left: RectPoints, right: RectPoints) -> Option<RectPoints> {
		let x = left.x.max(right.x);
		let y = left.y.max(right.y);
		let right_edge = left.x.saturating_add(left.width).min(right.x.saturating_add(right.width));
		let bottom_edge =
			left.y.saturating_add(left.height).min(right.y.saturating_add(right.height));

		(right_edge > x && bottom_edge > y).then(|| {
			RectPoints::new(x, y, right_edge.saturating_sub(x), bottom_edge.saturating_sub(y))
		})
	}

	fn build_frozen_image_patch(image: &RgbaImage, rect: RectPoints) -> Option<FrozenImagePatch> {
		let x = rect.x.min(image.width());
		let y = rect.y.min(image.height());
		let max_width = image.width().saturating_sub(x);
		let max_height = image.height().saturating_sub(y);
		let width = rect.width.min(max_width);
		let height = rect.height.min(max_height);

		if width == 0 || height == 0 {
			return None;
		}

		let rect = RectPoints::new(x, y, width, height);
		let before = imageops::crop_imm(image, x, y, width, height).to_image();
		let after = Self::mosaic_patch(&before);

		Some(FrozenImagePatch { rect, before, after })
	}

	fn mosaic_patch(region: &RgbaImage) -> RgbaImage {
		let mut out = region.clone();

		for block_y in (0..region.height()).step_by(FROZEN_MOSAIC_BLOCK_SIZE_PX as usize) {
			for block_x in (0..region.width()).step_by(FROZEN_MOSAIC_BLOCK_SIZE_PX as usize) {
				let block_width = FROZEN_MOSAIC_BLOCK_SIZE_PX.min(region.width() - block_x);
				let block_height = FROZEN_MOSAIC_BLOCK_SIZE_PX.min(region.height() - block_y);
				let mut sum = [0_u64; 4];
				let mut samples = 0_u64;

				for y in block_y..block_y.saturating_add(block_height) {
					for x in block_x..block_x.saturating_add(block_width) {
						let pixel = region.get_pixel(x, y);

						sum[0] = sum[0].saturating_add(u64::from(pixel[0]));
						sum[1] = sum[1].saturating_add(u64::from(pixel[1]));
						sum[2] = sum[2].saturating_add(u64::from(pixel[2]));
						sum[3] = sum[3].saturating_add(u64::from(pixel[3]));
						samples = samples.saturating_add(1);
					}
				}

				if samples == 0 {
					continue;
				}

				let fill = image::Rgba([
					(sum[0] / samples) as u8,
					(sum[1] / samples) as u8,
					(sum[2] / samples) as u8,
					(sum[3] / samples) as u8,
				]);

				for y in block_y..block_y.saturating_add(block_height) {
					for x in block_x..block_x.saturating_add(block_width) {
						out.put_pixel(x, y, fill);
					}
				}
			}
		}

		out
	}

	fn apply_frozen_image_patch(image: &mut RgbaImage, patch: &FrozenImagePatch, use_after: bool) {
		let source = if use_after { &patch.after } else { &patch.before };

		imageops::replace(image, source, i64::from(patch.rect.x), i64::from(patch.rect.y));
	}

	fn map_rect_into_window_image(
		monitor: MonitorRect,
		capture_rect_points: RectPoints,
		window_image: &RgbaImage,
		selection_rect_points: RectPoints,
	) -> Option<RectPoints> {
		let capture_rect_px = monitor.local_rect_to_pixels(capture_rect_points);
		let selection_rect_px = monitor.local_rect_to_pixels(selection_rect_points);
		let selection_rect_px = Self::intersect_rect_points(selection_rect_px, capture_rect_px)?;

		if capture_rect_px.is_empty() || window_image.width() == 0 || window_image.height() == 0 {
			return None;
		}

		let rel_left = selection_rect_px.x.saturating_sub(capture_rect_px.x);
		let rel_top = selection_rect_px.y.saturating_sub(capture_rect_px.y);
		let rel_right = rel_left.saturating_add(selection_rect_px.width);
		let rel_bottom = rel_top.saturating_add(selection_rect_px.height);
		let capture_width = u64::from(capture_rect_px.width.max(1));
		let capture_height = u64::from(capture_rect_px.height.max(1));
		let target_width = u64::from(window_image.width());
		let target_height = u64::from(window_image.height());
		let left = (u64::from(rel_left) * target_width) / capture_width;
		let top = (u64::from(rel_top) * target_height) / capture_height;
		let right = (u64::from(rel_right) * target_width).div_ceil(capture_width);
		let bottom = (u64::from(rel_bottom) * target_height).div_ceil(capture_height);
		let width = right.saturating_sub(left) as u32;
		let height = bottom.saturating_sub(top) as u32;

		(width > 0 && height > 0).then(|| {
			RectPoints::new(
				left.min(target_width) as u32,
				top.min(target_height) as u32,
				width,
				height,
			)
		})
	}

	fn refresh_frozen_cursor_samples(&mut self, monitor: MonitorRect) {
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
	}

	fn note_frozen_image_mutated(&mut self, monitor: MonitorRect) {
		self.state.frozen_generation = self.state.frozen_generation.wrapping_add(1);

		self.refresh_frozen_cursor_samples(monitor);
		self.sync_frozen_toolbar_state();
		self.request_redraw_for_monitor(monitor);
		self.request_redraw_hud_window();
		self.request_redraw_toolbar_window();

		if self.state.alt_held || self.loupe_window_visible {
			self.request_redraw_loupe_window();
		}
	}

	fn clear_frozen_redo_history(&mut self) {
		self.frozen_edit_redo_stack.clear();
		self.frozen_brush.redo_strokes.clear();
		self.frozen_mosaic_redo_stack.clear();
		self.frozen_text_redo_annotations.clear();
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

	fn push_frozen_mosaic_edit(&mut self, edit: FrozenMosaicEdit) {
		self.frozen_mosaic_undo_stack.push(edit);
	}

	fn apply_frozen_mosaic_edit(&mut self, rect_points: RectPoints) -> bool {
		let Some(monitor) = self.state.monitor else {
			return false;
		};

		if !self.frozen_final_capture_ready() {
			return false;
		}

		let preview_rect_px = monitor.local_rect_to_pixels(rect_points);
		let Some(preview_patch) = self
			.state
			.frozen_image
			.as_ref()
			.and_then(|image| Self::build_frozen_image_patch(image, preview_rect_px))
		else {
			return false;
		};
		let window_patch = match (self.frozen_window_image.as_ref(), self.state.frozen_capture_rect)
		{
			(Some(window_image), Some(capture_rect_points)) => Self::map_rect_into_window_image(
				monitor,
				capture_rect_points,
				window_image,
				rect_points,
			)
			.and_then(|rect| Self::build_frozen_image_patch(window_image, rect)),
			_ => None,
		};

		if let Some(image) = self.state.frozen_image.as_mut() {
			Self::apply_frozen_image_patch(image, &preview_patch, true);
		}
		if let (Some(window_image), Some(window_patch)) =
			(self.frozen_window_image.as_mut(), window_patch.as_ref())
		{
			Self::apply_frozen_image_patch(window_image, window_patch, true);
		}

		self.push_frozen_mosaic_edit(FrozenMosaicEdit { preview_patch, window_patch });
		self.push_frozen_edit_to_undo_history(FrozenEditKind::MosaicEdit);
		self.note_frozen_image_mutated(monitor);

		true
	}

	fn replay_frozen_mosaic_edit(&mut self, edit: &FrozenMosaicEdit, use_after: bool) -> bool {
		let Some(monitor) = self.state.monitor else {
			return false;
		};

		if let Some(image) = self.state.frozen_image.as_mut() {
			Self::apply_frozen_image_patch(image, &edit.preview_patch, use_after);
		}
		if let (Some(window_image), Some(window_patch)) =
			(self.frozen_window_image.as_mut(), edit.window_patch.as_ref())
		{
			Self::apply_frozen_image_patch(window_image, window_patch, use_after);
		}

		self.note_frozen_image_mutated(monitor);

		true
	}

	fn undo_frozen_mosaic_edit(&mut self) -> bool {
		if !self.frozen_final_capture_ready() {
			return false;
		}

		let Some(edit) = self.frozen_mosaic_undo_stack.pop() else {
			return false;
		};
		let reapplied = self.replay_frozen_mosaic_edit(&edit, false);

		self.frozen_mosaic_redo_stack.push(edit);
		self.sync_frozen_toolbar_state();

		reapplied
	}

	fn redo_frozen_mosaic_edit(&mut self) -> bool {
		if !self.frozen_final_capture_ready() {
			return false;
		}

		let Some(edit) = self.frozen_mosaic_redo_stack.pop() else {
			return false;
		};
		let reapplied = self.replay_frozen_mosaic_edit(&edit, true);

		self.frozen_mosaic_undo_stack.push(edit);
		self.sync_frozen_toolbar_state();

		reapplied
	}

	fn commit_frozen_mosaic_drag(&mut self) -> bool {
		let preview_rect = self.state.frozen_mosaic_preview_rect;

		self.stop_frozen_mosaic_drag();

		let Some(preview_rect) = preview_rect else {
			return false;
		};

		if preview_rect.width <= 1 && preview_rect.height <= 1 {
			return false;
		}

		self.apply_frozen_mosaic_edit(preview_rect)
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
		};

		if redone {
			self.frozen_edit_undo_stack.push(edit_kind);
		} else {
			self.frozen_edit_redo_stack.push(edit_kind);
		}

		self.sync_frozen_toolbar_state();

		redone
	}

	fn flatten_window_image_with_matte(image: &RgbaImage, matte: image::Rgba<u8>) -> RgbaImage {
		let mut out = image.clone();

		for pixel in out.pixels_mut() {
			let alpha = u16::from(pixel[3]);
			let inv_alpha = 255_u16.saturating_sub(alpha);

			for channel in 0..3 {
				let src = u16::from(pixel[channel]);
				let bg = u16::from(matte[channel]);
				let blended =
					(src.saturating_mul(alpha) + bg.saturating_mul(inv_alpha) + 127) / 255;

				pixel[channel] = blended as u8;
			}

			pixel[3] = 255;
		}

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

	fn frozen_text_tool_active(&self) -> bool {
		self.toolbar_state.selected_tool == FrozenToolbarTool::Text
	}

	fn sync_text_input_ime_state(&self) {
		let ime_allowed = self.frozen_text_tool_active() && self.frozen_text_edit.is_some();

		for overlay_window in self.windows.values() {
			overlay_window.window.set_ime_allowed(ime_allowed);
		}

		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			toolbar_window.window.set_ime_allowed(ime_allowed);
		}
	}

	fn sync_frozen_text_ime_cursor_area(&self, monitor: MonitorRect) {
		let Some(edit_state) = self.frozen_text_edit.as_ref() else {
			return;
		};
		let Some(overlay_window) = self.windows.values().find(|window| window.monitor == monitor)
		else {
			return;
		};
		let (visible_text, caret_char_index) = edit_state.visible_text_and_caret_char_index();
		let caret_rect = overlay_window.renderer.frozen_text_edit_caret_rect_for_window(
			edit_state.anchor,
			visible_text.as_str(),
			&FontId::proportional(self.toolbar_state.text_style.font_size_points),
			caret_char_index.unwrap_or_else(|| visible_text.chars().count()),
		);

		overlay_window.window.set_ime_cursor_area(
			LogicalPosition::new(
				f64::from(caret_rect.min.x.max(0.0)),
				f64::from(caret_rect.min.y.max(0.0)),
			),
			LogicalSize::new(
				f64::from(caret_rect.width().max(1.0)),
				f64::from(caret_rect.height().max(self.toolbar_state.text_style.font_size_points)),
			),
		);
	}

	fn finish_frozen_text_editing(&mut self, commit: bool) -> bool {
		let Some(edit_state) = self.frozen_text_edit.take() else {
			self.sync_text_input_ime_state();

			return false;
		};
		let had_visible_text = !edit_state.visible_text().trim().is_empty();

		if commit && edit_state.has_committed_text() {
			self.frozen_text_annotations.push(FrozenTextAnnotation {
				anchor: edit_state.anchor,
				text: edit_state.text,
				style: self.toolbar_state.text_style,
			});
			self.push_frozen_edit_to_undo_history(FrozenEditKind::TextAnnotation);
			self.sync_frozen_toolbar_state();
		}

		self.frozen_text_recent_input = None;

		self.sync_text_input_ime_state();

		had_visible_text
	}

	fn note_frozen_text_input_event(&mut self) -> u64 {
		self.frozen_text_input_generation = self.frozen_text_input_generation.wrapping_add(1);

		self.frozen_text_input_generation
	}

	fn append_text_to_frozen_edit_for_input_event(
		&mut self,
		source: FrozenTextInputSource,
		generation: u64,
		text: &str,
	) -> bool {
		let text = text.replace('\r', "");

		if text.is_empty() {
			return false;
		}
		if self.frozen_text_recent_input.as_ref().is_some_and(|recent| {
			recent.source != source
				&& recent.text == text
				&& generation == recent.generation.saturating_add(1)
		}) {
			self.frozen_text_recent_input = None;

			return false;
		}

		let changed = self.append_text_to_frozen_edit(text.as_str());

		if changed {
			self.frozen_text_recent_input =
				Some(FrozenTextRecentInput { source, text, generation });
		}

		changed
	}

	fn append_text_to_frozen_edit(&mut self, text: &str) -> bool {
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};
		let text = text.replace('\r', "");

		if text.is_empty() {
			return false;
		}

		edit_state.text.push_str(&text);

		edit_state.ime_preedit = None;
		edit_state.ime_preedit_cursor_char_range = None;
		self.frozen_text_recent_input = None;

		true
	}

	fn backspace_frozen_text_edit(&mut self) -> bool {
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};
		let had_preedit = edit_state.has_ime_preedit();

		edit_state.ime_preedit = None;
		edit_state.ime_preedit_cursor_char_range = None;

		had_preedit || edit_state.text.pop().is_some()
	}

	fn undo_frozen_text_annotation(&mut self) -> bool {
		let Some(annotation) = self.frozen_text_annotations.pop() else {
			return false;
		};

		self.frozen_text_redo_annotations.push(annotation);

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	fn redo_frozen_text_annotation(&mut self) -> bool {
		let Some(annotation) = self.frozen_text_redo_annotations.pop() else {
			return false;
		};

		self.frozen_text_annotations.push(annotation);

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	fn set_frozen_text_ime_preedit(
		&mut self,
		preedit: Option<String>,
		cursor_range: Option<(usize, usize)>,
	) -> bool {
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};
		let normalized = preedit.filter(|text| !text.is_empty());
		let normalized_cursor_range = normalized.as_deref().and_then(|text| {
			FrozenTextEditState::normalize_ime_preedit_cursor_char_range(text, cursor_range)
		});

		if edit_state.ime_preedit == normalized
			&& edit_state.ime_preedit_cursor_char_range == normalized_cursor_range
		{
			return false;
		}

		edit_state.ime_preedit = normalized;
		edit_state.ime_preedit_cursor_char_range = normalized_cursor_range;

		true
	}

	fn begin_frozen_text_edit_at(&mut self, monitor: MonitorRect, cursor: GlobalPoint) -> bool {
		let Some((local_x, local_y)) = monitor.local_u32(cursor) else {
			return false;
		};
		let Some(capture_rect) = self.state.frozen_capture_rect else {
			return false;
		};

		if !capture_rect.contains((local_x, local_y)) {
			return false;
		}

		let _ = self.finish_frozen_text_editing(true);

		self.frozen_text_edit =
			Some(FrozenTextEditState::new(Pos2::new(local_x as f32, local_y as f32)));
		self.frozen_text_recent_input = None;

		self.sync_text_input_ime_state();
		self.sync_frozen_text_ime_cursor_area(monitor);
		#[cfg(target_os = "macos")]
		self.focus_frozen_text_input_window(Some(monitor));

		true
	}

	fn frozen_text_edit_hit_rect_for_monitor(&self, monitor: MonitorRect) -> Option<Rect> {
		if self.state.monitor != Some(monitor) {
			return None;
		}

		let edit_state = self.frozen_text_edit.as_ref()?;
		let visible_text = edit_state.visible_text();

		Some(WindowRenderer::frozen_text_edit_interaction_rect(
			edit_state.anchor,
			visible_text.as_str(),
			&FontId::proportional(self.toolbar_state.text_style.font_size_points),
		))
	}

	fn begin_frozen_text_edit_drag_at(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) -> bool {
		let Some((local_x, local_y)) = monitor.local_u32(cursor) else {
			return false;
		};
		let Some(hit_rect) = self.frozen_text_edit_hit_rect_for_monitor(monitor) else {
			return false;
		};
		let pointer = Pos2::new(local_x as f32, local_y as f32);

		if !hit_rect.contains(pointer) {
			return false;
		}

		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};

		edit_state.dragging = true;
		edit_state.drag_offset = pointer - edit_state.anchor;

		true
	}

	fn stop_frozen_text_edit_drag(&mut self) -> bool {
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};
		let was_dragging = edit_state.dragging;

		edit_state.dragging = false;
		edit_state.drag_offset = Vec2::ZERO;

		was_dragging
	}

	fn update_frozen_text_edit_drag_anchor(&mut self, global: GlobalPoint) -> bool {
		let Some(monitor) = self.state.monitor else {
			let _ = self.stop_frozen_text_edit_drag();

			return false;
		};
		let Some(capture_rect) = self.state.frozen_capture_rect else {
			let _ = self.stop_frozen_text_edit_drag();

			return false;
		};
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};

		if !edit_state.dragging {
			return false;
		}

		let (cursor_x, cursor_y) = Self::clamped_local_point_in_monitor(monitor, global);
		let max_x = capture_rect.x.saturating_add(capture_rect.width.saturating_sub(1)) as f32;
		let max_y = capture_rect.y.saturating_add(capture_rect.height.saturating_sub(1)) as f32;
		let changed = {
			let next_anchor = Pos2::new(
				(cursor_x as f32 - edit_state.drag_offset.x).clamp(capture_rect.x as f32, max_x),
				(cursor_y as f32 - edit_state.drag_offset.y).clamp(capture_rect.y as f32, max_y),
			);

			if next_anchor == edit_state.anchor {
				false
			} else {
				edit_state.anchor = next_anchor;

				true
			}
		};

		if !changed {
			return false;
		}

		self.sync_frozen_text_ime_cursor_area(monitor);
		self.request_redraw_for_monitor(monitor);

		true
	}

	fn sync_frozen_text_edit_for_selected_tool(&mut self) -> bool {
		if self.frozen_text_tool_active() {
			self.sync_text_input_ime_state();

			return false;
		}

		self.finish_frozen_text_editing(true)
	}

	fn render_frozen_text_annotation_into_image(
		image: &mut RgbaImage,
		export_transform: FrozenExportTransform,
		annotation: &FrozenTextAnnotation,
	) {
		let raster_annotation = RasterTextAnnotation {
			anchor_px: export_transform.point_to_pixels(annotation.anchor),
			font_size_px: annotation.style.font_size_points * export_transform.scalar_scale(),
			fill_rgba: annotation.style.color.export_rgba(),
			text: annotation.text.as_str(),
		};

		text_rendering::render_text_annotations(image, &[raster_annotation]);
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

				self.update_hud_window_position(monitor, cursor);
			}

			self.maybe_start_loupe_window_warmup_redraw();
			self.request_redraw_hud_window();

			if self.state.alt_held || self.loupe_window_visible {
				self.request_redraw_loupe_window();
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
			self.commit_frozen_mosaic_drag();

			let _ = self.finish_frozen_brush_stroke();

			self.stop_frozen_selection_drag();
			self.sync_overlay_cursor_icons();
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
			WindowEvent::Ime(event) => self.handle_ime_event(window_id, event),
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

	fn note_window_focus_change(&mut self, window_id: WindowId, focused: bool) {
		if focused {
			self.focused_window_ids.insert(window_id);

			self.pending_focus_loss_cleanup = false;

			return;
		}

		self.focused_window_ids.remove(&window_id);

		if self.focused_window_ids.is_empty() {
			self.pending_focus_loss_cleanup = true;
		}
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
			self.stop_frozen_selection_drag();
			self.stop_frozen_mosaic_drag();

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

			if !toolbar_left_button_down && self.frozen_text_edit.is_some() {
				self.focus_frozen_text_input_window(self.state.monitor);
			}
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

	fn handle_modifiers_changed(&mut self, modifiers: &Modifiers) -> OverlayControl {
		self.keyboard_modifiers = modifiers.state();

		OverlayControl::Continue
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

	fn set_alt_held(&mut self, alt: bool) {
		if self.state.alt_held == alt {
			return;
		}

		self.state.alt_held = alt;

		if !alt {
			self.handle_alt_release();

			return;
		}

		let Some((monitor, cursor)) = self.alt_activation_cursor_context() else {
			return;
		};

		self.set_alt_loupe_window_visible(Some(monitor), true);

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}

		match self.state.mode {
			OverlayMode::Live => self.request_live_alt_samples(monitor, cursor),
			OverlayMode::Frozen => self.request_frozen_alt_samples(cursor),
		}
	}

	fn apply_loupe_activation_input(&mut self, pressed: bool, repeat: bool) -> bool {
		let previous_alt_held = self.state.alt_held;

		match self.config.alt_activation {
			AltActivationMode::Hold => self.set_alt_held(pressed),
			AltActivationMode::Toggle => {
				if pressed && !repeat {
					self.set_alt_held(!self.state.alt_held);
				}
			},
		}

		previous_alt_held != self.state.alt_held
	}

	fn apply_loupe_activation_key_event(&mut self, pressed: bool, repeat: bool) -> bool {
		self.loupe_activation_key_down = pressed;

		if !pressed && !self.state.alt_held {
			return false;
		}
		if pressed && !self.loupe_activation_shortcut_available() {
			return false;
		}

		self.apply_loupe_activation_input(pressed, repeat)
	}

	fn clear_loupe_activation_on_focus_loss(&mut self) {
		let should_reset = self.loupe_activation_key_down
			|| (matches!(self.config.alt_activation, AltActivationMode::Hold)
				&& self.state.alt_held);

		if !should_reset {
			return;
		}

		self.loupe_activation_key_down = false;

		if self.apply_loupe_activation_input(false, false) {
			let _ = self.request_redraw_for_alt_state_change();
		}
	}

	fn maybe_clear_loupe_activation_after_focus_loss(&mut self) {
		if !self.pending_focus_loss_cleanup || !self.focused_window_ids.is_empty() {
			return;
		}

		self.pending_focus_loss_cleanup = false;

		self.clear_loupe_activation_on_focus_loss();
	}

	fn request_redraw_for_alt_state_change(&mut self) -> OverlayControl {
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

	fn alt_activation_cursor_context(&mut self) -> Option<(MonitorRect, GlobalPoint)> {
		if let Some((monitor, cursor)) = self.last_fresh_event_cursor() {
			self.seed_alt_activation_cursor_context(monitor, cursor);

			return Some((monitor, cursor));
		}

		let cursor = self.sample_mouse_location();
		let Some(monitor) = self.monitor_at(cursor) else {
			if self.state.cursor.is_none() {
				self.state.cursor = Some(cursor);
			}

			return self.active_cursor_monitor().zip(self.state.cursor);
		};

		self.seed_alt_activation_cursor_context(monitor, cursor);

		Some((monitor, cursor))
	}

	fn seed_alt_activation_cursor_context(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		let old_monitor = self.active_cursor_monitor();
		let old_cursor = self.state.cursor;

		match self.state.mode {
			OverlayMode::Live => {
				self.update_cursor_for_live_move(old_monitor, old_cursor, monitor, cursor)
			},
			OverlayMode::Frozen => {
				self.update_cursor_state(monitor, cursor);
				self.update_hud_window_position(monitor, cursor);
			},
		}
	}

	fn handle_alt_release(&mut self) {
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
		if self.live_loupe_uses_hud_window() {
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

			self.maybe_apply_pending_startup_aux_live_stream_filter_upgrade(monitor);

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
		let should_trace_frozen_selection_drag_timing =
			self.should_trace_frozen_selection_drag_timing();
		let cursor_move_started_at = should_trace_frozen_selection_drag_timing.then(Instant::now);
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

		let timing = self.run_cursor_move_updates(
			should_trace_frozen_selection_drag_timing,
			cursor_move_started_at,
			old_monitor,
			old_cursor,
			monitor,
			global,
		);

		if should_trace_frozen_selection_drag_timing {
			self.trace_frozen_selection_drag_cursor_move(monitor, old_monitor, old_cursor, timing);
		}
		if self.update_frozen_brush_stroke(global) {
			self.request_redraw_for_monitor(monitor);
		}

		OverlayControl::Continue
	}

	fn handle_cursor_moved_without_overlay_window(
		&mut self,
		window_id: WindowId,
		old_monitor: Option<MonitorRect>,
	) -> OverlayControl {
		let should_trace_frozen_selection_drag_timing =
			self.should_trace_frozen_selection_drag_timing();
		let cursor_move_started_at = should_trace_frozen_selection_drag_timing.then(Instant::now);

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

		let timing = self.run_cursor_move_updates(
			should_trace_frozen_selection_drag_timing,
			cursor_move_started_at,
			old_monitor,
			old_cursor,
			monitor,
			global,
		);

		if should_trace_frozen_selection_drag_timing {
			self.trace_frozen_selection_drag_cursor_move(monitor, old_monitor, old_cursor, timing);
		}
		if self.update_frozen_brush_stroke(global) {
			self.request_redraw_for_monitor(monitor);
		}

		OverlayControl::Continue
	}

	fn run_cursor_move_updates(
		&mut self,
		should_trace_frozen_selection_drag_timing: bool,
		cursor_move_started_at: Option<Instant>,
		old_monitor: Option<MonitorRect>,
		old_cursor: Option<GlobalPoint>,
		monitor: MonitorRect,
		global: GlobalPoint,
	) -> FrozenSelectionDragCursorMoveTiming {
		let cursor_update_elapsed =
			Self::measure_duration_if(should_trace_frozen_selection_drag_timing, || {
				self.update_cursor_for_live_move(old_monitor, old_cursor, monitor, global)
			});
		let previous_drag_rect = self.state.drag_rect;
		let live_drag_update_elapsed =
			Self::measure_duration_if(should_trace_frozen_selection_drag_timing, || {
				self.update_live_drag_rect(monitor, global);
			});
		let (frozen_rect_changed, frozen_drag_update_elapsed) =
			if should_trace_frozen_selection_drag_timing {
				let frozen_drag_update_started_at = Instant::now();
				let frozen_rect_changed = self.update_frozen_selection_drag_rect(global);

				self.update_frozen_mosaic_drag_rect(global);
				self.update_frozen_text_edit_drag_anchor(global);

				(frozen_rect_changed, Some(frozen_drag_update_started_at.elapsed()))
			} else {
				let frozen_rect_changed = self.update_frozen_selection_drag_rect(global);

				self.update_frozen_mosaic_drag_rect(global);
				self.update_frozen_text_edit_drag_anchor(global);

				(frozen_rect_changed, None)
			};
		let sync_cursor_icons_elapsed =
			Self::measure_duration_if(should_trace_frozen_selection_drag_timing, || {
				self.sync_overlay_cursor_icons();
			});
		let request_samples_elapsed =
			Self::measure_duration_if(should_trace_frozen_selection_drag_timing, || {
				self.request_cursor_move_samples(monitor, global);
			});

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

		FrozenSelectionDragCursorMoveTiming {
			cursor_update_elapsed: cursor_update_elapsed.unwrap_or_default(),
			live_drag_update_elapsed: live_drag_update_elapsed.unwrap_or_default(),
			frozen_drag_update_elapsed: frozen_drag_update_elapsed.unwrap_or_default(),
			frozen_rect_changed,
			sync_cursor_icons_elapsed: sync_cursor_icons_elapsed.unwrap_or_default(),
			request_samples_elapsed: request_samples_elapsed.unwrap_or_default(),
			total_elapsed: cursor_move_started_at
				.map_or(Duration::ZERO, |started_at| started_at.elapsed()),
		}
	}

	fn measure_duration_if(enabled: bool, operation: impl FnOnce()) -> Option<Duration> {
		if enabled {
			let started_at = Instant::now();

			operation();

			Some(started_at.elapsed())
		} else {
			operation();

			None
		}
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

	fn current_frozen_interaction_cursor(&mut self) -> GlobalPoint {
		if let Some((_, cursor)) = self.last_fresh_event_cursor() {
			return cursor;
		}
		if let Some(cursor) = self.state.cursor {
			return cursor;
		}

		let raw = self.current_device_cursor();

		self.resolve_device_cursor_point(raw).map(|(_, cursor, _)| cursor).unwrap_or(raw)
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

	fn trace_frozen_selection_drag_cursor_move(
		&self,
		monitor: MonitorRect,
		old_monitor: Option<MonitorRect>,
		old_cursor: Option<GlobalPoint>,
		timing: FrozenSelectionDragCursorMoveTiming,
	) {
		if !self.should_trace_frozen_selection_drag_timing() {
			return;
		}

		tracing::trace!(
			op = "overlay.frozen_selection_drag.cursor_move_timing",
			monitor_id = monitor.id,
			old_monitor_id = old_monitor.map(|target| target.id),
			old_cursor = ?old_cursor,
			cursor = ?self.state.cursor,
			interaction = ?self.frozen_selection_drag.interaction,
			frozen_rect_changed = timing.frozen_rect_changed,
			cursor_update_us = timing.cursor_update_elapsed.as_micros(),
			live_drag_update_us = timing.live_drag_update_elapsed.as_micros(),
			frozen_drag_update_us = timing.frozen_drag_update_elapsed.as_micros(),
			sync_cursor_icons_us = timing.sync_cursor_icons_elapsed.as_micros(),
			request_samples_us = timing.request_samples_elapsed.as_micros(),
			total_us = timing.total_elapsed.as_micros(),
			overlay_window_count = self.windows.len(),
			"Frozen selection drag cursor move timing."
		);
	}

	fn should_trace_frozen_selection_drag_timing(&self) -> bool {
		tracing::enabled!(tracing::Level::TRACE)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.frozen_selection_drag.active
	}

	fn update_cursor_for_live_move(
		&mut self,
		old_monitor: Option<MonitorRect>,
		old_cursor: Option<GlobalPoint>,
		monitor: MonitorRect,
		global: GlobalPoint,
	) {
		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);

		if Self::live_hud_redraw_needed_for_cursor_update(old_cursor, global, old_monitor, monitor)
		{
			self.request_redraw_hud_window();
		}
		if self.should_try_pending_follow_window_move_on_live_cursor_update() {
			self.maybe_apply_pending_hud_and_loupe_moves();
		}
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
			return self.handle_frozen_left_mouse_input(monitor, state);
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

	fn handle_frozen_left_mouse_input(
		&mut self,
		monitor: MonitorRect,
		state: ElementState,
	) -> OverlayControl {
		self.reset_toolbar_pointer_state();

		if self.frozen_text_tool_active() {
			match state {
				ElementState::Pressed => {
					let cursor = self.current_device_cursor();
					let started_drag = self.begin_frozen_text_edit_drag_at(monitor, cursor);

					if !started_drag {
						let started = self.begin_frozen_text_edit_at(monitor, cursor);

						if !started {
							let _ = self.finish_frozen_text_editing(true);
						}
					}

					self.sync_overlay_cursor_icons();
				},
				ElementState::Released => {
					let stopped_drag = self.stop_frozen_text_edit_drag();

					if stopped_drag {
						self.sync_overlay_cursor_icons();
					}
				},
			}

			self.request_redraw_for_monitor(monitor);

			return OverlayControl::Continue;
		}
		if self.frozen_text_edit.is_some() {
			let _ = self.finish_frozen_text_editing(true);
		}

		match state {
			ElementState::Pressed => {
				let cursor = self.current_frozen_interaction_cursor();

				if self.toolbar_state.selected_tool == FrozenToolbarTool::Pen {
					let _ = self.begin_frozen_brush_stroke(cursor);
				} else if !self.begin_frozen_selection_drag(cursor) {
					let _ = self.begin_frozen_mosaic_drag(cursor);
				}

				self.sync_overlay_cursor_icons();
			},
			ElementState::Released => {
				let _ = self.finish_frozen_brush_stroke();

				self.stop_frozen_selection_drag();
				self.stop_frozen_mosaic_drag();
				self.sync_overlay_cursor_icons();
			},
		}

		self.request_redraw_for_monitor(monitor);

		OverlayControl::Continue
	}

	fn handle_ime_event(&mut self, window_id: WindowId, event: &Ime) -> OverlayControl {
		if !matches!(self.state.mode, OverlayMode::Frozen) || self.frozen_text_edit.is_none() {
			return OverlayControl::Continue;
		}

		let Some(monitor) =
			self.windows.get(&window_id).map(|window| window.monitor).or(self.state.monitor)
		else {
			return OverlayControl::Continue;
		};
		let changed = match event {
			Ime::Commit(text) => {
				let generation = self.note_frozen_text_input_event();

				self.append_text_to_frozen_edit_for_input_event(
					FrozenTextInputSource::Ime,
					generation,
					text,
				)
			},
			Ime::Preedit(text, cursor_range) => {
				self.set_frozen_text_ime_preedit(Some(text.clone()), *cursor_range)
			},
			Ime::Enabled | Ime::Disabled => false,
		};

		if changed {
			self.sync_frozen_text_ime_cursor_area(monitor);
			self.request_redraw_for_monitor(monitor);
		}

		OverlayControl::Continue
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

	fn scroll_capture_direction_from_external_input_delta_y(
		delta_y: f64,
	) -> Option<ScrollDirection> {
		if delta_y == 0.0 {
			return None;
		}

		Self::scroll_capture_direction_from_delta_y(delta_y)
	}

	fn scroll_capture_motion_rows_from_wheel_delta(delta: &MouseScrollDelta) -> f64 {
		match delta {
			MouseScrollDelta::LineDelta(_, y) => f64::from(*y).abs(),
			MouseScrollDelta::PixelDelta(delta) => {
				#[cfg(target_os = "macos")]
				{
					Self::normalize_macos_scroll_pixel_component(delta.y).abs()
				}
				#[cfg(not(target_os = "macos"))]
				{
					delta.y.abs()
				}
			},
		}
	}

	fn accumulate_scroll_capture_downward_motion_rows(&mut self, motion_rows: f64) {
		if !motion_rows.is_finite() || motion_rows <= 0.0 {
			return;
		}

		self.scroll_capture.downward_motion_rows_pending =
			(self.scroll_capture.downward_motion_rows_pending + motion_rows.abs())
				.clamp(0.0, SCROLL_CAPTURE_INPUT_MOTION_PRIOR_ROWS_MAX);
	}

	fn clear_scroll_capture_downward_motion_rows(&mut self) {
		self.scroll_capture.downward_motion_rows_pending = 0.0;
	}

	fn consume_scroll_capture_downward_motion_rows(&mut self, consumed_rows: u32) {
		if consumed_rows == 0 {
			return;
		}

		let remaining = self.scroll_capture.downward_motion_rows_pending - f64::from(consumed_rows);

		self.scroll_capture.downward_motion_rows_pending = remaining.max(0.0);
	}

	fn consume_scroll_capture_downward_motion_rows_for_outcome(
		&mut self,
		outcome: &ScrollObserveOutcome,
	) {
		if let ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows } =
			outcome
		{
			self.consume_scroll_capture_downward_motion_rows(*growth_rows);
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

			if matches!(direction, ScrollDirection::Down) {
				self.accumulate_scroll_capture_downward_motion_rows(
					Self::scroll_capture_motion_rows_from_wheel_delta(delta),
				);
			} else {
				self.clear_scroll_capture_downward_motion_rows();
			}
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
		if let Some(direction) = Self::scroll_capture_direction_from_external_input_delta_y(delta_y)
		{
			if self.should_absorb_upward_external_input_into_active_downward_gesture(
				direction,
				gesture_active,
			) {
				self.record_scroll_capture_input_direction_at(
					ScrollDirection::Down,
					gesture_active,
					at,
				);
				self.accumulate_scroll_capture_downward_motion_rows(delta_y.abs());
			} else {
				self.record_scroll_capture_input_direction_at(direction, gesture_active, at);

				if matches!(direction, ScrollDirection::Down) {
					self.accumulate_scroll_capture_downward_motion_rows(delta_y.abs());
				} else {
					self.clear_scroll_capture_downward_motion_rows();
				}
			}
		}

		if gesture_ended {
			self.finish_scroll_capture_input_direction_at(at);
		}
	}

	fn should_absorb_upward_external_input_into_active_downward_gesture(
		&self,
		direction: ScrollDirection,
		gesture_active: bool,
	) -> bool {
		gesture_active
			&& matches!(direction, ScrollDirection::Up)
			&& self.scroll_capture.input_direction == Some(ScrollDirection::Down)
			&& self.scroll_capture.downward_motion_rows_pending > 0.0
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

		#[cfg(not(target_os = "macos"))]
		if !capture_rect.contains(cursor_pixels) {
			return;
		}

		#[cfg(target_os = "macos")]
		let _cursor_inside_capture_rect = capture_rect.contains(cursor_pixels);

		#[cfg(target_os = "macos")]
		if delta_y != 0.0
			&& !gesture_ended
			&& !self.scroll_capture.overlay_mouse_passthrough_persistent
		{
			self.arm_scroll_overlay_mouse_passthrough_window(
				Instant::now(),
				"external_scroll_input",
			);
		}

		self.apply_scroll_capture_input_delta_y(delta_y, gesture_active, gesture_ended, at);
	}

	fn scroll_capture_trace_snapshot_at(
		&self,
		observation_at: Instant,
	) -> ScrollCaptureTraceSessionSnapshot {
		ScrollCaptureTraceSessionSnapshot::capture(
			self.scroll_capture.session.as_ref(),
			self.scroll_capture_preview_dimensions(),
			self.scroll_capture.input_direction,
			self.scroll_capture.input_gesture_active,
			self.scroll_capture.downward_motion_rows_pending,
			self.scroll_capture_input_age_ms_at(observation_at),
		)
	}

	#[cfg(test)]
	fn scroll_capture_input_allows_observation(&self) -> bool {
		self.scroll_capture_observation_block_reason().is_none()
	}

	#[cfg(test)]
	fn scroll_capture_input_allows_growth(&self) -> bool {
		self.scroll_capture_input_allows_observation()
	}

	#[cfg(test)]
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

	#[cfg(target_os = "macos")]
	fn scroll_capture_should_force_stream_refresh_at(&self, now: Instant) -> bool {
		if !self.scroll_capture_has_fresh_downward_backlog_at(now) {
			return false;
		}
		if self.scroll_capture.input_gesture_active {
			return false;
		}

		let Some(input_direction_at) = self.scroll_capture.input_direction_at else {
			return false;
		};

		now.saturating_duration_since(input_direction_at) <= SCROLL_CAPTURE_INPUT_FRESHNESS
	}

	fn scroll_capture_has_fresh_downward_backlog_at(&self, now: Instant) -> bool {
		if self.scroll_capture.input_direction != Some(ScrollDirection::Down)
			|| self.scroll_capture.downward_motion_rows_pending <= 0.0
		{
			return false;
		}

		let Some(input_direction_at) = self.scroll_capture.input_direction_at else {
			return false;
		};

		now.saturating_duration_since(input_direction_at) <= SCROLL_CAPTURE_INPUT_FRESHNESS
	}

	#[cfg(target_os = "macos")]
	fn scroll_capture_should_schedule_stale_stream_refresh_at(&self, now: Instant) -> bool {
		if !self.scroll_capture.input_gesture_active {
			return true;
		}

		self.scroll_capture.last_stream_event_at.is_none_or(|last_stream_event_at| {
			now.saturating_duration_since(last_stream_event_at)
				>= SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW
		})
	}

	fn scroll_capture_should_allow_post_stall_burst_search_at(
		&self,
		frame_seq: u64,
		now: Instant,
	) -> bool {
		self.scroll_capture.pending_post_stall_burst_after_seq.is_some_and(|after_seq| {
			frame_seq > after_seq && self.scroll_capture_has_fresh_downward_backlog_at(now)
		})
	}

	#[cfg(target_os = "macos")]
	fn scroll_capture_should_arm_post_stall_burst_for_time_gap_at(
		&self,
		frame_captured_at: Instant,
	) -> bool {
		let Some(previous_captured_at) = self.scroll_capture.last_consumed_stream_frame_captured_at
		else {
			return false;
		};

		self.scroll_capture_has_fresh_downward_backlog_at(frame_captured_at)
			&& frame_captured_at.saturating_duration_since(previous_captured_at)
				>= SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW
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

	fn handle_frozen_text_key_event(&mut self, event: &KeyEvent) -> Option<OverlayControl> {
		self.frozen_text_edit.as_ref()?;

		if event.state != ElementState::Pressed {
			return Some(OverlayControl::Continue);
		}

		let changed =
			self.handle_frozen_text_pressed_key(&event.logical_key, event.text.as_deref());

		if changed {
			self.sync_text_input_ime_state();

			if let Some(monitor) = self.state.monitor {
				self.sync_frozen_text_ime_cursor_area(monitor);
				self.request_redraw_for_monitor(monitor);
			}
		}

		Some(OverlayControl::Continue)
	}

	fn handle_frozen_text_pressed_key(&mut self, logical_key: &Key, text: Option<&str>) -> bool {
		match logical_key {
			Key::Named(NamedKey::Escape) => {
				let _ = self.finish_frozen_text_editing(false);

				true
			},
			Key::Named(NamedKey::Enter) => {
				if self.frozen_text_edit.as_ref().is_some_and(FrozenTextEditState::has_ime_preedit)
				{
					return false;
				}
				if self.keyboard_modifiers.shift_key() {
					let generation = self.note_frozen_text_input_event();

					self.append_text_to_frozen_edit_for_input_event(
						FrozenTextInputSource::Key,
						generation,
						"\n",
					)
				} else {
					let _ = self.finish_frozen_text_editing(true);

					true
				}
			},
			Key::Named(NamedKey::Backspace) => self.backspace_frozen_text_edit(),
			_ if !self.keyboard_modifiers.control_key()
				&& !self.keyboard_modifiers.super_key()
				&& !self.keyboard_modifiers.alt_key() =>
			{
				let Some(text) = text else {
					return false;
				};
				let generation = self.note_frozen_text_input_event();

				self.append_text_to_frozen_edit_for_input_event(
					FrozenTextInputSource::Key,
					generation,
					text,
				)
			},
			_ => false,
		}
	}

	fn handle_key_event(&mut self, event: &KeyEvent) -> OverlayControl {
		if matches!(self.state.mode, OverlayMode::Frozen)
			&& let Some(control) = self.handle_frozen_text_key_event(event)
		{
			return control;
		}
		if matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
			let pressed = event.state == ElementState::Pressed;

			if self.apply_loupe_activation_key_event(pressed, event.repeat) {
				return self.request_redraw_for_alt_state_change();
			}

			return OverlayControl::Continue;
		}
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
			Key::Named(NamedKey::Escape) => self.cancel_overlay("escape_key"),
			Key::Character(key_text)
				if (key_text == "h" || key_text == "H")
					&& self.plain_character_shortcut_available() =>
			{
				self.toolbar_state.visible = !self.toolbar_state.visible;

				self.request_redraw_all();

				OverlayControl::Continue
			},
			Key::Character(key_text)
				if key_text.as_str().eq_ignore_ascii_case("c")
					&& self.plain_character_shortcut_available() =>
			{
				self.auto_center_frozen_capture_rect();

				OverlayControl::Continue
			},
			Key::Character(key_text)
				if key_text.as_str().eq_ignore_ascii_case("s")
					&& self.is_save_shortcut_pressed() =>
			{
				self.begin_png_action(PngAction::Save);

				OverlayControl::Continue
			},
			Key::Character(key_text)
				if key_text.as_str().eq_ignore_ascii_case("s")
					&& self.plain_character_shortcut_available() =>
			{
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
					return self.start_scroll_capture();
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

	fn loupe_activation_shortcut_available(&self) -> bool {
		!self.keyboard_modifiers.shift_key()
			&& !self.keyboard_modifiers.alt_key()
			&& !self.keyboard_modifiers.control_key()
			&& !self.keyboard_modifiers.super_key()
	}

	fn plain_character_shortcut_available(&self) -> bool {
		!self.loupe_activation_key_down
			&& !self.keyboard_modifiers.alt_key()
			&& !self.keyboard_modifiers.control_key()
			&& !self.keyboard_modifiers.super_key()
	}

	fn handle_scroll_capture_key_event(&mut self, event: &KeyEvent) -> OverlayControl {
		match &event.logical_key {
			Key::Named(NamedKey::Escape) => self.cancel_overlay("scroll_capture_escape_key"),
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

		let mut export_image =
			self.cropped_frozen_capture_image().or_else(|| self.state.frozen_image.clone())?;

		self.render_frozen_committed_overlays_into_image(&mut export_image);

		Some(export_image)
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

	fn scroll_capture_selection_is_ready(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& self.state.monitor.is_some()
			&& self.state.frozen_capture_rect.is_some()
			&& self.frozen_capture_source == FrozenCaptureSource::DragRegion
			&& self.frozen_final_capture_ready()
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

	fn toolbar_scroll_capture_slot_available(&self) -> bool {
		if self.scroll_capture.active {
			return true;
		}

		#[cfg(target_os = "macos")]
		{
			matches!(self.state.mode, OverlayMode::Frozen)
				&& self.state.monitor.is_some()
				&& self.state.frozen_capture_rect.is_some()
				&& self.frozen_capture_source == FrozenCaptureSource::DragRegion
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
		capture_rect_points: RectPoints,
		capture_rect_pixels: RectPoints,
		base_frame: RgbaImage,
	) -> Result<ScrollCaptureState> {
		let use_worker_sampling = self.should_use_scroll_capture_worker_sampling();
		let trace_recorder = ScrollCaptureTraceRecorder::from_env(
			monitor,
			capture_rect_pixels,
			SCROLL_CAPTURE_PREVIEW_WIDTH_PX,
			&base_frame,
		);
		let preview_latest_frame = Some(base_frame.clone());
		let session = ScrollSession::new(base_frame, SCROLL_CAPTURE_PREVIEW_WIDTH_PX)?;
		let preview_committed_image = Some(session.preview_image().clone());
		let preview_display_image = preview_committed_image.clone();

		Ok(ScrollCaptureState {
			active: true,
			paused: false,
			monitor: Some(monitor),
			#[cfg(target_os = "macos")]
			capture_rect_points: Some(capture_rect_points),
			capture_rect_pixels: Some(capture_rect_pixels),
			input_direction: None,
			input_direction_at: None,
			input_gesture_active: false,
			downward_motion_rows_pending: 0.0,
			#[cfg(target_os = "macos")]
			overlay_mouse_passthrough_active: false,
			#[cfg(target_os = "macos")]
			overlay_mouse_passthrough_persistent: false,
			#[cfg(target_os = "macos")]
			overlay_mouse_passthrough_until: None,
			#[cfg(target_os = "macos")]
			external_scroll_input_drain_reader: self
				.scroll_capture
				.external_scroll_input_drain_reader
				.clone(),
			last_external_scroll_input_seq: 0,
			#[cfg(target_os = "macos")]
			pixel_delta_residual: MacOSScrollPixelResidual::default(),
			#[cfg(target_os = "macos")]
			live_stream: (!use_worker_sampling).then(|| {
				MacLiveFrameStream::with_scroll_capture_region_and_waker(
					self.config.self_capture_exception_window_ids.clone(),
					capture_rect_points,
					capture_rect_pixels,
					self.scroll_frame_waker.clone(),
				)
			}),
			#[cfg(target_os = "macos")]
			live_stream_backlog: VecDeque::new(),
			last_stream_frame_seq: 0,
			#[cfg(target_os = "macos")]
			last_stream_frame_fingerprint: None,
			#[cfg(target_os = "macos")]
			consecutive_identical_stream_frames: 0,
			#[cfg(target_os = "macos")]
			last_consumed_stream_frame_captured_at: None,
			#[cfg(target_os = "macos")]
			last_stream_event_at: None,
			#[cfg(target_os = "macos")]
			last_stream_poll_at: None,
			#[cfg(target_os = "macos")]
			last_duplicate_stream_refresh_at: None,
			pending_post_stall_burst_after_seq: None,
			#[cfg(target_os = "macos")]
			live_stream_stale_grace: None,
			next_sample_at: Some(Instant::now() + SCROLL_CAPTURE_SAMPLE_INTERVAL),
			next_request_id: 0,
			inflight_request_id: None,
			#[cfg(target_os = "macos")]
			inflight_request_observation: None,
			#[cfg(all(test, target_os = "macos"))]
			force_worker_sampling_in_tests: false,
			session: Some(session),
			preview_committed_image,
			preview_latest_frame,
			preview_display_image,
			retained_overlay_preview_image: None,
			retained_overlay_preview_motion_rows_hint: None,
			last_overlay_preview_motion_rows_hint: None,
			last_overlay_preview_provisional_motion_rows_hint: None,
			last_overlay_preview_existing_candidate_height: None,
			last_overlay_preview_existing_candidate_motion_rows_hint: None,
			last_overlay_preview_ledger_candidate_height: None,
			last_overlay_preview_ledger_candidate_motion_rows_hint: None,
			last_overlay_preview_retained_candidate_height: None,
			last_overlay_preview_retained_candidate_motion_rows_hint: None,
			last_overlay_preview_retained_hint_matches_motion_rows: false,
			last_overlay_preview_fresh_latest_frame_can_drive: false,
			last_overlay_preview_strong_unresolved_registration: false,
			last_overlay_preview_latest_frame_present: false,
			last_overlay_preview_used_provisional: false,
			trace_recorder,
		})
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

	fn start_scroll_capture(&mut self) -> OverlayControl {
		if self.scroll_capture.active {
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "already_active",
				"Skipped starting scroll capture because a session is already active."
			);

			return OverlayControl::Continue;
		}

		#[cfg(not(target_os = "macos"))]
		{
			tracing::info!(
				op = "scroll_capture.start_rejected",
				reason = "unsupported_platform",
				"Skipped starting scroll capture because the current platform is unsupported."
			);

			OverlayControl::Continue
		}
		#[cfg(target_os = "macos")]
		{
			let Some((monitor, capture_rect_points, capture_rect_pixels, base_frame)) =
				self.try_prepare_scroll_capture_start()
			else {
				return OverlayControl::Continue;
			};

			if let Some(guard) = self.scroll_capture_start_guard.clone() {
				match guard() {
					Ok(true) => {},
					Ok(false) => return OverlayControl::Continue,
					Err(err) => {
						self.state.set_error(format!("{err:#}"));
						self.request_redraw_all();

						return OverlayControl::Continue;
					},
				}
			}
			if let Some(hook) = self.scroll_capture_starting_hook.clone()
				&& let Err(err) = hook()
			{
				self.state.set_error(format!("{err:#}"));
				self.request_redraw_all();

				return OverlayControl::Continue;
			}

			let base_frame_dimensions = base_frame.dimensions();

			self.scroll_capture = match self.build_scroll_capture_state(
				monitor,
				capture_rect_points,
				capture_rect_pixels,
				base_frame,
			) {
				Ok(scroll_capture) => scroll_capture,
				Err(err) => {
					self.state.set_error(format!("{err:#}"));
					self.request_redraw_all();

					return OverlayControl::Continue;
				},
			};

			if let Some(hook) = self.scroll_capture_started_hook.clone() {
				hook();
			}
			if let Some(trace_recorder) = self.scroll_capture.trace_recorder.as_ref() {
				tracing::info!(
					op = "scroll_capture.trace_recording_enabled",
					manifest_path = %trace_recorder.manifest_path().display(),
					"Enabled scroll-capture live trace recording for this session."
				);
			}

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

			self.sync_frozen_toolbar_state();
			self.refresh_scroll_preview_committed_image();
			self.refresh_scroll_preview_display_image();
			self.sync_scroll_preview_segments();
			self.position_scroll_preview_window(monitor);
			self.update_scroll_toolbar_default_position(monitor);
			self.set_scroll_overlay_mouse_passthrough_persistent(true, "scroll_capture_started");
			self.focus_scroll_keyboard_window();
			self.maybe_apply_pending_startup_aux_live_stream_filter_upgrade(monitor);

			if let Some(preview) = self.scroll_preview_window.as_ref() {
				preview.window.set_visible(true);
				preview.window.request_redraw();
			}
			if let (Some(monitor), Some(live_stream)) =
				(self.scroll_capture.monitor, self.scroll_capture.live_stream.as_ref())
			{
				live_stream.prime_monitor_nonblocking(monitor);
			}

			self.request_redraw_for_monitor(monitor);

			OverlayControl::Continue
		}
	}

	fn toggle_scroll_capture_paused(&mut self) {
		if !self.scroll_capture.active {
			return;
		}

		self.scroll_capture.paused = !self.scroll_capture.paused;

		#[cfg(target_os = "macos")]
		if self.scroll_capture.paused {
			self.set_scroll_overlay_mouse_passthrough_persistent(false, "paused");
		}
		if !self.scroll_capture.paused {
			#[cfg(target_os = "macos")]
			{
				self.set_scroll_overlay_mouse_passthrough_persistent(true, "resumed");

				if let (Some(monitor), Some(live_stream)) =
					(self.scroll_capture.monitor, self.scroll_capture.live_stream.as_ref())
				{
					live_stream.prime_monitor_nonblocking(monitor);
				}
			}
			#[cfg(not(target_os = "macos"))]
			{
				self.scroll_capture.next_sample_at =
					Some(Instant::now() + SCROLL_CAPTURE_SAMPLE_INTERVAL);
			}
		}

		self.request_redraw_scroll_preview_window();
	}

	fn prepare_active_scroll_capture_output(&mut self) {
		if !self.scroll_capture.active {
			return;
		}

		self.maybe_tick_scroll_capture();
		self.refresh_scroll_preview_committed_image();
		self.refresh_scroll_preview_display_image();
		self.sync_scroll_preview_segments();
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

		self.refresh_scroll_preview_committed_image();
		self.clear_scroll_capture_inflight_request();

		#[cfg(target_os = "macos")]
		{
			if let (Some(monitor), Some(live_stream)) =
				(self.scroll_capture.monitor, self.scroll_capture.live_stream.as_ref())
			{
				live_stream.prime_monitor_nonblocking(monitor);
			}
		}
		#[cfg(not(target_os = "macos"))]
		{
			self.scroll_capture.next_sample_at =
				Some(Instant::now() + SCROLL_CAPTURE_SAMPLE_INTERVAL);
		}

		self.refresh_scroll_preview_display_image();
		self.sync_scroll_preview_segments();
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
	fn set_scroll_overlay_mouse_passthrough_persistent(
		&mut self,
		passthrough: bool,
		reason: &'static str,
	) {
		let now = Instant::now();

		self.scroll_capture.overlay_mouse_passthrough_persistent = passthrough;
		self.scroll_capture.overlay_mouse_passthrough_until = None;

		self.set_scroll_overlay_mouse_passthrough_state(now, passthrough, reason);
	}

	#[cfg(target_os = "macos")]
	fn arm_scroll_overlay_mouse_passthrough_window(&mut self, now: Instant, reason: &'static str) {
		if self.scroll_capture.overlay_mouse_passthrough_persistent {
			return;
		}

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
		self.scroll_capture.overlay_mouse_passthrough_persistent = false;
		self.scroll_capture.overlay_mouse_passthrough_until = None;

		self.set_scroll_overlay_mouse_passthrough_state(now, false, reason);
	}

	#[cfg(target_os = "macos")]
	fn sync_scroll_overlay_mouse_passthrough_window(&mut self, now: Instant) {
		if self.scroll_capture.overlay_mouse_passthrough_persistent {
			return;
		}
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
	fn frozen_text_input_overlay_window(&self, monitor: Option<MonitorRect>) -> Option<&Window> {
		monitor
			.and_then(|target| self.windows.values().find(|window| window.monitor == target))
			.or_else(|| self.windows.values().next())
			.map(|overlay_window| overlay_window.window.as_ref())
	}

	#[cfg(target_os = "macos")]
	fn focus_frozen_text_input_window(&self, monitor: Option<MonitorRect>) {
		macos_activate_app();

		let Some(target_window) = self.frozen_text_input_overlay_window(monitor) else {
			tracing::info!(
				op = "overlay.frozen_text_focus_requested",
				target = "missing_window",
				monitor_id = ?monitor.map(|target| target.id),
				"Requested frozen text input focus, but no overlay window was available."
			);

			return;
		};

		tracing::info!(
			op = "overlay.frozen_text_focus_requested",
			target = "overlay_window",
			monitor_id = ?monitor.map(|target| target.id),
			"Requested frozen text input focus."
		);

		macos_make_window_key(target_window);
	}

	#[cfg(target_os = "macos")]
	fn focus_frozen_keyboard_window(&self) {
		macos_activate_app();

		if self.frozen_text_edit.is_some()
			&& let Some(target_window) = self.frozen_text_input_overlay_window(self.state.monitor)
		{
			tracing::info!(
				op = "scroll_capture.frozen_focus_requested",
				target = "overlay_window",
				state_mode = ?self.state.mode,
				toolbar_window_visible = self.toolbar_window_visible,
				monitor_id = ?self.state.monitor.map(|monitor| monitor.id),
				"Requested frozen keyboard focus for text editing."
			);

			macos_make_window_key(target_window);

			return;
		}

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

		self.toolbar_state.default_slot_position = Some(toolbar_pos);
		self.toolbar_state.floating_position = Some(toolbar_pos);

		let _ = self.update_toolbar_outer_position(monitor, toolbar_pos);
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

			return !frozen_toolbar_matches_default_slot(toolbar_pos, current_default_pos);
		}

		false
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

		// On macOS the frozen toolbar is now rendered in its own native HUD window; keep this
		// fullscreen overlay free of toolbar UI so shader-backed blur and monitor-aligned offsets
		// do not conflict with native-window positioning.
		let draw_toolbar = !cfg!(target_os = "macos")
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.toolbar_state.visible
			&& self.state.monitor == Some(overlay_monitor)
			&& self.frozen_final_capture_ready();
		let toolbar_input =
			if draw_toolbar { self.toolbar_pointer_state(overlay_monitor, None) } else { None };

		self.log_frozen_overlay_redraw_trace(window_id, overlay_monitor, draw_toolbar);

		let overlay_screen_rect = self.overlay_window_screen_rect(window_id, overlay_monitor);
		let toolbar_visible_for_badge = if cfg!(target_os = "macos") {
			!self.should_hide_toolbar_window(overlay_monitor)
		} else {
			draw_toolbar
		};
		#[cfg(target_os = "macos")]
		let toolbar_ready_for_badge = if toolbar_visible_for_badge {
			let ready = self.advance_frozen_toolbar_readiness_sample(overlay_screen_rect);

			if !ready {
				self.request_redraw_for_monitor(overlay_monitor);
			}

			ready
		} else {
			false
		};
		#[cfg(not(target_os = "macos"))]
		let toolbar_ready_for_badge =
			toolbar_visible_for_badge && self.frozen_toolbar_ready_for_draw(overlay_screen_rect);
		let frozen_toolbar_reserved_rect = self.frozen_size_badge_toolbar_reserved_rect(
			overlay_monitor,
			overlay_screen_rect,
			toolbar_ready_for_badge,
		);
		let frozen_selection_resize_handles_enabled = self.frozen_selection_drag_target().is_some();
		let Some(gpu) = self.gpu.as_ref() else {
			return self.exit(OverlayExit::Error(String::from("Missing GPU context")));
		};
		let frozen_text_style = self.toolbar_state.text_style;
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
				!self.scroll_capture.active,
				self.scroll_capture.active,
				frozen_selection_resize_handles_enabled,
				self.frozen_capture_source,
				self.frozen_capture_source == FrozenCaptureSource::FullscreenFallback,
				frozen_toolbar_reserved_rect,
				&self.frozen_edit_undo_stack,
				(!self.scroll_capture.active).then_some(&self.frozen_brush),
				&self.frozen_text_annotations,
				self.frozen_text_edit.as_ref(),
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
		if self.should_dispatch_pending_freeze_capture(overlay_monitor)
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
					self.inflight_freeze_capture = Some(overlay_monitor);
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
						self.inflight_freeze_capture = Some(overlay_monitor);
						self.inflight_window_freeze_capture = pending_window_target;
						self.pending_window_freeze_capture = None;
					} else {
						self.request_redraw_for_monitor(overlay_monitor);
					}
				} else {
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
		self.reset_runtime_for_exit();
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

	fn finalize_scroll_capture_for_exit(&mut self) {
		if self.scroll_capture.active {
			self.maybe_tick_scroll_capture();
			self.refresh_scroll_preview_committed_image();
			self.refresh_scroll_preview_display_image();
			self.sync_scroll_preview_segments();
		}

		let scroll_capture_final_snapshot = self.scroll_capture_trace_snapshot_at(Instant::now());
		let final_preview_image = self.current_scroll_preview_render_image();

		if let (Some(trace_recorder), Some(session)) =
			(self.scroll_capture.trace_recorder.as_mut(), self.scroll_capture.session.as_ref())
		{
			let final_preview_image =
				final_preview_image.unwrap_or_else(|| session.preview_image().clone());

			trace_recorder.finalize_session(
				session,
				&final_preview_image,
				scroll_capture_final_snapshot,
			);
		}
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
		self.skip_toolbar_focus_on_next_show = false;
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

		self.frozen_text_annotations.clear();
		self.frozen_text_redo_annotations.clear();

		self.frozen_text_edit = None;
		self.frozen_text_recent_input = None;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrozenTextInputSource {
	Key,
	Ime,
}

#[derive(Clone, Debug, PartialEq)]
struct FrozenTextRecentInput {
	source: FrozenTextInputSource,
	text: String,
	generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrozenEditKind {
	BrushStroke,
	MosaicEdit,
	TextAnnotation,
}

#[derive(Clone, Copy, Debug)]
enum FrozenCommittedOverlay<'a> {
	Brush(&'a FrozenBrushStroke),
	Text(&'a FrozenTextAnnotation),
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
mod tests;
