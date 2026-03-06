mod backend;
#[cfg(target_os = "macos")]
mod live_frame_stream_macos;
mod overlay;
mod png;
mod scroll_capture;
mod state;
mod worker;

pub use crate::backend::{
	CaptureBackend, StubCaptureBackend, XcapCaptureBackend, default_capture_backend,
};
pub use crate::overlay::{
	AltActivationMode, HudAnchor, OutputNaming, OverlayConfig, OverlayControl, OverlayExit,
	OverlaySession, ThemeMode, ToolbarPlacement, WindowCaptureAlphaMode,
};
pub use crate::state::{GlobalPoint, MonitorRect, Rgb};

pub fn overlay_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
