//! Public macOS live-frame sampling bridge used by the native host FFI layer.

use crate::live_frame_stream_macos::{CursorSampleRequest, MacLiveFrameStream};
use crate::state::{GlobalPoint, LiveCursorSample, MonitorRect, RectPoints};

/// Live cursor sample plus the ScreenCaptureKit frame metadata that produced it.
pub struct HostLiveCursorSample {
	/// Sampled RGB and optional patch payload.
	pub sample: LiveCursorSample,
	/// Age of the sampled ScreenCaptureKit frame in microseconds.
	pub frame_age_micros: u64,
	/// Monotonic live stream frame sequence.
	pub frame_seq: u64,
	/// Live stream generation for the active monitor stream.
	pub stream_generation: u64,
}

/// Thin public wrapper around the proven macOS live frame stream used by the
/// native host for RGB and loupe sampling.
pub struct HostMacLiveSampler {
	stream: MacLiveFrameStream,
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

	#[must_use]
	/// Samples the current RGB value and optional loupe patch with frame provenance.
	pub fn sample_cursor_with_metadata(
		&mut self,
		monitor: MonitorRect,
		point: GlobalPoint,
		patch_width_px: u32,
		patch_height_px: u32,
	) -> Option<HostLiveCursorSample> {
		let (x_px, y_px) = monitor.local_u32_pixels(point)?;
		let sample = self.stream.latest_cursor_frame_sample(
			monitor,
			CursorSampleRequest::with_optional_patch(
				x_px,
				y_px,
				patch_width_px > 0 && patch_height_px > 0,
				patch_width_px,
				patch_height_px,
			),
		)?;

		Some(HostLiveCursorSample {
			sample: sample.sample,
			frame_age_micros: sample.frame_age.as_micros().min(u128::from(u64::MAX)) as u64,
			frame_seq: sample.frame_seq,
			stream_generation: sample.stream_generation,
		})
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
	/// Returns a fresh RGBA region from the live monitor stream when possible.
	///
	/// This keeps stationary native-host color sampling responsive to animated content
	/// without falling back to slower window-list captures.
	pub fn peek_region_rgba(
		&mut self,
		monitor: MonitorRect,
		origin: GlobalPoint,
		width: u32,
		height: u32,
	) -> Option<HostRgbaRegion> {
		let rect = clipped_region_rect(monitor, origin, width, height)?;
		let rect_px = monitor.local_rect_to_pixels(rect);
		let image = self.stream.latest_rgba_region(monitor, rect_px)?;

		Some(HostRgbaRegion {
			width: image.width(),
			height: image.height(),
			rgba: image.into_raw(),
		})
	}

	#[must_use]
	/// Returns the oldest queued RGBA region after `after_frame_seq`.
	///
	/// Callers that need scroll-capture continuity should update `after_frame_seq`
	/// with the returned frame sequence and drain until this returns `None`.
	pub fn next_region_rgba_after_seq(
		&mut self,
		monitor: MonitorRect,
		origin: GlobalPoint,
		width: u32,
		height: u32,
		after_frame_seq: u64,
		wait_for_fresh: bool,
	) -> Option<HostRgbaRegionFrame> {
		let rect = clipped_region_rect(monitor, origin, width, height)?;
		let rect_px = monitor.local_rect_to_pixels(rect);
		let frames = if wait_for_fresh {
			self.stream.ordered_rgba_regions_after_seq(monitor, rect_px, after_frame_seq)
		} else {
			self.stream.ordered_rgba_regions_after_seq_nonblocking(
				monitor,
				rect_px,
				after_frame_seq,
			)
		}?;
		let frame = frames.into_iter().next()?;

		Some(HostRgbaRegionFrame {
			frame_seq: frame.frame_seq,
			frame_age_micros: frame.captured_at.elapsed().as_micros().min(u128::from(u64::MAX))
				as u64,
			region: HostRgbaRegion {
				width: frame.image.width(),
				height: frame.image.height(),
				rgba: frame.image.into_raw(),
			},
		})
	}

	#[must_use]
	/// Returns the oldest queued RGBA region after `after_frame_seq` using an
	/// already-authoritative monitor-local pixel rectangle.
	pub fn next_region_rgba_after_seq_pixels(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		after_frame_seq: u64,
		wait_for_fresh: bool,
	) -> Option<HostRgbaRegionFrame> {
		if rect_px.is_empty() {
			return None;
		}

		let frames = if wait_for_fresh {
			self.stream.ordered_rgba_regions_after_seq(monitor, rect_px, after_frame_seq)
		} else {
			self.stream.ordered_rgba_regions_after_seq_nonblocking(
				monitor,
				rect_px,
				after_frame_seq,
			)
		}?;
		let frame = frames.into_iter().next()?;

		Some(HostRgbaRegionFrame {
			frame_seq: frame.frame_seq,
			frame_age_micros: frame.captured_at.elapsed().as_micros().min(u128::from(u64::MAX))
				as u64,
			region: HostRgbaRegion {
				width: frame.image.width(),
				height: frame.image.height(),
				rgba: frame.image.into_raw(),
			},
		})
	}

	#[must_use]
	/// Returns the latest cached full-monitor RGBA snapshot when one is already warm.
	///
	/// This does not block on a fresh capture. When the latest frame is unavailable, the
	/// underlying stream is primed and `None` is returned.
	/// The returned payload intentionally does not carry frame age or sequence metadata, so it
	/// must not be used as the first frozen screenshot frame.
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

/// Owned RGBA pixels for a sampled host monitor region.
pub struct HostRgbaRegion {
	/// Region width in physical pixels.
	pub width: u32,
	/// Region height in physical pixels.
	pub height: u32,
	/// Packed RGBA8 pixels in row-major order.
	pub rgba: Vec<u8>,
}

/// Owned RGBA pixels plus ScreenCaptureKit frame provenance.
pub struct HostRgbaRegionFrame {
	/// Monotonic live stream frame sequence.
	pub frame_seq: u64,
	/// Age of the sampled ScreenCaptureKit frame in microseconds.
	pub frame_age_micros: u64,
	/// Region pixels for this frame.
	pub region: HostRgbaRegion,
}

fn clipped_region_rect(
	monitor: MonitorRect,
	origin: GlobalPoint,
	width: u32,
	height: u32,
) -> Option<RectPoints> {
	let width = i32::try_from(width).ok()?;
	let height = i32::try_from(height).ok()?;
	let right = origin.x.checked_add(width)?;
	let bottom = origin.y.checked_add(height)?;

	monitor.clip_global_rect(origin.x, origin.y, right, bottom)
}

#[cfg(test)]
mod tests {
	use crate::state::{GlobalPoint, MonitorRect, RectPoints};

	#[test]
	fn live_region_rect_treats_size_as_extent_not_bottom_right() {
		let monitor = MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};
		let rect = super::clipped_region_rect(monitor, GlobalPoint::new(280, 120), 724, 632)
			.expect("region should be inside monitor");

		assert_eq!(rect, RectPoints::new(280, 120, 724, 632));
		assert_eq!(monitor.local_rect_to_pixels(rect), RectPoints::new(560, 240, 1_448, 1_264));
	}

	#[test]
	fn live_region_rect_clips_against_nonzero_monitor_origin() {
		let monitor = MonitorRect {
			id: 2,
			origin: GlobalPoint::new(-100, -50),
			width: 400,
			height: 300,
			scale_factor_x1000: 1_000,
		};
		let rect = super::clipped_region_rect(monitor, GlobalPoint::new(-150, -60), 120, 100)
			.expect("partially visible region should clip");

		assert_eq!(rect, RectPoints::new(0, 0, 70, 90));
	}
}
