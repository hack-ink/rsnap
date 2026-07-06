use color_eyre::eyre::{self, Result};
use image::RgbaImage;

use crate::scroll_capture::{
	DirectionMatch, DownwardRegistrationWithSource, DownwardSampleMatch, DownwardSampleMatchSource,
	MotionObservation, ScrollDirection, ScrollObserveOutcome, ScrollSession, support,
};

impl ScrollSession {
	pub(crate) fn observe_downward_sample(
		&mut self,
		frame: RgbaImage,
	) -> Result<ScrollObserveOutcome> {
		self.observe_downward_sample_with_motion_hint(frame, None)
	}

	pub(crate) fn observe_downward_sample_with_motion_hint(
		&mut self,
		frame: RgbaImage,
		motion_rows_hint: Option<u32>,
	) -> Result<ScrollObserveOutcome> {
		self.observe_downward_sample_with_motion_hint_and_burst(frame, motion_rows_hint, false)
	}

	pub(crate) fn observe_downward_sample_with_motion_hint_and_burst(
		&mut self,
		frame: RgbaImage,
		motion_rows_hint: Option<u32>,
		allow_burst_search: bool,
	) -> Result<ScrollObserveOutcome> {
		self.observe_sample_with_motion_context(
			frame,
			ScrollDirection::Down,
			motion_rows_hint,
			allow_burst_search,
		)
	}

	#[cfg(test)]
	pub(crate) fn observe_upward_sample(
		&mut self,
		frame: RgbaImage,
	) -> Result<ScrollObserveOutcome> {
		self.observe_sample_with_motion_context(frame, ScrollDirection::Up, None, false)
	}

	fn observe_sample_with_motion_context(
		&mut self,
		frame: RgbaImage,
		input_direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
		allow_burst_search: bool,
	) -> Result<ScrollObserveOutcome> {
		let previous_hint = self.transient_motion_rows_hint;
		let previous_burst = self.transient_burst_search_enabled;

		self.transient_motion_rows_hint = motion_rows_hint;
		self.transient_burst_search_enabled = allow_burst_search;

		self.record_last_sample_eval_context();

		let result = self.observe_sample(frame, input_direction);

		self.transient_motion_rows_hint = previous_hint;
		self.transient_burst_search_enabled = previous_burst;

		result
	}

	fn observe_sample(
		&mut self,
		frame: RgbaImage,
		input_direction: ScrollDirection,
	) -> Result<ScrollObserveOutcome> {
		self.clear_last_downward_sample_registration();

		if frame.width() != self.anchor_frame.width() {
			return Err(eyre::eyre!(
				"frame width mismatch: expected {} got {}",
				self.anchor_frame.width(),
				frame.width()
			));
		}

		let use_resume_local_sample = matches!(input_direction, ScrollDirection::Down)
			&& self.resume_frontier_top_y.is_some();

		if self.matches_downward_no_change_frame(input_direction, use_resume_local_sample, &frame) {
			self.log_decision(
				"scroll_capture.sample_no_change",
				input_direction,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("frame_matches_last_downward_observed_frame"),
			);

			return Ok(ScrollObserveOutcome::NoChange);
		}

		let fingerprint = support::scroll_capture_fingerprint(&frame);

		if matches!(input_direction, ScrollDirection::Up)
			&& self.resume_frontier_top_y.is_some()
			&& self.last_unconfirmed_upward_fingerprint.as_deref() == Some(fingerprint.as_slice())
		{
			self.log_decision(
				"scroll_capture.sample_no_change",
				input_direction,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("frame_matches_last_unconfirmed_upward_fingerprint"),
			);

			return Ok(ScrollObserveOutcome::NoChange);
		}

		let sample_delta =
			self.sample_delta_for_input(input_direction, use_resume_local_sample, &fingerprint);
		let (sample_motion, downward_sample_match) =
			self.classify_input_sample_motion(input_direction, use_resume_local_sample, &frame);
		let preview_changed =
			sample_delta.is_some_and(|delta| delta > 0) || sample_motion.is_some();

		tracing::info!(
			op = "scroll_capture.sample_eval",
			input_direction = ?input_direction,
			use_resume_local_sample,
			sample_delta,
			sample_motion_direction = ?sample_motion.map(|motion| motion.direction),
			sample_motion_rows = ?sample_motion.map(|motion| motion.motion_rows),
			preview_changed,
			frame_equals_last_sample = frame == self.last_sample_frame,
			frame_equals_last_downward_observed = frame == self.last_downward_observed_frame,
			frame_equals_last_preview_only_downward_local = self
				.last_preview_only_downward_local_sample
				.as_ref()
				.is_some_and(|previous| frame == previous.frame),
			last_preview_only_downward_local_viewport_top_y = ?self
				.last_preview_only_downward_local_sample
				.as_ref()
				.map(|sample| sample.viewport_top_y),
			frame_equals_last_committed = frame == self.last_committed_frame,
			last_motion_rows_hint = ?self.last_motion_rows_hint,
			transient_motion_rows_hint = ?self.transient_motion_rows_hint,
			effective_motion_rows_hint = ?self.effective_motion_rows_hint(),
			current_viewport_top_y = self.current_viewport_top_y,
			observed_viewport_top_y = self.observed_viewport_top_y,
			resume_frontier_top_y = ?self.resume_frontier_top_y,
			resume_frontier_requires_reacquire = self.resume_frontier_requires_reacquire,
			"Scroll-capture session evaluated a sampled frame before commit resolution."
		);

		if !preview_changed {
			self.log_decision(
				"scroll_capture.sample_no_change",
				input_direction,
				None,
				Some(self.observed_viewport_top_y),
				Some(0),
				Some("sample_delta_and_motion_both_absent"),
			);

			return Ok(ScrollObserveOutcome::NoChange);
		}
		if matches!(input_direction, ScrollDirection::Up) {
			return self.observe_upward_input(
				frame,
				fingerprint,
				sample_delta,
				sample_motion,
				preview_changed,
			);
		}

		self.observe_downward_input(frame, sample_motion, downward_sample_match, preview_changed)
	}

