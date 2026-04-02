/// Deterministic scroll-capture fixtures and harnesses used by Criterion benches.
pub mod bench_support {
	use image::{Rgba, RgbaImage, imageops};

	use crate::scroll_capture::{
		OverlapSearchConfig, ScrollDirection, ScrollObserveOutcome, ScrollSession,
		evaluate_overlap_direction, max_directional_motion_rows, scroll_capture_fingerprint,
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
			let bytes = scroll_capture_fingerprint(&self.fixture.fingerprint_frame);

			ScrollCaptureFingerprintMetrics {
				byte_len: bytes.len(),
				checksum: checksum_bytes(&bytes),
			}
		}

		#[must_use]
		/// Runs the overlap matcher and returns the resulting comparison metrics.
		pub fn run_overlap_match(&self) -> ScrollCaptureOverlapMetrics {
			let max_motion_rows = max_directional_motion_rows(
				&self.fixture.base_frame,
				&self.fixture.next_frame,
				self.overlap_config,
			);
			let matched = evaluate_overlap_direction(
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
				let mut r = ((x.wrapping_mul(13) + y.wrapping_mul(17) + stripe.wrapping_mul(29))
					% 251) as u8;
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
}

use std::ops::RangeInclusive;

use color_eyre::eyre::{self, Result};
use image::{
	RgbaImage,
	imageops::{self, FilterType},
};
#[cfg(target_os = "macos")]
use objc2::{AnyThread, runtime::AnyObject};
#[cfg(target_os = "macos")]
use objc2_core_foundation::CFData;
#[cfg(target_os = "macos")]
use objc2_core_graphics::{
	CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
	CGImageByteOrderInfo,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSDictionary};
#[cfg(target_os = "macos")]
use objc2_vision::{VNImageOption, VNImageRequestHandler, VNTranslationalImageRegistrationRequest};

const FINGERPRINT_GRID_COLUMNS: u32 = 12;
const FINGERPRINT_GRID_ROWS: u32 = 16;
const DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS: u32 = 48;
const DOWNWARD_KEYFRAME_SEARCH_MOTION_TOLERANCE_ROWS: u32 = 4;
const DOWNWARD_KEYFRAME_SEARCH_MAX_TOLERANCE_ROWS: u32 = 24;
const LOCAL_DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS: u32 = 4;
const LOCAL_DOWNWARD_SEARCH_MAX_TOLERANCE_ROWS: u32 = 12;
const DOWNWARD_REGISTRATION_AMBIGUOUS_GAP_ROWS: u32 = 24;
const DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS: u32 = 4;
const DOWNWARD_REGISTRATION_MIN_OVERLAP_DIVISOR: u32 = 3;
const DOWNWARD_KEYFRAME_SEARCH_LIMIT: usize = 4;
const DOWNWARD_KEYFRAME_MIN_OVERLAP_DIVISOR: u32 = 5;
const INITIAL_DOWNWARD_MAX_MOTION_ROWS: u32 = 256;
pub(crate) const PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS: u32 = 24;
pub(crate) const PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS: u32 = 12;
const PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS: u32 = 4;
const EXTREME_TRANSIENT_PREVIEW_LOCAL_TAIL_MULTIPLIER: u32 = 12;
const REPEATED_PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS: u32 = 4;
const TINY_OBSERVED_BURST_RECOVERY_MAX_MOTION_ROWS: u32 = 2;
const TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS: u32 = 1;
const TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MIN_LAST_HINT_ROWS: u32 = 7;
pub(crate) const UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS: u32 = 8;
const BOOTSTRAP_HINTED_INITIAL_GROWTH_MAX_ROWS: u32 = 40;
const DOWNWARD_COMMITTED_KEYFRAME_LOCAL_OVERRUN_MAX_ROWS: u32 = 24;
const FALLBACK_DOWNWARD_GROWTH_MIN_ROWS: u32 = 8;
const FALLBACK_DOWNWARD_GROWTH_MAX_ROWS: u32 = 16;
const TRANSIENT_MOTION_HINT_MAX_MULTIPLIER: u32 = 3;
const TRANSIENT_MOTION_HINT_MIN_CAP_ROWS: u32 = 12;
const WORKER_PAIRWISE_CORROBORATION_MIN_ROWS: u32 = 32;
const WORKER_PAIRWISE_CORROBORATION_TOLERANCE_ROWS: u32 = 24;
const DIRECTION_WARNING_MARGIN_X100: u32 = 90;
const RESUME_DIRECT_PROOF_MAX_MEAN_ABS_DIFF_X100: u32 = 320;
const INFORMATIVE_SPAN_ROW_SAMPLES: u32 = 24;
const INFORMATIVE_SPAN_SCORE_FLOOR_X100: u32 = 24;
const INFORMATIVE_SPAN_HORIZONTAL_PADDING_PX: u32 = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScrollFrameFingerprint {
	grid_columns: u32,
	grid_rows: u32,
	samples: Vec<[u8; 4]>,
}
impl ScrollFrameFingerprint {
	#[must_use]
	pub(crate) fn from_image(image: &RgbaImage) -> Self {
		let width = image.width().max(1);
		let height = image.height().max(1);
		let informative_span = informative_column_span(image, 0, height);
		let informative_left =
			informative_span.map_or(0, |span| span.start_x.min(width.saturating_sub(1)));
		let informative_right = informative_span
			.map_or(width, |span| span.end_exclusive_x.min(width).max(informative_left + 1));
		let informative_width = informative_right.saturating_sub(informative_left).max(1);
		let margin_x = ((informative_width as f32) * 0.05).round() as u32;
		let margin_y = ((height as f32) * 0.05).round() as u32;
		let left =
			informative_left.saturating_add(margin_x).min(informative_right.saturating_sub(1));
		let right = informative_right.saturating_sub(margin_x).max(left + 1);
		let top = margin_y.min(height.saturating_sub(1));
		let bottom = height.saturating_sub(margin_y).max(top + 1);
		let mut samples =
			Vec::with_capacity((FINGERPRINT_GRID_COLUMNS * FINGERPRINT_GRID_ROWS) as usize);

		for row in 0..FINGERPRINT_GRID_ROWS {
			let y = evenly_spaced_sample(top, bottom, row, FINGERPRINT_GRID_ROWS);

			for column in 0..FINGERPRINT_GRID_COLUMNS {
				let x = evenly_spaced_sample(left, right, column, FINGERPRINT_GRID_COLUMNS);
				let pixel = image.get_pixel(x, y).0;

				samples.push(pixel);
			}
		}

		Self { grid_columns: FINGERPRINT_GRID_COLUMNS, grid_rows: FINGERPRINT_GRID_ROWS, samples }
	}

	#[must_use]
	pub(crate) fn into_bytes(self) -> Vec<u8> {
		let mut bytes = Vec::with_capacity(self.samples.len().saturating_mul(4));

		for sample in self.samples {
			bytes.extend_from_slice(&sample);
		}

		bytes
	}

	#[must_use]
	#[cfg(test)]
	pub(crate) fn distance(&self, other: &Self) -> u64 {
		if self.grid_columns != other.grid_columns || self.grid_rows != other.grid_rows {
			return u64::MAX;
		}

		self.samples
			.iter()
			.zip(&other.samples)
			.map(|(left, right)| {
				u64::from(left[0].abs_diff(right[0]))
					+ u64::from(left[1].abs_diff(right[1]))
					+ u64::from(left[2].abs_diff(right[2]))
					+ u64::from(left[3].abs_diff(right[3]))
			})
			.sum()
	}
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverlapMatch {
	pub(crate) rows: u32,
	pub(crate) matched: bool,
	pub(crate) mean_abs_diff_x100: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScrollCommitTelemetry {
	pub(crate) current_viewport_top_y: i32,
	pub(crate) preview_dimensions: (u32, u32),
	pub(crate) export_dimensions: (u32, u32),
	pub(crate) growth_commit_count: usize,
	pub(crate) preview_segment_count: usize,
	pub(crate) export_segment_count: usize,
	pub(crate) preview_export_segments_aligned: bool,
	pub(crate) last_commit_decision_source: Option<&'static str>,
	pub(crate) last_commit_detected_motion_rows: Option<u32>,
	pub(crate) last_commit_effective_motion_rows_hint: Option<u32>,
	pub(crate) last_block_reason: Option<&'static str>,
	pub(crate) last_downward_sample_registration_result: Option<&'static str>,
	pub(crate) last_downward_sample_registration_source: Option<&'static str>,
	pub(crate) last_downward_sample_registration_motion_rows: Option<u32>,
	pub(crate) last_downward_sample_registration_provisional_viewport_top_y: Option<i32>,
	pub(crate) observed_sample_registration_result: Option<&'static str>,
	pub(crate) observed_sample_registration_reason: Option<&'static str>,
	pub(crate) observed_sample_registration_motion_rows: Option<u32>,
	pub(crate) observed_sample_registration_mean_abs_diff_x100: Option<u32>,
	pub(crate) preview_only_local_registration_result: Option<&'static str>,
	pub(crate) preview_only_local_registration_reason: Option<&'static str>,
	pub(crate) preview_only_local_registration_motion_rows: Option<u32>,
	pub(crate) preview_only_local_registration_mean_abs_diff_x100: Option<u32>,
	pub(crate) last_downward_viewport_candidate_count: Option<usize>,
	pub(crate) last_downward_viewport_candidates_before_prune: Option<String>,
	pub(crate) last_downward_viewport_candidates_after_prune: Option<String>,
	pub(crate) sample_eval_last_motion_rows_hint: Option<u32>,
	pub(crate) sample_eval_transient_motion_rows_hint: Option<u32>,
	pub(crate) sample_eval_effective_motion_rows_hint: Option<u32>,
	pub(crate) sample_eval_transient_burst_search_enabled: bool,
	pub(crate) preview_only_local_viewport_top_y: Option<i32>,
	pub(crate) last_preview_segment_height_px: Option<u32>,
	pub(crate) last_export_segment_height_px: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OverlapSearchConfig {
	pub(crate) min_overlap_rows: u32,
	pub(crate) max_column_samples: u32,
	pub(crate) max_row_samples: u32,
	pub(crate) max_mean_abs_diff_x100: u32,
}
impl Default for OverlapSearchConfig {
	fn default() -> Self {
		Self {
			min_overlap_rows: 24,
			max_column_samples: 32,
			max_row_samples: 16,
			max_mean_abs_diff_x100: 850,
		}
	}
}

fn worker_pairwise_overlap_search_config() -> OverlapSearchConfig {
	OverlapSearchConfig {
		min_overlap_rows: 24,
		max_column_samples: 96,
		max_row_samples: 96,
		max_mean_abs_diff_x100: 850,
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreviewOnlyDownwardLocalSample {
	frame: RgbaImage,
	viewport_top_y: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct ScrollSession {
	anchor_frame: RgbaImage,
	anchor_preview: RgbaImage,
	export_image: RgbaImage,
	preview_image: RgbaImage,
	bottom_segments: Vec<RgbaImage>,
	bottom_preview_segments: Vec<RgbaImage>,
	growth_history: Vec<GrowthCommit>,
	last_committed_frame: RgbaImage,
	worker_pairwise_previous_frame: RgbaImage,
	last_sample_frame: RgbaImage,
	last_sample_fingerprint: Option<Vec<u8>>,
	last_downward_observed_frame: RgbaImage,
	last_downward_observed_fingerprint: Option<Vec<u8>>,
	last_preview_only_downward_local_sample: Option<PreviewOnlyDownwardLocalSample>,
	seeded_preview_only_local_after_observed_burst_commit: bool,
	pending_unresolved_burst_registered_growth_viewport_top_y: Option<i32>,
	last_blocked_preview_only_local_candidate: Option<BlockedPreviewOnlyLocalCandidate>,
	pending_suppressed_huge_preview_only_local_followup: Option<DownwardViewportCandidate>,
	pending_suppressed_huge_preview_only_local_followup_remaining_blocks: u8,
	pending_extreme_preview_only_local_tail_followup: Option<DownwardViewportCandidate>,
	pending_extreme_preview_only_local_tail_followup_remaining_blocks: u8,
	last_unconfirmed_upward_fingerprint: Option<Vec<u8>>,
	last_motion_rows_hint: Option<u32>,
	transient_motion_rows_hint: Option<u32>,
	transient_burst_search_enabled: bool,
	last_downward_sample_registration_result: Option<&'static str>,
	last_downward_sample_registration_source: Option<&'static str>,
	last_downward_sample_registration_motion_rows: Option<u32>,
	last_downward_sample_registration_provisional_viewport_top_y: Option<i32>,
	last_observed_sample_registration_result: Option<&'static str>,
	last_observed_sample_registration_reason: Option<&'static str>,
	last_observed_sample_registration_motion_rows: Option<u32>,
	last_observed_sample_registration_mean_abs_diff_x100: Option<u32>,
	last_preview_only_local_registration_result: Option<&'static str>,
	last_preview_only_local_registration_reason: Option<&'static str>,
	last_preview_only_local_registration_motion_rows: Option<u32>,
	last_preview_only_local_registration_mean_abs_diff_x100: Option<u32>,
	last_downward_viewport_candidate_count: Option<usize>,
	last_downward_viewport_candidates_before_prune: Option<String>,
	last_downward_viewport_candidates_after_prune: Option<String>,
	blocked_underconsumed_observed_recovery_in_burst: bool,
	blocked_lagging_exactly_corroborated_preview_local_tail_in_burst: bool,
	blocked_followup_after_suppressed_huge_preview_local_jump: bool,
	blocked_followup_after_extreme_preview_local_tail: bool,
	blocked_far_committed_only_recovery_after_corroborated_huge_local_jump: bool,
	consecutive_transient_burst_missing_downward_candidate_frames: u32,
	last_block_reason: Option<&'static str>,
	last_sample_eval_last_motion_rows_hint: Option<u32>,
	last_sample_eval_transient_motion_rows_hint: Option<u32>,
	last_sample_eval_effective_motion_rows_hint: Option<u32>,
	last_sample_eval_transient_burst_search_enabled: bool,
	current_viewport_top_y: i32,
	observed_viewport_top_y: i32,
	resume_frontier_top_y: Option<i32>,
	resume_frontier_requires_reacquire: bool,
	preview_width_px: u32,
}
impl ScrollSession {
	pub(crate) fn new(base_frame: RgbaImage, preview_width_px: u32) -> Result<Self> {
		let fingerprint = scroll_capture_fingerprint(&base_frame);
		let anchor_preview = resize_strip_to_preview_width(&base_frame, preview_width_px.max(1));

		Ok(Self {
			anchor_frame: base_frame.clone(),
			anchor_preview: anchor_preview.clone(),
			export_image: base_frame.clone(),
			preview_image: anchor_preview,
			bottom_segments: Vec::new(),
			bottom_preview_segments: Vec::new(),
			growth_history: Vec::new(),
			last_committed_frame: base_frame.clone(),
			worker_pairwise_previous_frame: base_frame.clone(),
			last_sample_frame: base_frame.clone(),
			last_sample_fingerprint: Some(fingerprint.clone()),
			last_downward_observed_frame: base_frame,
			last_downward_observed_fingerprint: Some(fingerprint),
			last_preview_only_downward_local_sample: None,
			seeded_preview_only_local_after_observed_burst_commit: false,
			pending_unresolved_burst_registered_growth_viewport_top_y: None,
			last_blocked_preview_only_local_candidate: None,
			pending_suppressed_huge_preview_only_local_followup: None,
			pending_suppressed_huge_preview_only_local_followup_remaining_blocks: 0,
			pending_extreme_preview_only_local_tail_followup: None,
			pending_extreme_preview_only_local_tail_followup_remaining_blocks: 0,
			last_unconfirmed_upward_fingerprint: None,
			last_motion_rows_hint: None,
			transient_motion_rows_hint: None,
			transient_burst_search_enabled: false,
			last_downward_sample_registration_result: None,
			last_downward_sample_registration_source: None,
			last_downward_sample_registration_motion_rows: None,
			last_downward_sample_registration_provisional_viewport_top_y: None,
			last_observed_sample_registration_result: None,
			last_observed_sample_registration_reason: None,
			last_observed_sample_registration_motion_rows: None,
			last_observed_sample_registration_mean_abs_diff_x100: None,
			last_preview_only_local_registration_result: None,
			last_preview_only_local_registration_reason: None,
			last_preview_only_local_registration_motion_rows: None,
			last_preview_only_local_registration_mean_abs_diff_x100: None,
			last_downward_viewport_candidate_count: None,
			last_downward_viewport_candidates_before_prune: None,
			last_downward_viewport_candidates_after_prune: None,
			blocked_underconsumed_observed_recovery_in_burst: false,
			blocked_lagging_exactly_corroborated_preview_local_tail_in_burst: false,
			blocked_followup_after_suppressed_huge_preview_local_jump: false,
			blocked_followup_after_extreme_preview_local_tail: false,
			blocked_far_committed_only_recovery_after_corroborated_huge_local_jump: false,
			consecutive_transient_burst_missing_downward_candidate_frames: 0,
			last_block_reason: None,
			last_sample_eval_last_motion_rows_hint: None,
			last_sample_eval_transient_motion_rows_hint: None,
			last_sample_eval_effective_motion_rows_hint: None,
			last_sample_eval_transient_burst_search_enabled: false,
			current_viewport_top_y: 0,
			observed_viewport_top_y: 0,
			resume_frontier_top_y: None,
			resume_frontier_requires_reacquire: false,
			preview_width_px: preview_width_px.max(1),
		})
	}

	pub(crate) fn observe_downward_sample(
		&mut self,
		frame: RgbaImage,
	) -> Result<ScrollObserveOutcome> {
		self.observe_downward_sample_with_motion_hint(frame, None)
	}

	pub(crate) fn observe_downward_sample_with_motion_hint(
		&mut self,
		frame: RgbaImage,
		motion_rows_hint: Option<u32>,
	) -> Result<ScrollObserveOutcome> {
		self.observe_downward_sample_with_motion_hint_and_burst(frame, motion_rows_hint, false)
	}

	pub(crate) fn observe_downward_sample_with_motion_hint_and_burst(
		&mut self,
		frame: RgbaImage,
		motion_rows_hint: Option<u32>,
		allow_burst_search: bool,
	) -> Result<ScrollObserveOutcome> {
		self.observe_sample_with_motion_context(
			frame,
			ScrollDirection::Down,
			motion_rows_hint,
			allow_burst_search,
		)
	}

	#[cfg(test)]
	pub(crate) fn observe_upward_sample(
		&mut self,
		frame: RgbaImage,
	) -> Result<ScrollObserveOutcome> {
		self.observe_sample_with_motion_context(frame, ScrollDirection::Up, None, false)
	}

	pub(crate) fn observe_worker_pairwise_vision_frame(
		&mut self,
		frame: RgbaImage,
	) -> Result<ScrollObserveOutcome> {
		self.clear_last_downward_sample_registration();

		if frame.width() != self.anchor_frame.width() {
			return Err(eyre::eyre!(
				"frame width mismatch: expected {} got {}",
				self.anchor_frame.width(),
				frame.width()
			));
		}

		let fingerprint = scroll_capture_fingerprint(&frame);
		let previous_worker_frame = self.worker_pairwise_previous_frame.clone();

		if frame == previous_worker_frame {
			self.record_last_sample(&frame, fingerprint.clone());
			self.record_last_downward_observed_sample(&frame, fingerprint);
			self.worker_pairwise_previous_frame = frame;
			self.clear_preview_only_downward_recovery_carryover();
			self.log_decision(
				"scroll_capture.worker_pairwise_no_change",
				ScrollDirection::Down,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("frame_matches_last_committed_frame"),
			);

			return Ok(ScrollObserveOutcome::NoChange);
		}

		let Some(matched) =
			classify_vision_downward_sample_motion_against(&previous_worker_frame, &frame)
		else {
			self.record_last_sample(&frame, fingerprint.clone());
			self.record_last_downward_observed_sample(&frame, fingerprint);
			self.worker_pairwise_previous_frame = frame;
			self.clear_preview_only_downward_recovery_carryover();
			self.log_decision(
				"scroll_capture.worker_pairwise_no_change",
				ScrollDirection::Down,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("worker_pairwise_vision_no_downward_offset"),
			);

			return Ok(ScrollObserveOutcome::NoChange);
		};
		let corroborated_shift_rows =
			estimate_pairwise_downward_shift_rows(&previous_worker_frame, &frame);
		if matched.motion_rows >= WORKER_PAIRWISE_CORROBORATION_MIN_ROWS
			&& corroborated_shift_rows.is_none_or(|estimated| {
				estimated == 0
					|| matched.motion_rows.abs_diff(estimated)
						> WORKER_PAIRWISE_CORROBORATION_TOLERANCE_ROWS
			}) {
			self.record_last_sample(&frame, fingerprint.clone());
			self.record_last_downward_observed_sample(&frame, fingerprint);
			self.worker_pairwise_previous_frame = frame;
			self.clear_preview_only_downward_recovery_carryover();
			self.log_decision(
				"scroll_capture.worker_pairwise_growth_blocked",
				ScrollDirection::Down,
				Some(MotionObservation {
					direction: ScrollDirection::Down,
					motion_rows: matched.motion_rows,
				}),
				Some(self.current_viewport_top_y),
				Some(0),
				Some("worker_pairwise_large_growth_missing_corroboration"),
			);

			return Ok(ScrollObserveOutcome::NoChange);
		}

		let candidate_viewport_top_y = self
			.current_viewport_top_y
			.saturating_add(i32::try_from(matched.motion_rows).unwrap_or_default());
		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate_viewport_top_y);
		let frame_max_growth_rows = frame.height().saturating_sub(1).max(1);

		if growth_rows == 0 || growth_rows > frame_max_growth_rows {
			self.record_last_sample(&frame, fingerprint.clone());
			self.record_last_downward_observed_sample(&frame, fingerprint);
			self.worker_pairwise_previous_frame = frame;
			self.clear_preview_only_downward_recovery_carryover();
			self.log_decision(
				"scroll_capture.worker_pairwise_growth_blocked",
				ScrollDirection::Down,
				Some(MotionObservation {
					direction: ScrollDirection::Down,
					motion_rows: matched.motion_rows,
				}),
				Some(candidate_viewport_top_y),
				Some(growth_rows),
				Some("worker_pairwise_growth_exceeded_frame_bounds"),
			);

			return Ok(ScrollObserveOutcome::NoChange);
		}

		self.log_decision(
			"scroll_capture.worker_pairwise_growth_candidate",
			ScrollDirection::Down,
			Some(MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: matched.motion_rows,
			}),
			Some(candidate_viewport_top_y),
			Some(growth_rows),
			Some("worker_pairwise_vision"),
		);
		self.worker_pairwise_previous_frame = frame.clone();
		self.clear_preview_only_downward_recovery_carryover();

		self.apply_growth(
			frame.clone(),
			growth_rows,
			candidate_viewport_top_y,
			"worker_pairwise_vision",
			Some(matched.motion_rows),
			None,
			None,
		)
	}

	fn observe_sample_with_motion_context(
		&mut self,
		frame: RgbaImage,
		input_direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
		allow_burst_search: bool,
	) -> Result<ScrollObserveOutcome> {
		let previous_hint = self.transient_motion_rows_hint;
		let previous_burst = self.transient_burst_search_enabled;
		self.transient_motion_rows_hint = motion_rows_hint;
		self.transient_burst_search_enabled = allow_burst_search;
		self.record_last_sample_eval_context();
		let result = self.observe_sample(frame, input_direction);
		self.transient_motion_rows_hint = previous_hint;
		self.transient_burst_search_enabled = previous_burst;
		result
	}

	fn observe_sample(
		&mut self,
		frame: RgbaImage,
		input_direction: ScrollDirection,
	) -> Result<ScrollObserveOutcome> {
		self.clear_last_downward_sample_registration();

		if frame.width() != self.anchor_frame.width() {
			return Err(eyre::eyre!(
				"frame width mismatch: expected {} got {}",
				self.anchor_frame.width(),
				frame.width()
			));
		}
		let use_resume_local_sample = matches!(input_direction, ScrollDirection::Down)
			&& self.resume_frontier_top_y.is_some();

		if self.matches_downward_no_change_frame(input_direction, use_resume_local_sample, &frame) {
			self.log_decision(
				"scroll_capture.sample_no_change",
				input_direction,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("frame_matches_last_downward_observed_frame"),
			);
			return Ok(ScrollObserveOutcome::NoChange);
		}

		let fingerprint = scroll_capture_fingerprint(&frame);

		if matches!(input_direction, ScrollDirection::Up)
			&& self.resume_frontier_top_y.is_some()
			&& self.last_unconfirmed_upward_fingerprint.as_deref() == Some(fingerprint.as_slice())
		{
			self.log_decision(
				"scroll_capture.sample_no_change",
				input_direction,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("frame_matches_last_unconfirmed_upward_fingerprint"),
			);
			return Ok(ScrollObserveOutcome::NoChange);
		}

		let sample_delta =
			self.sample_delta_for_input(input_direction, use_resume_local_sample, &fingerprint);
		let (sample_motion, downward_sample_match) =
			self.classify_input_sample_motion(input_direction, use_resume_local_sample, &frame);
		let preview_changed =
			sample_delta.is_some_and(|delta| delta > 0) || sample_motion.is_some();

		tracing::info!(
			op = "scroll_capture.sample_eval",
			input_direction = ?input_direction,
			use_resume_local_sample,
			sample_delta,
			sample_motion_direction = ?sample_motion.map(|motion| motion.direction),
			sample_motion_rows = ?sample_motion.map(|motion| motion.motion_rows),
				preview_changed,
				frame_equals_last_sample = frame == self.last_sample_frame,
				frame_equals_last_downward_observed = frame == self.last_downward_observed_frame,
				frame_equals_last_preview_only_downward_local = self
					.last_preview_only_downward_local_sample
					.as_ref()
					.is_some_and(|previous| frame == previous.frame),
				last_preview_only_downward_local_viewport_top_y = ?self
					.last_preview_only_downward_local_sample
					.as_ref()
					.map(|sample| sample.viewport_top_y),
				frame_equals_last_committed = frame == self.last_committed_frame,
			last_motion_rows_hint = ?self.last_motion_rows_hint,
			transient_motion_rows_hint = ?self.transient_motion_rows_hint,
			effective_motion_rows_hint = ?self.effective_motion_rows_hint(),
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			"Scroll-capture session evaluated a sampled frame before commit resolution."
		);

		if !preview_changed {
			self.log_decision(
				"scroll_capture.sample_no_change",
				input_direction,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("sample_delta_and_motion_both_absent"),
			);
			return Ok(ScrollObserveOutcome::NoChange);
		}
		if matches!(input_direction, ScrollDirection::Up) {
			return self.observe_upward_input(
				frame,
				fingerprint,
				sample_delta,
				sample_motion,
				preview_changed,
			);
		}

		self.observe_downward_input(frame, sample_motion, downward_sample_match, preview_changed)
	}

	fn matches_downward_no_change_frame(
		&self,
		input_direction: ScrollDirection,
		use_resume_local_sample: bool,
		frame: &RgbaImage,
	) -> bool {
		matches!(input_direction, ScrollDirection::Down)
			&& !use_resume_local_sample
			&& frame
				== if self.initial_downward_bootstrap_active() {
					&self.last_sample_frame
				} else {
					&self.last_downward_observed_frame
				}
	}

	fn sample_delta_for_input(
		&self,
		input_direction: ScrollDirection,
		use_resume_local_sample: bool,
		fingerprint: &[u8],
	) -> Option<u32> {
		match (input_direction, use_resume_local_sample) {
			(ScrollDirection::Down, true) => self
				.last_sample_fingerprint
				.as_ref()
				.map(|previous| scroll_capture_fingerprint_delta(previous, fingerprint)),
			(ScrollDirection::Down, false) => self
				.last_downward_observed_fingerprint
				.as_ref()
				.map(|previous| scroll_capture_fingerprint_delta(previous, fingerprint)),
			(ScrollDirection::Up, _) => self
				.last_sample_fingerprint
				.as_ref()
				.map(|previous| scroll_capture_fingerprint_delta(previous, fingerprint)),
		}
	}

	fn classify_input_sample_motion(
		&mut self,
		input_direction: ScrollDirection,
		use_resume_local_sample: bool,
		frame: &RgbaImage,
	) -> (Option<MotionObservation>, Option<DownwardSampleMatch>) {
		if matches!(input_direction, ScrollDirection::Up) {
			return (self.classify_sample_motion(frame), None);
		}

		let downward_registration = if use_resume_local_sample {
			self.classify_downward_sample_motion_against(&self.last_sample_frame, frame)
				.0
				.map_source(DownwardSampleMatchSource::ObservedSample)
		} else {
			self.classify_downward_sample_motion_with_local_recovery(frame)
		};

		match downward_registration {
			DownwardRegistrationWithSource::Matched(matched) => {
				self.record_last_downward_sample_registration(
					"matched",
					Some(matched.source),
					Some(matched.matched.motion_rows),
				);
				(
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: matched.matched.motion_rows,
					}),
					Some(matched),
				)
			},
			DownwardRegistrationWithSource::Ambiguous { best, .. } => {
				self.record_last_downward_sample_registration(
					"ambiguous",
					Some(best.source),
					Some(best.matched.motion_rows),
				);
				self.log_decision(
					"scroll_capture.down_input_ambiguous_registration",
					ScrollDirection::Down,
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: best.matched.motion_rows,
					}),
					Some(self.current_viewport_top_y),
					Some(0),
					Some("sample_downward_registration_competed_with_far_apart_candidate"),
				);

				(None, None)
			},
			DownwardRegistrationWithSource::NoMatch => {
				self.record_last_downward_sample_registration("no_match", None, None);
				(None, None)
			},
		}
	}

	fn observe_downward_input(
		&mut self,
		frame: RgbaImage,
		sample_motion: Option<MotionObservation>,
		downward_sample_match: Option<DownwardSampleMatch>,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		self.last_unconfirmed_upward_fingerprint = None;

		if let Some(motion) = sample_motion {
			match motion.direction {
				ScrollDirection::Up => {
					let committed_down_match = self.evaluate_reference_overlap_direction(
						&self.last_committed_frame,
						&frame,
						ScrollDirection::Down,
						self.effective_motion_rows_hint(),
					);
					let committed_up_match = self.evaluate_reference_overlap_direction(
						&self.last_committed_frame,
						&frame,
						ScrollDirection::Up,
						self.effective_motion_rows_hint(),
					);

					if let Some(up_match) = upward_confirmation_match_for_downward_input(
						committed_up_match,
						committed_down_match,
						self.current_viewport_top_y > 0,
					) {
						return Ok(self.fail_closed_downward_non_monotonic_frame(
							preview_changed,
							self.last_sample_frame.clone(),
							self.last_sample_fingerprint.clone(),
							"scroll_capture.down_input_detected_upward_motion",
							MotionObservation {
								direction: ScrollDirection::Up,
								motion_rows: up_match.motion_rows,
							},
							"downward_input_confirmed_upward_motion_with_last_committed_match",
						));
					}

					return Ok(self.fail_closed_downward_non_monotonic_frame(
						preview_changed,
						self.last_sample_frame.clone(),
						self.last_sample_fingerprint.clone(),
						"scroll_capture.down_input_detected_upward_motion",
						motion,
						"downward_input_upward_motion_lacked_committed_support",
					));
				},
				ScrollDirection::Down => {
					return self.observe_downward_motion(
						frame,
						downward_sample_match.unwrap_or(DownwardSampleMatch {
							matched: DirectionMatch {
								mean_abs_diff_x100: u32::MAX,
								motion_rows: motion.motion_rows,
							},
							source: DownwardSampleMatchSource::ObservedSample,
						}),
						preview_changed,
					);
				},
			}
		}

		self.observe_fallback_downward_growth(frame, preview_changed)
	}

	fn observe_upward_input(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		sample_delta: Option<u32>,
		sample_motion: Option<MotionObservation>,
		_preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		let diagnostics = self.diagnose_upward_input(&frame);

		self.log_upward_input_diagnostics(&diagnostics, sample_delta, sample_motion, &frame);

		if let Some(outcome) = self.observe_upward_input_while_rewind_active(
			&frame,
			&fingerprint,
			sample_motion,
			&diagnostics,
		) {
			return Ok(outcome);
		}
		if let Some(motion) = sample_motion {
			return Ok(self.observe_upward_input_with_sample_motion(
				&frame,
				fingerprint,
				motion,
				&diagnostics,
			));
		}
		if let Some((up_match, from_committed)) = preferred_upward_input_override_match(
			diagnostics.sample_override_match,
			diagnostics.committed_override_match,
		) {
			let (op, block_reason) = if from_committed {
				(
					"scroll_capture.rewind_armed_from_committed_match",
					"upward_input_matched_last_committed_frame",
				)
			} else {
				("scroll_capture.rewind_armed", "upward_input_matched_last_sample_frame")
			};

			return Ok(self.arm_upward_rewind_with_match(
				&frame,
				fingerprint,
				up_match,
				from_committed,
				op,
				block_reason,
			));
		}

		self.log_decision(
			"scroll_capture.up_input_without_rewind_match",
			ScrollDirection::Up,
			None,
			None,
			None,
			Some("preview_changed_without_upward_match"),
		);

		Ok(self.arm_unconfirmed_upward_rewind(
			&frame,
			fingerprint,
			None,
			diagnostics.committed_down_match_eval.final_match.is_none(),
			"scroll_capture.rewind_armed_without_match",
			"upward_input_preview_changed_without_reliable_upward_proof",
		))
	}

	fn observe_upward_input_while_rewind_active(
		&mut self,
		frame: &RgbaImage,
		fingerprint: &[u8],
		sample_motion: Option<MotionObservation>,
		diagnostics: &UpwardInputDiagnostics,
	) -> Option<ScrollObserveOutcome> {
		if rewind_active_upward_motion_should_fail_closed(
			diagnostics.sample_override_match,
			diagnostics.committed_override_match,
			diagnostics.committed_down_match_eval.final_match,
			self.resume_frontier_top_y.is_some(),
		) {
			self.log_decision(
				"scroll_capture.rewind_armed_without_match",
				ScrollDirection::Up,
				sample_motion,
				None,
				None,
				Some("rewind_active_upward_input_conflicted_with_last_committed_downward_match"),
			);

			return Some(self.arm_unconfirmed_upward_rewind(
				frame,
				fingerprint.to_vec(),
				sample_motion,
				false,
				"scroll_capture.rewind_armed_without_match",
				"rewind_active_upward_input_conflicted_with_last_committed_downward_match",
			));
		}

		rewind_active_upward_override_match(
			diagnostics.sample_override_match,
			diagnostics.committed_override_match,
			self.resume_frontier_top_y.is_some(),
		)
		.map(|(up_match, from_committed)| {
			let (op, block_reason) = if from_committed {
				(
					"scroll_capture.rewind_armed_from_committed_match",
					"rewind_active_upward_input_preferred_conservative_last_committed_match",
				)
			} else {
				(
					"scroll_capture.rewind_armed",
					"rewind_active_upward_input_preferred_last_sample_match",
				)
			};

			self.arm_upward_rewind_with_match(
				frame,
				fingerprint.to_vec(),
				up_match,
				from_committed,
				op,
				block_reason,
			)
		})
	}

	fn observe_upward_input_with_sample_motion(
		&mut self,
		frame: &RgbaImage,
		fingerprint: Vec<u8>,
		motion: MotionObservation,
		diagnostics: &UpwardInputDiagnostics,
	) -> ScrollObserveOutcome {
		if matches!(motion.direction, ScrollDirection::Up) {
			self.record_last_sample(frame, fingerprint);
			self.observe_upward_rewind(motion.motion_rows);
			self.log_decision(
				"scroll_capture.rewind_armed",
				ScrollDirection::Up,
				Some(motion),
				None,
				None,
				Some("upward_input_classified_as_upward_motion"),
			);

			return ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up };
		}

		if let Some((up_match, from_committed)) = preferred_upward_input_override_match(
			diagnostics.sample_override_match,
			diagnostics.committed_override_match,
		) {
			let (op, block_reason) = if from_committed {
				(
					"scroll_capture.rewind_armed_from_committed_match",
					"upward_input_overrode_non_upward_sample_motion_with_last_committed_match",
				)
			} else {
				(
					"scroll_capture.rewind_armed",
					"upward_input_overrode_non_upward_sample_motion_with_last_sample_match",
				)
			};

			return self.arm_upward_rewind_with_match(
				frame,
				fingerprint,
				up_match,
				from_committed,
				op,
				block_reason,
			);
		}

		self.log_decision(
			"scroll_capture.up_input_motion_mismatch",
			ScrollDirection::Up,
			Some(motion),
			None,
			None,
			Some("upward_input_classified_as_non_upward_motion"),
		);

		self.arm_unconfirmed_upward_rewind(
			frame,
			fingerprint,
			Some(motion),
			diagnostics.committed_down_match_eval.final_match.is_none(),
			"scroll_capture.rewind_armed_without_match",
			"upward_input_preview_changed_without_reliable_upward_proof",
		)
	}

	fn diagnose_upward_input(&self, frame: &RgbaImage) -> UpwardInputDiagnostics {
		let effective_motion_rows_hint = self.effective_motion_rows_hint();
		let sample_down_match_eval = self.diagnose_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Down,
			effective_motion_rows_hint,
		);
		let sample_up_match_eval = self.diagnose_upward_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			effective_motion_rows_hint,
		);
		let committed_down_match_eval = self.diagnose_reference_overlap_direction(
			&self.last_committed_frame,
			frame,
			ScrollDirection::Down,
			effective_motion_rows_hint,
		);
		let committed_up_match_eval = self.diagnose_upward_reference_overlap_direction(
			&self.last_committed_frame,
			frame,
			effective_motion_rows_hint,
		);

