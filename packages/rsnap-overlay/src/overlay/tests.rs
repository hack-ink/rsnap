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
use egui::RawInput;
use image::Rgba;
#[cfg(target_os = "macos")]
use image::imageops;
#[cfg(target_os = "macos")]
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta};
#[cfg(target_os = "macos")]
use winit::keyboard::ModifiersState;
use winit::keyboard::{Key, NamedKey};
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
use crate::overlay::rendering;
#[cfg(target_os = "macos")]
use crate::overlay::session_state::ScrollCaptureLiveFrame;
use crate::overlay::{
	self, ActiveFrozenBrushStroke, FROZEN_BRUSH_COLOR_RGBA, FROZEN_EDIT_HISTORY_LIMIT,
	FROZEN_TEXT_CARET_REPAINT_INTERVAL, FrozenBrushModelState, FrozenBrushStroke,
	FrozenCommittedOverlay, FrozenEditKind, FrozenExportTransform, FrozenImagePatch,
	FrozenMosaicEdit, FrozenSelectionDragState, FrozenTextAnnotation, FrozenTextColor,
	FrozenTextEditState, FrozenTextInputSource, FrozenToolbarState, FrozenToolbarTool,
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

	rendering::configure_egui_fonts(&mut fonts);

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

#[test]
fn current_export_image_includes_frozen_brush_strokes() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session
		.state
		.finish_freeze(monitor, image::RgbaImage::from_pixel(8, 8, Rgba([12, 34, 56, 255])));

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.authoritative_frozen_capture_ready = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(2, 2)));
	assert!(session.update_frozen_brush_stroke(GlobalPoint::new(5, 2)));
	assert!(session.finish_frozen_brush_stroke());

	let export_image = session.current_export_image().expect("annotated export image");

	assert_eq!(export_image.get_pixel(7, 7), &Rgba([12, 34, 56, 255]));
	assert_eq!(export_image.get_pixel(2, 2), &Rgba(FROZEN_BRUSH_COLOR_RGBA));
}

#[test]
fn frozen_brush_undo_and_redo_update_export_image() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session
		.state
		.finish_freeze(monitor, image::RgbaImage::from_pixel(8, 8, Rgba([12, 34, 56, 255])));

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.authoritative_frozen_capture_ready = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(3, 3)));
	assert!(session.finish_frozen_brush_stroke());
	assert!(session.perform_frozen_undo());

	let undone = session.current_export_image().expect("undo export image");

	assert_eq!(undone.get_pixel(3, 3), &Rgba([12, 34, 56, 255]));
	assert!(session.perform_frozen_redo());

	let redone = session.current_export_image().expect("redo export image");

	assert_eq!(redone.get_pixel(3, 3), &Rgba(FROZEN_BRUSH_COLOR_RGBA));
}

#[test]
fn current_export_image_antialiases_frozen_brush_edges() {
	let monitor = test_monitor();
	let background = Rgba([240, 240, 240, 255]);
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, image::RgbaImage::from_pixel(16, 16, background));

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 16, 16));
	session.authoritative_frozen_capture_ready = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(3, 3)));
	assert!(session.update_frozen_brush_stroke(GlobalPoint::new(12, 12)));
	assert!(session.finish_frozen_brush_stroke());

	let export_image = session.current_export_image().expect("annotated export image");
	let has_antialiased_edge = export_image
		.pixels()
		.any(|pixel| pixel != &background && pixel != &Rgba(FROZEN_BRUSH_COLOR_RGBA));

	assert!(has_antialiased_edge, "expected blended edge pixels around the exported brush");
}

fn significant_y_direction_reversals(points: &[Pos2], min_delta: f32) -> usize {
	let mut last_direction = 0_i8;
	let mut reversals = 0;

	for window in points.windows(2) {
		let delta_y = window[1].y - window[0].y;
		let direction = if delta_y > min_delta {
			1
		} else if delta_y < -min_delta {
			-1
		} else {
			0
		};

		if direction == 0 {
			continue;
		}
		if last_direction != 0 && direction != last_direction {
			reversals += 1;
		}

		last_direction = direction;
	}

	reversals
}

#[test]
fn rendered_frozen_brush_points_round_corners_into_a_curve() {
	let points = [Pos2::new(1.0, 1.0), Pos2::new(1.0, 5.0), Pos2::new(5.0, 5.0)];
	let rendered = OverlaySession::rendered_frozen_brush_points(
		&points,
		overlay::FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
	);

	assert_eq!(rendered.first().copied(), Some(points[0]));
	assert_eq!(rendered.last().copied(), Some(points[2]));
	assert!(rendered.len() > points.len());
	assert!(rendered.iter().any(|point| {
		(point.x - points[0].x).abs() > f32::EPSILON && (point.y - points[2].y).abs() > f32::EPSILON
	}));
}

