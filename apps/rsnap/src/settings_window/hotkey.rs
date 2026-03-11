use global_hotkey::hotkey::{Code, HotKey};
use winit::event::KeyEvent;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

use super::CaptureHotkeyNotice;

const CAPTURE_HOTKEY_GUIDANCE_PRESS_NONMOD: &str =
	"Press a non-modifier key to complete the shortcut.";

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
