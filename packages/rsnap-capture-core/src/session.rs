//! Minimal reference session used to drive the new host/core boundary.

use std::collections::VecDeque;

use crate::geometry::GlobalPoint;
use crate::protocol::{
	CaptureMode, CursorIntent, HostEffectKind, HostEvent, HostReport, HostRequest, PermissionKind,
	SceneModel, SessionConfig, ToolbarItemKind, ToolbarItemModel,
};

const RESIZE_HANDLE_RADIUS_POINTS: i32 = 12;
const RESIZE_EDGE_TOLERANCE_POINTS: i32 = 4;
const LIVE_SELECTION_DEFAULT_WIDTH: u32 = 320;
const LIVE_SELECTION_DEFAULT_HEIGHT: u32 = 200;
const LIVE_SELECTION_DRAG_THRESHOLD_POINTS: u32 = 1;

/// Reference capture-session core that owns semantic state and emits host requests.
#[derive(Debug)]
pub struct CaptureSessionCore {
	config: SessionConfig,
	scene: SceneModel,
	selected_toolbar_item: ToolbarItemKind,
	live_press_start: Option<GlobalPoint>,
	live_press_target: Option<crate::geometry::GlobalRect>,
	pending_requests: VecDeque<HostRequest>,
}
impl CaptureSessionCore {
	/// Creates a new session core with the provided host-owned configuration.
	#[must_use]
	pub fn with_config(config: SessionConfig) -> Self {
		Self {
			config,
			scene: SceneModel::hidden(),
			selected_toolbar_item: ToolbarItemKind::Pointer,
			live_press_start: None,
			live_press_target: None,
			pending_requests: VecDeque::new(),
		}
	}

	/// Returns the immutable session configuration.
	#[must_use]
	pub fn config(&self) -> &SessionConfig {
		&self.config
	}

	/// Returns the current semantic scene snapshot.
	#[must_use]
	pub fn scene_model(&self) -> &SceneModel {
		&self.scene
	}

	/// Enters live mode and requests native live capture.
	pub fn enter_live(&mut self) {
		self.scene.mode = CaptureMode::Live;
		self.scene.cursor_intent = CursorIntent::Default;
		self.scene.pointer = None;
		self.scene.active_monitor = None;
		self.scene.highlighted_window = None;
		self.scene.live_selection_preview = None;
		self.scene.frozen_selection = None;
		self.scene.status_message = None;
		self.selected_toolbar_item = ToolbarItemKind::Pointer;
		self.live_press_start = None;
		self.live_press_target = None;
		self.refresh_toolbar_actions();
		self.pending_requests.push_back(HostRequest::StartLiveCapture);
	}

	/// Handles one host-to-core event.
	pub fn handle_host_event(&mut self, event: HostEvent) {
		match event {
			HostEvent::SessionActivated => self.enter_live(),
			HostEvent::PointerMoved { point, rgb, active_monitor, highlighted_window } => {
				self.update_live_pointer_context(point, active_monitor, highlighted_window);
				self.scene.hud.pointer = Some(point);
				self.scene.hud.rgb = rgb;
				self.update_cursor_intent(point);
			},
			HostEvent::PrimaryInteractionStarted { point, active_monitor, highlighted_window } => {
				if self.scene.mode == CaptureMode::Live {
					self.update_live_pointer_context(point, active_monitor, highlighted_window);
					self.live_press_start = Some(point);
					self.live_press_target = resolve_live_target(
						self.scene.active_monitor,
						self.scene.highlighted_window,
					);
					self.scene.live_selection_preview = None;
				}
			},
			HostEvent::PrimaryInteractionUpdated { point, active_monitor, highlighted_window } => {
				if self.scene.mode == CaptureMode::Live {
					self.update_live_pointer_context(point, active_monitor, highlighted_window);
					self.scene.live_selection_preview =
						self.compute_live_selection_preview(point, self.scene.active_monitor);
				}
			},
			HostEvent::PrimaryInteractionCompleted {
				point,
				active_monitor,
				highlighted_window,
			} => {
				if self.scene.mode == CaptureMode::Live {
					self.update_live_pointer_context(point, active_monitor, highlighted_window);
					self.finalize_live_selection(point, self.scene.active_monitor);
				}
			},
			HostEvent::CancelRequested => self.hide_and_stop_capture(),
			HostEvent::CopyRequested => {
				if self.scene.mode == CaptureMode::Frozen {
					self.pending_requests
						.push_back(HostRequest::PerformHostEffect(HostEffectKind::CopyCapture));
				}
			},
			HostEvent::SaveRequested => {
				if self.scene.mode == CaptureMode::Frozen {
					self.pending_requests
						.push_back(HostRequest::PerformHostEffect(HostEffectKind::SaveCapture));
				}
			},
			HostEvent::RecognizeTextRequested => {
				if self.scene.mode == CaptureMode::Frozen && self.config.allow_text_input {
					self.pending_requests
						.push_back(HostRequest::PerformHostEffect(HostEffectKind::RecognizeText));
				}
			},
			HostEvent::ToggleLoupe => {
				self.scene.hud.loupe_visible = !self.scene.hud.loupe_visible;
			},
			HostEvent::ToolbarItemInvoked { item } => self.handle_toolbar_item(item),
		}
	}

