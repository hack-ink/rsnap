use image::RgbaImage;

use crate::overlay::trace_recording::ScrollCaptureTraceFrameEntry;
use crate::overlay::{
	OverlaySession, ScrollSession,
	replay_support::{
		RecordedScrollCaptureReplayFrameSource, RecordedScrollCaptureReplayRecordedOutcome,
		RecordedScrollCaptureReplayStepResult, RecordedScrollCaptureSemanticIssue,
		ScrollCaptureReplayOutcome,
	},
};
use crate::scroll_capture::ScrollCommitTelemetry;

#[derive(Default)]
pub(super) struct ReplayStats {
	pub(super) step_results: Vec<RecordedScrollCaptureReplayStepResult>,
	pub(super) previous_recorded_export_height: Option<u32>,
	pub(super) previous_recorded_preview_height: Option<u32>,
	pub(super) previous_replayed_export_height: Option<u32>,
	pub(super) previous_replayed_preview_height: Option<u32>,
	pub(super) previous_live_frame_seq: Option<u64>,
	pub(super) previous_recorded_frame: Option<RgbaImage>,
	pub(super) max_recorded_export_jump: u32,
	pub(super) max_recorded_preview_jump: u32,
	pub(super) max_replayed_export_jump: u32,
	pub(super) max_replayed_preview_jump: u32,
	pub(super) max_recorded_committed_growth_rows: u32,
	pub(super) max_replayed_committed_growth_rows: u32,
}
impl ReplayStats {
	pub(super) fn update_recorded_height_jumps(
		&mut self,
		recorded_export_height: Option<u32>,
		recorded_preview_height: Option<u32>,
	) {
		if let Some(recorded_export_height) = recorded_export_height {
			if let Some(previous) = self.previous_recorded_export_height {
				self.max_recorded_export_jump = self
					.max_recorded_export_jump
					.max(recorded_export_height.saturating_sub(previous));
			}

			self.previous_recorded_export_height = Some(recorded_export_height);
		}
		if let Some(recorded_preview_height) = recorded_preview_height {
			if let Some(previous) = self.previous_recorded_preview_height {
				self.max_recorded_preview_jump = self
					.max_recorded_preview_jump
					.max(recorded_preview_height.saturating_sub(previous));
			}

			self.previous_recorded_preview_height = Some(recorded_preview_height);
		}
	}

	pub(super) fn update_replayed_height_jumps(
		&mut self,
		replayed_export_height: u32,
		replayed_preview_height: u32,
	) {
		if let Some(previous) = self.previous_replayed_export_height {
			self.max_replayed_export_jump =
				self.max_replayed_export_jump.max(replayed_export_height.saturating_sub(previous));
		}

		self.previous_replayed_export_height = Some(replayed_export_height);

		if let Some(previous) = self.previous_replayed_preview_height {
			self.max_replayed_preview_jump = self
				.max_replayed_preview_jump
				.max(replayed_preview_height.saturating_sub(previous));
		}

		self.previous_replayed_preview_height = Some(replayed_preview_height);
	}

	pub(super) fn update_live_frame_gap(
		&mut self,
		frame_source: RecordedScrollCaptureReplayFrameSource,
	) -> Option<u64> {
		match frame_source {
			RecordedScrollCaptureReplayFrameSource::LiveStream { frame_seq } => {
				let gap = self
					.previous_live_frame_seq
					.map(|previous| frame_seq.saturating_sub(previous))
					.unwrap_or(1);

				self.previous_live_frame_seq = Some(frame_seq);

				Some(gap)
			},
			RecordedScrollCaptureReplayFrameSource::Worker { .. } => None,
		}
	}