#[test]
fn corrected_frozen_brush_points_preserve_open_stroke_endpoints() {
	let points = [Pos2::new(1.0, 1.0), Pos2::new(4.0, 2.0), Pos2::new(7.0, 6.0)];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);

	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[2]));
	assert!(corrected.len() >= 2);
}

#[test]
fn corrected_frozen_brush_points_keep_annotation_loops_open() {
	let points = [
		Pos2::new(2.0, 0.0),
		Pos2::new(6.0, 1.0),
		Pos2::new(8.0, 4.0),
		Pos2::new(7.0, 8.0),
		Pos2::new(3.0, 9.0),
		Pos2::new(0.0, 6.0),
		Pos2::new(1.5, 1.0),
	];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);

	assert!(corrected.len() >= 2);
	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[6]));
	assert_ne!(corrected.first().copied(), corrected.last().copied());
}

#[test]
fn corrected_frozen_brush_points_suppress_small_local_dent() {
	let points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(5.0, 0.4),
		Pos2::new(8.0, -1.6),
		Pos2::new(11.0, 0.6),
		Pos2::new(16.0, 0.3),
	];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);
	let deepest_y = corrected.iter().fold(f32::INFINITY, |deepest, point| deepest.min(point.y));

	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[4]));
	assert!(deepest_y > -0.8, "expected final stroke to smooth away the local dent: {corrected:?}");
}

#[test]
fn corrected_frozen_brush_points_preserve_monotonic_arc_trend() {
	let points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(3.0, 1.8),
		Pos2::new(6.0, 3.8),
		Pos2::new(9.0, 5.1),
		Pos2::new(11.0, 4.6),
		Pos2::new(14.0, 6.4),
		Pos2::new(18.0, 8.8),
	];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);

	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[6]));
	assert!(corrected.windows(2).all(|pair| pair[1].y + 0.05 >= pair[0].y));
}

#[test]
fn corrected_frozen_brush_points_preserve_small_wave_backbone() {
	let points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(4.0, 1.8),
		Pos2::new(8.0, -1.7),
		Pos2::new(12.0, 1.9),
		Pos2::new(16.0, -1.8),
		Pos2::new(20.0, 1.7),
		Pos2::new(24.0, 0.0),
	];
	let corrected = OverlaySession::corrected_frozen_brush_points(&points);
	let reversals = significant_y_direction_reversals(&corrected, 0.12);
	let (min_y, max_y) = corrected.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |acc, point| {
		(acc.0.min(point.y), acc.1.max(point.y))
	});

	assert_eq!(corrected.first().copied(), Some(points[0]));
	assert_eq!(corrected.last().copied(), Some(points[6]));
	assert!(
		reversals >= 3,
		"expected the corrected stroke to keep the wave skeleton: {corrected:?}"
	);
	assert!(
		max_y - min_y >= 1.0,
		"expected the corrected stroke to retain visible wave amplitude: {corrected:?}"
	);
}

#[test]
fn preview_frozen_brush_points_keep_live_modeled_path_before_commit() {
	let active_stroke = ActiveFrozenBrushStroke {
		raw_points: vec![
			Pos2::new(0.0, 0.0),
			Pos2::new(4.0, 6.0),
			Pos2::new(8.0, -2.0),
			Pos2::new(12.0, 4.0),
		],
		points: vec![Pos2::new(0.0, 0.0), Pos2::new(6.0, 2.0), Pos2::new(12.0, 4.0)],
		model_state: FrozenBrushModelState {
			filtered_input_point: Pos2::new(12.0, 4.0),
			modeled_point: Pos2::new(12.0, 4.0),
			modeled_velocity: Vec2::ZERO,
			modeled_elapsed_seconds: 0.03,
		},
		started_at: Instant::now(),
		last_sample_at: Instant::now(),
	};
	let preview = OverlaySession::preview_frozen_brush_points(&active_stroke);
	let committed = OverlaySession::corrected_frozen_brush_points(&active_stroke.raw_points);

	assert_eq!(preview.first().copied(), active_stroke.raw_points.first().copied());
	assert_eq!(preview.last().copied(), active_stroke.raw_points.last().copied());
	assert_ne!(preview, active_stroke.points);
	assert_ne!(preview, committed);
}

