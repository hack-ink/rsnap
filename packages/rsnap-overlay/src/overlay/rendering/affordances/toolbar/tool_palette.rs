use crate::overlay::rendering::WindowRenderer;
use crate::overlay::{FrozenToolbarState, FrozenToolbarTool};

#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_SCROLL_MODE: [FrozenToolbarTool; 3] =
	[FrozenToolbarTool::Ocr, FrozenToolbarTool::Copy, FrozenToolbarTool::Save];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_SCROLL_MODE: [FrozenToolbarTool; 2] =
	[FrozenToolbarTool::Copy, FrozenToolbarTool::Save];
#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_WITH_SCROLL_AND_AUTO_CENTER: [FrozenToolbarTool; 13] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::AutoCenter,
	FrozenToolbarTool::Scroll,
	FrozenToolbarTool::Ocr,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_WITH_SCROLL_AND_AUTO_CENTER: [FrozenToolbarTool; 12] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::AutoCenter,
	FrozenToolbarTool::Scroll,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_WITH_AUTO_CENTER: [FrozenToolbarTool; 12] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::AutoCenter,
	FrozenToolbarTool::Ocr,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_WITH_AUTO_CENTER: [FrozenToolbarTool; 11] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::AutoCenter,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_WITH_SCROLL: [FrozenToolbarTool; 12] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::Scroll,
	FrozenToolbarTool::Ocr,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_WITH_SCROLL: [FrozenToolbarTool; 11] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::Scroll,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(target_os = "macos")]
const FROZEN_TOOLBAR_TOOLS_WITHOUT_SCROLL: [FrozenToolbarTool; 11] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::Ocr,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];
#[cfg(not(target_os = "macos"))]
const FROZEN_TOOLBAR_TOOLS_WITHOUT_SCROLL: [FrozenToolbarTool; 10] = [
	FrozenToolbarTool::Pointer,
	FrozenToolbarTool::Pen,
	FrozenToolbarTool::Arrow,
	FrozenToolbarTool::Text,
	FrozenToolbarTool::Mosaic,
	FrozenToolbarTool::Spotlight,
	FrozenToolbarTool::Undo,
	FrozenToolbarTool::Redo,
	FrozenToolbarTool::Copy,
	FrozenToolbarTool::Save,
];

impl WindowRenderer {
	pub(in crate::overlay) fn frozen_toolbar_tools(
		toolbar_state: &FrozenToolbarState,
	) -> &'static [FrozenToolbarTool] {
		if toolbar_state.scroll_capture_active {
			&FROZEN_TOOLBAR_TOOLS_SCROLL_MODE
		} else if toolbar_state.auto_center_available && toolbar_state.scroll_capture_available {
			&FROZEN_TOOLBAR_TOOLS_WITH_SCROLL_AND_AUTO_CENTER
		} else if toolbar_state.auto_center_available {
			&FROZEN_TOOLBAR_TOOLS_WITH_AUTO_CENTER
		} else if toolbar_state.scroll_capture_available {
			&FROZEN_TOOLBAR_TOOLS_WITH_SCROLL
		} else {
			&FROZEN_TOOLBAR_TOOLS_WITHOUT_SCROLL
		}
	}
}
