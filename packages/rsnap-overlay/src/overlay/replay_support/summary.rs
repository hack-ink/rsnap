use color_eyre::eyre::{self, Result};

use crate::overlay::{
	OverlaySession,
	replay_support::{
		RecordedScrollCaptureReplayMode, RecordedScrollCaptureReplayRecordedOutcome,
		RecordedScrollCaptureReplayStepResult, RecordedScrollCaptureReplaySummary,
		RecordedScrollCaptureSemanticIssue, ReplayStats, ScrollCaptureReplayOutcome,
	},
	trace_recording::LoadedScrollCaptureLiveTrace,
};

pub(super) fn finalize_replay_summary(
	trace: LoadedScrollCaptureLiveTrace,
	session: &OverlaySession,
	replay_stats: ReplayStats,
	replay_mode: RecordedScrollCaptureReplayMode,
) -> Result<RecordedScrollCaptureReplaySummary> {
	let final_session = session.scroll_capture.session.as_ref().ok_or_else(|| {
		eyre::eyre!("scroll-capture session missing after replaying recorded live trace")
	})?;
	let first_outcome_divergence_frame = replay_stats
		.step_results
		.iter()
		.find(|step| !recorded_step_outcome_matches_replayed(step))
		.map(|step| step.frame_index);
	let first_export_height_drift_frame = replay_stats
		.step_results
		.iter()
		.find(|step| {
			step.recorded_export_height.is_some_and(|recorded| recorded != step.export_height)
		})
		.map(|step| step.frame_index);
	let first_preview_height_drift_frame = replay_stats
		.step_results
		.iter()
		.find(|step| {
			step.recorded_preview_height.is_some_and(|recorded| recorded != step.preview_height)
		})
		.map(|step| step.frame_index);
	let first_semantic_issue_frame = replay_stats
		.step_results
		.iter()
		.find(|step| step.semantic_issue.is_some())
		.map(|step| step.frame_index);
	let first_missed_downward_motion_frame = replay_stats
		.step_results
		.iter()
		.find(|step| {
			matches!(
				step.semantic_issue,
				Some(RecordedScrollCaptureSemanticIssue::MissedDownwardMotion)
			)
		})
		.map(|step| step.frame_index);
	let first_underconsumed_downward_motion_frame = replay_stats
		.step_results
		.iter()
		.find(|step| {
			matches!(
				step.semantic_issue,
				Some(RecordedScrollCaptureSemanticIssue::UnderconsumedDownwardMotion)
			)
		})
		.map(|step| step.frame_index);
	let first_growth_overshoot_frame = replay_stats
		.step_results
		.iter()
		.find(|step| {
			matches!(
				step.semantic_issue,
				Some(RecordedScrollCaptureSemanticIssue::GrowthExceedsRecordedShift)
			)
		})
		.map(|step| step.frame_index);

	Ok(RecordedScrollCaptureReplaySummary {
		replay_mode,
		trace_id: trace.manifest.trace_id.clone(),
		manifest_path: trace.manifest_path.clone(),
		final_export_height: final_session.export_image().height(),
		final_preview_height: session
			.scroll_capture_preview_dimensions()
			.map_or(final_session.preview_image().height(), |dimensions| dimensions[1]),
		final_viewport_top_y: final_session.current_viewport_top_y(),
		recorded_final_export_height: trace
			.manifest
			.final_snapshot
			.as_ref()
			.and_then(|snapshot| snapshot.export_dimensions)
			.map(|dimensions| dimensions[1]),
		recorded_final_preview_height: trace
			.manifest
			.final_snapshot
			.as_ref()
			.and_then(|snapshot| snapshot.preview_dimensions)
			.map(|dimensions| dimensions[1]),
		final_preview_path: trace
			.manifest
			.final_preview_path
			.as_deref()
			.map(|path| trace.resolve_frame_path(path)),
		final_export_path: trace
			.manifest
			.final_export_path
			.as_deref()
			.map(|path| trace.resolve_frame_path(path)),
		first_outcome_divergence_frame,
		first_export_height_drift_frame,
		first_preview_height_drift_frame,
		max_recorded_committed_growth_rows: replay_stats.max_recorded_committed_growth_rows,
		max_replayed_committed_growth_rows: replay_stats.max_replayed_committed_growth_rows,
		max_recorded_export_jump: replay_stats.max_recorded_export_jump,
		max_recorded_preview_jump: replay_stats.max_recorded_preview_jump,
		max_replayed_export_jump: replay_stats.max_replayed_export_jump,
		max_replayed_preview_jump: replay_stats.max_replayed_preview_jump,
		first_semantic_issue_frame,
		first_missed_downward_motion_frame,
		first_underconsumed_downward_motion_frame,
		first_growth_overshoot_frame,
		step_results: replay_stats.step_results,
	})
}

pub(super) fn recorded_step_outcome_matches_replayed(
	step: &RecordedScrollCaptureReplayStepResult,
) -> bool {
	if recorded_outcome_matches_replayed(&step.recorded_outcome, step.replayed_outcome) {
		return true;
	}

	matches!(
		(&step.recorded_outcome, step.replayed_outcome),
		(
			RecordedScrollCaptureReplayRecordedOutcome::NoChange,
			ScrollCaptureReplayOutcome::PreviewUpdated,
		) | (
			RecordedScrollCaptureReplayRecordedOutcome::PreviewUpdated,
			ScrollCaptureReplayOutcome::NoChange,
		)
	) && step.recorded_export_height == Some(step.export_height)
		&& step.recorded_preview_height == Some(step.preview_height)
}

fn recorded_outcome_matches_replayed(
	recorded: &RecordedScrollCaptureReplayRecordedOutcome,
	replayed: ScrollCaptureReplayOutcome,
) -> bool {
	match (recorded, replayed) {
		(
			RecordedScrollCaptureReplayRecordedOutcome::NoChange,
			ScrollCaptureReplayOutcome::NoChange,
		) => true,
		(
			RecordedScrollCaptureReplayRecordedOutcome::PreviewUpdated,
			ScrollCaptureReplayOutcome::PreviewUpdated,
		) => true,
		(
			RecordedScrollCaptureReplayRecordedOutcome::Unsupported { direction },
			ScrollCaptureReplayOutcome::UnsupportedUp,
		) => *direction == "up",
		(
			RecordedScrollCaptureReplayRecordedOutcome::Committed { direction, growth_rows },
			ScrollCaptureReplayOutcome::CommittedDown { growth_rows: replayed_growth_rows },
		) => *direction == "down" && *growth_rows == replayed_growth_rows,
		(RecordedScrollCaptureReplayRecordedOutcome::Error { .. }, _) => false,
		_ => false,
	}
}