	/// Applies one host-owned capability or effect report.
	pub fn handle_host_report(&mut self, report: HostReport) {
		match report {
			HostReport::FreezeSnapshotCommitted { selection } => {
				self.scene.mode = CaptureMode::Frozen;
				self.scene.live_selection_preview = None;
				self.scene.frozen_selection = Some(selection);
				self.scene.active_monitor = None;
				self.scene.highlighted_window = None;
				self.scene.cursor_intent = CursorIntent::Grab;
				self.scene.status_message = None;
				self.refresh_toolbar_actions();
			},
			HostReport::HostEffectCompleted { effect } => {
				self.scene.status_message = Some(match effect {
					HostEffectKind::CopyCapture => String::from("Copied capture."),
					HostEffectKind::SaveCapture => String::from("Saved capture."),
					HostEffectKind::RecognizeText => String::from("Recognized text."),
				});
			},
			HostReport::PermissionChanged { kind, granted } => {
				let permission = match kind {
					PermissionKind::ScreenRecording => "Screen Recording",
					PermissionKind::Accessibility => "Accessibility",
					PermissionKind::InputMonitoring => "Input Monitoring",
				};
				let verb = if granted { "granted" } else { "revoked" };

				self.scene.status_message = Some(format!("{permission} permission {verb}."));
			},
			HostReport::StatusMessage { message } => {
				self.scene.status_message = Some(message);
			},
		}
	}

	/// Pops the next core-to-host request, if one is pending.
	#[must_use]
	pub fn pop_host_request(&mut self) -> Option<HostRequest> {
		self.pending_requests.pop_front()
	}

	fn hide_and_stop_capture(&mut self) {
		self.scene = SceneModel::hidden();
		self.selected_toolbar_item = ToolbarItemKind::Pointer;
		self.live_press_start = None;
		self.live_press_target = None;
		self.pending_requests.push_back(HostRequest::StopLiveCapture);
	}

	fn handle_toolbar_item(&mut self, item: ToolbarItemKind) {
		if self.scene.mode != CaptureMode::Frozen {
			return;
		}

		match item {
			ToolbarItemKind::Pointer
			| ToolbarItemKind::Pen
			| ToolbarItemKind::Arrow
			| ToolbarItemKind::Text
			| ToolbarItemKind::Mosaic
			| ToolbarItemKind::Spotlight => {
				self.selected_toolbar_item = item;
				self.scene.status_message = None;
			},
			ToolbarItemKind::Undo
			| ToolbarItemKind::Redo
			| ToolbarItemKind::AutoCenter
			| ToolbarItemKind::Scroll => {},
			ToolbarItemKind::Ocr => {
				if self.config.allow_text_input {
					self.pending_requests
						.push_back(HostRequest::PerformHostEffect(HostEffectKind::RecognizeText));
				}
			},
			ToolbarItemKind::Copy => {
				self.pending_requests
					.push_back(HostRequest::PerformHostEffect(HostEffectKind::CopyCapture));
			},
			ToolbarItemKind::Save => {
				self.pending_requests
					.push_back(HostRequest::PerformHostEffect(HostEffectKind::SaveCapture));
			},
		}

		self.refresh_toolbar_actions();
		self.update_cursor_intent(self.scene.pointer.unwrap_or_default());
	}

