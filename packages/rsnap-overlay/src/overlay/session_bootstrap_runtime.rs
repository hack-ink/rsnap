#[cfg(not(target_os = "macos"))]
use std::env;
#[cfg(not(target_os = "macos"))]
use std::panic;
use std::{
	collections::{HashMap, HashSet},
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

#[cfg(not(target_os = "macos"))]
use device_query::DeviceState;
use winit::keyboard::ModifiersState;

use crate::overlay::frozen_transition_runtime::FrozenTransitionRuntime;
use crate::overlay::runtime_model::{
	FrozenCaptureSource, LiveCaptureInteraction, OverlayEventLoopPhase,
};
use crate::overlay::runtime_timing::{CURSOR_POLL_INTERVAL_MIN, LIVE_WINDOW_LIST_REFRESH_INTERVAL};
use crate::overlay::session_state::{
	FrozenArrowDragState, FrozenBrushState, FrozenCaptureSessionState, FrozenMosaicDragState,
	FrozenSelectionDragState, FrozenSpotlightDragState, FrozenToolbarState, ScrollCaptureState,
	SlowOperationLogger,
};
use crate::overlay::{OverlayConfig, OverlaySession, OverlayState};

struct InitialSessionRuntime {
	live_bg_request_interval: Duration,
	window_list_refresh_interval: Duration,
	now: Instant,
	loupe_sample_side_px: u32,
	state: OverlayState,
}

impl OverlaySession {
	#[cfg(not(target_os = "macos"))]
	fn try_create_cursor_device() -> Option<DeviceState> {
		let has_display =
			env::var_os("DISPLAY").is_some() || env::var_os("WAYLAND_DISPLAY").is_some();

		if !has_display {
			tracing::warn!(
				op = "overlay.cursor_device_unavailable",
				"Skipping cursor-device initialization because no display server is available."
			);

			return None;
		}

		match panic::catch_unwind(DeviceState::new) {
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
		#[cfg(not(target_os = "macos"))] cursor_device: Option<DeviceState>,
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
		#[cfg(not(target_os = "macos"))] cursor_device: Option<DeviceState>,
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

	fn initial_timing() -> (Duration, Duration, Instant) {
		(Duration::from_millis(500), LIVE_WINDOW_LIST_REFRESH_INTERVAL, Instant::now())
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
}
