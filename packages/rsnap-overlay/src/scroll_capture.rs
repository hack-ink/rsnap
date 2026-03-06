use color_eyre::eyre::{Result, eyre};
use image::RgbaImage;

const FINGERPRINT_GRID_COLUMNS: u32 = 12;
const FINGERPRINT_GRID_ROWS: u32 = 16;

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
		let margin_x = ((width as f32) * 0.05).round() as u32;
		let margin_y = ((height as f32) * 0.05).round() as u32;
		let left = margin_x.min(width.saturating_sub(1));
		let right = width.saturating_sub(margin_x).max(left + 1);
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

const SESSION_FINGERPRINT_DELTA_MAX: u32 = 10;
const SESSION_STABLE_SAMPLE_COUNT: u8 = 2;
const SESSION_FALLBACK_APPEND_MIN_ROWS: u32 = 8;
const SESSION_FALLBACK_APPEND_MAX_ROWS: u32 = 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum ScrollObserveOutcome {
	NoChange,
	PreviewUpdated,
	Appended { appended_rows: u32 },
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ScrollSession {
	stitched_image: RgbaImage,
	preview_image: RgbaImage,
	segments: Vec<RgbaImage>,
	committed_frames: Vec<RgbaImage>,
	last_sample_fingerprint: Option<Vec<u8>>,
	last_committed_fingerprint: Vec<u8>,
	stable_sample_count: u8,
	seen_motion_since_commit: bool,
}
impl ScrollSession {
	#[allow(dead_code)]
	pub(crate) fn new(base_frame: RgbaImage) -> Result<Self> {
		let fingerprint = scroll_capture_fingerprint(&base_frame);

		Ok(Self {
			stitched_image: base_frame.clone(),
			preview_image: base_frame.clone(),
			segments: vec![base_frame.clone()],
			committed_frames: vec![base_frame],
			last_sample_fingerprint: None,
			last_committed_fingerprint: fingerprint,
			stable_sample_count: 0,
			seen_motion_since_commit: false,
		})
	}

	#[allow(dead_code)]
	pub(crate) fn observe_sample(&mut self, frame: RgbaImage) -> Result<ScrollObserveOutcome> {
		let frame_size = frame.dimensions();
		let fingerprint = scroll_capture_fingerprint(&frame);
		let sample_delta = self
			.last_sample_fingerprint
			.as_ref()
			.map(|previous| scroll_capture_fingerprint_delta(previous, &fingerprint));
		let moved_since_previous = sample_delta.is_some_and(|delta| delta > 0);

		if sample_delta.is_some_and(|delta| delta <= SESSION_FINGERPRINT_DELTA_MAX) {
			self.stable_sample_count = self.stable_sample_count.saturating_add(1);
		} else {
			self.stable_sample_count = 0;
		}

		let moved_since_commit =
			scroll_capture_fingerprint_delta(&self.last_committed_fingerprint, &fingerprint) > 0;

		if moved_since_previous || moved_since_commit {
			self.seen_motion_since_commit = true;
		}

		let preview_changed = self.preview_image.as_raw() != frame.as_raw();

		self.preview_image = frame.clone();
		self.last_sample_fingerprint = Some(fingerprint.clone());

		tracing::debug!(
			op = "scroll_session.sample",
			frame_px = ?frame_size,
			preview_changed,
			sample_delta,
			moved_since_previous,
			moved_since_commit,
			stable_sample_count = self.stable_sample_count,
			seen_motion_since_commit = self.seen_motion_since_commit,
			"Evaluated scroll sample."
		);

		if self.stable_sample_count < SESSION_STABLE_SAMPLE_COUNT || !self.seen_motion_since_commit
		{
			return Ok(if preview_changed {
				ScrollObserveOutcome::PreviewUpdated
			} else {
				ScrollObserveOutcome::NoChange
			});
		}

		let Some(previous_frame) = self.committed_frames.last() else {
			return Ok(if preview_changed {
				ScrollObserveOutcome::PreviewUpdated
			} else {
				ScrollObserveOutcome::NoChange
			});
		};
		let overlap_rows = scroll_capture_estimate_overlap(previous_frame, &frame);
		let frame_differs_from_commit = self.last_committed_fingerprint != fingerprint;
		let fallback_append_rows = (frame.height() / 20)
			.clamp(SESSION_FALLBACK_APPEND_MIN_ROWS, SESSION_FALLBACK_APPEND_MAX_ROWS)
			.min(frame.height().saturating_sub(1));
		let Some(strip) = scroll_capture_strip_from_overlap(&frame, overlap_rows).or_else(|| {
			if (moved_since_previous || moved_since_commit)
				&& frame_differs_from_commit
				&& fallback_append_rows > 0
			{
				tracing::debug!(
					op = "scroll_session.sample_fallback_append",
					frame_px = ?frame_size,
					overlap_rows,
					fallback_append_rows,
					"Using fallback append rows because overlap produced no strip."
				);

				scroll_capture_strip_from_overlap(
					&frame,
					frame.height().saturating_sub(fallback_append_rows),
				)
			} else {
				None
			}
		}) else {
			tracing::debug!(
				op = "scroll_session.sample_no_append",
				frame_px = ?frame_size,
				overlap_rows,
				preview_changed,
				moved_since_previous,
				moved_since_commit,
				stable_sample_count = self.stable_sample_count,
				"Scroll sample produced no append."
			);

			return Ok(if preview_changed {
				ScrollObserveOutcome::PreviewUpdated
			} else {
				ScrollObserveOutcome::NoChange
			});
		};
		let appended_rows = strip.height();
		let stitched = scroll_capture_append_image(&self.stitched_image, &strip)
			.map_err(|err| eyre!("{err:#}"))?;

		self.stitched_image = stitched.clone();
		self.preview_image = stitched;
		self.segments.push(strip);
		self.committed_frames.push(frame);
		self.last_committed_fingerprint = fingerprint;
		self.stable_sample_count = 0;
		self.seen_motion_since_commit = false;

		tracing::info!(
			op = "scroll_session.appended",
			frame_px = ?frame_size,
			overlap_rows,
			fallback_append_rows,
			appended_rows,
			stitched_px = ?self.stitched_image.dimensions(),
			"Scroll session appended a new strip."
		);

		Ok(ScrollObserveOutcome::Appended { appended_rows })
	}

	#[allow(dead_code)]
	pub(crate) fn preview_image(&self) -> &RgbaImage {
		&self.preview_image
	}

	#[allow(dead_code)]
	pub(crate) fn export_image(&self) -> &RgbaImage {
		&self.stitched_image
	}

	#[allow(dead_code)]
	pub(crate) fn undo_last_append(&mut self) -> bool {
		if self.segments.len() <= 1 || self.committed_frames.len() <= 1 {
			return false;
		}

		let _ = self.segments.pop();
		let _ = self.committed_frames.pop();
		let Some(stitched) = scroll_capture_rebuild_stitched_image(&self.segments) else {
			return false;
		};
		let Some(last_frame) = self.committed_frames.last() else {
			return false;
		};

		self.stitched_image = stitched.clone();
		self.preview_image = stitched;
		self.last_committed_fingerprint = scroll_capture_fingerprint(last_frame);
		self.last_sample_fingerprint = None;
		self.stable_sample_count = 0;
		self.seen_motion_since_commit = false;

		true
	}
}

#[must_use]
pub(crate) fn detect_vertical_overlap(
	previous: &RgbaImage,
	next: &RgbaImage,
	config: OverlapSearchConfig,
) -> OverlapMatch {
	if previous.width() == 0 || next.width() == 0 || previous.height() == 0 || next.height() == 0 {
		return OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };
	}

	let max_overlap = previous.height().min(next.height());

	if max_overlap < config.min_overlap_rows.max(1) {
		return OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };
	}

	let mut best = OverlapMatch { rows: 0, matched: false, mean_abs_diff_x100: u32::MAX };

	for overlap_rows in (config.min_overlap_rows.max(1)..=max_overlap).rev() {
		let diff = overlap_mean_abs_diff_x100(previous, next, overlap_rows, config);

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

#[must_use]
pub(crate) fn scroll_capture_estimate_overlap(previous: &RgbaImage, next: &RgbaImage) -> u32 {
	let max_overlap = previous.height().min(next.height());
	let mut config = OverlapSearchConfig::default();

	if max_overlap <= config.min_overlap_rows {
		config.min_overlap_rows = 1;
	}

	let overlap = detect_vertical_overlap(previous, next, config);

	if overlap.matched { overlap.rows } else { 0 }
}

#[must_use]
pub(crate) fn scroll_capture_strip_from_overlap(
	frame: &RgbaImage,
	overlap_rows: u32,
) -> Option<RgbaImage> {
	let overlap_rows = overlap_rows.min(frame.height());
	let appended_rows = frame.height().saturating_sub(overlap_rows);

	if appended_rows == 0 {
		return None;
	}

	Some(image::imageops::crop_imm(frame, 0, overlap_rows, frame.width(), appended_rows).to_image())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StitchAppendSummary {
	pub(crate) overlap_rows: u32,
	pub(crate) appended_rows: u32,
	pub(crate) total_height: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct ScrollStitcher {
	width: u32,
	height: u32,
	pixels: Vec<u8>,
	appended_row_counts: Vec<u32>,
}
impl ScrollStitcher {
	pub(crate) fn new(base_frame: &RgbaImage) -> Result<Self> {
		let pixels = base_frame.as_raw().clone();

		Ok(Self {
			width: base_frame.width(),
			height: base_frame.height(),
			pixels,
			appended_row_counts: Vec::new(),
		})
	}

	pub(crate) fn append_frame(
		&mut self,
		frame: &RgbaImage,
		overlap_rows: u32,
	) -> Result<StitchAppendSummary> {
		if frame.width() != self.width {
			return Err(eyre!(
				"frame width mismatch: expected {} got {}",
				self.width,
				frame.width()
			));
		}

		let overlap_rows = overlap_rows.min(frame.height());
		let appended_rows = frame.height().saturating_sub(overlap_rows);

		if appended_rows == 0 {
			return Ok(StitchAppendSummary {
				overlap_rows,
				appended_rows,
				total_height: self.height,
			});
		}

		let row_bytes = frame_row_bytes(frame.width())?;
		let start = usize::try_from(overlap_rows)
			.ok()
			.and_then(|row| row.checked_mul(row_bytes))
			.ok_or_else(|| eyre!("overlap row offset overflow"))?;

		self.pixels.extend_from_slice(&frame.as_raw()[start..]);
		self.height = self.height.saturating_add(appended_rows);
		self.appended_row_counts.push(appended_rows);

		Ok(StitchAppendSummary { overlap_rows, appended_rows, total_height: self.height })
	}

	#[must_use]
	#[cfg(test)]
	pub(crate) fn undo_last_append(&mut self) -> bool {
		let Some(appended_rows) = self.appended_row_counts.pop() else {
			return false;
		};
		let Ok(row_bytes) = frame_row_bytes(self.width) else {
			return false;
		};
		let Some(bytes_to_remove) =
			usize::try_from(appended_rows).ok().and_then(|rows| rows.checked_mul(row_bytes))
		else {
			return false;
		};

		if bytes_to_remove > self.pixels.len() {
			return false;
		}

		let new_len = self.pixels.len().saturating_sub(bytes_to_remove);

		self.pixels.truncate(new_len);
		self.height = self.height.saturating_sub(appended_rows);

		true
	}

	#[must_use]
	#[cfg(test)]
	pub(crate) fn height(&self) -> u32 {
		self.height
	}

	pub(crate) fn snapshot_image(&self) -> Result<RgbaImage> {
		RgbaImage::from_raw(self.width, self.height, self.pixels.clone())
			.ok_or_else(|| eyre!("failed to materialize stitched image"))
	}
}

pub(crate) fn scroll_capture_append_image(
	base: &RgbaImage,
	strip: &RgbaImage,
) -> Result<RgbaImage> {
	let mut stitcher = ScrollStitcher::new(base)?;

	let _ = stitcher.append_frame(strip, 0)?;

	stitcher.snapshot_image()
}

pub(crate) fn scroll_capture_rebuild_stitched_image(segments: &[RgbaImage]) -> Option<RgbaImage> {
	let mut stitched = segments.first()?.clone();

	for strip in &segments[1..] {
		let Ok(appended) = scroll_capture_append_image(&stitched, strip) else {
			return None;
		};

		stitched = appended;
	}

	Some(stitched)
}

fn overlap_mean_abs_diff_x100(
	previous: &RgbaImage,
	next: &RgbaImage,
	overlap_rows: u32,
	config: OverlapSearchConfig,
) -> u32 {
	let width = previous.width().min(next.width());
	let column_samples = width.min(config.max_column_samples).max(1);
	let row_samples = overlap_rows.min(config.max_row_samples).max(1);
	let previous_start_y = previous.height().saturating_sub(overlap_rows);
	let mut total_abs_diff = 0_u64;
	let mut comparisons = 0_u64;

	for row in 0..row_samples {
		let local_y = evenly_spaced_sample(0, overlap_rows, row, row_samples);
		let previous_y = previous_start_y.saturating_add(local_y);
		let next_y = local_y.min(next.height().saturating_sub(1));

		for column in 0..column_samples {
			let x = evenly_spaced_sample(0, width, column, column_samples);
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

fn evenly_spaced_sample(start: u32, end_exclusive: u32, index: u32, count: u32) -> u32 {
	let span = end_exclusive.saturating_sub(start).max(1);

	if count <= 1 {
		return start.min(end_exclusive.saturating_sub(1));
	}

	let numerator =
		(u64::from(index) * u64::from(span.saturating_sub(1))) / u64::from(count.saturating_sub(1));

	start.saturating_add(numerator as u32).min(end_exclusive.saturating_sub(1))
}

fn frame_row_bytes(width: u32) -> Result<usize> {
	let width = usize::try_from(width).map_err(|_| eyre!("image width does not fit usize"))?;

	width.checked_mul(4).ok_or_else(|| eyre!("image row byte count overflow"))
}

#[cfg(test)]
mod tests {
	use image::Rgba;

	use crate::scroll_capture::{
		OverlapSearchConfig, ScrollFrameFingerprint, ScrollObserveOutcome, ScrollSession,
		ScrollStitcher, detect_vertical_overlap, scroll_capture_append_image,
		scroll_capture_estimate_overlap, scroll_capture_fingerprint,
		scroll_capture_fingerprint_delta, scroll_capture_rebuild_stitched_image,
		scroll_capture_strip_from_overlap,
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
	fn stitcher_appends_without_duplicate_overlap_and_supports_undo() {
		let base = make_test_image(3, &[[1, 0, 0, 255], [2, 0, 0, 255], [3, 0, 0, 255]]);
		let next = make_test_image(3, &[[3, 0, 0, 255], [4, 0, 0, 255], [5, 0, 0, 255]]);
		let mut stitcher = ScrollStitcher::new(&base).unwrap();
		let summary = stitcher.append_frame(&next, 1).unwrap();
		let stitched = stitcher.snapshot_image().unwrap();

		assert_eq!(summary.appended_rows, 2);
		assert_eq!(stitched.height(), 5);
		assert_eq!(stitched.get_pixel(0, 0), &Rgba([1, 0, 0, 255]));
		assert_eq!(stitched.get_pixel(0, 3), &Rgba([4, 0, 0, 255]));
		assert_eq!(stitched.get_pixel(0, 4), &Rgba([5, 0, 0, 255]));
		assert!(stitcher.undo_last_append());
		assert_eq!(stitcher.height(), 3);
	}

	#[test]
	fn fingerprint_wrapper_returns_zero_delta_for_identical_images() {
		let image = image::RgbaImage::from_pixel(12, 12, Rgba([9, 8, 7, 255]));
		let left = scroll_capture_fingerprint(&image);
		let right = scroll_capture_fingerprint(&image);

		assert_eq!(scroll_capture_fingerprint_delta(&left, &right), 0);
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
	fn strip_and_append_helpers_preserve_vertical_order() {
		let base = make_test_image(2, &[[1, 0, 0, 255], [2, 0, 0, 255]]);
		let next = make_test_image(2, &[[2, 0, 0, 255], [3, 0, 0, 255], [4, 0, 0, 255]]);
		let overlap = scroll_capture_estimate_overlap(&base, &next);
		let strip = scroll_capture_strip_from_overlap(&next, overlap).unwrap();
		let stitched = scroll_capture_append_image(&base, &strip).unwrap();

		assert_eq!(overlap, 1);
		assert_eq!(strip.height(), 2);
		assert_eq!(stitched.height(), 4);
		assert_eq!(stitched.get_pixel(0, 2), &Rgba([3, 0, 0, 255]));
		assert_eq!(stitched.get_pixel(0, 3), &Rgba([4, 0, 0, 255]));
	}

	#[test]
	fn rebuild_helper_reassembles_segment_stack() {
		let first = make_test_image(2, &[[1, 0, 0, 255], [2, 0, 0, 255]]);
		let second = make_test_image(2, &[[3, 0, 0, 255]]);
		let third = make_test_image(2, &[[4, 0, 0, 255]]);
		let rebuilt = scroll_capture_rebuild_stitched_image(&[first, second, third]).unwrap();

		assert_eq!(rebuilt.height(), 4);
		assert_eq!(rebuilt.get_pixel(0, 0), &Rgba([1, 0, 0, 255]));
		assert_eq!(rebuilt.get_pixel(0, 3), &Rgba([4, 0, 0, 255]));
	}

	#[test]
	fn session_stays_noop_for_identical_samples() {
		let base = make_test_image(2, &[[1, 0, 0, 255], [2, 0, 0, 255], [3, 0, 0, 255]]);
		let mut session = ScrollSession::new(base.clone()).unwrap();

		let outcome = session.observe_sample(base.clone()).unwrap();

		assert_eq!(outcome, ScrollObserveOutcome::NoChange);
		assert_eq!(session.export_image(), &base);

		let outcome = session.observe_sample(base.clone()).unwrap();

		assert_eq!(outcome, ScrollObserveOutcome::NoChange);
		assert_eq!(session.export_image(), &base);
	}

	#[test]
	fn session_preview_updates_before_append_commits() {
		let base = make_test_image(
			3,
			&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255]],
		);
		let next = make_test_image(
			3,
			&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone()).unwrap();

		let outcome = session.observe_sample(next.clone()).unwrap();

		assert_eq!(outcome, ScrollObserveOutcome::PreviewUpdated);
		assert_eq!(session.preview_image(), &next);
		assert_eq!(session.export_image(), &base);
	}

	#[test]
	fn session_appends_after_moved_then_stable_samples() {
		let base = make_test_image(
			3,
			&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
		);
		let moved = make_test_image(
			3,
			&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
		);
		let mut session = ScrollSession::new(base.clone()).unwrap();

		assert_eq!(
			session.observe_sample(moved.clone()).unwrap(),
			ScrollObserveOutcome::PreviewUpdated
		);
		assert_eq!(session.observe_sample(moved.clone()).unwrap(), ScrollObserveOutcome::NoChange);

		let outcome = session.observe_sample(moved).unwrap();
		let ScrollObserveOutcome::Appended { appended_rows } = outcome else {
			panic!("expected append outcome");
		};

		assert_eq!(appended_rows, 1);
		assert_eq!(session.export_image().height(), 6);
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
		let mut session = ScrollSession::new(base.clone()).unwrap();

		let _ = session.observe_sample(moved.clone()).unwrap();
		let _ = session.observe_sample(moved.clone()).unwrap();
		let outcome = session.observe_sample(moved).unwrap();

		assert!(matches!(outcome, ScrollObserveOutcome::Appended { .. }));

		assert!(session.undo_last_append());
		assert_eq!(session.export_image(), &base);
	}
}
