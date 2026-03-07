use std::ops::RangeInclusive;

use color_eyre::eyre::{self, Result};
use image::{RgbaImage, imageops::FilterType};

const FINGERPRINT_GRID_COLUMNS: u32 = 12;
const FINGERPRINT_GRID_ROWS: u32 = 16;
const DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS: u32 = 12;
const INITIAL_DOWNWARD_MAX_MOTION_ROWS: u32 = 192;
const MOTION_SEARCH_BAND_ROWS: u32 = 96;
const DIRECTION_WARNING_MARGIN_X100: u32 = 90;
const INFORMATIVE_SPAN_ROW_SAMPLES: u32 = 24;
const INFORMATIVE_SPAN_SCORE_FLOOR_X100: u32 = 24;
const INFORMATIVE_SPAN_HORIZONTAL_PADDING_PX: u32 = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScrollFrameFingerprint {
	grid_columns: u32,
	grid_rows: u32,
	samples: Vec<[u8; 4]>,
}
impl ScrollFrameFingerprint {
	#[must_use]
	pub(crate) fn from_image(image: &RgbaImage) -> Self {
		let width = image.width().max(1);
		let height = image.height().max(1);
		let informative_span = informative_column_span(image, 0, height);
		let informative_left =
			informative_span.map_or(0, |span| span.start_x.min(width.saturating_sub(1)));
		let informative_right = informative_span
			.map_or(width, |span| span.end_exclusive_x.min(width).max(informative_left + 1));
		let informative_width = informative_right.saturating_sub(informative_left).max(1);
		let margin_x = ((informative_width as f32) * 0.05).round() as u32;
		let margin_y = ((height as f32) * 0.05).round() as u32;
		let left =
			informative_left.saturating_add(margin_x).min(informative_right.saturating_sub(1));
		let right = informative_right.saturating_sub(margin_x).max(left + 1);
		let top = margin_y.min(height.saturating_sub(1));
		let bottom = height.saturating_sub(margin_y).max(top + 1);
		let mut samples =
			Vec::with_capacity((FINGERPRINT_GRID_COLUMNS * FINGERPRINT_GRID_ROWS) as usize);

		for row in 0..FINGERPRINT_GRID_ROWS {
			let y = evenly_spaced_sample(top, bottom, row, FINGERPRINT_GRID_ROWS);

			for column in 0..FINGERPRINT_GRID_COLUMNS {
				let x = evenly_spaced_sample(left, right, column, FINGERPRINT_GRID_COLUMNS);
				let pixel = image.get_pixel(x, y).0;

				samples.push(pixel);
			}
		}

		Self { grid_columns: FINGERPRINT_GRID_COLUMNS, grid_rows: FINGERPRINT_GRID_ROWS, samples }
	}

	#[must_use]
	pub(crate) fn into_bytes(self) -> Vec<u8> {
		let mut bytes = Vec::with_capacity(self.samples.len().saturating_mul(4));

		for sample in self.samples {
			bytes.extend_from_slice(&sample);
		}

		bytes
	}

	#[must_use]
	#[cfg(test)]
	pub(crate) fn distance(&self, other: &Self) -> u64 {
		if self.grid_columns != other.grid_columns || self.grid_rows != other.grid_rows {
			return u64::MAX;
		}

		self.samples
			.iter()
			.zip(&other.samples)
			.map(|(left, right)| {
				u64::from(left[0].abs_diff(right[0]))
					+ u64::from(left[1].abs_diff(right[1]))
					+ u64::from(left[2].abs_diff(right[2]))
					+ u64::from(left[3].abs_diff(right[3]))
			})
			.sum()
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverlapMatch {
	pub(crate) rows: u32,
	pub(crate) matched: bool,
	pub(crate) mean_abs_diff_x100: u32,
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
			max_column_samples: 32,
			max_row_samples: 8,
			max_mean_abs_diff_x100: 850,
		}
	}
}

#[derive(Clone, Debug)]
pub(crate) struct ScrollSession {
	anchor_frame: RgbaImage,
	anchor_preview: RgbaImage,
	export_image: RgbaImage,
	preview_image: RgbaImage,
	bottom_segments: Vec<RgbaImage>,
	bottom_preview_segments: Vec<RgbaImage>,
	growth_history: Vec<GrowthCommit>,
	last_committed_frame: RgbaImage,
	last_sample_frame: RgbaImage,
	last_sample_fingerprint: Option<Vec<u8>>,
	last_motion_rows_hint: Option<u32>,
	rewind_rows_pending: u32,
	current_viewport_top_y: i32,
	preview_width_px: u32,
}
impl ScrollSession {
	pub(crate) fn new(base_frame: RgbaImage, preview_width_px: u32) -> Result<Self> {
		let fingerprint = scroll_capture_fingerprint(&base_frame);
		let anchor_preview = resize_strip_to_preview_width(&base_frame, preview_width_px.max(1));

		Ok(Self {
			anchor_frame: base_frame.clone(),
			anchor_preview: anchor_preview.clone(),
			export_image: base_frame.clone(),
			preview_image: anchor_preview,
			bottom_segments: Vec::new(),
			bottom_preview_segments: Vec::new(),
			growth_history: Vec::new(),
			last_committed_frame: base_frame.clone(),
			last_sample_frame: base_frame,
			last_sample_fingerprint: Some(fingerprint),
			last_motion_rows_hint: None,
			rewind_rows_pending: 0,
			current_viewport_top_y: 0,
			preview_width_px: preview_width_px.max(1),
		})
	}

