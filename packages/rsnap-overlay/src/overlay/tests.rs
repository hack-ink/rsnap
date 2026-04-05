mod live_runtime;
mod rendering_behaviors;
mod scroll_input_runtime;
mod self_capture_runtime;
mod stream_refresh_runtime;
mod worker_observation_runtime;
mod worker_tick_runtime;

#[cfg(target_os = "macos")]
use std::collections::VecDeque;
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::Mutex;
#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicUsize;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "macos")]
use std::thread;
use std::time::Duration;
use std::time::Instant;

#[cfg(target_os = "macos")]
use color_eyre::eyre;
#[cfg(target_os = "macos")]
use color_eyre::eyre::Result;
use egui::FontDefinitions;
use egui::FontFamily;
use egui::RawInput;
use egui_phosphor::Variant;
use image::Rgba;
#[cfg(target_os = "macos")]
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
#[cfg(target_os = "macos")]
use winit::keyboard::ModifiersState;
#[cfg(target_os = "macos")]
use winit::window::WindowId;

#[cfg(target_os = "macos")]
use crate::backend;
#[cfg(target_os = "macos")]
use crate::backend::CaptureBackend;
#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::MacLiveFrameStream;
use crate::overlay::FrozenCaptureSource;
use crate::overlay::PngAction;
#[cfg(target_os = "macos")]
use crate::overlay::session_state::ScrollCaptureLiveFrame;
use crate::overlay::{
	self, FrozenSelectionDragState, FrozenToolbarState, FrozenToolbarTool,
	HUD_LOUPE_STRIP_GAP_POINTS, HudRedrawSummary, HudTheme, OCCLUDED_FRAME_REDRAW_RETRY_WINDOW,
	OverlaySession, Pos2, Rect, SCROLL_CAPTURE_SAMPLE_INTERVAL,
	SELECTION_DASHED_BORDER_DASH_LENGTH_PX, SELECTION_DASHED_BORDER_GAP_LENGTH_PX,
	SELECTION_DASHED_BORDER_WIDTH_PX, SELECTION_SIZE_BADGE_GAP_PX,
	SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX, SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX,
	SelectionDashedBorderCache, SelectionDashedBorderMetrics, SelectionFlowGeometryCache,
	SelectionSizeBadgeTarget, SurfaceFrameSkipReason, TOOLBAR_CAPTURE_GAP_PX,
	TOOLBAR_SCREEN_MARGIN_PX, ToolbarPlacement, Vec2, WindowRenderer, hud_helpers,
};
#[cfg(target_os = "macos")]
use crate::overlay::{
	AltActivationMode, HUD_PILL_CORNER_RADIUS_POINTS, HudPillGeometry,
	InflightScrollCaptureObservation, KCG_SCROLL_EVENT_UNIT_PIXEL, LiveSampleApplyResult,
	LiveStreamStaleGrace, MacOSScrollPixelResidual, OverlayControl, OverlayExit,
	SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW, SCROLL_CAPTURE_INPUT_FRESHNESS,
	SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES, SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE,
	ScrollCaptureFrameSource, StartupLiveRgbPlan, WindowCaptureAlphaMode,
};
use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome, ScrollSession};
#[cfg(target_os = "macos")]
use crate::state::LiveCursorSample;
use crate::state::{
	GlobalPoint, LoupeSample, MonitorRect, MonitorRectPoints, OverlayMode, OverlayState,
	RectPoints, Rgb,
};
#[cfg(target_os = "macos")]
use crate::state::{WindowListSnapshot, WindowRect};
#[cfg(target_os = "macos")]
use crate::worker::OverlayWorker;
#[cfg(target_os = "macos")]
use crate::worker::{WorkerErrorSource, WorkerResponse};

#[cfg(target_os = "macos")]
struct SequenceScrollCaptureBackend {
	frames: VecDeque<Option<image::RgbaImage>>,
}
#[cfg(target_os = "macos")]
impl SequenceScrollCaptureBackend {
	fn new(frames: impl IntoIterator<Item = Option<image::RgbaImage>>) -> Self {
		Self { frames: frames.into_iter().collect() }
	}
}

#[cfg(target_os = "macos")]
impl CaptureBackend for SequenceScrollCaptureBackend {
	fn capture_monitor(&mut self, _monitor: MonitorRect) -> Result<image::RgbaImage> {
		Err(eyre::eyre!("unused in this test"))
	}

	fn capture_monitor_region_for_scroll_capture(
		&mut self,
		_monitor: MonitorRect,
		_rect_px: RectPoints,
	) -> Result<Option<image::RgbaImage>> {
		Ok(self.frames.pop_front().unwrap_or(None))
	}

	fn pixel_rgb_in_monitor(
		&mut self,
		_monitor: MonitorRect,
		_point: GlobalPoint,
	) -> Result<Option<Rgb>> {
		Ok(None)
	}

