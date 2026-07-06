use std::time::Instant;

use crate::overlay::frozen_brush_model;
use crate::overlay::{
	FrozenBrushStroke, FrozenEditKind, FrozenToolbarTool, GlobalPoint, MonitorRect, OverlayMode,
	OverlaySession, Pos2, RectPoints,
};

impl OverlaySession {
	fn frozen_capture_rect_for_monitor(&self, monitor: MonitorRect) -> Option<RectPoints> {
		self.state
			.frozen_capture_rect
			.or_else(|| Some(RectPoints::new(0, 0, monitor.width, monitor.height)))
			.filter(|capture_rect| !capture_rect.is_empty())
	}

	pub(super) fn frozen_brush_capture_target(&self) -> Option<(MonitorRect, RectPoints)> {
		if !matches!(self.state.mode, OverlayMode::Frozen)
			|| self.scroll_capture.active
			|| !self.frozen_display_ready()
		{
			return None;
		}

		let monitor = self.state.monitor?;
		let capture_rect = self.frozen_capture_rect_for_monitor(monitor)?;

		Some((monitor, capture_rect))
	}

	pub(super) fn begin_frozen_brush_stroke(&mut self, global: GlobalPoint) -> bool {
		if self.toolbar_state.selected_tool != FrozenToolbarTool::Pen {
			return false;
		}
		if self.frozen_brush.active_stroke.is_some() {
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

		let point = Pos2::new(cursor_x as f32, cursor_y as f32);
		let sampled_at = Instant::now();

		self.frozen_brush.active_stroke = Some(frozen_brush_model::new_active_stroke(
			point,
			sampled_at,
			self.toolbar_state.brush_style,
		));

		self.request_redraw_for_monitor(monitor);

		true
	}

	pub(super) fn update_frozen_brush_stroke(&mut self, global: GlobalPoint) -> bool {
		let Some((monitor, capture_rect)) = self.frozen_brush_capture_target() else {
			return false;
		};
		let Some((cursor_x, cursor_y)) = monitor.local_u32(global) else {
			return false;
		};
		let Some(active_stroke) = self.frozen_brush.active_stroke.as_mut() else {
			return false;
		};
		let point = Self::clamped_point_in_capture_rect(capture_rect, cursor_x, cursor_y);
		let sampled_at = Instant::now();

		if !frozen_brush_model::append_raw_sample(active_stroke, point, sampled_at) {
			return false;
		}

		true
	}

	pub(super) fn finish_frozen_brush_stroke(&mut self) -> bool {
		let Some(stroke) = self.frozen_brush.active_stroke.take() else {
			return false;
		};

		if stroke.points.is_empty() {
			return false;
		}

		self.frozen_brush.committed_strokes.push(FrozenBrushStroke {
			points: frozen_brush_model::finished_points(&stroke),
			style: stroke.style,
		});
		self.push_frozen_edit_to_undo_history(FrozenEditKind::BrushStroke);
		self.sync_frozen_toolbar_state();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	pub(super) fn undo_frozen_brush_stroke(&mut self) -> bool {
		let Some(stroke) = self.frozen_brush.committed_strokes.pop() else {
			return false;
		};

		self.frozen_brush.redo_strokes.push(stroke);
		self.sync_frozen_toolbar_state();

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	pub(super) fn redo_frozen_brush_stroke(&mut self) -> bool {
		let Some(stroke) = self.frozen_brush.redo_strokes.pop() else {
			return false;
		};

		self.frozen_brush.committed_strokes.push(stroke);
		self.sync_frozen_toolbar_state();

		self.toolbar_state.needs_redraw = true;

		self.request_redraw_toolbar_window();

		if let Some(monitor) = self.state.monitor {
			self.request_redraw_for_monitor(monitor);
		}

		true
	}

	fn clamped_point_in_capture_rect(
		capture_rect: RectPoints,
		cursor_x: u32,
		cursor_y: u32,
	) -> Pos2 {
		let max_x = capture_rect.x.saturating_add(capture_rect.width.saturating_sub(1));
		let max_y = capture_rect.y.saturating_add(capture_rect.height.saturating_sub(1));

		Pos2::new(
			cursor_x.clamp(capture_rect.x, max_x) as f32,
			cursor_y.clamp(capture_rect.y, max_y) as f32,
		)
	}
}
