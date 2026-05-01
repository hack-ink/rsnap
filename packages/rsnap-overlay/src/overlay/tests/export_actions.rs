use std::path::PathBuf;

use image::RgbaImage;
#[cfg(target_os = "macos")]
use image::imageops;

use crate::overlay::tests::{
	self, FrozenAnnotationColor, FrozenArrowAnnotation, FrozenBrushStyle, FrozenCaptureSource,
	FrozenEditKind, FrozenExportTransform, FrozenSpotlightAnnotation, FrozenTextAnnotation,
	FrozenTextEditState, FrozenToolbarTool, GlobalPoint, OutputNaming, OverlayControl,
	OverlaySession, PngAction, Pos2, PreparedHostEffectRequest, RectPoints, Rgba, ScrollSession,
	WindowCaptureAlphaMode, WindowFreezeCaptureTarget,
};

#[test]
fn begin_png_action_copies_preview_render_image_during_active_scroll_capture() {
	let mut session = OverlaySession::new();
	let base = tests::make_scroll_capture_test_image(3, &[[10, 0, 0, 255]; 8]);
	let grown = tests::make_scroll_capture_test_image(3, &[[20, 0, 0, 255]; 12]);
	let mut scroll_session = ScrollSession::new(base, 320).expect("scroll session");
	let _ = scroll_session.observe_downward_sample(grown).expect("observe");
	let expected_export = scroll_session.export_image().clone();
	let monitor = tests::test_monitor();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	tests::promote_session_export_authority_ready(&mut session);

	session.scroll_capture.active = true;
	session.scroll_capture.session = Some(scroll_session);
	session.scroll_capture.preview_display_image =
		Some(RgbaImage::from_pixel(320, 64, Rgba([77, 0, 0, 255])));

	session.begin_png_action(PngAction::Copy);

	assert_eq!(session.pending_png_action, Some(PngAction::Copy));
	assert_eq!(session.pending_encode_png.as_ref(), Some(&expected_export));
	assert_eq!(session.state.error_message.as_deref(), Some("Copying..."));
}

#[test]
fn encoded_png_exits_with_host_effect_request() {
	#[derive(Clone, Copy)]
	enum EncodedPngCase {
		Copy,
		Save,
	}

	impl EncodedPngCase {
		fn action(self) -> PngAction {
			match self {
				Self::Copy => PngAction::Copy,
				Self::Save => PngAction::Save,
			}
		}

		fn png_bytes(self) -> Vec<u8> {
			match self {
				Self::Copy => vec![1, 2, 3, 4],
				Self::Save => vec![4, 3, 2, 1],
			}
		}

		fn name(self) -> &'static str {
			match self {
				Self::Copy => "copy",
				Self::Save => "save",
			}
		}
	}

	for case in [EncodedPngCase::Copy, EncodedPngCase::Save] {
		let mut session = OverlaySession::new();

		session.pending_png_action = Some(case.action());

		if matches!(case, EncodedPngCase::Save) {
			session.config.output_dir = PathBuf::from("/tmp/rsnap-save-test");
			session.config.output_filename_prefix = String::from("captured-prefix");
			session.config.output_naming = OutputNaming::Sequence;
		}

		let expected_png_bytes = case.png_bytes();
		let control = session.handle_encoded_png_response(expected_png_bytes.clone());

		match (case, control) {
			(
				EncodedPngCase::Copy,
				OverlayControl::HostEffect(PreparedHostEffectRequest::CopyPng { png_bytes }),
			) => {
				assert_eq!(png_bytes, expected_png_bytes);
			},
			(
				EncodedPngCase::Save,
				OverlayControl::HostEffect(PreparedHostEffectRequest::SavePng {
					png_bytes,
					output_dir,
					output_filename_prefix,
					output_naming,
				}),
			) => {
				assert_eq!(png_bytes, expected_png_bytes);
				assert_eq!(output_dir, PathBuf::from("/tmp/rsnap-save-test"));
				assert_eq!(output_filename_prefix, "captured-prefix");
				assert_eq!(output_naming, OutputNaming::Sequence);
			},
			(_, control) => panic!("expected {} host effect request, got {control:?}", case.name()),
		}
	}
}

#[test]
fn report_host_effect_error_keeps_overlay_session_alive() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.session_active = true;

	session.report_host_effect_error("copy failed");

	assert!(session.session_active);
	assert_eq!(session.state.error_message.as_deref(), Some("copy failed"));
}

