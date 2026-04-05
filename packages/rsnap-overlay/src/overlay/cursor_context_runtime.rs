#[cfg(target_os = "macos")]
#[allow(unused_imports)]
use crate::overlay::{CursorSampleRequest, StartupLiveRgbPlan};
#[allow(unused_imports)]
use crate::overlay::{
	DeviceCursorPointSource, FreezeCaptureTarget, GlobalPoint, Instant,
	LIVE_EVENT_CURSOR_CACHE_TTL, MonitorRect, OverlayMode, OverlaySession,
};

impl OverlaySession {
	pub(super) fn initialize_cursor_state_for_cursor(
		&mut self,
		cursor: GlobalPoint,
		monitor: Option<MonitorRect>,
	) {
		let Some(monitor) = monitor else {
			self.state.cursor = Some(cursor);
			self.state.rgb = None;
			self.cursor_monitor = None;

			return;
		};

		self.update_cursor_state(monitor, cursor);
		self.update_hud_window_position(monitor, cursor);

		if matches!(self.state.mode, OverlayMode::Live) {
			if self.use_fake_hud_blur() {
				self.maybe_request_live_bg(monitor);
			}

			self.request_live_samples_for_cursor(monitor, cursor);
		}
	}

	pub(super) fn monitor_for_cursor_in_rects(
		monitors: &[MonitorRect],
		cursor: GlobalPoint,
	) -> Option<MonitorRect> {
		monitors.iter().copied().find(|monitor| monitor.contains(cursor))
	}

	pub(super) fn prime_startup_cursor_context(
		&mut self,
		cursor: GlobalPoint,
		monitor: Option<MonitorRect>,
	) {
		let Some(monitor) = monitor else {
			self.state.cursor = Some(cursor);
			self.state.rgb = None;
			self.cursor_monitor = None;

			return;
		};

		self.update_cursor_state(monitor, cursor);
		self.update_hud_window_position(monitor, cursor);
	}

	#[cfg(target_os = "macos")]
	pub(super) fn startup_live_rgb_plan(
		startup_monitor: Option<MonitorRect>,
	) -> StartupLiveRgbPlan {
		StartupLiveRgbPlan { focus_window: true, seed_monitor: startup_monitor }
	}

	#[cfg(target_os = "macos")]
	pub(super) fn prime_startup_live_stream_nonblocking(
		&self,
		startup_monitor: Option<MonitorRect>,
	) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		let Some(monitor) = startup_monitor else {
			return;
		};
		let Some(stream) = self.live_sample_stream.as_ref() else {
			return;
		};

