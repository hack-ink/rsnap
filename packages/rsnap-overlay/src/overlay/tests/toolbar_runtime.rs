#[cfg(target_os = "macos")]
use crate::overlay::tests::overlay::toolbar_layout_model;
use crate::overlay::tests::{
	self, FrozenToolbarTool, GlobalPoint, HudTheme, OverlaySession, Pos2, Rect,
	TOOLBAR_SCREEN_MARGIN_PX, Vec2, WindowRenderer,
};

#[test]
fn toolbar_cursor_left_during_drag_keeps_drag_session_alive() {
	let mut session = OverlaySession::new();

	session.toolbar_left_button_down = true;
	session.toolbar_left_button_went_down = true;
	session.toolbar_pointer_local = Some(Pos2::new(48.0, 18.0));
	session.toolbar_state.dragging = true;
	session.toolbar_state.drag_offset = Vec2::new(12.0, 6.0);
	session.toolbar_state.drag_anchor = Some(Pos2::new(40.0, 14.0));
	session.toolbar_state.annotation_size_control_hovered = true;
	session.toolbar_state.annotation_size_wheel_accumulator = 24.0;

	let _ = session.handle_toolbar_cursor_left();

	assert!(session.toolbar_left_button_down);
	assert!(session.toolbar_left_button_went_down);
	assert_eq!(session.toolbar_pointer_local, Some(Pos2::new(48.0, 18.0)));
	assert!(session.toolbar_state.dragging);
	assert_eq!(session.toolbar_state.drag_offset, Vec2::new(12.0, 6.0));
	assert_eq!(session.toolbar_state.drag_anchor, Some(Pos2::new(40.0, 14.0)));
	assert!(!session.toolbar_state.annotation_size_control_hovered);
	assert_eq!(session.toolbar_state.annotation_size_wheel_accumulator, 0.0);
}

#[test]
fn toolbar_drag_start_eligibility_uses_live_cursor_then_cached_pointer() {
	#[cfg(target_os = "macos")]
	let primary_origin = toolbar_layout_model::frozen_toolbar_window_primary_origin();
	#[cfg(not(target_os = "macos"))]
	let primary_origin = Pos2::ZERO;
	let primary_rect = WindowRenderer::frozen_toolbar_primary_rect(
		&OverlaySession::new().toolbar_state,
		primary_origin,
	);
	let cached_cursor = primary_rect.center();
	let stale_cursor = Pos2::new(primary_rect.right() + 12.0, primary_rect.center().y);

	assert!(primary_rect.contains(cached_cursor));
	assert!(!primary_rect.contains(stale_cursor));

	for (label, cached_pointer, live_pointer) in [
		("live cursor wins over stale cache", stale_cursor, Some(cached_cursor)),
		("cached pointer wins when live cursor is missing", cached_cursor, None),
	] {
		let mut session = OverlaySession::new();

		session.toolbar_pointer_local = Some(cached_pointer);

		assert!(session.resolve_toolbar_drag_start_eligibility(live_pointer), "{label}");
	}
}

#[test]
fn toolbar_cursor_local_from_sampled_global_returns_none_when_sampling_fails() {
	let outer_position = GlobalPoint::new(320, 140);

	assert_eq!(
		OverlaySession::toolbar_cursor_local_from_sampled_global(outer_position, None),
		None
	);
}

#[test]
fn toolbar_visible_capsule_hit_test_excludes_gap_between_capsules() {
	let mut session = OverlaySession::new();

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	let primary_rect =
		WindowRenderer::frozen_toolbar_primary_rect(&session.toolbar_state, Pos2::ZERO);
	let window_rect =
		WindowRenderer::frozen_toolbar_window_rect(&session.toolbar_state, Pos2::ZERO);
	let primary_point = primary_rect.center();
	let gap_point = Pos2::new(primary_rect.center().x, primary_rect.max.y + 1.0);
	let style_point = Pos2::new(primary_rect.center().x, window_rect.max.y - 1.0);

	assert!(WindowRenderer::frozen_toolbar_visible_capsules_contain(
		&session.toolbar_state,
		Pos2::ZERO,
		primary_point
	));
	assert!(window_rect.contains(gap_point));
	assert!(!WindowRenderer::frozen_toolbar_visible_capsules_contain(
		&session.toolbar_state,
		Pos2::ZERO,
		gap_point
	));
	assert!(WindowRenderer::frozen_toolbar_visible_capsules_contain(
		&session.toolbar_state,
		Pos2::ZERO,
		style_point
	));
}