#[test]
fn preview_frozen_brush_points_follow_modeled_centerline_instead_of_raw_wobble() {
	let active_stroke = ActiveFrozenBrushStroke {
		raw_points: vec![
			Pos2::new(0.0, 0.0),
			Pos2::new(1.0, 0.55),
			Pos2::new(2.0, -0.50),
			Pos2::new(3.0, 0.48),
			Pos2::new(4.0, -0.42),
			Pos2::new(5.0, 0.36),
			Pos2::new(6.0, -0.28),
			Pos2::new(7.0, 0.18),
			Pos2::new(8.0, 0.0),
		],
		points: vec![Pos2::new(0.0, 0.0), Pos2::new(4.0, 0.04), Pos2::new(8.0, 0.0)],
		model_state: FrozenBrushModelState {
			filtered_input_point: Pos2::new(8.0, 0.0),
			modeled_point: Pos2::new(8.0, 0.0),
			modeled_velocity: Vec2::ZERO,
			modeled_elapsed_seconds: 0.05,
		},
		started_at: Instant::now(),
		last_sample_at: Instant::now(),
	};
	let preview = OverlaySession::preview_frozen_brush_points(&active_stroke);
	let (min_y, max_y) = preview.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |acc, point| {
		(acc.0.min(point.y), acc.1.max(point.y))
	});

	assert_eq!(preview.first().copied(), active_stroke.raw_points.first().copied());
	assert_eq!(preview.last().copied(), active_stroke.raw_points.last().copied());
	assert!(
		max_y - min_y <= 0.20,
		"expected preview to stay close to the modeled centerline instead of exposing raw wobble: {preview:?}"
	);
}

#[test]
fn rendered_live_frozen_brush_wave_preview_avoids_hard_inflection_kinks() {
	let raw_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(4.0, 1.8),
		Pos2::new(8.0, -1.7),
		Pos2::new(12.0, 1.9),
		Pos2::new(16.0, -1.8),
		Pos2::new(20.0, 1.7),
		Pos2::new(24.0, 0.0),
	];
	let started_at = Instant::now();
	let mut stroke = OverlaySession::new_active_frozen_brush_stroke(raw_points[0], started_at);

	for (index, point) in raw_points.iter().copied().enumerate().skip(1) {
		let sampled_at = started_at
			+ Duration::from_secs_f32(
				index as f32 * overlay::FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS,
			);

		OverlaySession::append_frozen_brush_raw_sample(&mut stroke, point, sampled_at);
	}

	let preview = OverlaySession::preview_frozen_brush_points(&stroke);
	let rendered = OverlaySession::rendered_frozen_brush_points(
		&preview,
		overlay::FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
	);
	let max_turn_angle = rendered.windows(3).fold(0.0_f32, |max_turn, window| {
		max_turn.max(OverlaySession::frozen_brush_turn_angle(window[0], window[1], window[2]))
	});

	assert!(
		significant_y_direction_reversals(&rendered, 0.12) >= 2,
		"expected live preview to keep visible oscillation: {rendered:?}"
	);
	assert!(
		max_turn_angle <= 0.48,
		"expected live preview to round inflections instead of producing hard kinks: {rendered:?}"
	);
}

#[test]
fn rendered_live_frozen_brush_arc_preview_avoids_corner_snap() {
	let raw_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(2.4, 0.2),
		Pos2::new(4.7, 1.1),
		Pos2::new(6.6, 2.8),
		Pos2::new(7.7, 5.1),
		Pos2::new(7.8, 7.8),
		Pos2::new(6.8, 10.2),
		Pos2::new(4.9, 12.0),
	];
	let started_at = Instant::now();
	let mut stroke = OverlaySession::new_active_frozen_brush_stroke(raw_points[0], started_at);

	for (index, point) in raw_points.iter().copied().enumerate().skip(1) {
		let sampled_at = started_at
			+ Duration::from_secs_f32(
				index as f32 * overlay::FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS,
			);

		OverlaySession::append_frozen_brush_raw_sample(&mut stroke, point, sampled_at);
	}

	let preview = OverlaySession::preview_frozen_brush_points(&stroke);
	let rendered = OverlaySession::rendered_frozen_brush_points(
		&preview,
		overlay::FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
	);
	let max_turn_angle = rendered.windows(3).fold(0.0_f32, |max_turn, window| {
		max_turn.max(OverlaySession::frozen_brush_turn_angle(window[0], window[1], window[2]))
	});

	assert!(
		max_turn_angle <= 0.42,
		"expected sustained arc preview to stay rounded instead of preserving a corner: {rendered:?}"
	);
}

#[test]
fn rendered_live_frozen_brush_suppresses_slow_straight_wobble() {
	let raw_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(0.45, 0.18),
		Pos2::new(0.9, -0.15),
		Pos2::new(1.35, 0.17),
		Pos2::new(1.8, -0.13),
		Pos2::new(2.25, 0.16),
		Pos2::new(2.7, -0.12),
		Pos2::new(3.15, 0.14),
		Pos2::new(3.6, -0.10),
		Pos2::new(4.05, 0.12),
		Pos2::new(4.5, -0.09),
		Pos2::new(4.95, 0.10),
		Pos2::new(5.4, -0.08),
		Pos2::new(5.85, 0.08),
		Pos2::new(6.3, -0.07),
		Pos2::new(6.75, 0.06),
		Pos2::new(7.2, -0.05),
		Pos2::new(7.6, 0.03),
		Pos2::new(8.0, 0.0),
	];
	let started_at = Instant::now();
	let mut stroke = OverlaySession::new_active_frozen_brush_stroke(raw_points[0], started_at);

	for (index, point) in raw_points.iter().copied().enumerate().skip(1) {
		let sampled_at = started_at
			+ Duration::from_secs_f32(
				index as f32 * overlay::FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS,
			);

		OverlaySession::append_frozen_brush_raw_sample(&mut stroke, point, sampled_at);
	}

	let preview = OverlaySession::preview_frozen_brush_points(&stroke);
	let rendered = OverlaySession::rendered_frozen_brush_points(
		&preview,
		overlay::FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS,
	);
	let (min_y, max_y) = rendered.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |acc, point| {
		(acc.0.min(point.y), acc.1.max(point.y))
	});

	assert!(
		significant_y_direction_reversals(&rendered, 0.03) <= 1,
		"expected slow straight preview to suppress visible wobble reversals: {rendered:?}"
	);
	assert!(
		max_y - min_y <= 0.26,
		"expected slow straight preview to stay close to a single line: {rendered:?}"
	);
}

