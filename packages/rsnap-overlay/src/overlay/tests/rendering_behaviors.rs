use std::slice;
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::Instant;

use egui::Id;
use egui::LayerId;
use egui::Order;
use egui::Ui;
use image::RgbaImage;
#[cfg(target_os = "macos")]
use objc::runtime::Object;
use winit::window::CursorIcon;

use crate::OverlayControl;
#[cfg(target_os = "macos")]
use crate::overlay::WindowCaptureAlphaMode;
use crate::overlay::session_state::FrozenAnnotationStyleCapsulePlacement;
use crate::overlay::tests::{
	self, Duration, ElementState, FrozenCaptureSource, FrozenSelectionDragState,
	FrozenToolbarState, FrozenToolbarTool, GlobalPoint, HUD_LOUPE_STRIP_GAP_POINTS, HudTheme,
	MonitorRect, MonitorRectPoints, MouseButton, OverlayMode, OverlaySession, OverlayState,
	PngAction, Pos2, Rect, RectPoints, Rgba, SELECTION_SIZE_BADGE_GAP_PX,
	SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX, SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX, ScrollSession,
	SelectionDashedBorderCache, SelectionFlowGeometryCache, SelectionSizeBadgeTarget,
	TOOLBAR_CAPTURE_GAP_PX, TOOLBAR_SCREEN_MARGIN_PX, ToolbarPlacement, Vec2, WindowRenderer,
	overlay,
};
use crate::overlay::{
	FROZEN_TEXT_CARET_BLINK_PERIOD_SECS, FROZEN_TEXT_FONT_SIZE_POINTS, FontId,
	FrozenAnnotationColor, FrozenEditKind, FrozenSelectionCorner, FrozenSelectionInteractionKind,
	FrozenTextAnnotation, FrozenTextEditState,
};
#[cfg(target_os = "macos")]
use crate::state::MonitorImageSnapshot;
use crate::worker::{WorkerErrorSource, WorkerResponse};

fn test_mosaic_source_image() -> RgbaImage {
	RgbaImage::from_fn(8, 8, |x, y| {
		Rgba([(x * 17) as u8, (y * 23) as u8, ((x + y) * 11) as u8, 255])
	})
}

fn average_patch_color(image: &RgbaImage, x: u32, y: u32, width: u32, height: u32) -> Rgba<u8> {
	let mut sum = [0_u64; 4];
	let mut samples = 0_u64;

	for py in y..y.saturating_add(height) {
		for px in x..x.saturating_add(width) {
			let pixel = image.get_pixel(px, py);

			sum[0] += u64::from(pixel[0]);
			sum[1] += u64::from(pixel[1]);
			sum[2] += u64::from(pixel[2]);
			sum[3] += u64::from(pixel[3]);
			samples += 1;
		}
	}

	Rgba([
		(sum[0] / samples) as u8,
		(sum[1] / samples) as u8,
		(sum[2] / samples) as u8,
		(sum[3] / samples) as u8,
	])
}

#[cfg(target_os = "macos")]
#[test]
fn pending_freeze_capture_dispatches_even_with_seeded_preview() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.pending_freeze_capture = Some(monitor);

	assert!(session.should_dispatch_pending_freeze_capture(monitor));
}

#[cfg(target_os = "macos")]
#[test]
fn snapshot_background_capture_finishes_frozen_transition_immediately() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let frozen_image = tests::test_frozen_image();
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: Instant::now(),
		monitor,
		image: Arc::new(frozen_image.clone()),
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.pending_freeze_capture = Some(monitor);
	session.pending_window_freeze_capture = Some(crate::overlay::WindowFreezeCaptureTarget {
		monitor,
		window_id: 11,
		rect: capture_rect,
	});

	assert!(session.maybe_finish_frozen_capture_from_snapshot(
		monitor,
		session.pending_window_freeze_capture,
		None,
		Some(snapshot),
		"live_stream_snapshot",
	));
	assert!(session.authoritative_frozen_capture_ready);
	assert!(session.pending_freeze_capture.is_none());
	assert!(session.pending_window_freeze_capture.is_none());
	assert_eq!(session.state.frozen_image.as_ref(), Some(&frozen_image));
	assert!(session.toolbar_state.final_capture_ready);
}

#[cfg(target_os = "macos")]
#[test]
fn snapshot_matte_window_capture_keeps_authoritative_handoff_pending() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: Instant::now(),
		monitor,
		image: Arc::new(tests::test_frozen_image()),
	});
	let window_target =
		crate::overlay::WindowFreezeCaptureTarget { monitor, window_id: 11, rect: capture_rect };
	let mut session = OverlaySession::new();

	session.config.window_capture_alpha_mode = WindowCaptureAlphaMode::MatteDark;

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.pending_freeze_capture = Some(monitor);
	session.pending_window_freeze_capture = Some(window_target);

	assert!(!session.maybe_finish_frozen_capture_from_snapshot(
		monitor,
		Some(window_target),
		None,
		Some(snapshot),
		"live_stream_snapshot",
	));
	assert!(!session.authoritative_frozen_capture_ready);
	assert_eq!(session.pending_freeze_capture, Some(monitor));
	assert_eq!(session.pending_window_freeze_capture, Some(window_target));
	assert!(session.state.frozen_image.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn stale_snapshot_does_not_finish_frozen_transition_immediately() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: Instant::now()
			- crate::live_frame_stream_macos::STREAM_REGION_FRAME_MAX_AGE
			- Duration::from_millis(1),
		monitor,
		image: Arc::new(tests::test_frozen_image()),
	});
	let window_target =
		crate::overlay::WindowFreezeCaptureTarget { monitor, window_id: 11, rect: capture_rect };
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.pending_freeze_capture = Some(monitor);
	session.pending_window_freeze_capture = Some(window_target);

	assert!(!session.maybe_finish_frozen_capture_from_snapshot(
		monitor,
		Some(window_target),
		None,
		Some(snapshot),
		"live_stream_snapshot",
	));
	assert!(!session.authoritative_frozen_capture_ready);
	assert_eq!(session.pending_freeze_capture, Some(monitor));
	assert_eq!(session.pending_window_freeze_capture, Some(window_target));
	assert!(session.state.frozen_image.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn snapshot_seeded_preview_keeps_authoritative_handoff_pending() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let frozen_image = tests::test_frozen_image();
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: Instant::now(),
		monitor,
		image: Arc::new(frozen_image.clone()),
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.pending_freeze_capture = Some(monitor);

	assert!(session.maybe_seed_frozen_capture_preview_from_snapshot(
		monitor,
		None,
		Some(snapshot),
		"live_stream_snapshot_seeded_unverified",
	));
	assert!(!session.authoritative_frozen_capture_ready);
	assert_eq!(session.pending_freeze_capture, Some(monitor));
	assert_eq!(session.state.frozen_image.as_ref(), Some(&frozen_image));
	assert!(!session.toolbar_state.final_capture_ready);
}

#[cfg(target_os = "macos")]
#[test]
fn snapshot_seeded_preview_makes_toolbar_eligible_before_final_capture_ready() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: Instant::now(),
		monitor,
		image: Arc::new(tests::test_frozen_image()),
	});
	let mut session = OverlaySession::new();

	session.toolbar_state.visible = true;

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.pending_freeze_capture = Some(monitor);

	assert!(session.maybe_seed_frozen_capture_preview_from_snapshot(
		monitor,
		None,
		Some(snapshot),
		"live_stream_snapshot_seeded_unverified",
	));
	assert!(session.frozen_preview_visible());
	assert!(!session.authoritative_frozen_capture_ready);
	assert!(session.startup_aux_window_creation_pending);
}

#[cfg(target_os = "macos")]
#[test]
fn stale_snapshot_does_not_seed_frozen_preview() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let snapshot = Arc::new(MonitorImageSnapshot {
		captured_at: Instant::now()
			- crate::live_frame_stream_macos::STREAM_REGION_FRAME_MAX_AGE
			- Duration::from_millis(1),
		monitor,
		image: Arc::new(tests::test_frozen_image()),
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.pending_freeze_capture = Some(monitor);

	assert!(!session.maybe_seed_frozen_capture_preview_from_snapshot(
		monitor,
		None,
		Some(snapshot),
		"live_stream_snapshot_seeded_unverified",
	));
	assert_eq!(session.pending_freeze_capture, Some(monitor));
	assert!(!session.authoritative_frozen_capture_ready);
	assert!(session.state.frozen_image.is_none());
	assert!(!session.toolbar_state.final_capture_ready);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn pending_freeze_capture_waits_for_empty_frozen_image_off_macos() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.pending_freeze_capture = Some(monitor);

	assert!(!session.should_dispatch_pending_freeze_capture(monitor));
}

#[test]
fn frozen_final_capture_ready_requires_no_pending_or_inflight_capture() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	assert!(!session.frozen_final_capture_ready());

	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.authoritative_frozen_capture_ready = true;

	assert!(session.frozen_final_capture_ready());

	session.pending_freeze_capture = Some(monitor);

	assert!(!session.frozen_final_capture_ready());

	session.pending_freeze_capture = None;
	session.inflight_freeze_capture = Some(monitor);

	assert!(!session.frozen_final_capture_ready());
}

#[test]
fn frozen_preview_does_not_become_final_ready_when_capture_tracking_clears_without_success() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 220, 180);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.inflight_freeze_capture = Some(monitor);

	assert!(!session.frozen_final_capture_ready());
	assert!(!session.scroll_capture_selection_is_ready());

	// Emulate a preview-first failure where the authoritative capture tracking clears.
	session.inflight_freeze_capture = None;

	assert!(!session.frozen_final_capture_ready());
	assert!(!session.scroll_capture_selection_is_ready());

	session.begin_png_action(PngAction::Copy);

	assert_eq!(session.pending_png_action, None);
	assert!(session.pending_encode_png.is_none());
	assert_eq!(session.state.error_message.as_deref(), Some("Preparing capture..."));
}

#[test]
fn unrelated_worker_errors_do_not_clear_pending_freeze_capture_state() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.pending_freeze_capture = Some(monitor);
	session.pending_freeze_capture_armed = true;
	session.pending_window_freeze_capture = Some(crate::overlay::WindowFreezeCaptureTarget {
		monitor,
		window_id: 42,
		rect: RectPoints::new(10, 20, 30, 40),
	});

	let control = session.maybe_tick_worker_response_limiter(WorkerResponse::Error {
		source: WorkerErrorSource::RefreshWindowList,
		message: String::from("window refresh failed"),
	});

	assert!(matches!(control, OverlayControl::Continue));
	assert_eq!(session.pending_freeze_capture, Some(monitor));
	assert!(session.pending_freeze_capture_armed);
	assert!(session.inflight_freeze_capture.is_none());
	assert!(session.pending_window_freeze_capture.is_some());
	assert_eq!(session.state.error_message.as_deref(), Some("window refresh failed"));
}

