use std::sync::{
	Arc, Mutex,
	atomic::{AtomicU64, Ordering},
	mpsc,
};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSError;
use objc2_screen_capture_kit::{SCContentFilter, SCStream, SCStreamOutputType};

use crate::live_frame_stream_macos::frame_store::SharedLatestFrame;
use crate::live_frame_stream_macos::stream_config::{self, StreamCaptureTarget};
use crate::live_frame_stream_macos::stream_filter::{self, StreamFilterConfig, StreamFilterMode};
use crate::live_frame_stream_macos::stream_output::StreamOutput;
use crate::live_frame_stream_macos::{STREAM_ERROR_RETAIN_FAILED_CODE, STREAM_ERROR_TIMEOUT_CODE};
use crate::state::MonitorRect;

pub(super) struct StreamState {
	pub(super) monitor_id: u32,
	pub(super) stream_generation: u64,
	self_capture_filter_complete: bool,
	stream: Retained<SCStream>,
	pub(super) output: Retained<StreamOutput>,
	sample_handler_queue: DispatchRetained<DispatchQueue>,
}
impl StreamState {
	pub(super) fn self_capture_filter_complete(&self) -> bool {
		self.self_capture_filter_complete
	}
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

pub(super) fn setup_stream_for_monitor(
	monitor: MonitorRect,
	filter: &StreamFilterConfig,
	capture_target: StreamCaptureTarget,
	frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	frame_seq_counter: Arc<AtomicU64>,
	shared_latest_frame: Arc<SharedLatestFrame>,
) -> Option<StreamState> {
	let setup_started_at = Instant::now();
	let prepared_filter = stream_filter::prepare_stream_filter_for_monitor(
		monitor,
		&filter.self_capture_exception_window_ids,
	)?;
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

pub(super) fn teardown_stream(state: &mut Option<StreamState>) {
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

pub(super) fn stream_error(code: isize) -> Retained<NSError> {
	NSError::new(code, objc2_foundation::ns_string!("io.hackink.rsnap.live_frame_stream"))
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
	let config = stream_config::build_stream_config_for_monitor(monitor, capture_target);
	let config_build_ms = config_build_started_at.elapsed().as_millis();
	let queue_build_started_at = Instant::now();
	let sample_handler_queue = stream_config::build_sample_handler_queue_for_monitor(monitor.id);
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
