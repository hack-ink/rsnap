import CRsnapHostFFI
import CoreGraphics
import Foundation

public enum FrozenOverlayExportColor: UInt32, Equatable, Sendable {
	case white = 0
	case yellow = 1
	case green = 2
	case blue = 3
	case red = 4
	case black = 5

	fileprivate var ffiColor: RsnapFrozenAnnotationColor {
		switch self {
		case .white:
			RSNAP_FROZEN_ANNOTATION_COLOR_WHITE
		case .yellow:
			RSNAP_FROZEN_ANNOTATION_COLOR_YELLOW
		case .green:
			RSNAP_FROZEN_ANNOTATION_COLOR_GREEN
		case .blue:
			RSNAP_FROZEN_ANNOTATION_COLOR_BLUE
		case .red:
			RSNAP_FROZEN_ANNOTATION_COLOR_RED
		case .black:
			RSNAP_FROZEN_ANNOTATION_COLOR_BLACK
		}
	}
}

public struct FrozenOverlayExportStrokeStyle: Equatable, Sendable {
	public var strokeWidthPoints: CGFloat
	public var color: FrozenOverlayExportColor

	public init(strokeWidthPoints: CGFloat, color: FrozenOverlayExportColor) {
		self.strokeWidthPoints = strokeWidthPoints
		self.color = color
	}
}

public struct FrozenOverlayExportSpotlightStyle: Equatable, Sendable {
	public var borderWidthPoints: CGFloat
	public var borderColor: FrozenOverlayExportColor

	public init(borderWidthPoints: CGFloat, borderColor: FrozenOverlayExportColor) {
		self.borderWidthPoints = borderWidthPoints
		self.borderColor = borderColor
	}
}

public struct FrozenOverlayExportTextStyle: Equatable, Sendable {
	public var fontSizePoints: CGFloat
	public var color: FrozenOverlayExportColor

	public init(fontSizePoints: CGFloat, color: FrozenOverlayExportColor) {
		self.fontSizePoints = fontSizePoints
		self.color = color
	}
}

public enum FrozenOverlayExportElement: Equatable, Sendable {
	case pen(points: [CGPoint], style: FrozenOverlayExportStrokeStyle)
	case arrow(start: CGPoint, end: CGPoint, style: FrozenOverlayExportStrokeStyle)
	case mosaic(rect: CGRect)
	case spotlight(rect: CGRect, style: FrozenOverlayExportSpotlightStyle)
	case text(anchor: CGPoint, text: String, style: FrozenOverlayExportTextStyle)
}

public struct FrozenOverlayEditStyle: Equatable {
	public var strokeWidthPoints: CGFloat
	public var strokeColor: FrozenOverlayExportColor
	public var spotlightBorderWidthPoints: CGFloat
	public var spotlightColor: FrozenOverlayExportColor
	public var textFontSizePoints: CGFloat
	public var textColor: FrozenOverlayExportColor

	public init(
		strokeWidthPoints: CGFloat,
		strokeColor: FrozenOverlayExportColor,
		spotlightBorderWidthPoints: CGFloat,
		spotlightColor: FrozenOverlayExportColor,
		textFontSizePoints: CGFloat,
		textColor: FrozenOverlayExportColor
	) {
		self.strokeWidthPoints = strokeWidthPoints
		self.strokeColor = strokeColor
		self.spotlightBorderWidthPoints = spotlightBorderWidthPoints
		self.spotlightColor = spotlightColor
		self.textFontSizePoints = textFontSizePoints
		self.textColor = textColor
	}
}

public struct FrozenOverlayActiveTextEdit: Equatable {
	public var anchor: CGPoint
	public var text: String

	public init(anchor: CGPoint, text: String) {
		self.anchor = anchor
		self.text = text
	}
}

