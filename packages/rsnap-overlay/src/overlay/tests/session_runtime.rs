use std::time::{Duration, Instant};

#[cfg(not(target_os = "macos"))]
use image::{Rgba, RgbaImage};

#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::MacLiveFrameStream;
#[cfg(not(target_os = "macos"))]
use crate::overlay::OverlayConfig;
#[cfg(target_os = "macos")]
use crate::overlay::tests::GlobalPoint;
use crate::overlay::tests::{
	self, HudTheme, OCCLUDED_FRAME_REDRAW_RETRY_WINDOW, OverlayMode, OverlaySession,
	SurfaceFrameSkipReason, WindowRenderer, hud_helpers, overlay,
};
#[cfg(target_os = "macos")]
use crate::overlay::tests::{LiveCaptureInteraction, ModifiersState};
#[cfg(not(target_os = "macos"))]
use crate::state::RectPoints;

#[cfg(not(target_os = "macos"))]
#[test]
fn cached_live_background_fast_path_advances_frozen_generation() {
	let monitor = tests::test_monitor();
	let capture_rect = RectPoints::new(0, 0, monitor.width, monitor.height);
	let first_image = tests::test_frozen_image();
	let second_image = RgbaImage::from_pixel(8, 8, Rgba([90, 12, 45, 255]));
	let mut session = OverlaySession::with_config(OverlayConfig {
		show_hud_blur: true,
		..OverlayConfig::default()
	});

	session.state.begin_freeze(monitor);
	session.state.commit_frozen_final_image(monitor, first_image);

	session.state.mode = OverlayMode::Live;

	let previous_generation = session.state.frozen_generation;

	session.state.live_bg_monitor = Some(monitor);
	session.state.live_bg_image = Some(second_image.clone());

	session.begin_frozen_capture_with_rect(monitor, Some(capture_rect), None, None);

	assert!(matches!(session.state.mode, OverlayMode::Frozen));
	assert_eq!(session.state.monitor, Some(monitor));
	assert_eq!(session.state.frozen_generation, previous_generation.wrapping_add(1));
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&second_image));
	assert_eq!(session.state.frozen_export_image.as_ref(), Some(&second_image));
}

#[cfg(target_os = "macos")]
#[test]
fn sync_live_surface_bg_from_stream_promotes_clean_live_snapshot() {
	let monitor = tests::test_monitor();
	let stream = MacLiveFrameStream::new();
	let captured_at = tests::fresh_live_stream_snapshot_captured_at();
	let mut session = OverlaySession::new();

	stream.debug_set_active_stream_generation(monitor.id, 1);
	stream.debug_set_self_capture_filter_complete(monitor.id, true);
	stream.debug_store_test_snapshot_with_metadata(monitor, 7, 1, captured_at);

	session.live_sample_stream = Some(stream);
	session.cursor_monitor = Some(monitor);

	session.sync_live_surface_bg_from_stream(monitor);

	assert_eq!(session.state.live_bg_monitor, Some(monitor));
	assert!(session.state.live_bg_image.is_some());
	assert_eq!(session.last_live_surface_bg_snapshot_at, Some(captured_at));
	assert_eq!(session.state.live_bg_generation, 1);

	session.sync_live_surface_bg_from_stream(monitor);

	assert_eq!(session.state.live_bg_generation, 1);
}

#[test]
fn due_egui_repaint_deadline_is_consumed_once_ready() {
	let session = OverlaySession::new();
	let due_at = Instant::now() - Duration::from_millis(1);

	*session.egui_repaint_deadline.lock().unwrap_or_else(|err| err.into_inner()) = Some(due_at);

	assert!(session.take_due_egui_repaint_deadline(Instant::now()));
	assert!(session.egui_repaint_deadline.lock().unwrap_or_else(|err| err.into_inner()).is_none());
	assert!(!session.take_due_egui_repaint_deadline(Instant::now()));
}

#[test]
fn frozen_spotlight_export_dim_matches_preview_scrim_alpha() {
	let visible_numerator = u16::from(u8::MAX - OverlaySession::frozen_spotlight_scrim_alpha());

	assert_eq!(OverlaySession::frozen_spotlight_outside_brightness_numerator(), visible_numerator,);

	for channel in [0_u8, 1, 17, 64, 120, 180, 210, 254, 255] {
		let preview_dimmed = ((u16::from(channel) * visible_numerator) / 255) as u8;

		assert_eq!(
			OverlaySession::dim_frozen_spotlight_channel(channel),
			preview_dimmed,
			"channel {channel}",
		);
	}
}

