use image::{Rgba, RgbaImage};

use crate::overlay::tests::rendering_behaviors::{
	GlobalPoint, MonitorRect, OverlaySession, RectPoints, ScrollSession, tests,
};

#[test]
fn scroll_preview_prefers_right_side_when_space_exists() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_400,
		height: 900,
		scale_factor_x1000: 1_000,
	};
	let mut session = OverlaySession::new();

	session.state.frozen_capture_rect = Some(RectPoints::new(120, 160, 400, 320));

	let preview = session.scroll_preview_local_rect(monitor);

	assert_eq!(preview.min.y, 160.0);
	assert_eq!(preview.height(), 320.0);
	assert!(preview.min.x >= 120.0 + 400.0);
}

#[test]
fn scroll_preview_falls_back_to_left_when_right_side_is_tight() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 900,
		scale_factor_x1000: 1_000,
	};
	let mut session = OverlaySession::new();

	session.state.frozen_capture_rect = Some(RectPoints::new(760, 180, 200, 260));

	let preview = session.scroll_preview_local_rect(monitor);

	assert_eq!(preview.min.y, 180.0);
	assert_eq!(preview.height(), 260.0);
	assert!(preview.max.x <= 760.0);
}

#[test]
fn scroll_preview_grows_with_render_height_until_monitor_limit() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_400,
		height: 900,
		scale_factor_x1000: 1_000,
	};
	let mut session = OverlaySession::new();

	session.state.frozen_capture_rect = Some(RectPoints::new(120, 160, 400, 320));
	session.scroll_capture.preview_display_image = Some(RgbaImage::new(320, 960));

	let preview = session.scroll_preview_local_rect(monitor);

	assert_eq!(preview.min.y, 160.0);
	assert_eq!(preview.height(), 724.0);
}

#[test]
fn current_scroll_preview_render_image_prefers_committed_export_during_scroll_capture() {
	let mut session = OverlaySession::new();
	let base = tests::make_scroll_capture_test_image(3, &[[10, 0, 0, 255]; 8]);
	let grown = tests::make_scroll_capture_test_image(3, &[[20, 0, 0, 255]; 12]);
	let mismatched_preview = RgbaImage::from_pixel(320, 40, Rgba([99, 0, 0, 255]));
	let mut scroll_session = ScrollSession::new(base, 320).expect("scroll session");
	let _ = scroll_session.observe_downward_sample(grown).expect("observe");
	let expected_export = scroll_session.export_image().clone();

	session.scroll_capture.active = true;
	session.scroll_capture.session = Some(scroll_session);
	session.scroll_capture.preview_display_image = Some(mismatched_preview.clone());

	assert_eq!(session.current_scroll_preview_render_image().as_ref(), Some(&expected_export));
}

#[test]
fn current_scroll_preview_render_image_uses_preview_display_when_scroll_capture_is_inactive() {
	let preview = RgbaImage::from_pixel(320, 64, Rgba([42, 0, 0, 255]));
	let mut session = OverlaySession::new();

	session.scroll_capture.preview_display_image = Some(preview.clone());

	assert_eq!(session.current_scroll_preview_render_image().as_ref(), Some(&preview));
}

#[test]
fn scroll_capture_preview_dimensions_follow_render_authority_during_scroll_capture() {
	let mut session = OverlaySession::new();
	let base = tests::make_scroll_capture_test_image(3, &[[10, 0, 0, 255]; 8]);
	let grown = tests::make_scroll_capture_test_image(3, &[[20, 0, 0, 255]; 12]);
	let mismatched_preview = RgbaImage::from_pixel(320, 40, Rgba([99, 0, 0, 255]));
	let mut scroll_session = ScrollSession::new(base, 320).expect("scroll session");
	let _ = scroll_session.observe_downward_sample(grown).expect("observe");
	let expected_export = scroll_session.export_image().clone();

	session.scroll_capture.active = true;
	session.scroll_capture.session = Some(scroll_session);
	session.scroll_capture.preview_display_image = Some(mismatched_preview.clone());

	assert_eq!(
		session.scroll_capture_preview_dimensions(),
		Some([expected_export.width(), expected_export.height()])
	);
}

#[test]
fn refresh_scroll_preview_display_image_uses_export_sized_render_buffer_during_active_capture() {
	let mut session = OverlaySession::new();
	let base = tests::make_scroll_capture_test_image(3, &[[10, 0, 0, 255]; 8]);
	let grown = tests::make_scroll_capture_test_image(3, &[[20, 0, 0, 255]; 12]);
	let mut scroll_session = ScrollSession::new(base, 320).expect("scroll session");
	let _ = scroll_session.observe_downward_sample(grown).expect("observe");
	let expected_committed = scroll_session.export_image().clone();
	let expected_render = scroll_session.export_image().clone();

	session.scroll_capture.active = true;
	session.scroll_capture.session = Some(scroll_session);

	session.refresh_scroll_preview_committed_image();
	session.refresh_scroll_preview_display_image();

	assert_eq!(session.scroll_capture.preview_committed_image.as_ref(), Some(&expected_committed));
	assert_eq!(session.scroll_capture.preview_display_image.as_ref(), Some(&expected_render));
	assert_eq!(session.scroll_capture.last_overlay_preview_provisional_motion_rows_hint, None);
	assert_eq!(session.scroll_capture.last_overlay_preview_existing_candidate_height, None);
	assert_eq!(
		session.scroll_capture.last_overlay_preview_existing_candidate_motion_rows_hint,
		None
	);
	assert_eq!(session.scroll_capture.last_overlay_preview_ledger_candidate_height, None);
	assert_eq!(session.scroll_capture.last_overlay_preview_ledger_candidate_motion_rows_hint, None);
	assert_eq!(session.scroll_capture.last_overlay_preview_retained_candidate_height, None);
	assert_eq!(
		session.scroll_capture.last_overlay_preview_retained_candidate_motion_rows_hint,
		None
	);
	assert!(!session.scroll_capture.last_overlay_preview_retained_hint_matches_motion_rows);
	assert!(!session.scroll_capture.last_overlay_preview_fresh_latest_frame_can_drive);
	assert!(!session.scroll_capture.last_overlay_preview_strong_unresolved_registration);
	assert!(!session.scroll_capture.last_overlay_preview_latest_frame_present);
	assert!(!session.scroll_capture.last_overlay_preview_used_provisional);
	assert_eq!(
		session.scroll_capture_preview_dimensions(),
		Some([expected_render.width(), expected_render.height()])
	);
}
