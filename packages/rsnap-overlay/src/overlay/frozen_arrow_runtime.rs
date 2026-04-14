use crate::overlay::{
	FrozenArrowAnnotation, FrozenArrowDragState, FrozenArrowGeometry, FrozenEditKind,
	FrozenToolbarTool, GlobalPoint, OverlaySession, Pos2, Vec2,
};

const FROZEN_ARROW_MIN_LENGTH_POINTS: f32 = 6.0;
const FROZEN_ARROW_STROKE_WIDTH_MULTIPLIER: f32 = 1.4;
const FROZEN_ARROW_STROKE_WIDTH_MIN_POINTS: f32 = 4.5;
const FROZEN_ARROW_OUTLINE_WIDTH_MULTIPLIER: f32 = 0.4;
const FROZEN_ARROW_OUTLINE_WIDTH_MIN_POINTS: f32 = 1.5;
const FROZEN_ARROW_HEAD_LENGTH_MULTIPLIER: f32 = 4.2;
const FROZEN_ARROW_HEAD_WIDTH_MULTIPLIER: f32 = 3.2;
const FROZEN_ARROW_HEAD_LENGTH_MIN_POINTS: f32 = 16.0;
const FROZEN_ARROW_HEAD_WIDTH_MIN_POINTS: f32 = 14.0;

impl OverlaySession {
	pub(super) fn frozen_arrow_stroke_width_points(stroke_width_points: f32) -> f32 {
		(stroke_width_points * FROZEN_ARROW_STROKE_WIDTH_MULTIPLIER)
			.max(FROZEN_ARROW_STROKE_WIDTH_MIN_POINTS)
	}

	pub(super) fn frozen_arrow_outline_width_points(stroke_width_points: f32) -> f32 {
		(stroke_width_points * FROZEN_ARROW_OUTLINE_WIDTH_MULTIPLIER)
			.max(FROZEN_ARROW_OUTLINE_WIDTH_MIN_POINTS)
	}

	pub(super) fn frozen_arrow_outline_stroke_width_points(stroke_width_points: f32) -> f32 {
		Self::frozen_arrow_stroke_width_points(stroke_width_points)
			+ Self::frozen_arrow_outline_width_points(stroke_width_points) * 2.0
	}

