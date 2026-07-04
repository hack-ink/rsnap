#![allow(
	dead_code,
	reason = "XY-113 narrows the public crate facade while leaving ScreenCaptureKit implementation cleanup to a separate follow-up lane."
)]

mod frame_store;
mod live_frame_buffer;
mod stream_config;
mod stream_facade;
mod stream_filter;
mod stream_lifecycle;
mod stream_output;
mod stream_setup;
mod stream_worker;

pub(crate) use self::stream_facade::{CursorSampleRequest, MacLiveFrameStream};

use std::time::Duration;

use objc2::rc::Retained;
use objc2_foundation::NSError;

#[cfg(test)]
use self::frame_store::SharedLatestFrame;
#[cfg(test)]
use self::live_frame_buffer::{QueuedPixelBufferFrame, SharedPixelBuffer};
#[cfg(test)]
use self::stream_config::{StreamCaptureRegion, StreamCaptureTarget};
#[cfg(test)]
use self::stream_lifecycle::{
	StreamReuseDecision, refresh_stream_requires_setup_backoff, stream_reuse_decision,
	stream_setup_backoff,
};

pub(crate) const STREAM_REGION_FRAME_MAX_AGE: Duration = Duration::from_millis(90);

const STREAM_RPC_TIMEOUT: Duration = Duration::from_secs(3);
const STREAM_SETUP_BACKOFF: Duration = Duration::from_millis(300);
const STREAM_INCOMPLETE_EXCEPTION_UPGRADE_BACKOFF: Duration = Duration::from_secs(3);
const STREAM_FRAME_QUEUE_CAPACITY: usize = 16;
const STREAM_ACTIVE_GESTURE_FORCE_REFRESH_MIN_AGE: Duration = Duration::from_millis(60);
const STREAM_REGION_FRAME_REFRESH_TIMEOUT: Duration = Duration::from_millis(180);
const STREAM_REGION_FRAME_AHEAD_WAIT_TIMEOUT: Duration = Duration::from_millis(24);
const STREAM_REGION_FRAME_REFRESH_POLL_INTERVAL: Duration = Duration::from_millis(4);
const STREAM_POST_SETUP_FRAME_GRACE: Duration = STREAM_SETUP_BACKOFF;
const STREAM_ERROR_TIMEOUT_CODE: isize = 1;
const STREAM_ERROR_NULL_CONTENT_CODE: isize = 2;
const STREAM_ERROR_RETAIN_FAILED_CODE: isize = 3;

fn stream_error(code: isize) -> Retained<NSError> {
	stream_setup::stream_error(code)
}

#[cfg(test)]
mod tests {
	use std::ptr::{self, NonNull};
	use std::time::Duration;

	use objc2_core_foundation::CFRetained;
	use objc2_core_video::{CVPixelBufferCreate, kCVPixelFormatType_32BGRA, kCVReturnSuccess};

	use crate::live_frame_stream_macos::STREAM_POST_SETUP_FRAME_GRACE;
	use crate::live_frame_stream_macos::{self, stream_config, stream_worker};

	fn test_pixel_buffer() -> live_frame_stream_macos::SharedPixelBuffer {
		let mut buffer = ptr::null_mut();
		let res = unsafe {
			CVPixelBufferCreate(
				None,
				1,
				1,
				kCVPixelFormatType_32BGRA,
				None,
				NonNull::from(&mut buffer),
			)
		};

		assert_eq!(res, kCVReturnSuccess);

		live_frame_stream_macos::SharedPixelBuffer(unsafe {
			CFRetained::from_raw(NonNull::new(buffer).expect("test pixel buffer"))
		})
	}

	#[test]
	fn with_waker_streams_preserve_self_capture_exception_window_ids() {
		let stream = live_frame_stream_macos::MacLiveFrameStream::with_self_capture_exception_window_ids_and_waker(
			vec![7, 11],
			None,
		);

		assert_eq!(stream.debug_self_capture_exception_window_ids(), &[7, 11]);
	}

	#[test]
	fn stream_reuse_decision_retries_incomplete_same_monitor_streams() {
		assert_eq!(
			live_frame_stream_macos::stream_reuse_decision(Some(7), true, 7),
			live_frame_stream_macos::StreamReuseDecision::ReuseCurrent
		);
		assert_eq!(
			live_frame_stream_macos::stream_reuse_decision(Some(7), false, 7),
			live_frame_stream_macos::StreamReuseDecision::RetryUpgradeUsingCurrent
		);
		assert_eq!(
			live_frame_stream_macos::stream_reuse_decision(Some(7), true, 9),
			live_frame_stream_macos::StreamReuseDecision::SetupFresh
		);
	}