	fn rgba_patch_in_monitor(
		&mut self,
		_monitor: MonitorRect,
		_point: GlobalPoint,
		_width_px: u32,
		_height_px: u32,
	) -> Result<Option<image::RgbaImage>> {
		Ok(None)
	}

	fn refresh_window_cache(&mut self) -> Result<Arc<WindowListSnapshot>> {
		Err(eyre::eyre!("unused in this test"))
	}
}

fn make_scroll_capture_test_image(width: u32, rows: &[[u8; 4]]) -> image::RgbaImage {
	let mut image = image::RgbaImage::new(width, rows.len() as u32);

	for (y, row) in rows.iter().enumerate() {
		for x in 0..width {
			image.put_pixel(x, y as u32, Rgba(*row));
		}
	}

	image
}

fn make_scroll_capture_window(
	document: &[[u8; 4]],
	width: u32,
	start_row: usize,
	window_rows: usize,
) -> image::RgbaImage {
	make_scroll_capture_test_image(width, &document[start_row..start_row + window_rows])
}

#[cfg(target_os = "macos")]
fn make_sparse_worker_capture_window(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
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

#[cfg(target_os = "macos")]
fn make_browser_like_worker_capture_window(
	width: u32,
	height: u32,
	start_row: u32,
) -> image::RgbaImage {
	let scrollbar_left = width.saturating_sub(18);
	let content_left = 56_u32;
	let content_right = width.saturating_sub(48);
	let heading_width = 220_u32;
	let paragraph_width = content_right.saturating_sub(content_left);
	let mut image = make_sparse_worker_capture_window(width, height, start_row);

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

fn set_scroll_capture_input(session: &mut OverlaySession, direction: ScrollDirection) {
	session.scroll_capture.input_direction = Some(direction);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;
}

#[cfg(target_os = "macos")]
fn enable_test_worker_scroll_capture_path(session: &mut OverlaySession) {
	session.scroll_capture.force_worker_sampling_in_tests = true;
}

#[cfg(target_os = "macos")]
fn drain_scroll_capture_worker_until_idle(session: &mut OverlaySession) {
	for _ in 0..64 {
		let _ = session.drain_worker_responses();

		if session.scroll_capture.inflight_request_id.is_none() {
			return;
		}

		thread::sleep(Duration::from_millis(5));
	}

	panic!(
		"timed out waiting for worker scroll-capture response; inflight_request_id={:?}",
		session.scroll_capture.inflight_request_id
	);
}

fn observe_scroll_capture_frame(
	session: &mut OverlaySession,
	frame: image::RgbaImage,
) -> Option<ScrollObserveOutcome> {
	match session.observe_scroll_capture_frame(frame).transpose() {
		Ok(outcome) => outcome,
		Err(err) => panic!("observe_scroll_capture_frame failed: {err:#}"),
	}
}

fn scroll_capture_export_height(session: &OverlaySession) -> u32 {
	match session.scroll_capture.session.as_ref() {
		Some(scroll_session) => scroll_session.export_image().height(),
		None => panic!("scroll_capture_export_height requires an active scroll session"),
	}
}

fn test_monitor() -> MonitorRect {
	MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	}
}

fn test_monitor_with_scale(width: u32, height: u32, scale_factor_x1000: u32) -> MonitorRect {
	MonitorRect { id: 1, origin: GlobalPoint::new(0, 0), width, height, scale_factor_x1000 }
}

fn test_frozen_image() -> image::RgbaImage {
	image::RgbaImage::from_pixel(8, 8, Rgba([12, 34, 56, 255]))
}

fn test_egui_context() -> egui::Context {
	let ctx = egui::Context::default();
	let mut fonts = FontDefinitions::default();
	let phosphor_fill = String::from("phosphor-fill");
	let proportional_fallback =
		fonts.families.get(&FontFamily::Proportional).and_then(|names| names.first()).cloned();

	egui_phosphor::add_to_fonts(&mut fonts, Variant::Regular);

	fonts.font_data.insert(phosphor_fill.clone(), Variant::Fill.font_data().into());
	fonts
		.families
		.entry(FontFamily::Name(phosphor_fill.clone().into()))
		.or_default()
		.extend([phosphor_fill]);

	if let Some(fallback) = proportional_fallback {
		let family = fonts.families.entry(FontFamily::Name("phosphor-fill".into())).or_default();

		if !family.contains(&fallback) {
			family.push(fallback);
		}
	}

	ctx.set_fonts(fonts);

	let _ = ctx.run_ui(RawInput::default(), |_ui| {});

	ctx
}

#[cfg(target_os = "macos")]
fn configured_session_with_macos_worker() -> (OverlaySession, u64) {
	let worker = OverlayWorker::new(backend::default_capture_backend(), None);
	let worker_debug_id = worker.debug_id();
	let mut session = OverlaySession::new();

	session.worker = Some(worker);
	session.live_sample_stream = Some(MacLiveFrameStream::new());
	session.scroll_capture.active = true;
	session.scroll_capture.live_stream = Some(MacLiveFrameStream::with_waker(None));
	session.config.self_capture_exception_window_ids = vec![17];

	(session, worker_debug_id)
}

#[cfg(target_os = "macos")]
fn seed_ready_scroll_capture_selection(session: &mut OverlaySession) {
	let monitor = test_monitor_with_scale(8, 8, 1_000);

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(1, 1, 4, 4));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.authoritative_frozen_capture_ready = true;
}

