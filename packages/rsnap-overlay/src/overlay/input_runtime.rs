use std::time::{Duration, Instant};

#[cfg(not(target_os = "macos"))]
use device_query::DeviceQuery;
#[cfg(target_os = "macos")]
use winit::keyboard::ModifiersState;

use crate::overlay::live_capture_target::LiveClickCaptureTarget;
use crate::overlay::runtime_timing::CURSOR_EVENT_TICK_TTL;
#[cfg(target_os = "macos")]
use crate::overlay::runtime_timing::SLOW_OP_WARN_CURSOR_LOCATION;
use crate::overlay::{
	CursorMoveTrace, DeviceCursorPointSource, ElementState, FrozenSelectionDragCursorMoveTiming,
	FrozenToolbarTool, GlobalPoint, LiveCaptureInteraction, Modifiers, MonitorRect, OverlayControl,
	OverlayMode, OverlaySession, PhysicalPosition, PhysicalSize, WindowId,
};

impl OverlaySession {
	pub(super) fn note_window_focus_change(&mut self, window_id: WindowId, focused: bool) {
		if focused {
			self.focused_window_ids.insert(window_id);

			self.pending_focus_loss_cleanup = false;

			return;
		}

		let was_focused = self.focused_window_ids.remove(&window_id);

		if was_focused && self.focused_window_ids.is_empty() {
			self.pending_focus_loss_cleanup = true;
		}
	}

	pub(super) fn handle_modifiers_changed(&mut self, modifiers: &Modifiers) -> OverlayControl {
		self.keyboard_modifiers = modifiers.state();

		OverlayControl::Continue
	}

	#[cfg(target_os = "macos")]
	pub(super) fn handle_modifiers_state_changed(
		&mut self,
		modifiers: ModifiersState,
	) -> OverlayControl {
		self.keyboard_modifiers = modifiers;

		OverlayControl::Continue
	}

	#[cfg(not(target_os = "macos"))]
	pub(super) fn sample_mouse_location(&mut self) -> GlobalPoint {
		let Some(cursor_device) = self.cursor_device.as_ref() else {
			return GlobalPoint::new(0, 0);
		};
		let mouse = cursor_device.get_mouse();

		GlobalPoint::new(mouse.coords.0, mouse.coords.1)
	}

	#[cfg(target_os = "macos")]
	pub(super) fn sample_mouse_location(&mut self) -> GlobalPoint {
		let started_at = Instant::now();
		let point = super::macos_mouse_location().unwrap_or(GlobalPoint::new(0, 0));
		let elapsed = started_at.elapsed();

		self.slow_op_logger.warn_if_slow(
			"overlay.macos_cursor_location",
			elapsed,
			SLOW_OP_WARN_CURSOR_LOCATION,
			|| format!("sample point=({}, {})", point.x, point.y),
		);

		point
	}

	pub(super) fn last_fresh_event_cursor(&self) -> Option<(MonitorRect, GlobalPoint)> {
		self.last_fresh_event_cursor_with_ttl(CURSOR_EVENT_TICK_TTL)
	}

	pub(super) fn last_fresh_event_cursor_with_ttl(
		&self,
		ttl: Duration,
	) -> Option<(MonitorRect, GlobalPoint)> {
		let event_cursor_at = self.last_event_cursor_at?;
		let event_cursor = self.last_event_cursor?;

		if event_cursor_at.elapsed() > ttl {
			return None;
		}

		Some(event_cursor)
	}

	pub(super) fn set_alt_held(&mut self, alt: bool) {
		if self.state.alt_held == alt {
			return;
		}
		if alt && !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		self.state.alt_held = alt;

		if !alt {
			self.handle_alt_release();

			return;
		}
		if self.live_capture_hides_auxiliary_windows() {
			self.state.loupe = None;

			self.set_alt_loupe_window_visible(None, false);

			return;
		}

		let Some((monitor, cursor)) = self.loupe_activation_cursor_context() else {
			return;
		};

		self.set_alt_loupe_window_visible(Some(monitor), true);

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}

