//! Public session-level overlay API used by the desktop application crate.
//!
//! Backend implementations remain internal to this crate and are not part of the
//! app-shell integration surface.

mod backend;
#[cfg(target_os = "macos")]
mod live_frame_stream_macos;
mod overlay;
mod png;
mod scroll_capture;
mod state;
mod worker;

pub use crate::overlay::{
	AltActivationMode, HudAnchor, OutputNaming, OverlayConfig, OverlayControl, OverlayExit,
	OverlaySession, ThemeMode, ToolbarPlacement, WindowCaptureAlphaMode,
};
pub use crate::state::{
	GlobalPoint, LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints, Rgb, WindowHit,
	WindowListSnapshot, WindowRect,
};

/// Returns the `rsnap-overlay` crate version.
pub fn overlay_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