	pub(crate) fn observe_downward_sample(
		&mut self,
		frame: RgbaImage,
	) -> Result<ScrollObserveOutcome> {
		if frame.width() != self.anchor_frame.width() {
			return Err(eyre::eyre!(
				"frame width mismatch: expected {} got {}",
				self.anchor_frame.width(),
				frame.width()
			));
		}

		let fingerprint = scroll_capture_fingerprint(&frame);
		let sample_delta = self
			.last_sample_fingerprint
			.as_ref()
			.map(|previous| scroll_capture_fingerprint_delta(previous, &fingerprint));
		let sample_motion = self.classify_sample_motion(&frame);
		let preview_changed =
			sample_delta.is_some_and(|delta| delta > 0) || sample_motion.is_some();

		self.last_sample_frame = frame.clone();
		self.last_sample_fingerprint = Some(fingerprint);

		if !preview_changed {
			return Ok(ScrollObserveOutcome::NoChange);
		}

		if let Some(motion) = sample_motion {
			match motion.direction {
				ScrollDirection::Up => {
					self.rewind_rows_pending =
						self.rewind_rows_pending.saturating_add(motion.motion_rows);

					return Ok(ScrollObserveOutcome::UnsupportedDirection {
						direction: ScrollDirection::Up,
					});
				},
				ScrollDirection::Down => {
					self.last_motion_rows_hint = Some(motion.motion_rows);

					let growth_rows = self.consume_downward_motion_rows(motion.motion_rows);

					if growth_rows > 0 {
						let next_viewport_top_y = self
							.current_viewport_top_y
							.saturating_add(i32::try_from(growth_rows).unwrap_or_default());

						return self.apply_growth(frame, growth_rows, next_viewport_top_y);
					}

					return Ok(preview_update_outcome(preview_changed));
				},
			}
		}

		self.observe_fallback_downward_growth(frame, preview_changed)
	}

	fn classify_sample_motion(&self, frame: &RgbaImage) -> Option<MotionObservation> {
		let down_match = self.evaluate_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Down,
			self.last_motion_rows_hint,
		);
		let up_match = self.evaluate_reference_overlap_direction(
			&self.last_sample_frame,
			frame,
			ScrollDirection::Up,
			self.last_motion_rows_hint,
		);

