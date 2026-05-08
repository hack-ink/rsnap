#ifndef RSNAP_HOST_FFI_H
#define RSNAP_HOST_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSNAP_HOST_FFI_ABI_VERSION 20u
#define RSNAP_TOOLBAR_ITEM_CAPACITY 16u
#define RSNAP_STATUS_MESSAGE_CAPACITY 256u
#define RSNAP_LIVE_SAMPLE_PATCH_CAPACITY 4096u

typedef struct RsnapSessionHandle RsnapSessionHandle;
typedef struct RsnapLiveSamplerHandle RsnapLiveSamplerHandle;
typedef struct RsnapScrollSessionHandle RsnapScrollSessionHandle;

typedef enum RsnapStatus {
	RSNAP_STATUS_OK = 0,
	RSNAP_STATUS_NULL_HANDLE = 1,
	RSNAP_STATUS_NULL_OUTPUT = 2,
	RSNAP_STATUS_EMPTY = 3,
	RSNAP_STATUS_INVALID_INPUT = 4,
} RsnapStatus;

typedef enum RsnapPlatformTag {
	RSNAP_PLATFORM_MACOS = 0,
	RSNAP_PLATFORM_WINDOWS = 1,
	RSNAP_PLATFORM_LINUX = 2,
	RSNAP_PLATFORM_UNSUPPORTED = 3,
} RsnapPlatformTag;

typedef struct RsnapSessionConfig {
	enum RsnapPlatformTag platform;
	uint8_t allow_text_input;
	uint8_t prefers_toolbar_above_selection;
} RsnapSessionConfig;

typedef struct RsnapPoint {
	int32_t x;
	int32_t y;
} RsnapPoint;

typedef struct RsnapRgb {
	uint8_t r;
	uint8_t g;
	uint8_t b;
} RsnapRgb;

typedef struct RsnapRect {
	int32_t x;
	int32_t y;
	uint32_t width;
	uint32_t height;
} RsnapRect;

typedef struct RsnapMonitorRect {
	uint32_t id;
	struct RsnapPoint origin;
	uint32_t width;
	uint32_t height;
	uint32_t scale_factor_x1000;
} RsnapMonitorRect;

typedef struct RsnapWindowRect {
	uint32_t window_id;
	uint8_t has_window_id;
	int64_t x;
	int64_t y;
	int64_t width;
	int64_t height;
} RsnapWindowRect;

typedef enum RsnapHostEventKind {
	RSNAP_HOST_EVENT_SESSION_ACTIVATED = 0,
	RSNAP_HOST_EVENT_POINTER_MOVED = 1,
	RSNAP_HOST_EVENT_CANCEL_REQUESTED = 3,
	RSNAP_HOST_EVENT_COPY_REQUESTED = 4,
	RSNAP_HOST_EVENT_SAVE_REQUESTED = 5,
	RSNAP_HOST_EVENT_RECOGNIZE_TEXT_REQUESTED = 6,
	RSNAP_HOST_EVENT_TOGGLE_LOUPE = 7,
	RSNAP_HOST_EVENT_TOOLBAR_ITEM_INVOKED = 8,
	RSNAP_HOST_EVENT_PRIMARY_INTERACTION_STARTED = 9,
	RSNAP_HOST_EVENT_PRIMARY_INTERACTION_UPDATED = 10,
	RSNAP_HOST_EVENT_PRIMARY_INTERACTION_COMPLETED = 11,
} RsnapHostEventKind;

typedef enum RsnapToolbarItemKind {
	RSNAP_TOOLBAR_ITEM_POINTER = 0,
	RSNAP_TOOLBAR_ITEM_PEN = 1,
	RSNAP_TOOLBAR_ITEM_ARROW = 2,
	RSNAP_TOOLBAR_ITEM_TEXT = 3,
	RSNAP_TOOLBAR_ITEM_MOSAIC = 4,
	RSNAP_TOOLBAR_ITEM_SPOTLIGHT = 5,
	RSNAP_TOOLBAR_ITEM_UNDO = 6,
	RSNAP_TOOLBAR_ITEM_REDO = 7,
	RSNAP_TOOLBAR_ITEM_AUTO_CENTER = 8,
	RSNAP_TOOLBAR_ITEM_SCROLL = 9,
	RSNAP_TOOLBAR_ITEM_OCR = 10,
	RSNAP_TOOLBAR_ITEM_COPY = 11,
	RSNAP_TOOLBAR_ITEM_SAVE = 12,
} RsnapToolbarItemKind;

