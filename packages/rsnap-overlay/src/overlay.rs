pub(crate) mod replay_support;

mod aux_window_runtime;
mod capture_window_runtime;
mod config_runtime;
mod coordinate_geometry;
mod cursor_context_runtime;
mod cursor_icon_runtime;
mod cursor_runtime;
mod exit_runtime;
mod frozen_arrow_runtime;
mod frozen_auto_center_runtime;
mod frozen_brush_runtime;
mod frozen_capture_backend_adapter;
mod frozen_capture_handoff_runtime;
mod frozen_capture_session_runtime;
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
mod keyboard_input_runtime;
mod live_capture_target;
mod loupe_input_runtime;
#[cfg(target_os = "macos")]
mod macos_capture_host;
#[cfg(target_os = "macos")]
mod macos_cursor_runtime;
#[cfg(target_os = "macos")]
mod macos_native_capture_shell_runtime;
#[cfg(target_os = "macos")]
mod macos_window_bridge;
mod output_action_runtime;
mod redraw_runtime;
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
mod toolbar_input_runtime;
mod toolbar_layout_model;
mod toolbar_runtime;
mod trace_recording;
mod window_content_policy;
mod window_event_runtime;
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

use std::mem;
use std::ptr;
use std::slice;
use std::{
	borrow::Cow,
	collections::{HashMap, HashSet},
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Report, WrapErr};
#[cfg(not(target_os = "macos"))]
use device_query::DeviceState;
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
	SelectionFlowStyle, WindowRendererPath,
};
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
		WindowListSnapshot,
	},
	worker::{
		FreezeCaptureTarget, OverlayWorker, WorkerErrorSource, WorkerRequestSendError,
		WorkerResponse,
	},
};
#[cfg(test)]
use rsnap_capture_core::OutputNaming;

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
	cursor_device: Option<DeviceState>,
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

	fn note_frozen_image_mutated(&mut self, monitor: MonitorRect) {
		self.state.frozen_generation = self.state.frozen_generation.wrapping_add(1);

		self.sync_frozen_toolbar_state();
		self.request_redraw_for_monitor(monitor);
		self.request_redraw_toolbar_window();
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

		let cursor_local = toolbar_cursor_local_override.or_else(|| {
			self.state
				.cursor
				.and_then(|cursor| coordinate_geometry::global_to_local(cursor, monitor))
		})?;

		Some(FrozenToolbarPointerState {
			cursor_local,
			#[cfg(not(target_os = "macos"))]
			left_button_down,
			left_button_went_down,
			left_button_went_up,
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

#[cfg(test)]
mod tests;
