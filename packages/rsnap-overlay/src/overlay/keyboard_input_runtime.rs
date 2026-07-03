use winit::event::KeyEvent;

#[cfg(target_os = "macos")]
use crate::overlay::FrozenGlobalHotkey;
#[cfg(not(target_os = "macos"))]
use crate::overlay::WindowId;
use crate::overlay::{
	ElementState, FrozenTextEditState, FrozenTextInputSource, Ime, Key, MonitorRect, NamedKey,
	OverlayControl, OverlayKeyboardInputEvent, OverlayMode, OverlaySession, PngAction,
};

impl OverlaySession {
	#[cfg(not(target_os = "macos"))]
	pub(super) fn handle_ime_event(&mut self, window_id: WindowId, event: &Ime) -> OverlayControl {
		let monitor =
			self.windows.get(&window_id).map(|window| window.monitor).or(self.state.monitor);

		self.handle_overlay_ime_event(monitor, event)
	}

	pub(super) fn handle_overlay_ime_event(
		&mut self,
		monitor: Option<MonitorRect>,
		event: &Ime,
	) -> OverlayControl {
		if !matches!(self.state.mode, OverlayMode::Frozen) || self.frozen_text_edit.is_none() {
			return OverlayControl::Continue;
		}

		let Some(monitor) = monitor.or(self.state.monitor) else {
			return OverlayControl::Continue;
		};
		let changed = self.apply_frozen_text_ime_event(event);

		if changed {
			self.sync_frozen_text_ime_cursor_area(monitor);
			self.request_redraw_for_monitor(monitor);
		}

		OverlayControl::Continue
	}

	pub(super) fn apply_frozen_text_ime_event(&mut self, event: &Ime) -> bool {
		match event {
			Ime::Commit(text) => {
				let generation = self.note_frozen_text_input_event();

				self.append_text_to_frozen_edit_for_input_event(
					FrozenTextInputSource::Ime,
					generation,
					text,
				)
			},
			Ime::Preedit(text, cursor_range) => {
				self.set_frozen_text_ime_preedit(Some(text.clone()), *cursor_range)
			},
			Ime::Disabled => self.set_frozen_text_ime_preedit(None, None),
			Ime::Enabled => false,
		}
	}

	fn handle_frozen_text_key_event(
		&mut self,
		event: &OverlayKeyboardInputEvent,
	) -> Option<OverlayControl> {
		self.frozen_text_edit.as_ref()?;

		if event.state != ElementState::Pressed {
			return Some(OverlayControl::Continue);
		}

		let changed =
			self.handle_frozen_text_pressed_key(&event.logical_key, event.text.as_deref());

		if changed {
			self.sync_text_input_ime_state();

			if let Some(monitor) = self.state.monitor {
				self.sync_frozen_text_ime_cursor_area(monitor);
				self.request_redraw_for_monitor(monitor);
			}
		}

		Some(OverlayControl::Continue)
	}

	pub(super) fn handle_frozen_text_pressed_key(
		&mut self,
		logical_key: &Key,
		text: Option<&str>,
	) -> bool {
		match logical_key {
			Key::Named(NamedKey::Escape) => {
				let _ = self.finish_frozen_text_editing(false);

				true
			},
			Key::Named(NamedKey::Enter) => {
				if self.frozen_text_edit.as_ref().is_some_and(FrozenTextEditState::has_ime_preedit)
				{
					return false;
				}
				if self.keyboard_modifiers.shift_key() {
					let generation = self.note_frozen_text_input_event();

					self.append_text_to_frozen_edit_for_input_event(
						FrozenTextInputSource::Key,
						generation,
						"\n",
					)
				} else {
					let _ = self.finish_frozen_text_editing(true);

					true
				}
			},
			Key::Named(NamedKey::Backspace) => self.backspace_frozen_text_edit(),
			_ if !self.keyboard_modifiers.control_key()
				&& !self.keyboard_modifiers.super_key()
				&& !self.keyboard_modifiers.alt_key() =>
			{
				let Some(text) = text else {
					return false;
				};
				let generation = self.note_frozen_text_input_event();

				self.append_text_to_frozen_edit_for_input_event(
					FrozenTextInputSource::Key,
					generation,
					text,
				)
			},
			_ => false,
		}
	}

	#[cfg(target_os = "macos")]
	/// Handles a host-level Escape hotkey press when the overlay is active.
	pub fn handle_global_escape_hotkey(&mut self) -> OverlayControl {
		if self.frozen_text_edit.is_some() {
			let changed = self.handle_frozen_text_pressed_key(&Key::Named(NamedKey::Escape), None);

			if changed {
				self.sync_text_input_ime_state();

				if let Some(monitor) = self.state.monitor {
					self.sync_frozen_text_ime_cursor_area(monitor);
					self.request_redraw_for_monitor(monitor);
				}
			}

			return OverlayControl::Continue;
		}
		if self.scroll_capture.active {
			return self.cancel_overlay("global_escape_hotkey_scroll_capture");
		}

		self.cancel_overlay("global_escape_hotkey")
	}

	#[cfg(target_os = "macos")]
	/// Handles a host-level Tab hotkey press/release while the live overlay is active.
	pub fn handle_global_loupe_hotkey(&mut self, pressed: bool) -> OverlayControl {
		if self.apply_loupe_activation_key_event(pressed, false) {
			return self.request_redraw_for_alt_state_change();
		}

		OverlayControl::Continue
	}

