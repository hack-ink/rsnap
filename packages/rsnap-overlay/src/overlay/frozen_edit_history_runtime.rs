use crate::overlay::{FROZEN_EDIT_HISTORY_LIMIT, FrozenEditKind, OverlaySession};

impl OverlaySession {
	pub(super) fn clear_frozen_redo_history(&mut self) {
		self.frozen_edit_redo_stack.clear();
		self.frozen_brush.redo_strokes.clear();
		self.frozen_mosaic_redo_stack.clear();
		self.frozen_text_redo_annotations.clear();
		self.frozen_arrow_redo_annotations.clear();
		self.frozen_spotlight_redo_annotations.clear();
	}

	fn discard_evicted_frozen_edit_payload(&mut self, edit_kind: FrozenEditKind) {
		match edit_kind {
			FrozenEditKind::BrushStroke => {
				if !self.frozen_brush.committed_strokes.is_empty() {
					self.frozen_brush.committed_strokes.remove(0);
				}
			},
			FrozenEditKind::MosaicEdit => {
				if !self.frozen_mosaic_undo_stack.is_empty() {
					self.frozen_mosaic_undo_stack.remove(0);
				}
			},
			FrozenEditKind::TextAnnotation => {
				if !self.frozen_text_annotations.is_empty() {
					self.frozen_text_annotations.remove(0);
				}
			},
			FrozenEditKind::ArrowAnnotation => {
				if !self.frozen_arrow_annotations.is_empty() {
					self.frozen_arrow_annotations.remove(0);
				}
			},
			FrozenEditKind::SpotlightAnnotation => {
				if !self.frozen_spotlight_annotations.is_empty() {
					self.frozen_spotlight_annotations.remove(0);
				}
			},
		}
	}

	pub(super) fn push_frozen_edit_to_undo_history(&mut self, edit_kind: FrozenEditKind) {
		self.frozen_edit_undo_stack.push(edit_kind);

		if self.frozen_edit_undo_stack.len() > FROZEN_EDIT_HISTORY_LIMIT {
			let evicted = self.frozen_edit_undo_stack.remove(0);

			self.discard_evicted_frozen_edit_payload(evicted);
		}

		self.clear_frozen_redo_history();
	}

	pub(super) fn frozen_undo_available(&self) -> bool {
		!self.frozen_edit_undo_stack.is_empty()
	}

	pub(super) fn frozen_redo_available(&self) -> bool {
		!self.frozen_edit_redo_stack.is_empty()
	}

	pub(super) fn perform_frozen_undo(&mut self) -> bool {
		let Some(edit_kind) = self.frozen_edit_undo_stack.pop() else {
			return false;
		};
		let undone = match edit_kind {
			FrozenEditKind::BrushStroke => self.undo_frozen_brush_stroke(),
			FrozenEditKind::MosaicEdit => self.undo_frozen_mosaic_edit(),
			FrozenEditKind::TextAnnotation => self.undo_frozen_text_annotation(),
			FrozenEditKind::ArrowAnnotation => self.undo_frozen_arrow_annotation(),
			FrozenEditKind::SpotlightAnnotation => self.undo_frozen_spotlight_annotation(),
		};

		if undone {
			self.frozen_edit_redo_stack.push(edit_kind);
		} else {
			self.frozen_edit_undo_stack.push(edit_kind);
		}

		self.sync_frozen_toolbar_state();

		undone
	}

	pub(super) fn perform_frozen_redo(&mut self) -> bool {
		let Some(edit_kind) = self.frozen_edit_redo_stack.pop() else {
			return false;
		};
		let redone = match edit_kind {
			FrozenEditKind::BrushStroke => self.redo_frozen_brush_stroke(),
			FrozenEditKind::MosaicEdit => self.redo_frozen_mosaic_edit(),
			FrozenEditKind::TextAnnotation => self.redo_frozen_text_annotation(),
			FrozenEditKind::ArrowAnnotation => self.redo_frozen_arrow_annotation(),
			FrozenEditKind::SpotlightAnnotation => self.redo_frozen_spotlight_annotation(),
		};

		if redone {
			self.frozen_edit_undo_stack.push(edit_kind);
		} else {
			self.frozen_edit_redo_stack.push(edit_kind);
		}

		self.sync_frozen_toolbar_state();

		redone
	}
}
