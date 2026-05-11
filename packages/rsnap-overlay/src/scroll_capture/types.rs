use std::ops::RangeInclusive;

use image::RgbaImage;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverlapMatch {
	pub(crate) rows: u32,
	pub(crate) matched: bool,
	pub(crate) mean_abs_diff_x100: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScrollCommitTelemetry {
	pub(crate) current_viewport_top_y: i32,
	pub(crate) preview_dimensions: (u32, u32),
	pub(crate) export_dimensions: (u32, u32),
	pub(crate) growth_commit_count: usize,
	pub(crate) preview_segment_count: usize,
	pub(crate) export_segment_count: usize,
	pub(crate) preview_export_segments_aligned: bool,
	pub(crate) last_commit_decision_source: Option<&'static str>,
	pub(crate) last_commit_detected_motion_rows: Option<u32>,
	pub(crate) last_commit_effective_motion_rows_hint: Option<u32>,
	pub(crate) last_block_reason: Option<&'static str>,
	pub(crate) last_downward_sample_registration_result: Option<&'static str>,
	pub(crate) last_downward_sample_registration_source: Option<&'static str>,
	pub(crate) last_downward_sample_registration_motion_rows: Option<u32>,
	pub(crate) last_downward_sample_registration_provisional_viewport_top_y: Option<i32>,
	pub(crate) observed_sample_registration_result: Option<&'static str>,
	pub(crate) observed_sample_registration_reason: Option<&'static str>,
	pub(crate) observed_sample_registration_motion_rows: Option<u32>,
	pub(crate) observed_sample_registration_mean_abs_diff_x100: Option<u32>,
	pub(crate) preview_only_local_registration_result: Option<&'static str>,
	pub(crate) preview_only_local_registration_reason: Option<&'static str>,
	pub(crate) preview_only_local_registration_motion_rows: Option<u32>,
	pub(crate) preview_only_local_registration_mean_abs_diff_x100: Option<u32>,
	pub(crate) last_downward_viewport_candidate_count: Option<usize>,
	pub(crate) last_downward_viewport_candidates_before_prune: Option<String>,
	pub(crate) last_downward_viewport_candidates_after_prune: Option<String>,
	pub(crate) sample_eval_last_motion_rows_hint: Option<u32>,
	pub(crate) sample_eval_transient_motion_rows_hint: Option<u32>,
	pub(crate) sample_eval_effective_motion_rows_hint: Option<u32>,
	pub(crate) sample_eval_transient_burst_search_enabled: bool,
	pub(crate) preview_only_local_viewport_top_y: Option<i32>,
	pub(crate) last_preview_segment_height_px: Option<u32>,
	pub(crate) last_export_segment_height_px: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OverlapSearchConfig {
	pub(crate) min_overlap_rows: u32,
	pub(crate) max_column_samples: u32,
	pub(crate) max_row_samples: u32,
	pub(crate) max_mean_abs_diff_x100: u32,
}
impl Default for OverlapSearchConfig {
	fn default() -> Self {
		Self {
			min_overlap_rows: 24,
			max_column_samples: 160,
			max_row_samples: 64,
			max_mean_abs_diff_x100: 850,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreviewOnlyDownwardLocalSample {
	pub(crate) frame: RgbaImage,
	pub(crate) viewport_top_y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DirectionMatch {
	pub(crate) mean_abs_diff_x100: u32,
	pub(crate) motion_rows: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DownwardSampleMatch {
	pub(crate) matched: DirectionMatch,
	pub(crate) source: DownwardSampleMatchSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DownwardViewportCandidate {
	pub(crate) source: DownwardViewportCandidateSource,
	pub(crate) viewport_top_y: i32,
	pub(crate) motion_rows: u32,
	pub(crate) mean_abs_diff_x100: u32,
}
impl DownwardViewportCandidate {
	pub(crate) fn competing_block_reason(self, competing: Self) -> &'static str {
		match (self.source, competing.source) {
			(
				DownwardViewportCandidateSource::CommittedKeyframe,
				DownwardViewportCandidateSource::CommittedKeyframe,
			) => "conflicting_committed_keyframe_authority",
			_ => "conflicting_downward_viewport_authority",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BlockedPreviewOnlyLocalCandidate {
	pub(crate) candidate: DownwardViewportCandidate,
	pub(crate) repeats: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverlapSearchRange {
	pub(crate) start: u32,
	pub(crate) end: u32,
}
impl OverlapSearchRange {
	pub(crate) fn as_range(self) -> RangeInclusive<u32> {
		self.start..=self.end
	}
}

impl From<RangeInclusive<u32>> for OverlapSearchRange {
	fn from(range: RangeInclusive<u32>) -> Self {
		Self { start: *range.start(), end: *range.end() }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DirectionMatchEval {
	pub(crate) preferred_range: Option<OverlapSearchRange>,
	pub(crate) max_motion_rows: u32,
	pub(crate) preferred_only_match: Option<DirectionMatch>,
	pub(crate) final_match: Option<DirectionMatch>,
	pub(crate) used_full_range_fallback: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MotionObservation {
	pub(crate) direction: ScrollDirection,
	pub(crate) motion_rows: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UpInputMatchLog {
	pub(crate) sample_motion: Option<MotionObservation>,
	pub(crate) sample_down_match: Option<DirectionMatch>,
	pub(crate) sample_up_match: Option<DirectionMatch>,
	pub(crate) committed_down_match: Option<DirectionMatch>,
	pub(crate) committed_up_match: Option<DirectionMatch>,
	pub(crate) sample_override_wins: bool,
	pub(crate) committed_override_wins: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UpInputSearchWindowLog<'a> {
	pub(crate) sample_delta: Option<u32>,
	pub(crate) sample_down_match_eval: &'a DirectionMatchEval,
	pub(crate) sample_up_match_eval: &'a DirectionMatchEval,
	pub(crate) committed_down_match_eval: &'a DirectionMatchEval,
	pub(crate) committed_up_match_eval: &'a DirectionMatchEval,
	pub(crate) frame_equals_last_sample: bool,
	pub(crate) frame_equals_last_committed: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UpwardInputDiagnostics {
	pub(crate) sample_down_match_eval: DirectionMatchEval,
	pub(crate) sample_up_match_eval: DirectionMatchEval,
	pub(crate) committed_down_match_eval: DirectionMatchEval,
	pub(crate) committed_up_match_eval: DirectionMatchEval,
	pub(crate) sample_override_match: Option<DirectionMatch>,
	pub(crate) committed_override_match: Option<DirectionMatch>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResumeFrontierMatchLog {
	pub(crate) motion_rows: u32,
	pub(crate) candidate_observed_viewport_top_y: i32,
	pub(crate) residual_growth_rows: u32,
	pub(crate) raw_committed_down_match: Option<DirectionMatch>,
	pub(crate) trusted_committed_down_match: Option<DirectionMatch>,
	pub(crate) committed_up_match: Option<DirectionMatch>,
	pub(crate) frame_reacquires_last_committed_viewport: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResumeFrontierDirectMatchContext {
	pub(crate) motion_rows: u32,
	pub(crate) candidate_observed_viewport_top_y: i32,
	pub(crate) residual_growth_rows: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct GrowthCommit {
	pub(crate) frame: RgbaImage,
	pub(crate) growth_rows: u32,
	pub(crate) viewport_top_y: i32,
	pub(crate) decision_source: &'static str,
	pub(crate) detected_motion_rows: Option<u32>,
	pub(crate) effective_motion_rows_hint: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InformativeSpan {
	pub(crate) start_x: u32,
	pub(crate) end_exclusive_x: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScrollDirection {
	Up,
	Down,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScrollObserveOutcome {
	NoChange,
	PreviewUpdated,
	UnsupportedDirection { direction: ScrollDirection },
	Committed { direction: ScrollDirection, growth_rows: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DownwardRegistration {
	NoMatch,
	Matched(DirectionMatch),
	Ambiguous { best: DirectionMatch, competing: DirectionMatch },
}
impl DownwardRegistration {
	pub(crate) fn map_source(
		self,
		source: DownwardSampleMatchSource,
	) -> DownwardRegistrationWithSource {
		match self {
			Self::NoMatch => DownwardRegistrationWithSource::NoMatch,
			Self::Matched(matched) => {
				DownwardRegistrationWithSource::Matched(DownwardSampleMatch { matched, source })
			},
			Self::Ambiguous { best, competing } => DownwardRegistrationWithSource::Ambiguous {
				best: DownwardSampleMatch { matched: best, source },
				competing: DownwardSampleMatch { matched: competing, source },
			},
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DownwardSampleMatchSource {
	ObservedSample,
	PreviewOnlyLocalSample,
}
impl DownwardSampleMatchSource {
	pub(crate) const fn label(self) -> &'static str {
		match self {
			Self::ObservedSample => "observed_sample",
			Self::PreviewOnlyLocalSample => "preview_only_local_sample",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DownwardRegistrationWithSource {
	NoMatch,
	Matched(DownwardSampleMatch),
	Ambiguous { best: DownwardSampleMatch, competing: DownwardSampleMatch },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DownwardViewportCandidateSource {
	ObservedSample,
	PreviewOnlyLocalSample,
	CommittedKeyframe,
}
impl DownwardViewportCandidateSource {
	pub(crate) const fn priority(self) -> u8 {
		match self {
			Self::CommittedKeyframe => 0,
			Self::ObservedSample => 1,
			Self::PreviewOnlyLocalSample => 2,
		}
	}

	pub(crate) const fn decision_source(self) -> &'static str {
		match self {
			Self::ObservedSample => "sample_motion_downward_growth_from_observed_keyframe",
			Self::PreviewOnlyLocalSample => {
				"sample_motion_downward_growth_from_preview_only_local_sample"
			},
			Self::CommittedKeyframe => "sample_motion_downward_growth_from_committed_keyframe",
		}
	}

	pub(crate) const fn fallback_decision_source(self) -> &'static str {
		match self {
			Self::ObservedSample => "fallback_downward_registration_from_observed_keyframe",
			Self::PreviewOnlyLocalSample => {
				"fallback_downward_registration_from_preview_only_local_sample"
			},
			Self::CommittedKeyframe => "fallback_downward_registration_from_committed_keyframe",
		}
	}
}

impl From<DownwardSampleMatchSource> for DownwardViewportCandidateSource {
	fn from(value: DownwardSampleMatchSource) -> Self {
		match value {
			DownwardSampleMatchSource::ObservedSample => Self::ObservedSample,
			DownwardSampleMatchSource::PreviewOnlyLocalSample => Self::PreviewOnlyLocalSample,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommittedDownwardViewportCandidateMode {
	LastCommittedOnly,
	IncludeRecentHistory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DownwardViewportResolution {
	NoMatch,
	Selected(DownwardViewportCandidate),
	Ambiguous { preferred: DownwardViewportCandidate, competing: DownwardViewportCandidate },
}
