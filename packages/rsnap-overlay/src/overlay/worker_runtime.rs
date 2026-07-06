mod live_sampling;

#[cfg(target_os = "macos")]
use crate::overlay::mem;
use crate::overlay::runtime_timing::PENDING_CLICK_HIT_TEST_TIMEOUT;
use crate::overlay::{
	CapturedMonitorRegionResult, FrozenCaptureWorkerState, Instant, LiveCaptureInteraction,
	MonitorRect, OverlayControl, OverlayMode, OverlaySession, WindowFreezeCaptureTarget,
	WorkerErrorSource, WorkerRequestSendError, WorkerResponse,
};

pub(super) const FREEZE_CAPTURE_SEND_FULL_RETRY_LIMIT: u64 = 8;

impl OverlaySession {
	pub(super) fn maybe_timeout_pending_click_hit_test(&mut self, now: Instant) -> bool {
		let Some(request_id) = self.pending_click_hit_test_request_id else {
			self.pending_click_hit_test_requested_at = None;

			return false;
		};
		let Some(requested_at) = self.pending_click_hit_test_requested_at else {
			return false;
		};
		let Some(elapsed) = now.checked_duration_since(requested_at) else {
			return false;
		};

		if elapsed < PENDING_CLICK_HIT_TEST_TIMEOUT {
			return false;
		}

		self.pending_click_hit_test_request_id = None;
		self.pending_click_hit_test_requested_at = None;

		tracing::warn!(
			request_id,
			elapsed_ms = elapsed.as_millis(),
			timeout_ms = PENDING_CLICK_HIT_TEST_TIMEOUT.as_millis(),
			"Pending click hit test timed out."
		);

		if !matches!(self.state.mode, OverlayMode::Live) {
			return true;
		}

		let LiveCaptureInteraction::PressPending { monitor, .. } = self.live_capture_interaction
		else {
			return true;
		};
		let next_interaction = self
			.state
			.cursor
			.filter(|cursor| monitor.contains(*cursor))
			.and_then(|cursor| {
				self.live_capture_target_from_snapshot(monitor, cursor)
					.map(|target| LiveCaptureInteraction::HoverWindow { monitor, target })
			})
			.unwrap_or(LiveCaptureInteraction::Idle);

		self.set_live_capture_interaction(next_interaction);
		self.request_redraw_for_monitor(monitor);

		true
	}

	fn clear_freeze_capture_tracking(&mut self) {
		self.clear_frozen_capture_session_state();

		self.freeze_capture_send_full_count = 0;
	}

	pub(super) fn abort_pending_freeze_capture(&mut self, message: impl Into<String>) {
		let message = message.into();

		self.note_frozen_transition_aborted(message.as_str());

		if matches!(self.state.mode, OverlayMode::Frozen)
			&& let Some(monitor) = self.frozen_capture_monitor()
			&& self.state.monitor == Some(monitor)
		{
			self.set_frozen_capture_export_failed(monitor);
		} else {
			self.clear_freeze_capture_tracking();
		}

		self.freeze_capture_send_full_count = 0;

		self.restore_capture_windows_visibility();
		self.state.set_error(message);

		self.toolbar_state.needs_redraw = true;

		self.sync_frozen_toolbar_state();
		self.request_redraw_toolbar_window();
		self.request_redraw_all();
	}

	pub(super) fn note_freeze_capture_request_started(
		&mut self,
		overlay_monitor: MonitorRect,
		pending_window_target: Option<WindowFreezeCaptureTarget>,
	) {
		self.set_frozen_capture_worker_state(FrozenCaptureWorkerState::Inflight);

		self.freeze_capture_send_full_count = 0;

		self.note_frozen_transition_worker_requested(overlay_monitor, pending_window_target);
	}

