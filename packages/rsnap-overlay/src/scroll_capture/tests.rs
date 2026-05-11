use image::Rgba;

use crate::scroll_capture::{
	self, DirectionMatch, DownwardRegistration, DownwardViewportCandidate,
	DownwardViewportCandidateSource, DownwardViewportResolution, MotionObservation,
	OverlapSearchConfig, PreviewOnlyDownwardLocalSample, ScrollDirection, ScrollFrameFingerprint,
	ScrollObserveOutcome, ScrollSession, support, test_support,
};

fn make_test_image(width: u32, rows: &[[u8; 4]]) -> image::RgbaImage {
	test_support::make_test_image(width, rows)
}

fn make_window(
	document: &[[u8; 4]],
	width: u32,
	start_row: usize,
	window_rows: usize,
) -> image::RgbaImage {
	test_support::make_window(document, width, start_row, window_rows)
}

fn make_sparse_textlike_window(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
	test_support::make_sparse_textlike_window(width, height, start_row)
}

fn make_sparse_textlike_window_with_moving_edge_scrollbar(
	width: u32,
	height: u32,
	start_row: u32,
	thumb_top: u32,
) -> image::RgbaImage {
	test_support::make_sparse_textlike_window_with_moving_edge_scrollbar(
		width, height, start_row, thumb_top,
	)
}

fn make_browser_like_window(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
	test_support::make_browser_like_window(width, height, start_row)
}

fn make_unregistered_composited_frame(width: u32, height: u32, seed: u32) -> image::RgbaImage {
	image::RgbaImage::from_fn(width, height, |x, y| {
		let mut value = seed
			.wrapping_add(x.wrapping_mul(0x045D_9F3B))
			.wrapping_add(y.wrapping_mul(0x9E37_79B9));

		value ^= value >> 16;
		value = value.wrapping_mul(0x85EB_CA6B);
		value ^= value >> 13;

		Rgba([(value & 0xff) as u8, ((value >> 8) & 0xff) as u8, ((value >> 16) & 0xff) as u8, 255])
	})
}

fn make_static_sidebar_center_frame(
	width: u32,
	height: u32,
	center_start_x: u32,
	center_width: u32,
	center_start_row: u32,
	center_seed: u32,
	center_scrolls: bool,
) -> image::RgbaImage {
	image::RgbaImage::from_fn(width, height, |x, y| {
		let in_center = x >= center_start_x && x < center_start_x.saturating_add(center_width);

		if !in_center {
			let stripe = (y % 32) as u8;

			return Rgba([
				stripe.saturating_mul(5),
				80_u8.saturating_add(stripe),
				180_u8.saturating_sub(stripe.saturating_mul(2)),
				255,
			]);
		}

		let document_row = if center_scrolls { center_start_row.saturating_add(y) } else { y };
		let mut value = center_seed
			.wrapping_add(document_row.wrapping_mul(0x9E37_79B9))
			.wrapping_add(x.wrapping_mul(0x85EB_CA6B));

		value ^= value >> 16;
		value = value.wrapping_mul(0xC2B2_AE35);
		value ^= value >> 13;

		Rgba([(value & 0xff) as u8, ((value >> 8) & 0xff) as u8, ((value >> 16) & 0xff) as u8, 255])
	})
}

fn make_codex_like_right_static_rail_frame(
	width: u32,
	height: u32,
	center_start_row: u32,
) -> image::RgbaImage {
	let left_blank_width = width / 10;
	let right_rail_width = width / 5;
	let right_rail_start = width.saturating_sub(right_rail_width);

	image::RgbaImage::from_fn(width, height, |x, y| {
		if x < left_blank_width {
			return Rgba([24, 24, 28, 255]);
		}
		if x >= right_rail_start {
			let local_x = x.saturating_sub(right_rail_start);
			let row = y / 34;
			let y_in_row = y % 34;
			let inside_block = local_x > 12
				&& local_x < right_rail_width.saturating_sub(12)
				&& (5..=25).contains(&y_in_row);
			let text_marker = inside_block && (8..=15).contains(&y_in_row) && local_x % 29 < 14;
			let base = if inside_block { 34 + ((row % 5) as u8).saturating_mul(5) } else { 25 };
			let marker = if text_marker { 48 } else { 0 };
			let value = base.saturating_add(marker);

			return Rgba([value, value, value.saturating_add(4), 255]);
		}

		let document_row = center_start_row.saturating_add(y);
		let local_x = x.saturating_sub(left_blank_width);
		let band = ((document_row / 22) % 11) as u8;
		let mut value = document_row
			.wrapping_mul(0x9E37_79B9)
			.wrapping_add(local_x.wrapping_mul(0x85EB_CA6B))
			.wrapping_add((band as u32).wrapping_mul(0x045D_9F3B));

		value ^= value >> 16;
		value = value.wrapping_mul(0xC2B2_AE35);
		value ^= value >> 13;

		Rgba([(value & 0xff) as u8, ((value >> 8) & 0xff) as u8, ((value >> 16) & 0xff) as u8, 255])
	})
}

#[cfg(target_os = "macos")]
fn make_dense_unique_scroll_frame(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
	image::RgbaImage::from_fn(width, height, |x, y| {
		let document_row = start_row.saturating_add(y);
		let mut value = document_row
			.wrapping_mul(0x9E37_79B9)
			.wrapping_add(x.wrapping_mul(0x85EB_CA6B))
			.wrapping_add(document_row.rotate_left(13) ^ x.rotate_left(7));

		value ^= value >> 16;
		value = value.wrapping_mul(0xC2B2_AE35);
		value ^= value >> 13;

		Rgba([(value & 0xff) as u8, ((value >> 8) & 0xff) as u8, ((value >> 16) & 0xff) as u8, 255])
	})
}

#[cfg(target_os = "macos")]
fn build_worker_pairwise_session(frame: image::RgbaImage) -> ScrollSession {
	ScrollSession::new(frame, 320).expect("worker pairwise test session should initialize")
}

#[cfg(target_os = "macos")]
fn growth_rows_i32(growth_rows: u32) -> i32 {
	i32::try_from(growth_rows).expect("worker pairwise growth rows should fit in i32")
}

#[cfg(target_os = "macos")]
fn worker_pairwise_growth_rows(
	previous: &image::RgbaImage,
	next: &image::RgbaImage,
	reason: &str,
) -> u32 {
	match support::classify_vision_downward_sample_motion_against(previous, next) {
		Some(matched) => matched.motion_rows,
		None => panic!("{reason}"),
	}
}

#[cfg(target_os = "macos")]
fn assert_worker_pairwise_commit(
	session: &mut ScrollSession,
	previous: &image::RgbaImage,
	next: image::RgbaImage,
	reason: &str,
) -> u32 {
	let growth_rows = worker_pairwise_growth_rows(previous, &next, reason);
	let outcome = match session.observe_worker_pairwise_vision_frame(next) {
		Ok(outcome) => outcome,
		Err(err) => panic!("{reason}: {err:#}"),
	};

	assert_eq!(
		outcome,
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows }
	);

	growth_rows
}