#[cfg(target_os = "macos")]
#[test]
fn reset_for_start_clears_reused_session_transient_flags() {
	let mut session = OverlaySession {
		session_active: true,
		window_list_refresh_inflight: true,
		drop_next_window_list_refresh_snapshot: true,
		png_encode_inflight: true,
		pending_self_capture_exception_window_ids_worker_refresh: true,
		pending_startup_aux_live_stream_filter_upgrade: true,
		capture_windows_hidden: true,
		loupe_activation_key_down: true,
		keyboard_modifiers: ModifiersState::SHIFT,
		live_capture_interaction: LiveCaptureInteraction::PressPending {
			monitor: tests::test_monitor(),
			press_global: GlobalPoint::new(12, 34),
			click_target: None,
			release_global: None,
			released: false,
		},
		hud_window_visible: true,
		toolbar_window_visible: true,
		toolbar_window_drawn_once: true,
		toolbar_badge_slot_ready: true,
		toolbar_window_warmup_redraws_remaining: 3,
		..OverlaySession::default()
	};

	tests::promote_session_export_authority_ready(&mut session);

	session.reset_for_start();

	assert!(!session.is_active());
	assert!(!session.window_list_refresh_inflight);
	assert!(!session.drop_next_window_list_refresh_snapshot);
	assert!(!session.png_encode_inflight);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
	assert!(!session.pending_startup_aux_live_stream_filter_upgrade);
	assert!(!tests::session_export_authority_ready(&session));
	assert!(!session.capture_windows_hidden);
	assert!(!session.loupe_activation_key_down);
	assert_eq!(session.keyboard_modifiers, ModifiersState::default());
	assert_eq!(session.live_capture_interaction, LiveCaptureInteraction::Idle);
	assert!(!session.hud_window_visible);
	assert!(!session.toolbar_window_visible);
	assert!(!session.toolbar_window_drawn_once);
	assert!(!session.toolbar_badge_slot_ready);
	assert_eq!(session.toolbar_window_warmup_redraws_remaining, 0);
}

#[test]
fn is_active_tracks_explicit_session_state() {
	let inactive = OverlaySession::default();
	let active = OverlaySession { session_active: true, ..OverlaySession::default() };

	assert!(!inactive.is_active());
	assert!(active.is_active());
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_visual_handoff_ends_once_display_is_ready() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	assert!(!session.frozen_visual_handoff_pending_for_monitor(monitor));
}

#[cfg(target_os = "macos")]
#[test]
fn frozen_surface_bg_unlocks_as_soon_as_display_is_ready() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	assert!(session.allow_frozen_surface_bg_for_overlay_monitor(monitor, false));
}

#[cfg(target_os = "macos")]
#[test]
fn live_surface_bg_refresh_disables_during_drag_capture_and_loupe() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Live;
	session.cursor_monitor = Some(monitor);

	assert!(!session.should_draw_live_surface_bg_for_overlay_monitor(monitor));
	assert!(session.should_refresh_live_surface_bg_for_overlay_monitor(monitor));

	session.state.live_bg_monitor = Some(monitor);
	session.state.live_bg_image = Some(tests::test_frozen_image());

	assert!(session.should_draw_live_surface_bg_for_overlay_monitor(monitor));
	assert!(!session.should_refresh_live_surface_bg_for_overlay_monitor(monitor));

	session.state.alt_held = true;

	assert!(session.should_draw_live_surface_bg_for_overlay_monitor(monitor));
	assert!(!session.should_refresh_live_surface_bg_for_overlay_monitor(monitor));

	session.set_live_capture_interaction(LiveCaptureInteraction::PressPending {
		monitor,
		press_global: monitor.origin,
		click_target: None,
		release_global: None,
		released: false,
	});

	assert!(session.should_draw_live_surface_bg_for_overlay_monitor(monitor));
	assert!(!session.should_refresh_live_surface_bg_for_overlay_monitor(monitor));

	session.set_live_capture_interaction(LiveCaptureInteraction::DraggingSelection {
		monitor,
		press_global: monitor.origin,
		current_global: monitor.origin,
	});

	assert!(session.should_draw_live_surface_bg_for_overlay_monitor(monitor));
	assert!(!session.should_refresh_live_surface_bg_for_overlay_monitor(monitor));

	session.state.live_bg_monitor = None;
	session.state.live_bg_image = None;

	assert!(!session.should_draw_live_surface_bg_for_overlay_monitor(monitor));
	assert!(session.should_refresh_live_surface_bg_for_overlay_monitor(monitor));
}

#[test]
fn selection_flow_overlay_draw_respects_config() {
	let mut session = OverlaySession::new();

	assert!(session.selection_flow_enabled_for_overlay_draw());

	session.config.selection_flow_enabled = false;

	assert!(!session.selection_flow_enabled_for_overlay_draw());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn handle_capture_redraw_does_not_rearm_inflight_freeze_capture() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::set_session_inflight_freeze_capture(&mut session, Some(monitor));

	session.capture_windows_hidden = true;

	let _ = session.handle_capture_and_toolbar_redraw_post(monitor, false);

	assert_eq!(tests::session_inflight_freeze_capture(&session), Some(monitor));
	assert!(tests::session_pending_freeze_capture(&session).is_none());
	assert!(!tests::session_frozen_capture_armed(&session));
	assert!(session.capture_windows_hidden);
}

