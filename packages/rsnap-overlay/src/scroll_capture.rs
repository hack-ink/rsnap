pub mod bench_support;

mod downward_resolution;
mod support;

pub(crate) use self::support::{
	compose_provisional_preview_image, scroll_capture_fingerprint, scroll_capture_fingerprint_delta,
};

use std::ops::RangeInclusive;

use color_eyre::eyre::{self};
use image::RgbaImage;

#[cfg(test)]
use self::support::detect_vertical_overlap;

pub(crate) const PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS: u32 = 24;
pub(crate) const PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS: u32 = 12;
pub(crate) const UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS: u32 = 8;

const FINGERPRINT_GRID_COLUMNS: u32 = 12;
const FINGERPRINT_GRID_ROWS: u32 = 16;
const DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS: u32 = 48;
const DOWNWARD_KEYFRAME_SEARCH_MOTION_TOLERANCE_ROWS: u32 = 4;
const DOWNWARD_KEYFRAME_SEARCH_MAX_TOLERANCE_ROWS: u32 = 48;
const LOCAL_DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS: u32 = 4;
const LOCAL_DOWNWARD_SEARCH_MAX_TOLERANCE_ROWS: u32 = 48;
const DOWNWARD_REGISTRATION_AMBIGUOUS_GAP_ROWS: u32 = 24;
const DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS: u32 = 4;
const DOWNWARD_REGISTRATION_MIN_OVERLAP_DIVISOR: u32 = 3;
const DOWNWARD_KEYFRAME_SEARCH_LIMIT: usize = 4;
const DOWNWARD_KEYFRAME_MIN_OVERLAP_DIVISOR: u32 = 5;
const INITIAL_DOWNWARD_MAX_MOTION_ROWS: u32 = 256;
const PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS: u32 = 4;
const EXTREME_TRANSIENT_PREVIEW_LOCAL_TAIL_MULTIPLIER: u32 = 12;
const REPEATED_PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS: u32 = 4;
const TINY_OBSERVED_BURST_RECOVERY_MAX_MOTION_ROWS: u32 = 2;
const TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS: u32 = 1;
const TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MIN_LAST_HINT_ROWS: u32 = 7;
const BOOTSTRAP_HINTED_INITIAL_GROWTH_MAX_ROWS: u32 = 1_024;
const DOWNWARD_COMMITTED_KEYFRAME_LOCAL_OVERRUN_MAX_ROWS: u32 = 24;
const FALLBACK_DOWNWARD_GROWTH_MIN_ROWS: u32 = 8;
const FALLBACK_DOWNWARD_GROWTH_MAX_ROWS: u32 = 16;
const TRANSIENT_MOTION_HINT_MAX_MULTIPLIER: u32 = 3;
const TRANSIENT_MOTION_HINT_MIN_CAP_ROWS: u32 = 12;
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
		let informative_span = self::support::informative_column_span(image, 0, height);
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
			let y = self::support::evenly_spaced_sample(top, bottom, row, FINGERPRINT_GRID_ROWS);

			for column in 0..FINGERPRINT_GRID_COLUMNS {
				let x = self::support::evenly_spaced_sample(
					left,
					right,
					column,
					FINGERPRINT_GRID_COLUMNS,
				);
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
	worker_pairwise_requires_committed_reacquire: bool,
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
	pub(crate) fn new(
		base_frame: RgbaImage,
		preview_width_px: u32,
	) -> color_eyre::eyre::Result<Self> {
		let fingerprint = scroll_capture_fingerprint(&base_frame);
		let anchor_preview =
			self::support::resize_strip_to_preview_width(&base_frame, preview_width_px.max(1));

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
			worker_pairwise_requires_committed_reacquire: false,
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
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		self.observe_downward_sample_with_motion_hint(frame, None)
	}

	pub(crate) fn observe_downward_sample_with_motion_hint(
		&mut self,
		frame: RgbaImage,
		motion_rows_hint: Option<u32>,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		self.observe_downward_sample_with_motion_hint_and_burst(frame, motion_rows_hint, false)
	}

	pub(crate) fn observe_downward_sample_with_motion_hint_and_burst(
		&mut self,
		frame: RgbaImage,
		motion_rows_hint: Option<u32>,
		allow_burst_search: bool,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		self.observe_sample_with_motion_context(frame, ScrollDirection::Up, None, false)
	}

	pub(crate) fn observe_worker_pairwise_vision_frame(
		&mut self,
		frame: RgbaImage,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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

		if self.worker_pairwise_requires_committed_reacquire {
			if frame == self.last_committed_frame {
				self.worker_pairwise_requires_committed_reacquire = false;

				return Ok(self.observe_worker_pairwise_no_change(
					frame,
					fingerprint,
					"worker_pairwise_reacquired_last_committed_frame",
				));
			}

			return Ok(self.block_worker_pairwise_until_committed_reacquire(frame, fingerprint));
		}
		if frame == previous_worker_frame {
			let reason = if frame == self.last_committed_frame {
				"frame_matches_last_committed_frame"
			} else {
				"frame_matches_worker_pairwise_previous_frame"
			};

			return Ok(self.observe_worker_pairwise_no_change(frame, fingerprint, reason));
		}

		let Some(matched) = self::support::classify_vision_downward_sample_motion_against(
			&previous_worker_frame,
			&frame,
		) else {
			if let Some(upward_motion_rows) =
				self::support::trusted_pairwise_upward_shift_rows(&previous_worker_frame, &frame)
			{
				return Ok(self.observe_worker_pairwise_upward_motion(
					frame,
					fingerprint,
					upward_motion_rows,
				));
			}

			return Ok(self.observe_worker_pairwise_no_change(
				frame,
				fingerprint,
				"worker_pairwise_vision_no_downward_offset",
			));
		};
		let corroborated_shift_rows =
			self::support::trusted_pairwise_downward_shift_rows_near_motion(
				&previous_worker_frame,
				&frame,
				matched.motion_rows,
				WORKER_PAIRWISE_CORROBORATION_TOLERANCE_ROWS,
			);

		self.observe_resolved_worker_pairwise_downward_motion(
			frame,
			fingerprint,
			matched.motion_rows,
			corroborated_shift_rows,
		)
	}

	fn observe_resolved_worker_pairwise_downward_motion(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		vision_motion_rows: u32,
		corroborated_shift_rows: Option<u32>,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		let effective_motion_rows = match Self::resolve_worker_pairwise_motion_rows(
			vision_motion_rows,
			corroborated_shift_rows,
		) {
			Ok(motion_rows) => motion_rows,
			Err(block_reason) => {
				return Ok(self.block_worker_pairwise_growth(
					frame,
					fingerprint,
					vision_motion_rows,
					self.current_viewport_top_y,
					0,
					block_reason,
				));
			},
		};

		tracing::debug!(
			op = "scroll_capture.worker_pairwise_motion_resolved",
			vision_motion_rows,
			corroborated_motion_rows = corroborated_shift_rows,
			effective_motion_rows,
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			"Scroll-capture worker pairwise motion resolved against pixel overlap."
		);

		if effective_motion_rows == 0 {
			return Ok(self.block_worker_pairwise_growth(
				frame,
				fingerprint,
				effective_motion_rows,
				self.current_viewport_top_y,
				0,
				"worker_pairwise_zero_effective_motion",
			));
		}

		let candidate_viewport_top_y = self
			.current_viewport_top_y
			.saturating_add(i32::try_from(effective_motion_rows).unwrap_or_default());
		let growth_rows = self.growth_rows_for_candidate_viewport_top_y(candidate_viewport_top_y);
		let frame_max_growth_rows = frame.height().saturating_sub(1).max(1);

		if growth_rows == 0 || growth_rows > frame_max_growth_rows {
			return Ok(self.block_worker_pairwise_growth(
				frame,
				fingerprint,
				effective_motion_rows,
				candidate_viewport_top_y,
				growth_rows,
				"worker_pairwise_growth_exceeded_frame_bounds",
			));
		}

		self.log_decision(
			"scroll_capture.worker_pairwise_growth_candidate",
			ScrollDirection::Down,
			Some(MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: effective_motion_rows,
			}),
			Some(candidate_viewport_top_y),
			Some(growth_rows),
			Some("worker_pairwise_vision"),
		);

		self.worker_pairwise_previous_frame = frame.clone();
		self.worker_pairwise_requires_committed_reacquire = false;

		self.clear_preview_only_downward_recovery_carryover();

		if self.resume_frontier_top_y.is_some() {
			let outcome = self.observe_downward_motion_while_resume_frontier_active(
				frame.clone(),
				effective_motion_rows,
				true,
			)?;

			if !matches!(
				outcome,
				ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, .. }
			) {
				self.record_last_sample(&frame, fingerprint);
			}

			return Ok(outcome);
		}

		self.apply_growth(
			frame.clone(),
			growth_rows,
			candidate_viewport_top_y,
			"worker_pairwise_vision",
			Some(vision_motion_rows),
			Some(effective_motion_rows),
			None,
		)
	}

	fn observe_worker_pairwise_no_change(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		reason: &'static str,
	) -> ScrollObserveOutcome {
		if reason == "worker_pairwise_vision_no_downward_offset"
			&& frame != self.last_committed_frame
		{
			self.record_last_sample(&frame, fingerprint);
			self.clear_preview_only_downward_recovery_carryover();

			self.worker_pairwise_requires_committed_reacquire = true;

			self.log_decision(
				"scroll_capture.worker_pairwise_no_change",
				ScrollDirection::Down,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some(reason),
			);

			return ScrollObserveOutcome::NoChange;
		}

		self.update_worker_pairwise_reference_frame(frame, fingerprint);
		self.log_decision(
			"scroll_capture.worker_pairwise_no_change",
			ScrollDirection::Down,
			None,
			Some(self.observed_viewport_top_y),
			Some(0),
			Some(reason),
		);

		ScrollObserveOutcome::NoChange
	}

	fn observe_worker_pairwise_upward_motion(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		motion_rows: u32,
	) -> ScrollObserveOutcome {
		self.record_last_sample(&frame, fingerprint);
		self.clear_preview_only_downward_recovery_carryover();

		if self.current_viewport_top_y <= 0 && self.resume_frontier_top_y.is_none() {
			if frame != self.last_committed_frame {
				self.worker_pairwise_requires_committed_reacquire = true;
			}

			self.log_decision(
				"scroll_capture.worker_pairwise_upward_at_top",
				ScrollDirection::Up,
				Some(MotionObservation { direction: ScrollDirection::Up, motion_rows }),
				Some(self.current_viewport_top_y),
				Some(0),
				Some("worker_pairwise_upward_motion_without_committed_growth"),
			);

			return ScrollObserveOutcome::PreviewUpdated;
		}

		self.worker_pairwise_previous_frame = frame.clone();

		self.observe_upward_rewind(motion_rows);
		self.log_decision(
			"scroll_capture.worker_pairwise_rewind_armed",
			ScrollDirection::Up,
			Some(MotionObservation { direction: ScrollDirection::Up, motion_rows }),
			None,
			None,
			Some("worker_pairwise_detected_upward_motion"),
		);

		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	}

	fn block_worker_pairwise_growth(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
		motion_rows: u32,
		candidate_viewport_top_y: i32,
		growth_rows: u32,
		reason: &'static str,
	) -> ScrollObserveOutcome {
		self.record_last_sample(&frame, fingerprint);
		self.clear_preview_only_downward_recovery_carryover();

		if motion_rows > 0 {
			self.worker_pairwise_requires_committed_reacquire = true;
		}

		self.log_decision(
			"scroll_capture.worker_pairwise_growth_blocked",
			ScrollDirection::Down,
			Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
			Some(candidate_viewport_top_y),
			Some(growth_rows),
			Some(reason),
		);

		ScrollObserveOutcome::NoChange
	}

	fn block_worker_pairwise_until_committed_reacquire(
		&mut self,
		frame: RgbaImage,
		fingerprint: Vec<u8>,
	) -> ScrollObserveOutcome {
		self.record_last_sample(&frame, fingerprint);
		self.clear_preview_only_downward_recovery_carryover();
		self.log_decision(
			"scroll_capture.worker_pairwise_growth_blocked",
			ScrollDirection::Down,
			None,
			Some(self.current_viewport_top_y),
			Some(0),
			Some("worker_pairwise_requires_committed_reacquire_after_blocked_gap"),
		);

		ScrollObserveOutcome::NoChange
	}

	fn resolve_worker_pairwise_motion_rows(
		vision_motion_rows: u32,
		corroborated_shift_rows: Option<u32>,
	) -> std::result::Result<u32, &'static str> {
		let Some(corroborated_shift_rows) = corroborated_shift_rows else {
			return Err("worker_pairwise_missing_or_ambiguous_overlap_corroboration");
		};

		if corroborated_shift_rows == 0 {
			return Err("worker_pairwise_zero_overlap_corroboration");
		}
		if vision_motion_rows.abs_diff(corroborated_shift_rows)
			> WORKER_PAIRWISE_CORROBORATION_TOLERANCE_ROWS
		{
			return Err("worker_pairwise_vision_overlap_motion_mismatch");
		}

		Ok(corroborated_shift_rows)
	}

	fn update_worker_pairwise_reference_frame(&mut self, frame: RgbaImage, fingerprint: Vec<u8>) {
		self.record_last_sample(&frame, fingerprint.clone());
		self.record_last_downward_observed_sample(&frame, fingerprint);

		self.worker_pairwise_previous_frame = frame;

		self.clear_preview_only_downward_recovery_carryover();
	}

	fn observe_sample_with_motion_context(
		&mut self,
		frame: RgbaImage,
		input_direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
		allow_burst_search: bool,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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

					if let Some(up_match) =
						self::support::upward_confirmation_match_for_downward_input(
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
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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
		if let Some((up_match, from_committed)) =
			self::support::preferred_upward_input_override_match(
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
		if self::support::rewind_active_upward_motion_should_fail_closed(
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

		self::support::rewind_active_upward_override_match(
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

		if let Some((up_match, from_committed)) =
			self::support::preferred_upward_input_override_match(
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
			sample_override_match: self::support::preferred_upward_override_match(
				sample_up_match_eval.final_match,
				sample_down_match_eval.final_match,
			),
			committed_override_match: self::support::preferred_upward_override_match(
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
		let max_motion_rows = self::support::max_directional_motion_rows(previous, next, config);
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

		tracing::debug!(
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

		self::support::preview_update_outcome(preview_changed)
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

	#[allow(clippy::too_many_lines)]
	fn observe_downward_motion(
		&mut self,
		frame: RgbaImage,
		observed_match: DownwardSampleMatch,
		preview_changed: bool,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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
				return self.handle_missing_downward_viewport_authority(
					&frame,
					observed_match,
					motion_rows,
					preview_changed,
				);
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

				return Ok(self::support::preview_update_outcome(preview_changed));
			},
		};

		if self.should_fail_closed_tiny_observed_recovery_in_burst(candidate) {
			return self.block_downward_growth_candidate(
				&frame,
				motion_rows,
				candidate,
				preview_changed,
				"tiny_observed_recovery_under_transient_burst",
			);
		}
		if self.should_fail_closed_outsized_observed_recovery_after_one_pixel_preview_local_commit(
			candidate,
		) {
			return self.block_downward_growth_candidate(
				&frame,
				motion_rows,
				candidate,
				preview_changed,
				"outsized_observed_recovery_after_one_pixel_preview_local_commit",
			);
		}
		if self.should_fail_closed_tiny_preview_only_local_recovery_in_burst(candidate) {
			return self.block_downward_growth_candidate(
				&frame,
				motion_rows,
				candidate,
				preview_changed,
				"tiny_preview_only_local_recovery_under_transient_burst",
			);
		}
		if self
			.should_fail_closed_exactly_corroborated_preview_local_tail_in_extreme_burst(candidate)
		{
			self.pending_extreme_preview_only_local_tail_followup = Some(candidate);
			self.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 1;

			return self.block_downward_growth_candidate(
				&frame,
				motion_rows,
				candidate,
				preview_changed,
				"exactly_corroborated_preview_local_tail_under_extreme_transient_burst",
			);
		}
		if self.should_fail_closed_preview_only_local_tail_after_unresolved_burst(candidate) {
			return self.block_downward_growth_candidate(
				&frame,
				motion_rows,
				candidate,
				preview_changed,
				"preview_only_local_tail_after_unresolved_transient_burst",
			);
		}
		if self.should_fail_closed_tiny_committed_keyframe_recovery_in_burst(candidate) {
			return self.block_downward_growth_candidate(
				&frame,
				motion_rows,
				candidate,
				preview_changed,
				"tiny_committed_keyframe_recovery_under_transient_burst",
			);
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

	fn handle_missing_downward_viewport_authority(
		&mut self,
		frame: &RgbaImage,
		observed_match: DownwardSampleMatch,
		motion_rows: u32,
		preview_changed: bool,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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
		} else if self.blocked_far_committed_only_recovery_after_corroborated_huge_local_jump {
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
		self.consecutive_transient_burst_missing_downward_candidate_frames =
			if self.transient_burst_search_enabled && preview_only_local_viewport_top_y.is_some() {
				self.consecutive_transient_burst_missing_downward_candidate_frames.saturating_add(1)
			} else {
				0
			};

		self.refresh_local_downward_sample(frame);

		if self.should_refresh_downward_observed_baseline_after_huge_suppressed_jump() {
			self.record_last_downward_observed_sample(frame, scroll_capture_fingerprint(frame));
		}
		if reset_preview_only_local_baseline {
			self.clear_preview_only_downward_local_sample();
		} else {
			self.refresh_preview_only_downward_local_sample(
				frame,
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

		Ok(self::support::preview_update_outcome(preview_changed))
	}

	fn block_downward_growth_candidate(
		&mut self,
		frame: &RgbaImage,
		motion_rows: u32,
		candidate: DownwardViewportCandidate,
		preview_changed: bool,
		block_reason: &'static str,
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		self.consecutive_transient_burst_missing_downward_candidate_frames = 0;

		self.refresh_local_downward_sample(frame);
		self.refresh_preview_only_downward_local_sample(
			frame,
			self.stable_preview_only_downward_local_viewport_top_y(),
		);
		self.log_decision(
			"scroll_capture.downward_growth_blocked",
			ScrollDirection::Down,
			Some(MotionObservation { direction: ScrollDirection::Down, motion_rows }),
			Some(candidate.viewport_top_y),
			Some(candidate.motion_rows),
			Some(block_reason),
		);

		Ok(self::support::preview_update_outcome(preview_changed))
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
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		let candidate_observed_viewport_top_y = self
			.observed_viewport_top_y
			.saturating_add(i32::try_from(motion_rows).unwrap_or_default());
		let Some(resume_frontier_top_y) = self.resume_frontier_top_y else {
			return Ok(self::support::preview_update_outcome(preview_changed));
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

			return Ok(self::support::preview_update_outcome(preview_changed));
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

		Some(self::support::preview_update_outcome(preview_changed))
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

			return Some(self::support::preview_update_outcome(preview_changed));
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

			return Some(self::support::preview_update_outcome(preview_changed));
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
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
		let direct_match_hint_rows = Some(self.resume_frontier_direct_match_hint_rows(context));
		let raw_committed_down_match = self.evaluate_reference_overlap_direction_preferred_only(
			&self.last_committed_frame,
			&frame,
			ScrollDirection::Down,
			direct_match_hint_rows,
		);
		let trusted_committed_down_match = raw_committed_down_match
			.filter(|matched| self::support::resume_direct_match_is_trustworthy(*matched));
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

		self::support::preview_update_outcome(preview_changed)
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
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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
	) -> color_eyre::eyre::Result<ScrollObserveOutcome> {
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

			return Ok(self::support::preview_update_outcome(preview_changed));
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

			return Ok(self::support::preview_update_outcome(preview_changed));
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

			return Ok(self::support::preview_update_outcome(preview_changed));
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
				let prefer_local =
					self.should_prefer_preview_only_local_recovery_after_extreme_tail_block(
						primary, local,
					) || (!self.should_prefer_observed_sample_over_preview_only_local_recovery(
						primary, local,
					) && (self
						.should_prefer_preview_only_local_recovery_over_observed_sample(
							primary, local,
						) || local.matched.mean_abs_diff_x100
						<= primary.matched.mean_abs_diff_x100));

				if prefer_local {
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
		let max_motion_rows = self::support::max_directional_motion_rows(previous, next, config);

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

	pub(crate) fn last_block_reason(&self) -> Option<&'static str> {
		self.last_block_reason
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
			self.worker_pairwise_requires_committed_reacquire = false;
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
			self.worker_pairwise_requires_committed_reacquire = false;
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreviewOnlyDownwardLocalSample {
	frame: RgbaImage,
	viewport_top_y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectionMatch {
	mean_abs_diff_x100: u32,
	motion_rows: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DownwardSampleMatch {
	matched: DirectionMatch,
	source: DownwardSampleMatchSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DownwardViewportCandidate {
	source: DownwardViewportCandidateSource,
	viewport_top_y: i32,
	motion_rows: u32,
	mean_abs_diff_x100: u32,
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
struct BlockedPreviewOnlyLocalCandidate {
	candidate: DownwardViewportCandidate,
	repeats: u8,
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
enum DownwardRegistration {
	NoMatch,
	Matched(DirectionMatch),
	Ambiguous { best: DirectionMatch, competing: DirectionMatch },
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
enum DownwardRegistrationWithSource {
	NoMatch,
	Matched(DownwardSampleMatch),
	Ambiguous { best: DownwardSampleMatch, competing: DownwardSampleMatch },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownwardViewportCandidateSource {
	ObservedSample,
	PreviewOnlyLocalSample,
	CommittedKeyframe,
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
enum CommittedDownwardViewportCandidateMode {
	LastCommittedOnly,
	IncludeRecentHistory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownwardViewportResolution {
	NoMatch,
	Selected(DownwardViewportCandidate),
	Ambiguous { preferred: DownwardViewportCandidate, competing: DownwardViewportCandidate },
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;
