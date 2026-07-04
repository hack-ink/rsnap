use std::path::PathBuf;

use serde::Serialize;

use crate::overlay::trace_recording::{
	ScrollCaptureTraceDirection, ScrollCaptureTraceFrameSource, ScrollCaptureTraceRecordedOutcome,
};
use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
/// Public replay outcome surface that does not leak private scroll-capture internals.
pub enum ScrollCaptureReplayOutcome {
	/// The step did not change preview or export state.
	NoChange,
	/// The step updated preview state without committing stitched growth.
	PreviewUpdated,
	/// The step detected upward or rewind-like motion and failed closed.
	UnsupportedUp,
	/// The step committed downward stitched growth.
	CommittedDown {
		/// Number of newly proven rows appended during this step.
		growth_rows: u32,
	},
}
impl From<ScrollObserveOutcome> for ScrollCaptureReplayOutcome {
	fn from(value: ScrollObserveOutcome) -> Self {
		match value {
			ScrollObserveOutcome::NoChange => Self::NoChange,
			ScrollObserveOutcome::PreviewUpdated => Self::PreviewUpdated,
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up } => {
				Self::UnsupportedUp
			},
			ScrollObserveOutcome::UnsupportedDirection { .. } => Self::NoChange,
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows } => {
				Self::CommittedDown { growth_rows }
			},
			ScrollObserveOutcome::Committed { .. } => Self::NoChange,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
/// Strategy used when replaying recorded frames back through scroll-capture logic.
pub enum RecordedScrollCaptureReplayMode {
	/// Reuse the recorded frame source for each step.
	RecordedSource,
	/// Force every recorded frame through the macOS worker pairwise path.
	ForceWorkerPairwise,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// Semantic failure classes inferred from recorded frame-to-frame motion.
pub enum RecordedScrollCaptureSemanticIssue {
	/// Recorded frames moved downward but the recorded outcome did not convert that into growth.
	MissedDownwardMotion,
	/// Recorded frames moved downward significantly more than the recorded committed growth.
	UnderconsumedDownwardMotion,
	/// Recorded committed growth exceeded the visible recorded frame-to-frame shift by a large margin.
	GrowthExceedsRecordedShift,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// Public frame-source surface for one replayed live-trace step.
pub enum RecordedScrollCaptureReplayFrameSource {
	Worker { request_id: u64 },
	LiveStream { frame_seq: u64 },
}
impl From<ScrollCaptureTraceFrameSource> for RecordedScrollCaptureReplayFrameSource {
	fn from(value: ScrollCaptureTraceFrameSource) -> Self {
		match value {
			ScrollCaptureTraceFrameSource::Worker { request_id } => Self::Worker { request_id },
			ScrollCaptureTraceFrameSource::LiveStream { frame_seq } => {
				Self::LiveStream { frame_seq }
			},
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// Public recorded-outcome surface for one frame in a replayed live trace.
pub enum RecordedScrollCaptureReplayRecordedOutcome {
	/// The live frame did not change preview or export state.
	NoChange,
	/// The live frame updated preview state without committing growth.
	PreviewUpdated,
	/// The live frame detected unsupported motion.
	Unsupported {
		/// Direction recorded during the live session.
		direction: &'static str,
	},
	/// The live frame committed stitched growth.
	Committed {
		/// Direction recorded during the live session.
		direction: &'static str,
		/// Newly appended rows recorded during the live session.
		growth_rows: u32,
	},
	/// The live frame recorded an observation error.
	Error {
		/// Error string captured during the live session.
		message: String,
	},
}
impl From<ScrollCaptureTraceRecordedOutcome> for RecordedScrollCaptureReplayRecordedOutcome {
	fn from(value: ScrollCaptureTraceRecordedOutcome) -> Self {
		match value {
			ScrollCaptureTraceRecordedOutcome::NoChange => Self::NoChange,
			ScrollCaptureTraceRecordedOutcome::PreviewUpdated => Self::PreviewUpdated,
			ScrollCaptureTraceRecordedOutcome::UnsupportedDirection { direction } => {
				Self::Unsupported { direction: replay_direction_name(direction) }
			},
			ScrollCaptureTraceRecordedOutcome::Committed { direction, growth_rows } => {
				Self::Committed { direction: replay_direction_name(direction), growth_rows }
			},
			ScrollCaptureTraceRecordedOutcome::Error { message } => Self::Error { message },
		}
	}
}

#[derive(Clone, Debug, Serialize)]
/// Deterministic replay summary for one recorded live trace.
pub struct RecordedScrollCaptureReplaySummary {
	/// Replay strategy used for this summary.
	pub replay_mode: RecordedScrollCaptureReplayMode,
	/// Stable trace id from the manifest.
	pub trace_id: String,
	/// Manifest path used to load the trace.
	pub manifest_path: PathBuf,
	/// Final stitched export height after replaying the recorded trace.
	pub final_export_height: u32,
	/// Final preview height after replaying the recorded trace.
	pub final_preview_height: u32,
	/// Final viewport top ledger after replaying the recorded trace.
	pub final_viewport_top_y: i32,
	/// Final stitched export height recorded during the live session, when present.
	pub recorded_final_export_height: Option<u32>,
	/// Final preview height recorded during the live session, when present.
	pub recorded_final_preview_height: Option<u32>,
	/// Final recorded live-trace preview artifact, when present.
	pub final_preview_path: Option<PathBuf>,
	/// Final recorded live-trace export artifact, when present.
	pub final_export_path: Option<PathBuf>,
	/// First frame where recorded and replayed outcomes diverged.
	pub first_outcome_divergence_frame: Option<usize>,
	/// First frame where replayed export height drifted from the recorded live trace.
	pub first_export_height_drift_frame: Option<usize>,
	/// First frame where replayed preview height drifted from the recorded live trace.
	pub first_preview_height_drift_frame: Option<usize>,
	/// Largest committed growth recorded during the live session.
	pub max_recorded_committed_growth_rows: u32,
	/// Largest committed growth observed while replaying the live trace.
	pub max_replayed_committed_growth_rows: u32,
	/// Largest step-to-step export-height jump recorded during the live session.
	pub max_recorded_export_jump: u32,
	/// Largest step-to-step preview-height jump recorded during the live session.
	pub max_recorded_preview_jump: u32,
	/// Largest step-to-step export-height jump observed while replaying the live trace.
	pub max_replayed_export_jump: u32,
	/// Largest step-to-step preview-height jump observed while replaying the live trace.
	pub max_replayed_preview_jump: u32,
	/// First frame where semantic analysis flagged a likely incorrect recorded behavior.
	pub first_semantic_issue_frame: Option<usize>,
	/// First frame where visible downward motion was recorded but no stitched growth occurred.
	pub first_missed_downward_motion_frame: Option<usize>,
	/// First frame where committed growth consumed materially less motion than visible in the recorded frames.
	pub first_underconsumed_downward_motion_frame: Option<usize>,
	/// First frame where committed growth exceeded the recorded frame-to-frame shift estimate by a large margin.
	pub first_growth_overshoot_frame: Option<usize>,
	/// Ordered per-frame results observed during replay.
	pub step_results: Vec<RecordedScrollCaptureReplayStepResult>,
}

#[derive(Clone, Debug, Serialize)]
/// One deterministic replay step result for a recorded live trace.
pub struct RecordedScrollCaptureReplayStepResult {
	/// Zero-based frame index within the trace.
	pub frame_index: usize,
	/// Relative frame image path inside the trace manifest.
	pub frame_path: String,
	/// Milliseconds since the trace start when this frame was observed live.
	pub observed_at_ms: u64,
	/// Public frame-source surface for the recorded frame.
	pub frame_source: RecordedScrollCaptureReplayFrameSource,
	/// Gap from the previous live-stream frame sequence, when the source was SCStream-backed.
	pub live_frame_gap: Option<u64>,
	/// Outcome recorded during the live session.
	pub recorded_outcome: RecordedScrollCaptureReplayRecordedOutcome,
	/// Outcome observed while replaying the recorded trace offline.
	pub replayed_outcome: ScrollCaptureReplayOutcome,
	/// Export height after the replay step completes.
	pub export_height: u32,
	/// Preview height after the replay step completes.
	pub preview_height: u32,
	/// Session-side preview-display height after the replay step completes.
	pub session_preview_height: u32,
	/// Export height recorded during the live session after this frame, when present.
	pub recorded_export_height: Option<u32>,
	/// Preview height recorded during the live session after this frame, when present.
	pub recorded_preview_height: Option<u32>,
	/// Viewport top ledger after the replay step completes.
	pub viewport_top_y: i32,
	/// Last commit decision source visible after this frame replay completes.
	pub last_commit_decision_source: Option<&'static str>,
	/// Last commit detected motion rows visible after this frame replay completes.
	pub last_commit_detected_motion_rows: Option<u32>,
	/// Last fail-closed block reason visible after this frame replay completes.
	pub last_block_reason: Option<&'static str>,
	/// Replay-side downward sample registration result observed for this frame.
	pub replayed_downward_sample_registration_result: Option<&'static str>,
	/// Replay-side downward sample registration source observed for this frame.
	pub replayed_downward_sample_registration_source: Option<&'static str>,
	/// Replay-side downward sample registration motion rows observed for this frame.
	pub replayed_downward_sample_registration_motion_rows: Option<u32>,
	/// Replay-side provisional viewport top inferred from the registration source, when any.
	pub replayed_downward_sample_registration_provisional_viewport_top_y: Option<i32>,
	/// Replay-side observed-sample registration result before source arbitration.
	pub replayed_observed_sample_registration_result: Option<&'static str>,
	/// Replay-side observed-sample registration no-match reason before source arbitration.
	pub replayed_observed_sample_registration_reason: Option<&'static str>,
	/// Replay-side observed-sample registration motion rows before source arbitration.
	pub replayed_observed_sample_registration_motion_rows: Option<u32>,
	/// Replay-side observed-sample registration mean diff before source arbitration.
	pub replayed_observed_sample_registration_mean_abs_diff_x100: Option<u32>,
	/// Replay-side preview-local registration result before source arbitration.
	pub replayed_preview_only_local_registration_result: Option<&'static str>,
	/// Replay-side preview-local registration no-match reason before source arbitration.
	pub replayed_preview_only_local_registration_reason: Option<&'static str>,
	/// Replay-side preview-local registration motion rows before source arbitration.
	pub replayed_preview_only_local_registration_motion_rows: Option<u32>,
	/// Replay-side preview-local registration mean diff before source arbitration.
	pub replayed_preview_only_local_registration_mean_abs_diff_x100: Option<u32>,
	/// Candidate count that reached viewport selection during replay for this frame, when observed.
	pub replayed_downward_viewport_candidate_count: Option<usize>,
	/// Candidate set before committed/local pruning during replay for this frame, when observed.
	pub replayed_downward_viewport_candidates_before_prune: Option<String>,
	/// Candidate set after committed/local pruning during replay for this frame, when observed.
	pub replayed_downward_viewport_candidates_after_prune: Option<String>,
	/// Last local continuity hint visible while replaying this frame.
	pub replayed_sample_eval_last_motion_rows_hint: Option<u32>,
	/// Last transient motion hint visible while replaying this frame.
	pub replayed_sample_eval_transient_motion_rows_hint: Option<u32>,
	/// Effective motion hint visible while replaying this frame.
	pub replayed_sample_eval_effective_motion_rows_hint: Option<u32>,
	/// Whether burst search remained enabled while replaying this frame.
	pub replayed_sample_eval_transient_burst_search_enabled: bool,
	/// Preview-only local viewport ledger still retained after this frame, when any.
	pub replayed_preview_only_local_viewport_top_y: Option<i32>,
	/// Replay-side pending downward input rows visible after this frame.
	pub replayed_downward_motion_rows_pending: f64,
	/// Replay-side gesture-active flag visible after this frame.
	pub replayed_input_gesture_active: bool,
	/// Session-side preview display mode selected after this frame.
	pub replayed_session_preview_display_mode: &'static str,
	/// Session-side hinted preview motion hint, when the hinted path is available.
	pub replayed_session_preview_hinted_motion_rows_hint: Option<u32>,
	/// Session-side hinted preview frame source, when the hinted path is available.
	pub replayed_session_preview_hinted_frame_source: Option<&'static str>,
	/// Overlay-side pending-derived motion hint visible after this frame.
	pub replayed_overlay_preview_motion_rows_hint: Option<u32>,
	/// Overlay-side provisional motion hint visible after this frame.
	pub replayed_overlay_preview_provisional_motion_rows_hint: Option<u32>,
	/// Existing overlay-preview image candidate considered during refresh, when any.
	pub replayed_overlay_preview_existing_candidate_height: Option<u32>,
	/// Existing overlay-preview image candidate motion hint considered during refresh, when any.
	pub replayed_overlay_preview_existing_candidate_motion_rows_hint: Option<u32>,
	/// Retained overlay-preview ledger candidate considered during refresh, when any.
	pub replayed_overlay_preview_ledger_candidate_height: Option<u32>,
	/// Retained overlay-preview ledger motion hint considered during refresh, when any.
	pub replayed_overlay_preview_ledger_candidate_motion_rows_hint: Option<u32>,
	/// Overlay-side retained candidate height considered during refresh, when any.
	pub replayed_overlay_preview_retained_candidate_height: Option<u32>,
	/// Overlay-side retained candidate motion hint considered during refresh, when any.
	pub replayed_overlay_preview_retained_candidate_motion_rows_hint: Option<u32>,
	/// Whether the retained ledger hint matched the current pending motion band.
	pub replayed_overlay_preview_retained_hint_matches_motion_rows: bool,
	/// Whether the overlay refresh considered the fresh-latest-frame authority path.
	pub replayed_overlay_preview_fresh_latest_frame_can_drive: bool,
	/// Retained overlay-preview ledger height visible after this frame, when any.
	pub replayed_retained_overlay_preview_height: Option<u32>,
	/// Retained overlay-preview ledger motion hint visible after this frame, when any.
	pub replayed_retained_overlay_preview_motion_rows_hint: Option<u32>,
	/// Whether overlay refresh saw a strong unresolved registration.
	pub replayed_overlay_preview_strong_unresolved_registration: bool,
	/// Whether overlay refresh had a latest frame to work from.
	pub replayed_overlay_preview_latest_frame_present: bool,
	/// Whether overlay refresh selected any non-session overlay preview.
	pub replayed_overlay_preview_used_provisional: bool,
	/// Estimated downward shift visible between the previous recorded frame and this recorded frame.
	pub recorded_estimated_downward_shift_rows: Option<u32>,
	/// Semantic issue detected directly from the recorded frame progression, when any.
	pub semantic_issue: Option<RecordedScrollCaptureSemanticIssue>,
}

fn replay_direction_name(direction: ScrollCaptureTraceDirection) -> &'static str {
	match direction {
		ScrollCaptureTraceDirection::Up => "up",
		ScrollCaptureTraceDirection::Down => "down",
	}
}
