use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily, Pos2};
use font8x8::{BASIC_FONTS, UnicodeFonts};
use fontdue::{
	Font, FontSettings,
	layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle},
};
use image::{Rgba, RgbaImage};

use crate::system_fonts;

const BITMAP_GLYPH_SIDE_PX: u32 = 8;
const BITMAP_GLYPH_ADVANCE_PX: u32 = 8;
const BITMAP_LINE_GAP_PX: u32 = 2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterTextAnnotation<'a> {
	pub(crate) anchor_px: Pos2,
	pub(crate) font_size_px: f32,
	pub(crate) fill_rgba: [u8; 4],
	pub(crate) text: &'a str,
}

#[derive(Debug)]
struct ExportTextFont {
	font_data: Arc<FontData>,
	fontdue_font: OnceLock<Option<Font>>,
}
impl ExportTextFont {
	fn new(font_data: Arc<FontData>) -> Self {
		Self { font_data, fontdue_font: OnceLock::new() }
	}

	fn font(&self) -> Option<&Font> {
		self.fontdue_font
			.get_or_init(|| {
				Font::from_bytes(
					self.font_data.as_ref().as_ref(),
					FontSettings {
						collection_index: self.font_data.index,
						..FontSettings::default()
					},
				)
				.ok()
			})
			.as_ref()
	}

	fn supports_char(&self, ch: char) -> bool {
		self.font().is_some_and(|font| font.has_glyph(ch))
	}
}

#[derive(Clone, Copy, Debug)]
struct TextFontRun<'a> {
	font_index: usize,
	text: &'a str,
}

pub(crate) fn render_text_annotations(
	image: &mut RgbaImage,
	annotations: &[RasterTextAnnotation<'_>],
) {
	let fonts = export_text_fonts();

	for annotation in annotations {
		if annotation.text.trim().is_empty() {
			continue;
		}

		if let Some(runs) = build_text_font_runs(fonts, annotation.text)
			&& render_with_font_stack(image, *annotation, fonts, &runs)
		{
			continue;
		}

		render_with_bitmap_fallback(image, *annotation);
	}
}

fn export_text_fonts() -> &'static [ExportTextFont] {
	static FONTS: OnceLock<Vec<ExportTextFont>> = OnceLock::new();

	FONTS.get_or_init(load_export_text_fonts).as_slice()
}

fn load_export_text_fonts() -> Vec<ExportTextFont> {
	let mut fonts = FontDefinitions::default();

	system_fonts::configure_text_font_fallbacks(&mut fonts);

	fonts
		.families
		.get(&FontFamily::Proportional)
		.into_iter()
		.flat_map(|family| family.iter())
		.filter_map(|font_name| fonts.font_data.get(font_name).cloned())
		.map(ExportTextFont::new)
		.collect()
}

fn build_text_font_runs<'a>(
	fonts: &[ExportTextFont],
	text: &'a str,
) -> Option<Vec<TextFontRun<'a>>> {
	let mut runs = Vec::new();
	let mut active_font_index = None;
	let mut run_start = 0;
	let mut previous_visible_font_index = None;

	for (byte_index, ch) in text.char_indices() {
		let font_index =
			font_index_for_char(fonts, ch, active_font_index, previous_visible_font_index)?;

		if active_font_index.is_some_and(|current| current != font_index) {
			runs.push(TextFontRun {
				font_index: active_font_index?,
				text: &text[run_start..byte_index],
			});

			run_start = byte_index;
		}

		active_font_index = Some(font_index);

		if !ch.is_whitespace() && ch != '\n' {
			previous_visible_font_index = Some(font_index);
		}
	}

	if let Some(font_index) = active_font_index {
		runs.push(TextFontRun { font_index, text: &text[run_start..] });
	}

	Some(runs)
}

fn font_index_for_char(
	fonts: &[ExportTextFont],
	ch: char,
	active_font_index: Option<usize>,
	previous_visible_font_index: Option<usize>,
) -> Option<usize> {
	if ch.is_whitespace() || ch == '\n' {
		return active_font_index
			.or(previous_visible_font_index)
			.or_else(|| (!fonts.is_empty()).then_some(0));
	}

	fonts.iter().position(|font| font.supports_char(ch))
}

