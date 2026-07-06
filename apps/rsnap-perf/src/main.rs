#![allow(missing_docs)]

mod fixtures;
mod measurement;

use std::{path::Path, time::Duration};

use color_eyre::eyre::{self, Result};
use image::RgbaImage;

use crate::measurement::PerfCaseResult;
use rsnap_capture_core::{
	self, BgraFrameView, CaptureFrameBackgroundKind, CaptureFrameRenderImageRef,
	CaptureFrameRenderKind, CaptureFrameSourceKind, DisplayPointRect, FrozenSelectionTransformKind,
	RectPoints, frozen_overlay_export,
};
use rsnap_capture_core::{
	ScrollCaptureBenchHarness, ScrollCaptureBenchScenario, ScrollCaptureFingerprintMetrics,
	ScrollCaptureOverlapMetrics, ScrollCaptureSessionMetrics,
};

fn main() -> Result<()> {
	color_eyre::install()?;

	let mut results = Vec::new();

	run_export_cases(&mut results)?;
	run_scroll_capture_cases(&mut results)?;

	for result in &results {
		result.print();
		result.require_budget()?;
	}

	println!("[perf] deterministic local performance sweep passed");

	Ok(())
}

fn run_export_cases(results: &mut Vec<PerfCaseResult>) -> Result<()> {
	let image = fixtures::build_export_fixture(1_440, 900);
	let auto_center_image =
		fixtures::build_auto_center_fixture(1_440, 900, RectPoints::new(420, 240, 360, 220));
	let bgra_bytes_per_row = 640 * 4 + 16;
	let bgra_frame = fixtures::build_bgra_fixture(640, 480, bgra_bytes_per_row);

	verify_export_round_trip(&image)?;
	verify_crop_exactness(&image)?;
	verify_mosaic_patch()?;
	verify_frozen_overlay_export(&image)?;
	verify_bgra_frame_sampling(&bgra_frame, bgra_bytes_per_row)?;
	verify_capture_frame_plan()?;
	verify_scroll_minimap_plan()?;
	verify_frozen_selection_transform()?;
	verify_frozen_overlay_edit_session()?;
	verify_auto_center_content_bounds(&auto_center_image)?;

	let wallpaper_fixture = fixtures::write_wallpaper_fixture_png()?;

	verify_wallpaper_png_thumbnail(&wallpaper_fixture)?;
	run_core_export_perf_cases(results, &image)?;
	run_bgra_frame_perf_case(results, &bgra_frame, bgra_bytes_per_row)?;
	run_capture_frame_perf_case(results)?;
	run_scroll_minimap_perf_case(results)?;
	run_frozen_selection_transform_perf_case(results)?;
	run_frozen_overlay_edit_perf_case(results)?;
	run_auto_center_perf_case(results, &auto_center_image)?;
	run_wallpaper_thumbnail_perf_case(results, &wallpaper_fixture)?;

	fixtures::remove_wallpaper_fixture(wallpaper_fixture);

	Ok(())
}

