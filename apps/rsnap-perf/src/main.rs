#![allow(missing_docs)]

use std::{
	fs, hint,
	path::Path,
	time::{Duration, Instant},
};

use color_eyre::eyre::{Result, ensure, eyre};
use image::{Rgba, RgbaImage};
use rsnap_capture_core::{
	BgraFrameView, CaptureFrameBackgroundKind, CaptureFrameRenderImageRef, CaptureFrameRenderKind,
	CaptureFrameSourceKind, DisplayPointRect, FrozenSelectionTransformInput,
	FrozenSelectionTransformKind, RectPoints, ScrollMinimapInput,
	auto_center_margin_balance_shift_points, capture_frame_aspect_fill_crop_rect,
	capture_frame_background_plan, capture_frame_plan, capture_frame_wallpaper_png_thumbnail,
	capture_frame_wallpaper_request_plan, crop_rgba_image, detect_auto_center_content_bounds_rgba,
	encode_png_lossless_fast, frozen_mosaic_light_privacy_patch,
	frozen_selection_transform_hit_test, frozen_selection_transform_rect,
	loupe_patch_rgba_from_bgra_frame, render_capture_frame_effect, sample_rgb_from_bgra_frame,
	scroll_minimap_plan,
};
use rsnap_overlay::bench_support::{
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
	let image = build_export_fixture(1_440, 900);
	let auto_center_image =
		build_auto_center_fixture(1_440, 900, RectPoints::new(420, 240, 360, 220));
	let bgra_bytes_per_row = 640 * 4 + 16;
	let bgra_frame = build_bgra_fixture(640, 480, bgra_bytes_per_row);
	verify_export_round_trip(&image)?;
	verify_crop_exactness(&image)?;
	verify_mosaic_patch()?;
	verify_bgra_frame_sampling(&bgra_frame, bgra_bytes_per_row)?;
	verify_capture_frame_plan()?;
	verify_scroll_minimap_plan()?;
	verify_frozen_selection_transform()?;
	verify_auto_center_content_bounds(&auto_center_image)?;
	let wallpaper_fixture = write_wallpaper_fixture_png()?;
	verify_wallpaper_png_thumbnail(&wallpaper_fixture)?;

	results.push(time_case(
		"export_png_lossless_fast_1440x900",
		4,
		Duration::from_millis(900),
		|| {
			let png = encode_png_lossless_fast(&image)?;

			Ok(checksum_bytes(&png))
		},
	)?);

	results.push(time_case("crop_rgba_960x540", 200, Duration::from_millis(900), || {
		let crop = crop_rgba_image(&image, RectPoints::new(240, 160, 960, 540))
			.ok_or_else(|| eyre!("export crop performance fixture is invalid"))?;

		Ok(checksum_bytes(crop.as_raw()))
	})?);

	results.push(time_case(
		"frozen_mosaic_light_privacy_patch_960x540",
		1_000,
		Duration::from_millis(120),
		|| {
			let patch = frozen_mosaic_light_privacy_patch(
				1_440,
				900,
				DisplayPointRect::new(240.5, 160.25, 960.0, 540.0),
			)
			.ok_or_else(|| eyre!("mosaic patch performance fixture is invalid"))?;

			Ok(checksum_bytes(patch.as_raw()))
		},
	)?);

	run_bgra_frame_perf_case(results, &bgra_frame, bgra_bytes_per_row)?;

	run_capture_frame_perf_case(results)?;

	run_scroll_minimap_perf_case(results)?;
	run_frozen_selection_transform_perf_case(results)?;

	results.push(time_case(
		"auto_center_content_bounds_rgba_1440x900",
		50,
		Duration::from_millis(900),
		|| {
			let bounds = detect_auto_center_content_bounds_rgba(
				auto_center_image.width(),
				auto_center_image.height(),
				auto_center_image.as_raw(),
			)
			.map_err(|error| eyre!("auto-center performance fixture is invalid: {error:?}"))?
			.ok_or_else(|| eyre!("auto-center performance fixture did not detect content"))?;
			let shift_x = auto_center_margin_balance_shift_points(
				f64::from(bounds.x),
				f64::from(bounds.width),
				f64::from(auto_center_image.width()),
				720.0,
			);
			let shift_y = auto_center_margin_balance_shift_points(
				f64::from(bounds.y),
				f64::from(bounds.height),
				f64::from(auto_center_image.height()),
				450.0,
			);

			Ok(checksum_f64s(&[
				f64::from(bounds.x),
				f64::from(bounds.y),
				f64::from(bounds.width),
				f64::from(bounds.height),
				shift_x,
				shift_y,
			]))
		},
	)?);

	results.push(time_case(
		"wallpaper_png_thumbnail_stream_lanczos_512x288_to_128",
		20,
		Duration::from_millis(500),
		|| {
			let thumbnail = capture_frame_wallpaper_png_thumbnail(&wallpaper_fixture, 128)?
				.ok_or_else(|| eyre!("wallpaper thumbnail performance fixture is invalid"))?;

			Ok(checksum_bytes(thumbnail.as_raw()))
		},
	)?);

	let _ = fs::remove_file(wallpaper_fixture);

	Ok(())
}

