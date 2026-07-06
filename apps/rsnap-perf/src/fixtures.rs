#![allow(missing_docs)]

use std::env;
use std::path::PathBuf;
use std::process;
use std::{fs, path::Path};

use color_eyre::eyre::{self, Result};
use image::{Rgba, RgbaImage};

use rsnap_capture_core::{
	DisplayPointRect, FrozenSelectionTransformInput, FrozenSelectionTransformKind, RectPoints,
	ScrollMinimapInput, ToolbarItemKind,
};
use rsnap_capture_core::{
	FrozenOverlayEditColor, FrozenOverlayEditPoint, FrozenOverlayEditRect,
	FrozenOverlayEditSession, FrozenOverlayEditSpotlightStyle, FrozenOverlayEditStrokeStyle,
	FrozenOverlayEditStyle, FrozenOverlayEditTextStyle,
};
use rsnap_capture_core::{
	FrozenOverlayExportArrow, FrozenOverlayExportElement, FrozenOverlayExportMosaic,
	FrozenOverlayExportPen, FrozenOverlayExportPoint, FrozenOverlayExportSpotlight,
	FrozenOverlayExportSpotlightStyle, FrozenOverlayExportStrokeStyle, FrozenOverlayExportText,
	FrozenOverlayExportTextStyle, ScrollCaptureBenchScenario, ScrollCaptureOverlapMetrics,
	ScrollCaptureSessionMetrics,
};

pub(crate) fn build_export_fixture(width: u32, height: u32) -> RgbaImage {
	RgbaImage::from_fn(width, height, |x, y| {
		let diagonal = x.wrapping_add(y);
		let r = pattern_byte(x.wrapping_mul(13).wrapping_add(y.wrapping_mul(7)));
		let g = pattern_byte(x.wrapping_mul(3).wrapping_add(y.wrapping_mul(17)));
		let b = pattern_byte(diagonal.wrapping_mul(11).wrapping_add((x / 5) * 19));
		let a = if (x / 32 + y / 32).is_multiple_of(7) { 220 } else { 255 };

		Rgba([r, g, b, a])
	})
}

pub(crate) fn frozen_overlay_export_fixture() -> Vec<FrozenOverlayExportElement> {
	vec![
		FrozenOverlayExportElement::Mosaic(FrozenOverlayExportMosaic {
			rect: DisplayPointRect::new(180.0, 160.0, 320.0, 180.0),
		}),
		FrozenOverlayExportElement::Spotlight(FrozenOverlayExportSpotlight {
			rect: DisplayPointRect::new(760.0, 180.0, 360.0, 240.0),
			style: FrozenOverlayExportSpotlightStyle {
				border_width_points: 1.5,
				border_rgba: [255, 255, 255, 255],
			},
		}),
		FrozenOverlayExportElement::Pen(FrozenOverlayExportPen {
			points: vec![
				FrozenOverlayExportPoint::new(120.0, 120.0),
				FrozenOverlayExportPoint::new(360.0, 260.0),
				FrozenOverlayExportPoint::new(520.0, 220.0),
			],
			style: FrozenOverlayExportStrokeStyle {
				stroke_width_points: 3.0,
				rgba: [102, 178, 255, 255],
			},
		}),
		FrozenOverlayExportElement::Arrow(FrozenOverlayExportArrow {
			start: FrozenOverlayExportPoint::new(520.0, 650.0),
			end: FrozenOverlayExportPoint::new(980.0, 520.0),
			style: FrozenOverlayExportStrokeStyle {
				stroke_width_points: 4.0,
				rgba: [255, 107, 107, 255],
			},
		}),
		FrozenOverlayExportElement::Text(FrozenOverlayExportText {
			anchor: FrozenOverlayExportPoint::new(120.0, 720.0),
			text: "Rsnap".to_owned(),
			style: FrozenOverlayExportTextStyle {
				font_size_points: 20.0,
				rgba: [255, 255, 255, 255],
			},
		}),
	]
}

pub(crate) fn write_wallpaper_fixture_png() -> Result<PathBuf> {
	let image = build_export_fixture(512, 288);
	let png = rsnap_capture_core::encode_png_lossless_fast(&image)?;
	let path = env::temp_dir().join(format!("rsnap-perf-wallpaper-fixture-{}.png", process::id()));

	fs::write(&path, png).map_err(|error| {
		eyre::eyre!("failed to write wallpaper performance fixture {}: {error}", path.display())
	})?;

	Ok(path)
}

