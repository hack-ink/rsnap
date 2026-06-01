//! Portable frozen-overlay export compositing used by native hosts.

use std::f32::consts::PI;

use color_eyre::eyre::{self, Result};
use egui::Pos2;
use image::{
	Rgba, RgbaImage,
	imageops::{self, FilterType},
};

use crate::text_rendering::{self, RasterTextAnnotation};
use rsnap_capture_core::{self, DisplayPointRect};

const SPOTLIGHT_VISIBLE_NUMERATOR: u16 = 173;
const STROKE_EXPORT_ALPHA: f32 = 0.96;
const DARK_TEXT_SHADOW_RGBA: [u8; 4] = [0, 0, 0, 115];
const LIGHT_TEXT_SHADOW_RGBA: [u8; 4] = [255, 255, 255, 122];

/// Point-space coordinate used by frozen-overlay export annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportPoint {
	/// X coordinate in frozen capture point-space.
	pub x: f64,
	/// Y coordinate in frozen capture point-space.
	pub y: f64,
}
impl FrozenOverlayExportPoint {
	/// Creates a frozen-overlay export point.
	#[must_use]
	pub const fn new(x: f64, y: f64) -> Self {
		Self { x, y }
	}
}

/// Stroke style used by pen and arrow export annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportStrokeStyle {
	/// Stroke width in frozen capture points.
	pub stroke_width_points: f32,
	/// Source color as non-premultiplied RGBA bytes.
	pub rgba: [u8; 4],
}

/// Spotlight border style used by export annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportSpotlightStyle {
	/// Border width in frozen capture points.
	pub border_width_points: f32,
	/// Border color as non-premultiplied RGBA bytes.
	pub border_rgba: [u8; 4],
}

/// Text style used by export annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportTextStyle {
	/// Font size in frozen capture points.
	pub font_size_points: f32,
	/// Text fill color as non-premultiplied RGBA bytes.
	pub rgba: [u8; 4],
}

/// Pen stroke export annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenOverlayExportPen {
	/// Stroke points in frozen capture point-space.
	pub points: Vec<FrozenOverlayExportPoint>,
	/// Stroke style.
	pub style: FrozenOverlayExportStrokeStyle,
}

/// Arrow export annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportArrow {
	/// Arrow tail in frozen capture point-space.
	pub start: FrozenOverlayExportPoint,
	/// Arrow tip in frozen capture point-space.
	pub end: FrozenOverlayExportPoint,
	/// Stroke style.
	pub style: FrozenOverlayExportStrokeStyle,
}

/// Mosaic privacy export annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportMosaic {
	/// Mosaic rectangle in frozen capture point-space.
	pub rect: DisplayPointRect,
}

/// Spotlight export annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenOverlayExportSpotlight {
	/// Spotlight rectangle in frozen capture point-space.
	pub rect: DisplayPointRect,
	/// Spotlight border style.
	pub style: FrozenOverlayExportSpotlightStyle,
}

/// Text export annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenOverlayExportText {
	/// Text anchor in frozen capture point-space.
	pub anchor: FrozenOverlayExportPoint,
	/// Text payload.
	pub text: String,
	/// Text style.
	pub style: FrozenOverlayExportTextStyle,
}

#[derive(Clone, Copy, Debug)]
struct ArrowGeometry {
	shaft_end: Pos2,
	head_left: Pos2,
	head_right: Pos2,
}

#[derive(Clone, Copy, Debug)]
struct PixelRect {
	x: u32,
	y: u32,
	width: u32,
	height: u32,
}

#[derive(Clone, Copy, Debug)]
struct FloatRect {
	x: f32,
	y: f32,
	width: f32,
	height: f32,
}

#[derive(Clone, Copy, Debug)]
struct ExportTransform {
	selection: DisplayPointRect,
	image_width: u32,
	image_height: u32,
	scale_x: f64,
	scale_y: f64,
}
impl ExportTransform {
	fn new(selection: DisplayPointRect, image_width: u32, image_height: u32) -> Result<Self> {
		eyre::ensure!(
			image_width > 0 && image_height > 0,
			"frozen-overlay export image dimensions must be non-empty"
		);
		eyre::ensure!(
			valid_rect(selection),
			"frozen-overlay export selection must be finite and non-empty"
		);

		Ok(Self {
			selection,
			image_width,
			image_height,
			scale_x: f64::from(image_width) / selection.width,
			scale_y: f64::from(image_height) / selection.height,
		})
	}

