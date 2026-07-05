import CRsnapHostFFI
import CoreGraphics
import Foundation

public struct SessionConfiguration: Equatable, Sendable {
	public var allowTextInput: Bool
	public var prefersToolbarAboveSelection: Bool

	public init(
		allowTextInput: Bool = true,
		prefersToolbarAboveSelection: Bool = false
	) {
		self.allowTextInput = allowTextInput
		self.prefersToolbarAboveSelection = prefersToolbarAboveSelection
	}
}

public struct MonitorSnapshot: Equatable, Sendable {
	public var id: UInt32
	public var frame: CGRect
	public var scaleFactorX1000: UInt32

	public init(id: UInt32, frame: CGRect, scaleFactorX1000: UInt32) {
		self.id = id
		self.frame = frame
		self.scaleFactorX1000 = scaleFactorX1000
	}
}

public struct WindowSnapshot: Equatable, Sendable {
	public var windowID: UInt32?
	public var frame: CGRect

	public init(windowID: UInt32?, frame: CGRect) {
		self.windowID = windowID
		self.frame = frame
	}
}

public enum SceneKind: UInt32, Equatable, Sendable {
	case hidden = 0
	case live = 1
	case frozen = 2
}

public enum CursorIntent: UInt32, Equatable, Sendable {
	case `default` = 0
	case crosshair = 1
	case grab = 2
	case grabbing = 3
	case resizeNorth = 4
	case resizeSouth = 5
	case resizeEast = 6
	case resizeWest = 7
	case resizeNorthEast = 8
	case resizeNorthWest = 9
	case resizeSouthEast = 10
	case resizeSouthWest = 11
	case text = 12
}

public enum ToolbarItemKind: UInt32, Equatable, Sendable {
	case pointer = 0
	case pen = 1
	case arrow = 2
	case text = 3
	case mosaic = 4
	case spotlight = 5
	case undo = 6
	case redo = 7
	case autoCenter = 8
	case scroll = 9
	case ocr = 10
	case copy = 11
	case save = 12

	public var isModeTool: Bool {
		switch self {
		case .pointer, .pen, .arrow, .text, .mosaic, .spotlight:
			return true
		case .undo, .redo, .autoCenter, .scroll, .ocr, .copy, .save:
			return false
		}
	}

	var ffiKind: RsnapToolbarItemKind {
		switch self {
		case .pointer:
			RSNAP_TOOLBAR_ITEM_POINTER
		case .pen:
			RSNAP_TOOLBAR_ITEM_PEN
		case .arrow:
			RSNAP_TOOLBAR_ITEM_ARROW
		case .text:
			RSNAP_TOOLBAR_ITEM_TEXT
		case .mosaic:
			RSNAP_TOOLBAR_ITEM_MOSAIC
		case .spotlight:
			RSNAP_TOOLBAR_ITEM_SPOTLIGHT
		case .undo:
			RSNAP_TOOLBAR_ITEM_UNDO
		case .redo:
			RSNAP_TOOLBAR_ITEM_REDO
		case .autoCenter:
			RSNAP_TOOLBAR_ITEM_AUTO_CENTER
		case .scroll:
			RSNAP_TOOLBAR_ITEM_SCROLL
		case .ocr:
			RSNAP_TOOLBAR_ITEM_OCR
		case .copy:
			RSNAP_TOOLBAR_ITEM_COPY
		case .save:
			RSNAP_TOOLBAR_ITEM_SAVE
		}
	}
}

public struct ToolbarItem: Equatable, Sendable {
	public var kind: ToolbarItemKind
	public var enabled: Bool
	public var selected: Bool

	public init(kind: ToolbarItemKind, enabled: Bool, selected: Bool) {
		self.kind = kind
		self.enabled = enabled
		self.selected = selected
	}
}

public struct SceneSnapshot: Equatable, Sendable {
	public var mode: SceneKind
	public var cursorIntent: CursorIntent
	public var pointer: CGPoint?
	public var activeMonitor: MonitorSnapshot?
	public var highlightedWindow: WindowSnapshot?
	public var liveSelectionPreview: CGRect?
	public var frozenSelection: CGRect?
	public var rgb: RGBSample?
	public var loupeVisible: Bool
	public var toolbarItems: [ToolbarItem]
	public var statusMessage: String?