#[cfg(target_os = "macos")]
fn assert_worker_pairwise_successive_growth(frames: Vec<image::RgbaImage>, reason: &str) {
	let base_height = frames[0].height();
	let mut session = build_worker_pairwise_session(frames[0].clone());
	let mut expected_export_height = base_height;
	let mut expected_viewport_top_y = 0_i32;

	for window in frames.windows(2) {
		let growth_rows =
			assert_worker_pairwise_commit(&mut session, &window[0], window[1].clone(), reason);

		expected_export_height = expected_export_height.saturating_add(growth_rows);
		expected_viewport_top_y += growth_rows_i32(growth_rows);
	}

	assert_eq!(session.export_image().height(), expected_export_height);
	assert_eq!(session.current_viewport_top_y(), expected_viewport_top_y);
}

#[cfg(target_os = "macos")]
fn assert_worker_pairwise_repeat_between_steps(
	base: image::RgbaImage,
	step_one: image::RgbaImage,
	step_two: image::RgbaImage,
	first_reason: &str,
	followup_reason: &str,
) {
	let mut session = build_worker_pairwise_session(base.clone());
	let step_one_reference = step_one.clone();
	let first_growth =
		assert_worker_pairwise_commit(&mut session, &base, step_one.clone(), first_reason);
	let no_change_outcome = match session.observe_worker_pairwise_vision_frame(step_one) {
		Ok(outcome) => outcome,
		Err(err) => panic!("{first_reason}: {err:#}"),
	};

	assert!(matches!(
		no_change_outcome,
		ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
	));

	let followup_growth = assert_worker_pairwise_commit(
		&mut session,
		&step_one_reference,
		step_two.clone(),
		followup_reason,
	);

	assert_eq!(session.export_image().height(), base.height() + first_growth + followup_growth);
	assert_eq!(session.current_viewport_top_y(), growth_rows_i32(first_growth + followup_growth));
}

#[cfg(target_os = "macos")]
fn assert_worker_pairwise_blocked_overshot_does_not_commit_tail(
	base: image::RgbaImage,
	blocked: image::RgbaImage,
	followup: image::RgbaImage,
	reason: &str,
) {
	let mut session = build_worker_pairwise_session(base.clone());
	let no_change_outcome = match session.observe_worker_pairwise_vision_frame(blocked.clone()) {
		Ok(outcome) => outcome,
		Err(err) => panic!("{reason}: {err:#}"),
	};

	assert!(matches!(
		no_change_outcome,
		ScrollObserveOutcome::NoChange | ScrollObserveOutcome::PreviewUpdated
	));
	assert_eq!(session.export_image().height(), base.height());
	assert_eq!(session.current_viewport_top_y(), 0);

	let followup_outcome = match session.observe_worker_pairwise_vision_frame(followup.clone()) {
		Ok(outcome) => outcome,
		Err(err) => panic!("{reason}: {err:#}"),
	};

	assert!(
		!matches!(followup_outcome, ScrollObserveOutcome::Committed { .. }),
		"{reason}: blocked overshot followup must not commit tail, got {followup_outcome:?}"
	);
	assert_eq!(session.export_image().height(), base.height());
	assert_eq!(session.current_viewport_top_y(), 0);
}

#[test]
fn overlap_detection_prefers_largest_matching_suffix() {
	let previous = make_test_image(
		5,
		&[
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
		],
	);
	let next = make_test_image(
		5,
		&[[40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255], [70, 0, 0, 255], [80, 0, 0, 255]],
	);
	let overlap = scroll_capture::detect_vertical_overlap(
		&previous,
		&next,
		OverlapSearchConfig { min_overlap_rows: 1, ..Default::default() },
	);

	assert!(overlap.matched);
	assert_eq!(overlap.rows, 3);
}

#[test]
fn fingerprint_wrapper_returns_zero_delta_for_identical_images() {
	let image = image::RgbaImage::from_pixel(12, 12, Rgba([9, 8, 7, 255]));
	let left = scroll_capture::scroll_capture_fingerprint(&image);
	let right = scroll_capture::scroll_capture_fingerprint(&image);

	assert_eq!(scroll_capture::scroll_capture_fingerprint_delta(&left, &right), 0);
}

#[test]
fn fingerprint_struct_distance_detects_changed_image() {
	let base = image::RgbaImage::from_pixel(12, 12, Rgba([9, 8, 7, 255]));
	let changed = image::RgbaImage::from_pixel(12, 12, Rgba([30, 8, 7, 255]));
	let left = ScrollFrameFingerprint::from_image(&base);
	let right = ScrollFrameFingerprint::from_image(&changed);

	assert!(left.distance(&right) > 0);
}

#[test]
fn session_commits_downward_growth_on_first_matching_sample() {
	let base = make_test_image(
		3,
		&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
	);
	let moved = make_test_image(
		3,
		&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
	);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();
	let outcome = session.observe_downward_sample(moved).unwrap();

	assert_eq!(
		outcome,
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(session.export_image().height(), 6);
	assert_eq!(session.export_image().get_pixel(0, 5), &Rgba([60, 0, 0, 255]));
}

#[test]
fn session_fails_closed_on_direction_ambiguous_periodic_shift() {
	let document = (0..96)
		.map(|row| if row % 2 == 0 { [16, 120, 220, 255] } else { [230, 90, 24, 255] })
		.collect::<Vec<_>>();
	let base = make_window(&document, 6, 0, 48);
	let moved = make_window(&document, 6, 1, 48);
	let mut session = ScrollSession::new(base, 320).unwrap();
	let outcome = session.observe_downward_sample(moved).unwrap();
	let telemetry = session.commit_telemetry();

	assert!(!matches!(outcome, ScrollObserveOutcome::Committed { .. }));
	assert_eq!(session.export_image().height(), 48);
	assert_eq!(telemetry.observed_sample_registration_reason, Some("direction_ambiguous"));
}

#[test]
fn session_tracks_large_slowdown_after_first_growth() {
	let mut session = ScrollSession::new(make_sparse_textlike_window(256, 240, 0), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_sparse_textlike_window(256, 240, 114)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 114 }
	);

	let outcome =
		session.observe_downward_sample(make_sparse_textlike_window(256, 240, 180)).unwrap();

	assert_eq!(
		outcome,
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 66 }
	);

	let outcome =
		session.observe_downward_sample(make_sparse_textlike_window(256, 240, 314)).unwrap();

	assert_eq!(
		outcome,
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 134 }
	);
	assert_eq!(session.export_image().height(), 554);
	assert_eq!(session.current_viewport_top_y(), 314);
}

