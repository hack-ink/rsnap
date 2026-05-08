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
	RsnapFloatRect display_frame = {.x = 0.0, .y = 0.0, .width = 1440.0, .height = 900.0};
	RsnapFloatRect selection = {.x = 100.0, .y = 200.0, .width = 300.0, .height = 150.0};
	RsnapFloatRect mosaic_source = {.x = 4.2, .y = 9.1, .width = 28.4, .height = 21.0};
	uint8_t rgba[4 * 4 * 4] = {0};
	RsnapSessionHandle *handle = rsnap_session_create(config);
	RsnapScrollSessionHandle *scroll_handle =
		rsnap_scroll_session_create(4, 4, rgba, sizeof(rgba), 4);

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
