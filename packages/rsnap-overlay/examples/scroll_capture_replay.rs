//! Deterministic scroll-capture replay runner used by cargo-make verification tasks.

#![allow(unused_crate_dependencies)]

use std::path::PathBuf;
use std::process::ExitCode;

use color_eyre::eyre::WrapErr;
use directories::ProjectDirs;
use rsnap_overlay::replay_support::{
	RecordedScrollCaptureReplayMode, replay_recorded_scroll_capture_trace_with_mode,
};
use tracing_subscriber::{EnvFilter, fmt};

fn main() -> ExitCode {
	if let Err(err) = run() {
		eprintln!("[replay] {err}");

		return ExitCode::FAILURE;
	}

	ExitCode::SUCCESS
}

fn run() -> color_eyre::Result<()> {
	color_eyre::install()?;
	let _ = fmt()
		.with_env_filter(
			EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| EnvFilter::new("warn,rsnap_overlay=info")),
		)
		.with_target(false)
		.with_level(true)
		.try_init();

	let mut args = std::env::args().skip(1);
	let mut trace_manifest_path = None;
	let mut list_only = false;
	let mut emit_json = false;
	let mut summary_only = false;
	let mut force_worker_pairwise = false;

	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--list" | "--self-check" => {
				list_only = true;
			},
			"--json" => {
				emit_json = true;
			},
			"--summary-only" => {
				summary_only = true;
			},
			"--force-worker-pairwise" => {
				force_worker_pairwise = true;
			},
			"--trace" => {
				let Some(value) = args.next() else {
					color_eyre::eyre::bail!("--trace requires a manifest path");
				};
				trace_manifest_path = Some(value);
			},
			other => {
				color_eyre::eyre::bail!(
					"unknown argument {other}; supported flags are --list, --self-check, --json, --summary-only, --force-worker-pairwise, and --trace <manifest-path>"
				);
			},
		}
	}

	if list_only {
		println!("latest-recorded-live-trace");

		return Ok(());
	}

	let trace_manifest_path = trace_manifest_path.map(Ok).unwrap_or_else(|| {
		latest_recorded_trace_manifest().map(|path| path.display().to_string())
	})?;
	let replay_mode = if force_worker_pairwise {
		RecordedScrollCaptureReplayMode::ForceWorkerPairwise
	} else {
		RecordedScrollCaptureReplayMode::RecordedSource
	};
	let mut summary =
		replay_recorded_scroll_capture_trace_with_mode(&trace_manifest_path, replay_mode)?;
	if summary_only {
		summary.step_results.clear();
	}
	if emit_json {
		println!("{}", serde_json::to_string_pretty(&summary)?);
	} else {
		print_recorded_trace_summary(&summary);
	}

	Ok(())
}

fn print_recorded_trace_summary(
	summary: &rsnap_overlay::replay_support::RecordedScrollCaptureReplaySummary,
) {
	println!(
		"[replay] mode={:?} trace={} manifest={} final_export_height={} final_preview_height={} final_viewport_top_y={} recorded_final_export_height={:?} recorded_final_preview_height={:?} first_outcome_divergence_frame={:?} first_export_height_drift_frame={:?} first_preview_height_drift_frame={:?} first_semantic_issue_frame={:?} first_missed_downward_motion_frame={:?} first_underconsumed_downward_motion_frame={:?} first_growth_overshoot_frame={:?} max_recorded_committed_growth_rows={} max_replayed_committed_growth_rows={} max_recorded_export_jump={} max_recorded_preview_jump={} max_replayed_export_jump={} max_replayed_preview_jump={} final_preview_path={:?} final_export_path={:?}",
		summary.replay_mode,
		summary.trace_id,
		summary.manifest_path.display(),
		summary.final_export_height,
		summary.final_preview_height,
		summary.final_viewport_top_y,
		summary.recorded_final_export_height,
		summary.recorded_final_preview_height,
		summary.first_outcome_divergence_frame,
		summary.first_export_height_drift_frame,
		summary.first_preview_height_drift_frame,
		summary.first_semantic_issue_frame,
		summary.first_missed_downward_motion_frame,
		summary.first_underconsumed_downward_motion_frame,
		summary.first_growth_overshoot_frame,
		summary.max_recorded_committed_growth_rows,
		summary.max_replayed_committed_growth_rows,
		summary.max_recorded_export_jump,
		summary.max_recorded_preview_jump,
		summary.max_replayed_export_jump,
		summary.max_replayed_preview_jump,
		summary.final_preview_path,
		summary.final_export_path
	);

	for step in &summary.step_results {
		println!(
			"[replay]   frame={} path={} observed_at_ms={} source={:?} live_frame_gap={:?} recorded={:?} replayed={:?} estimated_shift={:?} semantic_issue={:?} export_height={} preview_height={} recorded_export_height={:?} recorded_preview_height={:?} viewport_top_y={} last_commit_source={:?} last_commit_motion_rows={:?} last_block_reason={:?} replayed_registration_result={:?} replayed_registration_source={:?} replayed_registration_motion_rows={:?} replayed_candidates_before={:?} replayed_candidates_after={:?} replayed_last_hint={:?} replayed_transient_hint={:?} replayed_effective_hint={:?} replayed_burst={} replayed_preview_local_top={:?}",
			step.frame_index,
			step.frame_path,
			step.observed_at_ms,
			step.frame_source,
			step.live_frame_gap,
			step.recorded_outcome,
			step.replayed_outcome,
			step.recorded_estimated_downward_shift_rows,
			step.semantic_issue,
			step.export_height,
			step.preview_height,
			step.recorded_export_height,
			step.recorded_preview_height,
			step.viewport_top_y,
			step.last_commit_decision_source,
			step.last_commit_detected_motion_rows,
			step.last_block_reason,
			step.replayed_downward_sample_registration_result,
			step.replayed_downward_sample_registration_source,
			step.replayed_downward_sample_registration_motion_rows,
			step.replayed_downward_viewport_candidates_before_prune,
			step.replayed_downward_viewport_candidates_after_prune,
			step.replayed_sample_eval_last_motion_rows_hint,
			step.replayed_sample_eval_transient_motion_rows_hint,
			step.replayed_sample_eval_effective_motion_rows_hint,
			step.replayed_sample_eval_transient_burst_search_enabled,
			step.replayed_preview_only_local_viewport_top_y
		);
	}
}

fn latest_recorded_trace_manifest() -> color_eyre::Result<PathBuf> {
	let project_dirs = ProjectDirs::from("ink", "hack", "rsnap")
		.expect("rsnap project directories should be available");
	let trace_root = project_dirs.data_local_dir().join("scroll-capture-traces");
	let mut manifests: Vec<PathBuf> = std::fs::read_dir(&trace_root)
		.wrap_err_with(|| format!("failed to read {}", trace_root.display()))?
		.filter_map(Result::ok)
		.map(|entry| entry.path().join("manifest.json"))
		.filter(|path| path.exists())
		.collect();

	manifests.sort();
	manifests.pop().ok_or_else(|| {
		color_eyre::eyre::eyre!(
			"no recorded scroll-capture trace manifests found under {}; record a fresh live trace first or pass --trace <manifest-path>",
			trace_root.display()
		)
	})
}