typedef struct RsnapHostEvent {
	uint32_t kind;
	struct RsnapPoint point;
	uint8_t has_point;
	struct RsnapRgb rgb;
	uint8_t has_rgb;
	struct RsnapMonitorRect active_monitor;
	uint8_t has_active_monitor;
	struct RsnapWindowRect highlighted_window;
	uint8_t has_highlighted_window;
	uint32_t toolbar_item_kind;
} RsnapHostEvent;

typedef enum RsnapHostReportKind {
	RSNAP_HOST_REPORT_FREEZE_SNAPSHOT_COMMITTED = 0,
	RSNAP_HOST_REPORT_HOST_EFFECT_COMPLETED = 1,
	RSNAP_HOST_REPORT_PERMISSION_CHANGED = 2,
	RSNAP_HOST_REPORT_STATUS_MESSAGE = 3,
} RsnapHostReportKind;

typedef enum RsnapHostEffectKind {
	RSNAP_HOST_EFFECT_COPY_CAPTURE = 0,
	RSNAP_HOST_EFFECT_SAVE_CAPTURE = 1,
	RSNAP_HOST_EFFECT_RECOGNIZE_TEXT = 2,
} RsnapHostEffectKind;

typedef enum RsnapPermissionKind {
	RSNAP_PERMISSION_SCREEN_RECORDING = 0,
} RsnapPermissionKind;

typedef struct RsnapHostReport {
	uint32_t kind;
	struct RsnapRect selection;
	uint8_t has_selection;
	uint32_t effect_kind;
	uint32_t permission_kind;
	uint8_t granted;
	uint32_t status_message_len;
	uint8_t status_message[RSNAP_STATUS_MESSAGE_CAPACITY];
} RsnapHostReport;

typedef enum RsnapSceneKind {
	RSNAP_SCENE_HIDDEN = 0,
	RSNAP_SCENE_LIVE = 1,
	RSNAP_SCENE_FROZEN = 2,
} RsnapSceneKind;

typedef enum RsnapCursorIntent {
	RSNAP_CURSOR_DEFAULT = 0,
	RSNAP_CURSOR_CROSSHAIR = 1,
	RSNAP_CURSOR_GRAB = 2,
	RSNAP_CURSOR_GRABBING = 3,
	RSNAP_CURSOR_RESIZE_NORTH = 4,
	RSNAP_CURSOR_RESIZE_SOUTH = 5,
	RSNAP_CURSOR_RESIZE_EAST = 6,
	RSNAP_CURSOR_RESIZE_WEST = 7,
	RSNAP_CURSOR_RESIZE_NORTH_EAST = 8,
	RSNAP_CURSOR_RESIZE_NORTH_WEST = 9,
	RSNAP_CURSOR_RESIZE_SOUTH_EAST = 10,
	RSNAP_CURSOR_RESIZE_SOUTH_WEST = 11,
	RSNAP_CURSOR_TEXT = 12,
} RsnapCursorIntent;

typedef struct RsnapToolbarItem {
	uint32_t kind;
	uint8_t enabled;
	uint8_t selected;
	uint8_t present;
} RsnapToolbarItem;

