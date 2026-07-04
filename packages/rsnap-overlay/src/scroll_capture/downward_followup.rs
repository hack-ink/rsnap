use crate::scroll_capture::{
	DownwardViewportCandidate, DownwardViewportCandidateSource,
	PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS,
	ScrollSession,
};

impl ScrollSession {
	#[allow(clippy::too_many_arguments)]
	pub(super) fn apply_pending_preview_local_followup_blocks(
		&mut self,
		suppressed_preview_only_local_candidate: Option<DownwardViewportCandidate>,
		pending_suppressed_huge_preview_only_local_followup: Option<DownwardViewportCandidate>,
		pending_suppressed_huge_preview_only_local_followup_remaining_blocks: u8,
		pending_extreme_preview_only_local_tail_followup: Option<DownwardViewportCandidate>,
		pending_extreme_preview_only_local_tail_followup_remaining_blocks: u8,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		if self
			.should_fail_closed_suppressed_huge_preview_local_jump_corroborated_by_observed_and_committed(
				suppressed_preview_only_local_candidate,
				candidates,
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
			candidates,
		) {
			if let Some(pending_candidate) = pending_suppressed_huge_preview_only_local_followup
				&& pending_suppressed_huge_preview_only_local_followup_remaining_blocks > 1
			{
				self.pending_suppressed_huge_preview_only_local_followup = Some(pending_candidate);
				self.pending_suppressed_huge_preview_only_local_followup_remaining_blocks =
					pending_suppressed_huge_preview_only_local_followup_remaining_blocks - 1;
			}

			self.blocked_followup_after_suppressed_huge_preview_local_jump = true;

			candidates.clear();
		}
		if self.should_fail_closed_committed_followup_after_extreme_preview_local_tail_block(
			pending_extreme_preview_only_local_tail_followup,
			candidates,
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
	}

	pub(super) fn should_fail_closed_suppressed_huge_preview_local_jump_corroborated_by_observed_and_committed(
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

	pub(super) fn should_fail_closed_committed_followup_after_suppressed_huge_preview_local_jump(
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

	pub(super) fn should_fail_closed_committed_followup_after_extreme_preview_local_tail_block(
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

	pub(super) fn suppressed_huge_preview_only_local_followup_block_budget(
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
}