public struct FrozenOverlayEditSnapshot: Equatable {
	public var canUndo: Bool
	public var canRedo: Bool
	public var keepsFrozenSelectionFixed: Bool
	public var isMovingMovableAnnotation: Bool
	public var hasActiveInteraction: Bool
	public var elements: [FrozenOverlayExportElement]
	public var previewPen: FrozenOverlayExportElement?
	public var previewArrow: FrozenOverlayExportElement?
	public var previewMosaic: FrozenOverlayExportElement?
	public var previewSpotlight: FrozenOverlayExportElement?
	public var previewText: FrozenOverlayExportElement?
	public var activeTextEdit: FrozenOverlayActiveTextEdit?

	public init(
		canUndo: Bool,
		canRedo: Bool,
		keepsFrozenSelectionFixed: Bool,
		isMovingMovableAnnotation: Bool,
		hasActiveInteraction: Bool,
		elements: [FrozenOverlayExportElement],
		previewPen: FrozenOverlayExportElement?,
		previewArrow: FrozenOverlayExportElement?,
		previewMosaic: FrozenOverlayExportElement?,
		previewSpotlight: FrozenOverlayExportElement?,
		previewText: FrozenOverlayExportElement?,
		activeTextEdit: FrozenOverlayActiveTextEdit?
	) {
		self.canUndo = canUndo
		self.canRedo = canRedo
		self.keepsFrozenSelectionFixed = keepsFrozenSelectionFixed
		self.isMovingMovableAnnotation = isMovingMovableAnnotation
		self.hasActiveInteraction = hasActiveInteraction
		self.elements = elements
		self.previewPen = previewPen
		self.previewArrow = previewArrow
		self.previewMosaic = previewMosaic
		self.previewSpotlight = previewSpotlight
		self.previewText = previewText
		self.activeTextEdit = activeTextEdit
	}
}

final class FrozenOverlayExportFFIStorage {
	var elements: [RsnapFrozenOverlayExportElement] = []
	private var pointBuffers: [UnsafeMutableBufferPointer<RsnapFloatPoint>] = []
	private var textBuffers: [UnsafeMutableBufferPointer<CChar>] = []

	init(_ elements: [FrozenOverlayExportElement]) {
		self.elements = elements.map { element in
			switch element {
			case .pen(let points, let style):
				return encodePen(points: points, style: style)
			case .arrow(let start, let end, let style):
				return encodeArrow(start: start, end: end, style: style)
			case .mosaic(let rect):
				return encodeMosaic(rect: rect)
			case .spotlight(let rect, let style):
				return encodeSpotlight(rect: rect, style: style)
			case .text(let anchor, let text, let style):
				return encodeText(anchor: anchor, text: text, style: style)
			}
		}
	}

	deinit {
		for buffer in pointBuffers {
			buffer.baseAddress?.deinitialize(count: buffer.count)
			buffer.baseAddress?.deallocate()
		}
		for buffer in textBuffers {
			buffer.baseAddress?.deinitialize(count: buffer.count)
			buffer.baseAddress?.deallocate()
		}
	}

	private func encodePen(
		points: [CGPoint],
		style: FrozenOverlayExportStrokeStyle
	) -> RsnapFrozenOverlayExportElement {
		let buffer = allocatePoints(points)
		return element(
			kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_PEN,
			points: buffer.baseAddress,
			pointsLen: buffer.count,
			strokeWidthPoints: style.strokeWidthPoints,
			color: style.color
		)
	}

	private func encodeArrow(
		start: CGPoint,
		end: CGPoint,
		style: FrozenOverlayExportStrokeStyle
	) -> RsnapFrozenOverlayExportElement {
		element(
			kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_ARROW,
			start: rsnapFloatPoint(from: start),
			end: rsnapFloatPoint(from: end),
			strokeWidthPoints: style.strokeWidthPoints,
			color: style.color
		)
	}

	private func encodeMosaic(rect: CGRect) -> RsnapFrozenOverlayExportElement {
		element(kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_MOSAIC, rect: rsnapFloatRect(from: rect))
	}

