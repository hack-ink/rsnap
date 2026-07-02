use std::time::Duration;

use image::{Rgba, RgbaImage};

#[cfg(target_os = "macos")]
use crate::overlay::tests::rendering_behaviors::overlay::macos_cursor_runtime;
use crate::overlay::tests::rendering_behaviors::{
	CursorIcon, ElementState, FrozenCaptureSource, FrozenSelectionCorner, FrozenSelectionDragState,
	FrozenSelectionInteractionKind, FrozenToolbarState, FrozenToolbarTool, GlobalPoint,
	MonitorRect, MouseButton, OverlayMode, OverlaySession, OverlayState, Pos2, Rect, RectPoints,
	SELECTION_SIZE_BADGE_GAP_PX, SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX, TOOLBAR_CAPTURE_GAP_PX,
	ToolbarPlacement, Vec2, WindowRenderer, tests,
};

#[test]
fn frozen_selection_drag_starts_only_for_drag_region_inside_capture_rect() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);

	assert!(!session.begin_frozen_selection_drag(GlobalPoint::new(150, 180)));

	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(!session.begin_frozen_selection_drag(GlobalPoint::new(50, 80)));
	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(150, 180)));
	assert_eq!(
		session.frozen_selection_drag,
		FrozenSelectionDragState {
			active: true,
			interaction: FrozenSelectionInteractionKind::Move,
			anchor_rect: capture_rect,
			pointer_offset_x: 50,
			pointer_offset_y: 60,
			press_cursor_x: 150,
			press_cursor_y: 180,
		}
	);

	session.stop_frozen_selection_drag();

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 120, 200, 240));

	assert!(!session.begin_frozen_selection_drag(GlobalPoint::new(-1, 180)));
}

#[test]
fn frozen_selection_drag_starts_corner_resize_from_handle_hit_zone() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(95, 115)));
	assert_eq!(
		session.frozen_selection_drag,
		FrozenSelectionDragState {
			active: true,
			interaction: FrozenSelectionInteractionKind::Resize(FrozenSelectionCorner::TopLeft),
			anchor_rect: capture_rect,
			pointer_offset_x: 0,
			pointer_offset_y: 0,
			press_cursor_x: 95,
			press_cursor_y: 115,
		}
	);
}

#[test]
fn frozen_selection_drag_or_resize_updates_capture_rect_and_toolbar_position() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);

	for (press, drag_to, expected_rect) in [
		(
			GlobalPoint::new(150, 180),
			GlobalPoint::new(300, 360),
			RectPoints::new(250, 300, 200, 240),
		),
		(
			GlobalPoint::new(95, 115),
			GlobalPoint::new(160, 190),
			RectPoints::new(165, 195, 135, 165),
		),
	] {
		let mut session = OverlaySession::new();

		session.state.begin_freeze(monitor);

		tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

		session.state.frozen_capture_rect = Some(capture_rect);
		session.frozen_capture_source = FrozenCaptureSource::DragRegion;

		session.seed_frozen_toolbar_default_position(monitor, capture_rect);

		assert!(session.begin_frozen_selection_drag(press));
		assert!(session.update_frozen_selection_drag_rect(drag_to));

		let expected_toolbar_pos =
			session.frozen_toolbar_default_position_for_capture_rect(monitor, expected_rect);

		assert_eq!(session.state.frozen_capture_rect, Some(expected_rect));
		assert_eq!(session.toolbar_state.floating_position, Some(expected_toolbar_pos));
	}
}

#[test]
fn frozen_selection_drag_hides_auxiliary_windows_while_active() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.toolbar_state.visible = true;
	session.hud_window_visible = true;
	session.loupe_window_visible = true;
	session.toolbar_window_visible = true;

	assert!(!session.frozen_selection_drag_hides_auxiliary_windows());
	assert!(!session.should_hide_toolbar_window(monitor));
	assert!(session.should_hide_scroll_preview_window());
	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(150, 180)));
	assert!(session.frozen_selection_drag_hides_auxiliary_windows());
	assert!(!session.hud_window_visible);
	assert!(!session.loupe_window_visible);
	assert!(!session.toolbar_window_visible);
	assert!(session.skip_toolbar_focus_on_next_show);
	assert!(session.should_hide_toolbar_window(monitor));
	assert!(!session.should_focus_frozen_toolbar_window_on_show());

	session.stop_frozen_selection_drag();

	assert!(!session.frozen_selection_drag_hides_auxiliary_windows());
	assert!(!session.should_hide_toolbar_window(monitor));
	assert!(!session.should_focus_frozen_toolbar_window_on_show());
}