#[test]
fn begin_png_action_copies_preview_render_image_during_active_scroll_capture() {
	let mut session = OverlaySession::new();
	let base = make_scroll_capture_test_image(3, &[[10, 0, 0, 255]; 8]);
	let grown = make_scroll_capture_test_image(3, &[[20, 0, 0, 255]; 12]);
	let mut scroll_session = ScrollSession::new(base, 320).expect("scroll session");
	let _ = scroll_session.observe_downward_sample(grown).expect("observe");
	let expected_export = scroll_session.export_image().clone();
	let monitor = test_monitor();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.authoritative_frozen_capture_ready = true;
	session.scroll_capture.active = true;
	session.scroll_capture.session = Some(scroll_session);
	session.scroll_capture.preview_display_image =
		Some(image::RgbaImage::from_pixel(320, 64, Rgba([77, 0, 0, 255])));

	session.begin_png_action(PngAction::Copy);

	assert_eq!(session.pending_png_action, Some(PngAction::Copy));
	assert_eq!(session.pending_encode_png.as_ref(), Some(&expected_export));
	assert_eq!(session.state.error_message.as_deref(), Some("Copying..."));
}

#[cfg(target_os = "macos")]
#[test]
fn begin_ocr_action_exits_with_deferred_request_and_clears_stale_png_output_intent() {
	let monitor = test_monitor();
	let expected_export = test_frozen_image();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, expected_export.clone());

	session.state.frozen_capture_rect =
		Some(RectPoints::new(0, 0, expected_export.width(), expected_export.height()));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.authoritative_frozen_capture_ready = true;

	session.begin_png_action(PngAction::Copy);

	assert_eq!(session.pending_png_action, Some(PngAction::Copy));
	assert_eq!(session.pending_encode_png.as_ref(), Some(&expected_export));

	let control = session.begin_ocr_action();
	let OverlayControl::Exit(OverlayExit::DeferredTextRecognition(request)) = control else {
		panic!("expected deferred OCR exit");
	};

	assert_eq!(session.pending_png_action, None);
	assert!(session.pending_encode_png.is_none());
	assert_eq!(request.export_image().as_ref(), Some(&expected_export));
	assert_eq!(request.request_id, 0);
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn begin_ocr_action_drag_region_still_uses_frozen_image_under_matte_mode() {
	let monitor = test_monitor();
	let expected_export = test_frozen_image();
	let mut session = OverlaySession::new();

	session.config.window_capture_alpha_mode = WindowCaptureAlphaMode::MatteLight;

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, expected_export.clone());

	session.state.frozen_capture_rect =
		Some(RectPoints::new(0, 0, expected_export.width(), expected_export.height()));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.authoritative_frozen_capture_ready = true;

	let control = session.begin_ocr_action();
	let OverlayControl::Exit(OverlayExit::DeferredTextRecognition(request)) = control else {
		panic!("expected deferred OCR exit");
	};

	assert_eq!(request.export_image().as_ref(), Some(&expected_export));
	assert!(session.frozen_window_image.is_none());
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn begin_ocr_action_skips_deferred_request_when_drag_region_crop_is_out_of_bounds() {
	let monitor = test_monitor();
	let frozen_image = test_frozen_image();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, frozen_image.clone());

	session.state.frozen_capture_rect = Some(RectPoints::new(monitor.width + 10, 20, 100, 80));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.authoritative_frozen_capture_ready = true;

	let control = session.begin_ocr_action();

	assert!(matches!(control, OverlayControl::Continue));
	assert_eq!(session.state.frozen_image.as_ref(), Some(&frozen_image));
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn begin_ocr_action_uses_scroll_capture_export_image_in_deferred_request() {
	let monitor = test_monitor();
	let base = make_scroll_capture_test_image(3, &[[10, 0, 0, 255]; 8]);
	let grown = make_scroll_capture_test_image(3, &[[20, 0, 0, 255]; 12]);
	let mut scroll_session = ScrollSession::new(base, 320).expect("scroll session");
	let _ = scroll_session.observe_downward_sample(grown).expect("observe");
	let expected_export = scroll_session.export_image().clone();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;
	session.authoritative_frozen_capture_ready = true;
	session.scroll_capture.active = true;
	session.scroll_capture.session = Some(scroll_session);
	session.scroll_capture.preview_display_image =
		Some(image::RgbaImage::from_pixel(320, 64, Rgba([77, 0, 0, 255])));

	let control = session.begin_ocr_action();
	let OverlayControl::Exit(OverlayExit::DeferredTextRecognition(request)) = control else {
		panic!("expected deferred OCR exit");
	};

	assert_eq!(request.export_image().as_ref(), Some(&expected_export));
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn duplicate_live_frames_schedule_forced_refresh_when_downward_backlog_is_fresh() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let observed_at = Instant::now();
	let frame = ScrollCaptureLiveFrame {
		frame_seq: 7,
		captured_at: observed_at,
		image: image::RgbaImage::from_pixel(16, 16, Rgba([7, 8, 9, 255])),
	};
	let mut session = OverlaySession::new();

	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.live_stream = Some(MacLiveFrameStream::new());
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(observed_at);
	session.scroll_capture.input_gesture_active = true;
	session.scroll_capture.downward_motion_rows_pending = 512.0;

	assert!(session.note_scroll_capture_live_stream_frame_activity(&frame));
	assert!(!session.note_scroll_capture_live_stream_frame_activity(&frame));
	assert!(!session.note_scroll_capture_live_stream_frame_activity(&frame));
	assert!(!session.note_scroll_capture_live_stream_frame_activity(&frame));
	assert_eq!(session.scroll_capture.consecutive_identical_stream_frames, 3);

	session.maybe_schedule_duplicate_stream_refresh(frame.frame_seq, observed_at);

	assert_eq!(
		session
			.scroll_capture
			.live_stream
			.as_ref()
			.and_then(MacLiveFrameStream::debug_last_request_kind),
		Some("refresh_monitor_nonblocking_if_stale")
	);
	assert_eq!(session.scroll_capture.pending_post_stall_burst_after_seq, Some(frame.frame_seq));
	assert_eq!(session.scroll_capture.last_duplicate_stream_refresh_at, Some(observed_at));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn scroll_capture_is_unavailable_on_non_macos_even_with_drag_selection() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let mut session = OverlaySession::new();

	session.state.mode = OverlayMode::Frozen;
	session.state.monitor = Some(monitor);
	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 200, 240));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	assert!(!session.scroll_capture_is_available());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_guard_error_keeps_frozen_capture_available() {
	let mut session = OverlaySession::new();

	seed_ready_scroll_capture_selection(&mut session);

	session.set_scroll_capture_start_guard(Arc::new(|| {
		Err(eyre::eyre!("Open System Settings and retry."))
	}));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.state.frozen_image.is_some());
	assert!(
		session
			.state
			.error_message
			.as_deref()
			.is_some_and(|message| message.contains("Open System Settings and retry."))
	);
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_guard_silent_reject_keeps_frozen_capture_available_without_error() {
	let mut session = OverlaySession::new();

	seed_ready_scroll_capture_selection(&mut session);

	session.set_scroll_capture_start_guard(Arc::new(|| Ok(false)));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.state.frozen_image.is_some());
	assert!(session.state.error_message.is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_starting_hook_error_keeps_frozen_capture_available() {
	let mut session = OverlaySession::new();

	seed_ready_scroll_capture_selection(&mut session);

	session.set_scroll_capture_start_guard(Arc::new(|| Ok(true)));
	session
		.set_scroll_capture_starting_hook(Arc::new(|| Err(eyre::eyre!("Observer was not ready."))));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.state.frozen_image.is_some());
	assert!(
		session
			.state
			.error_message
			.as_deref()
			.is_some_and(|message| message.contains("Observer was not ready."))
	);
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_preflight_runs_before_permission_guard() {
	let guard_calls = Arc::new(AtomicUsize::new(0));
	let mut session = OverlaySession::new();

	session.set_scroll_capture_start_guard(Arc::new({
		let guard_calls = Arc::clone(&guard_calls);

		move || {
			guard_calls.fetch_add(1, Ordering::SeqCst);

			Ok(true)
		}
	}));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert_eq!(guard_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
	assert_eq!(
		session.state.error_message.as_deref(),
		Some("Scroll capture requires a dragged region selection.")
	);
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_starting_hook_runs_before_started_hook() {
	let hook_order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
	let mut session = OverlaySession::new();

	seed_ready_scroll_capture_selection(&mut session);

	session.set_scroll_capture_start_guard(Arc::new(|| Ok(true)));

	session.set_scroll_capture_starting_hook(Arc::new({
		let hook_order = Arc::clone(&hook_order);

		move || {
			let mut hook_order = match hook_order.lock() {
				Ok(hook_order) => hook_order,
				Err(poisoned) => poisoned.into_inner(),
			};

			hook_order.push("starting");

			Ok(())
		}
	}));
	session.set_scroll_capture_started_hook(Arc::new({
		let hook_order = Arc::clone(&hook_order);

		move || {
			let mut hook_order = match hook_order.lock() {
				Ok(hook_order) => hook_order,
				Err(poisoned) => poisoned.into_inner(),
			};

			hook_order.push("started");
		}
	}));

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.scroll_capture.active);

	let hook_order = match hook_order.lock() {
		Ok(hook_order) => hook_order,
		Err(poisoned) => poisoned.into_inner(),
	};

	assert_eq!(*hook_order, vec!["starting", "started"]);
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_start_preserves_existing_live_sample_stream() {
	let mut session = OverlaySession::new();

	seed_ready_scroll_capture_selection(&mut session);

	session.live_sample_stream = Some(MacLiveFrameStream::new());

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.scroll_capture.active);
	assert!(session.live_sample_stream.is_some());
	assert!(session.scroll_capture.live_stream.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn scroll_capture_start_skips_scroll_live_stream_when_worker_sampling_is_forced() {
	let mut session = OverlaySession::new();

	seed_ready_scroll_capture_selection(&mut session);
	enable_test_worker_scroll_capture_path(&mut session);

	let control = session.start_scroll_capture();

	assert!(matches!(control, OverlayControl::Continue));
	assert!(session.scroll_capture.active);
	assert!(session.scroll_capture.live_stream.is_none());
	assert!(session.scroll_capture.live_stream_backlog.is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn reset_for_start_preserves_external_scroll_input_drain_reader() {
	let mut session = OverlaySession::default();

	session.set_external_scroll_input_drain_reader(Arc::new(|_, _| {
		vec![(1, Instant::now(), 10.0, 20.0, 4.0, true, false)]
	}));
	session.reset_for_start();

	assert!(session.scroll_capture.external_scroll_input_drain_reader.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn reset_for_start_clears_reused_session_transient_flags() {
	let mut session = OverlaySession {
		window_list_refresh_inflight: true,
		drop_next_window_list_refresh_snapshot: true,
		png_encode_inflight: true,
		pending_self_capture_exception_window_ids_worker_refresh: true,
		authoritative_frozen_capture_ready: true,
		capture_windows_hidden: true,
		loupe_activation_key_down: true,
		keyboard_modifiers: ModifiersState::SHIFT,
		left_mouse_button_down: true,
		left_mouse_button_down_monitor: Some(test_monitor()),
		left_mouse_button_down_global: Some(GlobalPoint::new(12, 34)),
		hud_window_visible: true,
		toolbar_window_visible: true,
		toolbar_window_warmup_redraws_remaining: 3,
		..OverlaySession::default()
	};

	session.reset_for_start();

	assert!(!session.window_list_refresh_inflight);
	assert!(!session.drop_next_window_list_refresh_snapshot);
	assert!(!session.png_encode_inflight);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
	assert!(!session.authoritative_frozen_capture_ready);
	assert!(!session.capture_windows_hidden);
	assert!(!session.loupe_activation_key_down);
	assert_eq!(session.keyboard_modifiers, ModifiersState::default());
	assert!(!session.left_mouse_button_down);
	assert!(session.left_mouse_button_down_monitor.is_none());
	assert!(session.left_mouse_button_down_global.is_none());
	assert!(!session.hud_window_visible);
	assert!(!session.toolbar_window_visible);
	assert_eq!(session.toolbar_window_warmup_redraws_remaining, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn drain_external_scroll_input_events_through_advances_last_seen_seq() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let start = Instant::now();
	let events = Arc::new([
		(1, start, 150.0, 160.0, -4.0, true, false),
		(2, start + Duration::from_millis(2), 150.0, 160.0, -4.0, false, true),
	]);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(start);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(session.scroll_capture.input_gesture_active);
	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);

	session.drain_external_scroll_input_events_through(start);

	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 1);

	session.drain_external_scroll_input_events_through(start + Duration::from_millis(2));

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert!(!session.scroll_capture.input_gesture_active);
	assert_eq!(session.scroll_capture.last_external_scroll_input_seq, 2);
}

#[cfg(target_os = "macos")]
#[test]
fn drain_external_scroll_input_events_through_uses_pairing_time_for_freshness() {
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let through = Instant::now();
	let recorded_at = through - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50);
	let events = Arc::new([(1, recorded_at, 150.0, 160.0, -4.0, false, false)]);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(RectPoints::new(100, 120, 200, 240));
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, paired_through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= paired_through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(through);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert_eq!(session.scroll_capture.input_direction_at, Some(through));
	assert_eq!(session.scroll_capture_observation_block_reason(), None);
}

#[cfg(target_os = "macos")]
#[test]
fn replayed_stream_input_uses_frame_time_for_stale_gate_without_global_relaxation() {
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
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let through = Instant::now() - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(50);
	let recorded_at = through - Duration::from_millis(12);
	let events = Arc::new([(1, recorded_at, 150.0, 160.0, -4.0, false, false)]);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, paired_through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= paired_through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(through);

	assert_eq!(session.scroll_capture.input_direction, Some(ScrollDirection::Down));
	assert_eq!(session.scroll_capture.input_direction_at, Some(through));
	assert_eq!(session.scroll_capture_observation_block_reason(), Some("stale_input"));
	assert_eq!(session.scroll_capture_observation_block_reason_at(through), None);
	assert_eq!(
		session
			.observe_scroll_capture_frame_at(
				make_scroll_capture_window(&document, 3, 1, 5),
				through,
			)
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
}

#[cfg(target_os = "macos")]
#[test]
fn replayed_downward_input_allows_bounded_stale_live_stream_frame() {
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
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let through = Instant::now();
	let events =
		Arc::new([(7, through - Duration::from_millis(10), 150.0, 160.0, 4.0, false, false)]);
	let stale_at = through + SCROLL_CAPTURE_INPUT_FRESHNESS + Duration::from_millis(1);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, paired_through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= paired_through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(through);

	assert_eq!(
		session.scroll_capture.live_stream_stale_grace,
		Some(LiveStreamStaleGrace {
			external_input_seq: 7,
			remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
		})
	);
	assert_eq!(
		session
			.observe_scroll_capture_frame_at(
				make_scroll_capture_window(&document, 3, 1, 5),
				stale_at,
			)
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(scroll_capture_export_height(&session), 6);
}

#[cfg(target_os = "macos")]
#[test]
fn stale_live_stream_frame_is_observed_even_without_direction_freshness() {
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
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let through = Instant::now();
	let wheel_at = through + Duration::from_millis(10);
	let events =
		Arc::new([(7, through - Duration::from_millis(10), 150.0, 160.0, 4.0, false, false)]);
	let stale_at = wheel_at + SCROLL_CAPTURE_INPUT_FRESHNESS + Duration::from_millis(1);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
	session.set_external_scroll_input_drain_reader(Arc::new({
		let events = Arc::clone(&events);

		move |after_seq, paired_through| {
			events
				.iter()
				.copied()
				.filter(|event| event.0 > after_seq && event.1 <= paired_through)
				.collect()
		}
	}));

	session.drain_external_scroll_input_events_through(through);
	session.record_scroll_capture_input_direction_from_overlay_wheel_at(
		&MouseScrollDelta::LineDelta(0.0, -1.0),
		wheel_at,
	);

	assert_eq!(session.scroll_capture.input_direction_at, Some(wheel_at));
	assert_eq!(
		session.scroll_capture.live_stream_stale_grace,
		Some(LiveStreamStaleGrace {
			external_input_seq: 7,
			remaining_stale_frames: SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
		})
	);
	assert_eq!(
		session
			.observe_scroll_capture_frame_at(
				make_scroll_capture_window(&document, 3, 1, 5),
				stale_at,
			)
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(scroll_capture_export_height(&session), 6);
}

#[cfg(target_os = "macos")]
#[test]
fn handle_scroll_capture_frame_passes_allow_stale_input_into_live_stream_gate() {
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
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let observed_at = Instant::now();
	let input_at = observed_at - SCROLL_CAPTURE_INPUT_FRESHNESS - Duration::from_millis(1);
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(input_at);
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());

	assert_eq!(
		session
			.handle_scroll_capture_frame(
				make_scroll_capture_window(&document, 3, 1, 5),
				ScrollCaptureFrameSource::LiveStream { frame_seq: 143 },
				true,
				observed_at,
			)
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(scroll_capture_export_height(&session), 6);
}

#[cfg(target_os = "macos")]
#[test]
fn fresh_live_stream_frame_without_direction_metadata_fails_closed_as_no_change() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
	];
	let monitor = MonitorRect {
		id: 1,
		origin: GlobalPoint::new(0, 0),
		width: 1_000,
		height: 800,
		scale_factor_x1000: 1_000,
	};
	let capture_rect = RectPoints::new(100, 120, 200, 240);
	let observed_at = Instant::now();
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.monitor = Some(monitor);
	session.scroll_capture.capture_rect_pixels = Some(capture_rect);
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());

	session.handle_scroll_capture_frame(
		make_scroll_capture_window(&document, 3, 1, 5),
		ScrollCaptureFrameSource::LiveStream { frame_seq: 143 },
		false,
		observed_at,
	);

	assert_eq!(scroll_capture_export_height(&session), 5);
}

#[test]
fn downward_frame_motion_commits_even_with_legacy_upward_input_direction() {
	let document = [
		[10, 0, 0, 255],
		[20, 0, 0, 255],
		[30, 0, 0, 255],
		[40, 0, 0, 255],
		[50, 0, 0, 255],
		[60, 0, 0, 255],
		[70, 0, 0, 255],
	];
	let mut session = OverlaySession::new();

	session.scroll_capture.active = true;
	session.scroll_capture.session =
		Some(ScrollSession::new(make_scroll_capture_window(&document, 3, 0, 5), 320).unwrap());
	session.scroll_capture.input_direction = Some(ScrollDirection::Down);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	assert_eq!(
		session
			.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 1, 5))
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);

	let height_after_first_append =
		session.scroll_capture.session.as_ref().unwrap().export_image().height();

	session.scroll_capture.input_direction = Some(ScrollDirection::Up);
	session.scroll_capture.input_direction_at = Some(Instant::now());
	session.scroll_capture.input_gesture_active = true;

	assert_eq!(
		session
			.observe_scroll_capture_frame(make_scroll_capture_window(&document, 3, 2, 5))
			.transpose()
			.unwrap(),
		Some(ScrollObserveOutcome::Committed { direction: ScrollDirection::Down, growth_rows: 1 })
	);
	assert_eq!(
		session.scroll_capture.session.as_ref().unwrap().export_image().height(),
		height_after_first_append + 1
	);
}

#[cfg(target_os = "macos")]
#[test]
fn pixel_delta_residuals_accumulate_until_whole_pixels_emit() {
	let mut residual = MacOSScrollPixelResidual::default();
	let first = OverlaySession::normalize_macos_scroll_wheel_delta(
		&MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.4, -0.4)),
		&mut residual,
	);
	let second = OverlaySession::normalize_macos_scroll_wheel_delta(
		&MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.7, -0.8)),
		&mut residual,
	);

	assert_eq!(first.units, KCG_SCROLL_EVENT_UNIT_PIXEL);
	assert_eq!(first.posted_x, 0);
	assert_eq!(first.posted_y, 0);
	assert!((first.residual.x - 0.4).abs() < f64::EPSILON);
	assert!((first.residual.y + 0.4).abs() < f64::EPSILON);
	assert_eq!(second.posted_x, 1);
	assert_eq!(second.posted_y, -1);
	assert!((second.residual.x - 0.1).abs() < 1e-9);
	assert!((second.residual.y + 0.2).abs() < 1e-9);
}