	fn scalar_scale(self) -> f32 {
		((self.scale_x + self.scale_y) * 0.5) as f32
	}

	fn point_to_pixels(self, point: FrozenOverlayExportPoint) -> Option<Pos2> {
		if !point.x.is_finite() || !point.y.is_finite() {
			return None;
		}

		let x = (point.x - self.selection.x) * self.scale_x;
		let y = (self.selection.y + self.selection.height - point.y) * self.scale_y;

		f64_pair_to_pos2(x, y)
	}

	fn float_rect(self, rect: DisplayPointRect) -> Option<FloatRect> {
		if !valid_rect(rect) {
			return None;
		}

		let x = (rect.x - self.selection.x) * self.scale_x;
		let y = (self.selection.y + self.selection.height - (rect.y + rect.height)) * self.scale_y;
		let width = rect.width * self.scale_x;
		let height = rect.height * self.scale_y;
		let origin = f64_pair_to_pos2(x, y)?;
		let size = f64_pair_to_pos2(width, height)?;

		Some(FloatRect { x: origin.x, y: origin.y, width: size.x, height: size.y })
	}

	fn integral_image_rect(self, rect: DisplayPointRect) -> Option<PixelRect> {
		let rect = self.float_rect(rect)?;
		let left = rect.x.floor().max(0.0);
		let top = rect.y.floor().max(0.0);
		let right = (rect.x + rect.width).ceil().min(self.image_width as f32);
		let bottom = (rect.y + rect.height).ceil().min(self.image_height as f32);

		if left >= right || top >= bottom {
			return None;
		}

		Some(PixelRect {
			x: left as u32,
			y: top as u32,
			width: (right - left) as u32,
			height: (bottom - top) as u32,
		})
	}

	fn source_image_rect(self, rect: DisplayPointRect) -> Option<DisplayPointRect> {
		if !valid_rect(rect) {
			return None;
		}

		let right = rect.x + rect.width;
		let bottom = rect.y + rect.height;
		let selection_bottom = self.selection.y + self.selection.height;

		Some(DisplayPointRect::new(
			(rect.x - self.selection.x) * self.scale_x,
			(selection_bottom - bottom) * self.scale_y,
			(right - rect.x) * self.scale_x,
			(bottom - rect.y) * self.scale_y,
		))
	}
}

/// One committed frozen-overlay edit to composite into an exported image.
#[derive(Clone, Debug, PartialEq)]
pub enum FrozenOverlayExportElement {
	/// Pen stroke annotation.
	Pen(FrozenOverlayExportPen),
	/// Arrow annotation.
	Arrow(FrozenOverlayExportArrow),
	/// Mosaic privacy rectangle.
	Mosaic(FrozenOverlayExportMosaic),
	/// Spotlight annotation.
	Spotlight(FrozenOverlayExportSpotlight),
	/// Text annotation.
	Text(FrozenOverlayExportText),
}

/// Composites committed frozen-overlay annotations into a row-major RGBA export image.
///
/// The compositor intentionally mirrors the native host's previous export order:
/// mosaics first, spotlight scrim/restores second, then pen, arrow, and text annotations.
pub fn render_frozen_overlay_export_rgba(
	width: u32,
	height: u32,
	rgba: &[u8],
	selection: DisplayPointRect,
	elements: &[FrozenOverlayExportElement],
) -> Result<RgbaImage> {
	let transform = ExportTransform::new(selection, width, height)?;
	let mut image = rgba_image_from_bytes(width, height, rgba)?;

	apply_mosaics(&mut image, transform, elements);
	apply_spotlights(&mut image, transform, elements);
	render_pen_annotations(&mut image, transform, elements);
	render_arrow_annotations(&mut image, transform, elements);
	render_text_annotations(&mut image, transform, elements);

	Ok(image)
}

