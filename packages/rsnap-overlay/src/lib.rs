//! Legacy-free Rust capture helpers still being migrated into `rsnap-capture-core`.
//!
//! This crate intentionally exposes only the remaining Rust-owned transition helpers
//! used by native-host FFI and deterministic performance checks. The retired Rust UI
//! overlay runtime is no longer part of the public or compiled crate surface.

pub mod bench_support {
	//! Benchmark harness exports used by Criterion benches.

	pub use crate::scroll_capture::bench_support::{
		ScrollCaptureBenchHarness, ScrollCaptureBenchScenario, ScrollCaptureFingerprintMetrics,
		ScrollCaptureOverlapMetrics, ScrollCaptureSessionMetrics,
	};
}
pub mod frozen_edit;
pub mod frozen_export;
pub mod scroll_stitching {
	//! Narrow native-host wrapper around the existing scroll-capture stitching engine.

	use color_eyre::eyre::{self, Result};
	use image::RgbaImage;

	use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome, ScrollSession};

	/// RGBA image payload used by native-host FFI wrappers.
	#[derive(Clone, Debug, Eq, PartialEq)]
	pub struct ScrollStitchImage {
		/// Image width in pixels.
		pub width: u32,
		/// Image height in pixels.
		pub height: u32,
		/// Row-major RGBA bytes.
		pub rgba: Vec<u8>,
	}

	/// Result of observing one candidate scroll-capture frame.
	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub enum ScrollStitchObserveOutcome {
		/// The candidate did not change the committed stitched image.
		NoChange,
		/// Only preview state changed.
		PreviewUpdated,
		/// Downward growth was committed.
		Committed {
			/// Number of appended pixel rows.
			growth_rows: u32,
		},
		/// The candidate proved motion in a direction that this wrapper does not append.
		UnsupportedDirection,
	}

	/// Native-host scroll stitcher that preserves the proven pairwise registration logic.
	pub struct ScrollStitchSession {
		inner: ScrollSession,
	}
	impl ScrollStitchSession {
		/// Creates a stitcher from the first frozen viewport frame.
		pub fn new_from_rgba(
			width: u32,
			height: u32,
			rgba: &[u8],
			preview_width_px: u32,
		) -> Result<Self> {
			let frame = rgba_image_from_bytes(width, height, rgba)?;
			let inner = ScrollSession::new(frame, preview_width_px)?;

			Ok(Self { inner })
		}

		/// Observes a discrete native screenshot using the macOS worker pairwise path.
		pub fn observe_worker_pairwise_rgba(
			&mut self,
			width: u32,
			height: u32,
			rgba: &[u8],
		) -> Result<ScrollStitchObserveOutcome> {
			let frame = rgba_image_from_bytes(width, height, rgba)?;

			self.inner
				.observe_worker_pairwise_vision_frame(frame)
				.map(scroll_stitch_observe_outcome_from)
		}

		/// Observes a discrete native screenshot with pairwise registration and an
		/// optional downward motion hint for committed-frontier catch-up.
		pub fn observe_downward_rgba_with_motion_hint(
			&mut self,
			width: u32,
			height: u32,
			rgba: &[u8],
			motion_rows_hint: Option<u32>,
			_allow_burst_search: bool,
		) -> Result<ScrollStitchObserveOutcome> {
			let frame = rgba_image_from_bytes(width, height, rgba)?;

			self.inner
				.observe_worker_pairwise_vision_frame_with_motion_hint(frame, motion_rows_hint)
				.map(scroll_stitch_observe_outcome_from)
		}

		/// Returns the committed stitched export image.
		#[must_use]
		pub fn export_image(&self) -> ScrollStitchImage {
			let image = self.inner.export_image();

			ScrollStitchImage {
				width: image.width(),
				height: image.height(),
				rgba: image.as_raw().clone(),
			}
		}

		/// Returns the lightweight committed stitched preview image.
		#[must_use]
		pub fn preview_image(&self) -> ScrollStitchImage {
			let image = self.inner.preview_image();

			ScrollStitchImage {
				width: image.width(),
				height: image.height(),
				rgba: image.as_raw().clone(),
			}
		}

		/// Returns the committed stitched export dimensions without cloning pixels.
		#[must_use]
		pub fn export_dimensions(&self) -> (u32, u32) {
			let image = self.inner.export_image();

			(image.width(), image.height())
		}

		/// Returns the current committed viewport top offset in pixels.
		#[must_use]
		pub fn current_viewport_top_y(&self) -> i32 {
			self.inner.current_viewport_top_y()
		}

		/// Reverts the last committed append, when one exists.
		#[must_use]
		pub fn undo_last_append(&mut self) -> bool {
			self.inner.undo_last_append()
		}
	}

	fn scroll_stitch_observe_outcome_from(
		value: ScrollObserveOutcome,
	) -> ScrollStitchObserveOutcome {
		match value {
			ScrollObserveOutcome::NoChange => ScrollStitchObserveOutcome::NoChange,
			ScrollObserveOutcome::PreviewUpdated => ScrollStitchObserveOutcome::PreviewUpdated,
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows } => {
				ScrollStitchObserveOutcome::Committed { growth_rows }
			},
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Up, .. }
			| ScrollObserveOutcome::UnsupportedDirection { .. } => {
				ScrollStitchObserveOutcome::UnsupportedDirection
			},
		}
	}

	fn rgba_image_from_bytes(width: u32, height: u32, rgba: &[u8]) -> Result<RgbaImage> {
		let expected_len = usize::try_from(width)
			.ok()
			.zip(usize::try_from(height).ok())
			.and_then(|(width, height)| width.checked_mul(height))
			.and_then(|pixels| pixels.checked_mul(4))
			.ok_or_else(|| eyre::eyre!("scroll-capture frame dimensions overflow"))?;

		if rgba.len() != expected_len {
			return Err(eyre::eyre!(
				"scroll-capture frame byte length mismatch: expected {} got {}",
				expected_len,
				rgba.len()
			));
		}

		RgbaImage::from_raw(width, height, rgba.to_vec())
			.ok_or_else(|| eyre::eyre!("scroll-capture frame could not be decoded as RGBA"))
	}
}
#[cfg(target_os = "macos")]
pub mod host_live_sampling_macos;

#[cfg(target_os = "macos")]
mod live_frame_stream_macos;
#[cfg(target_os = "macos")]
mod macos_color;
mod point;
mod scroll_capture;
mod state;
mod system_fonts;
mod text_rendering;

#[cfg(test)]
use criterion as _;

/// Returns the `rsnap-overlay` crate version.
pub fn overlay_version() -> &'static str {
	env!("CARGO_PKG_VERSION")
}
