//! FFI-safe ABI types and constants shared by the native-host bridge.

mod capture_frame;
mod frozen_overlay;
mod geometry;
mod handles;
mod scroll;
mod session;
mod status;

pub use self::capture_frame::{
	RsnapCaptureFrameBackgroundKind, RsnapCaptureFrameBackgroundPlan, RsnapCaptureFrameColorStop,
	RsnapCaptureFramePlan, RsnapCaptureFrameRenderKind, RsnapCaptureFrameShadow,
	RsnapCaptureFrameSourceKind, RsnapCaptureFrameWallpaperRequest,
};
pub use self::frozen_overlay::{
	RsnapFrozenAnnotationColor, RsnapFrozenOverlayEditSnapshot, RsnapFrozenOverlayEditStyle,
	RsnapFrozenOverlayExportElement, RsnapFrozenOverlayExportElementKind,
	RsnapFrozenSelectionTransformKind,
};
pub use self::geometry::{
	RsnapFloatPoint, RsnapFloatRect, RsnapMonitorRect, RsnapOwnedBytes, RsnapOwnedRgbaRegion,
	RsnapPixelRect, RsnapPoint, RsnapRect, RsnapRgb, RsnapWindowRect,
};
pub use self::handles::{
	RsnapFrozenOverlayEditSessionHandle, RsnapScrollSessionHandle, RsnapSessionHandle,
};
pub use self::scroll::{
	RsnapScrollMinimapPlan, RsnapScrollObserveOutcomeKind, RsnapScrollObserveResult,
};
pub use self::session::{
	RsnapCursorIntent, RsnapHostEffectKind, RsnapHostEvent, RsnapHostEventKind, RsnapHostReport,
	RsnapHostReportKind, RsnapHostRequestKind, RsnapHostRequestValue, RsnapPermissionKind,
	RsnapPlatformTag, RsnapSceneKind, RsnapSceneModel, RsnapSessionConfig, RsnapToolbarItem,
	RsnapToolbarItemKind,
};
pub use self::status::RsnapStatus;

/// ABI version exported by the thin C host bridge.
pub const RSNAP_HOST_FFI_ABI_VERSION: u32 = 39;

/// Maximum frozen toolbar items copied into one scene snapshot.
pub(crate) const RSNAP_TOOLBAR_ITEM_CAPACITY: usize = 16;
/// Maximum UTF-8 status-message bytes copied into fixed-size ABI payloads.
pub(crate) const RSNAP_STATUS_MESSAGE_CAPACITY: usize = 256;
pub(crate) fn frozen_overlay_empty_element() -> RsnapFrozenOverlayExportElement {
	self::frozen_overlay::frozen_overlay_empty_element()
}