#[test]
fn frozen_brush_model_response_follows_fast_strokes_more_closely() {
	let points = [Pos2::new(0.0, 0.0), Pos2::new(16.0, 0.0)];
	let slow = OverlaySession::frozen_brush_input_response(&points, 16.0 / 120.0);
	let fast = OverlaySession::frozen_brush_input_response(&points, 16.0 / 1_600.0);

	assert!(slow < fast);
	assert!(slow >= overlay::FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MIN);
	assert!(fast <= overlay::FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MAX);
}

#[test]
fn frozen_brush_model_response_boosts_sustained_curve_motion() {
	let straight_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(2.0, 0.0),
		Pos2::new(4.0, 0.0),
		Pos2::new(6.0, 0.0),
		Pos2::new(8.0, 0.0),
	];
	let curved_points = [
		Pos2::new(0.0, 0.0),
		Pos2::new(2.0, 0.5),
		Pos2::new(3.7, 1.6),
		Pos2::new(5.0, 3.2),
		Pos2::new(5.8, 5.1),
	];
	let straight = OverlaySession::frozen_brush_input_response(&straight_points, 2.0 / 120.0);
	let curved = OverlaySession::frozen_brush_input_response(&curved_points, 2.0 / 120.0);

	assert!(
		curved > straight,
		"expected sustained curved motion to get a higher live response than straight motion: straight={straight}, curved={curved}"
	);
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
fn window_matte_mosaic_export_and_ocr_match_preview_pixels() {
	let monitor = test_monitor_with_scale(8, 8, 1_000);
	let capture_rect = RectPoints::new(2, 1, 4, 4);
	let window_id = 7;
	let background = image::RgbaImage::from_pixel(8, 8, Rgba([18, 24, 32, 255]));
	let window_image = image::RgbaImage::from_fn(4, 4, |x, y| {
		let alpha = match (x + y) % 4 {
			0 => 64,
			1 => 112,
			2 => 176,
			_ => 224,
		};

		Rgba([
			40_u8.saturating_add((x * 37) as u8),
			28_u8.saturating_add((y * 41) as u8),
			52_u8.saturating_add(((x + y) * 23) as u8),
			alpha,
		])
	});
	let mut session = OverlaySession::new();

	session.config.window_capture_alpha_mode = WindowCaptureAlphaMode::MatteLight;

	session.state.begin_freeze(monitor);

	session.state.frozen_capture_rect = Some(capture_rect);
	session.frozen_capture_source = FrozenCaptureSource::Window;
	session.inflight_window_freeze_capture =
		Some(crate::overlay::WindowFreezeCaptureTarget { monitor, window_id, rect: capture_rect });

	session.handle_captured_freeze_response(
		monitor,
		background,
		Some(window_image),
		Some(window_id),
	);

	assert!(session.authoritative_frozen_capture_ready);
	assert!(session.apply_frozen_mosaic_edit(capture_rect));

	let expected_export = imageops::crop_imm(
		session
			.state
			.frozen_image
			.as_ref()
			.expect("window matte preview should populate the frozen image"),
		capture_rect.x,
		capture_rect.y,
		capture_rect.width,
		capture_rect.height,
	)
	.to_image();

	assert_eq!(session.current_export_image().as_ref(), Some(&expected_export));

	let control = session.begin_ocr_action();
	let OverlayControl::Exit(OverlayExit::DeferredTextRecognition(request)) = control else {
		panic!("expected deferred OCR exit");
	};

	assert_eq!(request.export_image().as_ref(), Some(&expected_export));
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

#[test]
fn begin_frozen_text_edit_at_starts_text_input_inside_capture_rect() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert_eq!(
		session.frozen_text_edit.as_ref().map(|edit| edit.anchor),
		Some(Pos2::new(140.0, 160.0))
	);
	assert!(!session.frozen_selection_drag.active);
}