fn rgba_image_from_bytes(width: u32, height: u32, rgba: &[u8]) -> Result<RgbaImage> {
	let expected_len = usize::try_from(width)
		.ok()
		.zip(usize::try_from(height).ok())
		.and_then(|(width, height)| width.checked_mul(height))
		.and_then(|pixels| pixels.checked_mul(4))
		.ok_or_else(|| eyre::eyre!("frozen-overlay export dimensions overflow"))?;

	eyre::ensure!(
		rgba.len() == expected_len,
		"frozen-overlay export byte length mismatch: expected {} got {}",
		expected_len,
		rgba.len()
	);

	RgbaImage::from_raw(width, height, rgba.to_vec())
		.ok_or_else(|| eyre::eyre!("frozen-overlay export RGBA payload is invalid"))
}

fn apply_mosaics(
	image: &mut RgbaImage,
	transform: ExportTransform,
	elements: &[FrozenOverlayExportElement],
) {
	for element in elements {
		let FrozenOverlayExportElement::Mosaic(annotation) = element else {
			continue;
		};

		apply_mosaic(image, transform, annotation.rect);
	}
}

fn apply_mosaic(image: &mut RgbaImage, transform: ExportTransform, rect: DisplayPointRect) {
	let Some(destination) = transform.integral_image_rect(rect) else {
		return;
	};
	let Some(source_rect) = transform.source_image_rect(rect) else {
		return;
	};
	let Some(patch) = rsnap_capture_core::frozen_mosaic_light_privacy_patch(
		image.width(),
		image.height(),
		source_rect,
	) else {
		return;
	};
	let patch = if patch.width() == destination.width && patch.height() == destination.height {
		patch
	} else {
		imageops::resize(&patch, destination.width, destination.height, FilterType::Lanczos3)
	};

	imageops::replace(image, &patch, i64::from(destination.x), i64::from(destination.y));
}

fn apply_spotlights(
	image: &mut RgbaImage,
	transform: ExportTransform,
	elements: &[FrozenOverlayExportElement],
) {
	let spotlights = elements
		.iter()
		.filter_map(|element| match element {
			FrozenOverlayExportElement::Spotlight(annotation) => Some(annotation),
			_ => None,
		})
		.collect::<Vec<_>>();

	if spotlights.is_empty() {
		return;
	}

	let original = image.clone();

	dim_image_for_spotlight(image);

	for spotlight in &spotlights {
		restore_spotlight_rect(image, &original, transform, spotlight.rect);
	}
	for spotlight in spotlights {
		render_spotlight_border(image, transform, spotlight.rect, spotlight.style);
	}
}

fn dim_image_for_spotlight(image: &mut RgbaImage) {
	for pixel in image.pixels_mut() {
		for channel in 0..3 {
			pixel[channel] =
				((u16::from(pixel[channel]) * SPOTLIGHT_VISIBLE_NUMERATOR) / 255) as u8;
		}
	}
}

fn restore_spotlight_rect(
	image: &mut RgbaImage,
	original: &RgbaImage,
	transform: ExportTransform,
	rect: DisplayPointRect,
) {
	let Some(destination) = transform.integral_image_rect(rect) else {
		return;
	};
	let row_stride = image.width() as usize * 4;
	let left_byte = destination.x as usize * 4;
	let copy_len = destination.width as usize * 4;
	let original_bytes = original.as_raw();
	let image_bytes = image.as_mut();

	for row in destination.y..destination.y + destination.height {
		let start = row as usize * row_stride + left_byte;
		let end = start + copy_len;

		image_bytes[start..end].copy_from_slice(&original_bytes[start..end]);
	}
}

fn render_spotlight_border(
	image: &mut RgbaImage,
	transform: ExportTransform,
	rect: DisplayPointRect,
	style: FrozenOverlayExportSpotlightStyle,
) {
	let line_width = style.border_width_points * transform.scalar_scale();

	if line_width <= f32::EPSILON {
		return;
	}

	let Some(rect) = transform.float_rect(rect) else {
		return;
	};
	let inset = line_width * 0.5;
	let left = rect.x + inset;
	let right = rect.x + rect.width - inset;
	let top = rect.y + inset;
	let bottom = rect.y + rect.height - inset;
	let color = with_scaled_alpha(style.border_rgba, STROKE_EXPORT_ALPHA);

	draw_segments(
		image,
		&[
			(Pos2::new(left, top), Pos2::new(right, top)),
			(Pos2::new(right, top), Pos2::new(right, bottom)),
			(Pos2::new(right, bottom), Pos2::new(left, bottom)),
			(Pos2::new(left, bottom), Pos2::new(left, top)),
		],
		line_width,
		Rgba(color),
	);
}

