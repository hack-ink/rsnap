use std::collections::VecDeque;
use std::ops::Deref;
use std::process;
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicU64, Ordering},
	mpsc::{self, Receiver, Sender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use block2::RcBlock;
use image::RgbaImage;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, Message};
use objc2_core_foundation::CFRetained;
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
			self.ivars().shared_latest_frame.store(self.ivars().monitor_id, &frame);

			if let Some(frame_waker) = self.ivars().frame_waker.as_ref() {
				frame_waker();
			}
		}
	}
);

const STREAM_RPC_TIMEOUT: Duration = Duration::from_secs(3);
const STREAM_SETUP_BACKOFF: Duration = Duration::from_millis(300);
const STREAM_FRAME_QUEUE_CAPACITY: usize = 8;
const STREAM_REGION_FRAME_MAX_AGE: Duration = Duration::from_millis(90);
const STREAM_REGION_FRAME_REFRESH_TIMEOUT: Duration = Duration::from_millis(180);
const STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL: Duration = Duration::from_millis(4);
const STREAM_ERROR_TIMEOUT_CODE: isize = 1;
const STREAM_ERROR_NULL_CONTENT_CODE: isize = 2;
const STREAM_ERROR_RETAIN_FAILED_CODE: isize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamFilterMode {
	ExcludeCurrentProcess,
}

