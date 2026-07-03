//! Shared runtime model enums for overlay session coordination.

#[cfg(target_os = "macos")]
use egui_phosphor::regular::FILE_TEXT;
use egui_phosphor::regular::{
	ARROW_CLOCKWISE, ARROW_COUNTER_CLOCKWISE, ARROW_UP_RIGHT, ARROWS_DOWN_UP, ARROWS_IN_CARDINAL,
	CHECKERBOARD, COPY, CURSOR, FLOPPY_DISK, FRAME_CORNERS, PENCIL_SIMPLE, TEXT_T,
};
use wgpu::SurfaceTexture;

use crate::overlay::live_capture_target::LiveClickCaptureTarget;
use crate::overlay::{
	FrozenArrowAnnotation, FrozenBrushStroke, FrozenTextAnnotation, FrozenToolbarState,
	GlobalPoint, MonitorRect, RectPoints,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Single source of truth for live capture entry.
///
/// State flow:
/// `Idle` -> `HoverWindow` -> `PressPending` -> `DraggingSelection` -> `FrozenFromDrag`
/// `Idle` -> `HoverWindow` -> `PressPending` -> `FrozenFromClick`
///
/// Hover and drag visuals are derived from this state instead of being coordinated through
/// separate button, hover, and drag flags.
pub(super) enum LiveCaptureInteraction {
	#[default]
	Idle,
	HoverWindow {
		monitor: MonitorRect,
		target: LiveClickCaptureTarget,
	},
	PressPending {
		monitor: MonitorRect,
		press_global: GlobalPoint,
		click_target: Option<LiveClickCaptureTarget>,
		release_global: Option<GlobalPoint>,
		released: bool,
	},
	DraggingSelection {
		monitor: MonitorRect,
		press_global: GlobalPoint,
		current_global: GlobalPoint,
	},
	FrozenFromClick {
		monitor: MonitorRect,
		target: LiveClickCaptureTarget,
	},
	FrozenFromDrag {
		monitor: MonitorRect,
		capture_rect: RectPoints,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OverlayEventLoopPhase {
	Idle,
	WindowEvent,
	AboutToWait,
	RedrawDispatch,
	HudRedraw,
	LoupeRedraw,
	ToolbarRedraw,
	OverlayRedraw,
}
impl OverlayEventLoopPhase {
	pub(super) const fn as_str(self) -> &'static str {
		match self {
			Self::Idle => "idle",
			Self::WindowEvent => "window_event",
			Self::AboutToWait => "about_to_wait",
			Self::RedrawDispatch => "redraw_dispatch",
			Self::HudRedraw => "hud_redraw",
			Self::LoupeRedraw => "loupe_redraw",
			Self::ToolbarRedraw => "toolbar_redraw",
			Self::OverlayRedraw => "overlay_window_redraw",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HudTheme {
	Dark,
	Light,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrozenToolbarTool {
	Pointer,
	Pen,
	Arrow,
	Text,
	Mosaic,
	Spotlight,
	Undo,
	Redo,
	AutoCenter,
	Scroll,
	#[cfg(target_os = "macos")]
	Ocr,
	Copy,
	Save,
}
impl FrozenToolbarTool {
	pub(super) const fn label(self) -> &'static str {
		match self {
			Self::Pointer => "Pointer",
			Self::Pen => "Pen",
			Self::Arrow => "Arrow",
			Self::Text => "Text",
			Self::Mosaic => "Mosaic",
			Self::Spotlight => "Spotlight",
			Self::Undo => "Undo",
			Self::Redo => "Redo",
			Self::AutoCenter => "Auto-center (C)",
			Self::Scroll => "Scroll Capture",
			#[cfg(target_os = "macos")]
			Self::Ocr => "Recognize Text",
			Self::Copy => "Copy",
			Self::Save => "Save",
		}
	}

	pub(super) const fn icon(self) -> &'static str {
		match self {
			Self::Pointer => CURSOR,
			Self::Pen => PENCIL_SIMPLE,
			Self::Arrow => ARROW_UP_RIGHT,
			Self::Text => TEXT_T,
			Self::Mosaic => CHECKERBOARD,
			Self::Spotlight => FRAME_CORNERS,
			Self::Undo => ARROW_COUNTER_CLOCKWISE,
			Self::Redo => ARROW_CLOCKWISE,
			Self::AutoCenter => ARROWS_IN_CARDINAL,
			Self::Scroll => ARROWS_DOWN_UP,
			#[cfg(target_os = "macos")]
			Self::Ocr => FILE_TEXT,
			Self::Copy => COPY,
			Self::Save => FLOPPY_DISK,
		}
	}

	pub(super) const fn is_mode_tool(self) -> bool {
		matches!(
			self,
			Self::Pointer | Self::Pen | Self::Arrow | Self::Text | Self::Mosaic | Self::Spotlight
		)
	}

	pub(super) const fn requires_final_capture(self) -> bool {
		match self {
			Self::Pointer
			| Self::Pen
			| Self::Arrow
			| Self::Text
			| Self::AutoCenter
			| Self::Spotlight => false,
			Self::Mosaic | Self::Undo | Self::Redo => true,
			Self::Scroll | Self::Copy | Self::Save => true,
			#[cfg(target_os = "macos")]
			Self::Ocr => true,
		}
	}

	pub(super) fn is_available(self, toolbar_state: &FrozenToolbarState) -> bool {
		match self {
			Self::Undo => toolbar_state.undo_available,
			Self::Redo => toolbar_state.redo_available,
			_ => true,
		}
	}

	pub(super) fn unavailable_label(self, toolbar_state: &FrozenToolbarState) -> &'static str {
		if self.requires_final_capture() && !toolbar_state.final_capture_ready {
			return "Preparing capture...";
		}

		match self {
			Self::Undo => "Nothing to undo",
			Self::Redo => "Nothing to redo",
			_ => "Preparing capture...",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScrollCaptureFrameSource {
	Worker { request_id: u64 },
	LiveStream { frame_seq: u64 },
}
impl ScrollCaptureFrameSource {
	pub(super) const fn as_str(self) -> &'static str {
		match self {
			Self::Worker { .. } => "worker",
			Self::LiveStream { .. } => "live_stream",
		}
	}

	pub(super) const fn worker_request_id(self) -> Option<u64> {
		match self {
			Self::Worker { request_id } => Some(request_id),
			Self::LiveStream { .. } => None,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PngAction {
	Copy,
	Save,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum FrozenCaptureSource {
	#[default]
	None,
	DragRegion,
	Window,
	FullscreenFallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrozenSelectionCorner {
	TopLeft,
	TopRight,
	BottomLeft,
	BottomRight,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum FrozenSelectionInteractionKind {
	#[default]
	Move,
	Resize(FrozenSelectionCorner),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeviceCursorPointSource {
	DevicePoints,
	DevicePixelsFallback,
	EventRecentFallback,
}
impl DeviceCursorPointSource {
	pub(super) const fn as_str(self) -> &'static str {
		match self {
			Self::DevicePoints => "device_points",
			Self::DevicePixelsFallback => "device_pixels_fallback",
			Self::EventRecentFallback => "event_recent_fallback",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SelectionFlowStyle {
	Band,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum WindowRendererPath {
	Overlay,
	LoupeTile,
}
impl WindowRendererPath {
	pub(super) const fn as_str(self) -> &'static str {
		match self {
			Self::Overlay => "overlay",
			Self::LoupeTile => "loupe_tile",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SurfaceFrameSkipReason {
	Timeout,
	Occluded,
}
impl SurfaceFrameSkipReason {
	pub(super) const fn as_str(self) -> &'static str {
		match self {
			Self::Timeout => "timeout",
			Self::Occluded => "occluded",
		}
	}

	pub(super) const fn should_request_redraw(self) -> bool {
		matches!(self, Self::Timeout)
	}
}

pub(super) enum AcquiredSurfaceFrame {
	Ready(SurfaceTexture),
	Skipped(SurfaceFrameSkipReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrozenEditKind {
	BrushStroke,
	MosaicEdit,
	TextAnnotation,
	ArrowAnnotation,
	SpotlightAnnotation,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum FrozenCommittedOverlay<'a> {
	Brush(&'a FrozenBrushStroke),
	Text(&'a FrozenTextAnnotation),
	Arrow(&'a FrozenArrowAnnotation),
}
