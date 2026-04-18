use egui::{FontId, Pos2, Rect, Vec2};
use image::RgbaImage;
use winit::dpi::{LogicalPosition, LogicalSize};

use crate::overlay::{
	FrozenEditKind, FrozenExportTransform, FrozenTextAnnotation, FrozenTextEditState,
	FrozenToolbarTool, GlobalPoint, MonitorRect, OverlaySession, WindowRenderer,
};
use crate::text_rendering::{self, RasterTextAnnotation};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FrozenTextRecentInput {
	pub(super) source: FrozenTextInputSource,
	pub(super) text: String,
	pub(super) generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrozenTextInputSource {
	Key,
	Ime,
}

impl OverlaySession {
	pub(super) fn reset_frozen_text_state(&mut self) {
		self.frozen_text_annotations.clear();
		self.frozen_text_redo_annotations.clear();

		self.frozen_text_edit = None;

		self.sync_text_input_ime_state();
	}

	pub(super) fn frozen_text_tool_active(&self) -> bool {
		!self.scroll_capture.active && self.toolbar_state.selected_tool == FrozenToolbarTool::Text
	}

	pub(super) fn sync_text_input_ime_state(&mut self) {
		#[cfg(target_os = "macos")]
		{
			let ime_allowed = self.frozen_text_tool_active() && self.frozen_text_edit.is_some();

			for overlay_window in self.windows.values() {
				overlay_window.window.set_ime_allowed(ime_allowed);
			}

			if let Some(toolbar_window) = self.toolbar_window.as_ref() {
				toolbar_window.window.set_ime_allowed(false);
			}
		}

		#[cfg(not(target_os = "macos"))]
		let ime_allowed = self.frozen_text_tool_active() && self.frozen_text_edit.is_some();

		#[cfg(not(target_os = "macos"))]
		for overlay_window in self.windows.values() {
			overlay_window.window.set_ime_allowed(ime_allowed);
		}

		#[cfg(not(target_os = "macos"))]
		if let Some(toolbar_window) = self.toolbar_window.as_ref() {
			toolbar_window.window.set_ime_allowed(ime_allowed);
		}
	}

	pub(super) fn sync_frozen_text_ime_cursor_area(&mut self, monitor: MonitorRect) {
		#[cfg(target_os = "macos")]
		{
			let Some(edit_state) = self.frozen_text_edit.as_ref() else {
				return;
			};
			let Some(overlay_window) =
				self.windows.values().find(|window| window.monitor == monitor)
			else {
				return;
			};
			let (visible_text, caret_char_index) = edit_state.visible_text_and_caret_char_index();
			let caret_rect = overlay_window.renderer.frozen_text_edit_caret_rect_for_window(
				edit_state.anchor,
				visible_text.as_str(),
				&FontId::proportional(self.toolbar_state.text_style.font_size_points),
				caret_char_index.unwrap_or_else(|| visible_text.chars().count()),
			);

			overlay_window.window.set_ime_cursor_area(
				LogicalPosition::new(
					f64::from(caret_rect.min.x.max(0.0)),
					f64::from(caret_rect.min.y.max(0.0)),
				),
				LogicalSize::new(
					f64::from(caret_rect.width().max(1.0)),
					f64::from(
						caret_rect.height().max(self.toolbar_state.text_style.font_size_points),
					),
				),
			);
		}

		#[cfg(not(target_os = "macos"))]
		let Some(edit_state) = self.frozen_text_edit.as_ref() else {
			return;
		};
		#[cfg(not(target_os = "macos"))]
		let Some(overlay_window) = self.windows.values().find(|window| window.monitor == monitor)
		else {
			return;
		};
		#[cfg(not(target_os = "macos"))]
		let (visible_text, caret_char_index) = edit_state.visible_text_and_caret_char_index();
		#[cfg(not(target_os = "macos"))]
		let caret_rect = overlay_window.renderer.frozen_text_edit_caret_rect_for_window(
			edit_state.anchor,
			visible_text.as_str(),
			&FontId::proportional(self.toolbar_state.text_style.font_size_points),
			caret_char_index.unwrap_or_else(|| visible_text.chars().count()),
		);

		#[cfg(not(target_os = "macos"))]
		overlay_window.window.set_ime_cursor_area(
			LogicalPosition::new(
				f64::from(caret_rect.min.x.max(0.0)),
				f64::from(caret_rect.min.y.max(0.0)),
			),
			LogicalSize::new(
				f64::from(caret_rect.width().max(1.0)),
				f64::from(caret_rect.height().max(self.toolbar_state.text_style.font_size_points)),
			),
		);
	}

	pub(super) fn should_refresh_frozen_text_ime_cursor_area_for_text_style_change(
		&self,
		monitor: MonitorRect,
	) -> bool {
		self.state.monitor == Some(monitor)
			&& self.frozen_text_tool_active()
			&& self.frozen_text_edit.as_ref().is_some_and(FrozenTextEditState::has_ime_preedit)
	}

	pub(super) fn refresh_frozen_text_ime_cursor_area_for_text_style_change(
		&mut self,
		monitor: MonitorRect,
	) {
		if self.should_refresh_frozen_text_ime_cursor_area_for_text_style_change(monitor) {
			self.sync_frozen_text_ime_cursor_area(monitor);
		}
	}

	pub(super) fn finish_frozen_text_editing(&mut self, commit: bool) -> bool {
		let Some(edit_state) = self.frozen_text_edit.take() else {
			self.sync_text_input_ime_state();

			return false;
		};
		let committed_text = edit_state.visible_text();
		let had_visible_text = !committed_text.trim().is_empty();

		if commit && had_visible_text {
			self.frozen_text_annotations.push(FrozenTextAnnotation {
				anchor: edit_state.anchor,
				text: committed_text,
				style: self.toolbar_state.text_style,
			});
			self.push_frozen_edit_to_undo_history(FrozenEditKind::TextAnnotation);
			self.sync_frozen_toolbar_state();
		}

		self.frozen_text_recent_input = None;

		self.sync_text_input_ime_state();

		had_visible_text
	}

	pub(super) fn note_frozen_text_input_event(&mut self) -> u64 {
		self.frozen_text_input_generation = self.frozen_text_input_generation.wrapping_add(1);

		self.frozen_text_input_generation
	}

	pub(super) fn append_text_to_frozen_edit_for_input_event(
		&mut self,
		source: FrozenTextInputSource,
		generation: u64,
		text: &str,
	) -> bool {
		let text = text.replace('\r', "");

		if text.is_empty() {
			return false;
		}
		if self.frozen_text_recent_input.as_ref().is_some_and(|recent| {
			recent.source != source
				&& recent.text == text
				&& generation == recent.generation.saturating_add(1)
		}) {
			self.frozen_text_recent_input = None;

			return false;
		}

		let changed = self.append_text_to_frozen_edit(text.as_str());

		if changed {
			self.frozen_text_recent_input =
				Some(FrozenTextRecentInput { source, text, generation });
		}

		changed
	}

	pub(super) fn append_text_to_frozen_edit(&mut self, text: &str) -> bool {
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};
		let text = text.replace('\r', "");

		if text.is_empty() {
			return false;
		}

		edit_state.text.push_str(&text);

		edit_state.ime_preedit = None;
		edit_state.ime_preedit_cursor_char_range = None;

		edit_state.reset_caret_blink();

		self.frozen_text_recent_input = None;

		true
	}

	pub(super) fn backspace_frozen_text_edit(&mut self) -> bool {
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};
		let had_preedit = edit_state.has_ime_preedit();

		edit_state.ime_preedit = None;
		edit_state.ime_preedit_cursor_char_range = None;

		let changed = had_preedit || edit_state.text.pop().is_some();

		if changed {
			edit_state.reset_caret_blink();

			self.frozen_text_recent_input = None;
		}

		changed
	}

	pub(super) fn undo_frozen_text_annotation(&mut self) -> bool {
		let Some(annotation) = self.frozen_text_annotations.pop() else {
			return false;
		};

		self.frozen_text_redo_annotations.push(annotation);

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	pub(super) fn redo_frozen_text_annotation(&mut self) -> bool {
		let Some(annotation) = self.frozen_text_redo_annotations.pop() else {
			return false;
		};

		self.frozen_text_annotations.push(annotation);

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	pub(super) fn set_frozen_text_ime_preedit(
		&mut self,
		preedit: Option<String>,
		cursor_range: Option<(usize, usize)>,
	) -> bool {
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};
		let normalized = preedit.filter(|text| !text.is_empty());
		let normalized_cursor_range = normalized.as_deref().and_then(|text| {
			FrozenTextEditState::normalize_ime_preedit_cursor_char_range(text, cursor_range)
		});

		if edit_state.ime_preedit == normalized
			&& edit_state.ime_preedit_cursor_char_range == normalized_cursor_range
		{
			return false;
		}

		edit_state.ime_preedit = normalized;
		edit_state.ime_preedit_cursor_char_range = normalized_cursor_range;

		edit_state.reset_caret_blink();

		true
	}

	pub(super) fn begin_frozen_text_edit_at(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) -> bool {
		if self.state.monitor != Some(monitor) {
			return false;
		}

		let Some((local_x, local_y)) = monitor.local_u32(cursor) else {
			return false;
		};
		let Some(capture_rect) = self.state.frozen_capture_rect else {
			return false;
		};

		if !capture_rect.contains((local_x, local_y)) {
			return false;
		}

		let _ = self.finish_frozen_text_editing(true);

		self.frozen_text_edit =
			Some(FrozenTextEditState::new(Pos2::new(local_x as f32, local_y as f32)));
		self.frozen_text_recent_input = None;

		self.sync_text_input_ime_state();
		self.sync_frozen_text_ime_cursor_area(monitor);
		#[cfg(target_os = "macos")]
		self.focus_frozen_text_input_window(Some(monitor));

		true
	}

	pub(super) fn frozen_text_edit_hit_rect_for_monitor(
		&self,
		monitor: MonitorRect,
	) -> Option<Rect> {
		if self.state.monitor != Some(monitor) {
			return None;
		}

		let edit_state = self.frozen_text_edit.as_ref()?;
		let visible_text = edit_state.visible_text();

		Some(WindowRenderer::frozen_text_edit_interaction_rect(
			edit_state.anchor,
			visible_text.as_str(),
			&FontId::proportional(self.toolbar_state.text_style.font_size_points),
		))
	}

	pub(super) fn begin_frozen_text_edit_drag_at(
		&mut self,
		monitor: MonitorRect,
		cursor: GlobalPoint,
	) -> bool {
		let Some((local_x, local_y)) = monitor.local_u32(cursor) else {
			return false;
		};
		let Some(hit_rect) = self.frozen_text_edit_hit_rect_for_monitor(monitor) else {
			return false;
		};
		let pointer = Pos2::new(local_x as f32, local_y as f32);

		if !hit_rect.contains(pointer) {
			return false;
		}

		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};

		edit_state.dragging = true;
		edit_state.drag_offset = pointer - edit_state.anchor;

		true
	}

	pub(super) fn stop_frozen_text_edit_drag(&mut self) -> bool {
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};
		let was_dragging = edit_state.dragging;

		edit_state.dragging = false;
		edit_state.drag_offset = Vec2::ZERO;

		was_dragging
	}

	pub(super) fn update_frozen_text_edit_drag_anchor(&mut self, global: GlobalPoint) -> bool {
		let Some(monitor) = self.state.monitor else {
			let _ = self.stop_frozen_text_edit_drag();

			return false;
		};
		let Some(capture_rect) = self.state.frozen_capture_rect else {
			let _ = self.stop_frozen_text_edit_drag();

			return false;
		};
		let Some(edit_state) = self.frozen_text_edit.as_mut() else {
			return false;
		};

		if !edit_state.dragging {
			return false;
		}

		let (cursor_x, cursor_y) = Self::clamped_local_point_in_monitor(monitor, global);
		let max_x = capture_rect.x.saturating_add(capture_rect.width.saturating_sub(1)) as f32;
		let max_y = capture_rect.y.saturating_add(capture_rect.height.saturating_sub(1)) as f32;
		let changed = {
			let next_anchor = Pos2::new(
				(cursor_x as f32 - edit_state.drag_offset.x).clamp(capture_rect.x as f32, max_x),
				(cursor_y as f32 - edit_state.drag_offset.y).clamp(capture_rect.y as f32, max_y),
			);

			if next_anchor == edit_state.anchor {
				false
			} else {
				edit_state.anchor = next_anchor;

				true
			}
		};

		if !changed {
			return false;
		}

		self.sync_frozen_text_ime_cursor_area(monitor);
		self.request_redraw_for_monitor(monitor);

		true
	}

	pub(super) fn sync_frozen_text_edit_for_selected_tool(&mut self) -> bool {
		if self.frozen_text_tool_active() {
			self.sync_text_input_ime_state();

			return false;
		}

		self.finish_frozen_text_editing(true)
	}

	#[cfg(test)]
	pub(super) fn visible_frozen_text_annotations(&self) -> &[FrozenTextAnnotation] {
		if self.scroll_capture.active { &[] } else { &self.frozen_text_annotations }
	}

	#[cfg(test)]
	pub(super) fn visible_frozen_text_edit(&self) -> Option<&FrozenTextEditState> {
		if self.scroll_capture.active { None } else { self.frozen_text_edit.as_ref() }
	}

	pub(super) fn render_frozen_text_annotation_into_image(
		image: &mut RgbaImage,
		export_transform: FrozenExportTransform,
		annotation: &FrozenTextAnnotation,
	) {
		let raster_annotation = RasterTextAnnotation {
			anchor_px: export_transform.point_to_pixels(annotation.anchor),
			font_size_px: annotation.style.font_size_points * export_transform.scalar_scale(),
			fill_rgba: annotation.style.color.export_rgba(),
			text: annotation.text.as_str(),
		};

		text_rendering::render_text_annotations(image, &[raster_annotation]);
	}

	#[cfg(target_os = "macos")]
	pub(super) fn focus_frozen_text_input_window(&mut self, monitor: Option<MonitorRect>) {
		tracing::info!(
			op = "overlay.frozen_text_focus_requested",
			target = "key_focus_shell",
			monitor_id = ?monitor.map(|target| target.id),
			"Requested frozen text input focus."
		);
	}
}