typedef struct RsnapSceneModel {
	uint32_t scene_kind;
	uint32_t cursor_intent;
	struct RsnapPoint pointer;
	uint8_t has_pointer;
	struct RsnapMonitorRect active_monitor;
	uint8_t has_active_monitor;
	struct RsnapWindowRect highlighted_window;
	uint8_t has_highlighted_window;
	struct RsnapRect live_selection_preview;
	uint8_t has_live_selection_preview;
	struct RsnapRect frozen_selection;
	uint8_t has_frozen_selection;
	struct RsnapRgb rgb;
	uint8_t has_rgb;
	uint8_t loupe_visible;
	uint32_t toolbar_item_count;
	struct RsnapToolbarItem toolbar_items[RSNAP_TOOLBAR_ITEM_CAPACITY];
	uint32_t status_message_len;
	uint8_t status_message[RSNAP_STATUS_MESSAGE_CAPACITY];
} RsnapSceneModel;

typedef enum RsnapHostRequestKind {
	RSNAP_HOST_REQUEST_START_LIVE_CAPTURE = 0,
	RSNAP_HOST_REQUEST_STOP_LIVE_CAPTURE = 1,
	RSNAP_HOST_REQUEST_REQUEST_FREEZE_SNAPSHOT = 2,
	RSNAP_HOST_REQUEST_COPY_CAPTURE = 3,
	RSNAP_HOST_REQUEST_SAVE_CAPTURE = 4,
	RSNAP_HOST_REQUEST_RECOGNIZE_TEXT = 5,
	RSNAP_HOST_REQUEST_REQUEST_SCREEN_RECORDING_PERMISSION = 6,
	RSNAP_HOST_REQUEST_START_SCROLL_CAPTURE = 9,
} RsnapHostRequestKind;

typedef struct RsnapHostRequestValue {
	uint32_t kind;
	struct RsnapRect selection;
	uint8_t has_selection;
	uint8_t selection_editable;
} RsnapHostRequestValue;

typedef struct RsnapLiveSample {
	struct RsnapRgb rgb;
	uint8_t has_rgb;
	uint8_t has_frame_metadata;
	uint64_t frame_age_micros;
	uint64_t frame_seq;
	uint64_t stream_generation;
	uint32_t patch_width;
	uint32_t patch_height;
	uint32_t patch_len;
	uint8_t patch_rgba[RSNAP_LIVE_SAMPLE_PATCH_CAPACITY];
} RsnapLiveSample;

typedef struct RsnapRgbaRegion {
	uint32_t width;
	uint32_t height;
	size_t len;
	size_t capacity;
	uint8_t *rgba;
} RsnapRgbaRegion;

typedef struct RsnapOwnedRgbaRegion {
	uint32_t width;
	uint32_t height;
	size_t len;
	size_t capacity;
	uint8_t *rgba;
} RsnapOwnedRgbaRegion;

typedef struct RsnapOwnedBytes {
	size_t len;
	size_t capacity;
	uint8_t *bytes;
} RsnapOwnedBytes;

typedef struct RsnapPixelRect {
	uint32_t x;
	uint32_t y;
	uint32_t width;
	uint32_t height;
} RsnapPixelRect;

typedef struct RsnapFloatRect {
	double x;
	double y;
	double width;
	double height;
} RsnapFloatRect;

typedef enum RsnapScrollObserveOutcomeKind {
	RSNAP_SCROLL_OBSERVE_NO_CHANGE = 0,
	RSNAP_SCROLL_OBSERVE_PREVIEW_UPDATED = 1,
	RSNAP_SCROLL_OBSERVE_COMMITTED = 2,
	RSNAP_SCROLL_OBSERVE_UNSUPPORTED_DIRECTION = 3,
} RsnapScrollObserveOutcomeKind;

typedef struct RsnapScrollObserveResult {
	uint32_t kind;
	uint32_t growth_rows;
	uint32_t export_width;
	uint32_t export_height;
	int32_t current_viewport_top_y;
} RsnapScrollObserveResult;

