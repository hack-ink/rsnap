use egui::Ui;
use egui::Visuals;
use global_hotkey::hotkey::{Code, HotKey};
use winit::event::KeyEvent;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

use crate::settings::AppSettings;
use crate::settings_window::CaptureHotkeyNotice;
use crate::settings_window::SettingsWindow;
use crate::settings_window::{SETTINGS_VALUE_BOX_WIDTH, SettingsWindowAction};

pub(super) const CAPTURE_HOTKEY_GUIDANCE_PRESS_NONMOD: &str =
	"Press a non-modifier key to complete the shortcut.";

impl CaptureHotkeyNotice {
	pub(super) fn as_rich_text(&self, visuals: &Visuals) -> egui::RichText {
		match self {
			Self::Error(text) => {
				egui::RichText::new(text).color(egui::Color32::from_rgb(255, 130, 130))
			},
			Self::Hint(text) => egui::RichText::new(text).color(visuals.weak_text_color()),
			Self::Success(text) => {
				egui::RichText::new(text).color(egui::Color32::from_rgb(120, 200, 120))
			},
		}
	}
}

impl SettingsWindow {
	pub fn set_capture_hotkey_recording_active(&mut self, active: bool) {
		if self.capture_hotkey_recording == active {
			return;
		}

		self.capture_hotkey_recording = active;

		self.window.request_redraw();
	}

	pub fn set_capture_hotkey_notice(&mut self, notice: Option<CaptureHotkeyNotice>) {
		self.capture_hotkey_notice = notice;

		self.window.request_redraw();
	}

	fn begin_recording_capture_hotkey(&mut self) {
		self.capture_hotkey_recording = true;
		self.capture_hotkey_notice = None;

		self.queue_action(SettingsWindowAction::Begin);
		self.window.request_redraw();
	}

	pub(super) fn cancel_recording_capture_hotkey(&mut self) {
		if !self.capture_hotkey_recording {
			return;
		}

		self.capture_hotkey_recording = false;
		self.capture_hotkey_notice = None;

		self.queue_action(SettingsWindowAction::Cancel);
		self.window.request_redraw();
	}

	pub(super) fn handle_capture_hotkey_recording_input(&mut self, event: &KeyEvent) {
		match self::handle_capture_hotkey_recording_input(&self.modifiers, event) {
			CaptureHotkeyInput::Cancel => self.cancel_recording_capture_hotkey(),
			CaptureHotkeyInput::Notice(notice) => {
				self.capture_hotkey_notice = Some(notice);

				self.window.request_redraw();
			},
			CaptureHotkeyInput::Apply(hotkey) => {
				self.capture_hotkey_notice = None;

				self.window.request_redraw();
				self.queue_action(SettingsWindowAction::Apply(hotkey));
			},
		}
	}

	pub(crate) fn format_capture_hotkey(raw: &str) -> String {
		self::format_capture_hotkey(raw)
	}

