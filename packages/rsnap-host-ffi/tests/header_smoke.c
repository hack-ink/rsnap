#include "rsnap_host_ffi.h"

int main(void) {
	RsnapSessionConfig config = {
		.platform = RSNAP_PLATFORM_MACOS,
		.allow_text_input = 1,
		.prefers_toolbar_above_selection = 0,
	};
	RsnapHostRequestValue request = {0};
	RsnapSceneModel scene = {0};
	RsnapScrollObserveResult scroll_result = {0};
	RsnapOwnedRgbaRegion scroll_export = {0};
	RsnapOwnedRgbaRegion mosaic_patch = {0};
	RsnapOwnedBytes png_export = {0};
	RsnapPixelRect crop = {.x = 0, .y = 0, .width = 2, .height = 2};
	RsnapPixelRect display_crop = {0};
	RsnapCaptureFramePlan frame_plan = {0};
	RsnapCaptureFrameBackgroundPlan background_plan = {0};
	RsnapCaptureFrameWallpaperRequest wallpaper_request = {0};
	RsnapScrollMinimapPlan minimap_plan = {0};
	RsnapPixelRect auto_center_rect = {0};
	RsnapFloatRect aspect_crop = {0};
	RsnapFloatRect display_frame = {.x = 0.0, .y = 0.0, .width = 1440.0, .height = 900.0};
	RsnapFloatRect selection = {.x = 100.0, .y = 200.0, .width = 300.0, .height = 150.0};
	RsnapFloatRect mosaic_source = {.x = 4.2, .y = 9.1, .width = 28.4, .height = 21.0};
	uint8_t rgba[4 * 4 * 4] = {0};
	uint8_t auto_center_rgba[4 * 4 * 4] = {0};
	RsnapSessionHandle *handle = rsnap_session_create(config);
	RsnapScrollSessionHandle *scroll_handle =
		rsnap_scroll_session_create(4, 4, rgba, sizeof(rgba), 4);
	for (size_t index = 0; index < sizeof(auto_center_rgba); index += 4) {
		auto_center_rgba[index] = 180;
		auto_center_rgba[index + 1] = 180;
		auto_center_rgba[index + 2] = 180;
		auto_center_rgba[index + 3] = 255;
	}
	auto_center_rgba[(1 * 4 + 1) * 4] = 24;
	auto_center_rgba[(1 * 4 + 1) * 4 + 1] = 32;
	auto_center_rgba[(1 * 4 + 1) * 4 + 2] = 40;

	if (handle == 0) {
		return 1;
	}
	if (scroll_handle == 0) {
		rsnap_session_destroy(handle);
		return 8;
	}
	if (rsnap_host_ffi_abi_version() != RSNAP_HOST_FFI_ABI_VERSION) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 2;
	}
	if (rsnap_scroll_session_observe_downward_frame(
			scroll_handle,
			4,
			4,
			rgba,
			sizeof(rgba),
			&scroll_result
		) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 9;
	}
	if (rsnap_scroll_session_take_export_rgba(scroll_handle, &scroll_export) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 10;
	}
	rsnap_owned_rgba_region_release(&scroll_export);
	if (rsnap_export_rgba_to_png(4, 4, rgba, sizeof(rgba), &png_export) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 11;
	}
	rsnap_owned_bytes_release(&png_export);
	if (rsnap_export_rgba_crop_to_png(4, 4, rgba, sizeof(rgba), crop, &png_export) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 12;
	}
	rsnap_owned_bytes_release(&png_export);
	if (rsnap_frozen_display_crop_rect(2880, 1800, display_frame, selection, &display_crop) !=
		RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 13;
	}
	if (display_crop.x != 200 || display_crop.y != 1100 || display_crop.width != 600 ||
		display_crop.height != 300) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 14;
	}
	if (rsnap_frozen_mosaic_light_privacy_patch_rgba(100, 80, mosaic_source, &mosaic_patch) !=
		RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 15;
	}
	if (mosaic_patch.width != 3 || mosaic_patch.height != 3 || mosaic_patch.len != 36) {
		rsnap_owned_rgba_region_release(&mosaic_patch);
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 16;
	}
	rsnap_owned_rgba_region_release(&mosaic_patch);
	if (rsnap_capture_frame_plan(
			320,
			180,
			2.0,
			RSNAP_CAPTURE_FRAME_SOURCE_WINDOW,
			&frame_plan
		) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 17;
	}
	if (frame_plan.canvas_width != 416.0 || frame_plan.canvas_height != 276.0 ||
		frame_plan.image_rect.x != 48.0 || frame_plan.corner_radius != 9.9) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 18;
	}
	if (rsnap_capture_frame_aspect_fill_crop_rect(1600, 900, 1000.0, 1000.0, &aspect_crop) !=
		RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 19;
	}
	if (aspect_crop.x != 350.0 || aspect_crop.y != 0.0 || aspect_crop.width != 900.0 ||
		aspect_crop.height != 900.0) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 20;
	}
	if (rsnap_capture_frame_background_plan(
			RSNAP_CAPTURE_FRAME_BACKGROUND_SYSTEM_WALLPAPER,
			&background_plan
		) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 21;
	}
	if (background_plan.prefers_wallpaper != 1 || background_plan.wallpaper_overlay_alpha != 0.10 ||
		background_plan.locations[1] != 0.54 || background_plan.colors[2].red != 0.95) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 22;
	}
	if (rsnap_capture_frame_wallpaper_request_plan(
			RSNAP_CAPTURE_FRAME_BACKGROUND_SYSTEM_WALLPAPER,
			1535.2,
			996.0,
			&wallpaper_request
		) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 27;
	}
	if (wallpaper_request.target_pixel_size != 1536 ||
		wallpaper_request.overlay_alpha != 0.10) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 28;
	}
	if (rsnap_scroll_minimap_plan(
			(RsnapFloatRect){.x = 100.0, .y = 100.0, .width = 100.0, .height = 100.0},
			100.0,
			200.0,
			(RsnapFloatRect){.x = 0.0, .y = 0.0, .width = 500.0, .height = 500.0},
			96.0,
			44.0,
			10.0,
			10.0,
			3.0,
			20.0,
			100.0,
			&minimap_plan
		) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 25;
	}
	if (minimap_plan.frame.x != 210.0 || minimap_plan.frame.y != 54.0 ||
		minimap_plan.frame.width != 96.0 || minimap_plan.frame.height != 192.0 ||
		minimap_plan.image_frame.x != 213.0 || minimap_plan.has_viewport_frame != 1 ||
		minimap_plan.viewport_frame.height != 93.0) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 26;
	}
	if (rsnap_auto_center_content_bounds_rgba(
			4,
			4,
			auto_center_rgba,
			sizeof(auto_center_rgba),
			&auto_center_rect
		) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 23;
	}
	if (auto_center_rect.x != 1 || auto_center_rect.y != 1 || auto_center_rect.width != 1 ||
		auto_center_rect.height != 1 ||
		rsnap_auto_center_margin_balance_shift_points(1.0, 1.0, 4.0, 40.0) != -5.0) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 24;
	}
	if (rsnap_session_enter_live(handle) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 3;
	}
	if (rsnap_session_take_next_request(handle, &request) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 4;
	}
	if (request.kind != RSNAP_HOST_REQUEST_START_LIVE_CAPTURE) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 5;
	}
	if (rsnap_session_copy_scene_model(handle, &scene) != RSNAP_STATUS_OK) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 6;
	}
	if (scene.scene_kind != RSNAP_SCENE_LIVE) {
		rsnap_scroll_session_destroy(scroll_handle);
		rsnap_session_destroy(handle);
		return 7;
	}

	rsnap_scroll_session_destroy(scroll_handle);
	rsnap_session_destroy(handle);
	return 0;
}