	public init(
		mode: SceneKind,
		cursorIntent: CursorIntent,
		pointer: CGPoint?,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?,
		liveSelectionPreview: CGRect?,
		frozenSelection: CGRect?,
		rgb: RGBSample?,
		loupeVisible: Bool,
		toolbarItems: [ToolbarItem],
		statusMessage: String?
	) {
		self.mode = mode
		self.cursorIntent = cursorIntent
		self.pointer = pointer
		self.activeMonitor = activeMonitor
		self.highlightedWindow = highlightedWindow
		self.liveSelectionPreview = liveSelectionPreview
		self.frozenSelection = frozenSelection
		self.rgb = rgb
		self.loupeVisible = loupeVisible
		self.toolbarItems = toolbarItems
		self.statusMessage = statusMessage
	}
}

public enum HostRequest: Equatable, Sendable {
	case startLiveCapture
	case stopLiveCapture
	case requestFreezeSnapshot(selection: CGRect, selectionEditable: Bool)
	case startScrollCapture
	case copyCapture
	case saveCapture
	case recognizeText
	case requestScreenRecordingPermission
}

public enum HostEffectKind: UInt32, Equatable, Sendable {
	case copyCapture = 0
	case saveCapture = 1
	case recognizeText = 2
}

public enum PermissionKind: UInt32, Equatable, Sendable {
	case screenRecording = 0
}

public enum HostEvent: Sendable {
	case sessionActivated
	case pointerMoved(
		point: CGPoint,
		rgb: RGBSample?,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?
	)
	case primaryInteractionStarted(
		point: CGPoint,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?
	)
	case primaryInteractionUpdated(
		point: CGPoint,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?
	)
	case primaryInteractionCompleted(
		point: CGPoint,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?
	)
	case cancelRequested
	case copyRequested
	case saveRequested
	case recognizeTextRequested
	case toggleLoupe
	case toolbarItemInvoked(ToolbarItemKind)
}

public enum HostReport: Sendable {
	case freezeSnapshotCommitted(selection: CGRect)
	case hostEffectCompleted(HostEffectKind)
	case permissionChanged(PermissionKind, granted: Bool)
	case statusMessage(String)
}

public enum HostBridgeError: Error, CustomStringConvertible {
	case abiVersionMismatch(expected: UInt32, actual: UInt32)
	case sessionCreationFailed
	case ffiStatus(context: String, code: UInt32)
	case invalidSceneKind(UInt32)
	case invalidCursorIntent(UInt32)
	case invalidRequestKind(UInt32)

	public var description: String {
		switch self {
		case .abiVersionMismatch(let expected, let actual):
			return "ABI mismatch: expected \(expected), got \(actual)"
		case .sessionCreationFailed:
			return "Failed to create rsnap host session."
		case .ffiStatus(let context, let code):
			return "FFI status \(code) while \(context)"
		case .invalidSceneKind(let rawValue):
			return "Unknown scene kind \(rawValue)"
		case .invalidCursorIntent(let rawValue):
			return "Unknown cursor intent \(rawValue)"
		case .invalidRequestKind(let rawValue):
			return "Unknown host request kind \(rawValue)"
		}
	}
}

public final class RsnapHostSession {
	private let handle: OpaquePointer
	public let configuration: SessionConfiguration

	public init(configuration: SessionConfiguration = .init()) throws {
		let actualAbi = rsnap_host_ffi_abi_version()
		if actualAbi != RSNAP_HOST_FFI_ABI_VERSION {
			throw HostBridgeError.abiVersionMismatch(
				expected: RSNAP_HOST_FFI_ABI_VERSION,
				actual: actualAbi
			)
		}

		let config = RsnapSessionConfig(
			platform: RSNAP_PLATFORM_MACOS,
			allow_text_input: configuration.allowTextInput ? 1 : 0,
			prefers_toolbar_above_selection: configuration.prefersToolbarAboveSelection ? 1 : 0
		)
		guard let handle = rsnap_session_create(config) else {
			throw HostBridgeError.sessionCreationFailed
		}

		self.handle = handle
		self.configuration = configuration
	}

	deinit {
		rsnap_session_destroy(handle)
	}

	public func enterLive() throws {
		try rsnapRequireOk(
			rsnap_session_enter_live(handle),
			context: "entering live mode"
		)
	}

	public func send(event: HostEvent) throws {
		try rsnapRequireOk(
			rsnap_session_handle_host_event(handle, encode(event: event)),
			context: "sending host event"
		)
	}

