use crate::overlay::hud_pill_style::HUD_PILL_STROKE_WIDTH_POINTS;

pub(in crate::overlay) const FROZEN_TOOLBAR_BUTTON_SIZE_POINTS: f32 = 24.0;
pub(in crate::overlay) const FROZEN_TOOLBAR_ITEM_SPACING_POINTS: f32 = 4.0;
pub(in crate::overlay) const TOOLBAR_CAPTURE_GAP_PX: f32 = 10.0;
pub(in crate::overlay) const TOOLBAR_DEFAULT_SLOT_POSITION_EPSILON_POINTS: f32 = 1.0;
pub(in crate::overlay) const TOOLBAR_DRAG_START_THRESHOLD_PX: f32 = 6.0;
pub(in crate::overlay) const TOOLBAR_EXPANDED_HEIGHT_PX: f32 = FROZEN_TOOLBAR_BUTTON_SIZE_POINTS
	+ 2.0 * TOOLBAR_PILL_INNER_MARGIN_Y_POINTS
	+ 2.0 * HUD_PILL_STROKE_WIDTH_POINTS;
pub(in crate::overlay) const TOOLBAR_PILL_INNER_MARGIN_Y_POINTS: f32 = 6.0;
pub(in crate::overlay) const TOOLBAR_SCREEN_MARGIN_PX: f32 = 10.0;
#[cfg(target_os = "macos")]
pub(in crate::overlay) const TOOLBAR_WINDOW_WARMUP_REDRAWS: u8 = 30;
