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

public struct RGBSample: Equatable, Sendable {
	public var r: UInt8
	public var g: UInt8
	public var b: UInt8

	public init(r: UInt8, g: UInt8, b: UInt8) {
		self.r = r
		self.g = g
		self.b = b
	}
}

public struct LiveSampleSnapshot: Equatable, Sendable {
	public var rgb: RGBSample?
	public var capturedAtUptime: TimeInterval?
	public var frameAgeMicroseconds: UInt64?
	public var frameSequence: UInt64?
	public var streamGeneration: UInt64?
	public var patchWidth: Int
	public var patchHeight: Int
	public var patchRGBA: Data?

	public init(
		rgb: RGBSample?,
		capturedAtUptime: TimeInterval? = nil,
		frameAgeMicroseconds: UInt64? = nil,
		frameSequence: UInt64? = nil,
		streamGeneration: UInt64? = nil,
		patchWidth: Int = 0,
		patchHeight: Int = 0,
		patchRGBA: Data? = nil
	) {
		self.rgb = rgb
		self.capturedAtUptime = capturedAtUptime
		self.frameAgeMicroseconds = frameAgeMicroseconds
		self.frameSequence = frameSequence
		self.streamGeneration = streamGeneration
		self.patchWidth = patchWidth
		self.patchHeight = patchHeight
		self.patchRGBA = patchRGBA
	}
}

public struct RGBARegionSnapshot: Equatable, Sendable {
	public var width: Int
	public var height: Int
	public var rgba: Data

	public init(width: Int, height: Int, rgba: Data) {
		self.width = width
		self.height = height
		self.rgba = rgba
	}
}

public enum RsnapExportEncoder {
	public static func pngData(from image: RGBARegionSnapshot) throws -> Data {
		var outPNG = RsnapOwnedBytes()
		let status = image.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_export_rgba_to_png(
				UInt32(max(image.width, 0)),
				UInt32(max(image.height, 0)),
				baseAddress,
				image.rgba.count,
				&outPNG
			)
		}
		try requireOk(status, context: "encoding export PNG")

		return try data(from: outPNG, context: "taking encoded export PNG")
	}

	public static func pngData(from image: RGBARegionSnapshot, crop: CGRect) throws -> Data {
		let cropRect = try encode(crop: crop)
		var outPNG = RsnapOwnedBytes()
		let status = image.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_export_rgba_crop_to_png(
				UInt32(max(image.width, 0)),
				UInt32(max(image.height, 0)),
				baseAddress,
				image.rgba.count,
				cropRect,
				&outPNG
			)
		}
		try requireOk(status, context: "encoding cropped export PNG")

		return try data(from: outPNG, context: "taking encoded cropped export PNG")
	}

	private static func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private static func encode(crop: CGRect) throws -> RsnapPixelRect {
		let x = crop.origin.x.rounded()
		let y = crop.origin.y.rounded()
		let width = crop.width.rounded()
		let height = crop.height.rounded()
		let maxValue = CGFloat(UInt32.max)

		guard
			x >= 0,
			y >= 0,
			width >= 0,
			height >= 0,
			x <= maxValue,
			y <= maxValue,
			width <= maxValue,
			height <= maxValue
		else {
			throw HostBridgeError.ffiStatus(
				context: "encoding export crop rectangle",
				code: RSNAP_STATUS_INVALID_INPUT.rawValue)
		}

		return RsnapPixelRect(
			x: UInt32(x),
			y: UInt32(y),
			width: UInt32(width),
			height: UInt32(height)
		)
	}

	private static func data(from outPNG: RsnapOwnedBytes, context: String) throws -> Data {
		guard outPNG.len > 0, let bytes = outPNG.bytes else {
			throw HostBridgeError.ffiStatus(context: context, code: RSNAP_STATUS_EMPTY.rawValue)
		}

		let ownedBytes = UnsafeMutablePointer<RsnapOwnedBytes>.allocate(capacity: 1)
		ownedBytes.initialize(to: outPNG)
		return Data(
			bytesNoCopy: bytes,
			count: outPNG.len,
			deallocator: .custom { _, _ in
				rsnap_owned_bytes_release(ownedBytes)
				ownedBytes.deinitialize(count: 1)
				ownedBytes.deallocate()
			}
		)
	}
}