		match (down_match, up_match) {
			(Some(down), Some(up)) => {
				if down.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					< up.mean_abs_diff_x100
				{
					Some(MotionObservation {
						direction: ScrollDirection::Down,
						motion_rows: down.motion_rows,
					})
				} else if up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100
				{
					Some(MotionObservation {
						direction: ScrollDirection::Up,
						motion_rows: up.motion_rows,
					})
				} else {
					None
				}
			},
			(Some(down), None) => Some(MotionObservation {
				direction: ScrollDirection::Down,
				motion_rows: down.motion_rows,
			}),
			(None, Some(up)) => Some(MotionObservation {
				direction: ScrollDirection::Up,
				motion_rows: up.motion_rows,
			}),
			(None, None) => None,
		}
	}

	pub(crate) fn preview_image(&self) -> &RgbaImage {
		&self.preview_image
	}

	pub(crate) fn export_image(&self) -> &RgbaImage {
		&self.export_image
	}

	pub(crate) fn undo_last_append(&mut self) -> bool {
		let Some(_commit) = self.growth_history.pop() else {
			return false;
		};
		let _ = self.bottom_segments.pop();
		let _ = self.bottom_preview_segments.pop();
		let Ok(export_image) = self.rebuild_export_image() else {
			return false;
		};
		let Ok(preview_image) = self.rebuild_preview_image() else {
			return false;
		};

		self.export_image = export_image;
		self.preview_image = preview_image;

		if let Some(previous) = self.growth_history.last() {
			self.last_motion_rows_hint = Some(previous.growth_rows);
			self.current_viewport_top_y = previous.viewport_top_y;
			self.last_committed_frame = previous.frame.clone();
			self.last_sample_frame = previous.frame.clone();
			self.last_sample_fingerprint = Some(scroll_capture_fingerprint(&previous.frame));
			self.rewind_rows_pending = 0;
		} else {
			self.last_committed_frame = self.anchor_frame.clone();
			self.last_sample_frame = self.anchor_frame.clone();
			self.last_sample_fingerprint = Some(scroll_capture_fingerprint(&self.anchor_frame));
			self.last_motion_rows_hint = None;
			self.rewind_rows_pending = 0;
			self.current_viewport_top_y = 0;
		}

		true
	}

	fn evaluate_reference_overlap_direction(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		direction: ScrollDirection,
		motion_rows_hint: Option<u32>,
	) -> Option<DirectionMatch> {
		let config = OverlapSearchConfig::default();
		let preferred_range =
			self.preferred_motion_range_from_hint(previous, next, motion_rows_hint, config)?;
		let preferred =
			evaluate_overlap_direction(previous, next, direction, preferred_range, config);

		if preferred.is_some()
			|| motion_rows_hint.is_none()
			|| matches!(direction, ScrollDirection::Up)
		{
			return preferred;
		}

		evaluate_overlap_direction(
			previous,
			next,
			direction,
			1..=max_directional_motion_rows(previous, next, config),
			config,
		)
	}

	fn preferred_motion_range_from_hint(
		&self,
		previous: &RgbaImage,
		next: &RgbaImage,
		motion_rows_hint: Option<u32>,
		config: OverlapSearchConfig,
	) -> Option<RangeInclusive<u32>> {
		let max_motion_rows = max_directional_motion_rows(previous, next, config);

		if let Some(last_growth_rows) = motion_rows_hint {
			let tolerance = DOWNWARD_SEARCH_MOTION_TOLERANCE_ROWS.min(max_motion_rows);
			let min_motion_rows = last_growth_rows.saturating_sub(tolerance).max(1);
			let max_motion_rows = last_growth_rows.saturating_add(tolerance).min(max_motion_rows);

			return Some(min_motion_rows..=max_motion_rows);
		}

		Some(1..=INITIAL_DOWNWARD_MAX_MOTION_ROWS.min(max_motion_rows).max(1))
	}

	fn consume_downward_motion_rows(&mut self, motion_rows: u32) -> u32 {
		if self.rewind_rows_pending == 0 {
			return motion_rows;
		}

		let growth_rows = motion_rows.saturating_sub(self.rewind_rows_pending);

		self.rewind_rows_pending = self.rewind_rows_pending.saturating_sub(motion_rows);

		growth_rows
	}

	fn observe_fallback_downward_growth(
		&mut self,
		frame: RgbaImage,
		preview_changed: bool,
	) -> Result<ScrollObserveOutcome> {
		if self.rewind_rows_pending > 0 {
			return Ok(preview_update_outcome(preview_changed));
		}

		let down_match = self.evaluate_reference_overlap_direction(
			&self.last_committed_frame,
			&frame,
			ScrollDirection::Down,
			self.last_motion_rows_hint,
		);
		let up_match = self.evaluate_reference_overlap_direction(
			&self.last_committed_frame,
			&frame,
			ScrollDirection::Up,
			self.last_motion_rows_hint,
		);

		match (down_match, up_match) {
			(Some(down), Some(up)) => {
				if up.mean_abs_diff_x100.saturating_add(DIRECTION_WARNING_MARGIN_X100)
					<= down.mean_abs_diff_x100
				{
					return Ok(ScrollObserveOutcome::UnsupportedDirection {
						direction: ScrollDirection::Up,
					});
				}

				let next_viewport_top_y = self
					.current_viewport_top_y
					.saturating_add(i32::try_from(down.motion_rows).unwrap_or_default());

				self.last_motion_rows_hint = Some(down.motion_rows);

				self.apply_growth(frame, down.motion_rows, next_viewport_top_y)
			},
			(Some(down), None) => {
				let next_viewport_top_y = self
					.current_viewport_top_y
					.saturating_add(i32::try_from(down.motion_rows).unwrap_or_default());

				self.last_motion_rows_hint = Some(down.motion_rows);

				self.apply_growth(frame, down.motion_rows, next_viewport_top_y)
			},
			(None, Some(_up)) => {
				Ok(ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up })
			},
			(None, None) => Ok(preview_update_outcome(preview_changed)),
		}
	}

	fn apply_growth(
		&mut self,
		frame: RgbaImage,
		growth_rows: u32,
		viewport_top_y: i32,
	) -> Result<ScrollObserveOutcome> {
		let strip = crop_bottom_rows(&frame, growth_rows)
			.ok_or_else(|| eyre::eyre!("failed to extract growth strip"))?;
		let preview_strip = resize_strip_to_preview_width(&strip, self.preview_width_px);

		self.export_image = append_vertical_image(&self.export_image, &strip)?;
		self.preview_image = append_vertical_image(&self.preview_image, &preview_strip)?;

		self.bottom_segments.push(strip);
		self.bottom_preview_segments.push(preview_strip);

		self.current_viewport_top_y = viewport_top_y;
		self.last_committed_frame = frame.clone();
		self.rewind_rows_pending = 0;

		self.growth_history.push(GrowthCommit { frame, growth_rows, viewport_top_y });

		Ok(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows })
	}

	fn rebuild_export_image(&self) -> Result<RgbaImage> {
		let mut ordered = Vec::with_capacity(self.bottom_segments.len().saturating_add(1));

		ordered.push(&self.anchor_frame);

		for strip in &self.bottom_segments {
			ordered.push(strip);
		}

		stack_vertical_images(&ordered)
	}

	fn rebuild_preview_image(&self) -> Result<RgbaImage> {
		let mut ordered = Vec::with_capacity(self.bottom_preview_segments.len().saturating_add(1));

		ordered.push(&self.anchor_preview);

		for strip in &self.bottom_preview_segments {
			ordered.push(strip);
		}

		stack_vertical_images(&ordered)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectionMatch {
	mean_abs_diff_x100: u32,
	motion_rows: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MotionObservation {
	direction: ScrollDirection,
	motion_rows: u32,
}

#[derive(Clone, Debug)]
struct GrowthCommit {
	frame: RgbaImage,
	growth_rows: u32,
	viewport_top_y: i32,
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
enum OverlapOrientation {
	PreviousBottomToNextTop,
	PreviousTopToNextBottom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InformativeSpan {
	start_x: u32,
	end_exclusive_x: u32,
}
#[must_use]
pub(crate) fn scroll_capture_fingerprint(image: &RgbaImage) -> Vec<u8> {
	ScrollFrameFingerprint::from_image(image).into_bytes()
}

#[must_use]
pub(crate) fn scroll_capture_fingerprint_delta(left: &[u8], right: &[u8]) -> u32 {
	if left.len() != right.len() || left.is_empty() || !left.len().is_multiple_of(4) {
		return u32::MAX;
	}

	let mut total_abs_diff = 0_u64;
	let mut comparisons = 0_u64;

	for (left_pixel, right_pixel) in left.chunks_exact(4).zip(right.chunks_exact(4)) {
		total_abs_diff = total_abs_diff
			.saturating_add(u64::from(left_pixel[0].abs_diff(right_pixel[0])))
			.saturating_add(u64::from(left_pixel[1].abs_diff(right_pixel[1])))
			.saturating_add(u64::from(left_pixel[2].abs_diff(right_pixel[2])))
			.saturating_add(u64::from(left_pixel[3].abs_diff(right_pixel[3])));
		comparisons = comparisons.saturating_add(4);
	}

	if comparisons == 0 { u32::MAX } else { (total_abs_diff / comparisons) as u32 }
}

#[cfg(test)]
#[must_use]
pub(crate) fn detect_vertical_overlap(
	previous: &RgbaImage,
	next: &RgbaImage,
	config: OverlapSearchConfig,
) -> OverlapMatch {
	detect_vertical_overlap_in_range(
		previous,
		next,
		1..=previous.height().min(next.height()),
		ScrollDirection::Down,
		config,
		overlap_global_informative_span(previous, next),
	)
}

fn evaluate_overlap_direction(
	previous: &RgbaImage,
	next: &RgbaImage,
	direction: ScrollDirection,
	range: RangeInclusive<u32>,
	config: OverlapSearchConfig,
) -> Option<DirectionMatch> {
	let max_overlap = previous.height().min(next.height());
	let overlap = detect_vertical_overlap_in_range(
		previous,
		next,
		range,
		direction,
		config,
		overlap_global_informative_span(previous, next),
	);

	if !overlap.matched {
		return None;
	}

	let motion_rows = max_overlap.saturating_sub(overlap.rows);

	if motion_rows == 0 {
		return None;
	}

	Some(DirectionMatch { mean_abs_diff_x100: overlap.mean_abs_diff_x100, motion_rows })
}

fn preview_update_outcome(preview_changed: bool) -> ScrollObserveOutcome {
	if preview_changed {
		ScrollObserveOutcome::PreviewUpdated
	} else {
		ScrollObserveOutcome::NoChange
	}
}

fn max_directional_motion_rows(
	previous: &RgbaImage,
	next: &RgbaImage,
	config: OverlapSearchConfig,
) -> u32 {
	let max_overlap = previous.height().min(next.height());
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };

	max_overlap.saturating_sub(effective_min_overlap).max(1)
}

fn detect_vertical_overlap_in_range(
	previous: &RgbaImage,
	next: &RgbaImage,
	range: RangeInclusive<u32>,
	direction: ScrollDirection,
	config: OverlapSearchConfig,
	informative_span: Option<InformativeSpan>,
) -> OverlapMatch {
	if previous.width() == 0 || next.width() == 0 || previous.height() == 0 || next.height() == 0 {
		return OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };
	}

	let Some(informative_span) = informative_span else {
		return OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };
	};
	let max_overlap = previous.height().min(next.height());
	let effective_min_overlap =
		if max_overlap <= config.min_overlap_rows { 1 } else { config.min_overlap_rows.max(1) };
	let max_motion_rows = max_overlap.saturating_sub(effective_min_overlap).max(1);
	let search_start = (*range.start()).max(1).min(max_motion_rows);
	let search_end = (*range.end()).max(search_start).min(max_motion_rows);
	let orientation = match direction {
		ScrollDirection::Down => OverlapOrientation::PreviousBottomToNextTop,
		ScrollDirection::Up => OverlapOrientation::PreviousTopToNextBottom,
	};
	let mut best = OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };

	for motion_rows in search_start..=search_end {
		let overlap_rows = max_overlap.saturating_sub(motion_rows);

		if overlap_rows < effective_min_overlap {
			continue;
		}

		let band_rows = overlap_rows.clamp(1, MOTION_SEARCH_BAND_ROWS);
		let diff = motion_mean_abs_diff_x100(
			previous,
			next,
			motion_rows,
			band_rows,
			config,
			orientation,
			informative_span,
		);

		if diff > config.max_mean_abs_diff_x100 {
			continue;
		}
		if !best.matched
			|| diff < best.mean_abs_diff_x100
			|| (diff == best.mean_abs_diff_x100 && overlap_rows > best.rows)
		{
			best = OverlapMatch { rows: overlap_rows, matched: true, mean_abs_diff_x100: diff };
		}
	}

	best
}