#[test]
fn burst_motion_hint_does_not_override_underconsumed_visual_authority() {
	let mut session = ScrollSession::new(make_sparse_textlike_window(256, 240, 0), 320).unwrap();

	assert_eq!(
		session
			.observe_downward_growth_to_viewport(
				make_sparse_textlike_window(256, 240, 16),
				16,
				true,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows: 16 }),
				"test_tiny_smooth_scroll_commit",
			)
			.unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 16 }
	);

	for start_row in [96, 112, 128, 144] {
		let _ = session
			.observe_downward_sample_with_motion_hint_and_burst(
				make_sparse_textlike_window(256, 240, start_row),
				Some(224),
				true,
			)
			.unwrap();
	}

	let telemetry = session.commit_telemetry();

	assert_eq!(telemetry.sample_eval_effective_motion_rows_hint, Some(224));
	assert_eq!(session.current_viewport_top_y(), 16);
	assert_eq!(session.export_image().height(), 256);
}

#[test]
fn initial_smooth_scroll_burst_can_commit_strong_match_beyond_underreported_hint() {
	let document = (0_u32..1_024)
		.map(|row| {
			let mut value = row.wrapping_mul(0x9E37_79B9);

			value ^= value >> 16;
			value = value.wrapping_mul(0x85EB_CA6B);
			value ^= value >> 13;

			[(value & 0xff) as u8, ((value >> 8) & 0xff) as u8, ((value >> 16) & 0xff) as u8, 255]
		})
		.collect::<Vec<_>>();
	let mut session = ScrollSession::new(make_window(&document, 12, 0, 640), 320).unwrap();

	assert_eq!(
		session
			.observe_downward_sample_with_motion_hint_and_burst(
				make_window(&document, 12, 300, 640),
				Some(42),
				true,
			)
			.unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 300 }
	);
	assert_eq!(session.current_viewport_top_y(), 300);
}

#[test]
fn transient_burst_input_hint_does_not_commit_without_visual_match() {
	let mut session = ScrollSession::new(make_sparse_textlike_window(256, 240, 0), 320).unwrap();

	assert_eq!(
		session
			.observe_downward_growth_to_viewport(
				make_sparse_textlike_window(256, 240, 96),
				96,
				true,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows: 96 }),
				"test_initial_committed_growth",
			)
			.unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 96 }
	);

	let composited_smooth_frame = make_unregistered_composited_frame(256, 240, 180);

	for _ in 0..2 {
		assert_eq!(
			session
				.observe_downward_sample_with_motion_hint_and_burst(
					composited_smooth_frame.clone(),
					Some(84),
					true,
				)
				.unwrap(),
			ScrollObserveOutcome::PreviewUpdated
		);
	}

	assert_eq!(
		session
			.observe_downward_sample_with_motion_hint_and_burst(
				composited_smooth_frame,
				Some(84),
				true,
			)
			.unwrap(),
		ScrollObserveOutcome::PreviewUpdated
	);

	let telemetry = session.commit_telemetry();

	assert_eq!(telemetry.last_commit_decision_source, Some("test_initial_committed_growth"));
	assert_eq!(telemetry.growth_commit_count, 1);
	assert_eq!(session.current_viewport_top_y(), 96);
	assert_eq!(session.export_image().height(), 336);
}

#[test]
fn initial_transient_burst_input_hint_waits_for_visual_match() {
	let composited_smooth_frame = make_unregistered_composited_frame(256, 240, 180);
	let mut session = ScrollSession::new(make_sparse_textlike_window(256, 240, 0), 320).unwrap();

	for _ in 0..2 {
		assert_eq!(
			session
				.observe_downward_sample_with_motion_hint_and_burst(
					composited_smooth_frame.clone(),
					Some(96),
					true,
				)
				.unwrap(),
			ScrollObserveOutcome::PreviewUpdated
		);
	}

	assert_eq!(
		session
			.observe_downward_sample_with_motion_hint_and_burst(
				composited_smooth_frame,
				Some(96),
				true,
			)
			.unwrap(),
		ScrollObserveOutcome::PreviewUpdated
	);

	let telemetry = session.commit_telemetry();

	assert_eq!(telemetry.last_commit_decision_source, None);
	assert_eq!(telemetry.growth_commit_count, 0);
	assert_eq!(session.current_viewport_top_y(), 0);
	assert_eq!(session.export_image().height(), 240);
}

#[test]
fn large_transient_burst_input_hint_waits_for_visual_match() {
	let composited_smooth_frame = make_unregistered_composited_frame(256, 640, 520);
	let mut session = ScrollSession::new(make_sparse_textlike_window(256, 640, 0), 320).unwrap();

	for _ in 0..2 {
		assert_eq!(
			session
				.observe_downward_sample_with_motion_hint_and_burst(
					composited_smooth_frame.clone(),
					Some(432),
					true,
				)
				.unwrap(),
			ScrollObserveOutcome::PreviewUpdated
		);
	}

	assert_eq!(
		session
			.observe_downward_sample_with_motion_hint_and_burst(
				composited_smooth_frame,
				Some(432),
				true,
			)
			.unwrap(),
		ScrollObserveOutcome::PreviewUpdated
	);

	let telemetry = session.commit_telemetry();

	assert_eq!(telemetry.last_commit_decision_source, None);
	assert_eq!(telemetry.growth_commit_count, 0);
	assert_eq!(session.current_viewport_top_y(), 0);
	assert_eq!(session.export_image().height(), 640);
}

#[test]
fn static_sidebars_do_not_drive_downward_stitching_without_center_match() {
	let base = make_static_sidebar_center_frame(320, 240, 145, 30, 0, 1, false);
	let unrelated_center = make_static_sidebar_center_frame(320, 240, 145, 30, 96, 2, false);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();
	let outcome = session
		.observe_downward_sample_with_motion_hint_and_burst(unrelated_center, Some(96), true)
		.unwrap();

	assert!(matches!(
		outcome,
		ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange
	));
	assert_eq!(session.current_viewport_top_y(), 0);
	assert_eq!(session.export_image(), &base);
	assert_eq!(session.commit_telemetry().growth_commit_count, 0);
}

#[test]
fn dynamic_scroll_center_does_not_stitch_when_static_sidebars_are_in_selection() {
	let base = make_static_sidebar_center_frame(320, 240, 145, 30, 0, 7, true);
	let moved = make_static_sidebar_center_frame(320, 240, 145, 30, 72, 7, true);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();
	let outcome =
		session.observe_downward_sample_with_motion_hint_and_burst(moved, Some(72), true).unwrap();

	assert!(matches!(
		outcome,
		ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange
	));
	assert_eq!(session.current_viewport_top_y(), 0);
	assert_eq!(session.export_image(), &base);
	assert_eq!(session.commit_telemetry().growth_commit_count, 0);
}

