//! Minimal reference session used to drive the new host/core boundary.

mod selection_interaction;

use std::collections::VecDeque;

use crate::geometry::{GlobalPoint, GlobalRect, MonitorRect, WindowRect};
use crate::protocol::{
	CaptureMode, CursorIntent, HostEffectKind, HostEvent, HostReport, HostRequest, PermissionKind,
	SceneModel, SessionConfig, ToolbarItemKind, ToolbarItemModel,
};

/// Reference capture-session core that owns semantic state and emits host requests.
#[derive(Debug)]
pub struct CaptureSessionCore {
	config: SessionConfig,
	scene: SceneModel,
	selected_toolbar_item: ToolbarItemKind,
	live_press_start: Option<GlobalPoint>,
	live_press_target: Option<GlobalRect>,
	pending_frozen_selection_editable: bool,
	frozen_selection_editable: bool,
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
			pending_frozen_selection_editable: false,
			frozen_selection_editable: false,
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
		self.scene.cursor_intent = CursorIntent::Crosshair;
		self.scene.pointer = None;
		self.scene.active_monitor = None;
		self.scene.highlighted_window = None;
		self.scene.live_selection_preview = None;
		self.scene.frozen_selection = None;
		self.scene.status_message = None;
		self.selected_toolbar_item = ToolbarItemKind::Pointer;
		self.live_press_start = None;
		self.live_press_target = None;
		self.pending_frozen_selection_editable = false;
		self.frozen_selection_editable = false;

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
					self.live_press_target = selection_interaction::resolve_target(
						self.scene.active_monitor,
						self.scene.highlighted_window,
					);
					self.scene.live_selection_preview = None;
				}
			},
			HostEvent::PrimaryInteractionUpdated { point, active_monitor, highlighted_window } => {
				if self.scene.mode == CaptureMode::Live {
					self.update_live_pointer_context(point, active_monitor, highlighted_window);

					self.scene.live_selection_preview = selection_interaction::drag_preview(
						self.live_press_start,
						point,
						self.scene.active_monitor,
					);
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
				let selection_editable = self.pending_frozen_selection_editable
					|| (self.scene.mode == CaptureMode::Frozen && self.frozen_selection_editable);

				self.scene.mode = CaptureMode::Frozen;
				self.scene.live_selection_preview = None;
				self.scene.frozen_selection = Some(selection);
				self.scene.active_monitor = None;
				self.scene.highlighted_window = None;
				self.frozen_selection_editable = selection_editable;
				self.pending_frozen_selection_editable = false;
				self.scene.cursor_intent = if self.frozen_selection_editable {
					CursorIntent::Grab
				} else {
					CursorIntent::Default
				};
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
			ToolbarItemKind::Undo | ToolbarItemKind::Redo | ToolbarItemKind::AutoCenter => {},
			ToolbarItemKind::Scroll => {
				if self.frozen_selection_editable {
					self.pending_requests.push_back(HostRequest::StartScrollCapture);
				}
			},
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
			];

			if self.frozen_selection_editable {
				items.push(self.toolbar_item(ToolbarItemKind::Scroll, true));
			}
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
		active_monitor: Option<MonitorRect>,
		highlighted_window: Option<WindowRect>,
	) {
		self.scene.pointer = Some(point);
		self.scene.active_monitor = active_monitor;
		self.scene.highlighted_window = highlighted_window;
		self.scene.hud.pointer = Some(point);
	}

	fn finalize_live_selection(&mut self, point: GlobalPoint, active_monitor: Option<MonitorRect>) {
		let had_live_press = self.live_press_start.is_some();
		let release_drag_selection = if had_live_press {
			selection_interaction::drag_preview(self.live_press_start, point, active_monitor)
		} else {
			None
		};
		let selection_editable = release_drag_selection.is_some();
		let selection = if had_live_press {
			release_drag_selection
				.or(self.live_press_target)
				.or_else(|| {
					selection_interaction::resolve_target(
						self.scene.active_monitor,
						self.scene.highlighted_window,
					)
				})
				.or_else(|| Some(selection_interaction::default_selection(point, active_monitor)))
		} else {
			self.scene
				.live_selection_preview
				.or(self.live_press_target)
				.or_else(|| {
					selection_interaction::resolve_target(
						self.scene.active_monitor,
						self.scene.highlighted_window,
					)
				})
				.or_else(|| Some(selection_interaction::default_selection(point, active_monitor)))
		};

		self.live_press_start = None;
		self.live_press_target = None;

		if let Some(selection) = selection {
			self.pending_frozen_selection_editable = selection_editable;
			self.scene.live_selection_preview = Some(selection);
			self.scene.status_message = None;

			self.pending_requests
				.push_back(HostRequest::RequestFreezeSnapshot { selection, selection_editable });
		}
	}

	fn update_cursor_intent(&mut self, point: GlobalPoint) {
		self.scene.cursor_intent = match self.scene.mode {
			CaptureMode::Hidden => CursorIntent::Default,
			CaptureMode::Live => CursorIntent::Crosshair,
			CaptureMode::Frozen => {
				if !self.frozen_selection_editable {
					CursorIntent::Default
				} else {
					self.scene.frozen_selection.map_or(CursorIntent::Default, |selection| {
						selection_interaction::frozen_cursor_intent(
							point,
							selection,
							self.selected_toolbar_item,
						)
					})
				}
			},
		};
	}
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
			width: 1_440,
			height: 900,
			scale_factor_x1000: 2_000,
		}
	}

	fn highlighted_window() -> WindowRect {
		WindowRect { window_id: Some(42), x: 10, y: 20, width: 320, height: 240 }
	}

	fn enter_frozen_with_drag_selection(session: &mut CaptureSessionCore, selection: GlobalRect) {
		session.enter_live();

		let _ = session.pop_host_request();
		let start = GlobalPoint::new(selection.x, selection.y);
		let end = GlobalPoint::new(
			selection.x.saturating_add_unsigned(selection.width),
			selection.y.saturating_add_unsigned(selection.height),
		);

		session.handle_host_event(HostEvent::PrimaryInteractionStarted {
			point: start,
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});
		session.handle_host_event(HostEvent::PrimaryInteractionCompleted {
			point: end,
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::RequestFreezeSnapshot { selection, selection_editable: true })
		);

		session.handle_host_report(HostReport::FreezeSnapshotCommitted { selection });
	}

	#[test]
	fn enter_live_requests_capture_and_crosshair_cursor() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();

		assert_eq!(session.scene_model().mode, CaptureMode::Live);
		assert_eq!(session.scene_model().cursor_intent, CursorIntent::Crosshair);
		assert_eq!(session.pop_host_request(), Some(HostRequest::StartLiveCapture));
	}

	#[test]
	fn freeze_commit_enables_frozen_actions() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		enter_frozen_with_drag_selection(&mut session, GlobalRect::new(10, 20, 100, 50));

		assert_eq!(session.scene_model().mode, CaptureMode::Frozen);
		assert_eq!(session.scene_model().cursor_intent, CursorIntent::Grab);
		assert_eq!(session.scene_model().toolbar_items.len(), 13);
		assert!(
			session
				.scene_model()
				.toolbar_items
				.iter()
				.any(|item| item.kind == ToolbarItemKind::Scroll && item.enabled)
		);
	}

	#[test]
	fn pointer_update_tracks_rgb_and_frozen_grab() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		enter_frozen_with_drag_selection(&mut session, GlobalRect::new(10, 20, 100, 50));

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

		enter_frozen_with_drag_selection(&mut session, GlobalRect::new(10, 20, 100, 50));

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
	fn live_pointer_update_without_window_clears_previous_highlight() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();

		let _ = session.pop_host_request();

		session.handle_host_event(HostEvent::PointerMoved {
			point: GlobalPoint::new(120, 180),
			rgb: None,
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});
		session.handle_host_event(HostEvent::PointerMoved {
			point: GlobalPoint::new(900, 700),
			rgb: None,
			active_monitor: Some(active_monitor()),
			highlighted_window: None,
		});

		assert_eq!(session.scene_model().pointer, Some(GlobalPoint::new(900, 700)));
		assert_eq!(session.scene_model().active_monitor, Some(active_monitor()));
		assert_eq!(session.scene_model().highlighted_window, None);
	}

	#[test]
	fn primary_click_freezes_highlighted_window_not_editable() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();

		let _ = session.pop_host_request();

		session.handle_host_event(HostEvent::PrimaryInteractionStarted {
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
				selection_editable: false,
			})
		);
	}

	#[test]
	fn primary_click_frozen_window_selection_keeps_default_cursor() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();

		let _ = session.pop_host_request();

		session.handle_host_event(HostEvent::PrimaryInteractionStarted {
			point: GlobalPoint::new(20, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});
		session.handle_host_event(HostEvent::PrimaryInteractionCompleted {
			point: GlobalPoint::new(20, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		let selection = highlighted_window().global_rect().unwrap();

		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::RequestFreezeSnapshot { selection, selection_editable: false })
		);

		session.handle_host_report(HostReport::FreezeSnapshotCommitted { selection });
		session.handle_host_event(HostEvent::PointerMoved {
			point: GlobalPoint::new(80, 90),
			rgb: None,
			active_monitor: None,
			highlighted_window: None,
		});

		assert_eq!(session.scene_model().cursor_intent, CursorIntent::Default);
	}

	#[test]
	fn primary_click_fullscreen_fallback_is_not_editable() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();

		let _ = session.pop_host_request();

		session.handle_host_event(HostEvent::PrimaryInteractionStarted {
			point: GlobalPoint::new(20, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: None,
		});
		session.handle_host_event(HostEvent::PrimaryInteractionCompleted {
			point: GlobalPoint::new(20, 30),
			active_monitor: Some(active_monitor()),
			highlighted_window: None,
		});

		let monitor = active_monitor();
		let selection =
			GlobalRect::new(monitor.origin.x, monitor.origin.y, monitor.width, monitor.height);

		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::RequestFreezeSnapshot { selection, selection_editable: false })
		);

		session.handle_host_report(HostReport::FreezeSnapshotCommitted { selection });
		session.handle_host_event(HostEvent::PointerMoved {
			point: GlobalPoint::new(80, 90),
			rgb: None,
			active_monitor: None,
			highlighted_window: None,
		});

		assert_eq!(session.scene_model().cursor_intent, CursorIntent::Default);
	}

	#[test]
	fn primary_drag_freezes_release_rect_editable_with_thin_preview_allowed() {
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
			point: GlobalPoint::new(90, 130),
			active_monitor: Some(active_monitor()),
			highlighted_window: Some(highlighted_window()),
		});

		assert_eq!(
			session.pop_host_request(),
			Some(HostRequest::RequestFreezeSnapshot {
				selection: GlobalRect::new(20, 30, 70, 100),
				selection_editable: true,
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
	fn scroll_toolbar_invocation_requests_scroll_capture_for_drag_region() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		enter_frozen_with_drag_selection(&mut session, GlobalRect::new(10, 20, 100, 50));

		assert!(
			session
				.scene_model()
				.toolbar_items
				.iter()
				.any(|item| item.kind == ToolbarItemKind::Scroll && item.enabled)
		);

		session.handle_host_event(HostEvent::ToolbarItemInvoked { item: ToolbarItemKind::Scroll });

		assert_eq!(session.pop_host_request(), Some(HostRequest::StartScrollCapture));
	}

	#[test]
	fn scroll_toolbar_survives_drag_region_recommit() {
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		enter_frozen_with_drag_selection(&mut session, GlobalRect::new(10, 20, 100, 50));

		session.handle_host_report(HostReport::FreezeSnapshotCommitted {
			selection: GlobalRect::new(20, 30, 100, 50),
		});

		assert!(
			session
				.scene_model()
				.toolbar_items
				.iter()
				.any(|item| item.kind == ToolbarItemKind::Scroll && item.enabled)
		);

		session.handle_host_event(HostEvent::ToolbarItemInvoked { item: ToolbarItemKind::Scroll });

		assert_eq!(session.pop_host_request(), Some(HostRequest::StartScrollCapture));
	}

	#[test]
	fn scroll_toolbar_is_absent_for_non_editable_frozen_selection() {
		let selection = GlobalRect::new(10, 20, 100, 50);
		let mut session = CaptureSessionCore::with_config(SessionConfig::default());

		session.enter_live();

		let _ = session.pop_host_request();

		session.handle_host_report(HostReport::FreezeSnapshotCommitted { selection });
		session.handle_host_event(HostEvent::ToolbarItemInvoked { item: ToolbarItemKind::Scroll });

		assert!(
			session
				.scene_model()
				.toolbar_items
				.iter()
				.all(|item| item.kind != ToolbarItemKind::Scroll)
		);
		assert_eq!(session.pop_host_request(), None);
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
