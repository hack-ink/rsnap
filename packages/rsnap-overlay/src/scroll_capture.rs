pub mod bench_support;

mod downward_candidates;
mod downward_committed_recovery;
mod downward_recovery;
mod downward_registration;
mod downward_resolution;
mod fingerprint;
mod image_stack;
mod informative_span;
mod logging;
mod pairwise_shift;
mod resume_frontier;
mod sample_input;
mod sample_registration;
mod sample_state;
mod state_access;
mod support;
mod types;
mod upward_input;
mod worker_pairwise;

pub(crate) use self::fingerprint::ScrollFrameFingerprint;
pub(crate) use self::image_stack::compose_provisional_preview_image;
pub(crate) use self::support::scroll_capture_fingerprint;
#[cfg(any(test, target_os = "macos"))]
pub(crate) use self::support::scroll_capture_fingerprint_delta;
#[cfg(test)]
pub(crate) use self::types::OverlapMatch;
pub(crate) use self::types::{
	BlockedPreviewOnlyLocalCandidate, CommittedDownwardViewportCandidateMode, DirectionMatch,
	DirectionMatchEval, DownwardRegistration, DownwardRegistrationWithSource, DownwardSampleMatch,
	DownwardSampleMatchSource, DownwardViewportCandidate, DownwardViewportCandidateSource,
	DownwardViewportResolution, GrowthCommit, InformativeSpan, MotionObservation,
	OverlapSearchConfig, OverlapSearchRange, PreviewOnlyDownwardLocalSample,
	ResumeFrontierDirectMatchContext, ResumeFrontierMatchLog, ScrollCommitTelemetry,
	ScrollDirection, ScrollObserveOutcome, UpInputMatchLog, UpInputSearchWindowLog,
	UpwardInputDiagnostics,
};

use color_eyre::eyre::{self, Result};
use image::RgbaImage;

#[cfg(test)]
use self::support::detect_vertical_overlap;

pub(crate) const PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS: u32 = 24;
pub(crate) const PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS: u32 = 12;
pub(crate) const UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS: u32 = 8;
pub(crate) const TRANSIENT_BURST_UNDERCONSUMED_HINT_MIN_ROWS: u32 = 48;

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
const DIRECTION_WARNING_MARGIN_X100: u32 = 90;
const RESUME_DIRECT_PROOF_MAX_MEAN_ABS_DIFF_X100: u32 = 320;
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
	pub(crate) fn new(base_frame: RgbaImage, preview_width_px: u32) -> Result<Self> {
		let fingerprint = scroll_capture_fingerprint(&base_frame);
		let anchor_preview =
			self::image_stack::resize_strip_to_preview_width(&base_frame, preview_width_px.max(1));

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

		if let Some(outcome) =
			self.block_invalid_downward_candidate(&frame, motion_rows, candidate, preview_changed)?
		{
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
			candidate.source.decision_source(),
		)
	}

	fn block_invalid_downward_candidate(
		&mut self,
		frame: &RgbaImage,
		motion_rows: u32,
		candidate: DownwardViewportCandidate,
		preview_changed: bool,
	) -> Result<Option<ScrollObserveOutcome>> {
		if self.transient_burst_candidate_underconsumes_input_hint(candidate) {
			return Ok(Some(self.block_downward_growth_candidate(
				frame,
				motion_rows,
				candidate,
				preview_changed,
				"visual_motion_underconsumed_input_hint",
			)?));
		}
		if self.should_fail_closed_tiny_observed_recovery_in_burst(candidate) {
			return Ok(Some(self.block_downward_growth_candidate(
				frame,
				motion_rows,
				candidate,
				preview_changed,
				"tiny_observed_recovery_under_transient_burst",
			)?));
		}
		if self.should_fail_closed_outsized_observed_recovery_after_one_pixel_preview_local_commit(
			candidate,
		) {
			return Ok(Some(self.block_downward_growth_candidate(
				frame,
				motion_rows,
				candidate,
				preview_changed,
				"outsized_observed_recovery_after_one_pixel_preview_local_commit",
			)?));
		}
		if self.should_fail_closed_tiny_preview_only_local_recovery_in_burst(candidate) {
			return Ok(Some(self.block_downward_growth_candidate(
				frame,
				motion_rows,
				candidate,
				preview_changed,
				"tiny_preview_only_local_recovery_under_transient_burst",
			)?));
		}
		if self
			.should_fail_closed_exactly_corroborated_preview_local_tail_in_extreme_burst(candidate)
		{
			self.pending_extreme_preview_only_local_tail_followup = Some(candidate);
			self.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 1;

			return Ok(Some(self.block_downward_growth_candidate(
				frame,
				motion_rows,
				candidate,
				preview_changed,
				"exactly_corroborated_preview_local_tail_under_extreme_transient_burst",
			)?));
		}
		if self.should_fail_closed_preview_only_local_tail_after_unresolved_burst(candidate) {
			return Ok(Some(self.block_downward_growth_candidate(
				frame,
				motion_rows,
				candidate,
				preview_changed,
				"preview_only_local_tail_after_unresolved_transient_burst",
			)?));
		}
		if self.should_fail_closed_tiny_committed_keyframe_recovery_in_burst(candidate) {
			return Ok(Some(self.block_downward_growth_candidate(
				frame,
				motion_rows,
				candidate,
				preview_changed,
				"tiny_committed_keyframe_recovery_under_transient_burst",
			)?));
		}

		Ok(None)
	}

	fn handle_missing_downward_viewport_authority(
		&mut self,
		frame: &RgbaImage,
		observed_match: DownwardSampleMatch,
		motion_rows: u32,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
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

		self.record_transient_burst_missing_downward_candidate_frame(preview_changed);
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
	) -> Result<ScrollObserveOutcome> {
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
					if self.initial_downward_bootstrap_active()
						&& self.last_motion_rows_hint.is_none()
					{
						frame_max_growth_rows
					} else {
						frame_max_growth_rows.min(hint.max(INITIAL_DOWNWARD_MAX_MOTION_ROWS)).max(1)
					}
				})
				.unwrap_or(frame_max_growth_rows.clamp(1, INITIAL_DOWNWARD_MAX_MOTION_ROWS));
		}

		frame_max_growth_rows.clamp(1, INITIAL_DOWNWARD_MAX_MOTION_ROWS)
	}
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;