enum WorkerRequest {
	EnsureMonitor {
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

#[allow(dead_code)]
pub(crate) struct OrderedRegionFrame {
	pub(crate) frame_seq: u64,
	pub(crate) captured_at: Instant,
	pub(crate) image: RgbaImage,
}

pub(crate) struct MacLiveFrameStream {
	request_tx: Sender<WorkerRequest>,
	shared_latest_frame: Arc<SharedLatestFrame>,
	worker: Option<JoinHandle<()>>,
}
impl MacLiveFrameStream {
	pub(crate) fn new() -> Self {
		Self::with_waker(None)
	}

	pub(crate) fn with_waker(frame_waker: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
		let (request_tx, request_rx) = mpsc::channel();
		let shared_latest_frame = Arc::new(SharedLatestFrame::default());
		let worker_shared_latest_frame = shared_latest_frame.clone();
		let worker = thread::spawn(move || {
			stream_worker_loop(request_rx, frame_waker, worker_shared_latest_frame);
		});

		Self { request_tx, shared_latest_frame, worker: Some(worker) }
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
		x_px: u32,
		y_px: u32,
		want_patch: bool,
		patch_width_px: u32,
		patch_height_px: u32,
	) -> Option<LiveCursorSample> {
		let sample =
			self.shared_latest_frame.latest_frame_for_monitor(monitor.id).and_then(|frame| {
				sample_cursor_from_pixel_buffer(
					&frame.pixel_buffer,
					x_px,
					y_px,
					want_patch,
					patch_width_px,
					patch_height_px,
				)
			});

		if sample.is_none() {
			self.ensure_monitor_nonblocking(monitor);
		}

		sample
	}

	pub(crate) fn latest_rgba_snapshot(
		&mut self,
		monitor: MonitorRect,
	) -> Option<Arc<MonitorImageSnapshot>> {
		self.request(|reply_tx| WorkerRequest::LatestRgbaSnapshot { monitor, reply_tx }).flatten()
	}

	pub(crate) fn latest_rgba_region(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
	) -> Option<RgbaImage> {
		self.request(|reply_tx| WorkerRequest::LatestRgbaRegion { monitor, rect_px, reply_tx })
			.flatten()
	}

	pub(crate) fn latest_rgba_region_if_new(
		&mut self,
		monitor: MonitorRect,
		rect_px: RectPoints,
		after_frame_seq: u64,
	) -> Option<(u64, RgbaImage)> {
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
		self.request(|reply_tx| WorkerRequest::OrderedRgbaRegionsAfterSeq {
			monitor,
			rect_px,
			after_frame_seq,
			reply_tx,
		})
		.flatten()
	}

	fn request<T>(&self, build_request: impl FnOnce(Sender<T>) -> WorkerRequest) -> Option<T> {
		let (reply_tx, reply_rx) = mpsc::channel();

		self.request_tx.send(build_request(reply_tx)).ok()?;

		reply_rx.recv_timeout(STREAM_RPC_TIMEOUT).ok()
	}

	fn ensure_monitor_nonblocking(&self, monitor: MonitorRect) {
		if !self.shared_latest_frame.begin_ensure_monitor(monitor.id) {
			return;
		}
		if self.request_tx.send(WorkerRequest::EnsureMonitor { monitor }).is_err() {
			self.shared_latest_frame.finish_ensure_monitor(monitor.id);
		}
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
	captured_at: Instant,
	pixel_buffer: SharedPixelBuffer,
}

#[derive(Clone)]
struct LatestQueuedPixelBufferFrame {
	monitor_id: u32,
	frame: QueuedPixelBufferFrame,
}

#[derive(Default)]
struct SharedLatestFrame {
	latest: Mutex<Option<LatestQueuedPixelBufferFrame>>,
	pending_monitor: Mutex<Option<u32>>,
}
impl SharedLatestFrame {
	fn store(&self, monitor_id: u32, frame: &QueuedPixelBufferFrame) {
		match self.latest.lock() {
			Ok(mut guard) => {
				*guard = Some(LatestQueuedPixelBufferFrame { monitor_id, frame: frame.clone() });
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				*guard = Some(LatestQueuedPixelBufferFrame { monitor_id, frame: frame.clone() });
			},
		}

		self.finish_ensure_monitor(monitor_id);
	}

	fn latest_frame_for_monitor(&self, monitor_id: u32) -> Option<QueuedPixelBufferFrame> {
		match self.latest.lock() {
			Ok(guard) => guard
				.as_ref()
				.and_then(|latest| (latest.monitor_id == monitor_id).then(|| latest.frame.clone())),
			Err(poisoned) => poisoned
				.into_inner()
				.as_ref()
				.and_then(|latest| (latest.monitor_id == monitor_id).then(|| latest.frame.clone())),
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

	fn finish_ensure_monitor(&self, monitor_id: u32) {
		match self.pending_monitor.lock() {
			Ok(mut guard) => {
				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					*guard = None;
				}
			},
			Err(poisoned) => {
				let mut guard = poisoned.into_inner();

				if guard.is_some_and(|pending_monitor_id| pending_monitor_id == monitor_id) {
					*guard = None;
				}
			},
		}
	}
}

struct StreamOutputIvars {
	monitor_id: u32,
	frames: Mutex<VecDeque<QueuedPixelBufferFrame>>,
	frame_seq_counter: Arc<AtomicU64>,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	shared_latest_frame: Arc<SharedLatestFrame>,
}
impl StreamOutputIvars {
	fn new(
		monitor_id: u32,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
		frame_seq_counter: Arc<AtomicU64>,
		shared_latest_frame: Arc<SharedLatestFrame>,
	) -> Self {
		Self {
			monitor_id,
			frames: Mutex::new(VecDeque::with_capacity(STREAM_FRAME_QUEUE_CAPACITY)),
			frame_seq_counter,
			frame_waker,
			shared_latest_frame,
		}
	}
}

struct StreamState {
	monitor_id: u32,
	stream: Retained<SCStream>,
	output: Retained<StreamOutput>,
}

impl StreamOutput {
	fn new(
		monitor_id: u32,
		frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
		frame_seq_counter: Arc<AtomicU64>,
		shared_latest_frame: Arc<SharedLatestFrame>,
	) -> Retained<Self> {
		let this = Self::alloc().set_ivars(StreamOutputIvars::new(
			monitor_id,
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

fn stream_worker_loop(
	request_rx: Receiver<WorkerRequest>,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) {
	let frame_seq_counter = Arc::new(AtomicU64::new(0));
	let mut state: Option<StreamState> = None;
	let mut last_setup_attempt_at: Option<Instant> = None;

	while let Ok(request) = request_rx.recv() {
		match request {
			WorkerRequest::EnsureMonitor { monitor } => {
				let _ = ensure_stream(
					&mut state,
					&mut last_setup_attempt_at,
					STREAM_SETUP_BACKOFF,
					monitor,
					frame_waker.clone(),
					frame_seq_counter.clone(),
					shared_latest_frame.clone(),
				);

				shared_latest_frame.finish_ensure_monitor(monitor.id);
			},
			WorkerRequest::SampleCursor {
				monitor,
				x_px,
				y_px,
				want_patch,
				patch_width_px,
				patch_height_px,
				reply_tx,
			} => {
				let rgb = ensure_stream(
					&mut state,
					&mut last_setup_attempt_at,
					STREAM_SETUP_BACKOFF,
					monitor,
					frame_waker.clone(),
					frame_seq_counter.clone(),
					shared_latest_frame.clone(),
				)
				.and_then(|_| {
					let stream_state = state.as_ref()?;

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
			},
			WorkerRequest::LatestRgbaSnapshot { monitor, reply_tx } => {
				let snapshot = ensure_stream(
					&mut state,
					&mut last_setup_attempt_at,
					STREAM_SETUP_BACKOFF,
					monitor,
					frame_waker.clone(),
					frame_seq_counter.clone(),
					shared_latest_frame.clone(),
				)
				.and_then(|_| {
					let stream_state = state.as_ref()?;
					let frame = stream_state.output.latest_frame()?;
					let (width_px, height_px) = pixel_buffer_size_px(&frame.pixel_buffer)?;
					let image =
						rgba_image_from_pixel_buffer(&frame.pixel_buffer, width_px, height_px)?;

					Some(Arc::new(MonitorImageSnapshot {
						captured_at: frame.captured_at,
						monitor,
						image: Arc::new(image),
					}))
				});
				let _ = reply_tx.send(snapshot);
			},
			WorkerRequest::LatestRgbaRegion { monitor, rect_px, reply_tx } => {
				let image = latest_fresh_rgba_region(
					&mut state,
					&mut last_setup_attempt_at,
					monitor,
					rect_px,
					frame_waker.clone(),
					frame_seq_counter.clone(),
					shared_latest_frame.clone(),
				);
				let _ = reply_tx.send(image);
			},
			WorkerRequest::OrderedRgbaRegionsAfterSeq {
				monitor,
				rect_px,
				after_frame_seq,
				reply_tx,
			} => {
				let frames = ordered_fresh_rgba_regions_after_seq(
					&mut state,
					&mut last_setup_attempt_at,
					monitor,
					rect_px,
					after_frame_seq,
					frame_waker.clone(),
					frame_seq_counter.clone(),
					shared_latest_frame.clone(),
				);
				let _ = reply_tx.send(frames);
			},
			WorkerRequest::Shutdown => break,
		}
	}

	teardown_stream(&mut state);
}

fn ensure_stream(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	setup_backoff: Duration,
	monitor: MonitorRect,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<()> {
	if state.as_ref().is_some_and(|current| current.monitor_id == monitor.id) {
		return Some(());
	}

	let now = Instant::now();

	if last_setup_attempt_at.is_some_and(|t| now.duration_since(t) < setup_backoff) {
		return None;
	}

	*last_setup_attempt_at = Some(now);

	teardown_stream(state);

	*state = Some(setup_stream_for_monitor(
		monitor,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	)?);

	Some(())
}

fn latest_fresh_rgba_region(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	rect_px: RectPoints,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<RgbaImage> {
	ensure_stream(
		state,
		last_setup_attempt_at,
		STREAM_SETUP_BACKOFF,
		monitor,
		frame_waker.clone(),
		frame_seq_counter.clone(),
		shared_latest_frame.clone(),
	)?;

	let now = Instant::now();
	let stream_state = state.as_ref()?;

	if let Some(frame) = stream_state.output.latest_frame()
		&& now.saturating_duration_since(frame.captured_at) <= STREAM_REGION_FRAME_MAX_AGE
	{
		return rgba_region_from_pixel_buffer(&frame.pixel_buffer, rect_px);
	}

	refresh_stream(
		state,
		last_setup_attempt_at,
		monitor,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	)?;

	let min_captured_at = Instant::now();
	let deadline = min_captured_at + STREAM_REGION_FRAME_REFRESH_TIMEOUT;

	loop {
		let stream_state = state.as_ref()?;

		if let Some(frame) = stream_state.output.latest_frame()
			&& frame.captured_at >= min_captured_at
		{
			return rgba_region_from_pixel_buffer(&frame.pixel_buffer, rect_px);
		}

		if Instant::now() >= deadline {
			return None;
		}

		thread::sleep(STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL);
	}
}

#[allow(clippy::too_many_arguments)]
fn ordered_fresh_rgba_regions_after_seq(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	rect_px: RectPoints,
	after_frame_seq: u64,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<Vec<OrderedRegionFrame>> {
	ensure_stream(
		state,
		last_setup_attempt_at,
		STREAM_SETUP_BACKOFF,
		monitor,
		frame_waker.clone(),
		frame_seq_counter.clone(),
		shared_latest_frame.clone(),
	)?;

	let stream_state = state.as_ref()?;
	let frames = stream_state.output.queued_frames_after_seq(after_frame_seq);
	let frames = ordered_rgba_regions_from_frames(frames, rect_px);

	if !frames.is_empty() {
		return Some(frames);
	}

	let latest_frame = stream_state.output.latest_frame()?;

	if Instant::now().saturating_duration_since(latest_frame.captured_at)
		<= STREAM_REGION_FRAME_MAX_AGE
	{
		return None;
	}

	refresh_stream(
		state,
		last_setup_attempt_at,
		monitor,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	)?;

	let min_captured_at = Instant::now();
	let deadline = min_captured_at + STREAM_REGION_FRAME_REFRESH_TIMEOUT;

	loop {
		let stream_state = state.as_ref()?;
		let frames = stream_state.output.queued_frames_after_seq(after_frame_seq);
		let frames = ordered_rgba_regions_from_frames(frames, rect_px);

		if !frames.is_empty() {
			return Some(frames);
		}
		if Instant::now() >= deadline {
			return None;
		}

		thread::sleep(STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL);
	}
}

fn refresh_stream(
	state: &mut Option<StreamState>,
	last_setup_attempt_at: &mut Option<Instant>,
	monitor: MonitorRect,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<()> {
	*last_setup_attempt_at = Some(Instant::now());

	teardown_stream(state);

	*state = Some(setup_stream_for_monitor(
		monitor,
		frame_waker,
		frame_seq_counter,
		shared_latest_frame,
	)?);

	Some(())
}

fn teardown_stream(state: &mut Option<StreamState>) {
	let Some(state) = state.take() else {
		return;
	};
	let stop_block = RcBlock::new(|_err: *mut NSError| {});

	unsafe { state.stream.stopCaptureWithCompletionHandler(Some(&stop_block)) };
}

fn setup_stream_for_monitor(
	monitor: MonitorRect,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<StreamState> {
	let content = get_shareable_content().ok()?;
	let display = find_display(&content, monitor.id)?;
	let excluded_windows: Retained<NSArray<SCWindow>> = NSArray::new();
	let Some((StreamFilterMode::ExcludeCurrentProcess, current_process_application)) =
		stream_filter_mode_for_current_process(find_current_process_application(&content))
	else {
		tracing::warn!(
			op = "live_frame_stream.setup_filter_missing_current_process",
			monitor_id = monitor.id,
			pid = process::id(),
			"Skipped ScreenCaptureKit stream setup because rsnap could not exclude its own windows from capture."
		);

		return None;
	};
	let excluded_applications = NSArray::from_retained_slice(&[current_process_application]);

	tracing::trace!(
		op = "live_frame_stream.setup_filter_excluding_current_process",
		monitor_id = monitor.id,
		pid = process::id(),
		"Configured ScreenCaptureKit to exclude rsnap windows from the live stream."
	);

	let filter = unsafe {
		SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
			SCContentFilter::alloc(),
			&display,
			&excluded_applications,
			&excluded_windows,
		)
	};
	let config = build_stream_config_for_monitor(monitor);
	let output = StreamOutput::new(monitor.id, frame_waker, frame_seq_counter, shared_latest_frame);
	let delegate_proto = ProtocolObject::from_ref(&*output);
	let stream = unsafe {
		SCStream::initWithFilter_configuration_delegate(
			SCStream::alloc(),
			&filter,
			&config,
			Some(delegate_proto),
		)
	};
	let output_proto = ProtocolObject::from_ref(&*output);

	if unsafe {
		stream.addStreamOutput_type_sampleHandlerQueue_error(
			output_proto,
			SCStreamOutputType::Screen,
			None,
		)
	}
	.is_err()
	{
		return None;
	}
	if start_capture_blocking(&stream).is_err() {
		return None;
	}

	Some(StreamState { monitor_id: monitor.id, stream, output })
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

fn stream_filter_mode_for_current_process<T>(
	current_process_application: Option<T>,
) -> Option<(StreamFilterMode, T)> {
	current_process_application
		.map(|application| (StreamFilterMode::ExcludeCurrentProcess, application))
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

	unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };

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

fn build_stream_config_for_monitor(monitor: MonitorRect) -> Retained<SCStreamConfiguration> {
	let config = unsafe { SCStreamConfiguration::new() };
	// Prefer full-resolution capture.
	let sf = monitor.scale_factor().max(1.0);
	let width_px = ((monitor.width as f32) * sf).round().max(1.0) as usize;
	let height_px = ((monitor.height as f32) * sf).round().max(1.0) as usize;

	unsafe { config.setWidth(width_px) };
	unsafe { config.setHeight(height_px) };
	// Keep cursor out of the frame so sampling isn't affected by pointer pixels.
	unsafe { config.setShowsCursor(false) };
	unsafe { config.setShowMouseClicks(false) };

	// 4cc("BGRA")
	let bgra = u32::from_be_bytes(*b"BGRA");

	unsafe { config.setPixelFormat(bgra) };
	unsafe { config.setMinimumFrameInterval(kCMTimeZero) };
	// Keep queue shallow while preserving enough buffering for rapid cursor updates.
	unsafe { config.setQueueDepth(3) };

	config
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
		let bytes = unsafe { std::slice::from_raw_parts(base, byte_len) };

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
) -> Option<RgbaImage> {
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
			let row =
				unsafe { std::slice::from_raw_parts(base.add(y * bytes_per_row), bytes_per_row) };

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
			let row = unsafe { std::slice::from_raw_parts(base.add(row_offset), bytes_per_row) };

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
	use crate::live_frame_stream_macos::{
		StreamFilterMode, sample_cursor_from_bgra_bytes, stream_filter_mode_for_current_process,
	};
	use crate::state::Rgb;

	#[test]
	fn stream_filter_mode_requires_current_process_application() {
		assert_eq!(
			stream_filter_mode_for_current_process(Some(42_u32)),
			Some((StreamFilterMode::ExcludeCurrentProcess, 42))
		);
		assert_eq!(stream_filter_mode_for_current_process::<u32>(None), None);
	}

	#[test]
	fn sample_cursor_from_bgra_bytes_reads_rgb_without_patch() {
		let sample = sample_cursor_from_bgra_bytes(
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
		let sample = sample_cursor_from_bgra_bytes(
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