uint32_t rsnap_host_ffi_abi_version(void);
RsnapSessionHandle *rsnap_session_create(struct RsnapSessionConfig config);
RsnapScrollSessionHandle *rsnap_scroll_session_create(
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	uint32_t preview_width_px
);
RsnapLiveSamplerHandle *rsnap_live_sampler_create(void);
RsnapLiveSamplerHandle *rsnap_live_sampler_create_with_self_capture_exception_window_ids(
	const uint32_t *window_ids,
	size_t window_id_count
);
enum RsnapStatus rsnap_live_sampler_prime_monitor(
	RsnapLiveSamplerHandle *handle,
	struct RsnapMonitorRect monitor
);
enum RsnapStatus rsnap_live_sampler_reset(
	RsnapLiveSamplerHandle *handle
);
void rsnap_session_destroy(RsnapSessionHandle *handle);
void rsnap_scroll_session_destroy(RsnapScrollSessionHandle *handle);
void rsnap_live_sampler_destroy(RsnapLiveSamplerHandle *handle);
enum RsnapStatus rsnap_scroll_session_observe_downward_frame(
	RsnapScrollSessionHandle *handle,
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	struct RsnapScrollObserveResult *out_result
);
enum RsnapStatus rsnap_scroll_session_take_export_rgba(
	RsnapScrollSessionHandle *handle,
	struct RsnapOwnedRgbaRegion *out_region
);
enum RsnapStatus rsnap_scroll_session_undo_last_append(
	RsnapScrollSessionHandle *handle,
	struct RsnapScrollObserveResult *out_result
);
enum RsnapStatus rsnap_export_rgba_to_png(
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	struct RsnapOwnedBytes *out_png
);
enum RsnapStatus rsnap_export_rgba_crop_to_png(
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	struct RsnapPixelRect crop_rect,
	struct RsnapOwnedBytes *out_png
);
enum RsnapStatus rsnap_frozen_display_crop_rect(
	uint32_t image_width,
	uint32_t image_height,
	struct RsnapFloatRect display_frame,
	struct RsnapFloatRect selection,
	struct RsnapPixelRect *out_rect
);
enum RsnapStatus rsnap_session_enter_live(RsnapSessionHandle *handle);
enum RsnapStatus rsnap_session_handle_host_event(
	RsnapSessionHandle *handle,
	struct RsnapHostEvent event
);
enum RsnapStatus rsnap_session_handle_host_report(
	RsnapSessionHandle *handle,
	struct RsnapHostReport report
);
enum RsnapStatus rsnap_session_copy_scene_model(
	const RsnapSessionHandle *handle,
	struct RsnapSceneModel *out_scene
);
enum RsnapStatus rsnap_session_take_next_request(
	RsnapSessionHandle *handle,
	struct RsnapHostRequestValue *out_request
);
enum RsnapStatus rsnap_live_sampler_sample_cursor(
	RsnapLiveSamplerHandle *handle,
	struct RsnapMonitorRect monitor,
	struct RsnapPoint point,
	uint32_t patch_width_px,
	uint32_t patch_height_px,
	struct RsnapLiveSample *out_sample
);
enum RsnapStatus rsnap_live_sampler_peek_region_rgba(
	RsnapLiveSamplerHandle *handle,
	struct RsnapMonitorRect monitor,
	struct RsnapRect rect,
	struct RsnapRgbaRegion *out_region
);
enum RsnapStatus rsnap_live_sampler_take_region_rgba(
	RsnapLiveSamplerHandle *handle,
	struct RsnapMonitorRect monitor,
	struct RsnapRect rect,
	struct RsnapOwnedRgbaRegion *out_region
);
enum RsnapStatus rsnap_live_sampler_peek_latest_monitor_rgba(
	RsnapLiveSamplerHandle *handle,
	struct RsnapMonitorRect monitor,
	struct RsnapRgbaRegion *out_region
);
enum RsnapStatus rsnap_live_sampler_take_latest_monitor_rgba(
	RsnapLiveSamplerHandle *handle,
	struct RsnapMonitorRect monitor,
	struct RsnapOwnedRgbaRegion *out_region
);
void rsnap_owned_rgba_region_release(struct RsnapOwnedRgbaRegion *region);
void rsnap_owned_bytes_release(struct RsnapOwnedBytes *bytes);

#ifdef __cplusplus
}
#endif

#endif