fn resize_strip_to_preview_width(strip: &RgbaImage, preview_width_px: u32) -> RgbaImage {
	if strip.width() <= preview_width_px {
		return strip.clone();
	}

	let preview_height = ((strip.height() as f32 / strip.width() as f32) * preview_width_px as f32)
		.round()
		.max(1.0) as u32;

	image::imageops::resize(strip, preview_width_px, preview_height, FilterType::Triangle)
}

fn crop_bottom_rows(frame: &RgbaImage, rows: u32) -> Option<RgbaImage> {
	let rows = rows.min(frame.height());

	if rows == 0 {
		return None;
	}

	let start_y = frame.height().saturating_sub(rows);

	Some(image::imageops::crop_imm(frame, 0, start_y, frame.width(), rows).to_image())
}

fn stack_vertical_images(images: &[&RgbaImage]) -> Result<RgbaImage> {
	let Some(first) = images.first() else {
		return Err(eyre::eyre!("cannot stack an empty image list"));
	};
	let width = first.width();
	let total_height = images.iter().try_fold(0_u32, |acc, image| {
		if image.width() != width {
			return Err(eyre::eyre!(
				"image width mismatch while stacking: expected {} got {}",
				width,
				image.width()
			));
		}

		acc.checked_add(image.height()).ok_or_else(|| eyre::eyre!("stacked image height overflow"))
	})?;
	let total_bytes = images.iter().try_fold(0_usize, |acc, image| {
		acc.checked_add(image.as_raw().len())
			.ok_or_else(|| eyre::eyre!("stacked image byte length overflow"))
	})?;
	let mut raw = Vec::with_capacity(total_bytes);

	for image in images {
		raw.extend_from_slice(image.as_raw());
	}

	RgbaImage::from_raw(width, total_height, raw)
		.ok_or_else(|| eyre::eyre!("failed to construct stacked image buffer"))
}

