#![allow(clippy::wildcard_imports)]

use super::*;

#[cfg(target_os = "macos")]
#[test]
fn stale_latched_worker_input_fails_closed_without_appending_growth() {
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
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	assert_eq!(
		session
			.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 1, 5))
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(
		session
			.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 2, 5))
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);

	let height_after_second_append =
		session.scroll_capture.session.as_ref().unwrap().export_image().height();

	session.scroll_capture.input_direction = Some(ScrollDirection::Up);
	session.scroll_capture.input_direction_at =
		Some(Instant::now() - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50));
	session.scroll_capture.input_gesture_active = false;
	session.scroll_capture.last_external_scroll_input_seq = 7;
	session.scroll_capture.inflight_request_id = Some(41);
	session.scroll_capture.inflight_request_observation = Some(InflightScrollCaptureObservation {
		was_observable: true,
		external_input_seq: 7,
		input_direction: Some(ScrollDirection::Down),
	});

	session.handle_captured_scroll_region(
		monitor,
		capture_rect,
		41,
		make_scroll_capture_window(&document, 3, 1, 5),
	);

	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert_eq!(session.scroll_capture.inflight_request_observation, None);

	let scroll_session_debug = format!("{:?}", session.scroll_capture.session.as_ref().unwrap());

	assert!(scroll_session_debug.contains("resume_frontier_top_y: None"), "{scroll_session_debug}");
	assert!(scroll_session_debug.contains("observed_viewport_top_y: 2"), "{scroll_session_debug}");
	assert_eq!(
		session.scroll_capture.session.as_ref().unwrap().export_image().height(),
		height_after_second_append
	);
}

#[cfg(target_os = "macos")]
#[test]
fn newer_same_direction_input_keeps_latched_worker_observation_context() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let base = make_sparse_worker_capture_window(512, 640, 0);
	let next = make_sparse_worker_capture_window(512, 640, 90);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = false;

	let height_before_worker_frame =
		session.scroll_capture.session.as_ref().unwrap().export_image().height();

	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.last_external_scroll_input_seq = 8;
	session.scroll_capture.inflight_request_id = Some(41);
	session.scroll_capture.inflight_request_observation = Some(InflightScrollCaptureObservation {
		was_observable: true,
		external_input_seq: 7,
		input_direction: Some(ScrollDirection::Down),
	});

	session.handle_captured_scroll_region(monitor, capture_rect, 41, next);

	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert_eq!(session.scroll_capture.inflight_request_observation, None);
	assert_eq!(
		session.scroll_capture.session.as_ref().unwrap().export_image().height(),
		height_before_worker_frame + 90
	);
	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 90);
}

#[cfg(target_os = "macos")]
#[test]
fn stale_same_direction_worker_frame_keeps_latched_worker_observation_context() {
	let monitor = test_monitor();
	let capture_rect = RectPoints::new(100, 120, 512, 640);
	let base = make_sparse_worker_capture_window(512, 640, 0);
	let next = make_sparse_worker_capture_window(512, 640, 90);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at =
		Some(Instant::now() - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50));
	session.scroll_capture.input_gesture_active = false;
	session.scroll_capture.last_external_scroll_input_seq = 8;
	session.scroll_capture.inflight_request_id = Some(41);
	session.scroll_capture.inflight_request_observation = Some(InflightScrollCaptureObservation {
		was_observable: true,
		external_input_seq: 7,
		input_direction: Some(ScrollDirection::Down),
	});

	session.handle_captured_scroll_region(monitor, capture_rect, 41, next);

	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert_eq!(session.scroll_capture.inflight_request_observation, None);
	assert_eq!(session.scroll_capture.session.as_ref().unwrap().export_image().height(), 730);
	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 90);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_frame_without_fresh_or_latched_input_fails_closed_without_appending_growth() {
	let monitor = test_monitor();
	let capture_rect = RectPoints::new(100, 120, 512, 640);
	let base = make_sparse_worker_capture_window(512, 640, 0);
	let next = make_sparse_worker_capture_window(512, 640, 90);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());
	session.scroll_capture.inflight_request_id = Some(41);
	session.scroll_capture.inflight_request_observation = Some(InflightScrollCaptureObservation {
		was_observable: false,
		external_input_seq: 7,
		input_direction: Some(ScrollDirection::Down),
	});

	let export_height_before =
		session.scroll_capture.session.as_ref().unwrap().export_image().height();
	let viewport_top_before =
		session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y();

	session.handle_captured_scroll_region(monitor, capture_rect, 41, next);

	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert_eq!(session.scroll_capture.inflight_request_observation, None);
	assert_eq!(
		session.scroll_capture.session.as_ref().unwrap().export_image().height(),
		export_height_before
	);
	assert_eq!(
		session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(),
		viewport_top_before
	);
}