	fn refresh_toolbar_actions(&mut self) {
		self.scene.toolbar_items = if self.scene.mode == CaptureMode::Frozen {
			let mut items = vec![
				self.toolbar_item(ToolbarItemKind::Pointer, true),
				self.toolbar_item(ToolbarItemKind::Pen, true),
				self.toolbar_item(ToolbarItemKind::Arrow, true),
				self.toolbar_item(ToolbarItemKind::Text, self.config.allow_text_input),
				self.toolbar_item(ToolbarItemKind::Mosaic, true),
				self.toolbar_item(ToolbarItemKind::Spotlight, true),
				self.toolbar_item(ToolbarItemKind::Undo, false),
				self.toolbar_item(ToolbarItemKind::Redo, false),
				self.toolbar_item(ToolbarItemKind::AutoCenter, true),
				self.toolbar_item(ToolbarItemKind::Scroll, false),
			];

			if self.config.allow_text_input {
				items.push(self.toolbar_item(ToolbarItemKind::Ocr, true));
			}

			items.push(self.toolbar_item(ToolbarItemKind::Copy, true));
			items.push(self.toolbar_item(ToolbarItemKind::Save, true));

			items
		} else {
			Vec::new()
		};
	}

	fn toolbar_item(&self, kind: ToolbarItemKind, enabled: bool) -> ToolbarItemModel {
		ToolbarItemModel {
			kind,
			enabled,
			selected: kind.is_mode_tool() && self.selected_toolbar_item == kind,
		}
	}

	fn update_live_pointer_context(
		&mut self,
		point: GlobalPoint,
		active_monitor: Option<crate::geometry::MonitorRect>,
		highlighted_window: Option<crate::geometry::WindowRect>,
	) {
		self.scene.pointer = Some(point);
		self.scene.active_monitor = active_monitor;
		self.scene.highlighted_window = highlighted_window;
		self.scene.hud.pointer = Some(point);
	}

	fn compute_live_selection_preview(
		&self,
		point: GlobalPoint,
		active_monitor: Option<crate::geometry::MonitorRect>,
	) -> Option<crate::geometry::GlobalRect> {
		let live_press_start = self.live_press_start?;
		let active_monitor = active_monitor?;
		if !active_monitor.contains(live_press_start) || !active_monitor.contains(point) {
			return None;
		}

		let left = live_press_start.x.min(point.x);
		let top = live_press_start.y.min(point.y);
		let width = live_press_start.x.abs_diff(point.x);
		let height = live_press_start.y.abs_diff(point.y);
		if width.max(height) < LIVE_SELECTION_DRAG_THRESHOLD_POINTS {
			return None;
		}

		Some(crate::geometry::GlobalRect::new(
			left,
			top,
			width.max(1),
			height.max(1),
		))
	}

	fn finalize_live_selection(
		&mut self,
		point: GlobalPoint,
		active_monitor: Option<crate::geometry::MonitorRect>,
	) {
		let selection = self
			.scene
			.live_selection_preview
			.or(self.live_press_target)
			.or_else(|| {
				resolve_live_target(self.scene.active_monitor, self.scene.highlighted_window)
			})
			.or_else(|| Some(default_live_selection(point, active_monitor)));

		self.live_press_start = None;
		self.live_press_target = None;

		if let Some(selection) = selection {
			self.scene.live_selection_preview = Some(selection);
			self.scene.status_message = None;
			self.pending_requests.push_back(HostRequest::RequestFreezeSnapshot { selection });
		}
	}

	fn update_cursor_intent(&mut self, point: GlobalPoint) {
		self.scene.cursor_intent = match self.scene.mode {
			CaptureMode::Hidden => CursorIntent::Default,
			CaptureMode::Live => CursorIntent::Default,
			CaptureMode::Frozen => {
				self.scene.frozen_selection.map_or(CursorIntent::Default, |selection| {
					self.frozen_cursor_intent(point, selection)
				})
			},
		};
	}