#[test]
fn frozen_toolbar_selected_mode_uses_fill_without_border() {
	for theme in [HudTheme::Dark, HudTheme::Light] {
		let style = WindowRenderer::frozen_toolbar_button_style(theme, true, false, true);

		assert!(style.bg_color.a() > 0);
		assert_eq!(style.border_color, None);
	}
}

#[test]
fn toolbar_window_hides_until_frozen_pixels_exist() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);

	assert!(session.should_hide_toolbar_window(monitor));

	session.pending_freeze_capture = Some(monitor);

	assert!(session.should_hide_toolbar_window(monitor));

	session.pending_freeze_capture = None;
	session.inflight_freeze_capture = Some(monitor);

	assert!(session.should_hide_toolbar_window(monitor));
}

#[test]
fn toolbar_window_stays_visible_while_final_capture_is_pending() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	assert!(!session.should_hide_toolbar_window(monitor));

	session.pending_freeze_capture = Some(monitor);

	assert!(!session.should_hide_toolbar_window(monitor));

	session.pending_freeze_capture = None;
	session.inflight_freeze_capture = Some(monitor);

	assert!(!session.should_hide_toolbar_window(monitor));
}

#[test]
fn force_pending_hud_and_loupe_moves_only_during_frozen_transition() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	assert!(!session.should_force_pending_hud_and_loupe_moves());

	session.state.begin_freeze(monitor);

	assert!(session.should_force_pending_hud_and_loupe_moves());

	session.state.finish_freeze(monitor, test_frozen_image());

	session.authoritative_frozen_capture_ready = true;

	assert!(!session.should_force_pending_hud_and_loupe_moves());

	session.inflight_freeze_capture = Some(monitor);

	assert!(session.should_force_pending_hud_and_loupe_moves());

	session.state.mode = OverlayMode::Live;

	assert!(!session.should_force_pending_hud_and_loupe_moves());
}