#[test]
fn complete_host_effect_request_runs_overlay_exit_cleanup() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	session.session_active = true;
	session.pending_png_action = Some(PngAction::Copy);
	session.pending_encode_png = Some(tests::test_frozen_image());

	session.complete_host_effect_request(&PreparedHostEffectRequest::CopyPng {
		png_bytes: vec![1, 2, 3],
	});

	assert!(!session.session_active);
	assert!(session.pending_png_action.is_none());
	assert!(session.pending_encode_png.is_none());
}

#[test]
fn current_export_image_includes_frozen_brush_strokes() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session
		.state
		.commit_frozen_final_image(monitor, RgbaImage::from_pixel(8, 8, Rgba([12, 34, 56, 255])));

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(2, 2)));
	assert!(session.update_frozen_brush_stroke(GlobalPoint::new(5, 2)));
	assert!(session.finish_frozen_brush_stroke());

	let export_image = session.current_export_image().expect("annotated export image");

	assert_eq!(export_image.get_pixel(7, 7), &Rgba([12, 34, 56, 255]));
	assert_eq!(
		export_image.get_pixel(2, 2),
		&Rgba(session.toolbar_state.brush_style.color.export_rgba())
	);
}

#[test]
fn current_export_image_uses_selected_brush_color() {
	let monitor = tests::test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session
		.state
		.commit_frozen_final_image(monitor, RgbaImage::from_pixel(8, 8, Rgba([12, 34, 56, 255])));

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;
	session.toolbar_state.brush_style.color = FrozenAnnotationColor::Green;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(2, 2)));
	assert!(session.finish_frozen_brush_stroke());

	let export_image = session.current_export_image().expect("annotated export image");

	assert_eq!(
		export_image.get_pixel(2, 2),
		&Rgba(session.toolbar_state.brush_style.color.export_rgba())
	);
}

#[test]
fn current_export_image_antialiases_frozen_brush_edges() {
	let monitor = tests::test_monitor();
	let background = Rgba([240, 240, 240, 255]);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(
		&mut session,
		monitor,
		RgbaImage::from_pixel(16, 16, background),
	);

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 16, 16));

	tests::promote_session_export_authority_ready(&mut session);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(3, 3)));
	assert!(session.update_frozen_brush_stroke(GlobalPoint::new(12, 12)));
	assert!(session.finish_frozen_brush_stroke());

	let export_image = session.current_export_image().expect("annotated export image");
	let has_antialiased_edge = export_image.pixels().any(|pixel| {
		pixel != &background
			&& pixel != &Rgba(session.toolbar_state.brush_style.color.export_rgba())
	});

	assert!(has_antialiased_edge, "expected blended edge pixels around the exported brush");
}

