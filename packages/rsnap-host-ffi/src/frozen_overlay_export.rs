//! Frozen-overlay export C ABI entrypoints.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;
use std::slice;

use crate::{
	RsnapFloatPoint, RsnapFloatRect, RsnapFrozenAnnotationColor, RsnapFrozenOverlayExportElement,
	RsnapFrozenOverlayExportElementKind, RsnapOwnedRgbaRegion, RsnapStatus,
};
use rsnap_capture_core::{
	FrozenOverlayExportArrow, FrozenOverlayExportElement, FrozenOverlayExportMosaic,
	FrozenOverlayExportPen, FrozenOverlayExportPoint, FrozenOverlayExportSpotlight,
	FrozenOverlayExportSpotlightStyle, FrozenOverlayExportStrokeStyle, FrozenOverlayExportText,
	FrozenOverlayExportTextStyle, frozen_overlay_export,
};

/// Composites frozen-overlay annotations into a full RGBA export image through Rust.
///
/// # Safety
///
/// `rgba` must point to `rgba_len` readable bytes containing `width * height * 4`
/// row-major RGBA data. `elements` must either be null with `elements_len == 0`, or point
/// to `elements_len` readable element records whose nested point and text pointers stay
/// valid for the duration of the call. The returned buffer must be released with
/// `rsnap_owned_rgba_region_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsnap_frozen_overlay_export_render_rgba(
	width: u32,
	height: u32,
	rgba: *const u8,
	rgba_len: usize,
	selection: RsnapFloatRect,
	elements: *const RsnapFrozenOverlayExportElement,
	elements_len: usize,
	out_region: *mut RsnapOwnedRgbaRegion,
) -> RsnapStatus {
	if out_region.is_null() {
		return RsnapStatus::NullOutput;
	}

	let Some(bytes) = (unsafe { crate::rgba_bytes(rgba, rgba_len) }) else {
		return RsnapStatus::InvalidInput;
	};
	let Some(elements) = (unsafe { decode_frozen_overlay_export_elements(elements, elements_len) })
	else {
		return RsnapStatus::InvalidInput;
	};
	let Ok(image) = frozen_overlay_export::render_frozen_overlay_export_rgba(
		width,
		height,
		bytes,
		crate::decode_float_rect(selection),
		&elements,
	) else {
		return RsnapStatus::InvalidInput;
	};

	unsafe {
		ptr::write(out_region, crate::owned_region_from_raw_rgba(width, height, image.into_raw()));
	}

	RsnapStatus::Ok
}

unsafe fn decode_frozen_overlay_export_elements(
	elements: *const RsnapFrozenOverlayExportElement,
	elements_len: usize,
) -> Option<Vec<FrozenOverlayExportElement>> {
	if elements_len == 0 {
		return Some(Vec::new());
	}
	if elements.is_null() {
		return None;
	}

	let elements = unsafe { slice::from_raw_parts(elements, elements_len) };

	elements
		.iter()
		.map(|element| unsafe { decode_frozen_overlay_export_element(element) })
		.collect()
}

unsafe fn decode_frozen_overlay_export_element(
	element: &RsnapFrozenOverlayExportElement,
) -> Option<FrozenOverlayExportElement> {
	let color = decode_frozen_annotation_color(element.color);

	match element.kind {
		RsnapFrozenOverlayExportElementKind::Pen => {
			Some(FrozenOverlayExportElement::Pen(FrozenOverlayExportPen {
				points: unsafe {
					decode_frozen_overlay_points(element.points, element.points_len)
				}?,
				style: FrozenOverlayExportStrokeStyle {
					stroke_width_points: decode_f32(element.stroke_width_points)?,
					rgba: color,
				},
			}))
		},
		RsnapFrozenOverlayExportElementKind::Arrow => {
			Some(FrozenOverlayExportElement::Arrow(FrozenOverlayExportArrow {
				start: decode_frozen_overlay_point(element.start)?,
				end: decode_frozen_overlay_point(element.end)?,
				style: FrozenOverlayExportStrokeStyle {
					stroke_width_points: decode_f32(element.stroke_width_points)?,
					rgba: color,
				},
			}))
		},
		RsnapFrozenOverlayExportElementKind::Mosaic => {
			Some(FrozenOverlayExportElement::Mosaic(FrozenOverlayExportMosaic {
				rect: crate::decode_float_rect(element.rect),
			}))
		},
		RsnapFrozenOverlayExportElementKind::Spotlight => {
			Some(FrozenOverlayExportElement::Spotlight(FrozenOverlayExportSpotlight {
				rect: crate::decode_float_rect(element.rect),
				style: FrozenOverlayExportSpotlightStyle {
					border_width_points: decode_f32(element.border_width_points)?,
					border_rgba: color,
				},
			}))
		},
		RsnapFrozenOverlayExportElementKind::Text => {
			Some(FrozenOverlayExportElement::Text(FrozenOverlayExportText {
				anchor: decode_frozen_overlay_point(element.start)?,
				text: unsafe { decode_optional_utf8(element.text) }?,
				style: FrozenOverlayExportTextStyle {
					font_size_points: decode_f32(element.font_size_points)?,
					rgba: color,
				},
			}))
		},
	}
}

unsafe fn decode_frozen_overlay_points(
	points: *const RsnapFloatPoint,
	points_len: usize,
) -> Option<Vec<FrozenOverlayExportPoint>> {
	if points_len == 0 {
		return Some(Vec::new());
	}
	if points.is_null() {
		return None;
	}

	unsafe { slice::from_raw_parts(points, points_len) }
		.iter()
		.map(|point| decode_frozen_overlay_point(*point))
		.collect()
}

fn decode_frozen_overlay_point(point: RsnapFloatPoint) -> Option<FrozenOverlayExportPoint> {
	if point.x.is_finite() && point.y.is_finite() {
		Some(FrozenOverlayExportPoint::new(point.x, point.y))
	} else {
		None
	}
}

unsafe fn decode_optional_utf8(text: *const c_char) -> Option<String> {
	if text.is_null() {
		return Some(String::new());
	}

	unsafe { CStr::from_ptr(text) }.to_str().ok().map(ToOwned::to_owned)
}

fn decode_f32(value: f64) -> Option<f32> {
	(value.is_finite() && value >= f64::from(f32::MIN) && value <= f64::from(f32::MAX))
		.then_some(value as f32)
}

fn decode_frozen_annotation_color(color: RsnapFrozenAnnotationColor) -> [u8; 4] {
	match color {
		RsnapFrozenAnnotationColor::White => [255, 255, 255, 255],
		RsnapFrozenAnnotationColor::Yellow => [255, 219, 77, 255],
		RsnapFrozenAnnotationColor::Green => [92, 214, 149, 255],
		RsnapFrozenAnnotationColor::Blue => [102, 178, 255, 255],
		RsnapFrozenAnnotationColor::Red => [255, 107, 107, 255],
		RsnapFrozenAnnotationColor::Black => [24, 24, 24, 255],
	}
}