#[test]
fn tinted_hud_body_fill_amount_zero_keeps_base_fill() {
	for theme in [HudTheme::Dark, HudTheme::Light] {
		let base_fill = hud_helpers::hud_body_fill_srgba8(theme, false);
		let no_tint = WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 0.0, 0.585);

		assert_eq!(no_tint.r(), base_fill[0]);
		assert_eq!(no_tint.g(), base_fill[1]);
		assert_eq!(no_tint.b(), base_fill[2]);
		assert_eq!(no_tint.a(), 255);
	}
}

#[test]
fn tinted_hud_body_fill_100pct_tint_is_visibly_blue() {
	let dark_min_delta: u16 = 57;
	let light_min_delta: u16 = 24;
	let sky_tint = 0.585;

	for theme in [HudTheme::Dark, HudTheme::Light] {
		let base_fill =
			WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 0.0, sky_tint);
		let tinted_fill =
			WindowRenderer::tinted_hud_body_fill(theme, false, false, 1.0, 1.0, sky_tint);
		let rgb_delta = u16::from(base_fill.r()).abs_diff(u16::from(tinted_fill.r()))
			+ u16::from(base_fill.g()).abs_diff(u16::from(tinted_fill.g()))
			+ u16::from(base_fill.b()).abs_diff(u16::from(tinted_fill.b()));
		let min_delta =
			if matches!(theme, HudTheme::Dark) { dark_min_delta } else { light_min_delta };

		assert!(
			rgb_delta >= min_delta,
			"expected minimum tint delta >= {min_delta}, got {rgb_delta}"
		);
	}
}

