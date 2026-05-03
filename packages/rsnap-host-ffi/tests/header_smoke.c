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