	fn frozen_cursor_intent(
		&self,
		point: GlobalPoint,
		selection: crate::geometry::GlobalRect,
	) -> CursorIntent {
		let selection_left = selection.x;
		let selection_top = selection.y;
		let selection_right = selection.x.saturating_add_unsigned(selection.width);
		let selection_bottom = selection.y.saturating_add_unsigned(selection.height);

		if point_in_handle(point, selection_left, selection_top, RESIZE_HANDLE_RADIUS_POINTS) {
			return CursorIntent::ResizeNorthWest;
		}
		if point_in_handle(point, selection_right, selection_bottom, RESIZE_HANDLE_RADIUS_POINTS) {
			return CursorIntent::ResizeSouthEast;
		}
		if point_in_handle(point, selection_right, selection_top, RESIZE_HANDLE_RADIUS_POINTS) {
			return CursorIntent::ResizeNorthEast;
		}
		if point_in_handle(point, selection_left, selection_bottom, RESIZE_HANDLE_RADIUS_POINTS) {
			return CursorIntent::ResizeSouthWest;
		}

		let on_vertical_edge = point.y >= selection_top
			&& point.y <= selection_bottom
			&& (point.x - selection_left).abs() <= RESIZE_EDGE_TOLERANCE_POINTS;
		if on_vertical_edge {
			return CursorIntent::ResizeWest;
		}

		let on_right_edge = point.y >= selection_top
			&& point.y <= selection_bottom
			&& (point.x - selection_right).abs() <= RESIZE_EDGE_TOLERANCE_POINTS;
		if on_right_edge {
			return CursorIntent::ResizeEast;
		}

		let on_top_edge = point.x >= selection_left
			&& point.x <= selection_right
			&& (point.y - selection_top).abs() <= RESIZE_EDGE_TOLERANCE_POINTS;
		if on_top_edge {
			return CursorIntent::ResizeNorth;
		}

		let on_bottom_edge = point.x >= selection_left
			&& point.x <= selection_right
			&& (point.y - selection_bottom).abs() <= RESIZE_EDGE_TOLERANCE_POINTS;
		if on_bottom_edge {
			return CursorIntent::ResizeSouth;
		}

		if selection.contains(point) {
			return match self.selected_toolbar_item {
				ToolbarItemKind::Text => CursorIntent::Text,
				ToolbarItemKind::Pointer => CursorIntent::Grab,
				_ => CursorIntent::Default,
			};
		}

		CursorIntent::Default
	}
}

fn point_in_handle(point: GlobalPoint, handle_x: i32, handle_y: i32, radius: i32) -> bool {
	(point.x - handle_x).abs() <= radius && (point.y - handle_y).abs() <= radius
}

fn resolve_live_target(
	active_monitor: Option<crate::geometry::MonitorRect>,
	highlighted_window: Option<crate::geometry::WindowRect>,
) -> Option<crate::geometry::GlobalRect> {
	highlighted_window.and_then(crate::geometry::WindowRect::global_rect).or_else(|| {
		active_monitor.map(|monitor| {
			crate::geometry::GlobalRect::new(
				monitor.origin.x,
				monitor.origin.y,
				monitor.width,
				monitor.height,
			)
		})
	})
}

fn default_live_selection(
	point: GlobalPoint,
	active_monitor: Option<crate::geometry::MonitorRect>,
) -> crate::geometry::GlobalRect {
	let half_width = (LIVE_SELECTION_DEFAULT_WIDTH / 2) as i32;
	let half_height = (LIVE_SELECTION_DEFAULT_HEIGHT / 2) as i32;

	let unclamped_x = point.x.saturating_sub(half_width);
	let unclamped_y = point.y.saturating_sub(half_height);

	let (origin_x, origin_y) = if let Some(monitor) = active_monitor {
		let max_x = if monitor.width > LIVE_SELECTION_DEFAULT_WIDTH {
			monitor
				.origin
				.x
				.saturating_add_unsigned(monitor.width)
				.saturating_sub_unsigned(LIVE_SELECTION_DEFAULT_WIDTH)
		} else {
			monitor.origin.x
		};
		let max_y = if monitor.height > LIVE_SELECTION_DEFAULT_HEIGHT {
			monitor
				.origin
				.y
				.saturating_add_unsigned(monitor.height)
				.saturating_sub_unsigned(LIVE_SELECTION_DEFAULT_HEIGHT)
		} else {
			monitor.origin.y
		};
		(unclamped_x.clamp(monitor.origin.x, max_x), unclamped_y.clamp(monitor.origin.y, max_y))
	} else {
		(unclamped_x, unclamped_y)
	};

	crate::geometry::GlobalRect::new(
		origin_x,
		origin_y,
		LIVE_SELECTION_DEFAULT_WIDTH,
		LIVE_SELECTION_DEFAULT_HEIGHT,
	)
}

