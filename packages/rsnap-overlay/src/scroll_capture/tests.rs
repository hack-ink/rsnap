use image::Rgba;

use crate::scroll_capture::support;
use crate::scroll_capture::{
	self, DirectionMatch, DownwardRegistration, DownwardViewportCandidate,
	DownwardViewportCandidateSource, DownwardViewportResolution, MotionObservation,
	OverlapSearchConfig, PreviewOnlyDownwardLocalSample, ScrollDirection, ScrollFrameFingerprint,
	ScrollObserveOutcome, ScrollSession,
};

fn make_test_image(width: u32, rows: &[[u8; 4]]) -> image::RgbaImage {
	let mut image = image::RgbaImage::new(width, rows.len() as u32);

	for (y, row) in rows.iter().enumerate() {
		for x in 0..width {
			image.put_pixel(x, y as u32, Rgba(*row));
		}
	}

	image
}

fn make_window(
	document: &[[u8; 4]],
	width: u32,
	start_row: usize,
	window_rows: usize,
) -> image::RgbaImage {
	make_test_image(width, &document[start_row..start_row + window_rows])
}

fn make_sparse_textlike_window(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
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

fn make_sparse_textlike_window_with_moving_edge_scrollbar(
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

fn make_browser_like_window(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
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

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_uses_latest_committed_live_frame_for_followup_growth() {
	let base = make_sparse_textlike_window(512, 640, 0);
	let step_one = make_sparse_textlike_window(512, 640, 180);
	let step_two = make_sparse_textlike_window(512, 640, 360);
	let first_match = support::classify_vision_downward_sample_motion_against(&base, &step_one)
		.expect("first pairwise registration should detect downward motion");
	let followup_match =
		support::classify_vision_downward_sample_motion_against(&step_one, &step_two)
			.expect("followup pairwise registration should detect downward motion");
	let mut session = ScrollSession::new(base, 320).unwrap();

	assert_eq!(
		session.observe_worker_pairwise_vision_frame(step_one).unwrap(),
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: first_match.motion_rows,
		}
	);
	assert_eq!(
		session.observe_worker_pairwise_vision_frame(step_two).unwrap(),
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: followup_match.motion_rows,
		}
	);
	assert_eq!(
		session.export_image().height(),
		640 + first_match.motion_rows + followup_match.motion_rows
	);
	assert_eq!(
		session.current_viewport_top_y(),
		i32::try_from(first_match.motion_rows + followup_match.motion_rows).unwrap()
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_handles_repeated_frame_between_growth_steps() {
	let base = make_sparse_textlike_window(512, 640, 0);
	let step_one = make_sparse_textlike_window(512, 640, 180);
	let step_two = make_sparse_textlike_window(512, 640, 360);
	let first_match = support::classify_vision_downward_sample_motion_against(&base, &step_one)
		.expect("first pairwise registration should detect downward motion");
	let followup_match =
		support::classify_vision_downward_sample_motion_against(&step_one, &step_two)
			.expect("followup pairwise registration should detect downward motion");
	let mut session = ScrollSession::new(base, 320).unwrap();

	assert_eq!(
		session.observe_worker_pairwise_vision_frame(step_one.clone()).unwrap(),
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: first_match.motion_rows,
		}
	);
	assert_eq!(
		session.observe_worker_pairwise_vision_frame(step_one).unwrap(),
		ScrollObserveOutcome::NoChange
	);
	assert_eq!(
		session.observe_worker_pairwise_vision_frame(step_two).unwrap(),
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: followup_match.motion_rows,
		}
	);
	assert_eq!(
		session.export_image().height(),
		640 + first_match.motion_rows + followup_match.motion_rows
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_recovers_after_blocked_overshot_frame() {
	let base = make_browser_like_window(512, 640, 0);
	let blocked = make_browser_like_window(512, 640, 760);
	let followup = make_browser_like_window(512, 640, 844);
	let matched = support::classify_vision_downward_sample_motion_against(&blocked, &followup)
		.expect("pairwise registration should detect the followup step after the blocked overshot");
	let mut session = ScrollSession::new(base, 320).unwrap();

	assert_eq!(
		session.observe_worker_pairwise_vision_frame(blocked).unwrap(),
		ScrollObserveOutcome::NoChange
	);
	assert_eq!(session.export_image().height(), 640);
	assert_eq!(session.current_viewport_top_y(), 0);
	assert_eq!(
		session.observe_worker_pairwise_vision_frame(followup).unwrap(),
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
	let frames = [0_u32, 180, 300, 380, 420]
		.into_iter()
		.map(|start_row| make_sparse_textlike_window(512, 640, start_row))
		.collect::<Vec<_>>();
	let mut session = ScrollSession::new(frames[0].clone(), 320).unwrap();
	let mut expected_export_height = 640_u32;
	let mut expected_viewport_top_y = 0_i32;

	for window in frames.windows(2) {
		let previous = &window[0];
		let next = window[1].clone();
		let matched = support::classify_vision_downward_sample_motion_against(previous, &next)
			.expect("pairwise registration should detect each slowdown step");

		assert_eq!(
			session.observe_worker_pairwise_vision_frame(next).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: matched.motion_rows,
			}
		);

		expected_export_height = expected_export_height.saturating_add(matched.motion_rows);
		expected_viewport_top_y += i32::try_from(matched.motion_rows).unwrap();
	}

	assert_eq!(session.export_image().height(), expected_export_height);
	assert_eq!(session.current_viewport_top_y(), expected_viewport_top_y);
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
	let frames = [0_u32, 180, 360, 540, 720]
		.into_iter()
		.map(|start_row| make_browser_like_window(512, 640, start_row))
		.collect::<Vec<_>>();
	let mut session = ScrollSession::new(frames[0].clone(), 320).unwrap();
	let mut expected_export_height = 640_u32;
	let mut expected_viewport_top_y = 0_i32;

	for window in frames.windows(2) {
		let previous = &window[0];
		let next = window[1].clone();
		let matched = support::classify_vision_downward_sample_motion_against(previous, &next)
			.expect("pairwise registration should detect each browser-like step");

		assert_eq!(
			session.observe_worker_pairwise_vision_frame(next).unwrap(),
			ScrollObserveOutcome::Committed {
				direction: ScrollDirection::Down,
				growth_rows: matched.motion_rows,
			}
		);

		expected_export_height = expected_export_height.saturating_add(matched.motion_rows);
		expected_viewport_top_y += i32::try_from(matched.motion_rows).unwrap();
	}

	assert_eq!(session.export_image().height(), expected_export_height);
	assert_eq!(session.current_viewport_top_y(), expected_viewport_top_y);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_handles_repeated_browser_like_frame_between_growth_steps() {
	let base = make_browser_like_window(512, 640, 0);
	let step_one = make_browser_like_window(512, 640, 180);
	let step_two = make_browser_like_window(512, 640, 360);
	let first_match = support::classify_vision_downward_sample_motion_against(&base, &step_one)
		.expect("first browser-like step should register downward motion");
	let followup_match =
		support::classify_vision_downward_sample_motion_against(&step_one, &step_two)
			.expect("followup browser-like step should register downward motion");
	let mut session = ScrollSession::new(base, 320).unwrap();

	assert_eq!(
		session.observe_worker_pairwise_vision_frame(step_one.clone()).unwrap(),
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: first_match.motion_rows,
		}
	);
	assert_eq!(
		session.observe_worker_pairwise_vision_frame(step_one).unwrap(),
		ScrollObserveOutcome::NoChange
	);
	assert_eq!(
		session.observe_worker_pairwise_vision_frame(step_two).unwrap(),
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: followup_match.motion_rows,
		}
	);
	assert_eq!(
		session.export_image().height(),
		640 + first_match.motion_rows + followup_match.motion_rows
	);
}

#[cfg(target_os = "macos")]
#[test]
fn worker_pairwise_vision_browser_like_followup_uses_adjacent_worker_frame() {
	let base = make_browser_like_window(512, 640, 0);
	let blocked = make_browser_like_window(512, 640, 700);
	let followup = make_browser_like_window(512, 640, 784);
	let matched = support::classify_vision_downward_sample_motion_against(&blocked, &followup)
		.expect(
			"browser-like pairwise registration should use the immediately previous worker frame",
		);
	let mut session = ScrollSession::new(base, 320).unwrap();

	assert_eq!(
		session.observe_worker_pairwise_vision_frame(blocked).unwrap(),
		ScrollObserveOutcome::NoChange
	);
	assert_eq!(
		session.observe_worker_pairwise_vision_frame(followup).unwrap(),
		ScrollObserveOutcome::Committed {
			direction: ScrollDirection::Down,
			growth_rows: matched.motion_rows,
		}
	);
	assert_eq!(session.export_image().height(), 640 + matched.motion_rows);
	assert_eq!(session.current_viewport_top_y(), i32::try_from(matched.motion_rows).unwrap());
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
