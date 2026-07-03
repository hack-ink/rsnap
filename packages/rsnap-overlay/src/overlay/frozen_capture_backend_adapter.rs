#[cfg(target_os = "macos")]
use image::RgbaImage;

#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::STREAM_REGION_FRAME_MAX_AGE;
#[cfg(target_os = "macos")]
use crate::overlay::runtime_timing::DISPLAY_FIRST_FREEZE_LIVE_TIMEOUT;
#[cfg(target_os = "macos")]
use crate::overlay::{
	Arc, FreezeCaptureTarget, FrozenCaptureWorkerState, GlobalPoint, Instant, MonitorRect,
	OverlayMode, OverlaySession, WindowCaptureAlphaMode, WindowFreezeCaptureTarget,
};
#[cfg(target_os = "macos")]
use crate::state::MonitorImageSnapshot;

#[cfg(target_os = "macos")]
struct FrozenCaptureDisplayCandidate {
	display_image: RgbaImage,
	export_image: Option<RgbaImage>,
	cursor: Option<GlobalPoint>,
	source: &'static str,
	snapshot_age_ms: u128,
	captured_window_id: Option<u32>,
}

#[cfg(target_os = "macos")]
struct FrozenCaptureBackendDispatch {
	freeze_target: FreezeCaptureTarget,
	window_target: Option<WindowFreezeCaptureTarget>,
}

