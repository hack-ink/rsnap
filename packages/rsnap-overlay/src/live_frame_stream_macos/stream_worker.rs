use std::sync::{
	Arc,
	atomic::AtomicU64,
	mpsc::{Receiver, Sender},
};
use std::time::{Duration, Instant};

use image::RgbaImage;

use crate::live_frame_stream_macos::frame_store::{SharedLatestFrame, StreamGenerationStatus};
use crate::live_frame_stream_macos::live_frame_buffer::{self, OrderedRegionFrame};
use crate::live_frame_stream_macos::stream_config::StreamCaptureTarget;
use crate::live_frame_stream_macos::stream_filter::StreamFilterConfig;
use crate::live_frame_stream_macos::stream_lifecycle::{self, StreamRequestProgress};
use crate::live_frame_stream_macos::stream_setup::{self, StreamState};
use crate::live_frame_stream_macos::{STREAM_REGION_FRAME_MAX_AGE, STREAM_SETUP_BACKOFF};
use crate::state::{LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints};

pub(super) enum WorkerRequest {
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

pub(super) fn stream_rect_for_requested_region(
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

pub(super) fn should_refresh_monitor_frame(
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

pub(super) fn stream_worker_loop(
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

	stream_setup::teardown_stream(&mut state);
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

	stream_setup::teardown_stream(state);

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
			live_frame_buffer::sample_cursor_from_pixel_buffer(
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
	use std::sync::{Arc, atomic::AtomicU64};
	use std::time::Duration;

	use crate::live_frame_stream_macos::frame_store::SharedLatestFrame;
	use crate::live_frame_stream_macos::stream_config::StreamCaptureTarget;
	use crate::live_frame_stream_macos::stream_filter::StreamFilterConfig;
	use crate::state::{GlobalPoint, MonitorRect};

	#[test]
	fn queued_refresh_request_stays_pending_while_waiting_for_previous_first_frame() {
		let shared = Arc::new(SharedLatestFrame::default());
		let now = std::time::Instant::now();
		let monitor = MonitorRect {
			id: 7,
			origin: GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};
		let mut state = None;
		let mut last_setup_attempt_at = None;

		assert!(shared.begin_refresh_monitor(monitor.id, 11, now));

		shared.mark_waiting_for_frame_until(monitor.id, now + Duration::from_secs(1));

		assert!(super::handle_refresh_monitor_request(
			&mut state,
			&mut last_setup_attempt_at,
			monitor,
			&StreamFilterConfig::default(),
			StreamCaptureTarget::FullMonitor,
			None,
			Arc::new(AtomicU64::new(0)),
			shared.clone(),
		));
		assert!(state.is_none());
		assert!(shared.finish_refresh_monitor(monitor.id));
	}
}