	public func send(report: HostReport) throws {
		try rsnapRequireOk(
			rsnap_session_handle_host_report(handle, encode(report: report)),
			context: "sending host report"
		)
	}

	public func currentScene() throws -> SceneSnapshot {
		var outScene = RsnapSceneModel()
		try rsnapRequireOk(
			rsnap_session_copy_scene_model(handle, &outScene),
			context: "copying scene model"
		)

		return try decode(scene: outScene)
	}

	public func takeNextRequest() throws -> HostRequest? {
		var outRequest = RsnapHostRequestValue()
		let status = rsnap_session_take_next_request(handle, &outRequest)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		try rsnapRequireOk(status, context: "draining queued host request")

		return try decode(request: outRequest)
	}

	public func drainRequests() throws -> [HostRequest] {
		var requests: [HostRequest] = []
		while let request = try takeNextRequest() {
			requests.append(request)
		}
		return requests
	}

	private func encode(event: HostEvent) -> RsnapHostEvent {
		switch event {
		case .sessionActivated:
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_SESSION_ACTIVATED.rawValue,
				point: RsnapPoint(),
				has_point: 0,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: RsnapMonitorRect(),
				has_active_monitor: 0,
				highlighted_window: RsnapWindowRect(),
				has_highlighted_window: 0,
				toolbar_item_kind: 0
			)
		case .pointerMoved(let point, let rgb, let activeMonitor, let highlightedWindow):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_POINTER_MOVED.rawValue,
				point: encode(point: point),
				has_point: 1,
				rgb: encode(rgb: rgb),
				has_rgb: rgb == nil ? 0 : 1,
				active_monitor: encode(monitor: activeMonitor),
				has_active_monitor: activeMonitor == nil ? 0 : 1,
				highlighted_window: encode(window: highlightedWindow),
				has_highlighted_window: highlightedWindow == nil ? 0 : 1,
				toolbar_item_kind: 0
			)
		case .primaryInteractionStarted(let point, let activeMonitor, let highlightedWindow):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_PRIMARY_INTERACTION_STARTED.rawValue,
				point: encode(point: point),
				has_point: 1,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: encode(monitor: activeMonitor),
				has_active_monitor: activeMonitor == nil ? 0 : 1,
				highlighted_window: encode(window: highlightedWindow),
				has_highlighted_window: highlightedWindow == nil ? 0 : 1,
				toolbar_item_kind: 0
			)
		case .primaryInteractionUpdated(let point, let activeMonitor, let highlightedWindow):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_PRIMARY_INTERACTION_UPDATED.rawValue,
				point: encode(point: point),
				has_point: 1,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: encode(monitor: activeMonitor),
				has_active_monitor: activeMonitor == nil ? 0 : 1,
				highlighted_window: encode(window: highlightedWindow),
				has_highlighted_window: highlightedWindow == nil ? 0 : 1,
				toolbar_item_kind: 0
			)
		case .primaryInteractionCompleted(let point, let activeMonitor, let highlightedWindow):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_PRIMARY_INTERACTION_COMPLETED.rawValue,
				point: encode(point: point),
				has_point: 1,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: encode(monitor: activeMonitor),
				has_active_monitor: activeMonitor == nil ? 0 : 1,
				highlighted_window: encode(window: highlightedWindow),
				has_highlighted_window: highlightedWindow == nil ? 0 : 1,
				toolbar_item_kind: 0
			)
		case .cancelRequested:
			return eventWith(kind: RSNAP_HOST_EVENT_CANCEL_REQUESTED.rawValue)
		case .copyRequested:
			return eventWith(kind: RSNAP_HOST_EVENT_COPY_REQUESTED.rawValue)
		case .saveRequested:
			return eventWith(kind: RSNAP_HOST_EVENT_SAVE_REQUESTED.rawValue)
		case .recognizeTextRequested:
			return eventWith(kind: RSNAP_HOST_EVENT_RECOGNIZE_TEXT_REQUESTED.rawValue)
		case .toggleLoupe:
			return eventWith(kind: RSNAP_HOST_EVENT_TOGGLE_LOUPE.rawValue)
		case .toolbarItemInvoked(let item):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_TOOLBAR_ITEM_INVOKED.rawValue,
				point: RsnapPoint(),
				has_point: 0,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: RsnapMonitorRect(),
				has_active_monitor: 0,
				highlighted_window: RsnapWindowRect(),
				has_highlighted_window: 0,
				toolbar_item_kind: item.rawValue
			)
		}
	}

	private func encode(report: HostReport) -> RsnapHostReport {
		var reportValue = RsnapHostReport()

		switch report {
		case .freezeSnapshotCommitted(let selection):
			reportValue.kind = RSNAP_HOST_REPORT_FREEZE_SNAPSHOT_COMMITTED.rawValue
			reportValue.selection = encode(rect: selection)
			reportValue.has_selection = 1
		case .hostEffectCompleted(let effect):
			reportValue.kind = RSNAP_HOST_REPORT_HOST_EFFECT_COMPLETED.rawValue
			reportValue.effect_kind = effect.rawValue
		case .permissionChanged(let permission, let granted):
			reportValue.kind = RSNAP_HOST_REPORT_PERMISSION_CHANGED.rawValue
			reportValue.permission_kind = permission.rawValue
			reportValue.granted = granted ? 1 : 0
		case .statusMessage(let message):
			reportValue.kind = RSNAP_HOST_REPORT_STATUS_MESSAGE.rawValue
			encodeStatusMessage(message, into: &reportValue)
		}

		return reportValue
	}

	private func decode(scene: RsnapSceneModel) throws -> SceneSnapshot {
		guard let mode = SceneKind(rawValue: scene.scene_kind) else {
			throw HostBridgeError.invalidSceneKind(scene.scene_kind)
		}
		guard let cursorIntent = CursorIntent(rawValue: scene.cursor_intent) else {
			throw HostBridgeError.invalidCursorIntent(scene.cursor_intent)
		}

		return SceneSnapshot(
			mode: mode,
			cursorIntent: cursorIntent,
			pointer: scene.has_pointer == 0 ? nil : decode(point: scene.pointer),
			activeMonitor: scene.has_active_monitor == 0
				? nil : decode(monitor: scene.active_monitor),
			highlightedWindow: scene.has_highlighted_window == 0
				? nil : decode(window: scene.highlighted_window),
			liveSelectionPreview: scene.has_live_selection_preview == 0
				? nil : decode(rect: scene.live_selection_preview),
			frozenSelection: scene.has_frozen_selection == 0
				? nil : decode(rect: scene.frozen_selection),
			rgb: scene.has_rgb == 0 ? nil : decode(rgb: scene.rgb),
			loupeVisible: scene.loupe_visible != 0,
			toolbarItems: decodeToolbarItems(scene),
			statusMessage: decodeStatusMessage(scene)
		)
	}

	private func eventWith(kind: UInt32) -> RsnapHostEvent {
		RsnapHostEvent(
			kind: kind,
			point: RsnapPoint(),
			has_point: 0,
			rgb: RsnapRgb(),
			has_rgb: 0,
			active_monitor: RsnapMonitorRect(),
			has_active_monitor: 0,
			highlighted_window: RsnapWindowRect(),
			has_highlighted_window: 0,
			toolbar_item_kind: 0
		)
	}

	private func decodeToolbarItems(_ scene: RsnapSceneModel) -> [ToolbarItem] {
		let count = min(Int(scene.toolbar_item_count), Int(RSNAP_TOOLBAR_ITEM_CAPACITY))
		return withUnsafeBytes(of: scene.toolbar_items) { rawBuffer in
			let buffer = rawBuffer.bindMemory(to: RsnapToolbarItem.self)
			return buffer.prefix(count).compactMap { item in
				guard item.present != 0, let kind = ToolbarItemKind(rawValue: item.kind) else {
					return nil
				}
				return ToolbarItem(
					kind: kind, enabled: item.enabled != 0, selected: item.selected != 0)
			}
		}
	}

	private func decodeStatusMessage(_ scene: RsnapSceneModel) -> String? {
		let count = min(Int(scene.status_message_len), Int(RSNAP_STATUS_MESSAGE_CAPACITY))
		guard count > 0 else {
			return nil
		}
		return withUnsafeBytes(of: scene.status_message) { rawBuffer in
			String(bytes: rawBuffer.prefix(count), encoding: .utf8)
		}
	}

	private func decode(request: RsnapHostRequestValue) throws -> HostRequest {
		switch request.kind {
		case RSNAP_HOST_REQUEST_START_LIVE_CAPTURE.rawValue:
			return .startLiveCapture
		case RSNAP_HOST_REQUEST_STOP_LIVE_CAPTURE.rawValue:
			return .stopLiveCapture
		case RSNAP_HOST_REQUEST_REQUEST_FREEZE_SNAPSHOT.rawValue:
			guard request.has_selection != 0 else {
				throw HostBridgeError.invalidRequestKind(request.kind)
			}
			return .requestFreezeSnapshot(
				selection: decode(rect: request.selection),
				selectionEditable: request.selection_editable != 0
			)
		case RSNAP_HOST_REQUEST_COPY_CAPTURE.rawValue:
			return .copyCapture
		case RSNAP_HOST_REQUEST_SAVE_CAPTURE.rawValue:
			return .saveCapture
		case RSNAP_HOST_REQUEST_RECOGNIZE_TEXT.rawValue:
			return .recognizeText
		case RSNAP_HOST_REQUEST_REQUEST_SCREEN_RECORDING_PERMISSION.rawValue:
			return .requestScreenRecordingPermission
		case RSNAP_HOST_REQUEST_START_SCROLL_CAPTURE.rawValue:
			return .startScrollCapture
		default:
			throw HostBridgeError.invalidRequestKind(request.kind)
		}
	}

	private func encodeStatusMessage(_ message: String, into report: inout RsnapHostReport) {
		let data = Array(message.utf8.prefix(Int(RSNAP_STATUS_MESSAGE_CAPACITY)))
		report.status_message_len = UInt32(data.count)
		withUnsafeMutableBytes(of: &report.status_message) { rawBuffer in
			rawBuffer.initializeMemory(as: UInt8.self, repeating: 0)
			rawBuffer.prefix(data.count).copyBytes(from: data)
		}
	}

	private func encode(point: CGPoint) -> RsnapPoint {
		RsnapPoint(x: Int32(point.x.rounded()), y: Int32(point.y.rounded()))
	}

	private func decode(point: RsnapPoint) -> CGPoint {
		CGPoint(x: Int(point.x), y: Int(point.y))
	}

	private func encode(rgb: RGBSample?) -> RsnapRgb {
		guard let rgb else {
			return RsnapRgb()
		}
		return RsnapRgb(r: rgb.r, g: rgb.g, b: rgb.b)
	}

	private func decode(rgb: RsnapRgb) -> RGBSample {
		RGBSample(r: rgb.r, g: rgb.g, b: rgb.b)
	}

	private func encode(rect: CGRect) -> RsnapRect {
		RsnapRect(
			x: Int32(rect.origin.x.rounded()),
			y: Int32(rect.origin.y.rounded()),
			width: UInt32(max(rect.width.rounded(), 0)),
			height: UInt32(max(rect.height.rounded(), 0))
		)
	}

	private func decode(rect: RsnapRect) -> CGRect {
		CGRect(
			x: Int(rect.x),
			y: Int(rect.y),
			width: Int(rect.width),
			height: Int(rect.height)
		)
	}

	private func encode(monitor: MonitorSnapshot?) -> RsnapMonitorRect {
		guard let monitor else {
			return RsnapMonitorRect()
		}
		return RsnapMonitorRect(
			id: monitor.id,
			origin: encode(point: monitor.frame.origin),
			width: UInt32(max(monitor.frame.width.rounded(), 0)),
			height: UInt32(max(monitor.frame.height.rounded(), 0)),
			scale_factor_x1000: monitor.scaleFactorX1000
		)
	}

	private func decode(monitor: RsnapMonitorRect) -> MonitorSnapshot {
		MonitorSnapshot(
			id: monitor.id,
			frame: CGRect(
				x: Int(monitor.origin.x),
				y: Int(monitor.origin.y),
				width: Int(monitor.width),
				height: Int(monitor.height)
			),
			scaleFactorX1000: monitor.scale_factor_x1000
		)
	}

	private func encode(window: WindowSnapshot?) -> RsnapWindowRect {
		guard let window else {
			return RsnapWindowRect()
		}
		return RsnapWindowRect(
			window_id: window.windowID ?? 0,
			has_window_id: window.windowID == nil ? 0 : 1,
			x: Int64(window.frame.origin.x.rounded()),
			y: Int64(window.frame.origin.y.rounded()),
			width: Int64(window.frame.width.rounded()),
			height: Int64(window.frame.height.rounded())
		)
	}

	private func decode(window: RsnapWindowRect) -> WindowSnapshot {
		WindowSnapshot(
			windowID: window.has_window_id == 0 ? nil : window.window_id,
			frame: CGRect(
				x: Int(window.x),
				y: Int(window.y),
				width: Int(window.width),
				height: Int(window.height)
			)
		)
	}
}
