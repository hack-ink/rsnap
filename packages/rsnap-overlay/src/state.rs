//! Shared image and geometry payloads for the remaining transition helpers.
#![allow(
	dead_code,
	reason = "Native-host live sampling returns these payloads across the FFI boundary; local Rust code only constructs part of each value."
)]

#[cfg(target_os = "macos")]
pub use rsnap_capture_core::geometry::{GlobalPoint, RectPoints};
pub use rsnap_capture_core::geometry::{MonitorRect, Rgb};

use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;

#[derive(Debug)]
/// Cached full-monitor frame used for RGB and loupe sampling.
pub struct MonitorImageSnapshot {
	/// When the frame was captured.
	pub captured_at: Instant,
	/// Live-stream generation that produced this frame.
	pub stream_generation: u64,
	/// The monitor that produced this frame.
	pub monitor: MonitorRect,
	/// The captured monitor image in RGBA pixel format.
	pub image: Arc<RgbaImage>,
}

#[derive(Debug)]
/// Combined live cursor sample containing the current RGB and optional loupe patch.
pub struct LiveCursorSample {
	/// The sampled RGB value under the cursor when available.
	pub rgb: Option<Rgb>,
	/// The sampled loupe patch when requested and available.
	pub patch: Option<RgbaImage>,
}
