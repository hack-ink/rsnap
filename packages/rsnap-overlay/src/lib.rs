mod backend;
mod overlay;
mod png;
mod state;
mod worker;

pub use crate::backend::{
	CaptureBackend, StubCaptureBackend, XcapCaptureBackend, default_capture_backend,
};
pub use crate::overlay::{HudAnchor, OverlayConfig, OverlayControl, OverlayExit, OverlaySession};
pub use crate::state::{GlobalPoint, MonitorRect, Rgb};

pub fn overlay_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
