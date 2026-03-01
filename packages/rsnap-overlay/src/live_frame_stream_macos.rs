use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use block2::RcBlock;
use image::RgbaImage;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, define_class};
use objc2_core_foundation::CFRetained;
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
	CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
	CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
	kCVReturnSuccess,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, ns_string};
use objc2_screen_capture_kit::{
	SCContentFilter, SCDisplay, SCShareableContent, SCStream, SCStreamConfiguration,
	SCStreamOutput, SCStreamOutputType, SCWindow,
};

use crate::state::{MonitorImageSnapshot, MonitorRect, Rgb};

#[derive(Debug, Default)]
struct StreamOutputIvars {
	latest: Mutex<Option<CFRetained<CVPixelBuffer>>>,
}

define_class!(
	#[unsafe(super = NSObject)]
	#[thread_kind = objc2::AnyThread]
	#[ivars = StreamOutputIvars]
	struct StreamOutput;

	unsafe impl NSObjectProtocol for StreamOutput {}

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

			let mut latest = self.ivars().latest.lock().unwrap();
			*latest = Some(image_buffer);
		}
	}
);

impl StreamOutput {
	fn new() -> Retained<Self> {
		let this = Self::alloc().set_ivars(StreamOutputIvars::default());
		unsafe { objc2::msg_send![super(this), init] }
	}

	fn latest_pixel_buffer(&self) -> Option<CFRetained<CVPixelBuffer>> {
		self.ivars().latest.lock().unwrap().clone()
	}
}

struct StreamState {
	monitor_id: u32,
	stream: Retained<SCStream>,
	output: Retained<StreamOutput>,
}

pub(crate) struct MacLiveFrameStream {
	request_tx: mpsc::Sender<WorkerRequest>,
	worker: Option<JoinHandle<()>>,
}

impl MacLiveFrameStream {
	pub(crate) fn new() -> Self {
		let (request_tx, request_rx) = mpsc::channel();
		let worker = thread::spawn(move || stream_worker_loop(request_rx));

		Self { request_tx, worker: Some(worker) }
	}

	pub(crate) fn sample_rgb(&mut self, monitor: MonitorRect, x_px: u32, y_px: u32) -> Option<Rgb> {
		self.request(|reply_tx| WorkerRequest::SampleRgb { monitor, x_px, y_px, reply_tx })
			.flatten()
	}

	pub(crate) fn sample_rgba_patch(
		&mut self,
		monitor: MonitorRect,
		center_x_px: u32,
		center_y_px: u32,
		width_px: u32,
		height_px: u32,
	) -> Option<RgbaImage> {
		self.request(|reply_tx| WorkerRequest::SampleRgbaPatch {
			monitor,
			center_x_px,
			center_y_px,
			width_px,
			height_px,
			reply_tx,
		})
		.flatten()
	}

	pub(crate) fn latest_rgba_snapshot(
		&mut self,
		monitor: MonitorRect,
	) -> Option<Arc<MonitorImageSnapshot>> {
		self.request(|reply_tx| WorkerRequest::LatestRgbaSnapshot { monitor, reply_tx }).flatten()
	}