#[test]
fn frozen_selection_drag_starts_only_for_drag_region_inside_capture_rect() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
fn frozen_mosaic_drag_waits_for_final_capture_ready() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let original = test_mosaic_source_image();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, original.clone());

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	assert!(!session.frozen_final_capture_ready());
	assert!(!session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
	assert!(!session.commit_frozen_mosaic_drag());
	assert!(!session.perform_frozen_undo());
	assert!(!session.perform_frozen_redo());
	assert_eq!(session.state.frozen_mosaic_preview_rect, None);
	assert_eq!(session.state.frozen_image.as_ref(), Some(&original));

	session.authoritative_frozen_capture_ready = true;

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
}

#[test]
fn frozen_mosaic_drag_updates_preview_rect_inside_capture_bounds() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_mosaic_source_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(2, 2, 4, 4));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;
	session.authoritative_frozen_capture_ready = true;

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(3, 3)));
	assert!(session.update_frozen_mosaic_drag_rect(GlobalPoint::new(30, 30)));
	assert_eq!(session.state.frozen_mosaic_preview_rect, Some(RectPoints::new(3, 3, 3, 3)));
}

#[test]
fn frozen_mosaic_commit_round_trips_through_undo_and_redo() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let original = test_mosaic_source_image();
	let expected_fill = average_patch_color(&original, 1, 1, 4, 4);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, original.clone());

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;
	session.authoritative_frozen_capture_ready = true;

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
	assert!(session.update_frozen_mosaic_drag_rect(GlobalPoint::new(4, 4)));
	assert!(session.commit_frozen_mosaic_drag());

	let edited =
		session.state.frozen_image.clone().expect("mosaic commit should retain the frozen image");

	assert_eq!(edited.get_pixel(2, 2), &expected_fill);
	assert_eq!(edited.get_pixel(4, 4), &expected_fill);
	assert_ne!(edited, original);
	assert_eq!(session.state.frozen_mosaic_preview_rect, None);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
	assert!(session.perform_frozen_undo());
	assert_eq!(session.state.frozen_image.as_ref(), Some(&original));
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.state.frozen_image.as_ref(), Some(&edited));
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn frozen_arrow_drag_commits_without_final_capture_and_round_trips_undo_redo() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_mosaic_source_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Arrow;

	assert!(!session.frozen_final_capture_ready());
	assert!(session.begin_frozen_arrow_drag(GlobalPoint::new(1, 1)));
	assert_eq!(
		session.active_frozen_arrow_preview().map(|annotation| (annotation.start, annotation.end)),
		Some((Pos2::new(1.0, 1.0), Pos2::new(1.0, 1.0)))
	);
	assert!(session.update_frozen_arrow_drag(GlobalPoint::new(7, 1)));
	assert_eq!(
		session.active_frozen_arrow_preview().map(|annotation| (annotation.start, annotation.end)),
		Some((Pos2::new(1.0, 1.0), Pos2::new(7.0, 1.0)))
	);
	assert!(session.commit_frozen_arrow_drag());
	assert_eq!(session.frozen_arrow_annotations.len(), 1);
	assert_eq!(session.frozen_arrow_annotations[0].start, Pos2::new(1.0, 1.0));
	assert_eq!(session.frozen_arrow_annotations[0].end, Pos2::new(7.0, 1.0));
	assert_eq!(session.frozen_edit_undo_stack.last(), Some(&FrozenEditKind::ArrowAnnotation));
	assert!(session.active_frozen_arrow_preview().is_none());
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_arrow_annotations.is_empty());
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_arrow_annotations.len(), 1);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn frozen_spotlight_drag_clamps_preview_and_round_trips_undo_redo() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_mosaic_source_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(2, 2, 4, 4));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Spotlight;

	assert!(!session.frozen_final_capture_ready());
	assert!(session.begin_frozen_spotlight_drag(GlobalPoint::new(3, 3)));
	assert_eq!(session.frozen_spotlight_preview_rect, Some(RectPoints::new(3, 3, 1, 1)));
	assert!(session.update_frozen_spotlight_drag_rect(GlobalPoint::new(30, 30)));
	assert_eq!(session.frozen_spotlight_preview_rect, Some(RectPoints::new(3, 3, 3, 3)));
	assert!(session.commit_frozen_spotlight_drag());
	assert_eq!(session.frozen_spotlight_annotations.len(), 1);
	assert_eq!(session.frozen_spotlight_annotations[0].rect, RectPoints::new(3, 3, 3, 3));
	assert_eq!(session.frozen_edit_undo_stack.last(), Some(&FrozenEditKind::SpotlightAnnotation));
	assert_eq!(session.frozen_spotlight_preview_rect, None);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_spotlight_annotations.is_empty());
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_spotlight_annotations.len(), 1);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn frozen_selection_drag_updates_capture_rect_and_toolbar_position() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	session.seed_frozen_toolbar_default_position(monitor, capture_rect);

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(150, 180)));
	assert!(session.update_frozen_selection_drag_rect(GlobalPoint::new(300, 360)));

	let expected_rect = RectPoints::new(250, 300, 200, 240);
	let expected_toolbar_pos =
		session.frozen_toolbar_default_position_for_capture_rect(monitor, expected_rect);

	assert_eq!(session.state.frozen_capture_rect, Some(expected_rect));
	assert_eq!(session.toolbar_state.floating_position, Some(expected_toolbar_pos));
}

#[test]
fn frozen_selection_drag_hides_auxiliary_windows_while_active() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
fn frozen_selection_drag_skips_toolbar_focus_even_before_first_show() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.toolbar_state.visible = true;

	assert!(!session.toolbar_window_visible);
	assert!(!session.skip_toolbar_focus_on_next_show);
	assert!(session.should_focus_frozen_toolbar_window_on_show());
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
	assert!(session.preserve_frontmost_on_next_toolbar_show);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_selection_drag_does_not_rearm_initial_frontmost_restore() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.preserve_frontmost_on_next_toolbar_show = false;

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(250, 240)));
	assert!(session.skip_toolbar_focus_on_next_show);
	assert!(!session.preserve_frontmost_on_next_toolbar_show);
}

#[test]
fn frozen_selection_resize_updates_capture_rect_and_toolbar_position() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	session.seed_frozen_toolbar_default_position(monitor, capture_rect);

	assert!(session.begin_frozen_selection_drag(GlobalPoint::new(95, 115)));
	assert!(session.update_frozen_selection_drag_rect(GlobalPoint::new(160, 190)));

	let expected_rect = RectPoints::new(165, 195, 135, 165);
	let expected_toolbar_pos =
		session.frozen_toolbar_default_position_for_capture_rect(monitor, expected_rect);

	assert_eq!(session.state.frozen_capture_rect, Some(expected_rect));
	assert_eq!(session.toolbar_state.floating_position, Some(expected_toolbar_pos));
}