#[cfg(target_os = "macos")]
#[test]
fn newer_opposite_direction_supersedes_latched_worker_observation_context() {
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
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	assert_eq!(
		session
			.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 1, 5))
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(
		session
			.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 2, 5))
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);

	let height_after_second_append =
		session.scroll_capture.session.as_ref().unwrap().export_image().height();

	session.scroll_capture.input_direction = Some(ScrollDirection::Up);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.last_external_scroll_input_seq = 8;
	session.scroll_capture.inflight_request_id = Some(41);
	session.scroll_capture.inflight_request_observation = Some(InflightScrollCaptureObservation {
		was_observable: true,
		external_input_seq: 7,
		input_direction: Some(ScrollDirection::Down),
	});

	session.handle_captured_scroll_region(
		monitor,
		capture_rect,
		41,
		make_scroll_capture_window(&document, 3, 3, 5),
	);

	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert_eq!(session.scroll_capture.inflight_request_observation, None);
	assert_eq!(
		session.scroll_capture.session.as_ref().unwrap().export_image().height(),
		height_after_second_append
	);
	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 2);
}

#[cfg(target_os = "macos")]
#[test]
fn successive_same_direction_worker_frames_do_not_stall_after_newer_input() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session =
		Some(ScrollSession::new(make_sparse_worker_capture_window(512, 640, 0), 320).unwrap());

	for (step, start_row) in [90_u32, 180, 270].into_iter().enumerate() {
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;
		session.scroll_capture.last_external_scroll_input_seq = (step as u64) + 2;
		session.scroll_capture.inflight_request_id = Some(41 + step as u64);
		session.scroll_capture.inflight_request_observation =
			Some(InflightScrollCaptureObservation {
				was_observable: true,
				external_input_seq: (step as u64) + 1,
				input_direction: Some(ScrollDirection::Down),
			});

		session.handle_captured_scroll_region(
			monitor,
			capture_rect,
			41 + step as u64,
			make_sparse_worker_capture_window(512, 640, start_row),
		);

		assert_eq!(session.scroll_capture.inflight_request_id, None);
		assert_eq!(session.scroll_capture.inflight_request_observation, None);
		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(),
			start_row as i32
		);
		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().export_image().height(),
			640 + start_row
		);
	}
}

#[cfg(target_os = "macos")]
#[test]
fn successive_browser_like_worker_frames_do_not_stall_after_newer_input() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session = Some(
		ScrollSession::new(make_browser_like_worker_capture_window(512, 640, 0), 320).unwrap(),
	);

	for (step, start_row) in [84_u32, 168, 252].into_iter().enumerate() {
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);
		session.scroll_capture.input_direction_at = Some(Instant::now());
		session.scroll_capture.input_gesture_active = true;
		session.scroll_capture.last_external_scroll_input_seq = (step as u64) + 12;
		session.scroll_capture.inflight_request_id = Some(81 + step as u64);
		session.scroll_capture.inflight_request_observation =
			Some(InflightScrollCaptureObservation {
				was_observable: true,
				external_input_seq: (step as u64) + 11,
				input_direction: Some(ScrollDirection::Down),
			});

		session.handle_captured_scroll_region(
			monitor,
			capture_rect,
			81 + step as u64,
			make_browser_like_worker_capture_window(512, 640, start_row),
		);

		assert_eq!(session.scroll_capture.inflight_request_id, None);
		assert_eq!(session.scroll_capture.inflight_request_observation, None);
		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(),
			start_row as i32
		);
		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().export_image().height(),
			640 + start_row
		);
	}
}

#[cfg(target_os = "macos")]
#[test]
fn missing_worker_scroll_frame_clears_inflight_without_mutating_session() {
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
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.last_external_scroll_input_seq = 11;
	session.scroll_capture.inflight_request_id = Some(41);
	session.scroll_capture.inflight_request_observation = Some(InflightScrollCaptureObservation {
		was_observable: true,
		external_input_seq: 11,
		input_direction: Some(ScrollDirection::Down),
	});

	let scroll_session_before = format!("{:?}", session.scroll_capture.session.as_ref().unwrap());

	session.handle_missing_scroll_region(monitor, capture_rect, 41);

	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert_eq!(session.scroll_capture.inflight_request_observation, None);
	assert_eq!(
		format!("{:?}", session.scroll_capture.session.as_ref().unwrap()),
		scroll_session_before
	);
}
