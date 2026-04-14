use crate::overlay::{
	GlobalPoint, HUD_LOUPE_STRIP_GAP_POINTS, MonitorRect, OverlayMode, OverlaySession, Pos2, Rect,
	TOOLBAR_SCREEN_MARGIN_PX, Vec2, WindowRenderer,
};

impl OverlaySession {
	pub(super) fn toolbar_positioning_size(&self) -> Vec2 {
		WindowRenderer::frozen_toolbar_positioning_size(&self.toolbar_state)
	}

	#[cfg(target_os = "macos")]
	pub(super) fn toolbar_outer_position_from_primary_anchor(
		&self,
		monitor: MonitorRect,
		primary_anchor: Pos2,
	) -> GlobalPoint {
		let primary_origin = super::frozen_toolbar_window_primary_origin();

		GlobalPoint::new(
			monitor.origin.x.saturating_add((primary_anchor.x - primary_origin.x).round() as i32),
			monitor.origin.y.saturating_add((primary_anchor.y - primary_origin.y).round() as i32),
		)
	}

	#[cfg(target_os = "macos")]
	pub(super) fn toolbar_primary_anchor_from_outer_position(
		&self,
		monitor: MonitorRect,
		outer_position: GlobalPoint,
	) -> Pos2 {
		let primary_origin = super::frozen_toolbar_window_primary_origin();

		Pos2::new(
			outer_position.x as f32 - monitor.origin.x as f32 + primary_origin.x,
			outer_position.y as f32 - monitor.origin.y as f32 + primary_origin.y,
		)
	}

	pub(super) fn update_hud_window_position(&mut self, monitor: MonitorRect, cursor: GlobalPoint) {
		if self.live_loupe_uses_hud_window()
			&& matches!(self.state.mode, OverlayMode::Live)
			&& self.state.alt_held
		{
			let _ = self.update_loupe_window_position(monitor);

			return;
		}

		let Some(hud_window) = self.hud_window.as_ref() else {
			return;
		};
		let scale = hud_window.window.scale_factor().max(1.0);
		let size = hud_window.window.inner_size();
		let hud_w_points = ((size.width as f64) / scale).ceil().max(1.0) as i32;
		let hud_h_points = ((size.height as f64) / scale).ceil().max(1.0) as i32;
		let monitor_right = monitor.origin.x.saturating_add_unsigned(monitor.width);
		let monitor_bottom = monitor.origin.y.saturating_add_unsigned(monitor.height);
		let offset_x = 48;
		let offset_y = 24;
		let mut x = cursor.x.saturating_add(offset_x);
		let mut y = cursor.y.saturating_add(offset_y);

		if x.saturating_add(hud_w_points) > monitor_right {
			x = cursor.x.saturating_sub(offset_x.saturating_add(hud_w_points));
		}
		if y.saturating_add(hud_h_points) > monitor_bottom {
			y = cursor.y.saturating_sub(offset_y.saturating_add(hud_h_points));
		}

		x = x.clamp(
			monitor.origin.x,
			monitor_right.saturating_sub(hud_w_points).max(monitor.origin.x),
		);
		y = y.clamp(
			monitor.origin.y,
			monitor_bottom.saturating_sub(hud_h_points).max(monitor.origin.y),
		);

		let desired = GlobalPoint::new(x, y);

		if self.hud_outer_pos == Some(desired) {
			if self.state.alt_held {
				let _ = self.update_loupe_window_position(monitor);
			}

			return;
		}

		self.hud_outer_pos = Some(desired);
		self.pending_hud_outer_pos = Some(desired);

		if self.state.alt_held {
			let _ = self.update_loupe_window_position(monitor);
		}
	}

	pub(super) fn update_loupe_window_position(&mut self, monitor: MonitorRect) -> bool {
		if !self.state.alt_held {
			self.pending_loupe_outer_pos = None;

			return false;
		}

		let Some(loupe_window) = self.loupe_window.as_ref() else {
			return false;
		};
		let loupe_scale = loupe_window.window.scale_factor().max(1.0);
		let loupe_size = loupe_window.window.inner_size();
		let loupe_w_points = ((loupe_size.width as f64) / loupe_scale).ceil().max(1.0) as i32;
		let loupe_h_points = ((loupe_size.height as f64) / loupe_scale).ceil().max(1.0) as i32;
		let monitor_right = monitor.origin.x.saturating_add_unsigned(monitor.width);
		let monitor_bottom = monitor.origin.y.saturating_add_unsigned(monitor.height);
		let max_x = monitor_right.saturating_sub(loupe_w_points).max(monitor.origin.x);
		let max_y = monitor_bottom.saturating_sub(loupe_h_points).max(monitor.origin.y);
		let gap = HUD_LOUPE_STRIP_GAP_POINTS;
		let (mut x, mut y) = if matches!(self.state.mode, OverlayMode::Live) {
			let hud_height_points = self.hud_window.as_ref().map(|hud_window| {
				let hud_scale = hud_window.window.scale_factor().max(1.0);
				let hud_size = hud_window.window.inner_size();

				((hud_size.height as f64) / hud_scale).ceil().max(1.0) as i32
			});
			let Some(desired) = Self::live_loupe_default_position(
				monitor,
				self.state.cursor,
				self.hud_outer_pos,
				hud_height_points,
				loupe_w_points,
				loupe_h_points,
			) else {
				return false;
			};

			(desired.x, desired.y)
		} else {
			let Some(hud_window) = self.hud_window.as_ref() else {
				return false;
			};
			let Some(hud_outer) = self.hud_outer_pos else {
				return false;
			};
			let hud_scale = hud_window.window.scale_factor().max(1.0);
			let hud_size = hud_window.window.inner_size();
			let hud_h_points = ((hud_size.height as f64) / hud_scale).ceil().max(1.0) as i32;
			let below_y = hud_outer.y.saturating_add(hud_h_points + gap);
			let above_y = hud_outer.y.saturating_sub(gap.saturating_add(loupe_h_points));

			(
				hud_outer.x,
				if below_y.saturating_add(loupe_h_points) <= monitor_bottom {
					below_y
				} else {
					above_y
				},
			)
		};

		x = x.clamp(monitor.origin.x, max_x);
		y = y.clamp(monitor.origin.y, max_y);

		let desired = GlobalPoint::new(x, y);

		if self.loupe_outer_pos == Some(desired) {
			self.pending_loupe_outer_pos = Some(desired);

			return true;
		}

		self.loupe_outer_pos = Some(desired);
		self.pending_loupe_outer_pos = Some(desired);

		true
	}

