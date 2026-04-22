//! Platform-neutral capture-session semantics and host/core boundary types.
//!
//! This crate is the durable landing zone for product semantics that should survive
//! the native-host reset. It intentionally contains no window toolkit, AppKit, or
//! `winit` ownership.

pub mod geometry;
pub mod protocol;
pub mod session;

pub use geometry::{
	GlobalPoint, GlobalRect, MonitorRect, MonitorRectPoints, RectPoints, Rgb, WindowHit, WindowRect,
};
pub use protocol::{
	CaptureMode, CursorIntent, DeferredTextRecognitionOutcome, DeferredTextRecognitionOutcomeKind,
	DeferredTextRecognitionRequest, HostEffectKind, HostEvent, HostReport, HostRequest, HudModel,
	OutputNaming, PermissionKind, PlatformTag, PreparedHostEffectRequest, SceneModel,
	SessionConfig, ToolbarItemKind, ToolbarItemModel,
};
pub use session::CaptureSessionCore;
