use std::path::PathBuf;
use std::{
	path::Path,
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Result, WrapErr};
use image::{self, RgbaImage};
use serde::Serialize;

use crate::overlay::trace_recording::ScrollCaptureTraceDirection;
use crate::overlay::trace_recording::ScrollCaptureTraceFrameEntry;
use crate::overlay::trace_recording::ScrollCaptureTraceInputEntry;
use crate::overlay::{
	GlobalPoint, MonitorRect, OverlaySession, RectPoints, ScrollCaptureFrameSource,
	ScrollDirection, ScrollObserveOutcome, ScrollSession,
	trace_recording::{
		LoadedScrollCaptureLiveTrace, ScrollCaptureLiveTraceEntry,
		ScrollCaptureTraceRecordedOutcome,
	},
};
use crate::scroll_capture::ScrollCommitTelemetry;

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
impl From<super::trace_recording::ScrollCaptureTraceFrameSource>
	for RecordedScrollCaptureReplayFrameSource
{
	fn from(value: super::trace_recording::ScrollCaptureTraceFrameSource) -> Self {
		match value {
			super::trace_recording::ScrollCaptureTraceFrameSource::Worker { request_id } => {
				Self::Worker { request_id }
			},
			super::trace_recording::ScrollCaptureTraceFrameSource::LiveStream { frame_seq } => {
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
				Self::Unsupported {
					direction: match direction {
						ScrollCaptureTraceDirection::Up => "up",
						ScrollCaptureTraceDirection::Down => "down",
					},
				}
			},
			ScrollCaptureTraceRecordedOutcome::Committed { direction, growth_rows } => {
				Self::Committed {
					direction: match direction {
						ScrollCaptureTraceDirection::Up => "up",
						ScrollCaptureTraceDirection::Down => "down",
					},
					growth_rows,
				}
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

#[derive(Default)]
struct ReplayStats {
	step_results: Vec<RecordedScrollCaptureReplayStepResult>,
	previous_recorded_export_height: Option<u32>,
	previous_recorded_preview_height: Option<u32>,
	previous_replayed_export_height: Option<u32>,
	previous_replayed_preview_height: Option<u32>,
	previous_live_frame_seq: Option<u64>,
	previous_recorded_frame: Option<RgbaImage>,
	max_recorded_export_jump: u32,
	max_recorded_preview_jump: u32,
	max_replayed_export_jump: u32,
	max_replayed_preview_jump: u32,
	max_recorded_committed_growth_rows: u32,
	max_replayed_committed_growth_rows: u32,
}

/// Replays one recorded live trace through shipping overlay logic.
pub fn replay_recorded_scroll_capture_trace(
	manifest_path: impl AsRef<Path>,
) -> Result<RecordedScrollCaptureReplaySummary> {
	replay_recorded_scroll_capture_trace_with_mode(
		manifest_path,
		RecordedScrollCaptureReplayMode::RecordedSource,
	)
}

/// Replays one recorded live trace through shipping overlay logic with an explicit frame-source mode.
pub fn replay_recorded_scroll_capture_trace_with_mode(
	manifest_path: impl AsRef<Path>,
	replay_mode: RecordedScrollCaptureReplayMode,
) -> Result<RecordedScrollCaptureReplaySummary> {
	let trace = LoadedScrollCaptureLiveTrace::load(manifest_path)?;
	let (mut session, started_at) = initialize_replay_session(&trace)?;
	let replay_stats = replay_trace_entries(&trace, &mut session, started_at, replay_mode)?;

	finalize_replay_summary(trace, &session, replay_stats, replay_mode)
}

fn classify_replayed_outcome(
	outcome: ScrollObserveOutcome,
	previous_replayed_export_height: Option<u32>,
	replayed_export_height: u32,
	previous_replayed_preview_height: Option<u32>,
	replayed_preview_height: u32,
) -> ScrollCaptureReplayOutcome {
	let replayed_outcome: ScrollCaptureReplayOutcome = outcome.into();

	if replayed_outcome == ScrollCaptureReplayOutcome::NoChange
		&& previous_replayed_export_height == Some(replayed_export_height)
		&& previous_replayed_preview_height
			.is_some_and(|previous| replayed_preview_height > previous)
	{
		ScrollCaptureReplayOutcome::PreviewUpdated
	} else {
		replayed_outcome
	}
}

fn initialize_replay_session(
	trace: &LoadedScrollCaptureLiveTrace,
) -> Result<(OverlaySession, Instant)> {
	let started_at = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(replay_monitor_from_trace(trace));
	session.scroll_capture.capture_rect_pixels = Some(replay_capture_rect_from_trace(trace));
	session.scroll_capture.session =
		Some(ScrollSession::new(trace.base_frame.clone(), trace.manifest.preview_width_px)?);

	session.refresh_scroll_preview_committed_image();

	session.scroll_capture.preview_latest_frame = Some(trace.base_frame.clone());

	session.refresh_scroll_preview_display_image();

	Ok((session, started_at))
}

fn replay_trace_entries(
	trace: &LoadedScrollCaptureLiveTrace,
	session: &mut OverlaySession,
	started_at: Instant,
	replay_mode: RecordedScrollCaptureReplayMode,
) -> Result<ReplayStats> {
	let mut replay_stats = ReplayStats::default();

	for entry in &trace.manifest.entries {
		match entry {
			ScrollCaptureLiveTraceEntry::Input(input) => {
				apply_replayed_input(session, input, started_at);
			},
			ScrollCaptureLiveTraceEntry::Frame(frame) => {
				replay_frame_entry(
					trace,
					session,
					frame,
					started_at,
					replay_mode,
					&mut replay_stats,
				)?;
			},
		}
	}

	Ok(replay_stats)
}

fn apply_replayed_input(
	session: &mut OverlaySession,
	input: &ScrollCaptureTraceInputEntry,
	started_at: Instant,
) {
	session.apply_external_scroll_input_delta_y(
		input.cursor_global_x,
		input.cursor_global_y,
		input.delta_y,
		input.gesture_active,
		input.gesture_ended,
		started_at + Duration::from_millis(input.applied_at_ms),
	);
	session.refresh_scroll_preview_display_image();
}

#[allow(clippy::too_many_lines)]
fn replay_frame_entry(
	trace: &LoadedScrollCaptureLiveTrace,
	session: &mut OverlaySession,
	frame: &ScrollCaptureTraceFrameEntry,
	started_at: Instant,
	replay_mode: RecordedScrollCaptureReplayMode,
	replay_stats: &mut ReplayStats,
) -> Result<()> {
	let recorded_export_height =
		frame.snapshot_after.export_dimensions.map(|dimensions| dimensions[1]);
	let recorded_preview_height =
		frame.snapshot_after.preview_dimensions.map(|dimensions| dimensions[1]);

	update_recorded_height_jumps(replay_stats, recorded_export_height, recorded_preview_height);

	let image = image::open(trace.resolve_frame_path(&frame.frame_path))
		.wrap_err("failed to open recorded live trace frame")?
		.into_rgba8();
	let recorded_estimated_downward_shift_rows = replay_stats
		.previous_recorded_frame
		.as_ref()
		.and_then(|previous| estimate_recorded_downward_shift_rows(previous, &image));
	let observed_at = started_at + Duration::from_millis(frame.observed_at_ms);
	let outcome = match replay_mode {
		RecordedScrollCaptureReplayMode::RecordedSource => match frame.frame_source {
			crate::overlay::trace_recording::ScrollCaptureTraceFrameSource::LiveStream {
				frame_seq,
			} => session.replay_recorded_live_stream_frame(
				image.clone(),
				frame_seq,
				observed_at,
				frame.allow_stale_input,
			),
			crate::overlay::trace_recording::ScrollCaptureTraceFrameSource::Worker { .. } => {
				session.handle_scroll_capture_frame(
					image.clone(),
					replay_frame_source(frame.frame_source),
					frame.allow_stale_input,
					observed_at,
				)
			},
		},
		RecordedScrollCaptureReplayMode::ForceWorkerPairwise => session
			.handle_scroll_capture_frame(
				image.clone(),
				ScrollCaptureFrameSource::Worker {
					request_id: replay_stats.step_results.len() as u64,
				},
				frame.allow_stale_input,
				observed_at,
			),
	}
	.transpose()?
	.ok_or_else(|| {
		eyre::eyre!(
			"recorded trace frame {} did not observe because the session vanished",
			frame.frame_path
		)
	})?;
	let active_session = session.scroll_capture.session.as_ref().ok_or_else(|| {
		eyre::eyre!(
			"scroll-capture session missing after replaying recorded frame {}",
			frame.frame_path
		)
	})?;
	let telemetry = active_session.commit_telemetry();
	let frame_source: RecordedScrollCaptureReplayFrameSource = frame.frame_source.into();
	let live_frame_gap = update_live_frame_gap(replay_stats, frame_source.clone());
	let recorded_outcome: RecordedScrollCaptureReplayRecordedOutcome = frame.outcome.clone().into();
	let replayed_export_height = active_session.export_image().height();
	let replayed_session_preview_height = active_session.preview_display_image().height();
	let replayed_preview_height = session
		.scroll_capture_preview_dimensions()
		.map_or(replayed_session_preview_height, |dimensions| dimensions[1]);
	let replayed_outcome = classify_replayed_outcome(
		outcome,
		replay_stats.previous_replayed_export_height,
		replayed_export_height,
		replay_stats.previous_replayed_preview_height,
		replayed_preview_height,
	);
	let semantic_issue =
		classify_recorded_semantic_issue(&recorded_outcome, recorded_estimated_downward_shift_rows);

	if let RecordedScrollCaptureReplayRecordedOutcome::Committed { growth_rows, .. } =
		recorded_outcome
	{
		replay_stats.max_recorded_committed_growth_rows =
			replay_stats.max_recorded_committed_growth_rows.max(growth_rows);
	}
	if let ScrollCaptureReplayOutcome::CommittedDown { growth_rows } = replayed_outcome {
		replay_stats.max_replayed_committed_growth_rows =
			replay_stats.max_replayed_committed_growth_rows.max(growth_rows);
	}

	update_replayed_height_jumps(replay_stats, replayed_export_height, replayed_preview_height);
	push_replay_step_result(
		replay_stats,
		session,
		active_session,
		&telemetry,
		frame,
		frame_source,
		live_frame_gap,
		recorded_outcome,
		replayed_outcome,
		recorded_export_height,
		recorded_preview_height,
		replayed_export_height,
		replayed_preview_height,
		replayed_session_preview_height,
		recorded_estimated_downward_shift_rows,
		semantic_issue,
	);

	replay_stats.previous_recorded_frame = Some(image);

	Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_replay_step_result(
	replay_stats: &mut ReplayStats,
	session: &OverlaySession,
	active_session: &ScrollSession,
	telemetry: &ScrollCommitTelemetry,
	frame: &ScrollCaptureTraceFrameEntry,
	frame_source: RecordedScrollCaptureReplayFrameSource,
	live_frame_gap: Option<u64>,
	recorded_outcome: RecordedScrollCaptureReplayRecordedOutcome,
	replayed_outcome: ScrollCaptureReplayOutcome,
	recorded_export_height: Option<u32>,
	recorded_preview_height: Option<u32>,
	replayed_export_height: u32,
	replayed_preview_height: u32,
	replayed_session_preview_height: u32,
	recorded_estimated_downward_shift_rows: Option<u32>,
	semantic_issue: Option<RecordedScrollCaptureSemanticIssue>,
) {
	replay_stats.step_results.push(RecordedScrollCaptureReplayStepResult {
		frame_index: replay_stats.step_results.len(),
		frame_path: frame.frame_path.clone(),
		observed_at_ms: frame.observed_at_ms,
		frame_source,
		live_frame_gap,
		recorded_outcome,
		replayed_outcome,
		export_height: replayed_export_height,
		preview_height: replayed_preview_height,
		session_preview_height: replayed_session_preview_height,
		recorded_export_height,
		recorded_preview_height,
		viewport_top_y: active_session.current_viewport_top_y(),
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
		replayed_observed_sample_registration_result: telemetry.observed_sample_registration_result,
		replayed_observed_sample_registration_reason: telemetry.observed_sample_registration_reason,
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
		replayed_downward_motion_rows_pending: session.scroll_capture.downward_motion_rows_pending,
		replayed_input_gesture_active: session.scroll_capture.input_gesture_active,
		replayed_session_preview_display_mode: active_session.preview_display_mode(),
		replayed_session_preview_hinted_motion_rows_hint: None,
		replayed_session_preview_hinted_frame_source: None,
		replayed_overlay_preview_motion_rows_hint: session
			.scroll_capture
			.last_overlay_preview_motion_rows_hint,
		replayed_overlay_preview_provisional_motion_rows_hint: session
			.scroll_capture
			.last_overlay_preview_provisional_motion_rows_hint,
		replayed_overlay_preview_existing_candidate_height: session
			.scroll_capture
			.last_overlay_preview_existing_candidate_height,
		replayed_overlay_preview_existing_candidate_motion_rows_hint: session
			.scroll_capture
			.last_overlay_preview_existing_candidate_motion_rows_hint,
		replayed_overlay_preview_ledger_candidate_height: session
			.scroll_capture
			.last_overlay_preview_ledger_candidate_height,
		replayed_overlay_preview_ledger_candidate_motion_rows_hint: session
			.scroll_capture
			.last_overlay_preview_ledger_candidate_motion_rows_hint,
		replayed_overlay_preview_retained_candidate_height: session
			.scroll_capture
			.last_overlay_preview_retained_candidate_height,
		replayed_overlay_preview_retained_candidate_motion_rows_hint: session
			.scroll_capture
			.last_overlay_preview_retained_candidate_motion_rows_hint,
		replayed_overlay_preview_retained_hint_matches_motion_rows: session
			.scroll_capture
			.last_overlay_preview_retained_hint_matches_motion_rows,
		replayed_overlay_preview_fresh_latest_frame_can_drive: session
			.scroll_capture
			.last_overlay_preview_fresh_latest_frame_can_drive,
		replayed_retained_overlay_preview_height: session
			.scroll_capture
			.retained_overlay_preview_image
			.as_ref()
			.map(RgbaImage::height),
		replayed_retained_overlay_preview_motion_rows_hint: session
			.scroll_capture
			.retained_overlay_preview_motion_rows_hint,
		replayed_overlay_preview_strong_unresolved_registration: session
			.scroll_capture
			.last_overlay_preview_strong_unresolved_registration,
		replayed_overlay_preview_latest_frame_present: session
			.scroll_capture
			.last_overlay_preview_latest_frame_present,
		replayed_overlay_preview_used_provisional: session
			.scroll_capture
			.last_overlay_preview_used_provisional,
		recorded_estimated_downward_shift_rows,
		semantic_issue,
	});
}

fn update_recorded_height_jumps(
	replay_stats: &mut ReplayStats,
	recorded_export_height: Option<u32>,
	recorded_preview_height: Option<u32>,
) {
	if let Some(recorded_export_height) = recorded_export_height {
		if let Some(previous) = replay_stats.previous_recorded_export_height {
			replay_stats.max_recorded_export_jump = replay_stats
				.max_recorded_export_jump
				.max(recorded_export_height.saturating_sub(previous));
		}

		replay_stats.previous_recorded_export_height = Some(recorded_export_height);
	}
	if let Some(recorded_preview_height) = recorded_preview_height {
		if let Some(previous) = replay_stats.previous_recorded_preview_height {
			replay_stats.max_recorded_preview_jump = replay_stats
				.max_recorded_preview_jump
				.max(recorded_preview_height.saturating_sub(previous));
		}

		replay_stats.previous_recorded_preview_height = Some(recorded_preview_height);
	}
}

fn update_replayed_height_jumps(
	replay_stats: &mut ReplayStats,
	replayed_export_height: u32,
	replayed_preview_height: u32,
) {
	if let Some(previous) = replay_stats.previous_replayed_export_height {
		replay_stats.max_replayed_export_jump = replay_stats
			.max_replayed_export_jump
			.max(replayed_export_height.saturating_sub(previous));
	}

	replay_stats.previous_replayed_export_height = Some(replayed_export_height);

	if let Some(previous) = replay_stats.previous_replayed_preview_height {
		replay_stats.max_replayed_preview_jump = replay_stats
			.max_replayed_preview_jump
			.max(replayed_preview_height.saturating_sub(previous));
	}

	replay_stats.previous_replayed_preview_height = Some(replayed_preview_height);
}

fn update_live_frame_gap(
	replay_stats: &mut ReplayStats,
	frame_source: RecordedScrollCaptureReplayFrameSource,
) -> Option<u64> {
	match frame_source {
		RecordedScrollCaptureReplayFrameSource::LiveStream { frame_seq } => {
			let gap = replay_stats
				.previous_live_frame_seq
				.map(|previous| frame_seq.saturating_sub(previous))
				.unwrap_or(1);

			replay_stats.previous_live_frame_seq = Some(frame_seq);

			Some(gap)
		},
		RecordedScrollCaptureReplayFrameSource::Worker { .. } => None,
	}
}

fn finalize_replay_summary(
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

fn estimate_recorded_downward_shift_rows(previous: &RgbaImage, current: &RgbaImage) -> Option<u32> {
	if previous.dimensions() != current.dimensions() {
		return None;
	}

	let (width, height) = previous.dimensions();

	if width < 2 || height < 3 {
		return None;
	}

	let margin_x = (width / 8).min(width.saturating_sub(2) / 2);
	let start_x = margin_x;
	let end_x = width.saturating_sub(margin_x).max(start_x + 1);
	let x_step = ((end_x.saturating_sub(start_x)) / 48).max(1);
	let y_step = 2_u32;
	let max_shift = height.saturating_sub(1).min(96);
	let mut best_shift = 0_u32;
	let mut best_score = overlap_abs_diff(previous, current, 0, start_x, end_x, x_step, y_step)?;

	for shift in 1..=max_shift {
		let Some(score) =
			overlap_abs_diff(previous, current, shift, start_x, end_x, x_step, y_step)
		else {
			continue;
		};

		if score < best_score {
			best_score = score;
			best_shift = shift;
		}
	}

	Some(best_shift)
}

fn overlap_abs_diff(
	previous: &RgbaImage,
	current: &RgbaImage,
	shift: u32,
	start_x: u32,
	end_x: u32,
	x_step: u32,
	y_step: u32,
) -> Option<u64> {
	let height = previous.height();

	if shift >= height {
		return None;
	}

	let overlap_height = height - shift;

	if overlap_height < 2 {
		return None;
	}

	let mut sum = 0_u64;
	let mut samples = 0_u64;
	let mut y = 0_u32;

	while y < overlap_height {
		let mut x = start_x;

		while x < end_x {
			let prev = previous.get_pixel(x, y + shift);
			let curr = current.get_pixel(x, y);
			let prev_luma = u16::from(prev[0]) + u16::from(prev[1]) + u16::from(prev[2]);
			let curr_luma = u16::from(curr[0]) + u16::from(curr[1]) + u16::from(curr[2]);

			sum += u64::from(prev_luma.abs_diff(curr_luma));
			samples += 1;
			x = x.saturating_add(x_step);
		}

		y = y.saturating_add(y_step);
	}

	if samples == 0 {
		return None;
	}

	Some(sum / samples)
}

fn classify_recorded_semantic_issue(
	recorded_outcome: &RecordedScrollCaptureReplayRecordedOutcome,
	recorded_estimated_downward_shift_rows: Option<u32>,
) -> Option<RecordedScrollCaptureSemanticIssue> {
	let shift = recorded_estimated_downward_shift_rows?;

	if shift < 4 {
		return None;
	}

	match recorded_outcome {
		RecordedScrollCaptureReplayRecordedOutcome::NoChange
		| RecordedScrollCaptureReplayRecordedOutcome::PreviewUpdated => {
			Some(RecordedScrollCaptureSemanticIssue::MissedDownwardMotion)
		},
		RecordedScrollCaptureReplayRecordedOutcome::Committed {
			direction: "down",
			growth_rows,
		} if growth_rows.saturating_mul(2).saturating_add(2) < shift => {
			Some(RecordedScrollCaptureSemanticIssue::UnderconsumedDownwardMotion)
		},
		RecordedScrollCaptureReplayRecordedOutcome::Committed {
			direction: "down",
			growth_rows,
		} if *growth_rows > shift.saturating_add(8) => {
			Some(RecordedScrollCaptureSemanticIssue::GrowthExceedsRecordedShift)
		},
		_ => None,
	}
}

#[cfg(target_os = "macos")]
fn replay_frame_source(
	frame_source: crate::overlay::trace_recording::ScrollCaptureTraceFrameSource,
) -> ScrollCaptureFrameSource {
	match frame_source {
		crate::overlay::trace_recording::ScrollCaptureTraceFrameSource::Worker { .. } => {
			unreachable!("macOS live traces should not contain worker-backed scroll frames")
		},
		crate::overlay::trace_recording::ScrollCaptureTraceFrameSource::LiveStream {
			frame_seq,
		} => ScrollCaptureFrameSource::LiveStream { frame_seq },
	}
}

#[cfg(not(target_os = "macos"))]
fn replay_frame_source(
	frame_source: crate::overlay::trace_recording::ScrollCaptureTraceFrameSource,
) -> ScrollCaptureFrameSource {
	match frame_source {
		crate::overlay::trace_recording::ScrollCaptureTraceFrameSource::Worker { request_id } => {
			ScrollCaptureFrameSource::Worker { request_id }
		},
		crate::overlay::trace_recording::ScrollCaptureTraceFrameSource::LiveStream {
			frame_seq,
		} => {
			let _ = frame_seq;

			unreachable!("non-macOS replay should not receive live-stream scroll frames")
		},
	}
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

fn recorded_step_outcome_matches_replayed(step: &RecordedScrollCaptureReplayStepResult) -> bool {
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

fn replay_monitor_from_trace(trace: &LoadedScrollCaptureLiveTrace) -> MonitorRect {
	MonitorRect {
		id: trace.manifest.monitor.id,
		origin: GlobalPoint::new(trace.manifest.monitor.origin_x, trace.manifest.monitor.origin_y),
		width: trace.manifest.monitor.width,
		height: trace.manifest.monitor.height,
		scale_factor_x1000: trace.manifest.monitor.scale_factor_x1000,
	}
}

fn replay_capture_rect_from_trace(trace: &LoadedScrollCaptureLiveTrace) -> RectPoints {
	RectPoints::new(
		trace.manifest.capture_rect_pixels.x,
		trace.manifest.capture_rect_pixels.y,
		trace.manifest.capture_rect_pixels.width,
		trace.manifest.capture_rect_pixels.height,
	)
}

#[cfg(test)]
mod tests {
	use std::env;
	use std::{
		fs,
		path::PathBuf,
		process,
		time::{Duration, Instant},
	};

	use image::{Rgba, RgbaImage};

	use crate::overlay::replay_support::{self, RecordedScrollCaptureReplayMode};
	use crate::overlay::{
		GlobalPoint, MonitorRect, OverlaySession, RectPoints, ScrollCaptureFrameSource,
		trace_recording::{
			ScrollCaptureTraceFrameRecord, ScrollCaptureTraceInputRecord,
			ScrollCaptureTraceRecorder, ScrollCaptureTraceSessionSnapshot,
		},
	};
	use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome, ScrollSession};

	fn temp_trace_root() -> PathBuf {
		let root = env::temp_dir().join(format!(
			"rsnap-recorded-trace-replay-test-{}-{}",
			std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(),
			process::id()
		));
		let _ = fs::remove_dir_all(&root);

		root
	}

	fn monitor() -> MonitorRect {
		MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		}
	}

	fn capture_rect() -> RectPoints {
		RectPoints::new(100, 120, 3, 5)
	}

	fn large_capture_rect() -> RectPoints {
		RectPoints::new(100, 120, 256, 120)
	}

	fn make_window(rows: &[[u8; 4]], start: usize) -> RgbaImage {
		let mut image = RgbaImage::new(3, 5);

		for (y, row) in rows[start..start + 5].iter().enumerate() {
			for x in 0..3 {
				image.put_pixel(x, y as u32, Rgba(*row));
			}
		}

		image
	}

	fn make_sparse_textlike_window(width: u32, height: u32, start_row: u32) -> RgbaImage {
		let stripe_x = 104_u32.min(width.saturating_sub(1));
		let mut image = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));

		for y in 0..height {
			let document_row = start_row.saturating_add(y);
			let shade = ((document_row.saturating_mul(17)) % 180) as u8;

			for x in stripe_x..stripe_x.saturating_add(6).min(width) {
				image.put_pixel(x, y, Rgba([shade, shade, shade, 255]));
			}
			for x in stripe_x.saturating_add(10)..stripe_x.saturating_add(13).min(width) {
				if document_row % 19 < 9 {
					image.put_pixel(x, y, Rgba([40, 40, 40, 255]));
				}
			}
		}

		image
	}

	#[test]
	fn replay_recorded_live_trace_round_trips_one_commit() {
		let rows = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
		];
		let base_frame = make_window(&rows, 0);
		let next_frame = make_window(&rows, 1);
		let mut session = OverlaySession::new();
		let root = temp_trace_root();
		let mut recorder = ScrollCaptureTraceRecorder::new_for_root_dir(
			root,
			monitor(),
			capture_rect(),
			320,
			&base_frame,
		)
		.unwrap();
		let manifest_path = recorder.manifest_path().to_path_buf();
		let started_at = Instant::now();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor());
		session.scroll_capture.capture_rect_pixels = Some(capture_rect());
		session.scroll_capture.session = Some(ScrollSession::new(base_frame.clone(), 320).unwrap());

		session.apply_external_scroll_input_delta_y(
			150.0,
			160.0,
			4.0,
			true,
			false,
			started_at + Duration::from_millis(10),
		);
		recorder.record_replayed_input(ScrollCaptureTraceInputRecord {
			seq: 1,
			cursor_global: (150.0, 160.0),
			delta_y: 4.0,
			gesture_active: true,
			gesture_ended: false,
			recorded_age: Duration::from_millis(2),
			applied_at: started_at + Duration::from_millis(10),
			snapshot_after: ScrollCaptureTraceSessionSnapshot::capture(
				session.scroll_capture.session.as_ref(),
				session
					.scroll_capture
					.session
					.as_ref()
					.map(ScrollSession::preview_display_image)
					.map(|image| [image.width(), image.height()]),
				Some(ScrollDirection::Down),
				true,
				4.0,
				Some(2),
			),
		});

		let outcome = session
			.observe_scroll_capture_frame_at(
				next_frame.clone(),
				started_at + Duration::from_millis(20),
			)
			.transpose()
			.unwrap()
			.unwrap();

		recorder.record_frame_observation(ScrollCaptureTraceFrameRecord {
			frame: &next_frame,
			source: ScrollCaptureFrameSource::LiveStream { frame_seq: 9 },
			allow_stale_input: false,
			prior_block_reason: None,
			observed_at: started_at + Duration::from_millis(20),
			snapshot_after: ScrollCaptureTraceSessionSnapshot::capture(
				session.scroll_capture.session.as_ref(),
				session
					.scroll_capture
					.session
					.as_ref()
					.map(ScrollSession::preview_display_image)
					.map(|image| [image.width(), image.height()]),
				session.scroll_capture.input_direction,
				session.scroll_capture.input_gesture_active,
				session.scroll_capture.downward_motion_rows_pending,
				Some(0),
			),
			outcome: &Ok(outcome),
		});

		drop(recorder);

		let summary = replay_support::replay_recorded_scroll_capture_trace(&manifest_path).unwrap();

		assert_eq!(summary.step_results.len(), 1);
		assert_eq!(
			summary.step_results[0].recorded_outcome,
			super::RecordedScrollCaptureReplayRecordedOutcome::Committed {
				direction: "down",
				growth_rows: 1,
			}
		);
		assert_eq!(
			summary.step_results[0].replayed_outcome,
			super::ScrollCaptureReplayOutcome::CommittedDown { growth_rows: 1 }
		);
		assert_eq!(summary.final_export_height, 6);
		assert_eq!(summary.max_replayed_export_jump, 0);
		assert_eq!(summary.max_replayed_preview_jump, 0);
	}

	#[test]
	fn replay_recorded_live_trace_round_trips_one_commit_forces_worker_pairwise() {
		let base_frame = make_sparse_textlike_window(256, 120, 0);
		let next_frame = make_sparse_textlike_window(256, 120, 9);
		let mut session = OverlaySession::new();
		let root = temp_trace_root();
		let mut recorder = ScrollCaptureTraceRecorder::new_for_root_dir(
			root,
			monitor(),
			large_capture_rect(),
			320,
			&base_frame,
		)
		.unwrap();
		let manifest_path = recorder.manifest_path().to_path_buf();
		let started_at = Instant::now();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(monitor());
		session.scroll_capture.capture_rect_pixels = Some(large_capture_rect());
		session.scroll_capture.session = Some(ScrollSession::new(base_frame.clone(), 320).unwrap());

		session.apply_external_scroll_input_delta_y(
			150.0,
			160.0,
			9.0,
			true,
			false,
			started_at + Duration::from_millis(10),
		);
		recorder.record_replayed_input(ScrollCaptureTraceInputRecord {
			seq: 1,
			cursor_global: (150.0, 160.0),
			delta_y: 4.0,
			gesture_active: true,
			gesture_ended: false,
			recorded_age: Duration::from_millis(2),
			applied_at: started_at + Duration::from_millis(10),
			snapshot_after: ScrollCaptureTraceSessionSnapshot::capture(
				session.scroll_capture.session.as_ref(),
				session
					.scroll_capture
					.session
					.as_ref()
					.map(ScrollSession::preview_display_image)
					.map(|image| [image.width(), image.height()]),
				Some(ScrollDirection::Down),
				true,
				9.0,
				Some(2),
			),
		});

		let outcome = session
			.observe_scroll_capture_frame_at(
				next_frame.clone(),
				started_at + Duration::from_millis(20),
			)
			.transpose()
			.unwrap()
			.unwrap();

		recorder.record_frame_observation(ScrollCaptureTraceFrameRecord {
			frame: &next_frame,
			source: ScrollCaptureFrameSource::LiveStream { frame_seq: 9 },
			allow_stale_input: false,
			prior_block_reason: None,
			observed_at: started_at + Duration::from_millis(20),
			snapshot_after: ScrollCaptureTraceSessionSnapshot::capture(
				session.scroll_capture.session.as_ref(),
				session
					.scroll_capture
					.session
					.as_ref()
					.map(ScrollSession::preview_display_image)
					.map(|image| [image.width(), image.height()]),
				session.scroll_capture.input_direction,
				session.scroll_capture.input_gesture_active,
				session.scroll_capture.downward_motion_rows_pending,
				Some(0),
			),
			outcome: &Ok(outcome),
		});

		drop(recorder);

		let ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows } =
			outcome
		else {
			panic!("expected recorded-source setup to commit one downward growth step");
		};
		let summary = replay_support::replay_recorded_scroll_capture_trace_with_mode(
			&manifest_path,
			RecordedScrollCaptureReplayMode::ForceWorkerPairwise,
		)
		.unwrap();

		assert_eq!(summary.replay_mode, RecordedScrollCaptureReplayMode::ForceWorkerPairwise);
		assert_eq!(summary.step_results.len(), 1);
		assert_eq!(
			summary.step_results[0].recorded_outcome,
			super::RecordedScrollCaptureReplayRecordedOutcome::Committed {
				direction: "down",
				growth_rows,
			}
		);
		assert_eq!(
			summary.step_results[0].replayed_outcome,
			super::ScrollCaptureReplayOutcome::CommittedDown { growth_rows }
		);
		assert_eq!(summary.final_export_height, base_frame.height() + growth_rows);
		assert_eq!(summary.max_replayed_export_jump, 0);
		assert_eq!(summary.max_replayed_preview_jump, 0);
	}

	#[test]
	fn estimated_downward_shift_rows_detects_simple_shift() {
		let rows = [
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
		let previous = make_window(&rows, 0);
		let current = make_window(&rows, 2);

		assert_eq!(super::estimate_recorded_downward_shift_rows(&previous, &current), Some(2));
	}

	#[test]
	fn classify_replayed_outcome_upgrades_no_change_when_only_preview_grew() {
		assert_eq!(
			super::classify_replayed_outcome(
				ScrollObserveOutcome::NoChange,
				Some(100),
				100,
				Some(120),
				145,
			),
			super::ScrollCaptureReplayOutcome::PreviewUpdated
		);
	}

	#[test]
	fn classify_replayed_outcome_keeps_no_change_when_export_changed() {
		assert_eq!(
			super::classify_replayed_outcome(
				ScrollObserveOutcome::NoChange,
				Some(100),
				101,
				Some(120),
				145,
			),
			super::ScrollCaptureReplayOutcome::NoChange
		);
	}

	#[test]
	fn recorded_step_outcome_match_ignores_no_change_vs_preview_updated_when_heights_align() {
		let step = super::RecordedScrollCaptureReplayStepResult {
			frame_index: 0,
			frame_path: String::new(),
			observed_at_ms: 0,
			frame_source: super::RecordedScrollCaptureReplayFrameSource::LiveStream {
				frame_seq: 1,
			},
			live_frame_gap: Some(1),
			recorded_outcome: super::RecordedScrollCaptureReplayRecordedOutcome::NoChange,
			replayed_outcome: super::ScrollCaptureReplayOutcome::PreviewUpdated,
			export_height: 100,
			preview_height: 148,
			session_preview_height: 148,
			recorded_export_height: Some(100),
			recorded_preview_height: Some(148),
			viewport_top_y: 0,
			last_commit_decision_source: None,
			last_commit_detected_motion_rows: None,
			last_block_reason: None,
			replayed_downward_sample_registration_result: None,
			replayed_downward_sample_registration_source: None,
			replayed_downward_sample_registration_motion_rows: None,
			replayed_downward_sample_registration_provisional_viewport_top_y: None,
			replayed_observed_sample_registration_result: None,
			replayed_observed_sample_registration_reason: None,
			replayed_observed_sample_registration_motion_rows: None,
			replayed_observed_sample_registration_mean_abs_diff_x100: None,
			replayed_preview_only_local_registration_result: None,
			replayed_preview_only_local_registration_reason: None,
			replayed_preview_only_local_registration_motion_rows: None,
			replayed_preview_only_local_registration_mean_abs_diff_x100: None,
			replayed_downward_viewport_candidate_count: None,
			replayed_downward_viewport_candidates_before_prune: None,
			replayed_downward_viewport_candidates_after_prune: None,
			replayed_sample_eval_last_motion_rows_hint: None,
			replayed_sample_eval_transient_motion_rows_hint: None,
			replayed_sample_eval_effective_motion_rows_hint: None,
			replayed_sample_eval_transient_burst_search_enabled: false,
			replayed_preview_only_local_viewport_top_y: None,
			replayed_downward_motion_rows_pending: 0.0,
			replayed_input_gesture_active: false,
			replayed_session_preview_display_mode: "committed",
			replayed_session_preview_hinted_motion_rows_hint: None,
			replayed_session_preview_hinted_frame_source: None,
			replayed_overlay_preview_motion_rows_hint: None,
			replayed_overlay_preview_provisional_motion_rows_hint: None,
			replayed_overlay_preview_existing_candidate_height: None,
			replayed_overlay_preview_existing_candidate_motion_rows_hint: None,
			replayed_overlay_preview_ledger_candidate_height: None,
			replayed_overlay_preview_ledger_candidate_motion_rows_hint: None,
			replayed_overlay_preview_retained_candidate_height: None,
			replayed_overlay_preview_retained_candidate_motion_rows_hint: None,
			replayed_overlay_preview_retained_hint_matches_motion_rows: false,
			replayed_overlay_preview_fresh_latest_frame_can_drive: false,
			replayed_retained_overlay_preview_height: None,
			replayed_retained_overlay_preview_motion_rows_hint: None,
			replayed_overlay_preview_strong_unresolved_registration: false,
			replayed_overlay_preview_latest_frame_present: false,
			replayed_overlay_preview_used_provisional: false,
			recorded_estimated_downward_shift_rows: None,
			semantic_issue: None,
		};

		assert!(super::recorded_step_outcome_matches_replayed(&step));
	}

	#[test]
	fn recorded_step_outcome_match_keeps_divergence_when_only_outcome_label_matches_bad_heights() {
		let step = super::RecordedScrollCaptureReplayStepResult {
			frame_index: 0,
			frame_path: String::new(),
			observed_at_ms: 0,
			frame_source: super::RecordedScrollCaptureReplayFrameSource::LiveStream {
				frame_seq: 1,
			},
			live_frame_gap: Some(1),
			recorded_outcome: super::RecordedScrollCaptureReplayRecordedOutcome::NoChange,
			replayed_outcome: super::ScrollCaptureReplayOutcome::PreviewUpdated,
			export_height: 100,
			preview_height: 148,
			session_preview_height: 148,
			recorded_export_height: Some(100),
			recorded_preview_height: Some(147),
			viewport_top_y: 0,
			last_commit_decision_source: None,
			last_commit_detected_motion_rows: None,
			last_block_reason: None,
			replayed_downward_sample_registration_result: None,
			replayed_downward_sample_registration_source: None,
			replayed_downward_sample_registration_motion_rows: None,
			replayed_downward_sample_registration_provisional_viewport_top_y: None,
			replayed_observed_sample_registration_result: None,
			replayed_observed_sample_registration_reason: None,
			replayed_observed_sample_registration_motion_rows: None,
			replayed_observed_sample_registration_mean_abs_diff_x100: None,
			replayed_preview_only_local_registration_result: None,
			replayed_preview_only_local_registration_reason: None,
			replayed_preview_only_local_registration_motion_rows: None,
			replayed_preview_only_local_registration_mean_abs_diff_x100: None,
			replayed_downward_viewport_candidate_count: None,
			replayed_downward_viewport_candidates_before_prune: None,
			replayed_downward_viewport_candidates_after_prune: None,
			replayed_sample_eval_last_motion_rows_hint: None,
			replayed_sample_eval_transient_motion_rows_hint: None,
			replayed_sample_eval_effective_motion_rows_hint: None,
			replayed_sample_eval_transient_burst_search_enabled: false,
			replayed_preview_only_local_viewport_top_y: None,
			replayed_downward_motion_rows_pending: 0.0,
			replayed_input_gesture_active: false,
			replayed_session_preview_display_mode: "committed",
			replayed_session_preview_hinted_motion_rows_hint: None,
			replayed_session_preview_hinted_frame_source: None,
			replayed_overlay_preview_motion_rows_hint: None,
			replayed_overlay_preview_provisional_motion_rows_hint: None,
			replayed_overlay_preview_existing_candidate_height: None,
			replayed_overlay_preview_existing_candidate_motion_rows_hint: None,
			replayed_overlay_preview_ledger_candidate_height: None,
			replayed_overlay_preview_ledger_candidate_motion_rows_hint: None,
			replayed_overlay_preview_retained_candidate_height: None,
			replayed_overlay_preview_retained_candidate_motion_rows_hint: None,
			replayed_overlay_preview_retained_hint_matches_motion_rows: false,
			replayed_overlay_preview_fresh_latest_frame_can_drive: false,
			replayed_retained_overlay_preview_height: None,
			replayed_retained_overlay_preview_motion_rows_hint: None,
			replayed_overlay_preview_strong_unresolved_registration: false,
			replayed_overlay_preview_latest_frame_present: false,
			replayed_overlay_preview_used_provisional: false,
			recorded_estimated_downward_shift_rows: None,
			semantic_issue: None,
		};

		assert!(!super::recorded_step_outcome_matches_replayed(&step));
	}

	#[test]
	fn semantic_issue_flags_missed_downward_motion_when_shift_exists_without_growth() {
		assert_eq!(
			super::classify_recorded_semantic_issue(
				&super::RecordedScrollCaptureReplayRecordedOutcome::PreviewUpdated,
				Some(12),
			),
			Some(super::RecordedScrollCaptureSemanticIssue::MissedDownwardMotion)
		);
	}
}
