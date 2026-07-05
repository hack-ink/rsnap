use rsnap_capture_core::CaptureSessionCore;
use rsnap_overlay::frozen_edit::FrozenOverlayEditSession;
#[cfg(target_os = "macos")]
use rsnap_overlay::host_live_sampling_macos::HostMacLiveSampler;
use rsnap_overlay::scroll_stitching::ScrollStitchSession;

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