		stream.prime_monitor_nonblocking(monitor);
	}

	#[cfg(target_os = "macos")]
	pub(super) fn seed_startup_live_cursor_rgb(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) {
		if !matches!(self.state.mode, OverlayMode::Live) || self.state.rgb.is_some() {
			return;
		}
		if self.startup_aux_window_creation_pending {
			return;
		}

		let Some(stream) = self.live_sample_stream.as_ref() else {
			return;
		};
		let Some((x_px, y_px)) = monitor.local_u32_pixels(cursor) else {
			return;
		};

		if let Some(sample) =
			stream.latest_cursor_sample(monitor, CursorSampleRequest::rgb(x_px, y_px))
			&& let Some(rgb) = sample.rgb
		{
			self.state.rgb = Some(rgb);
		}
	}

	#[cfg(target_os = "macos")]
	pub(super) fn kick_startup_live_sampling(&mut self) {
		if !matches!(self.state.mode, OverlayMode::Live) {
			return;
		}

		let Some(cursor) = self.state.cursor else {
			return;
		};
		let Some(monitor) = self.active_cursor_monitor() else {
			return;
		};

		if self.use_fake_hud_blur() {
			self.maybe_request_live_bg(monitor);
		}

		let _ = self.request_live_samples_for_cursor(monitor, cursor);
	}

	pub(super) fn maybe_request_live_bg(&mut self, monitor: MonitorRect) {
		if !matches!(self.state.mode, OverlayMode::Live) || !self.use_fake_hud_blur() {
			return;
		}
		if self.state.live_bg_monitor == Some(monitor) && self.state.live_bg_image.is_some() {
			return;
		}

		let force = self.state.alt_held && self.state.live_bg_image.is_none();

		if !force && self.last_live_bg_request_at.elapsed() < self.live_bg_request_interval {
			return;
		}

		let Some(worker) = &self.worker else {
			return;
		};

		if worker.request_freeze_capture(monitor, FreezeCaptureTarget::Monitor) {
			self.last_live_bg_request_at = Instant::now();
		}
	}

	pub(super) fn monitor_at(&self, cursor: GlobalPoint) -> Option<MonitorRect> {
		self.windows
			.values()
			.find(|window| window.monitor.contains(cursor))
			.map(|window| window.monitor)
	}

	pub(super) fn resolve_device_cursor_point(
		&self,
		raw: GlobalPoint,
	) -> Option<(MonitorRect, GlobalPoint, DeviceCursorPointSource)> {
		if let Some(monitor) = self.monitor_at(raw) {
			return Some((monitor, raw, DeviceCursorPointSource::DevicePoints));
		}

		for monitor in self.windows.values().map(|window| window.monitor) {
			let sf = f64::from(monitor.scale_factor()).max(1.0);
			let origin_px_x = (monitor.origin.x as f64 * sf).round() as i64;
			let origin_px_y = (monitor.origin.y as f64 * sf).round() as i64;
			let size_px_x = (monitor.width as f64 * sf).round() as i64;
			let size_px_y = (monitor.height as f64 * sf).round() as i64;
			let local_px_x = (raw.x as i64).saturating_sub(origin_px_x);
			let local_px_y = (raw.y as i64).saturating_sub(origin_px_y);

			if local_px_x < 0
				|| local_px_y < 0
				|| local_px_x >= size_px_x
				|| local_px_y >= size_px_y
			{
				continue;
			}

			let local_points_x = (local_px_x as f64 / sf).round() as i64;
			let local_points_y = (local_px_y as f64 / sf).round() as i64;
			let local_points_x = match i32::try_from(local_points_x) {
				Ok(value) => value,
				Err(_) => continue,
			};
			let local_points_y = match i32::try_from(local_points_y) {
				Ok(value) => value,
				Err(_) => continue,
			};
			let candidate = GlobalPoint::new(
				monitor.origin.x.saturating_add(local_points_x),
				monitor.origin.y.saturating_add(local_points_y),
			);

			if monitor.contains(candidate) {
				return Some((monitor, candidate, DeviceCursorPointSource::DevicePixelsFallback));
			}
		}

		None
	}

	pub(super) fn resolve_live_cursor_point(
		&self,
		raw_device: GlobalPoint,
	) -> Option<(MonitorRect, GlobalPoint, DeviceCursorPointSource)> {
		let Some((device_monitor, device_global, device_source)) =
			self.resolve_device_cursor_point(raw_device)
		else {
			let (monitor, global) = self.last_event_cursor?;
			let event_cursor_at = self.last_event_cursor_at?;

			if event_cursor_at.elapsed() > LIVE_EVENT_CURSOR_CACHE_TTL {
				return None;
			}

			return Some((monitor, global, DeviceCursorPointSource::EventRecentFallback));
		};

		if let (Some(event_cursor_at), Some((event_monitor, event_global))) =
			(self.last_event_cursor_at, self.last_event_cursor)
			&& self.state.cursor == Some(device_global)
			&& event_global != device_global
			&& event_cursor_at.elapsed() <= LIVE_EVENT_CURSOR_CACHE_TTL
		{
			return Some((
				event_monitor,
				event_global,
				DeviceCursorPointSource::EventRecentFallback,
			));
		}

		Some((device_monitor, device_global, device_source))
	}

	pub(super) fn active_cursor_monitor(&self) -> Option<MonitorRect> {
		self.cursor_monitor.or_else(|| self.state.cursor.and_then(|cursor| self.monitor_at(cursor)))
	}

	pub(super) fn monitor_for_mode(&self) -> Option<MonitorRect> {
		match self.state.mode {
			OverlayMode::Frozen => self.active_cursor_monitor().or(self.state.monitor),
			OverlayMode::Live => self.active_cursor_monitor(),
		}
	}
}
