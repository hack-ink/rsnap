#[cfg(target_os = "macos")]
use std::ptr;

#[cfg(target_os = "macos")]
use crate::overlay::RectPoints;
#[cfg(target_os = "macos")]
use crate::overlay::tests::WorkerRequestSendError;
#[cfg(target_os = "macos")]
use crate::overlay::tests::{
	self, Arc, InflightScrollCaptureObservation, ScrollCaptureLiveFrame, WindowListSnapshot,
	WindowRect,
};
#[cfg(target_os = "macos")]
use crate::overlay::tests::{
	GlobalPoint, Instant, OverlaySession, ScrollDirection, WorkerErrorSource, WorkerResponse,
};
#[cfg(target_os = "macos")]
use crate::overlay::worker_runtime::FREEZE_CAPTURE_SEND_FULL_RETRY_LIMIT;

#[cfg(target_os = "macos")]
#[test]
fn apply_self_capture_exception_window_ids_to_active_streams_updates_live_stream_filters() {
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.window_list_snapshot = Some(Arc::new(WindowListSnapshot {
		captured_at: Instant::now(),
		windows: Arc::new(vec![WindowRect {
			window_id: Some(9),
			x: 10,
			y: 12,
			width: 30,
			height: 40,
		}]),
	}));

	session.apply_self_capture_exception_window_ids_to_active_streams();

	assert_eq!(
		session.live_sample_stream.as_ref().unwrap().debug_self_capture_exception_window_ids(),
		&[17]
	);
	assert_eq!(
		session
			.scroll_capture
			.live_stream
			.as_ref()
			.unwrap()
			.debug_self_capture_exception_window_ids(),
		&[17]
	);
	assert_ne!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
	assert!(session.window_list_snapshot.is_none());
	assert!(
		session.last_window_list_refresh_request_at.elapsed()
			>= session.window_list_refresh_interval
	);
	assert_eq!(session.scroll_capture.last_stream_frame_seq, 0);
	assert_eq!(session.scroll_capture.live_stream_stale_grace, None);
}