#[test]
fn tinted_hud_body_fill_preserves_alpha() {
	for theme in [HudTheme::Dark, HudTheme::Light] {
		let tint_hue = 0.585;
		let opaque = WindowRenderer::tinted_hud_body_fill(theme, false, true, 0.25, 1.0, tint_hue);
		let translucent =
			WindowRenderer::tinted_hud_body_fill(theme, false, false, 0.33, 1.0, tint_hue);

		assert_eq!(opaque.a(), 255);
		assert_eq!(translucent.a(), (0.33_f32 * 255.0).round().clamp(0.0, 255.0) as u8);
	}
}

#[test]
fn tinted_hud_body_fill_blur_active_enforces_min_opacity() {
	for theme in [HudTheme::Dark, HudTheme::Light] {
		let tint_hue = 0.585;
		let fill = WindowRenderer::tinted_hud_body_fill(theme, true, false, 0.0, 0.0, tint_hue);
		let expected =
			(hud_helpers::hud_blur_tint_alpha(theme) * 255.0).round().clamp(0.0, 255.0) as u8;

		assert_eq!(fill.a(), expected);
	}
}

#[test]
fn frozen_toolbar_clamps_floating_position() {
	let monitor = Rect::from_min_size(Pos2::new(-200.0, -100.0), Vec2::new(500.0, 400.0));
	let toolbar_size = Vec2::new(220.0, 42.0);
	let clamped = WindowRenderer::clamp_toolbar_position(
		monitor,
		toolbar_size,
		Pos2::new(-400.0, -240.0),
		TOOLBAR_SCREEN_MARGIN_PX,
		TOOLBAR_SCREEN_MARGIN_PX,
	);

	assert_eq!(clamped.x, monitor.min.x + TOOLBAR_SCREEN_MARGIN_PX);
	assert_eq!(clamped.y, monitor.min.y + TOOLBAR_SCREEN_MARGIN_PX);
}