#[test]
fn toolbar_position_update_queues_pending_move_without_window() {
	let monitor = tests::test_monitor();
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	let mut session = OverlaySession::new();

	session.toolbar_inner_size_points = Some((460, 54));

	assert!(session.update_toolbar_outer_position(monitor, Pos2::new(120.0, 160.0)));
	assert_eq!(session.toolbar_state.floating_position, Some(Pos2::new(120.0, 160.0)));

	#[cfg(target_os = "macos")]
	let expected_outer = GlobalPoint::new(120, (160.0 - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let expected_outer = GlobalPoint::new(120, 160);

	assert_eq!(session.toolbar_outer_pos, Some(expected_outer));
	assert_eq!(session.pending_toolbar_outer_pos, Some(expected_outer));
}

#[test]
fn toolbar_window_position_sync_updates_runtime_state_without_requeueing_in_bounds_move() {
	let monitor = tests::test_monitor();
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	let mut session = OverlaySession::new();

	session.toolbar_inner_size_points = Some((460, 54));

	assert!(session.sync_toolbar_outer_position_from_window(monitor, GlobalPoint::new(120, 160)));

	#[cfg(target_os = "macos")]
	let expected_anchor = Pos2::new(120.0, 160.0 + primary_origin.y);
	#[cfg(not(target_os = "macos"))]
	let expected_anchor = Pos2::new(120.0, 160.0);

	assert_eq!(session.toolbar_state.floating_position, Some(expected_anchor));
	assert_eq!(session.toolbar_outer_pos, Some(GlobalPoint::new(120, 160)));
	assert_eq!(session.pending_toolbar_outer_pos, None);
}

#[test]
fn toolbar_position_update_near_edge_clamps_with_runtime_positioning_geometry() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();
	let desired = Pos2::new(900.0, 760.0);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let positioning_size = WindowRenderer::frozen_toolbar_positioning_size(&session.toolbar_state);
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	#[cfg(not(target_os = "macos"))]
	let startup_window_size = Vec2::new(startup_size.x, startup_size.y);

	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	let expected_toolbar_size = positioning_size;
	let expected = WindowRenderer::clamp_toolbar_position(
		screen_rect,
		expected_toolbar_size,
		desired,
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);

	#[cfg(not(target_os = "macos"))]
	assert_ne!(
		expected,
		WindowRenderer::clamp_toolbar_position(
			screen_rect,
			startup_window_size,
			desired,
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		)
	);
	assert!(session.update_toolbar_outer_position(monitor, desired));
	assert_eq!(session.toolbar_state.floating_position, Some(expected));

	#[cfg(target_os = "macos")]
	let expected_outer =
		GlobalPoint::new(expected.x.round() as i32, (expected.y - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let expected_outer = GlobalPoint::new(expected.x.round() as i32, expected.y.round() as i32);

	assert_eq!(session.toolbar_outer_pos, Some(expected_outer));
}

#[test]
fn toolbar_window_position_sync_near_edge_clamps_with_runtime_positioning_geometry() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();
	let desired_outer = GlobalPoint::new(900, 760);
	let desired_local = Pos2::new(desired_outer.x as f32, desired_outer.y as f32);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let positioning_size = WindowRenderer::frozen_toolbar_positioning_size(&session.toolbar_state);
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	#[cfg(not(target_os = "macos"))]
	let startup_window_size = Vec2::new(startup_size.x, startup_size.y);

	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	let expected_toolbar_size = positioning_size;
	let expected = WindowRenderer::clamp_toolbar_position(
		screen_rect,
		expected_toolbar_size,
		desired_local,
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);

	#[cfg(not(target_os = "macos"))]
	assert_ne!(
		expected,
		WindowRenderer::clamp_toolbar_position(
			screen_rect,
			startup_window_size,
			desired_local,
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		)
	);
	assert!(session.sync_toolbar_outer_position_from_window(monitor, desired_outer));
	assert_eq!(session.toolbar_state.floating_position, Some(expected));

	#[cfg(target_os = "macos")]
	let expected_outer =
		GlobalPoint::new(expected.x.round() as i32, (expected.y - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let expected_outer = GlobalPoint::new(expected.x.round() as i32, expected.y.round() as i32);

	assert_eq!(session.toolbar_outer_pos, Some(expected_outer));
	assert_eq!(
		session.pending_toolbar_outer_pos,
		(session.toolbar_outer_pos != Some(desired_outer)).then_some(expected_outer)
	);
}

#[test]
fn toolbar_position_update_clamps_scroll_mode_pen_from_primary_anchor_width() {
	let monitor = tests::test_monitor();
	let desired = Pos2::new(900.0, 160.0);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let primary_toolbar_state = FrozenToolbarState {
		selected_tool: FrozenToolbarTool::Pen,
		scroll_capture_active: true,
		..FrozenToolbarState::default()
	};
	let primary_size = WindowRenderer::frozen_toolbar_primary_size(&primary_toolbar_state);
	let full_toolbar_size = WindowRenderer::frozen_toolbar_size(&primary_toolbar_state);
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	let mut session = OverlaySession::new();

	assert!(
		full_toolbar_size.x > primary_size.x,
		"regression setup requires the secondary style capsule to be wider than the primary pill",
	);

	session.toolbar_state = primary_toolbar_state;
	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	let expected = WindowRenderer::clamp_toolbar_position(
		screen_rect,
		primary_size,
		desired,
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);
	let union_clamped = WindowRenderer::clamp_toolbar_position(
		screen_rect,
		full_toolbar_size,
		desired,
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);

	assert_ne!(expected, union_clamped);
	assert!(session.update_toolbar_outer_position(monitor, desired));
	assert_eq!(session.toolbar_state.floating_position, Some(expected));

	#[cfg(target_os = "macos")]
	let expected_outer =
		GlobalPoint::new(expected.x.round() as i32, (expected.y - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let expected_outer = GlobalPoint::new(expected.x.round() as i32, expected.y.round() as i32);

	assert_eq!(session.toolbar_outer_pos, Some(expected_outer));
}

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
fn toolbar_event_outer_position_prefers_window_position_over_cached_position() {
	let monitor = tests::test_monitor();
	let window_outer_pos = Some(GlobalPoint::new(220, 260));
	let cached_outer_pos = Some(GlobalPoint::new(340, 420));
	let floating_position = Some(Pos2::new(80.0, 90.0));

	assert_eq!(
		OverlaySession::toolbar_event_outer_position_from_sources(
			monitor,
			window_outer_pos,
			cached_outer_pos,
			floating_position,
		),
		window_outer_pos
	);
}

#[test]
fn toolbar_event_outer_position_falls_back_to_cached_position() {
	let monitor = tests::test_monitor();
	let cached_outer_pos = Some(GlobalPoint::new(340, 420));
	let floating_position = Some(Pos2::new(80.0, 90.0));

	assert_eq!(
		OverlaySession::toolbar_event_outer_position_from_sources(
			monitor,
			None,
			cached_outer_pos,
			floating_position,
		),
		cached_outer_pos
	);
}

#[test]
fn toolbar_event_outer_position_falls_back_to_floating_position() {
	let monitor = tests::test_monitor();
	let floating_position = Some(Pos2::new(80.4, 90.6));
	#[cfg(target_os = "macos")]
	let primary_origin = overlay::frozen_toolbar_window_primary_origin();
	#[cfg(target_os = "macos")]
	let expected = GlobalPoint::new(80, (90.6 - primary_origin.y).round() as i32);
	#[cfg(not(target_os = "macos"))]
	let expected = GlobalPoint::new(80, 91);

	assert_eq!(
		OverlaySession::toolbar_event_outer_position_from_sources(
			monitor,
			None,
			None,
			floating_position,
		),
		Some(expected)
	);
}

#[test]
fn frozen_selection_resize_preserves_handle_press_offset() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
	session.state.finish_freeze(monitor, image);

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
fn auto_center_frozen_capture_rect_recenters_detected_content() {
	let monitor = tests::test_monitor_with_scale(80, 60, 2_000);
	let capture_rect = RectPoints::new(20, 16, 40, 24);
	let mut image = RgbaImage::from_pixel(160, 120, Rgba([14, 16, 20, 255]));
	let mut session = OverlaySession::new();

	for y in 40..52 {
		for x in 52..68 {
			image.put_pixel(x, y, Rgba([228, 232, 240, 255]));
		}
	}

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, image);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	session.seed_frozen_toolbar_default_position(monitor, capture_rect);

	assert!(session.auto_center_frozen_capture_rect());

	let expected_rect = RectPoints::new(10, 11, 40, 24);
	let expected_toolbar_pos =
		session.frozen_toolbar_default_position_for_capture_rect(monitor, expected_rect);

	assert_eq!(session.state.frozen_capture_rect, Some(expected_rect));
	assert_eq!(session.toolbar_state.floating_position, Some(expected_toolbar_pos));
}

#[test]
fn auto_center_frozen_capture_rect_works_outside_pointer_mode() {
	let monitor = tests::test_monitor_with_scale(80, 60, 2_000);
	let capture_rect = RectPoints::new(20, 16, 40, 24);
	let mut image = RgbaImage::from_pixel(160, 120, Rgba([14, 16, 20, 255]));
	let mut session = OverlaySession::new();

	for y in 40..52 {
		for x in 52..68 {
			image.put_pixel(x, y, Rgba([228, 232, 240, 255]));
		}
	}

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, image);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	assert!(session.frozen_auto_center_available());
	assert!(session.auto_center_frozen_capture_rect());
	assert_eq!(session.state.frozen_capture_rect, Some(RectPoints::new(10, 11, 40, 24)));
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
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.state.cursor = Some(GlobalPoint::new(95, 115));

	assert_eq!(session.frozen_selection_cursor_icon_for_monitor(monitor), CursorIcon::NwseResize);

	session.state.cursor = Some(GlobalPoint::new(150, 180));

	assert_eq!(session.frozen_selection_cursor_icon_for_monitor(monitor), CursorIcon::Grab);
}

#[test]
fn live_overlay_cursor_icon_uses_crosshair() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;

	assert_eq!(session.overlay_cursor_icon_for_monitor(monitor), CursorIcon::Crosshair);
}

#[test]
fn frozen_text_edit_caret_rect_starts_at_anchor_when_text_is_empty() {
	let ctx = tests::test_egui_context();
	let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("text-caret-empty")));
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let caret_rect = WindowRenderer::frozen_text_edit_caret_rect(&painter, anchor, "", &font_id);

	assert!((caret_rect.min.x - anchor.x).abs() <= f32::EPSILON);
	assert!((caret_rect.min.y - anchor.y).abs() <= f32::EPSILON);
	assert!(caret_rect.height() >= FROZEN_TEXT_FONT_SIZE_POINTS);
}

#[test]
fn frozen_text_edit_caret_rect_tracks_multiline_text_end() {
	let ctx = tests::test_egui_context();
	let painter =
		ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("text-caret-multiline")));
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let caret_rect =
		WindowRenderer::frozen_text_edit_caret_rect(&painter, anchor, "A\nB", &font_id);

	assert!(caret_rect.min.y > anchor.y);
	assert!(caret_rect.min.x > anchor.x);
}

#[test]
fn frozen_text_edit_caret_rect_tracks_explicit_preedit_cursor_position() {
	let ctx = tests::test_egui_context();
	let painter =
		ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("text-caret-preedit-cursor")));
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let caret_rect = WindowRenderer::frozen_text_edit_caret_rect_at_char_index(
		&painter, anchor, "ABCD", &font_id, 2,
	);
	let end_rect = WindowRenderer::frozen_text_edit_caret_rect(&painter, anchor, "ABCD", &font_id);

	assert!(caret_rect.min.x > anchor.x);
	assert!(caret_rect.min.x < end_rect.min.x);
	assert!((caret_rect.min.y - anchor.y).abs() <= f32::EPSILON);
}

#[test]
fn frozen_text_placeholder_fill_tracks_selected_text_color() {
	let blue =
		WindowRenderer::frozen_text_placeholder_fill(FrozenAnnotationColor::Blue, HudTheme::Dark);
	let red =
		WindowRenderer::frozen_text_placeholder_fill(FrozenAnnotationColor::Red, HudTheme::Dark);

	assert!(blue.b() > blue.r());
	assert!(red.r() > red.b());
	assert!(blue.a() < 255);
	assert!(red.a() < 255);
}

#[test]
fn frozen_text_edit_interaction_rect_uses_placeholder_bounds_when_empty() {
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let rect = WindowRenderer::frozen_text_edit_interaction_rect(anchor, "", &font_id);

	assert!(rect.contains(anchor));
	assert!(rect.width() > FROZEN_TEXT_FONT_SIZE_POINTS);
	assert!(rect.height() >= FROZEN_TEXT_FONT_SIZE_POINTS);
}

#[test]
fn frozen_text_edit_interaction_rect_covers_full_width_text_layout() {
	let ctx = tests::test_egui_context();
	let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("text-hitbox-cjk")));
	let anchor = Pos2::new(140.0, 160.0);
	let font_id = FontId::proportional(FROZEN_TEXT_FONT_SIZE_POINTS);
	let rect = WindowRenderer::frozen_text_edit_interaction_rect(anchor, "你好世界", &font_id);
	let caret_rect =
		WindowRenderer::frozen_text_edit_caret_rect(&painter, anchor, "你好世界", &font_id);

	assert!(rect.contains(caret_rect.min));
	assert!(rect.contains(Pos2::new(caret_rect.max.x, caret_rect.min.y)));
}

