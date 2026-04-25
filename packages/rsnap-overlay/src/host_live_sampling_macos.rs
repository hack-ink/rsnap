//! Public macOS live-frame sampling bridge used by the native host FFI layer.

use image::imageops::crop_imm;

use crate::live_frame_stream_macos::{CursorSampleRequest, MacLiveFrameStream};
use crate::state::{GlobalPoint, LiveCursorSample, MonitorRect};

/// Thin public wrapper around the proven macOS live frame stream used by the
/// native host for RGB and loupe sampling.
pub struct HostMacLiveSampler {
	stream: MacLiveFrameStream,
}

/// Owned RGBA pixels for a sampled host monitor region.
pub struct HostRgbaRegion {
	/// Region width in physical pixels.
	pub width: u32,
	/// Region height in physical pixels.
	pub height: u32,
	/// Packed RGBA8 pixels in row-major order.
	pub rgba: Vec<u8>,
}

impl HostMacLiveSampler {
	#[must_use]
	/// Creates a host sampler that excludes the current process from capture.
	pub fn new() -> Self {
		Self::with_self_capture_exception_window_ids(Vec::new())
	}

	#[must_use]
	/// Creates a host sampler with explicit current-process exception windows.
	pub fn with_self_capture_exception_window_ids(
		self_capture_exception_window_ids: Vec<u32>,
	) -> Self {
		Self {
			stream: MacLiveFrameStream::with_self_capture_exception_window_ids(
				self_capture_exception_window_ids,
			),
		}
	}

	#[must_use]
	/// Samples the current RGB value and optional loupe patch at the given point.
	pub fn sample_cursor(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		patch_width_px: u32,
		patch_height_px: u32,
	) -> LiveCursorSample {
		let Some((x_px, y_px)) = monitor.local_u32_pixels(point) else {
			return LiveCursorSample { rgb: None, patch: None };
		};

		self.stream
			.latest_cursor_sample(
				monitor,
				CursorSampleRequest::with_optional_patch(
					x_px,
					y_px,
					patch_width_px > 0 && patch_height_px > 0,
					patch_width_px,
					patch_height_px,
				),
			)
			.unwrap_or(LiveCursorSample { rgb: None, patch: None })
	}

	/// Starts warming the ScreenCaptureKit stream for the requested monitor without
	/// blocking on the first frame.
	pub fn prime_monitor(&self, monitor: MonitorRect) {
		self.stream.prime_monitor_nonblocking(monitor);
	}

	/// Stops any active ScreenCaptureKit stream but keeps the sampler worker alive.
	pub fn reset(&self) {
		self.stream.reset();
	}

	#[must_use]
	/// Returns a cached RGBA region from the latest monitor frame when one is already warm.
	///
	/// This does not block on a fresh capture. When the latest frame is unavailable, the
	/// underlying stream is primed and `None` is returned.
	pub fn peek_region_rgba(
		&self,
		monitor: MonitorRect,
		origin: GlobalPoint,
		width: u32,
		height: u32,
	) -> Option<HostRgbaRegion> {
		let width = i32::try_from(width).ok()?;
		let height = i32::try_from(height).ok()?;
		let rect = monitor.clip_global_rect(origin.x, origin.y, width, height)?;
		let snapshot = self.stream.peek_latest_rgba_snapshot(monitor)?;
		let rect_px = monitor.local_rect_to_pixels(rect);
		let image = crop_imm(
			snapshot.image.as_ref(),
			rect_px.x,
			rect_px.y,
			rect_px.width.max(1),
			rect_px.height.max(1),
		)
		.to_image();

		Some(HostRgbaRegion {
			width: image.width(),
			height: image.height(),
			rgba: image.into_raw(),
		})
	}

	#[must_use]
	/// Returns the latest cached full-monitor RGBA snapshot when one is already warm.
	///
	/// This does not block on a fresh capture. When the latest frame is unavailable, the
	/// underlying stream is primed and `None` is returned.
	pub fn peek_latest_monitor_rgba(&self, monitor: MonitorRect) -> Option<HostRgbaRegion> {
		let snapshot = self.stream.peek_latest_rgba_snapshot(monitor)?;
		let image = snapshot.image.as_ref();
		Some(HostRgbaRegion {
			width: image.width(),
			height: image.height(),
			rgba: image.clone().into_raw(),
		})
	}
}

impl Default for HostMacLiveSampler {
	fn default() -> Self {
		Self::new()
	}
}
