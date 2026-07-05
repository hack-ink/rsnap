#![allow(
	dead_code,
	reason = "XY-113 narrows the public crate facade while leaving backend implementation cleanup to a separate follow-up lane."
)]

mod contract;
mod image_capture;
#[cfg(target_os = "macos")]
mod macos_region_capture;
mod window_list;
mod xcap_capture_backend;

pub use self::contract::{CaptureBackend, CaptureBackendError};

use self::xcap_capture_backend::XcapCaptureBackend;

#[must_use]
/// Builds the default capture backend used by overlay worker threads.
pub fn default_capture_backend() -> Box<dyn CaptureBackend> {
	Box::new(XcapCaptureBackend::new())
}

#[must_use]
/// Builds the default capture backend with explicit current-process self-capture exceptions.
pub fn default_capture_backend_with_self_capture_exception_window_ids(
	self_capture_exception_window_ids: Vec<u32>,
) -> Box<dyn CaptureBackend> {
	Box::new(XcapCaptureBackend::with_self_capture_exception_window_ids(
		self_capture_exception_window_ids,
	))
}