fn append_vertical_image(base: &RgbaImage, strip: &RgbaImage) -> Result<RgbaImage> {
	if base.width() != strip.width() {
		return Err(eyre::eyre!(
			"image width mismatch while appending: expected {} got {}",
			base.width(),
			strip.width()
		));
	}

	stack_vertical_images(&[base, strip])
}

fn motion_mean_abs_diff_x100(
	previous: &RgbaImage,
	next: &RgbaImage,
	motion_rows: u32,
	band_rows: u32,
	config: OverlapSearchConfig,
	orientation: OverlapOrientation,
	informative_span: InformativeSpan,
) -> u32 {
	let width = previous.width().min(next.width());
	let max_overlap = previous.height().min(next.height());
	let overlap_rows = max_overlap.saturating_sub(motion_rows);

	if overlap_rows == 0 {
		return u32::MAX;
	}

	let band_rows = band_rows.min(overlap_rows).max(1);
	let column_samples = width.min(config.max_column_samples).max(1);
	let row_samples = band_rows.min(config.max_row_samples).max(1);
	let previous_overlap_start_y = previous.height().saturating_sub(overlap_rows);
	let next_overlap_start_y = next.height().saturating_sub(overlap_rows);
	let previous_start_y = match orientation {
		OverlapOrientation::PreviousBottomToNextTop => previous_overlap_start_y,
		OverlapOrientation::PreviousTopToNextBottom => 0,
	};
	let next_start_y = match orientation {
		OverlapOrientation::PreviousBottomToNextTop => 0,
		OverlapOrientation::PreviousTopToNextBottom => next_overlap_start_y,
	};
	let x_start = informative_span.start_x.min(width.saturating_sub(1));
	let x_end = informative_span.end_exclusive_x.min(width).max(x_start + 1);
	let effective_width = x_end.saturating_sub(x_start).max(1);
	let column_samples = effective_width.min(column_samples).max(1);
	let mut total_abs_diff = 0_u64;
	let mut comparisons = 0_u64;

	for row in 0..row_samples {
		let local_y = evenly_spaced_sample(0, band_rows, row, row_samples);
		let previous_y =
			previous_start_y.saturating_add(local_y).min(previous.height().saturating_sub(1));
		let next_y = next_start_y.saturating_add(local_y).min(next.height().saturating_sub(1));

		for column in 0..column_samples {
			let x = evenly_spaced_sample(x_start, x_end, column, column_samples);
			let previous_pixel = previous.get_pixel(x, previous_y).0;
			let next_pixel = next.get_pixel(x, next_y).0;

			total_abs_diff = total_abs_diff
				.saturating_add(u64::from(previous_pixel[0].abs_diff(next_pixel[0])))
				.saturating_add(u64::from(previous_pixel[1].abs_diff(next_pixel[1])))
				.saturating_add(u64::from(previous_pixel[2].abs_diff(next_pixel[2])));
			comparisons = comparisons.saturating_add(3);
		}
	}

	if comparisons == 0 {
		return u32::MAX;
	}

	((total_abs_diff.saturating_mul(100)) / comparisons) as u32
}