fn run_scroll_minimap_perf_case(results: &mut Vec<PerfCaseResult>) -> Result<()> {
	results.push(time_case(
		"scroll_minimap_plan_100x200",
		10_000,
		Duration::from_millis(60),
		|| {
			let plan = scroll_minimap_plan(scroll_minimap_fixture())
				.ok_or_else(|| eyre!("scroll minimap plan performance fixture is invalid"))?;

			Ok(checksum_f64s(&[
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
	let source_image = build_export_fixture(1_440, 900);
	let source = CaptureFrameRenderImageRef::new(
		source_image.width(),
		source_image.height(),
		source_image.as_raw(),
	)?;

	results.push(time_case(
		"capture_frame_plan_and_background_1440x900",
		10_000,
		Duration::from_millis(60),
		|| {
			let plan = capture_frame_plan(1_440, 900, 2.0, CaptureFrameSourceKind::Window)
				.ok_or_else(|| eyre!("capture frame plan performance fixture is invalid"))?;
			let crop = capture_frame_aspect_fill_crop_rect(
				2_400,
				1_600,
				plan.canvas_width,
				plan.canvas_height,
			)
			.ok_or_else(|| eyre!("capture frame aspect-fill performance fixture is invalid"))?;
			let background =
				capture_frame_background_plan(CaptureFrameBackgroundKind::SystemWallpaper);
			let wallpaper_request = capture_frame_wallpaper_request_plan(
				CaptureFrameBackgroundKind::SystemWallpaper,
				plan.canvas_width,
				plan.canvas_height,
			)
			.ok_or_else(|| {
				eyre!("capture frame wallpaper request performance fixture is invalid")
			})?;

			Ok(checksum_f64s(&[
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

	results.push(time_case(
		"capture_frame_render_rgba_1440x900",
		4,
		Duration::from_millis(1_200),
		|| {
			let rendered = render_capture_frame_effect(
				source,
				CaptureFrameBackgroundKind::Aurora,
				2.0,
				CaptureFrameSourceKind::Window,
				CaptureFrameRenderKind::FramedCapture,
				None,
			)?
			.ok_or_else(|| eyre!("capture frame render performance fixture is invalid"))?;

			Ok(checksum_bytes(rendered.as_raw()))
		},
	)?);

	Ok(())
}

fn run_bgra_frame_perf_case(
	results: &mut Vec<PerfCaseResult>,
	bgra_frame: &[u8],
	bgra_bytes_per_row: usize,
) -> Result<()> {
	results.push(time_case(
		"bgra_loupe_patch_rgba_64x64",
		4_000,
		Duration::from_millis(120),
		|| {
			let patch = loupe_patch_rgba_from_bgra_frame(
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
			.ok_or_else(|| eyre!("BGRA loupe patch performance fixture is invalid"))?;

			Ok(checksum_bytes(patch.as_raw()))
		},
	)?);

	Ok(())
}

fn run_frozen_selection_transform_perf_case(results: &mut Vec<PerfCaseResult>) -> Result<()> {
	results.push(time_case(
		"frozen_selection_transform_rect",
		10_000,
		Duration::from_millis(60),
		|| {
			let rect = frozen_selection_transform_rect(selection_transform_fixture())
				.ok_or_else(|| eyre!("selection transform performance fixture is invalid"))?;

			Ok(checksum_f64s(&[rect.x, rect.y, rect.width, rect.height]))
		},
	)?);

	Ok(())
}

fn run_scroll_capture_cases(results: &mut Vec<PerfCaseResult>) -> Result<()> {
	for scenario in ScrollCaptureBenchScenario::ALL {
		let harness = ScrollCaptureBenchHarness::new(scenario);
		let name = scenario.as_str();

		verify_scroll_fingerprint(scenario, harness.run_fingerprint())?;
		results.push(time_case(
			format!("scroll_capture_fingerprint_{name}"),
			500,
			Duration::from_millis(250),
			|| {
				let metrics = harness.run_fingerprint();

				Ok(u64::from(metrics.checksum).wrapping_add(metrics.byte_len as u64))
			},
		)?);

		verify_scroll_overlap(scenario, harness.run_overlap_match())?;
		results.push(time_case(
			format!("scroll_capture_overlap_match_{name}"),
			120,
			Duration::from_millis(900),
			|| {
				let metrics = harness.run_overlap_match();

				Ok(scroll_overlap_checksum(metrics))
			},
		)?);

		verify_scroll_session(scenario, harness.run_session_commit())?;
		results.push(time_case(
			format!("scroll_capture_session_commit_{name}"),
			80,
			Duration::from_millis(1_800),
			|| {
				let metrics = harness.run_session_commit();

				Ok(scroll_session_checksum(metrics))
			},
		)?);
	}

	Ok(())
}

fn verify_export_round_trip(image: &RgbaImage) -> Result<()> {
	let png = encode_png_lossless_fast(image)?;
	let decoded = image::load_from_memory(&png)
		.map_err(|error| eyre!("failed to decode lossless PNG fixture: {error}"))?
		.into_rgba8();

	ensure!(
		decoded.dimensions() == image.dimensions(),
		"lossless PNG round trip changed dimensions"
	);
	ensure!(decoded.as_raw() == image.as_raw(), "lossless PNG round trip changed pixels");

	Ok(())
}

fn verify_crop_exactness(image: &RgbaImage) -> Result<()> {
	let rect = RectPoints::new(240, 160, 960, 540);
	let crop =
		crop_rgba_image(image, rect).ok_or_else(|| eyre!("export crop fixture is invalid"))?;

	ensure!(crop.dimensions() == (rect.width, rect.height), "crop changed dimensions");
	ensure!(crop.get_pixel(0, 0) == image.get_pixel(rect.x, rect.y), "crop origin mismatch");
	ensure!(
		crop.get_pixel(rect.width - 1, rect.height - 1)
			== image.get_pixel(rect.x + rect.width - 1, rect.y + rect.height - 1),
		"crop tail pixel mismatch"
	);

	Ok(())
}

fn verify_mosaic_patch() -> Result<()> {
	let patch =
		frozen_mosaic_light_privacy_patch(100, 80, DisplayPointRect::new(4.2, 9.1, 28.4, 21.0))
			.ok_or_else(|| eyre!("mosaic patch fixture is invalid"))?;

	ensure!(patch.dimensions() == (3, 3), "mosaic patch dimensions changed");
	ensure!(
		patch.as_raw()[..12] == [211, 211, 211, 255, 205, 205, 205, 255, 202, 201, 199, 255],
		"mosaic patch seeded color bytes changed"
	);

	Ok(())
}

fn verify_bgra_frame_sampling(bgra: &[u8], bytes_per_row: usize) -> Result<()> {
	let frame = BgraFrameView { width: 640, height: 480, bytes_per_row, bytes: bgra };
	let rgb = sample_rgb_from_bgra_frame(
		frame,
		DisplayPointRect::new(0.0, 0.0, 640.0, 480.0),
		17.2,
		479.5,
	)
	.ok_or_else(|| eyre!("BGRA RGB fixture is invalid"))?;
	ensure!(rgb.r == 27 && rgb.g == 37 && rgb.b == 47, "BGRA RGB sample changed");
	let patch = loupe_patch_rgba_from_bgra_frame(
		frame,
		DisplayPointRect::new(0.0, 0.0, 640.0, 480.0),
		0.0,
		479.0,
		3,
	)
	.ok_or_else(|| eyre!("BGRA loupe fixture is invalid"))?;
	ensure!(patch.dimensions() == (3, 3), "BGRA loupe dimensions changed");
	ensure!(patch.as_raw()[..8] == [10, 20, 30, 200, 10, 20, 30, 200], "BGRA loupe bytes changed");

	Ok(())
}

fn verify_capture_frame_plan() -> Result<()> {
	let plan = capture_frame_plan(320, 180, 2.0, CaptureFrameSourceKind::Window)
		.ok_or_else(|| eyre!("capture frame plan fixture is invalid"))?;
	ensure!(plan.canvas_width == 416.0, "capture frame canvas width changed");
	ensure!(plan.canvas_height == 276.0, "capture frame canvas height changed");
	ensure!(
		plan.image_rect == DisplayPointRect::new(48.0, 48.0, 320.0, 180.0),
		"capture frame image rect changed"
	);
	ensure!(plan.corner_radius == 9.9, "capture frame corner radius changed");

	let crop = capture_frame_aspect_fill_crop_rect(1600, 900, 1000.0, 1000.0)
		.ok_or_else(|| eyre!("capture frame aspect-fill fixture is invalid"))?;
	ensure!(
		crop == DisplayPointRect::new(350.0, 0.0, 900.0, 900.0),
		"capture frame aspect-fill crop changed"
	);

	let background = capture_frame_background_plan(CaptureFrameBackgroundKind::SystemWallpaper);
	ensure!(background.prefers_wallpaper, "capture frame wallpaper flag changed");
	ensure!(background.wallpaper_overlay_alpha == 0.10, "capture frame wallpaper overlay changed");
	ensure!(
		background.colors[2].red == 0.95 && background.locations == [0.0, 0.54, 1.0],
		"capture frame background gradient changed"
	);
	let wallpaper_request = capture_frame_wallpaper_request_plan(
		CaptureFrameBackgroundKind::SystemWallpaper,
		1535.2,
		996.0,
	)
	.ok_or_else(|| eyre!("capture frame wallpaper request fixture is invalid"))?;
	ensure!(wallpaper_request.target_pixel_size == 1536, "capture frame wallpaper target changed");
	ensure!(wallpaper_request.overlay_alpha == 0.10, "capture frame wallpaper overlay changed");

	let source_rgba = vec![255; 4 * 2 * 4];
	let source = CaptureFrameRenderImageRef::new(4, 2, &source_rgba)?;
	let rendered = render_capture_frame_effect(
		source,
		CaptureFrameBackgroundKind::Aurora,
		2.0,
		CaptureFrameSourceKind::DragRegion,
		CaptureFrameRenderKind::WindowSnapshot,
		None,
	)?
	.ok_or_else(|| eyre!("capture frame render fixture is invalid"))?;
	ensure!(rendered.width() == 100, "capture frame render width changed");
	ensure!(rendered.height() == 98, "capture frame render height changed");
	ensure!(
		rendered.as_raw()[((48 * 100 + 48) * 4)..((48 * 100 + 49) * 4)] == [255, 255, 255, 255],
		"capture frame render source pixels changed"
	);

	Ok(())
}

fn verify_scroll_minimap_plan() -> Result<()> {
	let plan = scroll_minimap_plan(scroll_minimap_fixture())
		.ok_or_else(|| eyre!("scroll minimap plan fixture is invalid"))?;

	ensure!(
		plan.frame == DisplayPointRect::new(210.0, 54.0, 96.0, 192.0),
		"scroll minimap frame changed"
	);
	ensure!(
		plan.image_frame == DisplayPointRect::new(213.0, 57.0, 90.0, 186.0),
		"scroll minimap image frame changed"
	);
	ensure!(
		plan.viewport_frame == Some(DisplayPointRect::new(213.0, 131.4, 90.0, 93.0)),
		"scroll minimap viewport frame changed"
	);

	Ok(())
}

fn verify_frozen_selection_transform() -> Result<()> {
	let selection = DisplayPointRect::new(100.0, 80.0, 240.0, 160.0);
	let hit = frozen_selection_transform_hit_test(102.0, 238.0, selection, 12.0, 4.0)
		.ok_or_else(|| eyre!("selection transform hit fixture is invalid"))?;
	ensure!(hit == FrozenSelectionTransformKind::ResizeTopLeft, "selection hit changed");
	let rect = frozen_selection_transform_rect(selection_transform_fixture())
		.ok_or_else(|| eyre!("selection transform fixture is invalid"))?;
	ensure!(
		rect == DisplayPointRect::new(100.0, 228.0, 12.0, 12.0),
		"selection transform rect changed"
	);

	Ok(())
}

fn verify_auto_center_content_bounds(image: &RgbaImage) -> Result<()> {
	let bounds =
		detect_auto_center_content_bounds_rgba(image.width(), image.height(), image.as_raw())
			.map_err(|error| eyre!("auto-center fixture is invalid: {error:?}"))?
			.ok_or_else(|| eyre!("auto-center fixture did not detect content"))?;
	ensure!(bounds == RectPoints::new(420, 240, 360, 220), "auto-center bounds changed");
	ensure!(
		auto_center_margin_balance_shift_points(420.0, 360.0, 1_440.0, 720.0) == -60.0,
		"auto-center horizontal shift changed"
	);
	ensure!(
		auto_center_margin_balance_shift_points(240.0, 220.0, 900.0, 450.0) == -50.0,
		"auto-center vertical shift changed"
	);

	Ok(())
}

fn verify_wallpaper_png_thumbnail(path: &Path) -> Result<()> {
	let thumbnail = capture_frame_wallpaper_png_thumbnail(path, 128)?
		.ok_or_else(|| eyre!("wallpaper thumbnail fixture did not decode"))?;

	ensure!(thumbnail.width() <= 128, "wallpaper thumbnail width exceeded target");
	ensure!(thumbnail.height() <= 128, "wallpaper thumbnail height exceeded target");
	ensure!(
		thumbnail.as_raw().len() == thumbnail.width() as usize * thumbnail.height() as usize * 4,
		"wallpaper thumbnail byte length changed"
	);

	Ok(())
}

fn verify_scroll_fingerprint(
	scenario: ScrollCaptureBenchScenario,
	metrics: ScrollCaptureFingerprintMetrics,
) -> Result<()> {
	ensure!(metrics.byte_len == 768, "scroll fingerprint byte length changed");
	ensure!(metrics.checksum != 0, "scroll fingerprint checksum is empty");
	ensure!(
		metrics.checksum == expected_scroll_fingerprint_checksum(scenario),
		"scroll fingerprint checksum changed for {}: expected={} actual={}",
		scenario.as_str(),
		expected_scroll_fingerprint_checksum(scenario),
		metrics.checksum
	);

	Ok(())
}

fn verify_scroll_overlap(
	scenario: ScrollCaptureBenchScenario,
	metrics: ScrollCaptureOverlapMetrics,
) -> Result<()> {
	ensure!(metrics.matched, "scroll overlap did not match for {}", scenario.as_str());
	ensure!(
		metrics.motion_rows == expected_scroll_motion_rows(scenario),
		"scroll overlap motion changed for {}",
		scenario.as_str()
	);
	ensure!(
		metrics.overlap_rows == expected_scroll_overlap_rows(scenario),
		"scroll overlap rows changed for {}",
		scenario.as_str()
	);
	ensure!(
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
	ensure!(metrics.committed, "scroll session did not commit for {}", scenario.as_str());
	ensure!(
		metrics.growth_rows == expected_scroll_motion_rows(scenario),
		"scroll session growth changed for {}",
		scenario.as_str()
	);
	ensure!(
		metrics.export_height == expected_scroll_export_height(scenario),
		"scroll session export height changed for {}",
		scenario.as_str()
	);
	ensure!(
		metrics.preview_height == expected_scroll_preview_height(scenario),
		"scroll session preview height changed for {}",
		scenario.as_str()
	);

	Ok(())
}

fn time_case(
	name: impl Into<String>,
	iterations: u32,
	budget: Duration,
	mut run_once: impl FnMut() -> Result<u64>,
) -> Result<PerfCaseResult> {
	let started_at = Instant::now();
	let mut checksum = 0_u64;

	for _ in 0..iterations {
		checksum = checksum.wrapping_add(hint::black_box(run_once()?));
	}

	Ok(PerfCaseResult {
		name: name.into(),
		iterations,
		elapsed: started_at.elapsed(),
		budget,
		checksum,
	})
}

fn build_export_fixture(width: u32, height: u32) -> RgbaImage {
	RgbaImage::from_fn(width, height, |x, y| {
		let diagonal = x.wrapping_add(y);
		let r = pattern_byte(x.wrapping_mul(13).wrapping_add(y.wrapping_mul(7)));
		let g = pattern_byte(x.wrapping_mul(3).wrapping_add(y.wrapping_mul(17)));
		let b = pattern_byte(diagonal.wrapping_mul(11).wrapping_add((x / 5) * 19));
		let a = if (x / 32 + y / 32).is_multiple_of(7) { 220 } else { 255 };

		Rgba([r, g, b, a])
	})
}

fn write_wallpaper_fixture_png() -> Result<std::path::PathBuf> {
	let image = build_export_fixture(512, 288);
	let png = encode_png_lossless_fast(&image)?;
	let path = std::env::temp_dir()
		.join(format!("rsnap-perf-wallpaper-fixture-{}.png", std::process::id()));
	fs::write(&path, png).map_err(|error| {
		eyre!("failed to write wallpaper performance fixture {}: {error}", path.display())
	})?;

	Ok(path)
}

fn build_auto_center_fixture(width: u32, height: u32, content: RectPoints) -> RgbaImage {
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

fn build_bgra_fixture(width: u32, height: u32, bytes_per_row: usize) -> Vec<u8> {
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

fn scroll_minimap_fixture() -> ScrollMinimapInput {
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

fn selection_transform_fixture() -> FrozenSelectionTransformInput {
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

fn pattern_byte(value: u32) -> u8 {
	let reduced = value % 251;

	reduced.to_le_bytes()[0]
}

fn checksum_bytes(bytes: &[u8]) -> u64 {
	bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |acc, byte| {
		acc.wrapping_mul(0x0000_0001_0000_01b3).wrapping_add(u64::from(*byte) + 1)
	})
}

fn checksum_f64s(values: &[f64]) -> u64 {
	values.iter().fold(0xcbf2_9ce4_8422_2325_u64, |acc, value| {
		acc.wrapping_mul(0x0000_0001_0000_01b3).wrapping_add(value.to_bits())
	})
}

fn scroll_overlap_checksum(metrics: ScrollCaptureOverlapMetrics) -> u64 {
	bool_bit(metrics.matched)
		.wrapping_add(u64::from(metrics.motion_rows) << 8)
		.wrapping_add(u64::from(metrics.overlap_rows) << 24)
		.wrapping_add(u64::from(metrics.mean_abs_diff_x100) << 40)
}

fn scroll_session_checksum(metrics: ScrollCaptureSessionMetrics) -> u64 {
	bool_bit(metrics.committed)
		.wrapping_add(u64::from(metrics.growth_rows) << 8)
		.wrapping_add(u64::from(metrics.export_height) << 24)
		.wrapping_add(u64::from(metrics.preview_height) << 40)
}

fn bool_bit(value: bool) -> u64 {
	u64::from(u8::from(value))
}

fn expected_scroll_fingerprint_checksum(scenario: ScrollCaptureBenchScenario) -> u32 {
	match scenario {
		ScrollCaptureBenchScenario::Baseline => 1_186_711_576,
		ScrollCaptureBenchScenario::Wide => 996_223_489,
	}
}

fn expected_scroll_motion_rows(scenario: ScrollCaptureBenchScenario) -> u32 {
	match scenario {
		ScrollCaptureBenchScenario::Baseline => 12,
		ScrollCaptureBenchScenario::Wide => 20,
	}
}

fn expected_scroll_overlap_rows(scenario: ScrollCaptureBenchScenario) -> u32 {
	match scenario {
		ScrollCaptureBenchScenario::Baseline => 116,
		ScrollCaptureBenchScenario::Wide => 140,
	}
}

fn expected_scroll_export_height(scenario: ScrollCaptureBenchScenario) -> u32 {
	match scenario {
		ScrollCaptureBenchScenario::Baseline => 140,
		ScrollCaptureBenchScenario::Wide => 180,
	}
}

fn expected_scroll_preview_height(scenario: ScrollCaptureBenchScenario) -> u32 {
	expected_scroll_export_height(scenario)
}

struct PerfCaseResult {
	name: String,
	iterations: u32,
	elapsed: Duration,
	budget: Duration,
	checksum: u64,
}
impl PerfCaseResult {
	fn print(&self) {
		println!(
			"[perf] {} iterations={} elapsed={} budget={} checksum={:#018x}",
			self.name,
			self.iterations,
			format_duration(self.elapsed),
			format_duration(self.budget),
			self.checksum
		);
	}

	fn require_budget(&self) -> Result<()> {
		ensure!(
			self.elapsed <= self.budget,
			"performance case {} exceeded budget: elapsed={} budget={}",
			self.name,
			format_duration(self.elapsed),
			format_duration(self.budget)
		);

		Ok(())
	}
}

fn format_duration(duration: Duration) -> String {
	let micros = duration.as_micros();
	let millis = micros / 1_000;
	let fractional = micros % 1_000;

	format!("{millis}.{fractional:03}ms")
}