#[test]
fn pairwise_shift_estimate_rejects_narrow_dynamic_center_with_static_sidebars_present() {
	let base = make_static_sidebar_center_frame(320, 240, 145, 30, 0, 7, true);
	let moved = make_static_sidebar_center_frame(320, 240, 145, 30, 72, 7, true);

	assert_eq!(support::estimate_pairwise_downward_shift_rows(&base, &moved), None);
}

#[test]
fn pairwise_shift_estimate_rejects_wide_dynamic_center_with_static_right_rail_present() {
	let base = make_codex_like_right_static_rail_frame(640, 360, 0);
	let moved = make_codex_like_right_static_rail_frame(640, 360, 72);

	assert_eq!(support::estimate_pairwise_downward_shift_rows(&base, &moved), None);
}

#[test]
fn pairwise_shift_estimate_ignores_static_sidebars_without_center_scroll_match() {
	let base = make_static_sidebar_center_frame(320, 240, 145, 30, 0, 1, false);
	let unrelated_center = make_static_sidebar_center_frame(320, 240, 145, 30, 72, 2, false);

	assert_eq!(support::estimate_pairwise_downward_shift_rows(&base, &unrelated_center), None);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_rejects_narrow_dynamic_center_with_static_sidebars_present() {
	let base = make_static_sidebar_center_frame(320, 240, 145, 30, 0, 7, true);
	let moved = make_static_sidebar_center_frame(320, 240, 145, 30, 72, 7, true);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();

	assert_ne!(
		session.observe_worker_pairwise_vision_frame(moved).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 72 }
	);
	assert_eq!(session.current_viewport_top_y(), 0);
	assert_eq!(session.export_image(), &base);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_rejects_wide_dynamic_center_with_static_right_rail_present() {
	let base = make_codex_like_right_static_rail_frame(640, 360, 0);
	let moved = make_codex_like_right_static_rail_frame(640, 360, 72);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();

	assert_ne!(
		session.observe_worker_pairwise_vision_frame(moved).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 72 }
	);
	assert_eq!(session.current_viewport_top_y(), 0);
	assert_eq!(session.export_image(), &base);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_ignores_static_sidebars_without_center_scroll_match() {
	let base = make_static_sidebar_center_frame(320, 240, 145, 30, 0, 1, false);
	let unrelated_center = make_static_sidebar_center_frame(320, 240, 145, 30, 72, 2, false);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();

	assert_ne!(
		session.observe_worker_pairwise_vision_frame(unrelated_center).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 72 }
	);
	assert_eq!(session.current_viewport_top_y(), 0);
	assert_eq!(session.export_image(), &base);
}

#[test]
fn transient_burst_visual_match_underconsuming_large_input_hint_does_not_commit() {
	let document = (0..512_u32)
		.map(|row| {
			let value = row.wrapping_mul(37).wrapping_add(row.rotate_left(5)) as u8;

			[value, value.wrapping_mul(3), value.wrapping_add(91), 255]
		})
		.collect::<Vec<_>>();
	let mut session = ScrollSession::new(make_window(&document, 256, 0, 240), 320).unwrap();

	assert_eq!(
		session
			.observe_downward_growth_to_viewport(
				make_window(&document, 256, 96, 240),
				96,
				true,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows: 96 }),
				"test_initial_committed_growth",
			)
			.unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 96 }
	);

	session.transient_burst_search_enabled = true;
	session.transient_motion_rows_hint = Some(168);

	let candidate = DownwardViewportCandidate {
		source: DownwardViewportCandidateSource::ObservedSample,
		viewport_top_y: 97,
		motion_rows: 1,
		mean_abs_diff_x100: 0,
	};

	assert_eq!(
		session
			.block_invalid_downward_candidate(
				&make_window(&document, 256, 97, 240),
				1,
				candidate,
				true
			)
			.unwrap(),
		Some(ScrollObserveOutcome::PreviewUpdated)
	);
	assert_eq!(session.current_viewport_top_y(), 96);
	assert_eq!(session.export_image().height(), 336);
	assert_eq!(session.last_block_reason(), Some("visual_motion_underconsumed_input_hint"));
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_commits_substantial_downward_growth_with_corroboration() {
	let base = make_sparse_textlike_window(512, 640, 0);
	let moved = make_sparse_textlike_window(512, 640, 90);
	let matched = support::classify_vision_downward_sample_motion_against(&base, &moved)
		.expect("vision registration should detect the substantial downward motion");
	let mut session = ScrollSession::new(base, 320).unwrap();
	let outcome = session.observe_worker_pairwise_vision_frame(moved).unwrap();

	assert!(matched.motion_rows >= 32);
	assert_eq!(
		outcome,
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: matched.motion_rows,
		}
	);
	assert_eq!(session.export_image().height(), 640 + matched.motion_rows);
	assert_eq!(session.current_viewport_top_y(), i32::try_from(matched.motion_rows).unwrap());
}

#[test]
fn pairwise_downward_shift_estimate_matches_sparse_textlike_motion() {
	let base = make_sparse_textlike_window(512, 640, 0);
	let moved = make_sparse_textlike_window(512, 640, 58);

	assert_eq!(support::estimate_pairwise_downward_shift_rows(&base, &moved), Some(58));
}

#[test]
fn pairwise_downward_shift_estimate_matches_browser_like_motion_above_legacy_cap() {
	let base = make_browser_like_window(512, 640, 0);
	let moved = make_browser_like_window(512, 640, 320);

	assert_eq!(support::estimate_pairwise_downward_shift_rows(&base, &moved), Some(320));
}

#[test]
fn pairwise_downward_shift_estimate_tracks_successive_browser_like_steps() {
	let frames = [0_u32, 180, 360, 540, 720]
		.into_iter()
		.map(|start_row| make_browser_like_window(512, 640, start_row))
		.collect::<Vec<_>>();

	for window in frames.windows(2) {
		assert_eq!(
			support::estimate_pairwise_downward_shift_rows(&window[0], &window[1]),
			Some(180)
		);
	}
}

#[test]
fn pairwise_shift_estimate_fails_closed_when_periodic_content_does_not_visibly_change() {
	let document: Vec<[u8; 4]> = (0..256)
		.map(|row| {
			let bucket = (row % 24) as u8;

			[
				bucket.saturating_mul(7),
				255_u8.saturating_sub(bucket.saturating_mul(3)),
				bucket.saturating_mul(5),
				255,
			]
		})
		.collect();
	let base = make_window(&document, 8, 0, 96);
	let moved = make_window(&document, 8, 24, 96);

	assert_eq!(support::estimate_pairwise_downward_shift_rows(&base, &moved), None);
	assert_eq!(
		support::trusted_pairwise_downward_shift_rows_near_motion(&base, &moved, 24, 24),
		None
	);
}

#[test]
fn worker_pairwise_motion_resolution_prefers_overlap_rows_for_stitch_boundary() {
	assert_eq!(ScrollSession::resolve_worker_pairwise_motion_rows(180, Some(168)), Ok(168));
}