#[test]
fn begin_frozen_text_edit_at_ignores_non_authoritative_monitor() {
	let monitor = test_monitor();
	let other_monitor = MonitorRect {
		id: 2,
		origin: GlobalPoint::new(1_000, 0),
		width: monitor.width,
		height: monitor.height,
		scale_factor_x1000: monitor.scale_factor_x1000,
	};
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(!session.begin_frozen_text_edit_at(other_monitor, GlobalPoint::new(1_140, 160)));
	assert!(session.frozen_text_edit.is_none());
}

#[test]
fn default_frozen_text_style_uses_16_point_font() {
	let session = OverlaySession::new();

	assert_eq!(
		session.toolbar_state.text_style.font_size_points,
		overlay::FROZEN_TEXT_FONT_SIZE_POINTS
	);
	assert_eq!(session.toolbar_state.text_style.font_size_points, 16.0);
	assert_eq!(session.toolbar_state.text_style.color, FrozenTextColor::Blue);
}

fn test_frozen_mosaic_edit() -> FrozenMosaicEdit {
	let patch = FrozenImagePatch {
		rect: RectPoints::new(0, 0, 1, 1),
		before: image::RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])),
		after: image::RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255])),
	};

	FrozenMosaicEdit { preview_patch: patch.clone(), window_patch: Some(patch) }
}

#[test]
fn frozen_text_edit_drag_repositions_anchor_within_capture_rect() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.begin_frozen_text_edit_drag_at(monitor, GlobalPoint::new(141, 161)));
	assert!(session.update_frozen_text_edit_drag_anchor(GlobalPoint::new(200, 210)));
	assert_eq!(
		session.frozen_text_edit.as_ref().map(|edit| edit.anchor),
		Some(Pos2::new(199.0, 209.0))
	);
	assert!(session.stop_frozen_text_edit_drag());
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.dragging), Some(false));
}

#[test]
fn toolbar_mouse_release_stops_active_frozen_text_edit_drag() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.begin_frozen_text_edit_drag_at(monitor, GlobalPoint::new(141, 161)));

	session.toolbar_left_button_down = true;

	let _ = session.handle_toolbar_mouse_input(ElementState::Released);

	assert!(!session.toolbar_left_button_down);
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.dragging), Some(false));
}

#[test]
fn adjacent_text_events_from_key_and_ime_are_deduplicated() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));

	let key_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Key,
		key_generation,
		"A",
	));

	let ime_generation = session.note_frozen_text_input_event();

	assert!(!session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Ime,
		ime_generation,
		"A",
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("A"));

	let _ = session.finish_frozen_text_editing(false);

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(150, 165)));

	let ime_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Ime,
		ime_generation,
		"B",
	));

	let key_generation = session.note_frozen_text_input_event();

	assert!(!session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Key,
		key_generation,
		"B",
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("B"));
}

#[test]
fn non_adjacent_identical_text_events_from_different_sources_are_not_deduplicated() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));

	let key_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Key,
		key_generation,
		"A",
	));

	let _ = session.note_frozen_text_input_event();
	let ime_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Ime,
		ime_generation,
		"A",
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("AA"));
}

#[test]
fn backspace_clears_recent_input_dedupe_marker_before_cross_source_retype() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));

	let key_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Key,
		key_generation,
		"A",
	));
	assert!(session.backspace_frozen_text_edit());
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some(""));

	let ime_generation = session.note_frozen_text_input_event();

	assert!(session.append_text_to_frozen_edit_for_input_event(
		FrozenTextInputSource::Ime,
		ime_generation,
		"A",
	));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("A"));
}

#[test]
fn ime_disabled_clears_frozen_text_preedit_state() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((0, 0))));
	assert!(session.frozen_text_edit.as_ref().is_some_and(FrozenTextEditState::has_ime_preedit));
	assert!(session.apply_frozen_text_ime_event(&Ime::Disabled));
	assert_eq!(
		session.frozen_text_edit.as_ref().and_then(|edit| edit.ime_preedit.as_deref()),
		None
	);
	assert!(!session.frozen_text_edit.as_ref().is_some_and(FrozenTextEditState::has_ime_preedit));
}

#[test]
fn frozen_text_preedit_cursor_range_updates_caret_position() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("A"));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉字")), Some((3, 3))));

	let edit_state = session.frozen_text_edit.as_ref().expect("text edit");

	assert_eq!(edit_state.visible_text(), "A汉字");
	assert_eq!(edit_state.visible_text_and_caret_char_index().1, Some(2));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉字")), Some((0, 0))));
	assert_eq!(
		session
			.frozen_text_edit
			.as_ref()
			.and_then(|edit| edit.visible_text_and_caret_char_index().1),
		Some(1)
	);
}

#[test]
fn frozen_text_style_change_refresh_check_requires_active_ime_preedit() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(!session.should_refresh_frozen_text_ime_cursor_area_for_text_style_change(monitor));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((0, 0))));
	assert!(session.should_refresh_frozen_text_ime_cursor_area_for_text_style_change(monitor));
}

