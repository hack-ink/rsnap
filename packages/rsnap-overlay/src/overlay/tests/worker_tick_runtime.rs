#[cfg(target_os = "macos")]
use crate::overlay::tests::{
	self, Arc, GlobalPoint, Instant, MacLiveFrameStream, MonitorRect, OverlaySession,
	OverlayWorker, RectPoints, ScrollDirection, ScrollObserveOutcome, ScrollSession,
	SequenceScrollCaptureBackend,
};
use crate::overlay::tests::{Duration, SCROLL_CAPTURE_SAMPLE_INTERVAL};

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_stays_on_stream_path_without_worker_fallback() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));
	session.scroll_capture.live_stream = Some(MacLiveFrameStream::new());

	session.maybe_tick_scroll_capture();

	assert!(!session.scroll_capture.paused);
	assert!(session.state.error_message.is_none());
	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert!(matches!(
		session.scroll_capture.live_stream.as_ref().unwrap().debug_last_request_kind(),
		Some("ordered_rgba_regions_after_seq_nonblocking")
			| Some("refresh_monitor_nonblocking_if_stale")
			| Some("prime_monitor_nonblocking")
	));
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_drains_external_input_without_a_new_stream_frame() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let rect = RectPoints::new(100, 120, 200, 240);
	let tick_at = Instant::now();
	let event_at = tick_at - Duration::from_millis(1);
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

	session.maybe_tick_scroll_capture();

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(session.scroll_capture.input_gesture_active);
	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_does_not_synthesize_preview_growth_from_input_without_semantic_sample()
{
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
	let rect = RectPoints::new(100, 120, 200, 240);
	let tick_at = Instant::now();
	let event_at = tick_at - Duration::from_millis(1);
	let events = Arc::new([(1, event_at, 150.0, 160.0, -4.0, true, false)]);
	let base_frame = tests::make_scroll_capture_window(&document, 3, 0, 5);
	let latest_frame = tests::make_scroll_capture_window(&document, 3, 1, 5);
	let scroll_session = ScrollSession::new(base_frame.clone(), 320).unwrap();
	let committed_preview = scroll_session.preview_image().clone();
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.live_stream = Some(MacLiveFrameStream::new());
	session.scroll_capture.session = Some(scroll_session);
	session.scroll_capture.preview_committed_image = Some(committed_preview.clone());
	session.scroll_capture.preview_display_image = Some(committed_preview.clone());
	session.scroll_capture.preview_latest_frame = Some(latest_frame);
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

	session.maybe_tick_scroll_capture();

	assert_eq!(session.scroll_capture.preview_display_image.as_ref(), Some(&committed_preview));
	assert_eq!(tests::scroll_capture_export_height(&session), base_frame.height());
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_does_not_double_count_preview_growth_from_same_latest_frame() {
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
	let tick_at = Instant::now();
	let event_at = tick_at - Duration::from_millis(1);
	let events = Arc::new([(1, event_at, 150.0, 160.0, -4.0, true, false)]);
	let base_frame = tests::make_scroll_capture_window(&document, 3, 0, 5);
	let moved_frame = tests::make_scroll_capture_window(&document, 3, 1, 5);
	let mut session = OverlaySession::new();
	let mut scroll_session = ScrollSession::new(base_frame, 320).unwrap();

	assert!(matches!(
		scroll_session.observe_downward_sample(moved_frame.clone()).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	));

	let committed_preview = scroll_session.preview_image().clone();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.live_stream = Some(MacLiveFrameStream::new());
	session.scroll_capture.session = Some(scroll_session);
	session.scroll_capture.preview_committed_image = Some(committed_preview.clone());
	session.scroll_capture.preview_display_image = Some(committed_preview.clone());
	session.scroll_capture.preview_latest_frame = Some(moved_frame);
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

	session.maybe_tick_scroll_capture();

	assert_eq!(session.scroll_capture.preview_display_image.as_ref(), Some(&committed_preview));
	assert_eq!(tests::scroll_capture_export_height(&session), committed_preview.height());
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_worker_path_recovers_after_blocked_overshot_frame() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let rect = RectPoints::new(100, 120, 512, 640);
	let base = tests::make_browser_like_worker_capture_window(512, 640, 0);
	let blocked = tests::make_browser_like_worker_capture_window(512, 640, 760);
	let followup = tests::make_browser_like_worker_capture_window(512, 640, 844);
	let mut session = OverlaySession::new();

	session.worker = Some(OverlayWorker::new(
		Box::new(SequenceScrollCaptureBackend::new([Some(blocked), Some(followup)])),
		None,
	));
	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());

	tests::enable_test_worker_scroll_capture_path(&mut session);
	tests::set_scroll_capture_input(&mut session, ScrollDirection::Down);

	session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_some());

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(tests::scroll_capture_export_height(&session), 640);
	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 0);

	tests::set_scroll_capture_input(&mut session, ScrollDirection::Down);

	session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_some());

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(tests::scroll_capture_export_height(&session), 724);
	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 84);
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_worker_path_retries_immediately_after_blocked_overshot_frame_during_fresh_downward_input()
 {
	let monitor = tests::test_monitor();
	let rect = RectPoints::new(100, 120, 512, 640);
	let base = tests::make_browser_like_worker_capture_window(512, 640, 0);
	let blocked = tests::make_browser_like_worker_capture_window(512, 640, 760);
	let followup = tests::make_browser_like_worker_capture_window(512, 640, 844);
	let mut session = OverlaySession::new();

	session.worker = Some(OverlayWorker::new(
		Box::new(SequenceScrollCaptureBackend::new([Some(blocked), Some(followup)])),
		None,
	));
	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());

	tests::enable_test_worker_scroll_capture_path(&mut session);
	tests::set_scroll_capture_input(&mut session, ScrollDirection::Down);

	session.scroll_capture.last_external_scroll_input_seq = 1;
	session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_some());

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(tests::scroll_capture_export_height(&session), 640);

	session.scroll_capture.last_external_scroll_input_seq = 2;
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	session.maybe_tick_scroll_capture();

	assert!(
		session.scroll_capture.inflight_request_id.is_some(),
		"fresh downward input after a blocked worker frame should retry immediately"
	);

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 84);
	assert_eq!(tests::scroll_capture_export_height(&session), 724);
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_worker_path_recovers_across_interleaved_no_frame_and_blocked_browser_steps()
 {
	let monitor = tests::test_monitor();
	let rect = RectPoints::new(100, 120, 512, 640);
	let mut session = OverlaySession::new();

	session.worker = Some(OverlayWorker::new(
		Box::new(SequenceScrollCaptureBackend::new([
			None,
			Some(tests::make_browser_like_worker_capture_window(512, 640, 84)),
			Some(tests::make_browser_like_worker_capture_window(512, 640, 700)),
			Some(tests::make_browser_like_worker_capture_window(512, 640, 784)),
			None,
			Some(tests::make_browser_like_worker_capture_window(512, 640, 868)),
		])),
		None,
	));
	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(
		ScrollSession::new(tests::make_browser_like_worker_capture_window(512, 640, 0), 320)
			.unwrap(),
	);

	tests::enable_test_worker_scroll_capture_path(&mut session);

	for expected_top_y in [84_i32, 168, 252] {
		let mut attempts = 0_u8;

		while session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y()
			< expected_top_y
		{
			attempts = attempts.saturating_add(1);

			assert!(
				attempts <= 4,
				"worker path failed to recover to expected_top_y={expected_top_y}"
			);

			tests::set_scroll_capture_input(&mut session, ScrollDirection::Down);

			session.scroll_capture.last_external_scroll_input_seq =
				session.scroll_capture.last_external_scroll_input_seq.saturating_add(1);
			session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

			session.maybe_tick_scroll_capture();

			assert!(session.scroll_capture.inflight_request_id.is_some());

			session.scroll_capture.last_external_scroll_input_seq =
				session.scroll_capture.last_external_scroll_input_seq.saturating_add(1);
			session.scroll_capture.input_direction = Some(ScrollDirection::Down);
			session.scroll_capture.input_direction_at = Some(Instant::now());
			session.scroll_capture.input_gesture_active = true;

			tests::drain_scroll_capture_worker_until_idle(&mut session);
		}

		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(),
			expected_top_y
		);
		assert_eq!(tests::scroll_capture_export_height(&session), 640 + expected_top_y as u32);
	}
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_worker_path_keeps_same_direction_superseded_response() {
	let monitor = tests::test_monitor();
	let rect = RectPoints::new(100, 120, 512, 640);
	let base = tests::make_sparse_worker_capture_window(512, 640, 0);
	let moved = tests::make_sparse_worker_capture_window(512, 640, 180);
	let mut session = OverlaySession::new();

	session.worker =
		Some(OverlayWorker::new(Box::new(SequenceScrollCaptureBackend::new([Some(moved)])), None));
	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());

	tests::enable_test_worker_scroll_capture_path(&mut session);
	tests::set_scroll_capture_input(&mut session, ScrollDirection::Down);

	session.scroll_capture.last_external_scroll_input_seq = 1;
	session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_some());

	session.scroll_capture.last_external_scroll_input_seq = 2;
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(tests::scroll_capture_export_height(&session), 820);
	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 180);
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_worker_path_commits_successive_browser_like_frames_after_newer_same_direction_input()
 {
	let monitor = tests::test_monitor();
	let rect = RectPoints::new(100, 120, 512, 640);
	let mut session = OverlaySession::new();

	session.worker = Some(OverlayWorker::new(
		Box::new(SequenceScrollCaptureBackend::new([
			Some(tests::make_browser_like_worker_capture_window(512, 640, 84)),
			Some(tests::make_browser_like_worker_capture_window(512, 640, 168)),
			Some(tests::make_browser_like_worker_capture_window(512, 640, 252)),
		])),
		None,
	));
	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(
		ScrollSession::new(tests::make_browser_like_worker_capture_window(512, 640, 0), 320)
			.unwrap(),
	);

	tests::enable_test_worker_scroll_capture_path(&mut session);

	for (step, expected_top_y) in [84_i32, 168, 252].into_iter().enumerate() {
		tests::set_scroll_capture_input(&mut session, ScrollDirection::Down);

		session.scroll_capture.last_external_scroll_input_seq = (step as u64) + 1;
		session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

		session.maybe_tick_scroll_capture();

		assert!(session.scroll_capture.inflight_request_id.is_some());

		session.scroll_capture.last_external_scroll_input_seq = (step as u64) + 2;
		session.scroll_capture.input_direction = Some(ScrollDirection::Down);

		tests::drain_scroll_capture_worker_until_idle(&mut session);

		assert_eq!(session.scroll_capture.inflight_request_id, None);
		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(),
			expected_top_y
		);
		assert_eq!(
			session.scroll_capture.session.as_ref().unwrap().export_image().height(),
			640 + expected_top_y as u32
		);
	}
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_worker_path_drops_opposite_direction_superseded_response() {
	let monitor = tests::test_monitor();
	let rect = RectPoints::new(100, 120, 512, 640);
	let base = tests::make_sparse_worker_capture_window(512, 640, 0);
	let moved = tests::make_sparse_worker_capture_window(512, 640, 180);
	let mut session = OverlaySession::new();

	session.worker =
		Some(OverlayWorker::new(Box::new(SequenceScrollCaptureBackend::new([Some(moved)])), None));
	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());

	tests::enable_test_worker_scroll_capture_path(&mut session);
	tests::set_scroll_capture_input(&mut session, ScrollDirection::Down);

	session.scroll_capture.last_external_scroll_input_seq = 1;
	session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_some());

	session.scroll_capture.last_external_scroll_input_seq = 2;
	session.scroll_capture.input_direction = Some(ScrollDirection::Up);

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(tests::scroll_capture_export_height(&session), 640);
	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_worker_path_retries_immediately_after_no_new_frame_during_fresh_downward_input()
 {
	let monitor = tests::test_monitor();
	let rect = RectPoints::new(100, 120, 512, 640);
	let base = tests::make_browser_like_worker_capture_window(512, 640, 0);
	let moved = tests::make_browser_like_worker_capture_window(512, 640, 84);
	let mut session = OverlaySession::new();

	session.worker = Some(OverlayWorker::new(
		Box::new(SequenceScrollCaptureBackend::new([None, Some(moved)])),
		None,
	));
	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());

	tests::enable_test_worker_scroll_capture_path(&mut session);
	tests::set_scroll_capture_input(&mut session, ScrollDirection::Down);

	session.scroll_capture.last_external_scroll_input_seq = 1;
	session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_some());

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert_eq!(tests::scroll_capture_export_height(&session), 640);

	session.scroll_capture.last_external_scroll_input_seq = 2;
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	session.maybe_tick_scroll_capture();

	assert!(
		session.scroll_capture.inflight_request_id.is_some(),
		"fresh downward input after a worker no-frame response should retry immediately"
	);

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 84);
	assert_eq!(tests::scroll_capture_export_height(&session), 724);
}