	pub(super) fn render_hotkeys_section(&mut self, ui: &mut Ui, settings: &AppSettings) -> bool {
		let row_height = ui.spacing().interact_size.y;
		let value_width = ui.spacing().slider_width;
		let button_width = SETTINGS_VALUE_BOX_WIDTH;
		let row_label = "Capture hotkey";
		let display_label = if self.capture_hotkey_recording {
			format_capture_hotkey_recording_label(&self.modifiers)
		} else {
			Self::format_capture_hotkey(&settings.capture_hotkey)
		};
		let hover_text = if self.capture_hotkey_recording {
			"Press a non-modifier key to capture hotkey."
		} else {
			"Click Record to change capture hotkey."
		};
		let mut field_rect = egui::Rect::NOTHING;
		let mut button_rect = egui::Rect::NOTHING;

		ui.horizontal(|ui| {
			let (value_rect, value_response) =
				ui.allocate_exact_size(egui::vec2(value_width, row_height), egui::Sense::click());
			let visuals = ui.visuals().widgets.inactive;
			let corner_radius = visuals.corner_radius;

			ui.painter().rect_filled(value_rect, corner_radius, visuals.bg_fill);
			ui.painter().rect_stroke(
				value_rect,
				corner_radius,
				visuals.bg_stroke,
				egui::StrokeKind::Inside,
			);

			let text_rect = value_rect.shrink2(egui::vec2(6.0, 0.0));
			let font_id = egui::TextStyle::Body.resolve(ui.style());
			let painter = ui.painter().with_clip_rect(text_rect);

			painter.text(
				text_rect.left_center(),
				egui::Align2::LEFT_CENTER,
				&display_label,
				font_id,
				ui.visuals().text_color(),
			);

			if value_response.clicked() && !self.capture_hotkey_recording {
				self.begin_recording_capture_hotkey();
			}

			let button_response =
				ui.add_sized(egui::vec2(button_width, row_height), egui::Button::new("Rec"));

			field_rect = value_rect;
			button_rect = button_response.rect;

			if button_response.clicked() && !self.capture_hotkey_recording {
				self.begin_recording_capture_hotkey();
			}

			ui.label(row_label);
			value_response.on_hover_text(format!("{display_label}\n{hover_text}"));
			button_response.on_hover_text("Record a new hotkey");
		});

		if self.capture_hotkey_recording
			&& ui.ctx().input(|i| i.pointer.primary_clicked())
			&& let Some(pointer_pos) = ui.ctx().input(|i| i.pointer.interact_pos())
			&& !field_rect.contains(pointer_pos)
			&& !button_rect.contains(pointer_pos)
		{
			self.cancel_recording_capture_hotkey();
		}

		if let Some(notice) = &self.capture_hotkey_notice {
			ui.small(notice.as_rich_text(ui.visuals()));
		}

		false
	}
}

pub(super) enum CaptureHotkeyInput {
	Cancel,
	Notice(CaptureHotkeyNotice),
	Apply(HotKey),
}

pub(super) fn handle_capture_hotkey_recording_input(
	modifiers: &ModifiersState,
	event: &KeyEvent,
) -> CaptureHotkeyInput {
	let PhysicalKey::Code(physical_key) = event.physical_key else {
		return CaptureHotkeyInput::Notice(CaptureHotkeyNotice::Error(String::from(
			"Unsupported key for hotkey binding.",
		)));
	};

	if physical_key == KeyCode::Escape {
		return CaptureHotkeyInput::Cancel;
	}
	if is_modifier_keycode(physical_key) {
		return CaptureHotkeyInput::Notice(CaptureHotkeyNotice::Hint(String::from(
			CAPTURE_HOTKEY_GUIDANCE_PRESS_NONMOD,
		)));
	}

	let Some(code) = to_global_hotkey_code(physical_key) else {
		return CaptureHotkeyInput::Notice(CaptureHotkeyNotice::Error(String::from(
			"Unsupported key for hotkey binding.",
		)));
	};
	let (modifiers, has_required) = capture_modifiers(modifiers);

	if !has_required {
		return CaptureHotkeyInput::Notice(CaptureHotkeyNotice::Error(String::from(
			"Please include Alt, Ctrl, or Super in the shortcut.",
		)));
	}

	CaptureHotkeyInput::Apply(HotKey::new(Some(modifiers), code))
}

pub(super) fn format_capture_hotkey_recording_label(modifiers: &ModifiersState) -> String {
	let modifiers = capture_hotkey_modifiers_label(modifiers);

	if modifiers.is_empty() { String::from("…") } else { format!("{modifiers}+…") }
}

pub(super) fn format_capture_hotkey(raw: &str) -> String {
	let Ok(hotkey) = raw.parse::<HotKey>() else {
		return raw.to_owned();
	};
	let (shift, control, alt, command) = {
		(
			hotkey.mods.contains(global_hotkey::hotkey::Modifiers::SHIFT),
			hotkey.mods.contains(global_hotkey::hotkey::Modifiers::CONTROL),
			hotkey.mods.contains(global_hotkey::hotkey::Modifiers::ALT),
			hotkey.mods.contains(global_hotkey::hotkey::Modifiers::SUPER),
		)
	};

	if cfg!(target_os = "macos") {
		let mut parts = Vec::new();

		if command {
			parts.push(String::from("Cmd"));
		}
		if control {
			parts.push(String::from("Ctrl"));
		}
		if alt {
			parts.push(String::from("Opt"));
		}
		if shift {
			parts.push(String::from("Shift"));
		}

		parts.push(format_capture_key_code(hotkey.key));

		return parts.join("+");
	}

	let mut parts = Vec::new();

	if shift {
		parts.push(String::from("Shift"));
	}
	if control {
		parts.push(String::from("Ctrl"));
	}
	if alt {
		parts.push(String::from("Alt"));
	}
	if command {
		parts.push(String::from("Super"));
	}

	parts.push(format_capture_key_code(hotkey.key));

	parts.join("+")
}

