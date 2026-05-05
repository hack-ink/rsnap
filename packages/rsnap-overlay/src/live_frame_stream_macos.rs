#![allow(
	dead_code,
	reason = "XY-113 narrows the public crate facade while leaving ScreenCaptureKit implementation cleanup to a separate follow-up lane."
)]

use std::collections::VecDeque;
use std::ops::Deref;
use std::process;
use std::ptr;
use std::ptr::NonNull;
use std::slice;
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicU64, Ordering},
	mpsc::{self, Receiver, Sender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use image::RgbaImage;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, Message};
use objc2_core_foundation::{self, CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_media::{CMSampleBuffer, kCMTimeZero};
use objc2_core_video::{
	CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
	CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
	kCVReturnSuccess,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
	SCContentFilter, SCDisplay, SCRunningApplication, SCShareableContent, SCStream,
	SCStreamConfiguration, SCStreamDelegate, SCStreamOutput, SCStreamOutputType, SCWindow,
};

use crate::macos_color;
use crate::state::{LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints, Rgb};

objc2::define_class!(
	#[unsafe(super = NSObject)]
	#[thread_kind = objc2::AnyThread]
	#[ivars = StreamOutputIvars]
	struct StreamOutput;

	unsafe impl NSObjectProtocol for StreamOutput {}

	unsafe impl SCStreamDelegate for StreamOutput {
		#[unsafe(method(stream:didStopWithError:))]
		fn stream_did_stop_with_error(&self, _stream: &SCStream, error: &NSError) {
			tracing::info!(
				op = "live_frame_stream.stopped_with_error",
				monitor_id = self.ivars().monitor_id,
				error_code = error.code(),
				error_domain = %error.domain(),
				error_description = %error.localizedDescription(),
				"ScreenCaptureKit stopped delivering frames for the live stream."
			);
		}
	}

	unsafe impl SCStreamOutput for StreamOutput {
		#[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
		fn stream_did_output_sample_buffer_of_type(
			&self,
			_stream: &SCStream,
			sample_buffer: &CMSampleBuffer,
			r#type: SCStreamOutputType,
		) {
			if r#type != SCStreamOutputType::Screen {
				return;
			}

			let Some(image_buffer) = (unsafe { sample_buffer.image_buffer() }) else {
				return;
			};
			let frame_seq =
				self.ivars().frame_seq_counter.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
			let frame = QueuedPixelBufferFrame {
				frame_seq,
				stream_generation: self.ivars().stream_generation,
				captured_at: Instant::now(),
				pixel_buffer: SharedPixelBuffer(image_buffer),
			};
			let mut frames = match self.ivars().frames.lock() {
				Ok(guard) => guard,
				Err(poisoned) => poisoned.into_inner(),
			};

			if frames.len() >= STREAM_FRAME_QUEUE_CAPACITY {
				frames.pop_front();
			}
			frames.push_back(frame.clone());
			drop(frames);
			let store_outcome =
				self.ivars().shared_latest_frame.store(self.ivars().monitor_id, &frame);
			if store_outcome.completed_ensure || store_outcome.completed_refresh {
				tracing::info!(
					op = "live_frame_stream.frame_received",
					monitor_id = self.ivars().monitor_id,
					frame_seq,
					completed_ensure = store_outcome.completed_ensure,
					completed_refresh = store_outcome.completed_refresh,
					"Received a ScreenCaptureKit frame that satisfied a pending ensure or refresh request."
				);
			}

			if let Some(frame_waker) = self.ivars().frame_waker.as_ref() {
				frame_waker();
			}
		}
	}
);

pub(crate) const STREAM_REGION_FRAME_MAX_AGE: Duration = Duration::from_millis(90);

const STREAM_RPC_TIMEOUT: Duration = Duration::from_secs(3);
const STREAM_SETUP_BACKOFF: Duration = Duration::from_millis(300);
const STREAM_INCOMPLETE_EXCEPTION_UPGRADE_BACKOFF: Duration = Duration::from_secs(3);
const STREAM_FRAME_QUEUE_CAPACITY: usize = 16;
const STREAM_CONFIG_QUEUE_DEPTH: usize = 8;
const STREAM_ACTIVE_GESTURE_FORCE_REFRESH_MIN_AGE: Duration = Duration::from_millis(60);
const STREAM_REGION_FRAME_REFRESH_TIMEOUT: Duration = Duration::from_millis(180);
const STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL: Duration = Duration::from_millis(4);
const STREAM_POST_SETUP_FRAME_GRACE: Duration = STREAM_SETUP_BACKOFF;
const STREAM_ERROR_TIMEOUT_CODE: isize = 1;
const STREAM_ERROR_NULL_CONTENT_CODE: isize = 2;
const STREAM_ERROR_RETAIN_FAILED_CODE: isize = 3;