fn render_with_font_stack(
	image: &mut RgbaImage,
	annotation: RasterTextAnnotation<'_>,
	fonts: &[ExportTextFont],
	runs: &[TextFontRun<'_>],
) -> bool {
	let Some(parsed_fonts) = fonts.iter().map(ExportTextFont::font).collect::<Option<Vec<_>>>()
	else {
		return false;
	};
	let fill_rgba = Rgba(annotation.fill_rgba);
	let mut layout = Layout::new(CoordinateSystem::PositiveYDown);

	layout.reset(&LayoutSettings {
		x: annotation.anchor_px.x.max(0.0),
		y: annotation.anchor_px.y.max(0.0),
		..LayoutSettings::default()
	});

	for run in runs {
		layout.append(
			&parsed_fonts,
			&TextStyle::new(run.text, annotation.font_size_px.max(8.0), run.font_index),
		);
	}
	for glyph in layout.glyphs() {
		if glyph.width == 0 || glyph.height == 0 {
			continue;
		}

		let font = parsed_fonts[glyph.font_index];
		let (metrics, bitmap) = font.rasterize_config(glyph.key);

		if metrics.width == 0 || metrics.height == 0 || bitmap.is_empty() {
			continue;
		}

		let x = glyph.x.round() as i32;
		let y = glyph.y.round() as i32;

		blend_coverage_bitmap(image, x, y, metrics.width, metrics.height, &bitmap, fill_rgba);
	}

	true
}

fn render_with_bitmap_fallback(image: &mut RgbaImage, annotation: RasterTextAnnotation<'_>) {
	let fill_rgba = Rgba(annotation.fill_rgba);
	let scale =
		(annotation.font_size_px.max(8.0) / BITMAP_GLYPH_SIDE_PX as f32).round().max(1.0) as u32;
	let glyph_advance = BITMAP_GLYPH_ADVANCE_PX.saturating_mul(scale);
	let line_height = BITMAP_GLYPH_SIDE_PX
		.saturating_mul(scale)
		.saturating_add(BITMAP_LINE_GAP_PX.saturating_mul(scale));
	let origin_x = annotation.anchor_px.x.round() as i32;
	let mut cursor_x = origin_x;
	let mut cursor_y = annotation.anchor_px.y.round() as i32;

	for ch in annotation.text.chars() {
		match ch {
			'\n' => {
				cursor_x = origin_x;
				cursor_y += i32::try_from(line_height).unwrap_or(i32::MAX);
			},
			_ if ch.is_whitespace() => {
				cursor_x += i32::try_from(glyph_advance).unwrap_or(i32::MAX);
			},
			_ => {
				let Some(glyph) = BASIC_FONTS.get(ch) else {
					cursor_x += i32::try_from(glyph_advance).unwrap_or(i32::MAX);

					continue;
				};

				draw_bitmap_glyph(image, cursor_x, cursor_y, &glyph, scale, fill_rgba);

				cursor_x += i32::try_from(glyph_advance).unwrap_or(i32::MAX);
			},
		}
	}
}

fn draw_bitmap_glyph(
	image: &mut RgbaImage,
	origin_x: i32,
	origin_y: i32,
	glyph: &[u8; 8],
	scale: u32,
	color: Rgba<u8>,
) {
	for (row_index, row_bits) in glyph.iter().copied().enumerate() {
		for column_index in 0..8_usize {
			if row_bits & (1 << column_index) == 0 {
				continue;
			}

			let x = origin_x
				+ i32::try_from((7_usize.saturating_sub(column_index)) as u32 * scale)
					.unwrap_or(i32::MAX);
			let y = origin_y
				+ i32::try_from(u32::try_from(row_index).unwrap_or(0).saturating_mul(scale))
					.unwrap_or(i32::MAX);

			fill_rect(image, x, y, scale, scale, color);
		}
	}
}

fn fill_rect(image: &mut RgbaImage, x: i32, y: i32, width: u32, height: u32, color: Rgba<u8>) {
	for row in 0..height {
		for column in 0..width {
			blend_pixel(
				image,
				x.saturating_add(i32::try_from(column).unwrap_or(i32::MAX)),
				y.saturating_add(i32::try_from(row).unwrap_or(i32::MAX)),
				color,
				255,
			);
		}
	}
}

fn blend_coverage_bitmap(
	image: &mut RgbaImage,
	origin_x: i32,
	origin_y: i32,
	width: usize,
	height: usize,
	bitmap: &[u8],
	color: Rgba<u8>,
) {
	for row in 0..height {
		for column in 0..width {
			let coverage = bitmap[row.saturating_mul(width).saturating_add(column)];

			if coverage == 0 {
				continue;
			}

			blend_pixel(
				image,
				origin_x.saturating_add(i32::try_from(column).unwrap_or(i32::MAX)),
				origin_y.saturating_add(i32::try_from(row).unwrap_or(i32::MAX)),
				color,
				coverage,
			);
		}
	}
}

fn blend_pixel(image: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>, coverage: u8) {
	if x < 0 || y < 0 {
		return;
	}

	let Ok(x) = u32::try_from(x) else {
		return;
	};
	let Ok(y) = u32::try_from(y) else {
		return;
	};

	if x >= image.width() || y >= image.height() {
		return;
	}

	let alpha = (f32::from(color[3]) / 255.0) * (f32::from(coverage) / 255.0);

	if alpha <= 0.0 {
		return;
	}

	let pixel = image.get_pixel_mut(x, y);
	let inv_alpha = 1.0 - alpha;

	for channel in 0..3 {
		let dst = f32::from(pixel[channel]);
		let src = f32::from(color[channel]);

		pixel[channel] = (src * alpha + dst * inv_alpha).round().clamp(0.0, 255.0) as u8;
	}

	pixel[3] = 255;
}

#[cfg(test)]
mod tests {
	use egui::Pos2;
	use image::Rgba;

	use crate::text_rendering::RasterTextAnnotation;

	#[test]
	fn bitmap_fallback_draws_visible_pixels_for_ascii_text() {
		let mut image = image::RgbaImage::from_pixel(96, 48, Rgba([0, 0, 0, 0]));

		super::render_with_bitmap_fallback(
			&mut image,
			RasterTextAnnotation {
				anchor_px: Pos2::new(8.0, 8.0),
				font_size_px: 16.0,
				fill_rgba: [255, 255, 255, 255],
				text: "Text",
			},
		);

		assert!(image.pixels().any(|pixel| pixel[3] != 0));
	}

	#[test]
	fn bitmap_fallback_does_not_draw_shadow_offset_pixels() {
		let mut image = image::RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0]));

		super::render_with_bitmap_fallback(
			&mut image,
			RasterTextAnnotation {
				anchor_px: Pos2::new(8.0, 8.0),
				font_size_px: 16.0,
				fill_rgba: [255, 255, 255, 255],
				text: "A",
			},
		);

		for (x, y, pixel) in image.enumerate_pixels() {
			let inside_expected_bounds = (8..24).contains(&x) && (8..24).contains(&y);

			if !inside_expected_bounds {
				assert_eq!(pixel[3], 0, "unexpected pixel outside glyph bounds at ({x}, {y})");
			}
		}
	}
}
