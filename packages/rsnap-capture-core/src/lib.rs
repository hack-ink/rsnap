//! Platform-neutral capture-session semantics and host/core boundary types.
//!
//! This crate is the durable landing zone for product semantics that should survive
//! the native-host reset. It intentionally contains no window toolkit, AppKit, or
//! `winit` ownership.

pub mod export;
pub mod geometry;
pub mod protocol;
pub mod session;

pub use self::{
	export::{RgbaExportImage, crop_export_image, crop_rgba_image, encode_png_lossless_fast},
	geometry::{
		GlobalPoint, GlobalRect, MonitorRect, MonitorRectPoints, RectPoints, Rgb, WindowHit,
		WindowRect,
	},
	protocol::{
		CaptureMode, CursorIntent, DeferredTextRecognitionOutcome,
		DeferredTextRecognitionOutcomeKind, DeferredTextRecognitionRequest, HostEffectKind,
		HostEvent, HostReport, HostRequest, HudModel, OutputNaming, PermissionKind, PlatformTag,
		PreparedHostEffectRequest, SceneModel, SessionConfig, ToolbarItemKind, ToolbarItemModel,
	},
	session::CaptureSessionCore,
};
