pub(crate) mod replay_support;

mod aux_window_runtime;
mod capture_window_runtime;
mod config_runtime;
mod cursor_context_runtime;
mod cursor_runtime;
mod exit_runtime;
mod frozen_arrow_runtime;
mod frozen_brush_runtime;
mod frozen_capture_backend_adapter;
mod frozen_edit_history_runtime;
mod frozen_export_model;
mod frozen_export_runtime;
mod frozen_mosaic_runtime;
mod frozen_selection_geometry;
mod frozen_selection_handles;
mod frozen_selection_runtime;
mod frozen_spotlight_runtime;
mod frozen_text_runtime;
mod frozen_transition_runtime;
mod hud_geometry;
mod hud_helpers;
mod hud_pill_style;
mod hud_runtime;
mod image_helpers;
mod input_runtime;
#[cfg(target_os = "macos")]
mod macos_capture_host;
#[cfg(target_os = "macos")]
mod macos_cursor_runtime;
#[cfg(target_os = "macos")]
mod macos_native_capture_shell_runtime;
#[cfg(target_os = "macos")]
mod macos_window_bridge;
mod rendering;
mod runtime_model;
mod runtime_timing;
mod scroll_capture_runtime;
mod scroll_capture_timing;
mod scroll_input_runtime;
mod scroll_preview_geometry;
mod scroll_preview_runtime;
mod scroll_runtime;
mod session_bootstrap_runtime;
mod session_contracts;
mod session_state;
mod toolbar_geometry;
mod toolbar_layout_model;
mod toolbar_runtime;
mod trace_recording;
mod window_content_policy;
mod window_position_runtime;
mod window_runtime;
mod worker_runtime;

#[cfg(target_os = "macos")]
pub use self::macos_capture_host::{
	MacOSNativeCaptureInputEvent, MacOSNativeCaptureScrollDelta, ScrollCaptureHostAdapter,
	ScrollCaptureHostFrameRequestError, ScrollCaptureHostStartRequest,
};
#[cfg(target_os = "macos")]
pub use self::macos_native_capture_shell_runtime::{MacOSCaptureHost, MacOSCaptureHostSyncState};
pub use self::session_contracts::{
	FrozenGlobalHotkey, HudAnchor, OverlayConfig, OverlayControl, OverlayExit,
	OverlayKeyboardInputEvent, ThemeMode, ToolbarPlacement, WindowCaptureAlphaMode,
};

#[cfg(not(target_os = "macos"))]
use std::env;
use std::mem;
#[cfg(not(target_os = "macos"))]
use std::panic;
use std::ptr;
use std::slice;
#[cfg(target_os = "macos")]
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
	borrow::Cow,
	collections::{HashMap, HashSet},
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
use egui_phosphor::Variant;
use egui_wgpu::{Renderer, ScreenDescriptor};
#[cfg(target_os = "macos")]
use image::imageops;
use image::{Rgba, RgbaImage};
use wgpu::Adapter;
use wgpu::AddressMode;
use wgpu::BindGroupLayout;
use wgpu::BindingResource;
use wgpu::BindingType;
use wgpu::BlendState;
use wgpu::Buffer;
use wgpu::BufferBindingType;
use wgpu::BufferSize;
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