	pub(super) fn live_loupe_default_position(
		monitor: MonitorRect,
		cursor: Option<GlobalPoint>,
		hud_outer: Option<GlobalPoint>,
		hud_height_points: Option<i32>,
		loupe_w_points: i32,
		loupe_h_points: i32,
	) -> Option<GlobalPoint> {
		let monitor_right = monitor.origin.x.saturating_add_unsigned(monitor.width);
		let monitor_bottom = monitor.origin.y.saturating_add_unsigned(monitor.height);
		let max_x = monitor_right.saturating_sub(loupe_w_points).max(monitor.origin.x);
		let max_y = monitor_bottom.saturating_sub(loupe_h_points).max(monitor.origin.y);
		let gap = HUD_LOUPE_STRIP_GAP_POINTS;
		let (mut x, mut y) =
			if let (Some(hud_outer), Some(hud_height_points)) = (hud_outer, hud_height_points) {
				let below_y = hud_outer.y.saturating_add(hud_height_points + gap);
				let above_y = hud_outer.y.saturating_sub(gap.saturating_add(loupe_h_points));

				(
					hud_outer.x,
					if below_y.saturating_add(loupe_h_points) <= monitor_bottom {
						below_y
					} else {
						above_y
					},
				)
			} else {
				let cursor = cursor?;
				let offset_x = 48;
				let offset_y = 32;
				let mut x = cursor.x.saturating_add(offset_x);
				let mut y = cursor.y.saturating_add(offset_y);

				if x.saturating_add(loupe_w_points) > monitor_right {
					x = cursor.x.saturating_sub(offset_x.saturating_add(loupe_w_points));
				}
				if y.saturating_add(loupe_h_points) > monitor_bottom {
					y = cursor.y.saturating_sub(offset_y.saturating_add(loupe_h_points));
				}

				(x, y)
			};

		x = x.clamp(monitor.origin.x, max_x);
		y = y.clamp(monitor.origin.y, max_y);

		Some(GlobalPoint::new(x, y))
	}

	pub(super) fn update_toolbar_outer_position(
		&mut self,
		monitor: MonitorRect,
		local_pos: Pos2,
	) -> bool {
		let toolbar_size = self.toolbar_positioning_size();
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		let clamped_local_pos = WindowRenderer::clamp_toolbar_position(
			screen_rect,
			toolbar_size,
			local_pos,
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		);
		#[cfg(target_os = "macos")]
		let desired = self.toolbar_outer_position_from_primary_anchor(monitor, clamped_local_pos);
		#[cfg(not(target_os = "macos"))]
		let desired = GlobalPoint::new(
			monitor.origin.x.saturating_add(clamped_local_pos.x.round() as i32),
			monitor.origin.y.saturating_add(clamped_local_pos.y.round() as i32),
		);

		if self.toolbar_outer_pos == Some(desired) {
			return false;
		}

		self.toolbar_outer_pos = Some(desired);
		self.pending_toolbar_outer_pos = Some(desired);
		self.toolbar_state.floating_position = Some(clamped_local_pos);

		self.sync_frozen_annotation_style_capsule_placement(monitor);

		true
	}

	pub(super) fn sync_toolbar_outer_position_from_window(
		&mut self,
		monitor: MonitorRect,
		outer_position: GlobalPoint,
	) -> bool {
		let toolbar_size = self.toolbar_positioning_size();
		let screen_rect =
			Rect::from_min_size(Pos2::ZERO, Vec2::new(monitor.width as f32, monitor.height as f32));
		#[cfg(target_os = "macos")]
		let local_pos = self.toolbar_primary_anchor_from_outer_position(monitor, outer_position);
		#[cfg(not(target_os = "macos"))]
		let local_pos = Pos2::new(
			outer_position.x as f32 - monitor.origin.x as f32,
			outer_position.y as f32 - monitor.origin.y as f32,
		);
		let clamped_local_pos = WindowRenderer::clamp_toolbar_position(
			screen_rect,
			toolbar_size,
			local_pos,
			TOOLBAR_SCREEN_MARGIN_PX,
			TOOLBAR_SCREEN_MARGIN_PX,
		);
		#[cfg(target_os = "macos")]
		let desired = self.toolbar_outer_position_from_primary_anchor(monitor, clamped_local_pos);
		#[cfg(not(target_os = "macos"))]
		let desired = GlobalPoint::new(
			monitor.origin.x.saturating_add(clamped_local_pos.x.round() as i32),
			monitor.origin.y.saturating_add(clamped_local_pos.y.round() as i32),
		);
		let changed = self.toolbar_outer_pos != Some(desired)
			|| self.toolbar_state.floating_position != Some(clamped_local_pos);

		self.toolbar_outer_pos = Some(desired);
		self.toolbar_state.floating_position = Some(clamped_local_pos);

		self.sync_frozen_annotation_style_capsule_placement(monitor);

		self.pending_toolbar_outer_pos = (desired != outer_position).then_some(desired);

		changed
	}
}