#[test]
fn frozen_committed_text_annotations_are_clipped_to_capture_rect() {
	let ctx = tests::test_egui_context();
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 120.0));
	let capture_rect_points = RectPoints::new(40, 20, 80, 40);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: screen_rect.width() as u32,
		height: screen_rect.height() as u32,
		scale_factor_x1000: 1_000,
	};
	let style = OverlaySession::new().toolbar_state.text_style;
	let annotation = FrozenTextAnnotation {
		anchor: Pos2::new(capture_rect.max.x - 2.0, capture_rect.min.y + 4.0),
		text: String::from("edge"),
		style,
	};
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(capture_rect_points);

	let empty_output = ctx.run_ui(
		egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|_ui: &mut Ui| {
			assert!(WindowRenderer::render_frozen_capture_affordance(
				&ctx,
				&state,
				monitor,
				screen_rect,
				HudTheme::Dark,
				false,
				FrozenCaptureSource::None,
				None,
				&[],
				None,
				&[],
				None,
				&[],
				None,
				&[],
				None,
				style,
				false,
				true,
				1.0,
				&mut selection_flow_geometry_cache,
				&mut selection_dashed_border_cache,
			));
		},
	);
	let clipped_shape_count_without_text =
		empty_output.shapes.iter().filter(|shape| shape.clip_rect == capture_rect).count();
	let full_output = ctx.run_ui(
		egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|_ui: &mut Ui| {
			assert!(WindowRenderer::render_frozen_capture_affordance(
				&ctx,
				&state,
				monitor,
				screen_rect,
				HudTheme::Dark,
				false,
				FrozenCaptureSource::None,
				None,
				&[FrozenEditKind::TextAnnotation],
				None,
				&[],
				None,
				&[],
				None,
				slice::from_ref(&annotation),
				None,
				style,
				false,
				true,
				1.0,
				&mut selection_flow_geometry_cache,
				&mut selection_dashed_border_cache,
			));
		},
	);
	let clipped_shape_count_with_text =
		full_output.shapes.iter().filter(|shape| shape.clip_rect == capture_rect).count();

	assert!(
		clipped_shape_count_with_text > clipped_shape_count_without_text,
		"committed text should add shapes clipped to the frozen capture rect",
	);
}

#[test]
fn frozen_active_text_preview_is_clipped_to_capture_rect() {
	let ctx = tests::test_egui_context();
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 120.0));
	let capture_rect_points = RectPoints::new(40, 20, 80, 40);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: screen_rect.width() as u32,
		height: screen_rect.height() as u32,
		scale_factor_x1000: 1_000,
	};
	let style = OverlaySession::new().toolbar_state.text_style;
	let mut text_edit =
		FrozenTextEditState::new(Pos2::new(capture_rect.max.x - 2.0, capture_rect.min.y + 4.0));

	text_edit.text = String::from("editing");

	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(capture_rect_points);

	let empty_output = ctx.run_ui(
		egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|_ui: &mut Ui| {
			assert!(WindowRenderer::render_frozen_capture_affordance(
				&ctx,
				&state,
				monitor,
				screen_rect,
				HudTheme::Dark,
				false,
				FrozenCaptureSource::None,
				None,
				&[],
				None,
				&[],
				None,
				&[],
				None,
				&[],
				None,
				style,
				false,
				true,
				1.0,
				&mut selection_flow_geometry_cache,
				&mut selection_dashed_border_cache,
			));
		},
	);
	let clipped_shape_count_without_preview =
		empty_output.shapes.iter().filter(|shape| shape.clip_rect == capture_rect).count();
	let preview_output = ctx.run_ui(
		egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|_ui: &mut Ui| {
			assert!(WindowRenderer::render_frozen_capture_affordance(
				&ctx,
				&state,
				monitor,
				screen_rect,
				HudTheme::Dark,
				false,
				FrozenCaptureSource::None,
				None,
				&[],
				None,
				&[],
				None,
				&[],
				None,
				&[],
				Some(&text_edit),
				style,
				false,
				true,
				1.0,
				&mut selection_flow_geometry_cache,
				&mut selection_dashed_border_cache,
			));
		},
	);
	let clipped_shape_count_with_preview =
		preview_output.shapes.iter().filter(|shape| shape.clip_rect == capture_rect).count();

	assert!(
		clipped_shape_count_with_preview > clipped_shape_count_without_preview,
		"active text preview should add shapes clipped to the frozen capture rect",
	);
}

#[test]
fn frozen_text_caret_visible_blinks_on_half_periods() {
	assert!(WindowRenderer::frozen_text_caret_visible(0.0));
	assert!(WindowRenderer::frozen_text_caret_visible(FROZEN_TEXT_CARET_BLINK_PERIOD_SECS * 0.49,));
	assert!(
		!WindowRenderer::frozen_text_caret_visible(FROZEN_TEXT_CARET_BLINK_PERIOD_SECS * 0.51,)
	);
}

#[test]
fn frozen_toolbar_primary_size_stays_stable_when_annotation_style_capsule_appears() {
	let mut toolbar_state = FrozenToolbarState::default();
	let base_primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let base_window_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);

	toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	let pen_primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let pen_window_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);

	assert_eq!(pen_primary_size, base_primary_size);
	assert!(pen_window_size.x >= base_window_size.x);
	assert!(pen_window_size.y > base_window_size.y);

	toolbar_state.selected_tool = FrozenToolbarTool::Text;

	let text_primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let text_window_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);

	assert_eq!(text_primary_size, base_primary_size);
	assert!(text_window_size.x >= base_window_size.x);
	assert!(text_window_size.y > base_window_size.y);
	assert!(pen_window_size.y >= text_window_size.y);
}

#[test]
fn render_frozen_toolbar_ui_keeps_runtime_drag_when_pointer_snapshot_is_missing() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));

	session.toolbar_state.dragging = true;

	let toolbar_placement = session.config.toolbar_placement;
	let state = &session.state;
	let toolbar_state = &mut session.toolbar_state;
	let mut hud_pill = None;
	let _ = ctx.run_ui(
		egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|ui: &mut Ui| {
			WindowRenderer::render_frozen_toolbar_ui(
				ui.ctx(),
				state,
				monitor,
				HudTheme::Dark,
				toolbar_placement,
				false,
				false,
				1.0,
				0.0,
				0.0,
				Some(toolbar_state),
				None,
				&mut hud_pill,
			);
		},
	);

	assert!(hud_pill.is_some(), "toolbar should still render once readiness stabilizes");
	assert!(
		session.toolbar_state.dragging,
		"rendering without a pointer snapshot must not clear runtime-managed drag state"
	);
}

#[test]
fn frozen_base_toolbar_hud_pill_uses_half_height_corner_radius() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));

	let toolbar_placement = session.config.toolbar_placement;
	let state = &session.state;
	let toolbar_state = &mut session.toolbar_state;
	let mut hud_pill = None;
	let _ = ctx.run_ui(
		egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|ui: &mut Ui| {
			WindowRenderer::render_frozen_toolbar_ui(
				ui.ctx(),
				state,
				monitor,
				HudTheme::Dark,
				toolbar_placement,
				false,
				false,
				1.0,
				0.0,
				0.0,
				Some(toolbar_state),
				None,
				&mut hud_pill,
			);
		},
	);
	let hud_pill = hud_pill.expect("base toolbar should render after readiness stabilizes");

	assert_eq!(hud_pill.radius_points, (hud_pill.rect.height() * 0.5).round());
}

#[test]
fn frozen_annotation_toolbar_hud_pill_keeps_standard_corner_radius() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));

	let toolbar_placement = session.config.toolbar_placement;
	let state = &session.state;
	let toolbar_state = &mut session.toolbar_state;
	let mut hud_pill = None;
	let _ = ctx.run_ui(
		egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|ui: &mut Ui| {
			WindowRenderer::render_frozen_toolbar_ui(
				ui.ctx(),
				state,
				monitor,
				HudTheme::Dark,
				toolbar_placement,
				false,
				false,
				1.0,
				0.0,
				0.0,
				Some(toolbar_state),
				None,
				&mut hud_pill,
			);
		},
	);
	let hud_pill = hud_pill.expect("annotation toolbar should render after readiness stabilizes");

	assert_eq!(
		hud_pill.radius_points,
		f32::from(overlay::frozen_toolbar_corner_radius_u8(hud_pill.rect.height())),
	);
}

#[test]
fn frozen_annotation_toolbar_hud_pill_covers_full_toolbar_bounds() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));

	let toolbar_placement = session.config.toolbar_placement;
	let state = &session.state;
	let toolbar_state = &mut session.toolbar_state;
	let mut hud_pill = None;
	let _ = ctx.run_ui(
		egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|ui: &mut Ui| {
			WindowRenderer::render_frozen_toolbar_ui(
				ui.ctx(),
				state,
				monitor,
				HudTheme::Dark,
				toolbar_placement,
				false,
				false,
				1.0,
				0.0,
				0.0,
				Some(toolbar_state),
				None,
				&mut hud_pill,
			);
		},
	);
	let hud_pill = hud_pill.expect("annotation toolbar should render after readiness stabilizes");

	assert_eq!(hud_pill.rect.size(), WindowRenderer::frozen_toolbar_size(&session.toolbar_state));
}

#[cfg(target_os = "macos")]
#[test]
fn seeded_frozen_toolbar_default_slot_uses_positioning_window_size() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(760, 160, 160, 240);
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let mut session = OverlaySession::new();

	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	session.seed_frozen_toolbar_default_position(monitor, capture_rect);

	assert_eq!(
		session.toolbar_state.default_slot_position,
		session.toolbar_state.floating_position
	);
}

#[test]
fn frozen_selection_cursor_icon_tracks_active_resize_drag() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.frozen_selection_drag = FrozenSelectionDragState {
		active: true,
		interaction: FrozenSelectionInteractionKind::Resize(FrozenSelectionCorner::BottomRight),
		anchor_rect: capture_rect,
		pointer_offset_x: 0,
		pointer_offset_y: 0,
		press_cursor_x: 300,
		press_cursor_y: 360,
	};

	assert_eq!(session.frozen_selection_cursor_icon_for_monitor(monitor), CursorIcon::NwseResize);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_selection_cursor_rects_use_native_handle_hover_and_full_window_resize_drag() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);

	assert_eq!(
		overlay::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(95.0, 115.0)),
		Some(CursorIcon::NwseResize)
	);
	assert_eq!(
		overlay::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(305.0, 115.0)),
		Some(CursorIcon::NeswResize)
	);
	assert_eq!(
		overlay::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(150.0, 180.0)),
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
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
fn frozen_mosaic_cursor_rects_preserve_crosshair_hover_and_drag() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.authoritative_frozen_capture_ready = true;
	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);

	assert_eq!(
		overlay::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(150.0, 180.0)),
		Some(CursorIcon::Crosshair)
	);
	assert_eq!(overlay::overlay_cursor_rect_icon_at_point(&rects, Pos2::new(80.0, 100.0)), None);

	session.frozen_mosaic_drag.active = true;

	let rects = session.frozen_selection_cursor_rects_for_monitor(monitor);

	assert_eq!(rects.len(), 1);
	assert_eq!(rects[0].icon, CursorIcon::Crosshair);
	assert_eq!(rects[0].rect.min, Pos2::ZERO);
	assert_eq!(rects[0].rect.max, Pos2::new(monitor.width as f32, monitor.height as f32));
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

