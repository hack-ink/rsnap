mod annotation_runtime;
mod export_actions;
mod live_runtime;
mod rendering_behaviors;
mod scroll_capture_runtime;
mod scroll_capture_runtime_support;
mod scroll_input_runtime;
mod self_capture_runtime;
mod session_runtime;
mod stream_refresh_runtime;
mod toolbar_runtime;
mod worker_observation_runtime;
mod worker_tick_runtime;

#[cfg(target_os = "macos")]
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

#[cfg(target_os = "macos")]
use color_eyre::eyre::Result;
use egui::FontDefinitions;
use egui::RawInput;
use image::Rgba;
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
use crate::live_frame_stream_macos::MacLiveFrameStream;
#[cfg(target_os = "macos")]
use crate::live_frame_stream_macos::STREAM_REGION_FRAME_MAX_AGE;
use crate::overlay::FrozenCaptureSource;
#[cfg(target_os = "macos")]
use crate::overlay::LiveCaptureInteraction;
use crate::overlay::PngAction;
use crate::overlay::frozen_edit_history_runtime::FROZEN_EDIT_HISTORY_LIMIT;
use crate::overlay::frozen_text_runtime::FROZEN_TEXT_CARET_REPAINT_INTERVAL;
use crate::overlay::rendering;
#[cfg(target_os = "macos")]
use crate::overlay::session_state::ScrollCaptureLiveFrame;
use crate::overlay::session_state::{
	FrozenBrushStyle, FrozenCaptureSessionState, FrozenCaptureWorkerState,
	FrozenExportSessionState, WindowFreezeCaptureTarget,
};
use crate::overlay::toolbar_geometry::{TOOLBAR_CAPTURE_GAP_PX, TOOLBAR_SCREEN_MARGIN_PX};
use crate::overlay::{
	self, ActiveFrozenBrushStroke, FrozenAnnotationColor, FrozenArrowAnnotation,
	FrozenBrushModelState, FrozenBrushStroke, FrozenCommittedOverlay, FrozenEditKind,
	FrozenExportTransform, FrozenImagePatch, FrozenMosaicEdit, FrozenSelectionDragState,
	FrozenSpotlightAnnotation, FrozenTextAnnotation, FrozenTextEditState, FrozenTextInputSource,
	FrozenToolbarState, FrozenToolbarTool, HudRedrawSummary, HudTheme, OutputNaming,
	OverlayControl, OverlaySession, Pos2, PreparedHostEffectRequest, Rect,
	SCROLL_CAPTURE_SAMPLE_INTERVAL, SelectionDashedBorderCache, SelectionFlowGeometryCache,
	SelectionSizeBadgeTarget, SurfaceFrameSkipReason, ToolbarPlacement, Vec2,
	WindowCaptureAlphaMode, WindowRenderer, hud_helpers,
};
#[cfg(target_os = "macos")]
use crate::overlay::{
	HudPillGeometry, InflightScrollCaptureObservation, LiveSampleApplyResult, LiveStreamStaleGrace,
	MacOSScrollPixelResidual, OverlayExit, SCROLL_CAPTURE_ACTIVE_GESTURE_STALE_REFRESH_DEAD_WINDOW,
	SCROLL_CAPTURE_INPUT_FRESHNESS, SCROLL_CAPTURE_LIVE_STREAM_STALE_GRACE_FRAMES,
	SCROLL_CAPTURE_MOUSE_PASSTHROUGH_IDLE_GRACE, ScrollCaptureFrameSource,
	ScrollCaptureHostAdapter, ScrollCaptureHostFrameRequestError, StartupLiveRgbPlan,
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
use crate::worker::{WorkerErrorSource, WorkerRequestSendError, WorkerResponse};

fn make_scroll_capture_test_image(width: u32, rows: &[[u8; 4]]) -> image::RgbaImage {
	scroll_capture_runtime_support::make_scroll_capture_test_image(width, rows)
}

fn make_scroll_capture_window(
	document: &[[u8; 4]],
	width: u32,
	start_row: usize,
	window_rows: usize,
) -> image::RgbaImage {
	scroll_capture_runtime_support::make_scroll_capture_window(
		document,
		width,
		start_row,
		window_rows,
	)
}

#[cfg(target_os = "macos")]
fn make_sparse_worker_capture_window(width: u32, height: u32, start_row: u32) -> image::RgbaImage {
	scroll_capture_runtime_support::make_sparse_worker_capture_window(width, height, start_row)
}

#[cfg(target_os = "macos")]
fn make_browser_like_worker_capture_window(
	width: u32,
	height: u32,
	start_row: u32,
) -> image::RgbaImage {
	scroll_capture_runtime_support::make_browser_like_worker_capture_window(
		width, height, start_row,
	)
}

fn set_scroll_capture_input(session: &mut OverlaySession, direction: ScrollDirection) {
	scroll_capture_runtime_support::set_scroll_capture_input(session, direction);
}

#[cfg(target_os = "macos")]
fn enable_test_worker_scroll_capture_path(session: &mut OverlaySession) {
	scroll_capture_runtime_support::enable_test_worker_scroll_capture_path(session);
}

#[cfg(target_os = "macos")]
fn seed_worker_scroll_capture_session(
	session: &mut OverlaySession,
	monitor: MonitorRect,
	rect: RectPoints,
	base: image::RgbaImage,
	frames: impl IntoIterator<Item = Option<image::RgbaImage>>,
) {
	scroll_capture_runtime_support::seed_worker_scroll_capture_session(
		session, monitor, rect, base, frames,
	);
}

#[cfg(target_os = "macos")]
fn drain_scroll_capture_worker_until_idle(session: &mut OverlaySession) {
	scroll_capture_runtime_support::drain_scroll_capture_worker_until_idle(session);
}

fn observe_scroll_capture_frame(
	session: &mut OverlaySession,
	frame: image::RgbaImage,
) -> Option<ScrollObserveOutcome> {
	scroll_capture_runtime_support::observe_scroll_capture_frame(session, frame)
}

fn scroll_capture_export_height(session: &OverlaySession) -> u32 {
	scroll_capture_runtime_support::scroll_capture_export_height(session)
}

fn session_pending_freeze_capture(session: &OverlaySession) -> Option<MonitorRect> {
	match session.frozen_capture_session_state {
		FrozenCaptureSessionState::DisplayPending {
			monitor,
			worker_state: FrozenCaptureWorkerState::Idle | FrozenCaptureWorkerState::Armed,
			..
		} => Some(monitor),
		FrozenCaptureSessionState::DisplayReady { monitor, export } => match export {
			FrozenExportSessionState::Pending {
				worker_state: FrozenCaptureWorkerState::Idle | FrozenCaptureWorkerState::Armed,
				..
			} => Some(monitor),
			FrozenExportSessionState::Pending {
				worker_state: FrozenCaptureWorkerState::Inflight,
				..
			}
			| FrozenExportSessionState::Ready
			| FrozenExportSessionState::Failed => None,
		},
		FrozenCaptureSessionState::Inactive
		| FrozenCaptureSessionState::DisplayFailed { .. }
		| FrozenCaptureSessionState::DisplayPending {
			worker_state: FrozenCaptureWorkerState::Inflight,
			..
		} => None,
	}
}

fn session_inflight_freeze_capture(session: &OverlaySession) -> Option<MonitorRect> {
	match session.frozen_capture_session_state {
		FrozenCaptureSessionState::DisplayPending {
			monitor,
			worker_state: FrozenCaptureWorkerState::Inflight,
			..
		}
		| FrozenCaptureSessionState::DisplayReady {
			monitor,
			export:
				FrozenExportSessionState::Pending {
					worker_state: FrozenCaptureWorkerState::Inflight,
					..
				},
		} => Some(monitor),
		FrozenCaptureSessionState::Inactive
		| FrozenCaptureSessionState::DisplayFailed { .. }
		| FrozenCaptureSessionState::DisplayPending { .. }
		| FrozenCaptureSessionState::DisplayReady { .. } => None,
	}
}

fn session_pending_window_freeze_capture(
	session: &OverlaySession,
) -> Option<WindowFreezeCaptureTarget> {
	match session.frozen_capture_session_state {
		FrozenCaptureSessionState::DisplayPending {
			worker_state: FrozenCaptureWorkerState::Idle | FrozenCaptureWorkerState::Armed,
			window_target,
			..
		} => window_target,
		FrozenCaptureSessionState::DisplayReady { export, .. } => match export {
			FrozenExportSessionState::Pending {
				worker_state: FrozenCaptureWorkerState::Idle | FrozenCaptureWorkerState::Armed,
				window_target,
			} => window_target,
			FrozenExportSessionState::Pending {
				worker_state: FrozenCaptureWorkerState::Inflight,
				..
			}
			| FrozenExportSessionState::Ready
			| FrozenExportSessionState::Failed => None,
		},
		FrozenCaptureSessionState::Inactive
		| FrozenCaptureSessionState::DisplayFailed { .. }
		| FrozenCaptureSessionState::DisplayPending {
			worker_state: FrozenCaptureWorkerState::Inflight,
			..
		} => None,
	}
}

fn session_inflight_window_freeze_capture(
	session: &OverlaySession,
) -> Option<WindowFreezeCaptureTarget> {
	match session.frozen_capture_session_state {
		FrozenCaptureSessionState::DisplayPending {
			worker_state: FrozenCaptureWorkerState::Inflight,
			window_target,
			..
		}
		| FrozenCaptureSessionState::DisplayReady {
			export:
				FrozenExportSessionState::Pending {
					worker_state: FrozenCaptureWorkerState::Inflight,
					window_target,
				},
			..
		} => window_target,
		FrozenCaptureSessionState::Inactive
		| FrozenCaptureSessionState::DisplayFailed { .. }
		| FrozenCaptureSessionState::DisplayPending { .. }
		| FrozenCaptureSessionState::DisplayReady { .. } => None,
	}
}

fn session_frozen_capture_armed(session: &OverlaySession) -> bool {
	matches!(
		session.frozen_capture_session_state,
		FrozenCaptureSessionState::DisplayPending {
			worker_state: FrozenCaptureWorkerState::Armed,
			..
		} | FrozenCaptureSessionState::DisplayReady {
			export: FrozenExportSessionState::Pending {
				worker_state: FrozenCaptureWorkerState::Armed,
				..
			},
			..
		}
	)
}

#[cfg(target_os = "macos")]
fn session_export_authority_ready(session: &OverlaySession) -> bool {
	matches!(
		session.frozen_capture_session_state,
		FrozenCaptureSessionState::DisplayReady { export: FrozenExportSessionState::Ready, .. }
	)
}

fn set_session_frozen_capture_state(
	session: &mut OverlaySession,
	monitor: Option<MonitorRect>,
	worker_state: FrozenCaptureWorkerState,
	window_target: Option<WindowFreezeCaptureTarget>,
) {
	let Some(monitor) = monitor else {
		session.frozen_capture_session_state = FrozenCaptureSessionState::Inactive;

		return;
	};
	let display_ready = matches!(session.state.mode, OverlayMode::Frozen)
		&& session.state.monitor == Some(monitor)
		&& session.state.frozen_display_surface_image().is_some();

	session.frozen_capture_session_state = if display_ready {
		FrozenCaptureSessionState::DisplayReady {
			monitor,
			export: FrozenExportSessionState::Pending { worker_state, window_target },
		}
	} else {
		FrozenCaptureSessionState::DisplayPending { monitor, worker_state, window_target }
	};
}

fn set_session_pending_freeze_capture(session: &mut OverlaySession, monitor: Option<MonitorRect>) {
	let worker_state = if session_frozen_capture_armed(session) {
		FrozenCaptureWorkerState::Armed
	} else {
		FrozenCaptureWorkerState::Idle
	};
	let window_target = session_pending_window_freeze_capture(session)
		.or_else(|| session_inflight_window_freeze_capture(session));

	set_session_frozen_capture_state(session, monitor, worker_state, window_target);
}

fn set_session_pending_window_freeze_capture(
	session: &mut OverlaySession,
	window_target: Option<WindowFreezeCaptureTarget>,
) {
	let monitor = session_pending_freeze_capture(session)
		.or_else(|| session_inflight_freeze_capture(session))
		.or_else(|| window_target.map(|target| target.monitor));
	let worker_state = match session_inflight_freeze_capture(session) {
		Some(_) => FrozenCaptureWorkerState::Inflight,
		None if session_frozen_capture_armed(session) => FrozenCaptureWorkerState::Armed,
		None => FrozenCaptureWorkerState::Idle,
	};

	set_session_frozen_capture_state(session, monitor, worker_state, window_target);
}

fn set_session_inflight_freeze_capture(session: &mut OverlaySession, monitor: Option<MonitorRect>) {
	let window_target = session_pending_window_freeze_capture(session)
		.or_else(|| session_inflight_window_freeze_capture(session));

	set_session_frozen_capture_state(
		session,
		monitor,
		FrozenCaptureWorkerState::Inflight,
		window_target,
	);
}

fn set_session_inflight_window_freeze_capture(
	session: &mut OverlaySession,
	window_target: Option<WindowFreezeCaptureTarget>,
) {
	let monitor = session_inflight_freeze_capture(session)
		.or_else(|| session_pending_freeze_capture(session))
		.or_else(|| window_target.map(|target| target.monitor));

	set_session_frozen_capture_state(
		session,
		monitor,
		FrozenCaptureWorkerState::Inflight,
		window_target,
	);
}

fn set_session_pending_freeze_capture_armed(session: &mut OverlaySession, armed: bool) {
	let monitor = session_pending_freeze_capture(session)
		.or_else(|| session_inflight_freeze_capture(session));
	let window_target = session_pending_window_freeze_capture(session)
		.or_else(|| session_inflight_window_freeze_capture(session));
	let worker_state = if session_inflight_freeze_capture(session).is_some() {
		FrozenCaptureWorkerState::Inflight
	} else if armed {
		FrozenCaptureWorkerState::Armed
	} else {
		FrozenCaptureWorkerState::Idle
	};

	set_session_frozen_capture_state(session, monitor, worker_state, window_target);
}

fn promote_session_export_authority_ready(session: &mut OverlaySession) {
	let monitor = session
		.state
		.monitor
		.or_else(|| session_pending_freeze_capture(session))
		.or_else(|| session_inflight_freeze_capture(session))
		.unwrap_or_else(test_monitor);

	if session.state.frozen_export_image.is_none()
		&& let Some(display_image) = session.state.frozen_display_image.clone()
	{
		session.state.commit_frozen_export_image(display_image);
	}

	session.frozen_capture_session_state = FrozenCaptureSessionState::DisplayReady {
		monitor,
		export: FrozenExportSessionState::Ready,
	};
}

fn finish_frozen_display_state(
	session: &mut OverlaySession,
	monitor: MonitorRect,
	image: image::RgbaImage,
) {
	session.state.commit_frozen_display_image(monitor, image);

	session.frozen_capture_session_state = FrozenCaptureSessionState::DisplayReady {
		monitor,
		export: FrozenExportSessionState::Pending {
			worker_state: FrozenCaptureWorkerState::Idle,
			window_target: None,
		},
	};
}

fn finish_frozen_ready_state(
	session: &mut OverlaySession,
	monitor: MonitorRect,
	image: image::RgbaImage,
) {
	session.state.commit_frozen_final_image(monitor, image);

	session.frozen_capture_session_state = FrozenCaptureSessionState::DisplayReady {
		monitor,
		export: FrozenExportSessionState::Ready,
	};
}

#[cfg(target_os = "macos")]
fn commit_frozen_display_preview_state(
	session: &mut OverlaySession,
	monitor: MonitorRect,
	image: image::RgbaImage,
) {
	session.state.commit_frozen_display_image(monitor, image);

	session.frozen_capture_session_state = FrozenCaptureSessionState::DisplayReady {
		monitor,
		export: FrozenExportSessionState::Pending {
			worker_state: FrozenCaptureWorkerState::Idle,
			window_target: session_pending_window_freeze_capture(session)
				.or_else(|| session_inflight_window_freeze_capture(session)),
		},
	};
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
fn fresh_live_stream_snapshot_captured_at() -> Instant {
	// Bias "fresh snapshot" fixtures away from nextest scheduling jitter so these tests assert
	// display-first semantics, not whether the process lost >90ms of CPU between two lines.
	Instant::now() + Duration::from_secs(1)
}

#[cfg(target_os = "macos")]
fn stale_live_stream_snapshot_captured_at() -> Instant {
	Instant::now() - STREAM_REGION_FRAME_MAX_AGE - Duration::from_millis(1)
}

#[cfg(target_os = "macos")]
fn seed_ready_scroll_capture_selection(session: &mut OverlaySession) {
	let monitor = test_monitor_with_scale(8, 8, 1_000);

	session.state.begin_freeze(monitor);

	finish_frozen_display_state(session, monitor, test_frozen_image());

	session.state.frozen_capture_rect = Some(RectPoints::new(1, 1, 4, 4));
	session.frozen_capture_source = FrozenCaptureSource::DragRegion;

	promote_session_export_authority_ready(session);
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

fn test_frozen_mosaic_edit() -> FrozenMosaicEdit {
	let patch = FrozenImagePatch {
		rect: RectPoints::new(0, 0, 1, 1),
		before: image::RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])),
		after: image::RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255])),
	};

	FrozenMosaicEdit {
		preview_patch: patch.clone(),
		export_patch: patch.clone(),
		window_patch: Some(patch),
	}
}