#[test]
fn scroll_capture_sample_interval_matches_platform_worker_sampling_strategy() {
	#[cfg(target_os = "macos")]
	assert_eq!(SCROLL_CAPTURE_SAMPLE_INTERVAL, Duration::from_millis(250));
	#[cfg(not(target_os = "macos"))]
	assert_eq!(SCROLL_CAPTURE_SAMPLE_INTERVAL, Duration::from_millis(50));
}

#[cfg(target_os = "macos")]
#[test]
fn maybe_tick_scroll_capture_worker_path_backs_off_after_duplicate_committed_frame() {
	let monitor = tests::test_monitor();
	let rect = RectPoints::new(100, 120, 512, 640);
	let base = tests::make_browser_like_worker_capture_window(512, 640, 0);
	let step_one = tests::make_browser_like_worker_capture_window(512, 640, 84);
	let step_two = tests::make_browser_like_worker_capture_window(512, 640, 168);
	let mut session = OverlaySession::new();

	session.worker = Some(OverlayWorker::new(
		Box::new(SequenceScrollCaptureBackend::new([
			Some(step_one.clone()),
			Some(step_one),
			Some(step_two),
		])),
		None,
	));
	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(rect);
	session.scroll_capture.session = Some(ScrollSession::new(base, 320).unwrap());

	tests::enable_test_worker_scroll_capture_path(&mut session);
	tests::set_scroll_capture_input(&mut session, ScrollDirection::Down);

	session.scroll_capture.last_external_scroll_input_seq = 1;
	session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_some());

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 84);
	assert_eq!(tests::scroll_capture_export_height(&session), 724);

	session.scroll_capture.last_external_scroll_input_seq = 2;
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_some());

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 84);
	assert_eq!(tests::scroll_capture_export_height(&session), 724);

	session.maybe_tick_scroll_capture();

	assert!(
		session.scroll_capture.inflight_request_id.is_none(),
		"duplicate committed worker frame should back off instead of immediately re-requesting"
	);

	session.scroll_capture.last_external_scroll_input_seq = 3;
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.next_sample_at = Some(Instant::now() - Duration::from_millis(1));

	session.maybe_tick_scroll_capture();

	assert!(session.scroll_capture.inflight_request_id.is_some());

	tests::drain_scroll_capture_worker_until_idle(&mut session);

	assert_eq!(session.scroll_capture.session.as_ref().unwrap().current_viewport_top_y(), 168);
	assert_eq!(tests::scroll_capture_export_height(&session), 808);
}
