mod model;
mod semantic;
mod stats;
mod summary;

pub use model::{
	RecordedScrollCaptureReplayFrameSource, RecordedScrollCaptureReplayMode,
	RecordedScrollCaptureReplayRecordedOutcome, RecordedScrollCaptureReplayStepResult,
	RecordedScrollCaptureReplaySummary, RecordedScrollCaptureSemanticIssue,
	ScrollCaptureReplayOutcome,
};

use std::{
	path::Path,
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Result, WrapErr};
use image::{self, RgbaImage};

use crate::overlay::trace_recording::ScrollCaptureTraceFrameEntry;
use crate::overlay::trace_recording::ScrollCaptureTraceInputEntry;
use crate::overlay::{
	GlobalPoint, MonitorRect, OverlaySession, RectPoints, ScrollCaptureFrameSource,
	ScrollObserveOutcome, ScrollSession,
	trace_recording::{LoadedScrollCaptureLiveTrace, ScrollCaptureLiveTraceEntry},
};
use stats::{ReplayStats, ReplayStepResultRecord};

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

	summary::finalize_replay_summary(trace, &session, replay_stats, replay_mode)
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

	replay_stats.update_recorded_height_jumps(recorded_export_height, recorded_preview_height);

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
	let live_frame_gap = replay_stats.update_live_frame_gap(frame_source.clone());
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

	replay_stats.update_replayed_height_jumps(replayed_export_height, replayed_preview_height);
	replay_stats.push_step_result(ReplayStepResultRecord {
		session,
		active_session,
		telemetry: &telemetry,
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
	});

	replay_stats.previous_recorded_frame = Some(image);

	Ok(())
}

fn estimate_recorded_downward_shift_rows(previous: &RgbaImage, current: &RgbaImage) -> Option<u32> {
	semantic::estimate_recorded_downward_shift_rows(previous, current)
}

fn classify_recorded_semantic_issue(
	recorded_outcome: &RecordedScrollCaptureReplayRecordedOutcome,
	recorded_estimated_downward_shift_rows: Option<u32>,
) -> Option<RecordedScrollCaptureSemanticIssue> {
	semantic::classify_recorded_semantic_issue(
		recorded_outcome,
		recorded_estimated_downward_shift_rows,
	)
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
		sync::atomic::{AtomicU64, Ordering},
		time::{Duration, Instant},
	};

	use image::{Rgba, RgbaImage};

	use crate::overlay::replay_support::summary;
	use crate::overlay::replay_support::{self, RecordedScrollCaptureReplayMode};
	use crate::overlay::{
		GlobalPoint, MonitorRect, OverlaySession, RectPoints, ScrollCaptureFrameSource,
		trace_recording::{
			ScrollCaptureTraceFrameRecord, ScrollCaptureTraceInputRecord,
			ScrollCaptureTraceRecorder, ScrollCaptureTraceSessionSnapshot,
		},
	};
	use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome, ScrollSession};

	static TRACE_TEST_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

	fn temp_trace_root() -> PathBuf {
		let counter = TRACE_TEST_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
		let root = env::temp_dir().join(format!(
			"rsnap-recorded-trace-replay-test-{}-{}-{}",
			std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(),
			process::id(),
			counter
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

	#[cfg(target_os = "macos")]
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

	#[cfg(target_os = "macos")]
	#[test]
	fn replay_recorded_live_trace_round_trips_one_commit_in_recorded_source_mode() {
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
	fn replay_recorded_live_trace_round_trips_one_commit_in_worker_pairwise_mode() {
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

		assert!(summary::recorded_step_outcome_matches_replayed(&step));
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

		assert!(!summary::recorded_step_outcome_matches_replayed(&step));
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
