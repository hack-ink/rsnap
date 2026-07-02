use std::collections::HashMap;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use egui::{Pos2, Rect, Vec2};
use objc::declare::ClassDecl;
use objc::runtime::{BOOL, Class, Object, Sel, YES};
use objc::{Encode, Encoding, sel, sel_impl};
use objc2::MainThreadMarker;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use winit::window::{CursorIcon, Window};

pub(super) struct MacOSOverlayCursorRectSupport {
	view_key: usize,
}
impl MacOSOverlayCursorRectSupport {
	const fn new(view_key: usize) -> Self {
		Self { view_key }
	}

	pub(super) fn sync_cursor_rects(&self, window: &Window, rects: &[OverlayCursorRect]) {
		macos_resize_overlay_cursor_view(window, self.view_key);

		if macos_set_overlay_view_cursor_rects(self.view_key, rects) {
			macos_invalidate_overlay_cursor_rects(self.view_key);
		}

		macos_apply_overlay_cursor_for_current_pointer(self.view_key);
	}

	pub(super) fn apply_cursor_for_current_pointer(&self) {
		tracing::trace!(
			op = "overlay.macos_overlay_cursor_rect_support_apply_current_pointer",
			view_key = self.view_key,
			"Applying macOS overlay cursor rect authority for current pointer."
		);

		macos_apply_overlay_cursor_for_current_pointer(self.view_key);
	}

	pub(super) fn apply_cursor_for_current_pointer_or_fallback(&self, fallback_icon: CursorIcon) {
		if macos_overlay_view_cursor_rect_entries(self.view_key).is_none() {
			tracing::trace!(
				op = "overlay.macos_overlay_cursor_rect_support_apply_fallback",
				view_key = self.view_key,
				icon = ?fallback_icon,
				"Fell back to the session cursor icon because the render cursor rects are not ready yet."
			);

			macos_set_cursor_icon(fallback_icon);

			return;
		}

		self.apply_cursor_for_current_pointer();
	}
}

