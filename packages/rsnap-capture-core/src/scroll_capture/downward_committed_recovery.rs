use crate::scroll_capture::{
	DIRECTION_WARNING_MARGIN_X100, DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS, DownwardViewportCandidate,
	DownwardViewportCandidateSource, PREVIEW_ONLY_LOCAL_NEAR_CONTINUITY_ROWS,
	PREVIEW_ONLY_LOCAL_RECOVERY_MAX_MOTION_ROWS, ScrollSession,
	UNDERCONSUMED_OBSERVED_BURST_RECOVERY_GAP_ROWS,
};

impl ScrollSession {
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
}