	pub(super) fn frozen_arrow_expanded_triangle(
		a: Pos2,
		b: Pos2,
		c: Pos2,
		amount: f32,
	) -> (Pos2, Pos2, Pos2) {
		let centroid = Pos2::new((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0);
		let expand_vertex = |vertex: Pos2| {
			let delta = vertex - centroid;
			let distance = delta.length();

			if distance <= f32::EPSILON { vertex } else { vertex + delta / distance * amount }
		};

		(expand_vertex(a), expand_vertex(b), expand_vertex(c))
	}

	pub(super) fn frozen_arrow_geometry(
		annotation: &FrozenArrowAnnotation,
	) -> Option<FrozenArrowGeometry> {
		let delta = annotation.end - annotation.start;
		let length = delta.length();

		if length <= f32::EPSILON {
			return None;
		}

		let direction = delta / length;
		let stroke_width =
			Self::frozen_arrow_stroke_width_points(annotation.style.stroke_width_points);
		let head_length = (stroke_width * FROZEN_ARROW_HEAD_LENGTH_MULTIPLIER)
			.max(FROZEN_ARROW_HEAD_LENGTH_MIN_POINTS)
			.min(length * 0.75);
		let head_width = (stroke_width * FROZEN_ARROW_HEAD_WIDTH_MULTIPLIER)
			.max(FROZEN_ARROW_HEAD_WIDTH_MIN_POINTS)
			.min(head_length * 0.9);
		let shaft_end = annotation.end - direction * (head_length * 0.72);
		let head_base = annotation.end - direction * head_length;
		let normal = Vec2::new(-direction.y, direction.x) * (head_width * 0.5);

		Some(FrozenArrowGeometry {
			shaft_end,
			tip: annotation.end,
			head_left: head_base + normal,
			head_right: head_base - normal,
		})
	}

	pub(super) fn begin_frozen_arrow_drag(&mut self, global: GlobalPoint) -> bool {
		if self.toolbar_state.selected_tool != FrozenToolbarTool::Arrow
			|| self.frozen_arrow_drag.active
		{
			return false;
		}

		let Some((monitor, capture_rect)) = self.frozen_brush_capture_target() else {
			return false;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(global) else {
			return false;
		};

		if !capture_rect.contains((cursor_x, cursor_y)) {
			return false;
		}

		self.frozen_arrow_drag = FrozenArrowDragState {
			active: true,
			anchor_x: cursor_x,
			anchor_y: cursor_y,
			current_x: cursor_x,
			current_y: cursor_y,
		};

		self.request_redraw_for_monitor(monitor);

		true
	}

	pub(super) fn update_frozen_arrow_drag(&mut self, global: GlobalPoint) -> bool {
		if !self.frozen_arrow_drag.active {
			return false;
		}

		let Some((monitor, capture_rect)) = self.frozen_brush_capture_target() else {
			self.stop_frozen_arrow_drag();

			return false;
		};
		let (cursor_x, cursor_y) = Self::clamped_local_point_in_rect(monitor, capture_rect, global);

		if self.frozen_arrow_drag.current_x == cursor_x
			&& self.frozen_arrow_drag.current_y == cursor_y
		{
			return false;
		}

		self.frozen_arrow_drag.current_x = cursor_x;
		self.frozen_arrow_drag.current_y = cursor_y;

		self.request_redraw_for_monitor(monitor);

		true
	}

	pub(super) fn active_frozen_arrow_preview(&self) -> Option<FrozenArrowAnnotation> {
		self.frozen_arrow_drag.active.then(|| FrozenArrowAnnotation {
			start: Pos2::new(
				self.frozen_arrow_drag.anchor_x as f32,
				self.frozen_arrow_drag.anchor_y as f32,
			),
			end: Pos2::new(
				self.frozen_arrow_drag.current_x as f32,
				self.frozen_arrow_drag.current_y as f32,
			),
			style: self.toolbar_state.brush_style,
		})
	}

	fn frozen_arrow_meets_commit_threshold(annotation: &FrozenArrowAnnotation) -> bool {
		annotation.start.distance(annotation.end) >= FROZEN_ARROW_MIN_LENGTH_POINTS
			&& Self::frozen_arrow_geometry(annotation).is_some()
	}

	pub(super) fn stop_frozen_arrow_drag(&mut self) {
		let was_active = self.frozen_arrow_drag.active;

		self.frozen_arrow_drag = FrozenArrowDragState::default();

		if was_active && let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}
	}

	pub(super) fn commit_frozen_arrow_drag(&mut self) -> bool {
		let Some(annotation) = self.active_frozen_arrow_preview() else {
			return false;
		};

		self.stop_frozen_arrow_drag();

		if !Self::frozen_arrow_meets_commit_threshold(&annotation) {
			return false;
		}

		self.frozen_arrow_annotations.push(annotation);
		self.push_frozen_edit_to_undo_history(FrozenEditKind::ArrowAnnotation);
		self.sync_frozen_toolbar_state();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	pub(super) fn undo_frozen_arrow_annotation(&mut self) -> bool {
		let Some(annotation) = self.frozen_arrow_annotations.pop() else {
			return false;
		};

		self.frozen_arrow_redo_annotations.push(annotation);
		self.sync_frozen_toolbar_state();

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	pub(super) fn redo_frozen_arrow_annotation(&mut self) -> bool {
		let Some(annotation) = self.frozen_arrow_redo_annotations.pop() else {
			return false;
		};

		self.frozen_arrow_annotations.push(annotation);
		self.sync_frozen_toolbar_state();

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}
}
