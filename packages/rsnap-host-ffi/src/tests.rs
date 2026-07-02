use std::env;
use std::ffi::CString;
use std::fs;
use std::process;
use std::ptr;
use std::slice;

use crate::{
	RSNAP_HOST_FFI_ABI_VERSION, RsnapCaptureFrameBackgroundKind, RsnapCaptureFrameBackgroundPlan,
	RsnapCaptureFrameColorStop, RsnapCaptureFramePlan, RsnapCaptureFrameRenderKind,
	RsnapCaptureFrameSourceKind, RsnapCaptureFrameWallpaperRequest, RsnapCursorIntent,
	RsnapFloatPoint, RsnapFloatRect, RsnapFrozenAnnotationColor, RsnapFrozenOverlayEditSnapshot,
	RsnapFrozenOverlayEditStyle, RsnapFrozenOverlayExportElement,
	RsnapFrozenOverlayExportElementKind, RsnapFrozenSelectionTransformKind, RsnapHostEvent,
	RsnapHostEventKind, RsnapHostReport, RsnapHostReportKind, RsnapHostRequestKind,
	RsnapHostRequestValue, RsnapMonitorRect, RsnapOwnedBytes, RsnapOwnedRgbaRegion, RsnapPixelRect,
	RsnapPlatformTag, RsnapPoint, RsnapRect, RsnapRgb, RsnapSceneKind, RsnapSceneModel,
	RsnapScrollMinimapPlan, RsnapSessionConfig, RsnapSessionHandle, RsnapStatus,
	RsnapToolbarItemKind, RsnapWindowRect,
};
#[cfg(target_os = "macos")]
use crate::{RsnapScrollObserveOutcomeKind, RsnapScrollObserveResult};
use rsnap_capture_core::RgbaExportImage;

fn default_config() -> RsnapSessionConfig {
	RsnapSessionConfig {
		platform: RsnapPlatformTag::MacOS,
		allow_text_input: 1,
		prefers_toolbar_above_selection: 0,
	}
}

fn scroll_frame(width: u32, height: u32, top_row: u32) -> Vec<u8> {
	let mut rgba = Vec::with_capacity((width * height * 4) as usize);

	for y in 0..height {
		let document_row = top_row + y;

		for x in 0..width {
			rgba.push(((document_row * 17 + x * 13) % 251) as u8);
			rgba.push(((document_row * 29 + x * 7) % 251) as u8);
			rgba.push(((document_row * 5 + x * 31) % 251) as u8);
			rgba.push(255);
		}
	}

	rgba
}

fn png_dimensions(png: &[u8]) -> (u32, u32) {
	assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

	let width = u32::from_be_bytes(png[16..20].try_into().expect("PNG width bytes"));
	let height = u32::from_be_bytes(png[20..24].try_into().expect("PNG height bytes"));

	(width, height)
}

fn png_phys_chunk(png: &[u8]) -> Option<(u32, u32, u8)> {
	assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

	let mut offset = 8;

	while offset + 12 <= png.len() {
		let length =
			u32::from_be_bytes(png[offset..offset + 4].try_into().expect("PNG chunk length bytes"))
				as usize;
		let data_start = offset + 8;
		let data_end = data_start.checked_add(length)?;
		let next_offset = data_end.checked_add(4)?;

		if next_offset > png.len() {
			return None;
		}
		if &png[offset + 4..offset + 8] == b"pHYs" {
			assert_eq!(length, 9);

			let x = u32::from_be_bytes(
				png[data_start..data_start + 4].try_into().expect("pHYs x bytes"),
			);
			let y = u32::from_be_bytes(
				png[data_start + 4..data_start + 8].try_into().expect("pHYs y bytes"),
			);

			return Some((x, y, png[data_start + 8]));
		}

		offset = next_offset;
	}

	None
}

#[test]
fn ffi_session_enters_live_and_emits_request() {
	let handle = unsafe { crate::rsnap_session_create(default_config()) };
	let mut request = RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };
	let mut scene = RsnapSceneModel::default();

	assert_eq!(unsafe { crate::rsnap_session_enter_live(handle) }, RsnapStatus::Ok);
	assert_eq!(
		unsafe { crate::rsnap_session_take_next_request(handle, &mut request) },
		RsnapStatus::Ok
	);
	assert_eq!(request.kind, RsnapHostRequestKind::StartLiveCapture as u32);
	assert_eq!(
		unsafe { crate::rsnap_session_copy_scene_model(handle, &mut scene) },
		RsnapStatus::Ok
	);
	assert_eq!(scene.scene_kind, RsnapSceneKind::Live as u32);
	assert_eq!(scene.cursor_intent, RsnapCursorIntent::Default as u32);

	unsafe { crate::rsnap_session_destroy(handle) };
}