#[test]
fn frozen_selection_drag_releases_scroll_preview_hide_after_drag_stops() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(150, 180)));

	session.scroll_capture.active = true;

	assert!(session.should_hide_scroll_preview_window());

	session.stop_frozen_selection_drag();

	assert!(!session.should_hide_scroll_preview_window());
}

#[test]
fn frozen_selection_drag_defers_pending_toolbar_window_move() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(150, 180)));

	let last_move_at = session.last_toolbar_window_move_at;

	session.pending_toolbar_outer_pos = Some(GlobalPoint::new(220, 260));

	session.maybe_apply_pending_toolbar_window_move(last_move_at + Duration::from_millis(32));

	assert_eq!(session.pending_toolbar_outer_pos, Some(GlobalPoint::new(220, 260)));
	assert_eq!(session.last_toolbar_window_move_at, last_move_at);
}

#[test]
fn frozen_selection_drag_keeps_toolbar_show_unfocused_even_before_first_show() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.toolbar_state.visible = true;

	assert!(!session.toolbar_window_visible);
	assert!(!session.skip_toolbar_focus_on_next_show);
	assert!(!session.should_focus_frozen_toolbar_window_on_show());
	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(150, 180)));
	assert!(session.skip_toolbar_focus_on_next_show);
	assert!(!session.should_focus_frozen_toolbar_window_on_show());

	session.stop_frozen_selection_drag();

	assert!(session.skip_toolbar_focus_on_next_show);
	assert!(!session.should_focus_frozen_toolbar_window_on_show());
}

#[test]
fn entering_frozen_capture_skips_initial_toolbar_focus_restore() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	session.toolbar_state.visible = true;

	assert!(session.skip_toolbar_focus_on_next_show);
	assert!(!session.should_focus_frozen_toolbar_window_on_show());
	#[cfg(target_os = "macos")]
	assert!(!session.preserve_frontmost_on_next_toolbar_show);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_selection_drag_does_not_rearm_initial_frontmost_restore() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.preserve_frontmost_on_next_toolbar_show = false;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(250, 240)));
	assert!(session.skip_toolbar_focus_on_next_show);
	assert!(!session.preserve_frontmost_on_next_toolbar_show);
}

#[test]
fn frozen_selection_resize_preserves_handle_press_offset() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(95, 115)));
	assert!(session.update_frozen_selection_drag_rect(GlobalPoint::new(96, 116)));
	assert_eq!(session.state.frozen_capture_rect, Some(RectPoints::new(101, 121, 199, 239)));
}

#[test]
fn frozen_selection_drag_clamps_capture_rect_to_monitor_bounds() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(150, 180)));
	assert!(session.update_frozen_selection_drag_rect(GlobalPoint::new(-200, -300)));
	assert_eq!(session.state.frozen_capture_rect, Some(RectPoints::new(0, 0, 200, 240)));
	assert!(session.update_frozen_selection_drag_rect(GlobalPoint::new(1_500, 1_400)));
	assert_eq!(session.state.frozen_capture_rect, Some(RectPoints::new(800, 560, 200, 240)));
}

#[test]
fn frozen_selection_resize_clamps_to_minimum_size_and_monitor_bounds() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(305, 365)));
	assert!(session.update_frozen_selection_drag_rect(GlobalPoint::new(-200, -300)));
	assert_eq!(session.state.frozen_capture_rect, Some(RectPoints::new(100, 120, 1, 1)));
	assert!(session.update_frozen_selection_drag_rect(GlobalPoint::new(1_500, 1_400)));
	assert_eq!(session.state.frozen_capture_rect, Some(RectPoints::new(100, 120, 900, 680)));
}

#[test]
fn frozen_selection_rect_update_preserves_manual_toolbar_move() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	session.seed_frozen_toolbar_default_position(monitor, capture_rect);

	let moved_pos =
		session.toolbar_state.floating_position.expect("toolbar default position should be seeded")
			+ Vec2::new(18.0, 22.0);

	session.toolbar_state.floating_position = Some(moved_pos);

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(305, 365)));
	assert!(session.update_frozen_selection_drag_rect(GlobalPoint::new(360, 420)));

	let expected_rect = RectPoints::new(100, 120, 255, 295);
	let expected_default_pos =
		session.frozen_toolbar_default_position_for_capture_rect(monitor, expected_rect);

	assert_eq!(session.state.frozen_capture_rect, Some(expected_rect));
	assert_eq!(session.toolbar_state.floating_position, Some(moved_pos));
	assert_eq!(session.toolbar_state.default_slot_position, Some(expected_default_pos));
}