	private func encodeSpotlight(
		rect: CGRect,
		style: FrozenOverlayExportSpotlightStyle
	) -> RsnapFrozenOverlayExportElement {
		element(
			kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_SPOTLIGHT,
			rect: rsnapFloatRect(from: rect),
			borderWidthPoints: style.borderWidthPoints,
			color: style.borderColor
		)
	}

	private func encodeText(
		anchor: CGPoint,
		text: String,
		style: FrozenOverlayExportTextStyle
	) -> RsnapFrozenOverlayExportElement {
		let buffer = allocateText(text)
		return element(
			kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_TEXT,
			start: rsnapFloatPoint(from: anchor),
			text: buffer.baseAddress,
			fontSizePoints: style.fontSizePoints,
			color: style.color
		)
	}

	private func allocatePoints(_ points: [CGPoint]) -> UnsafeMutableBufferPointer<RsnapFloatPoint>
	{
		guard points.isEmpty == false else {
			return UnsafeMutableBufferPointer(start: nil, count: 0)
		}
		let encoded = points.map(rsnapFloatPoint(from:))
		let pointer = UnsafeMutablePointer<RsnapFloatPoint>.allocate(capacity: encoded.count)
		pointer.initialize(from: encoded, count: encoded.count)
		let buffer = UnsafeMutableBufferPointer(start: pointer, count: encoded.count)
		pointBuffers.append(buffer)
		return buffer
	}

	private func allocateText(_ text: String) -> UnsafeMutableBufferPointer<CChar> {
		let encoded = Array(text.utf8CString)
		let pointer = UnsafeMutablePointer<CChar>.allocate(capacity: encoded.count)
		pointer.initialize(from: encoded, count: encoded.count)
		let buffer = UnsafeMutableBufferPointer(start: pointer, count: encoded.count)
		textBuffers.append(buffer)
		return buffer
	}

	private func element(
		kind: RsnapFrozenOverlayExportElementKind,
		rect: RsnapFloatRect = RsnapFloatRect(),
		start: RsnapFloatPoint = RsnapFloatPoint(),
		end: RsnapFloatPoint = RsnapFloatPoint(),
		points: UnsafePointer<RsnapFloatPoint>? = nil,
		pointsLen: Int = 0,
		text: UnsafePointer<CChar>? = nil,
		strokeWidthPoints: CGFloat = 0,
		borderWidthPoints: CGFloat = 0,
		fontSizePoints: CGFloat = 0,
		color: FrozenOverlayExportColor = .blue
	) -> RsnapFrozenOverlayExportElement {
		RsnapFrozenOverlayExportElement(
			kind: kind,
			rect: rect,
			start: start,
			end: end,
			points: points,
			points_len: pointsLen,
			text: text,
			stroke_width_points: Double(strokeWidthPoints),
			border_width_points: Double(borderWidthPoints),
			font_size_points: Double(fontSizePoints),
			color: color.ffiColor
		)
	}

}

public final class RsnapFrozenOverlayEditSession {
	private let handle: OpaquePointer

	public init() throws {
		guard let handle = rsnap_frozen_overlay_edit_session_create() else {
			throw HostBridgeError.sessionCreationFailed
		}
		self.handle = handle
	}

	deinit {
		rsnap_frozen_overlay_edit_session_destroy(handle)
	}

	public func reset() throws {
		try rsnapRequireOk(
			rsnap_frozen_overlay_edit_session_reset(handle),
			context: "resetting frozen overlay edit session"
		)
	}

	public func begin(
		tool: ToolbarItemKind,
		at point: CGPoint,
		selection: CGRect,
		style: FrozenOverlayEditStyle
	) throws -> Bool {
		try boolResult { outChanged in
			rsnap_frozen_overlay_edit_session_begin(
				handle,
				tool.ffiKind,
				rsnapFloatPoint(from: point),
				rsnapFloatRect(from: selection),
				Self.encode(style: style),
				outChanged
			)
		}
	}