#[cfg(target_os = "macos")]
#[test]
fn begin_ocr_action_exits_with_deferred_request_and_clears_stale_png_output_intent() {
	let monitor = tests::test_monitor();
	let expected_export = tests::test_frozen_image();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, expected_export.clone());

	session.state.frozen_capture_rect =
		Some(RectPoints::new(0, 0, expected_export.width(), expected_export.height()));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	tests::promote_session_export_authority_ready(&mut session);

	session.begin_png_action(PngAction::Copy);

	assert_eq!(session.pending_png_action, Some(PngAction::Copy));
	assert_eq!(session.pending_encode_png.as_ref(), Some(&expected_export));

	let control = session.begin_ocr_action();
	let OverlayControl::HostEffect(PreparedHostEffectRequest::DeferredTextRecognition(request)) =
		control
	else {
		panic!("expected deferred OCR request");
	};

	assert_eq!(session.pending_png_action, None);
	assert!(session.pending_encode_png.is_none());
	assert_eq!(request.export_image().as_ref(), Some(&expected_export));
	assert_eq!(request.request_id, 0);
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn begin_ocr_action_drag_region_still_uses_frozen_image_under_matte_mode() {
	let monitor = tests::test_monitor();
	let expected_export = tests::test_frozen_image();
	let mut session = OverlaySession::new();

	session.config.window_capture_alpha_mode = WindowCaptureAlphaMode::MatteLight;

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, expected_export.clone());

	session.state.frozen_capture_rect =
		Some(RectPoints::new(0, 0, expected_export.width(), expected_export.height()));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	tests::promote_session_export_authority_ready(&mut session);

	let control = session.begin_ocr_action();
	let OverlayControl::HostEffect(PreparedHostEffectRequest::DeferredTextRecognition(request)) =
		control
	else {
		panic!("expected deferred OCR request");
	};

	assert_eq!(request.export_image().as_ref(), Some(&expected_export));
	assert!(session.frozen_window_image.is_none());
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn authoritative_freeze_response_updates_export_authority_without_overwriting_display_preview() {
	let monitor = tests::test_monitor();
	let preview_image = RgbaImage::from_pixel(8, 8, Rgba([18, 24, 32, 255]));
	let authoritative_image = RgbaImage::from_pixel(8, 8, Rgba([92, 108, 124, 255]));
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::commit_frozen_display_preview_state(&mut session, monitor, preview_image.clone());

	session.handle_captured_freeze_response(monitor, authoritative_image.clone(), None, None);

	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&preview_image));
	assert_eq!(session.state.frozen_export_image.as_ref(), Some(&authoritative_image));
	assert!(tests::session_export_authority_ready(&session));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn window_matte_capture_miss_restores_hidden_windows_before_returning() {
	let monitor = tests::test_monitor();
	let preview_image = tests::test_frozen_image();
	let capture_rect = RectPoints::new(2, 1, 4, 4);
	let mut session = OverlaySession::new();

	session.config.window_capture_alpha_mode = WindowCaptureAlphaMode::MatteDark;

	session.state.begin_freeze(monitor);
	session.state.commit_frozen_display_image(monitor, preview_image.clone());

	session.capture_windows_hidden = true;

	tests::set_session_inflight_window_freeze_capture(
		&mut session,
		Some(WindowFreezeCaptureTarget { monitor, window_id: 41, rect: capture_rect }),
	);

	session.handle_captured_freeze_response(monitor, tests::test_frozen_image(), None, None);

	assert!(!session.capture_windows_hidden);
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&preview_image));
	assert!(session.state.frozen_export_image.is_none());
	assert_eq!(
		session.state.error_message.as_deref(),
		Some("Window capture is unavailable. Please try again.")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn window_matte_mosaic_export_and_ocr_match_preview_pixels() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let capture_rect = RectPoints::new(2, 1, 4, 4);
	let window_id = 7;
	let background = RgbaImage::from_pixel(8, 8, Rgba([18, 24, 32, 255]));
	let window_image = RgbaImage::from_fn(4, 4, |x, y| {
		let alpha = match (x + y) % 4 {
			0 => 64,
			1 => 112,
			2 => 176,
			_ => 224,
		};

		Rgba([
			40_u8.saturating_add((x * 37) as u8),
			28_u8.saturating_add((y * 41) as u8),
			52_u8.saturating_add(((x + y) * 23) as u8),
			alpha,
		])
	});
	let mut session = OverlaySession::new();

	session.config.window_capture_alpha_mode = WindowCaptureAlphaMode::MatteLight;

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::Window;

	tests::set_session_inflight_window_freeze_capture(
		&mut session,
		Some(WindowFreezeCaptureTarget { monitor, window_id, rect: capture_rect }),
	);

	session.handle_captured_freeze_response(
		monitor,
		background,
		Some(window_image),
		Some(window_id),
	);

	assert!(tests::session_export_authority_ready(&session));
	assert!(session.apply_frozen_mosaic_edit(capture_rect));

	let expected_export = imageops::crop_imm(
		session
			.state
			.frozen_display_image
			.as_ref()
			.expect("window matte preview should populate the frozen display image"),
		capture_rect.x,
		capture_rect.y,
		capture_rect.width,
		capture_rect.height,
	)
	.to_image();

	assert_eq!(session.current_export_image().as_ref(), Some(&expected_export));

	let control = session.begin_ocr_action();
	let OverlayControl::HostEffect(PreparedHostEffectRequest::DeferredTextRecognition(request)) =
		control
	else {
		panic!("expected deferred OCR request");
	};

	assert_eq!(request.export_image().as_ref(), Some(&expected_export));
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn begin_ocr_action_skips_deferred_request_when_drag_region_crop_is_out_of_bounds() {
	let monitor = tests::test_monitor();
	let frozen_image = tests::test_frozen_image();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, frozen_image.clone());

	session.state.frozen_capture_rect = Some(RectPoints::new(monitor.width + 10, 20, 100, 80));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	tests::promote_session_export_authority_ready(&mut session);

	let control = session.begin_ocr_action();

	assert!(matches!(control, OverlayControl::Continue));
	assert_eq!(session.state.frozen_display_image.as_ref(), Some(&frozen_image));
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn begin_ocr_action_uses_scroll_capture_export_image_in_deferred_request() {
	let monitor = tests::test_monitor();
	let base = tests::make_scroll_capture_test_image(3, &[[10, 0, 0, 255]; 8]);
	let grown = tests::make_scroll_capture_test_image(3, &[[20, 0, 0, 255]; 12]);
	let mut scroll_session = ScrollSession::new(base, 320).expect("scroll session");
	let _ = scroll_session.observe_downward_sample(grown).expect("observe");
	let expected_export = scroll_session.export_image().clone();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_display_state(&mut session, monitor, tests::test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	tests::promote_session_export_authority_ready(&mut session);

	session.scroll_capture.active = true;
	session.scroll_capture.session = Some(scroll_session);
	session.scroll_capture.preview_display_image =
		Some(RgbaImage::from_pixel(320, 64, Rgba([77, 0, 0, 255])));

	let control = session.begin_ocr_action();
	let OverlayControl::HostEffect(PreparedHostEffectRequest::DeferredTextRecognition(request)) =
		control
	else {
		panic!("expected deferred OCR request");
	};

	assert_eq!(request.export_image().as_ref(), Some(&expected_export));
	assert!(session.state.error_message.is_none());
}

#[test]
fn current_export_image_renders_frozen_text_annotations() {
	let monitor = tests::test_monitor_with_scale(160, 120, 1_000);
	let base = RgbaImage::from_pixel(160, 120, Rgba([0, 0, 0, 255]));
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_ready_state(&mut session, monitor, base);

	session.state.frozen_capture_rect = Some(RectPoints::new(10, 12, 120, 80));

	session.frozen_text_annotations.push(FrozenTextAnnotation {
		anchor: Pos2::new(24.0, 24.0),
		text: String::from("Text"),
		style: session.toolbar_state.text_style,
	});
	session.push_frozen_edit_to_undo_history(FrozenEditKind::TextAnnotation);

	let export = session.current_export_image().expect("export image");

	assert_eq!(export.dimensions(), (120, 80));
	assert!(export.pixels().any(|pixel| *pixel != Rgba([0, 0, 0, 255])));
}

#[test]
fn current_export_image_applies_frozen_spotlight_outside_selection() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let base = RgbaImage::from_pixel(8, 8, Rgba([120, 180, 210, 255]));
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_ready_state(&mut session, monitor, base);

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));

	session
		.frozen_spotlight_annotations
		.push(FrozenSpotlightAnnotation { rect: RectPoints::new(2, 2, 3, 3) });
	session.push_frozen_edit_to_undo_history(FrozenEditKind::SpotlightAnnotation);

	let export = session.current_export_image().expect("export image");
	let dimmed = |channel: u8| {
		((u16::from(channel) * OverlaySession::frozen_spotlight_outside_brightness_numerator())
			/ 255) as u8
	};

	assert_eq!(export.get_pixel(0, 0), &Rgba([dimmed(120), dimmed(180), dimmed(210), 255]));
	assert_eq!(export.get_pixel(3, 3), &Rgba([120, 180, 210, 255]));
}

