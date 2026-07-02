use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::rc::Retained;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_media::kCMTimeZero;
use objc2_screen_capture_kit::SCStreamConfiguration;

use crate::state::{MonitorRect, RectPoints};

pub(super) const STREAM_CONFIG_QUEUE_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StreamCaptureRegion {
	pub(super) rect_points: RectPoints,
	pub(super) rect_pixels: RectPoints,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StreamCaptureTarget {
	FullMonitor,
	Region(StreamCaptureRegion),
}

pub(super) fn build_stream_config_for_monitor(
	monitor: MonitorRect,
	capture_target: StreamCaptureTarget,
) -> Retained<SCStreamConfiguration> {
	let config = unsafe { SCStreamConfiguration::new() };
	let sf = monitor.scale_factor().max(1.0);
	let (width_px, height_px) = match capture_target {
		StreamCaptureTarget::FullMonitor => (
			((monitor.width as f32) * sf).round().max(1.0) as usize,
			((monitor.height as f32) * sf).round().max(1.0) as usize,
		),
		StreamCaptureTarget::Region(region) => {
			(region.rect_pixels.width.max(1) as usize, region.rect_pixels.height.max(1) as usize)
		},
	};

	unsafe { config.setWidth(width_px) };
	unsafe { config.setHeight(height_px) };
	// Keep cursor out of the frame so sampling isn't affected by pointer pixels.
	unsafe { config.setShowsCursor(false) };
	unsafe { config.setShowMouseClicks(false) };

	// 4cc("BGRA")
	let bgra = u32::from_be_bytes(*b"BGRA");

	unsafe { config.setPixelFormat(bgra) };
	unsafe { config.setMinimumFrameInterval(kCMTimeZero) };
	// Give ScreenCaptureKit enough headroom to absorb bursty trackpad motion without
	// starving the registrar on fresh frames.
	unsafe { config.setQueueDepth(STREAM_CONFIG_QUEUE_DEPTH as isize) };

	if let StreamCaptureTarget::Region(region) = capture_target {
		let source_rect = CGRect::new(
			CGPoint::new(f64::from(region.rect_points.x), f64::from(region.rect_points.y)),
			CGSize::new(f64::from(region.rect_points.width), f64::from(region.rect_points.height)),
		);

		unsafe { config.setSourceRect(source_rect) };
	}

	config
}

pub(super) fn build_sample_handler_queue_for_monitor(
	monitor_id: u32,
) -> DispatchRetained<DispatchQueue> {
	DispatchQueue::new(&sample_handler_queue_label(monitor_id), DispatchQueueAttr::SERIAL)
}

pub(super) fn sample_handler_queue_label(monitor_id: u32) -> String {
	format!("io.hackink.rsnap.scroll-capture.sample-handler.monitor-{monitor_id}")
}