#[test]
fn cropped_frozen_capture_image_uses_moved_capture_rect() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 64,
		height: 48,
		scale_factor_x1000: 1_000,
	};
	let image = RgbaImage::from_fn(64, 48, |x, y| Rgba([x as u8, y as u8, 0, 255]));
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_ready_state(&mut session, monitor, image);

	session.state.frozen_capture_rect = Some(RectPoints::new(8, 6, 40, 32));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(28, 22)));
	assert!(session.update_frozen_selection_drag_rect(GlobalPoint::new(32, 28)));

	let cropped = session.cropped_frozen_capture_image().expect("moved frozen crop");

	assert_eq!(cropped.width(), 40);
	assert_eq!(cropped.height(), 32);
	assert_eq!(cropped.get_pixel(0, 0), &Rgba([12, 12, 0, 255]));
	assert_eq!(cropped.get_pixel(39, 31), &Rgba([51, 43, 0, 255]));
}

#[test]
fn auto_center_frozen_capture_rect_recenters_detected_content_across_tools() {
	let monitor = tests::test_monitor_with_scale(80, 60, 2_000);
	let capture_rect = RectPoints::new(20, 16, 40, 24);

	for (label, selected_tool, seed_toolbar) in [
		("pointer tool recenters toolbar", FrozenToolbarTool::Pointer, true),
		("annotation tool remains eligible", FrozenToolbarTool::Mosaic, false),
	] {
		let mut image = RgbaImage::from_pixel(160, 120, Rgba([14, 16, 20, 255]));
		let mut session = OverlaySession::new();

		for y in 40..52 {
			for x in 52..68 {
				image.put_pixel(x, y, Rgba([228, 232, 240, 255]));
			}
		}

		session.state.begin_freeze(monitor);

		tests::finish_frozen_ready_state(&mut session, monitor, image);

		session.state.frozen_capture_rect = Some(capture_rect);
		session.frozen_capture_source = FrozenCaptureSource::DragRegion;
		session.toolbar_state.selected_tool = selected_tool;

		if seed_toolbar {
			session.seed_frozen_toolbar_default_position(monitor, capture_rect);
		}

		assert!(session.frozen_auto_center_available(), "{label}");
		assert!(session.auto_center_frozen_capture_rect(), "{label}");

		let expected_rect = RectPoints::new(10, 11, 40, 24);

		assert_eq!(session.state.frozen_capture_rect, Some(expected_rect), "{label}");

		if seed_toolbar {
			let expected_toolbar_pos =
				session.frozen_toolbar_default_position_for_capture_rect(monitor, expected_rect);

			assert_eq!(
				session.toolbar_state.floating_position,
				Some(expected_toolbar_pos),
				"{label}",
			);
		}
	}
}

#[test]
fn auto_center_frozen_capture_rect_repeats_until_content_margins_balance() {
	let monitor = tests::test_monitor_with_scale(80, 60, 1_000);
	let capture_rect = RectPoints::new(20, 16, 40, 24);
	let mut image = RgbaImage::from_pixel(80, 60, Rgba([14, 16, 20, 255]));
	let mut session = OverlaySession::new();

	for y in 24..36 {
		for x in 38..68 {
			image.put_pixel(x, y, Rgba([228, 232, 240, 255]));
		}
	}

	session.state.begin_freeze(monitor);

	tests::finish_frozen_ready_state(&mut session, monitor, image);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.auto_center_frozen_capture_rect());
	assert_eq!(session.state.frozen_capture_rect, Some(RectPoints::new(33, 18, 40, 24)));
}

#[test]
fn frozen_selection_resize_hit_test_prefers_corner_handles() {
	let capture_rect = RectPoints::new(100, 120, 8, 8);

	assert_eq!(
		WindowRenderer::frozen_selection_resize_hit_test(capture_rect, Pos2::new(100.0, 120.0)),
		Some(FrozenSelectionCorner::TopLeft)
	);
	assert_eq!(
		WindowRenderer::frozen_selection_resize_hit_test(capture_rect, Pos2::new(108.0, 128.0)),
		Some(FrozenSelectionCorner::BottomRight)
	);
	assert_eq!(
		WindowRenderer::frozen_selection_resize_hit_test(capture_rect, Pos2::new(104.0, 124.0)),
		None
	);
}