fn render_pen_annotations(
	image: &mut RgbaImage,
	transform: ExportTransform,
	elements: &[FrozenOverlayExportElement],
) {
	for element in elements {
		let FrozenOverlayExportElement::Pen(annotation) = element else {
			continue;
		};
		let points = annotation
			.points
			.iter()
			.filter_map(|point| transform.point_to_pixels(*point))
			.collect::<Vec<_>>();
		let color = Rgba(with_scaled_alpha(annotation.style.rgba, STROKE_EXPORT_ALPHA));

		draw_polyline(
			image,
			&points,
			annotation.style.stroke_width_points * transform.scalar_scale(),
			color,
		);
	}
}

fn render_arrow_annotations(
	image: &mut RgbaImage,
	transform: ExportTransform,
	elements: &[FrozenOverlayExportElement],
) {
	for element in elements {
		let FrozenOverlayExportElement::Arrow(annotation) = element else {
			continue;
		};

		render_arrow_annotation(image, transform, *annotation);
	}
}

fn render_arrow_annotation(
	image: &mut RgbaImage,
	transform: ExportTransform,
	annotation: FrozenOverlayExportArrow,
) {
	let Some(start) = transform.point_to_pixels(annotation.start) else {
		return;
	};
	let Some(end) = transform.point_to_pixels(annotation.end) else {
		return;
	};
	let Some(geometry) =
		arrow_geometry(start, end, annotation.style.stroke_width_points, transform)
	else {
		return;
	};
	let stroke_width = annotation.style.stroke_width_points * 1.4 * transform.scalar_scale();
	let color = Rgba(with_scaled_alpha(annotation.style.rgba, STROKE_EXPORT_ALPHA));

	draw_segments(
		image,
		&[(start, geometry.shaft_end), (end, geometry.head_left), (end, geometry.head_right)],
		stroke_width,
		color,
	);
}

fn render_text_annotations(
	image: &mut RgbaImage,
	transform: ExportTransform,
	elements: &[FrozenOverlayExportElement],
) {
	for element in elements {
		let FrozenOverlayExportElement::Text(annotation) = element else {
			continue;
		};

		render_text_annotation(image, transform, annotation);
	}
}

fn render_text_annotation(
	image: &mut RgbaImage,
	transform: ExportTransform,
	annotation: &FrozenOverlayExportText,
) {
	if annotation.text.trim().is_empty() {
		return;
	}

	let Some(anchor) = transform.point_to_pixels(annotation.anchor) else {
		return;
	};
	let font_size_px = (annotation.style.font_size_points * transform.scalar_scale()).max(1.0);
	let shadow_anchor = Pos2::new(anchor.x, anchor.y + transform.scalar_scale().max(1.0));
	let shadow = RasterTextAnnotation {
		anchor_px: shadow_anchor,
		font_size_px,
		fill_rgba: text_shadow_rgba(annotation.style.rgba),
		text: annotation.text.as_str(),
	};
	let fill = RasterTextAnnotation {
		anchor_px: anchor,
		font_size_px,
		fill_rgba: annotation.style.rgba,
		text: annotation.text.as_str(),
	};

	text_rendering::render_text_annotations(image, &[shadow, fill]);
}

fn text_shadow_rgba(fill: [u8; 4]) -> [u8; 4] {
	if fill[0] <= 40 && fill[1] <= 40 && fill[2] <= 40 {
		LIGHT_TEXT_SHADOW_RGBA
	} else {
		DARK_TEXT_SHADOW_RGBA
	}
}