fn run_core_export_perf_cases(results: &mut Vec<PerfCaseResult>, image: &RgbaImage) -> Result<()> {
	results.push(measurement::time_case(
		"export_png_lossless_fast_1440x900",
		4,
		Duration::from_millis(900),
		|| {
			let png = rsnap_capture_core::encode_png_lossless_fast(image)?;

			Ok(fixtures::checksum_bytes(&png))
		},
	)?);
	results.push(measurement::time_case(
		"crop_rgba_960x540",
		200,
		Duration::from_millis(900),
		|| {
			let crop =
				rsnap_capture_core::crop_rgba_image(image, RectPoints::new(240, 160, 960, 540))
					.ok_or_else(|| eyre::eyre!("export crop performance fixture is invalid"))?;

			Ok(fixtures::checksum_bytes(crop.as_raw()))
		},
	)?);
	results.push(measurement::time_case(
		"frozen_mosaic_light_privacy_patch_960x540",
		1_000,
		Duration::from_millis(120),
		|| {
			let patch = rsnap_capture_core::frozen_mosaic_light_privacy_patch(
				1_440,
				900,
				DisplayPointRect::new(240.5, 160.25, 960.0, 540.0),
			)
			.ok_or_else(|| eyre::eyre!("mosaic patch performance fixture is invalid"))?;

			Ok(fixtures::checksum_bytes(patch.as_raw()))
		},
	)?);
	results.push(measurement::time_case(
		"frozen_overlay_export_rgba_1440x900",
		10,
		Duration::from_millis(900),
		|| {
			let rendered = frozen_overlay_export::render_frozen_overlay_export_rgba(
				image.width(),
				image.height(),
				image.as_raw(),
				DisplayPointRect::new(0.0, 0.0, 1_440.0, 900.0),
				&fixtures::frozen_overlay_export_fixture(),
			)?;

			Ok(fixtures::checksum_bytes(rendered.as_raw()))
		},
	)?);

	Ok(())
}

fn run_auto_center_perf_case(
	results: &mut Vec<PerfCaseResult>,
	auto_center_image: &RgbaImage,
) -> Result<()> {
	results.push(measurement::time_case(
		"auto_center_content_bounds_rgba_1440x900",
		50,
		Duration::from_millis(900),
		|| {
			let bounds = rsnap_capture_core::detect_auto_center_content_bounds_rgba(
				auto_center_image.width(),
				auto_center_image.height(),
				auto_center_image.as_raw(),
			)
			.map_err(|error| eyre::eyre!("auto-center performance fixture is invalid: {error:?}"))?
			.ok_or_else(|| eyre::eyre!("auto-center performance fixture did not detect content"))?;
			let shift_x = rsnap_capture_core::auto_center_margin_balance_shift_points(
				f64::from(bounds.x),
				f64::from(bounds.width),
				f64::from(auto_center_image.width()),
				720.0,
			);
			let shift_y = rsnap_capture_core::auto_center_margin_balance_shift_points(
				f64::from(bounds.y),
				f64::from(bounds.height),
				f64::from(auto_center_image.height()),
				450.0,
			);

			Ok(fixtures::checksum_f64s(&[
				f64::from(bounds.x),
				f64::from(bounds.y),
				f64::from(bounds.width),
				f64::from(bounds.height),
				shift_x,
				shift_y,
			]))
		},
	)?);

	Ok(())
}

fn run_wallpaper_thumbnail_perf_case(
	results: &mut Vec<PerfCaseResult>,
	wallpaper_fixture: &Path,
) -> Result<()> {
	results.push(measurement::time_case(
		"wallpaper_png_thumbnail_stream_lanczos_512x288_to_128",
		20,
		Duration::from_millis(500),
		|| {
			let thumbnail =
				rsnap_capture_core::capture_frame_wallpaper_png_thumbnail(wallpaper_fixture, 128)?
					.ok_or_else(|| {
						eyre::eyre!("wallpaper thumbnail performance fixture is invalid")
					})?;

			Ok(fixtures::checksum_bytes(thumbnail.as_raw()))
		},
	)?);

	Ok(())
}

fn run_scroll_minimap_perf_case(results: &mut Vec<PerfCaseResult>) -> Result<()> {
	results.push(measurement::time_case(
		"scroll_minimap_plan_100x200",
		10_000,
		Duration::from_millis(60),
		|| {
			let plan = rsnap_capture_core::scroll_minimap_plan(fixtures::scroll_minimap_fixture())
				.ok_or_else(|| eyre::eyre!("scroll minimap plan performance fixture is invalid"))?;

			Ok(fixtures::checksum_f64s(&[
				plan.frame.x,
				plan.frame.y,
				plan.frame.width,
				plan.frame.height,
				plan.image_frame.x,
				plan.image_frame.y,
				plan.image_frame.width,
				plan.image_frame.height,
				plan.viewport_frame.map_or(0.0, |rect| rect.y),
				plan.viewport_frame.map_or(0.0, |rect| rect.height),
			]))
		},
	)?);

	Ok(())
}

