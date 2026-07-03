use std::ffi::CStr;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

use objc::declare::ClassDecl;
use objc::runtime::{BOOL, Class, NO, Object, Sel, YES};
use objc2_foundation::{NSPoint, NSRange, NSRect, NSSize};
use winit::event::{ElementState, Ime};
use winit::keyboard::{Key, ModifiersState, NamedKey, NativeKey};

use crate::overlay::macos_cursor_runtime::MacOSOverlayPoint;
use crate::overlay::macos_native_capture_shell_runtime::{
	MacOSKeyFocusShellState, MacOSKeyFocusShellTarget, MacOSPassiveShellCallback, MacOSRange,
	MacOSRect,
};
use crate::overlay::{
	MacOSNativeCaptureInputDispatch, MacOSNativeCaptureInputEvent, OverlayKeyboardInputEvent,
};

macro_rules! sel {
	($($tt:tt)*) => {
		objc::sel!($($tt)*)
	};
}

macro_rules! sel_impl {
	($($tt:tt)*) => {
		objc::sel_impl!($($tt)*)
	};
}

pub(super) fn macos_key_focus_shell_panel_class() -> *const Class {
	static CLASS: OnceLock<usize> = OnceLock::new();

	(*CLASS.get_or_init(|| {
		if let Some(class) = Class::get("RsnapKeyFocusCaptureShellPanel") {
			return class as *const Class as usize;
		}

		let superclass = objc::class!(NSWindow);
		let mut decl = ClassDecl::new("RsnapKeyFocusCaptureShellPanel", superclass)
			.expect("key-focus capture shell panel class");

		unsafe {
			decl.add_method(
				objc::sel!(canBecomeMainWindow),
				macos_key_focus_shell_can_become_main_window as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(canBecomeKeyWindow),
				macos_key_focus_shell_can_become_key_window as extern "C" fn(&Object, Sel) -> BOOL,
			);
		}

		decl.register() as *const Class as usize
	})) as *const Class
}

pub(super) fn macos_key_focus_shell_view_class() -> *const Class {
	static CLASS: OnceLock<usize> = OnceLock::new();

	(*CLASS.get_or_init(|| {
		if let Some(class) = Class::get("RsnapKeyFocusCaptureShellView") {
			return class as *const Class as usize;
		}

		let superclass = objc::class!(NSView);
		let mut decl = ClassDecl::new("RsnapKeyFocusCaptureShellView", superclass)
			.expect("key-focus capture shell view class");

		unsafe {
			decl.add_method(
				objc::sel!(isFlipped),
				super::macos_passive_shell_view_is_flipped as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(acceptsFirstResponder),
				macos_key_focus_shell_view_accepts_first_responder
					as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(keyDown:),
				macos_key_focus_shell_view_key_down as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(keyUp:),
				macos_key_focus_shell_view_key_up as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(flagsChanged:),
				macos_key_focus_shell_view_flags_changed
					as extern "C" fn(&Object, Sel, *mut Object),
			);
			decl.add_method(
				objc::sel!(hasMarkedText),
				macos_key_focus_shell_view_has_marked_text as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(markedRange),
				macos_key_focus_shell_view_marked_range
					as extern "C" fn(&Object, Sel) -> MacOSRange,
			);
			decl.add_method(
				objc::sel!(selectedRange),
				macos_key_focus_shell_view_selected_range
					as extern "C" fn(&Object, Sel) -> MacOSRange,
			);
			decl.add_method(
				objc::sel!(setMarkedText:selectedRange:replacementRange:),
				macos_key_focus_shell_view_set_marked_text
					as extern "C" fn(&Object, Sel, *mut Object, MacOSRange, MacOSRange),
			);
			decl.add_method(
				objc::sel!(unmarkText),
				macos_key_focus_shell_view_unmark_text as extern "C" fn(&Object, Sel),
			);
			decl.add_method(
				objc::sel!(validAttributesForMarkedText),
				macos_key_focus_shell_view_valid_attributes_for_marked_text
					as extern "C" fn(&Object, Sel) -> *mut Object,
			);
			decl.add_method(
				objc::sel!(attributedSubstringForProposedRange:actualRange:),
				macos_key_focus_shell_view_attributed_substring_for_proposed_range
					as extern "C" fn(&Object, Sel, MacOSRange, *mut c_void) -> *mut Object,
			);
			decl.add_method(
				objc::sel!(characterIndexForPoint:),
				macos_key_focus_shell_view_character_index_for_point
					as extern "C" fn(&Object, Sel, MacOSOverlayPoint) -> usize,
			);
			decl.add_method(
				objc::sel!(firstRectForCharacterRange:actualRange:),
				macos_key_focus_shell_view_first_rect_for_character_range
					as extern "C" fn(&Object, Sel, MacOSRange, *mut c_void) -> MacOSRect,
			);
			decl.add_method(
				objc::sel!(insertText:replacementRange:),
				macos_key_focus_shell_view_insert_text
					as extern "C" fn(&Object, Sel, *mut Object, MacOSRange),
			);
			decl.add_method(
				objc::sel!(doCommandBySelector:),
				macos_key_focus_shell_view_do_command_by_selector
					as extern "C" fn(&Object, Sel, Sel),
			);
		}

		decl.register() as *const Class as usize
	})) as *const Class
}