#[cfg(target_os = "macos")]
#[test]
fn frozen_selection_cursor_rects_match_resize_hit_test_for_tiny_overlapping_handles() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 8, 8);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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
		overlay::overlay_cursor_rect_icon_at_point(&rects, top_overlap),
		Some(CursorIcon::NeswResize)
	);
	assert_eq!(
		WindowRenderer::frozen_selection_resize_hit_test(capture_rect, top_overlap_midline_tie),
		Some(FrozenSelectionCorner::TopLeft)
	);
	assert_eq!(
		overlay::overlay_cursor_rect_icon_at_point(&rects, top_overlap_midline_tie),
		Some(CursorIcon::NwseResize)
	);
	assert_eq!(WindowRenderer::frozen_selection_resize_hit_test(capture_rect, center_inside), None);
	assert_eq!(
		overlay::overlay_cursor_rect_icon_at_point(&rects, center_inside),
		Some(CursorIcon::Grab)
	);
}

#[test]
fn frozen_selection_cursor_icon_tracks_active_move_drag() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

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

	assert_eq!(session.frozen_selection_cursor_icon_for_monitor(monitor), CursorIcon::Grabbing);
}

#[test]
fn frozen_toolbar_default_position_centers_on_capture_rect_midpoint() {
	let monitor = tests::test_monitor_with_scale(400, 300, 2_000);
	let capture_rect = RectPoints::new(150, 100, 100, 60);
	let session = OverlaySession::new();
	let toolbar_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let toolbar_pos =
		session.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);
	let toolbar_midpoint_x = toolbar_pos.x + toolbar_size.x * 0.5;
	let capture_midpoint_x = capture_rect.x as f32 + capture_rect.width as f32 * 0.5;

	assert_eq!(toolbar_midpoint_x, capture_midpoint_x);
}

#[test]
fn auto_center_frozen_capture_rect_noops_for_uniform_crop() {
	let monitor = tests::test_monitor_with_scale(80, 60, 1_000);
	let capture_rect = RectPoints::new(20, 16, 40, 24);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, RgbaImage::from_pixel(80, 60, Rgba([24, 24, 28, 255])));

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
fn scroll_capture_and_export_wait_for_authoritative_frozen_capture() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(100, 120, 220, 180);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.authoritative_frozen_capture_ready = true;
	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(session.scroll_capture_selection_is_ready());

	session.inflight_freeze_capture = Some(monitor);

	assert!(!session.scroll_capture_selection_is_ready());

	session.begin_png_action(PngAction::Copy);

	assert_eq!(session.pending_png_action, None);
	assert!(session.pending_encode_png.is_none());
	assert_eq!(session.state.error_message.as_deref(), Some("Preparing capture..."));
}

#[test]
fn frozen_toolbar_default_position_fits_below_capture_rect() {
	let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(50.0, 100.0), Vec2::new(300.0, 200.0));
	let toolbar_size = Vec2::new(460.0, 54.0);
	let pos = WindowRenderer::frozen_toolbar_default_window_pos(
		monitor,
		capture_rect,
		toolbar_size,
		toolbar_size,
		ToolbarPlacement::Bottom,
	);
	let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
		TOOLBAR_SCREEN_MARGIN_PX,
		(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(TOOLBAR_SCREEN_MARGIN_PX),
	);

	assert!((pos.x - expected_x).abs() < f32::EPSILON);
	assert_eq!(pos.y, capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX);
}

#[test]
fn frozen_toolbar_default_position_falls_inside_when_no_space_below_capture_rect() {
	let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 600.0));
	let toolbar_size = Vec2::new(460.0, 54.0);
	let capture_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 560.0));
	let pos = WindowRenderer::frozen_toolbar_default_window_pos(
		monitor,
		capture_rect,
		toolbar_size,
		toolbar_size,
		ToolbarPlacement::Bottom,
	);
	let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
		TOOLBAR_SCREEN_MARGIN_PX,
		(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(TOOLBAR_SCREEN_MARGIN_PX),
	);
	let expected_y = capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y;

	assert_eq!(pos.x, expected_x);
	assert_eq!(pos.y, capture_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - toolbar_size.y);
	assert_eq!(pos.y, expected_y);
}

#[test]
fn frozen_toolbar_default_window_position_clamps_using_primary_anchor_width() {
	let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(640.0, 120.0), Vec2::new(120.0, 220.0));
	let toolbar_primary_size = Vec2::new(268.0, 54.0);
	let secondary_union_size = Vec2::new(340.0, 82.0);
	let pos = WindowRenderer::frozen_toolbar_default_window_pos(
		monitor,
		capture_rect,
		toolbar_primary_size,
		toolbar_primary_size,
		ToolbarPlacement::Bottom,
	);
	let ideal_x = capture_rect.center().x - toolbar_primary_size.x / 2.0;
	let max_x = (monitor.max.x - toolbar_primary_size.x - TOOLBAR_SCREEN_MARGIN_PX)
		.max(TOOLBAR_SCREEN_MARGIN_PX);
	let secondary_union_max_x = (monitor.max.x - secondary_union_size.x - TOOLBAR_SCREEN_MARGIN_PX)
		.max(TOOLBAR_SCREEN_MARGIN_PX);

	assert!(ideal_x > max_x);
	assert_ne!(max_x, secondary_union_max_x);
	assert_eq!(pos.x, max_x);
	assert_eq!(pos.y, capture_rect.max.y + TOOLBAR_CAPTURE_GAP_PX);
}

#[test]
fn frozen_toolbar_top_default_position_fits_above_capture_rect() {
	let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(50.0, 180.0), Vec2::new(300.0, 200.0));
	let toolbar_size = Vec2::new(460.0, 54.0);
	let pos = WindowRenderer::frozen_toolbar_default_window_pos(
		monitor,
		capture_rect,
		toolbar_size,
		toolbar_size,
		ToolbarPlacement::Top,
	);
	let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
		TOOLBAR_SCREEN_MARGIN_PX,
		(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(TOOLBAR_SCREEN_MARGIN_PX),
	);

	assert_eq!(pos.x, expected_x);
	assert_eq!(pos.y, capture_rect.min.y - TOOLBAR_CAPTURE_GAP_PX - toolbar_size.y);
}

#[test]
fn frozen_toolbar_top_default_position_falls_inside_when_no_space_above_capture_rect() {
	let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(0.0, 20.0), Vec2::new(500.0, 400.0));
	let toolbar_size = Vec2::new(460.0, 54.0);
	let pos = WindowRenderer::frozen_toolbar_default_window_pos(
		monitor,
		capture_rect,
		toolbar_size,
		toolbar_size,
		ToolbarPlacement::Top,
	);
	let expected_x = (capture_rect.center().x - toolbar_size.x / 2.0).clamp(
		TOOLBAR_SCREEN_MARGIN_PX,
		(monitor.max.x - toolbar_size.x - TOOLBAR_SCREEN_MARGIN_PX).max(TOOLBAR_SCREEN_MARGIN_PX),
	);

	assert_eq!(pos.x, expected_x);
	assert_eq!(pos.y, capture_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX);
}

#[test]
fn selection_size_badge_rect_fits_below_capture_rect() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(120.0, 160.0), Vec2::new(320.0, 240.0));
	let badge_rect =
		WindowRenderer::selection_size_badge_rect(screen_rect, capture_rect, Vec2::new(92.0, 26.0));

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.min.y, capture_rect.max.y + SELECTION_SIZE_BADGE_GAP_PX);
}

#[test]
fn selection_size_badge_rect_falls_inside_when_no_space_below() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(120.0, 420.0), Vec2::new(320.0, 160.0));
	let badge_rect =
		WindowRenderer::selection_size_badge_rect(screen_rect, capture_rect, Vec2::new(92.0, 26.0));

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.max.y, capture_rect.max.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX);
	assert!(badge_rect.max.y <= screen_rect.max.y - SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX);
}

#[test]
fn selection_size_badge_rect_clamps_narrow_left_capture_into_viewport() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(0.0, 160.0), Vec2::new(40.0, 120.0));
	let badge_rect =
		WindowRenderer::selection_size_badge_rect(screen_rect, capture_rect, Vec2::new(92.0, 26.0));

	assert_eq!(badge_rect.min.x, screen_rect.min.x);
	assert!(badge_rect.max.x > capture_rect.max.x);
}

#[test]
fn selection_size_badge_rect_clamps_near_left_narrow_capture_into_viewport() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(20.0, 160.0), Vec2::new(40.0, 120.0));
	let badge_rect =
		WindowRenderer::selection_size_badge_rect(screen_rect, capture_rect, Vec2::new(92.0, 26.0));

	assert_eq!(badge_rect.min.x, screen_rect.min.x);
	assert!(badge_rect.max.x > capture_rect.max.x);
}

#[test]
fn selection_size_badge_rect_keeps_tiny_bottom_capture_visible() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(120.0, 588.0), Vec2::new(140.0, 12.0));
	let badge_rect =
		WindowRenderer::selection_size_badge_rect(screen_rect, capture_rect, Vec2::new(92.0, 26.0));

	assert_eq!(badge_rect.max.y, screen_rect.max.y);
	assert!(badge_rect.min.y < capture_rect.min.y);
	assert!(badge_rect.min.y >= screen_rect.min.y);
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
fn frozen_drag_region_selection_size_badge_uses_above_slot_when_bottom_toolbar_slot_overlaps() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect = Rect::from_min_size(Pos2::new(200.0, 180.0), Vec2::new(200.0, 300.0));
	let reserved_rect = Rect::from_min_size(
		Pos2::new(200.0, 180.0 + 300.0 + TOOLBAR_CAPTURE_GAP_PX),
		WindowRenderer::frozen_toolbar_size(&FrozenToolbarState::default()),
	);
	let badge_rect =
		WindowRenderer::selection_size_badge_rect_preferring_outside_with_reserved_rect(
			screen_rect,
			capture_rect,
			Vec2::new(92.0, 26.0),
			Some(reserved_rect),
		);

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.max.y, capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn frozen_window_selection_size_badge_keeps_inside_fallback_when_bottom_toolbar_slot_overlaps() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect = Rect::from_min_size(Pos2::new(200.0, 180.0), Vec2::new(200.0, 300.0));
	let reserved_rect = Rect::from_min_size(
		Pos2::new(200.0, 180.0 + 300.0 + TOOLBAR_CAPTURE_GAP_PX),
		WindowRenderer::frozen_toolbar_size(&FrozenToolbarState::default()),
	);
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

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

