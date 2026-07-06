//! Shared image and geometry payloads for the remaining transition helpers.

#[cfg(target_os = "macos")]
pub use rsnap_capture_core::geometry::MonitorRect;
#[cfg(target_os = "macos")]
pub use rsnap_capture_core::geometry::{GlobalPoint, RectPoints};
