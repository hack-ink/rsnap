#ifndef RSNAP_HOST_FFI_H
#define RSNAP_HOST_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSNAP_HOST_FFI_ABI_VERSION 39u
#define RSNAP_TOOLBAR_ITEM_CAPACITY 16u
#define RSNAP_STATUS_MESSAGE_CAPACITY 256u

typedef struct RsnapSessionHandle RsnapSessionHandle;
typedef struct RsnapScrollSessionHandle RsnapScrollSessionHandle;
typedef struct RsnapFrozenOverlayEditSessionHandle RsnapFrozenOverlayEditSessionHandle;

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

typedef struct RsnapFloatPoint {
	double x;
	double y;
} RsnapFloatPoint;

typedef enum RsnapFrozenAnnotationColor {
	RSNAP_FROZEN_ANNOTATION_COLOR_WHITE = 0,
	RSNAP_FROZEN_ANNOTATION_COLOR_YELLOW = 1,
	RSNAP_FROZEN_ANNOTATION_COLOR_GREEN = 2,
	RSNAP_FROZEN_ANNOTATION_COLOR_BLUE = 3,
	RSNAP_FROZEN_ANNOTATION_COLOR_RED = 4,
	RSNAP_FROZEN_ANNOTATION_COLOR_BLACK = 5,
} RsnapFrozenAnnotationColor;

typedef enum RsnapFrozenOverlayExportElementKind {
	RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_PEN = 0,
	RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_ARROW = 1,
	RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_MOSAIC = 2,
	RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_SPOTLIGHT = 3,
	RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_TEXT = 4,
} RsnapFrozenOverlayExportElementKind;

typedef struct RsnapFrozenOverlayExportElement {
	enum RsnapFrozenOverlayExportElementKind kind;
	struct RsnapFloatRect rect;
	struct RsnapFloatPoint start;
	struct RsnapFloatPoint end;
	const struct RsnapFloatPoint *points;
	size_t points_len;
	const char *text;
	double stroke_width_points;
	double border_width_points;
	double font_size_points;
	enum RsnapFrozenAnnotationColor color;
} RsnapFrozenOverlayExportElement;

typedef struct RsnapFrozenOverlayEditStyle {
	double stroke_width_points;
	enum RsnapFrozenAnnotationColor stroke_color;
	double spotlight_border_width_points;
	enum RsnapFrozenAnnotationColor spotlight_color;
	double text_font_size_points;
	enum RsnapFrozenAnnotationColor text_color;
} RsnapFrozenOverlayEditStyle;

typedef struct RsnapFrozenOverlayEditSnapshot {
	uint8_t can_undo;
	uint8_t can_redo;
	uint8_t keeps_frozen_selection_fixed;
	uint8_t is_moving_movable_annotation;
	uint8_t has_active_interaction;
	struct RsnapFrozenOverlayExportElement *elements;
	size_t elements_len;
	uint8_t has_preview_pen;
	struct RsnapFrozenOverlayExportElement preview_pen;
	uint8_t has_preview_arrow;
	struct RsnapFrozenOverlayExportElement preview_arrow;
	uint8_t has_preview_mosaic;
	struct RsnapFrozenOverlayExportElement preview_mosaic;
	uint8_t has_preview_spotlight;
	struct RsnapFrozenOverlayExportElement preview_spotlight;
	uint8_t has_preview_text;
	struct RsnapFrozenOverlayExportElement preview_text;
	uint8_t has_active_text_edit;
	struct RsnapFrozenOverlayExportElement active_text_edit;
} RsnapFrozenOverlayEditSnapshot;

typedef enum RsnapCaptureFrameSourceKind {
	RSNAP_CAPTURE_FRAME_SOURCE_DRAG_REGION = 0,
	RSNAP_CAPTURE_FRAME_SOURCE_WINDOW = 1,
	RSNAP_CAPTURE_FRAME_SOURCE_FULL_SCREEN = 2,
	RSNAP_CAPTURE_FRAME_SOURCE_SCROLL_CAPTURE = 3,
	RSNAP_CAPTURE_FRAME_SOURCE_UNKNOWN = 4,
} RsnapCaptureFrameSourceKind;

typedef enum RsnapCaptureFrameBackgroundKind {
	RSNAP_CAPTURE_FRAME_BACKGROUND_SYSTEM_WALLPAPER = 0,
	RSNAP_CAPTURE_FRAME_BACKGROUND_AURORA = 1,
	RSNAP_CAPTURE_FRAME_BACKGROUND_GRAPHITE = 2,
	RSNAP_CAPTURE_FRAME_BACKGROUND_LINEN = 3,
} RsnapCaptureFrameBackgroundKind;

typedef enum RsnapCaptureFrameRenderKind {
	RSNAP_CAPTURE_FRAME_RENDER_FRAMED_CAPTURE = 0,
	RSNAP_CAPTURE_FRAME_RENDER_WINDOW_SNAPSHOT = 1,
} RsnapCaptureFrameRenderKind;

typedef struct RsnapCaptureFrameColorStop {
	double red;
	double green;
	double blue;
	double alpha;
} RsnapCaptureFrameColorStop;