#[test]
fn frozen_top_toolbar_reserved_rect_uses_inside_fallback_slot() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 400,
		height: 160,
		scale_factor_x1000: 1_000,
	};
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect_points = RectPoints::new(40, 20, 240, 110);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(capture_rect_points);

	let toolbar_state = FrozenToolbarState::default();
	let reserved_rect = WindowRenderer::frozen_toolbar_reserved_rect(
		&state,
		monitor,
		screen_rect,
		ToolbarPlacement::Top,
		&toolbar_state,
	)
	.expect("top fallback slot should still be reserved");

	assert_eq!(reserved_rect.min.y, capture_rect.min.y + TOOLBAR_SCREEN_MARGIN_PX);
	assert_eq!(reserved_rect.height(), WindowRenderer::frozen_toolbar_size(&toolbar_state).y);
}

#[test]
fn overlay_session_computes_frozen_toolbar_reserved_rect_without_inline_toolbar_state() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));

	let reserved_rect = session
		.frozen_size_badge_toolbar_reserved_rect(monitor, screen_rect, true)
		.expect("overlay redraw should reserve the default toolbar slot");

	assert_eq!(reserved_rect.min.y, 480.0 + TOOLBAR_CAPTURE_GAP_PX);
	assert_eq!(
		reserved_rect.height(),
		WindowRenderer::frozen_toolbar_size(&session.toolbar_state).y
	);
}

#[test]
fn frozen_annotation_capsule_keeps_top_toolbar_default_slot_stable() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(160, 220, 260, 240);
	let mut session = OverlaySession::new();

	session.config.toolbar_placement = ToolbarPlacement::Top;

	let base_pos = session.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	let styled_pos =
		session.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect);

	assert_eq!(styled_pos, base_pos);
}

#[test]
fn frozen_annotation_capsule_flips_above_without_moving_bottom_toolbar_anchor() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut toolbar_state = FrozenToolbarState {
		selected_tool: FrozenToolbarTool::Text,
		..FrozenToolbarState::default()
	};
	let primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let anchor_y = screen_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - primary_size.y;
	let capture_rect = Rect::from_min_size(
		Pos2::new(160.0, anchor_y - TOOLBAR_CAPTURE_GAP_PX - 200.0),
		Vec2::new(260.0, 200.0),
	);
	let anchor = WindowRenderer::frozen_toolbar_default_window_pos(
		screen_rect,
		capture_rect,
		primary_size,
		WindowRenderer::frozen_toolbar_positioning_size(&toolbar_state),
		ToolbarPlacement::Bottom,
	);

	assert_eq!(anchor.y, anchor_y);
	assert!(anchor.y + primary_size.y + TOOLBAR_SCREEN_MARGIN_PX <= screen_rect.max.y);

	let mut below_toolbar_state = toolbar_state.clone();

	below_toolbar_state.annotation_style_capsule_placement =
		FrozenAnnotationStyleCapsulePlacement::Below;

	assert!(
		WindowRenderer::frozen_toolbar_window_rect(&below_toolbar_state, anchor).max.y
			> screen_rect.max.y
	);

	WindowRenderer::sync_frozen_annotation_style_capsule_placement(
		&mut toolbar_state,
		screen_rect,
		anchor,
	);

	assert_eq!(
		toolbar_state.annotation_style_capsule_placement,
		FrozenAnnotationStyleCapsulePlacement::Above
	);
	assert_eq!(WindowRenderer::frozen_toolbar_primary_rect(&toolbar_state, anchor).min, anchor);
	assert!(
		WindowRenderer::frozen_toolbar_window_rect(&toolbar_state, anchor).min.y < anchor.y,
		"style capsule should render above the stable primary anchor when bottom space is tight",
	);
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_annotation_capsule_flip_keeps_native_toolbar_outer_position_stable() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let mut session = OverlaySession::new();

	session.toolbar_inner_size_points =
		Some((startup_size.x.ceil().max(1.0) as u32, startup_size.y.ceil().max(1.0) as u32));

	let primary_size = WindowRenderer::frozen_toolbar_primary_size(&session.toolbar_state);
	let primary_anchor =
		Pos2::new(160.0, screen_rect.max.y - TOOLBAR_SCREEN_MARGIN_PX - primary_size.y);
	let base_outer = session.toolbar_outer_position_from_primary_anchor(monitor, primary_anchor);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	WindowRenderer::sync_frozen_annotation_style_capsule_placement(
		&mut session.toolbar_state,
		screen_rect,
		primary_anchor,
	);

	assert_eq!(
		session.toolbar_state.annotation_style_capsule_placement,
		FrozenAnnotationStyleCapsulePlacement::Above
	);

	let mut below_toolbar_state = session.toolbar_state.clone();

	below_toolbar_state.annotation_style_capsule_placement =
		FrozenAnnotationStyleCapsulePlacement::Below;

	assert!(
		WindowRenderer::frozen_toolbar_window_rect(&below_toolbar_state, primary_anchor).max.y
			> screen_rect.max.y
	);
	assert_eq!(
		session.toolbar_outer_position_from_primary_anchor(monitor, primary_anchor),
		base_outer
	);
}

#[test]
fn frozen_toolbar_reserved_rect_covers_secondary_annotation_capsule() {
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

	let toolbar_state = FrozenToolbarState {
		selected_tool: FrozenToolbarTool::Text,
		..FrozenToolbarState::default()
	};
	let primary_size = WindowRenderer::frozen_toolbar_primary_size(&toolbar_state);
	let default_pos = WindowRenderer::frozen_toolbar_default_window_pos(
		screen_rect,
		capture_rect,
		primary_size,
		WindowRenderer::frozen_toolbar_size(&toolbar_state),
		ToolbarPlacement::Bottom,
	);
	let reserved_rect = WindowRenderer::frozen_toolbar_reserved_rect(
		&state,
		monitor,
		screen_rect,
		ToolbarPlacement::Bottom,
		&toolbar_state,
	)
	.expect("annotation style capsule should be reserved with the default slot");

	assert_eq!(reserved_rect.min, default_pos);
	assert_eq!(reserved_rect.size(), WindowRenderer::frozen_toolbar_size(&toolbar_state));
}

#[test]
fn frozen_toolbar_reserved_rect_uses_overlay_viewport_size() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 400,
		height: 260,
		scale_factor_x1000: 1_000,
	};
	let overlay_screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 120.0));
	let toolbar_window_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(92.0, 26.0));
	let capture_rect_points = RectPoints::new(60, 40, 220, 60);
	let capture_rect = Rect::from_min_size(
		Pos2::new(capture_rect_points.x as f32, capture_rect_points.y as f32),
		Vec2::new(capture_rect_points.width as f32, capture_rect_points.height as f32),
	);
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(capture_rect_points);
	session.toolbar_state.layout_last_screen_size_points = Some(toolbar_window_rect.size());

	let toolbar_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let overlay_default_pos = WindowRenderer::frozen_toolbar_default_window_pos(
		overlay_screen_rect,
		capture_rect.intersect(overlay_screen_rect),
		toolbar_size,
		toolbar_size,
		session.config.toolbar_placement,
	);
	let toolbar_window_default_pos = WindowRenderer::frozen_toolbar_default_window_pos(
		toolbar_window_rect,
		capture_rect.intersect(toolbar_window_rect),
		toolbar_size,
		toolbar_size,
		session.config.toolbar_placement,
	);

	session.toolbar_state.floating_position = Some(overlay_default_pos);

	let reserved_rect = session
		.frozen_size_badge_toolbar_reserved_rect(monitor, overlay_screen_rect, true)
		.expect("overlay viewport-aligned toolbar slot should still be reserved");

	assert_ne!(overlay_default_pos, toolbar_window_default_pos);
	assert_eq!(reserved_rect.min, overlay_default_pos);
	assert_eq!(reserved_rect.size(), toolbar_size);
}

#[test]
fn frozen_toolbar_reserved_rect_skips_hidden_toolbar_slot() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));

	assert_eq!(session.frozen_size_badge_toolbar_reserved_rect(monitor, screen_rect, false), None);
}

#[test]
fn frozen_toolbar_reserved_rect_waits_for_toolbar_birth_readiness() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));
	session.toolbar_state.layout_last_screen_size_points = Some(screen_rect.size());
	session.toolbar_state.layout_stable_frames = 0;

	assert!(!session.frozen_toolbar_ready_for_draw(screen_rect));
	assert_eq!(
		session.frozen_size_badge_toolbar_reserved_rect(
			monitor,
			screen_rect,
			session.frozen_toolbar_ready_for_draw(screen_rect)
		),
		None
	);

	session.toolbar_state.layout_stable_frames = 1;

	assert!(session.frozen_toolbar_ready_for_draw(screen_rect));
	assert!(
		session
			.frozen_size_badge_toolbar_reserved_rect(
				monitor,
				screen_rect,
				session.frozen_toolbar_ready_for_draw(screen_rect)
			)
			.is_some()
	);
}

#[test]
fn frozen_toolbar_ready_for_draw_ignores_preseeded_position_until_viewport_stabilizes() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	assert!(session.toolbar_state.floating_position.is_some());
	assert_eq!(session.toolbar_state.layout_last_screen_size_points, None);
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);
	assert!(!session.frozen_toolbar_ready_for_draw(screen_rect));
	assert_eq!(
		session.frozen_size_badge_toolbar_reserved_rect(
			monitor,
			screen_rect,
			session.frozen_toolbar_ready_for_draw(screen_rect)
		),
		None
	);
}

#[test]
fn frozen_toolbar_ready_for_draw_recovers_after_preseeded_position_is_sampled() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert_eq!(session.toolbar_state.layout_last_screen_size_points, Some(screen_rect.size()));
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);
	assert!(!session.frozen_toolbar_ready_for_draw(screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(screen_rect));
	assert_eq!(session.toolbar_state.layout_stable_frames, 1);
	assert!(session.frozen_toolbar_ready_for_draw(screen_rect));
}