		self.request_live_alt_samples(monitor, cursor);
	}

	pub(super) fn apply_loupe_activation_input(&mut self, pressed: bool, repeat: bool) -> bool {
		let previous_alt_held = self.state.alt_held;

		if pressed && !repeat {
			self.set_alt_held(!self.state.alt_held);
		}

		previous_alt_held != self.state.alt_held
	}

	pub(super) fn apply_loupe_activation_key_event(&mut self, pressed: bool, repeat: bool) -> bool {
		if self.loupe_activation_key_down == pressed && !repeat {
			return false;
		}
		if !matches!(self.state.mode, OverlayMode::Live) {
			self.loupe_activation_key_down = false;

			return false;
		}

		self.loupe_activation_key_down = pressed;

		if !pressed && !self.state.alt_held {
			return false;
		}
		if pressed && !self.loupe_activation_shortcut_available() {
			return false;
		}

		self.apply_loupe_activation_input(pressed, repeat)
	}

	pub(super) fn clear_loupe_activation_on_focus_loss(&mut self) {
		if !self.loupe_activation_key_down {
			return;
		}

		self.loupe_activation_key_down = false;
	}

	pub(super) fn maybe_clear_loupe_activation_after_focus_loss(&mut self) {
		if !self.pending_focus_loss_cleanup || !self.focused_window_ids.is_empty() {
			return;
		}

		self.pending_focus_loss_cleanup = false;

		self.clear_loupe_activation_on_focus_loss();
	}

	pub(super) fn request_redraw_for_alt_state_change(&mut self) -> OverlayControl {
		if matches!(self.state.mode, OverlayMode::Live) {
			self.request_redraw_hud_window();

			if !self.live_loupe_uses_hud_window()
				&& (self.state.alt_held || self.loupe_window_visible)
			{
				self.request_redraw_loupe_window();
			}
		}

		OverlayControl::Continue
	}

	pub(super) fn loupe_activation_cursor_context(&mut self) -> Option<(MonitorRect, GlobalPoint)> {
		if let Some((monitor, cursor)) = self.last_fresh_event_cursor() {
			self.seed_loupe_activation_cursor_context(monitor, cursor);

			return Some((monitor, cursor));
		}

		let cursor = self.sample_mouse_location();
		let Some(monitor) = self.monitor_at(cursor) else {
			if self.state.cursor.is_none() {
				self.state.cursor = Some(cursor);
			}

			return self.active_cursor_monitor().zip(self.state.cursor);
		};

		self.seed_loupe_activation_cursor_context(monitor, cursor);

		Some((monitor, cursor))
	}

	pub(super) fn seed_loupe_activation_cursor_context(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) {
		let old_monitor = self.active_cursor_monitor();
		let old_cursor = self.state.cursor;

		match self.state.mode {
			OverlayMode::Live => {
				self.update_cursor_for_live_move(old_monitor, old_cursor, monitor, cursor)
			},
			OverlayMode::Frozen => self.update_cursor_state(monitor, cursor),
		}
	}

	fn handle_alt_release(&mut self) {
		self.state.loupe = None;
		self.loupe_outer_pos = None;
		self.pending_loupe_outer_pos = None;

		self.set_alt_loupe_window_visible(None, false);

		if matches!(self.state.mode, OverlayMode::Live) {
			self.request_redraw_hud_window();
		}
	}

	pub(super) fn set_alt_loupe_window_visible(
		&mut self,
		monitor: Option<MonitorRect>,
		visible: bool,
	) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			self.loupe_window_visible = false;

			self.reset_loupe_window_warmup_redraws();

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(false);
				loupe_window.window.request_redraw();
			}

			return;
		}
		if self.live_loupe_uses_hud_window() {
			self.loupe_window_visible = false;

			self.reset_loupe_window_warmup_redraws();

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(false);
			}

			return;
		}
		if visible {
			let Some(monitor) = monitor else {
				return;
			};

			#[cfg(target_os = "macos")]
			if self.loupe_window.is_none() {
				self.request_aux_window_creation_if_needed();

				return;
			}

			self.maybe_apply_pending_startup_aux_live_stream_filter_upgrade(monitor);

			let visible = self.update_loupe_window_position(monitor);
			let was_visible = self.loupe_window_visible;

			self.loupe_window_visible = visible;

			if visible {
				self.force_apply_pending_loupe_window_move();
			}
			if visible {
				if !was_visible {
					self.maybe_start_loupe_window_warmup_redraw();
				}
			} else {
				self.reset_loupe_window_warmup_redraws();
			}

			if let Some(loupe_window) = self.loupe_window.as_ref() {
				loupe_window.window.set_visible(visible);
				loupe_window.window.request_redraw();
			}

			return;
		}

		self.loupe_window_visible = false;

		self.reset_loupe_window_warmup_redraws();

		if let Some(loupe_window) = self.loupe_window.as_ref() {
			loupe_window.window.set_visible(false);
			loupe_window.window.request_redraw();
		}
	}

	pub(super) fn request_live_alt_samples(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		let sample_updated = self.request_live_cursor_sample(monitor, cursor, true);
		let apply = self.live_sample_request_redraw_intent(false, sample_updated, true);

		if apply.any_changed() {
			self.request_redraw_live_sample_targets(monitor, apply);
		}
	}

	pub(super) fn handle_cursor_moved(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
	) -> OverlayControl {
		let old_monitor = self.active_cursor_monitor();
		let Some(overlay_window) = self.windows.get(&window_id) else {
			return self.handle_cursor_moved_without_overlay_window(window_id, old_monitor);
		};
		let window_monitor = overlay_window.monitor;
		let scale_factor = overlay_window.window.scale_factor();
		let window_size = overlay_window.window.inner_size();

		self.handle_cursor_moved_with_overlay_window(
			window_id,
			position,
			old_monitor,
			window_monitor,
			scale_factor,
			window_size,
		)
	}

	#[cfg(target_os = "macos")]
	pub(super) fn handle_native_overlay_pointer_moved(
		&mut self,
		monitor: MonitorRect,
		global: GlobalPoint,
	) -> OverlayControl {
		let should_trace_frozen_selection_drag_timing =
			self.should_trace_frozen_selection_drag_timing();
		let cursor_move_started_at = should_trace_frozen_selection_drag_timing.then(Instant::now);
		let now = Instant::now();
		let old_monitor = self.active_cursor_monitor();
		let old_cursor = self.state.cursor;
		let source = DeviceCursorPointSource::EventRecentFallback;
		let trace = CursorMoveTrace {
			window_id: WindowId::dummy(),
			position: PhysicalPosition::new(0.0, 0.0),
			old_cursor,
			device_cursor: global,
			event_global: global,
			monitor,
			global,
			source,
		};

		self.last_event_cursor = Some((monitor, global));
		self.last_event_cursor_at = Some(now);

		self.trace_cursor_moved_with_mapping(trace);

		let timing = self.run_cursor_move_updates(
			should_trace_frozen_selection_drag_timing,
			cursor_move_started_at,
			old_monitor,
			monitor,
			global,
			global,
		);

		if should_trace_frozen_selection_drag_timing {
			self.trace_frozen_selection_drag_cursor_move(monitor, old_monitor, old_cursor, timing);
		}
		if self.update_frozen_brush_stroke(global) {
			self.request_redraw_for_monitor(monitor);
		}

		OverlayControl::Continue
	}

	pub(super) fn handle_cursor_moved_with_overlay_window(
		&mut self,
		window_id: WindowId,
		position: PhysicalPosition<f64>,
		old_monitor: Option<MonitorRect>,
		window_monitor: MonitorRect,
		scale_factor: f64,
		window_size: PhysicalSize<u32>,
	) -> OverlayControl {
		let should_trace_frozen_selection_drag_timing =
			self.should_trace_frozen_selection_drag_timing();
		let cursor_move_started_at = should_trace_frozen_selection_drag_timing.then(Instant::now);
		let now = Instant::now();
		let event_global = Self::overlay_window_event_global_position(
			window_monitor,
			scale_factor,
			window_size,
			position,
		);
		let frozen_selection_drag_global = self.frozen_selection_drag_cursor_move_global(
			window_monitor,
			scale_factor,
			window_size,
			position,
			event_global,
		);
		let monitor = window_monitor;
		let global = event_global;
		let source = DeviceCursorPointSource::EventRecentFallback;
		let device_cursor = event_global;

		self.last_event_cursor = Some((monitor, event_global));
		self.last_event_cursor_at = Some(now);

		let old_cursor = self.state.cursor;
		let trace = CursorMoveTrace {
			window_id,
			position,
			old_cursor,
			device_cursor,
			event_global,
			monitor,
			global,
			source,
		};

		self.trace_cursor_moved_with_mapping(trace);

		let timing = self.run_cursor_move_updates(
			should_trace_frozen_selection_drag_timing,
			cursor_move_started_at,
			old_monitor,
			monitor,
			global,
			frozen_selection_drag_global,
		);

		if should_trace_frozen_selection_drag_timing {
			self.trace_frozen_selection_drag_cursor_move(monitor, old_monitor, old_cursor, timing);
		}
		if self.update_frozen_brush_stroke(global) {
			self.request_redraw_for_monitor(monitor);
		}

		OverlayControl::Continue
	}

	pub(super) fn overlay_window_event_global_position(
		window_monitor: MonitorRect,
		scale_factor: f64,
		window_size: PhysicalSize<u32>,
		position: PhysicalPosition<f64>,
	) -> GlobalPoint {
		let scale_factor = scale_factor.max(f64::MIN_POSITIVE);
		let logical_width = ((window_size.width as f64) / scale_factor).max(1.0);
		let logical_height = ((window_size.height as f64) / scale_factor).max(1.0);
		let max_local_x = logical_width as i32 - 1;
		let max_local_y = logical_height as i32 - 1;
		let local_x = (position.x / scale_factor).round() as i32;
		let local_y = (position.y / scale_factor).round() as i32;

		GlobalPoint::new(
			window_monitor.origin.x + local_x.clamp(0, max_local_x),
			window_monitor.origin.y + local_y.clamp(0, max_local_y),
		)
	}

	fn frozen_selection_drag_cursor_move_global(
		&self,
		window_monitor: MonitorRect,
		scale_factor: f64,
		window_size: PhysicalSize<u32>,
		position: PhysicalPosition<f64>,
		default_global: GlobalPoint,
	) -> GlobalPoint {
		if !matches!(self.state.mode, OverlayMode::Frozen) || !self.frozen_selection_drag.active {
			return default_global;
		}

		Self::overlay_window_frozen_selection_drag_global_position(
			window_monitor,
			scale_factor,
			window_size,
			position,
		)
	}

	pub(super) fn overlay_window_frozen_selection_drag_global_position(
		window_monitor: MonitorRect,
		scale_factor: f64,
		window_size: PhysicalSize<u32>,
		position: PhysicalPosition<f64>,
	) -> GlobalPoint {
		let scale_factor = scale_factor.max(f64::MIN_POSITIVE);
		let logical_width = ((window_size.width as f64) / scale_factor).max(1.0);
		let logical_height = ((window_size.height as f64) / scale_factor).max(1.0);
		let max_local_x = logical_width.ceil() as i32 - 1;
		let max_local_y = logical_height.ceil() as i32 - 1;
		let local_x = ((position.x / scale_factor).floor() as i32).clamp(0, max_local_x);
		let local_y = ((position.y / scale_factor).floor() as i32).clamp(0, max_local_y);

		GlobalPoint::new(window_monitor.origin.x + local_x, window_monitor.origin.y + local_y)
	}

	fn handle_cursor_moved_without_overlay_window(
		&mut self,
		window_id: WindowId,
		old_monitor: Option<MonitorRect>,
	) -> OverlayControl {
		let should_trace_frozen_selection_drag_timing =
			self.should_trace_frozen_selection_drag_timing();
		let cursor_move_started_at = should_trace_frozen_selection_drag_timing.then(Instant::now);

		if self.should_ignore_live_auxiliary_cursor_event(window_id) {
			return OverlayControl::Continue;
		}

		let now = Instant::now();
		let raw = self.sample_mouse_location();
		let Some((monitor, global, source)) = self.resolve_device_cursor_point(raw) else {
			return OverlayControl::Continue;
		};
		let old_cursor = self.state.cursor;

		self.last_event_cursor = Some((monitor, global));
		self.last_event_cursor_at = Some(now);

		if tracing::enabled!(tracing::Level::TRACE) {
			tracing::trace!(
				window_id = ?window_id,
				window_known = false,
				old_cursor = ?old_cursor,
				device_cursor = ?global,
				event_cursor = ?global,
				source = source.as_str(),
				"CursorMoved (no overlay window mapping)."
			);
		}

		let timing = self.run_cursor_move_updates(
			should_trace_frozen_selection_drag_timing,
			cursor_move_started_at,
			old_monitor,
			monitor,
			global,
			global,
		);

		if should_trace_frozen_selection_drag_timing {
			self.trace_frozen_selection_drag_cursor_move(monitor, old_monitor, old_cursor, timing);
		}
		if self.update_frozen_brush_stroke(global) {
			self.request_redraw_for_monitor(monitor);
		}

		OverlayControl::Continue
	}

	fn run_cursor_move_updates(
		&mut self,
		should_trace_frozen_selection_drag_timing: bool,
		cursor_move_started_at: Option<Instant>,
		old_monitor: Option<MonitorRect>,
		monitor: MonitorRect,
		global: GlobalPoint,
		frozen_selection_drag_global: GlobalPoint,
	) -> FrozenSelectionDragCursorMoveTiming {
		let old_cursor = self.state.cursor;
		let cursor_update_elapsed =
			Self::measure_duration_if(should_trace_frozen_selection_drag_timing, || {
				self.update_cursor_for_live_move(old_monitor, old_cursor, monitor, global)
			});
		let previous_drag_rect = self.state.drag_rect;
		let live_drag_update_elapsed =
			Self::measure_duration_if(should_trace_frozen_selection_drag_timing, || {
				self.update_live_drag_rect(monitor, global);
			});
		let (frozen_rect_changed, frozen_drag_update_elapsed) =
			if should_trace_frozen_selection_drag_timing {
				let frozen_drag_update_started_at = Instant::now();
				let frozen_rect_changed =
					self.update_frozen_selection_drag_rect(frozen_selection_drag_global);

				self.update_frozen_arrow_drag(global);
				self.update_frozen_mosaic_drag_rect(global);
				self.update_frozen_spotlight_drag_rect(global);
				self.update_frozen_text_edit_drag_anchor(global);

				(frozen_rect_changed, Some(frozen_drag_update_started_at.elapsed()))
			} else {
				let frozen_rect_changed =
					self.update_frozen_selection_drag_rect(frozen_selection_drag_global);

				self.update_frozen_arrow_drag(global);
				self.update_frozen_mosaic_drag_rect(global);
				self.update_frozen_spotlight_drag_rect(global);
				self.update_frozen_text_edit_drag_anchor(global);

				(frozen_rect_changed, None)
			};
		let sync_cursor_icons_elapsed =
			Self::measure_duration_if(should_trace_frozen_selection_drag_timing, || {
				self.sync_overlay_cursor_icons();
			});
		let request_samples_elapsed =
			Self::measure_duration_if(should_trace_frozen_selection_drag_timing, || {
				self.request_cursor_move_samples(monitor, global);
			});

		if let Some(old_monitor) = old_monitor
			&& old_monitor != monitor
		{
			self.request_redraw_for_monitor(old_monitor);
		}

		if Self::live_overlay_redraw_needed_for_cursor_update(
			old_monitor,
			monitor,
			previous_drag_rect,
			self.state.drag_rect,
		) {
			self.request_redraw_for_monitor(monitor);
		}

		FrozenSelectionDragCursorMoveTiming {
			cursor_update_elapsed: cursor_update_elapsed.unwrap_or_default(),
			live_drag_update_elapsed: live_drag_update_elapsed.unwrap_or_default(),
			frozen_drag_update_elapsed: frozen_drag_update_elapsed.unwrap_or_default(),
			frozen_rect_changed,
			sync_cursor_icons_elapsed: sync_cursor_icons_elapsed.unwrap_or_default(),
			request_samples_elapsed: request_samples_elapsed.unwrap_or_default(),
			total_elapsed: cursor_move_started_at
				.map_or(Duration::ZERO, |started_at| started_at.elapsed()),
		}
	}

	fn measure_duration_if(enabled: bool, operation: impl FnOnce()) -> Option<Duration> {
		if enabled {
			let started_at = Instant::now();

			operation();

			Some(started_at.elapsed())
		} else {
			operation();

			None
		}
	}

	fn should_ignore_live_auxiliary_cursor_event(&self, window_id: WindowId) -> bool {
		Self::should_ignore_live_auxiliary_cursor_event_for_role(
			self.state.mode,
			self.is_auxiliary_capture_window(window_id),
		)
	}

	fn is_auxiliary_capture_window(&self, window_id: WindowId) -> bool {
		self.hud_window.as_ref().is_some_and(|window| window.window.id() == window_id)
			|| self.loupe_window.as_ref().is_some_and(|window| window.window.id() == window_id)
			|| self.toolbar_window.as_ref().is_some_and(|window| window.window.id() == window_id)
			|| self
				.scroll_preview_window
				.as_ref()
				.is_some_and(|window| window.window.id() == window_id)
	}

	fn should_ignore_live_auxiliary_cursor_event_for_role(
		mode: OverlayMode,
		is_auxiliary_window: bool,
	) -> bool {
		matches!(mode, OverlayMode::Live) && is_auxiliary_window
	}

	pub(super) fn current_device_cursor(&mut self) -> GlobalPoint {
		self.sample_mouse_location()
	}

	pub(super) fn current_frozen_interaction_cursor(&mut self) -> GlobalPoint {
		if let Some((_, cursor)) = self.last_fresh_event_cursor() {
			return cursor;
		}
		if let Some(cursor) = self.state.cursor {
			return cursor;
		}

		let raw = self.current_device_cursor();

		self.resolve_device_cursor_point(raw).map(|(_, cursor, _)| cursor).unwrap_or(raw)
	}

	fn trace_cursor_moved_with_mapping(&self, trace: CursorMoveTrace) {
		if !tracing::enabled!(tracing::Level::TRACE) {
			return;
		}

		let delta_x =
			trace.global.x.abs_diff(trace.old_cursor.map_or(trace.global.x, |point| point.x));
		let delta_y =
			trace.global.y.abs_diff(trace.old_cursor.map_or(trace.global.y, |point| point.y));

		tracing::trace!(
			window_id = ?trace.window_id,
			window_known = true,
			window_position = ?trace.position,
			old_cursor = ?trace.old_cursor,
			device_cursor = ?trace.device_cursor,
			event_cursor = ?trace.event_global,
			source = trace.source.as_str(),
			monitor_id = trace.monitor.id,
			cursor_delta_x = delta_x,
			cursor_delta_y = delta_y,
			"CursorMoved coordinate source: {}.",
			trace.source.as_str()
		);
	}

	fn trace_frozen_selection_drag_cursor_move(
		&self,
		monitor: MonitorRect,
		old_monitor: Option<MonitorRect>,
		old_cursor: Option<GlobalPoint>,
		timing: FrozenSelectionDragCursorMoveTiming,
	) {
		if !self.should_trace_frozen_selection_drag_timing() {
			return;
		}

		tracing::trace!(
			op = "overlay.frozen_selection_drag.cursor_move_timing",
			monitor_id = monitor.id,
			old_monitor_id = old_monitor.map(|target| target.id),
			old_cursor = ?old_cursor,
			cursor = ?self.state.cursor,
			interaction = ?self.frozen_selection_drag.interaction,
			frozen_rect_changed = timing.frozen_rect_changed,
			cursor_update_us = timing.cursor_update_elapsed.as_micros(),
			live_drag_update_us = timing.live_drag_update_elapsed.as_micros(),
			frozen_drag_update_us = timing.frozen_drag_update_elapsed.as_micros(),
			sync_cursor_icons_us = timing.sync_cursor_icons_elapsed.as_micros(),
			request_samples_us = timing.request_samples_elapsed.as_micros(),
			total_us = timing.total_elapsed.as_micros(),
			overlay_window_count = self.windows.len(),
			"Frozen selection drag cursor move timing."
		);
	}

	pub(super) fn should_trace_frozen_selection_drag_timing(&self) -> bool {
		tracing::enabled!(tracing::Level::TRACE)
			&& matches!(self.state.mode, OverlayMode::Frozen)
			&& self.frozen_selection_drag.active
	}

	pub(super) fn update_cursor_for_live_move(
		&mut self,
		old_monitor: Option<MonitorRect>,
		old_cursor: Option<GlobalPoint>,
		monitor: MonitorRect,
		global: GlobalPoint,
	) {
		if self.frozen_display_handoff_pending() {
			return;
		}

		self.update_cursor_state(monitor, global);
		self.update_hud_window_position(monitor, global);

		if Self::live_hud_redraw_needed_for_cursor_update(old_cursor, global, old_monitor, monitor)
		{
			self.request_redraw_hud_window();
		}
		if self.should_try_pending_follow_window_move_on_live_cursor_update() {
			self.maybe_apply_pending_hud_and_loupe_moves();
		}
		if matches!(self.state.mode, OverlayMode::Live) && self.use_fake_hud_blur() {
			if self.state.live_bg_monitor != Some(monitor) {
				self.state.live_bg_monitor = None;
				self.state.live_bg_image = None;
			}

			self.maybe_request_live_bg(monitor);
		}
	}

	fn request_cursor_move_samples(&mut self, monitor: MonitorRect, global: GlobalPoint) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}
		if self.frozen_display_handoff_pending() {
			return;
		}
		if self.pending_click_hit_test_request_id.is_some() {
			return;
		}

		let press_pending = self.live_capture_interaction_is_press_pending();
		let is_dragging_window = self.live_capture_interaction_is_dragging();
		let had_snapshot_update = if press_pending || is_dragging_window || self.state.alt_held {
			false
		} else {
			self.apply_live_hover_cache_state(monitor, global)
		};
		let sample_requested =
			self.request_live_cursor_sample(monitor, global, self.state.alt_held);

		if !press_pending && !is_dragging_window && !self.state.alt_held {
			let _ = self.request_live_window_list_refresh_if_needed();
		}

		let apply = self.live_sample_request_redraw_intent(
			had_snapshot_update,
			sample_requested,
			self.state.alt_held || self.loupe_window_visible,
		);

		if apply.any_changed() {
			self.request_redraw_live_sample_targets(monitor, apply);
		}
	}

	pub(super) fn handle_left_mouse_input(
		&mut self,
		window_id: WindowId,
		state: ElementState,
	) -> OverlayControl {
		let monitor = self
			.windows
			.get(&window_id)
			.map(|w| w.monitor)
			.or_else(|| self.active_cursor_monitor())
			.or(self.state.monitor);
		let Some(monitor) = monitor else {
			return OverlayControl::Continue;
		};

		if matches!(self.state.mode, OverlayMode::Frozen) {
			return self.handle_frozen_left_mouse_input(monitor, state);
		}
		if !matches!(self.state.mode, OverlayMode::Live) {
			return OverlayControl::Continue;
		}
		if self.frozen_display_handoff_pending() {
			return OverlayControl::Continue;
		}

		self.maybe_timeout_pending_click_hit_test(Instant::now());

		match state {
			ElementState::Pressed => {
				let raw_cursor = self.current_device_cursor();
				let (press_monitor, press_global) = if let Some((press_monitor, press_global, _)) =
					self.resolve_live_cursor_point(raw_cursor)
				{
					(press_monitor, press_global)
				} else {
					(monitor, raw_cursor)
				};

				self.handle_live_overlay_left_mouse_input(press_monitor, press_global, state)
			},
			ElementState::Released => {
				let raw_cursor = self.current_device_cursor();
				let release_global = if let Some((_, release_global, _)) =
					self.resolve_live_cursor_point(raw_cursor)
				{
					release_global
				} else {
					raw_cursor
				};

				self.handle_live_overlay_left_mouse_input(monitor, release_global, state)
			},
		}
	}

	pub(super) fn handle_live_overlay_left_mouse_input(
		&mut self,
		monitor: MonitorRect,
		global: GlobalPoint,
		state: ElementState,
	) -> OverlayControl {
		if matches!(self.state.mode, OverlayMode::Frozen) {
			self.last_event_cursor = Some((monitor, global));
			self.last_event_cursor_at = Some(Instant::now());

			self.update_cursor_state(monitor, global);

			return self.handle_frozen_left_mouse_input(monitor, state);
		}
		if !matches!(self.state.mode, OverlayMode::Live) {
			return OverlayControl::Continue;
		}
		if self.frozen_display_handoff_pending() {
			return OverlayControl::Continue;
		}

		self.maybe_timeout_pending_click_hit_test(Instant::now());

		match state {
			ElementState::Pressed => {
				if self.live_capture_interaction_is_press_pending()
					|| self.live_capture_interaction_is_dragging()
				{
					return OverlayControl::Continue;
				}

				self.last_event_cursor = Some((monitor, global));
				self.last_event_cursor_at = Some(Instant::now());

				self.update_cursor_state(monitor, global);
				self.update_hud_window_position(monitor, global);
				self.begin_live_capture_press(monitor, global);

				if matches!(
					self.live_capture_interaction,
					LiveCaptureInteraction::PressPending { click_target: None, .. }
				) {
					self.request_click_capture_hit_test(monitor, global);
				}

				self.reset_toolbar_pointer_state();
				self.request_redraw_for_monitor(monitor);

				OverlayControl::Continue
			},
			ElementState::Released => {
				match self.live_capture_interaction {
					LiveCaptureInteraction::PressPending {
						monitor: press_monitor,
						press_global,
						click_target,
						..
					} => {
						if let Some(target) = click_target {
							self.begin_frozen_capture_from_click(press_monitor, target, global);
						} else if self.pending_click_hit_test_request_id.is_some() {
							self.set_live_capture_interaction(
								LiveCaptureInteraction::PressPending {
									monitor: press_monitor,
									press_global,
									click_target: None,
									release_global: Some(global),
									released: true,
								},
							);
						} else {
							self.begin_frozen_capture_from_click(
								press_monitor,
								LiveClickCaptureTarget::fullscreen_fallback(),
								global,
							);
						}
					},
					LiveCaptureInteraction::DraggingSelection { monitor, .. } => {
						if let Some(drag_rect) =
							self.state.drag_rect.filter(|rect| rect.monitor_id == monitor.id)
						{
							self.begin_frozen_capture_from_drag(monitor, drag_rect.rect, global);
						} else {
							self.set_live_capture_interaction(LiveCaptureInteraction::Idle);
						}
					},
					_ => {},
				}

				OverlayControl::Continue
			},
		}
	}

	pub(super) fn handle_frozen_left_mouse_input(
		&mut self,
		monitor: MonitorRect,
		state: ElementState,
	) -> OverlayControl {
		self.reset_toolbar_pointer_state();

		if self.frozen_text_tool_active() {
			match state {
				ElementState::Pressed => {
					let cursor = self.current_device_cursor();
					let started_drag = self.begin_frozen_text_edit_drag_at(monitor, cursor);

					if !started_drag {
						let started = self.begin_frozen_text_edit_at(monitor, cursor);

						if !started {
							let _ = self.finish_frozen_text_editing(true);
						}
					}

					self.sync_overlay_cursor_icons();
				},
				ElementState::Released => {
					let stopped_drag = self.stop_frozen_text_edit_drag();

					if stopped_drag {
						self.sync_overlay_cursor_icons();
					}
				},
			}

			self.request_redraw_for_monitor(monitor);

			return OverlayControl::Continue;
		}
		if self.frozen_text_edit.is_some() {
			let _ = self.finish_frozen_text_editing(true);
		}

		match state {
			ElementState::Pressed => {
				let cursor = self.current_frozen_interaction_cursor();

				match self.toolbar_state.selected_tool {
					FrozenToolbarTool::Pen => {
						let _ = self.begin_frozen_brush_stroke(cursor);
					},
					FrozenToolbarTool::Arrow => {
						let _ = self.begin_frozen_arrow_drag(cursor);
					},
					FrozenToolbarTool::Spotlight => {
						let _ = self.begin_frozen_spotlight_drag(cursor);
					},
					FrozenToolbarTool::Mosaic => {
						let _ = self.begin_frozen_mosaic_drag(cursor);
					},
					_ => {
						let _ = self.begin_frozen_selection_drag(cursor);
					},
				}

				self.sync_overlay_cursor_icons();
			},
			ElementState::Released => {
				let _ = self.commit_frozen_arrow_drag();
				let _ = self.commit_frozen_spotlight_drag();
				let _ = self.commit_frozen_mosaic_drag();
				let _ = self.finish_frozen_brush_stroke();

				self.stop_frozen_selection_drag();
				self.sync_overlay_cursor_icons();
			},
		}

		self.request_redraw_for_monitor(monitor);

		OverlayControl::Continue
	}
}