#[test]
fn ffi_session_applies_freeze_report() {
	let handle = unsafe { crate::rsnap_session_create(default_config()) };
	let mut scene = RsnapSceneModel::default();

	assert_eq!(unsafe { crate::rsnap_session_enter_live(handle) }, RsnapStatus::Ok);

	let mut request = RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };
	let _ = unsafe { crate::rsnap_session_take_next_request(handle, &mut request) };

	assert_eq!(
		unsafe {
			crate::rsnap_session_handle_host_report(
				handle,
				RsnapHostReport {
					kind: RsnapHostReportKind::FreezeSnapshotCommitted as u32,
					selection: RsnapRect { x: 20, y: 30, width: 100, height: 60 },
					has_selection: 1,
					effect_kind: 0,
					permission_kind: 0,
					granted: 0,
					status_message_len: 0,
					status_message: [0; crate::abi::RSNAP_STATUS_MESSAGE_CAPACITY],
				},
			)
		},
		RsnapStatus::Ok
	);
	assert_eq!(
		unsafe { crate::rsnap_session_copy_scene_model(handle, &mut scene) },
		RsnapStatus::Ok
	);
	assert_eq!(scene.scene_kind, RsnapSceneKind::Frozen as u32);
	assert_eq!(scene.has_frozen_selection, 1);

	unsafe { crate::rsnap_session_destroy(handle) };
}

#[test]
fn ffi_session_tracks_pointer_updates() {
	let handle = unsafe { crate::rsnap_session_create(default_config()) };
	let mut scene = RsnapSceneModel::default();
	let _ = unsafe { crate::rsnap_session_enter_live(handle) };
	let mut request = RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };
	let _ = unsafe { crate::rsnap_session_take_next_request(handle, &mut request) };

	assert_eq!(
		unsafe {
			crate::rsnap_session_handle_host_event(
				handle,
				RsnapHostEvent {
					kind: RsnapHostEventKind::PointerMoved as u32,
					point: RsnapPoint { x: 50, y: 60 },
					has_point: 1,
					rgb: RsnapRgb { r: 1, g: 2, b: 3 },
					has_rgb: 1,
					active_monitor: RsnapMonitorRect {
						id: 9,
						origin: RsnapPoint { x: 0, y: 0 },
						width: 1_440,
						height: 900,
						scale_factor_x1000: 2_000,
					},
					has_active_monitor: 1,
					highlighted_window: RsnapWindowRect {
						window_id: 42,
						has_window_id: 1,
						x: 20,
						y: 30,
						width: 500,
						height: 400,
					},
					has_highlighted_window: 1,
					toolbar_item_kind: 0,
				},
			)
		},
		RsnapStatus::Ok
	);
	assert_eq!(
		unsafe { crate::rsnap_session_copy_scene_model(handle, &mut scene) },
		RsnapStatus::Ok
	);
	assert_eq!(scene.has_pointer, 1);
	assert_eq!(scene.pointer.x, 50);
	assert_eq!(scene.has_rgb, 1);
	assert_eq!(scene.has_active_monitor, 1);
	assert_eq!(scene.active_monitor.id, 9);
	assert_eq!(scene.has_highlighted_window, 1);
	assert_eq!(scene.highlighted_window.window_id, 42);

	unsafe { crate::rsnap_session_destroy(handle) };
}

#[test]
fn destroy_allows_null() {
	let handle: *mut RsnapSessionHandle = ptr::null_mut();

	unsafe { crate::rsnap_session_destroy(handle) };
}

#[test]
fn ffi_export_rgba_to_png_returns_owned_png() {
	let rgba = scroll_frame(4, 4, 0);
	let mut png = RsnapOwnedBytes::default();

	assert_eq!(
		unsafe { crate::rsnap_export_rgba_to_png(4, 4, rgba.as_ptr(), rgba.len(), &mut png) },
		RsnapStatus::Ok
	);
	assert!(png.len > 0);

	let png_bytes = unsafe { slice::from_raw_parts(png.bytes, png.len) };

	assert_eq!(png_dimensions(png_bytes), (4, 4));
	assert_eq!(png_phys_chunk(png_bytes), None);

	unsafe {
		crate::rsnap_owned_bytes_release(&mut png);
	}

	assert!(png.bytes.is_null());
	assert_eq!(png.len, 0);
	assert_eq!(png.capacity, 0);
}

#[test]
fn ffi_export_rgba_to_png_with_screen_scale_writes_density() {
	let rgba = scroll_frame(4, 4, 0);
	let mut png = RsnapOwnedBytes::default();

	assert_eq!(
		unsafe {
			crate::rsnap_export_rgba_to_png_with_screen_scale(
				4,
				4,
				rgba.as_ptr(),
				rgba.len(),
				2_000,
				&mut png,
			)
		},
		RsnapStatus::Ok
	);

	let png_bytes = unsafe { slice::from_raw_parts(png.bytes, png.len) };

	assert_eq!(png_dimensions(png_bytes), (4, 4));
	assert_eq!(png_phys_chunk(png_bytes), Some((5_669, 5_669, 1)));

	unsafe {
		crate::rsnap_owned_bytes_release(&mut png);
	}
}

#[test]
fn ffi_export_rgba_to_png_with_screen_scale_rejects_zero_scale() {
	let rgba = scroll_frame(4, 4, 0);
	let mut png = RsnapOwnedBytes::default();

	assert_eq!(
		unsafe {
			crate::rsnap_export_rgba_to_png_with_screen_scale(
				4,
				4,
				rgba.as_ptr(),
				rgba.len(),
				0,
				&mut png,
			)
		},
		RsnapStatus::InvalidInput
	);
	assert!(png.bytes.is_null());
}