fn draw_polyline(image: &mut RgbaImage, points: &[Pos2], line_width: f32, color: Rgba<u8>) {
	if points.is_empty() || line_width <= f32::EPSILON {
		return;
	}
	if points.len() == 1 {
		draw_segments(image, &[(points[0], points[0])], line_width, color);

		return;
	}

	let segments = points.windows(2).map(|points| (points[0], points[1])).collect::<Vec<_>>();

	draw_segments(image, &segments, line_width, color);
}

fn draw_segments(
	image: &mut RgbaImage,
	segments: &[(Pos2, Pos2)],
	line_width: f32,
	color: Rgba<u8>,
) {
	if segments.is_empty() || image.width() == 0 || image.height() == 0 {
		return;
	}

	let radius = (line_width * 0.5).max(0.5);
	let Some(mask_rect) = segments_pixel_bounds(segments, image.width(), image.height(), radius)
	else {
		return;
	};
	let mut coverage_mask = vec![0_u8; mask_rect.width as usize * mask_rect.height as usize];

	for (start, end) in segments {
		rasterize_segment(
			&mut coverage_mask,
			mask_rect,
			image.width(),
			image.height(),
			*start,
			*end,
			radius,
		);
	}

	blend_coverage_mask(image, mask_rect, &coverage_mask, color);
}

fn rasterize_segment(
	coverage_mask: &mut [u8],
	mask_rect: PixelRect,
	width: u32,
	height: u32,
	start: Pos2,
	end: Pos2,
	radius: f32,
) {
	let delta = end - start;
	let delta_len_sq = delta.length_sq();

	if delta_len_sq <= f32::EPSILON {
		rasterize_circle(coverage_mask, mask_rect, width, height, start, radius);

		return;
	}

	let Some(bounds) = segment_pixel_bounds(start, end, width, height, radius)
		.and_then(|bounds| intersect_pixel_rect(bounds, mask_rect))
	else {
		return;
	};

	for y in bounds.y..bounds.y + bounds.height {
		for x in bounds.x..bounds.x + bounds.width {
			let sample = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
			let projection = ((sample - start).dot(delta) / delta_len_sq).clamp(0.0, 1.0);
			let nearest = start + delta * projection;

			update_coverage_mask(
				coverage_mask,
				mask_rect,
				x,
				y,
				stroke_coverage(sample.distance(nearest), radius),
			);
		}
	}
}

fn rasterize_circle(
	coverage_mask: &mut [u8],
	mask_rect: PixelRect,
	width: u32,
	height: u32,
	center: Pos2,
	radius: f32,
) {
	let Some(bounds) = circle_pixel_bounds(center, width, height, radius)
		.and_then(|bounds| intersect_pixel_rect(bounds, mask_rect))
	else {
		return;
	};

	for y in bounds.y..bounds.y + bounds.height {
		for x in bounds.x..bounds.x + bounds.width {
			let sample = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);

			update_coverage_mask(
				coverage_mask,
				mask_rect,
				x,
				y,
				stroke_coverage(sample.distance(center), radius),
			);
		}
	}
}

fn segments_pixel_bounds(
	segments: &[(Pos2, Pos2)],
	width: u32,
	height: u32,
	radius: f32,
) -> Option<PixelRect> {
	let mut bounds = None;

	for (start, end) in segments {
		let Some(segment_bounds) = segment_pixel_bounds(*start, *end, width, height, radius) else {
			continue;
		};

		bounds = Some(match bounds {
			Some(bounds) => union_pixel_rect(bounds, segment_bounds),
			None => segment_bounds,
		});
	}

	bounds
}

fn segment_pixel_bounds(
	start: Pos2,
	end: Pos2,
	width: u32,
	height: u32,
	radius: f32,
) -> Option<PixelRect> {
	pixel_bounds(
		start.x.min(end.x) - radius - 0.5,
		start.y.min(end.y) - radius - 0.5,
		start.x.max(end.x) + radius + 0.5,
		start.y.max(end.y) + radius + 0.5,
		width,
		height,
	)
}

fn circle_pixel_bounds(center: Pos2, width: u32, height: u32, radius: f32) -> Option<PixelRect> {
	pixel_bounds(
		center.x - radius - 0.5,
		center.y - radius - 0.5,
		center.x + radius + 0.5,
		center.y + radius + 0.5,
		width,
		height,
	)
}

