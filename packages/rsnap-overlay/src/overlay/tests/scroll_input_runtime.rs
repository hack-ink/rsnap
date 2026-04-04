#![allow(clippy::wildcard_imports)]

use super::*;

#[cfg(target_os = "macos")]
#[test]
fn wrapped_pixel_delta_normalizes_back_to_signed_values() {
	assert_eq!(OverlaySession::normalize_macos_scroll_pixel_component(4_294_967_294.0), -2.0);
	assert_eq!(OverlaySession::normalize_macos_scroll_pixel_component(4_294_967_290.0), -6.0);
}

#[test]
fn positive_vertical_wheel_delta_maps_to_upward_scroll_capture() {
	assert_eq!(
		OverlaySession::scroll_capture_direction_from_wheel_delta(&MouseScrollDelta::LineDelta(
			0.0, 1.0
		)),
		Some(ScrollDirection::Up)
	);
}

#[test]
fn negative_vertical_wheel_delta_maps_to_downward_scroll_capture() {
	assert_eq!(
		OverlaySession::scroll_capture_direction_from_wheel_delta(&MouseScrollDelta::LineDelta(
			0.0, -1.0
		)),
		Some(ScrollDirection::Down)
	);
}

#[test]
fn external_scroll_input_inside_capture_rect_uses_upward_observation_for_positive_delta() {
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

	session.handle_external_scroll_input_delta_y(150.0, 160.0, 4.0, true, false);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Up));
	assert!(session.scroll_capture.input_direction_at.is_some());
	assert!(session.scroll_capture.input_gesture_active);
	assert_eq!(session.scroll_capture.downward_motion_rows_pending, 0.0);
}

#[test]
fn external_scroll_input_inside_capture_rect_uses_downward_observation_for_negative_delta() {
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

	session.handle_external_scroll_input_delta_y(150.0, 160.0, -4.0, true, false);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(session.scroll_capture.input_direction_at.is_some());
	assert!(session.scroll_capture.input_gesture_active);
}

#[test]
fn upward_external_scroll_input_clears_existing_downward_motion_backlog() {
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
	session.scroll_capture.downward_motion_rows_pending = 128.0;

	session.handle_external_scroll_input_delta_y(150.0, 160.0, 12.0, true, false);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Up));
	assert_eq!(session.scroll_capture.downward_motion_rows_pending, 0.0);
}

#[test]
#[cfg(target_os = "macos")]
fn external_scroll_input_outside_capture_rect_on_same_monitor_is_still_consumed() {
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

	session.handle_external_scroll_input_delta_y(50.0, 50.0, -4.0, true, false);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(session.scroll_capture.input_direction_at.is_some());
	assert!(session.scroll_capture.input_gesture_active);
	assert_eq!(session.scroll_capture.downward_motion_rows_pending, 4.0);
}

#[test]
#[cfg(not(target_os = "macos"))]
fn external_scroll_input_outside_capture_rect_is_ignored() {
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

	session.handle_external_scroll_input_delta_y(50.0, 50.0, 4.0, true, false);

	assert_eq!(session.scroll_capture.input_direction, None);
	assert!(session.scroll_capture.input_direction_at.is_none());
}

#[test]
fn external_scroll_input_outside_scroll_monitor_is_ignored() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(1_000, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));

	session.handle_external_scroll_input_delta_y(50.0, 50.0, 4.0, true, false);

	assert_eq!(session.scroll_capture.input_direction, None);
	assert!(session.scroll_capture.input_direction_at.is_none());
	assert_eq!(session.scroll_capture.downward_motion_rows_pending, 0.0);
}