#[test]
fn worker_pairwise_motion_resolution_blocks_uncorroborated_or_conflicting_motion() {
	assert_eq!(
		ScrollSession::resolve_worker_pairwise_motion_rows(20, None),
		Err("worker_pairwise_missing_or_ambiguous_overlap_corroboration")
	);
	assert_eq!(
		ScrollSession::resolve_worker_pairwise_motion_rows(20, Some(0)),
		Err("worker_pairwise_zero_overlap_corroboration")
	);
	assert_eq!(
		ScrollSession::resolve_worker_pairwise_motion_rows(180, Some(220)),
		Err("worker_pairwise_vision_overlap_motion_mismatch")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_uses_latest_committed_live_frame_for_followup_growth() {
	assert_worker_pairwise_successive_growth(
		vec![
			make_sparse_textlike_window(512, 640, 0),
			make_sparse_textlike_window(512, 640, 180),
			make_sparse_textlike_window(512, 640, 360),
		],
		"pairwise registration should detect each successive sparse-textlike step",
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_handles_repeated_frame_between_growth_steps() {
	assert_worker_pairwise_repeat_between_steps(
		make_sparse_textlike_window(512, 640, 0),
		make_sparse_textlike_window(512, 640, 180),
		make_sparse_textlike_window(512, 640, 360),
		"first pairwise registration should detect downward motion",
		"followup pairwise registration should detect downward motion",
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_does_not_tail_rebase_after_blocked_overshot_frame() {
	assert_worker_pairwise_blocked_overshot_does_not_commit_tail(
		make_browser_like_window(512, 640, 0),
		make_browser_like_window(512, 640, 760),
		make_browser_like_window(512, 640, 844),
		"pairwise registration must keep the committed frontier after a blocked overshot",
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_clears_preview_local_followup_carryover_on_no_change() {
	let base = make_sparse_textlike_window(512, 640, 0);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();

	session.record_preview_only_downward_local_sample(&base, 123);

	session.pending_suppressed_huge_preview_only_local_followup = Some(DownwardViewportCandidate {
		source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
		viewport_top_y: 160,
		motion_rows: 160,
		mean_abs_diff_x100: 0,
	});
	session.pending_suppressed_huge_preview_only_local_followup_remaining_blocks = 2;
	session.pending_extreme_preview_only_local_tail_followup = Some(DownwardViewportCandidate {
		source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
		viewport_top_y: 161,
		motion_rows: 1,
		mean_abs_diff_x100: 0,
	});
	session.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 1;

	assert_eq!(
		session.observe_worker_pairwise_vision_frame(base).unwrap(),
		ScrollObserveOutcome::NoChange
	);
	assert!(session.last_preview_only_downward_local_sample.is_none());
	assert!(session.pending_suppressed_huge_preview_only_local_followup.is_none());
	assert_eq!(session.pending_suppressed_huge_preview_only_local_followup_remaining_blocks, 0);
	assert!(session.pending_extreme_preview_only_local_tail_followup.is_none());
	assert_eq!(session.pending_extreme_preview_only_local_tail_followup_remaining_blocks, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_clears_preview_local_followup_carryover_on_commit() {
	let base = make_sparse_textlike_window(512, 640, 0);
	let moved = make_sparse_textlike_window(512, 640, 180);
	let matched = support::classify_vision_downward_sample_motion_against(&base, &moved)
		.expect("pairwise registration should detect downward motion");
	let mut session = ScrollSession::new(base, 320).unwrap();

	session.record_preview_only_downward_local_sample(&moved, 180);

	session.pending_suppressed_huge_preview_only_local_followup = Some(DownwardViewportCandidate {
		source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
		viewport_top_y: 160,
		motion_rows: 160,
		mean_abs_diff_x100: 0,
	});
	session.pending_suppressed_huge_preview_only_local_followup_remaining_blocks = 2;
	session.pending_extreme_preview_only_local_tail_followup = Some(DownwardViewportCandidate {
		source: DownwardViewportCandidateSource::PreviewOnlyLocalSample,
		viewport_top_y: 161,
		motion_rows: 1,
		mean_abs_diff_x100: 0,
	});
	session.pending_extreme_preview_only_local_tail_followup_remaining_blocks = 1;

	assert_eq!(
		session.observe_worker_pairwise_vision_frame(moved).unwrap(),
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: matched.motion_rows,
		}
	);
	assert!(session.last_preview_only_downward_local_sample.is_none());
	assert!(session.pending_suppressed_huge_preview_only_local_followup.is_none());
	assert_eq!(session.pending_suppressed_huge_preview_only_local_followup_remaining_blocks, 0);
	assert!(session.pending_extreme_preview_only_local_tail_followup.is_none());
	assert_eq!(session.pending_extreme_preview_only_local_tail_followup_remaining_blocks, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_commits_successive_slowdown_steps() {
	assert_worker_pairwise_successive_growth(
		[0_u32, 180, 300, 380, 420]
			.into_iter()
			.map(|start_row| make_sparse_textlike_window(512, 640, start_row))
			.collect(),
		"pairwise registration should detect each slowdown step",
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_commits_browser_like_growth_above_legacy_cap() {
	let base = make_browser_like_window(512, 640, 0);
	let moved = make_browser_like_window(512, 640, 320);
	let matched = support::classify_vision_downward_sample_motion_against(&base, &moved)
		.expect("vision registration should detect the browser-like downward motion");
	let mut session = ScrollSession::new(base, 320).unwrap();

	assert!(matched.motion_rows > 256);
	assert_eq!(
		session.observe_worker_pairwise_vision_frame(moved).unwrap(),
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: matched.motion_rows,
		}
	);
	assert_eq!(session.export_image().height(), 640 + matched.motion_rows);
	assert_eq!(session.current_viewport_top_y(), i32::try_from(matched.motion_rows).unwrap());
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_commits_successive_browser_like_steps() {
	assert_worker_pairwise_successive_growth(
		[0_u32, 180, 360, 540, 720]
			.into_iter()
			.map(|start_row| make_browser_like_window(512, 640, start_row))
			.collect(),
		"pairwise registration should detect each browser-like step",
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_handles_repeated_browser_like_frame_between_growth_steps() {
	assert_worker_pairwise_repeat_between_steps(
		make_browser_like_window(512, 640, 0),
		make_browser_like_window(512, 640, 180),
		make_browser_like_window(512, 640, 360),
		"first browser-like step should register downward motion",
		"followup browser-like step should register downward motion",
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_catches_up_from_committed_frontier_after_reacquire_block() {
	let base = make_dense_unique_scroll_frame(512, 640, 0);
	let first = make_dense_unique_scroll_frame(512, 640, 180);
	let first_reference = first.clone();
	let catchup = make_dense_unique_scroll_frame(512, 640, 540);
	let mut session = build_worker_pairwise_session(base.clone());
	let first_growth = assert_worker_pairwise_commit(
		&mut session,
		&base,
		first,
		"initial pairwise registration should detect downward motion",
	);

	assert_eq!(
		support::trusted_pairwise_downward_shift_rows_near_motion(
			&first_reference,
			&catchup,
			360,
			24
		),
		Some(360)
	);

	session.worker_pairwise_requires_committed_reacquire = true;

	assert_eq!(session.current_viewport_top_y(), growth_rows_i32(first_growth));

	let catchup_outcome =
		session.observe_worker_pairwise_vision_frame_with_motion_hint(catchup, Some(360)).unwrap();

	assert_eq!(
		catchup_outcome,
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 360 }
	);
	assert_eq!(session.current_viewport_top_y(), 540);
	assert_eq!(session.export_image().height(), 640 + first_growth + 360);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_browser_like_followup_does_not_commit_tail_after_blocked_overshot() {
	assert_worker_pairwise_blocked_overshot_does_not_commit_tail(
		make_browser_like_window(512, 640, 0),
		make_browser_like_window(512, 640, 700),
		make_browser_like_window(512, 640, 784),
		"browser-like pairwise registration must not append only the tail after a blocked overshot",
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_blocks_rewind_recovery_until_frontier_is_reacquired() {
	let base = make_browser_like_window(512, 640, 0);
	let first = make_browser_like_window(512, 640, 100);
	let rewind = make_browser_like_window(512, 640, 60);
	let below_frontier = make_browser_like_window(512, 640, 80);
	let reacquired = make_browser_like_window(512, 640, 100);
	let beyond_frontier = make_browser_like_window(512, 640, 120);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();
	let first_growth = assert_worker_pairwise_commit(
		&mut session,
		&base,
		first,
		"initial pairwise registration should commit downward motion",
	);
	let height_after_first = session.export_image().height();

	assert_eq!(first_growth, 100);
	assert!(matches!(
		session.observe_worker_pairwise_vision_frame(rewind).unwrap(),
		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
	));
	assert_eq!(session.export_image().height(), height_after_first);
	assert_eq!(session.current_viewport_top_y(), 100);
	assert!(matches!(
		session.observe_worker_pairwise_vision_frame(below_frontier).unwrap(),
		ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
			| ScrollObserveOutcome::UnsupportedDirection { .. }
	));
	assert_eq!(session.export_image().height(), height_after_first);
	assert_eq!(session.current_viewport_top_y(), 100);
	assert!(matches!(
		session.observe_worker_pairwise_vision_frame(reacquired).unwrap(),
		ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange
	));
	assert_eq!(session.export_image().height(), height_after_first);
	assert_eq!(session.current_viewport_top_y(), 100);
	assert_eq!(
		session.observe_worker_pairwise_vision_frame(beyond_frontier).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
	);
	assert_eq!(session.export_image().height(), height_after_first + 20);
	assert_eq!(session.current_viewport_top_y(), 120);
}

#[test]
fn session_supports_multiple_downward_growth_steps() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
	];
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(session.export_image().height(), 7);
	assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
	assert_eq!(session.export_image().get_pixel(0, 6), &Rgba([70, 0, 0, 255]));
}

#[test]
fn downward_hot_path_falls_back_when_scroll_step_grows() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
		[90, 0, 0, 255],
	];
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 4, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 3 }
	);
	assert_eq!(session.export_image().height(), 9);
	assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
	assert_eq!(session.export_image().get_pixel(0, 8), &Rgba([90, 0, 0, 255]));
}

#[test]
fn session_reports_upward_motion_without_growing() {
	let base = make_test_image(
		3,
		&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
	);
	let moved = make_test_image(
		3,
		&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
	);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();
	let outcome = session.observe_downward_sample(moved).unwrap();

	assert!(matches!(
		outcome,
		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
	));
	assert_eq!(session.export_image(), &base);
}

#[test]
fn pure_upward_sequence_never_commits_growth() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
		[90, 0, 0, 255],
		[100, 0, 0, 255],
	];
	let mut session = ScrollSession::new(make_window(&document, 3, 5, 5), 320).unwrap();
	let initial_height = session.export_image().height();

	for start_row in (0..5).rev() {
		assert!(matches!(
			session.observe_downward_sample(make_window(&document, 3, start_row, 5)).unwrap(),
			ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
				| ScrollObserveOutcome::PreviewUpdated
				| ScrollObserveOutcome::NoChange
		));
		assert_eq!(session.export_image().height(), initial_height);
	}
}

#[test]
fn low_information_motion_does_not_commit_growth() {
	let base = make_test_image(
		3,
		&[[10, 0, 0, 255], [10, 0, 0, 255], [11, 0, 0, 255], [11, 0, 0, 255], [12, 0, 0, 255]],
	);
	let moved = make_test_image(
		3,
		&[[10, 0, 0, 255], [11, 0, 0, 255], [11, 0, 0, 255], [12, 0, 0, 255], [12, 0, 0, 255]],
	);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();
	let outcome = session.observe_downward_sample(moved).unwrap();

	assert!(matches!(
		outcome,
		ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
			| ScrollObserveOutcome::UnsupportedDirection { .. }
	));
	assert_eq!(session.export_image(), &base);
}

#[test]
fn session_commits_growth_with_sparse_informative_columns() {
	let base = make_sparse_textlike_window(256, 120, 0);
	let moved = make_sparse_textlike_window(256, 120, 9);
	let mut session = ScrollSession::new(base, 320).unwrap();
	let outcome = session.observe_downward_sample(moved).unwrap();

	assert_eq!(
		outcome,
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 9 }
	);
	assert_eq!(session.export_image().height(), 129);
}

#[test]
fn session_commits_growth_with_sparse_columns_and_moving_edge_scrollbar() {
	let base = make_sparse_textlike_window_with_moving_edge_scrollbar(256, 120, 0, 8);
	let moved = make_sparse_textlike_window_with_moving_edge_scrollbar(256, 120, 9, 40);
	let mut session = ScrollSession::new(base, 320).unwrap();
	let outcome = session.observe_downward_sample(moved).unwrap();

	assert_eq!(
		outcome,
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 9 }
	);
	assert_eq!(session.export_image().height(), 129);
}

#[test]
fn repeated_periodic_content_fails_closed_when_downward_registration_is_ambiguous() {
	let document: Vec<[u8; 4]> = (0..256)
		.map(|row| {
			let bucket = (row % 32) as u8;

			[
				bucket.saturating_mul(7),
				255_u8.saturating_sub(bucket.saturating_mul(3)),
				bucket.saturating_mul(5),
				255,
			]
		})
		.collect();
	let base = make_window(&document, 8, 0, 96);
	let moved = make_window(&document, 8, 24, 96);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();

	assert!(matches!(
		session.observe_downward_sample(moved).unwrap(),
		ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange
	));
	assert_eq!(session.export_image(), &base);
	assert_eq!(session.current_viewport_top_y, 0);
}

#[test]
fn sparse_textlike_small_downward_steps_eventually_append() {
	let base = make_sparse_textlike_window(256, 120, 0);
	let mut session = ScrollSession::new(base, 320).unwrap();
	let initial_height = session.export_image().height();
	let mut committed = 0_u32;

	for start_row in 1..=8 {
		if matches!(
			session
				.observe_downward_sample(make_sparse_textlike_window(256, 120, start_row))
				.unwrap(),
			ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, .. }
		) {
			committed = committed.saturating_add(1);
		}
	}

	assert!(committed > 0);
	assert!(session.export_image().height() > initial_height);
}

#[test]
fn observed_sample_requires_meaningful_overlap_before_committing_large_motion() {
	let document = (0_u16..320)
		.map(|row| {
			[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
		})
		.collect::<Vec<_>>();
	let base = make_window(&document, 3, 0, 160);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();

	session.last_motion_rows_hint = Some(128);

	let far = make_window(&document, 3, 130, 160);
	let export_before = session.export_image().clone();
	let preview_before = session.preview_image().clone();

	assert!(matches!(
		session.observe_downward_sample(far).unwrap(),
		ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange
	));
	assert_eq!(session.export_image(), &export_before);
	assert_eq!(session.preview_image(), &preview_before);
	assert_eq!(session.current_viewport_top_y, 0);
}

#[test]
fn periodic_far_downward_frame_does_not_use_full_range_fallback_after_local_miss() {
	let document = (0_u16..128)
		.map(|row| {
			let phase = (row % 40) as u8;

			[phase.saturating_mul(5), phase.saturating_mul(7), phase.saturating_mul(11), 255]
		})
		.collect::<Vec<_>>();
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 9, 48)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 9 }
	);

	let far = make_window(&document, 3, 40, 48);
	let match_eval = session.diagnose_reference_overlap_direction(
		&session.last_sample_frame,
		&far,
		ScrollDirection::Down,
		session.last_motion_rows_hint,
	);

	assert_eq!(session.last_motion_rows_hint, Some(9));
	assert!(match_eval.preferred_only_match.is_none());
	assert!(match_eval.final_match.is_none());
	assert!(!match_eval.used_full_range_fallback);

	let export_before = session.export_image().clone();
	let preview_before = session.preview_image().clone();
	let outcome = session.observe_downward_sample(far).unwrap();

	assert!(matches!(
		outcome,
		ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
			| ScrollObserveOutcome::UnsupportedDirection { .. }
	));
	assert_eq!(session.export_image(), &export_before);
	assert_eq!(session.preview_image(), &preview_before);
}

#[test]
fn committed_growth_rewrites_motion_hint_to_actual_growth_rows() {
	let document = (0_u16..160)
		.map(|row| {
			[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
		})
		.collect::<Vec<_>>();
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 20, 48)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
	);
	assert_eq!(session.last_motion_rows_hint, Some(20));
	assert_eq!(
		session
			.observe_downward_growth_to_viewport(
				make_window(&document, 3, 24, 48),
				24,
				true,
				Some(MotionObservation { direction: ScrollDirection::Down, motion_rows: 64 }),
				"test_residual_growth_rewrites_hint",
			)
			.unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 4 }
	);
	assert_eq!(session.last_motion_rows_hint, Some(4));
}

#[test]
fn hinted_downward_registration_does_not_escape_to_far_full_range_match() {
	let document = (0_u16..320)
		.map(|row| {
			[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
		})
		.collect::<Vec<_>>();
	let previous = make_window(&document, 3, 0, 160);
	let next = make_window(&document, 3, 100, 160);
	let session = ScrollSession::new(previous.clone(), 320).unwrap();

	assert!(matches!(
		session.evaluate_reference_downward_registration(&previous, &next, None, true),
		DownwardRegistration::Matched(DirectionMatch { motion_rows: 100, .. })
	));
	assert_eq!(
		session.evaluate_reference_downward_registration(&previous, &next, Some(20), true),
		DownwardRegistration::NoMatch
	);
}

#[test]
fn active_preview_helpers_stay_committed_even_with_provisional_like_session_state() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
	];
	let base = make_window(&document, 3, 0, 5);
	let latest = make_window(&document, 3, 1, 5);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();

	session.last_sample_frame = latest.clone();
	session.observed_viewport_top_y = 1;

	assert_eq!(session.preview_display_mode(), "committed");
	assert_eq!(session.preview_display_image(), session.export_image().clone());
}

#[test]
fn upward_motion_does_not_reset_downward_progress() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
	];
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert!(matches!(
		session.observe_downward_sample(make_window(&document, 3, 0, 5)).unwrap(),
		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
	));

	let resume_outcome = session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap();

	assert!(matches!(
		resume_outcome,
		ScrollObserveOutcome::NoChange
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	));
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(session.export_image().height(), 8);
	assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
	assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
}

#[test]
fn upward_input_never_commits_lower_frame_and_does_not_advance_frontier() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
	];
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);

	let height_after_first_append = session.export_image().height();

	assert!(matches!(
		session.observe_upward_sample(make_window(&document, 3, 2, 5)).unwrap(),
		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
	));
	assert_eq!(session.export_image().height(), height_after_first_append);
	assert!(matches!(
		session.observe_upward_sample(make_window(&document, 3, 2, 5)).unwrap(),
		ScrollObserveOutcome::PreviewUpdated | ScrollObserveOutcome::NoChange
	));
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
}

