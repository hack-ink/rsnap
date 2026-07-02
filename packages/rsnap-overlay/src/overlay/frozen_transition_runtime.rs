use std::time::Instant;

use crate::overlay::{
	LiveClickCaptureTarget, MonitorRect, OverlaySession, RectPoints, WindowFreezeCaptureTarget,
};

#[derive(Debug, Default)]
pub(super) struct FrozenTransitionRuntime {
	press_pending_at: Option<Instant>,
	started_at: Option<Instant>,
	preview_committed_at: Option<Instant>,
	preview_source: Option<&'static str>,
	final_ready_at: Option<Instant>,
	toolbar_visible_at: Option<Instant>,
	toolbar_first_draw_at: Option<Instant>,
	badge_slot_armed_at: Option<Instant>,
	target_window_id: Option<u32>,
}
impl FrozenTransitionRuntime {
	pub(super) fn clear_exit_window_runtime(&mut self) {
		self.press_pending_at = None;
		self.toolbar_first_draw_at = None;
		self.badge_slot_armed_at = None;
	}

	fn elapsed_ms_since(started_at: Option<Instant>, now: Instant) -> Option<u128> {
		started_at
			.and_then(|started_at| now.checked_duration_since(started_at))
			.map(|elapsed| elapsed.as_millis())
	}

	fn reset_timing(&mut self) {
		self.started_at = None;
		self.preview_committed_at = None;
		self.preview_source = None;
		self.final_ready_at = None;
		self.toolbar_visible_at = None;
		self.toolbar_first_draw_at = None;
		self.badge_slot_armed_at = None;
		self.target_window_id = None;
	}
}

impl OverlaySession {
	#[cfg(target_os = "macos")]
	pub(super) fn frozen_transition_started_at(&self) -> Option<Instant> {
		self.frozen_transition.started_at
	}

	#[cfg(all(target_os = "macos", test))]
	pub(super) fn debug_set_frozen_transition_started_at(&mut self, started_at: Option<Instant>) {
		self.frozen_transition.started_at = started_at;
	}

	pub(super) fn reset_frozen_transition_timing(&mut self) {
		self.frozen_transition.reset_timing();
	}

	fn log_frozen_transition_timing_info(&self, event: FrozenTransitionTimingInfo) {
		let transition = &self.frozen_transition;
		let now = Instant::now();
		let since_press_ms =
			FrozenTransitionRuntime::elapsed_ms_since(transition.press_pending_at, now);
		let since_begin_ms = FrozenTransitionRuntime::elapsed_ms_since(transition.started_at, now);
		let since_preview_ms =
			FrozenTransitionRuntime::elapsed_ms_since(transition.preview_committed_at, now);
		let since_final_ready_ms =
			FrozenTransitionRuntime::elapsed_ms_since(transition.final_ready_at, now);
		let since_toolbar_first_draw_ms =
			FrozenTransitionRuntime::elapsed_ms_since(transition.toolbar_first_draw_at, now);
		let slow_transition = matches!(
			event.op,
			"overlay.freeze_transition_preview_committed"
				| "overlay.freeze_transition_toolbar_visible"
				| "overlay.freeze_transition_toolbar_first_draw"
				| "overlay.freeze_transition_badge_slot_armed"
		) && since_press_ms.is_some_and(|elapsed_ms| elapsed_ms >= 400);

		if slow_transition {
			tracing::warn!(
				target: "rsnap",
				op = event.op,
				monitor_id = event.monitor.map(|monitor| monitor.id),
				frozen_capture_source = ?self.frozen_capture_source,
				alpha_mode = ?self.config.window_capture_alpha_mode,
				target_window_id = transition.target_window_id,
				captured_window_id = event.captured_window_id,
				source = event.source,
				preview_source = transition.preview_source,
				reason = event.reason,
				snapshot_age_ms = event.snapshot_age_ms,
				grace_ms = event.grace_ms,
				capture_windows_hidden = self.capture_windows_hidden,
				since_press_ms,
				since_begin_ms,
				since_preview_ms,
				since_final_ready_ms,
				since_toolbar_first_draw_ms,
				"{}",
				event.message
			);
		} else {
			tracing::info!(
				target: "rsnap",
				op = event.op,
				monitor_id = event.monitor.map(|monitor| monitor.id),
				frozen_capture_source = ?self.frozen_capture_source,
				alpha_mode = ?self.config.window_capture_alpha_mode,
				target_window_id = transition.target_window_id,
				captured_window_id = event.captured_window_id,
				source = event.source,
				preview_source = transition.preview_source,
				reason = event.reason,
				snapshot_age_ms = event.snapshot_age_ms,
				grace_ms = event.grace_ms,
				capture_windows_hidden = self.capture_windows_hidden,
				since_press_ms,
				since_begin_ms,
				since_preview_ms,
				since_final_ready_ms,
				since_toolbar_first_draw_ms,
				"{}",
				event.message
			);
		}
	}