	#[test]
	fn retry_upgrade_uses_slower_setup_backoff() {
		assert_eq!(
			live_frame_stream_macos::stream_setup_backoff(
				live_frame_stream_macos::StreamReuseDecision::SetupFresh,
				Duration::from_millis(300),
				false,
			),
			Duration::from_millis(300)
		);
		assert_eq!(
			live_frame_stream_macos::stream_setup_backoff(
				live_frame_stream_macos::StreamReuseDecision::RetryUpgradeUsingCurrent,
				Duration::from_millis(300),
				false,
			),
			Duration::from_secs(3)
		);
		assert_eq!(
			live_frame_stream_macos::stream_setup_backoff(
				live_frame_stream_macos::StreamReuseDecision::RetryUpgradeUsingCurrent,
				Duration::from_millis(300),
				true,
			),
			Duration::ZERO
		);
	}

	#[test]
	fn refresh_stream_requires_setup_backoff_only_for_recovery_paths() {
		assert!(live_frame_stream_macos::refresh_stream_requires_setup_backoff(None, 7));
		assert!(live_frame_stream_macos::refresh_stream_requires_setup_backoff(Some(9), 7));
		assert!(!live_frame_stream_macos::refresh_stream_requires_setup_backoff(Some(7), 7));
	}

	#[test]
	fn waiting_for_first_frame_expires_after_grace_window() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();
		let until = now + Duration::from_millis(50);

		shared.mark_waiting_for_frame_until(7, until);

