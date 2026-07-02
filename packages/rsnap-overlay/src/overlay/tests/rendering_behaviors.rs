mod annotation_rendering;
mod capture_affordances;
mod cursor_rects;
mod freeze_handoff;
mod scroll_preview;
mod selection_runtime;
mod toolbar_layout;

#[cfg(target_os = "macos")]
use std::sync::Arc;

use egui::Id;
use egui::LayerId;
use egui::Order;
use egui::RawInput;
use egui::Ui;
use image::RgbaImage;
#[cfg(target_os = "macos")]
use objc::runtime::Object;
use winit::window::CursorIcon;

use crate::overlay::OverlayControl;
#[cfg(target_os = "macos")]
use crate::overlay::WindowCaptureAlphaMode;
#[cfg(target_os = "macos")]
use crate::overlay::frozen_selection_runtime;
use crate::overlay::rendering::FROZEN_TEXT_CARET_BLINK_PERIOD_SECS;
use crate::overlay::session_state::{
	FROZEN_TEXT_FONT_SIZE_POINTS, FrozenAnnotationStyleCapsulePlacement, WindowFreezeCaptureTarget,
};
use crate::overlay::tests::{
	self, ElementState, FrozenCaptureSource, FrozenSelectionDragState, FrozenToolbarState,
	FrozenToolbarTool, GlobalPoint, HUD_LOUPE_STRIP_GAP_POINTS, HudTheme, MonitorRect,
	MonitorRectPoints, MouseButton, OverlayMode, OverlaySession, OverlayState, PngAction, Pos2,
	Rect, RectPoints, Rgba, SELECTION_SIZE_BADGE_GAP_PX, SELECTION_SIZE_BADGE_INSIDE_MARGIN_PX,
	SELECTION_SIZE_BADGE_SCREEN_MARGIN_PX, ScrollSession, SelectionDashedBorderCache,
	SelectionFlowGeometryCache, SelectionSizeBadgeTarget, TOOLBAR_CAPTURE_GAP_PX,
	TOOLBAR_SCREEN_MARGIN_PX, ToolbarPlacement, Vec2, WindowRenderer, overlay,
};
use crate::overlay::{
	FontId, FrozenAnnotationColor, FrozenEditKind, FrozenSelectionCorner,
	FrozenSelectionInteractionKind, FrozenTextAnnotation, FrozenTextEditState,
};
#[cfg(target_os = "macos")]
use crate::state::MonitorImageSnapshot;
use crate::worker::{WorkerErrorSource, WorkerResponse};

fn test_mosaic_source_image() -> RgbaImage {
	RgbaImage::from_fn(8, 8, |x, y| {
		Rgba([(x * 17) as u8, (y * 23) as u8, ((x + y) * 11) as u8, 255])
	})
}

fn average_patch_color(image: &RgbaImage, x: u32, y: u32, width: u32, height: u32) -> Rgba<u8> {
	let mut sum = [0_u64; 4];
	let mut samples = 0_u64;

	for py in y..y.saturating_add(height) {
		for px in x..x.saturating_add(width) {
			let pixel = image.get_pixel(px, py);

			sum[0] += u64::from(pixel[0]);
			sum[1] += u64::from(pixel[1]);
			sum[2] += u64::from(pixel[2]);
			sum[3] += u64::from(pixel[3]);
			samples += 1;
		}
	}

	Rgba([
		(sum[0] / samples) as u8,
		(sum[1] / samples) as u8,
		(sum[2] / samples) as u8,
		(sum[3] / samples) as u8,
	])
}
