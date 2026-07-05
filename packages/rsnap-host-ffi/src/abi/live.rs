use crate::abi::{RSNAP_LIVE_SAMPLE_PATCH_CAPACITY, RsnapRgb};

/// FFI-safe live cursor sample copied out of the native Rust sampler.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RsnapLiveSample {
	/// Sampled RGB value.
	pub rgb: RsnapRgb,
	/// Non-zero when `rgb` is present.
	pub has_rgb: u8,
	/// Non-zero when frame provenance fields are present.
	pub has_frame_metadata: u8,
	/// Age of the sampled ScreenCaptureKit frame in microseconds.
	pub frame_age_micros: u64,
	/// Monotonic sequence of the sampled ScreenCaptureKit frame.
	pub frame_seq: u64,
	/// Live stream generation that produced the sampled frame.
	pub stream_generation: u64,
	/// Sampled loupe patch width in pixels.
	pub patch_width: u32,
	/// Sampled loupe patch height in pixels.
	pub patch_height: u32,
	/// Byte count copied into `patch_rgba`.
	pub patch_len: u32,
	/// Optional RGBA patch bytes in row-major order.
	pub patch_rgba: [u8; RSNAP_LIVE_SAMPLE_PATCH_CAPACITY],
}
impl Default for RsnapLiveSample {
	fn default() -> Self {
		Self {
			rgb: RsnapRgb::default(),
			has_rgb: 0,
			has_frame_metadata: 0,
			frame_age_micros: 0,
			frame_seq: 0,
			stream_generation: 0,
			patch_width: 0,
			patch_height: 0,
			patch_len: 0,
			patch_rgba: [0; RSNAP_LIVE_SAMPLE_PATCH_CAPACITY],
		}
	}
}