#[test]
fn upward_rewind_blocks_partial_downward_recovery_until_baseline() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
	];
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert!(matches!(
		session.observe_downward_sample(make_window(&document, 3, 0, 5)).unwrap(),
		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
	));

	let height_after_upward_rewind = session.export_image().height();

	assert!(matches!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::NoChange
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	));
	assert_eq!(session.export_image().height(), height_after_upward_rewind);

	let partial_resume_outcome =
		session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap();

	assert!(matches!(
		partial_resume_outcome,
		ScrollObserveOutcome::NoChange
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	));
	assert_eq!(session.export_image().height(), height_after_upward_rewind);
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
}

#[test]
fn returning_below_last_committed_viewport_does_not_duplicate_growth() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
	];
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);

	let height_before_resume = session.export_image().height();

	assert!(matches!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
	));

	let return_outcome = session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap();

	assert!(matches!(
		return_outcome,
		ScrollObserveOutcome::NoChange
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
	));
	assert_eq!(session.export_image().height(), height_before_resume);
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 3, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(session.export_image().height(), 8);
	assert_eq!(session.export_image().get_pixel(0, 0), &Rgba([10, 0, 0, 255]));
	assert_eq!(session.export_image().get_pixel(0, 7), &Rgba([80, 0, 0, 255]));
}