extern "C" fn macos_key_focus_shell_can_become_main_window(_this: &Object, _cmd: Sel) -> BOOL {
	YES
}

extern "C" fn macos_key_focus_shell_can_become_key_window(_this: &Object, _cmd: Sel) -> BOOL {
	YES
}

extern "C" fn macos_key_focus_shell_view_accepts_first_responder(
	_this: &Object,
	_cmd: Sel,
) -> BOOL {
	YES
}

fn macos_key_focus_shell_state(
	this: &Object,
) -> Option<(Arc<Mutex<MacOSKeyFocusShellState>>, MacOSNativeCaptureInputDispatch)> {
	let callback = super::macos_shell_callback(this as *const Object as usize)?;

	match callback {
		MacOSPassiveShellCallback::KeyFocus { state, dispatch } => Some((state, dispatch)),
		_ => None,
	}
}

fn macos_nsstring_to_string(string: *mut Object) -> String {
	if string.is_null() {
		return String::new();
	}

	unsafe {
		let utf8: *const i8 = objc::msg_send![string, UTF8String];

		if utf8.is_null() {
			return String::new();
		}

		CStr::from_ptr(utf8).to_string_lossy().into_owned()
	}
}

fn macos_text_input_object_to_string(string: *mut Object) -> String {
	if string.is_null() {
		return String::new();
	}

	unsafe {
		let attributed_string_class = objc::class!(NSAttributedString);
		let is_attributed: BOOL = objc::msg_send![string, isKindOfClass: attributed_string_class];
		let string_object = if is_attributed == YES {
			let value: *mut Object = objc::msg_send![string, string];

			value
		} else {
			string
		};

		macos_nsstring_to_string(string_object)
	}
}

fn macos_utf16_offset_to_utf8(text: &str, utf16_offset: usize) -> usize {
	let mut utf16_count = 0;

	for (byte_index, ch) in text.char_indices() {
		if utf16_count >= utf16_offset {
			return byte_index;
		}

		utf16_count += ch.len_utf16();

		if utf16_count >= utf16_offset {
			return byte_index + ch.len_utf8();
		}
	}

	text.len()
}

fn macos_modifier_state_from_event(event: *mut Object) -> ModifiersState {
	if event.is_null() {
		return ModifiersState::empty();
	}

	const SHIFT: u64 = 1 << 17;
	const CONTROL: u64 = 1 << 18;
	const OPTION: u64 = 1 << 19;
	const COMMAND: u64 = 1 << 20;
	unsafe {
		let flags: u64 = objc::msg_send![event, modifierFlags];
		let mut state = ModifiersState::empty();

		state.set(ModifiersState::SHIFT, flags & SHIFT != 0);
		state.set(ModifiersState::CONTROL, flags & CONTROL != 0);
		state.set(ModifiersState::ALT, flags & OPTION != 0);
		state.set(ModifiersState::SUPER, flags & COMMAND != 0);

		state
	}
}

fn macos_key_focus_logical_key(event: *mut Object) -> Key {
	if event.is_null() {
		return Key::Unidentified(NativeKey::MacOS(0));
	}

	unsafe {
		let key_code: u16 = objc::msg_send![event, keyCode];

		match key_code {
			53 => Key::Named(NamedKey::Escape),
			36 | 76 => Key::Named(NamedKey::Enter),
			51 => Key::Named(NamedKey::Backspace),
			49 => Key::Named(NamedKey::Space),
			48 => Key::Named(NamedKey::Tab),
			_ => {
				let string: *mut Object = objc::msg_send![event, charactersIgnoringModifiers];
				let text = macos_nsstring_to_string(string);

				if text.is_empty() {
					Key::Unidentified(NativeKey::MacOS(key_code))
				} else {
					Key::Character(text.into())
				}
			},
		}
	}
}

fn macos_key_focus_text(event: *mut Object, logical_key: &Key) -> Option<String> {
	if event.is_null() {
		return None;
	}

	match logical_key {
		Key::Named(NamedKey::Escape | NamedKey::Backspace | NamedKey::Tab) => None,
		Key::Named(NamedKey::Enter) => Some(String::from("\r")),
		Key::Named(NamedKey::Space) => Some(String::from(" ")),
		_ => unsafe {
			let string: *mut Object = objc::msg_send![event, characters];
			let text = macos_nsstring_to_string(string);

			(!text.is_empty()).then_some(text)
		},
	}
}