#[test]
fn interactive_repaint_fps_uses_known_lower_monitor_refresh() {
	assert_eq!(OverlaySession::interactive_repaint_fps(Some(60.0), Some(144.0)), 60.0);
	assert_eq!(OverlaySession::interactive_repaint_fps(Some(75.0), Some(120.0)), 75.0);
}

#[test]
fn interactive_repaint_fps_caps_known_higher_refresh_to_contract_limit() {
	assert_eq!(OverlaySession::interactive_repaint_fps(Some(144.0), Some(60.0)), 120.0);
	assert_eq!(OverlaySession::interactive_repaint_fps(Some(240.0), None), 120.0);
}

#[test]
fn interactive_repaint_fps_falls_back_to_known_or_default_cap() {
	assert_eq!(OverlaySession::interactive_repaint_fps(None, Some(90.0)), 90.0);
	assert_eq!(OverlaySession::interactive_repaint_fps(None, Some(144.0)), 120.0);
	assert_eq!(OverlaySession::interactive_repaint_fps(None, None), 120.0);
}

#[test]
fn occluded_surface_skip_requests_redraw_until_retry_window_expires() {
	let now = Instant::now();
	let mut retry_until = None;

	assert!(overlay::should_request_overlay_redraw_after_surface_skip(
		SurfaceFrameSkipReason::Occluded,
		now,
		&mut retry_until,
	));
	assert_eq!(retry_until, Some(now + OCCLUDED_FRAME_REDRAW_RETRY_WINDOW));
	assert!(overlay::should_request_overlay_redraw_after_surface_skip(
		SurfaceFrameSkipReason::Occluded,
		now + Duration::from_millis(500),
		&mut retry_until,
	));
	assert!(!overlay::should_request_overlay_redraw_after_surface_skip(
		SurfaceFrameSkipReason::Occluded,
		now + OCCLUDED_FRAME_REDRAW_RETRY_WINDOW,
		&mut retry_until,
	));
	assert_eq!(retry_until, None);
}

#[test]
fn timeout_surface_skip_always_requests_redraw_without_touching_occluded_retry_window() {
	let now = Instant::now();
	let retry_deadline = now + Duration::from_millis(250);
	let mut retry_until = Some(retry_deadline);

	assert!(overlay::should_request_overlay_redraw_after_surface_skip(
		SurfaceFrameSkipReason::Timeout,
		now,
		&mut retry_until,
	));
	assert_eq!(retry_until, Some(retry_deadline));
}
