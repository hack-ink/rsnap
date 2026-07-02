#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::Mutex;
#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicUsize;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "macos")]
use std::time::Duration;
use std::time::Instant;

#[cfg(target_os = "macos")]
use color_eyre::eyre;
#[cfg(target_os = "macos")]
use image::{Rgba, RgbaImage};

#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::MacLiveFrameStream;
#[cfg(target_os = "macos")]
use crate::overlay::scroll_input_runtime::KCG_SCROLL_EVENT_UNIT_PIXEL;
#[cfg(target_os = "macos")]
use crate::overlay::session_state::ScrollCaptureLiveFrame;
use crate::overlay::tests::{self, GlobalPoint, MonitorRect, OverlaySession, RectPoints};
#[cfg(not(target_os = "macos"))]
use crate::overlay::tests::{FrozenCaptureSource, OverlayMode};
#[cfg(target_os = "macos")]
use crate::overlay::tests::{
	FrozenToolbarTool, LiveStreamStaleGrace, MacOSScrollPixelResidual, MouseScrollDelta,
	OverlayControl, PhysicalPosition, SCROLL_CAPTURE_INPUT_FRESHNESS,
	SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES, ScrollCaptureFrameSource,
	ScrollCaptureHostAdapter, ScrollCaptureHostFrameRequestError,
};
use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome, ScrollSession};

