#![allow(
	dead_code,
	reason = "XY-113 narrows the public crate facade while leaving ScreenCaptureKit implementation cleanup to a separate follow-up lane."
)]

mod frame_store;
mod live_frame_buffer;
mod stream_config;
mod stream_filter;
mod stream_lifecycle;
mod stream_output;

use std::ptr;
use std::ptr::NonNull;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::{
	Arc,
	atomic::AtomicU64,
	mpsc::{self, Receiver, Sender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use image::RgbaImage;
use objc2::rc::Retained;
use objc2_core_foundation::{self, CFRetained};
use objc2_core_video::CVPixelBufferCreate;
use objc2_foundation::NSError;

use self::frame_store::{SharedLatestFrame, StreamGenerationStatus};
use self::live_frame_buffer::{OrderedRegionFrame, QueuedPixelBufferFrame, SharedPixelBuffer};
use self::stream_config::{StreamCaptureRegion, StreamCaptureTarget};
use self::stream_filter::StreamFilterConfig;
use self::stream_lifecycle::{StreamRequestProgress, StreamState};
#[cfg(test)]
use self::stream_lifecycle::{
	StreamReuseDecision, refresh_stream_requires_setup_backoff, stream_reuse_decision,
	stream_setup_backoff,
};
use crate::state::{LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints, Rgb};

pub(crate) const STREAM_REGION_FRAME_MAX_AGE: Duration = Duration::from_millis(90);

const STREAM_RPC_TIMEOUT: Duration = Duration::from_secs(3);
const STREAM_SETUP_BACKOFF: Duration = Duration::from_millis(300);
const STREAM_INCOMPLETE_EXCEPTION_UPGRADE_BACKOFF: Duration = Duration::from_secs(3);
const STREAM_FRAME_QUEUE_CAPACITY: usize = 16;
const STREAM_ACTIVE_GESTURE_FORCE_REFRESH_MIN_AGE: Duration = Duration::from_millis(60);
const STREAM_REGION_FRAME_REFRESH_TIMEOUT: Duration = Duration::from_millis(180);
const STREAM_REGION_FRAME_AHEAD_WAIT_TIMEOUT: Duration = Duration::from_millis(24);
const STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL: Duration = Duration::from_millis(4);
const STREAM_POST_SETUP_FRAME_GRACE: Duration = STREAM_SETUP_BACKOFF;
const STREAM_ERROR_TIMEOUT_CODE: isize = 1;
const STREAM_ERROR_NULL_CONTENT_CODE: isize = 2;
const STREAM_ERROR_RETAIN_FAILED_CODE: isize = 3;

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
				let sample = self::live_frame_buffer::sample_cursor_from_pixel_buffer(
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
		let (width_px, height_px) =
			self::live_frame_buffer::pixel_buffer_size_px(&frame.pixel_buffer)?;
		let image = self::live_frame_buffer::rgba_image_from_pixel_buffer(
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
		let frames =
			crate::live_frame_stream_macos::live_frame_buffer::ordered_rgba_regions_from_frames(
				frames,
				stream_rect_px,
			);

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

fn stream_error(code: isize) -> Retained<NSError> {
	stream_lifecycle::stream_error(code)
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

	stream_lifecycle::teardown_stream(&mut state);
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

	stream_lifecycle::teardown_stream(state);

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

	let progress = stream_lifecycle::ensure_stream(
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

	let progress = stream_lifecycle::refresh_stream_nonblocking(
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
	let _ = stream_lifecycle::ensure_stream(
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
			self::live_frame_buffer::sample_cursor_from_pixel_buffer(
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
	let _ = stream_lifecycle::ensure_stream(
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
		let (width_px, height_px) =
			self::live_frame_buffer::pixel_buffer_size_px(&frame.pixel_buffer)?;
		let image = self::live_frame_buffer::rgba_image_from_pixel_buffer(
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
	let image = stream_lifecycle::latest_fresh_rgba_region(
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
		stream_lifecycle::ordered_queued_rgba_regions_after_seq_nonblocking(
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
		stream_lifecycle::ordered_fresh_rgba_regions_after_seq(
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

#[cfg(test)]
mod tests {
	use std::ptr::{self, NonNull};
	use std::sync::{Arc, atomic::AtomicU64};
	use std::time::Duration;

	use objc2_core_foundation::CFRetained;
	use objc2_core_video::{CVPixelBufferCreate, kCVPixelFormatType_32BGRA, kCVReturnSuccess};

	use crate::live_frame_stream_macos::STREAM_POST_SETUP_FRAME_GRACE;
	use crate::live_frame_stream_macos::{self, stream_config, stream_filter};

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
	fn with_waker_streams_preserve_self_capture_exception_window_ids() {
		let stream = live_frame_stream_macos::MacLiveFrameStream::with_self_capture_exception_window_ids_and_waker(
			vec![7, 11],
			None,
		);

		assert_eq!(stream.debug_self_capture_exception_window_ids(), &[7, 11]);
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
			&stream_filter::StreamFilterConfig::default(),
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
		assert!(stream.shared_latest_frame.pending_monitor_is_none());
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
		let config = stream_config::build_stream_config_for_monitor(
			monitor,
			live_frame_stream_macos::StreamCaptureTarget::FullMonitor,
		);

		assert_eq!(
			unsafe { config.queueDepth() },
			stream_config::STREAM_CONFIG_QUEUE_DEPTH as isize
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
		let config = stream_config::build_stream_config_for_monitor(
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
			stream_config::sample_handler_queue_label(7),
			"io.hackink.rsnap.scroll-capture.sample-handler.monitor-7"
		);
		assert_ne!(
			stream_config::sample_handler_queue_label(7),
			stream_config::sample_handler_queue_label(9)
		);
	}
}