pub(crate) struct OrderedRegionFrame {
	pub(crate) frame_seq: u64,
	pub(crate) captured_at: Instant,
	pub(crate) image: RgbaImage,
}

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
			stream_worker_loop(
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
			objc2_core_video::CVPixelBufferCreate(
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
				let sample = sample_cursor_from_pixel_buffer(
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
		let (width_px, height_px) = pixel_buffer_size_px(&frame.pixel_buffer)?;
		let image =
			rgba_image_from_pixel_buffer(&frame.pixel_buffer, width_px, height_px, monitor.id)?;

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

		let frames =
			self.shared_latest_frame.frames_after_seq_for_monitor(monitor.id, after_frame_seq);

		if frames.is_empty() {
			if self.shared_latest_frame.latest_frame_for_monitor(monitor.id).is_none() {
				self.prime_monitor_nonblocking(monitor);
			}

			return None;
		}

		let stream_rect_px = self.stream_rect_for_requested_region(rect_px)?;
		let frames = ordered_rgba_regions_from_frames(frames, stream_rect_px);

		(!frames.is_empty()).then_some(frames)
	}

	fn stream_rect_for_requested_region(
		&self,
		requested_rect_px: RectPoints,
	) -> Option<RectPoints> {
		stream_rect_for_requested_region(self.capture_target, requested_rect_px)
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

		if !should_refresh_monitor_frame(
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamCaptureRegion {
	rect_points: RectPoints,
	rect_pixels: RectPoints,
}

#[derive(Clone, Debug, Default)]
struct StreamFilterConfig {
	self_capture_exception_window_ids: Vec<u32>,
}

#[derive(Clone)]
struct SharedPixelBuffer(CFRetained<CVPixelBuffer>);
// Safety: CoreVideo pixel buffers are retained CF objects. This wrapper only exposes
// immutable queries plus read-only base-address locks, so sharing retained references
// across threads does not permit unsynchronized mutation from Rust.
unsafe impl Send for SharedPixelBuffer {}

impl Deref for SharedPixelBuffer {
	type Target = CFRetained<CVPixelBuffer>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

unsafe impl Sync for SharedPixelBuffer {}

#[derive(Clone)]
struct QueuedPixelBufferFrame {
	frame_seq: u64,
	stream_generation: u64,
	captured_at: Instant,
	pixel_buffer: SharedPixelBuffer,
}

#[derive(Clone)]
struct SharedQueuedPixelBufferFrames {
	monitor_id: u32,
	frames: VecDeque<QueuedPixelBufferFrame>,
}

#[derive(Default)]
struct SharedLatestFrame {
	frames: Mutex<Option<SharedQueuedPixelBufferFrames>>,
	pending_monitor: Mutex<Option<u32>>,
	pending_refresh_monitor: Mutex<Option<PendingMonitorRequest>>,
	waiting_for_frame_until: Mutex<Option<(u32, Instant)>>,
	active_stream_generation: Mutex<Option<StreamGenerationStatus>>,
	stream_filter_status: Mutex<Option<StreamFilterStatus>>,
	pending_stream_filter_complete_monitor: Mutex<Option<StreamGenerationStatus>>,
}
impl SharedLatestFrame {
	fn reset(&self, retired_stream: Option<StreamGenerationStatus>) {
		match self.frames.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
		match self.pending_monitor.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
		match self.pending_refresh_monitor.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
		match self.waiting_for_frame_until.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}

		if let Some(retired_stream) = retired_stream {
			match self.active_stream_generation.lock() {
				Ok(mut guard) => {
					*guard = Some(StreamGenerationStatus::retired_after(retired_stream))
				},
				Err(poisoned) => {
					let mut guard = poisoned.into_inner();

					*guard = Some(StreamGenerationStatus::retired_after(retired_stream));
				},
			}
		}

		match self.stream_filter_status.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
		match self.pending_stream_filter_complete_monitor.lock() {
			Ok(mut guard) => *guard = None,
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = None;
			},
		}
	}

	fn store(&self, monitor_id: u32, frame: &QueuedPixelBufferFrame) -> StoreFrameOutcome {
		if !self.stream_generation_is_active_for_monitor(monitor_id, frame.stream_generation) {
			return StoreFrameOutcome { completed_ensure: false, completed_refresh: false };
		}

		match self.frames.lock() {
			Ok(mut guard) => {
				let shared = guard.get_or_insert_with(|| SharedQueuedPixelBufferFrames {
					monitor_id,
					frames: VecDeque::with_capacity(STREAM_FRAME_QUEUE_CAPACITY),
				});

				if shared.monitor_id != monitor_id {
					shared.monitor_id = monitor_id;

					shared.frames.clear();
				}
				if shared.frames.len() >= STREAM_FRAME_QUEUE_CAPACITY {
					shared.frames.pop_front();
				}

				shared.frames.push_back(frame.clone());
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();
				let shared = guard.get_or_insert_with(|| SharedQueuedPixelBufferFrames {
					monitor_id,
					frames: VecDeque::with_capacity(STREAM_FRAME_QUEUE_CAPACITY),
				});

				if shared.monitor_id != monitor_id {
					shared.monitor_id = monitor_id;

					shared.frames.clear();
				}
				if shared.frames.len() >= STREAM_FRAME_QUEUE_CAPACITY {
					shared.frames.pop_front();
				}

				shared.frames.push_back(frame.clone());
			},
		}

		self.complete_pending_requests_for_stored_frame(monitor_id, frame.stream_generation)
	}

	fn complete_pending_requests_for_stored_frame(
		&self,
		monitor_id: u32,
		stream_generation: u64,
	) -> StoreFrameOutcome {
		self.complete_pending_stream_filter_status(monitor_id, stream_generation);
		self.clear_waiting_for_frame(monitor_id);
		StoreFrameOutcome {
			completed_ensure: self.finish_ensure_monitor(monitor_id),
			completed_refresh: self.finish_refresh_monitor(monitor_id),
		}
	}

	fn latest_frame_for_monitor(&self, monitor_id: u32) -> Option<QueuedPixelBufferFrame> {
		let active_stream_generation = self.active_stream_generation_for_monitor(monitor_id);

		match self.frames.lock() {
			Ok(guard) => guard
				.as_ref()
				.and_then(|latest| {
					(latest.monitor_id == monitor_id).then(|| {
						latest
							.frames
							.iter()
							.rev()
							.find(|frame| {
								active_stream_generation
									.is_none_or(|generation| frame.stream_generation == generation)
							})
							.cloned()
					})
				})
				.flatten(),
			Err(poisoned) => poisoned
				.into_inner()
				.as_ref()
				.and_then(|latest| {
					(latest.monitor_id == monitor_id).then(|| {
						latest
							.frames
							.iter()
							.rev()
							.find(|frame| {
								active_stream_generation
									.is_none_or(|generation| frame.stream_generation == generation)
							})
							.cloned()
					})
				})
				.flatten(),
		}
	}

	fn frames_after_seq_for_monitor(
		&self,
		monitor_id: u32,
		after_frame_seq: u64,
	) -> Vec<QueuedPixelBufferFrame> {
		let active_stream_generation = self.active_stream_generation_for_monitor(monitor_id);

		match self.frames.lock() {
			Ok(guard) => guard
				.as_ref()
				.filter(|shared| shared.monitor_id == monitor_id)
				.map(|shared| {
					shared
						.frames
						.iter()
						.filter(|frame| {
							frame.frame_seq > after_frame_seq
								&& active_stream_generation
									.is_none_or(|generation| frame.stream_generation == generation)
						})
						.cloned()
						.collect()
				})
				.unwrap_or_default(),
			Err(poisoned) => poisoned
				.into_inner()
				.as_ref()
				.filter(|shared| shared.monitor_id == monitor_id)
				.map(|shared| {
					shared
						.frames
						.iter()
						.filter(|frame| {
							frame.frame_seq > after_frame_seq
								&& active_stream_generation
									.is_none_or(|generation| frame.stream_generation == generation)
						})
						.cloned()
						.collect()
				})
				.unwrap_or_default(),
		}
	}

	fn begin_ensure_monitor(&self, monitor_id: u32) -> bool {
		match self.pending_monitor.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					return false;
				}

				*guard = Some(monitor_id);
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					return false;
				}

				*guard = Some(monitor_id);
			},
		}

		true
	}

	fn finish_ensure_monitor(&self, monitor_id: u32) -> bool {
		match self.pending_monitor.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					*guard = None;

					return true;
				}
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					*guard = None;

					return true;
				}
			},
		}

		false
	}

	fn begin_refresh_monitor(
		&self,
		monitor_id: u32,
		stalled_after_frame_seq: u64,
		now: Instant,
	) -> bool {
		match self.pending_refresh_monitor.lock() {
			Ok(mut guard) => {
				if let Some(pending) = *guard {
					if pending.monitor_id != monitor_id {
						return false;
					}
					if pending.stalled_after_frame_seq != stalled_after_frame_seq {
						*guard = Some(PendingMonitorRequest {
							monitor_id,
							stalled_after_frame_seq,
							started_at: now,
						});

						return true;
					}
					if now.saturating_duration_since(pending.started_at)
						< STREAM_POST_SETUP_FRAME_GRACE
					{
						return false;
					}

					tracing::info!(
						op = "live_frame_stream.stale_pending_refresh_recovered",
						monitor_id,
						stalled_after_frame_seq,
						pending_age_ms =
							now.saturating_duration_since(pending.started_at).as_millis() as u64,
						"Recovered a stale pending ScreenCaptureKit refresh so a new refresh can be scheduled."
					);

					*guard = Some(PendingMonitorRequest {
						monitor_id,
						stalled_after_frame_seq,
						started_at: now,
					});

					return true;
				}

				*guard = Some(PendingMonitorRequest {
					monitor_id,
					stalled_after_frame_seq,
					started_at: now,
				});
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if let Some(pending) = *guard {
					if pending.monitor_id != monitor_id {
						return false;
					}
					if pending.stalled_after_frame_seq != stalled_after_frame_seq {
						*guard = Some(PendingMonitorRequest {
							monitor_id,
							stalled_after_frame_seq,
							started_at: now,
						});

						return true;
					}
					if now.saturating_duration_since(pending.started_at)
						< STREAM_POST_SETUP_FRAME_GRACE
					{
						return false;
					}

					tracing::info!(
						op = "live_frame_stream.stale_pending_refresh_recovered",
						monitor_id,
						stalled_after_frame_seq,
						pending_age_ms =
							now.saturating_duration_since(pending.started_at).as_millis() as u64,
						"Recovered a stale pending ScreenCaptureKit refresh so a new refresh can be scheduled."
					);

					*guard = Some(PendingMonitorRequest {
						monitor_id,
						stalled_after_frame_seq,
						started_at: now,
					});

					return true;
				}

				*guard = Some(PendingMonitorRequest {
					monitor_id,
					stalled_after_frame_seq,
					started_at: now,
				});
			},
		}

		true
	}

	fn mark_waiting_for_frame(&self, monitor_id: u32) {
		self.mark_waiting_for_frame_until(
			monitor_id,
			Instant::now() + STREAM_POST_SETUP_FRAME_GRACE,
		);
	}

	fn mark_waiting_for_frame_until(&self, monitor_id: u32, until: Instant) {
		match self.waiting_for_frame_until.lock() {
			Ok(mut guard) => {
				*guard = Some((monitor_id, until));
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = Some((monitor_id, until));
			},
		}
	}

	fn waiting_for_frame_after_setup(&self, monitor_id: u32) -> bool {
		self.waiting_for_frame_after_setup_at(monitor_id, Instant::now())
	}

	fn waiting_for_frame_after_setup_at(&self, monitor_id: u32, now: Instant) -> bool {
		match self.waiting_for_frame_until.lock() {
			Ok(mut guard) => {
				let Some((pending_monitor_id, until)) = *guard else {
					return false;
				};

				if pending_monitor_id != monitor_id {
					return false;
				}
				if now < until {
					return true;
				}

				*guard = None;
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();
				let Some((pending_monitor_id, until)) = *guard else {
					return false;
				};

				if pending_monitor_id != monitor_id {
					return false;
				}
				if now < until {
					return true;
				}

				*guard = None;
			},
		}

		false
	}

	fn clear_waiting_for_frame(&self, monitor_id: u32) {
		match self.waiting_for_frame_until.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|(pending_monitor_id, _)| pending_monitor_id == monitor_id) {
					*guard = None;
				}
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|(pending_monitor_id, _)| pending_monitor_id == monitor_id) {
					*guard = None;
				}
			},
		}
	}

	fn finish_refresh_monitor(&self, monitor_id: u32) -> bool {
		match self.pending_refresh_monitor.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|pending| pending.monitor_id == monitor_id) {
					*guard = None;

					return true;
				}
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|pending| pending.monitor_id == monitor_id) {
					*guard = None;

					return true;
				}
			},
		}

		false
	}

	fn set_stream_filter_status(&self, monitor_id: u32, self_capture_filter_complete: bool) {
		match self.stream_filter_status.lock() {
			Ok(mut guard) => {
				*guard = Some(StreamFilterStatus { monitor_id, self_capture_filter_complete });
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = Some(StreamFilterStatus { monitor_id, self_capture_filter_complete });
			},
		}
	}

	fn self_capture_filter_complete_for_monitor(&self, monitor_id: u32) -> bool {
		match self.stream_filter_status.lock() {
			Ok(guard) => guard.as_ref().is_some_and(|status| {
				status.monitor_id == monitor_id && status.self_capture_filter_complete
			}),
			Err(poisoned) => poisoned.into_inner().as_ref().is_some_and(|status| {
				status.monitor_id == monitor_id && status.self_capture_filter_complete
			}),
		}
	}

	fn activate_stream_generation(&self, monitor_id: u32, stream_generation: u64) {
		match self.active_stream_generation.lock() {
			Ok(mut guard) => {
				*guard = Some(StreamGenerationStatus { monitor_id, stream_generation });
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = Some(StreamGenerationStatus { monitor_id, stream_generation });
			},
		}
	}

	fn active_stream_generation_for_monitor(&self, monitor_id: u32) -> Option<u64> {
		match self.active_stream_generation.lock() {
			Ok(guard) => guard.as_ref().and_then(|status| {
				(status.monitor_id == monitor_id).then_some(status.stream_generation)
			}),
			Err(poisoned) => poisoned.into_inner().as_ref().and_then(|status| {
				(status.monitor_id == monitor_id).then_some(status.stream_generation)
			}),
		}
	}

	fn stream_generation_is_active_for_monitor(
		&self,
		monitor_id: u32,
		stream_generation: u64,
	) -> bool {
		self.active_stream_generation_for_monitor(monitor_id)
			.is_none_or(|active_generation| active_generation == stream_generation)
	}

	fn defer_stream_filter_complete_until_next_frame(
		&self,
		monitor_id: u32,
		stream_generation: u64,
		self_capture_filter_complete: bool,
	) {
		self.set_stream_filter_status(monitor_id, false);

		match self.pending_stream_filter_complete_monitor.lock() {
			Ok(mut guard) => {
				*guard = self_capture_filter_complete
					.then_some(StreamGenerationStatus { monitor_id, stream_generation });
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = self_capture_filter_complete
					.then_some(StreamGenerationStatus { monitor_id, stream_generation });
			},
		}
	}

	fn complete_pending_stream_filter_status(&self, monitor_id: u32, stream_generation: u64) {
		let should_mark_complete = match self.pending_stream_filter_complete_monitor.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|pending| {
					pending.monitor_id == monitor_id
						&& pending.stream_generation == stream_generation
				}) {
					*guard = None;

					true
				} else {
					false
				}
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|pending| {
					pending.monitor_id == monitor_id
						&& pending.stream_generation == stream_generation
				}) {
					*guard = None;

					true
				} else {
					false
				}
			},
		};

		if should_mark_complete {
			self.set_stream_filter_status(monitor_id, true);
		}
	}
}

#[derive(Clone, Copy)]
struct PendingMonitorRequest {
	monitor_id: u32,
	stalled_after_frame_seq: u64,
	started_at: Instant,
}

#[derive(Clone, Copy)]
struct StreamFilterStatus {
	monitor_id: u32,
	self_capture_filter_complete: bool,
}

#[derive(Clone, Copy)]
struct StreamGenerationStatus {
	monitor_id: u32,
	stream_generation: u64,
}
impl StreamGenerationStatus {
	fn retired_after(status: Self) -> Self {
		Self {
			monitor_id: status.monitor_id,
			stream_generation: status.stream_generation.wrapping_add(1),
		}
	}
}

struct StoreFrameOutcome {
	completed_ensure: bool,
	completed_refresh: bool,
}

struct StreamOutputIvars {
	monitor_id: u32,
	stream_generation: u64,
	frames: Mutex<VecDeque<QueuedPixelBufferFrame>>,
	frame_seq_counter: Arc<AtomicU64>,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	shared_latest_frame: Arc<SharedLatestFrame>,
}
impl StreamOutputIvars {
	fn new(
		monitor_id: u32,
		stream_generation: u64,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
		frame_seq_counter: Arc<AtomicU64>,
		shared_latest_frame: Arc<SharedLatestFrame>,
	) -> Self {
		Self {
			monitor_id,
			stream_generation,
			frames: Mutex::new(VecDeque::with_capacity(STREAM_FRAME_QUEUE_CAPACITY)),
			frame_seq_counter,
			frame_waker,
			shared_latest_frame,
		}
	}
}

struct StreamState {
	monitor_id: u32,
	stream_generation: u64,
	self_capture_filter_complete: bool,
	stream: Retained<SCStream>,
	output: Retained<StreamOutput>,
	sample_handler_queue: DispatchRetained<DispatchQueue>,
}

struct CurrentProcessExceptionWindows {
	windows: Vec<Retained<SCWindow>>,
	fallback_excluded_windows: Vec<Retained<SCWindow>>,
	missing_window_ids: Vec<u32>,
}
impl CurrentProcessExceptionWindows {
	fn complete(&self) -> bool {
		self.missing_window_ids.is_empty()
	}
}

struct RefreshStreamArgs<'a> {
	state: &'a mut Option<StreamState>,
	last_setup_attempt_at: &'a mut Option<Instant>,
	monitor: MonitorRect,
	filter: &'a StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamCaptureTarget {
	FullMonitor,
	Region(StreamCaptureRegion),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamFilterMode {
	ExcludeCurrentProcess,
	ExcludeCurrentProcessShareableWindows,
}