#[cfg(target_os = "macos")]
#[test]
fn duplicate_live_frames_schedule_forced_refresh_when_downward_backlog_is_fresh() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let observed_at = Instant::now();
	let frame = ScrollCaptureLiveFrame {
		frame_seq: 7,
		captured_at: observed_at,
		image: RgbaImage::from_pixel(16, 16, Rgba([7, 8, 9, 255])),
	};
	let mut session = OverlaySession::new();

	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.live_stream = Some(MacLiveFrameStream::new());
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(observed_at);
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.downward_motion_rows_pending = 512.0;

	assert!(session.note_scroll_capture_live_stream_frame_activity(&frame));
	assert!(!session.note_scroll_capture_live_stream_frame_activity(&frame));
	assert!(!session.note_scroll_capture_live_stream_frame_activity(&frame));
	assert!(!session.note_scroll_capture_live_stream_frame_activity(&frame));
	assert_eq!(session.scroll_capture.consecutive_identical_stream_frames, 3);

	session.maybe_schedule_duplicate_stream_refresh(frame.frame_seq, observed_at);

	assert!(matches!(
		session
			.scroll_capture
			.live_stream
			.as_ref()
			.and_then(MacLiveFrameStream::debug_last_request_kind),
		Some("refresh_monitor_nonblocking_if_stale") | Some("prime_monitor_nonblocking")
	));
	assert_eq!(session.scroll_capture.pending_post_stall_burst_after_seq, Some(frame.frame_seq));
	assert_eq!(session.scroll_capture.last_duplicate_stream_refresh_at, Some(observed_at));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn scroll_capture_is_unavailable_on_non_macos_even_with_drag_selection() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 200, 240));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(!session.scroll_capture_is_available());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_guard_error_keeps_frozen_capture_available() {
	let mut session = OverlaySession::new();

	tests::seed_ready_scroll_capture_selection(&mut session);

	session.set_scroll_capture_start_guard(Arc::new(|| {
		Err(eyre::eyre!("Open System Settings and retry."))
	}));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.state.frozen_display_image.is_some());
	assert!(
		session
			.state
			.error_message
			.as_deref()
			.is_some_and(|message| message.contains("Open System Settings and retry."))
	);
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_guard_silent_reject_keeps_frozen_capture_available_without_error() {
	let mut session = OverlaySession::new();

	tests::seed_ready_scroll_capture_selection(&mut session);

	session.set_scroll_capture_start_guard(Arc::new(|| Ok(false)));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.state.frozen_display_image.is_some());
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_host_adapter_silent_reject_keeps_frozen_capture_available_without_error() {
	let mut session = OverlaySession::new();

	tests::seed_ready_scroll_capture_selection(&mut session);

	session.set_scroll_capture_host_adapter(ScrollCaptureHostAdapter::new(
		Arc::new(|_| Ok(false)),
		Arc::new(|| {}),
		Arc::new(|_, _, _| Ok(())),
		Arc::new(|_, _| Vec::new()),
	));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(!session.scroll_capture.active);
	assert!(session.state.frozen_display_image.is_some());
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_starting_hook_error_keeps_frozen_capture_available() {
	let mut session = OverlaySession::new();

	tests::seed_ready_scroll_capture_selection(&mut session);

	session.set_scroll_capture_start_guard(Arc::new(|| Ok(true)));
	session
		.set_scroll_capture_starting_hook(Arc::new(|| Err(eyre::eyre!("Observer was not ready."))));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.state.frozen_display_image.is_some());
	assert!(
		session
			.state
			.error_message
			.as_deref()
			.is_some_and(|message| message.contains("Observer was not ready."))
	);
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_host_busy_request_backs_off_without_error() {
	let mut session = OverlaySession::new();

	tests::seed_ready_scroll_capture_selection(&mut session);
	tests::enable_test_worker_scroll_capture_path(&mut session);

	session.set_scroll_capture_host_adapter(ScrollCaptureHostAdapter::new(
		Arc::new(|_| Ok(true)),
		Arc::new(|| {}),
		Arc::new(|_, _, _| Err(ScrollCaptureHostFrameRequestError::Busy)),
		Arc::new(|_, _| Vec::new()),
	));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.scroll_capture.active);

	tests::enable_test_worker_scroll_capture_path(&mut session);

	let before_retry = Instant::now();

	session.scroll_capture.next_sample_at = Some(before_retry - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_none());
	assert!(session.state.error_message.is_none());
	assert!(session.scroll_capture.next_sample_at.is_some_and(|next| next > before_retry));
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_preflight_runs_before_permission_guard() {
	let guard_calls = Arc::new(AtomicUsize::new(0));
	let mut session = OverlaySession::new();

	session.set_scroll_capture_start_guard(Arc::new({
		let guard_calls = Arc::clone(&guard_calls);

		move || {
			guard_calls.fetch_add(1, Ordering::SeqCst);

			Ok(true)
		}
	}));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert_eq!(guard_calls.load(Ordering::SeqCst), 0);
	assert_eq!(
		session.state.error_message.as_deref(),
		Some("Scroll capture requires a dragged region selection.")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_starting_hook_runs_before_started_hook() {
	let hook_order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
	let mut session = OverlaySession::new();

	tests::seed_ready_scroll_capture_selection(&mut session);

	session.set_scroll_capture_start_guard(Arc::new(|| Ok(true)));

	session.set_scroll_capture_starting_hook(Arc::new({
		let hook_order = Arc::clone(&hook_order);

		move || {
			let mut hook_order = match hook_order.lock() {
				Ok(hook_order) => hook_order,
				Err(poisoned) => poisoned.into_inner(),
			};

			hook_order.push("starting");

			Ok(())
		}
	}));
	session.set_scroll_capture_started_hook(Arc::new({
		let hook_order = Arc::clone(&hook_order);

		move || {
			let mut hook_order = match hook_order.lock() {
				Ok(hook_order) => hook_order,
				Err(poisoned) => poisoned.into_inner(),
			};

			hook_order.push("started");
		}
	}));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.scroll_capture.active);

	let hook_order = match hook_order.lock() {
		Ok(hook_order) => hook_order,
		Err(poisoned) => poisoned.into_inner(),
	};

	assert_eq!(*hook_order, vec!["starting", "started"]);
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_start_preserves_existing_live_sample_stream() {
	let mut session = OverlaySession::new();

	tests::seed_ready_scroll_capture_selection(&mut session);

	session.live_sample_stream = Some(MacLiveFrameStream::new());

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.scroll_capture.active);
	assert!(session.live_sample_stream.is_some());
	assert!(session.scroll_capture.live_stream.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_start_skips_scroll_live_stream_when_worker_sampling_is_forced() {
	let mut session = OverlaySession::new();

	tests::seed_ready_scroll_capture_selection(&mut session);
	tests::enable_test_worker_scroll_capture_path(&mut session);

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.scroll_capture.active);
	assert!(session.scroll_capture.live_stream.is_none());
	assert!(session.scroll_capture.live_stream_backlog.is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_start_disables_text_mode_while_active() {
	let mut session = OverlaySession::new();

	tests::seed_ready_scroll_capture_selection(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.scroll_capture.active);
	assert!(!session.frozen_text_tool_active());
}

#[cfg(target_os = "macos")]
#[test]
fn reset_for_start_preserves_external_scroll_input_drain_reader() {
	let mut session = OverlaySession::default();

	session.set_external_scroll_input_drain_reader(Arc::new(|_, _| {
		vec![(1, Instant::now(), 10.0, 20.0, 4.0, true, false)]
	}));
	session.reset_for_start();

	assert!(session.scroll_capture.external_scroll_input_drain_reader.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn drain_external_scroll_input_events_through_advances_last_seen_seq() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let start = Instant::now();
	let events = Arc::new([
		(1, start, 150.0, 160.0, -4.0, true, false),
		(2, start + Duration::from_millis(2), 150.0, 160.0, -4.0, false, true),
	]);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(start);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(session.scroll_capture.input_gesture_active);
	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);

	session.drain_external_scroll_input_events_through(start);

	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);

	session.drain_external_scroll_input_events_through(start + Duration::from_millis(2));

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(!session.scroll_capture.input_gesture_active);
	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 2);
}

#[cfg(target_os = "macos")]
#[test]
fn drain_external_scroll_input_events_through_uses_pairing_time_for_freshness() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let through = Instant::now();
	let recorded_at = through - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50);
	let events = Arc::new([(1, recorded_at, 150.0, 160.0, -4.0, false, false)]);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, paired_through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= paired_through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(through);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert_eq!(session.scroll_capture.input_direction_at, Some(through));
	assert_eq!(session.scroll_capture_observation_block_reason(), None);
}

#[cfg(target_os = "macos")]
#[test]
fn replayed_stream_input_uses_frame_time_for_stale_gate_without_global_relaxation() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
	];
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let through = Instant::now() - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50);
	let recorded_at = through - Duration::from_millis(12);
	let events = Arc::new([(1, recorded_at, 150.0, 160.0, -4.0, false, false)]);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session = Some(
		ScrollSession::new(tests::make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap(),
	);
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, paired_through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= paired_through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(through);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert_eq!(session.scroll_capture.input_direction_at, Some(through));
	assert_eq!(session.scroll_capture_observation_block_reason(), Some("stale_input"));
	assert_eq!(session.scroll_capture_observation_block_reason_at(through), None);
	assert_eq!(
		session
			.observe_scroll_capture_frame_at(
				tests::make_scroll_capture_window(&document, 3, 1, 5),
				through,
			)
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
}

#[cfg(target_os = "macos")]
#[test]
fn replayed_downward_input_allows_bounded_stale_live_stream_frame() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
	];
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let through = Instant::now();
	let events =
		Arc::new([(7, through - Duration::from_millis(10), 150.0, 160.0, 4.0, false, false)]);
	let stale_at = through + SCROLL_CAPTURE_INPUT_FRESHNESS + Duration::from_millis(1);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session = Some(
		ScrollSession::new(tests::make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap(),
	);
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, paired_through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= paired_through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(through);

	assert_eq!(
		session.scroll_capture.live_stream_stale_grace,
		Some(LiveStreamStaleGrace {
			external_input_seq: 7,
			remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
		})
	);
	assert_eq!(
		session
			.observe_scroll_capture_frame_at(
				tests::make_scroll_capture_window(&document, 3, 1, 5),
				stale_at,
			)
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(tests::scroll_capture_export_height(&session), 6);
}

#[cfg(target_os = "macos")]
#[test]
fn stale_live_stream_frame_is_observed_even_without_direction_freshness() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
	];
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let through = Instant::now();
	let wheel_at = through + Duration::from_millis(10);
	let events =
		Arc::new([(7, through - Duration::from_millis(10), 150.0, 160.0, 4.0, false, false)]);
	let stale_at = wheel_at + SCROLL_CAPTURE_INPUT_FRESHNESS + Duration::from_millis(1);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session = Some(
		ScrollSession::new(tests::make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap(),
	);
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, paired_through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= paired_through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(through);
	session.record_scroll_capture_input_direction_from_overlay_wheel_at(
		&MouseScrollDelta::LineDelta(0.0, -1.0),
		wheel_at,
	);

	assert_eq!(session.scroll_capture.input_direction_at, Some(wheel_at));
	assert_eq!(
		session.scroll_capture.live_stream_stale_grace,
		Some(LiveStreamStaleGrace {
			external_input_seq: 7,
			remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
		})
	);
	assert_eq!(
		session
			.observe_scroll_capture_frame_at(
				tests::make_scroll_capture_window(&document, 3, 1, 5),
				stale_at,
			)
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(tests::scroll_capture_export_height(&session), 6);
}

#[cfg(target_os = "macos")]
#[test]
fn handle_scroll_capture_frame_passes_allow_stale_input_into_live_stream_gate() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
	];
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let observed_at = Instant::now();
	let input_at = observed_at - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(1);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(input_at);
	session.scroll_capture.session = Some(
		ScrollSession::new(tests::make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap(),
	);

	assert_eq!(
		session
			.handle_scroll_capture_frame(
				tests::make_scroll_capture_window(&document, 3, 1, 5),
				ScrollCaptureFrameSource::LiveStream { frame_seq: 143 },
				true,
				observed_at,
			)
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(tests::scroll_capture_export_height(&session), 6);
}

#[cfg(target_os = "macos")]
#[test]
fn fresh_live_stream_frame_without_direction_metadata_fails_closed_as_no_change() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
	];
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let observed_at = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session = Some(
		ScrollSession::new(tests::make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap(),
	);

	session.handle_scroll_capture_frame(
		tests::make_scroll_capture_window(&document, 3, 1, 5),
		ScrollCaptureFrameSource::LiveStream { frame_seq: 143 },
		false,
		observed_at,
	);

	assert_eq!(tests::scroll_capture_export_height(&session), 5);
}