#[test]
fn frozen_selection_interaction_keeps_move_in_tiny_selection_center() {
	let capture_rect = RectPoints::new(100, 120, 8, 8);

	assert_eq!(
		OverlaySession::frozen_selection_interaction_kind(capture_rect, 104, 124),
		Some(FrozenSelectionInteractionKind::Move)
	);
}

#[test]
fn frozen_selection_cursor_icon_uses_corner_resize_hover() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.state.cursor = Some(GlobalPoint::new(95, 115));

	assert_eq!(session.frozen_selection_cursor_icon_for_monitor(monitor), CursorIcon::NwseResize);

	session.state.cursor = Some(GlobalPoint::new(150, 180));

	assert_eq!(session.frozen_selection_cursor_icon_for_monitor(monitor), CursorIcon::Grab);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_selection_cursor_rects_use_native_handle_hover_and_full_window_resize_drag() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);

	assert_eq!(
		macos_cursor_runtime::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(95.0, 115.0)),
		Some(CursorIcon::NwseResize)
	);
	assert_eq!(
		macos_cursor_runtime::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(305.0, 115.0)),
		Some(CursorIcon::NeswResize)
	);
	assert_eq!(
		macos_cursor_runtime::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(150.0, 180.0)),
		Some(CursorIcon::Grab)
	);

	session.frozen_selection_drag = FrozenSelectionDragState {
		active: true,
		interaction: FrozenSelectionInteractionKind::Resize(FrozenSelectionCorner::TopLeft),
		anchor_rect: capture_rect,
		pointer_offset_x: 0,
		pointer_offset_y: 0,
		press_cursor_x: 100,
		press_cursor_y: 120,
	};

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);

	assert_eq!(rects.len(), 1);
	assert_eq!(rects[0].icon, CursorIcon::NwseResize);
	assert_eq!(rects[0].rect.min, Pos2::ZERO);
	assert_eq!(rects[0].rect.max, Pos2::new(monitor.width as f32, monitor.height as f32));
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_selection_cursor_rects_preserve_grabbing_cursor_during_move_drag() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.frozen_selection_drag = FrozenSelectionDragState {
		active: true,
		interaction: FrozenSelectionInteractionKind::Move,
		anchor_rect: capture_rect,
		pointer_offset_x: 50,
		pointer_offset_y: 60,
		press_cursor_x: 150,
		press_cursor_y: 180,
	};

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);

	assert_eq!(rects.len(), 1);
	assert_eq!(rects[0].icon, CursorIcon::Grabbing);
	assert_eq!(rects[0].rect.min, Pos2::ZERO);
	assert_eq!(rects[0].rect.max, Pos2::new(monitor.width as f32, monitor.height as f32));
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_selection_cursor_rects_match_resize_hit_test_for_tiny_overlapping_handles() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 8, 8);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);
	let top_overlap = Pos2::new(107.0, 117.0);
	let top_overlap_midline_tie = Pos2::new(104.0, 117.0);
	let center_inside = Pos2::new(104.0, 124.0);

	assert_eq!(
		WindowRenderer::frozen_selection_resize_hit_test(capture_rect, top_overlap),
		Some(FrozenSelectionCorner::TopRight)
	);
	assert_eq!(
		macos_cursor_runtime::overlay_cursor_rect_icon_at_point(&rects, top_overlap),
		Some(CursorIcon::NeswResize)
	);
	assert_eq!(
		WindowRenderer::frozen_selection_resize_hit_test(capture_rect, top_overlap_midline_tie),
		Some(FrozenSelectionCorner::TopLeft)
	);
	assert_eq!(
		macos_cursor_runtime::overlay_cursor_rect_icon_at_point(&rects, top_overlap_midline_tie),
		Some(CursorIcon::NwseResize)
	);
	assert_eq!(WindowRenderer::frozen_selection_resize_hit_test(capture_rect, center_inside), None);
	assert_eq!(
		macos_cursor_runtime::overlay_cursor_rect_icon_at_point(&rects, center_inside),
		Some(CursorIcon::Grab)
	);
}

