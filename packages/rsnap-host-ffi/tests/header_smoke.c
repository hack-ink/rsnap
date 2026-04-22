#include "rsnap_host_ffi.h"

int main(void) {
	RsnapSessionConfig config = {
		.platform = RSNAP_PLATFORM_MACOS,
		.allow_text_input = 1,
		.prefers_toolbar_above_selection = 0,
	};
	RsnapHostRequestValue request = {0};
	RsnapSceneModel scene = {0};
	RsnapSessionHandle *handle = rsnap_session_create(config);

	if (handle == 0) {
		return 1;
	}
	if (rsnap_host_ffi_abi_version() != RSNAP_HOST_FFI_ABI_VERSION) {
		return 2;
	}
	if (rsnap_session_enter_live(handle) != RSNAP_STATUS_OK) {
		rsnap_session_destroy(handle);
		return 3;
	}
	if (rsnap_session_take_next_request(handle, &request) != RSNAP_STATUS_OK) {
		rsnap_session_destroy(handle);
		return 4;
	}
	if (request.kind != RSNAP_HOST_REQUEST_START_LIVE_CAPTURE) {
		rsnap_session_destroy(handle);
		return 5;
	}
	if (rsnap_session_copy_scene_model(handle, &scene) != RSNAP_STATUS_OK) {
		rsnap_session_destroy(handle);
		return 6;
	}
	if (scene.scene_kind != RSNAP_SCENE_LIVE) {
		rsnap_session_destroy(handle);
		return 7;
	}

	rsnap_session_destroy(handle);
	return 0;
}
