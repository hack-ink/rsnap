use std::time::Instant;

use winit::keyboard::ModifiersState;

use crate::overlay::{
	FrozenCaptureSource, OverlayControl, OverlayEventLoopPhase, OverlayExit, OverlaySession,
	ScrollCaptureState,
};
use rsnap_capture_core::PreparedHostEffectRequest;

#[derive(Debug)]
struct OverlayExitMetadata<'a> {
	exit_kind: &'static str,
	host_effect_kind: Option<&'static str>,
	png_bytes_len: Option<usize>,
	error_message: Option<&'a str>,
	ocr_request_id: Option<u64>,
}
impl<'a> OverlayExitMetadata<'a> {
	fn new(exit_kind: &'static str) -> Self {
		Self {
			exit_kind,
			host_effect_kind: None,
			png_bytes_len: None,
			error_message: None,
			ocr_request_id: None,
		}
	}

	fn with_host_effect_kind(mut self, host_effect_kind: &'static str) -> Self {
		self.host_effect_kind = Some(host_effect_kind);

		self
	}

	fn with_png_bytes_len(mut self, png_bytes_len: usize) -> Self {
		self.png_bytes_len = Some(png_bytes_len);

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

impl OverlaySession {
	/// Completes a host-owned effect request and finalizes overlay exit cleanup.
	pub fn complete_host_effect_request(&mut self, request: &PreparedHostEffectRequest) {
		let exit_metadata = Self::host_effect_exit_metadata(request);

		self.log_exit_begin(&exit_metadata);
		self.finalize_scroll_capture_for_exit();
		self.reset_runtime_for_exit();
		self.log_exit_end(&exit_metadata);
	}

	pub(super) fn cancel_overlay(&mut self, reason: &'static str) -> OverlayControl {
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

	pub(super) fn exit(&mut self, exit: OverlayExit) -> OverlayControl {
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
			OverlayExit::HostEffect(request) => Self::host_effect_exit_metadata(request),
			OverlayExit::Error(message) => {
				OverlayExitMetadata::new("error").with_error_message(message.as_str())
			},
		}
	}

	fn host_effect_exit_metadata(request: &PreparedHostEffectRequest) -> OverlayExitMetadata<'_> {
		match request {
			PreparedHostEffectRequest::CopyPng { png_bytes } => {
				OverlayExitMetadata::new("host_effect")
					.with_host_effect_kind("copy_png")
					.with_png_bytes_len(png_bytes.len())
			},
			PreparedHostEffectRequest::SavePng { png_bytes, .. } => {
				OverlayExitMetadata::new("host_effect")
					.with_host_effect_kind("save_png")
					.with_png_bytes_len(png_bytes.len())
			},
			#[cfg(target_os = "macos")]
			PreparedHostEffectRequest::DeferredTextRecognition(request) => {
				OverlayExitMetadata::new("host_effect")
					.with_host_effect_kind("deferred_text_recognition")
					.with_ocr_request_id(request.request_id)
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
			host_effect_kind = exit_metadata.host_effect_kind,
			png_bytes_len = exit_metadata.png_bytes_len,
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
		if self.scroll_capture.active
			&& let Some(host_adapter) = self.scroll_capture_host_adapter.as_ref()
		{
			(host_adapter.stop)();
		}

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
		self.toolbar_window_drawn_once = false;
		self.toolbar_badge_slot_ready = false;

		self.frozen_transition.clear_exit_window_runtime();

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
		self.pending_click_hit_test_request_id = None;
		self.pending_click_hit_test_requested_at = None;

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
			host_effect_kind = exit_metadata.host_effect_kind,
			png_bytes_len = exit_metadata.png_bytes_len,
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
