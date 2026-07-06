use std::sync::{Arc, OnceLock};

use font8x8::{BASIC_FONTS, UnicodeFonts};
use fontdue::{
	Font, FontSettings,
	layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle},
};
use image::{Rgba, RgbaImage};

use crate::point::PixelPoint;
use crate::system_fonts;
use crate::system_fonts::SystemTextFontData;

const BITMAP_GLYPH_SIDE_PX: u32 = 8;
const BITMAP_GLYPH_ADVANCE_PX: u32 = 8;
const BITMAP_LINE_GAP_PX: u32 = 2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterTextAnnotation<'a> {
	pub(crate) anchor_px: PixelPoint,
	pub(crate) font_size_px: f32,
	pub(crate) fill_rgba: [u8; 4],
	pub(crate) text: &'a str,
}

/// Pixel bounds for a frozen-overlay text annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayTextBounds {
	/// Text width in pixels.
	pub width: f32,
	/// Text height in pixels.
	pub height: f32,
}

#[derive(Debug)]
struct ExportTextFont {
	font_data: Arc<SystemTextFontData>,
	fontdue_font: OnceLock<Option<Font>>,
}
impl ExportTextFont {
	fn new(font_data: Arc<SystemTextFontData>) -> Self {
		Self { font_data, fontdue_font: OnceLock::new() }
	}