fn overlap_global_informative_span(left: &RgbaImage, right: &RgbaImage) -> Option<InformativeSpan> {
	let left_span = informative_column_span(left, 0, left.height());
	let right_span = informative_column_span(right, 0, right.height());
	let width = left.width().min(right.width());

	match (left_span, right_span) {
		(Some(left_span), Some(right_span)) => {
			let start_x = left_span.start_x.min(right_span.start_x);
			let end_exclusive_x =
				left_span.end_exclusive_x.max(right_span.end_exclusive_x).min(width);

			(end_exclusive_x > start_x).then_some(InformativeSpan { start_x, end_exclusive_x })
		},
		(Some(span), None) | (None, Some(span)) => {
			let end_exclusive_x = span.end_exclusive_x.min(width).max(span.start_x + 1);

			(end_exclusive_x > span.start_x)
				.then_some(InformativeSpan { start_x: span.start_x, end_exclusive_x })
		},
		(None, None) => None,
	}
}

fn informative_column_span(image: &RgbaImage, start_y: u32, rows: u32) -> Option<InformativeSpan> {
	if image.width() == 0 || image.height() == 0 || rows == 0 {
		return None;
	}

	let clamped_rows = rows.min(image.height().saturating_sub(start_y)).max(1);
	let row_samples = clamped_rows.min(INFORMATIVE_SPAN_ROW_SAMPLES.max(2)).max(2);
	let mut scores = vec![0_u32; image.width() as usize];
	let mut max_score = 0_u32;

	for row in 0..row_samples.saturating_sub(1) {
		let local_y = evenly_spaced_sample(0, clamped_rows, row, row_samples);
		let next_local_y = (local_y.saturating_add(1)).min(clamped_rows.saturating_sub(1));
		let y = start_y.saturating_add(local_y).min(image.height().saturating_sub(1));
		let next_y = start_y.saturating_add(next_local_y).min(image.height().saturating_sub(1));

		for x in 0..image.width() {
			let pixel = image.get_pixel(x, y).0;
			let next_pixel = image.get_pixel(x, next_y).0;
			let score = u32::from(pixel[0].abs_diff(next_pixel[0]))
				.saturating_add(u32::from(pixel[1].abs_diff(next_pixel[1])))
				.saturating_add(u32::from(pixel[2].abs_diff(next_pixel[2])));
			let slot = &mut scores[x as usize];

			*slot = slot.saturating_add(score);
			max_score = max_score.max(*slot);
		}
	}

	if max_score == 0 {
		return None;
	}

	let threshold = (max_score / 6).max(INFORMATIVE_SPAN_SCORE_FLOOR_X100);
	let mut start_x = None;
	let mut end_x = None;

	for (x, score) in scores.iter().enumerate() {
		if *score >= threshold {
			start_x.get_or_insert(x as u32);

			end_x = Some((x as u32).saturating_add(1));
		}
	}

	let start_x = start_x?;
	let end_exclusive_x = end_x?;
	let padding = INFORMATIVE_SPAN_HORIZONTAL_PADDING_PX.min(image.width() / 8);
	let start_x = start_x.saturating_sub(padding);
	let end_exclusive_x =
		end_exclusive_x.saturating_add(padding).min(image.width()).max(start_x.saturating_add(1));

	Some(InformativeSpan { start_x, end_exclusive_x })
}