fn pixel_bounds(
	min_x: f32,
	min_y: f32,
	max_x: f32,
	max_y: f32,
	width: u32,
	height: u32,
) -> Option<PixelRect> {
	if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
		return None;
	}

	let left = min_x.floor().max(0.0) as u32;
	let top = min_y.floor().max(0.0) as u32;
	let right = (max_x.ceil() + 1.0).clamp(0.0, width as f32) as u32;
	let bottom = (max_y.ceil() + 1.0).clamp(0.0, height as f32) as u32;

	if left >= right || top >= bottom {
		return None;
	}

	Some(PixelRect { x: left, y: top, width: right - left, height: bottom - top })
}

fn intersect_pixel_rect(first: PixelRect, second: PixelRect) -> Option<PixelRect> {
	let left = first.x.max(second.x);
	let top = first.y.max(second.y);
	let right = (first.x + first.width).min(second.x + second.width);
	let bottom = (first.y + first.height).min(second.y + second.height);

	if left >= right || top >= bottom {
		return None;
	}

	Some(PixelRect { x: left, y: top, width: right - left, height: bottom - top })
}

fn union_pixel_rect(first: PixelRect, second: PixelRect) -> PixelRect {
	let left = first.x.min(second.x);
	let top = first.y.min(second.y);
	let right = (first.x + first.width).max(second.x + second.width);
	let bottom = (first.y + first.height).max(second.y + second.height);

	PixelRect { x: left, y: top, width: right - left, height: bottom - top }
}