	fn font(&self) -> Option<&Font> {
		self.fontdue_font
			.get_or_init(|| {
				Font::from_bytes(
					self.font_data.bytes.as_slice(),
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

/// Measures a frozen-overlay text annotation with the platform font fallback stack.
#[must_use]
pub fn measure_frozen_overlay_text_bounds(
	text: &str,
	font_size_px: f32,
) -> Option<FrozenOverlayTextBounds> {
	if text.is_empty() {
		return None;
	}

	let fonts = export_text_fonts();

	if let Some(runs) = build_text_font_runs(fonts, text)
		&& let Some(bounds) = measure_with_font_stack(text, font_size_px, fonts, &runs)
	{
		return Some(bounds);
	}

	Some(measure_bitmap_text_bounds(text, font_size_px))
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
	collect_export_text_fonts(system_fonts::system_text_fonts().iter().map(|font| font.font_data()))
}

fn collect_export_text_fonts(
	font_data: impl IntoIterator<Item = Arc<SystemTextFontData>>,
) -> Vec<ExportTextFont> {
	font_data
		.into_iter()
		.filter_map(|font_data| {
			let export_font = ExportTextFont::new(font_data);

			export_font.font().is_some().then_some(export_font)
		})
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

fn measure_with_font_stack(
	text: &str,
	font_size_px: f32,
	fonts: &[ExportTextFont],
	runs: &[TextFontRun<'_>],
) -> Option<FrozenOverlayTextBounds> {
	let parsed_fonts: Vec<_> = fonts.iter().filter_map(ExportTextFont::font).collect();

	if parsed_fonts.is_empty() || parsed_fonts.len() != fonts.len() {
		return None;
	}

	let mut layout = Layout::new(CoordinateSystem::PositiveYDown);

	layout.reset(&LayoutSettings::default());

	for run in runs {
		layout.append(
			&parsed_fonts,
			&TextStyle::new(run.text, font_size_px.max(8.0), run.font_index),
		);
	}

	let mut max_x = 0.0_f32;
	let mut max_y = 0.0_f32;

	for glyph in layout.glyphs() {
		max_x = max_x.max(glyph.x + glyph.width as f32);
		max_y = max_y.max(glyph.y + glyph.height as f32);
	}

	if max_x <= 0.0 && max_y <= 0.0 {
		return None;
	}

	let line_count = text.lines().count().max(1) as f32;
	let line_height = font_size_px.max(8.0) * 1.2;

	Some(FrozenOverlayTextBounds {
		width: max_x.ceil().max(1.0),
		height: max_y.ceil().max(line_height * line_count).max(1.0),
	})
}

fn measure_bitmap_text_bounds(text: &str, font_size_px: f32) -> FrozenOverlayTextBounds {
	let scale = (font_size_px.max(8.0) / BITMAP_GLYPH_SIDE_PX as f32).round().max(1.0) as u32;
	let glyph_advance = BITMAP_GLYPH_ADVANCE_PX.saturating_mul(scale) as f32;
	let line_height = BITMAP_GLYPH_SIDE_PX
		.saturating_mul(scale)
		.saturating_add(BITMAP_LINE_GAP_PX.saturating_mul(scale)) as f32;
	let mut line_width = 0.0_f32;
	let mut max_width = 0.0_f32;
	let mut line_count = 1_u32;

	for ch in text.chars() {
		if ch == '\n' {
			max_width = max_width.max(line_width);
			line_width = 0.0;
			line_count = line_count.saturating_add(1);
		} else {
			line_width += glyph_advance;
		}
	}

	max_width = max_width.max(line_width);

	FrozenOverlayTextBounds {
		width: max_width.ceil().max(1.0),
		height: (line_height * line_count as f32).ceil().max(1.0),
	}
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

	fonts
		.iter()
		.position(|font| font.supports_char(ch))
		.or(active_font_index)
		.or(previous_visible_font_index)
		.or_else(|| (!fonts.is_empty()).then_some(0))
}

fn render_with_font_stack(
	image: &mut RgbaImage,
	annotation: RasterTextAnnotation<'_>,
	fonts: &[ExportTextFont],
	runs: &[TextFontRun<'_>],
) -> bool {
	let parsed_fonts: Vec<_> = fonts.iter().filter_map(ExportTextFont::font).collect();

	if parsed_fonts.is_empty() || parsed_fonts.len() != fonts.len() {
		return false;
	}

	let fill_rgba = Rgba(annotation.fill_rgba);
	let mut layout = Layout::new(CoordinateSystem::PositiveYDown);

	layout.reset(&LayoutSettings {
		x: annotation.anchor_px.x,
		y: annotation.anchor_px.y,
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
				+ i32::try_from(u32::try_from(column_index).unwrap_or(0).saturating_mul(scale))
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

	let src_alpha = (f32::from(color[3]) / 255.0) * (f32::from(coverage) / 255.0);

	if src_alpha <= 0.0 {
		return;
	}

	let pixel = image.get_pixel_mut(x, y);
	let dst_alpha = f32::from(pixel[3]) / 255.0;
	let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);

	if out_alpha <= 0.0 {
		return;
	}

	for channel in 0..3 {
		let dst = f32::from(pixel[channel]);
		let src = f32::from(color[channel]);

		pixel[channel] = ((src * src_alpha + dst * dst_alpha * (1.0 - src_alpha)) / out_alpha)
			.round()
			.clamp(0.0, 255.0) as u8;
	}

	pixel[3] = (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use image::Rgba;

	use crate::point::PixelPoint;
	use crate::system_fonts::{self, SystemTextFontData};
	use crate::text_rendering::RasterTextAnnotation;

	#[test]
	fn bitmap_fallback_draws_visible_pixels_for_ascii_text() {
		let mut image = image::RgbaImage::from_pixel(96, 48, Rgba([0, 0, 0, 0]));

		super::render_with_bitmap_fallback(
			&mut image,
			RasterTextAnnotation {
				anchor_px: PixelPoint::new(8.0, 8.0),
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
				anchor_px: PixelPoint::new(8.0, 8.0),
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

	#[test]
	fn bitmap_glyph_draw_uses_lsb_first_bit_order() {
		let mut image = image::RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 0]));

		super::draw_bitmap_glyph(
			&mut image,
			0,
			0,
			&[0b0000_0001, 0, 0, 0, 0, 0, 0, 0],
			1,
			Rgba([255, 255, 255, 255]),
		);

		assert_eq!(*image.get_pixel(0, 0), Rgba([255, 255, 255, 255]));
		assert_eq!(*image.get_pixel(7, 0), Rgba([0, 0, 0, 0]));
	}

	#[test]
	fn blend_pixel_preserves_partial_alpha_for_antialiased_text_edges() {
		let mut image = image::RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0]));

		super::blend_pixel(&mut image, 0, 0, Rgba([255, 255, 255, 255]), 128);

		assert_eq!(*image.get_pixel(0, 0), Rgba([255, 255, 255, 128]));
	}

	#[test]
	fn font_stack_rendering_preserves_negative_anchor_offsets_for_clipping() {
		let fonts = super::export_text_fonts();
		let text = "Wide";
		let runs =
			super::build_text_font_runs(fonts, text).expect("ASCII text should map to a font");
		let annotation_at_origin = RasterTextAnnotation {
			anchor_px: PixelPoint::new(0.0, 0.0),
			font_size_px: 28.0,
			fill_rgba: [255, 255, 255, 255],
			text,
		};
		let annotation_with_negative_anchor =
			RasterTextAnnotation { anchor_px: PixelPoint::new(-8.0, -8.0), ..annotation_at_origin };
		let mut image_at_origin = image::RgbaImage::from_pixel(96, 64, Rgba([0, 0, 0, 0]));
		let mut image_with_negative_anchor =
			image::RgbaImage::from_pixel(96, 64, Rgba([0, 0, 0, 0]));

		assert!(super::render_with_font_stack(
			&mut image_at_origin,
			annotation_at_origin,
			fonts,
			&runs,
		));
		assert!(super::render_with_font_stack(
			&mut image_with_negative_anchor,
			annotation_with_negative_anchor,
			fonts,
			&runs,
		));

		let visible_pixels_at_origin =
			image_at_origin.pixels().filter(|pixel| pixel[3] != 0).count();
		let visible_pixels_with_negative_anchor =
			image_with_negative_anchor.pixels().filter(|pixel| pixel[3] != 0).count();

		assert!(visible_pixels_at_origin > 0);
		assert!(visible_pixels_with_negative_anchor > 0);
		assert!(visible_pixels_with_negative_anchor < visible_pixels_at_origin);
	}

	#[test]
	fn collect_export_text_fonts_filters_unparsable_font_data() {
		let valid_font_data = system_fonts::system_text_fonts()
			.first()
			.expect("system font database should include at least one text font")
			.font_data();
		let fonts = super::collect_export_text_fonts([
			Arc::new(SystemTextFontData::from_static(b"not-a-font")),
			valid_font_data,
		]);

		assert_eq!(fonts.len(), 1);
		assert!(fonts[0].font().is_some());
	}

	#[test]
	fn font_stack_rendering_keeps_supported_chars_when_text_contains_missing_glyph() {
		let fonts = super::export_text_fonts();
		let text = "A\u{10ffff}B";
		let runs = super::build_text_font_runs(fonts, text)
			.expect("missing glyphs should not force the whole annotation onto bitmap fallback");
		let mut image = image::RgbaImage::from_pixel(128, 64, Rgba([0, 0, 0, 0]));

		assert!(super::render_with_font_stack(
			&mut image,
			RasterTextAnnotation {
				anchor_px: PixelPoint::new(8.0, 8.0),
				font_size_px: 24.0,
				fill_rgba: [255, 255, 255, 255],
				text,
			},
			fonts,
			&runs,
		));
		assert!(image.pixels().any(|pixel| pixel[3] != 0));
	}
}