	#[cfg(target_os = "macos")]
	/// Handles a host-level frozen shortcut while ordinary frozen mode runs without a key window.
	pub fn handle_global_frozen_hotkey(&mut self, hotkey: FrozenGlobalHotkey) -> OverlayControl {
		if !self.wants_global_frozen_hotkeys() {
			return OverlayControl::Continue;
		}

		match hotkey {
			FrozenGlobalHotkey::Copy => self.handle_frozen_copy_hotkey(),
			FrozenGlobalHotkey::AutoCenter => self.handle_frozen_auto_center_hotkey(),
			FrozenGlobalHotkey::ToggleToolbar => self.toggle_toolbar_visibility_hotkey(),
			FrozenGlobalHotkey::StartScrollCapture => self.handle_frozen_scroll_capture_hotkey(),
			FrozenGlobalHotkey::Save => self.handle_frozen_save_hotkey(),
		}
	}

	pub(super) fn handle_key_event(&mut self, event: &KeyEvent) -> OverlayControl {
		self.handle_overlay_keyboard_input_event(&OverlayKeyboardInputEvent::from_winit(event))
	}

	pub(super) fn handle_overlay_keyboard_input_event(
		&mut self,
		event: &OverlayKeyboardInputEvent,
	) -> OverlayControl {
		if matches!(self.state.mode, OverlayMode::Frozen)
			&& let Some(control) = self.handle_frozen_text_key_event(event)
		{
			return control;
		}
		if matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
			let pressed = event.state == ElementState::Pressed;

			if self.apply_loupe_activation_key_event(pressed, event.repeat) {
				return self.request_redraw_for_alt_state_change();
			}

			return OverlayControl::Continue;
		}
		if event.state != ElementState::Pressed {
			return OverlayControl::Continue;
		}
		if event.repeat {
			return OverlayControl::Continue;
		}
		if self.scroll_capture.active {
			return self.handle_scroll_capture_key_event(event);
		}

		match &event.logical_key {
			Key::Named(NamedKey::Escape) => self.cancel_overlay("escape_key"),
			Key::Character(key_text)
				if (key_text == "h" || key_text == "H")
					&& self.plain_character_shortcut_available() =>
			{
				self.toggle_toolbar_visibility_hotkey()
			},
			Key::Character(key_text)
				if key_text.as_str().eq_ignore_ascii_case("c")
					&& self.plain_character_shortcut_available() =>
			{
				self.handle_frozen_auto_center_hotkey()
			},
			Key::Character(key_text)
				if key_text.as_str().eq_ignore_ascii_case("s")
					&& self.is_save_shortcut_pressed() =>
			{
				self.handle_frozen_save_hotkey()
			},
			Key::Character(key_text)
				if key_text.as_str().eq_ignore_ascii_case("s")
					&& self.plain_character_shortcut_available() =>
			{
				self.handle_frozen_scroll_capture_hotkey()
			},
			Key::Named(NamedKey::Space) => self.handle_frozen_copy_hotkey(),
			_ => OverlayControl::Continue,
		}
	}

	fn toggle_toolbar_visibility_hotkey(&mut self) -> OverlayControl {
		self.toolbar_state.visible = !self.toolbar_state.visible;

		#[cfg(target_os = "macos")]
		if self.toolbar_state.visible {
			self.request_aux_window_creation_if_needed();
		}

		self.request_redraw_all();

		OverlayControl::Continue
	}

	fn handle_frozen_auto_center_hotkey(&mut self) -> OverlayControl {
		self.auto_center_frozen_capture_rect();

		OverlayControl::Continue
	}

	fn handle_frozen_save_hotkey(&mut self) -> OverlayControl {
		self.begin_png_action(PngAction::Save);

		OverlayControl::Continue
	}

	fn handle_frozen_scroll_capture_hotkey(&mut self) -> OverlayControl {
		let available = self.scroll_capture_is_available();
		let selection_ready = self.scroll_capture_selection_is_ready();

		tracing::info!(
			op = "scroll_capture.frozen_s_pressed",
			available,
			scroll_capture_active = self.scroll_capture.active,
			selection_ready,
			frozen_capture_source = ?self.frozen_capture_source,
			state_mode = ?self.state.mode,
			"Received `s` while frozen."
		);

		if selection_ready {
			return self.start_scroll_capture();
		}

		OverlayControl::Continue
	}

	fn handle_frozen_copy_hotkey(&mut self) -> OverlayControl {
		self.begin_png_action(PngAction::Copy);

		OverlayControl::Continue
	}

	pub(super) fn is_save_shortcut_pressed(&self) -> bool {
		#[cfg(target_os = "macos")]
		{
			self.keyboard_modifiers.super_key()
		}
		#[cfg(not(target_os = "macos"))]
		{
			self.keyboard_modifiers.control_key()
		}
	}

	pub(super) fn loupe_activation_shortcut_available(&self) -> bool {
		matches!(self.state.mode, OverlayMode::Live)
			&& !self.keyboard_modifiers.shift_key()
			&& !self.keyboard_modifiers.alt_key()
			&& !self.keyboard_modifiers.control_key()
			&& !self.keyboard_modifiers.super_key()
	}

	pub(super) fn plain_character_shortcut_available(&self) -> bool {
		!self.loupe_activation_key_down
			&& !self.keyboard_modifiers.alt_key()
			&& !self.keyboard_modifiers.control_key()
			&& !self.keyboard_modifiers.super_key()
	}
}