	pub(super) fn begin_frozen_transition_timing(
		&mut self,
		monitor: MonitorRect,
		capture_rect: RectPoints,
		window_target: Option<WindowFreezeCaptureTarget>,
	) {
		let now = Instant::now();

		self.reset_frozen_transition_timing();

		self.frozen_transition.started_at = Some(now);
		self.frozen_transition.target_window_id = window_target.map(|target| target.window_id);

		tracing::debug!(
			op = "overlay.freeze_transition_begin",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			capture_rect = ?capture_rect,
			target_window_id = self.frozen_transition.target_window_id,
			"Frozen transition started."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_begin",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition started.",
		});
	}

	pub(super) fn note_frozen_transition_press_pending(
		&mut self,
		monitor: MonitorRect,
		click_target: Option<LiveClickCaptureTarget>,
	) {
		let now = Instant::now();
		let reason = click_target.map(|target| {
			if target.window_target.is_some() {
				"window_target"
			} else if target.capture_rect.is_none() {
				"fullscreen_fallback"
			} else {
				"rect_target"
			}
		});

		self.frozen_transition.press_pending_at = Some(now);

		if self.frozen_transition.target_window_id.is_none() {
			self.frozen_transition.target_window_id =
				click_target.and_then(|target| target.window_target).map(|target| target.window_id);
		}

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_press_pending",
			monitor: Some(monitor),
			reason,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition intent was armed on mouse-down.",
		});
	}

	#[cfg(target_os = "macos")]
	pub(super) fn note_frozen_transition_preview_deferred(
		&self,
		monitor: MonitorRect,
		reason: &'static str,
		snapshot_age_ms: Option<u128>,
	) {
		let now = Instant::now();

		tracing::debug!(
			op = "overlay.freeze_transition_preview_deferred",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition.target_window_id,
			reason,
			snapshot_age_ms,
			since_begin_ms =
				FrozenTransitionRuntime::elapsed_ms_since(self.frozen_transition.started_at, now),
			"Frozen transition preview is deferred while capture settles."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_preview_deferred",
			monitor: Some(monitor),
			reason: Some(reason),
			source: None,
			snapshot_age_ms,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition preview is deferred while capture settles.",
		});
	}

	pub(super) fn note_frozen_transition_preview_committed(
		&mut self,
		monitor: MonitorRect,
		source: &'static str,
		snapshot_age_ms: Option<u128>,
	) {
		if self.frozen_transition.preview_committed_at.is_some() {
			return;
		}

		let now = Instant::now();

		self.frozen_transition.preview_committed_at = Some(now);
		self.frozen_transition.preview_source = Some(source);

		tracing::debug!(
			op = "overlay.freeze_transition_preview_committed",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition.target_window_id,
			source,
			snapshot_age_ms,
			since_begin_ms =
				FrozenTransitionRuntime::elapsed_ms_since(self.frozen_transition.started_at, now),
			"Frozen transition preview became visible."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_preview_committed",
			monitor: Some(monitor),
			reason: None,
			source: Some(source),
			snapshot_age_ms,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition preview became visible.",
		});
	}

	pub(super) fn note_frozen_transition_worker_requested(
		&mut self,
		monitor: MonitorRect,
		pending_window_target: Option<WindowFreezeCaptureTarget>,
	) {
		let now = Instant::now();

		if self.frozen_transition.target_window_id.is_none() {
			self.frozen_transition.target_window_id =
				pending_window_target.map(|target| target.window_id);
		}

		tracing::debug!(
			op = "overlay.freeze_transition_worker_requested",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition.target_window_id,
			capture_windows_hidden = self.capture_windows_hidden,
			since_begin_ms =
				FrozenTransitionRuntime::elapsed_ms_since(self.frozen_transition.started_at, now),
			since_preview_ms =
				FrozenTransitionRuntime::elapsed_ms_since(
					self.frozen_transition.preview_committed_at,
					now,
				),
			"Authoritative frozen capture was requested from the worker."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_worker_requested",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Authoritative frozen capture was requested from the worker.",
		});
	}

	pub(super) fn note_frozen_transition_final_ready(
		&mut self,
		monitor: MonitorRect,
		source: &'static str,
		captured_window_id: Option<u32>,
	) {
		if self.frozen_transition.final_ready_at.is_some() {
			return;
		}

		let now = Instant::now();

		self.frozen_transition.final_ready_at = Some(now);

		tracing::debug!(
			op = "overlay.freeze_transition_final_ready",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition.target_window_id,
			captured_window_id,
			source,
			preview_source = self.frozen_transition.preview_source,
			since_begin_ms =
				FrozenTransitionRuntime::elapsed_ms_since(self.frozen_transition.started_at, now),
			since_preview_ms =
				FrozenTransitionRuntime::elapsed_ms_since(
					self.frozen_transition.preview_committed_at,
					now,
				),
			"Frozen transition final capture is ready."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_final_ready",
			monitor: Some(monitor),
			reason: None,
			source: Some(source),
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id,
			message: "Frozen transition final capture is ready.",
		});
	}

	#[cfg(target_os = "macos")]
	pub(super) fn note_frozen_transition_toolbar_visible(&mut self, monitor: MonitorRect) {
		if self.frozen_transition.toolbar_visible_at.is_some() {
			return;
		}

		let now = Instant::now();

		self.frozen_transition.toolbar_visible_at = Some(now);

		tracing::debug!(
			op = "overlay.freeze_transition_toolbar_visible",
			monitor_id = monitor.id,
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition.target_window_id,
			preview_source = self.frozen_transition.preview_source,
			since_begin_ms =
				FrozenTransitionRuntime::elapsed_ms_since(self.frozen_transition.started_at, now),
			since_preview_ms =
				FrozenTransitionRuntime::elapsed_ms_since(
					self.frozen_transition.preview_committed_at,
					now,
				),
			since_final_ready_ms =
				FrozenTransitionRuntime::elapsed_ms_since(self.frozen_transition.final_ready_at, now),
			"Frozen transition toolbar became visible."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_toolbar_visible",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition toolbar became visible.",
		});
	}

	#[cfg(target_os = "macos")]
	pub(super) fn note_frozen_transition_toolbar_first_draw(&mut self, monitor: MonitorRect) {
		if self.frozen_transition.toolbar_first_draw_at.is_some() {
			return;
		}

		self.frozen_transition.toolbar_first_draw_at = Some(Instant::now());

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_toolbar_first_draw",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition rendered the first visible toolbar frame.",
		});
	}

	#[cfg(target_os = "macos")]
	pub(super) fn note_frozen_transition_badge_slot_armed(&mut self, monitor: MonitorRect) {
		if self.frozen_transition.badge_slot_armed_at.is_some() {
			return;
		}

		self.frozen_transition.badge_slot_armed_at = Some(Instant::now());

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_badge_slot_armed",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition armed overlay badge-slot avoidance after toolbar draw.",
		});
	}

	#[cfg(target_os = "macos")]
	pub(super) fn note_frozen_transition_authoritative_handoff_armed(&self, monitor: MonitorRect) {
		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_authoritative_handoff_armed",
			monitor: Some(monitor),
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition armed authoritative capture fallback.",
		});
	}

	pub(super) fn note_frozen_transition_aborted(&self, message: &str) {
		let now = Instant::now();

		tracing::debug!(
			op = "overlay.freeze_transition_aborted",
			frozen_capture_source = ?self.frozen_capture_source,
			alpha_mode = ?self.config.window_capture_alpha_mode,
			target_window_id = self.frozen_transition.target_window_id,
			preview_source = self.frozen_transition.preview_source,
			message,
			since_begin_ms =
				FrozenTransitionRuntime::elapsed_ms_since(self.frozen_transition.started_at, now),
			since_preview_ms =
				FrozenTransitionRuntime::elapsed_ms_since(
					self.frozen_transition.preview_committed_at,
					now,
				),
			"Frozen transition was aborted before completion."
		);

		self.log_frozen_transition_timing_info(FrozenTransitionTimingInfo {
			op: "overlay.freeze_transition_aborted",
			monitor: None,
			reason: None,
			source: None,
			snapshot_age_ms: None,
			grace_ms: None,
			captured_window_id: None,
			message: "Frozen transition was aborted before completion.",
		});
	}
}

struct FrozenTransitionTimingInfo {
	op: &'static str,
	monitor: Option<MonitorRect>,
	reason: Option<&'static str>,
	source: Option<&'static str>,
	snapshot_age_ms: Option<u128>,
	grace_ms: Option<u128>,
	captured_window_id: Option<u32>,
	message: &'static str,
}