impl Drop for MacOSOverlayCursorRectSupport {
	fn drop(&mut self) {
		let rects = macos_overlay_view_cursor_rects();

		match rects.lock() {
			Ok(mut guard) => {
				guard.remove(&self.view_key);
			},
			Err(poisoned) => {
				poisoned.into_inner().remove(&self.view_key);
			},
		}

		macos_remove_overlay_cursor_view(self.view_key);
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct OverlayCursorRect {
	pub(super) rect: Rect,
	pub(super) icon: CursorIcon,
}
impl OverlayCursorRect {
	pub(super) const fn new(rect: Rect, icon: CursorIcon) -> Self {
		Self { rect, icon }
	}
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct MacOSOverlayPoint {
	pub(super) x: f64,
	pub(super) y: f64,
}
unsafe impl Encode for MacOSOverlayPoint {
	fn encode() -> Encoding {
		unsafe { Encoding::from_str("{CGPoint=dd}") }
	}
}

pub(super) fn overlay_cursor_rect_icon_at_point(
	rects: &[OverlayCursorRect],
	point: Pos2,
) -> Option<CursorIcon> {
	rects.iter().find(|entry| entry.rect.contains(point)).map(|entry| entry.icon)
}

pub(super) fn sort_unique_axis_values(values: &mut Vec<f32>) {
	values.sort_by(f32::total_cmp);
	values.dedup_by(|a, b| (*a - *b).abs() <= f32::EPSILON);
}

pub(super) fn trim_rect_min_edge(min: f32, max: f32) -> f32 {
	let trimmed = min.next_up();

	if trimmed < max { trimmed } else { max }
}

pub(super) fn trim_rect_max_edge(max: f32, min: f32) -> f32 {
	let trimmed = max.next_down();

	if trimmed > min { trimmed } else { min }
}

pub(super) fn macos_install_overlay_cursor_rect_support(
	window: &Window,
) -> std::result::Result<MacOSOverlayCursorRectSupport, String> {
	let _ = MainThreadMarker::new().ok_or_else(|| {
		String::from("Installing macOS overlay cursor rect support requires the main thread.")
	})?;
	let Some(host_view) = super::macos_overlay_window_ns_view(window) else {
		return Err(String::from("Overlay cursor rect support requires an AppKit window handle."));
	};
	let bounds: NSRect = unsafe { objc::msg_send![host_view, bounds] };
	let overlay_class = macos_overlay_cursor_view_class();
	let overlay_view: *mut Object = unsafe {
		let overlay_view: *mut Object = objc::msg_send![overlay_class, alloc];

		objc::msg_send![overlay_view, initWithFrame: bounds]
	};

	if overlay_view.is_null() {
		return Err(String::from("Failed to create macOS overlay cursor view."));
	}

	unsafe {
		const NS_VIEW_WIDTH_SIZABLE: usize = 2;
		const NS_VIEW_HEIGHT_SIZABLE: usize = 16;

		let _: () = objc::msg_send![overlay_view, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
		let _: () = objc::msg_send![host_view, addSubview: overlay_view];
		let _: () = objc::msg_send![overlay_view, release];
	}

	Ok(MacOSOverlayCursorRectSupport::new(overlay_view as usize))
}

pub(super) fn macos_cursor_object_for_icon(icon: CursorIcon) -> *mut Object {
	let cursor_class = objc::class!(NSCursor);

	match icon {
		CursorIcon::Crosshair => unsafe { objc::msg_send![cursor_class, crosshairCursor] },
		CursorIcon::Grab => unsafe { objc::msg_send![cursor_class, openHandCursor] },
		CursorIcon::Grabbing => unsafe { objc::msg_send![cursor_class, closedHandCursor] },
		CursorIcon::Text => unsafe { objc::msg_send![cursor_class, IBeamCursor] },
		CursorIcon::NeswResize => unsafe {
			let responds: bool = objc::msg_send![cursor_class, respondsToSelector: objc::sel!(_windowResizeNorthEastSouthWestCursor)];

			if responds {
				objc::msg_send![cursor_class, performSelector: objc::sel!(_windowResizeNorthEastSouthWestCursor)]
			} else {
				objc::msg_send![cursor_class, arrowCursor]
			}
		},
		CursorIcon::NwseResize => unsafe {
			let responds: bool = objc::msg_send![cursor_class, respondsToSelector: objc::sel!(_windowResizeNorthWestSouthEastCursor)];

			if responds {
				objc::msg_send![cursor_class, performSelector: objc::sel!(_windowResizeNorthWestSouthEastCursor)]
			} else {
				objc::msg_send![cursor_class, arrowCursor]
			}
		},
		_ => unsafe { objc::msg_send![cursor_class, arrowCursor] },
	}
}

pub(super) fn macos_set_cursor_icon(icon: CursorIcon) {
	let cursor = macos_cursor_object_for_icon(icon);

	if cursor.is_null() {
		tracing::trace!(
			op = "overlay.macos_set_cursor_icon",
			icon = ?icon,
			cursor_available = false,
			"Skipped macOS cursor update because no cursor object was available."
		);

		return;
	}

	tracing::trace!(
		op = "overlay.macos_set_cursor_icon",
		icon = ?icon,
		cursor_available = true,
		"Setting macOS cursor icon."
	);

	unsafe {
		let _: () = objc::msg_send![cursor, set];
	}
}

pub(super) fn macos_cursor_icon_for_current_pointer(
	entries: Option<&[OverlayCursorRect]>,
	local_point: Option<Pos2>,
	overlay_bounds: Option<Rect>,
) -> Option<CursorIcon> {
	let local_point = local_point?;
	let overlay_bounds = overlay_bounds?;

	if !overlay_bounds.contains(local_point) {
		return None;
	}

	Some(match entries {
		Some(entries) => {
			overlay_cursor_rect_icon_at_point(entries, local_point).unwrap_or(CursorIcon::Default)
		},
		None => CursorIcon::Default,
	})
}

fn macos_overlay_view_cursor_rects() -> &'static Mutex<HashMap<usize, Vec<OverlayCursorRect>>> {
	static RECTS: OnceLock<Mutex<HashMap<usize, Vec<OverlayCursorRect>>>> = OnceLock::new();

	RECTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn macos_set_overlay_view_cursor_rects(view_key: usize, rects: &[OverlayCursorRect]) -> bool {
	let rects_by_view = macos_overlay_view_cursor_rects();

	match rects_by_view.lock() {
		Ok(mut guard) => {
			let unchanged =
				guard.get(&view_key).is_some_and(|existing| existing.as_slice() == rects);

			if unchanged || (rects.is_empty() && !guard.contains_key(&view_key)) {
				return false;
			}
			if rects.is_empty() {
				guard.remove(&view_key);
			} else {
				guard.insert(view_key, rects.to_vec());
			}
		},
		Err(poisoned) => {
			let mut guard = poisoned.into_inner();
			let unchanged =
				guard.get(&view_key).is_some_and(|existing| existing.as_slice() == rects);

			if unchanged || (rects.is_empty() && !guard.contains_key(&view_key)) {
				return false;
			}
			if rects.is_empty() {
				guard.remove(&view_key);
			} else {
				guard.insert(view_key, rects.to_vec());
			}
		},
	}

	true
}

fn macos_overlay_view_cursor_rect_entries(view_key: usize) -> Option<Vec<OverlayCursorRect>> {
	let rects = macos_overlay_view_cursor_rects();

	match rects.lock() {
		Ok(guard) => guard.get(&view_key).cloned(),
		Err(poisoned) => poisoned.into_inner().get(&view_key).cloned(),
	}
}

extern "C" fn macos_overlay_cursor_view_is_flipped(_this: &Object, _cmd: Sel) -> BOOL {
	let _ = _cmd;

	YES
}

extern "C" fn macos_overlay_cursor_view_hit_test(
	_this: &Object,
	_cmd: Sel,
	_point: MacOSOverlayPoint,
) -> *mut Object {
	let _ = (_cmd, _point);

	ptr::null_mut()
}

extern "C" fn macos_overlay_cursor_view_reset_cursor_rects(this: &Object, _cmd: Sel) {
	let _ = _cmd;
	let view_key = (this as *const Object) as usize;
	let Some(entries) = macos_overlay_view_cursor_rect_entries(view_key) else {
		return;
	};

	for entry in entries {
		let cursor = macos_cursor_object_for_icon(entry.icon);

		if cursor.is_null() {
			continue;
		}

		let rect = NSRect::new(
			NSPoint::new(f64::from(entry.rect.min.x), f64::from(entry.rect.min.y)),
			NSSize::new(f64::from(entry.rect.width()), f64::from(entry.rect.height())),
		);

		unsafe {
			let _: () = objc::msg_send![this, addCursorRect: rect cursor: cursor];
		}
	}
}

fn macos_overlay_cursor_view_class() -> *const Class {
	static CLASS: OnceLock<usize> = OnceLock::new();

	(*CLASS.get_or_init(|| {
		if let Some(class) = Class::get("RsnapOverlayCursorView") {
			return class as *const Class as usize;
		}

		let superclass = objc::class!(NSView);
		let mut decl = ClassDecl::new("RsnapOverlayCursorView", superclass)
			.expect("cursor overlay view class");

		unsafe {
			decl.add_method(
				objc::sel!(isFlipped),
				macos_overlay_cursor_view_is_flipped as extern "C" fn(&Object, Sel) -> BOOL,
			);
			decl.add_method(
				objc::sel!(hitTest:),
				macos_overlay_cursor_view_hit_test
					as extern "C" fn(&Object, Sel, MacOSOverlayPoint) -> *mut Object,
			);
			decl.add_method(
				objc::sel!(resetCursorRects),
				macos_overlay_cursor_view_reset_cursor_rects as extern "C" fn(&Object, Sel),
			);
		}

		decl.register() as *const Class as usize
	})) as *const Class
}

fn macos_resize_overlay_cursor_view(window: &Window, overlay_view_key: usize) {
	let Some(ns_view) = super::macos_overlay_window_ns_view(window) else {
		return;
	};
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return;
	}

	let bounds: NSRect = unsafe { objc::msg_send![ns_view, bounds] };

	unsafe {
		let _: () = objc::msg_send![overlay_view, setFrame: bounds];
	}
}

fn macos_invalidate_overlay_cursor_rects(overlay_view_key: usize) {
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return;
	}

	unsafe {
		let ns_window: *mut Object = objc::msg_send![overlay_view, window];

		if ns_window.is_null() {
			return;
		}

		let _: () = objc::msg_send![ns_window, invalidateCursorRectsForView: overlay_view];
	}
}

fn macos_overlay_view_current_local_point(overlay_view_key: usize) -> Option<Pos2> {
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return None;
	}

	unsafe {
		let ns_window: *mut Object = objc::msg_send![overlay_view, window];

		if ns_window.is_null() {
			return None;
		}

		let window_point: NSPoint = objc::msg_send![ns_window, mouseLocationOutsideOfEventStream];
		let local_point: NSPoint = objc::msg_send![overlay_view, convertPoint: window_point fromView: ptr::null_mut::<Object>()];

		Some(Pos2::new(local_point.x as f32, local_point.y as f32))
	}
}

fn macos_overlay_view_bounds(overlay_view_key: usize) -> Option<Rect> {
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return None;
	}

