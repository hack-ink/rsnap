use std::ptr;
use std::ptr::NonNull;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::{
	Arc,
	mpsc::{self, Sender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use image::RgbaImage;
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBufferCreate;

use crate::live_frame_stream_macos::STREAM_RPC_TIMEOUT;
use crate::live_frame_stream_macos::frame_store::SharedLatestFrame;
use crate::live_frame_stream_macos::live_frame_buffer::{
	self, OrderedRegionFrame, QueuedPixelBufferFrame, SharedPixelBuffer,
};
use crate::live_frame_stream_macos::stream_config::{StreamCaptureRegion, StreamCaptureTarget};
use crate::live_frame_stream_macos::stream_filter::StreamFilterConfig;
use crate::live_frame_stream_macos::stream_worker::{self, WorkerRequest};
use crate::state::{LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints, Rgb};

pub(crate) struct LiveCursorFrameSample {
	pub(crate) sample: LiveCursorSample,
	pub(crate) frame_age: Duration,
	pub(crate) frame_seq: u64,
	pub(crate) stream_generation: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct CursorSampleRequest {
	pub(crate) x_px: u32,
	pub(crate) y_px: u32,
	pub(crate) want_patch: bool,
	pub(crate) patch_width_px: u32,
	pub(crate) patch_height_px: u32,
}
impl CursorSampleRequest {
	pub(crate) fn rgb(x_px: u32, y_px: u32) -> Self {
		Self { x_px, y_px, want_patch: false, patch_width_px: 0, patch_height_px: 0 }
	}

	pub(crate) fn with_optional_patch(
		x_px: u32,
		y_px: u32,
		want_patch: bool,
		patch_width_px: u32,
		patch_height_px: u32,
	) -> Self {
		Self { x_px, y_px, want_patch, patch_width_px, patch_height_px }
	}
}

pub(crate) struct MacLiveFrameStream {
	request_tx: Sender<WorkerRequest>,
	shared_latest_frame: Arc<SharedLatestFrame>,
	capture_target: StreamCaptureTarget,
	worker: Option<JoinHandle<()>>,
	#[cfg(test)]
	debug_filter: StreamFilterConfig,
	#[cfg(test)]
	debug_last_request_kind: Arc<Mutex<Option<&'static str>>>,
}
impl MacLiveFrameStream {
	pub(crate) fn new() -> Self {
		Self::with_capture_target_and_filter_and_waker(
			StreamCaptureTarget::FullMonitor,
			StreamFilterConfig { self_capture_exception_window_ids: Vec::new() },
			None,
		)
	}

	pub(crate) fn with_self_capture_exception_window_ids(
		self_capture_exception_window_ids: Vec<u32>,
	) -> Self {
		Self::with_capture_target_and_filter_and_waker(
			StreamCaptureTarget::FullMonitor,
			StreamFilterConfig { self_capture_exception_window_ids },
			None,
		)
	}

	pub(crate) fn with_self_capture_exception_window_ids_and_waker(
		self_capture_exception_window_ids: Vec<u32>,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	) -> Self {
		Self::with_capture_target_and_filter_and_waker(
			StreamCaptureTarget::FullMonitor,
			StreamFilterConfig { self_capture_exception_window_ids },
			frame_waker,
		)
	}

	pub(crate) fn with_scroll_capture_region_and_waker(
		self_capture_exception_window_ids: Vec<u32>,
		rect_points: RectPoints,
		rect_pixels: RectPoints,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	) -> Self {
		Self::with_capture_target_and_filter_and_waker(
			StreamCaptureTarget::Region(StreamCaptureRegion { rect_points, rect_pixels }),
			StreamFilterConfig { self_capture_exception_window_ids },
			frame_waker,
		)
	}

	fn with_capture_target_and_filter_and_waker(
		capture_target: StreamCaptureTarget,
		filter: StreamFilterConfig,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	) -> Self {
		Self::with_filter_and_waker(capture_target, filter, frame_waker)
	}

	fn with_filter_and_waker(
		capture_target: StreamCaptureTarget,
		filter: StreamFilterConfig,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	) -> Self {
		#[cfg(test)]
		let debug_filter = filter.clone();
		#[cfg(test)]
		let debug_last_request_kind = Arc::new(Mutex::new(None));
		let (request_tx, request_rx) = mpsc::channel();
		let shared_latest_frame = Arc::new(SharedLatestFrame::default());
		let worker_shared_latest_frame = shared_latest_frame.clone();
		let worker = thread::spawn(move || {
			stream_worker::stream_worker_loop(
				request_rx,
				frame_waker,
				worker_shared_latest_frame,
				filter,
				capture_target,
			);
		});

		Self {
			request_tx,
			shared_latest_frame,
			capture_target,
			worker: Some(worker),
			#[cfg(test)]
			debug_filter,
			#[cfg(test)]
			debug_last_request_kind,
		}
	}

	pub(crate) fn with_waker(frame_waker: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
		Self::with_capture_target_and_filter_and_waker(
			StreamCaptureTarget::FullMonitor,
			StreamFilterConfig { self_capture_exception_window_ids: Vec::new() },
			frame_waker,
		)
	}

	#[cfg(test)]
	pub(crate) fn debug_self_capture_exception_window_ids(&self) -> &[u32] {
		&self.debug_filter.self_capture_exception_window_ids
	}

	#[cfg(test)]
	pub(crate) fn debug_last_request_kind(&self) -> Option<&'static str> {
		match self.debug_last_request_kind.lock() {
			Ok(guard) => *guard,
			Err(poisoned) => *poisoned.into_inner(),
		}
	}

	#[cfg(test)]
	pub(crate) fn debug_pending_monitor_is_none(&self) -> bool {
		self.shared_latest_frame.pending_monitor_is_none()
	}

	pub(crate) fn debug_set_self_capture_filter_complete(&self, monitor_id: u32, complete: bool) {
		self.shared_latest_frame.set_stream_filter_status(monitor_id, complete);
	}

	pub(crate) fn debug_store_test_snapshot(&self, monitor: MonitorRect, captured_at: Instant) {
		self.debug_store_test_snapshot_with_metadata(monitor, 1, 1, captured_at);
	}

	pub(crate) fn debug_store_test_snapshot_with_metadata(
		&self,
		monitor: MonitorRect,
		frame_seq: u64,
		stream_generation: u64,
		captured_at: Instant,
	) {
		let frame = QueuedPixelBufferFrame {
			frame_seq,
			stream_generation,
			captured_at,
			pixel_buffer: Self::debug_test_pixel_buffer(),
		};
		let _ = self.shared_latest_frame.store(monitor.id, &frame);
	}

	#[cfg(test)]
	pub(crate) fn debug_set_active_stream_generation(
		&self,
		monitor_id: u32,
		stream_generation: u64,
	) {
		self.shared_latest_frame.activate_stream_generation(monitor_id, stream_generation);
	}

	#[cfg(test)]
	fn record_debug_request_kind(&self, kind: &'static str) {
		match self.debug_last_request_kind.lock() {
			Ok(mut guard) => {
				*guard = Some(kind);
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = Some(kind);
			},
		}
	}

	fn debug_test_pixel_buffer() -> SharedPixelBuffer {
		let mut buffer = ptr::null_mut();
		let res = unsafe {
			CVPixelBufferCreate(
				None,
				1,
				1,
				objc2_core_video::kCVPixelFormatType_32BGRA,
				None,
				NonNull::from(&mut buffer),
			)
		};

		assert_eq!(res, objc2_core_video::kCVReturnSuccess);

		SharedPixelBuffer(unsafe {
			CFRetained::from_raw(NonNull::new(buffer).expect("test pixel buffer"))
		})
	}

	pub(crate) fn sample_rgb(&mut self, monitor: MonitorRect, x_px: u32, y_px: u32) -> Option<Rgb> {
		self.request(|reply_tx| WorkerRequest::SampleCursor {
			monitor,
			x_px,
			y_px,
			want_patch: false,
			patch_width_px: 0,
			patch_height_px: 0,
			reply_tx,
		})
		.flatten()
		.and_then(|sample| sample.rgb)
	}

	pub(crate) fn sample_rgba_patch(
		&mut self,
		monitor: MonitorRect,
		center_x_px: u32,
		center_y_px: u32,
		width_px: u32,
		height_px: u32,
	) -> Option<RgbaImage> {
		self.request(|reply_tx| WorkerRequest::SampleCursor {
			monitor,
			x_px: center_x_px,
			y_px: center_y_px,
			want_patch: true,
			patch_width_px: width_px,
			patch_height_px: height_px,
			reply_tx,
		})
		.flatten()
		.and_then(|sample| sample.patch)
	}

	pub(crate) fn latest_cursor_sample(
		&self,
		monitor: MonitorRect,
		request: CursorSampleRequest,
	) -> Option<LiveCursorSample> {
		self.latest_cursor_frame_sample(monitor, request).map(|sample| sample.sample)
	}

	pub(crate) fn latest_cursor_frame_sample(
		&self,
		monitor: MonitorRect,
		request: CursorSampleRequest,
	) -> Option<LiveCursorFrameSample> {
		let sample =
			self.shared_latest_frame.latest_frame_for_monitor(monitor.id).and_then(|frame| {
				let sample = live_frame_buffer::sample_cursor_from_pixel_buffer(
					&frame.pixel_buffer,
					request.x_px,
					request.y_px,
					request.want_patch,
					request.patch_width_px,
					request.patch_height_px,
				)?;

				Some(LiveCursorFrameSample {
					sample,
					frame_age: Instant::now().saturating_duration_since(frame.captured_at),
					frame_seq: frame.frame_seq,
					stream_generation: frame.stream_generation,
				})
			});

		if sample.is_none() {
			self.prime_monitor_nonblocking(monitor);
		}

		sample
	}

	pub(crate) fn latest_rgba_snapshot(
		&mut self,
		monitor: MonitorRect,
	) -> Option<Arc<MonitorImageSnapshot>> {
		self.request(|reply_tx| WorkerRequest::LatestRgbaSnapshot { monitor, reply_tx }).flatten()
	}

	pub(crate) fn peek_latest_rgba_snapshot(
		&self,
		monitor: MonitorRect,
	) -> Option<Arc<MonitorImageSnapshot>> {
		let Some(frame) = self.shared_latest_frame.latest_frame_for_monitor(monitor.id) else {
			self.prime_monitor_nonblocking(monitor);

			return None;
		};
		let (width_px, height_px) = live_frame_buffer::pixel_buffer_size_px(&frame.pixel_buffer)?;
		let image = live_frame_buffer::rgba_image_from_pixel_buffer(
			&frame.pixel_buffer,
			width_px,
			height_px,
			monitor.id,
		)?;

		Some(Arc::new(MonitorImageSnapshot {
			captured_at: frame.captured_at,
			stream_generation: frame.stream_generation,
			monitor,
			image: Arc::new(image),
		}))
	}

	pub(crate) fn latest_frame_frontier_for_monitor(
		&self,
		monitor: MonitorRect,
	) -> Option<(u64, u64)> {
		self.shared_latest_frame
			.latest_frame_for_monitor(monitor.id)
			.map(|frame| (frame.frame_seq, frame.stream_generation))
	}

	pub(crate) fn self_capture_filter_complete_for_monitor(&self, monitor: MonitorRect) -> bool {
		self.shared_latest_frame.self_capture_filter_complete_for_monitor(monitor.id)
	}

	pub(crate) fn latest_rgba_region(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Option<RgbaImage> {
		#[cfg(test)]
		self.record_debug_request_kind("latest_rgba_region");

		self.request(|reply_tx| WorkerRequest::LatestRgbaRegion { monitor, rect_px, reply_tx })
			.flatten()
	}

	pub(crate) fn latest_rgba_region_if_new(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		after_frame_seq: u64,
	) -> Option<(u64, RgbaImage)> {
		#[cfg(test)]
		self.record_debug_request_kind("latest_rgba_region_if_new");

		let mut frames = self.ordered_rgba_regions_after_seq(monitor, rect_px, after_frame_seq)?;
		let frame = frames.pop()?;

		Some((frame.frame_seq, frame.image))
	}

	pub(crate) fn ordered_rgba_regions_after_seq(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		after_frame_seq: u64,
	) -> Option<Vec<OrderedRegionFrame>> {
		#[cfg(test)]
		self.record_debug_request_kind("ordered_rgba_regions_after_seq");

		self.request(|reply_tx| WorkerRequest::OrderedRgbaRegionsAfterSeq {
			monitor,
			rect_px,
			after_frame_seq,
			reply_tx,
		})
		.flatten()
	}

	pub(crate) fn ordered_rgba_regions_after_seq_nonblocking(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		after_frame_seq: u64,
	) -> Option<Vec<OrderedRegionFrame>> {
		#[cfg(test)]
		self.record_debug_request_kind("ordered_rgba_regions_after_seq_nonblocking");

		let frames = self
			.shared_latest_frame
			.frames_after_seq_for_monitor(monitor.id, after_frame_seq)
			.into_iter()
			.collect::<Vec<_>>();

		if frames.is_empty() {
			if self.shared_latest_frame.latest_frame_for_monitor(monitor.id).is_none() {
				self.prime_monitor_nonblocking(monitor);
			}

			return None;
		}

		let stream_rect_px = self.stream_rect_for_requested_region(rect_px)?;
		let frames = live_frame_buffer::ordered_rgba_regions_from_frames(frames, stream_rect_px);

		(!frames.is_empty()).then_some(frames)
	}

	fn stream_rect_for_requested_region(
		&self,
		requested_rect_px: RectPoints,
	) -> Option<RectPoints> {
		stream_worker::stream_rect_for_requested_region(self.capture_target, requested_rect_px)
	}

	fn request<T>(&self, build_request: impl FnOnce(Sender<T>) -> WorkerRequest) -> Option<T> {
		let (reply_tx, reply_rx) = mpsc::channel();

		self.request_tx.send(build_request(reply_tx)).ok()?;

		reply_rx.recv_timeout(STREAM_RPC_TIMEOUT).ok()
	}

	pub(crate) fn prime_monitor_nonblocking(&self, monitor: MonitorRect) {
		#[cfg(test)]
		self.record_debug_request_kind("prime_monitor_nonblocking");

		if !self.shared_latest_frame.begin_ensure_monitor(monitor.id) {
			return;
		}
		if self
			.request_tx
			.send(WorkerRequest::EnsureMonitor { monitor, force_retry_upgrade: false })
			.is_err()
		{
			self.shared_latest_frame.finish_ensure_monitor(monitor.id);
		}
	}

	pub(crate) fn reset(&self) {
		let _ = self.request_tx.send(WorkerRequest::Reset);
	}

	pub(crate) fn upgrade_monitor_nonblocking(&self, monitor: MonitorRect) -> bool {
		#[cfg(test)]
		self.record_debug_request_kind("upgrade_monitor_nonblocking");

		if !self.shared_latest_frame.begin_ensure_monitor(monitor.id) {
			return false;
		}
		if self
			.request_tx
			.send(WorkerRequest::EnsureMonitor { monitor, force_retry_upgrade: true })
			.is_err()
		{
			self.shared_latest_frame.finish_ensure_monitor(monitor.id);

			return false;
		}

		true
	}

	pub(crate) fn refresh_monitor_nonblocking_if_stale(
		&self,
		monitor: MonitorRect,
		after_frame_seq: u64,
		force_refresh: bool,
	) -> bool {
		#[cfg(test)]
		self.record_debug_request_kind("refresh_monitor_nonblocking_if_stale");

		let now = Instant::now();

		if self.shared_latest_frame.waiting_for_frame_after_setup(monitor.id) {
			return false;
		}

		let Some(latest_frame) = self.shared_latest_frame.latest_frame_for_monitor(monitor.id)
		else {
			self.prime_monitor_nonblocking(monitor);

			return true;
		};
		let frame_age = Instant::now().saturating_duration_since(latest_frame.captured_at);

		if !stream_worker::should_refresh_monitor_frame(
			latest_frame.frame_seq,
			after_frame_seq,
			frame_age,
			force_refresh,
		) {
			return false;
		}
		if !self.shared_latest_frame.begin_refresh_monitor(monitor.id, after_frame_seq, now) {
			return false;
		}
		if self.request_tx.send(WorkerRequest::RefreshMonitor { monitor }).is_err() {
			self.shared_latest_frame.finish_refresh_monitor(monitor.id);

			return false;
		}

		true
	}
}

impl Drop for MacLiveFrameStream {
	fn drop(&mut self) {
		let _ = self.request_tx.send(WorkerRequest::Shutdown);

		if let Some(worker) = self.worker.take() {
			let _ = worker.join();
		}
	}
}
