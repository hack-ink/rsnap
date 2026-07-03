#[cfg(target_os = "macos")]
use image::RgbaImage;

#[cfg(target_os = "macos")]
use crate::overlay::GlobalPoint;
use crate::overlay::{
	FrozenCaptureSessionState, FrozenCaptureWorkerState, FrozenExportSessionState, MonitorRect,
	OverlayMode, OverlaySession, WindowFreezeCaptureTarget,
};

impl OverlaySession {
	pub(super) fn maybe_keep_frozen_capture_redraw(&self) {
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

	pub(super) fn frozen_capture_redraw_pending(&self) -> bool {
		!self.frozen_display_ready() && self.frozen_capture_export_pending()
	}

	pub(super) fn frozen_capture_monitor(&self) -> Option<MonitorRect> {
		match self.frozen_capture_session_state {
			FrozenCaptureSessionState::Inactive => None,
			FrozenCaptureSessionState::DisplayPending { monitor, .. }
			| FrozenCaptureSessionState::DisplayFailed { monitor }
			| FrozenCaptureSessionState::DisplayReady { monitor, .. } => Some(monitor),
		}
	}

	pub(super) fn frozen_capture_window_target(&self) -> Option<WindowFreezeCaptureTarget> {
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

	pub(super) fn frozen_capture_worker_state(&self) -> Option<FrozenCaptureWorkerState> {
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

	pub(super) fn frozen_capture_export_pending(&self) -> bool {
		matches!(
			self.frozen_capture_session_state,
			FrozenCaptureSessionState::DisplayPending { .. }
				| FrozenCaptureSessionState::DisplayReady {
					export: FrozenExportSessionState::Pending { .. },
					..
				}
		)
	}

	pub(super) fn frozen_capture_export_ready(&self) -> bool {
		matches!(
			self.frozen_capture_session_state,
			FrozenCaptureSessionState::DisplayReady { export: FrozenExportSessionState::Ready, .. }
		)
	}

	pub(super) fn frozen_capture_dispatch_pending(&self) -> bool {
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

	pub(super) fn frozen_capture_worker_armed(&self) -> bool {
		self.frozen_capture_worker_state() == Some(FrozenCaptureWorkerState::Armed)
	}

	pub(super) fn frozen_capture_worker_inflight(&self) -> bool {
		self.frozen_capture_worker_state() == Some(FrozenCaptureWorkerState::Inflight)
	}

	pub(super) fn set_frozen_capture_display_pending(
		&mut self,
		monitor: MonitorRect,
		worker_state: FrozenCaptureWorkerState,
		window_target: Option<WindowFreezeCaptureTarget>,
	) {
		self.frozen_capture_session_state =
			FrozenCaptureSessionState::DisplayPending { monitor, worker_state, window_target };
	}

	pub(super) fn frozen_display_handoff_pending(&self) -> bool {
		matches!(
			self.frozen_capture_session_state,
			FrozenCaptureSessionState::DisplayPending { .. }
		) && !matches!(self.state.mode, OverlayMode::Frozen)
	}

	pub(super) fn commit_first_frozen_display_handoff(&mut self, monitor: MonitorRect) {
		if matches!(self.state.mode, OverlayMode::Frozen) {
			return;
		}

		self.state.begin_freeze(monitor);

		self.state.drag_rect = None;
		self.state.hovered_window_rect = None;
	}

	pub(super) fn promote_frozen_capture_display_ready(&mut self, monitor: MonitorRect) {
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

	pub(super) fn set_frozen_capture_worker_state(
		&mut self,
		worker_state: FrozenCaptureWorkerState,
	) {
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

	pub(super) fn set_frozen_capture_export_ready(&mut self, monitor: MonitorRect) {
		self.frozen_capture_session_state = FrozenCaptureSessionState::DisplayReady {
			monitor,
			export: FrozenExportSessionState::Ready,
		};
	}

	pub(super) fn set_frozen_capture_export_failed(&mut self, monitor: MonitorRect) {
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

	pub(super) fn clear_frozen_capture_session_state(&mut self) {
		self.frozen_capture_session_state = FrozenCaptureSessionState::Inactive;
	}

	pub(super) fn frozen_display_ready(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& matches!(
				self.frozen_capture_session_state,
				FrozenCaptureSessionState::DisplayReady { .. }
			) && self.state.frozen_display_surface_image().is_some()
	}

	pub(super) fn frozen_display_ready_for_monitor(&self, monitor: MonitorRect) -> bool {
		self.frozen_display_ready() && self.state.monitor == Some(monitor)
	}

	pub(super) fn frozen_visual_handoff_pending_for_monitor(&self, monitor: MonitorRect) -> bool {
		let _ = monitor;

		false
	}

	pub(super) fn frozen_preview_visible(&self) -> bool {
		self.frozen_display_ready()
	}

	pub(super) fn maybe_tick_toolbar_window_warmup_redraw(&mut self) {
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

	pub(super) fn pending_freeze_capture_matches(&self, monitor: MonitorRect) -> bool {
		self.frozen_capture_monitor() == Some(monitor) && self.frozen_capture_dispatch_pending()
	}

	#[cfg(target_os = "macos")]
	pub(super) fn should_dispatch_pending_freeze_capture(&self, monitor: MonitorRect) -> bool {
		self.pending_freeze_capture_matches(monitor)
	}

	#[cfg(not(target_os = "macos"))]
	pub(super) fn should_dispatch_pending_freeze_capture(&self, monitor: MonitorRect) -> bool {
		self.pending_freeze_capture_matches(monitor) && !self.frozen_preview_visible()
	}

	pub(super) fn frozen_final_capture_ready(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Frozen)
			&& self.frozen_capture_export_ready()
			&& self.state.frozen_export_image.is_some()
	}

	pub(super) fn pending_window_freeze_capture_for_monitor(
		&self,
		monitor: MonitorRect,
	) -> Option<WindowFreezeCaptureTarget> {
		self.frozen_capture_window_target().filter(|target| target.monitor == monitor)
	}

	#[cfg(target_os = "macos")]
	pub(super) fn commit_frozen_preview(
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
}