fn run_capture_frame_perf_case(results: &mut Vec<PerfCaseResult>) -> Result<()> {
	let source_image = fixtures::build_export_fixture(1_440, 900);
	let source = CaptureFrameRenderImageRef::new(
		source_image.width(),
		source_image.height(),
		source_image.as_raw(),
	)?;

	results.push(measurement::time_case(
		"capture_frame_plan_and_background_1440x900",
		10_000,
		Duration::from_millis(60),
		|| {
			let plan = rsnap_capture_core::capture_frame_plan(
				1_440,
				900,
				2.0,
				CaptureFrameSourceKind::Window,
			)
			.ok_or_else(|| eyre::eyre!("capture frame plan performance fixture is invalid"))?;
			let crop = rsnap_capture_core::capture_frame_aspect_fill_crop_rect(
				2_400,
				1_600,
				plan.canvas_width,
				plan.canvas_height,
			)
			.ok_or_else(|| {
				eyre::eyre!("capture frame aspect-fill performance fixture is invalid")
			})?;
			let background = rsnap_capture_core::capture_frame_background_plan(
				CaptureFrameBackgroundKind::SystemWallpaper,
			);
			let wallpaper_request = rsnap_capture_core::capture_frame_wallpaper_request_plan(
				CaptureFrameBackgroundKind::SystemWallpaper,
				plan.canvas_width,
				plan.canvas_height,
			)
			.ok_or_else(|| {
				eyre::eyre!("capture frame wallpaper request performance fixture is invalid")
			})?;

			Ok(fixtures::checksum_f64s(&[
				plan.canvas_width,
				plan.canvas_height,
				plan.image_rect.x,
				plan.corner_radius,
				plan.shadows[0].blur,
				plan.shadows[1].offset_y,
				crop.x,
				crop.y,
				crop.width,
				crop.height,
				background.colors[0].red,
				background.colors[1].green,
				background.locations[1],
				background.wallpaper_overlay_alpha,
				f64::from(wallpaper_request.target_pixel_size),
				wallpaper_request.overlay_alpha,
			]))
		},
	)?);
	results.push(measurement::time_case(
		"capture_frame_render_rgba_1440x900",
		4,
		Duration::from_millis(1_200),
		|| {
			let rendered = rsnap_capture_core::render_capture_frame_effect(
				source,
				CaptureFrameBackgroundKind::Aurora,
				2.0,
				CaptureFrameSourceKind::Window,
				CaptureFrameRenderKind::FramedCapture,
				None,
			)?
			.ok_or_else(|| eyre::eyre!("capture frame render performance fixture is invalid"))?;

			Ok(fixtures::checksum_bytes(rendered.as_raw()))
		},
	)?);

	Ok(())
}

fn run_bgra_frame_perf_case(
	results: &mut Vec<PerfCaseResult>,
	bgra_frame: &[u8],
	bgra_bytes_per_row: usize,
) -> Result<()> {
	results.push(measurement::time_case(
		"bgra_loupe_patch_rgba_64x64",
		4_000,
		Duration::from_millis(120),
		|| {
			let patch = rsnap_capture_core::loupe_patch_rgba_from_bgra_frame(
				BgraFrameView {
					width: 640,
					height: 480,
					bytes_per_row: bgra_bytes_per_row,
					bytes: bgra_frame,
				},
				DisplayPointRect::new(0.0, 0.0, 640.0, 480.0),
				24.0,
				470.0,
				64,
			)
			.ok_or_else(|| eyre::eyre!("BGRA loupe patch performance fixture is invalid"))?;

			Ok(fixtures::checksum_bytes(patch.as_raw()))
		},
	)?);

	Ok(())
}

