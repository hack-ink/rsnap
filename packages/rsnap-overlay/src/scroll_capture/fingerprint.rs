use image::RgbaImage;

use crate::scroll_capture::support;

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
		let informative_span = support::informative_column_span(image, 0, height);
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
			let y = support::evenly_spaced_sample(top, bottom, row, FINGERPRINT_GRID_ROWS);

			for column in 0..FINGERPRINT_GRID_COLUMNS {
				let x =
					support::evenly_spaced_sample(left, right, column, FINGERPRINT_GRID_COLUMNS);
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
