#[cfg(target_os = "macos")]
#[allow(unused_imports)]
use crate::overlay::tests::{
	self, Arc, MacLiveFrameStream, OverlayControl,
	SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW, SCROLL_CAPTURE_INPUT_FRESHNESS,
	ScrollCaptureLiveFrame,
};
#[cfg(target_os = "macos")]
#[allow(unused_imports)]
use crate::overlay::tests::{
	Duration, GlobalPoint, Instant, MonitorRect, OverlaySession, RectPoints, ScrollDirection,
	ScrollSession, overlay,
};

#[cfg(target_os = "macos")]
#[test]
fn handle_scroll_input_ready_drains_input_and_polls_stream_fallback() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let rect = RectPoints::new(100, 120, 200, 240);
	let handled_at = Instant::now();
	let event_at = handled_at - Duration::from_millis(1);
	let events = Arc::new([(1, event_at, 150.0, 160.0, -4.0, true, false)]);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.live_stream = Some(MacLiveFrameStream::new());
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

	assert!(matches!(session.handle_scroll_input_ready(), OverlayControl::Continue));
	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(session.scroll_capture.input_gesture_active);
	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);
	assert!(matches!(
		session.scroll_capture.live_stream.as_ref().unwrap().debug_last_request_kind(),
		Some("ordered_rgba_regions_after_seq_nonblocking")
			| Some("refresh_monitor_nonblocking_if_stale")
	));
}

#[cfg(target_os = "macos")]
#[test]
fn drain_external_scroll_input_worker_path_does_not_arm_live_stream_stale_grace() {
	let monitor = tests::test_monitor();
	let rect = RectPoints::new(100, 120, 512, 640);
	let through = Instant::now();
	let recorded_at = through - Duration::from_millis(1);
	let events = Arc::new([(1, recorded_at, 150.0, 160.0, -4.0, false, false)]);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.live_stream = Some(MacLiveFrameStream::new());

	tests::enable_test_worker_scroll_capture_path(&mut session);

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

	session.drain_external_scroll_input_events_through(through);

	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);
	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(!session.scroll_capture.input_gesture_active);
	assert_eq!(session.scroll_capture.live_stream_stale_grace, None);
}

#[cfg(target_os = "macos")]
#[test]
fn force_stream_refresh_stays_disabled_while_downward_gesture_is_still_active() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(now);
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.downward_motion_rows_pending = 512.0;

	assert!(!session.scroll_capture_should_force_stream_refresh_at(now));
}

#[cfg(target_os = "macos")]
#[test]
fn stale_stream_refresh_stays_disabled_while_gesture_is_still_active() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.last_stream_event_at = Some(
		now - SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW + Duration::from_millis(1),
	);

	assert!(!session.scroll_capture_should_schedule_stale_stream_refresh_at(now));
}

#[cfg(target_os = "macos")]
#[test]
fn stale_stream_refresh_reenables_after_gesture_ends() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.input_gesture_active = false;

	assert!(session.scroll_capture_should_schedule_stale_stream_refresh_at(now));
}

#[cfg(target_os = "macos")]
#[test]
fn stale_stream_refresh_reenables_during_gesture_after_stream_goes_dead() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.last_stream_event_at = Some(
		now - SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW - Duration::from_millis(1),
	);

	assert!(session.scroll_capture_should_schedule_stale_stream_refresh_at(now));
}

#[cfg(target_os = "macos")]
#[test]
fn post_stall_burst_search_stays_enabled_during_active_gesture_when_downward_backlog_is_fresh() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.pending_post_stall_burst_after_seq = Some(80);
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(now);
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.downward_motion_rows_pending = 512.0;

	assert!(session.scroll_capture_should_allow_post_stall_burst_search_at(81, now));
}

#[cfg(target_os = "macos")]
#[test]
fn force_stream_refresh_stays_enabled_for_fresh_pending_downward_motion_after_gesture_end() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at =
		Some(now - SCROLL_CAPTURE_INPUT_FRESHNESS + Duration::from_millis(50));
	session.scroll_capture.input_gesture_active = false;
	session.scroll_capture.downward_motion_rows_pending = 512.0;

	assert!(session.scroll_capture_should_force_stream_refresh_at(now));
}

#[cfg(target_os = "macos")]
#[test]
fn force_stream_refresh_stops_after_downward_input_becomes_stale() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at =
		Some(now - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(1));
	session.scroll_capture.input_gesture_active = false;
	session.scroll_capture.downward_motion_rows_pending = 512.0;

	assert!(!session.scroll_capture_should_force_stream_refresh_at(now));
}

#[cfg(target_os = "macos")]
#[test]
fn post_stall_burst_search_stays_enabled_while_fresh_downward_backlog_remains() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.pending_post_stall_burst_after_seq = Some(80);
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(now);
	session.scroll_capture.input_gesture_active = false;
	session.scroll_capture.downward_motion_rows_pending = 512.0;

	assert!(session.scroll_capture_should_allow_post_stall_burst_search_at(81, now));
	assert!(session.scroll_capture_should_allow_post_stall_burst_search_at(
		82,
		now + Duration::from_millis(50)
	));
}

#[cfg(target_os = "macos")]
#[test]
fn post_stall_burst_search_arms_for_large_capture_time_gap_even_when_frame_seq_is_contiguous() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(now);
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.downward_motion_rows_pending = 512.0;
	session.scroll_capture.last_consumed_stream_frame_captured_at = Some(
		now - SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW - Duration::from_millis(1),
	);

	assert!(session.scroll_capture_should_arm_post_stall_burst_for_time_gap_at(now));
}

#[cfg(target_os = "macos")]
#[test]
fn consuming_live_frame_backlog_arms_time_gap_burst_after_draining_fresh_input() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
	];
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let rect = RectPoints::new(100, 120, 200, 240);
	let captured_at = Instant::now();
	let event_at = captured_at - Duration::from_millis(1);
	let events = Arc::new([(1, event_at, 150.0, 160.0, -74.0, true, false)]);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(
		ScrollSession::new(tests::make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap(),
	);
	session.scroll_capture.last_consumed_stream_frame_captured_at = Some(
		captured_at
			- SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW
			- Duration::from_millis(1),
	);
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

	session.test_push_scroll_capture_live_frame(ScrollCaptureLiveFrame {
		frame_seq: 9,
		captured_at,
		image: tests::make_scroll_capture_window(&document, 3, 0, 5),
	});
	session.test_consume_scroll_capture_backlog(1);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);
	assert_eq!(session.scroll_capture.pending_post_stall_burst_after_seq, Some(8));
}

#[cfg(target_os = "macos")]
#[test]
fn post_stall_burst_search_does_not_arm_for_small_capture_time_gap() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(now);
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.downward_motion_rows_pending = 512.0;
	session.scroll_capture.last_consumed_stream_frame_captured_at = Some(
		now - SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW + Duration::from_millis(10),
	);

	assert!(!session.scroll_capture_should_arm_post_stall_burst_for_time_gap_at(now));
}

#[cfg(target_os = "macos")]
#[test]
fn post_stall_burst_search_stops_after_downward_backlog_goes_stale() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.pending_post_stall_burst_after_seq = Some(80);
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at =
		Some(now - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(1));
	session.scroll_capture.input_gesture_active = false;
	session.scroll_capture.downward_motion_rows_pending = 512.0;

	assert!(!session.scroll_capture_should_allow_post_stall_burst_search_at(81, now));
}
