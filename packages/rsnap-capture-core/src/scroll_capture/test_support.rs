use image::Rgba;

pub(crate) fn make_test_image(width: u32, rows: &[[u8; 4]]) -> image::RgbaImage {
	let mut image = image::RgbaImage::new(width, rows.len() as u32);

	for (y, row) in rows.iter().enumerate() {
		for x in 0..width {
			image.put_pixel(x, y as u32, Rgba(*row));
		}
	}

	image
}

pub(crate) fn make_window(
	document: &[[u8; 4]],
	width: u32,
	start_row: usize,
	window_rows: usize,
) -> image::RgbaImage {
	make_test_image(width, &document[start_row..start_row + window_rows])
}

pub(crate) fn make_sparse_textlike_window(
	width: u32,
	height: u32,
	start_row: u32,
) -> image::RgbaImage {
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

pub(crate) fn make_sparse_textlike_window_with_moving_edge_scrollbar(
	width: u32,
	height: u32,
	start_row: u32,
	thumb_top: u32,
) -> image::RgbaImage {
	let track_left = width.saturating_sub(18);
	let thumb_height = (height / 4).max(12).min(height.max(1));
	let thumb_top = thumb_top.min(height.saturating_sub(thumb_height));
	let thumb_right = width.saturating_sub(3).max(track_left.saturating_add(4));
	let mut image = make_sparse_textlike_window(width, height, start_row);

	for y in 0..height {
		for x in track_left..width {
			image.put_pixel(x, y, Rgba([224, 224, 224, 255]));
		}
	}
	for y in thumb_top..thumb_top.saturating_add(thumb_height) {
		for x in track_left.saturating_add(3)..thumb_right {
			image.put_pixel(x, y, Rgba([28, 28, 28, 255]));
		}
	}

	image
}

pub(crate) fn make_browser_like_window(
	width: u32,
	height: u32,
	start_row: u32,
) -> image::RgbaImage {
	let scrollbar_left = width.saturating_sub(18);
	let content_left = 56_u32;
	let content_right = width.saturating_sub(48);
	let heading_width = 220_u32;
	let paragraph_width = content_right.saturating_sub(content_left);
	let mut image = make_sparse_textlike_window(width, height, start_row);

	for y in 0..height {
		let document_row = start_row.saturating_add(y);

		if document_row % 420 < 18 {
			for x in content_left..content_left.saturating_add(heading_width) {
				image.put_pixel(x, y, Rgba([26, 26, 26, 255]));
			}
		} else if document_row % 420 >= 54 && document_row % 420 < 220 {
			if document_row % 24 < 3 {
				let trim = ((document_row / 24) % 5) * 18;

				for x in
					content_left..content_left.saturating_add(paragraph_width.saturating_sub(trim))
				{
					image.put_pixel(x, y, Rgba([72, 72, 72, 255]));
				}
			}
		} else if document_row % 420 >= 270 && document_row % 420 < 360 && document_row % 20 < 2 {
			for x in content_left.saturating_add(20)
				..content_left.saturating_add(paragraph_width.saturating_sub(70))
			{
				image.put_pixel(x, y, Rgba([98, 98, 98, 255]));
			}
		}

		for x in scrollbar_left..width {
			image.put_pixel(x, y, Rgba([232, 232, 232, 255]));
		}
	}

	let thumb_height = (height / 5).max(16);
	let thumb_top = (start_row / 3) % height.max(thumb_height + 1);
	let thumb_top = thumb_top.min(height.saturating_sub(thumb_height));

	for y in thumb_top..thumb_top.saturating_add(thumb_height) {
		for x in scrollbar_left.saturating_add(3)..width.saturating_sub(4) {
			image.put_pixel(x, y, Rgba([96, 96, 96, 255]));
		}
	}

	image
}
