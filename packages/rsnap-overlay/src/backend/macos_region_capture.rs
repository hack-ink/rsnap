use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr};
use image::RgbaImage;
use objc2_foundation::NSProcessInfo;

use crate::backend::{CaptureBackendError, image_capture};
use crate::live_frame_stream_macos::MacLiveFrameStream;
use crate::state::{MonitorRect, RectPoints};

const REGION_FRAME_WAIT_TIMEOUT: Duration = Duration::from_millis(120);
const REGION_FRAME_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(8);

#[derive(Debug, Default)]
pub(super) struct RegionCaptureTracker {
	last_capture: HashMap<u32, RegionCaptureState>,
}
impl RegionCaptureTracker {
	pub(super) fn capture_monitor_region(
		&mut self,
		live_frame_stream: &mut MacLiveFrameStream,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Result<RgbaImage> {
		let after_frame_seq = self.after_seq(monitor, rect_px);

		if let Some((frame_seq, image)) =
			wait_for_live_stream_region(live_frame_stream, monitor, rect_px, after_frame_seq)
		{
			self.record(monitor, rect_px, frame_seq);

			tracing::trace!(
				op = "capture_backend.region_stream_hit",
				monitor_id = monitor.id,
				rect_px = ?rect_px,
				frame_seq,
				frame_px = ?image.dimensions(),
				"Captured monitor region from ScreenCaptureKit stream."
			);

			return Ok(image);
		}
		if let Some(image) = live_frame_stream.latest_rgba_region(monitor, rect_px) {
			tracing::trace!(
				op = "capture_backend.region_stream_stale_reuse",
				monitor_id = monitor.id,
				rect_px = ?rect_px,
				after_frame_seq,
				frame_px = ?image.dimensions(),
				"Reused the latest ScreenCaptureKit region frame after waiting for a fresher frame."
			);

			return Ok(image);
		}
		if let Some(snapshot) = live_frame_stream.latest_rgba_snapshot(monitor) {
			let image = image_capture::crop_monitor_image_region(&snapshot.image, rect_px)
				.wrap_err("failed to crop ScreenCaptureKit snapshot for region capture")?;

			tracing::trace!(
				op = "capture_backend.region_snapshot_crop_fallback",
				monitor_id = monitor.id,
				rect_px = ?rect_px,
				frame_px = ?image.dimensions(),
				"Fell back to cropping the latest ScreenCaptureKit monitor snapshot."
			);

			return Ok(image);
		}

		Err(CaptureBackendError::NotSupported { backend: "ScreenCaptureKit region stream" }.into())
	}

	fn after_seq(&self, monitor: MonitorRect, rect_px: RectPoints) -> u64 {
		self.last_capture
			.get(&monitor.id)
			.filter(|state| state.rect_px == rect_px)
			.map_or(0, |state| state.frame_seq)
	}

	fn record(&mut self, monitor: MonitorRect, rect_px: RectPoints, frame_seq: u64) {
		let _ = self.last_capture.insert(monitor.id, RegionCaptureState { rect_px, frame_seq });
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RegionCaptureState {
	rect_px: RectPoints,
	frame_seq: u64,
}

pub(super) fn capture_monitor_region_for_scroll_capture(
	monitor: MonitorRect,
	rect_px: RectPoints,
) -> Result<Option<RgbaImage>> {
	if !supports_scroll_capture_screenshot_api() {
		let image = image_capture::capture_monitor_region_with_core_graphics(monitor, rect_px)
			.wrap_err_with(|| {
				format!("failed to capture monitor region via CoreGraphics fallback: {monitor:?}")
			})?;

		tracing::trace!(
			op = "capture_backend.region_core_graphics_fallback",
			monitor_id = monitor.id,
			rect_px = ?rect_px,
			frame_px = ?image.dimensions(),
			"Captured monitor region from the CoreGraphics fallback because the screenshot API is unavailable."
		);

		return Ok(Some(image));
	}

	let image =
		image_capture::capture_monitor_region_image_with_screenshot_manager(monitor, rect_px)
			.wrap_err_with(|| {
				format!("failed to capture monitor region via SCScreenshotManager: {monitor:?}")
			})?;

	tracing::trace!(
		op = "capture_backend.region_screenshot_hit",
		monitor_id = monitor.id,
		rect_px = ?rect_px,
		frame_px = ?image.dimensions(),
		"Captured monitor region from ScreenCaptureKit screenshot API."
	);

	Ok(Some(image))
}

fn supports_scroll_capture_screenshot_api() -> bool {
	let process_info = NSProcessInfo::processInfo();

	image_capture::macos_supports_scroll_capture_screenshot_api_with_version(
		process_info.operatingSystemVersion(),
	)
}

fn wait_for_live_stream_region(
	live_frame_stream: &mut MacLiveFrameStream,
	monitor: MonitorRect,
	rect_px: RectPoints,
	after_frame_seq: u64,
) -> Option<(u64, RgbaImage)> {
	let deadline = Instant::now() + REGION_FRAME_WAIT_TIMEOUT;

	loop {
		if let Some(frame) =
			live_frame_stream.latest_rgba_region_if_new(monitor, rect_px, after_frame_seq)
		{
			return Some(frame);
		}

		let remaining = deadline.saturating_duration_since(Instant::now());

		if remaining.is_zero() {
			return None;
		}

		thread::sleep(remaining.min(REGION_FRAME_WAIT_POLL_INTERVAL));
	}
}

#[cfg(test)]
mod tests {
	use crate::backend::macos_region_capture::RegionCaptureTracker;
	use crate::state::{GlobalPoint, MonitorRect, RectPoints};

	#[test]
	fn region_capture_after_seq_only_reuses_matching_monitor_and_rect() {
		let monitor = MonitorRect {
			id: 11,
			origin: GlobalPoint::new(0, 0),
			width: 800,
			height: 600,
			scale_factor_x1000: 2_000,
		};
		let other_monitor = MonitorRect { id: 22, ..monitor };
		let rect = RectPoints::new(10, 20, 300, 200);
		let other_rect = RectPoints::new(10, 20, 320, 200);
		let mut tracker = RegionCaptureTracker::default();

		assert_eq!(tracker.after_seq(monitor, rect), 0);

		tracker.record(monitor, rect, 41);

		assert_eq!(tracker.after_seq(monitor, rect), 41);
		assert_eq!(tracker.after_seq(monitor, other_rect), 0);
		assert_eq!(tracker.after_seq(other_monitor, rect), 0);
	}
}