public enum ScrollObserveOutcome: UInt32, Equatable, Sendable {
	case noChange = 0
	case previewUpdated = 1
	case committed = 2
	case unsupportedDirection = 3
}

public struct ScrollObserveResult: Equatable, Sendable {
	public var outcome: ScrollObserveOutcome
	public var growthRows: Int
	public var exportWidth: Int
	public var exportHeight: Int
	public var currentViewportTopY: Int

	public init(
		outcome: ScrollObserveOutcome,
		growthRows: Int,
		exportWidth: Int,
		exportHeight: Int,
		currentViewportTopY: Int
	) {
		self.outcome = outcome
		self.growthRows = growthRows
		self.exportWidth = exportWidth
		self.exportHeight = exportHeight
		self.currentViewportTopY = currentViewportTopY
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
		try requireOk(
			rsnap_session_enter_live(handle),
			context: "entering live mode"
		)
	}

	public func send(event: HostEvent) throws {
		try requireOk(
			rsnap_session_handle_host_event(handle, encode(event: event)),
			context: "sending host event"
		)
	}

	public func send(report: HostReport) throws {
		try requireOk(
			rsnap_session_handle_host_report(handle, encode(report: report)),
			context: "sending host report"
		)
	}

	public func currentScene() throws -> SceneSnapshot {
		var outScene = RsnapSceneModel()
		try requireOk(
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
		try requireOk(status, context: "draining queued host request")

		return try decode(request: outRequest)
	}

	public func drainRequests() throws -> [HostRequest] {
		var requests: [HostRequest] = []
		while let request = try takeNextRequest() {
			requests.append(request)
		}
		return requests
	}

	private func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
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

public final class RsnapScrollCaptureSession: @unchecked Sendable {
	private let handle: OpaquePointer
	private let stateLock = NSLock()

	public init(baseImage: RGBARegionSnapshot, previewWidthPixels: Int) throws {
		let actualAbi = rsnap_host_ffi_abi_version()
		if actualAbi != RSNAP_HOST_FFI_ABI_VERSION {
			throw HostBridgeError.abiVersionMismatch(
				expected: RSNAP_HOST_FFI_ABI_VERSION,
				actual: actualAbi
			)
		}

		let width = UInt32(max(baseImage.width, 0))
		let height = UInt32(max(baseImage.height, 0))
		let previewWidth = UInt32(max(previewWidthPixels, 1))
		let maybeHandle = baseImage.rgba.withUnsafeBytes { buffer -> OpaquePointer? in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return nil
			}
			return rsnap_scroll_session_create(
				width,
				height,
				baseAddress,
				baseImage.rgba.count,
				previewWidth
			)
		}
		guard let handle = maybeHandle else {
			throw HostBridgeError.sessionCreationFailed
		}
		self.handle = handle
	}

	deinit {
		rsnap_scroll_session_destroy(handle)
	}

	public func observeDownwardFrame(_ frame: RGBARegionSnapshot) throws -> ScrollObserveResult {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outResult = RsnapScrollObserveResult()
		let status = frame.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_scroll_session_observe_downward_frame(
				handle,
				UInt32(max(frame.width, 0)),
				UInt32(max(frame.height, 0)),
				baseAddress,
				frame.rgba.count,
				&outResult
			)
		}
		try requireOk(status, context: "observing scroll-capture frame")

		return try decode(result: outResult)
	}