#[test]
fn render_frozen_toolbar_ui_waits_for_readiness_before_first_visible_frame() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(200, 180, 200, 300);
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut session = OverlaySession::new();
	let toolbar_placement = session.config.toolbar_placement;

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	assert!(session.toolbar_state.visible);
	assert_eq!(session.toolbar_state.layout_last_screen_size_points, None);
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);

	for frame in 0..2 {
		let state = &session.state;
		let toolbar_state = &mut session.toolbar_state;
		let mut hud_pill = None;
		let _ = ctx.run_ui(
			egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
			|ui: &mut Ui| {
				WindowRenderer::render_frozen_toolbar_ui(
					ui.ctx(),
					state,
					monitor,
					HudTheme::Dark,
					toolbar_placement,
					false,
					false,
					1.0,
					0.0,
					0.0,
					Some(toolbar_state),
					None,
					&mut hud_pill,
				);
			},
		);

		assert!(
			hud_pill.is_none(),
			"frame {frame} should not draw the toolbar before readiness stabilizes"
		);
	}

	let state = &session.state;
	let toolbar_state = &mut session.toolbar_state;
	let mut hud_pill = None;
	let _ = ctx.run_ui(
		egui::RawInput { screen_rect: Some(screen_rect), ..Default::default() },
		|ui: &mut Ui| {
			WindowRenderer::render_frozen_toolbar_ui(
				ui.ctx(),
				state,
				monitor,
				HudTheme::Dark,
				toolbar_placement,
				false,
				false,
				1.0,
				0.0,
				0.0,
				Some(toolbar_state),
				None,
				&mut hud_pill,
			);
		},
	);

	assert!(hud_pill.is_some(), "third frame should draw the stabilized toolbar");
}

#[test]
fn frozen_toolbar_reserved_rect_restores_near_default_slot() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let capture_rect = Rect::from_min_size(Pos2::new(200.0, 180.0), Vec2::new(200.0, 300.0));
	let mut state = OverlayState::new();
	let mut toolbar_state = FrozenToolbarState::default();
	let toolbar_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);
	let default_pos = WindowRenderer::frozen_toolbar_default_window_pos(
		screen_rect,
		capture_rect,
		toolbar_size,
		toolbar_size,
		ToolbarPlacement::Bottom,
	);
	let restored_pos = default_pos + Vec2::new(0.4, -0.35);

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));
	toolbar_state.visible = true;
	toolbar_state.floating_position = Some(restored_pos);

	assert_eq!(
		WindowRenderer::frozen_toolbar_reserved_rect(
			&state,
			monitor,
			screen_rect,
			ToolbarPlacement::Bottom,
			&toolbar_state,
		),
		Some(Rect::from_min_size(restored_pos, toolbar_size))
	);
}

#[test]
fn frozen_toolbar_overlay_viewport_sample_recovers_from_toolbar_window_pollution() {
	let monitor = tests::test_monitor();
	let overlay_screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let toolbar_window_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(92.0, 26.0));
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(200, 180, 200, 300));
	session.toolbar_state.layout_last_screen_size_points = Some(toolbar_window_rect.size());
	session.toolbar_state.layout_stable_frames = 1;

	assert!(!session.frozen_toolbar_ready_for_draw(overlay_screen_rect));
	assert!(!session.advance_frozen_toolbar_readiness_sample(overlay_screen_rect));
	assert_eq!(
		session.toolbar_state.layout_last_screen_size_points,
		Some(overlay_screen_rect.size())
	);
	assert_eq!(session.toolbar_state.layout_stable_frames, 0);
	assert_eq!(
		session.frozen_size_badge_toolbar_reserved_rect(
			monitor,
			overlay_screen_rect,
			session.frozen_toolbar_ready_for_draw(overlay_screen_rect)
		),
		None
	);
	assert!(!session.advance_frozen_toolbar_readiness_sample(overlay_screen_rect));
	assert_eq!(session.toolbar_state.layout_stable_frames, 1);
	assert_eq!(
		session.frozen_size_badge_toolbar_reserved_rect(
			monitor,
			overlay_screen_rect,
			session.frozen_toolbar_ready_for_draw(overlay_screen_rect)
		),
		Some(
			WindowRenderer::frozen_toolbar_reserved_rect(
				&session.state,
				monitor,
				overlay_screen_rect,
				session.config.toolbar_placement,
				&session.toolbar_state,
			)
			.expect("reserved rect after overlay viewport stabilization")
		)
	);
}

#[test]
fn selection_size_badge_reserved_rect_prefers_upper_band_when_bottom_space_is_reserved() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 220.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 40.0), Vec2::new(200.0, 150.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(80.0, 140.0), Vec2::new(120.0, 40.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(
		badge_rect.min.y,
		reserved_rect.min.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX - 26.0
	);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_reserved_rect_keeps_preferred_inside_when_top_space_is_clear() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 200.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 20.0), Vec2::new(200.0, 150.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(80.0, 28.0), Vec2::new(120.0, 40.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(badge_rect.max.y, capture_rect.max.y - SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_reserved_rect_falls_above_capture_when_inside_space_is_exhausted() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 220.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 170.0), Vec2::new(120.0, 50.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(40.0, 178.0), Vec2::new(120.0, 40.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.max.y, capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_reserved_rect_uses_above_slot_at_top_edge_when_visible() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 112.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 34.0), Vec2::new(120.0, 50.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(40.0, 42.0), Vec2::new(120.0, 40.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(badge_rect.min.y, screen_rect.min.y);
	assert_eq!(badge_rect.max.y, capture_rect.min.y - SELECTION_SIZE_BADGE_GAP_PX);
	assert!(!badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_reserved_rect_accepts_overlap_when_no_non_overlapping_slot_exists() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 52.0));
	let capture_rect = Rect::from_min_size(Pos2::new(40.0, 20.0), Vec2::new(120.0, 32.0));
	let reserved_rect = Rect::from_min_size(Pos2::new(40.0, 22.0), Vec2::new(120.0, 24.0));
	let badge_rect = WindowRenderer::selection_size_badge_rect_with_reserved_rect(
		screen_rect,
		capture_rect,
		Vec2::new(92.0, 26.0),
		Some(reserved_rect),
	);

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert_eq!(badge_rect.min.y, capture_rect.min.y);
	assert!(badge_rect.intersects(reserved_rect));
}

#[test]
fn selection_size_badge_text_uses_monitor_pixel_dimensions() {
	let monitor = tests::test_monitor_with_scale(1_000, 800, 2_000);

	assert_eq!(
		WindowRenderer::selection_size_badge_text(monitor, RectPoints::new(10, 20, 120, 80)),
		"240x160"
	);
}

#[test]
fn selection_size_badge_layout_keeps_visual_bounds_within_right_edge_rect() {
	let ctx = tests::test_egui_context();
	let layout = WindowRenderer::selection_size_badge_layout(&ctx, "240x160", HudTheme::Light, 1.0);
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(760.0, 160.0), Vec2::new(40.0, 120.0));
	let badge_rect =
		WindowRenderer::selection_size_badge_rect(screen_rect, capture_rect, layout.badge_size);
	let text_anchor = WindowRenderer::selection_size_badge_text_anchor(badge_rect, layout, 1.0);
	let visual_bounds =
		WindowRenderer::selection_size_badge_visual_bounds(text_anchor, layout.text_size, 1.0);

	assert_eq!(badge_rect.max.x, capture_rect.max.x);
	assert!(visual_bounds.min.x >= badge_rect.min.x);
	assert!(visual_bounds.max.x <= badge_rect.max.x);
}

#[test]
fn selection_size_badge_layout_keeps_visual_bounds_within_bottom_fallback_rect() {
	let ctx = tests::test_egui_context();
	let layout = WindowRenderer::selection_size_badge_layout(&ctx, "240x160", HudTheme::Light, 1.0);
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
	let capture_rect = Rect::from_min_size(Pos2::new(120.0, 588.0), Vec2::new(140.0, 12.0));
	let badge_rect =
		WindowRenderer::selection_size_badge_rect(screen_rect, capture_rect, layout.badge_size);
	let text_anchor = WindowRenderer::selection_size_badge_text_anchor(badge_rect, layout, 1.0);
	let visual_bounds =
		WindowRenderer::selection_size_badge_visual_bounds(text_anchor, layout.text_size, 1.0);

	assert_eq!(badge_rect.max.y, screen_rect.max.y);
	assert!(visual_bounds.min.y >= badge_rect.min.y);
	assert!(visual_bounds.max.y <= badge_rect.max.y);
}

#[test]
fn live_capture_size_badge_target_prefers_drag_then_hover_then_fullscreen() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Live;
	state.cursor = Some(GlobalPoint::new(320, 260));
	state.hovered_window_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id,
		rect: RectPoints::new(120, 140, 300, 220),
	});

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, true),
		Some(SelectionSizeBadgeTarget {
			rect: Rect::from_min_size(Pos2::new(120.0, 140.0), Vec2::new(300.0, 220.0)),
			size_points: RectPoints::new(120, 140, 300, 220),
		})
	);

	state.drag_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id,
		rect: RectPoints::new(180, 200, 260, 180),
	});

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, true),
		Some(SelectionSizeBadgeTarget {
			rect: Rect::from_min_size(Pos2::new(180.0, 200.0), Vec2::new(260.0, 180.0)),
			size_points: RectPoints::new(180, 200, 260, 180),
		})
	);

	state.drag_rect = None;
	state.hovered_window_rect = None;

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, true),
		Some(SelectionSizeBadgeTarget {
			rect: screen_rect,
			size_points: RectPoints::new(0, 0, monitor.width, monitor.height),
		})
	);
}

#[test]
fn live_capture_size_badge_target_skips_fullscreen_fallback_while_primary_down() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Live;
	state.cursor = Some(GlobalPoint::new(320, 260));

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, false),
		None
	);
}

#[test]
fn frozen_capture_size_badge_target_uses_frozen_rect() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1_000.0, 800.0));
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Frozen;
	state.frozen_capture_rect = Some(RectPoints::new(140, 180, 320, 240));

	assert_eq!(
		WindowRenderer::frozen_capture_size_badge_target(&state, screen_rect),
		Some(SelectionSizeBadgeTarget {
			rect: Rect::from_min_size(Pos2::new(140.0, 180.0), Vec2::new(320.0, 240.0)),
			size_points: RectPoints::new(140, 180, 320, 240),
		})
	);
}

#[test]
fn frozen_capture_size_badge_target_keeps_tiny_frozen_rect() {
	let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1_000.0, 800.0));
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Frozen;
	state.frozen_capture_rect = Some(RectPoints::new(140, 180, 2, 1));

	assert_eq!(
		WindowRenderer::frozen_capture_size_badge_target(&state, screen_rect),
		Some(SelectionSizeBadgeTarget {
			rect: Rect::from_min_size(Pos2::new(140.0, 180.0), Vec2::new(2.0, 1.0)),
			size_points: RectPoints::new(140, 180, 2, 1),
		})
	);
}