		assert!(shared.waiting_for_frame_after_setup_at(7, now + Duration::from_millis(25)));
		assert!(!shared.waiting_for_frame_after_setup_at(7, now + Duration::from_millis(60)));
		assert!(!shared.waiting_for_frame_after_setup_at(7, now + Duration::from_millis(61)));
	}

	#[test]
	fn shared_latest_frame_tracks_self_capture_filter_completeness_per_monitor() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();

		assert!(!shared.self_capture_filter_complete_for_monitor(7));

		shared.set_stream_filter_status(7, false);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));

		shared.set_stream_filter_status(7, true);

		assert!(shared.self_capture_filter_complete_for_monitor(7));
		assert!(!shared.self_capture_filter_complete_for_monitor(9));
	}

	#[test]
	fn deferred_filter_complete_waits_for_matching_first_frame() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let pixel_buffer = test_pixel_buffer();
		let other_monitor_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 1,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: pixel_buffer.clone(),
		};
		let matching_monitor_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 2,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer,
		};

		shared.activate_stream_generation(7, 1);
		shared.defer_stream_filter_complete_until_next_frame(7, 1, true);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));

		let _ = shared.store(9, &other_monitor_frame);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));

		let _ = shared.store(7, &matching_monitor_frame);

		assert!(shared.self_capture_filter_complete_for_monitor(7));
	}

	#[test]
	fn deferred_filter_complete_ignores_stale_generation_frames() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let pixel_buffer = test_pixel_buffer();
		let stale_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 1,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: pixel_buffer.clone(),
		};
		let current_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 2,
			stream_generation: 2,
			captured_at: std::time::Instant::now(),
			pixel_buffer,
		};

		shared.activate_stream_generation(7, 2);
		shared.defer_stream_filter_complete_until_next_frame(7, 2, true);

		let _ = shared.store(7, &stale_frame);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));
		assert!(shared.latest_frame_for_monitor(7).is_none());

		let _ = shared.store(7, &current_frame);

		assert!(shared.self_capture_filter_complete_for_monitor(7));
		assert_eq!(
			shared.latest_frame_for_monitor(7).map(|frame| frame.stream_generation),
			Some(2)
		);
	}

	#[test]
	fn reset_discards_cached_frames_and_rejects_retired_stream_frames() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let pixel_buffer = test_pixel_buffer();
		let retired_frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 1,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: pixel_buffer.clone(),
		};

		shared.activate_stream_generation(7, 1);

		let _ = shared.store(7, &retired_frame);

		assert_eq!(
			shared.latest_frame_for_monitor(7).map(|frame| frame.stream_generation),
			Some(1)
		);

		shared.reset(Some(live_frame_stream_macos::frame_store::StreamGenerationStatus {
			monitor_id: 7,
			stream_generation: 1,
		}));

		assert!(shared.latest_frame_for_monitor(7).is_none());

		let _ = shared.store(7, &retired_frame);

		assert!(shared.latest_frame_for_monitor(7).is_none());
	}

	#[test]
	fn incomplete_filter_never_flips_complete_after_first_frame() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let frame = live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq: 1,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: test_pixel_buffer(),
		};

		shared.activate_stream_generation(7, 1);
		shared.defer_stream_filter_complete_until_next_frame(7, 1, false);

		let _ = shared.store(7, &frame);

		assert!(!shared.self_capture_filter_complete_for_monitor(7));
	}

	#[test]
	fn mac_live_frame_stream_reports_self_capture_filter_completeness_from_shared_status() {
		let stream = live_frame_stream_macos::MacLiveFrameStream::with_waker(None);
		let monitor = crate::state::MonitorRect {
			id: 7,
			origin: crate::state::GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};

		assert!(!stream.self_capture_filter_complete_for_monitor(monitor));

		stream.debug_set_self_capture_filter_complete(monitor.id, true);

		assert!(stream.self_capture_filter_complete_for_monitor(monitor));
	}

	#[test]
	fn stored_frame_completion_clears_pending_ensure_for_same_monitor() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_ensure_monitor(7));

		shared.mark_waiting_for_frame_until(7, now + Duration::from_secs(1));

		let outcome = shared.complete_pending_requests_for_stored_frame(7, 1);

		assert!(outcome.completed_ensure);
		assert!(!outcome.completed_refresh);
		assert!(!shared.waiting_for_frame_after_setup_at(7, now));
		assert!(!shared.finish_ensure_monitor(7));
	}

	#[test]
	fn stored_frame_completion_leaves_other_monitor_refresh_pending() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_refresh_monitor(7, 11, now));

		shared.mark_waiting_for_frame_until(7, now + Duration::from_secs(1));

		let outcome = shared.complete_pending_requests_for_stored_frame(9, 1);

		assert!(!outcome.completed_ensure);
		assert!(!outcome.completed_refresh);
		assert!(shared.waiting_for_frame_after_setup_at(7, now));
		assert!(shared.finish_refresh_monitor(7));
	}

	#[test]
	fn stored_frame_completion_clears_pending_refresh_for_same_monitor() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_refresh_monitor(7, 11, now));

		shared.mark_waiting_for_frame_until(7, now + Duration::from_secs(1));

		let outcome = shared.complete_pending_requests_for_stored_frame(7, 1);

		assert!(!outcome.completed_ensure);
		assert!(outcome.completed_refresh);
		assert!(!shared.waiting_for_frame_after_setup_at(7, now));
		assert!(!shared.finish_refresh_monitor(7));
	}

	#[test]
	fn stale_pending_refresh_retries_again_after_each_grace_window_for_same_stalled_frontier() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_refresh_monitor(7, 11, now));
		assert!(!shared.begin_refresh_monitor(7, 11, now + Duration::from_millis(100)));
		assert!(shared.begin_refresh_monitor(
			7,
			11,
			now + STREAM_POST_SETUP_FRAME_GRACE + Duration::from_millis(1),
		));
		assert!(!shared.begin_refresh_monitor(
			7,
			11,
			now + STREAM_POST_SETUP_FRAME_GRACE + Duration::from_millis(2),
		));
		assert!(shared.begin_refresh_monitor(
			7,
			11,
			now + STREAM_POST_SETUP_FRAME_GRACE.saturating_mul(2) + Duration::from_millis(1),
		));
	}

	#[test]
	fn stale_pending_refresh_rearms_when_stalled_frontier_advances() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let now = std::time::Instant::now();

		assert!(shared.begin_refresh_monitor(7, 11, now));
		assert!(shared.begin_refresh_monitor(
			7,
			11,
			now + STREAM_POST_SETUP_FRAME_GRACE + Duration::from_millis(1),
		));
		assert!(shared.begin_refresh_monitor(
			7,
			12,
			now + STREAM_POST_SETUP_FRAME_GRACE + Duration::from_millis(2),
		));
	}

	#[test]
	fn shared_frame_history_returns_all_frames_after_frontier() {
		let shared = live_frame_stream_macos::SharedLatestFrame::default();
		let monitor_id = 7;
		let pixel_buffer = test_pixel_buffer();
		let make_frame = |frame_seq| live_frame_stream_macos::QueuedPixelBufferFrame {
			frame_seq,
			stream_generation: 1,
			captured_at: std::time::Instant::now(),
			pixel_buffer: pixel_buffer.clone(),
		};

		for frame_seq in 1..=4 {
			let frame = make_frame(frame_seq);
			let _ = shared.store(monitor_id, &frame);
		}

		let queued = shared.frames_after_seq_for_monitor(monitor_id, 1);
		let seqs: Vec<u64> = queued.into_iter().map(|frame| frame.frame_seq).collect();

		assert_eq!(seqs, vec![2, 3, 4]);
	}

	#[test]
	fn nonblocking_after_seq_query_does_not_prime_when_same_monitor_already_has_latest_frame() {
		let monitor = crate::state::MonitorRect {
			id: 7,
			origin: crate::state::GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};
		let rect = crate::state::RectPoints::new(0, 0, 1, 1);
		let mut stream = live_frame_stream_macos::MacLiveFrameStream::with_waker(None);

		stream.debug_store_test_snapshot_with_metadata(monitor, 4, 1, std::time::Instant::now());

		assert!(stream.ordered_rgba_regions_after_seq_nonblocking(monitor, rect, 4).is_none());
		assert!(stream.debug_pending_monitor_is_none());
	}

	#[test]
	fn force_refresh_immediately_refreshes_when_seq_is_stalled() {
		assert!(stream_worker::should_refresh_monitor_frame(7, 7, Duration::from_millis(0), true,));
		assert!(!stream_worker::should_refresh_monitor_frame(
			7,
			7,
			Duration::from_millis(10),
			false,
		));
	}

	#[test]
	fn force_refresh_does_not_refresh_when_newer_frame_already_exists() {
		assert!(!stream_worker::should_refresh_monitor_frame(
			8,
			7,
			Duration::from_millis(200),
			true,
		));
	}

	#[test]
	fn stream_config_uses_cadence_queue_depth_contract() {
		let monitor = crate::state::MonitorRect {
			id: 7,
			origin: crate::state::GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};
		let config = stream_config::build_stream_config_for_monitor(
			monitor,
			live_frame_stream_macos::StreamCaptureTarget::FullMonitor,
		);

		assert_eq!(
			unsafe { config.queueDepth() },
			stream_config::STREAM_CONFIG_QUEUE_DEPTH as isize
		);
	}

	#[test]
	fn stream_config_uses_source_rect_for_scroll_capture_region() {
		let monitor = crate::state::MonitorRect {
			id: 7,
			origin: crate::state::GlobalPoint::new(0, 0),
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		};
		let region_points = crate::state::RectPoints::new(120, 80, 300, 220);
		let region_pixels = monitor.local_rect_to_pixels(region_points);
		let config = stream_config::build_stream_config_for_monitor(
			monitor,
			live_frame_stream_macos::StreamCaptureTarget::Region(
				live_frame_stream_macos::StreamCaptureRegion {
					rect_points: region_points,
					rect_pixels: region_pixels,
				},
			),
		);
		let source_rect = unsafe { config.sourceRect() };

		assert_eq!(unsafe { config.width() }, region_pixels.width as usize);
		assert_eq!(unsafe { config.height() }, region_pixels.height as usize);
		assert_eq!(source_rect.origin.x, f64::from(region_points.x));
		assert_eq!(source_rect.origin.y, f64::from(region_points.y));
		assert_eq!(source_rect.size.width, f64::from(region_points.width));
		assert_eq!(source_rect.size.height, f64::from(region_points.height));
	}

	#[test]
	fn stream_rect_maps_scroll_capture_region_requests_to_stream_local_rect() {
		let capture_target = live_frame_stream_macos::StreamCaptureTarget::Region(
			live_frame_stream_macos::StreamCaptureRegion {
				rect_points: crate::state::RectPoints::new(60, 40, 220, 120),
				rect_pixels: crate::state::RectPoints::new(120, 80, 440, 240),
			},
		);

		assert_eq!(
			stream_worker::stream_rect_for_requested_region(
				capture_target,
				crate::state::RectPoints::new(120, 80, 440, 240),
			),
			Some(crate::state::RectPoints::new(0, 0, 440, 240))
		);
		assert_eq!(
			stream_worker::stream_rect_for_requested_region(
				capture_target,
				crate::state::RectPoints::new(140, 100, 100, 80),
			),
			Some(crate::state::RectPoints::new(20, 20, 100, 80))
		);
		assert_eq!(
			stream_worker::stream_rect_for_requested_region(
				capture_target,
				crate::state::RectPoints::new(100, 80, 100, 80),
			),
			None
		);
	}

	#[test]
	fn sample_handler_queue_label_is_monitor_scoped() {
		assert_eq!(
			stream_config::sample_handler_queue_label(7),
			"io.hackink.rsnap.scroll-capture.sample-handler.monitor-7"
		);
		assert_ne!(
			stream_config::sample_handler_queue_label(7),
			stream_config::sample_handler_queue_label(9)
		);
	}
}