struct PreparedStreamFilter {
	filter_mode: StreamFilterMode,
	filter: Retained<SCContentFilter>,
	self_capture_filter_complete: bool,
	self_capture_exception_window_ids_complete: bool,
	excepting_window_count: usize,
	fallback_excluded_window_count: usize,
	missing_window_ids: Vec<u32>,
	shareable_content_ms: u128,
	find_display_ms: u128,
	exception_windows_ms: u128,
	filter_build_ms: u128,
}

struct StartedStreamArtifacts {
	stream_generation: u64,
	stream: Retained<SCStream>,
	output: Retained<StreamOutput>,
	sample_handler_queue: DispatchRetained<DispatchQueue>,
	config_build_ms: u128,
	queue_build_ms: u128,
	output_build_ms: u128,
	stream_init_ms: u128,
	add_output_ms: u128,
	start_capture_ms: u128,
}

enum WorkerRequest {
	EnsureMonitor {
		monitor: MonitorRect,
		force_retry_upgrade: bool,
	},
	Reset,
	RefreshMonitor {
		monitor: MonitorRect,
	},
	SampleCursor {
		monitor: MonitorRect,
		x_px: u32,
		y_px: u32,
		want_patch: bool,
		patch_width_px: u32,
		patch_height_px: u32,
		reply_tx: Sender<Option<LiveCursorSample>>,
	},
	LatestRgbaSnapshot {
		monitor: MonitorRect,
		reply_tx: Sender<Option<Arc<MonitorImageSnapshot>>>,
	},
	LatestRgbaRegion {
		monitor: MonitorRect,
		rect_px: RectPoints,
		reply_tx: Sender<Option<RgbaImage>>,
	},
	OrderedRgbaRegionsAfterSeq {
		monitor: MonitorRect,
		rect_px: RectPoints,
		after_frame_seq: u64,
		reply_tx: Sender<Option<Vec<OrderedRegionFrame>>>,
	},
	Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamRequestProgress {
	AwaitingFirstFrame,
	Settled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamReuseDecision {
	SetupFresh,
	ReuseCurrent,
	RetryUpgradeUsingCurrent,
}

impl StreamOutput {
	fn new(
		monitor_id: u32,
		stream_generation: u64,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
		frame_seq_counter: Arc<AtomicU64>,
		shared_latest_frame: Arc<SharedLatestFrame>,
	) -> Retained<Self> {
		let this = Self::alloc().set_ivars(StreamOutputIvars::new(
			monitor_id,
			stream_generation,
			frame_waker,
			frame_seq_counter,
			shared_latest_frame,
		));

		unsafe { objc2::msg_send![super(this), init] }
	}

	fn latest_frame(&self) -> Option<QueuedPixelBufferFrame> {
		match self.ivars().frames.lock() {
			Ok(guard) => guard.back().cloned(),
			Err(poisoned) => poisoned.into_inner().back().cloned(),
		}
	}

	fn latest_pixel_buffer(&self) -> Option<SharedPixelBuffer> {
		self.latest_frame().map(|frame| frame.pixel_buffer)
	}

	fn queued_frames_after_seq(&self, after_frame_seq: u64) -> Vec<QueuedPixelBufferFrame> {
		match self.ivars().frames.lock() {
			Ok(guard) => {
				guard.iter().filter(|frame| frame.frame_seq > after_frame_seq).cloned().collect()
			},
			Err(poisoned) => poisoned
				.into_inner()
				.iter()
				.filter(|frame| frame.frame_seq > after_frame_seq)
				.cloned()
				.collect(),
		}
	}
}

fn stream_rect_for_requested_region(
	capture_target: StreamCaptureTarget,
	requested_rect_px: RectPoints,
) -> Option<RectPoints> {
	match capture_target {
		StreamCaptureTarget::FullMonitor => Some(requested_rect_px),
		StreamCaptureTarget::Region(region) => {
			let relative_x = requested_rect_px.x.checked_sub(region.rect_pixels.x)?;
			let relative_y = requested_rect_px.y.checked_sub(region.rect_pixels.y)?;
			let requested_right = requested_rect_px.x.checked_add(requested_rect_px.width)?;
			let requested_bottom = requested_rect_px.y.checked_add(requested_rect_px.height)?;
			let region_right = region.rect_pixels.x.checked_add(region.rect_pixels.width)?;
			let region_bottom = region.rect_pixels.y.checked_add(region.rect_pixels.height)?;

			if requested_right > region_right || requested_bottom > region_bottom {
				return None;
			}

			Some(RectPoints::new(
				relative_x,
				relative_y,
				requested_rect_px.width,
				requested_rect_px.height,
			))
		},
	}
}

fn should_refresh_monitor_frame(
	latest_frame_seq: u64,
	after_frame_seq: u64,
	frame_age: Duration,
	force_refresh: bool,
) -> bool {
	if latest_frame_seq > after_frame_seq {
		return false;
	}
	if force_refresh {
		let _ = frame_age;

		return true;
	}

	frame_age > STREAM_REGION_FRAME_MAX_AGE
}

fn stream_worker_loop(
	request_rx: Receiver<WorkerRequest>,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	shared_latest_frame: Arc<SharedLatestFrame>,
	filter: StreamFilterConfig,
	capture_target: StreamCaptureTarget,
) {
	let frame_seq_counter = Arc::new(AtomicU64::new(0));
	let mut state: Option<StreamState> = None;
	let mut last_setup_attempt_at: Option<Instant> = None;

	while let Ok(request) = request_rx.recv() {
		if !handle_stream_worker_request(
			request,
			&mut state,
			&mut last_setup_attempt_at,
			&filter,
			capture_target,
			frame_waker.clone(),
			frame_seq_counter.clone(),
			shared_latest_frame.clone(),
		) {
			break;
		}
	}

	teardown_stream(&mut state);
}

fn handle_reset_request(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> bool {
	let retired_stream = state.as_ref().map(|state| StreamGenerationStatus {
		monitor_id: state.monitor_id,
		stream_generation: state.stream_generation,
	});

	teardown_stream(state);

	*last_setup_attempt_at = None;

	shared_latest_frame.reset(retired_stream);

	true
}

#[allow(clippy::too_many_arguments)]
fn handle_stream_worker_request(
	request: WorkerRequest,
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> bool {
	match request {
		WorkerRequest::EnsureMonitor { monitor, force_retry_upgrade } => {
			handle_ensure_monitor_request(
				state,
				last_setup_attempt_at,
				monitor,
				force_retry_upgrade,
				filter,
				capture_target,
				frame_waker,
				frame_seq_counter,
				shared_latest_frame,
			)
		},
		WorkerRequest::Reset => {
			handle_reset_request(state, last_setup_attempt_at, shared_latest_frame)
		},
		WorkerRequest::RefreshMonitor { monitor } => handle_refresh_monitor_request(
			state,
			last_setup_attempt_at,
			monitor,
			filter,
			capture_target,
			frame_waker,
			frame_seq_counter,
			shared_latest_frame,
		),
		WorkerRequest::SampleCursor {
			monitor,
			x_px,
			y_px,
			want_patch,
			patch_width_px,
			patch_height_px,
			reply_tx,
		} => {
			reply_with_sample_cursor(
				state,
				last_setup_attempt_at,
				monitor,
				filter,
				capture_target,
				frame_waker,
				frame_seq_counter,
				shared_latest_frame,
				x_px,
				y_px,
				want_patch,
				patch_width_px,
				patch_height_px,
				reply_tx,
			);

			true
		},
		WorkerRequest::LatestRgbaSnapshot { monitor, reply_tx } => {
			reply_with_latest_rgba_snapshot(
				state,
				last_setup_attempt_at,
				monitor,
				filter,
				capture_target,
				frame_waker,
				frame_seq_counter,
				shared_latest_frame,
				reply_tx,
			);

			true
		},
		WorkerRequest::LatestRgbaRegion { monitor, rect_px, reply_tx } => {
			reply_with_latest_rgba_region(
				state,
				last_setup_attempt_at,
				monitor,
				rect_px,
				filter,
				capture_target,
				frame_waker,
				frame_seq_counter,
				shared_latest_frame,
				reply_tx,
			);

			true
		},
		WorkerRequest::OrderedRgbaRegionsAfterSeq {
			monitor,
			rect_px,
			after_frame_seq,
			reply_tx,
		} => {
			reply_with_ordered_rgba_regions_after_seq(
				state,
				last_setup_attempt_at,
				monitor,
				rect_px,
				after_frame_seq,
				filter,
				capture_target,
				frame_waker,
				frame_seq_counter,
				shared_latest_frame,
				reply_tx,
				false,
			);

			true
		},
		WorkerRequest::Shutdown => false,
	}
}

#[allow(clippy::too_many_arguments)]
fn handle_ensure_monitor_request(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	force_retry_upgrade: bool,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> bool {
	tracing::info!(
		op = "live_frame_stream.ensure_monitor_begin",
		monitor_id = monitor.id,
		current_monitor_id = state.as_ref().map(|current| current.monitor_id),
		"Handling an asynchronous ScreenCaptureKit ensure request."
	);

	let progress = ensure_stream(
		state,
		last_setup_attempt_at,
		STREAM_SETUP_BACKOFF,
		force_retry_upgrade,
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame.clone(),
	);

	if progress == StreamRequestProgress::Settled {
		shared_latest_frame.finish_ensure_monitor(monitor.id);
	}

	true
}

#[allow(clippy::too_many_arguments)]
fn handle_refresh_monitor_request(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> bool {
	if shared_latest_frame.waiting_for_frame_after_setup(monitor.id) {
		tracing::info!(
			op = "live_frame_stream.refresh_monitor_skipped_waiting_for_first_frame",
			monitor_id = monitor.id,
			current_monitor_id = state.as_ref().map(|current| current.monitor_id),
			pending_refresh_preserved = true,
			"Skipped a queued ScreenCaptureKit refresh because the stream is still waiting for the first frame from the previous setup."
		);

		return true;
	}

	tracing::info!(
		op = "live_frame_stream.refresh_monitor_begin",
		monitor_id = monitor.id,
		current_monitor_id = state.as_ref().map(|current| current.monitor_id),
		"Handling an asynchronous ScreenCaptureKit refresh request."
	);

	let progress = refresh_stream_nonblocking(
		state,
		last_setup_attempt_at,
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame.clone(),
	);

	if progress == StreamRequestProgress::Settled {
		shared_latest_frame.finish_refresh_monitor(monitor.id);
	}

	true
}

#[allow(clippy::too_many_arguments)]
fn reply_with_sample_cursor(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
	x_px: u32,
	y_px: u32,
	want_patch: bool,
	patch_width_px: u32,
	patch_height_px: u32,
	reply_tx: Sender<Option<LiveCursorSample>>,
) {
	let _ = ensure_stream(
		state,
		last_setup_attempt_at,
		STREAM_SETUP_BACKOFF,
		false,
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	);
	let rgb = state.as_ref().and_then(|stream_state| {
		stream_state.output.latest_pixel_buffer().and_then(|pixel_buffer| {
			sample_cursor_from_pixel_buffer(
				&pixel_buffer,
				x_px,
				y_px,
				want_patch,
				patch_width_px,
				patch_height_px,
			)
		})
	});
	let _ = reply_tx.send(rgb);
}

#[allow(clippy::too_many_arguments)]
fn reply_with_latest_rgba_snapshot(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
	reply_tx: Sender<Option<Arc<MonitorImageSnapshot>>>,
) {
	let _ = ensure_stream(
		state,
		last_setup_attempt_at,
		STREAM_SETUP_BACKOFF,
		false,
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	);
	let snapshot = state.as_ref().and_then(|stream_state| {
		let frame = stream_state.output.latest_frame()?;
		let (width_px, height_px) = pixel_buffer_size_px(&frame.pixel_buffer)?;
		let image =
			rgba_image_from_pixel_buffer(&frame.pixel_buffer, width_px, height_px, monitor.id)?;

		Some(Arc::new(MonitorImageSnapshot {
			captured_at: frame.captured_at,
			stream_generation: frame.stream_generation,
			monitor,
			image: Arc::new(image),
		}))
	});
	let _ = reply_tx.send(snapshot);
}

#[allow(clippy::too_many_arguments)]
fn reply_with_latest_rgba_region(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	rect_px: RectPoints,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
	reply_tx: Sender<Option<RgbaImage>>,
) {
	let image = latest_fresh_rgba_region(
		state,
		last_setup_attempt_at,
		monitor,
		rect_px,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	);
	let _ = reply_tx.send(image);
}

#[allow(clippy::too_many_arguments)]
fn reply_with_ordered_rgba_regions_after_seq(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	rect_px: RectPoints,
	after_frame_seq: u64,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
	reply_tx: Sender<Option<Vec<OrderedRegionFrame>>>,
	nonblocking: bool,
) {
	let frames = if nonblocking {
		ordered_queued_rgba_regions_after_seq_nonblocking(
			state,
			last_setup_attempt_at,
			monitor,
			rect_px,
			after_frame_seq,
			filter,
			capture_target,
			frame_waker,
			frame_seq_counter,
			shared_latest_frame,
		)
	} else {
		ordered_fresh_rgba_regions_after_seq(
			state,
			last_setup_attempt_at,
			monitor,
			rect_px,
			after_frame_seq,
			filter,
			capture_target,
			frame_waker,
			frame_seq_counter,
			shared_latest_frame,
		)
	};
	let _ = reply_tx.send(frames);
}

#[allow(clippy::too_many_arguments)]
fn refresh_stream_nonblocking(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> StreamRequestProgress {
	let now = Instant::now();
	let current_monitor_id = state.as_ref().map(|current| current.monitor_id);

	if refresh_stream_requires_setup_backoff(current_monitor_id, monitor.id)
		&& let Some(last) = *last_setup_attempt_at
		&& now.duration_since(last) < STREAM_SETUP_BACKOFF
	{
		tracing::info!(
			op = "live_frame_stream.refresh_monitor_backoff",
			monitor_id = monitor.id,
			current_monitor_id,
			elapsed_since_last_setup_ms = now.duration_since(last).as_millis(),
			backoff_ms = STREAM_SETUP_BACKOFF.as_millis(),
			"Skipped ScreenCaptureKit refresh because setup backoff is still active."
		);

		return StreamRequestProgress::Settled;
	}
	if current_monitor_id != Some(monitor.id) {
		tracing::info!(
			op = "live_frame_stream.refresh_monitor_recover_via_ensure",
			monitor_id = monitor.id,
			current_monitor_id,
			"Refresh request found no matching live stream and is falling back to ensure."
		);

		return ensure_stream(
			state,
			last_setup_attempt_at,
			STREAM_SETUP_BACKOFF,
			false,
			monitor,
			filter,
			capture_target,
			frame_waker,
			frame_seq_counter,
			shared_latest_frame,
		);
	}

	refresh_stream(RefreshStreamArgs {
		state,
		last_setup_attempt_at,
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	})
}

fn refresh_stream_requires_setup_backoff(
	current_monitor_id: Option<u32>,
	requested_monitor_id: u32,
) -> bool {
	current_monitor_id != Some(requested_monitor_id)
}

#[allow(clippy::too_many_arguments)]
fn ensure_stream(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	setup_backoff: Duration,
	force_retry_upgrade: bool,
	monitor: MonitorRect,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> StreamRequestProgress {
	let reuse_decision = stream_reuse_decision(
		state.as_ref().map(|current| current.monitor_id),
		state.as_ref().is_some_and(|current| current.self_capture_filter_complete),
		monitor.id,
	);
	let setup_backoff = stream_setup_backoff(reuse_decision, setup_backoff, force_retry_upgrade);

	if reuse_decision == StreamReuseDecision::ReuseCurrent {
		return StreamRequestProgress::Settled;
	}

	let now = Instant::now();

	if let Some(last_attempt_at) = *last_setup_attempt_at
		&& now.duration_since(last_attempt_at) < setup_backoff
	{
		tracing::info!(
			op = "live_frame_stream.ensure_stream_backoff",
			monitor_id = monitor.id,
			reuse_decision = ?reuse_decision,
			elapsed_since_last_setup_ms = now.duration_since(last_attempt_at).as_millis(),
			backoff_ms = setup_backoff.as_millis(),
			"Skipped ScreenCaptureKit setup because the current setup backoff window is still active."
		);

		return StreamRequestProgress::Settled;
	}

	*last_setup_attempt_at = Some(now);

	let Some(next_state) = setup_stream_for_monitor(
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame.clone(),
	) else {
		tracing::warn!(
			op = "live_frame_stream.ensure_stream_setup_failed",
			monitor_id = monitor.id,
			reuse_decision = ?reuse_decision,
			had_existing_state = state.is_some(),
			"ScreenCaptureKit setup did not produce a usable live stream."
		);

		return StreamRequestProgress::Settled;
	};

	if reuse_decision == StreamReuseDecision::RetryUpgradeUsingCurrent {
		if !next_state.self_capture_filter_complete {
			tracing::info!(
				op = "live_frame_stream.ensure_stream_upgrade_deferred",
				monitor_id = monitor.id,
				"Retained the current live stream because the replacement setup still lacked complete self-capture exclusions."
			);

			let mut next_state = Some(next_state);

			teardown_stream(&mut next_state);

			return StreamRequestProgress::Settled;
		}

		let stream_generation = next_state.stream_generation;

		shared_latest_frame.activate_stream_generation(monitor.id, stream_generation);

		let mut previous_state = state.replace(next_state);

		shared_latest_frame.defer_stream_filter_complete_until_next_frame(
			monitor.id,
			stream_generation,
			true,
		);

		teardown_stream(&mut previous_state);

		shared_latest_frame.mark_waiting_for_frame(monitor.id);

		tracing::debug!(
			op = "live_frame_stream.ensure_stream_ready",
			monitor_id = monitor.id,
			reuse_decision = ?reuse_decision,
			self_capture_filter_complete = true,
			replaced_existing_state = true,
			"ScreenCaptureKit setup replaced the existing live stream."
		);

		return StreamRequestProgress::AwaitingFirstFrame;
	}

	teardown_stream(state);

	let self_capture_filter_complete = next_state.self_capture_filter_complete;
	let stream_generation = next_state.stream_generation;

	shared_latest_frame.activate_stream_generation(monitor.id, stream_generation);

	*state = Some(next_state);

	shared_latest_frame.defer_stream_filter_complete_until_next_frame(
		monitor.id,
		stream_generation,
		self_capture_filter_complete,
	);
	shared_latest_frame.mark_waiting_for_frame(monitor.id);

	tracing::debug!(
		op = "live_frame_stream.ensure_stream_ready",
		monitor_id = monitor.id,
		reuse_decision = ?reuse_decision,
		self_capture_filter_complete,
		replaced_existing_state = false,
		"ScreenCaptureKit setup produced a live stream."
	);

	StreamRequestProgress::AwaitingFirstFrame
}

#[allow(clippy::too_many_arguments)]
fn latest_fresh_rgba_region(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	rect_px: RectPoints,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<RgbaImage> {
	let stream_rect_px = stream_rect_for_requested_region(capture_target, rect_px)?;
	let _ = ensure_stream(
		state,
		last_setup_attempt_at,
		STREAM_SETUP_BACKOFF,
		false,
		monitor,
		filter,
		capture_target,
		frame_waker.clone(),
		frame_seq_counter.clone(),
		shared_latest_frame.clone(),
	);
	let now = Instant::now();
	let stream_state = state.as_ref()?;

	if let Some(frame) = stream_state.output.latest_frame()
		&& now.saturating_duration_since(frame.captured_at) <= STREAM_REGION_FRAME_MAX_AGE
	{
		return rgba_region_from_pixel_buffer(&frame.pixel_buffer, stream_rect_px);
	}

	let _ = refresh_stream(RefreshStreamArgs {
		state,
		last_setup_attempt_at,
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	});
	let min_captured_at = Instant::now();
	let deadline = min_captured_at + STREAM_REGION_FRAME_REFRESH_TIMEOUT;

	loop {
		let stream_state = state.as_ref()?;

		if let Some(frame) = stream_state.output.latest_frame()
			&& frame.captured_at >= min_captured_at
		{
			return rgba_region_from_pixel_buffer(&frame.pixel_buffer, stream_rect_px);
		}

		if Instant::now() >= deadline {
			return None;
		}

		thread::sleep(STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL);
	}
}

#[allow(clippy::too_many_arguments)]
fn ordered_queued_rgba_regions_after_seq_nonblocking(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	rect_px: RectPoints,
	after_frame_seq: u64,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<Vec<OrderedRegionFrame>> {
	let stream_rect_px = stream_rect_for_requested_region(capture_target, rect_px)?;
	let _ = ensure_stream(
		state,
		last_setup_attempt_at,
		STREAM_SETUP_BACKOFF,
		false,
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	);
	let stream_state = state.as_ref()?;
	let frames = stream_state.output.queued_frames_after_seq(after_frame_seq);
	let frames = ordered_rgba_regions_from_frames(frames, stream_rect_px);

	(!frames.is_empty()).then_some(frames)
}

#[allow(clippy::too_many_arguments)]
fn ordered_fresh_rgba_regions_after_seq(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	rect_px: RectPoints,
	after_frame_seq: u64,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<Vec<OrderedRegionFrame>> {
	let stream_rect_px = stream_rect_for_requested_region(capture_target, rect_px)?;
	let _ = ensure_stream(
		state,
		last_setup_attempt_at,
		STREAM_SETUP_BACKOFF,
		false,
		monitor,
		filter,
		capture_target,
		frame_waker.clone(),
		frame_seq_counter.clone(),
		shared_latest_frame.clone(),
	);
	let stream_state = state.as_ref()?;
	let frames = stream_state.output.queued_frames_after_seq(after_frame_seq);
	let frames = ordered_rgba_regions_from_frames(frames, stream_rect_px);

	if !frames.is_empty() {
		return Some(frames);
	}

	let latest_frame = stream_state.output.latest_frame()?;

	if Instant::now().saturating_duration_since(latest_frame.captured_at)
		<= STREAM_REGION_FRAME_MAX_AGE
	{
		return None;
	}

	let _ = refresh_stream(RefreshStreamArgs {
		state,
		last_setup_attempt_at,
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	});
	let min_captured_at = Instant::now();
	let deadline = min_captured_at + STREAM_REGION_FRAME_REFRESH_TIMEOUT;

	loop {
		let stream_state = state.as_ref()?;
		let frames = stream_state.output.queued_frames_after_seq(after_frame_seq);
		let frames = ordered_rgba_regions_from_frames(frames, stream_rect_px);

		if !frames.is_empty() {
			return Some(frames);
		}
		if Instant::now() >= deadline {
			return None;
		}

		thread::sleep(STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL);
	}
}

fn refresh_stream(args: RefreshStreamArgs<'_>) -> StreamRequestProgress {
	let RefreshStreamArgs {
		state,
		last_setup_attempt_at,
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	} = args;

	tracing::info!(
		op = "live_frame_stream.refresh_stream_begin",
		monitor_id = monitor.id,
		current_monitor_id = state.as_ref().map(|current| current.monitor_id),
		"Refreshing the ScreenCaptureKit live stream."
	);

	*last_setup_attempt_at = Some(Instant::now());

	let Some(next_state) = setup_stream_for_monitor(
		monitor,
		filter,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame.clone(),
	) else {
		return StreamRequestProgress::Settled;
	};
	let self_capture_filter_complete = next_state.self_capture_filter_complete;
	let stream_generation = next_state.stream_generation;
	let replaced_existing_state = state.is_some();

	shared_latest_frame.activate_stream_generation(monitor.id, stream_generation);

	let mut previous_state = state.replace(next_state);

	shared_latest_frame.defer_stream_filter_complete_until_next_frame(
		monitor.id,
		stream_generation,
		self_capture_filter_complete,
	);

	teardown_stream(&mut previous_state);

	shared_latest_frame.mark_waiting_for_frame(monitor.id);

	tracing::debug!(
		op = "live_frame_stream.refresh_stream_ready",
		monitor_id = monitor.id,
		self_capture_filter_complete,
		replaced_existing_state,
		"Refresh completed and installed a new ScreenCaptureKit live stream."
	);

	StreamRequestProgress::AwaitingFirstFrame
}

fn teardown_stream(state: &mut Option<StreamState>) {
	let Some(state) = state.take() else {
		return;
	};

	tracing::info!(
		op = "live_frame_stream.teardown_stream",
		monitor_id = state.monitor_id,
		"Stopping the current ScreenCaptureKit live stream."
	);

	let stop_block = RcBlock::new(|_err: *mut NSError| {});

	unsafe { state.stream.stopCaptureWithCompletionHandler(Some(&stop_block)) };
}

fn setup_stream_for_monitor(
	monitor: MonitorRect,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<StreamState> {
	let setup_started_at = Instant::now();
	let prepared_filter =
		prepare_stream_filter_for_monitor(monitor, &filter.self_capture_exception_window_ids)?;
	let started_stream = build_and_start_stream_artifacts(
		monitor,
		capture_target,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
		prepared_filter.filter_mode,
		prepared_filter.filter,
	)?;

	tracing::debug!(
			op = "live_frame_stream.setup_stream_ready",
			monitor_id = monitor.id,
			shareable_content_mode = "on_screen_windows_only",
			filter_mode = ?prepared_filter.filter_mode,
			self_capture_filter_complete = prepared_filter.self_capture_filter_complete,
			self_capture_exception_window_ids_complete =
				prepared_filter.self_capture_exception_window_ids_complete,
			excepting_window_count = prepared_filter.excepting_window_count,
			fallback_excluded_window_count = prepared_filter.fallback_excluded_window_count,
		missing_window_ids = ?prepared_filter.missing_window_ids,
		shareable_content_ms = prepared_filter.shareable_content_ms,
		find_display_ms = prepared_filter.find_display_ms,
		exception_windows_ms = prepared_filter.exception_windows_ms,
		filter_build_ms = prepared_filter.filter_build_ms,
		config_build_ms = started_stream.config_build_ms,
		queue_build_ms = started_stream.queue_build_ms,
		output_build_ms = started_stream.output_build_ms,
		stream_init_ms = started_stream.stream_init_ms,
		add_output_ms = started_stream.add_output_ms,
		start_capture_ms = started_stream.start_capture_ms,
		total_setup_ms = setup_started_at.elapsed().as_millis(),
		"ScreenCaptureKit setup created a live stream for the requested monitor."
	);

	Some(StreamState {
		monitor_id: monitor.id,
		stream_generation: started_stream.stream_generation,
		self_capture_filter_complete: prepared_filter.self_capture_filter_complete,
		stream: started_stream.stream,
		output: started_stream.output,
		sample_handler_queue: started_stream.sample_handler_queue,
	})
}

fn prepare_stream_filter_for_monitor(
	monitor: MonitorRect,
	self_capture_exception_window_ids: &[u32],
) -> Option<PreparedStreamFilter> {
	let shareable_content_started_at = Instant::now();
	let content = load_shareable_content_for_monitor(monitor.id)?;
	let shareable_content_ms = shareable_content_started_at.elapsed().as_millis();
	let find_display_started_at = Instant::now();
	let display = find_display_for_monitor(&content, monitor.id)?;
	let find_display_ms = find_display_started_at.elapsed().as_millis();
	let exception_windows_started_at = Instant::now();
	let excepting_windows =
		find_current_process_exception_windows(&content, self_capture_exception_window_ids);
	let exception_windows_ms = exception_windows_started_at.elapsed().as_millis();
	let self_capture_exception_window_ids_complete = excepting_windows.complete();
	let filter_build_started_at = Instant::now();
	let prepared_filter =
		build_stream_content_filter(monitor.id, &display, &content, excepting_windows);
	let filter_build_ms = filter_build_started_at.elapsed().as_millis();

	Some(PreparedStreamFilter {
		filter_mode: prepared_filter.filter_mode,
		filter: prepared_filter.filter,
		self_capture_filter_complete: prepared_filter.self_capture_filter_complete,
		self_capture_exception_window_ids_complete,
		excepting_window_count: prepared_filter.excepting_window_count,
		fallback_excluded_window_count: prepared_filter.fallback_excluded_window_count,
		missing_window_ids: prepared_filter.missing_window_ids,
		shareable_content_ms,
		find_display_ms,
		exception_windows_ms,
		filter_build_ms,
	})
}

fn build_stream_content_filter(
	monitor_id: u32,
	display: &SCDisplay,
	content: &SCShareableContent,
	excepting_windows: CurrentProcessExceptionWindows,
) -> PreparedStreamFilter {
	let excepting_window_count = excepting_windows.windows.len();
	let fallback_excluded_window_count = excepting_windows.fallback_excluded_windows.len();
	let missing_window_ids = excepting_windows.missing_window_ids;
	let preferred_filter_mode =
		stream_filter_mode_for_current_process(missing_window_ids.is_empty());

	match preferred_filter_mode {
		StreamFilterMode::ExcludeCurrentProcess => {
			let excluded_windows: Retained<NSArray<SCWindow>> =
				NSArray::from_retained_slice(&excepting_windows.windows);

			if let Some(current_process_application) = find_current_process_application(content) {
				let excluded_applications =
					NSArray::from_retained_slice(&[current_process_application]);

				tracing::trace!(
					op = "live_frame_stream.setup_filter_excluding_current_process",
					monitor_id,
					pid = process::id(),
					excepting_window_count,
					"Configured ScreenCaptureKit to exclude Rsnap windows from the live stream."
				);

				PreparedStreamFilter {
					filter_mode: StreamFilterMode::ExcludeCurrentProcess,
					filter: unsafe {
						SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
							SCContentFilter::alloc(),
							display,
							&excluded_applications,
							&excluded_windows,
						)
					},
					self_capture_filter_complete: true,
					self_capture_exception_window_ids_complete: true,
					excepting_window_count,
					fallback_excluded_window_count,
					missing_window_ids,
					shareable_content_ms: 0,
					find_display_ms: 0,
					exception_windows_ms: 0,
					filter_build_ms: 0,
				}
			} else {
				log_missing_current_process_fallback(
					monitor_id,
					excepting_window_count,
					fallback_excluded_window_count,
				);

				build_shareable_window_filter(
					monitor_id,
					display,
					excepting_windows.fallback_excluded_windows,
					excepting_window_count,
					fallback_excluded_window_count,
					missing_window_ids,
					false,
				)
			}
		},
		StreamFilterMode::ExcludeCurrentProcessShareableWindows => build_shareable_window_filter(
			monitor_id,
			display,
			excepting_windows.fallback_excluded_windows,
			excepting_window_count,
			fallback_excluded_window_count,
			missing_window_ids,
			true,
		),
	}
}

fn build_shareable_window_filter(
	monitor_id: u32,
	display: &SCDisplay,
	fallback_excluded_windows: Vec<Retained<SCWindow>>,
	excepting_window_count: usize,
	fallback_excluded_window_count: usize,
	missing_window_ids: Vec<u32>,
	log_partial_match: bool,
) -> PreparedStreamFilter {
	let excluded_windows: Retained<NSArray<SCWindow>> =
		NSArray::from_retained_slice(&fallback_excluded_windows);

	if log_partial_match {
		tracing::debug!(
			op = "live_frame_stream.setup_filter_fallback_excluding_shareable_windows",
			monitor_id,
			pid = process::id(),
			excepting_window_count,
			fallback_excluded_window_count,
			missing_window_ids = ?missing_window_ids,
			"ScreenCaptureKit omitted at least one requested self-capture exception window; falling back to excluding only Rsnap's currently shareable windows."
		);
	}

	PreparedStreamFilter {
		filter_mode: StreamFilterMode::ExcludeCurrentProcessShareableWindows,
		filter: unsafe {
			SCContentFilter::initWithDisplay_excludingWindows(
				SCContentFilter::alloc(),
				display,
				&excluded_windows,
			)
		},
		self_capture_filter_complete: false,
		self_capture_exception_window_ids_complete: missing_window_ids.is_empty(),
		excepting_window_count,
		fallback_excluded_window_count,
		missing_window_ids,
		shareable_content_ms: 0,
		find_display_ms: 0,
		exception_windows_ms: 0,
		filter_build_ms: 0,
	}
}

fn log_missing_current_process_fallback(
	monitor_id: u32,
	excepting_window_count: usize,
	fallback_excluded_window_count: usize,
) {
	tracing::debug!(
		op = "live_frame_stream.setup_filter_fallback_missing_current_process",
		monitor_id,
		pid = process::id(),
		excepting_window_count,
		fallback_excluded_window_count,
		"ScreenCaptureKit omitted Rsnap's running application during stream setup; falling back to excluding only Rsnap's currently shareable windows."
	);
}

fn build_and_start_stream_artifacts(
	monitor: MonitorRect,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
	filter_mode: StreamFilterMode,
	filter: Retained<SCContentFilter>,
) -> Option<StartedStreamArtifacts> {
	let config_build_started_at = Instant::now();
	let config = build_stream_config_for_monitor(monitor, capture_target);
	let config_build_ms = config_build_started_at.elapsed().as_millis();
	let queue_build_started_at = Instant::now();
	let sample_handler_queue = build_sample_handler_queue_for_monitor(monitor.id);
	let queue_build_ms = queue_build_started_at.elapsed().as_millis();
	let output_build_started_at = Instant::now();
	let stream_generation = frame_seq_counter.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
	let output = StreamOutput::new(
		monitor.id,
		stream_generation,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	);
	let output_build_ms = output_build_started_at.elapsed().as_millis();
	let stream_init_started_at = Instant::now();
	let delegate_proto = ProtocolObject::from_ref(&*output);
	let stream = unsafe {
		SCStream::initWithFilter_configuration_delegate(
			SCStream::alloc(),
			&filter,
			&config,
			Some(delegate_proto),
		)
	};
	let stream_init_ms = stream_init_started_at.elapsed().as_millis();
	let add_output_started_at = Instant::now();
	let output_proto = ProtocolObject::from_ref(&*output);

	if unsafe {
		stream.addStreamOutput_type_sampleHandlerQueue_error(
			output_proto,
			SCStreamOutputType::Screen,
			Some(&sample_handler_queue),
		)
	}
	.is_err()
	{
		log_add_stream_output_failed(monitor.id, filter_mode);

		return None;
	}

	let add_output_ms = add_output_started_at.elapsed().as_millis();
	let start_capture_started_at = Instant::now();

	if let Err(error) = start_capture_blocking(&stream) {
		log_start_capture_failed(monitor.id, filter_mode, &error);

		return None;
	}

	Some(StartedStreamArtifacts {
		stream_generation,
		stream,
		output,
		sample_handler_queue,
		config_build_ms,
		queue_build_ms,
		output_build_ms,
		stream_init_ms,
		add_output_ms,
		start_capture_ms: start_capture_started_at.elapsed().as_millis(),
	})
}

fn load_shareable_content_for_monitor(monitor_id: u32) -> Option<Retained<SCShareableContent>> {
	match get_shareable_content() {
		Ok(content) => Some(content),
		Err(error) => {
			tracing::warn!(
				op = "live_frame_stream.get_shareable_content_failed",
				monitor_id,
				error_code = error.code(),
				error_domain = %error.domain(),
				error_description = %error.localizedDescription(),
				"Failed to load ScreenCaptureKit shareable content during live stream setup."
			);

			None
		},
	}
}

fn find_display_for_monitor(
	content: &SCShareableContent,
	monitor_id: u32,
) -> Option<Retained<SCDisplay>> {
	let Some(display) = find_display(content, monitor_id) else {
		tracing::warn!(
			op = "live_frame_stream.find_display_failed",
			monitor_id,
			"Failed to find the requested monitor in ScreenCaptureKit shareable content."
		);

		return None;
	};

	Some(display)
}

fn log_add_stream_output_failed(monitor_id: u32, filter_mode: StreamFilterMode) {
	tracing::warn!(
		op = "live_frame_stream.add_stream_output_failed",
		monitor_id,
		filter_mode = ?filter_mode,
		"Failed to register the ScreenCaptureKit stream output."
	);
}

fn log_start_capture_failed(monitor_id: u32, filter_mode: StreamFilterMode, error: &NSError) {
	tracing::warn!(
		op = "live_frame_stream.start_capture_failed",
		monitor_id,
		filter_mode = ?filter_mode,
		error_code = error.code(),
		error_domain = %error.domain(),
		error_description = %error.localizedDescription(),
		"ScreenCaptureKit failed to start the live stream."
	);
}

fn find_current_process_exception_windows(
	content: &SCShareableContent,
	self_capture_exception_window_ids: &[u32],
) -> CurrentProcessExceptionWindows {
	if self_capture_exception_window_ids.is_empty() {
		return CurrentProcessExceptionWindows {
			windows: Vec::new(),
			fallback_excluded_windows: Vec::new(),
			missing_window_ids: Vec::new(),
		};
	}

	let current_pid = process::id();
	let windows = unsafe { content.windows() };
	let mut matched = Vec::new();
	let mut fallback_excluded_windows = Vec::new();
	let mut matched_window_ids = Vec::new();

	for window in windows.iter() {
		let window_id = unsafe { window.windowID() };
		let is_requested_exception = self_capture_exception_window_ids.contains(&window_id);

		if is_requested_exception {
			matched_window_ids.push(window_id);
			matched.push(window.retain());
		}
		if window_is_owned_by_current_process(&window, current_pid) && !is_requested_exception {
			fallback_excluded_windows.push(window.retain());
		}
	}

	let missing_window_ids =
		missing_exception_window_ids(self_capture_exception_window_ids, &matched_window_ids);

	if !missing_window_ids.is_empty() {
		tracing::debug!(
			op = "live_frame_stream.self_capture_exception_window_ids_partial_match",
			requested_window_ids = ?self_capture_exception_window_ids,
			missing_window_ids = ?missing_window_ids,
			matched_window_count = matched.len(),
			fallback_excluded_window_count = fallback_excluded_windows.len(),
			"ScreenCaptureKit did not expose every requested current-process exception window; continuing stream setup with a capturable window-exclusion fallback."
		);
	}

	CurrentProcessExceptionWindows {
		windows: matched,
		fallback_excluded_windows,
		missing_window_ids,
	}
}

fn missing_exception_window_ids(
	self_capture_exception_window_ids: &[u32],
	matched_window_ids: &[u32],
) -> Vec<u32> {
	self_capture_exception_window_ids
		.iter()
		.copied()
		.filter(|window_id| !matched_window_ids.contains(window_id))
		.collect()
}

fn stream_reuse_decision(
	current_monitor_id: Option<u32>,
	self_capture_filter_complete: bool,
	requested_monitor_id: u32,
) -> StreamReuseDecision {
	match current_monitor_id {
		Some(current_monitor_id)
			if current_monitor_id == requested_monitor_id && self_capture_filter_complete =>
		{
			StreamReuseDecision::ReuseCurrent
		},
		Some(current_monitor_id) if current_monitor_id == requested_monitor_id => {
			StreamReuseDecision::RetryUpgradeUsingCurrent
		},
		_ => StreamReuseDecision::SetupFresh,
	}
}

fn stream_setup_backoff(
	reuse_decision: StreamReuseDecision,
	default_setup_backoff: Duration,
	force_retry_upgrade: bool,
) -> Duration {
	match reuse_decision {
		StreamReuseDecision::RetryUpgradeUsingCurrent if force_retry_upgrade => Duration::ZERO,
		StreamReuseDecision::RetryUpgradeUsingCurrent => {
			STREAM_INCOMPLETE_EXCEPTION_UPGRADE_BACKOFF
		},
		StreamReuseDecision::SetupFresh | StreamReuseDecision::ReuseCurrent => {
			default_setup_backoff
		},
	}
}

fn find_current_process_application(
	content: &SCShareableContent,
) -> Option<Retained<SCRunningApplication>> {
	let current_pid = process::id();
	let applications = unsafe { content.applications() };

	for application in applications.iter() {
		let Ok(application_pid) = u32::try_from(unsafe { application.processID() }) else {
			continue;
		};

		if application_pid == current_pid {
			return Some(application.retain());
		}
	}

	None
}

fn window_is_owned_by_current_process(window: &SCWindow, current_pid: u32) -> bool {
	unsafe { window.owningApplication() }
		.and_then(|application| u32::try_from(unsafe { application.processID() }).ok())
		.is_some_and(|window_pid| window_pid == current_pid)
}

fn stream_filter_mode_for_current_process(
	self_capture_exception_window_ids_complete: bool,
) -> StreamFilterMode {
	if self_capture_exception_window_ids_complete {
		StreamFilterMode::ExcludeCurrentProcess
	} else {
		StreamFilterMode::ExcludeCurrentProcessShareableWindows
	}
}

fn get_shareable_content() -> Result<Retained<SCShareableContent>, Retained<NSError>> {
	let (tx, rx) = mpsc::sync_channel::<Result<Retained<SCShareableContent>, Retained<NSError>>>(1);
	let tx = Mutex::new(Some(tx));
	let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
		let mut maybe_tx = match tx.lock() {
			Ok(guard) => guard,
			Err(poisoned) => poisoned.into_inner(),
		};
		let Some(tx) = maybe_tx.take() else {
			return;
		};

		if !err.is_null() {
			let Some(err) = (unsafe { Retained::retain(err) }) else {
				let _ = tx.send(Err(stream_error(STREAM_ERROR_RETAIN_FAILED_CODE)));

				return;
			};
			let _ = tx.send(Err(err));

			return;
		}

		let Some(content) = (unsafe { Retained::retain(content) }) else {
			let err = stream_error(STREAM_ERROR_NULL_CONTENT_CODE);
			let _ = tx.send(Err(err));

			return;
		};
		let _ = tx.send(Ok(content));
	});

	unsafe {
		SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
			false,
			true,
			&block,
		)
	};

	rx.recv_timeout(Duration::from_secs(2)).map_err(|_| stream_error(STREAM_ERROR_TIMEOUT_CODE))?
}

fn start_capture_blocking(stream: &SCStream) -> Result<(), Retained<NSError>> {
	let (tx, rx) = mpsc::sync_channel::<Result<(), Retained<NSError>>>(1);
	let tx = Mutex::new(Some(tx));
	let block = RcBlock::new(move |err: *mut NSError| {
		let mut maybe_tx = match tx.lock() {
			Ok(guard) => guard,
			Err(poisoned) => poisoned.into_inner(),
		};
		let Some(tx) = maybe_tx.take() else {
			return;
		};

		if err.is_null() {
			let _ = tx.send(Ok(()));

			return;
		}

		let Some(err) = (unsafe { Retained::retain(err) }) else {
			let _ = tx.send(Err(stream_error(STREAM_ERROR_RETAIN_FAILED_CODE)));

			return;
		};
		let _ = tx.send(Err(err));
	});

	unsafe { stream.startCaptureWithCompletionHandler(Some(&block)) };

	rx.recv_timeout(Duration::from_secs(2)).map_err(|_| stream_error(STREAM_ERROR_TIMEOUT_CODE))?
}

fn stream_error(code: isize) -> Retained<NSError> {
	NSError::new(code, objc2_foundation::ns_string!("io.hackink.rsnap.live_frame_stream"))
}

fn find_display(content: &SCShareableContent, monitor_id: u32) -> Option<Retained<SCDisplay>> {
	let displays = unsafe { content.displays() };

	for display in displays.iter() {
		let display_id = unsafe { display.displayID() };

		if display_id == monitor_id {
			return Some(display);
		}
	}

	None
}

fn build_stream_config_for_monitor(
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

fn build_sample_handler_queue_for_monitor(monitor_id: u32) -> DispatchRetained<DispatchQueue> {
	DispatchQueue::new(&sample_handler_queue_label(monitor_id), DispatchQueueAttr::SERIAL)
}

fn sample_handler_queue_label(monitor_id: u32) -> String {
	format!("io.hackink.rsnap.scroll-capture.sample-handler.monitor-{monitor_id}")
}

fn pixel_buffer_size_px(pixel_buffer: &CFRetained<CVPixelBuffer>) -> Option<(u32, u32)> {
	let width = objc2_core_video::CVPixelBufferGetWidth(pixel_buffer);
	let height = objc2_core_video::CVPixelBufferGetHeight(pixel_buffer);
	let width = u32::try_from(width).ok()?;
	let height = u32::try_from(height).ok()?;

	Some((width, height))
}

fn sample_cursor_from_pixel_buffer(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
	x_px: u32,
	y_px: u32,
	want_patch: bool,
	patch_width_px: u32,
	patch_height_px: u32,
) -> Option<LiveCursorSample> {
	let (width, height) = pixel_buffer_size_px(pixel_buffer)?;
	let lock_result =
		unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	if lock_result != kCVReturnSuccess {
		return None;
	}

	let out = (|| {
		let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;

		if base.is_null() {
			return None;
		}

		let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
		let byte_len = (height as usize).saturating_mul(bytes_per_row);
		let bytes = unsafe { slice::from_raw_parts(base, byte_len) };

		sample_cursor_from_bgra_bytes(
			bytes,
			bytes_per_row,
			width,
			height,
			x_px,
			y_px,
			want_patch,
			patch_width_px,
			patch_height_px,
		)
	})();
	let _ =
		unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	out
}

#[allow(clippy::too_many_arguments)]
fn sample_cursor_from_bgra_bytes(
	bytes: &[u8],
	bytes_per_row: usize,
	width_px: u32,
	height_px: u32,
	x_px: u32,
	y_px: u32,
	want_patch: bool,
	patch_width_px: u32,
	patch_height_px: u32,
) -> Option<LiveCursorSample> {
	if x_px >= width_px || y_px >= height_px {
		return None;
	}

	let offset = (y_px as usize).saturating_mul(bytes_per_row).saturating_add((x_px as usize) * 4);
	let b = *bytes.get(offset)?;
	let g = *bytes.get(offset + 1)?;
	let r = *bytes.get(offset + 2)?;
	let _a = *bytes.get(offset + 3)?;
	let rgb = Some(Rgb::new(r, g, b));
	let patch = if want_patch {
		let out_patch_w = patch_width_px.max(1);
		let out_patch_h = patch_height_px.max(1);
		let half_w = (out_patch_w as i32) / 2;
		let half_h = (out_patch_h as i32) / 2;
		let center_x = x_px as i32;
		let center_y = y_px as i32;
		let in_w = width_px as i32;
		let in_h = height_px as i32;
		let mut out_patch = RgbaImage::new(out_patch_w, out_patch_h);

		for oy in 0..(out_patch_h as i32) {
			let iy = (center_y - half_h + oy).clamp(0, in_h.saturating_sub(1));

			for ox in 0..(out_patch_w as i32) {
				let ix = (center_x - half_w + ox).clamp(0, in_w.saturating_sub(1));
				let offset =
					(iy as usize).saturating_mul(bytes_per_row).saturating_add((ix as usize) * 4);
				let b = *bytes.get(offset)?;
				let g = *bytes.get(offset + 1)?;
				let r = *bytes.get(offset + 2)?;
				let a = *bytes.get(offset + 3)?;

				out_patch.put_pixel(ox as u32, oy as u32, image::Rgba([r, g, b, a]));
			}
		}

		Some(out_patch)
	} else {
		None
	};

	Some(LiveCursorSample { rgb, patch })
}

fn rgba_image_from_pixel_buffer(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
	width_px: u32,
	height_px: u32,
	display_id: u32,
) -> Option<RgbaImage> {
	if let Some(image) = macos_color::rgba_image_from_pixel_buffer_color_managed(
		pixel_buffer,
		width_px,
		height_px,
		Some(display_id),
	) {
		return Some(image);
	}

	let lock_result =
		unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	if lock_result != kCVReturnSuccess {
		return None;
	}

	let out = (|| {
		let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;

		if base.is_null() {
			return None;
		}

		let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
		let mut out = RgbaImage::new(width_px.max(1), height_px.max(1));
		let out_w = out.width() as usize;
		let out_h = out.height() as usize;

		for y in 0..out_h {
			let row = unsafe { slice::from_raw_parts(base.add(y * bytes_per_row), bytes_per_row) };

			for x in 0..out_w {
				let idx = x * 4;
				let b = row.get(idx).copied().unwrap_or(0);
				let g = row.get(idx + 1).copied().unwrap_or(0);
				let r = row.get(idx + 2).copied().unwrap_or(0);
				let a = row.get(idx + 3).copied().unwrap_or(255);

				out.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, a]));
			}
		}

		Some(out)
	})();
	let _ =
		unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	out
}

fn rgba_region_from_pixel_buffer(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
	rect_px: RectPoints,
) -> Option<RgbaImage> {
	let (buffer_width_px, buffer_height_px) = pixel_buffer_size_px(pixel_buffer)?;
	let width_px = rect_px.width.max(1).min(buffer_width_px.max(1));
	let height_px = rect_px.height.max(1).min(buffer_height_px.max(1));
	let x_px = rect_px.x.min(buffer_width_px.saturating_sub(width_px));
	let y_px = rect_px.y.min(buffer_height_px.saturating_sub(height_px));
	let lock_result =
		unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	if lock_result != kCVReturnSuccess {
		return None;
	}

	let out = (|| {
		let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;

		if base.is_null() {
			return None;
		}

		let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
		let mut out = RgbaImage::new(width_px.max(1), height_px.max(1));
		let out_w = out.width() as usize;
		let out_h = out.height() as usize;
		let src_x = x_px as usize;
		let src_y = y_px as usize;

		for y in 0..out_h {
			let row_offset = (src_y + y).saturating_mul(bytes_per_row);
			let row = unsafe { slice::from_raw_parts(base.add(row_offset), bytes_per_row) };

			for x in 0..out_w {
				let idx = (src_x + x).saturating_mul(4);
				let b = row.get(idx).copied().unwrap_or(0);
				let g = row.get(idx + 1).copied().unwrap_or(0);
				let r = row.get(idx + 2).copied().unwrap_or(0);
				let a = row.get(idx + 3).copied().unwrap_or(255);

				out.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, a]));
			}
		}

		Some(out)
	})();
	let _ =
		unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	out
}

fn ordered_rgba_regions_from_frames(
	frames: Vec<QueuedPixelBufferFrame>,
	rect_px: RectPoints,
) -> Vec<OrderedRegionFrame> {
	frames
		.into_iter()
		.filter_map(|frame| {
			let image = rgba_region_from_pixel_buffer(&frame.pixel_buffer, rect_px)?;

			Some(OrderedRegionFrame {
				frame_seq: frame.frame_seq,
				captured_at: frame.captured_at,
				image,
			})
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use std::ptr::{self, NonNull};
	use std::sync::{Arc, atomic::AtomicU64};
	use std::time::Duration;

	use objc2_core_foundation::CFRetained;
	use objc2_core_video::{CVPixelBufferCreate, kCVPixelFormatType_32BGRA, kCVReturnSuccess};

	use crate::live_frame_stream_macos::{self, STREAM_POST_SETUP_FRAME_GRACE, StreamFilterMode};
	use crate::state::Rgb;

	fn test_pixel_buffer() -> live_frame_stream_macos::SharedPixelBuffer {
		let mut buffer = ptr::null_mut();
		let res = unsafe {
			CVPixelBufferCreate(
				None,
				1,
				1,
				kCVPixelFormatType_32BGRA,
				None,
				NonNull::from(&mut buffer),
			)
		};

		assert_eq!(res, kCVReturnSuccess);

		live_frame_stream_macos::SharedPixelBuffer(unsafe {
			CFRetained::from_raw(NonNull::new(buffer).expect("test pixel buffer"))
		})
	}

	#[test]
	fn stream_filter_mode_prefers_process_exclusion_only_when_exception_list_is_complete() {
		assert_eq!(
			live_frame_stream_macos::stream_filter_mode_for_current_process(true),
			StreamFilterMode::ExcludeCurrentProcess
		);
		assert_eq!(
			live_frame_stream_macos::stream_filter_mode_for_current_process(false),
			StreamFilterMode::ExcludeCurrentProcessShareableWindows
		);
	}

	#[test]
	fn with_waker_streams_preserve_self_capture_exception_window_ids() {
		let stream = live_frame_stream_macos::MacLiveFrameStream::with_self_capture_exception_window_ids_and_waker(
			vec![7, 11],
			None,
		);

		assert_eq!(stream.debug_self_capture_exception_window_ids(), &[7, 11]);
	}

	#[test]
	fn missing_exception_window_ids_reports_unshareable_requested_windows() {
		assert_eq!(
			live_frame_stream_macos::missing_exception_window_ids(&[], &[]),
			Vec::<u32>::new()
		);
		assert_eq!(
			live_frame_stream_macos::missing_exception_window_ids(&[7, 11], &[7, 11]),
			Vec::<u32>::new()
		);
		assert_eq!(live_frame_stream_macos::missing_exception_window_ids(&[7, 11], &[11]), vec![7]);
	}

	#[test]
	fn stream_reuse_decision_retries_incomplete_same_monitor_streams() {
		assert_eq!(
			live_frame_stream_macos::stream_reuse_decision(Some(7), true, 7),
			live_frame_stream_macos::StreamReuseDecision::ReuseCurrent
		);
		assert_eq!(
			live_frame_stream_macos::stream_reuse_decision(Some(7), false, 7),
			live_frame_stream_macos::StreamReuseDecision::RetryUpgradeUsingCurrent
		);
		assert_eq!(
			live_frame_stream_macos::stream_reuse_decision(Some(7), true, 9),
			live_frame_stream_macos::StreamReuseDecision::SetupFresh
		);
	}

	#[test]
	fn retry_upgrade_uses_slower_setup_backoff() {
		assert_eq!(
			live_frame_stream_macos::stream_setup_backoff(
				live_frame_stream_macos::StreamReuseDecision::SetupFresh,
				Duration::from_millis(300),
				false,
			),
			Duration::from_millis(300)
		);
		assert_eq!(
			live_frame_stream_macos::stream_setup_backoff(
				live_frame_stream_macos::StreamReuseDecision::RetryUpgradeUsingCurrent,
				Duration::from_millis(300),
				false,
			),
			Duration::from_secs(3)
		);
		assert_eq!(
			live_frame_stream_macos::stream_setup_backoff(
				live_frame_stream_macos::StreamReuseDecision::RetryUpgradeUsingCurrent,
				Duration::from_millis(300),
				true,
			),
			Duration::ZERO
		);
	}

	#[test]
	fn refresh_stream_requires_setup_backoff_only_for_recovery_paths() {
		assert!(live_frame_stream_macos::refresh_stream_requires_setup_backoff(None, 7));
		assert!(live_frame_stream_macos::refresh_stream_requires_setup_backoff(Some(9), 7));
		assert!(!live_frame_stream_macos::refresh_stream_requires_setup_backoff(Some(7), 7));
	}

	#[test]
	fn waiting_for_first_frame_expires_after_grace_window() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();
		let until = now + Duration::from_millis(50);

		shared.mark_waiting_for_frame_until(7, until);

		assert!(shared.waiting_for_frame_after_setup_at(7, now + Duration::from_millis(25)));
		assert!(!shared.waiting_for_frame_after_setup_at(7, now + Duration::from_millis(60)));
		assert!(!shared.waiting_for_frame_after_setup_at(7, now + Duration::from_millis(61)));
	}

	#[test]
	fn shared_latest_frame_tracks_self_capture_filter_completeness_per_monitor() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();

		assert!(!shared.self_capture_filter_complete_for_monitor(7));

		shared.set_stream_filter_status(7, false);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));

		shared.set_stream_filter_status(7, true);

		assert!(shared.self_capture_filter_complete_for_monitor(7));
		assert!(!shared.self_capture_filter_complete_for_monitor(9));
	}

	#[test]
	fn deferred_filter_complete_waits_for_matching_first_frame() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let pixel_buffer = test_pixel_buffer();
		let other_monitor_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 1,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: pixel_buffer.clone(),
		};
		let matching_monitor_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 2,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer,
		};

		shared.activate_stream_generation(7, 1);
		shared.defer_stream_filter_complete_until_next_frame(7, 1, true);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));

		let _ = shared.store(9, &other_monitor_frame);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));

		let _ = shared.store(7, &matching_monitor_frame);

		assert!(shared.self_capture_filter_complete_for_monitor(7));
	}

	#[test]
	fn deferred_filter_complete_ignores_stale_generation_frames() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let pixel_buffer = test_pixel_buffer();
		let stale_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 1,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: pixel_buffer.clone(),
		};
		let current_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 2,
			stream_generation: 2,
			captured_at: std::time::Instant::now(),
			pixel_buffer,
		};

		shared.activate_stream_generation(7, 2);
		shared.defer_stream_filter_complete_until_next_frame(7, 2, true);

		let _ = shared.store(7, &stale_frame);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));
		assert!(shared.latest_frame_for_monitor(7).is_none());

		let _ = shared.store(7, &current_frame);

		assert!(shared.self_capture_filter_complete_for_monitor(7));
		assert_eq!(
			shared.latest_frame_for_monitor(7).map(|frame| frame.stream_generation),
			Some(2)
		);
	}

	#[test]
	fn reset_discards_cached_frames_and_rejects_retired_stream_frames() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let pixel_buffer = test_pixel_buffer();
		let retired_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 1,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: pixel_buffer.clone(),
		};

		shared.activate_stream_generation(7, 1);

		let _ = shared.store(7, &retired_frame);

		assert_eq!(
			shared.latest_frame_for_monitor(7).map(|frame| frame.stream_generation),
			Some(1)
		);

		shared.reset(Some(live_frame_stream_macos::StreamGenerationStatus {
			monitor_id: 7,
			stream_generation: 1,
		}));

		assert!(shared.latest_frame_for_monitor(7).is_none());

		let _ = shared.store(7, &retired_frame);

		assert!(shared.latest_frame_for_monitor(7).is_none());
	}

	#[test]
	fn incomplete_filter_never_flips_complete_after_first_frame() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 1,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: test_pixel_buffer(),
		};

		shared.activate_stream_generation(7, 1);
		shared.defer_stream_filter_complete_until_next_frame(7, 1, false);

		let _ = shared.store(7, &frame);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));
	}

	#[test]
	fn mac_live_frame_stream_reports_self_capture_filter_completeness_from_shared_status() {
		let stream = live_frame_stream_macos::MacLiveFrameStream::with_waker(None);
		let monitor = crate::state::MonitorRect {
			id: 7,
			origin: crate::state::GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};

		assert!(!stream.self_capture_filter_complete_for_monitor(monitor));

		stream.debug_set_self_capture_filter_complete(monitor.id, true);

		assert!(stream.self_capture_filter_complete_for_monitor(monitor));
	}

	#[test]
	fn stored_frame_completion_clears_pending_ensure_for_same_monitor() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_ensure_monitor(7));

		shared.mark_waiting_for_frame_until(7, now + Duration::from_secs(1));

		let outcome = shared.complete_pending_requests_for_stored_frame(7, 1);

		assert!(outcome.completed_ensure);
		assert!(!outcome.completed_refresh);
		assert!(!shared.waiting_for_frame_after_setup_at(7, now));
		assert!(!shared.finish_ensure_monitor(7));
	}

	#[test]
	fn stored_frame_completion_leaves_other_monitor_refresh_pending() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_refresh_monitor(7, 11, now));

		shared.mark_waiting_for_frame_until(7, now + Duration::from_secs(1));

		let outcome = shared.complete_pending_requests_for_stored_frame(9, 1);

		assert!(!outcome.completed_ensure);
		assert!(!outcome.completed_refresh);
		assert!(shared.waiting_for_frame_after_setup_at(7, now));
		assert!(shared.finish_refresh_monitor(7));
	}

	#[test]
	fn stored_frame_completion_clears_pending_refresh_for_same_monitor() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_refresh_monitor(7, 11, now));

		shared.mark_waiting_for_frame_until(7, now + Duration::from_secs(1));

		let outcome = shared.complete_pending_requests_for_stored_frame(7, 1);

		assert!(!outcome.completed_ensure);
		assert!(outcome.completed_refresh);
		assert!(!shared.waiting_for_frame_after_setup_at(7, now));
		assert!(!shared.finish_refresh_monitor(7));
	}

	#[test]
	fn stale_pending_refresh_retries_again_after_each_grace_window_for_same_stalled_frontier() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_refresh_monitor(7, 11, now));
		assert!(!shared.begin_refresh_monitor(7, 11, now + Duration::from_millis(100)));
		assert!(shared.begin_refresh_monitor(
			7,
			11,
			now + STREAM_POST_SETUP_FRAME_GRACE + Duration::from_millis(1),
		));
		assert!(!shared.begin_refresh_monitor(
			7,
			11,
			now + STREAM_POST_SETUP_FRAME_GRACE + Duration::from_millis(2),
		));
		assert!(shared.begin_refresh_monitor(
			7,
			11,
			now + STREAM_POST_SETUP_FRAME_GRACE.saturating_mul(2) + Duration::from_millis(1),
		));
	}

	#[test]
	fn stale_pending_refresh_rearms_when_stalled_frontier_advances() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_refresh_monitor(7, 11, now));
		assert!(shared.begin_refresh_monitor(
			7,
			11,
			now + STREAM_POST_SETUP_FRAME_GRACE + Duration::from_millis(1),
		));
		assert!(shared.begin_refresh_monitor(
			7,
			12,
			now + STREAM_POST_SETUP_FRAME_GRACE + Duration::from_millis(2),
		));
	}

	#[test]
	fn queued_refresh_request_stays_pending_while_waiting_for_previous_first_frame() {
		let shared = Arc::new(live_frame_stream_macos::SharedLatestFrame::default());
		let now = std::time::Instant::now();
		let monitor = crate::state::MonitorRect {
			id: 7,
			origin: crate::state::GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};
		let mut state = None;
		let mut last_setup_attempt_at = None;

		assert!(shared.begin_refresh_monitor(monitor.id, 11, now));

		shared.mark_waiting_for_frame_until(monitor.id, now + Duration::from_secs(1));

		assert!(live_frame_stream_macos::handle_refresh_monitor_request(
			&mut state,
			&mut last_setup_attempt_at,
			monitor,
			&live_frame_stream_macos::StreamFilterConfig::default(),
			live_frame_stream_macos::StreamCaptureTarget::FullMonitor,
			None,
			Arc::new(AtomicU64::new(0)),
			shared.clone(),
		));
		assert!(state.is_none());
		assert!(shared.finish_refresh_monitor(monitor.id));
	}

	#[test]
	fn shared_frame_history_returns_all_frames_after_frontier() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let monitor_id = 7;
		let pixel_buffer = test_pixel_buffer();
		let make_frame = |frame_seq| live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: pixel_buffer.clone(),
		};

		for frame_seq in 1..=4 {
			let frame = make_frame(frame_seq);
			let _ = shared.store(monitor_id, &frame);
		}

		let queued = shared.frames_after_seq_for_monitor(monitor_id, 1);
		let seqs: Vec<u64> = queued.into_iter().map(|frame| frame.frame_seq).collect();

		assert_eq!(seqs, vec![2, 3, 4]);
	}

	#[test]
	fn nonblocking_after_seq_query_does_not_prime_when_same_monitor_already_has_latest_frame() {
		let mut stream = live_frame_stream_macos::MacLiveFrameStream::with_waker(None);
		let monitor = crate::state::MonitorRect {
			id: 7,
			origin: crate::state::GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};
		let rect = crate::state::RectPoints::new(0, 0, 1, 1);
		let frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 4,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: test_pixel_buffer(),
		};
		let _ = stream.shared_latest_frame.store(monitor.id, &frame);

		assert!(stream.ordered_rgba_regions_after_seq_nonblocking(monitor, rect, 4).is_none());
		assert!(
			stream
				.shared_latest_frame
				.pending_monitor
				.lock()
				.expect("pending_monitor lock")
				.is_none()
		);
	}

	#[test]
	fn force_refresh_immediately_refreshes_when_seq_is_stalled() {
		assert!(live_frame_stream_macos::should_refresh_monitor_frame(
			7,
			7,
			Duration::from_millis(0),
			true,
		));
		assert!(!live_frame_stream_macos::should_refresh_monitor_frame(
			7,
			7,
			Duration::from_millis(10),
			false,
		));
	}

	#[test]
	fn force_refresh_does_not_refresh_when_newer_frame_already_exists() {
		assert!(!live_frame_stream_macos::should_refresh_monitor_frame(
			8,
			7,
			Duration::from_millis(200),
			true,
		));
	}

	#[test]
	fn stream_config_uses_cadence_queue_depth_contract() {
		let monitor = crate::state::MonitorRect {
			id: 7,
			origin: crate::state::GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};
		let config = live_frame_stream_macos::build_stream_config_for_monitor(
			monitor,
			live_frame_stream_macos::StreamCaptureTarget::FullMonitor,
		);

		assert_eq!(
			unsafe { config.queueDepth() },
			live_frame_stream_macos::STREAM_CONFIG_QUEUE_DEPTH as isize
		);
	}

	#[test]
	fn stream_config_uses_source_rect_for_scroll_capture_region() {
		let monitor = crate::state::MonitorRect {
			id: 7,
			origin: crate::state::GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};
		let region_points = crate::state::RectPoints::new(120, 80, 300, 220);
		let region_pixels = monitor.local_rect_to_pixels(region_points);
		let config = live_frame_stream_macos::build_stream_config_for_monitor(
			monitor,
			live_frame_stream_macos::StreamCaptureTarget::Region(
				live_frame_stream_macos::StreamCaptureRegion {
					rect_points: region_points,
					rect_pixels: region_pixels,
				},
			),
		);
		let source_rect = unsafe { config.sourceRect() };

		assert_eq!(unsafe { config.width() }, region_pixels.width as usize);
		assert_eq!(unsafe { config.height() }, region_pixels.height as usize);
		assert_eq!(source_rect.origin.x, f64::from(region_points.x));
		assert_eq!(source_rect.origin.y, f64::from(region_points.y));
		assert_eq!(source_rect.size.width, f64::from(region_points.width));
		assert_eq!(source_rect.size.height, f64::from(region_points.height));
	}

	#[test]
	fn stream_rect_maps_scroll_capture_region_requests_to_stream_local_rect() {
		let capture_target = live_frame_stream_macos::StreamCaptureTarget::Region(
			live_frame_stream_macos::StreamCaptureRegion {
				rect_points: crate::state::RectPoints::new(60, 40, 220, 120),
				rect_pixels: crate::state::RectPoints::new(120, 80, 440, 240),
			},
		);

		assert_eq!(
			live_frame_stream_macos::stream_rect_for_requested_region(
				capture_target,
				crate::state::RectPoints::new(120, 80, 440, 240),
			),
			Some(crate::state::RectPoints::new(0, 0, 440, 240))
		);
		assert_eq!(
			live_frame_stream_macos::stream_rect_for_requested_region(
				capture_target,
				crate::state::RectPoints::new(140, 100, 100, 80),
			),
			Some(crate::state::RectPoints::new(20, 20, 100, 80))
		);
		assert_eq!(
			live_frame_stream_macos::stream_rect_for_requested_region(
				capture_target,
				crate::state::RectPoints::new(100, 80, 100, 80),
			),
			None
		);
	}

	#[test]
	fn sample_handler_queue_label_is_monitor_scoped() {
		assert_eq!(
			live_frame_stream_macos::sample_handler_queue_label(7),
			"io.hackink.rsnap.scroll-capture.sample-handler.monitor-7"
		);
		assert_ne!(
			live_frame_stream_macos::sample_handler_queue_label(7),
			live_frame_stream_macos::sample_handler_queue_label(9)
		);
	}

	#[test]
	fn sample_cursor_from_bgra_bytes_reads_rgb_without_patch() {
		let sample = live_frame_stream_macos::sample_cursor_from_bgra_bytes(
			&[
				1, 2, 3, 255, 11, 12, 13, 254, //
				21, 22, 23, 253, 31, 32, 33, 252,
			],
			8,
			2,
			2,
			1,
			0,
			false,
			0,
			0,
		)
		.expect("sample should exist inside bounds");

		assert_eq!(sample.rgb, Some(Rgb::new(13, 12, 11)));
		assert!(sample.patch.is_none());
	}

	#[test]
	fn sample_cursor_from_bgra_bytes_clamps_patch_edges() {
		let sample = live_frame_stream_macos::sample_cursor_from_bgra_bytes(
			&[
				1, 2, 3, 255, 11, 12, 13, 254, //
				21, 22, 23, 253, 31, 32, 33, 252,
			],
			8,
			2,
			2,
			0,
			0,
			true,
			3,
			3,
		)
		.expect("sample should exist inside bounds");
		let patch = sample.patch.expect("patch should be present");

		assert_eq!(patch.dimensions(), (3, 3));
		assert_eq!(patch.get_pixel(0, 0).0, [3, 2, 1, 255]);
		assert_eq!(patch.get_pixel(1, 0).0, [3, 2, 1, 255]);
		assert_eq!(patch.get_pixel(2, 2).0, [33, 32, 31, 252]);
	}
}