fn macos_key_focus_keyboard_input_from_event(
	event: *mut Object,
	state: ElementState,
) -> OverlayKeyboardInputEvent {
	let logical_key = macos_key_focus_logical_key(event);
	let text = macos_key_focus_text(event, &logical_key);
	let repeat = if event.is_null() {
		false
	} else {
		unsafe {
			let repeat: BOOL = objc::msg_send![event, isARepeat];

			repeat == YES
		}
	};

	OverlayKeyboardInputEvent { logical_key, text, state, repeat }
}

fn macos_key_focus_target(this: &Object) -> Option<MacOSKeyFocusShellTarget> {
	let (state, _) = macos_key_focus_shell_state(this)?;
	let Ok(state) = state.lock() else {
		return None;
	};

	state.target
}

fn macos_key_focus_ime_allowed(this: &Object) -> bool {
	macos_key_focus_target(this).is_some_and(|target| target.ime_allowed)
}

fn macos_key_focus_dispatch_keyboard_input(this: &Object, event: *mut Object, state: ElementState) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let overlay_event = macos_key_focus_keyboard_input_from_event(event, state);
	let monitor =
		shell_state.lock().ok().and_then(|state| state.target.and_then(|target| target.monitor));

	dispatch.enqueue(MacOSNativeCaptureInputEvent::KeyboardInput { monitor, event: overlay_event });
}

extern "C" fn macos_key_focus_shell_view_key_down(this: &Object, _cmd: Sel, event: *mut Object) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let ime_allowed = macos_key_focus_ime_allowed(this);

	if let Ok(mut state) = shell_state.lock() {
		state.forward_key_event_to_app = false;
		state.had_ime_input_during_keydown = false;
		state.keyboard_modifiers = macos_modifier_state_from_event(event);
	}

	dispatch.enqueue(MacOSNativeCaptureInputEvent::ModifiersChanged {
		state: macos_modifier_state_from_event(event),
	});

	if ime_allowed {
		unsafe {
			let array: *mut Object = objc::msg_send![objc::class!(NSArray), arrayWithObject: event];
			let _: () = objc::msg_send![this, interpretKeyEvents: array];
		}
	}

	let should_forward = if let Ok(state) = shell_state.lock() {
		!state.had_ime_input_during_keydown || state.forward_key_event_to_app
	} else {
		true
	};

	if should_forward {
		macos_key_focus_dispatch_keyboard_input(this, event, ElementState::Pressed);
	}
}

extern "C" fn macos_key_focus_shell_view_key_up(this: &Object, _cmd: Sel, event: *mut Object) {
	macos_key_focus_dispatch_keyboard_input(this, event, ElementState::Released);
}

extern "C" fn macos_key_focus_shell_view_flags_changed(
	this: &Object,
	_cmd: Sel,
	event: *mut Object,
) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let modifiers = macos_modifier_state_from_event(event);

	if let Ok(mut state) = shell_state.lock() {
		state.keyboard_modifiers = modifiers;
	}

	dispatch.enqueue(MacOSNativeCaptureInputEvent::ModifiersChanged { state: modifiers });
}

extern "C" fn macos_key_focus_shell_view_has_marked_text(this: &Object, _cmd: Sel) -> BOOL {
	let Some((shell_state, _)) = macos_key_focus_shell_state(this) else {
		return NO;
	};
	let Ok(state) = shell_state.lock() else {
		return NO;
	};

	if state.marked_text.is_empty() { NO } else { YES }
}

extern "C" fn macos_key_focus_shell_view_marked_range(this: &Object, _cmd: Sel) -> MacOSRange {
	let Some((shell_state, _)) = macos_key_focus_shell_state(this) else {
		return MacOSRange::from(NSRange::new(usize::MAX, 0));
	};
	let Ok(state) = shell_state.lock() else {
		return MacOSRange::from(NSRange::new(usize::MAX, 0));
	};

	if state.marked_text.is_empty() {
		MacOSRange::from(NSRange::new(usize::MAX, 0))
	} else {
		MacOSRange::from(NSRange::new(0, state.marked_text.encode_utf16().count()))
	}
}

extern "C" fn macos_key_focus_shell_view_selected_range(this: &Object, _cmd: Sel) -> MacOSRange {
	let _ = this;

	MacOSRange::from(NSRange::new(usize::MAX, 0))
}