fn run_frozen_selection_transform_perf_case(results: &mut Vec<PerfCaseResult>) -> Result<()> {
	results.push(measurement::time_case(
		"frozen_selection_transform_rect",
		10_000,
		Duration::from_millis(60),
		|| {
			let rect = rsnap_capture_core::frozen_selection_transform_rect(
				fixtures::selection_transform_fixture(),
			)
			.ok_or_else(|| eyre::eyre!("selection transform performance fixture is invalid"))?;

			Ok(fixtures::checksum_f64s(&[rect.x, rect.y, rect.width, rect.height]))
		},
	)?);

	Ok(())
}

fn run_frozen_overlay_edit_perf_case(results: &mut Vec<PerfCaseResult>) -> Result<()> {
	results.push(measurement::time_case(
		"frozen_overlay_edit_session_lifecycle",
		2_000,
		Duration::from_millis(120),
		|| Ok(fixtures::run_frozen_overlay_edit_lifecycle()),
	)?);

	Ok(())
}

fn run_scroll_capture_cases(results: &mut Vec<PerfCaseResult>) -> Result<()> {
	for scenario in ScrollCaptureBenchScenario::ALL {
		let harness = ScrollCaptureBenchHarness::new(scenario);
		let name = scenario.as_str();

		verify_scroll_fingerprint(scenario, harness.run_fingerprint())?;

		results.push(measurement::time_case(
			format!("scroll_capture_fingerprint_{name}"),
			500,
			Duration::from_millis(250),
			|| {
				let metrics = harness.run_fingerprint();

				Ok(u64::from(metrics.checksum).wrapping_add(metrics.byte_len as u64))
			},
		)?);

		verify_scroll_overlap(scenario, harness.run_overlap_match())?;

		results.push(measurement::time_case(
			format!("scroll_capture_overlap_match_{name}"),
			120,
			Duration::from_millis(900),
			|| {
				let metrics = harness.run_overlap_match();

				Ok(fixtures::scroll_overlap_checksum(metrics))
			},
		)?);

		verify_scroll_session(scenario, harness.run_session_commit())?;

		results.push(measurement::time_case(
			format!("scroll_capture_session_commit_{name}"),
			80,
			Duration::from_millis(1_800),
			|| {
				let metrics = harness.run_session_commit();

				Ok(fixtures::scroll_session_checksum(metrics))
			},
		)?);
	}

	Ok(())
}

fn verify_export_round_trip(image: &RgbaImage) -> Result<()> {
	let png = rsnap_capture_core::encode_png_lossless_fast(image)?;
	let decoded = image::load_from_memory(&png)
		.map_err(|error| eyre::eyre!("failed to decode lossless PNG fixture: {error}"))?
		.into_rgba8();

	eyre::ensure!(
		decoded.dimensions() == image.dimensions(),
		"lossless PNG round trip changed dimensions"
	);
	eyre::ensure!(decoded.as_raw() == image.as_raw(), "lossless PNG round trip changed pixels");

	Ok(())
}

fn verify_crop_exactness(image: &RgbaImage) -> Result<()> {
	let rect = RectPoints::new(240, 160, 960, 540);
	let crop = rsnap_capture_core::crop_rgba_image(image, rect)
		.ok_or_else(|| eyre::eyre!("export crop fixture is invalid"))?;

	eyre::ensure!(crop.dimensions() == (rect.width, rect.height), "crop changed dimensions");
	eyre::ensure!(crop.get_pixel(0, 0) == image.get_pixel(rect.x, rect.y), "crop origin mismatch");
	eyre::ensure!(
		crop.get_pixel(rect.width - 1, rect.height - 1)
			== image.get_pixel(rect.x + rect.width - 1, rect.y + rect.height - 1),
		"crop tail pixel mismatch"
	);

	Ok(())
}

