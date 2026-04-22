use crate::live_frame_stream_macos::{CursorSampleRequest, MacLiveFrameStream};
use crate::state::{GlobalPoint, LiveCursorSample, MonitorRect};

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
}

impl Default for HostMacLiveSampler {
	fn default() -> Self {
		Self::new()
	}
}
