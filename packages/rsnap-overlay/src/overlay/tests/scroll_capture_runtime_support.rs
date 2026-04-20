use super::{
	GlobalPoint, Instant, MonitorRect, OverlaySession, RectPoints, Rgb, ScrollDirection,
	ScrollObserveOutcome,
};
pub(super) use crate::scroll_capture::test_support::{
	make_browser_like_window as make_browser_like_worker_capture_window,
	make_sparse_textlike_window as make_sparse_worker_capture_window,
	make_test_image as make_scroll_capture_test_image, make_window as make_scroll_capture_window,
};

#[cfg(target_os = "macos")]
use std::collections::VecDeque;
#[cfg(target_os = "macos")]
use std::thread;

#[cfg(target_os = "macos")]
use color_eyre::eyre;

#[cfg(target_os = "macos")]
use super::{Duration, OverlayWorker, Result};
#[cfg(target_os = "macos")]
use crate::backend::CaptureBackend;
#[cfg(target_os = "macos")]
use crate::scroll_capture::ScrollSession;

pub(super) fn set_scroll_capture_input(session: &mut OverlaySession, direction: ScrollDirection) {
	session.scroll_capture.input_direction = Some(direction);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;
}

#[cfg(target_os = "macos")]
pub(super) struct SequenceScrollCaptureBackend {
	frames: VecDeque<Option<image::RgbaImage>>,
}
#[cfg(target_os = "macos")]
impl SequenceScrollCaptureBackend {
	pub(super) fn new(frames: impl IntoIterator<Item = Option<image::RgbaImage>>) -> Self {
		Self { frames: frames.into_iter().collect() }
	}
}

#[cfg(target_os = "macos")]
impl CaptureBackend for SequenceScrollCaptureBackend {
	fn capture_monitor(&mut self, _monitor: MonitorRect) -> Result<image::RgbaImage> {
		Err(eyre::eyre!("unused in this test"))
	}

	fn capture_monitor_region_for_scroll_capture(
		&mut self,
		_monitor: MonitorRect,
		_rect_px: RectPoints,
	) -> Result<Option<image::RgbaImage>> {
		Ok(self.frames.pop_front().unwrap_or(None))
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
	) -> Result<Option<image::RgbaImage>> {
		Ok(None)
	}

	fn refresh_window_cache(&mut self) -> Result<std::sync::Arc<crate::state::WindowListSnapshot>> {
		Err(eyre::eyre!("unused in this test"))
	}
}

#[cfg(target_os = "macos")]
pub(super) fn enable_test_worker_scroll_capture_path(session: &mut OverlaySession) {
	session.scroll_capture.force_worker_sampling_in_tests = true;
}

#[cfg(target_os = "macos")]
pub(super) fn seed_worker_scroll_capture_session(
	session: &mut OverlaySession,
	monitor: MonitorRect,
	rect: RectPoints,
	base: image::RgbaImage,
	frames: impl IntoIterator<Item = Option<image::RgbaImage>>,
) {
	session.worker =
		Some(OverlayWorker::new(Box::new(SequenceScrollCaptureBackend::new(frames)), None));
	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());
	enable_test_worker_scroll_capture_path(session);
}

#[cfg(target_os = "macos")]
pub(super) fn drain_scroll_capture_worker_until_idle(session: &mut OverlaySession) {
	for _ in 0..64 {
		let _ = session.drain_worker_responses();

		if session.scroll_capture.inflight_request_id.is_none() {
			return;
		}

		thread::sleep(Duration::from_millis(5));
	}

	panic!(
		"timed out waiting for worker scroll-capture response; inflight_request_id={:?}",
		session.scroll_capture.inflight_request_id
	);
}

pub(super) fn observe_scroll_capture_frame(
	session: &mut OverlaySession,
	frame: image::RgbaImage,
) -> Option<ScrollObserveOutcome> {
	match session.observe_scroll_capture_frame(frame).transpose() {
		Ok(outcome) => outcome,
		Err(err) => panic!("observe_scroll_capture_frame failed: {err:#}"),
	}
}

pub(super) fn scroll_capture_export_height(session: &OverlaySession) -> u32 {
	match session.scroll_capture.session.as_ref() {
		Some(scroll_session) => scroll_session.export_image().height(),
		None => panic!("scroll_capture_export_height requires an active scroll session"),
	}
}
