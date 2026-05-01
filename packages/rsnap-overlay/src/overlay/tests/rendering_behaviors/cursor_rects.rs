use crate::overlay::tests::rendering_behaviors::{
	CursorIcon, GlobalPoint, OverlayMode, OverlaySession, Pos2, tests,
};
#[cfg(target_os = "macos")]
use crate::overlay::tests::rendering_behaviors::{Object, frozen_selection_runtime};
#[cfg(target_os = "macos")]
use crate::overlay::tests::rendering_behaviors::{Rect, Vec2, overlay};

#[test]
fn toolbar_cursor_global_position_from_outer_uses_cached_toolbar_origin() {
	let outer_position = GlobalPoint::new(220, 260);
	let cursor_local = Pos2::new(18.25, 12.75);

	assert_eq!(
		OverlaySession::toolbar_cursor_global_position_from_outer(outer_position, cursor_local),
		GlobalPoint::new(238, 273)
	);
}

#[test]
fn live_overlay_cursor_icon_uses_crosshair() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	assert_eq!(session.overlay_cursor_icon_for_monitor(monitor), CursorIcon::Crosshair);
}

#[cfg(target_os = "macos")]
#[test]
fn live_cursor_rects_cover_overlay_with_crosshair() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);

	assert_eq!(rects.len(), 1);
	assert_eq!(rects[0].icon, CursorIcon::Crosshair);
	assert_eq!(rects[0].rect.min, Pos2::ZERO);
	assert_eq!(rects[0].rect.max, Pos2::new(monitor.width as f32, monitor.height as f32));
}

#[cfg(target_os = "macos")]
#[test]
fn macos_live_cursor_sync_keeps_render_rects_active_during_native_shell_input() {
	assert!(frozen_selection_runtime::macos_overlay_cursor_uses_render_rects(
		OverlayMode::Live,
		true,
	));
	assert!(frozen_selection_runtime::macos_overlay_cursor_uses_render_rects(
		OverlayMode::Live,
		false,
	));
	assert!(frozen_selection_runtime::macos_overlay_cursor_uses_render_rects(
		OverlayMode::Frozen,
		true,
	));
}

#[cfg(target_os = "macos")]
#[test]
fn macos_cursor_object_maps_crosshair_icon() {
	let actual = overlay::macos_cursor_object_for_icon(CursorIcon::Crosshair) as usize;
	let expected: *mut Object = unsafe { objc::msg_send![objc::class!(NSCursor), crosshairCursor] };

	assert_eq!(actual, expected as usize);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_cursor_icon_defaults_without_active_rect_entries() {
	assert_eq!(
		overlay::macos_cursor_icon_for_current_pointer(
			None,
			Some(Pos2::new(150.0, 180.0)),
			Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0))),
		),
		Some(CursorIcon::Default)
	);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_cursor_icon_skips_windows_outside_pointer_bounds() {
	assert_eq!(
		overlay::macos_cursor_icon_for_current_pointer(
			None,
			Some(Pos2::new(450.0, 180.0)),
			Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0))),
		),
		None
	);
}
