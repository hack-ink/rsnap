use rsnap_capture_core::{CaptureSessionCore, FrozenOverlayEditSession, ScrollStitchSession};

/// Opaque session handle owned by the native host through the C ABI.
pub struct RsnapSessionHandle {
	pub(crate) session: CaptureSessionCore,
}

/// Opaque scroll-capture stitching handle owned by the native host through the C ABI.
pub struct RsnapScrollSessionHandle {
	pub(crate) session: ScrollStitchSession,
}

/// Opaque frozen-overlay edit handle owned by the native host through the C ABI.
pub struct RsnapFrozenOverlayEditSessionHandle {
	pub(crate) session: FrozenOverlayEditSession,
}