fn stroke_coverage(distance: f32, radius: f32) -> u8 {
	((radius + 0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u8
}

fn update_coverage_mask(
	coverage_mask: &mut [u8],
	mask_rect: PixelRect,
	x: u32,
	y: u32,
	coverage: u8,
) {
	if coverage == 0 {
		return;
	}

	let index = (y - mask_rect.y) as usize * mask_rect.width as usize + (x - mask_rect.x) as usize;

	coverage_mask[index] = coverage_mask[index].max(coverage);
}

fn blend_coverage_mask(
	image: &mut RgbaImage,
	mask_rect: PixelRect,
	coverage_mask: &[u8],
	color: Rgba<u8>,
) {
	let source_alpha = f32::from(color[3]) / 255.0;
	let image_width = image.width() as usize;
	let image_bytes = image.as_mut();
	let mask_width = mask_rect.width as usize;
	let left = mask_rect.x as usize;
	let top = mask_rect.y as usize;

	for local_y in 0..mask_rect.height as usize {
		let mask_row_start = local_y * mask_width;
		let image_row_start = ((top + local_y) * image_width + left) * 4;

		for local_x in 0..mask_width {
			let mask_alpha = coverage_mask[mask_row_start + local_x];

			if mask_alpha == 0 {
				continue;
			}

			let pixel_start = image_row_start + local_x * 4;
			let pixel_end = pixel_start + 4;

			blend_pixel_channels(
				&mut image_bytes[pixel_start..pixel_end],
				color,
				f32::from(mask_alpha) / 255.0 * source_alpha,
			);
		}
	}
}

fn blend_pixel_channels(pixel: &mut [u8], color: Rgba<u8>, src_a: f32) {
	let dst_a = f32::from(pixel[3]) / 255.0;
	let out_a = src_a + dst_a * (1.0 - src_a);

	if out_a <= f32::EPSILON {
		return;
	}

	for channel in 0..3 {
		let src = f32::from(color[channel]) / 255.0;
		let dst = f32::from(pixel[channel]) / 255.0;
		let out = (src * src_a + dst * dst_a * (1.0 - src_a)) / out_a;

		pixel[channel] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
	}

	pixel[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn with_scaled_alpha(mut rgba: [u8; 4], scale: f32) -> [u8; 4] {
	rgba[3] = (f32::from(rgba[3]) * scale).round().clamp(0.0, 255.0) as u8;

	rgba
}

fn arrow_geometry(
	start: Pos2,
	end: Pos2,
	stroke_width_points: f32,
	transform: ExportTransform,
) -> Option<ArrowGeometry> {
	let distance = start.distance(end);

	if distance <= f32::EPSILON {
		return None;
	}

	let stroke_width = stroke_width_points * 1.4 * transform.scalar_scale();
	let head_length = (stroke_width * 4.2).max(16.0 * transform.scalar_scale()).min(distance * 0.9);
	let head_spread = PI / 7.0;
	let angle = (end.y - start.y).atan2(end.x - start.x);
	let direction = Pos2::new(angle.cos(), angle.sin());
	let shaft_end = Pos2::new(
		end.x - direction.x * head_length * 0.72,
		end.y - direction.y * head_length * 0.72,
	);

	Some(ArrowGeometry {
		shaft_end,
		head_left: Pos2::new(
			end.x - (angle - head_spread).cos() * head_length,
			end.y - (angle - head_spread).sin() * head_length,
		),
		head_right: Pos2::new(
			end.x - (angle + head_spread).cos() * head_length,
			end.y - (angle + head_spread).sin() * head_length,
		),
	})
}

fn valid_rect(rect: DisplayPointRect) -> bool {
	rect.x.is_finite()
		&& rect.y.is_finite()
		&& rect.width.is_finite()
		&& rect.height.is_finite()
		&& rect.width > 0.0
		&& rect.height > 0.0
}

fn f64_pair_to_pos2(x: f64, y: f64) -> Option<Pos2> {
	if x.is_finite()
		&& y.is_finite()
		&& x >= f64::from(f32::MIN)
		&& y >= f64::from(f32::MIN)
		&& x <= f64::from(f32::MAX)
		&& y <= f64::from(f32::MAX)
	{
		Some(Pos2::new(x as f32, y as f32))
	} else {
		None
	}
}

#[cfg(test)]
mod tests {
	use image::{Rgba, RgbaImage};

	use crate::frozen_export::{
		self, FrozenOverlayExportArrow, FrozenOverlayExportElement, FrozenOverlayExportMosaic,
		FrozenOverlayExportPen, FrozenOverlayExportPoint, FrozenOverlayExportSpotlight,
		FrozenOverlayExportSpotlightStyle, FrozenOverlayExportStrokeStyle, FrozenOverlayExportText,
		FrozenOverlayExportTextStyle,
	};
	use rsnap_capture_core::DisplayPointRect;

	#[test]
	fn export_compositor_applies_mosaic_spotlight_and_stroke() {
		let image = RgbaImage::from_fn(20, 12, |x, y| Rgba([x as u8, y as u8, 120, 255]));
		let elements = vec![
			FrozenOverlayExportElement::Mosaic(FrozenOverlayExportMosaic {
				rect: DisplayPointRect::new(2.0, 2.0, 8.0, 4.0),
			}),
			FrozenOverlayExportElement::Spotlight(FrozenOverlayExportSpotlight {
				rect: DisplayPointRect::new(10.0, 2.0, 5.0, 4.0),
				style: FrozenOverlayExportSpotlightStyle {
					border_width_points: 1.0,
					border_rgba: [255, 255, 255, 255],
				},
			}),
			FrozenOverlayExportElement::Pen(FrozenOverlayExportPen {
				points: vec![
					FrozenOverlayExportPoint::new(0.0, 0.0),
					FrozenOverlayExportPoint::new(19.0, 11.0),
				],
				style: FrozenOverlayExportStrokeStyle {
					stroke_width_points: 2.0,
					rgba: [102, 178, 255, 255],
				},
			}),
		];
		let rendered = frozen_export::render_frozen_overlay_export_rgba(
			image.width(),
			image.height(),
			image.as_raw(),
			DisplayPointRect::new(0.0, 0.0, 20.0, 12.0),
			&elements,
		)
		.expect("valid export");

		assert_eq!(rendered.dimensions(), image.dimensions());
		assert_ne!(rendered.as_raw(), image.as_raw());
		assert_eq!(rendered.get_pixel(12, 8), image.get_pixel(12, 8));
		assert!(rendered.get_pixel(1, 5)[1] < image.get_pixel(1, 5)[1]);
	}

	#[test]
	fn export_compositor_maps_display_y_to_image_top_left_pixels() {
		let image = RgbaImage::from_pixel(20, 20, Rgba([24, 24, 24, 255]));
		let bottom_point = FrozenOverlayExportElement::Pen(FrozenOverlayExportPen {
			points: vec![FrozenOverlayExportPoint::new(10.0, 0.0)],
			style: FrozenOverlayExportStrokeStyle {
				stroke_width_points: 2.0,
				rgba: [255, 107, 107, 255],
			},
		});
		let top_point = FrozenOverlayExportElement::Pen(FrozenOverlayExportPen {
			points: vec![FrozenOverlayExportPoint::new(10.0, 20.0)],
			style: FrozenOverlayExportStrokeStyle {
				stroke_width_points: 2.0,
				rgba: [255, 107, 107, 255],
			},
		});
		let bottom_rendered = frozen_export::render_frozen_overlay_export_rgba(
			image.width(),
			image.height(),
			image.as_raw(),
			DisplayPointRect::new(0.0, 0.0, 20.0, 20.0),
			&[bottom_point],
		)
		.expect("valid bottom-point export");
		let top_rendered = frozen_export::render_frozen_overlay_export_rgba(
			image.width(),
			image.height(),
			image.as_raw(),
			DisplayPointRect::new(0.0, 0.0, 20.0, 20.0),
			&[top_point],
		)
		.expect("valid top-point export");

		assert_ne!(bottom_rendered.get_pixel(10, 19), image.get_pixel(10, 19));
		assert_eq!(bottom_rendered.get_pixel(10, 0), image.get_pixel(10, 0));
		assert_ne!(top_rendered.get_pixel(10, 0), image.get_pixel(10, 0));
		assert_eq!(top_rendered.get_pixel(10, 19), image.get_pixel(10, 19));
	}

	#[test]
	fn export_compositor_skips_offscreen_segments_without_dropping_visible_stroke() {
		let image = RgbaImage::from_pixel(20, 20, Rgba([24, 24, 24, 255]));
		let elements = vec![FrozenOverlayExportElement::Pen(FrozenOverlayExportPen {
			points: vec![
				FrozenOverlayExportPoint::new(-20.0, 10.0),
				FrozenOverlayExportPoint::new(-10.0, 10.0),
				FrozenOverlayExportPoint::new(10.0, 10.0),
				FrozenOverlayExportPoint::new(12.0, 10.0),
			],
			style: FrozenOverlayExportStrokeStyle {
				stroke_width_points: 2.0,
				rgba: [255, 107, 107, 255],
			},
		})];
		let rendered = frozen_export::render_frozen_overlay_export_rgba(
			image.width(),
			image.height(),
			image.as_raw(),
			DisplayPointRect::new(0.0, 0.0, 20.0, 20.0),
			&elements,
		)
		.expect("valid export");

		assert_ne!(rendered.get_pixel(10, 10), image.get_pixel(10, 10));
	}

	#[test]
	fn export_compositor_renders_arrow_and_text() {
		let image = RgbaImage::from_pixel(64, 40, Rgba([24, 24, 24, 255]));
		let elements = vec![
			FrozenOverlayExportElement::Arrow(FrozenOverlayExportArrow {
				start: FrozenOverlayExportPoint::new(4.0, 8.0),
				end: FrozenOverlayExportPoint::new(50.0, 20.0),
				style: FrozenOverlayExportStrokeStyle {
					stroke_width_points: 3.0,
					rgba: [255, 107, 107, 255],
				},
			}),
			FrozenOverlayExportElement::Text(FrozenOverlayExportText {
				anchor: FrozenOverlayExportPoint::new(6.0, 24.0),
				text: "Hi".to_owned(),
				style: FrozenOverlayExportTextStyle {
					font_size_points: 12.0,
					rgba: [255, 255, 255, 255],
				},
			}),
		];
		let rendered = frozen_export::render_frozen_overlay_export_rgba(
			image.width(),
			image.height(),
			image.as_raw(),
			DisplayPointRect::new(0.0, 0.0, 64.0, 40.0),
			&elements,
		)
		.expect("valid export");

		assert_ne!(rendered.as_raw(), image.as_raw());
		assert!(rendered.pixels().any(|pixel| pixel[0] > 200 || pixel[1] > 100 || pixel[2] > 100));
	}
}
