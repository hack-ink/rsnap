use egui::{Rect, Sense, Ui};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState};
use winit::window::{Window, WindowAttributes};

const SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET_MACOS: f32 = -3.0;
const SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET_DEFAULT: f32 = 0.0;
const SAVE_SHORTCUT_LABEL_MACOS: &str = "Cmd+S";
const SAVE_SHORTCUT_LABEL_DEFAULT: &str = "Ctrl+S";

pub(super) fn settings_window_attributes() -> WindowAttributes {
	let attrs = Window::default_attributes()
		.with_title("Settings")
		.with_inner_size(LogicalSize::new(520.0, 360.0))
		.with_resizable(false)
		.with_visible(true);

	#[cfg(target_os = "macos")]
	{
		use winit::platform::macos::WindowAttributesExtMacOS;

		attrs
			.with_titlebar_transparent(true)
			.with_title_hidden(true)
			.with_fullsize_content_view(true)
			.with_movable_by_window_background(false)
	}

	#[cfg(not(target_os = "macos"))]
	{
		attrs
	}
}

pub(super) fn save_shortcut_label() -> &'static str {
	if cfg!(target_os = "macos") { SAVE_SHORTCUT_LABEL_MACOS } else { SAVE_SHORTCUT_LABEL_DEFAULT }
}

pub(super) fn theme_buttons_y_offset() -> f32 {
	if cfg!(target_os = "macos") {
		SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET_MACOS
	} else {
		SETTINGS_TITLEBAR_THEME_BUTTONS_Y_OFFSET_DEFAULT
	}
}

pub(super) fn should_close_from_keyboard(modifiers: ModifiersState, event: &KeyEvent) -> bool {
	cfg!(target_os = "macos")
		&& event.state == ElementState::Pressed
		&& modifiers.super_key()
		&& matches!(&event.logical_key, Key::Character(c) if c.as_str().eq_ignore_ascii_case("w"))
}

pub(super) fn install_titlebar_drag(ui: &mut Ui, bar_rect: Rect, window: &Window) {
	#[cfg(target_os = "macos")]
	{
		let drag_response = ui.interact(
			bar_rect,
			ui.make_persistent_id("settings_titlebar_drag"),
			Sense::click_and_drag(),
		);

		if drag_response.drag_started() {
			let _ = window.drag_window();
		}
	}

	#[cfg(not(target_os = "macos"))]
	let _ = (ui, bar_rect, window);
}