	pub(super) fn handle_freeze_capture_request_send_error(
		&mut self,
		overlay_monitor: MonitorRect,
		err: WorkerRequestSendError,
	) {
		match err {
			WorkerRequestSendError::Full => {
				self.freeze_capture_send_full_count =
					self.freeze_capture_send_full_count.saturating_add(1);

				tracing::debug!(
					monitor_id = overlay_monitor.id,
					full_count = self.freeze_capture_send_full_count,
					"Freeze capture request dropped: worker queue full."
				);

				if self.freeze_capture_send_full_count >= FREEZE_CAPTURE_SEND_FULL_RETRY_LIMIT {
					self.abort_pending_freeze_capture("Capture worker is busy. Please try again.");
				} else {
					self.schedule_egui_repaint_after(
						self.repaint_interval_for_monitor(Some(overlay_monitor)),
					);
				}
			},
			WorkerRequestSendError::Disconnected => {
				tracing::warn!(
					monitor_id = overlay_monitor.id,
					"Freeze capture request failed: worker disconnected before capture could start."
				);

				self.abort_pending_freeze_capture("Capture worker is unavailable.");
			},
		}
	}

	pub(super) fn drain_worker_responses(&mut self) -> OverlayControl {
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

		if let Some(image) = self.pending_encode_png.take() {
			if let Some(worker) = self.worker.as_ref() {
				if let Err(image) = worker.request_encode_png(image) {
					self.pending_encode_png = Some(image);
				} else {
					#[cfg(target_os = "macos")]
					{
						self.png_encode_inflight = true;
					}
				}
			} else {
				self.pending_encode_png = Some(image);
			}
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

	pub(super) fn maybe_tick_worker_response_limiter(
		&mut self,
		resp: WorkerResponse,
	) -> OverlayControl {
		let control = match resp {
			#[cfg(not(target_os = "macos"))]
			WorkerResponse::SampledLiveCursor { monitor, point, request_id, sample } => {
				self.handle_sampled_live_cursor_response(monitor, point, request_id, sample);

				OverlayControl::Continue
			},
			WorkerResponse::RefreshedWindowList { snapshot } => {
				#[cfg(target_os = "macos")]
				{
					self.window_list_refresh_inflight = false;
				}

				#[cfg(target_os = "macos")]
				let should_apply_snapshot = !mem::take(&mut self.drop_next_window_list_refresh_snapshot);
				#[cfg(not(target_os = "macos"))]
				let should_apply_snapshot = true;

				if should_apply_snapshot {
					self.handle_refreshed_window_list(snapshot);
				}

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
			WorkerResponse::Error { source, message } => {
				let mut error_already_handled = false;

				match source {
					WorkerErrorSource::FreezeCapture => {
						self.abort_pending_freeze_capture(message.as_str());

						error_already_handled = true;
					},
					WorkerErrorSource::RefreshWindowList => {
						#[cfg(target_os = "macos")]
						{
							self.window_list_refresh_inflight = false;
							self.drop_next_window_list_refresh_snapshot = false;
						}
					},
					WorkerErrorSource::EncodePng => {
						#[cfg(target_os = "macos")]
						{
							self.png_encode_inflight = false;
						}
					},
					WorkerErrorSource::CaptureMonitorRegion => {
						self.clear_scroll_capture_inflight_request();
						self.scroll_capture_set_error(message);

						return OverlayControl::Continue;
					},
				}

				if !error_already_handled {
					self.state.set_error(message);
					self.request_redraw_all();
				}

				OverlayControl::Continue
			},
			WorkerResponse::EncodedPng { png_bytes } => {
				#[cfg(target_os = "macos")]
				{
					self.png_encode_inflight = false;
				}

				self.handle_encoded_png_response(png_bytes)
			},
		};

		#[cfg(target_os = "macos")]
		if matches!(control, OverlayControl::Continue) {
			self.maybe_request_redraw_for_pending_output();
			self.maybe_apply_pending_self_capture_exception_window_ids_worker_refresh();
		}

		control
	}
}
