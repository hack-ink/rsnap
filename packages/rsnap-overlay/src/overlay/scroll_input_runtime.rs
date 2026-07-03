use std::time::Instant;

#[cfg(target_os = "macos")]
use crate::overlay::scroll_capture_timing::{
	SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW,
	SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE,
};
use crate::overlay::scroll_capture_timing::{
	SCROLL_CAPTURE_INPUT_FRESHNESS, SCROLL_CAPTURE_INPUT_MOTION_PRIOR_ROWS_MAX,
};
use crate::overlay::{
	GlobalPoint, MouseScrollDelta, OverlayControl, OverlaySession,
	ScrollCaptureTraceSessionSnapshot, ScrollDirection, ScrollObserveOutcome, WindowId,
};
#[cfg(target_os = "macos")]
use crate::overlay::{MacOSScrollPixelResidual, MacOSScrollWheelEvent, MonitorRect, RectPoints};

#[cfg(target_os = "macos")]
pub(in crate::overlay) const KCG_SCROLL_EVENT_UNIT_LINE: u32 = 1;
#[cfg(target_os = "macos")]
pub(in crate::overlay) const KCG_SCROLL_EVENT_UNIT_PIXEL: u32 = 0;

#[cfg(target_os = "macos")]
const MACOS_SCROLL_PIXEL_DELTA_CLAMP: f64 = 240.0;
#[cfg(target_os = "macos")]
const MACOS_SCROLL_PIXEL_WRAP_MODULUS: f64 = 4_294_967_296.0;
#[cfg(target_os = "macos")]
const MACOS_SCROLL_PIXEL_WRAP_THRESHOLD: f64 = 1_000_000.0;

impl OverlaySession {
	pub(super) fn handle_scroll_mouse_wheel(
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

		if let Err(err) = super::macos_post_scroll_wheel_event(normalized, target_point) {
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
			source_state_id = super::macos_hid_event_source_state_id(),
			"Forwarded scroll wheel event."
		);

		true
	}

	#[cfg(target_os = "macos")]
	pub(super) fn normalize_macos_scroll_wheel_delta(
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
	pub(super) fn normalize_macos_scroll_pixel_component(value: f64) -> f64 {
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

	pub(super) fn scroll_capture_direction_from_wheel_delta(
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

	pub(super) fn scroll_capture_direction_from_external_input_delta_y(
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

	pub(super) fn consume_scroll_capture_downward_motion_rows(&mut self, consumed_rows: u32) {
		if consumed_rows == 0 {
			return;
		}

		let remaining = self.scroll_capture.downward_motion_rows_pending - f64::from(consumed_rows);

		self.scroll_capture.downward_motion_rows_pending = remaining.max(0.0);
	}

	pub(super) fn consume_scroll_capture_downward_motion_rows_for_outcome(
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

	pub(super) fn record_scroll_capture_input_direction_from_overlay_wheel_at(
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

	pub(super) fn apply_external_scroll_input_delta_y(
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

	pub(super) fn scroll_capture_trace_snapshot_at(
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
	pub(super) fn scroll_capture_input_allows_observation(&self) -> bool {
		self.scroll_capture_observation_block_reason().is_none()
	}

	#[cfg(test)]
	pub(super) fn scroll_capture_input_allows_growth(&self) -> bool {
		self.scroll_capture_input_allows_observation()
	}

	#[cfg(test)]
	pub(super) fn scroll_capture_observation_block_reason(&self) -> Option<&'static str> {
		self.scroll_capture_observation_block_reason_at(Instant::now())
	}

	pub(super) fn scroll_capture_observation_block_reason_at(
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
	pub(super) fn scroll_capture_input_age_ms(&self) -> Option<u64> {
		self.scroll_capture_input_age_ms_at(Instant::now())
	}

	pub(super) fn scroll_capture_input_age_ms_at(&self, observation_at: Instant) -> Option<u64> {
		self.scroll_capture.input_direction_at.map(|input_direction_at| {
			u64::try_from(observation_at.saturating_duration_since(input_direction_at).as_millis())
				.unwrap_or(u64::MAX)
		})
	}

	#[cfg(target_os = "macos")]
	pub(super) fn scroll_capture_should_force_stream_refresh_at(&self, now: Instant) -> bool {
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

	pub(super) fn scroll_capture_has_fresh_downward_backlog_at(&self, now: Instant) -> bool {
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
	pub(super) fn scroll_capture_should_schedule_stale_stream_refresh_at(
		&self,
		now: Instant,
	) -> bool {
		if !self.scroll_capture.input_gesture_active {
			return true;
		}

		self.scroll_capture.last_stream_event_at.is_none_or(|last_stream_event_at| {
			now.saturating_duration_since(last_stream_event_at)
				>= SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW
		})
	}

	pub(super) fn scroll_capture_should_allow_post_stall_burst_search_at(
		&self,
		frame_seq: u64,
		now: Instant,
	) -> bool {
		self.scroll_capture.pending_post_stall_burst_after_seq.is_some_and(|after_seq| {
			frame_seq > after_seq && self.scroll_capture_has_fresh_downward_backlog_at(now)
		})
	}

	#[cfg(target_os = "macos")]
	pub(super) fn scroll_capture_should_arm_post_stall_burst_for_time_gap_at(
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

	#[cfg(target_os = "macos")]
	pub(super) fn set_scroll_overlay_mouse_passthrough(&self, passthrough: bool) {
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
	pub(super) fn set_scroll_overlay_mouse_passthrough_persistent(
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
	pub(super) fn arm_scroll_overlay_mouse_passthrough_window(
		&mut self,
		now: Instant,
		reason: &'static str,
	) {
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
	pub(super) fn disarm_scroll_overlay_mouse_passthrough(
		&mut self,
		now: Instant,
		reason: &'static str,
	) {
		self.scroll_capture.overlay_mouse_passthrough_persistent = false;
		self.scroll_capture.overlay_mouse_passthrough_until = None;

		self.set_scroll_overlay_mouse_passthrough_state(now, false, reason);
	}

	#[cfg(target_os = "macos")]
	pub(super) fn sync_scroll_overlay_mouse_passthrough_window(&mut self, now: Instant) {
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
}