		UpwardInputDiagnostics {
			sample_override_match: preferred_upward_override_match(
				sample_up_match_eval.final_match,
				sample_down_match_eval.final_match,
			),
			committed_override_match: preferred_upward_override_match(
				committed_up_match_eval.final_match,
				committed_down_match_eval.final_match,
			),
			sample_down_match_eval,
			sample_up_match_eval,
			committed_down_match_eval,
			committed_up_match_eval,
		}
	}

	fn diagnose_upward_reference_overlap_direction(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
	) -> DirectionMatchEval {
		let hinted_eval = self.diagnose_reference_overlap_direction(
			previous,
			next,
			ScrollDirection::Up,
			motion_rows_hint,
		);

		if hinted_eval.final_match.is_some() || motion_rows_hint.is_none() {
			return hinted_eval;
		}

		let config = OverlapSearchConfig::default();
		let max_motion_rows = max_directional_motion_rows(previous, next, config);
		let fallback_range = Some(OverlapSearchRange { start: 1, end: max_motion_rows });
		let fallback_eval = self.diagnose_reference_overlap_direction_with_preferred_range(
			previous,
			next,
			ScrollDirection::Up,
			fallback_range,
			false,
		);

		if fallback_eval.final_match.is_some() { fallback_eval } else { hinted_eval }
	}

	fn log_upward_input_diagnostics(
		&self,
		diagnostics: &UpwardInputDiagnostics,
		sample_delta: Option<u32>,
		sample_motion: Option<MotionObservation>,
		frame: &RgbaImage,
	) {
		self.log_up_input_match_eval(UpInputMatchLog {
			sample_motion,
			sample_down_match: diagnostics.sample_down_match_eval.final_match,
			sample_up_match: diagnostics.sample_up_match_eval.final_match,
			committed_down_match: diagnostics.committed_down_match_eval.final_match,
			committed_up_match: diagnostics.committed_up_match_eval.final_match,
			sample_override_wins: diagnostics.sample_override_match.is_some(),
			committed_override_wins: diagnostics.committed_override_match.is_some(),
		});
		self.log_up_input_search_window_eval(UpInputSearchWindowLog {
			sample_delta,
			sample_down_match_eval: &diagnostics.sample_down_match_eval,
			sample_up_match_eval: &diagnostics.sample_up_match_eval,
			committed_down_match_eval: &diagnostics.committed_down_match_eval,
			committed_up_match_eval: &diagnostics.committed_up_match_eval,
			frame_equals_last_sample: *frame == self.last_sample_frame,
			frame_equals_last_committed: *frame == self.last_committed_frame,
		});
	}

	fn arm_upward_rewind_with_match(
		&mut self,
		frame: &RgbaImage,
		fingerprint: Vec<u8>,
		up_match: DirectionMatch,
		from_committed: bool,
		op: &'static str,
		block_reason: &'static str,
	) -> ScrollObserveOutcome {
		self.last_unconfirmed_upward_fingerprint = None;

		self.record_last_sample(frame, fingerprint);

		if from_committed {
			self.observe_upward_rewind_from_committed(up_match.motion_rows);
		} else {
			self.observe_upward_rewind(up_match.motion_rows);
		}

		self.log_decision(
			op,
			ScrollDirection::Up,
			Some(MotionObservation {
				direction: ScrollDirection::Up,
				motion_rows: up_match.motion_rows,
			}),
			None,
			None,
			Some(block_reason),
		);

		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	}

	fn arm_unconfirmed_upward_rewind(
		&mut self,
		frame: &RgbaImage,
		fingerprint: Vec<u8>,
		detected_motion: Option<MotionObservation>,
		refresh_sample: bool,
		op: &'static str,
		block_reason: &'static str,
	) -> ScrollObserveOutcome {
		if self.current_viewport_top_y <= 0 && self.resume_frontier_top_y.is_none() {
			if refresh_sample {
				self.last_unconfirmed_upward_fingerprint = None;

				self.record_last_sample(frame, fingerprint);
			}

			return ScrollObserveOutcome::PreviewUpdated;
		}
		if refresh_sample {
			self.last_unconfirmed_upward_fingerprint = None;

			self.record_last_sample(frame, fingerprint);
		} else {
			self.last_unconfirmed_upward_fingerprint = Some(fingerprint.clone());
		}

		self.observe_unconfirmed_upward_rewind();
		self.log_decision(op, ScrollDirection::Up, detected_motion, None, None, Some(block_reason));

		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	}

	fn log_decision(
		&mut self,
		op: &'static str,
		input_direction: ScrollDirection,
		detected_motion: Option<MotionObservation>,
		candidate_viewport_top_y: Option<i32>,
		growth_rows: Option<u32>,
		block_reason: Option<&'static str>,
	) {
		self.last_block_reason = block_reason;
		tracing::info!(
			op,
			input_direction = ?input_direction,
			detected_direction = ?detected_motion.map(|motion| motion.direction),
			detected_motion_rows = ?detected_motion.map(|motion| motion.motion_rows),
			candidate_viewport_top_y = ?candidate_viewport_top_y,
			growth_rows = ?growth_rows,
			block_reason = ?block_reason,
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			export_height_px = self.export_image.height(),
			preview_height_px = self.preview_image.height(),
			"Scroll-capture session evaluated a motion decision."
		);
	}

	fn log_up_input_match_eval(&self, log: UpInputMatchLog) {
		tracing::info!(
			op = "scroll_capture.up_input_match_eval",
			sample_motion_direction = ?log.sample_motion.map(|motion| motion.direction),
			sample_motion_rows = ?log.sample_motion.map(|motion| motion.motion_rows),
			sample_down_match_rows = ?log.sample_down_match.map(|matched| matched.motion_rows),
			sample_down_match_mean_abs_diff_x100 =
				?log.sample_down_match.map(|matched| matched.mean_abs_diff_x100),
			sample_up_match_rows = ?log.sample_up_match.map(|matched| matched.motion_rows),
			sample_up_match_mean_abs_diff_x100 =
				?log.sample_up_match.map(|matched| matched.mean_abs_diff_x100),
			committed_down_match_rows = ?log.committed_down_match.map(|matched| matched.motion_rows),
			committed_down_match_mean_abs_diff_x100 =
				?log.committed_down_match.map(|matched| matched.mean_abs_diff_x100),
			committed_up_match_rows = ?log.committed_up_match.map(|matched| matched.motion_rows),
			committed_up_match_mean_abs_diff_x100 =
				?log.committed_up_match.map(|matched| matched.mean_abs_diff_x100),
			sample_override_wins = log.sample_override_wins,
			committed_override_wins = log.committed_override_wins,
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			"Scroll-capture session evaluated upward rewind match candidates."
		);
	}

	fn log_up_input_search_window_eval(&self, log: UpInputSearchWindowLog<'_>) {
		tracing::info!(
			op = "scroll_capture.up_input_search_window_eval",
			last_motion_rows_hint = ?self.last_motion_rows_hint,
			transient_motion_rows_hint = ?self.transient_motion_rows_hint,
			effective_motion_rows_hint = ?self.effective_motion_rows_hint(),
			sample_delta = ?log.sample_delta,
			frame_equals_last_sample = log.frame_equals_last_sample,
			frame_equals_last_committed = log.frame_equals_last_committed,
			sample_preferred_range_start =
				?log.sample_down_match_eval.preferred_range.map(|range| range.start),
			sample_preferred_range_end =
				?log.sample_down_match_eval.preferred_range.map(|range| range.end),
			sample_max_motion_rows = log.sample_down_match_eval.max_motion_rows,
			sample_down_preferred_only_rows =
				?log.sample_down_match_eval.preferred_only_match.map(|matched| matched.motion_rows),
			sample_down_preferred_only_mean_abs_diff_x100 = ?log.sample_down_match_eval
				.preferred_only_match
				.map(|matched| matched.mean_abs_diff_x100),
			sample_down_final_rows =
				?log.sample_down_match_eval.final_match.map(|matched| matched.motion_rows),
			sample_down_final_mean_abs_diff_x100 = ?log.sample_down_match_eval
				.final_match
				.map(|matched| matched.mean_abs_diff_x100),
			sample_down_used_full_range_fallback =
				log.sample_down_match_eval.used_full_range_fallback,
			sample_up_final_rows =
				?log.sample_up_match_eval.final_match.map(|matched| matched.motion_rows),
			sample_up_final_mean_abs_diff_x100 = ?log.sample_up_match_eval
				.final_match
				.map(|matched| matched.mean_abs_diff_x100),
			committed_preferred_range_start =
				?log.committed_down_match_eval.preferred_range.map(|range| range.start),
			committed_preferred_range_end =
				?log.committed_down_match_eval.preferred_range.map(|range| range.end),
			committed_max_motion_rows = log.committed_down_match_eval.max_motion_rows,
			committed_down_preferred_only_rows = ?log.committed_down_match_eval
				.preferred_only_match
				.map(|matched| matched.motion_rows),
			committed_down_preferred_only_mean_abs_diff_x100 = ?log.committed_down_match_eval
				.preferred_only_match
				.map(|matched| matched.mean_abs_diff_x100),
			committed_down_final_rows =
				?log.committed_down_match_eval.final_match.map(|matched| matched.motion_rows),
			committed_down_final_mean_abs_diff_x100 = ?log.committed_down_match_eval
				.final_match
				.map(|matched| matched.mean_abs_diff_x100),
			committed_down_used_full_range_fallback =
				log.committed_down_match_eval.used_full_range_fallback,
			committed_up_final_rows =
				?log.committed_up_match_eval.final_match.map(|matched| matched.motion_rows),
			committed_up_final_mean_abs_diff_x100 = ?log.committed_up_match_eval
				.final_match
				.map(|matched| matched.mean_abs_diff_x100),
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			"Scroll-capture session evaluated upward-input search windows."
		);
	}

	fn log_resume_frontier_match_eval(&self, log: ResumeFrontierMatchLog) {
		tracing::info!(
			op = "scroll_capture.resume_frontier_match_eval",
			motion_rows = log.motion_rows,
			candidate_observed_viewport_top_y = log.candidate_observed_viewport_top_y,
			residual_growth_rows = log.residual_growth_rows,
			raw_committed_down_match_rows =
				?log.raw_committed_down_match.map(|matched| matched.motion_rows),
			raw_committed_down_match_mean_abs_diff_x100 =
				?log.raw_committed_down_match.map(|matched| matched.mean_abs_diff_x100),
			trusted_committed_down_match_rows =
				?log.trusted_committed_down_match.map(|matched| matched.motion_rows),
			trusted_committed_down_match_mean_abs_diff_x100 =
				?log.trusted_committed_down_match.map(|matched| matched.mean_abs_diff_x100),
			committed_up_match_rows = ?log.committed_up_match.map(|matched| matched.motion_rows),
			committed_up_match_mean_abs_diff_x100 =
				?log.committed_up_match.map(|matched| matched.mean_abs_diff_x100),
			frame_reacquires_last_committed_viewport = log.frame_reacquires_last_committed_viewport,
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			"Scroll-capture session evaluated resume-frontier match candidates."
		);
	}

	fn record_last_sample(&mut self, frame: &RgbaImage, fingerprint: Vec<u8>) {
		self.last_sample_frame = frame.clone();
		self.last_sample_fingerprint = Some(fingerprint);
	}

	fn record_last_downward_observed_sample(&mut self, frame: &RgbaImage, fingerprint: Vec<u8>) {
		self.last_downward_observed_frame = frame.clone();
		self.last_downward_observed_fingerprint = Some(fingerprint);
	}

	fn record_preview_only_downward_local_sample(
		&mut self,
		frame: &RgbaImage,
		viewport_top_y: i32,
	) {
		self.last_preview_only_downward_local_sample =
			Some(PreviewOnlyDownwardLocalSample { frame: frame.clone(), viewport_top_y });
	}

	fn clear_preview_only_downward_local_sample(&mut self) {
		self.last_preview_only_downward_local_sample = None;
		self.seeded_preview_only_local_after_observed_burst_commit = false;
		self.pending_unresolved_burst_registered_growth_viewport_top_y = None;
		self.last_blocked_preview_only_local_candidate = None;
	}

	fn clear_preview_only_downward_recovery_carryover(&mut self) {
		self.clear_preview_only_downward_local_sample();
		self.pending_suppressed_huge_preview_only_local_followup = None;
		self.pending_suppressed_huge_preview_only_local_followup_remaining_blocks = 0;
		self.pending_extreme_preview_only_local_tail_followup = None;
		self.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 0;
	}

	fn restore_last_sample(&mut self, frame: RgbaImage, fingerprint: Option<Vec<u8>>) {
		self.last_sample_frame = frame;
		self.last_sample_fingerprint = fingerprint;
	}

	fn fail_closed_downward_non_monotonic_frame(
		&mut self,
		preview_changed: bool,
		previous_sample_frame: RgbaImage,
		previous_sample_fingerprint: Option<Vec<u8>>,
		op: &'static str,
		detected_motion: MotionObservation,
		block_reason: &'static str,
	) -> ScrollObserveOutcome {
		self.restore_last_sample(previous_sample_frame, previous_sample_fingerprint);
		self.clear_preview_only_downward_local_sample();
		self.log_decision(
			op,
			ScrollDirection::Down,
			Some(detected_motion),
			Some(self.current_viewport_top_y),
			Some(0),
			Some(block_reason),
		);

		preview_update_outcome(preview_changed)
	}

	fn observe_upward_rewind(&mut self, motion_rows: u32) {
		let motion_rows = i32::try_from(motion_rows).unwrap_or(i32::MAX);

		self.observe_upward_rewind_to_observed_top_y(
			self.observed_viewport_top_y.saturating_sub(motion_rows),
			self.current_viewport_top_y,
		);
	}

	fn observe_upward_rewind_from_committed(&mut self, motion_rows: u32) {
		let motion_rows = i32::try_from(motion_rows).unwrap_or(i32::MAX);

		self.observe_upward_rewind_to_observed_top_y(
			self.current_viewport_top_y.saturating_sub(motion_rows),
			self.current_viewport_top_y,
		);
	}

	fn observe_upward_rewind_to_observed_top_y(
		&mut self,
		observed_viewport_top_y: i32,
		frontier_top_y: i32,
	) {
		self.last_motion_rows_hint = None;
		self.clear_preview_only_downward_local_sample();
		self.resume_frontier_requires_reacquire = true;

		self.resume_frontier_top_y.get_or_insert(frontier_top_y);

		self.observed_viewport_top_y = observed_viewport_top_y;
	}

	fn observe_unconfirmed_upward_rewind(&mut self) {
		self.last_motion_rows_hint = None;
		self.clear_preview_only_downward_local_sample();

		let frontier_top_y = self.current_viewport_top_y;

		self.resume_frontier_top_y.get_or_insert(frontier_top_y);

		self.resume_frontier_requires_reacquire = true;
		self.observed_viewport_top_y =
			self.observed_viewport_top_y.min(frontier_top_y.saturating_sub(1));
	}

	fn observe_downward_motion(
		&mut self,
		frame: RgbaImage,
		observed_match: DownwardSampleMatch,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		let motion_rows = observed_match.matched.motion_rows;

		if self.resume_frontier_top_y.is_some() {
			return self.observe_downward_motion_while_resume_frontier_active(
				frame,
				motion_rows,
				preview_changed,
			);
		}

		let candidate = match self.resolve_downward_viewport_candidate(&frame, observed_match) {
			DownwardViewportResolution::NoMatch => {
				let reset_preview_only_local_baseline =
					self.should_reset_preview_only_local_baseline_after_huge_far_committed_block();
				let preview_only_local_viewport_top_y = if self
					.blocked_underconsumed_observed_recovery_in_burst
					|| self.blocked_lagging_exactly_corroborated_preview_local_tail_in_burst
					|| self.blocked_followup_after_suppressed_huge_preview_local_jump
					|| self.blocked_followup_after_extreme_preview_local_tail
					|| self.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump
				{
					self.stable_preview_only_downward_local_viewport_top_y()
				} else {
					self.preview_only_downward_local_viewport_top_y_for_sample_match(observed_match)
				};
				let block_reason = if self.blocked_underconsumed_observed_recovery_in_burst {
					"underconsumed_observed_recovery_under_transient_burst"
				} else if self.blocked_lagging_exactly_corroborated_preview_local_tail_in_burst {
					"lagging_exactly_corroborated_preview_local_tail_under_transient_burst"
				} else if self.blocked_followup_after_suppressed_huge_preview_local_jump {
					"followup_after_suppressed_huge_preview_local_jump_under_transient_burst"
				} else if self.blocked_followup_after_extreme_preview_local_tail {
					"followup_after_extreme_preview_local_tail_under_transient_burst"
				} else if self
					.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump
				{
					"far_committed_only_recovery_after_corroborated_huge_local_jump_under_transient_burst"
				} else {
					"no_downward_viewport_candidate_resolved"
				};
				self.pending_unresolved_burst_registered_growth_viewport_top_y = if block_reason
					== "no_downward_viewport_candidate_resolved"
					&& self.last_downward_sample_registration_result == Some("matched")
				{
					self.last_downward_sample_registration_provisional_viewport_top_y.filter(
						|viewport_top_y| {
							self.transient_burst_growth_matches_pending_hint_band(*viewport_top_y)
						},
					)
				} else {
					None
				};
				self.consecutive_transient_burst_missing_downward_candidate_frames = if self
					.transient_burst_search_enabled
					&& preview_only_local_viewport_top_y.is_some()
				{
					self.consecutive_transient_burst_missing_downward_candidate_frames
						.saturating_add(1)
				} else {
					0
				};
				self.refresh_local_downward_sample(&frame);
				if self.should_refresh_downward_observed_baseline_after_huge_suppressed_jump() {
					self.record_last_downward_observed_sample(
						&frame,
						scroll_capture_fingerprint(&frame),
					);
				}
				if reset_preview_only_local_baseline {
					self.clear_preview_only_downward_local_sample();
				} else {
					self.refresh_preview_only_downward_local_sample(
						&frame,
						preview_only_local_viewport_top_y,
					);
				}
				self.log_decision(
					"scroll_capture.downward_viewport_authority_missing",
					ScrollDirection::Down,
					Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
					None,
					Some(0),
					Some(block_reason),
				);
				return Ok(preview_update_outcome(preview_changed));
			},
			DownwardViewportResolution::Selected(candidate) => candidate,
			DownwardViewportResolution::Ambiguous { preferred, competing } => {
				self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
				self.refresh_local_downward_sample(&frame);
				self.refresh_preview_only_downward_local_sample(
					&frame,
					self.preview_only_downward_local_viewport_top_y_for_sample_match(
						observed_match,
					),
				);
				self.log_decision(
					"scroll_capture.downward_viewport_authority_ambiguous",
					ScrollDirection::Down,
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: preferred.motion_rows,
					}),
					Some(preferred.viewport_top_y),
					Some(0),
					Some(preferred.competing_block_reason(competing)),
				);

				return Ok(preview_update_outcome(preview_changed));
			},
		};

		if self.should_fail_closed_tiny_observed_recovery_in_burst(candidate) {
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
			self.refresh_local_downward_sample(&frame);
			self.refresh_preview_only_downward_local_sample(
				&frame,
				self.stable_preview_only_downward_local_viewport_top_y(),
			);
			self.log_decision(
				"scroll_capture.downward_growth_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(candidate.viewport_top_y),
				Some(candidate.motion_rows),
				Some("tiny_observed_recovery_under_transient_burst"),
			);
			return Ok(preview_update_outcome(preview_changed));
		}
		if self.should_fail_closed_outsized_observed_recovery_after_one_pixel_preview_local_commit(
			candidate,
		) {
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
			self.refresh_local_downward_sample(&frame);
			self.refresh_preview_only_downward_local_sample(
				&frame,
				self.stable_preview_only_downward_local_viewport_top_y(),
			);
			self.log_decision(
				"scroll_capture.downward_growth_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(candidate.viewport_top_y),
				Some(candidate.motion_rows),
				Some("outsized_observed_recovery_after_one_pixel_preview_local_commit"),
			);
			return Ok(preview_update_outcome(preview_changed));
		}
		if self.should_fail_closed_tiny_preview_only_local_recovery_in_burst(candidate) {
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
			self.refresh_local_downward_sample(&frame);
			self.refresh_preview_only_downward_local_sample(
				&frame,
				self.stable_preview_only_downward_local_viewport_top_y(),
			);
			self.log_decision(
				"scroll_capture.downward_growth_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(candidate.viewport_top_y),
				Some(candidate.motion_rows),
				Some("tiny_preview_only_local_recovery_under_transient_burst"),
			);
			return Ok(preview_update_outcome(preview_changed));
		}
		if self
			.should_fail_closed_exactly_corroborated_preview_local_tail_in_extreme_burst(candidate)
		{
			self.pending_extreme_preview_only_local_tail_followup = Some(candidate);
			self.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 1;
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
			self.refresh_local_downward_sample(&frame);
			self.refresh_preview_only_downward_local_sample(
				&frame,
				self.stable_preview_only_downward_local_viewport_top_y(),
			);
			self.log_decision(
				"scroll_capture.downward_growth_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(candidate.viewport_top_y),
				Some(candidate.motion_rows),
				Some("exactly_corroborated_preview_local_tail_under_extreme_transient_burst"),
			);
			return Ok(preview_update_outcome(preview_changed));
		}
		if self.should_fail_closed_preview_only_local_tail_after_unresolved_burst(candidate) {
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
			self.refresh_local_downward_sample(&frame);
			self.refresh_preview_only_downward_local_sample(
				&frame,
				self.stable_preview_only_downward_local_viewport_top_y(),
			);
			self.log_decision(
				"scroll_capture.downward_growth_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(candidate.viewport_top_y),
				Some(candidate.motion_rows),
				Some("preview_only_local_tail_after_unresolved_transient_burst"),
			);
			return Ok(preview_update_outcome(preview_changed));
		}
		if self.should_fail_closed_tiny_committed_keyframe_recovery_in_burst(candidate) {
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
			self.refresh_local_downward_sample(&frame);
			self.refresh_preview_only_downward_local_sample(
				&frame,
				self.stable_preview_only_downward_local_viewport_top_y(),
			);
			self.log_decision(
				"scroll_capture.downward_growth_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(candidate.viewport_top_y),
				Some(candidate.motion_rows),
				Some("tiny_committed_keyframe_recovery_under_transient_burst"),
			);
			return Ok(preview_update_outcome(preview_changed));
		}

		self.observe_downward_growth_to_viewport(
			frame,
			candidate.viewport_top_y,
			preview_changed,
			Some(MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: candidate.motion_rows,
			}),
			candidate.source.decision_source(),
		)
	}

	fn should_fail_closed_tiny_observed_recovery_in_burst(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.source == DownwardViewportCandidateSource::ObservedSample
			&& candidate.motion_rows <= TINY_OBSERVED_BURST_RECOVERY_MAX_MOTION_ROWS
			&& self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
			&& self
				.last_motion_rows_hint
				.is_some_and(|last_hint| last_hint >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS)
			&& self.last_preview_only_downward_local_sample.is_none()
	}

	fn should_fail_closed_outsized_observed_recovery_after_one_pixel_preview_local_commit(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.source == DownwardViewportCandidateSource::ObservedSample
			&& candidate.motion_rows >= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS.saturating_mul(2)
			&& self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
			&& self.last_motion_rows_hint == Some(1)
			&& self.growth_history.last().is_some_and(|commit| {
				commit.growth_rows == 1
					&& commit.decision_source
						== DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
			})
	}

	fn should_fail_closed_tiny_preview_only_local_recovery_in_burst(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		if self.seeded_preview_only_local_catch_up_candidate_can_commit(candidate) {
			return false;
		}
		let small_recovery_lags_recent_continuity =
			self.last_motion_rows_hint.is_some_and(|last_hint| {
				last_hint >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
					&& candidate.motion_rows
						<= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.saturating_div(2)
					&& candidate.motion_rows
						< last_hint.saturating_sub(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
			});

		candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			&& candidate.motion_rows <= TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS
			&& candidate.mean_abs_diff_x100 > DIRECTION_WARNING_MARGIN_X100.saturating_mul(2)
			&& self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				last_hint >= TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MIN_LAST_HINT_ROWS
			}) || candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			&& small_recovery_lags_recent_continuity
			&& self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
	}

	fn should_fail_closed_exactly_corroborated_preview_local_tail_in_extreme_burst(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let Some(transient_motion_rows_hint) = self.normalized_transient_motion_rows_hint() else {
			return false;
		};

		candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			&& candidate.motion_rows <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& candidate.motion_rows >= last_motion_rows_hint.saturating_mul(2)
			&& transient_motion_rows_hint
				>= last_motion_rows_hint
					.saturating_mul(EXTREME_TRANSIENT_PREVIEW_LOCAL_TAIL_MULTIPLIER)
			&& self.last_observed_sample_registration_result == Some("matched")
			&& self.last_observed_sample_registration_motion_rows == Some(candidate.motion_rows)
			&& self.growth_history.iter().rev().take(2).count() == 2
			&& self.growth_history.iter().rev().take(2).all(|commit| {
				commit.decision_source
					== DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
			}) && self.last_downward_viewport_candidates_before_prune.as_ref().is_some_and(|value| {
			let exact_committed = format!(
				"CommittedKeyframe@{}/{}:",
				candidate.viewport_top_y, candidate.motion_rows
			);
			value.contains(&exact_committed)
		})
	}

	fn should_fail_closed_preview_only_local_tail_after_unresolved_burst(
		&mut self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let Some(transient_motion_rows_hint) = self.normalized_transient_motion_rows_hint() else {
			return false;
		};
		let candidate_is_extreme_preview_local_tail = candidate.source
			== DownwardViewportCandidateSource::PreviewOnlyLocalSample
			&& self.last_block_reason == Some("no_downward_viewport_candidate_resolved")
			&& self.transient_burst_search_enabled;
		let unresolved_burst_has_registered_growth_in_pending_band =
			candidate_is_extreme_preview_local_tail
				&& self
					.pending_unresolved_burst_registered_growth_viewport_top_y
					.take()
					.is_some_and(|viewport_top_y| {
						self.transient_burst_growth_matches_pending_hint_band(viewport_top_y)
					});

		candidate_is_extreme_preview_local_tail
			&& !unresolved_burst_has_registered_growth_in_pending_band
			&& candidate.motion_rows <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& candidate.motion_rows >= last_motion_rows_hint.saturating_mul(2)
			&& transient_motion_rows_hint
				>= last_motion_rows_hint
					.saturating_mul(EXTREME_TRANSIENT_PREVIEW_LOCAL_TAIL_MULTIPLIER)
			&& self
				.last_preview_only_downward_local_sample
				.as_ref()
				.is_some_and(|sample| sample.viewport_top_y == self.current_viewport_top_y)
	}

	fn should_fail_closed_tiny_committed_keyframe_recovery_in_burst(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y);

		candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
			&& growth_rows <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
			&& candidate.motion_rows
				> growth_rows.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS.saturating_mul(2))
			&& candidate.mean_abs_diff_x100 > DIRECTION_WARNING_MARGIN_X100.saturating_mul(2)
			&& self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				last_hint >= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS.saturating_add(2)
			})
	}

	fn observe_downward_motion_while_resume_frontier_active(
		&mut self,
		frame: RgbaImage,
		motion_rows: u32,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		let candidate_observed_viewport_top_y = self
			.observed_viewport_top_y
			.saturating_add(i32::try_from(motion_rows).unwrap_or_default());
		let Some(resume_frontier_top_y) = self.resume_frontier_top_y else {
			return Ok(preview_update_outcome(preview_changed));
		};
		let frame_reacquires_last_committed_viewport =
			self.frame_reacquires_last_committed_viewport(&frame);

		if let Some(outcome) = self.handle_resume_frontier_reacquire_block(
			motion_rows,
			preview_changed,
			resume_frontier_top_y,
			frame_reacquires_last_committed_viewport,
		) {
			return Ok(outcome);
		}

		let match_context = ResumeFrontierDirectMatchContext {
			motion_rows,
			candidate_observed_viewport_top_y,
			residual_growth_rows: self
				.growth_rows_for_candidate_viewport_top_y(candidate_observed_viewport_top_y),
		};

		if self.resume_frontier_requires_reacquire {
			if let Some(outcome) = self.block_resume_frontier_before_growth(
				motion_rows,
				preview_changed,
				resume_frontier_top_y,
				candidate_observed_viewport_top_y,
				&frame,
			) {
				return Ok(outcome);
			}

			return self.resolve_resume_frontier_direct_match(
				frame,
				preview_changed,
				frame_reacquires_last_committed_viewport,
				match_context,
			);
		}

		if let Some(outcome) = self.block_resume_frontier_before_growth(
			motion_rows,
			preview_changed,
			resume_frontier_top_y,
			candidate_observed_viewport_top_y,
			&frame,
		) {
			return Ok(outcome);
		}

		self.observed_viewport_top_y = resume_frontier_top_y;

		if match_context.residual_growth_rows == 0 {
			self.log_decision(
				"scroll_capture.resume_frontier_still_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("resume_active_candidate_reached_frontier_without_residual_growth"),
			);

			return Ok(preview_update_outcome(preview_changed));
		}

		self.resolve_resume_frontier_direct_match(
			frame,
			preview_changed,
			frame_reacquires_last_committed_viewport,
			match_context,
		)
	}

	fn handle_resume_frontier_reacquire_block(
		&mut self,
		motion_rows: u32,
		preview_changed: bool,
		resume_frontier_top_y: i32,
		frame_reacquires_last_committed_viewport: bool,
	) -> Option<ScrollObserveOutcome> {
		if !self.resume_frontier_requires_reacquire {
			return None;
		}
		if !frame_reacquires_last_committed_viewport {
			return None;
		}

		self.resume_frontier_requires_reacquire = false;
		self.observed_viewport_top_y = resume_frontier_top_y;

		self.log_decision(
			"scroll_capture.resume_frontier_still_blocked",
			ScrollDirection::Down,
			Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
			Some(self.observed_viewport_top_y),
			Some(0),
			Some("resume_active_reacquired_last_committed_frame"),
		);

		Some(preview_update_outcome(preview_changed))
	}

	fn block_resume_frontier_before_growth(
		&mut self,
		motion_rows: u32,
		preview_changed: bool,
		resume_frontier_top_y: i32,
		candidate_observed_viewport_top_y: i32,
		frame: &RgbaImage,
	) -> Option<ScrollObserveOutcome> {
		if frame == &self.last_committed_frame {
			self.observed_viewport_top_y = resume_frontier_top_y;

			self.log_decision(
				"scroll_capture.resume_frontier_still_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("resume_active_frame_matches_last_committed_frame"),
			);

			return Some(preview_update_outcome(preview_changed));
		}
		if self.resume_frontier_requires_reacquire {
			return None;
		}
		if candidate_observed_viewport_top_y < resume_frontier_top_y {
			self.observed_viewport_top_y = candidate_observed_viewport_top_y;

			self.log_decision(
				"scroll_capture.resume_frontier_still_blocked",
				ScrollDirection::Down,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("resume_active_candidate_observed_viewport_still_below_frontier"),
			);

			return Some(preview_update_outcome(preview_changed));
		}

		None
	}

	fn blocked_resume_frontier_observed_viewport_top_y(
		&self,
		candidate_observed_viewport_top_y: i32,
		preserve_candidate_progress: bool,
	) -> i32 {
		if preserve_candidate_progress || !self.resume_frontier_requires_reacquire {
			return candidate_observed_viewport_top_y;
		}

		self.resume_frontier_top_y.map_or(
			candidate_observed_viewport_top_y,
			|resume_frontier_top_y| {
				candidate_observed_viewport_top_y.min(resume_frontier_top_y.saturating_sub(1))
			},
		)
	}

	fn resolve_resume_frontier_direct_match(
		&mut self,
		frame: RgbaImage,
		preview_changed: bool,
		frame_reacquires_last_committed_viewport: bool,
		context: ResumeFrontierDirectMatchContext,
	) -> Result<ScrollObserveOutcome> {
		let direct_match_hint_rows = Some(self.resume_frontier_direct_match_hint_rows(context));
		let raw_committed_down_match = self.evaluate_reference_overlap_direction_preferred_only(
			&self.last_committed_frame,
			&frame,
			ScrollDirection::Down,
			direct_match_hint_rows,
		);
		let trusted_committed_down_match =
			raw_committed_down_match.filter(|matched| resume_direct_match_is_trustworthy(*matched));
		let committed_up_match = self.evaluate_reference_overlap_direction_preferred_only(
			&self.last_committed_frame,
			&frame,
			ScrollDirection::Up,
			direct_match_hint_rows,
		);

		self.log_resume_frontier_match_eval(ResumeFrontierMatchLog {
			motion_rows: context.motion_rows,
			candidate_observed_viewport_top_y: context.candidate_observed_viewport_top_y,
			residual_growth_rows: context.residual_growth_rows,
			raw_committed_down_match,
			trusted_committed_down_match,
			committed_up_match,
			frame_reacquires_last_committed_viewport,
		});

		let preserve_candidate_progress = self.resume_frontier_should_preserve_blocked_progress(
			&frame,
			context,
			committed_up_match,
		);

		match (trusted_committed_down_match, committed_up_match) {
			(Some(down), Some(up))
				if down.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< up.mean_abs_diff_x100 =>
			{
				self.resume_frontier_commit_direct_match(frame, preview_changed, down, context)
			},
			(Some(down), Some(up))
				if up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100 =>
			{
				Ok(self.block_resume_frontier_direct_match(
					context,
					preview_changed,
					false,
					MotionObservation {
						direction: ScrollDirection::Up,
						motion_rows: up.motion_rows,
					},
					"resume_active_sample_motion_matched_above_committed_frontier",
				))
			},
			(Some(_down), Some(_up)) => Ok(self.block_resume_frontier_direct_match(
				context,
				preview_changed,
				false,
				MotionObservation {
					direction: ScrollDirection::Down,
					motion_rows: context.motion_rows,
				},
				"resume_active_direct_committed_match_ambiguous",
			)),
			(Some(down), None) => {
				self.resume_frontier_commit_direct_match(frame, preview_changed, down, context)
			},
			(None, Some(up)) => Ok(self.block_resume_frontier_direct_match(
				context,
				preview_changed,
				false,
				MotionObservation { direction: ScrollDirection::Up, motion_rows: up.motion_rows },
				"resume_active_direct_committed_match_still_above_frontier",
			)),
			(None, None) => Ok(self.block_resume_frontier_without_direct_match(
				context,
				preview_changed,
				preserve_candidate_progress,
				raw_committed_down_match.is_some(),
			)),
		}
	}

	fn block_resume_frontier_direct_match(
		&mut self,
		context: ResumeFrontierDirectMatchContext,
		preview_changed: bool,
		preserve_candidate_progress: bool,
		detected_motion: MotionObservation,
		block_reason: &'static str,
	) -> ScrollObserveOutcome {
		self.observed_viewport_top_y = self.blocked_resume_frontier_observed_viewport_top_y(
			context.candidate_observed_viewport_top_y,
			preserve_candidate_progress,
		);

		self.log_decision(
			"scroll_capture.resume_frontier_still_blocked",
			ScrollDirection::Down,
			Some(detected_motion),
			Some(self.observed_viewport_top_y),
			Some(0),
			Some(block_reason),
		);

		preview_update_outcome(preview_changed)
	}

	fn block_resume_frontier_without_direct_match(
		&mut self,
		context: ResumeFrontierDirectMatchContext,
		preview_changed: bool,
		preserve_candidate_progress: bool,
		has_raw_committed_down_match: bool,
	) -> ScrollObserveOutcome {
		let block_reason = if has_raw_committed_down_match {
			"resume_active_direct_committed_match_too_weak"
		} else {
			"resume_active_direct_committed_match_not_ready"
		};

		self.block_resume_frontier_direct_match(
			context,
			preview_changed,
			preserve_candidate_progress,
			MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: context.residual_growth_rows,
			},
			block_reason,
		)
	}

	fn resume_frontier_should_preserve_blocked_progress(
		&self,
		frame: &RgbaImage,
		context: ResumeFrontierDirectMatchContext,
		committed_up_match: Option<DirectionMatch>,
	) -> bool {
		if !self.resume_frontier_requires_reacquire || context.residual_growth_rows == 0 {
			return false;
		}
		if committed_up_match.is_some() {
			return false;
		}

		let sample_down_match = self.evaluate_reference_overlap_direction_preferred_only(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Down,
			Some(context.motion_rows),
		);
		let sample_up_match = self.evaluate_reference_overlap_direction_preferred_only(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Up,
			Some(context.motion_rows),
		);

		matches!(
			(sample_down_match, sample_up_match),
			(Some(down), Some(up))
				if down.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< up.mean_abs_diff_x100
		) || matches!((sample_down_match, sample_up_match), (Some(_), None))
	}

	fn resume_frontier_direct_match_hint_rows(
		&self,
		context: ResumeFrontierDirectMatchContext,
	) -> u32 {
		if !self.resume_frontier_requires_reacquire {
			return context.residual_growth_rows;
		}
		if context.residual_growth_rows > 0 {
			return context.residual_growth_rows;
		}

		context.motion_rows
	}

	fn resume_frontier_commit_direct_match(
		&mut self,
		frame: RgbaImage,
		preview_changed: bool,
		down: DirectionMatch,
		context: ResumeFrontierDirectMatchContext,
	) -> Result<ScrollObserveOutcome> {
		let candidate_viewport_top_y = if self.resume_frontier_requires_reacquire {
			let resume_frontier_top_y =
				self.resume_frontier_top_y.unwrap_or(self.current_viewport_top_y);

			resume_frontier_top_y
				.saturating_add(i32::try_from(down.motion_rows).unwrap_or_default())
		} else {
			let growth_rows = down.motion_rows.min(context.residual_growth_rows);

			self.current_viewport_top_y
				.saturating_add(i32::try_from(growth_rows).unwrap_or_default())
		};

		self.observe_downward_growth_to_viewport(
			frame,
			candidate_viewport_top_y,
			preview_changed,
			Some(MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: down.motion_rows,
			}),
			"resume_active_direct_committed_frontier_match",
		)
	}

	fn growth_rows_for_candidate_viewport_top_y(&self, candidate_viewport_top_y: i32) -> u32 {
		self.resume_frontier_top_y.map_or_else(
			|| {
				u32::try_from(candidate_viewport_top_y.saturating_sub(self.current_viewport_top_y))
					.unwrap_or_default()
			},
			|frontier_top_y| {
				if candidate_viewport_top_y <= frontier_top_y {
					0
				} else {
					u32::try_from(candidate_viewport_top_y - frontier_top_y).unwrap_or_default()
				}
			},
		)
	}

	fn frame_reacquires_last_committed_viewport(&self, frame: &RgbaImage) -> bool {
		frame == &self.last_committed_frame
	}

	fn observe_downward_growth_to_viewport(
		&mut self,
		frame: RgbaImage,
		candidate_viewport_top_y: i32,
		preview_changed: bool,
		detected_motion: Option<MotionObservation>,
		decision_source: &'static str,
	) -> Result<ScrollObserveOutcome> {
		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate_viewport_top_y);
		let effective_motion_rows_hint = self.effective_motion_rows_hint();
		self.pending_unresolved_burst_registered_growth_viewport_top_y = None;

		if self.bootstrap_initial_growth_cap_rows().is_some_and(|cap| growth_rows > cap) {
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
			self.log_decision(
				"scroll_capture.downward_growth_blocked",
				ScrollDirection::Down,
				detected_motion,
				Some(candidate_viewport_top_y),
				Some(growth_rows),
				Some("bootstrap_growth_exceeded_initial_growth_cap"),
			);

			return Ok(preview_update_outcome(preview_changed));
		}

		if growth_rows == 0 {
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
			let block_reason = if self.resume_frontier_top_y.is_some() {
				Some("candidate_viewport_did_not_pass_resume_frontier")
			} else {
				Some("candidate_viewport_did_not_advance_current_frontier")
			};

			self.log_decision(
				"scroll_capture.downward_growth_blocked",
				ScrollDirection::Down,
				detected_motion,
				Some(candidate_viewport_top_y),
				Some(growth_rows),
				block_reason,
			);

			return Ok(preview_update_outcome(preview_changed));
		}
		let max_growth_rows = self.max_downward_growth_rows_for_frame(&frame);

		if growth_rows > max_growth_rows {
			self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
			self.log_decision(
				"scroll_capture.downward_growth_blocked",
				ScrollDirection::Down,
				detected_motion,
				Some(candidate_viewport_top_y),
				Some(growth_rows),
				Some("candidate_viewport_growth_exceeded_monotonic_cap"),
			);

			return Ok(preview_update_outcome(preview_changed));
		}
		self.log_decision(
			"scroll_capture.downward_growth_candidate",
			ScrollDirection::Down,
			detected_motion,
			Some(candidate_viewport_top_y),
			Some(growth_rows),
			Some(decision_source),
		);
		self.consecutive_transient_burst_missing_downward_candidate_frames = 0;
		let previous_motion_rows_hint = self.last_motion_rows_hint;
		self.last_motion_rows_hint = Some(growth_rows);

		self.apply_growth(
			frame,
			growth_rows,
			candidate_viewport_top_y,
			decision_source,
			detected_motion.map(|motion| motion.motion_rows),
			effective_motion_rows_hint,
			previous_motion_rows_hint,
		)
	}

	fn max_downward_growth_rows_for_frame(&self, frame: &RgbaImage) -> u32 {
		let config = OverlapSearchConfig::default();
		let effective_min_overlap = if frame.height() <= config.min_overlap_rows {
			1
		} else {
			config.min_overlap_rows.max(1)
		};
		let frame_max_growth_rows = frame.height().saturating_sub(effective_min_overlap).max(1);

		if self.transient_burst_search_enabled {
			return self
				.transient_motion_rows_hint
				.map(|hint| {
					frame_max_growth_rows.min(hint.max(INITIAL_DOWNWARD_MAX_MOTION_ROWS)).max(1)
				})
				.unwrap_or(frame_max_growth_rows.clamp(1, INITIAL_DOWNWARD_MAX_MOTION_ROWS));
		}

		frame_max_growth_rows.clamp(1, INITIAL_DOWNWARD_MAX_MOTION_ROWS)
	}

	fn classify_sample_motion(&self, frame: &RgbaImage) -> Option<MotionObservation> {
		let effective_motion_rows_hint = self.effective_motion_rows_hint();
		let down_match = self.evaluate_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Down,
			effective_motion_rows_hint,
		);
		let up_match = self.evaluate_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Up,
			effective_motion_rows_hint,
		);

		match (down_match, up_match) {
			(Some(down), Some(up)) => {
				if down.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< up.mean_abs_diff_x100
				{
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: down.motion_rows,
					})
				} else if up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100
				{
					Some(MotionObservation {
						direction: ScrollDirection::Up,
						motion_rows: up.motion_rows,
					})
				} else {
					None
				}
			},
			(Some(down), None) => Some(MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: down.motion_rows,
			}),
			(None, Some(up)) => Some(MotionObservation {
				direction: ScrollDirection::Up,
				motion_rows: up.motion_rows,
			}),
			(None, None) => None,
		}
	}

	fn classify_downward_sample_motion(
		&self,
		frame: &RgbaImage,
	) -> (DownwardRegistration, Option<&'static str>) {
		let previous = if self.initial_downward_bootstrap_active() {
			&self.last_sample_frame
		} else {
			&self.last_downward_observed_frame
		};

		self.classify_downward_sample_motion_against(previous, frame)
	}

	fn classify_downward_sample_motion_with_local_recovery(
		&mut self,
		frame: &RgbaImage,
	) -> DownwardRegistrationWithSource {
		let (primary_raw, primary_reason) = self.classify_downward_sample_motion(frame);
		let primary = primary_raw.map_source(DownwardSampleMatchSource::ObservedSample);
		self.record_registration_diagnostics(
			DownwardSampleMatchSource::ObservedSample,
			primary,
			primary_reason,
		);
		let Some(previous_local) = self.last_preview_only_downward_local_sample.as_ref() else {
			return primary;
		};

		let (local_raw, local_reason) =
			self.classify_preview_only_local_recovery_motion_against(&previous_local.frame, frame);
		let local = local_raw.map_source(DownwardSampleMatchSource::PreviewOnlyLocalSample);
		self.record_registration_diagnostics(
			DownwardSampleMatchSource::PreviewOnlyLocalSample,
			local,
			local_reason,
		);

		match (primary, local) {
			(
				DownwardRegistrationWithSource::Matched(primary),
				DownwardRegistrationWithSource::Matched(local),
			) => {
				if self.should_prefer_preview_only_local_recovery_after_extreme_tail_block(
					primary, local,
				) {
					DownwardRegistrationWithSource::Matched(local)
				} else if self
					.should_prefer_observed_sample_over_preview_only_local_recovery(primary, local)
				{
					DownwardRegistrationWithSource::Matched(primary)
				} else if self
					.should_prefer_preview_only_local_recovery_over_observed_sample(primary, local)
				{
					DownwardRegistrationWithSource::Matched(local)
				} else if local.matched.mean_abs_diff_x100 <= primary.matched.mean_abs_diff_x100 {
					DownwardRegistrationWithSource::Matched(local)
				} else {
					DownwardRegistrationWithSource::Matched(primary)
				}
			},
			(DownwardRegistrationWithSource::Matched(primary), _) => {
				DownwardRegistrationWithSource::Matched(primary)
			},
			(_, DownwardRegistrationWithSource::Matched(local)) => {
				DownwardRegistrationWithSource::Matched(local)
			},
			(primary, _) => primary,
		}
	}

	fn should_prefer_observed_sample_over_preview_only_local_recovery(
		&self,
		primary: DownwardSampleMatch,
		local: DownwardSampleMatch,
	) -> bool {
		let small_local_recovery_lags_recent_continuity =
			self.last_motion_rows_hint.is_some_and(|last_hint| {
				last_hint >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
					&& local.matched.motion_rows
						<= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.saturating_div(2)
					&& local.matched.motion_rows
						< last_hint.saturating_sub(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
			});

		small_local_recovery_lags_recent_continuity
			&& self.transient_burst_motion_hint_exceeds_local_authority(local.matched.motion_rows)
			&& primary.matched.motion_rows
				> local
					.matched
					.motion_rows
					.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				primary
					.matched
					.motion_rows
					.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
					>= last_hint && primary.matched.motion_rows <= last_hint
			}) && self
			.transient_pending_growth_cap_rows()
			.is_some_and(|cap| primary.matched.motion_rows <= cap)
	}

	fn should_prefer_preview_only_local_recovery_after_extreme_tail_block(
		&self,
		primary: DownwardSampleMatch,
		local: DownwardSampleMatch,
	) -> bool {
		let Some(pending_candidate) = self.pending_extreme_preview_only_local_tail_followup else {
			return false;
		};
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let Some(transient_motion_rows_hint) = self.normalized_transient_motion_rows_hint() else {
			return false;
		};

		primary.source == DownwardSampleMatchSource::ObservedSample
			&& local.source == DownwardSampleMatchSource::PreviewOnlyLocalSample
			&& primary.matched.motion_rows == pending_candidate.motion_rows
			&& local.matched.motion_rows >= last_motion_rows_hint
			&& local.matched.motion_rows < primary.matched.motion_rows
			&& local.matched.motion_rows <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& transient_motion_rows_hint
				>= last_motion_rows_hint
					.saturating_mul(EXTREME_TRANSIENT_PREVIEW_LOCAL_TAIL_MULTIPLIER)
	}

	fn should_prefer_preview_only_local_recovery_over_observed_sample(
		&self,
		primary: DownwardSampleMatch,
		local: DownwardSampleMatch,
	) -> bool {
		self.transient_burst_search_enabled
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				last_hint <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
					&& (local.matched.motion_rows >= last_hint
						|| (local.matched.motion_rows
							<= TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS
							&& self.consecutive_transient_burst_missing_downward_candidate_frames
								>= 2)) && local.matched.motion_rows
					<= last_hint.saturating_add(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS)
					&& primary.matched.motion_rows
						> local
							.matched
							.motion_rows
							.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
			}) || self
			.preview_only_local_slowdown_tail_followup_can_prefer_observed_override(primary, local)
	}

	fn preview_only_local_slowdown_tail_followup_can_prefer_observed_override(
		&self,
		primary: DownwardSampleMatch,
		local: DownwardSampleMatch,
	) -> bool {
		self.transient_burst_search_enabled
			&& self.last_preview_only_downward_local_sample.is_some()
			&& local.source == DownwardSampleMatchSource::PreviewOnlyLocalSample
			&& primary.source == DownwardSampleMatchSource::ObservedSample
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				let tiny_followup = last_hint <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
					&& local.matched.motion_rows
						<= TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS;
				let near_continuity_followup = last_hint
					<= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
					&& local.matched.motion_rows
						<= last_hint.saturating_add(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS);

				(tiny_followup || near_continuity_followup)
					&& primary.matched.motion_rows
						> local
							.matched
							.motion_rows
							.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
			}) && self.growth_history.last().is_some_and(|commit| {
			commit.decision_source
				== DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
				&& self
					.last_motion_rows_hint
					.is_some_and(|last_hint| commit.growth_rows <= last_hint)
		})
	}

	fn record_registration_diagnostics(
		&mut self,
		source: DownwardSampleMatchSource,
		registration: DownwardRegistrationWithSource,
		reason: Option<&'static str>,
	) {
		let (result, motion_rows, mean_abs_diff_x100) = match registration {
			DownwardRegistrationWithSource::NoMatch => ("no_match", None, None),
			DownwardRegistrationWithSource::Matched(matched) => (
				"matched",
				Some(matched.matched.motion_rows),
				Some(matched.matched.mean_abs_diff_x100),
			),
			DownwardRegistrationWithSource::Ambiguous { best, .. } => {
				("ambiguous", Some(best.matched.motion_rows), Some(best.matched.mean_abs_diff_x100))
			},
		};

		match source {
			DownwardSampleMatchSource::ObservedSample => {
				self.last_observed_sample_registration_result = Some(result);
				self.last_observed_sample_registration_reason = reason;
				self.last_observed_sample_registration_motion_rows = motion_rows;
				self.last_observed_sample_registration_mean_abs_diff_x100 = mean_abs_diff_x100;
			},
			DownwardSampleMatchSource::PreviewOnlyLocalSample => {
				self.last_preview_only_local_registration_result = Some(result);
				self.last_preview_only_local_registration_reason = reason;
				self.last_preview_only_local_registration_motion_rows = motion_rows;
				self.last_preview_only_local_registration_mean_abs_diff_x100 = mean_abs_diff_x100;
			},
		}
	}

	fn classify_downward_sample_motion_against(
		&self,
		previous: &RgbaImage,
		frame: &RgbaImage,
	) -> (DownwardRegistration, Option<&'static str>) {
		let config = OverlapSearchConfig::default();
		let preferred_ranges = self.sequential_downward_motion_ranges(previous, frame, config);

		let (registration, reason) = self
			.evaluate_reference_downward_registration_with_preferred_ranges(
				previous,
				frame,
				self.last_motion_rows_hint,
				&preferred_ranges,
				self.transient_burst_search_enabled,
			);

		match registration {
			DownwardRegistration::Matched(matched)
				if self.bootstrap_motion_exceeds_pending_hint(matched.motion_rows) =>
			{
				(DownwardRegistration::NoMatch, Some("bootstrap_hint_exceeded"))
			},
			other => (other, reason),
		}
	}

	fn classify_preview_only_local_recovery_motion_against(
		&self,
		previous: &RgbaImage,
		frame: &RgbaImage,
	) -> (DownwardRegistration, Option<&'static str>) {
		let config = OverlapSearchConfig::default();
		let preferred_range =
			self.preview_only_local_recovery_motion_range(previous, frame, config);
		let preferred_ranges = preferred_range.into_iter().collect::<Vec<_>>();
		let motion_rows_hint =
			self.last_motion_rows_hint.or(self.normalized_transient_motion_rows_hint());

		let (registration, reason) = self
			.evaluate_reference_downward_registration_with_preferred_ranges(
				previous,
				frame,
				motion_rows_hint,
				&preferred_ranges,
				self.transient_burst_search_enabled,
			);

		match registration {
			DownwardRegistration::Matched(matched)
				if self.bootstrap_motion_exceeds_pending_hint(matched.motion_rows) =>
			{
				(DownwardRegistration::NoMatch, Some("bootstrap_hint_exceeded"))
			},
			other => (other, reason),
		}
	}

	fn effective_motion_rows_hint(&self) -> Option<u32> {
		let transient = self.normalized_transient_motion_rows_hint();

		match (self.last_motion_rows_hint, transient) {
			(Some(last), Some(_transient)) => Some(last),
			(Some(last), None) => Some(last),
			(None, Some(transient)) => Some(transient),
			(None, None) => None,
		}
	}

	fn normalized_transient_motion_rows_hint(&self) -> Option<u32> {
		let transient = self.transient_motion_rows_hint?;
		if self.transient_burst_search_enabled {
			return Some(transient);
		}

		match self.last_motion_rows_hint {
			Some(last) => {
				let cap = last
					.saturating_mul(TRANSIENT_MOTION_HINT_MAX_MULTIPLIER)
					.max(TRANSIENT_MOTION_HINT_MIN_CAP_ROWS)
					.max(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS);

				(transient <= cap).then_some(transient)
			},
			None => Some(transient.min(INITIAL_DOWNWARD_MAX_MOTION_ROWS)),
		}
	}

	fn transient_burst_motion_hint_exceeds_local_authority(&self, local_motion_rows: u32) -> bool {
		if !self.transient_burst_search_enabled {
			return false;
		}

		let Some(transient) = self.transient_motion_rows_hint else {
			return false;
		};
		let capped_local_motion_rows =
			local_motion_rows.min(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS);
		let local_authority_rows = self
			.last_motion_rows_hint
			.unwrap_or(capped_local_motion_rows)
			.max(capped_local_motion_rows);
		let local_authority_cap = local_authority_rows
			.saturating_mul(TRANSIENT_MOTION_HINT_MAX_MULTIPLIER)
			.max(TRANSIENT_MOTION_HINT_MIN_CAP_ROWS);

		transient > local_authority_cap
	}

	fn preview_only_local_recovery_motion_range(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let max_motion_rows = max_directional_motion_rows(previous, next, config);

		if max_motion_rows == 0 {
			return None;
		}

		if self.initial_downward_bootstrap_active() && self.last_motion_rows_hint.is_none() {
			if let Some(hint) = self.normalized_transient_motion_rows_hint()
				&& hint <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			{
				let tolerance = (hint / 2)
					.clamp(1, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS)
					.min(max_motion_rows);
				let min_motion_rows = hint.saturating_sub(tolerance).max(1);
				let max_motion_rows = hint
					.saturating_add(tolerance)
					.min(max_motion_rows)
					.min(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS);

				return Some(min_motion_rows..=max_motion_rows);
			}

			return Some(
				1..=PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.min(max_motion_rows).max(1),
			);
		}

		if let Some(hint) =
			self.last_motion_rows_hint.or(self.normalized_transient_motion_rows_hint())
		{
			let tolerance = (hint / 2)
				.clamp(1, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS)
				.min(max_motion_rows);
			let min_motion_rows = if self.seeded_preview_only_local_after_observed_burst_commit
				|| self.preview_only_local_tail_followup_can_include_one_pixel_recovery()
			{
				1
			} else {
				hint.saturating_sub(tolerance).max(1)
			};
			let max_motion_rows = hint
				.saturating_add(tolerance)
				.min(max_motion_rows)
				.min(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS);

			return Some(min_motion_rows..=max_motion_rows);
		}

		Some(1..=PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.min(max_motion_rows).max(1))
	}

	fn preview_only_local_tail_followup_can_include_one_pixel_recovery(&self) -> bool {
		self.transient_burst_search_enabled
			&& self.last_preview_only_downward_local_sample.is_some()
			&& self
				.last_motion_rows_hint
				.is_some_and(|last_hint| last_hint <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
			&& self.growth_history.last().is_some_and(|commit| {
				commit.decision_source
					== DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
					&& commit.growth_rows <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
			})
	}

	fn refresh_local_downward_sample(&mut self, frame: &RgbaImage) {
		if !self.initial_downward_bootstrap_active() {
			return;
		}

		self.last_unconfirmed_upward_fingerprint = None;
		let fingerprint = scroll_capture_fingerprint(frame);
		self.record_last_sample(frame, fingerprint);
	}

	fn refresh_preview_only_downward_local_sample(
		&mut self,
		frame: &RgbaImage,
		provisional_viewport_top_y: Option<i32>,
	) {
		let Some(provisional_viewport_top_y) = provisional_viewport_top_y else {
			self.clear_preview_only_downward_local_sample();
			return;
		};
		if !self.should_refresh_preview_only_downward_local_sample(frame) {
			return;
		}
		self.last_unconfirmed_upward_fingerprint = None;
		self.record_preview_only_downward_local_sample(frame, provisional_viewport_top_y);
	}

	fn should_refresh_downward_observed_baseline_after_huge_suppressed_jump(&self) -> bool {
		self.pending_suppressed_huge_preview_only_local_followup.is_some()
			|| self.blocked_followup_after_suppressed_huge_preview_local_jump
			|| self.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump
	}

	fn should_reset_preview_only_local_baseline_after_huge_far_committed_block(&self) -> bool {
		self.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump
	}

	fn provisional_viewport_top_y_for_downward_sample_match(
		&self,
		observed_match: DownwardSampleMatch,
	) -> Option<i32> {
		let motion_rows = i32::try_from(observed_match.matched.motion_rows).unwrap_or_default();

		match observed_match.source {
			DownwardSampleMatchSource::ObservedSample => {
				Some(self.observed_viewport_top_y.saturating_add(motion_rows))
			},
			DownwardSampleMatchSource::PreviewOnlyLocalSample => self
				.last_preview_only_downward_local_sample
				.as_ref()
				.map(|sample| sample.viewport_top_y.saturating_add(motion_rows)),
		}
	}

	fn preview_only_downward_local_viewport_top_y_for_sample_match(
		&self,
		observed_match: DownwardSampleMatch,
	) -> Option<i32> {
		let provisional_viewport_top_y =
			self.provisional_viewport_top_y_for_downward_sample_match(observed_match)?;
		let candidate = DownwardViewportCandidate {
			source: observed_match.source.into(),
			viewport_top_y: provisional_viewport_top_y,
			motion_rows: observed_match.matched.motion_rows,
			mean_abs_diff_x100: observed_match.matched.mean_abs_diff_x100,
		};

		if self.should_suppress_observed_sample_candidate(candidate)
			|| self.should_suppress_preview_only_local_candidate(candidate)
		{
			return self.stable_preview_only_downward_local_viewport_top_y();
		}

		Some(provisional_viewport_top_y)
	}

	fn stable_preview_only_downward_local_viewport_top_y(&self) -> Option<i32> {
		self.last_preview_only_downward_local_sample
			.as_ref()
			.map(|sample| sample.viewport_top_y)
			.or(Some(self.observed_viewport_top_y))
	}

	fn should_refresh_preview_only_downward_local_sample(&self, frame: &RgbaImage) -> bool {
		if self.resume_frontier_top_y.is_some() || self.resume_frontier_requires_reacquire {
			return false;
		}
		if self.last_sample_frame != self.last_downward_observed_frame {
			return false;
		}
		if frame == &self.anchor_frame || frame == &self.last_committed_frame {
			return false;
		}
		if self
			.last_preview_only_downward_local_sample
			.as_ref()
			.is_some_and(|previous| *frame == previous.frame)
		{
			return false;
		}

		!self.growth_history.iter().any(|commit| frame == &commit.frame)
	}

	fn initial_downward_bootstrap_active(&self) -> bool {
		self.growth_history.is_empty()
			&& self.current_viewport_top_y == 0
			&& self.resume_frontier_top_y.is_none()
			&& !self.resume_frontier_requires_reacquire
	}

	fn bootstrap_motion_cap_from_pending_hint(&self) -> Option<u32> {
		if !self.initial_downward_bootstrap_active() || self.last_motion_rows_hint.is_some() {
			return None;
		}

		self.normalized_transient_motion_rows_hint().map(|hint| {
			let tolerance = (hint / 2).clamp(1, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS);

			hint.saturating_add(tolerance)
		})
	}

	fn bootstrap_motion_exceeds_pending_hint(&self, motion_rows: u32) -> bool {
		self.bootstrap_motion_cap_from_pending_hint().is_some_and(|cap| motion_rows > cap)
	}

	fn bootstrap_initial_growth_cap_rows(&self) -> Option<u32> {
		if !self.initial_downward_bootstrap_active() || self.last_motion_rows_hint.is_some() {
			return None;
		}

		self.bootstrap_motion_cap_from_pending_hint()
			.map(|cap| cap.min(BOOTSTRAP_HINTED_INITIAL_GROWTH_MAX_ROWS))
	}

	pub(crate) fn preview_image(&self) -> &RgbaImage {
		&self.preview_image
	}

	pub(crate) fn preview_display_image(&self) -> RgbaImage {
		self.export_image.clone()
	}

	pub(crate) fn preview_display_mode(&self) -> &'static str {
		"committed"
	}

	pub(crate) fn export_image(&self) -> &RgbaImage {
		&self.export_image
	}

	pub(crate) fn current_viewport_top_y(&self) -> i32 {
		self.current_viewport_top_y
	}

	pub(crate) fn export_dimensions(&self) -> (u32, u32) {
		self.export_image.dimensions()
	}

	pub(crate) fn commit_telemetry(&self) -> ScrollCommitTelemetry {
		let last_commit = self.growth_history.last();

		ScrollCommitTelemetry {
			current_viewport_top_y: self.current_viewport_top_y,
			preview_dimensions: self.preview_image.dimensions(),
			export_dimensions: self.export_image.dimensions(),
			growth_commit_count: self.growth_history.len(),
			preview_segment_count: self.bottom_preview_segments.len(),
			export_segment_count: self.bottom_segments.len(),
			preview_export_segments_aligned: self.bottom_segments.len()
				== self.bottom_preview_segments.len()
				&& self.bottom_segments.len() == self.growth_history.len(),
			last_commit_decision_source: last_commit.map(|commit| commit.decision_source),
			last_commit_detected_motion_rows: last_commit
				.and_then(|commit| commit.detected_motion_rows),
			last_commit_effective_motion_rows_hint: last_commit
				.and_then(|commit| commit.effective_motion_rows_hint),
			last_block_reason: self.last_block_reason,
			last_downward_sample_registration_result: self.last_downward_sample_registration_result,
			last_downward_sample_registration_source: self.last_downward_sample_registration_source,
			last_downward_sample_registration_motion_rows: self
				.last_downward_sample_registration_motion_rows,
			last_downward_sample_registration_provisional_viewport_top_y: self
				.last_downward_sample_registration_provisional_viewport_top_y,
			observed_sample_registration_result: self.last_observed_sample_registration_result,
			observed_sample_registration_reason: self.last_observed_sample_registration_reason,
			observed_sample_registration_motion_rows: self
				.last_observed_sample_registration_motion_rows,
			observed_sample_registration_mean_abs_diff_x100: self
				.last_observed_sample_registration_mean_abs_diff_x100,
			preview_only_local_registration_result: self
				.last_preview_only_local_registration_result,
			preview_only_local_registration_reason: self
				.last_preview_only_local_registration_reason,
			preview_only_local_registration_motion_rows: self
				.last_preview_only_local_registration_motion_rows,
			preview_only_local_registration_mean_abs_diff_x100: self
				.last_preview_only_local_registration_mean_abs_diff_x100,
			last_downward_viewport_candidate_count: self.last_downward_viewport_candidate_count,
			last_downward_viewport_candidates_before_prune: self
				.last_downward_viewport_candidates_before_prune
				.clone(),
			last_downward_viewport_candidates_after_prune: self
				.last_downward_viewport_candidates_after_prune
				.clone(),
			sample_eval_last_motion_rows_hint: self.last_sample_eval_last_motion_rows_hint,
			sample_eval_transient_motion_rows_hint: self
				.last_sample_eval_transient_motion_rows_hint,
			sample_eval_effective_motion_rows_hint: self
				.last_sample_eval_effective_motion_rows_hint,
			sample_eval_transient_burst_search_enabled: self
				.last_sample_eval_transient_burst_search_enabled,
			preview_only_local_viewport_top_y: self
				.last_preview_only_downward_local_sample
				.as_ref()
				.map(|sample| sample.viewport_top_y),
			last_preview_segment_height_px: self
				.bottom_preview_segments
				.last()
				.map(RgbaImage::height),
			last_export_segment_height_px: self.bottom_segments.last().map(RgbaImage::height),
		}
	}

	pub(crate) fn undo_last_append(&mut self) -> bool {
		let Some(_commit) = self.growth_history.pop() else {
			return false;
		};
		let _ = self.bottom_segments.pop();
		let _ = self.bottom_preview_segments.pop();
		let Ok(export_image) = self.rebuild_export_image() else {
			return false;
		};
		let Ok(preview_image) = self.rebuild_preview_image() else {
			return false;
		};

		self.export_image = export_image;
		self.preview_image = preview_image;

		if let Some(previous) = self.growth_history.last() {
			self.last_motion_rows_hint = Some(previous.growth_rows);
			self.current_viewport_top_y = previous.viewport_top_y;
			self.observed_viewport_top_y = previous.viewport_top_y;
			self.last_committed_frame = previous.frame.clone();
			self.worker_pairwise_previous_frame = previous.frame.clone();
			self.last_sample_frame = previous.frame.clone();
			self.last_sample_fingerprint = Some(scroll_capture_fingerprint(&previous.frame));
			self.last_downward_observed_frame = previous.frame.clone();
			self.last_downward_observed_fingerprint =
				Some(scroll_capture_fingerprint(&previous.frame));
			self.clear_preview_only_downward_local_sample();
			self.last_unconfirmed_upward_fingerprint = None;
			self.resume_frontier_top_y = None;
			self.resume_frontier_requires_reacquire = false;
		} else {
			self.last_committed_frame = self.anchor_frame.clone();
			self.worker_pairwise_previous_frame = self.anchor_frame.clone();
			self.last_sample_frame = self.anchor_frame.clone();
			self.last_sample_fingerprint = Some(scroll_capture_fingerprint(&self.anchor_frame));
			self.last_downward_observed_frame = self.anchor_frame.clone();
			self.last_downward_observed_fingerprint =
				Some(scroll_capture_fingerprint(&self.anchor_frame));
			self.clear_preview_only_downward_local_sample();
			self.last_unconfirmed_upward_fingerprint = None;
			self.last_motion_rows_hint = None;
			self.current_viewport_top_y = 0;
			self.observed_viewport_top_y = 0;
			self.resume_frontier_top_y = None;
			self.resume_frontier_requires_reacquire = false;
		}

		true
	}

	fn evaluate_reference_overlap_direction(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
	) -> Option<DirectionMatch> {
		let config = OverlapSearchConfig::default();
		let preferred_range =
			self.preferred_motion_range_from_hint(previous, next, motion_rows_hint, config)?;

		evaluate_overlap_direction(previous, next, direction, preferred_range, config)
	}

	fn evaluate_reference_downward_registration(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		allow_full_range_fallback: bool,
	) -> DownwardRegistration {
		let config = OverlapSearchConfig::default();
		let preferred_range = self.preferred_downward_motion_range_from_hint(
			previous,
			next,
			motion_rows_hint,
			config,
		);

		self.evaluate_reference_downward_registration_with_preferred_range(
			previous,
			next,
			motion_rows_hint,
			preferred_range,
			allow_full_range_fallback,
		)
	}

	fn evaluate_reference_downward_registration_with_preferred_ranges(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		preferred_ranges: &[RangeInclusive<u32>],
		allow_full_range_fallback: bool,
	) -> (DownwardRegistration, Option<&'static str>) {
		let config = OverlapSearchConfig::default();
		let max_overlap = previous.height().min(next.height());
		let max_motion_rows = max_directional_motion_rows(previous, next, config);
		let mut candidates = collect_overlap_direction_matches_in_ranges(
			previous,
			next,
			ScrollDirection::Down,
			preferred_ranges,
			config,
		);
		let mut no_match_reason = if candidates.is_empty() { Some("no_candidates") } else { None };

		if candidates.is_empty()
			&& allow_full_range_fallback
			&& (motion_rows_hint.is_none() || self.transient_burst_search_enabled)
		{
			candidates = collect_overlap_direction_matches(
				previous,
				next,
				ScrollDirection::Down,
				1..=max_motion_rows,
				config,
			);
			no_match_reason = if candidates.is_empty() { Some("no_candidates") } else { None };
		}
		candidates.retain(|matched| {
			downward_registration_has_meaningful_overlap(*matched, max_overlap, config)
		});
		if candidates.is_empty() {
			no_match_reason.get_or_insert("insufficient_overlap");
		}

		let classification = classify_downward_registration_candidates(&candidates);
		let upward_veto = self.evaluate_reference_overlap_direction(
			previous,
			next,
			ScrollDirection::Up,
			motion_rows_hint,
		);

		match (classification, upward_veto) {
			(DownwardRegistration::Matched(down), Some(up))
				if up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100 =>
			{
				(DownwardRegistration::NoMatch, Some("upward_veto"))
			},
			(DownwardRegistration::NoMatch, _) => (DownwardRegistration::NoMatch, no_match_reason),
			(other, _) => (other, None),
		}
	}

	fn evaluate_reference_downward_registration_with_preferred_range(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		preferred_range: Option<RangeInclusive<u32>>,
		allow_full_range_fallback: bool,
	) -> DownwardRegistration {
		let config = OverlapSearchConfig::default();
		let max_overlap = previous.height().min(next.height());
		let max_motion_rows = max_directional_motion_rows(previous, next, config);
		let mut candidates = preferred_range.as_ref().map_or_else(Vec::new, |range| {
			collect_overlap_direction_matches(
				previous,
				next,
				ScrollDirection::Down,
				range.clone(),
				config,
			)
		});
		let mut no_match_reason = if candidates.is_empty() { Some("no_candidates") } else { None };

		if candidates.is_empty()
			&& allow_full_range_fallback
			&& (motion_rows_hint.is_none() || self.transient_burst_search_enabled)
		{
			candidates = collect_overlap_direction_matches(
				previous,
				next,
				ScrollDirection::Down,
				1..=max_motion_rows,
				config,
			);
			no_match_reason = if candidates.is_empty() { Some("no_candidates") } else { None };
		}
		candidates.retain(|matched| {
			downward_registration_has_meaningful_overlap(*matched, max_overlap, config)
		});
		if candidates.is_empty() {
			no_match_reason.get_or_insert("insufficient_overlap");
		}

		let classification = classify_downward_registration_candidates(&candidates);
		let upward_veto = self.evaluate_reference_overlap_direction(
			previous,
			next,
			ScrollDirection::Up,
			motion_rows_hint,
		);

		match (classification, upward_veto) {
			(DownwardRegistration::Matched(down), Some(up))
				if up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100 =>
			{
				DownwardRegistration::NoMatch
			},
			(DownwardRegistration::NoMatch, _) => {
				let _ = no_match_reason;
				DownwardRegistration::NoMatch
			},
			(other, _) => other,
		}
	}

	fn sequential_downward_motion_ranges(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		config: OverlapSearchConfig,
	) -> Vec<RangeInclusive<u32>> {
		let mut ranges = Vec::new();
		let local_motion_rows_hint = self.last_motion_rows_hint;
		if let Some(local_range) = self.preferred_local_downward_motion_range_from_hint(
			previous,
			next,
			local_motion_rows_hint,
			config,
		) {
			ranges.push(local_range);
		}
		if self.initial_downward_bootstrap_active() && self.last_motion_rows_hint.is_none() {
			return ranges;
		}
		if let Some(transient_range) = self.transient_downward_motion_range(previous, next, config)
			&& !ranges.contains(&transient_range)
		{
			ranges.push(transient_range);
		}

		ranges
	}

	fn clear_last_downward_sample_registration(&mut self) {
		self.last_downward_sample_registration_result = None;
		self.last_downward_sample_registration_source = None;
		self.last_downward_sample_registration_motion_rows = None;
		self.last_downward_sample_registration_provisional_viewport_top_y = None;
		self.last_observed_sample_registration_result = None;
		self.last_observed_sample_registration_reason = None;
		self.last_observed_sample_registration_motion_rows = None;
		self.last_observed_sample_registration_mean_abs_diff_x100 = None;
		self.last_preview_only_local_registration_result = None;
		self.last_preview_only_local_registration_reason = None;
		self.last_preview_only_local_registration_motion_rows = None;
		self.last_preview_only_local_registration_mean_abs_diff_x100 = None;
		self.last_downward_viewport_candidate_count = None;
		self.last_downward_viewport_candidates_before_prune = None;
		self.last_downward_viewport_candidates_after_prune = None;
		self.blocked_underconsumed_observed_recovery_in_burst = false;
		self.blocked_lagging_exactly_corroborated_preview_local_tail_in_burst = false;
		self.blocked_followup_after_suppressed_huge_preview_local_jump = false;
		self.blocked_followup_after_extreme_preview_local_tail = false;
		self.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump = false;
	}

	fn record_last_downward_sample_registration(
		&mut self,
		result: &'static str,
		source: Option<DownwardSampleMatchSource>,
		motion_rows: Option<u32>,
	) {
		self.last_downward_sample_registration_result = Some(result);
		self.last_downward_sample_registration_source =
			source.map(DownwardSampleMatchSource::label);
		self.last_downward_sample_registration_motion_rows = motion_rows;
	}

	fn record_last_sample_eval_context(&mut self) {
		self.last_sample_eval_last_motion_rows_hint = self.last_motion_rows_hint;
		self.last_sample_eval_transient_motion_rows_hint = self.transient_motion_rows_hint;
		self.last_sample_eval_effective_motion_rows_hint = self.effective_motion_rows_hint();
		self.last_sample_eval_transient_burst_search_enabled = self.transient_burst_search_enabled;
	}

	fn transient_downward_motion_range(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let transient_motion_rows_hint = self.normalized_transient_motion_rows_hint()?;
		let max_motion_rows = max_directional_motion_rows(previous, next, config);

		if transient_motion_rows_hint == 0 || transient_motion_rows_hint > max_motion_rows {
			return None;
		}

		let tolerance = (transient_motion_rows_hint / 2)
			.clamp(
				LOCAL_DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS,
				LOCAL_DOWNWARD_SEARCH_MAX_TOLERANCE_ROWS,
			)
			.min(max_motion_rows);
		let min_motion_rows = transient_motion_rows_hint.saturating_sub(tolerance).max(1);
		let max_motion_rows =
			transient_motion_rows_hint.saturating_add(tolerance).min(max_motion_rows);

		Some(min_motion_rows..=max_motion_rows)
	}

	fn preferred_local_downward_motion_range_from_hint(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let max_motion_rows = max_directional_motion_rows(previous, next, config);

		if let Some(last_growth_rows) = motion_rows_hint {
			let tolerance = (last_growth_rows / 2)
				.clamp(
					LOCAL_DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS,
					LOCAL_DOWNWARD_SEARCH_MAX_TOLERANCE_ROWS,
				)
				.min(max_motion_rows);
			let min_motion_rows = last_growth_rows.saturating_sub(tolerance).max(1);
			let max_motion_rows = last_growth_rows.saturating_add(tolerance).min(max_motion_rows);

			return Some(min_motion_rows..=max_motion_rows);
		}

		Some(1..=INITIAL_DOWNWARD_MAX_MOTION_ROWS.min(max_motion_rows).max(1))
	}

	fn diagnose_reference_overlap_direction(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
	) -> DirectionMatchEval {
		let config = OverlapSearchConfig::default();
		let preferred_range = self
			.preferred_motion_range_from_hint(previous, next, motion_rows_hint, config)
			.map(OverlapSearchRange::from);

		self.diagnose_reference_overlap_direction_with_preferred_range(
			previous,
			next,
			direction,
			preferred_range,
			false,
		)
	}

	fn diagnose_reference_overlap_direction_with_preferred_range(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		direction: ScrollDirection,
		preferred_range: Option<OverlapSearchRange>,
		allow_downward_full_range_fallback: bool,
	) -> DirectionMatchEval {
		let config = OverlapSearchConfig::default();
		let max_motion_rows = max_directional_motion_rows(previous, next, config);
		let preferred_only_match = preferred_range.and_then(|range| {
			evaluate_overlap_direction(previous, next, direction, range.as_range(), config)
		});
		let mut final_match = preferred_only_match;
		let mut used_full_range_fallback = false;

		if final_match.is_none() && allow_downward_full_range_fallback {
			final_match =
				evaluate_overlap_direction(previous, next, direction, 1..=max_motion_rows, config);
			used_full_range_fallback = final_match.is_some();
		}

		DirectionMatchEval {
			preferred_range,
			max_motion_rows,
			preferred_only_match,
			final_match,
			used_full_range_fallback,
		}
	}

	fn evaluate_reference_overlap_direction_preferred_only(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
	) -> Option<DirectionMatch> {
		let config = OverlapSearchConfig::default();
		let preferred_range =
			self.preferred_motion_range_from_hint(previous, next, motion_rows_hint, config)?;

		evaluate_overlap_direction(previous, next, direction, preferred_range, config)
	}

	fn preferred_motion_range_from_hint(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let max_motion_rows = max_directional_motion_rows(previous, next, config);

		if let Some(last_growth_rows) = motion_rows_hint {
			let tolerance = DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS.min(max_motion_rows);
			let min_motion_rows = last_growth_rows.saturating_sub(tolerance).max(1);
			let max_motion_rows = last_growth_rows.saturating_add(tolerance).min(max_motion_rows);

			return Some(min_motion_rows..=max_motion_rows);
		}

		Some(1..=INITIAL_DOWNWARD_MAX_MOTION_ROWS.min(max_motion_rows).max(1))
	}

	fn preferred_downward_motion_range_from_hint(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let max_motion_rows = max_directional_motion_rows(previous, next, config);

		if let Some(last_growth_rows) = motion_rows_hint {
			let tolerance = (last_growth_rows / 2)
				.clamp(
					DOWNWARD_KEYFRAME_SEARCH_MOTION_TOLERANCE_ROWS,
					DOWNWARD_KEYFRAME_SEARCH_MAX_TOLERANCE_ROWS,
				)
				.min(max_motion_rows);
			let min_motion_rows = last_growth_rows.saturating_sub(tolerance).max(1);
			let max_motion_rows = last_growth_rows.saturating_add(tolerance).min(max_motion_rows);

			return Some(min_motion_rows..=max_motion_rows);
		}

		Some(1..=INITIAL_DOWNWARD_MAX_MOTION_ROWS.min(max_motion_rows).max(1))
	}

	fn resolve_downward_viewport_candidate(
		&mut self,
		frame: &RgbaImage,
		observed_match: DownwardSampleMatch,
	) -> DownwardViewportResolution {
		let pending_suppressed_huge_preview_only_local_followup =
			self.pending_suppressed_huge_preview_only_local_followup.take();
		let pending_suppressed_huge_preview_only_local_followup_remaining_blocks =
			self.pending_suppressed_huge_preview_only_local_followup_remaining_blocks;
		self.pending_suppressed_huge_preview_only_local_followup_remaining_blocks = 0;
		let pending_extreme_preview_only_local_tail_followup =
			self.pending_extreme_preview_only_local_tail_followup.take();
		let pending_extreme_preview_only_local_tail_followup_remaining_blocks =
			self.pending_extreme_preview_only_local_tail_followup_remaining_blocks;
		self.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 0;
		let mut candidates = Vec::with_capacity(DOWNWARD_KEYFRAME_SEARCH_LIMIT.saturating_add(1));
		let mut suppressed_observed_candidate = None;
		let mut suppressed_preview_only_local_candidate = None;

		let provisional_viewport_top_y =
			self.provisional_viewport_top_y_for_downward_sample_match(observed_match);
		self.last_downward_sample_registration_provisional_viewport_top_y =
			provisional_viewport_top_y;

		if let Some(viewport_top_y) = provisional_viewport_top_y {
			let candidate = DownwardViewportCandidate {
				source: observed_match.source.into(),
				viewport_top_y,
				motion_rows: observed_match.matched.motion_rows,
				mean_abs_diff_x100: observed_match.matched.mean_abs_diff_x100,
			};
			let suppress_observed = self.should_suppress_observed_sample_candidate(candidate);
			let suppress_preview_local =
				self.should_suppress_preview_only_local_candidate(candidate);

			if !suppress_observed && !suppress_preview_local {
				candidates.push(candidate);
			} else if suppress_observed
				&& candidate.source == DownwardViewportCandidateSource::ObservedSample
			{
				suppressed_observed_candidate = Some(candidate);
			} else if suppress_preview_local
				&& candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			{
				suppressed_preview_only_local_candidate = Some(candidate);
			}
		}
		self.collect_committed_downward_viewport_candidates(frame, &mut candidates);
		if self
			.should_fail_closed_suppressed_huge_preview_local_jump_corroborated_by_observed_and_committed(
				suppressed_preview_only_local_candidate,
				&candidates,
			) {
			self.pending_suppressed_huge_preview_only_local_followup =
				suppressed_preview_only_local_candidate;
			self.pending_suppressed_huge_preview_only_local_followup_remaining_blocks = self
				.suppressed_huge_preview_only_local_followup_block_budget(
					suppressed_preview_only_local_candidate,
				);
			candidates.clear();
		}
		if self.should_fail_closed_committed_followup_after_suppressed_huge_preview_local_jump(
			pending_suppressed_huge_preview_only_local_followup,
			&candidates,
		) {
			if let Some(pending_candidate) = pending_suppressed_huge_preview_only_local_followup {
				if pending_suppressed_huge_preview_only_local_followup_remaining_blocks > 1 {
					self.pending_suppressed_huge_preview_only_local_followup =
						Some(pending_candidate);
					self.pending_suppressed_huge_preview_only_local_followup_remaining_blocks =
						pending_suppressed_huge_preview_only_local_followup_remaining_blocks - 1;
				}
			}
			self.blocked_followup_after_suppressed_huge_preview_local_jump = true;
			candidates.clear();
		}
		if self.should_fail_closed_committed_followup_after_extreme_preview_local_tail_block(
			pending_extreme_preview_only_local_tail_followup,
			&candidates,
		) {
			if let Some(pending_candidate) = pending_extreme_preview_only_local_tail_followup
				&& pending_extreme_preview_only_local_tail_followup_remaining_blocks > 1
			{
				self.pending_extreme_preview_only_local_tail_followup = Some(pending_candidate);
				self.pending_extreme_preview_only_local_tail_followup_remaining_blocks =
					pending_extreme_preview_only_local_tail_followup_remaining_blocks - 1;
			}
			self.blocked_followup_after_extreme_preview_local_tail = true;
			candidates.clear();
		}
		self.restore_corroborated_observed_candidate(
			suppressed_observed_candidate,
			&mut candidates,
		);
		let preview_only_local_candidate_before_prune =
			candidates.iter().copied().find(|candidate| {
				candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			});
		let candidates_before_prune = candidates.clone();
		self.last_downward_viewport_candidates_before_prune =
			Some(format_downward_viewport_candidates(&candidates));
		self.prune_committed_keyframe_candidates_outside_local_continuity(&mut candidates);
		self.restore_repeated_small_preview_only_local_candidate_after_empty_prune(
			preview_only_local_candidate_before_prune,
			&mut candidates,
		);
		if self.should_fail_closed_lagging_exactly_corroborated_preview_local_tail_in_burst(
			&candidates,
		) {
			self.blocked_lagging_exactly_corroborated_preview_local_tail_in_burst = true;
			candidates.clear();
		}
		if self.should_fail_closed_underconsumed_observed_recovery_in_burst(
			&candidates_before_prune,
			&candidates,
		) {
			self.blocked_underconsumed_observed_recovery_in_burst = true;
			candidates.clear();
		}
		self.last_downward_viewport_candidate_count = Some(candidates.len());
		self.last_downward_viewport_candidates_after_prune =
			Some(format_downward_viewport_candidates(&candidates));
		select_downward_viewport_candidate(&mut candidates)
	}

	fn should_fail_closed_suppressed_huge_preview_local_jump_corroborated_by_observed_and_committed(
		&self,
		suppressed_preview_only_local_candidate: Option<DownwardViewportCandidate>,
		committed_candidates: &[DownwardViewportCandidate],
	) -> bool {
		let Some(candidate) = suppressed_preview_only_local_candidate else {
			return false;
		};
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		if candidate.source != DownwardViewportCandidateSource::PreviewOnlyLocalSample {
			return false;
		}

		let large_far_recovery_threshold = last_motion_rows_hint
			.saturating_mul(3)
			.max(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.saturating_mul(2));

		self.transient_burst_search_enabled
			&& self.last_observed_sample_registration_result == Some("matched")
			&& self.last_observed_sample_registration_motion_rows == Some(candidate.motion_rows)
			&& candidate.motion_rows > large_far_recovery_threshold
			&& self.growth_history.last().is_some_and(|commit| {
				commit.decision_source
					== DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
					&& commit.growth_rows
						>= last_motion_rows_hint
							.saturating_sub(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS)
			}) && committed_candidates.iter().any(|committed| {
			committed.source == DownwardViewportCandidateSource::CommittedKeyframe
				&& committed.motion_rows == candidate.motion_rows
				&& committed.viewport_top_y == candidate.viewport_top_y
		})
	}

	fn should_fail_closed_committed_followup_after_suppressed_huge_preview_local_jump(
		&self,
		pending_suppressed_preview_only_local_candidate: Option<DownwardViewportCandidate>,
		candidates: &[DownwardViewportCandidate],
	) -> bool {
		let Some(pending_candidate) = pending_suppressed_preview_only_local_candidate else {
			return false;
		};
		if pending_candidate.source != DownwardViewportCandidateSource::PreviewOnlyLocalSample {
			return false;
		}

		self.transient_burst_search_enabled
			&& self.last_preview_only_local_registration_result == Some("no_match")
			&& self.last_observed_sample_registration_result == Some("matched")
			&& self.last_observed_sample_registration_motion_rows
				== Some(pending_candidate.motion_rows)
			&& candidates.iter().all(|candidate| {
				candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
			}) && candidates.iter().any(|candidate| {
			candidate.viewport_top_y == pending_candidate.viewport_top_y
				&& candidate.motion_rows == pending_candidate.motion_rows
		})
	}

	fn should_fail_closed_committed_followup_after_extreme_preview_local_tail_block(
		&self,
		pending_preview_only_local_candidate: Option<DownwardViewportCandidate>,
		candidates: &[DownwardViewportCandidate],
	) -> bool {
		let Some(pending_candidate) = pending_preview_only_local_candidate else {
			return false;
		};
		if pending_candidate.source != DownwardViewportCandidateSource::PreviewOnlyLocalSample {
			return false;
		}

		self.transient_burst_search_enabled
			&& candidates.iter().all(|candidate| {
				candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
			}) && candidates.iter().any(|candidate| {
			candidate.viewport_top_y == pending_candidate.viewport_top_y
				&& candidate.motion_rows == pending_candidate.motion_rows
		})
	}

	fn suppressed_huge_preview_only_local_followup_block_budget(
		&self,
		candidate: Option<DownwardViewportCandidate>,
	) -> u8 {
		let Some(candidate) = candidate else {
			return 3;
		};
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return 3;
		};
		if candidate.source != DownwardViewportCandidateSource::PreviewOnlyLocalSample {
			return 3;
		}

		let continuity_rows = last_motion_rows_hint.max(1);
		let far_recovery_ratio =
			candidate.motion_rows.saturating_add(continuity_rows.saturating_sub(1))
				/ continuity_rows;

		u8::try_from(far_recovery_ratio.clamp(3, 5)).unwrap_or(5)
	}

	fn restore_corroborated_observed_candidate(
		&self,
		suppressed_observed_candidate: Option<DownwardViewportCandidate>,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		let Some(candidate) = suppressed_observed_candidate else {
			return;
		};
		if !self.observed_candidate_can_recover_from_committed_corroboration(candidate) {
			return;
		}
		if candidates.iter().any(|other| {
			other.source == DownwardViewportCandidateSource::CommittedKeyframe
				&& other.viewport_top_y == candidate.viewport_top_y
				&& other.motion_rows == candidate.motion_rows
		}) {
			candidates.push(candidate);
		}
	}

	fn observed_candidate_can_recover_from_committed_corroboration(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		if candidate.source != DownwardViewportCandidateSource::ObservedSample {
			return false;
		}

		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let corroboration_cap =
			last_motion_rows_hint.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);

		self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y) <= corroboration_cap
	}

	fn restore_repeated_small_preview_only_local_candidate_after_empty_prune(
		&mut self,
		preview_only_local_candidate_before_prune: Option<DownwardViewportCandidate>,
		candidates_after_prune: &mut Vec<DownwardViewportCandidate>,
	) {
		let Some(candidate) = preview_only_local_candidate_before_prune else {
			self.last_blocked_preview_only_local_candidate = None;
			return;
		};
		if candidate.source != DownwardViewportCandidateSource::PreviewOnlyLocalSample
			|| !candidates_after_prune.is_empty()
			|| !self.repeated_preview_only_local_candidate_can_restore_after_empty_prune(candidate)
		{
			self.last_blocked_preview_only_local_candidate = None;
			return;
		}

		let repeats = match self.last_blocked_preview_only_local_candidate {
			Some(previous) if previous.candidate == candidate => previous.repeats.saturating_add(1),
			_ => 1,
		};
		self.last_blocked_preview_only_local_candidate =
			Some(BlockedPreviewOnlyLocalCandidate { candidate, repeats });

		if repeats >= 2 {
			candidates_after_prune.push(candidate);
			self.last_blocked_preview_only_local_candidate = None;
		}
	}

	fn repeated_preview_only_local_candidate_can_restore_after_empty_prune(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.motion_rows <= REPEATED_PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& self.transient_burst_search_enabled
			&& self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
			&& self.last_motion_rows_hint.is_some()
	}

	fn should_fail_closed_lagging_exactly_corroborated_preview_local_tail_in_burst(
		&self,
		candidates_after_prune: &[DownwardViewportCandidate],
	) -> bool {
		if !self.transient_burst_search_enabled {
			return false;
		}
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let Some(preview_only_local_candidate) =
			candidates_after_prune.iter().copied().find(|candidate| {
				candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			})
		else {
			return false;
		};

		preview_only_local_candidate.motion_rows
			<= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.saturating_div(2)
			&& preview_only_local_candidate.motion_rows
				< last_motion_rows_hint
					.saturating_sub(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
			&& candidates_after_prune.iter().any(|candidate| {
				candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
					&& candidate.viewport_top_y == preview_only_local_candidate.viewport_top_y
					&& candidate.motion_rows == preview_only_local_candidate.motion_rows
					&& candidate.mean_abs_diff_x100
						<= preview_only_local_candidate
							.mean_abs_diff_x100
							.saturating_add(DIRECTION_WARNING_MARGIN_X100)
			})
	}

	fn should_fail_closed_underconsumed_observed_recovery_in_burst(
		&self,
		candidates_before_prune: &[DownwardViewportCandidate],
		candidates_after_prune: &[DownwardViewportCandidate],
	) -> bool {
		let Some(observed_candidate) = candidates_after_prune
			.iter()
			.copied()
			.find(|candidate| candidate.source == DownwardViewportCandidateSource::ObservedSample)
		else {
			return false;
		};

		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};

		if self.last_preview_only_downward_local_sample.is_some()
			|| !self
				.transient_burst_motion_hint_exceeds_local_authority(observed_candidate.motion_rows)
			|| last_motion_rows_hint
				< observed_candidate
					.motion_rows
					.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
		{
			return false;
		}

		let has_same_motion_committed_corroboration =
			candidates_after_prune.iter().any(|candidate| {
				candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
					&& candidate.viewport_top_y == observed_candidate.viewport_top_y
					&& candidate.motion_rows == observed_candidate.motion_rows
			});
		if !has_same_motion_committed_corroboration {
			return false;
		}

		candidates_before_prune.iter().any(|candidate| {
			candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
				&& candidate.motion_rows > observed_candidate.motion_rows
				&& candidate.motion_rows >= last_motion_rows_hint
				&& candidate.viewport_top_y >= observed_candidate.viewport_top_y
				&& candidate.viewport_top_y.abs_diff(observed_candidate.viewport_top_y)
					<= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
				&& candidate.mean_abs_diff_x100
					<= observed_candidate
						.mean_abs_diff_x100
						.saturating_add(DIRECTION_WARNING_MARGIN_X100)
		})
	}

	fn prune_committed_keyframe_candidates_outside_local_continuity(
		&mut self,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		let has_committed_candidate = candidates.iter().any(|candidate| {
			candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
		});
		let mut local_anchor = best_local_downward_viewport_candidate(candidates);
		if local_anchor.is_some_and(|anchor| {
			has_committed_candidate
				&& anchor.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
				&& self.transient_burst_motion_hint_exceeds_local_authority(anchor.motion_rows)
				&& !self
					.preview_only_local_anchor_has_exact_committed_corroboration(anchor, candidates)
				&& !self.preview_only_local_candidate_has_material_progress(anchor)
				&& ((anchor.motion_rows <= TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS
					&& self.consecutive_transient_burst_missing_downward_candidate_frames < 2)
					|| candidates.iter().any(|candidate| {
						self.committed_candidate_can_plausibly_replace_underconsumed_preview_local_anchor(
							anchor,
							*candidate,
						)
					}))
		}) {
			candidates.retain(|candidate| {
				candidate.source != DownwardViewportCandidateSource::PreviewOnlyLocalSample
			});
			local_anchor = best_local_downward_viewport_candidate(candidates);
		}

		let Some(local_anchor) = local_anchor else {
			candidates.retain(|candidate| {
				candidate.source != DownwardViewportCandidateSource::CommittedKeyframe
					|| !self.transient_burst_search_enabled
					|| !self.fallback_downward_growth_exceeds_continuity_budget(
						candidate.viewport_top_y,
					) || self.transient_burst_growth_matches_pending_hint_band(candidate.viewport_top_y)
					|| self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y)
						<= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			});

			if let Some(max_bootstrap_growth_rows) =
				self.bootstrap_committed_keyframe_growth_cap_rows()
			{
				candidates.retain(|candidate| {
					candidate.source != DownwardViewportCandidateSource::CommittedKeyframe
						|| self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y)
							<= max_bootstrap_growth_rows
				});
			}
			self.prune_committed_keyframe_candidates_without_local_anchor(candidates);
			return;
		};
		let allowed_overrun_rows = self
			.max_committed_keyframe_local_overrun_rows(local_anchor)
			.max(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);
		let max_allowed_motion_rows = self.max_committed_keyframe_motion_rows(local_anchor);
		let max_allowed_viewport_top_y = local_anchor
			.viewport_top_y
			.saturating_add(i32::try_from(allowed_overrun_rows).unwrap_or(i32::MAX));
		let local_observed_has_same_motion_committed_corroboration = local_anchor.source
			== DownwardViewportCandidateSource::ObservedSample
			&& candidates.iter().any(|candidate| {
				candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
					&& candidate.viewport_top_y == local_anchor.viewport_top_y
					&& candidate.motion_rows == local_anchor.motion_rows
			});

		candidates.retain(|candidate| {
			candidate.source != DownwardViewportCandidateSource::CommittedKeyframe
				|| (candidate.viewport_top_y <= max_allowed_viewport_top_y
					&& candidate.motion_rows <= max_allowed_motion_rows)
				|| (!local_observed_has_same_motion_committed_corroboration
					&& self.committed_candidate_can_override_untrustworthy_observed_local_recovery(
						local_anchor,
						*candidate,
					))
		});
		self.prune_committed_keyframe_candidates_for_transient_burst(candidates);
	}

	fn preview_only_local_anchor_has_exact_committed_corroboration(
		&self,
		local_anchor: DownwardViewportCandidate,
		candidates: &[DownwardViewportCandidate],
	) -> bool {
		local_anchor.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			&& candidates.iter().any(|candidate| {
				candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
					&& candidate.viewport_top_y == local_anchor.viewport_top_y
					&& candidate.motion_rows == local_anchor.motion_rows
					&& candidate.mean_abs_diff_x100
						<= local_anchor
							.mean_abs_diff_x100
							.saturating_add(DIRECTION_WARNING_MARGIN_X100)
			})
	}

	fn prune_committed_keyframe_candidates_without_local_anchor(
		&mut self,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		if !candidates
			.iter()
			.all(|candidate| candidate.source == DownwardViewportCandidateSource::CommittedKeyframe)
		{
			return;
		}

		let Some(preferred) = candidates.iter().copied().min_by(|left, right| {
			left.motion_rows
				.cmp(&right.motion_rows)
				.then(left.mean_abs_diff_x100.cmp(&right.mean_abs_diff_x100))
				.then(left.viewport_top_y.cmp(&right.viewport_top_y))
		}) else {
			return;
		};
		if self.should_fail_closed_far_committed_only_recovery_without_local_anchor(
			preferred, candidates,
		) {
			if self
				.should_fail_closed_far_committed_only_recovery_after_corroborated_huge_local_jump(
					preferred,
					self.growth_rows_for_candidate_viewport_top_y(preferred.viewport_top_y),
				) {
				self.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump = true;
			}
			candidates.clear();
			return;
		}

		candidates.retain(|candidate| *candidate == preferred);
	}

	fn should_fail_closed_far_committed_only_recovery_without_local_anchor(
		&self,
		preferred: DownwardViewportCandidate,
		candidates: &[DownwardViewportCandidate],
	) -> bool {
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		if !self.transient_burst_search_enabled {
			return false;
		}
		let preferred_growth_rows =
			self.growth_rows_for_candidate_viewport_top_y(preferred.viewport_top_y);
		if self
			.should_fail_closed_underconsumed_committed_only_recovery_after_suppressed_preview_local_match(
				preferred,
				preferred_growth_rows,
			) {
			return true;
		}
		if self
			.should_fail_closed_committed_only_recovery_after_corroborated_sample_registration_without_viewport_anchor(
				preferred,
				preferred_growth_rows,
			)
		{
			return true;
		}
		if self
			.should_fail_closed_committed_only_recovery_when_observed_burst_outpaces_recent_preview_local_commit(
				preferred,
				preferred_growth_rows,
			)
		{
			return true;
		}
		if self.should_fail_closed_far_committed_only_recovery_after_corroborated_huge_local_jump(
			preferred,
			preferred_growth_rows,
		) {
			return true;
		}
		if self.last_preview_only_downward_local_sample.is_some()
			&& self.last_preview_only_local_registration_result == Some("matched")
			&& last_motion_rows_hint <= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
			&& self.last_preview_only_local_registration_motion_rows.is_some_and(
				|local_motion_rows| {
					local_motion_rows
						<= last_motion_rows_hint
							.saturating_add(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS)
						&& preferred_growth_rows
							> local_motion_rows.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
				},
			) {
			return true;
		}
		if last_motion_rows_hint > DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS.saturating_mul(2) {
			let all_candidates_low_confidence = candidates.iter().all(|candidate| {
				candidate.mean_abs_diff_x100 > DIRECTION_WARNING_MARGIN_X100.saturating_mul(4)
			});

			return preferred_growth_rows <= last_motion_rows_hint && all_candidates_low_confidence;
		}

		let far_growth_threshold = PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			.max(last_motion_rows_hint.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS));

		self.growth_rows_for_candidate_viewport_top_y(preferred.viewport_top_y)
			> far_growth_threshold
			&& candidates.iter().all(|candidate| {
				self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y)
					> far_growth_threshold
			})
	}

	fn should_fail_closed_far_committed_only_recovery_after_corroborated_huge_local_jump(
		&self,
		preferred: DownwardViewportCandidate,
		preferred_growth_rows: u32,
	) -> bool {
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		if preferred.source != DownwardViewportCandidateSource::CommittedKeyframe {
			return false;
		}

		let large_far_recovery_threshold = last_motion_rows_hint
			.saturating_mul(3)
			.max(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.saturating_mul(2));
		let observed_material_lag_threshold = PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			.max(last_motion_rows_hint.saturating_mul(2));
		let observed_corroborates_or_materially_lags =
			self.last_observed_sample_registration_result == Some("matched")
				&& self.last_observed_sample_registration_motion_rows.is_some_and(
					|observed_motion_rows| {
						observed_motion_rows == preferred.motion_rows
							|| observed_motion_rows.saturating_add(observed_material_lag_threshold)
								< preferred.motion_rows
					},
				);

		self.transient_burst_search_enabled
			&& self.last_preview_only_local_registration_result == Some("matched")
			&& self.last_preview_only_local_registration_motion_rows == Some(preferred.motion_rows)
			&& observed_corroborates_or_materially_lags
			&& preferred.motion_rows > large_far_recovery_threshold
			&& preferred_growth_rows > large_far_recovery_threshold
			&& self.growth_history.last().is_some_and(|commit| {
				commit.decision_source
					== DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
					&& commit.growth_rows
						>= last_motion_rows_hint
							.saturating_sub(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS)
			})
	}

	fn should_fail_closed_underconsumed_committed_only_recovery_after_suppressed_preview_local_match(
		&self,
		preferred: DownwardViewportCandidate,
		preferred_growth_rows: u32,
	) -> bool {
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let Some(local_motion_rows) = self.last_preview_only_local_registration_motion_rows else {
			return false;
		};

		self.last_preview_only_downward_local_sample.is_some()
			&& self.last_preview_only_local_registration_result == Some("matched")
			&& self.transient_burst_motion_hint_exceeds_local_authority(preferred.motion_rows)
			&& !self.transient_burst_growth_matches_pending_hint_band(preferred.viewport_top_y)
			&& local_motion_rows > PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& local_motion_rows
				> preferred
					.motion_rows
					.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
			&& preferred_growth_rows
				<= last_motion_rows_hint
					.saturating_mul(2)
					.max(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS)
	}

	fn should_fail_closed_committed_only_recovery_after_corroborated_sample_registration_without_viewport_anchor(
		&self,
		preferred: DownwardViewportCandidate,
		preferred_growth_rows: u32,
	) -> bool {
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let Some(observed_motion_rows) = self.last_observed_sample_registration_motion_rows else {
			return false;
		};
		let Some(local_motion_rows) = self.last_preview_only_local_registration_motion_rows else {
			return false;
		};

		let corroborated_motion_floor =
			last_motion_rows_hint.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);
		let corroborated_motion_ceiling = observed_motion_rows
			.max(local_motion_rows)
			.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);

		preferred.source == DownwardViewportCandidateSource::CommittedKeyframe
			&& self.transient_burst_search_enabled
			&& self.last_preview_only_downward_local_sample.is_some()
			&& self.last_observed_sample_registration_result == Some("matched")
			&& self.last_preview_only_local_registration_result == Some("matched")
			&& last_motion_rows_hint <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& observed_motion_rows > corroborated_motion_floor
			&& local_motion_rows > corroborated_motion_floor
			&& preferred_growth_rows > corroborated_motion_floor
			&& preferred.motion_rows >= local_motion_rows
			&& preferred_growth_rows <= corroborated_motion_ceiling
	}

	fn should_fail_closed_committed_only_recovery_when_observed_burst_outpaces_recent_preview_local_commit(
		&self,
		preferred: DownwardViewportCandidate,
		preferred_growth_rows: u32,
	) -> bool {
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let Some(observed_motion_rows) = self.last_observed_sample_registration_motion_rows else {
			return false;
		};

		let recent_preview_local_commit = self.growth_history.last().is_some_and(|commit| {
			commit.decision_source
				== DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
				&& commit.growth_rows
					>= last_motion_rows_hint.saturating_sub(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS)
		});
		let corroborated_motion_floor =
			last_motion_rows_hint.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);

		preferred.source == DownwardViewportCandidateSource::CommittedKeyframe
			&& self.transient_burst_search_enabled
			&& recent_preview_local_commit
			&& self.last_observed_sample_registration_result == Some("matched")
			&& self.last_preview_only_local_registration_result == Some("no_match")
			&& last_motion_rows_hint <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& observed_motion_rows > corroborated_motion_floor
			&& preferred_growth_rows > corroborated_motion_floor
			&& preferred.motion_rows.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
				< observed_motion_rows
	}

	fn should_suppress_preview_only_local_candidate(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			&& self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
			&& candidate.motion_rows > PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& !self.preview_only_local_candidate_remains_trustworthy_in_burst(candidate)
	}

	fn should_suppress_observed_sample_candidate(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.source == DownwardViewportCandidateSource::ObservedSample
			&& self.transient_burst_search_enabled
			&& self.fallback_downward_growth_exceeds_continuity_budget(candidate.viewport_top_y)
			&& !self.observed_sample_candidate_remains_trustworthy_in_burst(candidate)
	}

	fn observed_sample_candidate_remains_trustworthy_in_burst(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		if candidate.source != DownwardViewportCandidateSource::ObservedSample {
			return false;
		}

		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y);
		self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				candidate.motion_rows.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
					>= last_hint && candidate.motion_rows <= last_hint
			}) && candidate.mean_abs_diff_x100 <= DIRECTION_WARNING_MARGIN_X100.saturating_mul(6)
			&& self.transient_pending_growth_cap_rows().is_some_and(|cap| growth_rows <= cap)
	}

	fn preview_only_local_candidate_has_material_progress(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		if self.seeded_preview_only_local_catch_up_candidate_can_commit(candidate) {
			return true;
		}

		candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample && {
			let growth_rows =
				self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y);
			growth_rows >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
				|| self.last_motion_rows_hint.is_some_and(|last_hint| {
					last_hint >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
						&& growth_rows.saturating_add(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS)
							>= last_hint
				}) || self.last_motion_rows_hint.is_some_and(|last_hint| {
				last_hint >= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
					&& candidate.motion_rows.abs_diff(last_hint)
						<= PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS
			})
		}
	}

	fn preview_only_local_candidate_remains_trustworthy_in_burst(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		if candidate.source != DownwardViewportCandidateSource::PreviewOnlyLocalSample {
			return true;
		}
		if candidate.motion_rows <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS {
			return true;
		}

		self.transient_burst_growth_matches_pending_hint_band(candidate.viewport_top_y)
			&& self.last_motion_rows_hint.is_some_and(|last_hint| {
				candidate.motion_rows
					<= last_hint.saturating_add(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS)
			})
	}

	fn seeded_preview_only_local_catch_up_candidate_can_commit(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			&& self.seeded_preview_only_local_after_observed_burst_commit
			&& candidate.viewport_top_y > self.current_viewport_top_y
			&& candidate.motion_rows <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
	}

	fn prune_committed_keyframe_candidates_for_transient_burst(
		&mut self,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		if !self.transient_burst_search_enabled {
			return;
		}

		let Some(local_candidate) = candidates
			.iter()
			.copied()
			.filter(|candidate| candidate.source == DownwardViewportCandidateSource::ObservedSample)
			.min_by(|left, right| {
				left.mean_abs_diff_x100
					.cmp(&right.mean_abs_diff_x100)
					.then(left.motion_rows.cmp(&right.motion_rows))
			})
		else {
			return;
		};

		let Some(previous_growth_rows) = self.last_motion_rows_hint else {
			return;
		};

		if local_candidate.motion_rows <= previous_growth_rows {
			return;
		}

		candidates.retain(|candidate| {
			candidate.source != DownwardViewportCandidateSource::CommittedKeyframe
				|| candidate.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< local_candidate.mean_abs_diff_x100
		});
	}

	fn max_committed_keyframe_local_overrun_rows(
		&self,
		local_anchor: DownwardViewportCandidate,
	) -> u32 {
		self.max_committed_keyframe_motion_rows(local_anchor).clamp(
			DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS,
			DOWNWARD_COMMITTED_KEYFRAME_LOCAL_OVERRUN_MAX_ROWS,
		)
	}

	fn max_committed_keyframe_motion_rows(&self, local_anchor: DownwardViewportCandidate) -> u32 {
		let continuity_rows = self
			.last_motion_rows_hint
			.unwrap_or(local_anchor.motion_rows)
			.max(local_anchor.motion_rows);
		let tolerance_rows = (continuity_rows / 2).clamp(1, DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);

		continuity_rows.saturating_add(tolerance_rows)
	}

	fn committed_candidate_can_plausibly_replace_underconsumed_preview_local_anchor(
		&self,
		local_anchor: DownwardViewportCandidate,
		committed_candidate: DownwardViewportCandidate,
	) -> bool {
		if committed_candidate.source != DownwardViewportCandidateSource::CommittedKeyframe {
			return false;
		}

		let allowed_overrun_rows = self
			.max_committed_keyframe_local_overrun_rows(local_anchor)
			.max(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);
		let max_allowed_motion_rows = self.max_committed_keyframe_motion_rows(local_anchor);
		let max_allowed_viewport_top_y = local_anchor
			.viewport_top_y
			.saturating_add(i32::try_from(allowed_overrun_rows).unwrap_or(i32::MAX));
		let local_anchor_tracks_recent_continuity = self
			.last_motion_rows_hint
			.is_some_and(|last_hint| local_anchor.motion_rows >= last_hint);
		let committed_is_not_materially_worse_than_local_anchor = committed_candidate
			.mean_abs_diff_x100
			<= local_anchor.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100);

		(committed_candidate.viewport_top_y <= max_allowed_viewport_top_y
			&& committed_candidate.motion_rows <= max_allowed_motion_rows)
			&& (!local_anchor_tracks_recent_continuity
				|| committed_is_not_materially_worse_than_local_anchor)
			|| self.transient_burst_growth_matches_pending_hint_band(
				committed_candidate.viewport_top_y,
			) || self.committed_candidate_can_override_untrustworthy_observed_local_recovery(
			local_anchor,
			committed_candidate,
		)
	}

	fn committed_candidate_can_override_untrustworthy_observed_local_recovery(
		&self,
		local_anchor: DownwardViewportCandidate,
		committed_candidate: DownwardViewportCandidate,
	) -> bool {
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};
		let Some(transient_growth_cap_rows) = self.transient_pending_growth_cap_rows() else {
			return false;
		};
		if committed_candidate.source != DownwardViewportCandidateSource::CommittedKeyframe {
			return false;
		}
		let local_growth_rows =
			self.growth_rows_for_candidate_viewport_top_y(local_anchor.viewport_top_y);
		let committed_growth_rows =
			self.growth_rows_for_candidate_viewport_top_y(committed_candidate.viewport_top_y);

		local_anchor.source == DownwardViewportCandidateSource::ObservedSample
			&& self.transient_burst_motion_hint_exceeds_local_authority(local_anchor.motion_rows)
			&& local_anchor.mean_abs_diff_x100 > DIRECTION_WARNING_MARGIN_X100.saturating_mul(4)
			&& local_anchor.motion_rows
				<= last_motion_rows_hint.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS)
			&& (committed_growth_rows
				<= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS.saturating_mul(2)
				|| self.transient_burst_growth_matches_pending_hint_band(
					committed_candidate.viewport_top_y,
				)) && committed_candidate.mean_abs_diff_x100
			<= DIRECTION_WARNING_MARGIN_X100.saturating_mul(2)
			&& committed_candidate
				.mean_abs_diff_x100
				.saturating_add(DIRECTION_WARNING_MARGIN_X100.saturating_mul(3))
				< local_anchor.mean_abs_diff_x100
			&& committed_candidate.motion_rows
				> local_anchor
					.motion_rows
					.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
			&& committed_growth_rows
				> local_growth_rows.saturating_add(UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS)
			&& committed_growth_rows <= transient_growth_cap_rows
	}

	fn bootstrap_committed_keyframe_growth_cap_rows(&self) -> Option<u32> {
		if !self.initial_downward_bootstrap_active() {
			return None;
		}

		self.transient_pending_growth_cap_rows()
	}

	fn transient_pending_growth_cap_rows(&self) -> Option<u32> {
		let hint = self.normalized_transient_motion_rows_hint()?;
		let tolerance = (hint / 2).clamp(1, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS);

		Some(hint.saturating_add(tolerance))
	}

	fn transient_burst_growth_matches_pending_hint_band(
		&self,
		candidate_viewport_top_y: i32,
	) -> bool {
		if !self.transient_burst_search_enabled {
			return false;
		}

		let Some(transient_hint) = self.normalized_transient_motion_rows_hint() else {
			return false;
		};
		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate_viewport_top_y);
		let min_growth_rows =
			(transient_hint / 2).max(self.last_motion_rows_hint.unwrap_or_default());

		self.transient_pending_growth_cap_rows()
			.is_some_and(|cap| growth_rows >= min_growth_rows && growth_rows <= cap)
	}

	fn collect_committed_downward_viewport_candidates(
		&self,
		frame: &RgbaImage,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		self.collect_committed_downward_viewport_candidates_with_mode(
			frame,
			candidates,
			CommittedDownwardViewportCandidateMode::IncludeRecentHistory,
		);
	}

	fn collect_fallback_downward_viewport_candidates(
		&self,
		frame: &RgbaImage,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		self.collect_committed_downward_viewport_candidates_with_mode(
			frame,
			candidates,
			CommittedDownwardViewportCandidateMode::LastCommittedOnly,
		);
	}

	fn collect_committed_downward_viewport_candidates_with_mode(
		&self,
		frame: &RgbaImage,
		candidates: &mut Vec<DownwardViewportCandidate>,
		mode: CommittedDownwardViewportCandidateMode,
	) {
		self.push_downward_viewport_candidate(
			&self.last_committed_frame,
			self.current_viewport_top_y,
			frame,
			DownwardViewportCandidateSource::CommittedKeyframe,
			candidates,
		);

		if mode == CommittedDownwardViewportCandidateMode::LastCommittedOnly
			|| DOWNWARD_KEYFRAME_SEARCH_LIMIT <= 1
		{
			return;
		}

		for commit in self
			.growth_history
			.iter()
			.rev()
			.skip(1)
			.take(DOWNWARD_KEYFRAME_SEARCH_LIMIT.saturating_sub(1))
		{
			self.push_downward_viewport_candidate(
				&commit.frame,
				commit.viewport_top_y,
				frame,
				DownwardViewportCandidateSource::CommittedKeyframe,
				candidates,
			);
		}
	}

	fn push_downward_viewport_candidate(
		&self,
		reference: &RgbaImage,
		reference_viewport_top_y: i32,
		frame: &RgbaImage,
		source: DownwardViewportCandidateSource,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		let predicted_motion_rows = self.downward_keyframe_motion_hint(reference_viewport_top_y);
		let allow_full_range_fallback =
			!(self.initial_downward_bootstrap_active() && predicted_motion_rows.is_none());
		let mut registration = self.evaluate_reference_downward_registration(
			reference,
			frame,
			predicted_motion_rows,
			allow_full_range_fallback,
		);

		if source == DownwardViewportCandidateSource::CommittedKeyframe
			&& self.should_retry_committed_keyframe_registration_across_full_range(registration)
		{
			let full_range_registration = self
				.evaluate_reference_downward_registration_with_preferred_range(
					reference,
					frame,
					predicted_motion_rows,
					None,
					true,
				);
			registration = self.prefer_full_range_committed_keyframe_registration(
				registration,
				full_range_registration,
			);
		}

		if let DownwardRegistration::Matched(matched) = registration {
			if self.bootstrap_motion_exceeds_pending_hint(matched.motion_rows) {
				return;
			}

			let max_overlap = reference.height().min(frame.height());
			let min_keyframe_overlap_rows = OverlapSearchConfig::default()
				.min_overlap_rows
				.max(max_overlap / DOWNWARD_KEYFRAME_MIN_OVERLAP_DIVISOR)
				.max(1);
			let overlap_rows = max_overlap.saturating_sub(matched.motion_rows);

			if overlap_rows < min_keyframe_overlap_rows {
				return;
			}

			let viewport_top_y = reference_viewport_top_y
				.saturating_add(i32::try_from(matched.motion_rows).unwrap_or_default());

			if viewport_top_y <= self.current_viewport_top_y {
				return;
			}

			candidates.push(DownwardViewportCandidate {
				source,
				viewport_top_y,
				motion_rows: matched.motion_rows,
				mean_abs_diff_x100: matched.mean_abs_diff_x100,
			});
		}
	}

	fn should_retry_committed_keyframe_registration_across_full_range(
		&self,
		registration: DownwardRegistration,
	) -> bool {
		let DownwardRegistration::Matched(matched) = registration else {
			return false;
		};
		let Some(last_motion_rows_hint) = self.last_motion_rows_hint else {
			return false;
		};

		let low_confidence_match =
			matched.mean_abs_diff_x100 > DIRECTION_WARNING_MARGIN_X100.saturating_mul(4);
		let tiny_underconsumed_match = self
			.transient_burst_motion_hint_exceeds_local_authority(matched.motion_rows)
			&& matched.mean_abs_diff_x100 > DIRECTION_WARNING_MARGIN_X100.saturating_mul(4)
			&& matched.motion_rows
				<= last_motion_rows_hint.saturating_add(DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);
		let large_overshot_match = matched.motion_rows > last_motion_rows_hint.saturating_mul(2);

		low_confidence_match && (tiny_underconsumed_match || large_overshot_match)
	}

	fn prefer_full_range_committed_keyframe_registration(
		&self,
		preferred_range_registration: DownwardRegistration,
		full_range_registration: DownwardRegistration,
	) -> DownwardRegistration {
		match (preferred_range_registration, full_range_registration) {
			(DownwardRegistration::Matched(preferred), DownwardRegistration::Matched(full))
				if full.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< preferred.mean_abs_diff_x100
					&& preferred.motion_rows.abs_diff(full.motion_rows)
						> UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS =>
			{
				DownwardRegistration::Matched(full)
			},
			(preferred, _) => preferred,
		}
	}

	fn downward_keyframe_motion_hint(&self, reference_viewport_top_y: i32) -> Option<u32> {
		let last_motion_rows = self.last_motion_rows_hint?;
		let already_traversed_rows = u32::try_from(
			self.current_viewport_top_y.saturating_sub(reference_viewport_top_y).max(0),
		)
		.unwrap_or_default();

		Some(already_traversed_rows.saturating_add(last_motion_rows))
	}

	fn fallback_downward_growth_blocked_while_resume_frontier_active(
		&mut self,
		candidate_viewport_top_y: i32,
		motion_rows: u32,
		preview_changed: bool,
		decision_source: &'static str,
	) -> Option<ScrollObserveOutcome> {
		let resume_frontier_top_y = self.resume_frontier_top_y?;
		let growth_rows = if candidate_viewport_top_y <= resume_frontier_top_y {
			0
		} else {
			u32::try_from(candidate_viewport_top_y - resume_frontier_top_y).unwrap_or_default()
		};

		self.log_decision(
			"scroll_capture.fallback_downward_blocked_while_resume_frontier_active",
			ScrollDirection::Down,
			Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
			Some(candidate_viewport_top_y),
			Some(growth_rows),
			Some(decision_source),
		);

		Some(preview_update_outcome(preview_changed))
	}

	fn fallback_downward_growth_exceeds_continuity_budget(
		&self,
		candidate_viewport_top_y: i32,
	) -> bool {
		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate_viewport_top_y);
		let Some(base_continuity_rows) = self.last_motion_rows_hint else {
			return false;
		};
		let local_overrun_rows = base_continuity_rows
			.saturating_mul(2)
			.clamp(FALLBACK_DOWNWARD_GROWTH_MIN_ROWS, FALLBACK_DOWNWARD_GROWTH_MAX_ROWS);
		let preview_local_rows = self
			.last_preview_only_downward_local_sample
			.as_ref()
			.map(|sample| {
				u32::try_from(
					sample.viewport_top_y.saturating_sub(self.current_viewport_top_y).max(0),
				)
				.unwrap_or_default()
			})
			.unwrap_or_default();
		let max_growth_rows = preview_local_rows.saturating_add(local_overrun_rows);

		growth_rows > max_growth_rows
	}

	fn observe_fallback_downward_growth(
		&mut self,
		frame: RgbaImage,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		let mut candidates = Vec::with_capacity(DOWNWARD_KEYFRAME_SEARCH_LIMIT);

		self.collect_fallback_downward_viewport_candidates(&frame, &mut candidates);

		match select_downward_viewport_candidate(&mut candidates) {
			DownwardViewportResolution::NoMatch => {
				self.refresh_preview_only_downward_local_sample(
					&frame,
					self.stable_preview_only_downward_local_viewport_top_y(),
				);
				self.log_decision(
					"scroll_capture.fallback_downward_no_match",
					ScrollDirection::Down,
					None,
					None,
					Some(0),
					Some("no_committed_keyframe_match"),
				);

				Ok(preview_update_outcome(preview_changed))
			},
			DownwardViewportResolution::Selected(candidate) => {
				if self.fallback_downward_growth_exceeds_continuity_budget(candidate.viewport_top_y)
				{
					self.refresh_preview_only_downward_local_sample(
						&frame,
						self.stable_preview_only_downward_local_viewport_top_y(),
					);
					self.log_decision(
						"scroll_capture.fallback_downward_growth_blocked",
						ScrollDirection::Down,
						Some(MotionObservation {
							direction: ScrollDirection::Down,
							motion_rows: candidate.motion_rows,
						}),
						Some(candidate.viewport_top_y),
						Some(
							self.growth_rows_for_candidate_viewport_top_y(candidate.viewport_top_y),
						),
						Some("fallback_committed_candidate_exceeded_local_continuity_budget"),
					);

					return Ok(preview_update_outcome(preview_changed));
				}

				if let Some(outcome) = self
					.fallback_downward_growth_blocked_while_resume_frontier_active(
						candidate.viewport_top_y,
						candidate.motion_rows,
						preview_changed,
						"resume_frontier_active_blocks_keyframe_fallback_downward_match",
					) {
					return Ok(outcome);
				}

				self.observe_downward_growth_to_viewport(
					frame,
					candidate.viewport_top_y,
					preview_changed,
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: candidate.motion_rows,
					}),
					candidate.source.fallback_decision_source(),
				)
			},
			DownwardViewportResolution::Ambiguous { preferred, competing } => {
				self.refresh_preview_only_downward_local_sample(
					&frame,
					self.stable_preview_only_downward_local_viewport_top_y(),
				);
				self.log_decision(
					"scroll_capture.fallback_ambiguous_downward_registration",
					ScrollDirection::Down,
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: preferred.motion_rows,
					}),
					Some(preferred.viewport_top_y),
					Some(0),
					Some(preferred.competing_block_reason(competing)),
				);

				Ok(preview_update_outcome(preview_changed))
			},
		}
	}

	fn apply_growth(
		&mut self,
		frame: RgbaImage,
		growth_rows: u32,
		viewport_top_y: i32,
		decision_source: &'static str,
		detected_motion_rows: Option<u32>,
		effective_motion_rows_hint: Option<u32>,
		previous_motion_rows_hint: Option<u32>,
	) -> Result<ScrollObserveOutcome> {
		let fingerprint = scroll_capture_fingerprint(&frame);
		let strip = crop_bottom_rows(&frame, growth_rows)
			.ok_or_else(|| eyre::eyre!("failed to extract growth strip"))?;
		let preview_strip = resize_strip_to_preview_width(&strip, self.preview_width_px);

		self.export_image = append_vertical_image(&self.export_image, &strip)?;
		self.preview_image = append_vertical_image(&self.preview_image, &preview_strip)?;

		self.bottom_segments.push(strip);
		self.bottom_preview_segments.push(preview_strip);

		self.current_viewport_top_y = viewport_top_y;
		self.observed_viewport_top_y = viewport_top_y;
		self.record_last_sample(&frame, fingerprint);
		self.record_last_downward_observed_sample(&frame, scroll_capture_fingerprint(&frame));
		if self.should_seed_preview_only_local_after_observed_burst_commit(
			decision_source,
			growth_rows,
			previous_motion_rows_hint,
		) {
			self.record_preview_only_downward_local_sample(&frame, viewport_top_y);
			self.seeded_preview_only_local_after_observed_burst_commit = true;
		} else if self.should_preserve_preview_only_local_after_preview_only_burst_commit(
			decision_source,
			growth_rows,
			previous_motion_rows_hint,
		) {
			self.record_preview_only_downward_local_sample(&frame, viewport_top_y);
			self.seeded_preview_only_local_after_observed_burst_commit = false;
			self.last_blocked_preview_only_local_candidate = None;
		} else {
			self.clear_preview_only_downward_local_sample();
		}
		self.last_unconfirmed_upward_fingerprint = None;
		self.last_committed_frame = frame.clone();
		self.resume_frontier_top_y = None;
		self.resume_frontier_requires_reacquire = false;

		self.growth_history.push(GrowthCommit {
			frame,
			growth_rows,
			viewport_top_y,
			decision_source,
			detected_motion_rows,
			effective_motion_rows_hint,
		});

		Ok(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows })
	}

	fn should_seed_preview_only_local_after_observed_burst_commit(
		&self,
		decision_source: &'static str,
		growth_rows: u32,
		previous_motion_rows_hint: Option<u32>,
	) -> bool {
		decision_source == DownwardViewportCandidateSource::ObservedSample.decision_source()
			&& self.transient_burst_search_enabled
			&& previous_motion_rows_hint.is_some_and(|previous| {
				previous >= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS && growth_rows < previous
			})
	}

	fn should_preserve_preview_only_local_after_preview_only_burst_commit(
		&self,
		decision_source: &'static str,
		growth_rows: u32,
		previous_motion_rows_hint: Option<u32>,
	) -> bool {
		decision_source == DownwardViewportCandidateSource::PreviewOnlyLocalSample.decision_source()
			&& previous_motion_rows_hint.is_some_and(|previous| {
				if self.transient_burst_search_enabled {
					growth_rows >= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
						&& growth_rows
							>= previous.saturating_sub(PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS)
						&& growth_rows
							<= previous
								.saturating_add(PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS)
				} else {
					previous <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
						&& growth_rows > 1 && growth_rows <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
						&& growth_rows <= previous
				}
			})
	}

	fn rebuild_export_image(&self) -> Result<RgbaImage> {
		let mut ordered = Vec::with_capacity(self.bottom_segments.len().saturating_add(1));

		ordered.push(&self.anchor_frame);

		for strip in &self.bottom_segments {
			ordered.push(strip);
		}

		stack_vertical_images(&ordered)
	}

	fn rebuild_preview_image(&self) -> Result<RgbaImage> {
		let mut ordered = Vec::with_capacity(self.bottom_preview_segments.len().saturating_add(1));

		ordered.push(&self.anchor_preview);

		for strip in &self.bottom_preview_segments {
			ordered.push(strip);
		}

		stack_vertical_images(&ordered)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectionMatch {
	mean_abs_diff_x100: u32,
	motion_rows: u32,
}

#[cfg(target_os = "macos")]
fn classify_vision_downward_sample_motion_against(
	previous: &RgbaImage,
	next: &RgbaImage,
) -> Option<DirectionMatch> {
	let previous_cg = cg_image_from_rgba_image(previous).ok()?;
	let next_cg = cg_image_from_rgba_image(next).ok()?;
	let options = NSDictionary::<VNImageOption, AnyObject>::new();
	let request = unsafe {
		VNTranslationalImageRegistrationRequest::initWithTargetedCGImage_options(
			VNTranslationalImageRegistrationRequest::alloc(),
			previous_cg.as_ref(),
			options.as_ref(),
		)
	};
	let request_array = NSArray::from_retained_slice(&[request
		.clone()
		.into_super()
		.into_super()
		.into_super()
		.into_super()]);
	let handler = unsafe {
		VNImageRequestHandler::initWithCGImage_options(
			VNImageRequestHandler::alloc(),
			next_cg.as_ref(),
			options.as_ref(),
		)
	};

	handler.performRequests_error(request_array.as_ref()).ok()?;

	let results = unsafe { request.results() }?;
	if results.count() == 0 {
		return None;
	}

	let translation = unsafe { results.objectAtIndex(0).alignmentTransform() };
	let motion_rows = translation.ty.round();
	if !motion_rows.is_finite() || motion_rows <= 0.0 {
		return None;
	}
	let motion_rows = motion_rows as u32;
	let config = OverlapSearchConfig::default();
	let matched = evaluate_overlap_direction(
		previous,
		next,
		ScrollDirection::Down,
		motion_rows..=motion_rows,
		config,
	)?;
	let max_overlap = previous.height().min(next.height());

	downward_registration_has_meaningful_overlap(matched, max_overlap, config).then_some(matched)
}

#[cfg(not(target_os = "macos"))]
fn classify_vision_downward_sample_motion_against(
	_previous: &RgbaImage,
	_next: &RgbaImage,
) -> Option<DirectionMatch> {
	None
}

fn estimate_pairwise_downward_shift_rows(previous: &RgbaImage, current: &RgbaImage) -> Option<u32> {
	if previous.dimensions() != current.dimensions() {
		return None;
	}
	let (_width, height) = previous.dimensions();
	if height < 3 {
		return None;
	}
	let max_shift = height.saturating_sub(1);

	evaluate_overlap_direction(
		previous,
		current,
		ScrollDirection::Down,
		1..=max_shift,
		worker_pairwise_overlap_search_config(),
	)
	.map(|matched| matched.motion_rows)
}

#[cfg(target_os = "macos")]
fn cg_image_from_rgba_image(
	image: &RgbaImage,
) -> Result<objc2_core_foundation::CFRetained<CGImage>> {
	let width = image.width() as usize;
	let height = image.height() as usize;
	if width == 0 || height == 0 {
		return Err(eyre::eyre!("vision registration image has zero dimensions"));
	}

	let bytes = CFData::from_bytes(image.as_raw());
	let provider = CGDataProvider::with_cf_data(Some(bytes.as_ref()))
		.ok_or_else(|| eyre::eyre!("failed to create CGDataProvider for Vision registration"))?;
	let color_space = CGColorSpace::new_device_rgb()
		.ok_or_else(|| eyre::eyre!("failed to create RGB colorspace for Vision registration"))?;
	let bitmap_info = CGBitmapInfo(CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0);

	unsafe {
		CGImage::new(
			width,
			height,
			8,
			32,
			width.saturating_mul(4),
			Some(color_space.as_ref()),
			bitmap_info,
			Some(provider.as_ref()),
			std::ptr::null(),
			false,
			CGColorRenderingIntent::RenderingIntentDefault,
		)
	}
	.ok_or_else(|| eyre::eyre!("failed to create CGImage for Vision registration"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownwardRegistration {
	NoMatch,
	Matched(DirectionMatch),
	Ambiguous { best: DirectionMatch, competing: DirectionMatch },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownwardSampleMatchSource {
	ObservedSample,
	PreviewOnlyLocalSample,
}

impl DownwardSampleMatchSource {
	const fn label(self) -> &'static str {
		match self {
			Self::ObservedSample => "observed_sample",
			Self::PreviewOnlyLocalSample => "preview_only_local_sample",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DownwardSampleMatch {
	matched: DirectionMatch,
	source: DownwardSampleMatchSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownwardRegistrationWithSource {
	NoMatch,
	Matched(DownwardSampleMatch),
	Ambiguous { best: DownwardSampleMatch, competing: DownwardSampleMatch },
}

impl DownwardRegistration {
	fn map_source(self, source: DownwardSampleMatchSource) -> DownwardRegistrationWithSource {
		match self {
			Self::NoMatch => DownwardRegistrationWithSource::NoMatch,
			Self::Matched(matched) => {
				DownwardRegistrationWithSource::Matched(DownwardSampleMatch { matched, source })
			},
			Self::Ambiguous { best, competing } => DownwardRegistrationWithSource::Ambiguous {
				best: DownwardSampleMatch { matched: best, source },
				competing: DownwardSampleMatch { matched: competing, source },
			},
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownwardViewportCandidateSource {
	ObservedSample,
	PreviewOnlyLocalSample,
	CommittedKeyframe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommittedDownwardViewportCandidateMode {
	LastCommittedOnly,
	IncludeRecentHistory,
}

impl DownwardViewportCandidateSource {
	const fn priority(self) -> u8 {
		match self {
			Self::CommittedKeyframe => 0,
			Self::ObservedSample => 1,
			Self::PreviewOnlyLocalSample => 2,
		}
	}

	const fn decision_source(self) -> &'static str {
		match self {
			Self::ObservedSample => "sample_motion_downward_growth_from_observed_keyframe",
			Self::PreviewOnlyLocalSample => {
				"sample_motion_downward_growth_from_preview_only_local_sample"
			},
			Self::CommittedKeyframe => "sample_motion_downward_growth_from_committed_keyframe",
		}
	}

	const fn fallback_decision_source(self) -> &'static str {
		match self {
			Self::ObservedSample => "fallback_downward_registration_from_observed_keyframe",
			Self::PreviewOnlyLocalSample => {
				"fallback_downward_registration_from_preview_only_local_sample"
			},
			Self::CommittedKeyframe => "fallback_downward_registration_from_committed_keyframe",
		}
	}
}

impl From<DownwardSampleMatchSource> for DownwardViewportCandidateSource {
	fn from(value: DownwardSampleMatchSource) -> Self {
		match value {
			DownwardSampleMatchSource::ObservedSample => Self::ObservedSample,
			DownwardSampleMatchSource::PreviewOnlyLocalSample => Self::PreviewOnlyLocalSample,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DownwardViewportCandidate {
	source: DownwardViewportCandidateSource,
	viewport_top_y: i32,
	motion_rows: u32,
	mean_abs_diff_x100: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockedPreviewOnlyLocalCandidate {
	candidate: DownwardViewportCandidate,
	repeats: u8,
}

impl DownwardViewportCandidate {
	fn competing_block_reason(self, competing: Self) -> &'static str {
		match (self.source, competing.source) {
			(
				DownwardViewportCandidateSource::CommittedKeyframe,
				DownwardViewportCandidateSource::CommittedKeyframe,
			) => "conflicting_committed_keyframe_authority",
			_ => "conflicting_downward_viewport_authority",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownwardViewportResolution {
	NoMatch,
	Selected(DownwardViewportCandidate),
	Ambiguous { preferred: DownwardViewportCandidate, competing: DownwardViewportCandidate },
}

fn select_downward_viewport_candidate(
	candidates: &mut [DownwardViewportCandidate],
) -> DownwardViewportResolution {
	if candidates.is_empty() {
		return DownwardViewportResolution::NoMatch;
	}

	if let Some(preferred_local) = prefer_local_downward_viewport_candidate(candidates) {
		let competing = candidates.iter().copied().find(|candidate| {
			candidate != &preferred_local
				&& candidate.viewport_top_y.abs_diff(preferred_local.viewport_top_y)
					>= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
				&& candidate.mean_abs_diff_x100
					<= preferred_local
						.mean_abs_diff_x100
						.saturating_add(DIRECTION_WARNING_MARGIN_X100)
		});

		return match competing {
			Some(competing) => {
				DownwardViewportResolution::Ambiguous { preferred: preferred_local, competing }
			},
			None => DownwardViewportResolution::Selected(preferred_local),
		};
	}

	candidates.sort_by(|left, right| {
		left.mean_abs_diff_x100
			.cmp(&right.mean_abs_diff_x100)
			.then(left.source.priority().cmp(&right.source.priority()))
			.then(left.motion_rows.cmp(&right.motion_rows))
	});

	let preferred = candidates[0];
	let competing = candidates.iter().copied().skip(1).find(|candidate| {
		candidate.viewport_top_y.abs_diff(preferred.viewport_top_y)
			>= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
			&& candidate.mean_abs_diff_x100
				<= preferred.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
	});

	match competing {
		Some(competing) => DownwardViewportResolution::Ambiguous { preferred, competing },
		None => DownwardViewportResolution::Selected(preferred),
	}
}

fn format_downward_viewport_candidates(candidates: &[DownwardViewportCandidate]) -> String {
	candidates
		.iter()
		.map(|candidate| {
			format!(
				"{:?}@{}/{}:{}",
				candidate.source,
				candidate.viewport_top_y,
				candidate.motion_rows,
				candidate.mean_abs_diff_x100
			)
		})
		.collect::<Vec<_>>()
		.join(",")
}

fn prefer_local_downward_viewport_candidate(
	candidates: &[DownwardViewportCandidate],
) -> Option<DownwardViewportCandidate> {
	let local = best_local_downward_viewport_candidate(candidates)?;
	let committed = candidates
		.iter()
		.copied()
		.filter(|candidate| candidate.source == DownwardViewportCandidateSource::CommittedKeyframe)
		.min_by(|left, right| {
			left.mean_abs_diff_x100
				.cmp(&right.mean_abs_diff_x100)
				.then(left.motion_rows.cmp(&right.motion_rows))
		});

	let Some(committed) = committed else {
		return Some(local);
	};

	let committed_is_nearby = committed.viewport_top_y.abs_diff(local.viewport_top_y)
		< DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS;
	let committed_is_only_modestly_better =
		committed.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
			>= local.mean_abs_diff_x100;

	if committed_is_nearby && committed_is_only_modestly_better { Some(local) } else { None }
}

fn best_local_downward_viewport_candidate(
	candidates: &[DownwardViewportCandidate],
) -> Option<DownwardViewportCandidate> {
	candidates
		.iter()
		.copied()
		.filter(|candidate| candidate.source != DownwardViewportCandidateSource::CommittedKeyframe)
		.min_by(|left, right| {
			left.mean_abs_diff_x100
				.cmp(&right.mean_abs_diff_x100)
				.then(left.source.priority().cmp(&right.source.priority()))
				.then(left.motion_rows.cmp(&right.motion_rows))
		})
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OverlapSearchRange {
	start: u32,
	end: u32,
}
impl OverlapSearchRange {
	fn as_range(self) -> RangeInclusive<u32> {
		self.start..=self.end
	}
}

impl From<RangeInclusive<u32>> for OverlapSearchRange {
	fn from(range: RangeInclusive<u32>) -> Self {
		Self { start: *range.start(), end: *range.end() }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectionMatchEval {
	preferred_range: Option<OverlapSearchRange>,
	max_motion_rows: u32,
	preferred_only_match: Option<DirectionMatch>,
	final_match: Option<DirectionMatch>,
	used_full_range_fallback: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MotionObservation {
	direction: ScrollDirection,
	motion_rows: u32,
}

#[derive(Clone, Copy, Debug)]
struct UpInputMatchLog {
	sample_motion: Option<MotionObservation>,
	sample_down_match: Option<DirectionMatch>,
	sample_up_match: Option<DirectionMatch>,
	committed_down_match: Option<DirectionMatch>,
	committed_up_match: Option<DirectionMatch>,
	sample_override_wins: bool,
	committed_override_wins: bool,
}

#[derive(Clone, Copy, Debug)]
struct UpInputSearchWindowLog<'a> {
	sample_delta: Option<u32>,
	sample_down_match_eval: &'a DirectionMatchEval,
	sample_up_match_eval: &'a DirectionMatchEval,
	committed_down_match_eval: &'a DirectionMatchEval,
	committed_up_match_eval: &'a DirectionMatchEval,
	frame_equals_last_sample: bool,
	frame_equals_last_committed: bool,
}

#[derive(Clone, Copy, Debug)]
struct UpwardInputDiagnostics {
	sample_down_match_eval: DirectionMatchEval,
	sample_up_match_eval: DirectionMatchEval,
	committed_down_match_eval: DirectionMatchEval,
	committed_up_match_eval: DirectionMatchEval,
	sample_override_match: Option<DirectionMatch>,
	committed_override_match: Option<DirectionMatch>,
}

#[derive(Clone, Copy, Debug)]
struct ResumeFrontierMatchLog {
	motion_rows: u32,
	candidate_observed_viewport_top_y: i32,
	residual_growth_rows: u32,
	raw_committed_down_match: Option<DirectionMatch>,
	trusted_committed_down_match: Option<DirectionMatch>,
	committed_up_match: Option<DirectionMatch>,
	frame_reacquires_last_committed_viewport: bool,
}

#[derive(Clone, Copy, Debug)]
struct ResumeFrontierDirectMatchContext {
	motion_rows: u32,
	candidate_observed_viewport_top_y: i32,
	residual_growth_rows: u32,
}

#[derive(Clone, Debug)]
struct GrowthCommit {
	frame: RgbaImage,
	growth_rows: u32,
	viewport_top_y: i32,
	decision_source: &'static str,
	detected_motion_rows: Option<u32>,
	effective_motion_rows_hint: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InformativeSpan {
	start_x: u32,
	end_exclusive_x: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScrollDirection {
	Up,
	Down,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScrollObserveOutcome {
	NoChange,
	PreviewUpdated,
	UnsupportedDirection { direction: ScrollDirection },
	Committed { direction: ScrollDirection, growth_rows: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OverlapOrientation {
	PreviousBottomToNextTop,
	PreviousTopToNextBottom,
}

#[must_use]
pub(crate) fn scroll_capture_fingerprint(image: &RgbaImage) -> Vec<u8> {
	ScrollFrameFingerprint::from_image(image).into_bytes()
}

#[must_use]
pub(crate) fn scroll_capture_fingerprint_delta(left: &[u8], right: &[u8]) -> u32 {
	if left.len() != right.len() || left.is_empty() || !left.len().is_multiple_of(4) {
		return u32::MAX;
	}

	let mut total_abs_diff = 0_u64;
	let mut comparisons = 0_u64;

	for (left_pixel, right_pixel) in left.chunks_exact(4).zip(right.chunks_exact(4)) {
		total_abs_diff = total_abs_diff
			.saturating_add(u64::from(left_pixel[0].abs_diff(right_pixel[0])))
			.saturating_add(u64::from(left_pixel[1].abs_diff(right_pixel[1])))
			.saturating_add(u64::from(left_pixel[2].abs_diff(right_pixel[2])))
			.saturating_add(u64::from(left_pixel[3].abs_diff(right_pixel[3])));
		comparisons = comparisons.saturating_add(4);
	}

	if comparisons == 0 { u32::MAX } else { (total_abs_diff / comparisons) as u32 }
}

#[cfg(test)]
#[must_use]
pub(crate) fn detect_vertical_overlap(
	previous: &RgbaImage,
	next: &RgbaImage,
	config: OverlapSearchConfig,
) -> OverlapMatch {
	detect_vertical_overlap_in_range(
		previous,
		next,
		1..=previous.height().min(next.height()),
		ScrollDirection::Down,
		config,
		overlap_global_informative_span(previous, next),
	)
}

fn evaluate_overlap_direction(
	previous: &RgbaImage,
	next: &RgbaImage,
	direction: ScrollDirection,
	range: RangeInclusive<u32>,
	config: OverlapSearchConfig,
) -> Option<DirectionMatch> {
	collect_overlap_direction_matches(previous, next, direction, range, config).into_iter().next()
}

fn collect_overlap_direction_matches(
	previous: &RgbaImage,
	next: &RgbaImage,
	direction: ScrollDirection,
	range: RangeInclusive<u32>,
	config: OverlapSearchConfig,
) -> Vec<DirectionMatch> {
	let Some(informative_span) = overlap_global_informative_span(previous, next) else {
		return Vec::new();
	};

	let max_overlap = previous.height().min(next.height());
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };
	let max_motion_rows = max_overlap.saturating_sub(effective_min_overlap).max(1);
	let search_start = (*range.start()).max(1).min(max_motion_rows);
	let search_end = (*range.end()).max(search_start).min(max_motion_rows);
	let orientation = match direction {
		ScrollDirection::Down => OverlapOrientation::PreviousBottomToNextTop,
		ScrollDirection::Up => OverlapOrientation::PreviousTopToNextBottom,
	};
	let mut matches = Vec::with_capacity(search_end.saturating_sub(search_start) as usize + 1);

	for motion_rows in search_start..=search_end {
		let overlap_rows = max_overlap.saturating_sub(motion_rows);

		if overlap_rows < effective_min_overlap {
			continue;
		}

		let diff = motion_mean_abs_diff_x100(
			previous,
			next,
			motion_rows,
			config,
			orientation,
			informative_span,
		);

		if diff > config.max_mean_abs_diff_x100 {
			continue;
		}

		matches.push(DirectionMatch { mean_abs_diff_x100: diff, motion_rows });
	}

	matches.sort_by(|left, right| {
		left.mean_abs_diff_x100
			.cmp(&right.mean_abs_diff_x100)
			.then(left.motion_rows.cmp(&right.motion_rows))
	});
	matches
}

fn collect_overlap_direction_matches_in_ranges(
	previous: &RgbaImage,
	next: &RgbaImage,
	direction: ScrollDirection,
	ranges: &[RangeInclusive<u32>],
	config: OverlapSearchConfig,
) -> Vec<DirectionMatch> {
	let mut matches = Vec::new();

	for range in ranges {
		matches.extend(collect_overlap_direction_matches(
			previous,
			next,
			direction,
			range.clone(),
			config,
		));
	}

	if matches.len() <= 1 {
		return matches;
	}

	matches.sort_by(|left, right| {
		left.motion_rows
			.cmp(&right.motion_rows)
			.then(left.mean_abs_diff_x100.cmp(&right.mean_abs_diff_x100))
	});

	let mut deduped: Vec<DirectionMatch> = Vec::with_capacity(matches.len());

	for matched in matches {
		if let Some(previous) = deduped.last_mut()
			&& previous.motion_rows == matched.motion_rows
		{
			if matched.mean_abs_diff_x100 < previous.mean_abs_diff_x100 {
				*previous = matched;
			}
			continue;
		}

		deduped.push(matched);
	}

	deduped.sort_by(|left, right| {
		left.mean_abs_diff_x100
			.cmp(&right.mean_abs_diff_x100)
			.then(left.motion_rows.cmp(&right.motion_rows))
	});

	deduped
}

fn classify_downward_registration_candidates(
	candidates: &[DirectionMatch],
) -> DownwardRegistration {
	let Some(best) = candidates.first().copied() else {
		return DownwardRegistration::NoMatch;
	};
	let competing = candidates.iter().copied().skip(1).find(|candidate| {
		candidate.motion_rows.abs_diff(best.motion_rows) >= DOWNWARD_REGISTRATION_AMBIGUOUS_GAP_ROWS
	});

	match competing {
		Some(competing)
			if best.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
				>= competing.mean_abs_diff_x100 =>
		{
			DownwardRegistration::Ambiguous { best, competing }
		},
		_ => DownwardRegistration::Matched(best),
	}
}

fn downward_registration_has_meaningful_overlap(
	matched: DirectionMatch,
	max_overlap: u32,
	config: OverlapSearchConfig,
) -> bool {
	let overlap_rows = max_overlap.saturating_sub(matched.motion_rows);
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };
	let min_overlap_rows =
		effective_min_overlap.max(max_overlap / DOWNWARD_REGISTRATION_MIN_OVERLAP_DIVISOR).max(1);

	overlap_rows >= min_overlap_rows
}

fn preview_update_outcome(preview_changed: bool) -> ScrollObserveOutcome {
	if preview_changed {
		ScrollObserveOutcome::PreviewUpdated
	} else {
		ScrollObserveOutcome::NoChange
	}
}

fn resume_direct_match_is_trustworthy(matched: DirectionMatch) -> bool {
	matched.mean_abs_diff_x100 <= RESUME_DIRECT_PROOF_MAX_MEAN_ABS_DIFF_X100
}

fn preferred_upward_override_match(
	up_match: Option<DirectionMatch>,
	down_match: Option<DirectionMatch>,
) -> Option<DirectionMatch> {
	match (up_match, down_match) {
		(Some(up), Some(_down)) if resume_direct_match_is_trustworthy(up) => Some(up),
		(Some(up), None) if resume_direct_match_is_trustworthy(up) => Some(up),
		_ => None,
	}
}

fn preferred_upward_input_override_match(
	sample_match: Option<DirectionMatch>,
	committed_match: Option<DirectionMatch>,
) -> Option<(DirectionMatch, bool)> {
	match (sample_match, committed_match) {
		(Some(sample), Some(committed))
			if committed.motion_rows <= sample.motion_rows
				&& committed.mean_abs_diff_x100
					<= sample.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100) =>
		{
			Some((committed, true))
		},
		(Some(sample), Some(_committed)) => Some((sample, false)),
		(Some(sample), None) => Some((sample, false)),
		(None, Some(committed)) => Some((committed, true)),
		(None, None) => None,
	}
}

fn upward_confirmation_match_for_downward_input(
	up_match: Option<DirectionMatch>,
	down_match: Option<DirectionMatch>,
	has_committed_growth: bool,
) -> Option<DirectionMatch> {
	if !has_committed_growth {
		return None;
	}

	match (up_match, down_match) {
		(Some(up), Some(down))
			if resume_direct_match_is_trustworthy(up)
				&& up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100 =>
		{
			Some(up)
		},
		(Some(up), None) if resume_direct_match_is_trustworthy(up) => Some(up),
		_ => None,
	}
}

fn rewind_active_upward_override_match(
	sample_match: Option<DirectionMatch>,
	committed_match: Option<DirectionMatch>,
	rewind_active: bool,
) -> Option<(DirectionMatch, bool)> {
	if !rewind_active {
		return None;
	}

	match (sample_match, committed_match) {
		(Some(sample), Some(committed))
			if committed.motion_rows < sample.motion_rows
				&& committed.mean_abs_diff_x100
					<= sample.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100) =>
		{
			Some((committed, true))
		},
		(Some(sample), _) => Some((sample, false)),
		(None, Some(committed)) => Some((committed, true)),
		(None, None) => None,
	}
}

fn rewind_active_upward_motion_should_fail_closed(
	sample_up_match: Option<DirectionMatch>,
	committed_up_match: Option<DirectionMatch>,
	committed_down_match: Option<DirectionMatch>,
	rewind_active: bool,
) -> bool {
	if !rewind_active {
		return false;
	}
	if committed_up_match.is_some() {
		return false;
	}

	matches!(
		(sample_up_match, committed_down_match),
		(Some(sample_up), Some(committed_down))
			if committed_down.mean_abs_diff_x100
				<= sample_up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
				&& committed_down.motion_rows >= sample_up.motion_rows
	)
}

fn max_directional_motion_rows(
	previous: &RgbaImage,
	next: &RgbaImage,
	config: OverlapSearchConfig,
) -> u32 {
	let max_overlap = previous.height().min(next.height());
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };

	max_overlap.saturating_sub(effective_min_overlap).max(1)
}

#[cfg(test)]
fn detect_vertical_overlap_in_range(
	previous: &RgbaImage,
	next: &RgbaImage,
	range: RangeInclusive<u32>,
	direction: ScrollDirection,
	config: OverlapSearchConfig,
	informative_span: Option<InformativeSpan>,
) -> OverlapMatch {
	if previous.width() == 0 || next.width() == 0 || previous.height() == 0 || next.height() == 0 {
		return OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };
	}

	let Some(informative_span) = informative_span else {
		return OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };
	};
	let max_overlap = previous.height().min(next.height());
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };
	let max_motion_rows = max_overlap.saturating_sub(effective_min_overlap).max(1);
	let search_start = (*range.start()).max(1).min(max_motion_rows);
	let search_end = (*range.end()).max(search_start).min(max_motion_rows);
	let orientation = match direction {
		ScrollDirection::Down => OverlapOrientation::PreviousBottomToNextTop,
		ScrollDirection::Up => OverlapOrientation::PreviousTopToNextBottom,
	};
	let mut best = OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };

	for motion_rows in search_start..=search_end {
		let overlap_rows = max_overlap.saturating_sub(motion_rows);

		if overlap_rows < effective_min_overlap {
			continue;
		}

		let diff = motion_mean_abs_diff_x100(
			previous,
			next,
			motion_rows,
			config,
			orientation,
			informative_span,
		);

		if diff > config.max_mean_abs_diff_x100 {
			continue;
		}
		if !best.matched
			|| diff < best.mean_abs_diff_x100
			|| (diff == best.mean_abs_diff_x100 && overlap_rows > best.rows)
		{
			best = OverlapMatch { rows: overlap_rows, matched: true, mean_abs_diff_x100: diff };
		}
	}

	best
}

fn resize_strip_to_preview_width(strip: &RgbaImage, preview_width_px: u32) -> RgbaImage {
	if strip.width() <= preview_width_px {
		return strip.clone();
	}

	let preview_height = ((strip.height() as f32 / strip.width() as f32) * preview_width_px as f32)
		.round()
		.max(1.0) as u32;

	imageops::resize(strip, preview_width_px, preview_height, FilterType::Triangle)
}

pub(crate) fn compose_provisional_preview_image(
	base_preview: &RgbaImage,
	latest_frame: Option<&RgbaImage>,
	motion_rows_hint: Option<u32>,
	preview_width_px: u32,
) -> RgbaImage {
	let Some(frame) = latest_frame else {
		return base_preview.clone();
	};
	let Some(motion_rows_hint) = motion_rows_hint else {
		return base_preview.clone();
	};
	let hinted_growth_rows = motion_rows_hint.min(frame.height());
	if hinted_growth_rows == 0 {
		return base_preview.clone();
	}

	let Some(strip) = crop_bottom_rows(frame, hinted_growth_rows) else {
		return base_preview.clone();
	};
	let preview_strip = resize_strip_to_preview_width(&strip, preview_width_px);

	append_vertical_image(base_preview, &preview_strip).unwrap_or_else(|_| base_preview.clone())
}

fn crop_bottom_rows(frame: &RgbaImage, rows: u32) -> Option<RgbaImage> {
	let rows = rows.min(frame.height());

	if rows == 0 {
		return None;
	}

	let start_y = frame.height().saturating_sub(rows);

	Some(imageops::crop_imm(frame, 0, start_y, frame.width(), rows).to_image())
}

fn stack_vertical_images(images: &[&RgbaImage]) -> Result<RgbaImage> {
	let Some(first) = images.first() else {
		return Err(eyre::eyre!("cannot stack an empty image list"));
	};
	let width = first.width();
	let total_height = images.iter().try_fold(0_u32, |acc, image| {
		if image.width() != width {
			return Err(eyre::eyre!(
				"image width mismatch while stacking: expected {} got {}",
				width,
				image.width()
			));
		}

		acc.checked_add(image.height()).ok_or_else(|| eyre::eyre!("stacked image height overflow"))
	})?;
	let total_bytes = images.iter().try_fold(0_usize, |acc, image| {
		acc.checked_add(image.as_raw().len())
			.ok_or_else(|| eyre::eyre!("stacked image byte length overflow"))
	})?;
	let mut raw = Vec::with_capacity(total_bytes);

	for image in images {
		raw.extend_from_slice(image.as_raw());
	}

	RgbaImage::from_raw(width, total_height, raw)
		.ok_or_else(|| eyre::eyre!("failed to construct stacked image buffer"))
}

fn append_vertical_image(base: &RgbaImage, strip: &RgbaImage) -> Result<RgbaImage> {
	if base.width() != strip.width() {
		return Err(eyre::eyre!(
			"image width mismatch while appending: expected {} got {}",
			base.width(),
			strip.width()
		));
	}

	stack_vertical_images(&[base, strip])
}

fn motion_mean_abs_diff_x100(
	previous: &RgbaImage,
	next: &RgbaImage,
	motion_rows: u32,
	config: OverlapSearchConfig,
	orientation: OverlapOrientation,
	informative_span: InformativeSpan,
) -> u32 {
	let width = previous.width().min(next.width());
	let max_overlap = previous.height().min(next.height());
	let overlap_rows = max_overlap.saturating_sub(motion_rows);

	if overlap_rows == 0 {
		return u32::MAX;
	}

	let column_samples = width.min(config.max_column_samples).max(1);
	let row_samples = overlap_rows.min(config.max_row_samples).max(1);
	let previous_overlap_start_y = previous.height().saturating_sub(overlap_rows);
	let next_overlap_start_y = next.height().saturating_sub(overlap_rows);
	let previous_start_y = match orientation {
		OverlapOrientation::PreviousBottomToNextTop => previous_overlap_start_y,
		OverlapOrientation::PreviousTopToNextBottom => 0,
	};
	let next_start_y = match orientation {
		OverlapOrientation::PreviousBottomToNextTop => 0,
		OverlapOrientation::PreviousTopToNextBottom => next_overlap_start_y,
	};
	let x_start = informative_span.start_x.min(width.saturating_sub(1));
	let x_end = informative_span.end_exclusive_x.min(width).max(x_start + 1);
	let effective_width = x_end.saturating_sub(x_start).max(1);
	let column_samples = effective_width.min(column_samples).max(1);
	let mut total_abs_diff = 0_u64;
	let mut comparisons = 0_u64;

	for row in 0..row_samples {
		let local_y = evenly_spaced_sample(0, overlap_rows, row, row_samples);
		let previous_y =
			previous_start_y.saturating_add(local_y).min(previous.height().saturating_sub(1));
		let next_y = next_start_y.saturating_add(local_y).min(next.height().saturating_sub(1));

		for column in 0..column_samples {
			let x = evenly_spaced_sample(x_start, x_end, column, column_samples);
			let previous_pixel = previous.get_pixel(x, previous_y).0;
			let next_pixel = next.get_pixel(x, next_y).0;

			total_abs_diff = total_abs_diff
				.saturating_add(u64::from(previous_pixel[0].abs_diff(next_pixel[0])))
				.saturating_add(u64::from(previous_pixel[1].abs_diff(next_pixel[1])))
				.saturating_add(u64::from(previous_pixel[2].abs_diff(next_pixel[2])));
			comparisons = comparisons.saturating_add(3);
		}
	}

	if comparisons == 0 {
		return u32::MAX;
	}

	((total_abs_diff.saturating_mul(100)) / comparisons) as u32
}

fn overlap_global_informative_span(left: &RgbaImage, right: &RgbaImage) -> Option<InformativeSpan> {
	let left_span = informative_column_span(left, 0, left.height());
	let right_span = informative_column_span(right, 0, right.height());
	let width = left.width().min(right.width());

	match (left_span, right_span) {
		(Some(left_span), Some(right_span)) => {
			let start_x = left_span.start_x.max(right_span.start_x);
			let end_exclusive_x =
				left_span.end_exclusive_x.min(right_span.end_exclusive_x).min(width);

			(end_exclusive_x > start_x).then_some(InformativeSpan { start_x, end_exclusive_x })
		},
		(Some(span), None) | (None, Some(span)) => {
			let end_exclusive_x = span.end_exclusive_x.min(width).max(span.start_x + 1);

			(end_exclusive_x > span.start_x)
				.then_some(InformativeSpan { start_x: span.start_x, end_exclusive_x })
		},
		(None, None) => None,
	}
}

fn informative_column_span(image: &RgbaImage, start_y: u32, rows: u32) -> Option<InformativeSpan> {
	if image.width() == 0 || image.height() == 0 || rows == 0 {
		return None;
	}

	let clamped_rows = rows.min(image.height().saturating_sub(start_y)).max(1);
	let row_samples = clamped_rows.min(INFORMATIVE_SPAN_ROW_SAMPLES.max(2)).max(2);
	let mut scores = vec![0_u32; image.width() as usize];
	let mut max_score = 0_u32;

	for row in 0..row_samples.saturating_sub(1) {
		let local_y = evenly_spaced_sample(0, clamped_rows, row, row_samples);
		let next_local_y = (local_y.saturating_add(1)).min(clamped_rows.saturating_sub(1));
		let y = start_y.saturating_add(local_y).min(image.height().saturating_sub(1));
		let next_y = start_y.saturating_add(next_local_y).min(image.height().saturating_sub(1));

		for x in 0..image.width() {
			let pixel = image.get_pixel(x, y).0;
			let next_pixel = image.get_pixel(x, next_y).0;
			let score = u32::from(pixel[0].abs_diff(next_pixel[0]))
				.saturating_add(u32::from(pixel[1].abs_diff(next_pixel[1])))
				.saturating_add(u32::from(pixel[2].abs_diff(next_pixel[2])));
			let slot = &mut scores[x as usize];

			*slot = slot.saturating_add(score);
			max_score = max_score.max(*slot);
		}
	}

	if max_score == 0 {
		return None;
	}

	let threshold = (max_score / 6).max(INFORMATIVE_SPAN_SCORE_FLOOR_X100);
	let mut start_x = None;
	let mut end_x = None;

	for (x, score) in scores.iter().enumerate() {
		if *score >= threshold {
			start_x.get_or_insert(x as u32);

			end_x = Some((x as u32).saturating_add(1));
		}
	}

	let start_x = start_x?;
	let end_exclusive_x = end_x?;
	let padding = INFORMATIVE_SPAN_HORIZONTAL_PADDING_PX.min(image.width() / 8);
	let start_x = start_x.saturating_sub(padding);
	let end_exclusive_x =
		end_exclusive_x.saturating_add(padding).min(image.width()).max(start_x.saturating_add(1));

	Some(InformativeSpan { start_x, end_exclusive_x })
}

fn evenly_spaced_sample(start: u32, end_exclusive: u32, index: u32, count: u32) -> u32 {
	let span = end_exclusive.saturating_sub(start).max(1);

	if count <= 1 {
		return start.min(end_exclusive.saturating_sub(1));
	}

	let numerator =
		(u64::from(index) * u64::from(span.saturating_sub(1))) / u64::from(count.saturating_sub(1));

	start.saturating_add(numerator as u32).min(end_exclusive.saturating_sub(1))
}

#[cfg(test)]
mod tests {
	use image::Rgba;

	use crate::scroll_capture::{
		self, DirectionMatch, DownwardRegistration, DownwardSampleMatch, DownwardSampleMatchSource,
		DownwardViewportCandidate, DownwardViewportCandidateSource, DownwardViewportResolution,
		GrowthCommit, MotionObservation, OverlapSearchConfig, PreviewOnlyDownwardLocalSample,
		ScrollDirection, ScrollFrameFingerprint, ScrollObserveOutcome, ScrollSession,
		classify_vision_downward_sample_motion_against, estimate_pairwise_downward_shift_rows,
		select_downward_viewport_candidate,
	};

	fn make_test_image(width: u32, rows: &[[u8; 4]]) -> image::RgbaImage {
		let mut image = image::RgbaImage::new(width, rows.len() as u32);

		for (y, row) in rows.iter().enumerate() {
			for x in 0..width {
				image.put_pixel(x, y as u32, Rgba(*row));
			}
		}

		image
	}

	fn make_window(
		document: &[[u8; 4]],
		width: u32,
		start_row: usize,
		window_rows: usize,
	) -> image::RgbaImage {
		make_test_image(width, &document[start_row..start_row + window_rows])
	}

	fn make_sparse_textlike_window(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
		let stripe_x = 104_u32;
		let mut image = image::RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));

		for y in 0..height {
			let document_row = start_row.saturating_add(y);
			let shade = ((document_row.saturating_mul(17)) % 180) as u8;

			for x in stripe_x..stripe_x.saturating_add(6) {
				image.put_pixel(x, y, Rgba([shade, shade, shade, 255]));
			}
			for x in stripe_x.saturating_add(10)..stripe_x.saturating_add(13) {
				if document_row % 19 < 9 {
					image.put_pixel(x, y, Rgba([40, 40, 40, 255]));
				}
			}
		}

		image
	}

	fn make_sparse_textlike_window_with_moving_edge_scrollbar(
		width: u32,
		height: u32,
		start_row: u32,
		thumb_top: u32,
	) -> image::RgbaImage {
		let mut image = make_sparse_textlike_window(width, height, start_row);
		let track_left = width.saturating_sub(18);
		let thumb_height = (height / 4).max(12).min(height.max(1));
		let thumb_top = thumb_top.min(height.saturating_sub(thumb_height));
		let thumb_right = width.saturating_sub(3).max(track_left.saturating_add(4));

		for y in 0..height {
			for x in track_left..width {
				image.put_pixel(x, y, Rgba([224, 224, 224, 255]));
			}
		}

		for y in thumb_top..thumb_top.saturating_add(thumb_height) {
			for x in track_left.saturating_add(3)..thumb_right {
				image.put_pixel(x, y, Rgba([28, 28, 28, 255]));
			}
		}

		image
	}

	fn make_browser_like_window(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
		let mut image = make_sparse_textlike_window(width, height, start_row);
		let scrollbar_left = width.saturating_sub(18);
		let content_left = 56_u32;
		let content_right = width.saturating_sub(48);
		let heading_width = 220_u32;
		let paragraph_width = content_right.saturating_sub(content_left);

		for y in 0..height {
			let document_row = start_row.saturating_add(y);

			if document_row % 420 < 18 {
				for x in content_left..content_left.saturating_add(heading_width) {
					image.put_pixel(x, y, Rgba([26, 26, 26, 255]));
				}
			} else if document_row % 420 >= 54 && document_row % 420 < 220 {
				if document_row % 24 < 3 {
					let trim = ((document_row / 24) % 5) * 18;
					for x in content_left
						..content_left.saturating_add(paragraph_width.saturating_sub(trim))
					{
						image.put_pixel(x, y, Rgba([72, 72, 72, 255]));
					}
				}
			} else if document_row % 420 >= 270 && document_row % 420 < 360 {
				if document_row % 20 < 2 {
					for x in content_left.saturating_add(20)
						..content_left.saturating_add(paragraph_width.saturating_sub(70))
					{
						image.put_pixel(x, y, Rgba([98, 98, 98, 255]));
					}
				}
			}

			for x in scrollbar_left..width {
				image.put_pixel(x, y, Rgba([232, 232, 232, 255]));
			}
		}

		let thumb_height = (height / 5).max(16);
		let thumb_top = (start_row / 3) % height.max(thumb_height + 1);
		let thumb_top = thumb_top.min(height.saturating_sub(thumb_height));
		for y in thumb_top..thumb_top.saturating_add(thumb_height) {
			for x in scrollbar_left.saturating_add(3)..width.saturating_sub(4) {
				image.put_pixel(x, y, Rgba([96, 96, 96, 255]));
			}
		}

		image
	}

	#[test]
	fn overlap_detection_prefers_largest_matching_suffix() {
		let previous = make_test_image(
			5,
			&[
				[10, 0, 0, 255],
				[20, 0, 0, 255],
				[30, 0, 0, 255],
				[40, 0, 0, 255],
				[50, 0, 0, 255],
				[60, 0, 0, 255],
			],
		);
		let next = make_test_image(
			5,
			&[[40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255], [70, 0, 0, 255], [80, 0, 0, 255]],
		);
		let overlap = scroll_capture::detect_vertical_overlap(
			&previous,
			&next,
			OverlapSearchConfig { min_overlap_rows: 1, ..Default::default() },
		);

		assert!(overlap.matched);
		assert_eq!(overlap.rows, 3);
	}

	#[test]
	fn fingerprint_wrapper_returns_zero_delta_for_identical_images() {
		let image = image::RgbaImage::from_pixel(12, 12, Rgba([9, 8, 7, 255]));
		let left = scroll_capture::scroll_capture_fingerprint(&image);
		let right = scroll_capture::scroll_capture_fingerprint(&image);

		assert_eq!(scroll_capture::scroll_capture_fingerprint_delta(&left, &right), 0);
	}

	#[test]
	fn fingerprint_struct_distance_detects_changed_image() {
		let base = image::RgbaImage::from_pixel(12, 12, Rgba([9, 8, 7, 255]));
		let changed = image::RgbaImage::from_pixel(12, 12, Rgba([30, 8, 7, 255]));
		let left = ScrollFrameFingerprint::from_image(&base);
		let right = ScrollFrameFingerprint::from_image(&changed);

		assert!(left.distance(&right) > 0);
	}

	#[test]
	fn session_commits_downward_growth_on_first_matching_sample() {
		let base = make_test_image(
			3,
			&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
		);
		let moved = make_test_image(
			3,
			&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();
		let outcome = session.observe_downward_sample(moved).unwrap();

		assert_eq!(
			outcome,
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), 6);
		assert_eq!(session.export_image().get_pixel(0, 5), &Rgba([60, 0, 0, 255]));
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_commits_substantial_downward_growth_with_corroboration() {
		let base = make_sparse_textlike_window(512, 640, 0);
		let moved = make_sparse_textlike_window(512, 640, 90);
		let matched = classify_vision_downward_sample_motion_against(&base, &moved)
			.expect("vision registration should detect the substantial downward motion");
		let mut session = ScrollSession::new(base, 320).unwrap();
		let outcome = session.observe_worker_pairwise_vision_frame(moved).unwrap();

		assert!(matched.motion_rows >= 32);
		assert_eq!(
			outcome,
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: matched.motion_rows,
			}
		);
		assert_eq!(session.export_image().height(), 640 + matched.motion_rows);
		assert_eq!(session.current_viewport_top_y(), i32::try_from(matched.motion_rows).unwrap());
	}

	#[test]
	fn pairwise_downward_shift_estimate_matches_sparse_textlike_motion() {
		let base = make_sparse_textlike_window(512, 640, 0);
		let moved = make_sparse_textlike_window(512, 640, 58);

		assert_eq!(estimate_pairwise_downward_shift_rows(&base, &moved), Some(58));
	}

	#[test]
	fn pairwise_downward_shift_estimate_matches_browser_like_motion_above_legacy_cap() {
		let base = make_browser_like_window(512, 640, 0);
		let moved = make_browser_like_window(512, 640, 320);

		assert_eq!(estimate_pairwise_downward_shift_rows(&base, &moved), Some(320));
	}

	#[test]
	fn pairwise_downward_shift_estimate_tracks_successive_browser_like_steps() {
		let frames = [0_u32, 180, 360, 540, 720]
			.into_iter()
			.map(|start_row| make_browser_like_window(512, 640, start_row))
			.collect::<Vec<_>>();

		for window in frames.windows(2) {
			assert_eq!(estimate_pairwise_downward_shift_rows(&window[0], &window[1]), Some(180));
		}
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_uses_latest_committed_live_frame_for_followup_growth() {
		let base = make_sparse_textlike_window(512, 640, 0);
		let step_one = make_sparse_textlike_window(512, 640, 180);
		let step_two = make_sparse_textlike_window(512, 640, 360);
		let first_match = classify_vision_downward_sample_motion_against(&base, &step_one)
			.expect("first pairwise registration should detect downward motion");
		let followup_match = classify_vision_downward_sample_motion_against(&step_one, &step_two)
			.expect("followup pairwise registration should detect downward motion");
		let mut session = ScrollSession::new(base, 320).unwrap();

		assert_eq!(
			session.observe_worker_pairwise_vision_frame(step_one).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: first_match.motion_rows,
			}
		);
		assert_eq!(
			session.observe_worker_pairwise_vision_frame(step_two).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: followup_match.motion_rows,
			}
		);
		assert_eq!(
			session.export_image().height(),
			640 + first_match.motion_rows + followup_match.motion_rows
		);
		assert_eq!(
			session.current_viewport_top_y(),
			i32::try_from(first_match.motion_rows + followup_match.motion_rows).unwrap()
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_handles_repeated_frame_between_growth_steps() {
		let base = make_sparse_textlike_window(512, 640, 0);
		let step_one = make_sparse_textlike_window(512, 640, 180);
		let step_two = make_sparse_textlike_window(512, 640, 360);
		let first_match = classify_vision_downward_sample_motion_against(&base, &step_one)
			.expect("first pairwise registration should detect downward motion");
		let followup_match = classify_vision_downward_sample_motion_against(&step_one, &step_two)
			.expect("followup pairwise registration should detect downward motion");
		let mut session = ScrollSession::new(base, 320).unwrap();

		assert_eq!(
			session.observe_worker_pairwise_vision_frame(step_one.clone()).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: first_match.motion_rows,
			}
		);
		assert_eq!(
			session.observe_worker_pairwise_vision_frame(step_one).unwrap(),
			ScrollObserveOutcome::NoChange
		);
		assert_eq!(
			session.observe_worker_pairwise_vision_frame(step_two).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: followup_match.motion_rows,
			}
		);
		assert_eq!(
			session.export_image().height(),
			640 + first_match.motion_rows + followup_match.motion_rows
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_recovers_after_blocked_overshot_frame() {
		let base = make_browser_like_window(512, 640, 0);
		let blocked = make_browser_like_window(512, 640, 760);
		let followup = make_browser_like_window(512, 640, 844);
		let matched = classify_vision_downward_sample_motion_against(&blocked, &followup).expect(
			"pairwise registration should detect the followup step after the blocked overshot",
		);
		let mut session = ScrollSession::new(base, 320).unwrap();

		assert_eq!(
			session.observe_worker_pairwise_vision_frame(blocked).unwrap(),
			ScrollObserveOutcome::NoChange
		);
		assert_eq!(session.export_image().height(), 640);
		assert_eq!(session.current_viewport_top_y(), 0);
		assert_eq!(
			session.observe_worker_pairwise_vision_frame(followup).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: matched.motion_rows,
			}
		);
		assert_eq!(session.export_image().height(), 640 + matched.motion_rows);
		assert_eq!(session.current_viewport_top_y(), i32::try_from(matched.motion_rows).unwrap());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_clears_preview_local_followup_carryover_on_no_change() {
		let base = make_sparse_textlike_window(512, 640, 0);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();
		session.record_preview_only_downward_local_sample(&base, 123);
		session.pending_suppressed_huge_preview_only_local_followup =
			Some(DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 160,
				motion_rows: 160,
				mean_abs_diff_x100: 0,
			});
		session.pending_suppressed_huge_preview_only_local_followup_remaining_blocks = 2;
		session.pending_extreme_preview_only_local_tail_followup =
			Some(DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 161,
				motion_rows: 1,
				mean_abs_diff_x100: 0,
			});
		session.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 1;

		assert_eq!(
			session.observe_worker_pairwise_vision_frame(base).unwrap(),
			ScrollObserveOutcome::NoChange
		);
		assert!(session.last_preview_only_downward_local_sample.is_none());
		assert!(session.pending_suppressed_huge_preview_only_local_followup.is_none());
		assert_eq!(session.pending_suppressed_huge_preview_only_local_followup_remaining_blocks, 0);
		assert!(session.pending_extreme_preview_only_local_tail_followup.is_none());
		assert_eq!(session.pending_extreme_preview_only_local_tail_followup_remaining_blocks, 0);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_clears_preview_local_followup_carryover_on_commit() {
		let base = make_sparse_textlike_window(512, 640, 0);
		let moved = make_sparse_textlike_window(512, 640, 180);
		let matched = classify_vision_downward_sample_motion_against(&base, &moved)
			.expect("pairwise registration should detect downward motion");
		let mut session = ScrollSession::new(base, 320).unwrap();
		session.record_preview_only_downward_local_sample(&moved, 180);
		session.pending_suppressed_huge_preview_only_local_followup =
			Some(DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 160,
				motion_rows: 160,
				mean_abs_diff_x100: 0,
			});
		session.pending_suppressed_huge_preview_only_local_followup_remaining_blocks = 2;
		session.pending_extreme_preview_only_local_tail_followup =
			Some(DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 161,
				motion_rows: 1,
				mean_abs_diff_x100: 0,
			});
		session.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 1;

		assert_eq!(
			session.observe_worker_pairwise_vision_frame(moved).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: matched.motion_rows,
			}
		);
		assert!(session.last_preview_only_downward_local_sample.is_none());
		assert!(session.pending_suppressed_huge_preview_only_local_followup.is_none());
		assert_eq!(session.pending_suppressed_huge_preview_only_local_followup_remaining_blocks, 0);
		assert!(session.pending_extreme_preview_only_local_tail_followup.is_none());
		assert_eq!(session.pending_extreme_preview_only_local_tail_followup_remaining_blocks, 0);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_commits_successive_slowdown_steps() {
		let frames = [0_u32, 180, 300, 380, 420]
			.into_iter()
			.map(|start_row| make_sparse_textlike_window(512, 640, start_row))
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(frames[0].clone(), 320).unwrap();
		let mut expected_export_height = 640_u32;
		let mut expected_viewport_top_y = 0_i32;

		for window in frames.windows(2) {
			let previous = &window[0];
			let next = window[1].clone();
			let matched = classify_vision_downward_sample_motion_against(previous, &next)
				.expect("pairwise registration should detect each slowdown step");

			assert_eq!(
				session.observe_worker_pairwise_vision_frame(next).unwrap(),
				ScrollObserveOutcome::Committed {
					direction: ScrollDirection::Down,
					growth_rows: matched.motion_rows,
				}
			);

			expected_export_height = expected_export_height.saturating_add(matched.motion_rows);
			expected_viewport_top_y += i32::try_from(matched.motion_rows).unwrap();
		}

		assert_eq!(session.export_image().height(), expected_export_height);
		assert_eq!(session.current_viewport_top_y(), expected_viewport_top_y);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_commits_browser_like_growth_above_legacy_cap() {
		let base = make_browser_like_window(512, 640, 0);
		let moved = make_browser_like_window(512, 640, 320);
		let matched = classify_vision_downward_sample_motion_against(&base, &moved)
			.expect("vision registration should detect the browser-like downward motion");
		let mut session = ScrollSession::new(base, 320).unwrap();

		assert!(matched.motion_rows > 256);
		assert_eq!(
			session.observe_worker_pairwise_vision_frame(moved).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: matched.motion_rows,
			}
		);
		assert_eq!(session.export_image().height(), 640 + matched.motion_rows);
		assert_eq!(session.current_viewport_top_y(), i32::try_from(matched.motion_rows).unwrap());
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_commits_successive_browser_like_steps() {
		let frames = [0_u32, 180, 360, 540, 720]
			.into_iter()
			.map(|start_row| make_browser_like_window(512, 640, start_row))
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(frames[0].clone(), 320).unwrap();
		let mut expected_export_height = 640_u32;
		let mut expected_viewport_top_y = 0_i32;

		for window in frames.windows(2) {
			let previous = &window[0];
			let next = window[1].clone();
			let matched = classify_vision_downward_sample_motion_against(previous, &next)
				.expect("pairwise registration should detect each browser-like step");

			assert_eq!(
				session.observe_worker_pairwise_vision_frame(next).unwrap(),
				ScrollObserveOutcome::Committed {
					direction: ScrollDirection::Down,
					growth_rows: matched.motion_rows,
				}
			);

			expected_export_height = expected_export_height.saturating_add(matched.motion_rows);
			expected_viewport_top_y += i32::try_from(matched.motion_rows).unwrap();
		}

		assert_eq!(session.export_image().height(), expected_export_height);
		assert_eq!(session.current_viewport_top_y(), expected_viewport_top_y);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_handles_repeated_browser_like_frame_between_growth_steps() {
		let base = make_browser_like_window(512, 640, 0);
		let step_one = make_browser_like_window(512, 640, 180);
		let step_two = make_browser_like_window(512, 640, 360);
		let first_match = classify_vision_downward_sample_motion_against(&base, &step_one)
			.expect("first browser-like step should register downward motion");
		let followup_match = classify_vision_downward_sample_motion_against(&step_one, &step_two)
			.expect("followup browser-like step should register downward motion");
		let mut session = ScrollSession::new(base, 320).unwrap();

		assert_eq!(
			session.observe_worker_pairwise_vision_frame(step_one.clone()).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: first_match.motion_rows,
			}
		);
		assert_eq!(
			session.observe_worker_pairwise_vision_frame(step_one).unwrap(),
			ScrollObserveOutcome::NoChange
		);
		assert_eq!(
			session.observe_worker_pairwise_vision_frame(step_two).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: followup_match.motion_rows,
			}
		);
		assert_eq!(
			session.export_image().height(),
			640 + first_match.motion_rows + followup_match.motion_rows
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn worker_pairwise_vision_browser_like_followup_uses_adjacent_worker_frame() {
		let base = make_browser_like_window(512, 640, 0);
		let blocked = make_browser_like_window(512, 640, 700);
		let followup = make_browser_like_window(512, 640, 784);
		let matched = classify_vision_downward_sample_motion_against(&blocked, &followup).expect(
			"browser-like pairwise registration should use the immediately previous worker frame",
		);
		let mut session = ScrollSession::new(base, 320).unwrap();

		assert_eq!(
			session.observe_worker_pairwise_vision_frame(blocked).unwrap(),
			ScrollObserveOutcome::NoChange
		);
		assert_eq!(
			session.observe_worker_pairwise_vision_frame(followup).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: matched.motion_rows,
			}
		);
		assert_eq!(session.export_image().height(), 640 + matched.motion_rows);
		assert_eq!(session.current_viewport_top_y(), i32::try_from(matched.motion_rows).unwrap());
	}

	#[test]
	fn session_supports_multiple_downward_growth_steps() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), 7);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 6), &Rgba([70, 0, 0, 255]));
	}

	#[test]
	fn downward_hot_path_falls_back_when_scroll_step_grows() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
			[90, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 4, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 3 }
		);
		assert_eq!(session.export_image().height(), 9);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 8), &Rgba([90, 0, 0, 255]));
	}

	#[test]
	fn session_reports_upward_motion_without_growing() {
		let base = make_test_image(
			3,
			&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
		);
		let moved = make_test_image(
			3,
			&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();
		let outcome = session.observe_downward_sample(moved).unwrap();

		assert!(matches!(
			outcome,
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.export_image(), &base);
	}

	#[test]
	fn pure_upward_sequence_never_commits_growth() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
			[90, 0, 0, 255],
			[100, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 5, 5), 320).unwrap();
		let initial_height = session.export_image().height();

		for start_row in (0..5).rev() {
			assert!(matches!(
				session.observe_downward_sample(make_window(&document, 3, start_row, 5)).unwrap(),
				ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
					| ScrollObserveOutcome::PreviewUpdated
					| ScrollObserveOutcome::NoChange
			));
			assert_eq!(session.export_image().height(), initial_height);
		}
	}

	#[test]
	fn low_information_motion_does_not_commit_growth() {
		let base = make_test_image(
			3,
			&[[10, 0, 0, 255], [10, 0, 0, 255], [11, 0, 0, 255], [11, 0, 0, 255], [12, 0, 0, 255]],
		);
		let moved = make_test_image(
			3,
			&[[10, 0, 0, 255], [11, 0, 0, 255], [11, 0, 0, 255], [12, 0, 0, 255], [12, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();
		let outcome = session.observe_downward_sample(moved).unwrap();

		assert!(matches!(
			outcome,
			ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
				| ScrollObserveOutcome::UnsupportedDirection { .. }
		));
		assert_eq!(session.export_image(), &base);
	}

	#[test]
	fn session_commits_growth_with_sparse_informative_columns() {
		let base = make_sparse_textlike_window(256, 120, 0);
		let moved = make_sparse_textlike_window(256, 120, 9);
		let mut session = ScrollSession::new(base, 320).unwrap();
		let outcome = session.observe_downward_sample(moved).unwrap();

		assert_eq!(
			outcome,
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 9 }
		);
		assert_eq!(session.export_image().height(), 129);
	}

	#[test]
	fn session_commits_growth_with_sparse_columns_and_moving_edge_scrollbar() {
		let base = make_sparse_textlike_window_with_moving_edge_scrollbar(256, 120, 0, 8);
		let moved = make_sparse_textlike_window_with_moving_edge_scrollbar(256, 120, 9, 40);
		let mut session = ScrollSession::new(base, 320).unwrap();
		let outcome = session.observe_downward_sample(moved).unwrap();

		assert_eq!(
			outcome,
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 9 }
		);
		assert_eq!(session.export_image().height(), 129);
	}

	#[test]
	fn repeated_periodic_content_fails_closed_when_downward_registration_is_ambiguous() {
		let document: Vec<[u8; 4]> = (0..256)
			.map(|row| {
				let bucket = (row % 32) as u8;

				[
					bucket.saturating_mul(7),
					255_u8.saturating_sub(bucket.saturating_mul(3)),
					bucket.saturating_mul(5),
					255,
				]
			})
			.collect();
		let base = make_window(&document, 8, 0, 96);
		let moved = make_window(&document, 8, 24, 96);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();

		assert!(matches!(
			session.observe_downward_sample(moved).unwrap(),
			ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.export_image(), &base);
		assert_eq!(session.current_viewport_top_y, 0);
	}

	#[test]
	fn sparse_textlike_small_downward_steps_eventually_append() {
		let base = make_sparse_textlike_window(256, 120, 0);
		let mut session = ScrollSession::new(base, 320).unwrap();
		let initial_height = session.export_image().height();
		let mut committed = 0_u32;

		for start_row in 1..=8 {
			if matches!(
				session
					.observe_downward_sample(make_sparse_textlike_window(256, 120, start_row))
					.unwrap(),
				ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, .. }
			) {
				committed = committed.saturating_add(1);
			}
		}

		assert!(committed > 0);
		assert!(session.export_image().height() > initial_height);
	}

	#[test]
	fn observed_sample_requires_meaningful_overlap_before_committing_large_motion() {
		let document = (0_u16..320)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let base = make_window(&document, 3, 0, 160);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();

		session.last_motion_rows_hint = Some(128);

		let far = make_window(&document, 3, 130, 160);
		let export_before = session.export_image().clone();
		let preview_before = session.preview_image().clone();

		assert!(matches!(
			session.observe_downward_sample(far).unwrap(),
			ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.export_image(), &export_before);
		assert_eq!(session.preview_image(), &preview_before);
		assert_eq!(session.current_viewport_top_y, 0);
	}

	#[test]
	fn periodic_far_downward_frame_does_not_use_full_range_fallback_after_local_miss() {
		let document = (0_u16..128)
			.map(|row| {
				let phase = (row % 40) as u8;

				[phase.saturating_mul(5), phase.saturating_mul(7), phase.saturating_mul(11), 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 9, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 9 }
		);

		let far = make_window(&document, 3, 40, 48);
		let match_eval = session.diagnose_reference_overlap_direction(
			&session.last_sample_frame,
			&far,
			ScrollDirection::Down,
			session.last_motion_rows_hint,
		);

		assert_eq!(session.last_motion_rows_hint, Some(9));
		assert!(match_eval.preferred_only_match.is_none());
		assert!(match_eval.final_match.is_none());
		assert!(!match_eval.used_full_range_fallback);

		let export_before = session.export_image().clone();
		let preview_before = session.preview_image().clone();
		let outcome = session.observe_downward_sample(far).unwrap();

		assert!(matches!(
			outcome,
			ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
				| ScrollObserveOutcome::UnsupportedDirection { .. }
		));
		assert_eq!(session.export_image(), &export_before);
		assert_eq!(session.preview_image(), &preview_before);
	}

	#[test]
	fn committed_growth_rewrites_motion_hint_to_actual_growth_rows() {
		let document = (0_u16..160)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 20, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
		);
		assert_eq!(session.last_motion_rows_hint, Some(20));
		assert_eq!(
			session
				.observe_downward_growth_to_viewport(
					make_window(&document, 3, 24, 48),
					24,
					true,
					Some(MotionObservation { direction: ScrollDirection::Down, motion_rows: 64 }),
					"test_residual_growth_rewrites_hint",
				)
				.unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 4 }
		);
		assert_eq!(session.last_motion_rows_hint, Some(4));
	}

	#[test]
	fn hinted_downward_registration_does_not_escape_to_far_full_range_match() {
		let document = (0_u16..320)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let previous = make_window(&document, 3, 0, 160);
		let next = make_window(&document, 3, 100, 160);
		let session = ScrollSession::new(previous.clone(), 320).unwrap();

		assert!(matches!(
			session.evaluate_reference_downward_registration(&previous, &next, None, true),
			DownwardRegistration::Matched(DirectionMatch { motion_rows: 100, .. })
		));
		assert_eq!(
			session.evaluate_reference_downward_registration(&previous, &next, Some(20), true),
			DownwardRegistration::NoMatch
		);
	}

	#[test]
	fn active_preview_helpers_stay_committed_even_with_provisional_like_session_state() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let base = make_window(&document, 3, 0, 5);
		let latest = make_window(&document, 3, 1, 5);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();

		session.last_sample_frame = latest.clone();
		session.observed_viewport_top_y = 1;

		assert_eq!(session.preview_display_mode(), "committed");
		assert_eq!(session.preview_display_image(), session.export_image().clone());
	}

	#[test]
	fn upward_motion_does_not_reset_downward_progress() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 0, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		let resume_outcome =
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap();
		assert!(matches!(
			resume_outcome,
			ScrollObserveOutcome::NoChange
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
		));
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), 8);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
	}

	#[test]
	fn upward_input_never_commits_lower_frame_and_does_not_advance_frontier() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);

		let height_after_first_append = session.export_image().height();

		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.export_image().height(), height_after_first_append);
		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange
		));
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
	}

	#[test]
	fn upward_rewind_blocks_partial_downward_recovery_until_baseline() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 0, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));

		let height_after_upward_rewind = session.export_image().height();

		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::NoChange
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
		));
		assert_eq!(session.export_image().height(), height_after_upward_rewind);
		let partial_resume_outcome =
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap();
		assert!(matches!(
			partial_resume_outcome,
			ScrollObserveOutcome::NoChange
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
		));
		assert_eq!(session.export_image().height(), height_after_upward_rewind);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
	}

	#[test]
	fn returning_below_last_committed_viewport_does_not_duplicate_growth() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);

		let height_before_resume = session.export_image().height();

		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		let return_outcome =
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap();
		assert!(matches!(
			return_outcome,
			ScrollObserveOutcome::NoChange
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
		));
		assert_eq!(session.export_image().height(), height_before_resume);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), 8);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
	}

	#[test]
	fn downward_input_upward_like_frame_does_not_arm_resume_frontier_or_poison_sample() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);

		let sample_before = session.last_sample_frame.clone();
		let sample_fingerprint_before = session.last_sample_fingerprint.clone();
		let height_before = session.export_image().height();

		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.export_image().height(), height_before);
		assert_eq!(session.current_viewport_top_y, 2);
		assert_eq!(session.observed_viewport_top_y, 2);
		assert_eq!(session.resume_frontier_top_y, None);
		assert!(!session.resume_frontier_requires_reacquire);
		assert_eq!(session.last_sample_frame, sample_before);
		assert_eq!(session.last_sample_fingerprint, sample_fingerprint_before);
	}

	#[test]
	fn viewport_selection_fails_closed_when_observed_and_committed_authority_conflict() {
		let observed = DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::ObservedSample,
			viewport_top_y: 120,
			motion_rows: 20,
			mean_abs_diff_x100: 100,
		};
		let committed = DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::CommittedKeyframe,
			viewport_top_y: 360,
			motion_rows: 260,
			mean_abs_diff_x100: 90,
		};
		let mut candidates = [observed, committed];

		assert_eq!(
			select_downward_viewport_candidate(&mut candidates),
			DownwardViewportResolution::Ambiguous { preferred: committed, competing: observed }
		);
	}

	#[test]
	fn committed_keyframe_candidate_requires_meaningful_overlap() {
		let document = (0_u16..96)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();
		let mut candidates = Vec::new();

		session.push_downward_viewport_candidate(
			&session.anchor_frame,
			0,
			&make_window(&document, 3, 40, 48),
			DownwardViewportCandidateSource::CommittedKeyframe,
			&mut candidates,
		);

		assert!(candidates.is_empty());
	}

	#[test]
	fn committed_fallback_can_recover_from_an_older_recent_keyframe() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_sparse_textlike_window(256, 120, 18)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 18 }
		);
		assert_eq!(
			session.observe_downward_sample(make_sparse_textlike_window(256, 120, 29)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 11 }
		);

		session.last_committed_frame =
			image::RgbaImage::from_pixel(256, 120, Rgba([255, 255, 255, 255]));
		let target = make_sparse_textlike_window(256, 120, 39);
		let mut candidates = Vec::new();

		session.collect_committed_downward_viewport_candidates(&target, &mut candidates);

		assert!(candidates.iter().any(|candidate| {
			candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
				&& candidate.viewport_top_y == 39
		}));
	}

	#[test]
	fn fallback_committed_candidates_ignore_older_recent_keyframes() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_sparse_textlike_window(256, 120, 18)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 18 }
		);
		assert_eq!(
			session.observe_downward_sample(make_sparse_textlike_window(256, 120, 29)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 11 }
		);

		session.last_committed_frame =
			image::RgbaImage::from_pixel(256, 120, Rgba([255, 255, 255, 255]));
		let target = make_sparse_textlike_window(256, 120, 39);
		let mut candidates = Vec::new();

		session.collect_fallback_downward_viewport_candidates(&target, &mut candidates);

		assert!(candidates.is_empty());
	}

	#[test]
	fn fallback_committed_growth_respects_local_continuity_budget() {
		let document = (0_u16..220)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 64), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 20, 64)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
		);

		session.last_motion_rows_hint = Some(2);
		session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
			frame: make_window(&document, 3, 24, 64),
			viewport_top_y: 24,
		});

		assert!(session.fallback_downward_growth_exceeds_continuity_budget(33));
		assert!(!session.fallback_downward_growth_exceeds_continuity_budget(32));
	}

	#[test]
	fn nearby_local_candidate_wins_when_committed_is_only_modestly_better() {
		let observed = DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::ObservedSample,
			viewport_top_y: 132,
			motion_rows: 12,
			mean_abs_diff_x100: 120,
		};
		let committed = DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::CommittedKeyframe,
			viewport_top_y: 130,
			motion_rows: 10,
			mean_abs_diff_x100: 80,
		};
		let mut candidates = [observed, committed];

		assert_eq!(
			select_downward_viewport_candidate(&mut candidates),
			DownwardViewportResolution::Selected(observed)
		);
	}

	#[test]
	fn burst_observed_sample_candidate_is_suppressed_when_it_far_exceeds_local_continuity_budget() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 14;
		session.last_motion_rows_hint = Some(2);
		session.transient_motion_rows_hint = Some(1_219);
		session.transient_burst_search_enabled = true;

		assert!(session.should_suppress_observed_sample_candidate(DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::ObservedSample,
			viewport_top_y: 419,
			motion_rows: 413,
			mean_abs_diff_x100: 0,
		}));
	}

	#[test]
	fn burst_observed_sample_candidate_is_kept_when_it_stays_within_local_continuity_budget() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 14;
		session.last_motion_rows_hint = Some(9);
		session.transient_motion_rows_hint = Some(74);
		session.transient_burst_search_enabled = true;

		assert!(!session.should_suppress_observed_sample_candidate(DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::ObservedSample,
			viewport_top_y: 30,
			motion_rows: 16,
			mean_abs_diff_x100: 0,
		}));
	}

	#[test]
	fn burst_observed_sample_candidate_near_recent_continuity_can_exceed_budget_without_suppression()
	 {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 130;
		session.last_motion_rows_hint = Some(38);
		session.transient_motion_rows_hint = Some(1_150);
		session.transient_burst_search_enabled = true;

		assert!(!session.should_suppress_observed_sample_candidate(DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::ObservedSample,
			viewport_top_y: 162,
			motion_rows: 32,
			mean_abs_diff_x100: 533,
		}));
	}

	#[test]
	fn burst_observed_sample_candidate_near_recent_continuity_still_suppresses_when_diff_is_too_high()
	 {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 14;
		session.last_motion_rows_hint = Some(9);
		session.transient_motion_rows_hint = Some(1_219);
		session.transient_burst_search_enabled = true;

		assert!(session.should_suppress_observed_sample_candidate(DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::ObservedSample,
			viewport_top_y: 31,
			motion_rows: 17,
			mean_abs_diff_x100: 733,
		}));
	}

	#[test]
	fn corroborated_observed_candidate_can_recover_after_initial_continuity_suppression() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 149;
		session.last_motion_rows_hint = Some(16);
		session.transient_motion_rows_hint = Some(12);
		session.transient_burst_search_enabled = true;

		let candidate = DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::ObservedSample,
			viewport_top_y: 169,
			motion_rows: 20,
			mean_abs_diff_x100: 0,
		};
		let mut candidates = vec![DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::CommittedKeyframe,
			viewport_top_y: 169,
			motion_rows: 20,
			mean_abs_diff_x100: 0,
		}];

		assert!(session.should_suppress_observed_sample_candidate(candidate));

		session.restore_corroborated_observed_candidate(Some(candidate), &mut candidates);

		assert!(candidates.contains(&candidate));
	}

	#[test]
	fn tiny_observed_recovery_fails_closed_during_large_transient_burst() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 261;
		session.last_motion_rows_hint = Some(24);
		session.transient_motion_rows_hint = Some(86);
		session.transient_burst_search_enabled = true;

		assert!(session.should_fail_closed_tiny_observed_recovery_in_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::ObservedSample,
				viewport_top_y: 263,
				motion_rows: 2,
				mean_abs_diff_x100: 0,
			}
		));
	}

	#[test]
	fn tiny_observed_recovery_does_not_block_when_recent_continuity_is_small() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 14;
		session.last_motion_rows_hint = Some(2);
		session.transient_motion_rows_hint = Some(1_217);
		session.transient_burst_search_enabled = true;

		assert!(!session.should_fail_closed_tiny_observed_recovery_in_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::ObservedSample,
				viewport_top_y: 15,
				motion_rows: 1,
				mean_abs_diff_x100: 0,
			}
		));
	}

	#[test]
	fn outsized_observed_recovery_after_one_pixel_preview_local_commit_fails_closed() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 74;
		session.last_motion_rows_hint = Some(1);
		session.transient_motion_rows_hint = Some(277);
		session.transient_burst_search_enabled = true;
		session.growth_history.push(super::GrowthCommit {
			frame: make_sparse_textlike_window(256, 120, 74),
			growth_rows: 1,
			viewport_top_y: 74,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(1),
			effective_motion_rows_hint: Some(277),
		});

		assert!(
			session
				.should_fail_closed_outsized_observed_recovery_after_one_pixel_preview_local_commit(
					DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::ObservedSample,
						viewport_top_y: 82,
						motion_rows: 8,
						mean_abs_diff_x100: 0,
					},
				)
		);
	}

	#[test]
	fn tiny_observed_burst_block_keeps_preview_local_baseline_stable() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 261;
		session.observed_viewport_top_y = 261;
		session.last_motion_rows_hint = Some(24);
		session.transient_motion_rows_hint = Some(86);
		session.transient_burst_search_enabled = true;

		session.refresh_preview_only_downward_local_sample(
			&make_sparse_textlike_window(256, 120, 261),
			session.stable_preview_only_downward_local_viewport_top_y(),
		);

		assert_eq!(
			session
				.last_preview_only_downward_local_sample
				.as_ref()
				.map(|sample| sample.viewport_top_y),
			Some(261)
		);
	}

	#[test]
	fn tiny_preview_only_local_recovery_fails_closed_during_large_transient_burst() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(7);
		session.transient_motion_rows_hint = Some(167);
		session.transient_burst_search_enabled = true;

		assert!(session.should_fail_closed_tiny_preview_only_local_recovery_in_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 303,
				motion_rows: 1,
				mean_abs_diff_x100: 232,
			}
		));
	}

	#[test]
	fn tiny_preview_only_local_recovery_does_not_block_recorded_small_commit() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(2);
		session.transient_motion_rows_hint = Some(1_217);
		session.transient_burst_search_enabled = true;

		assert!(!session.should_fail_closed_tiny_preview_only_local_recovery_in_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 15,
				motion_rows: 1,
				mean_abs_diff_x100: 97,
			}
		));
	}

	#[test]
	fn small_preview_only_local_recovery_lagging_recent_continuity_fails_closed_during_burst() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(26);
		session.transient_motion_rows_hint = Some(356);
		session.transient_burst_search_enabled = true;

		assert!(session.should_fail_closed_tiny_preview_only_local_recovery_in_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 84,
				motion_rows: 6,
				mean_abs_diff_x100: 0,
			}
		));
	}

	#[test]
	fn preview_only_local_tail_after_unresolved_burst_fails_closed() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 360;
		session.last_block_reason = Some("no_downward_viewport_candidate_resolved");
		session.last_motion_rows_hint = Some(9);
		session.transient_motion_rows_hint = Some(1_002);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
			frame: make_sparse_textlike_window(256, 120, 360),
			viewport_top_y: 360,
		});

		assert!(session.should_fail_closed_preview_only_local_tail_after_unresolved_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 378,
				motion_rows: 18,
				mean_abs_diff_x100: 0,
			}
		));
	}

	#[test]
	fn preview_only_local_tail_after_unresolved_burst_does_not_block_without_extreme_gap() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 360;
		session.last_block_reason = Some("no_downward_viewport_candidate_resolved");
		session.last_motion_rows_hint = Some(9);
		session.transient_motion_rows_hint = Some(18);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
			frame: make_sparse_textlike_window(256, 120, 360),
			viewport_top_y: 360,
		});

		assert!(!session.should_fail_closed_preview_only_local_tail_after_unresolved_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 378,
				motion_rows: 18,
				mean_abs_diff_x100: 0,
			}
		));
	}

	#[test]
	fn preview_only_local_tail_after_unresolved_burst_does_not_block_after_registered_growth_matches_pending_band()
	 {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 184;
		session.last_block_reason = Some("no_downward_viewport_candidate_resolved");
		session.last_motion_rows_hint = Some(1);
		session.transient_motion_rows_hint = Some(277);
		session.transient_burst_search_enabled = true;
		session.pending_unresolved_burst_registered_growth_viewport_top_y = Some(461);
		session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
			frame: make_sparse_textlike_window(256, 120, 184),
			viewport_top_y: 184,
		});

		assert!(!session.should_fail_closed_preview_only_local_tail_after_unresolved_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 186,
				motion_rows: 2,
				mean_abs_diff_x100: 125,
			}
		));
	}

	#[test]
	fn exactly_corroborated_preview_local_tail_fails_closed_in_extreme_transient_burst() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(10);
		session.transient_motion_rows_hint = Some(1_057);
		session.transient_burst_search_enabled = true;
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(20);
		session.last_downward_viewport_candidates_before_prune =
			Some("PreviewOnlyLocalSample@472/20:0,CommittedKeyframe@472/20:0".to_string());
		for (viewport_top_y, growth_rows) in [(442_i32, 8_u32), (452_i32, 10_u32)] {
			session.growth_history.push(super::GrowthCommit {
				frame: make_sparse_textlike_window(
					256,
					120,
					u32::try_from(viewport_top_y).unwrap(),
				),
				growth_rows,
				viewport_top_y,
				decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
					.decision_source(),
				detected_motion_rows: Some(growth_rows),
				effective_motion_rows_hint: Some(1_057),
			});
		}

		assert!(
			session.should_fail_closed_exactly_corroborated_preview_local_tail_in_extreme_burst(
				DownwardViewportCandidate {
					source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
					viewport_top_y: 472,
					motion_rows: 20,
					mean_abs_diff_x100: 0,
				},
			)
		);
	}

	#[test]
	fn moderate_transient_preview_local_tail_is_not_blocked_by_extreme_burst_rule() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(20);
		session.transient_motion_rows_hint = Some(110);
		session.transient_burst_search_enabled = true;
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(24);
		session.last_downward_viewport_candidates_before_prune =
			Some("PreviewOnlyLocalSample@261/24:329,CommittedKeyframe@512/275:460".to_string());
		session.growth_history.push(super::GrowthCommit {
			frame: make_sparse_textlike_window(256, 120, 237),
			growth_rows: 20,
			viewport_top_y: 237,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(20),
			effective_motion_rows_hint: Some(110),
		});
		session.growth_history.push(super::GrowthCommit {
			frame: make_sparse_textlike_window(256, 120, 217),
			growth_rows: 18,
			viewport_top_y: 217,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(18),
			effective_motion_rows_hint: Some(104),
		});

		assert!(
			!session.should_fail_closed_exactly_corroborated_preview_local_tail_in_extreme_burst(
				DownwardViewportCandidate {
					source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
					viewport_top_y: 261,
					motion_rows: 24,
					mean_abs_diff_x100: 329,
				},
			)
		);
	}

	#[test]
	fn burst_prefers_observed_sample_over_underconsumed_preview_only_local_recovery() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(38);
		session.transient_motion_rows_hint = Some(1_150);
		session.transient_burst_search_enabled = true;

		let primary = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 120, motion_rows: 32 },
			source: DownwardSampleMatchSource::ObservedSample,
		};
		let local = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 8 },
			source: DownwardSampleMatchSource::PreviewOnlyLocalSample,
		};

		assert!(
			session.should_prefer_observed_sample_over_preview_only_local_recovery(primary, local)
		);
	}

	#[test]
	fn burst_keeps_preview_only_local_recovery_when_observed_is_only_modestly_ahead() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(38);
		session.transient_motion_rows_hint = Some(1_150);
		session.transient_burst_search_enabled = true;

		let primary = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 120, motion_rows: 16 },
			source: DownwardSampleMatchSource::ObservedSample,
		};
		let local = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 8 },
			source: DownwardSampleMatchSource::PreviewOnlyLocalSample,
		};

		assert!(
			!session.should_prefer_observed_sample_over_preview_only_local_recovery(primary, local)
		);
	}

	#[test]
	fn tiny_recent_continuity_burst_prefers_preview_local_over_far_ahead_observed_sample() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(2);
		session.transient_motion_rows_hint = Some(225);
		session.transient_burst_search_enabled = true;

		let primary = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 12 },
			source: DownwardSampleMatchSource::ObservedSample,
		};
		let local = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 405, motion_rows: 3 },
			source: DownwardSampleMatchSource::PreviewOnlyLocalSample,
		};

		assert!(
			session.should_prefer_preview_only_local_recovery_over_observed_sample(primary, local)
		);
	}

	#[test]
	fn tiny_recent_continuity_burst_does_not_force_preview_local_when_observed_is_still_nearby() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(2);
		session.transient_motion_rows_hint = Some(225);
		session.transient_burst_search_enabled = true;

		let primary = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 6 },
			source: DownwardSampleMatchSource::ObservedSample,
		};
		let local = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 405, motion_rows: 3 },
			source: DownwardSampleMatchSource::PreviewOnlyLocalSample,
		};

		assert!(
			!session.should_prefer_preview_only_local_recovery_over_observed_sample(primary, local)
		);
	}

	#[test]
	fn tiny_recent_continuity_burst_does_not_force_one_pixel_preview_local_recovery() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(2);
		session.transient_motion_rows_hint = Some(1_211);
		session.transient_burst_search_enabled = true;

		let primary = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 553, motion_rows: 413 },
			source: DownwardSampleMatchSource::ObservedSample,
		};
		let local = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 97, motion_rows: 1 },
			source: DownwardSampleMatchSource::PreviewOnlyLocalSample,
		};

		assert!(
			!session.should_prefer_preview_only_local_recovery_over_observed_sample(primary, local)
		);
	}

	#[test]
	fn repeated_missing_burst_frames_can_prefer_one_pixel_preview_local_recovery() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(2);
		session.transient_motion_rows_hint = Some(277);
		session.transient_burst_search_enabled = true;
		session.consecutive_transient_burst_missing_downward_candidate_frames = 2;

		let primary = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 116 },
			source: DownwardSampleMatchSource::ObservedSample,
		};
		let local = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 149, motion_rows: 1 },
			source: DownwardSampleMatchSource::PreviewOnlyLocalSample,
		};

		assert!(
			session.should_prefer_preview_only_local_recovery_over_observed_sample(primary, local)
		);
	}

	#[test]
	fn preview_local_slowdown_followup_can_prefer_one_pixel_preview_local_recovery() {
		let previous = make_sparse_textlike_window(256, 120, 16);
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(4);
		session.transient_motion_rows_hint = Some(29);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample =
			Some(PreviewOnlyDownwardLocalSample { frame: previous.clone(), viewport_top_y: 145 });
		session.growth_history.push(GrowthCommit {
			frame: previous,
			growth_rows: 4,
			viewport_top_y: 145,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(4),
			effective_motion_rows_hint: Some(8),
		});

		let primary = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 41 },
			source: DownwardSampleMatchSource::ObservedSample,
		};
		let local = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 410, motion_rows: 1 },
			source: DownwardSampleMatchSource::PreviewOnlyLocalSample,
		};

		assert!(
			session.should_prefer_preview_only_local_recovery_over_observed_sample(primary, local)
		);
	}

	#[test]
	fn preview_local_slowdown_followup_can_prefer_near_continuity_preview_local_recovery() {
		let previous = make_sparse_textlike_window(256, 120, 16);
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(10);
		session.transient_motion_rows_hint = Some(1_150);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample =
			Some(PreviewOnlyDownwardLocalSample { frame: previous.clone(), viewport_top_y: 416 });
		session.growth_history.push(GrowthCommit {
			frame: previous,
			growth_rows: 10,
			viewport_top_y: 416,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(10),
			effective_motion_rows_hint: Some(10),
		});

		let primary = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 158 },
			source: DownwardSampleMatchSource::ObservedSample,
		};
		let local = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 697, motion_rows: 12 },
			source: DownwardSampleMatchSource::PreviewOnlyLocalSample,
		};

		assert!(
			session.should_prefer_preview_only_local_recovery_over_observed_sample(primary, local)
		);
	}

	#[test]
	fn preview_local_slowdown_followup_without_recent_small_preview_commit_does_not_prefer_local() {
		let previous = make_sparse_textlike_window(256, 120, 16);
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(4);
		session.transient_motion_rows_hint = Some(29);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample =
			Some(PreviewOnlyDownwardLocalSample { frame: previous.clone(), viewport_top_y: 145 });
		session.growth_history.push(GrowthCommit {
			frame: previous,
			growth_rows: 12,
			viewport_top_y: 145,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(12),
			effective_motion_rows_hint: Some(12),
		});

		let primary = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 41 },
			source: DownwardSampleMatchSource::ObservedSample,
		};
		let local = DownwardSampleMatch {
			matched: DirectionMatch { mean_abs_diff_x100: 410, motion_rows: 1 },
			source: DownwardSampleMatchSource::PreviewOnlyLocalSample,
		};

		assert!(
			!session.should_prefer_preview_only_local_recovery_over_observed_sample(primary, local)
		);
	}

	#[test]
	fn observed_burst_catch_up_commit_seeds_preview_local_baseline() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.transient_motion_rows_hint = Some(1_150);
		session.transient_burst_search_enabled = true;

		assert!(session.should_seed_preview_only_local_after_observed_burst_commit(
			"sample_motion_downward_growth_from_observed_keyframe",
			32,
			Some(38),
		));
	}

	#[test]
	fn non_observed_or_non_catch_up_commit_does_not_seed_preview_local_baseline() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.transient_motion_rows_hint = Some(1_150);
		session.transient_burst_search_enabled = true;

		assert!(!session.should_seed_preview_only_local_after_observed_burst_commit(
			"sample_motion_downward_growth_from_committed_keyframe",
			32,
			Some(38),
		));
		assert!(!session.should_seed_preview_only_local_after_observed_burst_commit(
			"sample_motion_downward_growth_from_observed_keyframe",
			38,
			Some(38),
		));
	}

	#[test]
	fn preview_local_burst_commit_preserves_local_baseline_for_next_frame() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.transient_motion_rows_hint = Some(226);
		session.transient_burst_search_enabled = true;

		assert!(session.should_preserve_preview_only_local_after_preview_only_burst_commit(
			"sample_motion_downward_growth_from_preview_only_local_sample",
			18,
			Some(12),
		));
	}

	#[test]
	fn preview_local_burst_commit_does_not_preserve_local_baseline_for_tiny_or_far_growth() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.transient_motion_rows_hint = Some(226);
		session.transient_burst_search_enabled = true;

		assert!(!session.should_preserve_preview_only_local_after_preview_only_burst_commit(
			"sample_motion_downward_growth_from_preview_only_local_sample",
			1,
			Some(12),
		));
		assert!(!session.should_preserve_preview_only_local_after_preview_only_burst_commit(
			"sample_motion_downward_growth_from_preview_only_local_sample",
			36,
			Some(12),
		));
	}

	#[test]
	fn preview_local_non_burst_small_slowdown_preserves_local_baseline_for_next_frame() {
		let session = ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		assert!(session.should_preserve_preview_only_local_after_preview_only_burst_commit(
			"sample_motion_downward_growth_from_preview_only_local_sample",
			4,
			Some(8),
		));
	}

	#[test]
	fn preview_local_non_burst_tiny_or_growing_commit_does_not_preserve_local_baseline() {
		let session = ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		assert!(!session.should_preserve_preview_only_local_after_preview_only_burst_commit(
			"sample_motion_downward_growth_from_preview_only_local_sample",
			1,
			Some(8),
		));
		assert!(!session.should_preserve_preview_only_local_after_preview_only_burst_commit(
			"sample_motion_downward_growth_from_preview_only_local_sample",
			10,
			Some(8),
		));
	}

	#[test]
	fn corroborated_huge_local_jump_after_preview_local_commit_blocks_far_committed_only_recovery()
	{
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 230;
		session.last_motion_rows_hint = Some(18);
		session.transient_motion_rows_hint = Some(226);
		session.transient_burst_search_enabled = true;
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(164);
		session.last_preview_only_local_registration_result = Some("matched");
		session.last_preview_only_local_registration_motion_rows = Some(164);
		session.growth_history.push(super::GrowthCommit {
			frame: make_sparse_textlike_window(256, 120, 230),
			growth_rows: 18,
			viewport_top_y: 230,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(18),
			effective_motion_rows_hint: Some(226),
		});

		assert!(
			session
				.should_fail_closed_far_committed_only_recovery_after_corroborated_huge_local_jump(
					DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 394,
						motion_rows: 164,
						mean_abs_diff_x100: 0,
					},
					164,
				)
		);
	}

	#[test]
	fn materially_smaller_observed_motion_still_blocks_huge_committed_only_recovery() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 230;
		session.last_motion_rows_hint = Some(18);
		session.transient_motion_rows_hint = Some(282);
		session.transient_burst_search_enabled = true;
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(112);
		session.last_preview_only_local_registration_result = Some("matched");
		session.last_preview_only_local_registration_motion_rows = Some(276);
		session.growth_history.push(super::GrowthCommit {
			frame: make_sparse_textlike_window(256, 120, 230),
			growth_rows: 18,
			viewport_top_y: 230,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(18),
			effective_motion_rows_hint: Some(282),
		});

		assert!(
			session
				.should_fail_closed_far_committed_only_recovery_after_corroborated_huge_local_jump(
					DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 506,
						motion_rows: 276,
						mean_abs_diff_x100: 0,
					},
					276,
				)
		);
	}

	#[test]
	fn nearby_committed_recovery_is_not_blocked_when_local_jump_is_not_huge() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 230;
		session.last_motion_rows_hint = Some(18);
		session.transient_motion_rows_hint = Some(226);
		session.transient_burst_search_enabled = true;
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(38);
		session.last_preview_only_local_registration_result = Some("matched");
		session.last_preview_only_local_registration_motion_rows = Some(38);
		session.growth_history.push(super::GrowthCommit {
			frame: make_sparse_textlike_window(256, 120, 230),
			growth_rows: 18,
			viewport_top_y: 230,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(18),
			effective_motion_rows_hint: Some(226),
		});

		assert!(
			!session
				.should_fail_closed_far_committed_only_recovery_after_corroborated_huge_local_jump(
					DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 268,
						motion_rows: 38,
						mean_abs_diff_x100: 0,
					},
					38,
				)
		);
	}

	#[test]
	fn suppressed_huge_preview_local_jump_corroborated_by_observed_and_committed_fails_closed() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 230;
		session.last_motion_rows_hint = Some(18);
		session.transient_motion_rows_hint = Some(226);
		session.transient_burst_search_enabled = true;
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(164);
		session.growth_history.push(super::GrowthCommit {
			frame: make_sparse_textlike_window(256, 120, 230),
			growth_rows: 18,
			viewport_top_y: 230,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(18),
			effective_motion_rows_hint: Some(226),
		});

		assert!(
			session
				.should_fail_closed_suppressed_huge_preview_local_jump_corroborated_by_observed_and_committed(
					Some(DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
						viewport_top_y: 394,
						motion_rows: 164,
						mean_abs_diff_x100: 0,
					}),
					&[DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 394,
						motion_rows: 164,
						mean_abs_diff_x100: 0,
					}],
				)
		);
	}

	#[test]
	fn suppressed_preview_local_jump_without_exact_committed_corroboration_stays_unblocked() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 230;
		session.last_motion_rows_hint = Some(18);
		session.transient_motion_rows_hint = Some(226);
		session.transient_burst_search_enabled = true;
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(164);
		session.growth_history.push(super::GrowthCommit {
			frame: make_sparse_textlike_window(256, 120, 230),
			growth_rows: 18,
			viewport_top_y: 230,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(18),
			effective_motion_rows_hint: Some(226),
		});

		assert!(
			!session
				.should_fail_closed_suppressed_huge_preview_local_jump_corroborated_by_observed_and_committed(
					Some(DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
						viewport_top_y: 394,
						motion_rows: 164,
						mean_abs_diff_x100: 0,
					}),
					&[DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 398,
						motion_rows: 186,
						mean_abs_diff_x100: 0,
					}],
				)
		);
	}

	#[test]
	fn committed_followup_after_suppressed_huge_preview_local_jump_fails_closed() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.transient_burst_search_enabled = true;
		session.last_preview_only_local_registration_result = Some("no_match");
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(164);

		assert!(
			session.should_fail_closed_committed_followup_after_suppressed_huge_preview_local_jump(
				Some(DownwardViewportCandidate {
					source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
					viewport_top_y: 394,
					motion_rows: 164,
					mean_abs_diff_x100: 0,
				}),
				&[
					DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 394,
						motion_rows: 164,
						mean_abs_diff_x100: 0,
					},
					DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 398,
						motion_rows: 186,
						mean_abs_diff_x100: 0,
					},
				],
			)
		);
	}

	#[test]
	fn committed_followup_without_pending_suppressed_preview_local_jump_stays_unblocked() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.transient_burst_search_enabled = true;
		session.last_preview_only_local_registration_result = Some("no_match");
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(164);

		assert!(
			!session
				.should_fail_closed_committed_followup_after_suppressed_huge_preview_local_jump(
					None,
					&[DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 394,
						motion_rows: 164,
						mean_abs_diff_x100: 0,
					}],
				)
		);
	}

	#[test]
	fn committed_followup_after_extreme_preview_local_tail_block_fails_closed() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.transient_burst_search_enabled = true;

		assert!(
			session.should_fail_closed_committed_followup_after_extreme_preview_local_tail_block(
				Some(DownwardViewportCandidate {
					source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
					viewport_top_y: 472,
					motion_rows: 20,
					mean_abs_diff_x100: 0,
				}),
				&[
					DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 472,
						motion_rows: 20,
						mean_abs_diff_x100: 0,
					},
					DownwardViewportCandidate {
						source: DownwardViewportCandidateSource::CommittedKeyframe,
						viewport_top_y: 472,
						motion_rows: 30,
						mean_abs_diff_x100: 0,
					},
				],
			)
		);
	}

	#[test]
	fn committed_followup_after_extreme_preview_local_tail_block_ignores_non_exact_match() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.transient_burst_search_enabled = true;

		assert!(
			!session.should_fail_closed_committed_followup_after_extreme_preview_local_tail_block(
				Some(DownwardViewportCandidate {
					source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
					viewport_top_y: 472,
					motion_rows: 20,
					mean_abs_diff_x100: 0,
				}),
				&[DownwardViewportCandidate {
					source: DownwardViewportCandidateSource::CommittedKeyframe,
					viewport_top_y: 472,
					motion_rows: 30,
					mean_abs_diff_x100: 0,
				}],
			)
		);
	}

	#[test]
	fn suppressed_huge_preview_local_followup_block_budget_scales_with_far_recovery_ratio() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();
		session.last_motion_rows_hint = Some(18);

		assert_eq!(
			session.suppressed_huge_preview_only_local_followup_block_budget(Some(
				DownwardViewportCandidate {
					source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
					viewport_top_y: 394,
					motion_rows: 164,
					mean_abs_diff_x100: 0,
				},
			)),
			5
		);
		assert_eq!(
			session.suppressed_huge_preview_only_local_followup_block_budget(Some(
				DownwardViewportCandidate {
					source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
					viewport_top_y: 290,
					motion_rows: 42,
					mean_abs_diff_x100: 0,
				},
			)),
			3
		);
	}

	#[test]
	fn huge_suppressed_jump_window_refreshes_observed_baseline_without_advancing_viewport() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		assert!(!session.should_refresh_downward_observed_baseline_after_huge_suppressed_jump());

		session.pending_suppressed_huge_preview_only_local_followup =
			Some(DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 394,
				motion_rows: 164,
				mean_abs_diff_x100: 0,
			});
		assert!(session.should_refresh_downward_observed_baseline_after_huge_suppressed_jump());

		session.pending_suppressed_huge_preview_only_local_followup = None;
		session.blocked_followup_after_suppressed_huge_preview_local_jump = true;
		assert!(session.should_refresh_downward_observed_baseline_after_huge_suppressed_jump());

		session.blocked_followup_after_suppressed_huge_preview_local_jump = false;
		session.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump = true;
		assert!(session.should_refresh_downward_observed_baseline_after_huge_suppressed_jump());
	}

	#[test]
	fn huge_far_committed_block_resets_preview_only_local_baseline() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.refresh_preview_only_downward_local_sample(
			&make_sparse_textlike_window(256, 120, 32),
			Some(32),
		);
		assert!(session.last_preview_only_downward_local_sample.is_some());
		assert!(!session.should_reset_preview_only_local_baseline_after_huge_far_committed_block());

		session.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump = true;
		assert!(session.should_reset_preview_only_local_baseline_after_huge_far_committed_block());
	}

	#[test]
	fn seeded_preview_only_local_catch_up_candidate_can_commit_small_tail_growth() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 162;
		session.seeded_preview_only_local_after_observed_burst_commit = true;

		assert!(session.seeded_preview_only_local_catch_up_candidate_can_commit(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 170,
				motion_rows: 8,
				mean_abs_diff_x100: 0,
			}
		));
	}

	#[test]
	fn unseeded_preview_only_local_candidate_still_needs_normal_burst_rules() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 162;

		assert!(!session.seeded_preview_only_local_catch_up_candidate_can_commit(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
				viewport_top_y: 170,
				motion_rows: 8,
				mean_abs_diff_x100: 0,
			}
		));
	}

	#[test]
	fn seeded_preview_only_local_recovery_range_includes_one_pixel_tail_growth() {
		let previous = make_sparse_textlike_window(256, 120, 0);
		let next = make_sparse_textlike_window(256, 120, 1);
		let mut session = ScrollSession::new(previous.clone(), 320).unwrap();

		session.last_motion_rows_hint = Some(4);
		session.seeded_preview_only_local_after_observed_burst_commit = true;

		let range = session
			.preview_only_local_recovery_motion_range(
				&previous,
				&next,
				OverlapSearchConfig::default(),
			)
			.unwrap();

		assert_eq!(*range.start(), 1);
		assert_eq!(*range.end(), 6);
	}

	#[test]
	fn unseeded_preview_only_local_recovery_range_keeps_hint_floor() {
		let previous = make_sparse_textlike_window(256, 120, 0);
		let next = make_sparse_textlike_window(256, 120, 1);
		let mut session = ScrollSession::new(previous.clone(), 320).unwrap();

		session.last_motion_rows_hint = Some(4);

		let range = session
			.preview_only_local_recovery_motion_range(
				&previous,
				&next,
				OverlapSearchConfig::default(),
			)
			.unwrap();

		assert_eq!(*range.start(), 2);
		assert_eq!(*range.end(), 6);
	}

	#[test]
	fn preview_local_slowdown_followup_range_allows_one_pixel_tail_in_burst() {
		let previous = make_sparse_textlike_window(256, 120, 16);
		let next = make_sparse_textlike_window(256, 120, 17);
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(4);
		session.transient_motion_rows_hint = Some(29);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample =
			Some(PreviewOnlyDownwardLocalSample { frame: previous.clone(), viewport_top_y: 145 });
		session.growth_history.push(GrowthCommit {
			frame: previous.clone(),
			growth_rows: 4,
			viewport_top_y: 145,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(4),
			effective_motion_rows_hint: Some(8),
		});

		let range = session
			.preview_only_local_recovery_motion_range(
				&previous,
				&next,
				OverlapSearchConfig::default(),
			)
			.unwrap();

		assert_eq!(*range.start(), 1);
		assert_eq!(*range.end(), 6);
	}

	#[test]
	fn preview_local_followup_without_recent_small_preview_commit_keeps_hint_floor_in_burst() {
		let previous = make_sparse_textlike_window(256, 120, 16);
		let next = make_sparse_textlike_window(256, 120, 17);
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(4);
		session.transient_motion_rows_hint = Some(29);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample =
			Some(PreviewOnlyDownwardLocalSample { frame: previous.clone(), viewport_top_y: 145 });
		session.growth_history.push(GrowthCommit {
			frame: previous.clone(),
			growth_rows: 12,
			viewport_top_y: 145,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(12),
			effective_motion_rows_hint: Some(12),
		});

		let range = session
			.preview_only_local_recovery_motion_range(
				&previous,
				&next,
				OverlapSearchConfig::default(),
			)
			.unwrap();

		assert_eq!(*range.start(), 2);
		assert_eq!(*range.end(), 6);
	}

	#[test]
	fn tiny_committed_keyframe_recovery_fails_closed_during_large_transient_burst() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 68;
		session.last_motion_rows_hint = Some(6);
		session.transient_motion_rows_hint = Some(401);
		session.transient_burst_search_enabled = true;

		assert!(session.should_fail_closed_tiny_committed_keyframe_recovery_in_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 70,
				motion_rows: 12,
				mean_abs_diff_x100: 654,
			}
		));
	}

	#[test]
	fn tiny_committed_keyframe_recovery_does_not_block_meaningful_growth_during_burst() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 68;
		session.last_motion_rows_hint = Some(6);
		session.transient_motion_rows_hint = Some(401);
		session.transient_burst_search_enabled = true;

		assert!(!session.should_fail_closed_tiny_committed_keyframe_recovery_in_burst(
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 81,
				motion_rows: 23,
				mean_abs_diff_x100: 696,
			}
		));
	}

	#[test]
	fn underconsumed_observed_recovery_fails_closed_when_nearby_committed_candidate_reaches_recent_continuity()
	 {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(20);
		session.transient_motion_rows_hint = Some(75);
		session.transient_burst_search_enabled = true;

		let candidates_before_prune = vec![
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::ObservedSample,
				viewport_top_y: 289,
				motion_rows: 8,
				mean_abs_diff_x100: 0,
			},
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 289,
				motion_rows: 8,
				mean_abs_diff_x100: 0,
			},
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 291,
				motion_rows: 30,
				mean_abs_diff_x100: 0,
			},
		];
		let candidates_after_prune = vec![
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::ObservedSample,
				viewport_top_y: 289,
				motion_rows: 8,
				mean_abs_diff_x100: 0,
			},
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 289,
				motion_rows: 8,
				mean_abs_diff_x100: 0,
			},
		];

		assert!(session.should_fail_closed_underconsumed_observed_recovery_in_burst(
			&candidates_before_prune,
			&candidates_after_prune,
		));
	}

	#[test]
	fn underconsumed_observed_recovery_does_not_block_small_recorded_burst_commit() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(4);
		session.transient_motion_rows_hint = Some(466);
		session.transient_burst_search_enabled = true;

		let candidates_before_prune = vec![
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::ObservedSample,
				viewport_top_y: 14,
				motion_rows: 2,
				mean_abs_diff_x100: 6,
			},
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 14,
				motion_rows: 2,
				mean_abs_diff_x100: 6,
			},
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 14,
				motion_rows: 6,
				mean_abs_diff_x100: 16,
			},
		];
		let candidates_after_prune = candidates_before_prune[..2].to_vec();

		assert!(!session.should_fail_closed_underconsumed_observed_recovery_in_burst(
			&candidates_before_prune,
			&candidates_after_prune,
		));
	}

	#[test]
	fn low_confidence_committed_only_recovery_without_local_anchor_fails_closed_during_burst() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 134;
		session.last_motion_rows_hint = Some(43);
		session.transient_motion_rows_hint = Some(1_142);
		session.transient_burst_search_enabled = true;

		let candidates = vec![
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 190,
				motion_rows: 56,
				mean_abs_diff_x100: 621,
			},
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 157,
				motion_rows: 73,
				mean_abs_diff_x100: 557,
			},
		];

		assert!(session.should_fail_closed_far_committed_only_recovery_without_local_anchor(
			candidates[1],
			&candidates,
		));
	}

	#[test]
	fn small_continuity_preview_local_registration_blocks_larger_committed_only_recovery() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 62;
		session.last_motion_rows_hint = Some(2);
		session.transient_motion_rows_hint = Some(225);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
			frame: make_sparse_textlike_window(256, 120, 31),
			viewport_top_y: 62,
		});
		session.last_preview_only_local_registration_result = Some("matched");
		session.last_preview_only_local_registration_motion_rows = Some(3);

		let candidates = vec![
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 74,
				motion_rows: 12,
				mean_abs_diff_x100: 0,
			},
			DownwardViewportCandidate {
				source: DownwardViewportCandidateSource::CommittedKeyframe,
				viewport_top_y: 78,
				motion_rows: 14,
				mean_abs_diff_x100: 0,
			},
		];

		assert!(session.should_fail_closed_far_committed_only_recovery_without_local_anchor(
			candidates[0],
			&candidates,
		));
	}

	#[test]
	fn suppressed_large_preview_local_registration_blocks_underconsumed_committed_only_recovery() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 202;
		session.last_motion_rows_hint = Some(8);
		session.transient_motion_rows_hint = Some(575);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
			frame: make_sparse_textlike_window(256, 120, 31),
			viewport_top_y: 202,
		});
		session.last_preview_only_local_registration_result = Some("matched");
		session.last_preview_only_local_registration_motion_rows = Some(272);

		let candidates = vec![DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::CommittedKeyframe,
			viewport_top_y: 220,
			motion_rows: 32,
			mean_abs_diff_x100: 765,
		}];

		assert!(
			session
				.should_fail_closed_underconsumed_committed_only_recovery_after_suppressed_preview_local_match(
					candidates[0],
					session.growth_rows_for_candidate_viewport_top_y(candidates[0].viewport_top_y),
				)
		);
	}

	#[test]
	fn corroborated_sample_registrations_block_committed_only_recovery_without_viewport_anchor() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 237;
		session.last_motion_rows_hint = Some(20);
		session.transient_motion_rows_hint = Some(145);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
			frame: make_sparse_textlike_window(256, 120, 237),
			viewport_top_y: 237,
		});
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(135);
		session.last_preview_only_local_registration_result = Some("matched");
		session.last_preview_only_local_registration_motion_rows = Some(116);

		let preferred = DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::CommittedKeyframe,
			viewport_top_y: 353,
			motion_rows: 116,
			mean_abs_diff_x100: 0,
		};

		assert!(session
			.should_fail_closed_committed_only_recovery_after_corroborated_sample_registration_without_viewport_anchor(
				preferred,
				session.growth_rows_for_candidate_viewport_top_y(preferred.viewport_top_y),
			));
	}

	#[test]
	fn corroborated_sample_registrations_block_older_keyframe_recovery_by_growth_band() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 237;
		session.last_motion_rows_hint = Some(20);
		session.transient_motion_rows_hint = Some(249);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
			frame: make_sparse_textlike_window(256, 120, 237),
			viewport_top_y: 237,
		});
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(258);
		session.last_preview_only_local_registration_result = Some("matched");
		session.last_preview_only_local_registration_motion_rows = Some(180);

		let preferred = DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::CommittedKeyframe,
			viewport_top_y: 464,
			motion_rows: 271,
			mean_abs_diff_x100: 700,
		};

		assert!(session
			.should_fail_closed_committed_only_recovery_after_corroborated_sample_registration_without_viewport_anchor(
				preferred,
				session.growth_rows_for_candidate_viewport_top_y(preferred.viewport_top_y),
			));
	}

	#[test]
	fn observed_burst_outpacing_recent_preview_local_commit_blocks_committed_only_recovery() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 237;
		session.last_motion_rows_hint = Some(20);
		session.transient_motion_rows_hint = Some(145);
		session.transient_burst_search_enabled = true;
		session.last_observed_sample_registration_result = Some("matched");
		session.last_observed_sample_registration_motion_rows = Some(135);
		session.last_preview_only_local_registration_result = Some("no_match");
		session.growth_history.push(super::GrowthCommit {
			frame: make_sparse_textlike_window(256, 120, 237),
			growth_rows: 20,
			viewport_top_y: 237,
			decision_source: DownwardViewportCandidateSource::PreviewOnlyLocalSample
				.decision_source(),
			detected_motion_rows: Some(20),
			effective_motion_rows_hint: Some(145),
		});

		let preferred = DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::CommittedKeyframe,
			viewport_top_y: 353,
			motion_rows: 116,
			mean_abs_diff_x100: 0,
		};

		assert!(session
			.should_fail_closed_committed_only_recovery_when_observed_burst_outpaces_recent_preview_local_commit(
				preferred,
				session.growth_rows_for_candidate_viewport_top_y(preferred.viewport_top_y),
			));
	}

	#[test]
	fn suppressed_large_preview_local_registration_helper_skips_hint_band_committed_recovery() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.current_viewport_top_y = 202;
		session.last_motion_rows_hint = Some(8);
		session.transient_motion_rows_hint = Some(575);
		session.transient_burst_search_enabled = true;
		session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
			frame: make_sparse_textlike_window(256, 120, 31),
			viewport_top_y: 202,
		});
		session.last_preview_only_local_registration_result = Some("matched");
		session.last_preview_only_local_registration_motion_rows = Some(272);

		let candidates = vec![DownwardViewportCandidate {
			source: DownwardViewportCandidateSource::CommittedKeyframe,
			viewport_top_y: 500,
			motion_rows: 310,
			mean_abs_diff_x100: 0,
		}];

		assert!(
			!session
				.should_fail_closed_underconsumed_committed_only_recovery_after_suppressed_preview_local_match(
					candidates[0],
					session.growth_rows_for_candidate_viewport_top_y(candidates[0].viewport_top_y),
				)
		);
	}

	#[test]
	fn weak_tiny_committed_keyframe_match_retries_full_range_during_burst() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(14);
		session.transient_motion_rows_hint = Some(380);
		session.transient_burst_search_enabled = true;

		assert!(session.should_retry_committed_keyframe_registration_across_full_range(
			DownwardRegistration::Matched(DirectionMatch {
				mean_abs_diff_x100: 733,
				motion_rows: 7,
			}),
		));
		assert_eq!(
			session.prefer_full_range_committed_keyframe_registration(
				DownwardRegistration::Matched(DirectionMatch {
					mean_abs_diff_x100: 733,
					motion_rows: 7,
				}),
				DownwardRegistration::Matched(DirectionMatch {
					mean_abs_diff_x100: 0,
					motion_rows: 50,
				}),
			),
			DownwardRegistration::Matched(DirectionMatch {
				mean_abs_diff_x100: 0,
				motion_rows: 50,
			}),
		);
	}

	#[test]
	fn modest_committed_keyframe_match_does_not_retry_full_range_during_burst() {
		let mut session =
			ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

		session.last_motion_rows_hint = Some(9);
		session.transient_motion_rows_hint = Some(1_284);
		session.transient_burst_search_enabled = true;

		assert!(!session.should_retry_committed_keyframe_registration_across_full_range(
			DownwardRegistration::Matched(DirectionMatch {
				mean_abs_diff_x100: 301,
				motion_rows: 27,
			}),
		));
	}

	#[test]
	fn session_preview_matches_export_after_downward_growth() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 4), 3).unwrap();
		let _ = session.observe_downward_sample(make_window(&document, 3, 1, 4)).unwrap();
		let _ = session.observe_downward_sample(make_window(&document, 3, 2, 4)).unwrap();

		assert_eq!(session.preview_image().height(), session.export_image().height());
		assert_eq!(session.preview_image().get_pixel(0, 0), session.export_image().get_pixel(0, 0));
		assert_eq!(
			session.preview_image().get_pixel(0, session.preview_image().height() - 1),
			session.export_image().get_pixel(0, session.export_image().height() - 1)
		);
	}

	#[test]
	fn session_undo_restores_previous_stitched_image() {
		let base = make_test_image(
			3,
			&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
		);
		let moved = make_test_image(
			3,
			&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(moved).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert!(session.undo_last_append());
		assert_eq!(session.export_image(), &base);
	}
}