#[test]
fn frozen_text_style_change_refresh_check_ignores_other_monitor() {
	let monitor = test_monitor();
	let other_monitor = MonitorRect {
		id: 2,
		origin: GlobalPoint::new(1_000, 0),
		width: monitor.width,
		height: monitor.height,
		scale_factor_x1000: monitor.scale_factor_x1000,
	};
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((0, 0))));
	assert!(
		!session.should_refresh_frozen_text_ime_cursor_area_for_text_style_change(other_monitor)
	);
}

#[test]
fn frozen_text_enter_does_not_finish_while_ime_preedit_is_active() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("A"));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((3, 3))));
	assert!(!session.handle_frozen_text_pressed_key(&Key::Named(NamedKey::Enter), None));
	assert_eq!(session.frozen_text_edit.as_ref().map(|edit| edit.text.as_str()), Some("A"));
	assert_eq!(
		session.frozen_text_edit.as_ref().and_then(|edit| edit.ime_preedit.as_deref()),
		Some("汉")
	);
	assert!(session.frozen_text_annotations.is_empty());
}

#[test]
fn frozen_text_caret_repaint_schedules_delayed_repaint_while_editing() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.frozen_text_edit = Some(FrozenTextEditState::new(Pos2::new(120.0, 140.0)));
	*session.egui_repaint_deadline.lock().unwrap_or_else(|err| err.into_inner()) = None;

	let started_at = Instant::now();

	session.maybe_keep_frozen_text_caret_repaint();

	let deadline = session
		.egui_repaint_deadline
		.lock()
		.unwrap_or_else(|err| err.into_inner())
		.expect("caret repaint should be scheduled");

	assert!(deadline >= started_at);
	assert!(
		deadline <= started_at + FROZEN_TEXT_CARET_REPAINT_INTERVAL + Duration::from_millis(20)
	);
}

#[test]
fn finish_frozen_text_editing_commits_current_toolbar_text_style() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;
	session.toolbar_state.text_style.font_size_points = 30.0;
	session.toolbar_state.text_style.color = FrozenTextColor::Yellow;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("Styled"));
	assert!(session.finish_frozen_text_editing(true));

	let annotation = session.frozen_text_annotations.first().expect("annotation");

	assert_eq!(annotation.style.font_size_points, 30.0);
	assert_eq!(annotation.style.color, FrozenTextColor::Yellow);
}

#[test]
fn finish_frozen_text_editing_commits_active_ime_preedit_text() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("A"));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((3, 3))));
	assert!(session.finish_frozen_text_editing(true));
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_text_annotations[0].text, "A汉");
}

#[test]
fn inline_toolbar_mode_switch_finishes_active_frozen_text_edit() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.authoritative_frozen_capture_ready = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("Switched"));

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pointer;

	let _ = session.handle_capture_and_toolbar_redraw_post(monitor, true);

	assert!(session.frozen_text_edit.is_none());
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_text_annotations[0].text, "Switched");
	assert_eq!(session.toolbar_state.selected_tool, FrozenToolbarTool::Pointer);
}

#[test]
fn inline_toolbar_mode_switch_commits_active_ime_preedit_text() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.authoritative_frozen_capture_ready = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("A"));
	assert!(session.set_frozen_text_ime_preedit(Some(String::from("汉")), Some((3, 3))));

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pointer;

	let _ = session.handle_capture_and_toolbar_redraw_post(monitor, true);

	assert!(session.frozen_text_edit.is_none());
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_text_annotations[0].text, "A汉");
}

#[test]
fn frozen_text_undo_and_redo_round_trip_annotations() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(100, 120, 220, 180));
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(140, 160)));
	assert!(session.append_text_to_frozen_edit("Undoable"));
	assert!(session.finish_frozen_text_editing(true));
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_text_annotations.is_empty());
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_text_annotations[0].text, "Undoable");
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn current_export_image_renders_frozen_text_annotations() {
	let monitor = test_monitor_with_scale(160, 120, 1_000);
	let base = image::RgbaImage::from_pixel(160, 120, Rgba([0, 0, 0, 255]));
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, base);

	session.state.frozen_capture_rect = Some(RectPoints::new(10, 12, 120, 80));

	session.frozen_text_annotations.push(FrozenTextAnnotation {
		anchor: Pos2::new(24.0, 24.0),
		text: String::from("Text"),
		style: session.toolbar_state.text_style,
	});
	session.push_frozen_edit_to_undo_history(FrozenEditKind::TextAnnotation);

	let export = session.current_export_image().expect("export image");

	assert_eq!(export.dimensions(), (120, 80));
	assert!(export.pixels().any(|pixel| *pixel != Rgba([0, 0, 0, 255])));
}

