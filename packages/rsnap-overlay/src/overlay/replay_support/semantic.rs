use image::RgbaImage;

use crate::overlay::replay_support::{
	RecordedScrollCaptureReplayRecordedOutcome, RecordedScrollCaptureSemanticIssue,
};

pub(super) fn estimate_recorded_downward_shift_rows(
	previous: &RgbaImage,
	current: &RgbaImage,
) -> Option<u32> {
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

pub(super) fn classify_recorded_semantic_issue(
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
