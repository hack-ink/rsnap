//! Deterministic scroll-capture fixtures and harnesses used by Criterion benches.

use image::{Rgba, RgbaImage, imageops};

use crate::scroll_capture::support::{self};
use crate::scroll_capture::{
	self, OverlapSearchConfig, ScrollDirection, ScrollObserveOutcome, ScrollSession,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Benchmark fixture shapes that exercise the common and wide capture paths.
pub enum ScrollCaptureBenchScenario {
	/// Standard-width capture data with modest scroll movement.
	Baseline,
	/// Wider capture data with a larger viewport and scroll delta.
	Wide,
}
impl ScrollCaptureBenchScenario {
	/// All supported benchmark scenarios in stable iteration order.
	pub const ALL: [Self; 2] = [Self::Baseline, Self::Wide];

	#[must_use]
	/// Returns the stable bench-function suffix for this scenario.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Baseline => "baseline",
			Self::Wide => "wide",
		}
	}

	const fn spec(self) -> ScrollCaptureBenchFixtureSpec {
		match self {
			Self::Baseline => ScrollCaptureBenchFixtureSpec {
				width: 192,
				document_rows: 320,
				window_rows: 128,
				motion_rows: 12,
				preview_width_px: 320,
			},
			Self::Wide => ScrollCaptureBenchFixtureSpec {
				width: 320,
				document_rows: 448,
				window_rows: 160,
				motion_rows: 20,
				preview_width_px: 320,
			},
		}
	}
}

#[derive(Clone, Copy, Debug, Default)]
/// Fingerprint benchmark output used for deterministic performance checks.
pub struct ScrollCaptureFingerprintMetrics {
	/// Total byte length of the generated fingerprint payload.
	pub byte_len: usize,
	/// Stable checksum of the generated fingerprint payload.
	pub checksum: u32,
}

#[derive(Clone, Copy, Debug, Default)]
/// Overlap-match benchmark output for a single downward sample.
pub struct ScrollCaptureOverlapMetrics {
	/// Whether the overlap search produced a valid match.
	pub matched: bool,
	/// Detected scroll motion in rows.
	pub motion_rows: u32,
	/// Rows that remained overlapped after applying the detected motion.
	pub overlap_rows: u32,
	/// Mean absolute difference metric for the matched overlap window.
	pub mean_abs_diff_x100: u32,
}

#[derive(Clone, Copy, Debug, Default)]
/// Session-commit benchmark output for a single growth observation.
pub struct ScrollCaptureSessionMetrics {
	/// Whether the sample committed new growth into the session.
	pub committed: bool,
	/// Number of rows added to the stitched export.
	pub growth_rows: u32,
	/// Export image height after the observation completes.
	pub export_height: u32,
	/// Preview image height after the observation completes.
	pub preview_height: u32,
}

/// Reusable scroll-capture benchmark harness backed by deterministic image fixtures.
pub struct ScrollCaptureBenchHarness {
	fixture: ScrollCaptureBenchFixture,
	overlap_config: OverlapSearchConfig,
}
impl ScrollCaptureBenchHarness {
	#[must_use]
	/// Builds the benchmark harness for the selected fixture scenario.
	pub fn new(scenario: ScrollCaptureBenchScenario) -> Self {
		Self {
			fixture: ScrollCaptureBenchFixture::new(scenario.spec()),
			overlap_config: OverlapSearchConfig::default(),
		}
	}

	#[must_use]
	/// Runs the fingerprint path and returns stable summary metrics.
	pub fn run_fingerprint(&self) -> ScrollCaptureFingerprintMetrics {
		let bytes = scroll_capture::scroll_capture_fingerprint(&self.fixture.fingerprint_frame);

		ScrollCaptureFingerprintMetrics { byte_len: bytes.len(), checksum: checksum_bytes(&bytes) }
	}