#[test]
fn downward_frame_motion_commits_even_with_legacy_upward_input_direction() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
	];
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.session = Some(
		ScrollSession::new(tests::make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap(),
	);
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	assert_eq!(
		session
			.observe_scroll_capture_frame(tests::make_scroll_capture_window(&document, 3, 1, 5))
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);

	let height_after_first_append =
		session.scroll_capture.session.as_ref().unwrap().export_image().height();

	session.scroll_capture.input_direction = Some(ScrollDirection::Up);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	assert_eq!(
		session
			.observe_scroll_capture_frame(tests::make_scroll_capture_window(&document, 3, 2, 5))
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(
		session.scroll_capture.session.as_ref().unwrap().export_image().height(),
		height_after_first_append + 1
	);
}

#[cfg(target_os = "macos")]
#[test]
fn pixel_delta_residuals_accumulate_until_whole_pixels_emit() {
	let mut residual = MacOSScrollPixelResidual::default();
	let first = OverlaySession::normalize_macos_scroll_wheel_delta(
		&MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.4, -0.4)),
		&mut residual,
	);
	let second = OverlaySession::normalize_macos_scroll_wheel_delta(
		&MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.7, -0.8)),
		&mut residual,
	);

	assert_eq!(first.units, KCG_SCROLL_EVENT_UNIT_PIXEL);
	assert_eq!(first.posted_x, 0);
	assert_eq!(first.posted_y, 0);
	assert!((first.residual.x - 0.4).abs() < f64::EPSILON);
	assert!((first.residual.y + 0.4).abs() < f64::EPSILON);
	assert_eq!(second.posted_x, 1);
	assert_eq!(second.posted_y, -1);
	assert!((second.residual.x - 0.1).abs() < 1e-9);
	assert!((second.residual.y + 0.2).abs() < 1e-9);
}