	pub(super) fn push_step_result(&mut self, record: ReplayStepResultRecord<'_>) {
		let telemetry = record.telemetry;
		let scroll = &record.session.scroll_capture;

		self.step_results.push(RecordedScrollCaptureReplayStepResult {
			frame_index: self.step_results.len(),
			frame_path: record.frame.frame_path.clone(),
			observed_at_ms: record.frame.observed_at_ms,
			frame_source: record.frame_source,
			live_frame_gap: record.live_frame_gap,
			recorded_outcome: record.recorded_outcome,
			replayed_outcome: record.replayed_outcome,
			export_height: record.replayed_export_height,
			preview_height: record.replayed_preview_height,
			session_preview_height: record.replayed_session_preview_height,
			recorded_export_height: record.recorded_export_height,
			recorded_preview_height: record.recorded_preview_height,
			viewport_top_y: record.active_session.current_viewport_top_y(),
			last_commit_decision_source: telemetry.last_commit_decision_source,
			last_commit_detected_motion_rows: telemetry.last_commit_detected_motion_rows,
			last_block_reason: telemetry.last_block_reason,
			replayed_downward_sample_registration_result: telemetry
				.last_downward_sample_registration_result,
			replayed_downward_sample_registration_source: telemetry
				.last_downward_sample_registration_source,
			replayed_downward_sample_registration_motion_rows: telemetry
				.last_downward_sample_registration_motion_rows,
			replayed_downward_sample_registration_provisional_viewport_top_y: telemetry
				.last_downward_sample_registration_provisional_viewport_top_y,
			replayed_observed_sample_registration_result: telemetry
				.observed_sample_registration_result,
			replayed_observed_sample_registration_reason: telemetry
				.observed_sample_registration_reason,
			replayed_observed_sample_registration_motion_rows: telemetry
				.observed_sample_registration_motion_rows,
			replayed_observed_sample_registration_mean_abs_diff_x100: telemetry
				.observed_sample_registration_mean_abs_diff_x100,
			replayed_preview_only_local_registration_result: telemetry
				.preview_only_local_registration_result,
			replayed_preview_only_local_registration_reason: telemetry
				.preview_only_local_registration_reason,
			replayed_preview_only_local_registration_motion_rows: telemetry
				.preview_only_local_registration_motion_rows,
			replayed_preview_only_local_registration_mean_abs_diff_x100: telemetry
				.preview_only_local_registration_mean_abs_diff_x100,
			replayed_downward_viewport_candidate_count: telemetry
				.last_downward_viewport_candidate_count,
			replayed_downward_viewport_candidates_before_prune: telemetry
				.last_downward_viewport_candidates_before_prune
				.clone(),
			replayed_downward_viewport_candidates_after_prune: telemetry
				.last_downward_viewport_candidates_after_prune
				.clone(),
			replayed_sample_eval_last_motion_rows_hint: telemetry.sample_eval_last_motion_rows_hint,
			replayed_sample_eval_transient_motion_rows_hint: telemetry
				.sample_eval_transient_motion_rows_hint,
			replayed_sample_eval_effective_motion_rows_hint: telemetry
				.sample_eval_effective_motion_rows_hint,
			replayed_sample_eval_transient_burst_search_enabled: telemetry
				.sample_eval_transient_burst_search_enabled,
			replayed_preview_only_local_viewport_top_y: telemetry.preview_only_local_viewport_top_y,
			replayed_downward_motion_rows_pending: scroll.downward_motion_rows_pending,
			replayed_input_gesture_active: scroll.input_gesture_active,
			replayed_session_preview_display_mode: record.active_session.preview_display_mode(),
			replayed_session_preview_hinted_motion_rows_hint: None,
			replayed_session_preview_hinted_frame_source: None,
			replayed_overlay_preview_motion_rows_hint: scroll.last_overlay_preview_motion_rows_hint,
			replayed_overlay_preview_provisional_motion_rows_hint: scroll
				.last_overlay_preview_provisional_motion_rows_hint,
			replayed_overlay_preview_existing_candidate_height: scroll
				.last_overlay_preview_existing_candidate_height,
			replayed_overlay_preview_existing_candidate_motion_rows_hint: scroll
				.last_overlay_preview_existing_candidate_motion_rows_hint,
			replayed_overlay_preview_ledger_candidate_height: scroll
				.last_overlay_preview_ledger_candidate_height,
			replayed_overlay_preview_ledger_candidate_motion_rows_hint: scroll
				.last_overlay_preview_ledger_candidate_motion_rows_hint,
			replayed_overlay_preview_retained_candidate_height: scroll
				.last_overlay_preview_retained_candidate_height,
			replayed_overlay_preview_retained_candidate_motion_rows_hint: scroll
				.last_overlay_preview_retained_candidate_motion_rows_hint,
			replayed_overlay_preview_retained_hint_matches_motion_rows: scroll
				.last_overlay_preview_retained_hint_matches_motion_rows,
			replayed_overlay_preview_fresh_latest_frame_can_drive: scroll
				.last_overlay_preview_fresh_latest_frame_can_drive,
			replayed_retained_overlay_preview_height: scroll
				.retained_overlay_preview_image
				.as_ref()
				.map(RgbaImage::height),
			replayed_retained_overlay_preview_motion_rows_hint: scroll
				.retained_overlay_preview_motion_rows_hint,
			replayed_overlay_preview_strong_unresolved_registration: scroll
				.last_overlay_preview_strong_unresolved_registration,
			replayed_overlay_preview_latest_frame_present: scroll
				.last_overlay_preview_latest_frame_present,
			replayed_overlay_preview_used_provisional: scroll.last_overlay_preview_used_provisional,
			recorded_estimated_downward_shift_rows: record.recorded_estimated_downward_shift_rows,
			semantic_issue: record.semantic_issue,
		});
	}
}

pub(super) struct ReplayStepResultRecord<'a> {
	pub(super) session: &'a OverlaySession,
	pub(super) active_session: &'a ScrollSession,
	pub(super) telemetry: &'a ScrollCommitTelemetry,
	pub(super) frame: &'a ScrollCaptureTraceFrameEntry,
	pub(super) frame_source: RecordedScrollCaptureReplayFrameSource,
	pub(super) live_frame_gap: Option<u64>,
	pub(super) recorded_outcome: RecordedScrollCaptureReplayRecordedOutcome,
	pub(super) replayed_outcome: ScrollCaptureReplayOutcome,
	pub(super) recorded_export_height: Option<u32>,
	pub(super) recorded_preview_height: Option<u32>,
	pub(super) replayed_export_height: u32,
	pub(super) replayed_preview_height: u32,
	pub(super) replayed_session_preview_height: u32,
	pub(super) recorded_estimated_downward_shift_rows: Option<u32>,
	pub(super) semantic_issue: Option<RecordedScrollCaptureSemanticIssue>,
}
