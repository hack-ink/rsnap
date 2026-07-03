use image::RgbaImage;

use crate::scroll_capture::{
	MotionObservation, PreviewOnlyDownwardLocalSample, ScrollDirection, ScrollObserveOutcome,
	ScrollSession, support,
};

impl ScrollSession {
	pub(super) fn record_last_sample(&mut self, frame: &RgbaImage, fingerprint: Vec<u8>) {
		self.last_sample_frame = frame.clone();
		self.last_sample_fingerprint = Some(fingerprint);
	}

	pub(super) fn record_last_downward_observed_sample(
		&mut self,
		frame: &RgbaImage,
		fingerprint: Vec<u8>,
	) {
		self.last_downward_observed_frame = frame.clone();
		self.last_downward_observed_fingerprint = Some(fingerprint);
	}

	pub(super) fn record_preview_only_downward_local_sample(
		&mut self,
		frame: &RgbaImage,
		viewport_top_y: i32,
	) {
		self.last_preview_only_downward_local_sample =
			Some(PreviewOnlyDownwardLocalSample { frame: frame.clone(), viewport_top_y });
	}

	pub(super) fn clear_preview_only_downward_local_sample(&mut self) {
		self.last_preview_only_downward_local_sample = None;
		self.seeded_preview_only_local_after_observed_burst_commit = false;
		self.pending_unresolved_burst_registered_growth_viewport_top_y = None;
		self.last_blocked_preview_only_local_candidate = None;
	}

	pub(super) fn clear_preview_only_downward_recovery_carryover(&mut self) {
		self.clear_preview_only_downward_local_sample();

		self.pending_suppressed_huge_preview_only_local_followup = None;
		self.pending_suppressed_huge_preview_only_local_followup_remaining_blocks = 0;
		self.pending_extreme_preview_only_local_tail_followup = None;
		self.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 0;
	}

	pub(super) fn restore_last_sample(&mut self, frame: RgbaImage, fingerprint: Option<Vec<u8>>) {
		self.last_sample_frame = frame;
		self.last_sample_fingerprint = fingerprint;
	}

	pub(super) fn fail_closed_downward_non_monotonic_frame(
		&mut self,
		preview_changed: bool,
		previous_sample_frame: RgbaImage,
		previous_sample_fingerprint: Option<Vec<u8>>,
		op: &'static str,
		detected_motion: MotionObservation,
		block_reason: &'static str,
	) -> ScrollObserveOutcome {
		self.restore_last_sample(previous_sample_frame, previous_sample_fingerprint);
		self.clear_preview_only_downward_local_sample();
		self.log_decision(
			op,
			ScrollDirection::Down,
			Some(detected_motion),
			Some(self.current_viewport_top_y),
			Some(0),
			Some(block_reason),
		);

		support::preview_update_outcome(preview_changed)
	}

	pub(super) fn observe_upward_rewind(&mut self, motion_rows: u32) {
		let motion_rows = i32::try_from(motion_rows).unwrap_or(i32::MAX);

		self.observe_upward_rewind_to_observed_top_y(
			self.observed_viewport_top_y.saturating_sub(motion_rows),
			self.current_viewport_top_y,
		);
	}

	pub(super) fn observe_upward_rewind_from_committed(&mut self, motion_rows: u32) {
		let motion_rows = i32::try_from(motion_rows).unwrap_or(i32::MAX);

		self.observe_upward_rewind_to_observed_top_y(
			self.current_viewport_top_y.saturating_sub(motion_rows),
			self.current_viewport_top_y,
		);
	}

	pub(super) fn observe_unconfirmed_upward_rewind(&mut self) {
		self.last_motion_rows_hint = None;

		self.clear_preview_only_downward_local_sample();

		let frontier_top_y = self.current_viewport_top_y;

		self.resume_frontier_top_y.get_or_insert(frontier_top_y);

		self.resume_frontier_requires_reacquire = true;
		self.observed_viewport_top_y =
			self.observed_viewport_top_y.min(frontier_top_y.saturating_sub(1));
	}

	fn observe_upward_rewind_to_observed_top_y(
		&mut self,
		observed_viewport_top_y: i32,
		frontier_top_y: i32,
	) {
		self.last_motion_rows_hint = None;

		self.clear_preview_only_downward_local_sample();

		self.resume_frontier_requires_reacquire = true;

		self.resume_frontier_top_y.get_or_insert(frontier_top_y);

		self.observed_viewport_top_y = observed_viewport_top_y;
	}
}