	unsafe {
		let bounds: NSRect = objc::msg_send![overlay_view, bounds];

		Some(Rect::from_min_size(
			Pos2::new(bounds.origin.x as f32, bounds.origin.y as f32),
			Vec2::new(bounds.size.width as f32, bounds.size.height as f32),
		))
	}
}

fn macos_apply_overlay_cursor_for_current_pointer(overlay_view_key: usize) {
	let entries = macos_overlay_view_cursor_rect_entries(overlay_view_key);
	let local_point = macos_overlay_view_current_local_point(overlay_view_key);
	let overlay_bounds = macos_overlay_view_bounds(overlay_view_key);
	let Some(icon) =
		macos_cursor_icon_for_current_pointer(entries.as_deref(), local_point, overlay_bounds)
	else {
		tracing::trace!(
			op = "overlay.macos_apply_overlay_cursor_for_current_pointer",
			view_key = overlay_view_key,
			entry_count = entries.as_ref().map_or(0, Vec::len),
			local_point = ?local_point,
			overlay_bounds = ?overlay_bounds,
			icon = ?"none",
			"Skipped macOS overlay cursor apply because the pointer is not within an active cursor rect."
		);

		return;
	};

	tracing::trace!(
		op = "overlay.macos_apply_overlay_cursor_for_current_pointer",
		view_key = overlay_view_key,
		entry_count = entries.as_ref().map_or(0, Vec::len),
		local_point = ?local_point,
		overlay_bounds = ?overlay_bounds,
		icon = ?icon,
		"Resolved macOS overlay cursor icon for current pointer."
	);

	macos_set_cursor_icon(icon);
}

fn macos_remove_overlay_cursor_view(overlay_view_key: usize) {
	let overlay_view = overlay_view_key as *mut Object;

	if overlay_view.is_null() {
		return;
	}

	unsafe {
		let superview: *mut Object = objc::msg_send![overlay_view, superview];

		if !superview.is_null() {
			let _: () = objc::msg_send![overlay_view, removeFromSuperview];
		}
	}
}