#[test]
fn ffi_export_rgba_crop_to_png_crops_dimensions() {
	let rgba = scroll_frame(4, 4, 0);
	let crop = RsnapPixelRect { x: 1, y: 0, width: 2, height: 3 };
	let mut png = RsnapOwnedBytes::default();

	assert_eq!(
		unsafe {
			crate::rsnap_export_rgba_crop_to_png(4, 4, rgba.as_ptr(), rgba.len(), crop, &mut png)
		},
		RsnapStatus::Ok
	);
	assert_eq!(png_dimensions(unsafe { slice::from_raw_parts(png.bytes, png.len) }), (2, 3));

	unsafe {
		crate::rsnap_owned_bytes_release(&mut png);
	}
}

#[test]
fn ffi_export_rgba_crop_to_png_with_screen_scale_writes_density() {
	let rgba = scroll_frame(4, 4, 0);
	let crop = RsnapPixelRect { x: 1, y: 0, width: 2, height: 3 };
	let mut png = RsnapOwnedBytes::default();

	assert_eq!(
		unsafe {
			crate::rsnap_export_rgba_crop_to_png_with_screen_scale(
				4,
				4,
				rgba.as_ptr(),
				rgba.len(),
				crop,
				2_000,
				&mut png,
			)
		},
		RsnapStatus::Ok
	);

	let png_bytes = unsafe { slice::from_raw_parts(png.bytes, png.len) };

	assert_eq!(png_dimensions(png_bytes), (2, 3));
	assert_eq!(png_phys_chunk(png_bytes), Some((5_669, 5_669, 1)));

	unsafe {
		crate::rsnap_owned_bytes_release(&mut png);
	}
}

#[test]
fn ffi_export_rgba_crop_to_png_rejects_out_of_bounds_crop() {
	let rgba = scroll_frame(4, 4, 0);
	let crop = RsnapPixelRect { x: 3, y: 3, width: 2, height: 2 };
	let mut png = RsnapOwnedBytes::default();

	assert_eq!(
		unsafe {
			crate::rsnap_export_rgba_crop_to_png(4, 4, rgba.as_ptr(), rgba.len(), crop, &mut png)
		},
		RsnapStatus::InvalidInput
	);
	assert!(png.bytes.is_null());
}