use self::frozen_export_model::{FrozenExportTransform, FrozenImagePatch, FrozenMosaicEdit};
use self::frozen_text_runtime::{FrozenTextInputSource, FrozenTextRecentInput};
use self::frozen_transition_runtime::FrozenTransitionRuntime;
use self::hud_geometry::LOUPE_TILE_CORNER_RADIUS_POINTS;
#[cfg(target_os = "macos")]
use self::macos_capture_host::ExternalScrollInputDrainReader;
#[cfg(all(test, target_os = "macos"))]
use self::macos_capture_host::{
	ScrollCaptureStartGuard, ScrollCaptureStartedHook, ScrollCaptureStartingHook,
};
#[cfg(target_os = "macos")]
use self::macos_window_bridge::{
	MacOSFrontmostApplication, MacOSNativeCaptureInputDispatch, macos_activate_app,
	macos_configure_hud_window, macos_configure_overlay_window_mouse_moved_events,
	macos_frontmost_application, macos_hid_event_source_state_id, macos_mouse_location,
	macos_overlay_window_ns_view, macos_post_scroll_wheel_event,
	macos_restore_frontmost_application, macos_set_capture_window_mouse_passthrough,
};
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
use self::runtime_model::{
	AcquiredSurfaceFrame, DeviceCursorPointSource, FrozenCaptureSource, FrozenCommittedOverlay,
	FrozenEditKind, FrozenSelectionCorner, FrozenSelectionInteractionKind, FrozenToolbarTool,
	HudTheme, LiveCaptureInteraction, OverlayEventLoopPhase, PngAction, ScrollCaptureFrameSource,
	SelectionFlowStyle, SurfaceFrameSkipReason, WindowRendererPath,
};
use self::runtime_timing::{
	CURSOR_POLL_INTERVAL_MIN, OCCLUDED_FRAME_REDRAW_RETRY_WINDOW, SLOW_OP_WARN_WINDOW_EVENT,
};
use self::session_bootstrap_runtime::InitialSessionRuntime;
#[cfg(all(target_os = "macos", test))]
use self::session_state::InflightScrollCaptureObservation;
use self::session_state::{
	ActiveFrozenBrushStroke, CursorMoveTrace, FrozenAnnotationColor, FrozenArrowAnnotation,
	FrozenArrowDragState, FrozenBrushModelState, FrozenBrushState, FrozenBrushStroke,
	FrozenCaptureSessionState, FrozenCaptureWorkerState, FrozenExportSessionState,
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
use self::toolbar_geometry::TOOLBAR_WINDOW_WARMUP_REDRAWS;
#[cfg(target_os = "macos")]
use self::trace_recording::ScrollCaptureTraceInputRecord;
use self::trace_recording::{
	ScrollCaptureTraceFrameRecord, ScrollCaptureTraceRecorder, ScrollCaptureTraceSessionSnapshot,
};
#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::{CursorSampleRequest, MacLiveFrameStream};
use crate::scroll_capture::{self, ScrollDirection, ScrollObserveOutcome, ScrollSession};
use crate::state::LiveCursorSample;
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
use rsnap_capture_core::DeferredTextRecognitionRequest;
#[cfg(test)]
use rsnap_capture_core::OutputNaming;
use rsnap_capture_core::PreparedHostEffectRequest;

#[cfg(target_os = "macos")]
#[allow(unused_macros)]
macro_rules! sel {
	($($tt:tt)*) => {
		objc::sel!($($tt)*)
	};
}

#[cfg(target_os = "macos")]
#[allow(unused_macros)]
macro_rules! sel_impl {
	($($tt:tt)*) => {
		objc::sel_impl!($($tt)*)
	};
}

type Result<T, E = Report> = std::result::Result<T, E>;

/// Transitional Rust-core session controller that owns product state, rendering,
/// explicit host requests, and host-sync state consumed by the native app host.
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
	#[cfg(target_os = "macos")]
	last_live_surface_bg_snapshot_at: Option<Instant>,
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
	pending_click_hit_test_requested_at: Option<Instant>,
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
	frozen_capture_session_state: FrozenCaptureSessionState,
	frozen_transition: FrozenTransitionRuntime,
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
	toolbar_window_drawn_once: bool,
	toolbar_badge_slot_ready: bool,
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
	scroll_capture_host_adapter: Option<ScrollCaptureHostAdapter>,
	#[cfg(all(test, target_os = "macos"))]
	scroll_capture_start_guard: Option<ScrollCaptureStartGuard>,
	#[cfg(all(test, target_os = "macos"))]
	scroll_capture_starting_hook: Option<ScrollCaptureStartingHook>,
	#[cfg(all(test, target_os = "macos"))]
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
		let now = Instant::now();

		Self {
			config: OverlayConfig::default(), worker: None,
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
			last_hud_window_move_at: now, last_loupe_window_move_at: now,
			last_toolbar_window_move_at: now, last_present_at: now,
			last_live_cursor_poll_at: now, last_frozen_cursor_poll_at: now,
			window_list_snapshot: None,
			last_window_list_refresh_request_at: now,
			window_list_refresh_interval: Duration::ZERO,
			last_live_bg_request_at: now,
			live_bg_request_interval: Duration::ZERO,
			#[cfg(target_os = "macos")]
			last_live_surface_bg_snapshot_at: None,
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
			pending_click_hit_test_requested_at: None,
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
			event_loop_last_progress_at: now,
			event_loop_last_progress_window_id: None, event_loop_last_progress_monitor_id: None,
			event_loop_last_progress_detail: None,
			event_loop_last_stall_warn_at: None,
			loupe_patch_width_px: 0,
			loupe_patch_height_px: 0,
			egui_repaint_deadline: Arc::new(Mutex::new(None)),
			frozen_capture_session_state: FrozenCaptureSessionState::Inactive,
			frozen_transition: FrozenTransitionRuntime::default(),
			frozen_window_image: None,
			frozen_capture_source: FrozenCaptureSource::None,
			capture_windows_hidden: false,
			#[cfg(target_os = "macos")]
			next_ocr_request_id: 0,
			pending_encode_png: None, pending_png_action: None,
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
			#[cfg(target_os = "macos")] toolbar_window_cursor_hittest_enabled: false,
			live_capture_interaction: LiveCaptureInteraction::Idle,
			frozen_brush: FrozenBrushState::default(), frozen_arrow_drag: FrozenArrowDragState::default(),
			frozen_selection_drag: FrozenSelectionDragState::default(),
			frozen_mosaic_drag: FrozenMosaicDragState::default(), frozen_spotlight_drag: FrozenSpotlightDragState::default(),
			frozen_spotlight_preview_rect: None, frozen_edit_undo_stack: Vec::new(),
			frozen_edit_redo_stack: Vec::new(), frozen_mosaic_undo_stack: Vec::new(), frozen_mosaic_redo_stack: Vec::new(),
			hud_window_visible: false, toolbar_window_visible: false, toolbar_window_drawn_once: false, toolbar_badge_slot_ready: false, skip_toolbar_focus_on_next_show: false,
			#[cfg(target_os = "macos")]
			preserve_frontmost_on_next_toolbar_show: false,
			toolbar_window_warmup_redraws_remaining: 0, loupe_window_visible: false, loupe_window_warmup_redraws_remaining: 0,
			scroll_capture: ScrollCaptureState::default(),
			#[cfg(target_os = "macos")]
			scroll_frame_waker: None,
			#[cfg(target_os = "macos")]
			scroll_capture_host_adapter: None,
			#[cfg(all(test, target_os = "macos"))]
			scroll_capture_start_guard: None,
			#[cfg(all(test, target_os = "macos"))]
			scroll_capture_starting_hook: None,
			#[cfg(all(test, target_os = "macos"))]
			scroll_capture_started_hook: None,
			#[cfg(target_os = "macos")] startup_aux_window_waker: None,
			#[cfg(target_os = "macos")] startup_aux_window_creation_pending: false,
			#[cfg(target_os = "macos")] startup_aux_window_creation_scheduled: false,
			#[cfg(target_os = "macos")] pending_startup_aux_live_stream_filter_upgrade: false,
			response_waker: None,
		}
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
	/// Returns whether the host should register ordinary frozen shortcuts
	/// while the session runs without a focused key window.
	#[must_use]
	pub fn wants_global_frozen_hotkeys(&self) -> bool {
		self.session_active
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& !self.scroll_capture.active
			&& self.frozen_text_edit.is_none()
	}

	#[cfg(target_os = "macos")]
	/// Registers a wake callback that creates non-critical startup windows after first paint.
	pub fn set_startup_aux_window_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
		self.startup_aux_window_waker = Some(waker);
	}

	#[cfg(target_os = "macos")]
	/// Supplies the explicit host-owned scroll-capture capability boundary.
	pub fn set_scroll_capture_host_adapter(&mut self, adapter: ScrollCaptureHostAdapter) {
		self.scroll_capture_host_adapter = Some(adapter);
	}

	#[cfg(target_os = "macos")]
	/// Supplies a reader that replays recorded external scroll input into the session.
	pub fn set_external_scroll_input_drain_reader(
		&mut self,
		reader: ExternalScrollInputDrainReader,
	) {
		self.scroll_capture.external_scroll_input_drain_reader = Some(reader.clone());

		if let Some(adapter) = self.scroll_capture_host_adapter.as_mut() {
			adapter.external_input_drain_reader = reader;
		}
	}

	#[cfg(all(test, target_os = "macos"))]
	/// Test-only compatibility shim for older scroll-capture preflight guards.
	pub fn set_scroll_capture_start_guard(&mut self, guard: ScrollCaptureStartGuard) {
		self.scroll_capture_start_guard = Some(guard);
	}

	#[cfg(all(test, target_os = "macos"))]
	/// Test-only compatibility shim for older scroll-capture start hooks.
	pub fn set_scroll_capture_starting_hook(&mut self, hook: ScrollCaptureStartingHook) {
		self.scroll_capture_starting_hook = Some(hook);
	}

	#[cfg(all(test, target_os = "macos"))]
	/// Test-only compatibility shim for older scroll-capture started hooks.
	pub fn set_scroll_capture_started_hook(&mut self, hook: ScrollCaptureStartedHook) {
		self.scroll_capture_started_hook = Some(hook);
	}

	/// Registers a wake callback for worker-thread responses.
	pub fn set_response_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
		self.response_waker = Some(waker);
	}

	/// Surfaces a host-effect failure on the overlay HUD without ending the session.
	pub fn report_host_effect_error(&mut self, message: impl Into<String>) {
		self.state.set_error(message);
		self.request_redraw_all();
	}

	#[doc(hidden)]
	pub fn debug_prepare_live_test_session(&mut self, monitor: MonitorRect) {
		self.state.mode = OverlayMode::Live;
		self.state.monitor = Some(monitor);
		self.state.cursor = None;
		self.state.hovered_window_rect = None;
		self.state.drag_rect = None;
		self.state.frozen_capture_rect = None;
		self.state.frozen_display_image = None;
		self.state.frozen_export_image = None;
		self.last_event_cursor = None;
		self.last_event_cursor_at = None;
		self.live_capture_interaction = LiveCaptureInteraction::Idle;
		self.capture_windows_hidden = false;
	}

	#[doc(hidden)]
	pub fn debug_set_window_list_snapshot(&mut self, snapshot: Arc<WindowListSnapshot>) {
		self.window_list_snapshot = Some(snapshot);
	}

	#[cfg(target_os = "macos")]
	#[doc(hidden)]
	pub fn debug_seed_macos_live_stream_snapshot(
		&mut self,
		monitor: MonitorRect,
		captured_at: Instant,
	) {
		let stream = self.live_sample_stream.get_or_insert_with(MacLiveFrameStream::new);

		stream.debug_set_self_capture_filter_complete(monitor.id, true);
		stream.debug_store_test_snapshot(monitor, captured_at);
	}

	#[doc(hidden)]
	pub fn debug_prepare_frozen_text_test_session(
		&mut self,
		monitor: MonitorRect,
		capture_rect: RectPoints,
		cursor: GlobalPoint,
	) {
		self.debug_prepare_live_test_session(monitor);

		self.session_active = true;

		self.state.begin_freeze(monitor);

		self.state.frozen_capture_rect = Some(capture_rect);

		let frozen_image = RgbaImage::from_pixel(
			capture_rect.width.max(1),
			capture_rect.height.max(1),
			Rgba([0, 0, 0, 255]),
		);

		self.state.commit_frozen_display_image(monitor, frozen_image.clone());
		self.state.commit_frozen_export_image(frozen_image);

		self.toolbar_state.selected_tool = FrozenToolbarTool::Text;
		self.toolbar_state.visible = true;
		self.toolbar_window_visible = true;

		let _ = self.begin_frozen_text_edit_at(monitor, cursor);
	}

	#[doc(hidden)]
	pub fn debug_cursor(&self) -> Option<GlobalPoint> {
		self.state.cursor
	}

	#[doc(hidden)]
	pub fn debug_hovered_window_rect(&self) -> Option<(u32, RectPoints)> {
		self.state
			.hovered_window_rect
			.map(|hovered_window_rect| (hovered_window_rect.monitor_id, hovered_window_rect.rect))
	}

	#[doc(hidden)]
	pub fn debug_drag_rect(&self) -> Option<(u32, RectPoints)> {
		self.state.drag_rect.map(|drag_rect| (drag_rect.monitor_id, drag_rect.rect))
	}

	#[doc(hidden)]
	pub fn debug_frozen_capture_rect(&self) -> Option<RectPoints> {
		self.state.frozen_capture_rect
	}

	#[doc(hidden)]
	pub fn debug_is_frozen_mode(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
	}

	#[doc(hidden)]
	pub fn debug_has_frozen_display_image(&self) -> bool {
		self.state.frozen_display_image.is_some()
	}

	#[doc(hidden)]
	pub fn debug_has_frozen_export_image(&self) -> bool {
		self.state.frozen_export_image.is_some()
	}

	#[doc(hidden)]
	pub fn debug_has_frozen_text_edit(&self) -> bool {
		self.frozen_text_edit.is_some()
	}

	#[doc(hidden)]
	pub fn debug_frozen_text_edit_text(&self) -> Option<&str> {
		self.frozen_text_edit.as_ref().map(|edit| edit.text.as_str())
	}

	#[doc(hidden)]
	pub fn debug_frozen_text_ime_preedit(&self) -> Option<&str> {
		self.frozen_text_edit.as_ref().and_then(|edit| edit.ime_preedit.as_deref())
	}

	#[doc(hidden)]
	pub fn debug_frozen_text_annotation_count(&self) -> usize {
		self.frozen_text_annotations.len()
	}

	#[cfg(target_os = "macos")]
	#[doc(hidden)]
	pub fn debug_wants_macos_frozen_text_host_focus(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& self.frozen_text_tool_active()
			&& self.frozen_text_edit.is_some()
	}

	#[doc(hidden)]
	pub fn debug_error_message(&self) -> Option<&str> {
		self.state.error_message.as_deref()
	}

	#[doc(hidden)]
	pub fn debug_capture_windows_hidden(&self) -> bool {
		self.capture_windows_hidden
	}

	#[cfg(target_os = "macos")]
	/// Applies one host-routed passive AppKit capture input event.
	pub fn handle_native_capture_input_event(
		&mut self,
		event: MacOSNativeCaptureInputEvent,
	) -> OverlayControl {
		let now = Instant::now();

		self.maybe_log_event_loop_stall(now);
		self.mark_progress_with_detail(
			OverlayEventLoopPhase::WindowEvent,
			Some("native_capture_input"),
		);

		match event {
			MacOSNativeCaptureInputEvent::OverlayPointerMoved { monitor, global } => {
				self.handle_native_overlay_pointer_moved(monitor, global)
			},
			MacOSNativeCaptureInputEvent::OverlayMouseInput { monitor, global, button, state } => {
				self.maybe_stop_frozen_selection_drag_for_mouse_input(state, button);

				match (state, button) {
					(ElementState::Pressed, MouseButton::Right) => {
						self.cancel_overlay("native_capture_right_click")
					},
					(_, MouseButton::Left) => {
						self.handle_live_overlay_left_mouse_input(monitor, global, state)
					},
					_ => OverlayControl::Continue,
				}
			},
			MacOSNativeCaptureInputEvent::ToolbarPointerMoved {
				monitor,
				local,
				global,
				outer_position,
			} => self.handle_native_toolbar_pointer_moved(
				monitor,
				local,
				global,
				Some(outer_position),
			),
			MacOSNativeCaptureInputEvent::ToolbarPointerLeft => self.handle_toolbar_cursor_left(),
			MacOSNativeCaptureInputEvent::ToolbarMouseInput { button, state } => {
				self.maybe_stop_frozen_selection_drag_for_mouse_input(state, button);

				match (state, button) {
					(ElementState::Pressed, MouseButton::Right) => {
						self.cancel_overlay("native_toolbar_right_click")
					},
					(_, MouseButton::Left) => self.handle_toolbar_mouse_input(state),
					_ => OverlayControl::Continue,
				}
			},
			MacOSNativeCaptureInputEvent::ToolbarScrollWheel { delta } => {
				let delta = match delta {
					MacOSNativeCaptureScrollDelta::Line { x, y } => {
						MouseScrollDelta::LineDelta(x, y)
					},
					MacOSNativeCaptureScrollDelta::Pixel { x, y } => {
						MouseScrollDelta::PixelDelta(PhysicalPosition::new(x, y))
					},
				};

				self.handle_toolbar_mouse_wheel(&delta)
			},
			MacOSNativeCaptureInputEvent::KeyboardInput { monitor: _, event } => {
				self.handle_overlay_keyboard_input_event(&event)
			},
			MacOSNativeCaptureInputEvent::Ime { monitor, event } => {
				self.handle_overlay_ime_event(monitor, &event)
			},
			MacOSNativeCaptureInputEvent::ModifiersChanged { state } => {
				self.handle_modifiers_state_changed(state)
			},
		}
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

	#[cfg(target_os = "macos")]
	/// Applies one host-owned scroll-capture frame back into the core.
	pub fn handle_host_scroll_capture_frame(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
		image: RgbaImage,
	) {
		self.handle_captured_scroll_region(monitor, rect_px, request_id, image);
	}

	#[cfg(target_os = "macos")]
	/// Applies one host-owned "no new frame" result back into the core.
	pub fn handle_host_scroll_capture_no_frame(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
	) {
		self.handle_missing_scroll_region(monitor, rect_px, request_id);
	}

	#[cfg(target_os = "macos")]
	/// Surfaces an explicit host-owned capability failure into the core.
	pub fn report_scroll_capture_capability_error(&mut self, message: impl Into<String>) {
		self.clear_scroll_capture_inflight_request();
		self.scroll_capture_set_error(message.into());
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
	fn clear_live_surface_bg(&mut self) {
		self.state.live_bg_monitor = None;
		self.state.live_bg_image = None;
		self.last_live_surface_bg_snapshot_at = None;
	}

	#[cfg(target_os = "macos")]
	fn sync_live_surface_bg_from_stream(&mut self, monitor: MonitorRect) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			self.clear_live_surface_bg();

			return;
		}
		if self.active_cursor_monitor() != Some(monitor) {
			return;
		}

		let Some(stream) = self.live_sample_stream.as_ref() else {
			return;
		};

		if !stream.self_capture_filter_complete_for_monitor(monitor) {
			return;
		}

		let Some(snapshot) = stream.peek_latest_rgba_snapshot(monitor) else {
			return;
		};
		let allow_refresh = self.frozen_display_handoff_pending();

		if !allow_refresh
			&& self.state.live_bg_monitor == Some(monitor)
			&& self.state.live_bg_image.is_some()
		{
			return;
		}
		if self.state.live_bg_monitor == Some(monitor)
			&& self.last_live_surface_bg_snapshot_at == Some(snapshot.captured_at)
		{
			return;
		}

		self.state.live_bg_monitor = Some(monitor);
		self.state.live_bg_image = Some(snapshot.image.as_ref().clone());
		self.state.live_bg_generation = self.state.live_bg_generation.wrapping_add(1);
		self.last_live_surface_bg_snapshot_at = Some(snapshot.captured_at);
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
		if !self.frozen_capture_redraw_pending() {
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

	fn frozen_capture_redraw_pending(&self) -> bool {
		!self.frozen_display_ready() && self.frozen_capture_export_pending()
	}

	fn frozen_capture_monitor(&self) -> Option<MonitorRect> {
		match self.frozen_capture_session_state {
			FrozenCaptureSessionState::Inactive => None,
			FrozenCaptureSessionState::DisplayPending { monitor, .. }
			| FrozenCaptureSessionState::DisplayFailed { monitor }
			| FrozenCaptureSessionState::DisplayReady { monitor, .. } => Some(monitor),
		}
	}

	fn frozen_capture_window_target(&self) -> Option<WindowFreezeCaptureTarget> {
		match self.frozen_capture_session_state {
			FrozenCaptureSessionState::DisplayPending { window_target, .. } => window_target,
			FrozenCaptureSessionState::DisplayReady {
				export: FrozenExportSessionState::Pending { window_target, .. },
				..
			} => window_target,
			FrozenCaptureSessionState::Inactive
			| FrozenCaptureSessionState::DisplayFailed { .. }
			| FrozenCaptureSessionState::DisplayReady { .. } => None,
		}
	}

	fn frozen_capture_worker_state(&self) -> Option<FrozenCaptureWorkerState> {
		match self.frozen_capture_session_state {
			FrozenCaptureSessionState::DisplayPending { worker_state, .. } => Some(worker_state),
			FrozenCaptureSessionState::DisplayReady {
				export: FrozenExportSessionState::Pending { worker_state, .. },
				..
			} => Some(worker_state),
			FrozenCaptureSessionState::Inactive
			| FrozenCaptureSessionState::DisplayFailed { .. }
			| FrozenCaptureSessionState::DisplayReady { .. } => None,
		}
	}

	fn frozen_capture_export_pending(&self) -> bool {
		matches!(
			self.frozen_capture_session_state,
			FrozenCaptureSessionState::DisplayPending { .. }
				| FrozenCaptureSessionState::DisplayReady {
					export: FrozenExportSessionState::Pending { .. },
					..
				}
		)
	}

	fn frozen_capture_export_ready(&self) -> bool {
		matches!(
			self.frozen_capture_session_state,
			FrozenCaptureSessionState::DisplayReady { export: FrozenExportSessionState::Ready, .. }
		)
	}

	fn frozen_capture_dispatch_pending(&self) -> bool {
		matches!(
			self.frozen_capture_session_state,
			FrozenCaptureSessionState::DisplayPending {
				worker_state: FrozenCaptureWorkerState::Idle | FrozenCaptureWorkerState::Armed,
				..
			} | FrozenCaptureSessionState::DisplayReady {
				export: FrozenExportSessionState::Pending {
					worker_state: FrozenCaptureWorkerState::Idle | FrozenCaptureWorkerState::Armed,
					..
				},
				..
			}
		)
	}

	fn frozen_capture_worker_armed(&self) -> bool {
		self.frozen_capture_worker_state() == Some(FrozenCaptureWorkerState::Armed)
	}

	fn frozen_capture_worker_inflight(&self) -> bool {
		self.frozen_capture_worker_state() == Some(FrozenCaptureWorkerState::Inflight)
	}

	fn set_frozen_capture_display_pending(
		&mut self,
		monitor: MonitorRect,
		worker_state: FrozenCaptureWorkerState,
		window_target: Option<WindowFreezeCaptureTarget>,
	) {
		self.frozen_capture_session_state =
			FrozenCaptureSessionState::DisplayPending { monitor, worker_state, window_target };
	}

	fn frozen_display_handoff_pending(&self) -> bool {
		matches!(
			self.frozen_capture_session_state,
			FrozenCaptureSessionState::DisplayPending { .. }
		) && !matches!(self.state.mode, OverlayMode::Frozen)
	}

	fn commit_first_frozen_display_handoff(&mut self, monitor: MonitorRect) {
		if matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}

		self.state.begin_freeze(monitor);

		self.state.drag_rect = None;
		self.state.hovered_window_rect = None;
	}

	fn promote_frozen_capture_display_ready(&mut self, monitor: MonitorRect) {
		let export = match self.frozen_capture_session_state {
			FrozenCaptureSessionState::DisplayPending { worker_state, window_target, .. } => {
				FrozenExportSessionState::Pending { worker_state, window_target }
			},
			FrozenCaptureSessionState::DisplayReady { export, .. } => export,
			FrozenCaptureSessionState::DisplayFailed { .. }
			| FrozenCaptureSessionState::Inactive => FrozenExportSessionState::Pending {
				worker_state: FrozenCaptureWorkerState::Idle,
				window_target: None,
			},
		};

		self.frozen_capture_session_state =
			FrozenCaptureSessionState::DisplayReady { monitor, export };
	}

	fn set_frozen_capture_worker_state(&mut self, worker_state: FrozenCaptureWorkerState) {
		self.frozen_capture_session_state = match self.frozen_capture_session_state {
			FrozenCaptureSessionState::DisplayPending { monitor, window_target, .. } => {
				FrozenCaptureSessionState::DisplayPending { monitor, worker_state, window_target }
			},
			FrozenCaptureSessionState::DisplayReady {
				monitor,
				export: FrozenExportSessionState::Pending { window_target, .. },
			} => FrozenCaptureSessionState::DisplayReady {
				monitor,
				export: FrozenExportSessionState::Pending { worker_state, window_target },
			},
			other => other,
		};
	}

	fn set_frozen_capture_export_ready(&mut self, monitor: MonitorRect) {
		self.frozen_capture_session_state = FrozenCaptureSessionState::DisplayReady {
			monitor,
			export: FrozenExportSessionState::Ready,
		};
	}

	fn set_frozen_capture_export_failed(&mut self, monitor: MonitorRect) {
		self.frozen_capture_session_state = match self.frozen_capture_session_state {
			FrozenCaptureSessionState::DisplayReady { .. } => {
				FrozenCaptureSessionState::DisplayReady {
					monitor,
					export: FrozenExportSessionState::Failed,
				}
			},
			FrozenCaptureSessionState::DisplayPending { .. }
			| FrozenCaptureSessionState::DisplayFailed { .. }
			| FrozenCaptureSessionState::Inactive => FrozenCaptureSessionState::DisplayFailed { monitor },
		};
	}

	fn clear_frozen_capture_session_state(&mut self) {
		self.frozen_capture_session_state = FrozenCaptureSessionState::Inactive;
	}

	fn frozen_display_ready(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& matches!(
				self.frozen_capture_session_state,
				FrozenCaptureSessionState::DisplayReady { .. }
			) && self.state.frozen_display_surface_image().is_some()
	}

	fn frozen_display_ready_for_monitor(&self, monitor: MonitorRect) -> bool {
		self.frozen_display_ready() && self.state.monitor == Some(monitor)
	}

	fn frozen_visual_handoff_pending_for_monitor(&self, monitor: MonitorRect) -> bool {
		let _ = monitor;

		false
	}

	fn frozen_preview_visible(&self) -> bool {
		self.frozen_display_ready()
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
				|| !self.frozen_display_ready()
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
		self.frozen_capture_monitor() == Some(monitor) && self.frozen_capture_dispatch_pending()
	}

	#[cfg(target_os = "macos")]
	fn should_dispatch_pending_freeze_capture(&self, monitor: MonitorRect) -> bool {
		self.pending_freeze_capture_matches(monitor)
	}

	#[cfg(not(target_os = "macos"))]
	fn should_dispatch_pending_freeze_capture(&self, monitor: MonitorRect) -> bool {
		self.pending_freeze_capture_matches(monitor) && !self.frozen_preview_visible()
	}

	fn frozen_final_capture_ready(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& self.frozen_capture_export_ready()
			&& self.state.frozen_export_image.is_some()
	}

	fn pending_window_freeze_capture_for_monitor(
		&self,
		monitor: MonitorRect,
	) -> Option<WindowFreezeCaptureTarget> {
		self.frozen_capture_window_target().filter(|target| target.monitor == monitor)
	}

	#[cfg(target_os = "macos")]
	fn commit_frozen_preview(
		&mut self,
		monitor: MonitorRect,
		image: RgbaImage,
		cursor: Option<GlobalPoint>,
	) {
		self.commit_first_frozen_display_handoff(monitor);
		self.state.commit_frozen_display_image(monitor, image);
		self.promote_frozen_capture_display_ready(monitor);

		if let Some(cursor) = cursor {
			self.update_cursor_state(monitor, cursor);
		}

		self.sync_overlay_cursor_icons();
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
		self.toolbar_window_drawn_once = false;
		self.toolbar_badge_slot_ready = false;
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
		self.set_frozen_capture_display_pending(
			monitor,
			FrozenCaptureWorkerState::Idle,
			window_target,
		);

		self.freeze_capture_send_full_count = 0;
		self.frozen_window_image = None;
		self.capture_windows_hidden = false;
		self.pending_click_hit_test_request_id = None;
		self.pending_click_hit_test_requested_at = None;

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
		self.begin_frozen_transition_timing(monitor, capture_rect, window_target);

		self.state.frozen_capture_rect = Some(capture_rect);
		self.state.frozen_mosaic_preview_rect = None;

		self.reset_frozen_annotation_state();

		self.skip_toolbar_focus_on_next_show = true;
		#[cfg(target_os = "macos")]
		{
			// Keep Rsnap active for the entire overlay session so AppKit continues to honor native
			// crosshair / grab / resize cursors. The pre-capture frontmost app is restored on exit.
			self.preserve_frontmost_on_next_toolbar_show = false;
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
		if self.begin_frozen_capture_with_rect_macos(monitor, window_target, cursor) {
			return;
		}

		#[cfg(not(target_os = "macos"))]
		self.begin_frozen_capture_with_rect_non_macos(monitor, window_target, cursor);
		// Do not request the first frozen redraw until the session has either committed a preview or
		// started the asynchronous export-authority path. Otherwise the overlay can briefly present
		// an empty black frozen frame before the real preview arrives.
		self.refresh_frozen_helper_windows_for_transition(monitor);
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

			self.commit_first_frozen_display_handoff(monitor);
			self.state.commit_frozen_final_image(monitor, image);
			self.note_frozen_transition_preview_committed(monitor, "cached_live_background", None);
			self.promote_frozen_capture_display_ready(monitor);
			self.set_frozen_capture_export_ready(monitor);
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
		let export_image = self.state.frozen_export_image.as_ref()?;
		let x = capture_rect_pixels.x.min(export_image.width());
		let y = capture_rect_pixels.y.min(export_image.height());
		let max_width = export_image.width().saturating_sub(x);
		let max_height = export_image.height().saturating_sub(y);
		let width = capture_rect_pixels.width.min(max_width);
		let height = capture_rect_pixels.height.min(max_height);

		if width == 0 || height == 0 {
			tracing::debug!(
				monitor_id = monitor.id,
				capture_rect_pixels = ?capture_rect_pixels,
				export_image_size = ?(export_image.width(), export_image.height()),
				"Scroll capture base-frame crop resolved to an empty region."
			);

			None
		} else {
			Some(imageops::crop_imm(export_image, x, y, width, height).to_image())
		}
	}

	fn note_frozen_image_mutated(&mut self, monitor: MonitorRect) {
		self.state.frozen_generation = self.state.frozen_generation.wrapping_add(1);

		self.sync_frozen_toolbar_state();
		self.request_redraw_for_monitor(monitor);
		self.request_redraw_toolbar_window();
	}

	fn handle_captured_freeze_response(
		&mut self,
		monitor: MonitorRect,
		image: RgbaImage,
		window_image: Option<RgbaImage>,
		captured_window_id: Option<u32>,
	) {
		if self.frozen_capture_monitor() == Some(monitor) && self.frozen_capture_export_pending() {
			let window_capture_target = self.frozen_capture_window_target();
			let had_display_image = self.frozen_display_ready();
			let frozen_preview_image = image;

			self.frozen_window_image = None;

			if self.reject_dirty_window_export_authority(
				monitor,
				window_capture_target,
				window_image.is_some(),
				captured_window_id,
			) {
				self.restore_capture_windows_visibility();

				return;
			}

			let frozen_preview_image = self.apply_window_capture_export_authority(
				monitor,
				had_display_image,
				frozen_preview_image,
				window_capture_target,
				window_image,
				captured_window_id,
			);

			if !had_display_image {
				self.commit_first_frozen_display_handoff(monitor);
				self.state.commit_frozen_display_image(monitor, frozen_preview_image.clone());
				self.promote_frozen_capture_display_ready(monitor);
				self.note_frozen_transition_preview_committed(
					monitor,
					"authoritative_capture",
					None,
				);
			}

			self.state.commit_frozen_export_image(frozen_preview_image.clone());
			self.set_frozen_capture_export_ready(monitor);
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
		if self.frozen_capture_worker_inflight() && self.frozen_capture_monitor() == Some(monitor) {
			self.clear_frozen_capture_session_state();
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

	fn reject_dirty_window_export_authority(
		&mut self,
		monitor: MonitorRect,
		window_capture_target: Option<WindowFreezeCaptureTarget>,
		window_image_present: bool,
		captured_window_id: Option<u32>,
	) -> bool {
		let Some(target) = window_capture_target else {
			return false;
		};

		if target.monitor != monitor
			|| !matches!(
				self.config.window_capture_alpha_mode,
				WindowCaptureAlphaMode::MatteLight | WindowCaptureAlphaMode::MatteDark
			) || (captured_window_id == Some(target.window_id) && window_image_present)
		{
			return false;
		}

		self.set_frozen_capture_export_failed(monitor);
		self.note_frozen_transition_aborted(
			"Window export authority did not resolve to a clean target window.",
		);
		self.state.set_error("Window capture is unavailable. Please try again.");

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();
		self.request_redraw_for_monitor(monitor);

		true
	}

	fn apply_window_capture_export_authority(
		&mut self,
		monitor: MonitorRect,
		had_display_image: bool,
		base_image: RgbaImage,
		window_capture_target: Option<WindowFreezeCaptureTarget>,
		window_image: Option<RgbaImage>,
		captured_window_id: Option<u32>,
	) -> RgbaImage {
		let Some((target, window_capture_image, window_id)) = window_capture_target
			.zip(window_image)
			.zip(captured_window_id)
			.map(|((target, window_capture_image), window_id)| {
				(target, window_capture_image, window_id)
			})
		else {
			return base_image;
		};

		if target.monitor != monitor || target.window_id != window_id {
			return base_image;
		}

		match self.config.window_capture_alpha_mode {
			WindowCaptureAlphaMode::Background => base_image,
			WindowCaptureAlphaMode::MatteLight | WindowCaptureAlphaMode::MatteDark => {
				let base_image = if had_display_image {
					self.state.frozen_display_image.clone().unwrap_or(base_image)
				} else {
					base_image
				};
				let window_capture_image = Self::compose_window_preview_layer(
					&window_capture_image,
					self.config.window_capture_alpha_mode,
				);
				let preview_image = Self::composite_window_capture_preview(
					base_image,
					&window_capture_image,
					monitor,
					target.rect,
					WindowCaptureAlphaMode::Background,
				);

				self.frozen_window_image = Some(window_capture_image);

				preview_image
			},
		}
	}

	fn handle_encoded_png_response(&mut self, png_bytes: Vec<u8>) -> OverlayControl {
		let Some(action) = self.pending_png_action.take() else {
			return OverlayControl::Continue;
		};

		match action {
			PngAction::Copy => {
				OverlayControl::HostEffect(PreparedHostEffectRequest::CopyPng { png_bytes })
			},
			PngAction::Save => OverlayControl::HostEffect(PreparedHostEffectRequest::SavePng {
				png_bytes,
				output_dir: self.config.output_dir.clone(),
				output_filename_prefix: self.config.output_filename_prefix.clone(),
				output_naming: self.config.output_naming,
			}),
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
			#[cfg(target_os = "macos")]
			WindowEvent::Ime(_) => OverlayControl::Continue,
			#[cfg(not(target_os = "macos"))]
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
			#[cfg(target_os = "macos")]
			WindowEvent::KeyboardInput { .. } => OverlayControl::Continue,
			#[cfg(not(target_os = "macos"))]
			WindowEvent::KeyboardInput { event, .. } => self.handle_key_event(event),
			#[cfg(target_os = "macos")]
			WindowEvent::ModifiersChanged(_) => OverlayControl::Continue,
			#[cfg(not(target_os = "macos"))]
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
		let requested_at_unix_ms = current_unix_millis();

		if self.scroll_capture.active {
			let image = self.scroll_capture.session.as_ref()?.export_image().clone();

			return Some(DeferredTextRecognitionRequest::prepared(
				request_id,
				requested_at_unix_ms,
				image,
			));
		}
		if self.frozen_capture_source == FrozenCaptureSource::Window {
			match self.config.window_capture_alpha_mode {
				WindowCaptureAlphaMode::Background => {},
				WindowCaptureAlphaMode::MatteLight => {
					if let Some(window_image) = self.frozen_window_image.take() {
						return Some(DeferredTextRecognitionRequest::prepared(
							request_id,
							requested_at_unix_ms,
							window_image,
						));
					}
				},
				WindowCaptureAlphaMode::MatteDark => {
					if let Some(window_image) = self.frozen_window_image.take() {
						return Some(DeferredTextRecognitionRequest::prepared(
							request_id,
							requested_at_unix_ms,
							window_image,
						));
					}
				},
			}
		}

		let crop_rect = self.deferred_text_recognition_crop_rect_pixels()?;
		let export_image = self.state.frozen_export_image.take()?;

		Some(DeferredTextRecognitionRequest::frozen_crop(
			request_id,
			requested_at_unix_ms,
			export_image,
			crop_rect,
		))
	}

	#[cfg(target_os = "macos")]
	fn deferred_text_recognition_crop_rect_pixels(&self) -> Option<Option<RectPoints>> {
		let export_image = self.state.frozen_export_image.as_ref()?;
		let Some(monitor) = self.state.monitor else {
			return Some(None);
		};
		let capture_rect = self
			.state
			.frozen_capture_rect
			.unwrap_or_else(|| RectPoints::new(0, 0, monitor.width, monitor.height));
		let capture_rect = monitor.local_rect_to_pixels(capture_rect);
		let x = capture_rect.x.min(export_image.width());
		let y = capture_rect.y.min(export_image.height());
		let max_width = export_image.width().saturating_sub(x);
		let max_height = export_image.height().saturating_sub(y);
		let width = capture_rect.width.min(max_width);
		let height = capture_rect.height.min(max_height);

		if width == 0 || height == 0 {
			return None;
		}
		if x == 0 && y == 0 && width == export_image.width() && height == export_image.height() {
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

		OverlayControl::HostEffect(PreparedHostEffectRequest::DeferredTextRecognition(request))
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
		tracing::info!(
			op = "overlay.live_focus_requested",
			target = "native_passive_shell",
			window_count = self.windows.len(),
			"Skipped live capture key focus because passive AppKit shells own live pointer input."
		);
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

		if toolbar_layout_model::frozen_toolbar_matches_default_slot(
			toolbar_pos,
			previous_default_pos,
		) {
			self.toolbar_state.floating_position = Some(current_default_pos);

			self.sync_frozen_annotation_style_capsule_placement(monitor);

			return !toolbar_layout_model::frozen_toolbar_matches_default_slot(
				toolbar_pos,
				current_default_pos,
			);
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

			ready && self.toolbar_window_drawn_once && self.toolbar_badge_slot_ready
		}

		#[cfg(not(target_os = "macos"))]
		{
			toolbar_visible_for_badge && self.frozen_toolbar_ready_for_draw(overlay_screen_rect)
		}
	}

	fn pending_frozen_display_handoff_state(
		&self,
		overlay_monitor: MonitorRect,
	) -> (bool, Option<MonitorRect>) {
		let pending_frozen_display_handoff = self.frozen_display_handoff_pending()
			|| self.frozen_visual_handoff_pending_for_monitor(overlay_monitor);
		let pending_frozen_display_handoff_monitor =
			self.frozen_capture_monitor().filter(|_| pending_frozen_display_handoff);

		(pending_frozen_display_handoff, pending_frozen_display_handoff_monitor)
	}

	fn allow_frozen_surface_bg_for_overlay_monitor(
		&self,
		overlay_monitor: MonitorRect,
		scroll_capture_active: bool,
	) -> bool {
		!scroll_capture_active
			&& !self.frozen_display_handoff_pending()
			&& !self.frozen_visual_handoff_pending_for_monitor(overlay_monitor)
	}

	fn should_draw_live_surface_bg_for_overlay_monitor(
		&self,
		overlay_monitor: MonitorRect,
	) -> bool {
		matches!(self.state.mode, OverlayMode::Live)
			&& self.state.live_bg_monitor == Some(overlay_monitor)
			&& self.state.live_bg_image.is_some()
	}

	fn selection_flow_enabled_for_overlay_draw(&self) -> bool {
		self.config.selection_flow_enabled
	}

	#[cfg(target_os = "macos")]
	fn should_refresh_live_surface_bg_for_overlay_monitor(
		&self,
		overlay_monitor: MonitorRect,
	) -> bool {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return false;
		}
		if self.active_cursor_monitor() != Some(overlay_monitor) {
			return false;
		}
		if self.frozen_display_handoff_pending()
			|| self.frozen_visual_handoff_pending_for_monitor(overlay_monitor)
		{
			return true;
		}

		self.state.live_bg_monitor != Some(overlay_monitor) || self.state.live_bg_image.is_none()
	}

	fn mark_overlay_window_redraw_progress(
		&mut self,
		window_id: WindowId,
		overlay_monitor: MonitorRect,
	) {
		self.sync_overlay_cursor_icons();
		self.sync_frozen_toolbar_state();

		self.event_loop_last_progress_window_id = Some(window_id);
		self.event_loop_last_progress_monitor_id = Some(overlay_monitor.id);
	}

	#[cfg(target_os = "macos")]
	fn maybe_sync_live_surface_bg_for_overlay_redraw(&mut self, overlay_monitor: MonitorRect) {
		if self.should_refresh_live_surface_bg_for_overlay_monitor(overlay_monitor) {
			self.sync_live_surface_bg_from_stream(overlay_monitor);
		}
	}

	#[cfg(not(target_os = "macos"))]
	fn maybe_sync_live_surface_bg_for_overlay_redraw(&mut self, _overlay_monitor: MonitorRect) {}

	fn finish_overlay_window_redraw(
		&mut self,
		overlay_monitor: MonitorRect,
		draw_toolbar: bool,
	) -> OverlayControl {
		self.maybe_arm_frozen_toolbar_badge_slot_after_overlay_draw(overlay_monitor);

		self.last_present_at = Instant::now();

		self.note_startup_overlay_frame_presented();

		self.handle_capture_and_toolbar_redraw_post(overlay_monitor, draw_toolbar)
	}

	fn handle_overlay_window_redraw(&mut self, window_id: WindowId) -> OverlayControl {
		let Some(overlay_monitor) = self.windows.get(&window_id).map(|overlay| overlay.monitor)
		else {
			return OverlayControl::Continue;
		};

		self.mark_overlay_window_redraw_progress(window_id, overlay_monitor);
		self.maybe_log_event_loop_stall(Instant::now());
		self.mark_progress(OverlayEventLoopPhase::OverlayRedraw);
		self.maybe_sync_live_surface_bg_for_overlay_redraw(overlay_monitor);

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
		let (scroll_capture_active, frozen_text_style) =
			(self.scroll_capture.active, self.toolbar_state.text_style);
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
		let (pending_frozen_display_handoff, pending_frozen_display_handoff_monitor) =
			self.pending_frozen_display_handoff_state(overlay_monitor);
		let allow_frozen_surface_bg = self
			.allow_frozen_surface_bg_for_overlay_monitor(overlay_monitor, scroll_capture_active);
		let allow_live_surface_bg =
			self.should_draw_live_surface_bg_for_overlay_monitor(overlay_monitor);
		let selection_flow_enabled = self.selection_flow_enabled_for_overlay_draw();
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
				selection_flow_enabled,
				self.config.selection_flow_stroke_width_px,
				allow_frozen_surface_bg,
				allow_live_surface_bg,
				pending_frozen_display_handoff,
				pending_frozen_display_handoff_monitor,
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

		self.finish_overlay_window_redraw(overlay_monitor, draw_toolbar)
	}

	#[cfg(target_os = "macos")]
	fn maybe_arm_frozen_toolbar_badge_slot_after_overlay_draw(
		&mut self,
		overlay_monitor: MonitorRect,
	) {
		if self.toolbar_badge_slot_ready
			|| !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.state.monitor != Some(overlay_monitor)
			|| !self.toolbar_window_visible
			|| !self.toolbar_window_drawn_once
		{
			return;
		}

		self.toolbar_badge_slot_ready = true;

		self.note_frozen_transition_badge_slot_armed(overlay_monitor);
		self.request_redraw_for_monitor(overlay_monitor);
	}

	#[cfg(not(target_os = "macos"))]
	fn maybe_arm_frozen_toolbar_badge_slot_after_overlay_draw(
		&mut self,
		_overlay_monitor: MonitorRect,
	) {
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
			final_capture_ready = self.frozen_final_capture_ready(),
			frozen_image_ready = self.frozen_display_ready(),
			frozen_capture_session_state = ?self.frozen_capture_session_state,
			pending_freeze_capture = self
				.frozen_capture_monitor()
				.filter(|_| self.frozen_capture_export_pending())
				.map(|m| m.id),
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
		toolbar_layout_model::advance_frozen_toolbar_readiness_sample_state(
			&mut self.toolbar_state,
			screen_rect,
		)
	}

	#[cfg(any(not(target_os = "macos"), test))]
	fn frozen_toolbar_ready_for_draw(&self, screen_rect: Rect) -> bool {
		let screen_size_points = screen_rect.size();
		let needs_new_sample = toolbar_layout_model::frozen_toolbar_needs_new_sample(
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
			let pending_window_target =
				self.pending_window_freeze_capture_for_monitor(overlay_monitor);
			let freeze_target = pending_window_target
				.map_or(FreezeCaptureTarget::Monitor, |target| FreezeCaptureTarget::Window {
					window_id: target.window_id,
				});
			#[cfg(target_os = "macos")]
			let _ = (&freeze_target, &pending_window_target, &overlay_monitor);

			#[cfg(not(target_os = "macos"))]
			{
				// Capture must happen on a post-hide redraw so the HUD/loupe are not included.
				if self.frozen_capture_worker_armed() {
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

					self.set_frozen_capture_worker_state(FrozenCaptureWorkerState::Armed);
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
}

impl Default for OverlaySession {
	fn default() -> Self {
		Self::new()
	}
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

#[derive(Clone, Copy, Debug)]
struct FrozenArrowGeometry {
	shaft_end: Pos2,
	tip: Pos2,
	head_left: Pos2,
	head_right: Pos2,
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

fn global_to_local(cursor: GlobalPoint, monitor: MonitorRect) -> Option<Pos2> {
	let (x, y) = monitor.local_u32(cursor)?;

	Some(Pos2::new(x as f32, y as f32))
}

#[cfg(target_os = "macos")]
fn current_unix_millis() -> u64 {
	match SystemTime::now().duration_since(UNIX_EPOCH) {
		Ok(duration) => duration.as_millis().try_into().unwrap_or(u64::MAX),
		Err(_err) => 0,
	}
}

#[cfg(test)]
mod tests;