#[test]
fn downward_input_upward_like_frame_does_not_arm_resume_frontier_or_poison_sample() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
		[80, 0, 0, 255],
	];
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 5), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 2, 5)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);

	let sample_before = session.last_sample_frame.clone();
	let sample_fingerprint_before = session.last_sample_fingerprint.clone();
	let height_before = session.export_image().height();

	assert!(matches!(
		session.observe_downward_sample(make_window(&document, 3, 1, 5)).unwrap(),
		ScrollObserveOutcome::UnsupportedDirection { direction: ScrollDirection::Up }
			| ScrollObserveOutcome::PreviewUpdated
			| ScrollObserveOutcome::NoChange
	));
	assert_eq!(session.export_image().height(), height_before);
	assert_eq!(session.current_viewport_top_y, 2);
	assert_eq!(session.observed_viewport_top_y, 2);
	assert_eq!(session.resume_frontier_top_y, None);
	assert!(!session.resume_frontier_requires_reacquire);
	assert_eq!(session.last_sample_frame, sample_before);
	assert_eq!(session.last_sample_fingerprint, sample_fingerprint_before);
}

#[test]
fn viewport_selection_fails_closed_when_observed_and_committed_authority_conflict() {
	let observed = DownwardViewportCandidate {
		source: DownwardViewportCandidateSource::ObservedSample,
		viewport_top_y: 120,
		motion_rows: 20,
		mean_abs_diff_x100: 100,
	};
	let committed = DownwardViewportCandidate {
		source: DownwardViewportCandidateSource::CommittedKeyframe,
		viewport_top_y: 360,
		motion_rows: 260,
		mean_abs_diff_x100: 90,
	};
	let mut candidates = [observed, committed];

	assert_eq!(
		support::select_downward_viewport_candidate(&mut candidates),
		DownwardViewportResolution::Ambiguous { preferred: committed, competing: observed }
	);
}