#[test]
fn frozen_selection_cursor_icon_tracks_active_drag() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);

	for (
		interaction,
		pointer_offset_x,
		pointer_offset_y,
		press_cursor_x,
		press_cursor_y,
		expected,
	) in [
		(
			FrozenSelectionInteractionKind::Resize(FrozenSelectionCorner::BottomRight),
			0,
			0,
			300,
			360,
			CursorIcon::NwseResize,
		),
		(FrozenSelectionInteractionKind::Move, 50, 60, 150, 180, CursorIcon::Grabbing),
	] {
		let mut session = OverlaySession::new();

		session.state.begin_freeze(monitor);

		tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

		session.state.frozen_capture_rect = Some(capture_rect);
		session.frozen_capture_source = FrozenCaptureSource::DragRegion;
		session.frozen_selection_drag = FrozenSelectionDragState {
			active: true,
			interaction,
			anchor_rect: capture_rect,
			pointer_offset_x,
			pointer_offset_y,
			press_cursor_x,
			press_cursor_y,
		};

		assert_eq!(session.frozen_selection_cursor_icon_for_monitor(monitor), expected);
	}
}

#[test]
fn auto_center_frozen_capture_rect_noops_for_uniform_crop() {
	let monitor = tests::test_monitor_with_scale(80, 60, 1_000);
	let capture_rect = RectPoints::new(20, 16, 40, 24);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(
		&mut session,
		monitor,
		RgbaImage::from_pixel(80, 60, Rgba([24, 24, 28, 255])),
	);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(!session.auto_center_frozen_capture_rect());
	assert_eq!(session.state.frozen_capture_rect, Some(capture_rect));
}

#[test]
fn global_left_release_stops_frozen_selection_drag() {
	let mut session = OverlaySession::new();

	session.frozen_selection_drag = FrozenSelectionDragState {
		active: true,
		interaction: FrozenSelectionInteractionKind::Move,
		anchor_rect: RectPoints::new(10, 20, 30, 40),
		pointer_offset_x: 12,
		pointer_offset_y: 34,
		press_cursor_x: 22,
		press_cursor_y: 54,
	};

	session
		.maybe_stop_frozen_selection_drag_for_mouse_input(ElementState::Pressed, MouseButton::Left);

	assert!(session.frozen_selection_drag.active);

	session.maybe_stop_frozen_selection_drag_for_mouse_input(
		ElementState::Released,
		MouseButton::Right,
	);

	assert!(session.frozen_selection_drag.active);

	session.maybe_stop_frozen_selection_drag_for_mouse_input(
		ElementState::Released,
		MouseButton::Left,
	);

	assert_eq!(session.frozen_selection_drag, FrozenSelectionDragState::default());
}

#[test]
fn frozen_selection_size_badge_falls_inside_when_default_bottom_toolbar_slot_overlaps() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect_points = RectPoints::new(200, 180, 200, 300);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(capture_rect_points);

	let toolbar_state = FrozenToolbarState { visible: true, ..FrozenToolbarState::default() };
	let reserved_rect = WindowRenderer::frozen_toolbar_reserved_rect(
		&state,
		monitor,
		screen_rect,
		ToolbarPlacement::Bottom,
		&toolbar_state,
	)
	.expect("default bottom toolbar slot should be reserved");
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(reserved_rect.min.y, capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX);
	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.max.y, capture_rect.max.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn frozen_selection_size_badge_keeps_below_placement_after_toolbar_leaves_default_slot() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect_points = RectPoints::new(200, 180, 200, 300);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(capture_rect_points);

	let default_toolbar_pos = WindowRenderer::frozen_toolbar_default_window_pos(
		screen_rect,
		capture_rect,
		WindowRenderer::frozen_toolbar_size(&FrozenToolbarState::default()),
		WindowRenderer::frozen_toolbar_size(&FrozenToolbarState::default()),
		ToolbarPlacement::Bottom,
	);
	let toolbar_state = FrozenToolbarState {
		visible: true,
		floating_position: Some(default_toolbar_pos + Vec2::new(0.0, 24.0)),
		..FrozenToolbarState::default()
	};
	let reserved_rect = WindowRenderer::frozen_toolbar_reserved_rect(
		&state,
		monitor,
		screen_rect,
		ToolbarPlacement::Bottom,
		&toolbar_state,
	);
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		reserved_rect,
	);

	assert!(reserved_rect.is_none());
	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.min.y, capture_rect.max.y + SELECTION_SIZE_BADGE_GAP_PX);
}