#[test]
fn toolbar_cursor_left_while_idle_clears_pointer_state() {
	let mut session = OverlaySession::new();

	session.toolbar_left_button_went_down = true;
	session.toolbar_left_button_went_up = true;
	session.toolbar_pointer_local = Some(Pos2::new(22.0, 12.0));
	session.toolbar_state.drag_offset = Vec2::new(5.0, 3.0);
	session.toolbar_state.drag_anchor = Some(Pos2::new(18.0, 11.0));
	session.toolbar_state.annotation_size_control_hovered = true;
	session.toolbar_state.annotation_size_wheel_accumulator = 16.0;

	let _ = session.handle_toolbar_cursor_left();

	assert_eq!(session.toolbar_pointer_local, None);
	assert!(!session.toolbar_left_button_went_down);
	assert!(!session.toolbar_left_button_went_up);
	assert_eq!(session.toolbar_state.drag_offset, Vec2::ZERO);
	assert_eq!(session.toolbar_state.drag_anchor, None);
	assert!(!session.toolbar_state.annotation_size_control_hovered);
	assert_eq!(session.toolbar_state.annotation_size_wheel_accumulator, 0.0);
}

#[test]
fn frozen_toolbar_selected_mode_uses_fill_without_border() {
	for theme in [HudTheme::Dark, HudTheme::Light] {
		let style = WindowRenderer::frozen_toolbar_button_style(theme, true, false, true);

		assert!(style.bg_color.a() > 0);
		assert_eq!(style.border_color, None);
	}
}

#[test]
fn toolbar_window_visibility_tracks_frozen_display_readiness() {
	let monitor = tests::test_monitor();

	for (label, has_display_pixels, expected_hide) in
		[("no frozen pixels", false, true), ("seeded preview pixels", true, false)]
	{
		let mut session = OverlaySession::new();

		session.state.begin_freeze(monitor);

		if has_display_pixels {
			tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());
		}

		assert_eq!(session.should_hide_toolbar_window(monitor), expected_hide, "{label}");

		tests::set_session_pending_freeze_capture(&mut session, Some(monitor));

		assert_eq!(session.should_hide_toolbar_window(monitor), expected_hide, "{label} pending");

		tests::set_session_pending_freeze_capture(&mut session, None);
		tests::set_session_inflight_freeze_capture(&mut session, Some(monitor));

		assert_eq!(session.should_hide_toolbar_window(monitor), expected_hide, "{label} inflight");
	}
}

#[cfg(target_os = "macos")]
#[test]
fn toolbar_window_is_needed_for_seeded_preview_before_final_capture_ready() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.toolbar_state.visible = true;

	session.request_aux_window_creation_if_needed();

	assert!(session.startup_aux_window_creation_pending);
	assert!(!tests::session_export_authority_ready(&session));
}

#[test]
fn frozen_toolbar_clamps_floating_position() {
	let monitor = Rect::from_min_size(Pos2::new(-200.0, -100.0), Vec2::new(500.0, 400.0));
	let toolbar_size = Vec2::new(220.0, 42.0);
	let clamped = WindowRenderer::clamp_toolbar_position(
		monitor,
		toolbar_size,
		Pos2::new(-400.0, -240.0),
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);

	assert_eq!(clamped.x, monitor.min.x + TOOLBAR_SCREEN_MARGIN_PX);
	assert_eq!(clamped.y, monitor.min.y + TOOLBAR_SCREEN_MARGIN_PX);
}