#[test]
fn ffi_frozen_display_crop_rect_returns_core_pixel_rect() {
	let mut out_rect = RsnapPixelRect::default();
	let status = unsafe {
		crate::rsnap_frozen_display_crop_rect(
			2_880,
			1_800,
			RsnapFloatRect { x: 0.0, y: 0.0, width: 1_440.0, height: 900.0 },
			RsnapFloatRect { x: 100.0, y: 200.0, width: 300.0, height: 150.0 },
			&mut out_rect,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(out_rect, RsnapPixelRect { x: 200, y: 1_100, width: 600, height: 300 });
}

#[test]
fn ffi_frozen_display_crop_rect_returns_empty_for_outside_selection() {
	let mut out_rect = RsnapPixelRect::default();
	let status = unsafe {
		crate::rsnap_frozen_display_crop_rect(
			200,
			200,
			RsnapFloatRect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 },
			RsnapFloatRect { x: 120.0, y: 10.0, width: 10.0, height: 20.0 },
			&mut out_rect,
		)
	};

	assert_eq!(status, RsnapStatus::Empty);
}

#[test]
fn ffi_frozen_mosaic_light_privacy_patch_returns_rgba_region() {
	let mut patch = RsnapOwnedRgbaRegion::default();
	let status = unsafe {
		crate::rsnap_frozen_mosaic_light_privacy_patch_rgba(
			100,
			80,
			RsnapFloatRect { x: 4.2, y: 9.1, width: 28.4, height: 21.0 },
			&mut patch,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(patch.width, 3);
	assert_eq!(patch.height, 3);
	assert_eq!(patch.len, 36);

	let bytes = unsafe { slice::from_raw_parts(patch.rgba, patch.len) };

	assert_eq!(&bytes[..12], &[211, 211, 211, 255, 205, 205, 205, 255, 202, 201, 199, 255]);

	unsafe {
		crate::rsnap_owned_rgba_region_release(&mut patch);
	}
}

#[test]
fn ffi_frozen_mosaic_light_privacy_patch_returns_empty_for_outside_rect() {
	let mut patch = RsnapOwnedRgbaRegion::default();
	let status = unsafe {
		crate::rsnap_frozen_mosaic_light_privacy_patch_rgba(
			100,
			80,
			RsnapFloatRect { x: 120.0, y: 10.0, width: 10.0, height: 20.0 },
			&mut patch,
		)
	};

	assert_eq!(status, RsnapStatus::Empty);
}

#[test]
fn ffi_frozen_overlay_export_render_rgba_returns_composited_region() {
	let mut rgba = vec![180_u8; 64 * 40 * 4];

	for alpha in (3..rgba.len()).step_by(4) {
		rgba[alpha] = 255;
	}

	let points = [RsnapFloatPoint { x: 2.0, y: 2.0 }, RsnapFloatPoint { x: 24.0, y: 18.0 }];
	let text = CString::new("Hi").expect("text has no interior nul");
	let elements = [
		RsnapFrozenOverlayExportElement {
			kind: RsnapFrozenOverlayExportElementKind::Mosaic,
			rect: RsnapFloatRect { x: 4.0, y: 4.0, width: 16.0, height: 10.0 },
			start: RsnapFloatPoint::default(),
			end: RsnapFloatPoint::default(),
			points: ptr::null(),
			points_len: 0,
			text: ptr::null(),
			stroke_width_points: 0.0,
			border_width_points: 0.0,
			font_size_points: 0.0,
			color: RsnapFrozenAnnotationColor::Blue,
		},
		RsnapFrozenOverlayExportElement {
			kind: RsnapFrozenOverlayExportElementKind::Pen,
			rect: RsnapFloatRect::default(),
			start: RsnapFloatPoint::default(),
			end: RsnapFloatPoint::default(),
			points: points.as_ptr(),
			points_len: points.len(),
			text: ptr::null(),
			stroke_width_points: 2.0,
			border_width_points: 0.0,
			font_size_points: 0.0,
			color: RsnapFrozenAnnotationColor::Blue,
		},
		RsnapFrozenOverlayExportElement {
			kind: RsnapFrozenOverlayExportElementKind::Text,
			rect: RsnapFloatRect::default(),
			start: RsnapFloatPoint { x: 6.0, y: 24.0 },
			end: RsnapFloatPoint::default(),
			points: ptr::null(),
			points_len: 0,
			text: text.as_ptr(),
			stroke_width_points: 0.0,
			border_width_points: 0.0,
			font_size_points: 12.0,
			color: RsnapFrozenAnnotationColor::White,
		},
	];
	let mut out = RsnapOwnedRgbaRegion::default();
	let status = unsafe {
		crate::rsnap_frozen_overlay_export_render_rgba(
			64,
			40,
			rgba.as_ptr(),
			rgba.len(),
			RsnapFloatRect { x: 0.0, y: 0.0, width: 64.0, height: 40.0 },
			elements.as_ptr(),
			elements.len(),
			&mut out,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(out.width, 64);
	assert_eq!(out.height, 40);
	assert_eq!(out.len, rgba.len());

	let bytes = unsafe { slice::from_raw_parts(out.rgba, out.len) };

	assert_ne!(bytes, rgba.as_slice());

	unsafe {
		crate::rsnap_owned_rgba_region_release(&mut out);
	}
}

#[test]
fn ffi_frozen_overlay_edit_session_copies_rust_owned_snapshot() {
	let handle = crate::rsnap_frozen_overlay_edit_session_create();

	assert!(!handle.is_null());

	let style = RsnapFrozenOverlayEditStyle {
		stroke_width_points: 3.0,
		stroke_color: RsnapFrozenAnnotationColor::Blue,
		spotlight_border_width_points: 0.0,
		spotlight_color: RsnapFrozenAnnotationColor::Blue,
		text_font_size_points: 16.0,
		text_color: RsnapFrozenAnnotationColor::White,
	};
	let selection = RsnapFloatRect { x: 0.0, y: 0.0, width: 200.0, height: 120.0 };
	let mut changed = 0;
	let status = unsafe {
		crate::rsnap_frozen_overlay_edit_session_begin(
			handle,
			RsnapToolbarItemKind::Text,
			RsnapFloatPoint { x: 12.0, y: 18.0 },
			selection,
			style,
			&mut changed,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(changed, 1);

	let text = CString::new("Hello").expect("text has no interior nul");
	let status = unsafe {
		crate::rsnap_frozen_overlay_edit_session_append_text(handle, text.as_ptr(), &mut changed)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(changed, 1);

	let status = unsafe {
		crate::rsnap_frozen_overlay_edit_session_commit_text(handle, style, &mut changed)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(changed, 1);

	let mut snapshot = RsnapFrozenOverlayEditSnapshot::default();
	let status =
		unsafe { crate::rsnap_frozen_overlay_edit_session_copy_snapshot(handle, &mut snapshot) };

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(snapshot.can_undo, 1);
	assert_eq!(snapshot.elements_len, 1);

	let elements = unsafe { slice::from_raw_parts(snapshot.elements, snapshot.elements_len) };

	assert_eq!(elements[0].kind, RsnapFrozenOverlayExportElementKind::Text);
	assert_eq!(unsafe { std::ffi::CStr::from_ptr(elements[0].text) }.to_str(), Ok("Hello"));

	unsafe {
		crate::rsnap_frozen_overlay_edit_snapshot_release(&mut snapshot);
		crate::rsnap_frozen_overlay_edit_session_destroy(handle);
	}
}

#[test]
fn ffi_bgra_frame_sample_rgb_returns_core_sample() {
	let bgra = bgra_frame(4, 3, 16);
	let mut rgb = RsnapRgb::default();
	let status = unsafe {
		crate::rsnap_bgra_frame_sample_rgb(
			4,
			3,
			16,
			bgra.as_ptr(),
			bgra.len(),
			RsnapFloatRect { x: 0.0, y: 0.0, width: 4.0, height: 3.0 },
			1.0,
			2.5,
			&mut rgb,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(rgb, RsnapRgb { r: 11, g: 21, b: 31 });
}

#[test]
fn ffi_bgra_frame_loupe_patch_returns_rgba_region() {
	let bgra = bgra_frame(4, 3, 16);
	let mut patch = RsnapOwnedRgbaRegion::default();
	let status = unsafe {
		crate::rsnap_bgra_frame_loupe_patch_rgba(
			4,
			3,
			16,
			bgra.as_ptr(),
			bgra.len(),
			RsnapFloatRect { x: 0.0, y: 0.0, width: 4.0, height: 3.0 },
			0.0,
			2.0,
			3,
			&mut patch,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(patch.width, 3);
	assert_eq!(patch.height, 3);
	assert_eq!(patch.len, 36);

	let bytes = unsafe { slice::from_raw_parts(patch.rgba, patch.len) };

	assert_eq!(&bytes[..8], &[10, 20, 30, 200, 10, 20, 30, 200]);

	unsafe {
		crate::rsnap_owned_rgba_region_release(&mut patch);
	}
}

#[test]
fn ffi_bgra_frame_loupe_patch_rejects_invalid_storage() {
	let bgra = bgra_frame(4, 3, 16);
	let mut patch = RsnapOwnedRgbaRegion::default();
	let status = unsafe {
		crate::rsnap_bgra_frame_loupe_patch_rgba(
			4,
			3,
			12,
			bgra.as_ptr(),
			bgra.len(),
			RsnapFloatRect { x: 0.0, y: 0.0, width: 4.0, height: 3.0 },
			0.0,
			2.0,
			3,
			&mut patch,
		)
	};

	assert_eq!(status, RsnapStatus::InvalidInput);
	assert!(patch.rgba.is_null());
}

#[test]
fn ffi_capture_frame_plan_returns_core_geometry() {
	let mut plan = RsnapCaptureFramePlan::default();
	let status = unsafe {
		crate::rsnap_capture_frame_plan(
			320,
			180,
			2.0,
			RsnapCaptureFrameSourceKind::Window,
			&mut plan,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(plan.canvas_width, 416.0);
	assert_eq!(plan.canvas_height, 276.0);
	assert_eq!(plan.image_rect, RsnapFloatRect { x: 48.0, y: 48.0, width: 320.0, height: 180.0 });
	assert_eq!(plan.corner_radius, 9.9);
	assert_eq!(plan.shadows[0].blur, 80.0);
	assert_eq!(plan.shadows[1].offset_y, -22.0);
}

#[test]
fn ffi_capture_frame_aspect_fill_crop_rect_returns_core_rect() {
	let mut rect = RsnapFloatRect::default();
	let status = unsafe {
		crate::rsnap_capture_frame_aspect_fill_crop_rect(1_600, 900, 1_000.0, 1_000.0, &mut rect)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(rect, RsnapFloatRect { x: 350.0, y: 0.0, width: 900.0, height: 900.0 });
}

#[test]
fn ffi_capture_frame_background_plan_returns_core_preset() {
	let mut plan = RsnapCaptureFrameBackgroundPlan::default();
	let status = unsafe {
		crate::rsnap_capture_frame_background_plan(
			RsnapCaptureFrameBackgroundKind::Graphite,
			&mut plan,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(plan.prefers_wallpaper, 0);
	assert_eq!(plan.wallpaper_overlay_alpha, 0.0);
	assert_eq!(plan.locations, [0.0, 0.54, 1.0]);
	assert_eq!(
		plan.colors[0],
		RsnapCaptureFrameColorStop { red: 0.08, green: 0.09, blue: 0.11, alpha: 1.0 }
	);
	assert_eq!(
		plan.colors[2],
		RsnapCaptureFrameColorStop { red: 0.56, green: 0.59, blue: 0.64, alpha: 1.0 }
	);
}

#[test]
fn ffi_capture_frame_wallpaper_request_returns_core_thumbnail_policy() {
	let mut request = RsnapCaptureFrameWallpaperRequest::default();
	let status = unsafe {
		crate::rsnap_capture_frame_wallpaper_request_plan(
			RsnapCaptureFrameBackgroundKind::SystemWallpaper,
			1_535.2,
			996.0,
			&mut request,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(request.target_pixel_size, 1_536);
	assert_eq!(request.overlay_alpha, 0.10);
}

#[test]
fn ffi_capture_frame_wallpaper_request_returns_empty_for_gradient_background() {
	let mut request = RsnapCaptureFrameWallpaperRequest::default();
	let status = unsafe {
		crate::rsnap_capture_frame_wallpaper_request_plan(
			RsnapCaptureFrameBackgroundKind::Aurora,
			1_536.0,
			996.0,
			&mut request,
		)
	};

	assert_eq!(status, RsnapStatus::Empty);
}

#[test]
fn ffi_capture_frame_wallpaper_png_thumbnail_returns_owned_region() {
	let path_buf =
		env::temp_dir().join(format!("rsnap-ffi-wallpaper-thumb-{}-rgba.png", process::id()));
	let png = RgbaExportImage::from_raw(
		4,
		2,
		vec![
			255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 255,
			0, 255, 255, 255, 255, 0, 255, 255, 20, 30, 40, 255,
		],
	)
	.expect("test RGBA payload should create an image")
	.to_png_bytes()
	.expect("test RGBA image should encode as PNG");

	fs::write(&path_buf, png).expect("test PNG should be written");

	let path = CString::new(path_buf.to_string_lossy().as_bytes())
		.expect("test PNG path should not contain interior NUL bytes");
	let mut thumbnail = RsnapOwnedRgbaRegion::default();
	let status = unsafe {
		crate::rsnap_capture_frame_wallpaper_png_thumbnail(path.as_ptr(), 64, &mut thumbnail)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert!(thumbnail.width <= 64);
	assert!(thumbnail.height <= 64);
	assert_eq!(thumbnail.len, thumbnail.width as usize * thumbnail.height as usize * 4);

	unsafe {
		crate::rsnap_owned_rgba_region_release(&mut thumbnail);
	}

	let _ = fs::remove_file(path_buf);
}

#[test]
fn ffi_capture_frame_render_rgba_returns_owned_composition() {
	let rgba = [255; 4 * 2 * 4];
	let mut rendered = RsnapOwnedRgbaRegion::default();
	let status = unsafe {
		crate::rsnap_capture_frame_render_rgba(
			4,
			2,
			rgba.as_ptr(),
			rgba.len(),
			2.0,
			RsnapCaptureFrameSourceKind::DragRegion,
			RsnapCaptureFrameBackgroundKind::Aurora,
			RsnapCaptureFrameRenderKind::WindowSnapshot,
			ptr::null(),
			&mut rendered,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(rendered.width, 100);
	assert_eq!(rendered.height, 98);
	assert_eq!(rendered.len, 100 * 98 * 4);

	let bytes = unsafe { slice::from_raw_parts(rendered.rgba, rendered.len) };
	let first_source_pixel = ((48 * rendered.width as usize) + 48) * 4;

	assert_eq!(&bytes[first_source_pixel..first_source_pixel + 4], &[255, 255, 255, 255]);

	unsafe {
		crate::rsnap_owned_rgba_region_release(&mut rendered);
	}
}

#[test]
fn ffi_scroll_minimap_plan_returns_core_geometry() {
	let mut plan = RsnapScrollMinimapPlan::default();
	let status = unsafe {
		crate::rsnap_scroll_minimap_plan(
			RsnapFloatRect { x: 100.0, y: 100.0, width: 100.0, height: 100.0 },
			100.0,
			200.0,
			RsnapFloatRect { x: 0.0, y: 0.0, width: 500.0, height: 500.0 },
			96.0,
			44.0,
			10.0,
			10.0,
			3.0,
			20.0,
			100.0,
			&mut plan,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(plan.frame, RsnapFloatRect { x: 210.0, y: 54.0, width: 96.0, height: 192.0 });
	assert_eq!(plan.image_frame, RsnapFloatRect { x: 213.0, y: 57.0, width: 90.0, height: 186.0 });
	assert_eq!(plan.has_viewport_frame, 1);
	assert_eq!(
		plan.viewport_frame,
		RsnapFloatRect { x: 213.0, y: 131.4, width: 90.0, height: 93.0 }
	);
}

#[test]
fn ffi_scroll_minimap_plan_returns_empty_when_too_tight() {
	let mut plan = RsnapScrollMinimapPlan::default();
	let status = unsafe {
		crate::rsnap_scroll_minimap_plan(
			RsnapFloatRect { x: 100.0, y: 100.0, width: 100.0, height: 100.0 },
			100.0,
			200.0,
			RsnapFloatRect { x: 0.0, y: 0.0, width: 230.0, height: 60.0 },
			96.0,
			44.0,
			10.0,
			10.0,
			3.0,
			20.0,
			100.0,
			&mut plan,
		)
	};

	assert_eq!(status, RsnapStatus::Empty);
}

#[test]
fn ffi_frozen_selection_transform_hit_test_returns_core_kind() {
	let mut kind = RsnapFrozenSelectionTransformKind::Move;
	let status = unsafe {
		crate::rsnap_frozen_selection_transform_hit_test(
			102.0,
			238.0,
			RsnapFloatRect { x: 100.0, y: 80.0, width: 240.0, height: 160.0 },
			12.0,
			4.0,
			&mut kind,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(kind, RsnapFrozenSelectionTransformKind::ResizeTopLeft);
}

#[test]
fn ffi_frozen_selection_transform_rect_returns_core_rect() {
	let mut rect = RsnapFloatRect::default();
	let status = unsafe {
		crate::rsnap_frozen_selection_transform_rect(
			RsnapFrozenSelectionTransformKind::ResizeBottomRight,
			RsnapFloatRect { x: 100.0, y: 80.0, width: 240.0, height: 160.0 },
			RsnapFloatRect { x: 0.0, y: 0.0, width: 500.0, height: 400.0 },
			340.0,
			80.0,
			50.0,
			300.0,
			12.0,
			&mut rect,
		)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(rect, RsnapFloatRect { x: 100.0, y: 228.0, width: 12.0, height: 12.0 });
}

#[test]
fn ffi_auto_center_content_bounds_returns_core_rect() {
	let rgba =
		auto_center_frame(100, 80, Some(RsnapPixelRect { x: 30, y: 20, width: 24, height: 18 }));
	let mut rect = RsnapPixelRect::default();
	let status = unsafe {
		crate::rsnap_auto_center_content_bounds_rgba(100, 80, rgba.as_ptr(), rgba.len(), &mut rect)
	};

	assert_eq!(status, RsnapStatus::Ok);
	assert_eq!(rect, RsnapPixelRect { x: 30, y: 20, width: 24, height: 18 });
	assert_eq!(crate::rsnap_auto_center_margin_balance_shift_points(30.0, 24.0, 100.0, 50.0), -4.0);
}

#[test]
fn ffi_auto_center_content_bounds_returns_empty_for_uniform_image() {
	let rgba = auto_center_frame(100, 80, None);
	let mut rect = RsnapPixelRect::default();
	let status = unsafe {
		crate::rsnap_auto_center_content_bounds_rgba(100, 80, rgba.as_ptr(), rgba.len(), &mut rect)
	};

	assert_eq!(status, RsnapStatus::Empty);
}

#[cfg(target_os = "macos")]
#[test]
fn ffi_scroll_session_observes_downward_frame_and_exports() {
	let base = scroll_frame(16, 96, 0);
	let moved = scroll_frame(16, 96, 24);
	let handle =
		unsafe { crate::rsnap_scroll_session_create(16, 96, base.as_ptr(), base.len(), 8) };

	assert!(!handle.is_null());

	let mut result = RsnapScrollObserveResult::default();
	let observe_status = unsafe {
		crate::rsnap_scroll_session_observe_downward_frame(
			handle,
			16,
			96,
			moved.as_ptr(),
			moved.len(),
			&mut result,
		)
	};

	assert_eq!(observe_status, RsnapStatus::Ok);
	assert_eq!(result.kind, RsnapScrollObserveOutcomeKind::Committed as u32);
	assert_eq!(result.growth_rows, 24);
	assert_eq!(result.export_width, 16);
	assert_eq!(result.export_height, 120);
	assert_eq!(result.current_viewport_top_y, 24);

	let mut export = RsnapOwnedRgbaRegion::default();

	assert_eq!(
		unsafe { crate::rsnap_scroll_session_take_export_rgba(handle, &mut export) },
		RsnapStatus::Ok
	);
	assert_eq!(export.width, 16);
	assert_eq!(export.height, 120);
	assert_eq!(export.len, 16 * 120 * 4);

	let mut preview = RsnapOwnedRgbaRegion::default();

	assert_eq!(
		unsafe { crate::rsnap_scroll_session_take_preview_rgba(handle, &mut preview) },
		RsnapStatus::Ok
	);
	assert_eq!(preview.width, 8);
	assert_eq!(preview.height, 60);
	assert_eq!(preview.len, 8 * 60 * 4);

	unsafe {
		crate::rsnap_owned_rgba_region_release(&mut export);
		crate::rsnap_owned_rgba_region_release(&mut preview);
		crate::rsnap_scroll_session_destroy(handle);
	}
}

#[cfg(target_os = "macos")]
#[test]
fn ffi_scroll_session_blocks_rewind_until_frontier_is_reacquired() {
	let base = scroll_frame(16, 128, 0);
	let first = scroll_frame(16, 128, 48);
	let rewind = scroll_frame(16, 128, 24);
	let below_frontier = scroll_frame(16, 128, 36);
	let reacquired = scroll_frame(16, 128, 48);
	let beyond_frontier = scroll_frame(16, 128, 60);
	let handle =
		unsafe { crate::rsnap_scroll_session_create(16, 128, base.as_ptr(), base.len(), 16) };

	assert!(!handle.is_null());

	let mut result = RsnapScrollObserveResult::default();

	assert_eq!(
		unsafe {
			crate::rsnap_scroll_session_observe_downward_frame(
				handle,
				16,
				128,
				first.as_ptr(),
				first.len(),
				&mut result,
			)
		},
		RsnapStatus::Ok
	);
	assert_eq!(result.kind, RsnapScrollObserveOutcomeKind::Committed as u32);
	assert_eq!(result.current_viewport_top_y, 48);
	assert_eq!(result.export_height, 176);

	for frame in [&rewind, &below_frontier, &reacquired] {
		assert_eq!(
			unsafe {
				crate::rsnap_scroll_session_observe_downward_frame(
					handle,
					16,
					128,
					frame.as_ptr(),
					frame.len(),
					&mut result,
				)
			},
			RsnapStatus::Ok
		);
		assert_ne!(result.kind, RsnapScrollObserveOutcomeKind::Committed as u32);
		assert_eq!(result.current_viewport_top_y, 48);
		assert_eq!(result.export_height, 176);
	}

	assert_eq!(
		unsafe {
			crate::rsnap_scroll_session_observe_downward_frame(
				handle,
				16,
				128,
				beyond_frontier.as_ptr(),
				beyond_frontier.len(),
				&mut result,
			)
		},
		RsnapStatus::Ok
	);
	assert_eq!(result.kind, RsnapScrollObserveOutcomeKind::Committed as u32);
	assert_eq!(result.growth_rows, 12);
	assert_eq!(result.current_viewport_top_y, 60);
	assert_eq!(result.export_height, 188);

	unsafe {
		crate::rsnap_scroll_session_destroy(handle);
	}
}

#[test]
fn ffi_click_freeze_request_carries_fixed_selection_payload() {
	let handle = unsafe { crate::rsnap_session_create(default_config()) };
	let mut request = RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };

	assert_eq!(unsafe { crate::rsnap_session_enter_live(handle) }, RsnapStatus::Ok);

	let _ = unsafe { crate::rsnap_session_take_next_request(handle, &mut request) };

	assert_eq!(
		unsafe {
			crate::rsnap_session_handle_host_event(
				handle,
				RsnapHostEvent {
					kind: RsnapHostEventKind::PrimaryInteractionCompleted as u32,
					point: RsnapPoint { x: 80, y: 110 },
					has_point: 1,
					rgb: RsnapRgb::default(),
					has_rgb: 0,
					active_monitor: RsnapMonitorRect {
						id: 9,
						origin: RsnapPoint { x: 0, y: 0 },
						width: 1_440,
						height: 900,
						scale_factor_x1000: 2_000,
					},
					has_active_monitor: 1,
					highlighted_window: RsnapWindowRect {
						window_id: 42,
						has_window_id: 1,
						x: 20,
						y: 30,
						width: 60,
						height: 80,
					},
					has_highlighted_window: 1,
					toolbar_item_kind: 0,
				},
			)
		},
		RsnapStatus::Ok
	);
	assert_eq!(
		unsafe { crate::rsnap_session_take_next_request(handle, &mut request) },
		RsnapStatus::Ok
	);
	assert_eq!(request.kind, RsnapHostRequestKind::RequestFreezeSnapshot as u32);
	assert_eq!(request.has_selection, 1);
	assert_eq!(request.selection, RsnapRect { x: 20, y: 30, width: 60, height: 80 });
	assert_eq!(request.selection_editable, 0);

	unsafe { crate::rsnap_session_destroy(handle) };
}

#[test]
fn ffi_drag_freeze_request_carries_editable_selection_payload() {
	let handle = unsafe { crate::rsnap_session_create(default_config()) };
	let mut request = RsnapHostRequestValue { kind: u32::MAX, ..RsnapHostRequestValue::default() };

	assert_eq!(unsafe { crate::rsnap_session_enter_live(handle) }, RsnapStatus::Ok);

	let _ = unsafe { crate::rsnap_session_take_next_request(handle, &mut request) };

	assert_eq!(
		unsafe {
			crate::rsnap_session_handle_host_event(
				handle,
				RsnapHostEvent {
					kind: RsnapHostEventKind::PrimaryInteractionStarted as u32,
					point: RsnapPoint { x: 80, y: 110 },
					has_point: 1,
					rgb: RsnapRgb::default(),
					has_rgb: 0,
					active_monitor: RsnapMonitorRect {
						id: 9,
						origin: RsnapPoint { x: 0, y: 0 },
						width: 1_440,
						height: 900,
						scale_factor_x1000: 2_000,
					},
					has_active_monitor: 1,
					highlighted_window: RsnapWindowRect {
						window_id: 42,
						has_window_id: 1,
						x: 20,
						y: 30,
						width: 60,
						height: 80,
					},
					has_highlighted_window: 1,
					toolbar_item_kind: 0,
				},
			)
		},
		RsnapStatus::Ok
	);
	assert_eq!(
		unsafe {
			crate::rsnap_session_handle_host_event(
				handle,
				RsnapHostEvent {
					kind: RsnapHostEventKind::PrimaryInteractionCompleted as u32,
					point: RsnapPoint { x: 140, y: 190 },
					has_point: 1,
					rgb: RsnapRgb::default(),
					has_rgb: 0,
					active_monitor: RsnapMonitorRect {
						id: 9,
						origin: RsnapPoint { x: 0, y: 0 },
						width: 1_440,
						height: 900,
						scale_factor_x1000: 2_000,
					},
					has_active_monitor: 1,
					highlighted_window: RsnapWindowRect {
						window_id: 42,
						has_window_id: 1,
						x: 20,
						y: 30,
						width: 60,
						height: 80,
					},
					has_highlighted_window: 1,
					toolbar_item_kind: 0,
				},
			)
		},
		RsnapStatus::Ok
	);
	assert_eq!(
		unsafe { crate::rsnap_session_take_next_request(handle, &mut request) },
		RsnapStatus::Ok
	);
	assert_eq!(request.kind, RsnapHostRequestKind::RequestFreezeSnapshot as u32);
	assert_eq!(request.has_selection, 1);
	assert_eq!(request.selection, RsnapRect { x: 80, y: 110, width: 60, height: 80 });
	assert_eq!(request.selection_editable, 1);

	unsafe { crate::rsnap_session_destroy(handle) };
}

#[test]
fn abi_version_matches_constant() {
	assert_eq!(crate::rsnap_host_ffi_abi_version(), RSNAP_HOST_FFI_ABI_VERSION);
}

fn auto_center_frame(width: u32, height: u32, content: Option<RsnapPixelRect>) -> Vec<u8> {
	let mut rgba = vec![180_u8; (width * height * 4) as usize];

	for pixel in rgba.chunks_exact_mut(4) {
		pixel[3] = 255;
	}

	if let Some(content) = content {
		for y in content.y..content.y + content.height {
			for x in content.x..content.x + content.width {
				let offset = ((y * width + x) * 4) as usize;

				rgba[offset] = 24;
				rgba[offset + 1] = 32;
				rgba[offset + 2] = 40;
			}
		}
	}

	rgba
}

fn bgra_frame(width: u32, height: u32, bytes_per_row: usize) -> Vec<u8> {
	let mut bytes = vec![0xEE; bytes_per_row * height as usize];

	for y in 0..height {
		for x in 0..width {
			let offset = y as usize * bytes_per_row + x as usize * 4;

			bytes[offset] = 30 + y as u8 * 15 + x as u8;
			bytes[offset + 1] = 20 + y as u8 * 10 + x as u8;
			bytes[offset + 2] = 10 + y as u8 * 5 + x as u8;
			bytes[offset + 3] = 200 + y as u8 + x as u8;
		}
	}

	bytes
}