fn verify_mosaic_patch() -> Result<()> {
	let patch = rsnap_capture_core::frozen_mosaic_light_privacy_patch(
		100,
		80,
		DisplayPointRect::new(4.2, 9.1, 28.4, 21.0),
	)
	.ok_or_else(|| eyre::eyre!("mosaic patch fixture is invalid"))?;

	eyre::ensure!(patch.dimensions() == (3, 3), "mosaic patch dimensions changed");
	eyre::ensure!(
		patch.as_raw()[..12] == [211, 211, 211, 255, 205, 205, 205, 255, 202, 201, 199, 255],
		"mosaic patch seeded color bytes changed"
	);

	Ok(())
}

fn verify_frozen_overlay_export(image: &RgbaImage) -> Result<()> {
	let rendered = frozen_overlay_export::render_frozen_overlay_export_rgba(
		image.width(),
		image.height(),
		image.as_raw(),
		DisplayPointRect::new(0.0, 0.0, 1_440.0, 900.0),
		&fixtures::frozen_overlay_export_fixture(),
	)?;

	eyre::ensure!(rendered.dimensions() == image.dimensions(), "frozen overlay dimensions changed");
	eyre::ensure!(rendered.as_raw() != image.as_raw(), "frozen overlay did not change pixels");

	Ok(())
}

fn verify_bgra_frame_sampling(bgra: &[u8], bytes_per_row: usize) -> Result<()> {
	let frame = BgraFrameView { width: 640, height: 480, bytes_per_row, bytes: bgra };
	let rgb = rsnap_capture_core::sample_rgb_from_bgra_frame(
		frame,
		DisplayPointRect::new(0.0, 0.0, 640.0, 480.0),
		17.2,
		479.5,
	)
	.ok_or_else(|| eyre::eyre!("BGRA RGB fixture is invalid"))?;

	eyre::ensure!(rgb.r == 27 && rgb.g == 37 && rgb.b == 47, "BGRA RGB sample changed");

	let patch = rsnap_capture_core::loupe_patch_rgba_from_bgra_frame(
		frame,
		DisplayPointRect::new(0.0, 0.0, 640.0, 480.0),
		0.0,
		479.0,
		3,
	)
	.ok_or_else(|| eyre::eyre!("BGRA loupe fixture is invalid"))?;

	eyre::ensure!(patch.dimensions() == (3, 3), "BGRA loupe dimensions changed");
	eyre::ensure!(
		patch.as_raw()[..8] == [10, 20, 30, 200, 10, 20, 30, 200],
		"BGRA loupe bytes changed"
	);

	Ok(())
}

