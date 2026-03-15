pub mod bench_support {
	use image::{Rgba, RgbaImage, imageops};

	use crate::scroll_capture::{
		OverlapSearchConfig, ScrollDirection, ScrollObserveOutcome, ScrollSession,
		evaluate_overlap_direction, max_directional_motion_rows, scroll_capture_fingerprint,
	};

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub enum ScrollCaptureBenchScenario {
		Baseline,
		Wide,
	}

	impl ScrollCaptureBenchScenario {
		pub const ALL: [Self; 2] = [Self::Baseline, Self::Wide];

		#[must_use]
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
	pub struct ScrollCaptureFingerprintMetrics {
		pub byte_len: usize,
		pub checksum: u32,
	}

	#[derive(Clone, Copy, Debug, Default)]
	pub struct ScrollCaptureOverlapMetrics {
		pub matched: bool,
		pub motion_rows: u32,
		pub overlap_rows: u32,
		pub mean_abs_diff_x100: u32,
	}

	#[derive(Clone, Copy, Debug, Default)]
	pub struct ScrollCaptureSessionMetrics {
		pub committed: bool,
		pub growth_rows: u32,
		pub export_height: u32,
		pub preview_height: u32,
	}

	pub struct ScrollCaptureBenchHarness {
		fixture: ScrollCaptureBenchFixture,
		overlap_config: OverlapSearchConfig,
	}

	impl ScrollCaptureBenchHarness {
		#[must_use]
		pub fn new(scenario: ScrollCaptureBenchScenario) -> Self {
			Self {
				fixture: ScrollCaptureBenchFixture::new(scenario.spec()),
				overlap_config: OverlapSearchConfig::default(),
			}
		}

		#[must_use]
		pub fn run_fingerprint(&self) -> ScrollCaptureFingerprintMetrics {
			let bytes = scroll_capture_fingerprint(&self.fixture.fingerprint_frame);

			ScrollCaptureFingerprintMetrics {
				byte_len: bytes.len(),
				checksum: checksum_bytes(&bytes),
			}
		}

		#[must_use]
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

const FINGERPRINT_GRID_COLUMNS: u32 = 12;
const FINGERPRINT_GRID_ROWS: u32 = 16;
const DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS: u32 = 12;
const INITIAL_DOWNWARD_MAX_MOTION_ROWS: u32 = 192;
const MOTION_SEARCH_BAND_ROWS: u32 = 96;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverlapMatch {
	pub(crate) rows: u32,
	pub(crate) matched: bool,
	pub(crate) mean_abs_diff_x100: u32,
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
			max_row_samples: 8,
			max_mean_abs_diff_x100: 850,
		}
	}
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
	last_sample_frame: RgbaImage,
	last_sample_fingerprint: Option<Vec<u8>>,
	last_unconfirmed_upward_fingerprint: Option<Vec<u8>>,
	last_motion_rows_hint: Option<u32>,
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
			last_sample_frame: base_frame,
			last_sample_fingerprint: Some(fingerprint),
			last_unconfirmed_upward_fingerprint: None,
			last_motion_rows_hint: None,
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
		self.observe_sample(frame, ScrollDirection::Down)
	}

	pub(crate) fn observe_upward_sample(
		&mut self,
		frame: RgbaImage,
	) -> Result<ScrollObserveOutcome> {
		self.observe_sample(frame, ScrollDirection::Up)
	}

	fn observe_sample(
		&mut self,
		frame: RgbaImage,
		input_direction: ScrollDirection,
	) -> Result<ScrollObserveOutcome> {
		if frame.width() != self.anchor_frame.width() {
			return Err(eyre::eyre!(
				"frame width mismatch: expected {} got {}",
				self.anchor_frame.width(),
				frame.width()
			));
		}

		let fingerprint = scroll_capture_fingerprint(&frame);

		if matches!(input_direction, ScrollDirection::Up)
			&& self.resume_frontier_top_y.is_some()
			&& self.last_unconfirmed_upward_fingerprint.as_deref() == Some(fingerprint.as_slice())
		{
			return Ok(ScrollObserveOutcome::NoChange);
		}

		let sample_delta = self
			.last_sample_fingerprint
			.as_ref()
			.map(|previous| scroll_capture_fingerprint_delta(previous, &fingerprint));
		let sample_motion = self.classify_sample_motion(&frame);
		let preview_changed =
			sample_delta.is_some_and(|delta| delta > 0) || sample_motion.is_some();

		if !preview_changed {
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

		let previous_sample_frame = self.last_sample_frame.clone();
		let previous_sample_fingerprint = self.last_sample_fingerprint.clone();

		self.last_unconfirmed_upward_fingerprint = None;

		self.record_last_sample(&frame, fingerprint);

		if let Some(motion) = sample_motion {
			match motion.direction {
				ScrollDirection::Up => {
					let committed_down_match = self.evaluate_reference_overlap_direction(
						&self.last_committed_frame,
						&frame,
						ScrollDirection::Down,
						self.last_motion_rows_hint,
					);
					let committed_up_match = self.evaluate_reference_overlap_direction(
						&self.last_committed_frame,
						&frame,
						ScrollDirection::Up,
						self.last_motion_rows_hint,
					);

					if let Some(up_match) = upward_confirmation_match_for_downward_input(
						committed_up_match,
						committed_down_match,
						self.current_viewport_top_y > 0,
					) {
						self.observe_upward_rewind_from_committed(up_match.motion_rows);
						self.log_decision(
							"scroll_capture.down_input_detected_upward_motion",
							input_direction,
							Some(MotionObservation {
								direction: ScrollDirection::Up,
								motion_rows: up_match.motion_rows,
							}),
							None,
							None,
							Some(
								"downward_input_confirmed_upward_motion_with_last_committed_match",
							),
						);

						return Ok(ScrollObserveOutcome::UnsupportedDirection {
							direction: ScrollDirection::Up,
						});
					}

					self.log_decision(
						"scroll_capture.down_input_detected_upward_motion",
						input_direction,
						Some(motion),
						None,
						None,
						Some("downward_input_upward_motion_lacked_committed_support"),
					);

					return Ok(preview_update_outcome(preview_changed));
				},
				ScrollDirection::Down => {
					return self.observe_downward_motion(
						frame,
						motion.motion_rows,
						preview_changed,
					);
				},
			}
		}

		self.observe_fallback_downward_growth(
			frame,
			preview_changed,
			previous_sample_frame,
			previous_sample_fingerprint,
		)
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
		let sample_down_match_eval = self.diagnose_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Down,
			self.last_motion_rows_hint,
		);
		let sample_up_match_eval = self.diagnose_upward_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			self.last_motion_rows_hint,
		);
		let committed_down_match_eval = self.diagnose_reference_overlap_direction(
			&self.last_committed_frame,
			frame,
			ScrollDirection::Down,
			self.last_motion_rows_hint,
		);
		let committed_up_match_eval = self.diagnose_upward_reference_overlap_direction(
			&self.last_committed_frame,
			frame,
			self.last_motion_rows_hint,
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
		&self,
		op: &'static str,
		input_direction: ScrollDirection,
		detected_motion: Option<MotionObservation>,
		candidate_viewport_top_y: Option<i32>,
		growth_rows: Option<u32>,
		block_reason: Option<&'static str>,
	) {
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

	fn restore_last_sample(&mut self, frame: RgbaImage, fingerprint: Option<Vec<u8>>) {
		self.last_sample_frame = frame;
		self.last_sample_fingerprint = fingerprint;
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
		self.resume_frontier_requires_reacquire = true;

		self.resume_frontier_top_y.get_or_insert(frontier_top_y);

		self.observed_viewport_top_y = observed_viewport_top_y;
	}

	fn observe_unconfirmed_upward_rewind(&mut self) {
		self.last_motion_rows_hint = None;

		let frontier_top_y = self.current_viewport_top_y;

		self.resume_frontier_top_y.get_or_insert(frontier_top_y);

		self.resume_frontier_requires_reacquire = true;
		self.observed_viewport_top_y =
			self.observed_viewport_top_y.min(frontier_top_y.saturating_sub(1));
	}

	fn observe_downward_motion(
		&mut self,
		frame: RgbaImage,
		motion_rows: u32,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		self.last_motion_rows_hint = Some(motion_rows);

		if self.resume_frontier_top_y.is_some() {
			return self.observe_downward_motion_while_resume_frontier_active(
				frame,
				motion_rows,
				preview_changed,
			);
		}

		let candidate_viewport_top_y = self
			.observed_viewport_top_y
			.saturating_add(i32::try_from(motion_rows).unwrap_or_default());

		self.observe_downward_growth_to_viewport(
			frame,
			candidate_viewport_top_y,
			preview_changed,
			Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
			"sample_motion_downward_growth",
		)
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
		self.observed_viewport_top_y = candidate_viewport_top_y;

		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate_viewport_top_y);

		if growth_rows == 0 {
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

		self.log_decision(
			"scroll_capture.downward_growth_candidate",
			ScrollDirection::Down,
			detected_motion,
			Some(candidate_viewport_top_y),
			Some(growth_rows),
			Some(decision_source),
		);

		self.apply_growth(frame, growth_rows, candidate_viewport_top_y)
	}

	fn classify_sample_motion(&self, frame: &RgbaImage) -> Option<MotionObservation> {
		let down_match = self.evaluate_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Down,
			self.last_motion_rows_hint,
		);
		let up_match = self.evaluate_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Up,
			self.last_motion_rows_hint,
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

	pub(crate) fn preview_image(&self) -> &RgbaImage {
		&self.preview_image
	}

	pub(crate) fn export_image(&self) -> &RgbaImage {
		&self.export_image
	}

	pub(crate) fn export_dimensions(&self) -> (u32, u32) {
		self.export_image.dimensions()
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
			self.last_sample_frame = previous.frame.clone();
			self.last_sample_fingerprint = Some(scroll_capture_fingerprint(&previous.frame));
			self.last_unconfirmed_upward_fingerprint = None;
			self.resume_frontier_top_y = None;
			self.resume_frontier_requires_reacquire = false;
		} else {
			self.last_committed_frame = self.anchor_frame.clone();
			self.last_sample_frame = self.anchor_frame.clone();
			self.last_sample_fingerprint = Some(scroll_capture_fingerprint(&self.anchor_frame));
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

	fn fallback_downward_growth_blocked_while_resume_frontier_active(
		&mut self,
		motion_rows: u32,
		preview_changed: bool,
		decision_source: &'static str,
		previous_sample_frame: RgbaImage,
		previous_sample_fingerprint: Option<Vec<u8>>,
	) -> Option<ScrollObserveOutcome> {
		let resume_frontier_top_y = self.resume_frontier_top_y?;
		let candidate_viewport_top_y = self
			.current_viewport_top_y
			.saturating_add(i32::try_from(motion_rows).unwrap_or_default());
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
		self.restore_last_sample(previous_sample_frame, previous_sample_fingerprint);

		Some(preview_update_outcome(preview_changed))
	}

	fn observe_fallback_downward_growth(
		&mut self,
		frame: RgbaImage,
		preview_changed: bool,
		previous_sample_frame: RgbaImage,
		previous_sample_fingerprint: Option<Vec<u8>>,
	) -> Result<ScrollObserveOutcome> {
		let down_match = self.evaluate_reference_overlap_direction(
			&self.last_committed_frame,
			&frame,
			ScrollDirection::Down,
			self.last_motion_rows_hint,
		);
		let up_match = self.evaluate_reference_overlap_direction(
			&self.last_committed_frame,
			&frame,
			ScrollDirection::Up,
			self.last_motion_rows_hint,
		);

		match (down_match, up_match) {
			(Some(down), Some(up)) => {
				if up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100
				{
					self.observe_upward_rewind_from_committed(up.motion_rows);
					self.log_decision(
						"scroll_capture.fallback_preferred_upward_match",
						ScrollDirection::Down,
						Some(MotionObservation {
							direction: ScrollDirection::Up,
							motion_rows: up.motion_rows,
						}),
						None,
						None,
						Some("last_committed_overlap_preferred_upward_match"),
					);

					return Ok(ScrollObserveOutcome::UnsupportedDirection {
						direction: ScrollDirection::Up,
					});
				}

				if let Some(outcome) = self
					.fallback_downward_growth_blocked_while_resume_frontier_active(
						down.motion_rows,
						preview_changed,
						"resume_frontier_active_blocks_last_committed_fallback_downward_match",
						previous_sample_frame.clone(),
						previous_sample_fingerprint.clone(),
					) {
					return Ok(outcome);
				}

				self.last_motion_rows_hint = Some(down.motion_rows);

				let candidate_viewport_top_y = self
					.current_viewport_top_y
					.saturating_add(i32::try_from(down.motion_rows).unwrap_or_default());

				self.observe_downward_growth_to_viewport(
					frame,
					candidate_viewport_top_y,
					preview_changed,
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: down.motion_rows,
					}),
					"fallback_downward_match_from_last_committed_frame",
				)
			},
			(Some(down), None) => {
				if let Some(outcome) = self
					.fallback_downward_growth_blocked_while_resume_frontier_active(
						down.motion_rows,
						preview_changed,
						"resume_frontier_active_blocks_last_committed_fallback_downward_match",
						previous_sample_frame,
						previous_sample_fingerprint,
					) {
					return Ok(outcome);
				}

				self.last_motion_rows_hint = Some(down.motion_rows);

				let candidate_viewport_top_y = self
					.current_viewport_top_y
					.saturating_add(i32::try_from(down.motion_rows).unwrap_or_default());

				self.observe_downward_growth_to_viewport(
					frame,
					candidate_viewport_top_y,
					preview_changed,
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: down.motion_rows,
					}),
					"fallback_downward_match_without_upward_candidate",
				)
			},
			(None, Some(up)) => {
				self.observe_upward_rewind_from_committed(up.motion_rows);
				self.log_decision(
					"scroll_capture.fallback_upward_match_only",
					ScrollDirection::Down,
					Some(MotionObservation {
						direction: ScrollDirection::Up,
						motion_rows: up.motion_rows,
					}),
					None,
					None,
					Some("last_committed_overlap_only_matched_upward"),
				);

				Ok(ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up })
			},
			(None, None) => Ok(preview_update_outcome(preview_changed)),
		}
	}

	fn apply_growth(
		&mut self,
		frame: RgbaImage,
		growth_rows: u32,
		viewport_top_y: i32,
	) -> Result<ScrollObserveOutcome> {
		let strip = crop_bottom_rows(&frame, growth_rows)
			.ok_or_else(|| eyre::eyre!("failed to extract growth strip"))?;
		let preview_strip = resize_strip_to_preview_width(&strip, self.preview_width_px);

		self.export_image = append_vertical_image(&self.export_image, &strip)?;
		self.preview_image = append_vertical_image(&self.preview_image, &preview_strip)?;

		self.bottom_segments.push(strip);
		self.bottom_preview_segments.push(preview_strip);

		self.current_viewport_top_y = viewport_top_y;
		self.observed_viewport_top_y = viewport_top_y;
		self.last_committed_frame = frame.clone();
		self.resume_frontier_top_y = None;
		self.resume_frontier_requires_reacquire = false;

		self.growth_history.push(GrowthCommit { frame, growth_rows, viewport_top_y });

		Ok(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows })
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
	let max_overlap = previous.height().min(next.height());
	let overlap = detect_vertical_overlap_in_range(
		previous,
		next,
		range,
		direction,
		config,
		overlap_global_informative_span(previous, next),
	);

	if !overlap.matched {
		return None;
	}

	let motion_rows = max_overlap.saturating_sub(overlap.rows);

	if motion_rows == 0 {
		return None;
	}

	Some(DirectionMatch { mean_abs_diff_x100: overlap.mean_abs_diff_x100, motion_rows })
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

		let band_rows = overlap_rows.clamp(1, MOTION_SEARCH_BAND_ROWS);
		let diff = motion_mean_abs_diff_x100(
			previous,
			next,
			motion_rows,
			band_rows,
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
	band_rows: u32,
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

	let band_rows = band_rows.min(overlap_rows).max(1);
	let column_samples = width.min(config.max_column_samples).max(1);
	let row_samples = band_rows.min(config.max_row_samples).max(1);
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
		let local_y = evenly_spaced_sample(0, band_rows, row, row_samples);
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
			let start_x = left_span.start_x.min(right_span.start_x);
			let end_exclusive_x =
				left_span.end_exclusive_x.max(right_span.end_exclusive_x).min(width);

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
		self, DirectionMatch, MotionObservation, OverlapSearchConfig, ScrollDirection,
		ScrollFrameFingerprint, ScrollObserveOutcome, ScrollSession,
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

	fn paint_row(frame: &mut image::RgbaImage, row: u32, color: [u8; 4]) {
		for x in 0..frame.width() {
			frame.put_pixel(x, row, Rgba(color));
		}
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
		));
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
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
		));

		let height_after_upward_rewind = session.export_image().height();

		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_after_upward_rewind);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
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
		));
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
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
	fn resume_frontier_blocks_repeated_return_to_last_committed_viewport() {
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

		let height_after_second_append = session.export_image().height();

		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert_eq!(session.observed_viewport_top_y, 1);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert_eq!(session.observed_viewport_top_y, 2);
		assert_eq!(session.export_image().height(), height_after_second_append);
		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert_eq!(session.observed_viewport_top_y, 1);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert_eq!(session.observed_viewport_top_y, 2);
		assert_eq!(session.export_image().height(), height_after_second_append);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.resume_frontier_top_y, None);
		assert_eq!(session.export_image().height(), 8);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
	}

	#[test]
	fn upward_input_uses_last_committed_match_when_sample_motion_looks_downward() {
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

		let stale_lower_sample = make_window(&document, 3, 0, 5);

		session.last_sample_frame = stale_lower_sample.clone();
		session.last_sample_fingerprint =
			Some(super::scroll_capture_fingerprint(&stale_lower_sample));

		assert_eq!(
			session.observe_upward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
		);
		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert_eq!(session.observed_viewport_top_y, 1);

		let height_before_return = session.export_image().height();

		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_return);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), height_before_return + 1);
	}

	#[test]
	fn blocked_fallback_downward_input_does_not_append_or_advance_sample_baseline() {
		let document = [
			[0, 0, 0, 255],
			[200, 0, 0, 255],
			[40, 0, 0, 255],
			[240, 0, 0, 255],
			[80, 0, 0, 255],
			[180, 0, 0, 255],
			[20, 0, 0, 255],
			[220, 0, 0, 255],
			[60, 0, 0, 255],
			[160, 0, 0, 255],
			[100, 0, 0, 255],
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
			session.observe_upward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		));

		let blocked_frame = make_window(&document, 3, 6, 5);
		let unrelated_sample = make_test_image(
			3,
			&[
				[255, 255, 0, 255],
				[0, 255, 255, 255],
				[255, 0, 255, 255],
				[0, 0, 255, 255],
				[255, 255, 255, 255],
			],
		);

		session.last_sample_frame = unrelated_sample.clone();
		session.last_sample_fingerprint =
			Some(super::scroll_capture_fingerprint(&unrelated_sample));

		let height_before_fallback = session.export_image().height();
		let observed_before_fallback = session.observed_viewport_top_y;
		let sample_before_fallback = session.last_sample_frame.clone();
		let sample_fingerprint_before_fallback = session.last_sample_fingerprint.clone();

		assert!(matches!(
			session.observe_downward_sample(blocked_frame).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_fallback);
		assert_eq!(session.observed_viewport_top_y, observed_before_fallback);
		assert_eq!(session.last_sample_frame, sample_before_fallback);
		assert_eq!(session.last_sample_fingerprint, sample_fingerprint_before_fallback);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([0, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 6), &Rgba([20, 0, 0, 255]));
	}

	#[test]
	fn resume_frontier_direct_gating_ignores_stale_observed_position() {
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
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		));

		session.observed_viewport_top_y = 50;

		let height_before_return = session.export_image().height();

		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_return);
		assert_eq!(session.observed_viewport_top_y, 2);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), height_before_return + 1);
		assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
	}

	#[test]
	fn resume_frontier_direct_proof_resumes_growth_across_skipped_anchor_undercount() {
		let document = (0_u16..96)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 20, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
		);

		let height_before_rewind = session.export_image().height();
		let rewind_frame = make_window(&document, 3, 5, 48);

		assert!(matches!(
			session.observe_upward_sample(rewind_frame).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(20));
		assert_eq!(session.observed_viewport_top_y, 5);
		assert!(session.resume_frontier_requires_reacquire);
		assert_eq!(
			session
				.observe_downward_motion_while_resume_frontier_active(
					make_window(&document, 3, 28, 48),
					14,
					true,
				)
				.unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 8 }
		);
		assert_eq!(session.export_image().height(), height_before_rewind + 8);
		assert_eq!(session.current_viewport_top_y, 28);
		assert_eq!(session.observed_viewport_top_y, 28);
		assert_eq!(session.resume_frontier_top_y, None);
		assert!(!session.resume_frontier_requires_reacquire);
		assert_eq!(session.export_image(), &make_test_image(3, &document[..76]));
	}

	#[test]
	fn resume_frontier_exact_last_committed_reacquire_remains_valid_when_direct_proof_is_too_weak()
	{
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
		let exact_last_committed_frame = session.last_committed_frame.clone();

		session.resume_frontier_top_y = Some(session.current_viewport_top_y);
		session.resume_frontier_requires_reacquire = true;
		session.observed_viewport_top_y = session.current_viewport_top_y - 1;

		let mut near_match = exact_last_committed_frame.clone();

		paint_row(&mut near_match, 4, [111, 0, 0, 255]);

		assert!(matches!(
			session
				.observe_downward_motion_while_resume_frontier_active(near_match, 1, true)
				.unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_resume);
		assert_eq!(session.observed_viewport_top_y, 1);
		assert!(session.resume_frontier_requires_reacquire);
		assert!(matches!(
			session
				.observe_downward_motion_while_resume_frontier_active(
					exact_last_committed_frame,
					1,
					true,
				)
				.unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_resume);
		assert_eq!(session.observed_viewport_top_y, 2);
		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert!(!session.resume_frontier_requires_reacquire);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), height_before_resume + 1);
		assert_eq!(session.current_viewport_top_y, 3);
		assert_eq!(session.observed_viewport_top_y, 3);
		assert_eq!(session.resume_frontier_top_y, None);
		assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
	}

	#[test]
	fn resume_frontier_direct_match_can_append_before_observed_return_reaches_frontier() {
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
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);

		session.observe_upward_rewind(2);

		assert_eq!(session.last_motion_rows_hint, None);
		assert_eq!(session.observed_viewport_top_y, 0);
		assert_eq!(session.resume_frontier_top_y, Some(2));

		let height_before_resume = session.export_image().height();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), height_before_resume + 1);
		assert_eq!(session.current_viewport_top_y, 3);
		assert_eq!(session.observed_viewport_top_y, 3);
		assert_eq!(session.resume_frontier_top_y, None);
		assert!(!session.resume_frontier_requires_reacquire);
		assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
	}

	#[test]
	fn large_downward_step_after_rewind_only_appends_residual_new_rows() {
		let document = (0_u16..96)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 20, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
		);

		let height_before_rewind = session.export_image().height();

		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 5, 48)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(20));
		assert_eq!(session.observed_viewport_top_y, 5);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 20, 48)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_rewind);
		assert_eq!(session.observed_viewport_top_y, 20);
		assert_eq!(session.resume_frontier_top_y, Some(20));
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 24, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 4 }
		);
		assert_eq!(session.export_image().height(), height_before_rewind + 4);
		assert_eq!(session.current_viewport_top_y, 24);
		assert_eq!(session.observed_viewport_top_y, 24);
		assert_eq!(session.resume_frontier_top_y, None);
		assert_eq!(session.export_image(), &make_test_image(3, &document[..72]));
	}

	#[test]
	fn moving_resume_frontier_survives_two_rewind_resume_cycles_without_duplicate_growth() {
		let document = (0_u16..128)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 20, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
		);

		let height_after_initial_growth = session.export_image().height();

		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 5, 48)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(20));
		assert_eq!(session.observed_viewport_top_y, 5);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 20, 48)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_after_initial_growth);
		assert_eq!(session.current_viewport_top_y, 20);
		assert_eq!(session.observed_viewport_top_y, 20);
		assert_eq!(session.resume_frontier_top_y, Some(20));
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 24, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 4 }
		);
		assert_eq!(session.export_image().height(), height_after_initial_growth + 4);
		assert_eq!(session.current_viewport_top_y, 24);
		assert_eq!(session.observed_viewport_top_y, 24);
		assert_eq!(session.resume_frontier_top_y, None);
		assert_eq!(session.export_image(), &make_test_image(3, &document[..72]));

		let height_after_first_resume = session.export_image().height();

		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 12, 48)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(24));
		assert_eq!(session.observed_viewport_top_y, 12);
		assert!(session.resume_frontier_requires_reacquire);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 24, 48)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_after_first_resume);
		assert_eq!(session.current_viewport_top_y, 24);
		assert_eq!(session.observed_viewport_top_y, 24);
		assert_eq!(session.resume_frontier_top_y, Some(24));
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 28, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 4 }
		);
		assert_eq!(session.export_image().height(), height_after_first_resume + 4);
		assert_eq!(session.current_viewport_top_y, 28);
		assert_eq!(session.observed_viewport_top_y, 28);
		assert_eq!(session.resume_frontier_top_y, None);
		assert!(!session.resume_frontier_requires_reacquire);
		assert_eq!(session.export_image(), &make_test_image(3, &document[..76]));
	}

	#[test]
	fn blocked_frontier_crossing_requires_reacquire_before_later_valid_resume() {
		let document = (0_u16..96)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 20, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
		);
		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 5, 48)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(20));
		assert_eq!(session.observed_viewport_top_y, 5);

		let unrelated_rows = vec![[250, 250, 250, 255]; 48];
		let unrelated_frame = make_test_image(3, &unrelated_rows);
		let height_before_block = session.export_image().height();

		assert!(matches!(
			session
				.observe_downward_motion_while_resume_frontier_active(unrelated_frame, 19, true)
				.unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_block);
		assert_eq!(session.current_viewport_top_y, 20);
		assert_eq!(session.observed_viewport_top_y, 19);
		assert_eq!(session.resume_frontier_top_y, Some(20));
		assert_eq!(session.export_image(), &make_test_image(3, &document[..68]));
		assert!(matches!(
			session
				.observe_downward_motion_while_resume_frontier_active(
					make_window(&document, 3, 20, 48),
					1,
					true,
				)
				.unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_block);
		assert_eq!(session.current_viewport_top_y, 20);
		assert_eq!(session.observed_viewport_top_y, 20);
		assert_eq!(session.resume_frontier_top_y, Some(20));
		assert_eq!(
			session
				.observe_downward_motion_while_resume_frontier_active(
					make_window(&document, 3, 28, 48),
					8,
					true,
				)
				.unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 8 }
		);
		assert_eq!(session.current_viewport_top_y, 28);
		assert_eq!(session.observed_viewport_top_y, 28);
		assert_eq!(session.resume_frontier_top_y, None);
		assert_eq!(session.export_image(), &make_test_image(3, &document[..76]));
	}

	#[test]
	fn resume_frontier_blocked_downward_path_preserves_local_progress_without_upward_conflict() {
		let document = (0_u16..160)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let width = 64;
		let mut session = ScrollSession::new(make_window(&document, width, 0, 48), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, width, 20, 48)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
		);

		let height_before_resume = session.export_image().height();

		session.resume_frontier_top_y = Some(20);
		session.resume_frontier_requires_reacquire = true;
		session.observed_viewport_top_y = 36;
		session.last_sample_frame = make_window(&document, width, 36, 48);
		session.last_sample_fingerprint =
			Some(scroll_capture::scroll_capture_fingerprint(&session.last_sample_frame));

		let resume_frame = make_window(&document, width, 52, 48);

		assert_eq!(
			session
				.evaluate_reference_overlap_direction_preferred_only(
					&session.last_sample_frame,
					&resume_frame,
					ScrollDirection::Down,
					Some(16),
				)
				.map(|matched| matched.motion_rows),
			Some(16)
		);
		assert_eq!(
			session
				.evaluate_reference_overlap_direction_preferred_only(
					&session.last_committed_frame,
					&resume_frame,
					ScrollDirection::Down,
					Some(16),
				)
				.map(|matched| matched.motion_rows),
			None
		);
		assert!(matches!(
			session
				.observe_downward_motion_while_resume_frontier_active(resume_frame, 16, true,)
				.unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_resume);
		assert_eq!(session.current_viewport_top_y, 20);
		assert_eq!(session.observed_viewport_top_y, 52);
		assert_eq!(session.resume_frontier_top_y, Some(20));
		assert!(session.resume_frontier_requires_reacquire);
	}

	#[test]
	fn resume_frontier_requires_reacquire_before_ambiguous_match_can_reach_frontier() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[10, 0, 0, 255],
			[20, 0, 0, 255],
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

		session.observe_upward_rewind(1);

		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert_eq!(session.observed_viewport_top_y, 1);

		let height_before_resume = session.export_image().height();

		assert!(matches!(
			session
				.observe_downward_motion_while_resume_frontier_active(
					make_window(&document, 3, 1, 5),
					1,
					true,
				)
				.unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_resume);
		assert_eq!(session.current_viewport_top_y, 2);
		assert_eq!(session.observed_viewport_top_y, 1);
		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert!(session.resume_frontier_requires_reacquire);
	}

	#[test]
	fn resume_frontier_blocks_same_viewport_small_mutation_without_new_growth() {
		let document = [
			[0, 0, 0, 255],
			[0, 0, 0, 255],
			[0, 0, 0, 255],
			[0, 0, 0, 255],
			[40, 0, 0, 255],
			[0, 0, 0, 255],
			[40, 0, 0, 255],
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
			session.observe_upward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));

		let height_before_resume = session.export_image().height();
		let mut same_viewport_with_small_mutation = make_window(&document, 3, 2, 5);

		paint_row(&mut same_viewport_with_small_mutation, 3, [16, 0, 0, 255]);

		assert!(matches!(
			session.observe_downward_sample(same_viewport_with_small_mutation).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_resume);
		assert_eq!(session.current_viewport_top_y, 2);
		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert_eq!(session.export_image(), &make_test_image(3, &document));
	}

	#[test]
	fn first_small_upward_rewind_after_large_downward_commit_arms_and_blocks_duplicate_return() {
		let document = (0_u16..220)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 64), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 40, 64)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 40 }
		);
		assert_eq!(session.last_motion_rows_hint, Some(40));

		let height_before_rewind = session.export_image().height();

		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 35, 64)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(40));

		let observed_after_first_rewind = session.observed_viewport_top_y;

		assert!(observed_after_first_rewind < 40);
		assert!(session.resume_frontier_requires_reacquire);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 40, 64)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_rewind);
		assert_eq!(session.current_viewport_top_y, 40);
		assert_eq!(session.observed_viewport_top_y, 40);
		assert_eq!(session.resume_frontier_top_y, Some(40));
	}

	#[test]
	fn upward_preview_change_without_overlap_proof_fails_closed_into_rewind_block() {
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
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);

		let height_before_rewind = session.export_image().height();
		let unrelated_frame = make_test_image(
			3,
			&[
				[200, 200, 0, 255],
				[0, 200, 200, 255],
				[200, 0, 200, 255],
				[20, 20, 20, 255],
				[240, 240, 240, 255],
			],
		);

		assert_eq!(
			session.observe_upward_sample(unrelated_frame).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
		);
		assert_eq!(session.export_image().height(), height_before_rewind);
		assert_eq!(session.resume_frontier_top_y, Some(2));
		assert!(session.resume_frontier_requires_reacquire);
		assert!(session.observed_viewport_top_y < 2);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_rewind);
		assert_eq!(session.current_viewport_top_y, 2);
		assert_eq!(session.resume_frontier_top_y, Some(2));
	}

	#[test]
	fn rewind_active_upward_override_prefers_smaller_committed_match() {
		let sample = DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 240 };
		let committed = DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 96 };

		assert_eq!(
			scroll_capture::rewind_active_upward_override_match(
				Some(sample),
				Some(committed),
				true
			),
			Some((committed, true))
		);
		assert_eq!(
			scroll_capture::rewind_active_upward_override_match(
				Some(sample),
				Some(committed),
				false
			),
			None
		);
	}

	#[test]
	fn rewind_active_repeated_upward_prefers_conservative_committed_match() {
		let document = (0_u16..220)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 64), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 40, 64)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 40 }
		);
		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 35, 64)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(40));

		let observed_after_first_rewind = session.observed_viewport_top_y;

		assert!(observed_after_first_rewind < 40);

		let stale_lower_sample = make_window(&document, 3, 116, 64);

		session.last_sample_frame = stale_lower_sample.clone();
		session.last_sample_fingerprint =
			Some(super::scroll_capture_fingerprint(&stale_lower_sample));
		session.last_motion_rows_hint = Some(48);

		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 20, 64)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(40));
		assert!(session.observed_viewport_top_y <= observed_after_first_rewind);
		assert!(session.resume_frontier_requires_reacquire);
	}

	#[test]
	fn rewind_active_fail_closed_upward_path_preserves_later_resume_growth() {
		let document = (0_u16..240)
			.map(|row| {
				[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
			})
			.collect::<Vec<_>>();
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 64), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 32, 64)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 32 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 64, 64)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 32 }
		);
		assert!(matches!(
			session.observe_upward_sample(make_window(&document, 3, 16, 64)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.resume_frontier_top_y, Some(64));
		assert!(session.resume_frontier_requires_reacquire);

		let observed_before_conflict = session.observed_viewport_top_y;

		assert!(observed_before_conflict < 64);

		let stale_lower_sample = make_window(&document, 3, 128, 64);

		session.last_sample_frame = stale_lower_sample.clone();
		session.last_sample_fingerprint =
			Some(super::scroll_capture_fingerprint(&stale_lower_sample));
		session.last_motion_rows_hint = Some(64);

		let height_before_resume = session.export_image().height();
		let resumed_target_top_y = 80_i32;
		let resumed_motion_rows =
			u32::try_from(resumed_target_top_y - observed_before_conflict).unwrap();

		assert_eq!(
			session.arm_unconfirmed_upward_rewind(
				&make_window(&document, 3, 0, 64),
				super::scroll_capture_fingerprint(&make_window(&document, 3, 0, 64)),
				Some(MotionObservation { direction: ScrollDirection::Up, motion_rows: 64 }),
				false,
				"scroll_capture.rewind_armed_without_match",
				"rewind_active_upward_input_conflicted_with_last_committed_downward_match",
			),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
		);
		assert_eq!(session.export_image().height(), height_before_resume);
		assert_eq!(session.resume_frontier_top_y, Some(64));
		assert_eq!(session.observed_viewport_top_y, observed_before_conflict);
		assert!(session.resume_frontier_requires_reacquire);
		assert_eq!(session.last_sample_frame, stale_lower_sample);
		assert_eq!(
			session
				.observe_downward_motion_while_resume_frontier_active(
					make_window(&document, 3, resumed_target_top_y as usize, 64),
					resumed_motion_rows,
					true,
				)
				.unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 16 }
		);
		assert_eq!(session.export_image().height(), height_before_resume + 16);
		assert_eq!(session.current_viewport_top_y, 80);
		assert_eq!(session.observed_viewport_top_y, 80);
		assert_eq!(session.resume_frontier_top_y, None);
		assert!(!session.resume_frontier_requires_reacquire);
		assert_eq!(session.export_image(), &make_test_image(3, &document[..144]));
	}

	#[test]
	fn trustworthy_committed_upward_match_overrides_non_upward_sample_motion() {
		let up = DirectionMatch { mean_abs_diff_x100: 120, motion_rows: 18 };
		let down = DirectionMatch { mean_abs_diff_x100: 90, motion_rows: 18 };

		assert_eq!(scroll_capture::preferred_upward_override_match(Some(up), Some(down)), Some(up));
		assert_eq!(scroll_capture::preferred_upward_override_match(Some(up), None), Some(up));
	}

	#[test]
	fn downward_input_requires_committed_growth_before_confirming_upward_match() {
		let up = DirectionMatch { mean_abs_diff_x100: 120, motion_rows: 18 };

		assert_eq!(
			scroll_capture::upward_confirmation_match_for_downward_input(Some(up), None, false),
			None
		);
		assert_eq!(
			scroll_capture::upward_confirmation_match_for_downward_input(Some(up), None, true),
			Some(up)
		);
	}

	#[test]
	fn downward_input_upward_confirmation_requires_direction_margin_when_downward_exists() {
		let weak_up = DirectionMatch { mean_abs_diff_x100: 120, motion_rows: 18 };
		let strong_down = DirectionMatch { mean_abs_diff_x100: 90, motion_rows: 18 };
		let strong_up = DirectionMatch { mean_abs_diff_x100: 40, motion_rows: 18 };
		let weak_down = DirectionMatch { mean_abs_diff_x100: 160, motion_rows: 18 };

		assert_eq!(
			scroll_capture::upward_confirmation_match_for_downward_input(
				Some(weak_up),
				Some(strong_down),
				true,
			),
			None
		);
		assert_eq!(
			scroll_capture::upward_confirmation_match_for_downward_input(
				Some(strong_up),
				Some(weak_down),
				true,
			),
			Some(strong_up)
		);
	}

	#[test]
	fn upward_input_prefers_conservative_committed_override_before_rewind_active() {
		let sample = DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 288 };
		let committed = DirectionMatch { mean_abs_diff_x100: 0, motion_rows: 48 };

		assert_eq!(
			scroll_capture::preferred_upward_input_override_match(Some(sample), Some(committed)),
			Some((committed, true))
		);
		assert_eq!(
			scroll_capture::preferred_upward_input_override_match(Some(committed), Some(sample)),
			Some((committed, false))
		);
	}

	#[test]
	fn weak_committed_upward_match_does_not_override_non_upward_sample_motion() {
		let up = DirectionMatch { mean_abs_diff_x100: 500, motion_rows: 18 };
		let down = DirectionMatch { mean_abs_diff_x100: 90, motion_rows: 18 };

		assert_eq!(scroll_capture::preferred_upward_override_match(Some(up), Some(down)), None);
		assert_eq!(scroll_capture::preferred_upward_override_match(Some(up), None), None);
	}

	#[test]
	fn rewind_active_sample_only_upward_conflict_with_committed_downward_fails_closed() {
		let sample_up = DirectionMatch { mean_abs_diff_x100: 80, motion_rows: 64 };
		let committed_down = DirectionMatch { mean_abs_diff_x100: 80, motion_rows: 80 };
		let committed_up = DirectionMatch { mean_abs_diff_x100: 70, motion_rows: 32 };

		assert!(scroll_capture::rewind_active_upward_motion_should_fail_closed(
			Some(sample_up),
			None,
			Some(committed_down),
			true,
		));
		assert!(!scroll_capture::rewind_active_upward_motion_should_fail_closed(
			Some(sample_up),
			Some(committed_up),
			Some(committed_down),
			true,
		));
		assert!(!scroll_capture::rewind_active_upward_motion_should_fail_closed(
			Some(sample_up),
			None,
			Some(committed_down),
			false,
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