#[test]
fn current_export_image_applies_multiple_frozen_spotlights_without_extra_darkening() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let base = RgbaImage::from_pixel(8, 8, Rgba([120, 180, 210, 255]));
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_ready_state(&mut session, monitor, base);

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));

	session
		.frozen_spotlight_annotations
		.push(FrozenSpotlightAnnotation { rect: RectPoints::new(1, 1, 2, 2) });
	session.push_frozen_edit_to_undo_history(FrozenEditKind::SpotlightAnnotation);
	session
		.frozen_spotlight_annotations
		.push(FrozenSpotlightAnnotation { rect: RectPoints::new(5, 5, 2, 2) });
	session.push_frozen_edit_to_undo_history(FrozenEditKind::SpotlightAnnotation);

	let export = session.current_export_image().expect("export image");
	let dimmed = |channel: u8| {
		((u16::from(channel) * OverlaySession::frozen_spotlight_outside_brightness_numerator())
			/ 255) as u8
	};

	assert_eq!(export.get_pixel(0, 0), &Rgba([dimmed(120), dimmed(180), dimmed(210), 255]));
	assert_eq!(export.get_pixel(4, 4), &Rgba([dimmed(120), dimmed(180), dimmed(210), 255]));
	assert_eq!(export.get_pixel(1, 1), &Rgba([120, 180, 210, 255]));
	assert_eq!(export.get_pixel(5, 5), &Rgba([120, 180, 210, 255]));
}