#[test]
fn frozen_export_transform_uses_actual_export_image_dimensions() {
	let capture_rect = RectPoints::new(10, 12, 20, 10);
	let transform = FrozenExportTransform::new(capture_rect, 60, 30).expect("transform");

	assert_eq!(transform.point_to_pixels(Pos2::new(10.0, 12.0)), Pos2::new(0.0, 0.0));
	assert_eq!(transform.point_to_pixels(Pos2::new(20.0, 17.0)), Pos2::new(30.0, 15.0));
	assert_eq!(transform.point_to_pixels(Pos2::new(30.0, 22.0)), Pos2::new(60.0, 30.0));
	assert_eq!(transform.scalar_scale(), 3.0);
}

#[test]
fn frozen_committed_overlay_iteration_preserves_cross_tool_order() {
	let monitor = test_monitor();
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session
		.state
		.finish_freeze(monitor, image::RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 255])));

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 16, 16));
	session.authoritative_frozen_capture_ready = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(2, 2)));
	assert!(session.finish_frozen_brush_stroke());

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(6, 6)));
	assert!(session.append_text_to_frozen_edit("middle"));
	assert!(session.finish_frozen_text_editing(true));

	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(10, 10)));
	assert!(session.finish_frozen_brush_stroke());

	let mut observed = Vec::new();

	OverlaySession::for_each_frozen_committed_overlay(
		&session.frozen_edit_undo_stack,
		&session.frozen_brush.committed_strokes,
		&session.frozen_text_annotations,
		|overlay| match overlay {
			FrozenCommittedOverlay::Brush(stroke) => {
				observed.push(format!("brush:{:.0}", stroke.points[0].x));
			},
			FrozenCommittedOverlay::Text(annotation) => {
				observed.push(format!("text:{}", annotation.text));
			},
		},
	);

	assert_eq!(observed, ["brush:2", "text:middle", "brush:10"]);
}

#[test]
fn frozen_annotation_history_undoes_across_tools_in_reverse_commit_order() {
	let monitor = test_monitor_with_scale(8, 8, 1_000);
	let original = image::RgbaImage::from_fn(8, 8, |x, y| {
		Rgba([(x * 17) as u8, (y * 23) as u8, ((x + y) * 11) as u8, 255])
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, original.clone());

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.authoritative_frozen_capture_ready = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Pen;

	assert!(session.begin_frozen_brush_stroke(GlobalPoint::new(2, 2)));
	assert!(session.finish_frozen_brush_stroke());

	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(3, 4)));
	assert!(session.append_text_to_frozen_edit("Layered"));
	assert!(session.finish_frozen_text_editing(true));

	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
	assert!(session.update_frozen_mosaic_drag_rect(GlobalPoint::new(4, 4)));
	assert!(session.commit_frozen_mosaic_drag());

	let mosaiced =
		session.state.frozen_image.clone().expect("mosaic commit should retain the frozen image");

	assert!(session.perform_frozen_undo());
	assert_eq!(session.state.frozen_image.as_ref(), Some(&original));
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.frozen_brush.committed_strokes.len(), 1);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_text_annotations.is_empty());
	assert_eq!(session.frozen_brush.committed_strokes.len(), 1);
	assert!(session.perform_frozen_undo());
	assert!(session.frozen_brush.committed_strokes.is_empty());
	assert!(!session.toolbar_state.undo_available);
	assert!(session.toolbar_state.redo_available);
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_brush.committed_strokes.len(), 1);
	assert!(session.frozen_text_annotations.is_empty());
	assert_eq!(session.state.frozen_image.as_ref(), Some(&original));
	assert!(session.perform_frozen_redo());
	assert_eq!(session.frozen_text_annotations.len(), 1);
	assert_eq!(session.state.frozen_image.as_ref(), Some(&original));
	assert!(session.perform_frozen_redo());
	assert_eq!(session.state.frozen_image.as_ref(), Some(&mosaiced));
	assert!(session.toolbar_state.undo_available);
	assert!(!session.toolbar_state.redo_available);
}

#[test]
fn committing_new_frozen_edit_clears_redo_across_tools() {
	let monitor = test_monitor_with_scale(8, 8, 1_000);
	let base = image::RgbaImage::from_fn(8, 8, |x, y| {
		Rgba([(x * 11) as u8, (y * 13) as u8, ((x + y) * 7) as u8, 255])
	});
	let mut session = OverlaySession::new();

	session.state.begin_freeze(monitor);
	session.state.finish_freeze(monitor, base);

	session.state.frozen_capture_rect = Some(RectPoints::new(0, 0, 8, 8));
	session.authoritative_frozen_capture_ready = true;
	session.toolbar_state.selected_tool = FrozenToolbarTool::Text;

	assert!(session.begin_frozen_text_edit_at(monitor, GlobalPoint::new(2, 2)));
	assert!(session.append_text_to_frozen_edit("redo"));
	assert!(session.finish_frozen_text_editing(true));
	assert!(session.perform_frozen_undo());
	assert!(session.toolbar_state.redo_available);

	session.toolbar_state.selected_tool = FrozenToolbarTool::Mosaic;

	assert!(session.begin_frozen_mosaic_drag(GlobalPoint::new(1, 1)));
	assert!(session.update_frozen_mosaic_drag_rect(GlobalPoint::new(4, 4)));
	assert!(session.commit_frozen_mosaic_drag());
	assert!(!session.toolbar_state.redo_available);
	assert!(!session.perform_frozen_redo());
	assert!(session.frozen_text_annotations.is_empty());
}

