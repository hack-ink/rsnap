//! Platform-neutral capture-session semantics and host/core boundary types.
//!
//! This crate is the durable landing zone for product semantics that should survive
//! the native-host reset. It intentionally contains no window toolkit, AppKit, or
//! `winit` ownership.

pub mod auto_center;
pub mod bgra_frame;
pub mod capture_frame;
pub mod export;
pub mod geometry;
pub mod minimap;
pub mod mosaic;
pub mod protocol;
pub mod scroll_stitching;
pub mod selection_transform;
pub mod session;
pub mod wallpaper;

mod scroll_capture;

pub use self::{
	auto_center::{
		AutoCenterImageError, auto_center_margin_balance_shift_points,
		detect_auto_center_content_bounds_rgba,
	},
	bgra_frame::{BgraFrameView, loupe_patch_rgba_from_bgra_frame, sample_rgb_from_bgra_frame},
	capture_frame::{
		CaptureFrameBackgroundKind, CaptureFrameBackgroundPlan, CaptureFrameColorStop,
		CaptureFramePlan, CaptureFrameRenderImageRef, CaptureFrameRenderKind, CaptureFrameShadow,
		CaptureFrameSourceKind, CaptureFrameWallpaperRequest, capture_frame_aspect_fill_crop_rect,
		capture_frame_background_plan, capture_frame_plan, capture_frame_wallpaper_request_plan,
		render_capture_frame_effect,
	},
	export::{DisplayPointRect, frozen_display_crop_rect},
	export::{
		RgbaExportImage, crop_export_image, crop_rgba_image, encode_png_lossless_fast,
		encode_png_lossless_fast_with_screen_scale,
	},
	geometry::{
		GlobalPoint, GlobalRect, MonitorRect, MonitorRectPoints, RectPoints, Rgb, WindowHit,
		WindowRect,
	},
	minimap::{ScrollMinimapInput, ScrollMinimapPlan, scroll_minimap_plan},
	mosaic::frozen_mosaic_light_privacy_patch,
	protocol::{
		CaptureMode, CursorIntent, DeferredTextRecognitionOutcome,
		DeferredTextRecognitionOutcomeKind, DeferredTextRecognitionRequest, HostEffectKind,
		HostEvent, HostReport, HostRequest, HudModel, OutputNaming, PermissionKind, PlatformTag,
		PreparedHostEffectRequest, SceneModel, SessionConfig, ToolbarItemKind, ToolbarItemModel,
	},
	scroll_capture::bench_support::{
		ScrollCaptureBenchHarness, ScrollCaptureBenchScenario, ScrollCaptureFingerprintMetrics,
		ScrollCaptureOverlapMetrics, ScrollCaptureSessionMetrics,
	},
	scroll_stitching::{ScrollStitchImage, ScrollStitchObserveOutcome, ScrollStitchSession},
	selection_transform::{
		FrozenSelectionTransformInput, FrozenSelectionTransformKind,
		frozen_selection_transform_hit_test, frozen_selection_transform_rect,
	},
	session::CaptureSessionCore,
	wallpaper::{
		capture_frame_wallpaper_png_thumbnail, capture_frame_wallpaper_png_thumbnail_cached,
	},
};

#[cfg(test)]
use criterion as _;
