use crate::scroll_capture::downward_candidates;
use crate::scroll_capture::{
	BlockedPreviewOnlyLocalCandidate, DIRECTION_WARNING_MARGIN_X100,
	DOWNWARD_COMMITTED_KEYFRAME_LOCAL_OVERRUN_MAX_ROWS, DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS,
	DownwardViewportCandidate, DownwardViewportCandidateSource,
	PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS,
	PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS,
	REPEATED_PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS, ScrollSession,
	TINY_PREVIEW_ONLY_LOCAL_BURST_RECOVERY_MAX_MOTION_ROWS,
	UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS,
};

impl ScrollSession {
	pub(super) fn restore_corroborated_observed_candidate(
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

	pub(super) fn observed_candidate_can_recover_from_committed_corroboration(
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

	pub(super) fn restore_repeated_small_preview_only_local_candidate_after_empty_prune(
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

	pub(super) fn repeated_preview_only_local_candidate_can_restore_after_empty_prune(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.motion_rows <= REPEATED_PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& self.transient_burst_search_enabled
			&& self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
			&& self.last_motion_rows_hint.is_some()
	}

	pub(super) fn should_fail_closed_lagging_exactly_corroborated_preview_local_tail_in_burst(
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

	pub(super) fn should_fail_closed_underconsumed_observed_recovery_in_burst(
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

	pub(super) fn prune_committed_keyframe_candidates_outside_local_continuity(
		&mut self,
		candidates: &mut Vec<DownwardViewportCandidate>,
	) {
		let has_committed_candidate = candidates.iter().any(|candidate| {
			candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
		});
		let mut local_anchor =
			downward_candidates::best_local_downward_viewport_candidate(candidates);

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

			local_anchor = downward_candidates::best_local_downward_viewport_candidate(candidates);
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

	pub(super) fn preview_only_local_anchor_has_exact_committed_corroboration(
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

	pub(super) fn prune_committed_keyframe_candidates_without_local_anchor(
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

	pub(super) fn should_fail_closed_far_committed_only_recovery_without_local_anchor(
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

	pub(super) fn should_fail_closed_far_committed_only_recovery_after_corroborated_huge_local_jump(
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

	pub(super) fn should_fail_closed_underconsumed_committed_only_recovery_after_suppressed_preview_local_match(
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

	pub(super) fn should_fail_closed_committed_only_recovery_after_corroborated_sample_registration_without_viewport_anchor(
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

	pub(super) fn should_fail_closed_committed_only_recovery_when_observed_burst_outpaces_recent_preview_local_commit(
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

	pub(super) fn should_suppress_preview_only_local_candidate(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			&& self.transient_burst_motion_hint_exceeds_local_authority(candidate.motion_rows)
			&& candidate.motion_rows > PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
			&& !self.preview_only_local_candidate_remains_trustworthy_in_burst(candidate)
	}

	pub(super) fn should_suppress_observed_sample_candidate(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.source == DownwardViewportCandidateSource::ObservedSample
			&& self.transient_burst_search_enabled
			&& self.fallback_downward_growth_exceeds_continuity_budget(candidate.viewport_top_y)
			&& !self.observed_sample_candidate_remains_trustworthy_in_burst(candidate)
	}

	pub(super) fn observed_sample_candidate_remains_trustworthy_in_burst(
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

	pub(super) fn preview_only_local_candidate_has_material_progress(
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

	pub(super) fn preview_only_local_candidate_remains_trustworthy_in_burst(
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

	pub(super) fn seeded_preview_only_local_catch_up_candidate_can_commit(
		&self,
		candidate: DownwardViewportCandidate,
	) -> bool {
		candidate.source == DownwardViewportCandidateSource::PreviewOnlyLocalSample
			&& self.seeded_preview_only_local_after_observed_burst_commit
			&& candidate.viewport_top_y > self.current_viewport_top_y
			&& candidate.motion_rows <= PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS
	}

	pub(super) fn prune_committed_keyframe_candidates_for_transient_burst(
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

	pub(super) fn max_committed_keyframe_local_overrun_rows(
		&self,
		local_anchor: DownwardViewportCandidate,
	) -> u32 {
		self.max_committed_keyframe_motion_rows(local_anchor).clamp(
			DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS,
			DOWNWARD_COMMITTED_KEYFRAME_LOCAL_OVERRUN_MAX_ROWS,
		)
	}

	pub(super) fn max_committed_keyframe_motion_rows(
		&self,
		local_anchor: DownwardViewportCandidate,
	) -> u32 {
		let continuity_rows = self
			.last_motion_rows_hint
			.unwrap_or(local_anchor.motion_rows)
			.max(local_anchor.motion_rows);
		let tolerance_rows = (continuity_rows / 2).clamp(1, DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS);

		continuity_rows.saturating_add(tolerance_rows)
	}

	pub(super) fn committed_candidate_can_plausibly_replace_underconsumed_preview_local_anchor(
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

	pub(super) fn committed_candidate_can_override_untrustworthy_observed_local_recovery(
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

	pub(super) fn bootstrap_committed_keyframe_growth_cap_rows(&self) -> Option<u32> {
		if !self.initial_downward_bootstrap_active() {
			return None;
		}

		self.transient_pending_growth_cap_rows()
	}

	pub(super) fn transient_pending_growth_cap_rows(&self) -> Option<u32> {
		let hint = self.normalized_transient_motion_rows_hint()?;
		let tolerance = (hint / 2).clamp(1, PREVIEW_ONLY_LOCAL_RECOVERY_MAX_TOLERANCE_ROWS);

		Some(hint.saturating_add(tolerance))
	}

	pub(super) fn transient_burst_growth_matches_pending_hint_band(
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
}