#[test]
fn frozen_selection_scrim_matches_live_drag_scrim() {
	for theme in [HudTheme::Dark, HudTheme::Light] {
		assert_eq!(
			WindowRenderer::frozen_selection_scrim_color(theme),
			WindowRenderer::live_drag_selection_scrim_color(theme),
		);
	}
}

#[test]
fn tinted_hud_body_fill_amount_zero_keeps_base_fill() {
	for theme in [HudTheme::Dark, HudTheme::Light] {
		let base_fill = hud_helpers::hud_body_fill_srgba8(theme, false);
		let no_tint = WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 0.0, 0.585);

		assert_eq!(no_tint.r(), base_fill[0]);
		assert_eq!(no_tint.g(), base_fill[1]);
		assert_eq!(no_tint.b(), base_fill[2]);
		assert_eq!(no_tint.a(), 255);
	}
}

#[test]
fn tinted_hud_body_fill_100pct_tint_is_visibly_blue() {
	let dark_min_delta: u16 = 57;
	let light_min_delta: u16 = 24;
	let sky_tint = 0.585;

	for theme in [HudTheme::Dark, HudTheme::Light] {
		let base_fill =
			WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 0.0, sky_tint);
		let tinted_fill =
			WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 1.0, sky_tint);
		let rgb_delta = u16::from(base_fill.r()).abs_diff(u16::from(tinted_fill.r()))
			+ u16::from(base_fill.g()).abs_diff(u16::from(tinted_fill.g()))
			+ u16::from(base_fill.b()).abs_diff(u16::from(tinted_fill.b()));
		let min_delta =
			if matches!(theme, HudTheme::Dark) { dark_min_delta } else { light_min_delta };

		assert!(
			rgb_delta >= min_delta,
			"expected minimum tint delta >= {min_delta}, got {rgb_delta}"
		);
	}
}

#[test]
fn tinted_hud_body_fill_preserves_alpha() {
	for theme in [HudTheme::Dark, HudTheme::Light] {
		let tint_hue = 0.585;
		let opaque = WindowRenderer::tinted_hud_body_fill(theme, false, true, 0.25, 1.0, tint_hue);
		let translucent =
			WindowRenderer::tinted_hud_body_fill(theme, false, false, 0.33, 1.0, tint_hue);

		assert_eq!(opaque.a(), 255);
		assert_eq!(translucent.a(), (0.33_f32 * 255.0).round().clamp(0.0, 255.0) as u8);
	}
}

#[test]
fn tinted_hud_body_fill_blur_active_enforces_min_opacity() {
	for theme in [HudTheme::Dark, HudTheme::Light] {
		let tint_hue = 0.585;
		let fill = WindowRenderer::tinted_hud_body_fill(theme, true, false, 0.0, 0.0, tint_hue);
		let expected =
			(hud_helpers::hud_blur_tint_alpha(theme) * 255.0).round().clamp(0.0, 255.0) as u8;

		assert_eq!(fill.a(), expected);
	}
}

#[test]
fn interactive_repaint_fps_is_fixed_contract_target() {
	assert_eq!(OverlaySession::interactive_repaint_fps(), 120.0);
}

#[test]
fn occluded_surface_skip_requests_redraw_until_retry_window_expires() {
	let now = Instant::now();
	let mut retry_until = None;

	assert!(overlay::should_request_overlay_redraw_after_surface_skip(
		SurfaceFrameSkipReason::Occluded,
		now,
		&mut retry_until,
	));
	assert_eq!(retry_until, Some(now + OCCLUDED_FRAME_REDRAW_RETRY_WINDOW));
	assert!(overlay::should_request_overlay_redraw_after_surface_skip(
		SurfaceFrameSkipReason::Occluded,
		now + Duration::from_millis(500),
		&mut retry_until,
	));
	assert!(!overlay::should_request_overlay_redraw_after_surface_skip(
		SurfaceFrameSkipReason::Occluded,
		now + OCCLUDED_FRAME_REDRAW_RETRY_WINDOW,
		&mut retry_until,
	));
	assert_eq!(retry_until, None);
}

#[test]
fn timeout_surface_skip_always_requests_redraw_without_touching_occluded_retry_window() {
	let now = Instant::now();
	let retry_deadline = now + Duration::from_millis(250);
	let mut retry_until = Some(retry_deadline);

	assert!(overlay::should_request_overlay_redraw_after_surface_skip(
		SurfaceFrameSkipReason::Timeout,
		now,
		&mut retry_until,
	));
	assert_eq!(retry_until, Some(retry_deadline));
}
