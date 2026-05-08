#![allow(missing_docs)]

use std::{
	hint,
	time::{Duration, Instant},
};

use color_eyre::eyre::{Result, ensure, eyre};
use image::{Rgba, RgbaImage};
use rsnap_capture_core::{
	DisplayPointRect, RectPoints, crop_rgba_image, encode_png_lossless_fast,
	frozen_mosaic_light_privacy_patch,
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
	verify_export_round_trip(&image)?;
	verify_crop_exactness(&image)?;
	verify_mosaic_patch()?;

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

fn pattern_byte(value: u32) -> u8 {
	let reduced = value % 251;

	reduced.to_le_bytes()[0]
}

fn checksum_bytes(bytes: &[u8]) -> u64 {
	bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |acc, byte| {
		acc.wrapping_mul(0x0000_0001_0000_01b3).wrapping_add(u64::from(*byte) + 1)
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