#[cfg(test)]
mod tests {
	use crate::geometry::{GlobalPoint, GlobalRect, MonitorRect, Rgb, WindowRect};
	use crate::protocol::{
		CaptureMode, CursorIntent, HostEffectKind, HostEvent, HostReport, HostRequest,
		SessionConfig, ToolbarItemKind,
	};
	use crate::session::CaptureSessionCore;

	fn active_monitor() -> MonitorRect {
		MonitorRect {
			id: 7,
			origin: GlobalPoint::new(0, 0),
			width: 1440,
			height: 900,
			scale_factor_x1000: 2_000,
		}
	}

	fn highlighted_window() -> WindowRect {
		WindowRect { window_id: Some(42), x: 10, y: 20, width: 320, height: 240 }
	}

	#[test]
	fn enter_live_requests_capture_and_default_cursor() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();

		assert_eq!(session.scene_model().mode, CaptureMode::Live);
		assert_eq!(session.scene_model().cursor_intent, CursorIntent::Default);
		assert_eq!(session.pop_host_request(), Some(HostRequest::StartLiveCapture));
	}

	#[test]
	fn freeze_commit_enables_frozen_actions() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_event(HostEvent::PrimaryInteractionCompleted {
			point: GlobalPoint::new(40, 60),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});
		session.handle_host_report(HostReport::FreezeSnapshotCommitted {
			selection: GlobalRect::new(10, 20, 100, 50),
		});

		assert_eq!(session.scene_model().mode, CaptureMode::Frozen);
		assert_eq!(session.scene_model().cursor_intent, CursorIntent::Grab);
		assert_eq!(session.scene_model().toolbar_items.len(), 13);
	}

	#[test]
	fn pointer_update_tracks_rgb_and_frozen_grab() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_report(HostReport::FreezeSnapshotCommitted {
			selection: GlobalRect::new(10, 20, 100, 50),
		});
		session.handle_host_event(HostEvent::PointerMoved {
			point: GlobalPoint::new(40, 40),
			rgb: Some(Rgb::new(1, 2, 3)),
			active_monitor: None,
			highlighted_window: None,
		});

		assert_eq!(session.scene_model().cursor_intent, CursorIntent::Grab);
		assert_eq!(session.scene_model().hud.rgb, Some(Rgb::new(1, 2, 3)));
	}

	#[test]
	fn frozen_pointer_cursor_tracks_resize_edges() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_report(HostReport::FreezeSnapshotCommitted {
			selection: GlobalRect::new(10, 20, 100, 50),
		});

		session.handle_host_event(HostEvent::PointerMoved {
			point: GlobalPoint::new(110, 45),
			rgb: None,
			active_monitor: None,
			highlighted_window: None,
		});
		assert_eq!(session.scene_model().cursor_intent, CursorIntent::ResizeEast);

		session.handle_host_event(HostEvent::PointerMoved {
			point: GlobalPoint::new(10, 20),
			rgb: None,
			active_monitor: None,
			highlighted_window: None,
		});
		assert_eq!(session.scene_model().cursor_intent, CursorIntent::ResizeNorthWest);
	}

	#[test]
	fn live_pointer_update_tracks_monitor_and_window() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_event(HostEvent::PointerMoved {
			point: GlobalPoint::new(120, 180),
			rgb: Some(Rgb::new(9, 8, 7)),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		assert_eq!(session.scene_model().pointer, Some(GlobalPoint::new(120, 180)));
		assert_eq!(session.scene_model().active_monitor, Some(active_monitor()));
		assert_eq!(session.scene_model().highlighted_window, Some(highlighted_window()));
	}

	#[test]
	fn primary_drag_updates_live_preview_and_freezes_with_preview_selection() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_event(HostEvent::PrimaryInteractionStarted {
			point: GlobalPoint::new(20, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});
		session.handle_host_event(HostEvent::PrimaryInteractionUpdated {
			point: GlobalPoint::new(80, 110),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		assert_eq!(
			session.scene_model().live_selection_preview,
			Some(GlobalRect::new(20, 30, 60, 80))
		);

		session.handle_host_event(HostEvent::PrimaryInteractionCompleted {
			point: GlobalPoint::new(80, 110),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		assert_eq!(session.scene_model().mode, CaptureMode::Live);
		assert_eq!(
			session.scene_model().live_selection_preview,
			Some(GlobalRect::new(20, 30, 60, 80))
		);
		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::RequestFreezeSnapshot {
				selection: GlobalRect::new(20, 30, 60, 80),
			})
		);
	}

	#[test]
	fn primary_drag_allows_thin_preview_once_drag_threshold_is_crossed() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_event(HostEvent::PrimaryInteractionStarted {
			point: GlobalPoint::new(20, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});
		session.handle_host_event(HostEvent::PrimaryInteractionUpdated {
			point: GlobalPoint::new(21, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		assert_eq!(
			session.scene_model().live_selection_preview,
			Some(GlobalRect::new(20, 30, 1, 1))
		);

		session.handle_host_event(HostEvent::PrimaryInteractionCompleted {
			point: GlobalPoint::new(21, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::RequestFreezeSnapshot {
				selection: GlobalRect::new(20, 30, 1, 1),
			})
		);
	}

	#[test]
	fn primary_interaction_below_drag_threshold_stays_click_targeted() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_event(HostEvent::PrimaryInteractionStarted {
			point: GlobalPoint::new(20, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});
		session.handle_host_event(HostEvent::PrimaryInteractionUpdated {
			point: GlobalPoint::new(20, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		assert_eq!(session.scene_model().live_selection_preview, None);

		session.handle_host_event(HostEvent::PrimaryInteractionCompleted {
			point: GlobalPoint::new(20, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::RequestFreezeSnapshot {
				selection: highlighted_window().global_rect().unwrap(),
			})
		);
	}

	#[test]
	fn cancel_hides_scene_and_requests_stop() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_event(HostEvent::CancelRequested);

		assert_eq!(session.scene_model().mode, CaptureMode::Hidden);
		assert_eq!(session.pop_host_request(), Some(HostRequest::StopLiveCapture));
	}

	#[test]
	fn copy_and_save_emit_host_effects_only_when_frozen() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.handle_host_event(HostEvent::CopyRequested);
		assert_eq!(session.pop_host_request(), None);

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_report(HostReport::FreezeSnapshotCommitted {
			selection: GlobalRect::new(0, 0, 10, 10),
		});
		session.handle_host_event(HostEvent::CopyRequested);
		session.handle_host_event(HostEvent::SaveRequested);

		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::PerformHostEffect(HostEffectKind::CopyCapture))
		);
		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::PerformHostEffect(HostEffectKind::SaveCapture))
		);
	}

	#[test]
	fn toolbar_item_invocation_updates_selected_tool_and_effects() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_report(HostReport::FreezeSnapshotCommitted {
			selection: GlobalRect::new(10, 20, 100, 50),
		});

		session.handle_host_event(HostEvent::ToolbarItemInvoked { item: ToolbarItemKind::Text });
		assert!(
			session
				.scene_model()
				.toolbar_items
				.iter()
				.any(|item| item.kind == ToolbarItemKind::Text && item.selected)
		);

		session.handle_host_event(HostEvent::ToolbarItemInvoked { item: ToolbarItemKind::Copy });
		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::PerformHostEffect(HostEffectKind::CopyCapture))
		);
	}

	#[test]
	fn recognize_text_requires_frozen_mode_and_text_support() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.handle_host_event(HostEvent::RecognizeTextRequested);
		assert_eq!(session.pop_host_request(), None);

		session.enter_live();
		let _ = session.pop_host_request();
		session.handle_host_report(HostReport::FreezeSnapshotCommitted {
			selection: GlobalRect::new(0, 0, 10, 10),
		});
		session.handle_host_event(HostEvent::RecognizeTextRequested);

		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::PerformHostEffect(HostEffectKind::RecognizeText))
		);
	}

	#[test]
	fn host_status_message_updates_scene_status() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.handle_host_report(HostReport::StatusMessage {
			message: String::from("Screen recording permission is required."),
		});

		assert_eq!(
			session.scene_model().status_message.as_deref(),
			Some("Screen recording permission is required.")
		);
	}
}