#[test]
fn committed_keyframe_candidate_requires_meaningful_overlap() {
	let document = (0_u16..96)
		.map(|row| {
			[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
		})
		.collect::<Vec<_>>();
	let session = ScrollSession::new(make_window(&document, 3, 0, 48), 320).unwrap();
	let mut candidates = Vec::new();

	session.push_downward_viewport_candidate(
		&session.anchor_frame,
		0,
		&make_window(&document, 3, 40, 48),
		DownwardViewportCandidateSource::CommittedKeyframe,
		&mut candidates,
	);

	assert!(candidates.is_empty());
}

#[test]
fn committed_fallback_can_recover_from_an_older_recent_keyframe() {
	let mut session = ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_sparse_textlike_window(256, 120, 18)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 18 }
	);
	assert_eq!(
		session.observe_downward_sample(make_sparse_textlike_window(256, 120, 29)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 11 }
	);

	session.last_committed_frame =
		image::RgbaImage::from_pixel(256, 120, Rgba([255, 255, 255, 255]));

	let target = make_sparse_textlike_window(256, 120, 39);
	let mut candidates = Vec::new();

	session.collect_committed_downward_viewport_candidates(&target, &mut candidates);

	assert!(candidates.iter().any(|candidate| {
		candidate.source == DownwardViewportCandidateSource::CommittedKeyframe
			&& candidate.viewport_top_y == 39
	}));
}

#[test]
fn fallback_committed_candidates_ignore_older_recent_keyframes() {
	let mut session = ScrollSession::new(make_sparse_textlike_window(256, 120, 0), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_sparse_textlike_window(256, 120, 18)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 18 }
	);
	assert_eq!(
		session.observe_downward_sample(make_sparse_textlike_window(256, 120, 29)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 11 }
	);

	session.last_committed_frame =
		image::RgbaImage::from_pixel(256, 120, Rgba([255, 255, 255, 255]));

	let target = make_sparse_textlike_window(256, 120, 39);
	let mut candidates = Vec::new();

	session.collect_fallback_downward_viewport_candidates(&target, &mut candidates);

	assert!(candidates.is_empty());
}

#[test]
fn fallback_committed_growth_respects_local_continuity_budget() {
	let document = (0_u16..220)
		.map(|row| {
			[((row * 17) % 251) as u8, ((row * 47) % 251) as u8, ((row * 89) % 251) as u8, 255]
		})
		.collect::<Vec<_>>();
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 64), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(make_window(&document, 3, 20, 64)).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 20 }
	);

	session.last_motion_rows_hint = Some(2);
	session.last_preview_only_downward_local_sample = Some(PreviewOnlyDownwardLocalSample {
		frame: make_window(&document, 3, 24, 64),
		viewport_top_y: 24,
	});

	assert!(session.fallback_downward_growth_exceeds_continuity_budget(33));
	assert!(!session.fallback_downward_growth_exceeds_continuity_budget(32));
}

#[test]
fn nearby_local_candidate_wins_when_committed_is_only_modestly_better() {
	let observed = DownwardViewportCandidate {
		source: DownwardViewportCandidateSource::ObservedSample,
		viewport_top_y: 132,
		motion_rows: 12,
		mean_abs_diff_x100: 120,
	};
	let committed = DownwardViewportCandidate {
		source: DownwardViewportCandidateSource::CommittedKeyframe,
		viewport_top_y: 130,
		motion_rows: 10,
		mean_abs_diff_x100: 80,
	};
	let mut candidates = [observed, committed];

	assert_eq!(
		support::select_downward_viewport_candidate(&mut candidates),
		DownwardViewportResolution::Selected(observed)
	);
}

#[test]
fn session_preview_matches_export_after_downward_growth() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
	];
	let mut session = ScrollSession::new(make_window(&document, 3, 0, 4), 3).unwrap();
	let _ = session.observe_downward_sample(make_window(&document, 3, 1, 4)).unwrap();
	let _ = session.observe_downward_sample(make_window(&document, 3, 2, 4)).unwrap();

	assert_eq!(session.preview_image().height(), session.export_image().height());
	assert_eq!(session.preview_image().get_pixel(0, 0), session.export_image().get_pixel(0, 0));
	assert_eq!(
		session.preview_image().get_pixel(0, session.preview_image().height() - 1),
		session.export_image().get_pixel(0, session.export_image().height() - 1)
	);
}

#[test]
fn session_undo_restores_previous_stitched_image() {
	let base = make_test_image(
		3,
		&[[10, 0, 0, 255], [20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255]],
	);
	let moved = make_test_image(
		3,
		&[[20, 0, 0, 255], [30, 0, 0, 255], [40, 0, 0, 255], [50, 0, 0, 255], [60, 0, 0, 255]],
	);
	let mut session = ScrollSession::new(base.clone(), 320).unwrap();

	assert_eq!(
		session.observe_downward_sample(moved).unwrap(),
		ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 }
	);
	assert!(session.undo_last_append());
	assert_eq!(session.export_image(), &base);
}