fn verify_capture_frame_plan() -> Result<()> {
	let plan =
		rsnap_capture_core::capture_frame_plan(320, 180, 2.0, CaptureFrameSourceKind::Window)
			.ok_or_else(|| eyre::eyre!("capture frame plan fixture is invalid"))?;

	eyre::ensure!(plan.canvas_width == 416.0, "capture frame canvas width changed");
	eyre::ensure!(plan.canvas_height == 276.0, "capture frame canvas height changed");
	eyre::ensure!(
		plan.image_rect == DisplayPointRect::new(48.0, 48.0, 320.0, 180.0),
		"capture frame image rect changed"
	);
	eyre::ensure!(plan.corner_radius == 9.9, "capture frame corner radius changed");

	let crop =
		rsnap_capture_core::capture_frame_aspect_fill_crop_rect(1_600, 900, 1_000.0, 1_000.0)
			.ok_or_else(|| eyre::eyre!("capture frame aspect-fill fixture is invalid"))?;

	eyre::ensure!(
		crop == DisplayPointRect::new(350.0, 0.0, 900.0, 900.0),
		"capture frame aspect-fill crop changed"
	);

	let background = rsnap_capture_core::capture_frame_background_plan(
		CaptureFrameBackgroundKind::SystemWallpaper,
	);

	eyre::ensure!(background.prefers_wallpaper, "capture frame wallpaper flag changed");
	eyre::ensure!(
		background.wallpaper_overlay_alpha == 0.10,
		"capture frame wallpaper overlay changed"
	);
	eyre::ensure!(
		background.colors[2].red == 0.95 && background.locations == [0.0, 0.54, 1.0],
		"capture frame background gradient changed"
	);

	let wallpaper_request = rsnap_capture_core::capture_frame_wallpaper_request_plan(
		CaptureFrameBackgroundKind::SystemWallpaper,
		1_535.2,
		996.0,
	)
	.ok_or_else(|| eyre::eyre!("capture frame wallpaper request fixture is invalid"))?;

	eyre::ensure!(
		wallpaper_request.target_pixel_size == 1_536,
		"capture frame wallpaper target changed"
	);
	eyre::ensure!(
		wallpaper_request.overlay_alpha == 0.10,
		"capture frame wallpaper overlay changed"
	);

	let source_rgba = vec![255; 4 * 2 * 4];
	let source = CaptureFrameRenderImageRef::new(4, 2, &source_rgba)?;
	let rendered = rsnap_capture_core::render_capture_frame_effect(
		source,
		CaptureFrameBackgroundKind::Aurora,
		2.0,
		CaptureFrameSourceKind::DragRegion,
		CaptureFrameRenderKind::WindowSnapshot,
		None,
	)?
	.ok_or_else(|| eyre::eyre!("capture frame render fixture is invalid"))?;

	eyre::ensure!(rendered.width() == 100, "capture frame render width changed");
	eyre::ensure!(rendered.height() == 98, "capture frame render height changed");
	eyre::ensure!(
		rendered.as_raw()[((48 * 100 + 48) * 4)..((48 * 100 + 49) * 4)] == [255, 255, 255, 255],
		"capture frame render source pixels changed"
	);

	Ok(())
}

fn verify_scroll_minimap_plan() -> Result<()> {
	let plan = rsnap_capture_core::scroll_minimap_plan(fixtures::scroll_minimap_fixture())
		.ok_or_else(|| eyre::eyre!("scroll minimap plan fixture is invalid"))?;

	eyre::ensure!(
		plan.frame == DisplayPointRect::new(210.0, 54.0, 96.0, 192.0),
		"scroll minimap frame changed"
	);
	eyre::ensure!(
		plan.image_frame == DisplayPointRect::new(213.0, 57.0, 90.0, 186.0),
		"scroll minimap image frame changed"
	);
	eyre::ensure!(
		plan.viewport_frame == Some(DisplayPointRect::new(213.0, 131.4, 90.0, 93.0)),
		"scroll minimap viewport frame changed"
	);

	Ok(())
}

fn verify_frozen_selection_transform() -> Result<()> {
	let selection = DisplayPointRect::new(100.0, 80.0, 240.0, 160.0);
	let hit =
		rsnap_capture_core::frozen_selection_transform_hit_test(102.0, 238.0, selection, 12.0, 4.0)
			.ok_or_else(|| eyre::eyre!("selection transform hit fixture is invalid"))?;

	eyre::ensure!(hit == FrozenSelectionTransformKind::ResizeTopLeft, "selection hit changed");

	let rect = rsnap_capture_core::frozen_selection_transform_rect(
		fixtures::selection_transform_fixture(),
	)
	.ok_or_else(|| eyre::eyre!("selection transform fixture is invalid"))?;

	eyre::ensure!(
		rect == DisplayPointRect::new(100.0, 228.0, 12.0, 12.0),
		"selection transform rect changed"
	);

	Ok(())
}

fn verify_frozen_overlay_edit_session() -> Result<()> {
	let checksum = fixtures::run_frozen_overlay_edit_lifecycle();

	eyre::ensure!(checksum != 0, "frozen overlay edit lifecycle checksum is empty");

	Ok(())
}