extern "C" fn macos_key_focus_shell_view_set_marked_text(
	this: &Object,
	_cmd: Sel,
	string: *mut Object,
	selected_range: MacOSRange,
	_replacement_range: MacOSRange,
) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let text = macos_text_input_object_to_string(string);
	let selected_range = NSRange::from(selected_range);
	let cursor_range = if text.is_empty() {
		None
	} else {
		let start = macos_utf16_offset_to_utf8(&text, selected_range.location);
		let end =
			macos_utf16_offset_to_utf8(&text, selected_range.location + selected_range.length);

		Some((start, end))
	};
	let monitor = if let Ok(mut state) = shell_state.lock() {
		state.marked_text = text.clone();
		state.had_ime_input_during_keydown = true;

		state.target.and_then(|target| target.monitor)
	} else {
		None
	};

	tracing::info!(
		op = "overlay.macos_key_focus_shell_set_marked_text",
		monitor_id = ?monitor.map(|target| target.id),
		text_len = text.chars().count(),
		"Received marked text from the native key-focus shell."
	);

	dispatch.enqueue(MacOSNativeCaptureInputEvent::Ime {
		monitor,
		event: Ime::Preedit(text, cursor_range),
	});
}

extern "C" fn macos_key_focus_shell_view_unmark_text(this: &Object, _cmd: Sel) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let monitor = if let Ok(mut state) = shell_state.lock() {
		state.marked_text.clear();

		state.target.and_then(|target| target.monitor)
	} else {
		None
	};

	unsafe {
		let input_context: *mut Object = objc::msg_send![this, inputContext];

		if !input_context.is_null() {
			let _: () = objc::msg_send![input_context, discardMarkedText];
		}
	}

	dispatch.enqueue(MacOSNativeCaptureInputEvent::Ime {
		monitor,
		event: Ime::Preedit(String::new(), None),
	});
}

extern "C" fn macos_key_focus_shell_view_valid_attributes_for_marked_text(
	_this: &Object,
	_cmd: Sel,
) -> *mut Object {
	unsafe { objc::msg_send![objc::class!(NSArray), array] }
}

extern "C" fn macos_key_focus_shell_view_attributed_substring_for_proposed_range(
	_this: &Object,
	_cmd: Sel,
	_range: MacOSRange,
	_actual_range: *mut c_void,
) -> *mut Object {
	ptr::null_mut::<Object>()
}

extern "C" fn macos_key_focus_shell_view_character_index_for_point(
	_this: &Object,
	_cmd: Sel,
	_point: MacOSOverlayPoint,
) -> usize {
	0
}

extern "C" fn macos_key_focus_shell_view_first_rect_for_character_range(
	this: &Object,
	_cmd: Sel,
	_range: MacOSRange,
	_actual_range: *mut c_void,
) -> MacOSRect {
	let Some(target) = macos_key_focus_target(this) else {
		return MacOSRect::from(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)));
	};
	let rect = NSRect::new(target.ime_origin, target.ime_size);

	unsafe {
		let ns_window: *mut Object = objc::msg_send![this, window];

		if ns_window.is_null() {
			return rect.into();
		}

		let view_rect: NSRect =
			objc::msg_send![this, convertRect: rect toView: ptr::null_mut::<Object>()];
		let screen_rect: NSRect = objc::msg_send![ns_window, convertRectToScreen: view_rect];

		MacOSRect::from(screen_rect)
	}
}

extern "C" fn macos_key_focus_shell_view_insert_text(
	this: &Object,
	_cmd: Sel,
	string: *mut Object,
	_replacement_range: MacOSRange,
) {
	let Some((shell_state, dispatch)) = macos_key_focus_shell_state(this) else {
		return;
	};
	let text = macos_text_input_object_to_string(string);

	if text.is_empty() {
		return;
	}

	let monitor = if let Ok(mut state) = shell_state.lock() {
		state.marked_text.clear();

		state.had_ime_input_during_keydown = true;

		state.target.and_then(|target| target.monitor)
	} else {
		None
	};

	tracing::info!(
		op = "overlay.macos_key_focus_shell_insert_text",
		monitor_id = ?monitor.map(|target| target.id),
		text_len = text.chars().count(),
		"Committed text from the native key-focus shell."
	);

	dispatch.enqueue(MacOSNativeCaptureInputEvent::Ime {
		monitor,
		event: Ime::Preedit(String::new(), None),
	});
	dispatch.enqueue(MacOSNativeCaptureInputEvent::Ime { monitor, event: Ime::Commit(text) });
}

extern "C" fn macos_key_focus_shell_view_do_command_by_selector(
	this: &Object,
	_cmd: Sel,
	_selector: Sel,
) {
	let Some((shell_state, _)) = macos_key_focus_shell_state(this) else {
		return;
	};

	if let Ok(mut state) = shell_state.lock() {
		state.forward_key_event_to_app = true;
	}
}