typedef struct RsnapCaptureFrameBackgroundPlan {
	struct RsnapCaptureFrameColorStop colors[3];
	double locations[3];
	uint8_t prefers_wallpaper;
	double wallpaper_overlay_alpha;
} RsnapCaptureFrameBackgroundPlan;

typedef struct RsnapCaptureFrameShadow {
	double offset_x;
	double offset_y;
	double blur;
	double alpha;
} RsnapCaptureFrameShadow;

typedef struct RsnapCaptureFramePlan {
	double canvas_width;
	double canvas_height;
	struct RsnapFloatRect image_rect;
	double corner_radius;
	struct RsnapCaptureFrameShadow shadows[3];
} RsnapCaptureFramePlan;

typedef struct RsnapCaptureFrameWallpaperRequest {
	uint32_t target_pixel_size;
	double overlay_alpha;
} RsnapCaptureFrameWallpaperRequest;

typedef struct RsnapScrollMinimapPlan {
	struct RsnapFloatRect frame;
	struct RsnapFloatRect image_frame;
	uint8_t has_viewport_frame;
	struct RsnapFloatRect viewport_frame;
} RsnapScrollMinimapPlan;

typedef enum RsnapFrozenSelectionTransformKind {
	RSNAP_FROZEN_SELECTION_TRANSFORM_MOVE = 0,
	RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_LEFT = 1,
	RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_RIGHT = 2,
	RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_TOP = 3,
	RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_BOTTOM = 4,
	RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_TOP_LEFT = 5,
	RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_TOP_RIGHT = 6,
	RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_BOTTOM_LEFT = 7,
	RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_BOTTOM_RIGHT = 8,
} RsnapFrozenSelectionTransformKind;

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
RsnapFrozenOverlayEditSessionHandle *rsnap_frozen_overlay_edit_session_create(void);
void rsnap_session_destroy(RsnapSessionHandle *handle);
void rsnap_scroll_session_destroy(RsnapScrollSessionHandle *handle);
void rsnap_frozen_overlay_edit_session_destroy(RsnapFrozenOverlayEditSessionHandle *handle);
enum RsnapStatus rsnap_frozen_overlay_edit_session_reset(
	RsnapFrozenOverlayEditSessionHandle *handle
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_begin(
	RsnapFrozenOverlayEditSessionHandle *handle,
	enum RsnapToolbarItemKind tool,
	struct RsnapFloatPoint point,
	struct RsnapFloatRect selection,
	struct RsnapFrozenOverlayEditStyle style,
	uint8_t *out_changed
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_update(
	RsnapFrozenOverlayEditSessionHandle *handle,
	struct RsnapFloatPoint point,
	struct RsnapFloatRect selection,
	uint8_t *out_changed
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_finish(
	RsnapFrozenOverlayEditSessionHandle *handle,
	struct RsnapFloatRect selection,
	uint8_t *out_changed
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_append_text(
	RsnapFrozenOverlayEditSessionHandle *handle,
	const char *text,
	uint8_t *out_changed
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_backspace_text(
	RsnapFrozenOverlayEditSessionHandle *handle,
	uint8_t *out_changed
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_commit_text(
	RsnapFrozenOverlayEditSessionHandle *handle,
	struct RsnapFrozenOverlayEditStyle style,
	uint8_t *out_changed
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_cancel_text(
	RsnapFrozenOverlayEditSessionHandle *handle
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_undo(
	RsnapFrozenOverlayEditSessionHandle *handle,
	uint8_t *out_changed
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_redo(
	RsnapFrozenOverlayEditSessionHandle *handle,
	uint8_t *out_changed
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_contains_movable_annotation(
	const RsnapFrozenOverlayEditSessionHandle *handle,
	struct RsnapFloatPoint point,
	uint8_t *out_contains
);
enum RsnapStatus rsnap_frozen_overlay_edit_session_copy_snapshot(
	const RsnapFrozenOverlayEditSessionHandle *handle,
	struct RsnapFrozenOverlayEditSnapshot *out_snapshot
);
void rsnap_frozen_overlay_edit_snapshot_release(
	struct RsnapFrozenOverlayEditSnapshot *snapshot
);
enum RsnapStatus rsnap_scroll_session_observe_downward_frame(
	RsnapScrollSessionHandle *handle,
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	struct RsnapScrollObserveResult *out_result
);
enum RsnapStatus rsnap_scroll_session_observe_downward_frame_with_motion_hint(
	RsnapScrollSessionHandle *handle,
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	uint32_t motion_rows_hint,
	uint8_t allow_burst_search,
	struct RsnapScrollObserveResult *out_result
);
enum RsnapStatus rsnap_scroll_session_take_export_rgba(
	RsnapScrollSessionHandle *handle,
	struct RsnapOwnedRgbaRegion *out_region
);
enum RsnapStatus rsnap_scroll_session_take_preview_rgba(
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
enum RsnapStatus rsnap_export_rgba_to_png_with_screen_scale(
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	uint32_t scale_factor_x1000,
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
enum RsnapStatus rsnap_export_rgba_crop_to_png_with_screen_scale(
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	struct RsnapPixelRect crop_rect,
	uint32_t scale_factor_x1000,
	struct RsnapOwnedBytes *out_png
);
enum RsnapStatus rsnap_frozen_overlay_export_render_rgba(
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	struct RsnapFloatRect selection,
	const struct RsnapFrozenOverlayExportElement *elements,
	size_t elements_len,
	struct RsnapOwnedRgbaRegion *out_region
);
enum RsnapStatus rsnap_frozen_display_crop_rect(
	uint32_t image_width,
	uint32_t image_height,
	struct RsnapFloatRect display_frame,
	struct RsnapFloatRect selection,
	struct RsnapPixelRect *out_rect
);
enum RsnapStatus rsnap_frozen_mosaic_light_privacy_patch_rgba(
	uint32_t image_width,
	uint32_t image_height,
	struct RsnapFloatRect source_rect,
	struct RsnapOwnedRgbaRegion *out_region
);
enum RsnapStatus rsnap_bgra_frame_sample_rgb(
	uint32_t width,
	uint32_t height,
	size_t bytes_per_row,
	const uint8_t *bgra,
	size_t bgra_len,
	struct RsnapFloatRect display_frame,
	double point_x,
	double point_y,
	struct RsnapRgb *out_rgb
);
enum RsnapStatus rsnap_bgra_frame_loupe_patch_rgba(
	uint32_t width,
	uint32_t height,
	size_t bytes_per_row,
	const uint8_t *bgra,
	size_t bgra_len,
	struct RsnapFloatRect display_frame,
	double point_x,
	double point_y,
	uint32_t side_pixels,
	struct RsnapOwnedRgbaRegion *out_region
);
enum RsnapStatus rsnap_capture_frame_plan(
	uint32_t image_width,
	uint32_t image_height,
	double screen_scale_factor,
	enum RsnapCaptureFrameSourceKind source_kind,
	struct RsnapCaptureFramePlan *out_plan
);
enum RsnapStatus rsnap_capture_frame_aspect_fill_crop_rect(
	uint32_t source_width,
	uint32_t source_height,
	double destination_width,
	double destination_height,
	struct RsnapFloatRect *out_rect
);
enum RsnapStatus rsnap_capture_frame_background_plan(
	enum RsnapCaptureFrameBackgroundKind background_kind,
	struct RsnapCaptureFrameBackgroundPlan *out_plan
);
enum RsnapStatus rsnap_capture_frame_wallpaper_request_plan(
	enum RsnapCaptureFrameBackgroundKind background_kind,
	double destination_width,
	double destination_height,
	struct RsnapCaptureFrameWallpaperRequest *out_request
);
enum RsnapStatus rsnap_capture_frame_wallpaper_png_thumbnail(
	const char *path,
	uint32_t target_pixel_size,
	struct RsnapOwnedRgbaRegion *out_region
);
enum RsnapStatus rsnap_capture_frame_render_rgba(
	uint32_t source_width,
	uint32_t source_height,
	const uint8_t *source_rgba,
	size_t source_rgba_len,
	double screen_scale_factor,
	enum RsnapCaptureFrameSourceKind source_kind,
	enum RsnapCaptureFrameBackgroundKind background_kind,
	enum RsnapCaptureFrameRenderKind render_kind,
	const char *wallpaper_path,
	struct RsnapOwnedRgbaRegion *out_region
);
enum RsnapStatus rsnap_scroll_minimap_plan(
	struct RsnapFloatRect selection,
	double export_width,
	double export_height,
	struct RsnapFloatRect bounds,
	double preferred_width,
	double minimum_width,
	double gap,
	double margin,
	double image_inset,
	double viewport_top_pixels,
	double viewport_height_pixels,
	struct RsnapScrollMinimapPlan *out_plan
);
enum RsnapStatus rsnap_frozen_selection_transform_hit_test(
	double point_x,
	double point_y,
	struct RsnapFloatRect selection,
	double handle_radius,
	double edge_tolerance,
	enum RsnapFrozenSelectionTransformKind *out_kind
);
enum RsnapStatus rsnap_frozen_selection_transform_rect(
	enum RsnapFrozenSelectionTransformKind kind,
	struct RsnapFloatRect initial_selection,
	struct RsnapFloatRect monitor_frame,
	double initial_pointer_x,
	double initial_pointer_y,
	double point_x,
	double point_y,
	double minimum_size,
	struct RsnapFloatRect *out_rect
);
enum RsnapStatus rsnap_auto_center_content_bounds_rgba(
	uint32_t width,
	uint32_t height,
	const uint8_t *rgba,
	size_t rgba_len,
	struct RsnapPixelRect *out_rect
);
double rsnap_auto_center_margin_balance_shift_points(
	double content_origin_px,
	double content_size_px,
	double crop_size_px,
	double capture_size_points
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
void rsnap_owned_rgba_region_release(struct RsnapOwnedRgbaRegion *region);
void rsnap_owned_bytes_release(struct RsnapOwnedBytes *bytes);

#ifdef __cplusplus
}
#endif

#endif