#[cfg(target_os = "macos")]
#[test]
fn complete_startup_aux_window_creation_kicks_first_live_sample_before_refresh_is_needed() {
	let monitor = tests::test_monitor();
	let cursor = GlobalPoint::new(120, 180);
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.startup_aux_window_creation_pending = true;
	session.cursor_monitor = Some(monitor);
	session.state.cursor = Some(cursor);
	session.window_list_snapshot = Some(Arc::new(WindowListSnapshot {
		captured_at: Instant::now(),
		windows: Arc::new(vec![WindowRect {
			window_id: Some(9),
			x: 10,
			y: 12,
			width: 30,
			height: 40,
		}]),
	}));

	session.complete_startup_aux_window_creation(true);

	assert!(!session.startup_aux_window_creation_pending);
	assert_eq!(session.latest_live_cursor_sample_request_id, Some(1));
	assert_eq!(session.applied_live_cursor_sample_request_id, Some(1));
	assert_eq!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(session.window_list_snapshot.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn complete_startup_aux_window_creation_defers_live_stream_upgrade_until_aux_window_use() {
	let monitor = tests::test_monitor();
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();
	let original_live_sample_stream = ptr::from_ref(session.live_sample_stream.as_ref().unwrap());
	let original_scroll_live_stream =
		ptr::from_ref(session.scroll_capture.live_stream.as_ref().unwrap());

	session.startup_aux_window_creation_pending = true;
	session.cursor_monitor = Some(monitor);

	session.note_live_cursor_sample_request_started(7);
	session.finish_sync_live_cursor_sample_attempt(7);

	session.window_list_snapshot = Some(Arc::new(WindowListSnapshot {
		captured_at: Instant::now(),
		windows: Arc::new(vec![WindowRect {
			window_id: Some(9),
			x: 10,
			y: 12,
			width: 30,
			height: 40,
		}]),
	}));

	session.complete_startup_aux_window_creation(true);

	assert!(!session.startup_aux_window_creation_pending);
	assert!(session.pending_startup_aux_live_stream_filter_upgrade);
	assert_eq!(session.latest_live_cursor_sample_request_id, Some(7));
	assert_eq!(session.applied_live_cursor_sample_request_id, Some(7));
	assert_eq!(
		ptr::from_ref(session.live_sample_stream.as_ref().unwrap()),
		original_live_sample_stream
	);
	assert_eq!(
		ptr::from_ref(session.scroll_capture.live_stream.as_ref().unwrap()),
		original_scroll_live_stream
	);
	assert_eq!(session.live_sample_stream.as_ref().unwrap().debug_last_request_kind(), None);
	assert_eq!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(session.window_list_snapshot.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn refresh_startup_live_stream_after_window_creation_rebuilds_and_reprimes_stream() {
	let monitor = tests::test_monitor();
	let (mut session, _original_worker_debug_id) = tests::configured_session_with_macos_worker();

	assert!(
		session
			.live_sample_stream
			.as_ref()
			.unwrap()
			.debug_self_capture_exception_window_ids()
			.is_empty()
	);

	session.refresh_startup_live_stream_after_window_creation(Some(monitor));

	assert_eq!(
		session.live_sample_stream.as_ref().unwrap().debug_self_capture_exception_window_ids(),
		&[17]
	);
	assert_eq!(
		session.live_sample_stream.as_ref().unwrap().debug_last_request_kind(),
		Some("prime_monitor_nonblocking")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn rebuild_active_scroll_capture_live_stream_rebuilds_and_reprimes_after_aux_window_creation() {
	let monitor = tests::test_monitor();
	let (mut session, _original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.scroll_capture.live_stream.as_ref().unwrap().prime_monitor_nonblocking(monitor);

	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_points = Some(RectPoints::new(1, 2, 30, 40));
	session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(2, 4, 60, 80));

	session.scroll_capture.live_stream_backlog.push_back(ScrollCaptureLiveFrame {
		frame_seq: 3,
		captured_at: Instant::now(),
		image: tests::test_frozen_image(),
	});

	session.scroll_capture.last_stream_frame_seq = 3;
	session.scroll_capture.last_stream_event_at = Some(Instant::now());
	session.scroll_capture.last_stream_poll_at = Some(Instant::now());

	assert!(session.rebuild_active_scroll_capture_live_stream());

	let rebuilt_scroll_live_stream = session.scroll_capture.live_stream.as_ref().unwrap();

	assert_eq!(rebuilt_scroll_live_stream.debug_self_capture_exception_window_ids(), &[17]);
	assert_eq!(rebuilt_scroll_live_stream.debug_last_request_kind(), None);
	assert!(session.scroll_capture.live_stream_backlog.is_empty());
	assert_eq!(session.scroll_capture.last_stream_frame_seq, 0);
	assert!(session.scroll_capture.last_stream_event_at.is_none());
	assert!(session.scroll_capture.last_stream_poll_at.is_none());

	rebuilt_scroll_live_stream.prime_monitor_nonblocking(monitor);

	assert_eq!(
		session.scroll_capture.live_stream.as_ref().unwrap().debug_last_request_kind(),
		Some("prime_monitor_nonblocking")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn armed_freeze_capture_without_worker_restores_visibility_and_surfaces_error() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.pending_freeze_capture = Some(monitor);
	session.pending_freeze_capture_armed = true;
	session.capture_windows_hidden = true;

	session.maybe_dispatch_armed_freeze_capture();

	assert!(session.pending_freeze_capture.is_none());
	assert!(session.inflight_freeze_capture.is_none());
	assert!(!session.pending_freeze_capture_armed);
	assert!(!session.capture_windows_hidden);
	assert_eq!(session.state.error_message.as_deref(), Some("Capture worker is unavailable."));
}

#[cfg(target_os = "macos")]
#[test]
fn authoritative_freeze_capture_keeps_overlay_windows_visible_on_macos() {
	let session = OverlaySession::new();

	assert!(!session.should_hide_overlay_windows_during_capture());
}

#[cfg(target_os = "macos")]
#[test]
fn repeated_freeze_capture_send_full_aborts_and_restores_hidden_windows() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.pending_freeze_capture = Some(monitor);
	session.pending_freeze_capture_armed = true;
	session.capture_windows_hidden = true;

	for _ in 0..FREEZE_CAPTURE_SEND_FULL_RETRY_LIMIT.saturating_sub(1) {
		session.handle_freeze_capture_request_send_error(monitor, WorkerRequestSendError::Full);

		assert_eq!(session.pending_freeze_capture, Some(monitor));
		assert!(session.pending_freeze_capture_armed);
		assert!(session.capture_windows_hidden);
		assert!(session.state.error_message.is_none());
	}

	session.handle_freeze_capture_request_send_error(monitor, WorkerRequestSendError::Full);

	assert!(session.pending_freeze_capture.is_none());
	assert!(session.inflight_freeze_capture.is_none());
	assert!(!session.pending_freeze_capture_armed);
	assert!(!session.capture_windows_hidden);
	assert_eq!(session.freeze_capture_send_full_count, 0);
	assert_eq!(
		session.state.error_message.as_deref(),
		Some("Capture worker is busy. Please try again.")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn showing_loupe_window_requests_lazy_creation_before_applying_stream_upgrade() {
	let monitor = tests::test_monitor();
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();
	let original_live_sample_stream = ptr::from_ref(session.live_sample_stream.as_ref().unwrap());
	let original_scroll_live_stream =
		ptr::from_ref(session.scroll_capture.live_stream.as_ref().unwrap());

	session.pending_startup_aux_live_stream_filter_upgrade = true;

	session.set_alt_loupe_window_visible(Some(monitor), true);

	assert!(session.pending_startup_aux_live_stream_filter_upgrade);
	assert!(session.startup_aux_window_creation_pending);
	assert_eq!(
		ptr::from_ref(session.live_sample_stream.as_ref().unwrap()),
		original_live_sample_stream
	);
	assert_eq!(
		ptr::from_ref(session.scroll_capture.live_stream.as_ref().unwrap()),
		original_scroll_live_stream
	);
	assert_eq!(session.live_sample_stream.as_ref().unwrap().debug_last_request_kind(), None);
	assert_eq!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
}

#[cfg(target_os = "macos")]
#[test]
fn apply_self_capture_exception_window_ids_to_active_streams_keeps_scroll_live_stream_disabled_in_worker_mode()
 {
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	tests::enable_test_worker_scroll_capture_path(&mut session);

	session.test_push_scroll_capture_live_frame(ScrollCaptureLiveFrame {
		frame_seq: 9,
		captured_at: Instant::now(),
		image: tests::test_frozen_image(),
	});

	session.scroll_capture.last_stream_event_at = Some(Instant::now());
	session.scroll_capture.last_stream_poll_at = Some(Instant::now());

	session.apply_self_capture_exception_window_ids_to_active_streams();

	assert_eq!(
		session.live_sample_stream.as_ref().unwrap().debug_self_capture_exception_window_ids(),
		&[17]
	);
	assert!(session.scroll_capture.live_stream.is_none());
	assert!(session.scroll_capture.live_stream_backlog.is_empty());
	assert!(session.scroll_capture.last_stream_event_at.is_none());
	assert!(session.scroll_capture.last_stream_poll_at.is_none());
	assert_ne!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
}

#[cfg(target_os = "macos")]
#[test]
fn apply_self_capture_exception_window_ids_to_active_streams_defers_worker_refresh_while_freeze_is_inflight()
 {
	let monitor = tests::test_monitor();
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.inflight_freeze_capture = Some(monitor);

	session.apply_self_capture_exception_window_ids_to_active_streams();

	assert_eq!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(session.pending_self_capture_exception_window_ids_worker_refresh);
	assert_eq!(
		session.live_sample_stream.as_ref().unwrap().debug_self_capture_exception_window_ids(),
		&[17]
	);
}

#[cfg(target_os = "macos")]
#[test]
fn apply_self_capture_exception_window_ids_to_active_streams_defers_worker_refresh_while_hit_test_is_inflight()
 {
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.pending_click_hit_test_request_id = Some(7);

	session.apply_self_capture_exception_window_ids_to_active_streams();

	assert_eq!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(session.pending_self_capture_exception_window_ids_worker_refresh);
}

#[cfg(target_os = "macos")]
#[test]
fn apply_self_capture_exception_window_ids_to_active_streams_defers_worker_refresh_while_window_list_refresh_is_inflight()
 {
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.window_list_refresh_inflight = true;

	session.apply_self_capture_exception_window_ids_to_active_streams();

	assert_eq!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(session.pending_self_capture_exception_window_ids_worker_refresh);
}

#[cfg(target_os = "macos")]
#[test]
fn apply_self_capture_exception_window_ids_to_active_streams_defers_worker_refresh_while_png_encode_is_inflight()
 {
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.png_encode_inflight = true;

	session.apply_self_capture_exception_window_ids_to_active_streams();

	assert_eq!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(session.pending_self_capture_exception_window_ids_worker_refresh);
}

#[cfg(target_os = "macos")]
#[test]
fn captured_freeze_response_applies_deferred_worker_refresh() {
	let monitor = tests::test_monitor();
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.inflight_freeze_capture = Some(monitor);
	session.pending_self_capture_exception_window_ids_worker_refresh = true;

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::CapturedFreeze {
		monitor,
		image: tests::test_frozen_image(),
		window_image: None,
		captured_window_id: None,
	});

	assert!(matches!(control, super::OverlayControl::Continue));
	assert_ne!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
}

#[cfg(target_os = "macos")]
#[test]
fn freeze_error_response_applies_deferred_worker_refresh() {
	let monitor = tests::test_monitor();
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.inflight_freeze_capture = Some(monitor);
	session.pending_self_capture_exception_window_ids_worker_refresh = true;

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::Error {
		source: WorkerErrorSource::FreezeCapture,
		message: String::from("freeze failed"),
	});

	assert!(matches!(control, super::OverlayControl::Continue));
	assert_ne!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
	assert_eq!(session.state.error_message.as_deref(), Some("freeze failed"));
}

#[cfg(target_os = "macos")]
#[test]
fn hit_test_response_applies_deferred_worker_refresh() {
	let monitor = tests::test_monitor();
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.pending_click_hit_test_request_id = Some(11);
	session.pending_self_capture_exception_window_ids_worker_refresh = true;

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::HitTestWindow {
		monitor,
		point: GlobalPoint::new(24, 36),
		request_id: 11,
		hit: None,
	});

	assert!(matches!(control, super::OverlayControl::Continue));
	assert_ne!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
}

#[cfg(target_os = "macos")]
#[test]
fn window_list_refresh_response_applies_deferred_worker_refresh() {
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.window_list_refresh_inflight = true;
	session.pending_self_capture_exception_window_ids_worker_refresh = true;

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::RefreshedWindowList {
		snapshot: Arc::new(WindowListSnapshot {
			captured_at: Instant::now(),
			windows: Arc::new(vec![WindowRect {
				window_id: Some(9),
				x: 10,
				y: 12,
				width: 30,
				height: 40,
			}]),
		}),
	});

	assert!(matches!(control, super::OverlayControl::Continue));
	assert_ne!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
}

#[cfg(target_os = "macos")]
#[test]
fn stale_window_list_refresh_response_is_dropped_after_self_capture_filter_change() {
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.window_list_snapshot = Some(Arc::new(WindowListSnapshot {
		captured_at: Instant::now(),
		windows: Arc::new(vec![WindowRect { window_id: Some(4), x: 1, y: 2, width: 3, height: 4 }]),
	}));
	session.window_list_refresh_inflight = true;

	session.apply_self_capture_exception_window_ids_to_active_streams();

	assert!(session.window_list_snapshot.is_none());
	assert!(session.drop_next_window_list_refresh_snapshot);
	assert!(session.pending_self_capture_exception_window_ids_worker_refresh);

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::RefreshedWindowList {
		snapshot: Arc::new(WindowListSnapshot {
			captured_at: Instant::now(),
			windows: Arc::new(vec![WindowRect {
				window_id: Some(9),
				x: 10,
				y: 12,
				width: 30,
				height: 40,
			}]),
		}),
	});

	assert!(matches!(control, super::OverlayControl::Continue));
	assert!(session.window_list_snapshot.is_none());
	assert!(!session.window_list_refresh_inflight);
	assert!(!session.drop_next_window_list_refresh_snapshot);
	assert_ne!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
}

#[cfg(target_os = "macos")]
#[test]
fn png_error_response_applies_deferred_worker_refresh() {
	let (mut session, original_worker_debug_id) = tests::configured_session_with_macos_worker();

	session.png_encode_inflight = true;
	session.pending_self_capture_exception_window_ids_worker_refresh = true;

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::Error {
		source: WorkerErrorSource::EncodePng,
		message: String::from("encode failed"),
	});

	assert!(matches!(control, super::OverlayControl::Continue));
	assert_ne!(session.worker.as_ref().unwrap().debug_id(), original_worker_debug_id);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
}

#[cfg(target_os = "macos")]
#[test]
fn capture_monitor_region_error_clears_scroll_capture_inflight_and_pauses_session() {
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.inflight_request_id = Some(41);
	session.scroll_capture.inflight_request_observation = Some(InflightScrollCaptureObservation {
		was_observable: true,
		external_input_seq: 9,
		input_direction: Some(ScrollDirection::Down),
	});

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::Error {
		source: WorkerErrorSource::CaptureMonitorRegion,
		message: String::from("capture timed out"),
	});

	assert!(matches!(control, super::OverlayControl::Continue));
	assert_eq!(session.scroll_capture.inflight_request_id, None);
	assert_eq!(session.scroll_capture.inflight_request_observation, None);
	assert!(session.scroll_capture.paused);
	assert_eq!(session.state.error_message.as_deref(), Some("capture timed out"));
}
