use std::sync::Arc;

use color_eyre::eyre::Result;
use image::RgbaImage;
use thiserror::Error;

use crate::state::{
	GlobalPoint, LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints, Rgb, WindowHit,
	WindowListSnapshot,
};

/// Capture backend contract used by the overlay worker.
pub trait CaptureBackend: Send {
	/// Returns the current global cursor position when the backend can provide it.
	fn global_cursor_position(&mut self) -> Result<Option<GlobalPoint>> {
		Ok(None)
	}

	/// Captures a full monitor image for the provided monitor descriptor.
	fn capture_monitor(&mut self, monitor: MonitorRect) -> Result<RgbaImage>;

	/// Captures a monitor sub-rectangle in monitor-local pixels.
	fn capture_monitor_region(
		&mut self,
		_monitor: MonitorRect,
		_rect_px: RectPoints,
	) -> Result<RgbaImage> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	/// Captures a fresh monitor region suitable for scroll-capture sampling when supported.
	fn capture_monitor_region_for_scroll_capture(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Result<Option<RgbaImage>> {
		self.capture_monitor_region(monitor, rect_px).map(Some)
	}

	/// Samples the RGB color under a global point on the target monitor.
	fn pixel_rgb_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
	) -> Result<Option<Rgb>>;

	/// Samples the live cursor color and optional loupe patch from the target monitor.
	fn live_sample_cursor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		want_patch: bool,
		patch_width_px: u32,
		patch_height_px: u32,
	) -> Result<LiveCursorSample> {
		let rgb = self.pixel_rgb_in_monitor(monitor, point)?;
		let patch = if want_patch {
			self.rgba_patch_in_monitor(monitor, point, patch_width_px, patch_height_px)?
		} else {
			None
		};

		Ok(LiveCursorSample { rgb, patch })
	}

	/// Hit-tests the topmost window under the given point on the target monitor.
	fn hit_test_window_in_monitor(
		&mut self,
		_monitor: MonitorRect,
		_point: GlobalPoint,
	) -> Result<Option<WindowHit>> {
		Ok(None)
	}

	/// Captures a single window by window identifier when supported.
	fn capture_window(&mut self, _window_id: u32) -> Result<RgbaImage> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	/// Samples an RGBA patch around a global point on the target monitor.
	fn rgba_patch_in_monitor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		width_px: u32,
		height_px: u32,
	) -> Result<Option<RgbaImage>>;

	/// Refreshes any cached full-monitor frame used for repeated sampling operations.
	fn refresh_monitor_cache(
		&mut self,
		_monitor: MonitorRect,
	) -> Result<Arc<MonitorImageSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	/// Returns the latest cached full-monitor frame when available.
	fn latest_monitor_cache_snapshot(&self) -> Option<Arc<MonitorImageSnapshot>> {
		None
	}

	/// Refreshes any cached window-list snapshot used for hit testing.
	fn refresh_window_cache(&mut self) -> Result<Arc<WindowListSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "capture backend" }.into())
	}

	/// Returns the latest cached window-list snapshot when available.
	fn latest_window_cache_snapshot(&self) -> Option<Arc<WindowListSnapshot>> {
		None
	}
}

#[derive(Debug, Error)]
/// Backend-specific capture errors surfaced through the overlay worker.
pub enum CaptureBackendError {
	#[error("screen capture is not supported on this platform (backend: {backend})")]
	NotSupported { backend: &'static str },

	#[cfg(not(target_os = "macos"))]
	#[error("no monitor matched rect: {monitor:?}")]
	MonitorNotFound { monitor: MonitorRect },

	#[error("no window matched id: {window_id}")]
	WindowNotFound { window_id: u32 },
}

/// No-op backend used by tests and unsupported-code paths.
pub(super) struct StubCaptureBackend {}
impl StubCaptureBackend {
	#[must_use]
	/// Creates a stub backend that reports unsupported operations.
	pub fn new() -> Self {
		Self {}
	}
}

impl Default for StubCaptureBackend {
	fn default() -> Self {
		Self::new()
	}
}

impl CaptureBackend for StubCaptureBackend {
	fn capture_monitor(&mut self, _monitor: MonitorRect) -> Result<RgbaImage> {
		Err(CaptureBackendError::NotSupported { backend: "stub" }.into())
	}

	fn capture_monitor_region(
		&mut self,
		_monitor: MonitorRect,
		_rect_px: RectPoints,
	) -> Result<RgbaImage> {
		Err(CaptureBackendError::NotSupported { backend: "stub" }.into())
	}

	fn pixel_rgb_in_monitor(
		&mut self,
		_monitor: MonitorRect,
		_point: GlobalPoint,
	) -> Result<Option<Rgb>> {
		Ok(None)
	}

	fn rgba_patch_in_monitor(
		&mut self,
		_monitor: MonitorRect,
		_point: GlobalPoint,
		_width_px: u32,
		_height_px: u32,
	) -> Result<Option<RgbaImage>> {
		Ok(None)
	}

	fn capture_window(&mut self, _window_id: u32) -> Result<RgbaImage> {
		Err(CaptureBackendError::NotSupported { backend: "stub" }.into())
	}

	fn refresh_monitor_cache(
		&mut self,
		_monitor: MonitorRect,
	) -> Result<Arc<MonitorImageSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "stub" }.into())
	}

	fn latest_monitor_cache_snapshot(&self) -> Option<Arc<MonitorImageSnapshot>> {
		None
	}

	fn refresh_window_cache(&mut self) -> Result<Arc<WindowListSnapshot>> {
		Err(CaptureBackendError::NotSupported { backend: "stub" }.into())
	}

	fn latest_window_cache_snapshot(&self) -> Option<Arc<WindowListSnapshot>> {
		None
	}
}

#[cfg(test)]
mod tests {
	use crate::backend::contract::{CaptureBackend, StubCaptureBackend};

	#[test]
	fn stub_backend_returns_cursor_position() {
		let mut backend = StubCaptureBackend::new();
		let pos = backend.global_cursor_position().unwrap();

		assert!(pos.is_none());
	}
}