	public func exportImage() throws -> RGBARegionSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outRegion = RsnapOwnedRgbaRegion()
		let status = rsnap_scroll_session_take_export_rgba(handle, &outRegion)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		if code != 0 {
			throw HostBridgeError.ffiStatus(
				context: "taking scroll-capture export RGBA", code: code)
		}
		guard outRegion.len > 0, let rgba = outRegion.rgba else {
			return nil
		}
		let ownedRegion = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		ownedRegion.initialize(to: outRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: outRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(ownedRegion)
				ownedRegion.deinitialize(count: 1)
				ownedRegion.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(outRegion.width),
			height: Int(outRegion.height),
			rgba: data
		)
	}

	private func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private func decode(result: RsnapScrollObserveResult) throws -> ScrollObserveResult {
		guard let outcome = ScrollObserveOutcome(rawValue: result.kind) else {
			throw HostBridgeError.ffiStatus(
				context: "decoding scroll observation", code: result.kind)
		}
		return ScrollObserveResult(
			outcome: outcome,
			growthRows: Int(result.growth_rows),
			exportWidth: Int(result.export_width),
			exportHeight: Int(result.export_height),
			currentViewportTopY: Int(result.current_viewport_top_y)
		)
	}
}

public final class RsnapLiveSampler: @unchecked Sendable {
	private let handle: OpaquePointer
	private let stateLock = NSLock()

	public init(selfCaptureExceptionWindowIDs: [UInt32] = []) throws {
		let actualAbi = rsnap_host_ffi_abi_version()
		if actualAbi != RSNAP_HOST_FFI_ABI_VERSION {
			throw HostBridgeError.abiVersionMismatch(
				expected: RSNAP_HOST_FFI_ABI_VERSION,
				actual: actualAbi
			)
		}
		let handle: OpaquePointer?
		if selfCaptureExceptionWindowIDs.isEmpty {
			handle = rsnap_live_sampler_create()
		} else {
			handle = selfCaptureExceptionWindowIDs.withUnsafeBufferPointer { buffer in
				rsnap_live_sampler_create_with_self_capture_exception_window_ids(
					buffer.baseAddress,
					buffer.count
				)
			}
		}
		guard let handle else {
			throw HostBridgeError.sessionCreationFailed
		}
		self.handle = handle
	}

	deinit {
		rsnap_live_sampler_destroy(handle)
	}