#[test]
fn current_export_image_renders_frozen_arrow_after_spotlight_scrim() {
	let monitor = tests::test_monitor_with_scale(8, 8, 1_000);
	let base = RgbaImage::from_pixel(8, 8, Rgba([120, 120, 120, 255]));
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_ready_state(&mut session, monitor, base);

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));

	session
		.frozen_spotlight_annotations
		.push(FrozenSpotlightAnnotation { rect: RectPoints::new(3, 3, 2, 2) });
	session.push_frozen_edit_to_undo_history(FrozenEditKind::SpotlightAnnotation);
	session.frozen_arrow_annotations.push(FrozenArrowAnnotation {
		start: Pos2::new(1.0, 1.0),
		end: Pos2::new(7.0, 1.0),
		style: FrozenBrushStyle::default(),
	});
	session.push_frozen_edit_to_undo_history(FrozenEditKind::ArrowAnnotation);

	let export = session.current_export_image().expect("export image");
	let dimmed =
		((120_u16 * OverlaySession::frozen_spotlight_outside_brightness_numerator()) / 255) as u8;

	assert_eq!(export.get_pixel(6, 6), &Rgba([dimmed, dimmed, dimmed, 255]));
	assert_eq!(export.get_pixel(4, 1), &Rgba(FrozenBrushStyle::default().color.export_rgba()));
}

#[test]
fn current_export_image_renders_frozen_arrow_outline_without_tinting() {
	let monitor = tests::test_monitor_with_scale(32, 32, 1_000);
	let base = RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 255]));
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	tests::finish_frozen_ready_state(&mut session, monitor, base);

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 32, 32));

	session.frozen_arrow_annotations.push(FrozenArrowAnnotation {
		start: Pos2::new(4.0, 16.0),
		end: Pos2::new(28.0, 16.0),
		style: FrozenBrushStyle { stroke_width_points: 6.0, ..FrozenBrushStyle::default() },
	});
	session.push_frozen_edit_to_undo_history(FrozenEditKind::ArrowAnnotation);

	let export = session.current_export_image().expect("export image");
	let halo_pixel = export.get_pixel(16, 10);

	assert_eq!(halo_pixel[3], 255);
	assert!(halo_pixel[0].abs_diff(halo_pixel[1]) <= 1);
	assert!(halo_pixel[1].abs_diff(halo_pixel[2]) <= 1);
	assert!(halo_pixel[0] > 0);
}

#[test]
fn scroll_capture_hides_frozen_text_annotations_in_preview() {
	let mut session = OverlaySession::new();

	session.frozen_text_annotations.push(FrozenTextAnnotation {
		anchor: Pos2::new(12.0, 18.0),
		text: String::from("visible"),
		style: session.toolbar_state.text_style,
	});

	assert_eq!(session.visible_frozen_text_annotations().len(), 1);

	session.scroll_capture.active = true;

	assert!(session.visible_frozen_text_annotations().is_empty());
}

#[test]
fn scroll_capture_hides_active_frozen_text_edit_in_preview() {
	let mut session = OverlaySession::new();

	session.frozen_text_edit = Some(FrozenTextEditState::new(Pos2::new(12.0, 18.0)));

	assert!(session.visible_frozen_text_edit().is_some());

	session.scroll_capture.active = true;

	assert!(session.visible_frozen_text_edit().is_none());
}

#[test]
fn frozen_export_transform_uses_actual_export_image_dimensions() {
	let capture_rect = RectPoints::new(10, 12, 20, 10);
	let transform = FrozenExportTransform::new(capture_rect, 60, 30).expect("transform");

	assert_eq!(transform.point_to_pixels(Pos2::new(10.0, 12.0)), Pos2::new(0.0, 0.0));
	assert_eq!(transform.point_to_pixels(Pos2::new(20.0, 17.0)), Pos2::new(30.0, 15.0));
	assert_eq!(transform.point_to_pixels(Pos2::new(30.0, 22.0)), Pos2::new(60.0, 30.0));
	assert_eq!(transform.scalar_scale(), 3.0);
}
