//! Public session-level overlay API used by the desktop application crate.
//!
//! Backend implementations remain internal to this crate and are not part of the
//! app-shell integration surface.

#![allow(unused_crate_dependencies)]

/// Benchmark harness exports used by Criterion benches.
pub mod bench_support {
	pub use crate::scroll_capture::bench_support::{
		ScrollCaptureBenchHarness, ScrollCaptureBenchScenario, ScrollCaptureFingerprintMetrics,
		ScrollCaptureOverlapMetrics, ScrollCaptureSessionMetrics,
	};
}

/// Deterministic replay harness exports for scroll-capture verification.
pub mod replay_support {
	pub use crate::overlay::replay_support::{
		RecordedScrollCaptureReplayMode, RecordedScrollCaptureReplayRecordedOutcome,
		RecordedScrollCaptureReplayStepResult, RecordedScrollCaptureReplaySummary,
		replay_recorded_scroll_capture_trace, replay_recorded_scroll_capture_trace_with_mode,
	};
}

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