fn is_modifier_keycode(physical_key: KeyCode) -> bool {
	matches!(
		physical_key,
		KeyCode::ShiftLeft
			| KeyCode::ShiftRight
			| KeyCode::ControlLeft
			| KeyCode::ControlRight
			| KeyCode::AltLeft
			| KeyCode::AltRight
			| KeyCode::SuperLeft
			| KeyCode::SuperRight,
	)
}

fn capture_hotkey_modifiers_label(modifiers: &ModifiersState) -> String {
	let mut parts = Vec::new();

	if modifiers.super_key() {
		parts.push(String::from("Cmd"));
	}
	if modifiers.control_key() {
		parts.push(String::from("Ctrl"));
	}
	if modifiers.alt_key() {
		parts.push(String::from("Opt"));
	}
	if modifiers.shift_key() {
		parts.push(String::from("Shift"));
	}

	parts.join("+")
}

fn format_capture_key_code(code: Code) -> String {
	let raw = code.to_string();

	if let Some(letter) = raw.strip_prefix("Key")
		&& letter.len() == 1
	{
		return letter.to_string();
	}
	if let Some(digit) = raw.strip_prefix("Digit")
		&& digit.len() == 1
	{
		return digit.to_string();
	}

	match raw.as_str() {
		"Escape" => String::from("Esc"),
		"Backquote" => String::from("`"),
		"Backslash" => String::from("\\"),
		"BracketLeft" => String::from("["),
		"BracketRight" => String::from("]"),
		"Comma" => String::from(","),
		"Equal" => String::from("="),
		"Minus" => String::from("-"),
		"Period" => String::from("."),
		"Quote" => String::from("'"),
		"Semicolon" => String::from(";"),
		"Slash" => String::from("/"),
		"Backspace" => String::from("Backspace"),
		"CapsLock" => String::from("CapsLock"),
		"Enter" => String::from("Enter"),
		"Space" => String::from("Space"),
		"Tab" => String::from("Tab"),
		"Delete" => String::from("Delete"),
		"Home" => String::from("Home"),
		"End" => String::from("End"),
		"Insert" => String::from("Insert"),
		"PageUp" => String::from("PageUp"),
		"PageDown" => String::from("PageDown"),
		"ArrowUp" => String::from("Up"),
		"ArrowDown" => String::from("Down"),
		"ArrowLeft" => String::from("Left"),
		"ArrowRight" => String::from("Right"),
		"NumpadAdd" => String::from("Num+"),
		"NumpadSubtract" => String::from("Num-"),
		"NumpadMultiply" => String::from("Num*"),
		"NumpadDivide" => String::from("Num/"),
		"NumpadDecimal" => String::from("Num."),
		"NumpadEqual" => String::from("Num="),
		"NumLock" => String::from("NumLock"),
		"NumpadEnter" => String::from("NumEnter"),
		_ => raw,
	}
}

fn capture_modifiers(modifiers: &ModifiersState) -> (global_hotkey::hotkey::Modifiers, bool) {
	let mut output = global_hotkey::hotkey::Modifiers::empty();
	let mut has_required = false;

	if modifiers.alt_key() {
		output.insert(global_hotkey::hotkey::Modifiers::ALT);

		has_required = true;
	}
	if modifiers.control_key() {
		output.insert(global_hotkey::hotkey::Modifiers::CONTROL);

		has_required = true;
	}
	if modifiers.super_key() {
		output.insert(global_hotkey::hotkey::Modifiers::SUPER);

		has_required = true;
	}
	if modifiers.shift_key() {
		output.insert(global_hotkey::hotkey::Modifiers::SHIFT);
	}

	(output, has_required)
}

fn to_global_hotkey_code(key_code: KeyCode) -> Option<Code> {
	let mut debug_name = format!("{key_code:?}");

	if debug_name == "SuperLeft" {
		debug_name = String::from("MetaLeft");
	} else if debug_name == "SuperRight" {
		debug_name = String::from("MetaRight");
	}

	debug_name.parse::<Code>().ok()
}