	public func sampleCursor(
		monitor: MonitorSnapshot,
		point: CGPoint,
		patchSidePixels: Int
	) throws -> LiveSampleSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outSample = RsnapLiveSample()
		let status = rsnap_live_sampler_sample_cursor(
			handle,
			RsnapMonitorRect(
				id: monitor.id,
				origin: RsnapPoint(
					x: Int32(monitor.frame.origin.x.rounded()),
					y: Int32(monitor.frame.origin.y.rounded())
				),
				width: UInt32(max(monitor.frame.width.rounded(), 0)),
				height: UInt32(max(monitor.frame.height.rounded(), 0)),
				scale_factor_x1000: monitor.scaleFactorX1000
			),
			RsnapPoint(x: Int32(point.x.rounded()), y: Int32(point.y.rounded())),
			UInt32(max(patchSidePixels, 0)),
			UInt32(max(patchSidePixels, 0)),
			&outSample
		)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: "sampling live cursor", code: code)
		}

		let patchData: Data? = withUnsafeBytes(of: outSample.patch_rgba) { rawBuffer in
			let count = min(Int(outSample.patch_len), rawBuffer.count)
			guard count > 0 else {
				return nil
			}
			return Data(rawBuffer.prefix(count))
		}

		let frameAgeMicroseconds =
			outSample.has_frame_metadata == 0 ? nil : UInt64(outSample.frame_age_micros)
		let capturedAtUptime = frameAgeMicroseconds.map {
			ProcessInfo.processInfo.systemUptime - (Double($0) / 1_000_000.0)
		}

		return LiveSampleSnapshot(
			rgb: outSample.has_rgb == 0
				? nil : RGBSample(r: outSample.rgb.r, g: outSample.rgb.g, b: outSample.rgb.b),
			capturedAtUptime: capturedAtUptime,
			frameAgeMicroseconds: frameAgeMicroseconds,
			frameSequence: outSample.has_frame_metadata == 0
				? nil : UInt64(outSample.frame_seq),
			streamGeneration: outSample.has_frame_metadata == 0
				? nil : UInt64(outSample.stream_generation),
			patchWidth: Int(outSample.patch_width),
			patchHeight: Int(outSample.patch_height),
			patchRGBA: patchData
		)
	}

	public func primeMonitor(_ monitor: MonitorSnapshot) throws {
		stateLock.lock()
		defer { stateLock.unlock() }

		let status = rsnap_live_sampler_prime_monitor(
			handle,
			RsnapMonitorRect(
				id: monitor.id,
				origin: RsnapPoint(
					x: Int32(monitor.frame.origin.x.rounded()),
					y: Int32(monitor.frame.origin.y.rounded())
				),
				width: UInt32(max(monitor.frame.width.rounded(), 0)),
				height: UInt32(max(monitor.frame.height.rounded(), 0)),
				scale_factor_x1000: monitor.scaleFactorX1000
			)
		)
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: "priming live monitor", code: code)
		}
	}

	public func reset() throws {
		stateLock.lock()
		defer { stateLock.unlock() }

		let status = rsnap_live_sampler_reset(handle)
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: "resetting live monitor sampler", code: code)
		}
	}

	public func peekRegion(
		monitor: MonitorSnapshot,
		rect: CGRect
	) throws -> RGBARegionSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		let encodedMonitor = RsnapMonitorRect(
			id: monitor.id,
			origin: RsnapPoint(
				x: Int32(monitor.frame.origin.x.rounded()),
				y: Int32(monitor.frame.origin.y.rounded())
			),
			width: UInt32(max(monitor.frame.width.rounded(), 0)),
			height: UInt32(max(monitor.frame.height.rounded(), 0)),
			scale_factor_x1000: monitor.scaleFactorX1000
		)
		let encodedRect = RsnapRect(
			x: Int32(rect.origin.x.rounded()),
			y: Int32(rect.origin.y.rounded()),
			width: UInt32(max(rect.width.rounded(), 0)),
			height: UInt32(max(rect.height.rounded(), 0))
		)
		var ownedRegion = RsnapOwnedRgbaRegion()
		let takeStatus = rsnap_live_sampler_take_region_rgba(
			handle,
			encodedMonitor,
			encodedRect,
			&ownedRegion
		)
		let takeCode = rsnap_status_code(takeStatus)
		if takeCode == 3 {
			return nil
		}
		if takeCode != 0 {
			throw HostBridgeError.ffiStatus(context: "taking live RGBA region", code: takeCode)
		}
		guard ownedRegion.len > 0, let rgba = ownedRegion.rgba else {
			return nil
		}
		let regionHandle = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		regionHandle.initialize(to: ownedRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: ownedRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(regionHandle)
				regionHandle.deinitialize(count: 1)
				regionHandle.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(ownedRegion.width),
			height: Int(ownedRegion.height),
			rgba: data
		)
	}

	/// Returns the live sampler's cache-only full-monitor snapshot.
	///
	/// This API does not expose the original frame capture time or stream sequence. Do not use it
	/// as a frozen screenshot source unless the FFI contract is extended to prove freshness.
	public func peekLatestMonitorImage(
		monitor: MonitorSnapshot
	) throws -> RGBARegionSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outRegion = RsnapOwnedRgbaRegion()
		let encodedMonitor = RsnapMonitorRect(
			id: monitor.id,
			origin: RsnapPoint(
				x: Int32(monitor.frame.origin.x.rounded()),
				y: Int32(monitor.frame.origin.y.rounded())
			),
			width: UInt32(max(monitor.frame.width.rounded(), 0)),
			height: UInt32(max(monitor.frame.height.rounded(), 0)),
			scale_factor_x1000: monitor.scaleFactorX1000
		)
		let status = rsnap_live_sampler_take_latest_monitor_rgba(
			handle,
			encodedMonitor,
			&outRegion
		)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		if code != 0 {
			throw HostBridgeError.ffiStatus(
				context: "peeking latest monitor RGBA snapshot", code: code)
		}
		guard outRegion.len > 0, let rgba = outRegion.rgba else {
			return nil
		}
		let ownedRegion = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		ownedRegion.initialize(to: outRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: outRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(ownedRegion)
				ownedRegion.deinitialize(count: 1)
				ownedRegion.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(outRegion.width),
			height: Int(outRegion.height),
			rgba: data
		)
	}
}