pub(crate) fn remove_wallpaper_fixture(path: impl AsRef<Path>) {
	let _ = fs::remove_file(path);
}

pub(crate) fn build_auto_center_fixture(width: u32, height: u32, content: RectPoints) -> RgbaImage {
	RgbaImage::from_fn(width, height, |x, y| {
		if x >= content.x
			&& x < content.x + content.width
			&& y >= content.y
			&& y < content.y + content.height
		{
			return Rgba([24, 32, 40, 255]);
		}

		Rgba([180, 180, 180, 255])
	})
}

pub(crate) fn build_bgra_fixture(width: u32, height: u32, bytes_per_row: usize) -> Vec<u8> {
	let mut bytes = vec![0xEE; bytes_per_row * height as usize];

	for y in 0..height {
		for x in 0..width {
			let offset = y as usize * bytes_per_row + x as usize * 4;

			bytes[offset] = pattern_byte(30 + y * 15 + x);
			bytes[offset + 1] = pattern_byte(20 + y * 10 + x);
			bytes[offset + 2] = pattern_byte(10 + y * 5 + x);
			bytes[offset + 3] = 200 + pattern_byte((x + y) % 55);
		}
	}

	bytes
}

pub(crate) fn scroll_minimap_fixture() -> ScrollMinimapInput {
	ScrollMinimapInput {
		selection: DisplayPointRect::new(100.0, 100.0, 100.0, 100.0),
		export_width: 100.0,
		export_height: 200.0,
		bounds: DisplayPointRect::new(0.0, 0.0, 500.0, 500.0),
		preferred_width: 96.0,
		minimum_width: 44.0,
		gap: 10.0,
		margin: 10.0,
		image_inset: 3.0,
		viewport_top_pixels: 20.0,
		viewport_height_pixels: 100.0,
	}
}

pub(crate) fn selection_transform_fixture() -> FrozenSelectionTransformInput {
	FrozenSelectionTransformInput {
		kind: FrozenSelectionTransformKind::ResizeBottomRight,
		initial_selection: DisplayPointRect::new(100.0, 80.0, 240.0, 160.0),
		monitor_frame: DisplayPointRect::new(0.0, 0.0, 500.0, 400.0),
		initial_pointer_x: 340.0,
		initial_pointer_y: 80.0,
		point_x: 50.0,
		point_y: 300.0,
		minimum_size: 12.0,
	}
}

pub(crate) fn run_frozen_overlay_edit_lifecycle() -> u64 {
	let selection = frozen_overlay_edit_selection();
	let style = frozen_overlay_edit_style();
	let mut session = FrozenOverlayEditSession::default();
	let mut checksum = bool_bit(session.begin(
		ToolbarItemKind::Pen,
		FrozenOverlayEditPoint::new(20.0, 30.0),
		selection,
		style,
	));

	for offset in 1..=12 {
		checksum = checksum.wrapping_add(bool_bit(session.update(
			FrozenOverlayEditPoint::new(20.0 + f64::from(offset * 4), 30.0 + f64::from(offset * 3)),
			selection,
		)));
	}

	checksum = checksum.wrapping_add(bool_bit(session.finish(selection)));
	checksum = checksum.wrapping_add(bool_bit(session.begin(
		ToolbarItemKind::Mosaic,
		FrozenOverlayEditPoint::new(90.0, 80.0),
		selection,
		style,
	)));
	checksum = checksum.wrapping_add(bool_bit(
		session.update(FrozenOverlayEditPoint::new(180.0, 150.0), selection),
	));
	checksum = checksum.wrapping_add(bool_bit(session.finish(selection)));
	checksum = checksum.wrapping_add(bool_bit(session.begin(
		ToolbarItemKind::Text,
		FrozenOverlayEditPoint::new(210.0, 110.0),
		selection,
		style,
	)));
	checksum = checksum.wrapping_add(bool_bit(session.append_text("Rsnap")));
	checksum = checksum.wrapping_add(bool_bit(session.commit_text_edit(style.text)));
	checksum = checksum.wrapping_add(bool_bit(
		session.contains_movable_annotation(FrozenOverlayEditPoint::new(212.0, 112.0)),
	));
	checksum = checksum.wrapping_add(bool_bit(session.begin(
		ToolbarItemKind::Pointer,
		FrozenOverlayEditPoint::new(212.0, 112.0),
		selection,
		style,
	)));
	checksum = checksum.wrapping_add(bool_bit(
		session.update(FrozenOverlayEditPoint::new(260.0, 160.0), selection),
	));

	let moving_snapshot = session.snapshot();

	checksum = checksum
		.wrapping_add(bool_bit(moving_snapshot.is_moving_movable_annotation))
		.wrapping_add(moving_snapshot.elements.len() as u64)
		.wrapping_add(bool_bit(moving_snapshot.preview_text.is_some()) << 8);
	checksum = checksum.wrapping_add(bool_bit(session.finish(selection)));
	checksum = checksum.wrapping_add(bool_bit(session.undo()) << 16);
	checksum = checksum.wrapping_add(bool_bit(session.redo()) << 24);

	let snapshot = session.snapshot();

	checksum
		.wrapping_add(snapshot.elements.len() as u64)
		.wrapping_add(bool_bit(snapshot.can_undo) << 32)
		.wrapping_add(bool_bit(snapshot.can_redo) << 40)
}