	fn matches_downward_no_change_frame(
		&self,
		input_direction: ScrollDirection,
		use_resume_local_sample: bool,
		frame: &RgbaImage,
	) -> bool {
		matches!(input_direction, ScrollDirection::Down)
			&& !use_resume_local_sample
			&& frame
				== if self.initial_downward_bootstrap_active() {
					&self.last_sample_frame
				} else {
					&self.last_downward_observed_frame
				}
	}

	fn sample_delta_for_input(
		&self,
		input_direction: ScrollDirection,
		use_resume_local_sample: bool,
		fingerprint: &[u8],
	) -> Option<u32> {
		match (input_direction, use_resume_local_sample) {
			(ScrollDirection::Down, true) => self
				.last_sample_fingerprint
				.as_ref()
				.map(|previous| support::scroll_capture_fingerprint_delta(previous, fingerprint)),
			(ScrollDirection::Down, false) => self
				.last_downward_observed_fingerprint
				.as_ref()
				.map(|previous| support::scroll_capture_fingerprint_delta(previous, fingerprint)),
			(ScrollDirection::Up, _) => self
				.last_sample_fingerprint
				.as_ref()
				.map(|previous| support::scroll_capture_fingerprint_delta(previous, fingerprint)),
		}
	}

	fn classify_input_sample_motion(
		&mut self,
		input_direction: ScrollDirection,
		use_resume_local_sample: bool,
		frame: &RgbaImage,
	) -> (Option<MotionObservation>, Option<DownwardSampleMatch>) {
		if matches!(input_direction, ScrollDirection::Up) {
			return (self.classify_sample_motion(frame), None);
		}

		let downward_registration = if use_resume_local_sample {
			self.classify_downward_sample_motion_against(&self.last_sample_frame, frame)
				.0
				.map_source(DownwardSampleMatchSource::ObservedSample)
		} else {
			self.classify_downward_sample_motion_with_local_recovery(frame)
		};

		match downward_registration {
			DownwardRegistrationWithSource::Matched(matched) => {
				self.record_last_downward_sample_registration(
					"matched",
					Some(matched.source),
					Some(matched.matched.motion_rows),
				);

				(
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: matched.matched.motion_rows,
					}),
					Some(matched),
				)
			},
			DownwardRegistrationWithSource::Ambiguous { best, .. } => {
				self.record_last_downward_sample_registration(
					"ambiguous",
					Some(best.source),
					Some(best.matched.motion_rows),
				);
				self.log_decision(
					"scroll_capture.down_input_ambiguous_registration",
					ScrollDirection::Down,
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: best.matched.motion_rows,
					}),
					Some(self.current_viewport_top_y),
					Some(0),
					Some("sample_downward_registration_competed_with_far_apart_candidate"),
				);

				(None, None)
			},
			DownwardRegistrationWithSource::NoMatch => {
				self.record_last_downward_sample_registration("no_match", None, None);

				(None, None)
			},
		}
	}

	fn observe_downward_input(
		&mut self,
		frame: RgbaImage,
		sample_motion: Option<MotionObservation>,
		downward_sample_match: Option<DownwardSampleMatch>,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		self.last_unconfirmed_upward_fingerprint = None;

		if let Some(motion) = sample_motion {
			match motion.direction {
				ScrollDirection::Up => {
					let committed_down_match = self.evaluate_reference_overlap_direction(
						&self.last_committed_frame,
						&frame,
						ScrollDirection::Down,
						self.effective_motion_rows_hint(),
					);
					let committed_up_match = self.evaluate_reference_overlap_direction(
						&self.last_committed_frame,
						&frame,
						ScrollDirection::Up,
						self.effective_motion_rows_hint(),
					);

					if let Some(up_match) = support::upward_confirmation_match_for_downward_input(
						committed_up_match,
						committed_down_match,
						self.current_viewport_top_y > 0,
					) {
						return Ok(self.fail_closed_downward_non_monotonic_frame(
							preview_changed,
							self.last_sample_frame.clone(),
							self.last_sample_fingerprint.clone(),
							"scroll_capture.down_input_detected_upward_motion",
							MotionObservation {
								direction: ScrollDirection::Up,
								motion_rows: up_match.motion_rows,
							},
							"downward_input_confirmed_upward_motion_with_last_committed_match",
						));
					}

					return Ok(self.fail_closed_downward_non_monotonic_frame(
						preview_changed,
						self.last_sample_frame.clone(),
						self.last_sample_fingerprint.clone(),
						"scroll_capture.down_input_detected_upward_motion",
						motion,
						"downward_input_upward_motion_lacked_committed_support",
					));
				},
				ScrollDirection::Down => {
					return self.observe_downward_motion(
						frame,
						downward_sample_match.unwrap_or(DownwardSampleMatch {
							matched: DirectionMatch {
								mean_abs_diff_x100: u32::MAX,
								motion_rows: motion.motion_rows,
							},
							source: DownwardSampleMatchSource::ObservedSample,
						}),
						preview_changed,
					);
				},
			}
		}

		self.observe_fallback_downward_growth(frame, preview_changed)
	}
}