	#[must_use]
	/// Runs the overlap matcher and returns the resulting comparison metrics.
	pub fn run_overlap_match(&self) -> ScrollCaptureOverlapMetrics {
		let max_motion_rows = support::max_directional_motion_rows(
			&self.fixture.base_frame,
			&self.fixture.next_frame,
			self.overlap_config,
		);
		let matched = support::evaluate_overlap_direction(
			&self.fixture.base_frame,
			&self.fixture.next_frame,
			ScrollDirection::Down,
			1..=max_motion_rows,
			self.overlap_config,
		);

		matched.map_or(
			ScrollCaptureOverlapMetrics {
				matched: false,
				motion_rows: 0,
				overlap_rows: 0,
				mean_abs_diff_x100: u32::MAX,
			},
			|matched| ScrollCaptureOverlapMetrics {
				matched: true,
				motion_rows: matched.motion_rows,
				overlap_rows: self
					.fixture
					.window_rows
					.min(self.fixture.base_frame.height())
					.saturating_sub(matched.motion_rows),
				mean_abs_diff_x100: matched.mean_abs_diff_x100,
			},
		)
	}

	#[must_use]
	/// Runs one downward observation through the session-commit path.
	pub fn run_session_commit(&self) -> ScrollCaptureSessionMetrics {
		let mut session = self.fixture.new_session();
		let outcome = session
			.observe_downward_sample(self.fixture.next_frame.clone())
			.expect("scroll-capture benchmark fixture should observe successfully");
		let (committed, growth_rows) = match outcome {
			ScrollObserveOutcome::Committed { growth_rows, .. } => (true, growth_rows),
			_ => (false, 0),
		};

		ScrollCaptureSessionMetrics {
			committed,
			growth_rows,
			export_height: session.export_image().height(),
			preview_height: session.preview_image().height(),
		}
	}
}

#[derive(Clone, Copy)]
struct ScrollCaptureBenchFixtureSpec {
	width: u32,
	document_rows: u32,
	window_rows: u32,
	motion_rows: u32,
	preview_width_px: u32,
}

struct ScrollCaptureBenchFixture {
	base_frame: RgbaImage,
	next_frame: RgbaImage,
	fingerprint_frame: RgbaImage,
	window_rows: u32,
	preview_width_px: u32,
}
impl ScrollCaptureBenchFixture {
	fn new(spec: ScrollCaptureBenchFixtureSpec) -> Self {
		let document = build_document(spec.width, spec.document_rows);
		let base_frame = crop_window(&document, 24, spec.window_rows);
		let next_frame = crop_window(&document, 24 + spec.motion_rows, spec.window_rows);
		let fingerprint_frame =
			crop_window(&document, 24 + spec.motion_rows.saturating_mul(2), spec.window_rows);

		Self {
			base_frame,
			next_frame,
			fingerprint_frame,
			window_rows: spec.window_rows,
			preview_width_px: spec.preview_width_px,
		}
	}

	fn new_session(&self) -> ScrollSession {
		ScrollSession::new(self.base_frame.clone(), self.preview_width_px)
			.expect("scroll-capture benchmark fixture should build a valid session")
	}
}

fn crop_window(document: &RgbaImage, start_row: u32, rows: u32) -> RgbaImage {
	imageops::crop_imm(document, 0, start_row, document.width(), rows).to_image()
}

fn build_document(width: u32, rows: u32) -> RgbaImage {
	let mut image = RgbaImage::new(width, rows);

	for y in 0..rows {
		for x in 0..width {
			let stripe = (y / 8) % 6;
			let lane = (x / 12) % 5;
			let mut r =
				((x.wrapping_mul(13) + y.wrapping_mul(17) + stripe.wrapping_mul(29)) % 251) as u8;
			let mut g =
				((x.wrapping_mul(7) + y.wrapping_mul(19) + lane.wrapping_mul(23)) % 251) as u8;
			let mut b = (((x / 2).wrapping_mul(11)
				+ y.wrapping_mul(5)
				+ stripe.wrapping_mul(31)
				+ lane.wrapping_mul(17))
				% 251) as u8;

			if x < 10 || x + 10 >= width {
				r = 8;
				g = 8;
				b = 8;
			}
			if y % 32 == 0 {
				r = r.saturating_add(21);
				g = g.saturating_add(9);
			}
			if (x / 24 + y / 16).is_multiple_of(2) {
				b = b.saturating_add(13);
			}

			image.put_pixel(x, y, Rgba([r, g, b, 255]));
		}
	}

	image
}

fn checksum_bytes(bytes: &[u8]) -> u32 {
	bytes.iter().fold(0_u32, |acc, byte| {
		acc.wrapping_mul(16_777_619).wrapping_add(u32::from(*byte).wrapping_add(1))
	})
}