#[test]
fn render_frozen_capture_affordance_keeps_tiny_frozen_badge_path() {
	let ctx = tests::test_egui_context();
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();

	state.mode = OverlayMode::Frozen;
	state.monitor = Some(monitor);
	state.frozen_capture_rect = Some(RectPoints::new(140, 180, 2, 1));

	assert!(WindowRenderer::render_frozen_capture_affordance(
		&ctx,
		&state,
		monitor,
		screen_rect,
		HudTheme::Dark,
		false,
		FrozenCaptureSource::None,
		None,
		&[],
		None,
		&[],
		None,
		&[],
		None,
		&[],
		None,
		FrozenToolbarState::default().text_style,
		false,
		true,
		1.0,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert!(selection_flow_geometry_cache.is_empty());
}

#[test]
fn render_live_capture_affordances_keep_hover_scrim_when_flow_disabled() {
	let ctx = tests::test_egui_context();
	let layer = LayerId::new(Order::Foreground, Id::new("live-hover-flow-disabled"));
	let painter = ctx.layer_painter(layer);
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();

	state.mode = OverlayMode::Live;
	state.hovered_window_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id,
		rect: RectPoints::new(100, 120, 240, 320),
	});

	assert!(WindowRenderer::render_live_capture_affordances(
		&ctx,
		&painter,
		&state,
		monitor,
		screen_rect,
		HudTheme::Light,
		false,
		1.0,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert_eq!(selection_dashed_border_cache.key, None);
}

#[test]
fn render_live_capture_affordances_draw_drag_border_when_flow_disabled() {
	let ctx = tests::test_egui_context();
	let layer = LayerId::new(Order::Foreground, Id::new("live-drag-flow-disabled"));
	let painter = ctx.layer_painter(layer);
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();

	state.mode = OverlayMode::Live;
	state.drag_rect = Some(MonitorRectPoints {
		monitor_id: monitor.id,
		rect: RectPoints::new(100, 120, 240, 320),
	});

	assert!(WindowRenderer::render_live_capture_affordances(
		&ctx,
		&painter,
		&state,
		monitor,
		screen_rect,
		HudTheme::Light,
		false,
		1.0,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert!(selection_dashed_border_cache.key.is_some());
}

#[test]
fn render_live_capture_affordances_skips_fullscreen_flow_without_hover_or_drag() {
	let ctx = tests::test_egui_context();
	let layer = LayerId::new(Order::Foreground, Id::new("live-idle-no-flow"));
	let painter = ctx.layer_painter(layer);
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut selection_dashed_border_cache = SelectionDashedBorderCache::default();
	let mut state = OverlayState::new();
	let mut selection_flow_geometry_cache = SelectionFlowGeometryCache::default();

	state.mode = OverlayMode::Live;
	state.cursor = Some(GlobalPoint::new(240, 260));

	assert!(WindowRenderer::render_live_capture_affordances(
		&ctx,
		&painter,
		&state,
		monitor,
		screen_rect,
		HudTheme::Dark,
		true,
		1.0,
		&mut selection_flow_geometry_cache,
		&mut selection_dashed_border_cache,
	));
	assert!(selection_flow_geometry_cache.is_empty());
	assert_eq!(selection_dashed_border_cache.key, None);
}

#[test]
fn live_capture_size_badge_target_keeps_tiny_drag_rect() {
	let monitor = tests::test_monitor();
	let screen_rect =
		Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
	let mut state = OverlayState::new();

	state.mode = OverlayMode::Live;
	state.drag_rect =
		Some(MonitorRectPoints { monitor_id: monitor.id, rect: RectPoints::new(180, 200, 2, 1) });

	assert_eq!(
		WindowRenderer::live_capture_size_badge_target(&state, monitor, screen_rect, false),
		Some(SelectionSizeBadgeTarget {
			rect: Rect::from_min_size(Pos2::new(180.0, 200.0), Vec2::new(2.0, 1.0)),
			size_points: RectPoints::new(180, 200, 2, 1),
		})
	);
}

#[test]
fn live_loupe_default_position_hangs_below_hud_strip_when_space_exists() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 800,
		height: 600,
		scale_factor_x1000: 1_000,
	};
	let hud_outer = GlobalPoint::new(220, 120);
	let pos = OverlaySession::live_loupe_default_position(
		monitor,
		Some(GlobalPoint::new(100, 100)),
		Some(hud_outer),
		Some(52),
		232,
		232,
	)
	.unwrap();

	assert_eq!(pos.x, hud_outer.x);
	assert_eq!(pos.y, hud_outer.y + 52 + HUD_LOUPE_STRIP_GAP_POINTS);
}

#[test]
fn live_loupe_default_position_falls_above_hud_strip_when_below_overflows() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 800,
		height: 500,
		scale_factor_x1000: 1_000,
	};
	let hud_outer = GlobalPoint::new(220, 300);
	let pos = OverlaySession::live_loupe_default_position(
		monitor,
		Some(GlobalPoint::new(100, 100)),
		Some(hud_outer),
		Some(52),
		232,
		232,
	)
	.unwrap();

	assert_eq!(pos.x, hud_outer.x);
	assert_eq!(pos.y, hud_outer.y - HUD_LOUPE_STRIP_GAP_POINTS - 232);
}

#[test]
fn scroll_toolbar_compacts_to_two_buttons() {
	let frozen_toolbar_size = WindowRenderer::frozen_toolbar_size(&FrozenToolbarState::default());
	let scroll_toolbar_size = WindowRenderer::frozen_toolbar_size(&FrozenToolbarState {
		scroll_capture_active: true,
		..FrozenToolbarState::default()
	});

	assert!(scroll_toolbar_size.x < frozen_toolbar_size.x);
	assert_eq!(scroll_toolbar_size.y, frozen_toolbar_size.y);
}

#[cfg(target_os = "macos")]
#[test]
fn drag_region_toolbar_size_stays_stable_while_final_capture_readiness_changes() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(120, 160, 320, 240));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	session.sync_frozen_toolbar_state();

	let pending_toolbar_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let pending_tools = WindowRenderer::frozen_toolbar_tools(&session.toolbar_state);

	assert!(!session.toolbar_state.final_capture_ready);
	assert!(pending_tools.contains(&FrozenToolbarTool::AutoCenter));
	assert!(pending_tools.contains(&FrozenToolbarTool::Scroll));

	session.authoritative_frozen_capture_ready = true;

	session.sync_frozen_toolbar_state();

	let ready_toolbar_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let ready_tools = WindowRenderer::frozen_toolbar_tools(&session.toolbar_state);

	assert!(session.toolbar_state.final_capture_ready);
	assert!(ready_tools.contains(&FrozenToolbarTool::AutoCenter));
	assert!(ready_tools.contains(&FrozenToolbarTool::Scroll));
	assert_eq!(pending_toolbar_size, ready_toolbar_size);
}

#[cfg(target_os = "macos")]
#[test]
fn drag_region_toolbar_recenters_when_auto_center_appears_after_preview_commit() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	let seeded_pos = session
		.toolbar_state
		.floating_position
		.expect("toolbar should seed before frozen preview is ready");
	let seeded_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);
	let capture_midpoint_x = capture_rect.x as f32 + capture_rect.width as f32 * 0.5;

	assert!(!session.toolbar_state.auto_center_available);
	assert_eq!(seeded_pos.x + seeded_size.x * 0.5, capture_midpoint_x);

	session.commit_frozen_preview(monitor, tests::test_frozen_image(), None);
	session.sync_frozen_toolbar_state();

	let ready_size = WindowRenderer::frozen_toolbar_size(&session.toolbar_state);

	assert!(session.toolbar_state.auto_center_available);
	assert!(ready_size.x > seeded_size.x);
	assert!(session.maybe_recenter_frozen_toolbar_default_slot(monitor));

	let recentered_pos =
		session.toolbar_state.floating_position.expect("toolbar should keep a default position");

	assert_eq!(recentered_pos.x + ready_size.x * 0.5, capture_midpoint_x);
	assert_eq!(session.toolbar_state.default_slot_position, Some(recentered_pos));
}

#[cfg(target_os = "macos")]
#[test]
fn late_toolbar_width_change_preserves_manual_toolbar_move() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(120, 160, 320, 240);
	let mut session = OverlaySession::new();

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	let seeded_default_pos = session
		.toolbar_state
		.floating_position
		.expect("toolbar should seed before frozen preview is ready");
	let moved_pos = seeded_default_pos + Vec2::new(24.0, 0.0);

	session.toolbar_state.floating_position = Some(moved_pos);

	session.commit_frozen_preview(monitor, tests::test_frozen_image(), None);
	session.sync_frozen_toolbar_state();

	assert!(!session.maybe_recenter_frozen_toolbar_default_slot(monitor));
	assert_eq!(session.toolbar_state.floating_position, Some(moved_pos));
	assert_eq!(
		session.toolbar_state.default_slot_position,
		Some(session.frozen_toolbar_default_position_for_capture_rect(monitor, capture_rect))
	);
}
#[test]
fn auto_center_toolbar_tool_only_appears_when_available() {
	let default_tools = WindowRenderer::frozen_toolbar_tools(&FrozenToolbarState::default());
	let auto_center_tools = WindowRenderer::frozen_toolbar_tools(&FrozenToolbarState {
		auto_center_available: true,
		..FrozenToolbarState::default()
	});

	assert!(!default_tools.contains(&FrozenToolbarTool::AutoCenter));
	assert!(auto_center_tools.contains(&FrozenToolbarTool::AutoCenter));

	#[cfg(target_os = "macos")]
	{
		assert!(default_tools.contains(&FrozenToolbarTool::Ocr));
		assert!(auto_center_tools.contains(&FrozenToolbarTool::Ocr));
	}
}

#[test]
fn toolbar_window_startup_size_covers_every_tool_permutation() {
	let startup_size = overlay::frozen_toolbar_window_startup_size_points();
	let toolbar_states = [
		FrozenToolbarState::default(),
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Pen,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Text,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState { auto_center_available: true, ..FrozenToolbarState::default() },
		FrozenToolbarState { scroll_capture_available: true, ..FrozenToolbarState::default() },
		FrozenToolbarState {
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Pen,
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			selected_tool: FrozenToolbarTool::Text,
			auto_center_available: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
		FrozenToolbarState {
			scroll_capture_active: true,
			scroll_capture_available: true,
			..FrozenToolbarState::default()
		},
	];

	for toolbar_state in toolbar_states {
		let toolbar_size = WindowRenderer::frozen_toolbar_size(&toolbar_state);

		assert!(
			startup_size.x >= toolbar_size.x,
			"startup width {} should cover toolbar width {} for {toolbar_state:?}",
			startup_size.x,
			toolbar_size.x
		);
		assert!(
			startup_size.y >= toolbar_size.y,
			"startup height {} should cover toolbar height {} for {toolbar_state:?}",
			startup_size.y,
			toolbar_size.y
		);
	}
}

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
