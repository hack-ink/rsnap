//! Public Rust-core session API used by the desktop application crate.
//!
//! Backend implementations remain internal to this crate and are not part of the
//! native-host integration surface exposed from `apps/rsnap`.

#![allow(unused_crate_dependencies)]

pub mod bench_support {
	//! Benchmark harness exports used by Criterion benches.

	pub use crate::scroll_capture::bench_support::{
		ScrollCaptureBenchHarness, ScrollCaptureBenchScenario, ScrollCaptureFingerprintMetrics,
		ScrollCaptureOverlapMetrics, ScrollCaptureSessionMetrics,
	};
}
pub mod replay_support {
	//! Deterministic replay harness exports for scroll-capture verification.

	pub use crate::overlay::replay_support::{
		RecordedScrollCaptureReplayMode, RecordedScrollCaptureReplayRecordedOutcome,
		RecordedScrollCaptureReplayStepResult, RecordedScrollCaptureReplaySummary,
		replay_recorded_scroll_capture_trace, replay_recorded_scroll_capture_trace_with_mode,
	};
}
pub mod session {
	//! Rust-core session, protocol, and shared state exports consumed by the native app host.

	pub use crate::overlay::{
		AltActivationMode, FrozenGlobalHotkey, HudAnchor, OverlayConfig, OverlayControl,
		OverlayExit, OverlayKeyboardInputEvent, OverlaySession, ThemeMode, ToolbarPlacement,
		WindowCaptureAlphaMode,
	};
	#[cfg(target_os = "macos")]
	pub use crate::overlay::{
		ScrollCaptureHostAdapter, ScrollCaptureHostFrameRequestError, ScrollCaptureHostStartRequest,
	};
	pub use crate::state::{
		GlobalPoint, LiveCursorSample, MonitorImageSnapshot, MonitorRect, RectPoints, Rgb,
		WindowHit, WindowListSnapshot, WindowRect,
	};
	pub use rsnap_capture_core::{
		DeferredTextRecognitionOutcome, DeferredTextRecognitionOutcomeKind,
		DeferredTextRecognitionRequest, OutputNaming, PreparedHostEffectRequest,
	};
}
#[cfg(target_os = "macos")]
pub mod host_effects_macos {
	//! Host-owned macOS effect helpers executed after the core emits explicit requests.

	pub use crate::deferred_text_recognition::{
		process_deferred_text_recognition, process_deferred_text_recognition_for_latest_capture,
	};
}
#[cfg(target_os = "macos")]
pub mod host_macos {
	//! Transitional macOS host adapters kept explicit so the crate root does not imply
	//! top-level host ownership.

	pub use crate::overlay::{
		MacOSCaptureHost, MacOSCaptureHostSyncState, MacOSNativeCaptureInputEvent,
		MacOSNativeCaptureScrollDelta,
	};
	pub use crate::scroll_capture_capability_macos::{
		MacOSScrollCaptureCapability, MacOSScrollCaptureCapabilityEvent,
	};
}

mod backend;
#[cfg(target_os = "macos")]
mod deferred_text_recognition;
#[cfg(target_os = "macos")]
mod live_frame_stream_macos;
#[cfg(target_os = "macos")]
mod macos_color;
#[cfg(target_os = "macos")]
mod ocr_macos;
mod overlay;
mod png;
mod scroll_capture;
#[cfg(target_os = "macos")]
mod scroll_capture_capability_macos;
mod state;
mod system_fonts;
mod text_rendering;
mod worker;

/// Returns the `rsnap-overlay` crate version.
pub fn overlay_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
