use crate::overlay::live_capture_target::LiveClickCaptureTarget;
use crate::overlay::runtime_timing::{CURSOR_POLL_INTERVAL_MIN, PENDING_CLICK_HIT_TEST_TIMEOUT};
use crate::overlay::{
	Arc, CapturedMonitorRegionResult, Duration, FrozenCaptureWorkerState, GlobalPoint, Instant,
	LiveCaptureInteraction, LiveCursorSample, LiveSampleApplyResult, MonitorRect, OverlayControl,
	OverlayMode, OverlaySession, WindowFreezeCaptureTarget, WindowListSnapshot, WorkerErrorSource,
	WorkerRequestSendError, WorkerResponse,
};
#[cfg(target_os = "macos")]
use crate::overlay::{CursorSampleRequest, mem};
use crate::state::{LoupeSample, WindowHit};

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

	pub(super) fn request_live_samples_for_cursor(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) -> bool {
		if self.pending_click_hit_test_request_id.is_some() {
			return false;
		}

		let press_pending = self.live_capture_interaction_is_press_pending();
		let is_dragging_window = self.live_capture_interaction_is_dragging();
		let had_snapshot_update = if press_pending || is_dragging_window || self.state.alt_held {
			false
		} else {
			self.apply_live_hover_cache_state(monitor, cursor)
		};
		let sample_updated = self.request_live_cursor_sample(monitor, cursor, self.state.alt_held);

		if !press_pending && !is_dragging_window && !self.state.alt_held {
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

	pub(super) fn request_live_window_list_refresh_if_needed(&mut self) -> bool {
		#[cfg(target_os = "macos")]
		if self.window_list_refresh_inflight {
			return false;
		}

		let now = Instant::now();
		let needs_refresh = self.window_list_snapshot.as_ref().is_none_or(|snapshot| {
			now.duration_since(snapshot.captured_at) > self.window_list_refresh_interval
				|| self.state.alt_held
		});
		let throttled = now.duration_since(self.last_window_list_refresh_request_at)
			< self.window_list_refresh_interval;

		if !needs_refresh || throttled {
			return false;
		}

		let Some(worker) = self.worker.as_ref() else {
			return false;
		};

		if !worker.request_refresh_window_list() {
			return false;
		}

		self.last_window_list_refresh_request_at = now;
		#[cfg(target_os = "macos")]
		{
			self.window_list_refresh_inflight = true;
		}

		true
	}

	fn log_live_sample_apply_timing(
		&self,
		path: &'static str,
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		elapsed: Duration,
		apply: LiveSampleApplyResult,
	) {
		tracing::trace!(
			op = "overlay.live_sample_apply_phase",
			path,
			request_id,
			monitor_id = monitor.id,
			point = ?point,
			latency_us = elapsed.as_micros(),
			alt_held = self.state.alt_held,
			overlay_changed = apply.overlay_changed,
			hud_changed = apply.hud_changed,
			loupe_changed = apply.loupe_changed,
			"Live sample apply phase timing."
		);

		if elapsed >= Duration::from_millis(12) {
			tracing::debug!(
				op = "overlay.live_sample_apply_latency",
				path,
				request_id,
				monitor_id = monitor.id,
				point = ?point,
				latency_ms = elapsed.as_millis(),
				alt_held = self.state.alt_held,
				overlay_changed = apply.overlay_changed,
				hud_changed = apply.hud_changed,
				loupe_changed = apply.loupe_changed,
				"Live cursor sample apply latency exceeded the target frame budget."
			);
		}
	}

	pub(super) fn request_live_cursor_sample(
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
				CursorSampleRequest::with_optional_patch(
					x_px,
					y_px,
					want_patch,
					patch_width_px,
					patch_height_px,
				),
			);

			self.note_live_cursor_sample_request_started(request_id);

			let Some(sample) = sample else {
				self.finish_sync_live_cursor_sample_attempt(request_id);

				return false;
			};

			self.finish_sync_live_cursor_sample_attempt(request_id);

			let apply = self.apply_live_cursor_sample_detail(monitor, cursor, sample);
			let sample_latency = self
				.latest_live_cursor_sample_requested_at
				.take()
				.map_or(Duration::ZERO, |requested_at| requested_at.elapsed());

			self.log_live_sample_apply_timing(
				"macos_stream",
				monitor,
				cursor,
				request_id,
				sample_latency,
				apply,
			);

			if apply.any_changed() {
				self.request_redraw_live_sample_targets(monitor, apply);
			}

			true
		}
		#[cfg(not(target_os = "macos"))]
		{
			if self.live_sample_request_pending() {
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
					self.note_live_cursor_sample_request_started(request_id);

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

	pub(super) fn apply_live_cursor_sample_detail(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		sample: LiveCursorSample,
	) -> LiveSampleApplyResult {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return LiveSampleApplyResult::default();
		}
		if self.frozen_display_handoff_pending() {
			return LiveSampleApplyResult::default();
		}
		if self.active_cursor_monitor() != Some(monitor) {
			return LiveSampleApplyResult::default();
		}

		let press_pending = self.live_capture_interaction_is_press_pending();
		let is_dragging_window = self.live_capture_interaction_is_dragging();
		let mut changed = LiveSampleApplyResult::default();

		if is_dragging_window {
			if self.state.hovered_window_rect.is_some() {
				self.state.hovered_window_rect = None;
				changed.overlay_changed = true;
				changed.hud_changed = true;
			}
		} else if !press_pending && self.apply_live_hover_cache_state(monitor, point) {
			changed.overlay_changed = true;
			changed.hud_changed = true;
		}
		if self.state.rgb != sample.rgb && sample.rgb.is_some() {
			self.state.rgb = sample.rgb;
			changed.hud_changed = true;
		}
		if self.state.alt_held {
			let loupe = sample.patch.map(|patch| LoupeSample { center: point, patch });
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

	pub(super) fn apply_live_hover_cache_state(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) -> bool {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return false;
		}
		if self.frozen_display_handoff_pending() {
			return false;
		}
		if self.live_capture_interaction_is_press_pending()
			|| self.live_capture_interaction_is_dragging()
		{
			return false;
		}
		if !monitor.contains(cursor) {
			return false;
		}

		let previous_hovered_window_rect = self.state.hovered_window_rect;
		let next_interaction = self
			.hovered_window_hit_from_window_list_snapshot(monitor, cursor)
			.map(|hit| LiveCaptureInteraction::HoverWindow {
				monitor,
				target: LiveClickCaptureTarget::from_window_hit(monitor, hit),
			})
			.unwrap_or(LiveCaptureInteraction::Idle);

		self.set_live_capture_interaction(next_interaction);

		self.state.hovered_window_rect != previous_hovered_window_rect
	}

	pub(super) fn live_sample_request_redraw_intent(
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

	pub(super) fn idle_live_sampling_request_allowed(
		&self,
		now: Instant,
		monitor: MonitorRect,
	) -> bool {
		self.last_idle_live_sample_request_at.is_none_or(|last_request_at| {
			now.duration_since(last_request_at) >= self.idle_live_sampling_interval(monitor)
		})
	}

	pub(super) fn hovered_window_hit_from_window_list_snapshot(
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

	pub(super) fn record_live_sample_stall(&mut self, cursor: GlobalPoint, monitor: MonitorRect) {
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

	#[cfg(not(target_os = "macos"))]
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

		let apply = self.apply_live_cursor_sample_detail(monitor, point, sample);
		let sample_latency = self
			.latest_live_cursor_sample_requested_at
			.take()
			.map_or(Duration::ZERO, |requested_at| requested_at.elapsed());

		self.log_live_sample_apply_timing(
			"worker_response",
			monitor,
			point,
			request_id,
			sample_latency,
			apply,
		);

		if apply.any_changed() {
			self.request_redraw_live_sample_targets(monitor, apply);
		}
	}

	fn handle_refreshed_window_list(&mut self, snapshot: Arc<WindowListSnapshot>) {
		self.window_list_snapshot = Some(snapshot);

		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}
		if self.frozen_display_handoff_pending() {
			return;
		}

		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};
		let press_pending = self.live_capture_interaction_is_press_pending();
		let is_dragging_window = self.live_capture_interaction_is_dragging();

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
		if press_pending {
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
		_point: GlobalPoint,
		request_id: u64,
		hit: Option<WindowHit>,
	) {
		if self.pending_click_hit_test_request_id != Some(request_id) {
			return;
		}

		self.pending_click_hit_test_request_id = None;
		self.pending_click_hit_test_requested_at = None;

		let click_target = hit
			.map_or_else(LiveClickCaptureTarget::fullscreen_fallback, |window_hit| {
				LiveClickCaptureTarget::from_window_hit(monitor, window_hit)
			});

		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}
		if self.frozen_display_handoff_pending() {
			return;
		}

		self.resolve_live_capture_click_target(monitor, click_target);
	}

	pub(super) fn request_click_capture_hit_test(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) {
		if self.pending_click_hit_test_request_id.is_some() {
			return;
		}

		self.request_live_window_list_refresh_if_needed();

		if let Some(target) = self.live_capture_target_from_snapshot(monitor, cursor) {
			self.resolve_live_capture_click_target(monitor, target);

			return;
		}

		let request_id = self.hit_test_request_id.wrapping_add(1);
		let Some(worker) = self.worker.as_ref() else {
			self.resolve_live_capture_click_target(
				monitor,
				LiveClickCaptureTarget::fullscreen_fallback(),
			);

			return;
		};

		self.hit_test_request_id = request_id;

		match worker.request_hit_test_window(monitor, cursor, request_id) {
			Ok(()) => {
				self.pending_click_hit_test_request_id = Some(request_id);
				self.pending_click_hit_test_requested_at = Some(Instant::now());

				self.schedule_egui_repaint_after(PENDING_CLICK_HIT_TEST_TIMEOUT);
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

				self.resolve_live_capture_click_target(
					monitor,
					LiveClickCaptureTarget::fullscreen_fallback(),
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

				self.resolve_live_capture_click_target(
					monitor,
					LiveClickCaptureTarget::fullscreen_fallback(),
				);
			},
		}
	}
}