fn evenly_spaced_sample(start: u32, end_exclusive: u32, index: u32, count: u32) -> u32 {
	let span = end_exclusive.saturating_sub(start).max(1);

	if count <= 1 {
		return start.min(end_exclusive.saturating_sub(1));
	}

	let numerator =
		(u64::from(index) * u64::from(span.saturating_sub(1))) / u64::from(count.saturating_sub(1));

	start.saturating_add(numerator as u32).min(end_exclusive.saturating_sub(1))
}

#[cfg(test)]
mod tests {
	use image::Rgba;

	use crate::scroll_capture::{
		OverlapSearchConfig, ScrollDirection, ScrollFrameFingerprint, ScrollObserveOutcome,
		ScrollSession, detect_vertical_overlap,
	};

	fn make_test_image(width: u32, rows: &[[u8; 4]]) -> image::RgbaImage {
		let mut image = image::RgbaImage::new(width, rows.len() as u32);

		for (y, row) in rows.iter().enumerate() {
			for x in 0..width {
				image.put_pixel(x, y as u32, Rgba(*row));
			}
		}

		image
	}

	fn make_window(
		document: &[[u8; 4]],
		width: u32,
		start_row: usize,
		window_rows: usize,
	) -> image::RgbaImage {
		make_test_image(width, &document[start_row..start_row + window_rows])
	}