	public func update(to point: CGPoint, selection: CGRect) throws -> Bool {
		try boolResult { outChanged in
			rsnap_frozen_overlay_edit_session_update(
				handle,
				rsnapFloatPoint(from: point),
				rsnapFloatRect(from: selection),
				outChanged
			)
		}
	}

	public func finish(selection: CGRect) throws -> Bool {
		try boolResult { outChanged in
			rsnap_frozen_overlay_edit_session_finish(
				handle,
				rsnapFloatRect(from: selection),
				outChanged
			)
		}
	}

	public func appendText(_ text: String) throws -> Bool {
		try text.withCString { textPointer in
			try boolResult { outChanged in
				rsnap_frozen_overlay_edit_session_append_text(handle, textPointer, outChanged)
			}
		}
	}

	public func backspaceText() throws -> Bool {
		try boolResult { outChanged in
			rsnap_frozen_overlay_edit_session_backspace_text(handle, outChanged)
		}
	}

	public func commitText(style: FrozenOverlayEditStyle) throws -> Bool {
		try boolResult { outChanged in
			rsnap_frozen_overlay_edit_session_commit_text(
				handle,
				Self.encode(style: style),
				outChanged
			)
		}
	}

	public func cancelText() throws {
		try rsnapRequireOk(
			rsnap_frozen_overlay_edit_session_cancel_text(handle),
			context: "canceling frozen overlay text edit"
		)
	}

	public func undo() throws -> Bool {
		try boolResult { outChanged in
			rsnap_frozen_overlay_edit_session_undo(handle, outChanged)
		}
	}

	public func redo() throws -> Bool {
		try boolResult { outChanged in
			rsnap_frozen_overlay_edit_session_redo(handle, outChanged)
		}
	}

	public func containsMovableAnnotation(at point: CGPoint) throws -> Bool {
		try boolResult { outContains in
			rsnap_frozen_overlay_edit_session_contains_movable_annotation(
				handle,
				rsnapFloatPoint(from: point),
				outContains
			)
		}
	}

	public func snapshot() throws -> FrozenOverlayEditSnapshot {
		var rawSnapshot = RsnapFrozenOverlayEditSnapshot()
		try rsnapRequireOk(
			rsnap_frozen_overlay_edit_session_copy_snapshot(handle, &rawSnapshot),
			context: "copying frozen overlay edit snapshot"
		)
		defer {
			rsnap_frozen_overlay_edit_snapshot_release(&rawSnapshot)
		}

		return FrozenOverlayEditSnapshot(
			canUndo: rawSnapshot.can_undo != 0,
			canRedo: rawSnapshot.can_redo != 0,
			keepsFrozenSelectionFixed: rawSnapshot.keeps_frozen_selection_fixed != 0,
			isMovingMovableAnnotation: rawSnapshot.is_moving_movable_annotation != 0,
			hasActiveInteraction: rawSnapshot.has_active_interaction != 0,
			elements: Self.decodeElements(rawSnapshot.elements, count: rawSnapshot.elements_len),
			previewPen: rawSnapshot.has_preview_pen == 0
				? nil : Self.decode(element: rawSnapshot.preview_pen),
			previewArrow: rawSnapshot.has_preview_arrow == 0
				? nil : Self.decode(element: rawSnapshot.preview_arrow),
			previewMosaic: rawSnapshot.has_preview_mosaic == 0
				? nil : Self.decode(element: rawSnapshot.preview_mosaic),
			previewSpotlight: rawSnapshot.has_preview_spotlight == 0
				? nil : Self.decode(element: rawSnapshot.preview_spotlight),
			previewText: rawSnapshot.has_preview_text == 0
				? nil : Self.decode(element: rawSnapshot.preview_text),
			activeTextEdit: rawSnapshot.has_active_text_edit == 0
				? nil : Self.decode(activeTextEdit: rawSnapshot.active_text_edit)
		)
	}

