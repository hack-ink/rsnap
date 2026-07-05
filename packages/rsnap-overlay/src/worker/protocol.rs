use std::sync::Arc;

use image::RgbaImage;

#[cfg(not(target_os = "macos"))]
use crate::state::LiveCursorSample;
use crate::state::{GlobalPoint, MonitorRect, RectPoints, WindowHit, WindowListSnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FreezeCaptureTarget {
	Monitor,
	Window { window_id: u32 },
}

#[derive(Debug)]
pub(crate) enum WorkerRequest {
	HitTestWindow {
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
	},
	#[cfg(not(target_os = "macos"))]
	SampleLiveCursor {
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		want_patch: bool,
		patch_width_px: u32,
		patch_height_px: u32,
	},
	RefreshWindowList,
	FreezeCapture {
		monitor: MonitorRect,
		target: FreezeCaptureTarget,
	},
	CaptureMonitorRegion {
		monitor: MonitorRect,
		rect_px: RectPoints,
		request_id: u64,
	},
	EncodePng {
		image: RgbaImage,
	},
}

#[derive(Debug)]
pub(crate) enum WorkerResponse {
	#[cfg(not(target_os = "macos"))]
	SampledLiveCursor {
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		sample: LiveCursorSample,
	},
	HitTestWindow {
		monitor: MonitorRect,
		point: GlobalPoint,
		request_id: u64,
		hit: Option<WindowHit>,
	},
	RefreshedWindowList {
		snapshot: Arc<WindowListSnapshot>,
	},
	CapturedFreeze {
		monitor: MonitorRect,
		image: RgbaImage,
		window_image: Option<RgbaImage>,
		captured_window_id: Option<u32>,
	},
	EncodedPng {
		png_bytes: Vec<u8>,
	},
	Error {
		source: WorkerErrorSource,
		message: String,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkerErrorSource {
	EncodePng,
	FreezeCapture,
	RefreshWindowList,
	CaptureMonitorRegion,
}

#[derive(Debug)]
pub(crate) enum CapturedMonitorRegionResult {
	Image(RgbaImage),
	NoNewFrame,
}

#[derive(Debug)]
pub(crate) enum WorkerRequestSendError {
	Full,
	Disconnected,
}

#[derive(Debug)]
pub(crate) struct CapturedMonitorRegionResponse {
	pub(crate) monitor: MonitorRect,
	pub(crate) rect_px: RectPoints,
	pub(crate) request_id: u64,
	pub(crate) result: CapturedMonitorRegionResult,
}
