use rsnap_capture_core::{CaptureSessionCore, FrozenOverlayEditSession, ScrollStitchSession};
#[cfg(target_os = "macos")]
use rsnap_overlay::host_live_sampling_macos::HostMacLiveSampler;

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

#[cfg(target_os = "macos")]
/// Opaque live-sampler handle owned by the native host through the C ABI.
pub struct RsnapLiveSamplerHandle {
	pub(crate) sampler: HostMacLiveSampler,
}