	fn request<T>(
		&self,
		build_request: impl FnOnce(mpsc::Sender<T>) -> WorkerRequest,
	) -> Option<T> {
		let (reply_tx, reply_rx) = mpsc::channel();
		self.request_tx.send(build_request(reply_tx)).ok()?;
		reply_rx.recv_timeout(STREAM_RPC_TIMEOUT).ok()
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

const STREAM_RPC_TIMEOUT: Duration = Duration::from_secs(3);
const STREAM_SETUP_BACKOFF: Duration = Duration::from_millis(300);
const STREAM_ERROR_TIMEOUT_CODE: isize = 1;
const STREAM_ERROR_NULL_CONTENT_CODE: isize = 2;

enum WorkerRequest {
	SampleRgb {
		monitor: MonitorRect,
		x_px: u32,
		y_px: u32,
		reply_tx: mpsc::Sender<Option<Rgb>>,
	},
	SampleRgbaPatch {
		monitor: MonitorRect,
		center_x_px: u32,
		center_y_px: u32,
		width_px: u32,
		height_px: u32,
		reply_tx: mpsc::Sender<Option<RgbaImage>>,
	},
	LatestRgbaSnapshot {
		monitor: MonitorRect,
		reply_tx: mpsc::Sender<Option<Arc<MonitorImageSnapshot>>>,
	},
	Shutdown,
}

fn stream_worker_loop(request_rx: mpsc::Receiver<WorkerRequest>) {
	let mut state: Option<StreamState> = None;
	let mut last_setup_attempt_at: Option<Instant> = None;

	while let Ok(request) = request_rx.recv() {
		match request {
			WorkerRequest::SampleRgb { monitor, x_px, y_px, reply_tx } => {
				let rgb = ensure_stream(
					&mut state,
					&mut last_setup_attempt_at,
					STREAM_SETUP_BACKOFF,
					monitor,
				)
				.and(state.as_ref())
				.and_then(|stream_state| {
					let pixel_buffer = stream_state.output.latest_pixel_buffer()?;
					sample_rgb_from_pixel_buffer(&pixel_buffer, x_px, y_px)
				});
				let _ = reply_tx.send(rgb);
			},
			WorkerRequest::SampleRgbaPatch {
				monitor,
				center_x_px,
				center_y_px,
				width_px,
				height_px,
				reply_tx,
			} => {
				let patch = ensure_stream(
					&mut state,
					&mut last_setup_attempt_at,
					STREAM_SETUP_BACKOFF,
					monitor,
				)
				.and(state.as_ref())
				.and_then(|stream_state| {
					let pixel_buffer = stream_state.output.latest_pixel_buffer()?;
					sample_patch_from_pixel_buffer(
						&pixel_buffer,
						center_x_px,
						center_y_px,
						width_px,
						height_px,
					)
				});
				let _ = reply_tx.send(patch);
			},
			WorkerRequest::LatestRgbaSnapshot { monitor, reply_tx } => {
				let snapshot = ensure_stream(
					&mut state,
					&mut last_setup_attempt_at,
					STREAM_SETUP_BACKOFF,
					monitor,
				)
				.and(state.as_ref())
				.and_then(|stream_state| {
					let pixel_buffer = stream_state.output.latest_pixel_buffer()?;
					let (width_px, height_px) = pixel_buffer_size_px(&pixel_buffer)?;
					let image = rgba_image_from_pixel_buffer(&pixel_buffer, width_px, height_px)?;
					Some(Arc::new(MonitorImageSnapshot {
						captured_at: Instant::now(),
						monitor,
						image: Arc::new(image),
					}))
				});
				let _ = reply_tx.send(snapshot);
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
	*state = Some(setup_stream_for_monitor(monitor)?);

	Some(())
}

fn teardown_stream(state: &mut Option<StreamState>) {
	let Some(state) = state.take() else {
		return;
	};

	let stop_block = RcBlock::new(|_err: *mut NSError| {});
	unsafe { state.stream.stopCaptureWithCompletionHandler(Some(&stop_block)) };
}

fn setup_stream_for_monitor(monitor: MonitorRect) -> Option<StreamState> {
	let content = get_shareable_content().ok()?;
	let display = find_display(&content, monitor.id)?;

	let excluded_windows: Retained<NSArray<SCWindow>> = NSArray::new();
	let filter = unsafe {
		SCContentFilter::initWithDisplay_excludingWindows(
			SCContentFilter::alloc(),
			&display,
			&excluded_windows,
		)
	};
	let config = build_stream_config_for_monitor(monitor);
	let stream = unsafe {
		SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &config, None)
	};

	let output = StreamOutput::new();
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

fn get_shareable_content() -> Result<Retained<SCShareableContent>, Retained<NSError>> {
	let (tx, rx) = mpsc::sync_channel::<Result<Retained<SCShareableContent>, Retained<NSError>>>(1);
	let tx = Mutex::new(Some(tx));

	let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
		let Some(tx) = tx.lock().unwrap().take() else {
			return;
		};

		if !err.is_null() {
			let err = unsafe { Retained::retain(err) }.unwrap();
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
		let Some(tx) = tx.lock().unwrap().take() else {
			return;
		};

		if err.is_null() {
			let _ = tx.send(Ok(()));
			return;
		}

		let err = unsafe { Retained::retain(err) }.unwrap();
		let _ = tx.send(Err(err));
	});

	unsafe { stream.startCaptureWithCompletionHandler(Some(&block)) };

	rx.recv_timeout(Duration::from_secs(2)).map_err(|_| stream_error(STREAM_ERROR_TIMEOUT_CODE))?
}

fn stream_error(code: isize) -> Retained<NSError> {
	NSError::new(code, ns_string!("io.hackink.rsnap.live_frame_stream"))
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

	// Keep queue shallow to avoid backpressure/jank.
	unsafe { config.setQueueDepth(2) };

	config
}

fn pixel_buffer_size_px(pixel_buffer: &CFRetained<CVPixelBuffer>) -> Option<(u32, u32)> {
	let width = objc2_core_video::CVPixelBufferGetWidth(pixel_buffer);
	let height = objc2_core_video::CVPixelBufferGetHeight(pixel_buffer);
	let width = u32::try_from(width).ok()?;
	let height = u32::try_from(height).ok()?;

	Some((width, height))
}

fn sample_rgb_from_pixel_buffer(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
	x_px: u32,
	y_px: u32,
) -> Option<Rgb> {
	let (width, height) = pixel_buffer_size_px(pixel_buffer)?;
	if x_px >= width || y_px >= height {
		return None;
	}

	let lock_result =
		unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };
	if lock_result != kCVReturnSuccess {
		return None;
	}

	let rgb = (|| {
		let base = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;
		if base.is_null() {
			return None;
		}
		let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
		let offset =
			(y_px as usize).saturating_mul(bytes_per_row).saturating_add((x_px as usize) * 4);
		let b = unsafe { *base.add(offset) };
		let g = unsafe { *base.add(offset + 1) };
		let r = unsafe { *base.add(offset + 2) };

		Some(Rgb::new(r, g, b))
	})();

	let _ =
		unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	rgb
}

fn sample_patch_from_pixel_buffer(
	pixel_buffer: &CFRetained<CVPixelBuffer>,
	center_x_px: u32,
	center_y_px: u32,
	width_px: u32,
	height_px: u32,
) -> Option<RgbaImage> {
	let (in_w, in_h) = pixel_buffer_size_px(pixel_buffer)?;
	let out_w = width_px.max(1);
	let out_h = height_px.max(1);
	let half_w = (out_w as i32) / 2;
	let half_h = (out_h as i32) / 2;
	let center_x = center_x_px as i32;
	let center_y = center_y_px as i32;
	let in_w = in_w as i32;
	let in_h = in_h as i32;

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
		let mut out = RgbaImage::new(out_w, out_h);

		for oy in 0..(out_h as i32) {
			let iy = (center_y - half_h + oy).clamp(0, in_h.saturating_sub(1));
			for ox in 0..(out_w as i32) {
				let ix = (center_x - half_w + ox).clamp(0, in_w.saturating_sub(1));
				let offset =
					(iy as usize).saturating_mul(bytes_per_row).saturating_add((ix as usize) * 4);
				let b = unsafe { *base.add(offset) };
				let g = unsafe { *base.add(offset + 1) };
				let r = unsafe { *base.add(offset + 2) };
				let a = unsafe { *base.add(offset + 3) };

				out.put_pixel(ox as u32, oy as u32, image::Rgba([r, g, b, a]));
			}
		}

		Some(out)
	})();

	let _ =
		unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly) };

	out
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
