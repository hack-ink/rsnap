//! Public macOS native-host entry points for the desktop app crate.

pub use rsnap_overlay::host_effects_macos::{
	process_deferred_text_recognition, process_deferred_text_recognition_for_latest_capture,
};
pub use rsnap_overlay::host_macos::{
	MacOSCaptureHost, MacOSCaptureHostSyncState, MacOSNativeCaptureInputEvent,
	MacOSNativeCaptureScrollDelta, MacOSScrollCaptureCapability, MacOSScrollCaptureCapabilityEvent,
};
