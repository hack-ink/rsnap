use crate::scroll_capture::{
	DIRECTION_WARNING_MARGIN_X100, DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS, DownwardViewportCandidate,
	DownwardViewportCandidateSource, DownwardViewportResolution,
};

pub(super) fn select_downward_viewport_candidate(
	candidates: &mut [DownwardViewportCandidate],
) -> DownwardViewportResolution {
	if candidates.is_empty() {
		return DownwardViewportResolution::NoMatch;
	}

	if let Some(preferred_local) = prefer_local_downward_viewport_candidate(candidates) {
		let ambiguity_margin = downward_viewport_competing_margin(candidates, preferred_local);
		let competing = candidates.iter().copied().find(|candidate| {
			candidate != &preferred_local
				&& candidate.viewport_top_y.abs_diff(preferred_local.viewport_top_y)
					>= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
				&& candidate.mean_abs_diff_x100
					<= preferred_local.mean_abs_diff_x100.saturating_add(ambiguity_margin)
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
	let ambiguity_margin = downward_viewport_competing_margin(candidates, preferred);
	let competing = candidates.iter().copied().skip(1).find(|candidate| {
		candidate.viewport_top_y.abs_diff(preferred.viewport_top_y)
			>= DOWNWARD_VIEWPORT_AUTHORITY_GAP_ROWS
			&& candidate.mean_abs_diff_x100
				<= preferred.mean_abs_diff_x100.saturating_add(ambiguity_margin)
	});

	match competing {
		Some(competing) => DownwardViewportResolution::Ambiguous { preferred, competing },
		None => DownwardViewportResolution::Selected(preferred),
	}
}

pub(super) fn format_downward_viewport_candidates(
	candidates: &[DownwardViewportCandidate],
) -> String {
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

pub(super) fn best_local_downward_viewport_candidate(
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

fn downward_viewport_competing_margin(
	candidates: &[DownwardViewportCandidate],
	preferred: DownwardViewportCandidate,
) -> u32 {
	let exact_corroborated = candidates.iter().any(|candidate| {
		candidate != &preferred
			&& candidate.viewport_top_y == preferred.viewport_top_y
			&& candidate.motion_rows == preferred.motion_rows
			&& candidate.mean_abs_diff_x100
				<= preferred.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
	});

	if exact_corroborated { 0 } else { DIRECTION_WARNING_MARGIN_X100 }
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