	fn make_sparse_textlike_window(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
		let stripe_x = 104_u32;
		let mut image = image::RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));

		for y in 0..height {
			let document_row = start_row.saturating_add(y);
			let shade = ((document_row.saturating_mul(17)) % 180) as u8;

			for x in stripe_x..stripe_x.saturating_add(6) {
				image.put_pixel(x, y, Rgba([shade, shade, shade, 255]));
			}
			for x in stripe_x.saturating_add(10)..stripe_x.saturating_add(13) {
				if document_row % 19 < 9 {
					image.put_pixel(x, y, Rgba([40, 40, 40, 255]));
				}
			}
		}

		image
	}

	#[test]
	fn overlap_detection_prefers_largest_matching_suffix() {
		let previous = make_test_image(
			5,
			&[
				[10, 0, 0, 255],
				[20, 0, 0, 255],
				[30, 0, 0, 255],
				[40, 0, 0, 255],
				[50, 0, 0, 255],
				[60, 0, 0, 255],
			],
		);
		let next = make_test_image(
			5,
			&[[40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255], [70, 0, 0, 255], [80, 0, 0, 255]],
		);
		let overlap = detect_vertical_overlap(
			&previous,
			&next,
			OverlapSearchConfig { min_overlap_rows: 1, ..Default::default() },
		);

		assert!(overlap.matched);
		assert_eq!(overlap.rows, 3);
	}

	#[test]
	fn fingerprint_wrapper_returns_zero_delta_for_identical_images() {
		let image = image::RgbaImage::from_pixel(12, 12, Rgba([9, 8, 7, 255]));
		let left = crate::scroll_capture::scroll_capture_fingerprint(&image);
		let right = crate::scroll_capture::scroll_capture_fingerprint(&image);

		assert_eq!(crate::scroll_capture::scroll_capture_fingerprint_delta(&left, &right), 0);
	}

	#[test]
	fn fingerprint_struct_distance_detects_changed_image() {
		let base = image::RgbaImage::from_pixel(12, 12, Rgba([9, 8, 7, 255]));
		let changed = image::RgbaImage::from_pixel(12, 12, Rgba([30, 8, 7, 255]));
		let left = ScrollFrameFingerprint::from_image(&base);
		let right = ScrollFrameFingerprint::from_image(&changed);

		assert!(left.distance(&right) > 0);
	}

	#[test]
	fn session_commits_downward_growth_on_first_matching_sample() {
		let base = make_test_image(
			3,
			&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
		);
		let moved = make_test_image(
			3,
			&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();
		let outcome = session.observe_downward_sample(moved).unwrap();

		assert_eq!(
			outcome,
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), 6);
		assert_eq!(session.export_image().get_pixel(0, 5), &Rgba([60, 0, 0, 255]));
	}

	#[test]
	fn session_supports_multiple_downward_growth_steps() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), 7);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 6), &Rgba([70, 0, 0, 255]));
	}

	#[test]
	fn downward_hot_path_falls_back_when_scroll_step_grows() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
			[90, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 4, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 3 }
		);
		assert_eq!(session.export_image().height(), 9);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 8), &Rgba([90, 0, 0, 255]));
	}

	#[test]
	fn session_reports_upward_motion_without_growing() {
		let base = make_test_image(
			3,
			&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
		);
		let moved = make_test_image(
			3,
			&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();
		let outcome = session.observe_downward_sample(moved).unwrap();

		assert!(matches!(
			outcome,
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image(), &base);
	}

	#[test]
	fn pure_upward_sequence_never_commits_growth() {
		let document = [
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
		let mut session = ScrollSession::new(make_window(&document, 3, 5, 5), 320).unwrap();
		let initial_height = session.export_image().height();

		for start_row in (0..5).rev() {
			assert!(matches!(
				session.observe_downward_sample(make_window(&document, 3, start_row, 5)).unwrap(),
				ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
					| ScrollObserveOutcome::PreviewUpdated
			));
			assert_eq!(session.export_image().height(), initial_height);
		}
	}

	#[test]
	fn low_information_motion_does_not_commit_growth() {
		let base = make_test_image(
			3,
			&[[10, 0, 0, 255], [10, 0, 0, 255], [11, 0, 0, 255], [11, 0, 0, 255], [12, 0, 0, 255]],
		);
		let moved = make_test_image(
			3,
			&[[10, 0, 0, 255], [11, 0, 0, 255], [11, 0, 0, 255], [12, 0, 0, 255], [12, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();
		let outcome = session.observe_downward_sample(moved).unwrap();

		assert!(matches!(
			outcome,
			ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
				| ScrollObserveOutcome::UnsupportedDirection { .. }
		));
		assert_eq!(session.export_image(), &base);
	}

	#[test]
	fn session_commits_growth_with_sparse_informative_columns() {
		let base = make_sparse_textlike_window(256, 120, 0);
		let moved = make_sparse_textlike_window(256, 120, 9);
		let mut session = ScrollSession::new(base, 320).unwrap();
		let outcome = session.observe_downward_sample(moved).unwrap();

		assert_eq!(
			outcome,
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 9 }
		);
		assert_eq!(session.export_image().height(), 129);
	}

	#[test]
	fn sparse_textlike_small_downward_steps_eventually_append() {
		let base = make_sparse_textlike_window(256, 120, 0);
		let mut session = ScrollSession::new(base, 320).unwrap();
		let initial_height = session.export_image().height();
		let mut committed = 0_u32;

		for start_row in 1..=8 {
			if matches!(
				session
					.observe_downward_sample(make_sparse_textlike_window(256, 120, start_row))
					.unwrap(),
				ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, .. }
			) {
				committed = committed.saturating_add(1);
			}
		}

		assert!(committed > 0);
		assert!(session.export_image().height() > initial_height);
	}

	#[test]
	fn upward_motion_does_not_reset_downward_progress() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 0, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), 8);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
	}

	#[test]
	fn upward_rewind_blocks_partial_downward_recovery_until_baseline() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 0, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		));

		let height_after_upward_rewind = session.export_image().height();

		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_after_upward_rewind);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_after_upward_rewind);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
	}

	#[test]
	fn returning_below_last_committed_viewport_does_not_duplicate_growth() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);

		let height_before_resume = session.export_image().height();

		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		));
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_before_resume);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), 8);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
	}

	#[test]
	fn alternating_down_up_down_only_grows_once_per_new_band() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
			[80, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 0, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		));

		let height_after_first_append = session.export_image().height();

		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
			ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(session.export_image().height(), height_after_first_append);
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, 0, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
		));
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(
			session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert_eq!(session.export_image().height(), 8);
		assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
		assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
	}

	#[test]
	fn session_preview_matches_export_after_downward_growth() {
		let document = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
		];
		let mut session = ScrollSession::new(make_window(&document, 3, 0, 4), 3).unwrap();
		let _ = session.observe_downward_sample(make_window(&document, 3, 1, 4)).unwrap();
		let _ = session.observe_downward_sample(make_window(&document, 3, 2, 4)).unwrap();

		assert_eq!(session.preview_image().height(), session.export_image().height());
		assert_eq!(session.preview_image().get_pixel(0, 0), session.export_image().get_pixel(0, 0));
		assert_eq!(
			session.preview_image().get_pixel(0, session.preview_image().height() - 1),
			session.export_image().get_pixel(0, session.export_image().height() - 1)
		);
	}

	#[test]
	fn session_undo_restores_previous_stitched_image() {
		let base = make_test_image(
			3,
			&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
		);
		let moved = make_test_image(
			3,
			&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone(), 320).unwrap();

		assert_eq!(
			session.observe_downward_sample(moved).unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
		);
		assert!(session.undo_last_append());
		assert_eq!(session.export_image(), &base);
	}
}