fn verify_auto_center_content_bounds(image: &RgbaImage) -> Result<()> {
	let bounds = rsnap_capture_core::detect_auto_center_content_bounds_rgba(
		image.width(),
		image.height(),
		image.as_raw(),
	)
	.map_err(|error| eyre::eyre!("auto-center fixture is invalid: {error:?}"))?
	.ok_or_else(|| eyre::eyre!("auto-center fixture did not detect content"))?;

	eyre::ensure!(bounds == RectPoints::new(420, 240, 360, 220), "auto-center bounds changed");
	eyre::ensure!(
		rsnap_capture_core::auto_center_margin_balance_shift_points(420.0, 360.0, 1_440.0, 720.0)
			== -60.0,
		"auto-center horizontal shift changed"
	);
	eyre::ensure!(
		rsnap_capture_core::auto_center_margin_balance_shift_points(240.0, 220.0, 900.0, 450.0)
			== -50.0,
		"auto-center vertical shift changed"
	);

	Ok(())
}

fn verify_wallpaper_png_thumbnail(path: &Path) -> Result<()> {
	let thumbnail = rsnap_capture_core::capture_frame_wallpaper_png_thumbnail(path, 128)?
		.ok_or_else(|| eyre::eyre!("wallpaper thumbnail fixture did not decode"))?;

	eyre::ensure!(thumbnail.width() <= 128, "wallpaper thumbnail width exceeded target");
	eyre::ensure!(thumbnail.height() <= 128, "wallpaper thumbnail height exceeded target");
	eyre::ensure!(
		thumbnail.as_raw().len() == thumbnail.width() as usize * thumbnail.height() as usize * 4,
		"wallpaper thumbnail byte length changed"
	);

	Ok(())
}

fn verify_scroll_fingerprint(
	scenario: ScrollCaptureBenchScenario,
	metrics: ScrollCaptureFingerprintMetrics,
) -> Result<()> {
	eyre::ensure!(metrics.byte_len == 768, "scroll fingerprint byte length changed");
	eyre::ensure!(metrics.checksum != 0, "scroll fingerprint checksum is empty");
	eyre::ensure!(
		metrics.checksum == fixtures::expected_scroll_fingerprint_checksum(scenario),
		"scroll fingerprint checksum changed for {}: expected={} actual={}",
		scenario.as_str(),
		fixtures::expected_scroll_fingerprint_checksum(scenario),
		metrics.checksum
	);

	Ok(())
}

fn verify_scroll_overlap(
	scenario: ScrollCaptureBenchScenario,
	metrics: ScrollCaptureOverlapMetrics,
) -> Result<()> {
	eyre::ensure!(metrics.matched, "scroll overlap did not match for {}", scenario.as_str());
	eyre::ensure!(
		metrics.motion_rows == fixtures::expected_scroll_motion_rows(scenario),
		"scroll overlap motion changed for {}",
		scenario.as_str()
	);
	eyre::ensure!(
		metrics.overlap_rows == fixtures::expected_scroll_overlap_rows(scenario),
		"scroll overlap rows changed for {}",
		scenario.as_str()
	);
	eyre::ensure!(
		metrics.mean_abs_diff_x100 == 0,
		"scroll overlap fixture should be exact for {}",
		scenario.as_str()
	);

	Ok(())
}

fn verify_scroll_session(
	scenario: ScrollCaptureBenchScenario,
	metrics: ScrollCaptureSessionMetrics,
) -> Result<()> {
	eyre::ensure!(metrics.committed, "scroll session did not commit for {}", scenario.as_str());
	eyre::ensure!(
		metrics.growth_rows == fixtures::expected_scroll_motion_rows(scenario),
		"scroll session growth changed for {}",
		scenario.as_str()
	);
	eyre::ensure!(
		metrics.export_height == fixtures::expected_scroll_export_height(scenario),
		"scroll session export height changed for {}",
		scenario.as_str()
	);
	eyre::ensure!(
		metrics.preview_height == fixtures::expected_scroll_preview_height(scenario),
		"scroll session preview height changed for {}",
		scenario.as_str()
	);

	Ok(())
}