pub(crate) fn checksum_bytes(bytes: &[u8]) -> u64 {
	bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |acc, byte| {
		acc.wrapping_mul(0x0000_0001_0000_01b3).wrapping_add(u64::from(*byte) + 1)
	})
}

pub(crate) fn checksum_f64s(values: &[f64]) -> u64 {
	values.iter().fold(0xcbf2_9ce4_8422_2325_u64, |acc, value| {
		acc.wrapping_mul(0x0000_0001_0000_01b3).wrapping_add(value.to_bits())
	})
}

pub(crate) fn scroll_overlap_checksum(metrics: ScrollCaptureOverlapMetrics) -> u64 {
	bool_bit(metrics.matched)
		.wrapping_add(u64::from(metrics.motion_rows) << 8)
		.wrapping_add(u64::from(metrics.overlap_rows) << 24)
		.wrapping_add(u64::from(metrics.mean_abs_diff_x100) << 40)
}

pub(crate) fn scroll_session_checksum(metrics: ScrollCaptureSessionMetrics) -> u64 {
	bool_bit(metrics.committed)
		.wrapping_add(u64::from(metrics.growth_rows) << 8)
		.wrapping_add(u64::from(metrics.export_height) << 24)
		.wrapping_add(u64::from(metrics.preview_height) << 40)
}

pub(crate) fn expected_scroll_fingerprint_checksum(scenario: ScrollCaptureBenchScenario) -> u32 {
	match scenario {
		ScrollCaptureBenchScenario::Baseline => 1_186_711_576,
		ScrollCaptureBenchScenario::Wide => 996_223_489,
	}
}

pub(crate) fn expected_scroll_motion_rows(scenario: ScrollCaptureBenchScenario) -> u32 {
	match scenario {
		ScrollCaptureBenchScenario::Baseline => 12,
		ScrollCaptureBenchScenario::Wide => 20,
	}
}

pub(crate) fn expected_scroll_overlap_rows(scenario: ScrollCaptureBenchScenario) -> u32 {
	match scenario {
		ScrollCaptureBenchScenario::Baseline => 116,
		ScrollCaptureBenchScenario::Wide => 140,
	}
}

pub(crate) fn expected_scroll_export_height(scenario: ScrollCaptureBenchScenario) -> u32 {
	match scenario {
		ScrollCaptureBenchScenario::Baseline => 140,
		ScrollCaptureBenchScenario::Wide => 180,
	}
}

pub(crate) fn expected_scroll_preview_height(scenario: ScrollCaptureBenchScenario) -> u32 {
	expected_scroll_export_height(scenario)
}

fn frozen_overlay_edit_selection() -> FrozenOverlayEditRect {
	FrozenOverlayEditRect::new(10.0, 20.0, 420.0, 260.0)
}

fn frozen_overlay_edit_style() -> FrozenOverlayEditStyle {
	FrozenOverlayEditStyle {
		stroke: FrozenOverlayEditStrokeStyle {
			stroke_width_points: 3.0,
			color: FrozenOverlayEditColor::Blue,
		},
		spotlight: FrozenOverlayEditSpotlightStyle {
			border_width_points: 1.5,
			border_color: FrozenOverlayEditColor::White,
		},
		text: FrozenOverlayEditTextStyle {
			font_size_points: 16.0,
			color: FrozenOverlayEditColor::White,
		},
	}
}

fn bool_bit(value: bool) -> u64 {
	u64::from(u8::from(value))
}

fn pattern_byte(value: u32) -> u8 {
	let reduced = value % 251;

	reduced.to_le_bytes()[0]
}