#[test]
fn evicting_old_mosaic_history_also_discards_patch_payloads() {
	let mut session = OverlaySession::new();

	for _ in 0..FROZEN_EDIT_HISTORY_LIMIT {
		session.push_frozen_mosaic_edit(test_frozen_mosaic_edit());
		session.push_frozen_edit_to_undo_history(FrozenEditKind::MosaicEdit);
	}

	assert_eq!(session.frozen_mosaic_undo_stack.len(), FROZEN_EDIT_HISTORY_LIMIT);

	for _ in 0..FROZEN_EDIT_HISTORY_LIMIT {
		session.push_frozen_edit_to_undo_history(FrozenEditKind::BrushStroke);
	}

	assert_eq!(session.frozen_edit_undo_stack.len(), FROZEN_EDIT_HISTORY_LIMIT);
	assert!(session.frozen_mosaic_undo_stack.is_empty());
}

#[test]
fn evicting_old_brush_and_text_history_discards_matching_payloads() {
	let mut session = OverlaySession::new();

	for index in 0..(FROZEN_EDIT_HISTORY_LIMIT + 2) {
		let x = index as f32;

		if index % 2 == 0 {
			session
				.frozen_brush
				.committed_strokes
				.push(FrozenBrushStroke { points: vec![Pos2::new(x, 0.0)] });
			session.push_frozen_edit_to_undo_history(FrozenEditKind::BrushStroke);
		} else {
			session.frozen_text_annotations.push(FrozenTextAnnotation {
				anchor: Pos2::new(x, 0.0),
				text: format!("text-{index}"),
				style: session.toolbar_state.text_style,
			});
			session.push_frozen_edit_to_undo_history(FrozenEditKind::TextAnnotation);
		}
	}

	assert_eq!(session.frozen_edit_undo_stack.len(), FROZEN_EDIT_HISTORY_LIMIT);
	assert_eq!(session.frozen_brush.committed_strokes.len(), FROZEN_EDIT_HISTORY_LIMIT / 2);
	assert_eq!(session.frozen_text_annotations.len(), FROZEN_EDIT_HISTORY_LIMIT / 2);
	assert_eq!(session.frozen_brush.committed_strokes[0].points[0], Pos2::new(2.0, 0.0));
	assert_eq!(session.frozen_text_annotations[0].text, "text-3");

	let mut observed = Vec::new();

	OverlaySession::for_each_frozen_committed_overlay(
		&session.frozen_edit_undo_stack,
		&session.frozen_brush.committed_strokes,
		&session.frozen_text_annotations,
		|overlay| match overlay {
			FrozenCommittedOverlay::Brush(stroke) => {
				observed.push(format!("brush:{:.0}", stroke.points[0].x));
			},
			FrozenCommittedOverlay::Text(annotation) => observed.push(annotation.text.clone()),
		},
	);

	let expected = (2..(FROZEN_EDIT_HISTORY_LIMIT + 2))
		.map(
			|index| {
				if index % 2 == 0 { format!("brush:{index}") } else { format!("text-{index}") }
			},
		)
		.collect::<Vec<_>>();

	assert_eq!(observed, expected);
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

	assert!(matches!(
		session
			.scroll_capture
			.live_stream
			.as_ref()
			.and_then(MacLiveFrameStream::debug_last_request_kind),
		Some("refresh_monitor_nonblocking_if_stale") | Some("prime_monitor_nonblocking")
	));
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
		session_active: true,
		window_list_refresh_inflight: true,
		drop_next_window_list_refresh_snapshot: true,
		png_encode_inflight: true,
		pending_self_capture_exception_window_ids_worker_refresh: true,
		pending_startup_aux_live_stream_filter_upgrade: true,
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

	assert!(!session.is_active());
	assert!(!session.window_list_refresh_inflight);
	assert!(!session.drop_next_window_list_refresh_snapshot);
	assert!(!session.png_encode_inflight);
	assert!(!session.pending_self_capture_exception_window_ids_worker_refresh);
	assert!(!session.pending_startup_aux_live_stream_filter_upgrade);
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

#[test]
fn is_active_tracks_explicit_session_state() {
	let inactive = OverlaySession::default();
	let active = OverlaySession { session_active: true, ..OverlaySession::default() };

	assert!(!inactive.is_active());
	assert!(active.is_active());
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