#[cfg(target_os = "macos")]
enum FrozenCaptureBackendSignal {
	None,
	Wait { reason: &'static str, arm_worker: bool },
	EscalateHiddenFallback { reason: &'static str },
	Dispatch(FrozenCaptureBackendDispatch),
}

#[cfg(target_os = "macos")]
struct FrozenCaptureBackendUpdate {
	display_candidate: Option<FrozenCaptureDisplayCandidate>,
	signal: FrozenCaptureBackendSignal,
}
#[cfg(target_os = "macos")]
impl FrozenCaptureBackendUpdate {
	fn none() -> Self {
		Self { display_candidate: None, signal: FrozenCaptureBackendSignal::None }
	}
}

#[cfg(target_os = "macos")]
impl OverlaySession {
	fn snapshot_can_finish_frozen_capture(
		&self,
		window_target: Option<WindowFreezeCaptureTarget>,
	) -> bool {
		window_target.is_none()
			|| self.config.window_capture_alpha_mode == WindowCaptureAlphaMode::Background
	}

	fn usable_frozen_capture_snapshot(
		&self,
		monitor: MonitorRect,
		snapshot: Option<Arc<MonitorImageSnapshot>>,
	) -> Option<(Arc<MonitorImageSnapshot>, u128)> {
		if self
			.live_sample_stream
			.as_ref()
			.is_some_and(|stream| !stream.self_capture_filter_complete_for_monitor(monitor))
		{
			return None;
		}

		let snapshot = snapshot.filter(|snapshot| snapshot.monitor == monitor)?;
		let snapshot_age = snapshot.captured_at.elapsed();

		if snapshot_age > STREAM_REGION_FRAME_MAX_AGE {
			return None;
		}

		Some((snapshot, snapshot_age.as_millis()))
	}

	fn request_pending_frozen_capture_live_stream_refresh(&self, monitor: MonitorRect) {
		let Some(stream) = self.live_sample_stream.as_ref() else {
			return;
		};
		let after_frame_seq =
			stream.latest_frame_frontier_for_monitor(monitor).map_or(0, |(frame_seq, _)| frame_seq);
		let _ = stream.refresh_monitor_nonblocking_if_stale(monitor, after_frame_seq, true);
	}

	pub(in crate::overlay) fn prewarm_frozen_capture_live_stream_refresh(
		&self,
		monitor: MonitorRect,
	) {
		if !matches!(self.state.mode, OverlayMode::Live) || self.frozen_display_handoff_pending() {
			return;
		}

		self.request_pending_frozen_capture_live_stream_refresh(monitor);
	}

	fn frozen_capture_display_candidate_from_snapshot(
		&self,
		monitor: MonitorRect,
		window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
		snapshot: Option<Arc<MonitorImageSnapshot>>,
		source: &'static str,
		export_ready: bool,
	) -> Option<FrozenCaptureDisplayCandidate> {
		let (snapshot, snapshot_age_ms) = self.usable_frozen_capture_snapshot(monitor, snapshot)?;
		let display_image = snapshot.image.as_ref().clone();
		let export_image = export_ready.then(|| display_image.clone());

		Some(FrozenCaptureDisplayCandidate {
			display_image,
			export_image,
			cursor,
			source,
			snapshot_age_ms,
			captured_window_id: export_ready
				.then(|| window_target.map(|target| target.window_id))
				.flatten(),
		})
	}

	fn frozen_capture_display_candidate_from_live_surface_bg(
		&self,
		monitor: MonitorRect,
		cursor: Option<GlobalPoint>,
		source: &'static str,
		export_candidate: Option<(Arc<MonitorImageSnapshot>, u128)>,
		captured_window_id: Option<u32>,
	) -> Option<FrozenCaptureDisplayCandidate> {
		let display_image = self
			.state
			.live_bg_image
			.as_ref()
			.filter(|_| self.state.live_bg_monitor == Some(monitor))
			.cloned()?;
		let (export_image, snapshot_age_ms) = match export_candidate {
			Some((snapshot, age_ms)) => (Some(snapshot.image.as_ref().clone()), age_ms),
			None => (None, 0),
		};

		Some(FrozenCaptureDisplayCandidate {
			display_image,
			export_image,
			cursor,
			source,
			snapshot_age_ms,
			captured_window_id,
		})
	}

	fn apply_frozen_capture_display_candidate(
		&mut self,
		monitor: MonitorRect,
		candidate: FrozenCaptureDisplayCandidate,
	) {
		let restore_hidden_capture_windows =
			self.capture_windows_hidden && candidate.export_image.is_some();
		let had_display_preview = self.frozen_preview_visible();

		if !had_display_preview {
			self.commit_frozen_preview(monitor, candidate.display_image, candidate.cursor);
			self.note_frozen_transition_preview_committed(
				monitor,
				candidate.source,
				Some(candidate.snapshot_age_ms),
			);
		}

		if let Some(export_image) = candidate.export_image {
			self.set_frozen_capture_export_ready(monitor);
			self.state.commit_frozen_export_image(export_image);
			self.note_frozen_transition_final_ready(
				monitor,
				candidate.source,
				candidate.captured_window_id,
			);

			self.freeze_capture_send_full_count = 0;
			self.frozen_window_image = None;

			if restore_hidden_capture_windows {
				self.destroy_live_only_aux_windows();
				self.restore_capture_windows_visibility();
			} else {
				self.capture_windows_hidden = false;
			}
		}

		self.toolbar_state.needs_redraw = true;

		self.sync_frozen_toolbar_state();
		self.request_redraw_for_monitor(monitor);
		self.request_aux_window_creation_if_needed();
		self.request_redraw_toolbar_window();
	}

	fn apply_frozen_capture_backend_signal(
		&mut self,
		monitor: MonitorRect,
		signal: FrozenCaptureBackendSignal,
	) {
		match signal {
			FrozenCaptureBackendSignal::None => {},
			FrozenCaptureBackendSignal::Wait { reason, arm_worker } => {
				self.set_frozen_capture_worker_state(if arm_worker {
					FrozenCaptureWorkerState::Armed
				} else {
					FrozenCaptureWorkerState::Idle
				});
				self.request_pending_frozen_capture_live_stream_refresh(monitor);
				self.note_frozen_transition_preview_deferred(monitor, reason, None);

				if arm_worker {
					self.note_frozen_transition_authoritative_handoff_armed(monitor);
				}
			},
			FrozenCaptureBackendSignal::EscalateHiddenFallback { reason } => {
				self.set_frozen_capture_worker_state(FrozenCaptureWorkerState::Armed);
				self.note_frozen_transition_preview_deferred(monitor, reason, None);
				self.note_frozen_transition_authoritative_handoff_armed(monitor);

				if !self.capture_windows_hidden {
					self.hide_capture_windows();
				}

				self.request_redraw_for_monitor(monitor);
			},
			FrozenCaptureBackendSignal::Dispatch(dispatch) => {
				let Some(worker) = &self.worker else {
					self.abort_pending_freeze_capture("Capture worker is unavailable.");

					return;
				};

				match worker.request_freeze_capture(monitor, dispatch.freeze_target) {
					Ok(()) => {
						self.note_freeze_capture_request_started(monitor, dispatch.window_target);
					},
					Err(err) => self.handle_freeze_capture_request_send_error(monitor, err),
				}
			},
		}
	}

	fn apply_frozen_capture_backend_update(
		&mut self,
		monitor: MonitorRect,
		update: FrozenCaptureBackendUpdate,
	) {
		if let Some(candidate) = update.display_candidate {
			self.apply_frozen_capture_display_candidate(monitor, candidate);
		}

		self.apply_frozen_capture_backend_signal(monitor, update.signal);
	}

	fn macos_begin_frozen_capture_backend_update(
		&self,
		monitor: MonitorRect,
		window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
	) -> FrozenCaptureBackendUpdate {
		let snapshot = self
			.live_sample_stream
			.as_ref()
			.and_then(|stream| stream.peek_latest_rgba_snapshot(monitor));
		let can_finish_from_snapshot = self.snapshot_can_finish_frozen_capture(window_target);

		if let Some(display_candidate) = self.frozen_capture_display_candidate_from_live_surface_bg(
			monitor,
			cursor,
			"live_surface_background_at_freeze_begin",
			can_finish_from_snapshot
				.then(|| self.usable_frozen_capture_snapshot(monitor, snapshot.clone()))
				.flatten(),
			can_finish_from_snapshot
				.then(|| window_target.map(|target| target.window_id))
				.flatten(),
		) {
			return FrozenCaptureBackendUpdate {
				display_candidate: Some(display_candidate),
				signal: if can_finish_from_snapshot
					&& self.usable_frozen_capture_snapshot(monitor, snapshot.clone()).is_some()
				{
					FrozenCaptureBackendSignal::None
				} else {
					let arm_worker = window_target.is_some()
						&& self.config.window_capture_alpha_mode
							!= WindowCaptureAlphaMode::Background;

					FrozenCaptureBackendSignal::Wait {
						reason: if window_target.is_some() {
							"waiting_for_export_authority"
						} else {
							"waiting_for_live_stream_snapshot"
						},
						arm_worker,
					}
				},
			};
		}

		if can_finish_from_snapshot
			&& let Some(display_candidate) = self.frozen_capture_display_candidate_from_snapshot(
				monitor,
				window_target,
				cursor,
				snapshot.clone(),
				"live_stream_snapshot_at_freeze_begin",
				true,
			) {
			return FrozenCaptureBackendUpdate {
				display_candidate: Some(display_candidate),
				signal: FrozenCaptureBackendSignal::None,
			};
		}

		let display_candidate = self.frozen_capture_display_candidate_from_snapshot(
			monitor,
			window_target,
			cursor,
			snapshot,
			"live_stream_snapshot_preview_at_freeze_begin",
			false,
		);
		let arm_worker = window_target.is_some()
			&& self.config.window_capture_alpha_mode != WindowCaptureAlphaMode::Background;
		let reason = if display_candidate.is_some() {
			"waiting_for_export_authority"
		} else {
			"waiting_for_live_stream_snapshot"
		};

		FrozenCaptureBackendUpdate {
			display_candidate,
			signal: FrozenCaptureBackendSignal::Wait { reason, arm_worker },
		}
	}

	fn maybe_macos_pending_frozen_capture_backend_update(
		&mut self,
		monitor: MonitorRect,
		now: Instant,
	) -> FrozenCaptureBackendUpdate {
		if !self.pending_freeze_capture_matches(monitor)
			|| self.frozen_capture_export_ready()
			|| self.frozen_capture_worker_inflight()
		{
			return FrozenCaptureBackendUpdate::none();
		}

		self.request_pending_frozen_capture_live_stream_refresh(monitor);

		let window_target = self.pending_window_freeze_capture_for_monitor(monitor);
		let snapshot = self
			.live_sample_stream
			.as_ref()
			.and_then(|stream| stream.peek_latest_rgba_snapshot(monitor));

		if self.snapshot_can_finish_frozen_capture(window_target)
			&& let Some(display_candidate) = self.frozen_capture_display_candidate_from_snapshot(
				monitor,
				window_target,
				self.state.cursor,
				snapshot.clone(),
				"live_stream_snapshot_followup",
				true,
			) {
			return FrozenCaptureBackendUpdate {
				display_candidate: Some(display_candidate),
				signal: FrozenCaptureBackendSignal::None,
			};
		}
		if self.frozen_preview_visible() {
			return FrozenCaptureBackendUpdate::none();
		}

		if let Some(display_candidate) = self.frozen_capture_display_candidate_from_snapshot(
			monitor,
			window_target,
			self.state.cursor,
			snapshot,
			"live_stream_snapshot_preview_followup",
			false,
		) {
			return FrozenCaptureBackendUpdate {
				display_candidate: Some(display_candidate),
				signal: FrozenCaptureBackendSignal::None,
			};
		}

		let Some(started_at) = self.frozen_transition_started_at() else {
			return FrozenCaptureBackendUpdate::none();
		};
		let Some(elapsed) = now.checked_duration_since(started_at) else {
			return FrozenCaptureBackendUpdate::none();
		};

		if elapsed < DISPLAY_FIRST_FREEZE_LIVE_TIMEOUT {
			return FrozenCaptureBackendUpdate::none();
		}

		FrozenCaptureBackendUpdate {
			display_candidate: None,
			signal: FrozenCaptureBackendSignal::EscalateHiddenFallback {
				reason: "timed_out_waiting_for_clean_preview",
			},
		}
	}

	fn maybe_macos_armed_frozen_capture_backend_dispatch(
		&mut self,
	) -> Option<FrozenCaptureBackendDispatch> {
		if !self.frozen_capture_worker_armed() {
			return None;
		}

		let Some(monitor) =
			self.frozen_capture_monitor().filter(|_| self.frozen_capture_export_pending())
		else {
			self.set_frozen_capture_worker_state(FrozenCaptureWorkerState::Idle);

			self.freeze_capture_send_full_count = 0;

			return None;
		};

		if !self.pending_freeze_capture_matches(monitor) {
			self.set_frozen_capture_worker_state(FrozenCaptureWorkerState::Idle);

			self.freeze_capture_send_full_count = 0;

			return None;
		}

		let window_target = self.pending_window_freeze_capture_for_monitor(monitor);

		if !self.capture_windows_hidden && self.snapshot_can_finish_frozen_capture(window_target) {
			self.set_frozen_capture_worker_state(FrozenCaptureWorkerState::Idle);

			self.freeze_capture_send_full_count = 0;

			return None;
		}
		if !self.capture_windows_hidden
			&& window_target.is_some()
			&& !self.snapshot_can_finish_frozen_capture(window_target)
			&& !self.frozen_preview_visible()
		{
			return None;
		}

		Some(FrozenCaptureBackendDispatch {
			freeze_target: window_target.map_or(FreezeCaptureTarget::Monitor, |target| {
				FreezeCaptureTarget::Window { window_id: target.window_id }
			}),
			window_target,
		})
	}

	pub(super) fn maybe_drive_macos_display_first_frozen_capture_backend(&mut self, now: Instant) {
		let Some(monitor) =
			self.frozen_capture_monitor().filter(|_| self.frozen_capture_export_pending())
		else {
			return;
		};
		let update = self.maybe_macos_pending_frozen_capture_backend_update(monitor, now);

		self.apply_frozen_capture_backend_update(monitor, update);

		let Some(monitor) =
			self.frozen_capture_monitor().filter(|_| self.frozen_capture_export_pending())
		else {
			return;
		};
		let Some(dispatch) = self.maybe_macos_armed_frozen_capture_backend_dispatch() else {
			return;
		};

		self.apply_frozen_capture_backend_signal(
			monitor,
			FrozenCaptureBackendSignal::Dispatch(dispatch),
		);
	}

	#[cfg(test)]
	pub(super) fn maybe_finish_frozen_capture_from_snapshot(
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

		let Some(display_candidate) = self.frozen_capture_display_candidate_from_snapshot(
			monitor,
			window_target,
			cursor,
			snapshot,
			source,
			true,
		) else {
			return false;
		};

		self.apply_frozen_capture_backend_update(
			monitor,
			FrozenCaptureBackendUpdate {
				display_candidate: Some(display_candidate),
				signal: FrozenCaptureBackendSignal::None,
			},
		);

		true
	}

	#[cfg(test)]
	pub(super) fn maybe_seed_frozen_capture_preview_from_snapshot(
		&mut self,
		monitor: MonitorRect,
		cursor: Option<GlobalPoint>,
		snapshot: Option<Arc<MonitorImageSnapshot>>,
		source: &'static str,
	) -> bool {
		let Some(display_candidate) = self.frozen_capture_display_candidate_from_snapshot(
			monitor, None, cursor, snapshot, source, false,
		) else {
			return false;
		};

		self.apply_frozen_capture_backend_update(
			monitor,
			FrozenCaptureBackendUpdate {
				display_candidate: Some(display_candidate),
				signal: FrozenCaptureBackendSignal::None,
			},
		);

		true
	}

	#[cfg(test)]
	pub(super) fn maybe_dispatch_armed_freeze_capture(&mut self) {
		let Some(dispatch) = self.maybe_macos_armed_frozen_capture_backend_dispatch() else {
			return;
		};
		let Some(monitor) =
			self.frozen_capture_monitor().filter(|_| self.frozen_capture_export_pending())
		else {
			return;
		};

		self.apply_frozen_capture_backend_signal(
			monitor,
			FrozenCaptureBackendSignal::Dispatch(dispatch),
		);
	}

	pub(super) fn begin_frozen_capture_with_rect_macos(
		&mut self,
		monitor: MonitorRect,
		window_target: Option<WindowFreezeCaptureTarget>,
		cursor: Option<GlobalPoint>,
	) -> bool {
		if let Some(cursor) = cursor {
			self.update_cursor_state(monitor, cursor);
		}

		self.capture_windows_hidden = false;

		let update = self.macos_begin_frozen_capture_backend_update(monitor, window_target, cursor);
		let finished = update
			.display_candidate
			.as_ref()
			.and_then(|candidate| candidate.export_image.as_ref())
			.is_some();

		self.apply_frozen_capture_backend_update(monitor, update);

		if matches!(self.state.mode, OverlayMode::Frozen) {
			self.state.live_bg_monitor = None;
			self.state.live_bg_image = None;
		}

		finished
	}
}
