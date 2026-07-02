use std::sync::{
	Arc, Mutex,
	atomic::{AtomicU64, Ordering},
	mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use image::RgbaImage;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSError;
use objc2_screen_capture_kit::{SCContentFilter, SCStream, SCStreamOutputType};

use crate::live_frame_stream_macos::frame_store::SharedLatestFrame;
use crate::live_frame_stream_macos::live_frame_buffer::{
	self, OrderedRegionFrame, QueuedPixelBufferFrame,
};
use crate::live_frame_stream_macos::stream_config::{self, StreamCaptureTarget};
use crate::live_frame_stream_macos::stream_filter::{self, StreamFilterConfig, StreamFilterMode};
use crate::live_frame_stream_macos::stream_output::StreamOutput;
use crate::live_frame_stream_macos::{
	STREAM_ERROR_RETAIN_FAILED_CODE, STREAM_ERROR_TIMEOUT_CODE,
	STREAM_INCOMPLETE_EXCEPTION_UPGRADE_BACKOFF, STREAM_REGION_FRAME_AHEAD_WAIT_TIMEOUT,
	STREAM_REGION_FRAME_MAX_AGE, STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL,
	STREAM_REGION_FRAME_REFRESH_TIMEOUT, STREAM_SETUP_BACKOFF,
};
use crate::state::{MonitorRect, RectPoints};

pub(super) struct StreamState {
	pub(super) monitor_id: u32,
	pub(super) stream_generation: u64,
	self_capture_filter_complete: bool,
	stream: Retained<SCStream>,
	pub(super) output: Retained<StreamOutput>,
	sample_handler_queue: DispatchRetained<DispatchQueue>,
}

pub(super) struct RefreshStreamArgs<'a> {
	pub(super) state: &'a mut Option<StreamState>,
	pub(super) last_setup_attempt_at: &'a mut Option<Instant>,
	pub(super) monitor: MonitorRect,
	pub(super) filter: &'a StreamFilterConfig,
	pub(super) capture_target: StreamCaptureTarget,
	pub(super) frame_waker: Option<Arc<dyn Fn() + Send + Sync>>,
	pub(super) frame_seq_counter: Arc<AtomicU64>,
	pub(super) shared_latest_frame: Arc<SharedLatestFrame>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StreamRequestProgress {
	AwaitingFirstFrame,
	Settled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StreamReuseDecision {
	SetupFresh,
	ReuseCurrent,
	RetryUpgradeUsingCurrent,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn refresh_stream_nonblocking(
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

pub(super) fn refresh_stream_requires_setup_backoff(
	current_monitor_id: Option<u32>,
	requested_monitor_id: u32,
) -> bool {
	current_monitor_id != Some(requested_monitor_id)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ensure_stream(
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
pub(super) fn latest_fresh_rgba_region(
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
	let stream_rect_px = super::stream_rect_for_requested_region(capture_target, rect_px)?;
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
		return live_frame_buffer::rgba_region_from_pixel_buffer(
			&frame.pixel_buffer,
			stream_rect_px,
		);
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
			return live_frame_buffer::rgba_region_from_pixel_buffer(
				&frame.pixel_buffer,
				stream_rect_px,
			);
		}

		if Instant::now() >= deadline {
			return None;
		}

		thread::sleep(STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL);
	}
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ordered_queued_rgba_regions_after_seq_nonblocking(
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
	let stream_rect_px = super::stream_rect_for_requested_region(capture_target, rect_px)?;
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
	let frames = fresh_queued_frames_after_seq(&stream_state.output, after_frame_seq);
	let frames = live_frame_buffer::ordered_rgba_regions_from_frames(frames, stream_rect_px);

	(!frames.is_empty()).then_some(frames)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ordered_fresh_rgba_regions_after_seq(
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
	let stream_rect_px = super::stream_rect_for_requested_region(capture_target, rect_px)?;
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
	let frames = fresh_queued_frames_after_seq(&stream_state.output, after_frame_seq);
	let frames = live_frame_buffer::ordered_rgba_regions_from_frames(frames, stream_rect_px);

	if !frames.is_empty() {
		return Some(frames);
	}

	let latest_frame = stream_state.output.latest_frame()?;

	if Instant::now().saturating_duration_since(latest_frame.captured_at)
		<= STREAM_REGION_FRAME_MAX_AGE
	{
		let deadline = Instant::now() + STREAM_REGION_FRAME_AHEAD_WAIT_TIMEOUT;

		loop {
			if Instant::now() >= deadline {
				break;
			}

			thread::sleep(STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL);

			let stream_state = state.as_ref()?;
			let frames = fresh_queued_frames_after_seq(&stream_state.output, after_frame_seq);
			let frames =
				live_frame_buffer::ordered_rgba_regions_from_frames(frames, stream_rect_px);

			if !frames.is_empty() {
				return Some(frames);
			}
		}
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
		let frames = fresh_queued_frames_after_seq(&stream_state.output, after_frame_seq);
		let frames = live_frame_buffer::ordered_rgba_regions_from_frames(frames, stream_rect_px);

		if !frames.is_empty() {
			return Some(frames);
		}
		if Instant::now() >= deadline {
			return None;
		}

		thread::sleep(STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL);
	}
}

pub(super) fn refresh_stream(args: RefreshStreamArgs<'_>) -> StreamRequestProgress {
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

pub(super) fn stream_reuse_decision(
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

pub(super) fn stream_setup_backoff(
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

pub(super) fn stream_error(code: isize) -> Retained<NSError> {
	NSError::new(code, objc2_foundation::ns_string!("io.hackink.rsnap.live_frame_stream"))
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

fn fresh_queued_frames_after_seq(
	output: &StreamOutput,
	after_frame_seq: u64,
) -> Vec<QueuedPixelBufferFrame> {
	let now = Instant::now();

	output
		.queued_frames_after_seq(after_frame_seq)
		.into_iter()
		.filter(|frame| {
			now.saturating_duration_since(frame.captured_at) <= STREAM_REGION_FRAME_MAX_AGE
		})
		.collect()
}