	private func boolResult(_ body: (UnsafeMutablePointer<UInt8>) -> RsnapStatus) throws -> Bool {
		var changed: UInt8 = 0
		try rsnapRequireOk(body(&changed), context: "running frozen overlay edit operation")
		return changed != 0
	}

	private static func decodeElements(
		_ elements: UnsafeMutablePointer<RsnapFrozenOverlayExportElement>?,
		count: Int
	) -> [FrozenOverlayExportElement] {
		guard let elements, count > 0 else {
			return []
		}
		return UnsafeBufferPointer(start: elements, count: count).compactMap(decode(element:))
	}

	private static func decode(element: RsnapFrozenOverlayExportElement)
		-> FrozenOverlayExportElement?
	{
		switch element.kind {
		case RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_PEN:
			return .pen(
				points: decodePoints(element.points, count: element.points_len),
				style: FrozenOverlayExportStrokeStyle(
					strokeWidthPoints: element.stroke_width_points,
					color: decode(color: element.color)
				)
			)
		case RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_ARROW:
			return .arrow(
				start: cgPoint(from: element.start),
				end: cgPoint(from: element.end),
				style: FrozenOverlayExportStrokeStyle(
					strokeWidthPoints: element.stroke_width_points,
					color: decode(color: element.color)
				)
			)
		case RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_MOSAIC:
			return .mosaic(rect: cgRect(from: element.rect))
		case RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_SPOTLIGHT:
			return .spotlight(
				rect: cgRect(from: element.rect),
				style: FrozenOverlayExportSpotlightStyle(
					borderWidthPoints: element.border_width_points,
					borderColor: decode(color: element.color)
				)
			)
		case RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_TEXT:
			return .text(
				anchor: cgPoint(from: element.start),
				text: decode(text: element.text),
				style: FrozenOverlayExportTextStyle(
					fontSizePoints: element.font_size_points,
					color: decode(color: element.color)
				)
			)
		default:
			return nil
		}
	}

	private static func decode(activeTextEdit element: RsnapFrozenOverlayExportElement)
		-> FrozenOverlayActiveTextEdit
	{
		FrozenOverlayActiveTextEdit(
			anchor: cgPoint(from: element.start),
			text: decode(text: element.text)
		)
	}

	private static func decodePoints(
		_ points: UnsafePointer<RsnapFloatPoint>?,
		count: Int
	) -> [CGPoint] {
		guard let points, count > 0 else {
			return []
		}
		return UnsafeBufferPointer(start: points, count: count).map(cgPoint(from:))
	}

	private static func decode(text: UnsafePointer<CChar>?) -> String {
		guard let text else {
			return ""
		}
		return String(cString: text)
	}

	private static func encode(style: FrozenOverlayEditStyle) -> RsnapFrozenOverlayEditStyle {
		RsnapFrozenOverlayEditStyle(
			stroke_width_points: Double(style.strokeWidthPoints),
			stroke_color: style.strokeColor.ffiColor,
			spotlight_border_width_points: Double(style.spotlightBorderWidthPoints),
			spotlight_color: style.spotlightColor.ffiColor,
			text_font_size_points: Double(style.textFontSizePoints),
			text_color: style.textColor.ffiColor
		)
	}

	private static func decode(color: RsnapFrozenAnnotationColor) -> FrozenOverlayExportColor {
		switch color {
		case RSNAP_FROZEN_ANNOTATION_COLOR_WHITE:
			.white
		case RSNAP_FROZEN_ANNOTATION_COLOR_YELLOW:
			.yellow
		case RSNAP_FROZEN_ANNOTATION_COLOR_GREEN:
			.green
		case RSNAP_FROZEN_ANNOTATION_COLOR_BLUE:
			.blue
		case RSNAP_FROZEN_ANNOTATION_COLOR_RED:
			.red
		case RSNAP_FROZEN_ANNOTATION_COLOR_BLACK:
			.black
		default:
			.blue
		}
	}
}
