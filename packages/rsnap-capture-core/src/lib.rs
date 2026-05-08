//! Platform-neutral capture-session semantics and host/core boundary types.
//!
//! This crate is the durable landing zone for product semantics that should survive
//! the native-host reset. It intentionally contains no window toolkit, AppKit, or
//! `winit` ownership.

pub mod auto_center;
pub mod capture_frame;
pub mod export;
pub mod geometry;
pub mod minimap;
pub mod mosaic;
pub mod protocol;
pub mod session;

pub use self::{
	auto_center::{
		AutoCenterImageError, auto_center_margin_balance_shift_points,
		detect_auto_center_content_bounds_rgba,
	},
	capture_frame::{
		CaptureFrameBackgroundKind, CaptureFrameBackgroundPlan, CaptureFrameColorStop,
		CaptureFramePlan, CaptureFrameShadow, CaptureFrameSourceKind, CaptureFrameWallpaperRequest,
		capture_frame_aspect_fill_crop_rect, capture_frame_background_plan, capture_frame_plan,
		capture_frame_wallpaper_request_plan,
	},
	export::{DisplayPointRect, frozen_display_crop_rect},
	export::{RgbaExportImage, crop_export_image, crop_rgba_image, encode_png_lossless_fast},
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
	session::CaptureSessionCore,
};