#[test]
fn external_scroll_input_terminal_event_preserves_last_direction_for_freshness() {
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
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	session.handle_external_scroll_input_delta_y(150.0, 160.0, 0.0, false, true);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(session.scroll_capture.input_direction_at.is_some());
	assert!(!session.scroll_capture.input_gesture_active);
	assert!(session.scroll_capture_input_allows_growth());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_overlay_mouse_passthrough_window_arms_and_expires() {
	let now = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;

	session.arm_scroll_overlay_mouse_passthrough_window(now, "test");

	assert!(session.scroll_capture.overlay_mouse_passthrough_active);
	assert_eq!(
		session.scroll_capture.overlay_mouse_passthrough_until,
		Some(now + SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE)
	);

	session.sync_scroll_overlay_mouse_passthrough_window(
		now + SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE / 2,
	);

	assert!(session.scroll_capture.overlay_mouse_passthrough_active);

	session.sync_scroll_overlay_mouse_passthrough_window(
		now + SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE + Duration::from_millis(1),
	);

	assert!(!session.scroll_capture.overlay_mouse_passthrough_active);
	assert!(session.scroll_capture.overlay_mouse_passthrough_until.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_start_enables_persistent_passthrough() {
	let mut session = OverlaySession::new();

	seed_ready_scroll_capture_selection(&mut session);

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.scroll_capture.active);
	assert!(session.scroll_capture.overlay_mouse_passthrough_active);
	assert!(session.scroll_capture.overlay_mouse_passthrough_persistent);
	assert!(session.scroll_capture.overlay_mouse_passthrough_until.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_pause_and_resume_toggle_persistent_passthrough() {
	let mut session = OverlaySession::new();

	seed_ready_scroll_capture_selection(&mut session);

	let _ = session.start_scroll_capture();

	session.toggle_scroll_capture_paused();

	assert!(session.scroll_capture.paused);
	assert!(!session.scroll_capture.overlay_mouse_passthrough_active);
	assert!(!session.scroll_capture.overlay_mouse_passthrough_persistent);

	session.toggle_scroll_capture_paused();

	assert!(!session.scroll_capture.paused);
	assert!(session.scroll_capture.overlay_mouse_passthrough_active);
	assert!(session.scroll_capture.overlay_mouse_passthrough_persistent);
	assert!(session.scroll_capture.overlay_mouse_passthrough_until.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn external_scroll_input_extends_passthrough_window_inside_capture_rect() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let earlier = Instant::now() - Duration::from_millis(20);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));

	session.arm_scroll_overlay_mouse_passthrough_window(earlier, "test");

	let first_deadline = session.scroll_capture.overlay_mouse_passthrough_until;

	session.handle_external_scroll_input_delta_y(150.0, 160.0, 4.0, true, false);

	assert!(session.scroll_capture.overlay_mouse_passthrough_active);
	assert!(session.scroll_capture.overlay_mouse_passthrough_until > first_deadline);
}

#[test]
fn terminal_positive_scroll_event_sets_upward_observation_before_finishing() {
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

	session.handle_external_scroll_input_delta_y(150.0, 160.0, 4.0, false, true);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Up));
	assert!(session.scroll_capture.input_direction_at.is_some());
	assert!(!session.scroll_capture.input_gesture_active);
	assert!(session.scroll_capture_input_allows_growth());
}

#[test]
fn terminal_negative_scroll_event_still_allows_downward_growth() {
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

	session.handle_external_scroll_input_delta_y(150.0, 160.0, -4.0, false, true);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(session.scroll_capture.input_direction_at.is_some());
	assert!(!session.scroll_capture.input_gesture_active);
	assert!(session.scroll_capture_input_allows_growth());
}

#[cfg(target_os = "macos")]
#[test]
fn overlay_wheel_fallback_records_direction_with_drain_reader_present() {
	let observed_at = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;

	session.set_external_scroll_input_drain_reader(Arc::new(|_, _| Vec::new()));
	session.record_scroll_capture_input_direction_from_overlay_wheel_at(
		&MouseScrollDelta::LineDelta(0.0, 1.0),
		observed_at,
	);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Up));
	assert_eq!(session.scroll_capture.input_direction_at, Some(observed_at));
	assert!(!session.scroll_capture.input_gesture_active);
}

#[test]
fn missing_scroll_direction_does_not_allow_growth() {
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;

	assert!(!session.scroll_capture_input_allows_growth());
}

#[test]
fn fresh_upward_direction_still_allows_observation() {
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.input_direction = Some(ScrollDirection::Up);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	assert!(session.scroll_capture_input_allows_observation());
	assert!(session.scroll_capture_input_allows_growth());
}

#[test]
fn fresh_downward_direction_allows_growth_without_active_gesture() {
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = false;

	assert!(session.scroll_capture_input_allows_growth());
}

#[test]
fn upward_direction_still_allows_growth_gate() {
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.input_direction = Some(ScrollDirection::Up);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	assert!(session.scroll_capture_input_allows_growth());
}

#[test]
fn upward_input_does_not_dirty_later_downward_growth() {
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
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());

	set_scroll_capture_input(&mut session, ScrollDirection::Down);

	assert_eq!(
		observe_scroll_capture_frame(&mut session, make_scroll_capture_window(&document, 3, 1, 5),),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(
		observe_scroll_capture_frame(&mut session, make_scroll_capture_window(&document, 3, 2, 5),),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);

	let height_after_second_append = scroll_capture_export_height(&session);

	set_scroll_capture_input(&mut session, ScrollDirection::Up);

	assert!(matches!(
		observe_scroll_capture_frame(&mut session, make_scroll_capture_window(&document, 3, 0, 5),),
		Some(
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		)
	));
	assert_eq!(scroll_capture_export_height(&session), height_after_second_append);

	set_scroll_capture_input(&mut session, ScrollDirection::Down);

	assert_eq!(
		observe_scroll_capture_frame(&mut session, make_scroll_capture_window(&document, 3, 2, 5),),
		Some(ScrollObserveOutcome::NoChange)
	);
	assert_eq!(scroll_capture_export_height(&session), height_after_second_append);

	set_scroll_capture_input(&mut session, ScrollDirection::Up);

	assert!(matches!(
		observe_scroll_capture_frame(&mut session, make_scroll_capture_window(&document, 3, 1, 5),),
		Some(
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		)
	));
	assert_eq!(scroll_capture_export_height(&session), height_after_second_append);

	set_scroll_capture_input(&mut session, ScrollDirection::Down);

	assert_eq!(
		observe_scroll_capture_frame(&mut session, make_scroll_capture_window(&document, 3, 2, 5),),
		Some(ScrollObserveOutcome::NoChange)
	);
	assert_eq!(scroll_capture_export_height(&session), height_after_second_append);
	assert_eq!(
		observe_scroll_capture_frame(&mut session, make_scroll_capture_window(&document, 3, 3, 5),),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
}
