import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

struct FrozenBrushStroke: Equatable {
	var points: [CGPoint]
	var style: FrozenBrushStyle
}

struct FrozenArrowAnnotation: Equatable {
	var start: CGPoint
	var end: CGPoint
	var style: FrozenBrushStyle
}

struct FrozenSpotlightAnnotation: Equatable {
	var rect: CGRect
	var style: FrozenSpotlightStyle
}

struct FrozenTextAnnotation: Equatable {
	var anchor: CGPoint
	var text: String
	var style: FrozenTextStyle
}

struct FrozenTextEditState {
	var anchor: CGPoint
	var text: String
}

func drawFrozenSpotlightBorder(
	for rect: CGRect,
	style: FrozenSpotlightStyle,
	scale: CGFloat,
	alpha: CGFloat,
	in context: CGContext
) {
	let lineWidth = style.borderWidthPoints * scale
	guard lineWidth > .ulpOfOne else {
		return
	}
	context.saveGState()
	context.setStrokeColor(style.borderColor.nsColor(alpha: alpha).cgColor)
	context.setLineWidth(lineWidth)
	context.stroke(rect.insetBy(dx: lineWidth / 2, dy: lineWidth / 2))
	context.restoreGState()
}

func drawFrozenArrow(
	from start: CGPoint,
	to end: CGPoint,
	style: FrozenBrushStyle,
	scale: CGFloat,
	in context: CGContext
) {
	let distance = hypot(end.x - start.x, end.y - start.y)
	guard distance > .ulpOfOne else {
		return
	}
	let strokeWidth = style.strokeWidthPoints * 1.4 * scale
	let headLength = min(max(strokeWidth * 4.2, 16 * scale), distance * 0.9)
	let headSpread: CGFloat = .pi / 7
	let angle = atan2(end.y - start.y, end.x - start.x)
	let direction = CGPoint(x: cos(angle), y: sin(angle))
	let shaftEnd = CGPoint(
		x: end.x - direction.x * headLength * 0.72,
		y: end.y - direction.y * headLength * 0.72
	)
	let left = CGPoint(
		x: end.x - cos(angle - headSpread) * headLength,
		y: end.y - sin(angle - headSpread) * headLength
	)
	let right = CGPoint(
		x: end.x - cos(angle + headSpread) * headLength,
		y: end.y - sin(angle + headSpread) * headLength
	)

	context.saveGState()
	context.setStrokeColor(style.color.nsColor(alpha: 0.96).cgColor)
	context.setLineWidth(strokeWidth)
	context.setLineCap(.round)
	context.setLineJoin(.round)
	context.beginPath()
	context.move(to: start)
	context.addLine(to: shaftEnd)
	context.strokePath()
	context.beginPath()
	context.move(to: end)
	context.addLine(to: left)
	context.move(to: end)
	context.addLine(to: right)
	context.strokePath()
	context.restoreGState()
}

struct FrozenSelectionInteractionState {
	let kind: FrozenSelectionTransformKind
	let initialPointer: CGPoint
	let initialSelection: CGRect
	let monitorFrame: CGRect
}

struct CaptureChromeState {
	var loupePatch: CGImage?
	var rgbSample: RGBSample?
	var hostLocalFrozenSelecting = false
	var frozenSelectionSnapshot: CGRect?
	var frozenSelectionEditable = false
	var frozenSelectionInteraction: FrozenSelectionInteractionState?
	var frozenDisplayFrame: CGRect?
	var frozenDisplayImage: CGImage?
	var frozenBaseImage: CGImage?
	var captureFrameSource: CaptureFrameSource = .unknown
	var captureFrameWindowID: CGWindowID?
	var scrollMinimapPreview: ScrollCaptureMinimapSnapshot?
	var frozenOverlay = FrozenOverlayState()
	var annotationStyle = FrozenAnnotationStyleState()

	var frozenSelectionTransformAllowed: Bool {
		frozenSelectionEditable && !frozenOverlay.keepsFrozenSelectionFixed
	}

	mutating func resetLiveChrome() {
		loupePatch = nil
	}

	mutating func beginHostLocalFrozenSelecting() {
		hostLocalFrozenSelecting = true
		frozenSelectionSnapshot = nil
		frozenSelectionEditable = false
		frozenSelectionInteraction = nil
		frozenDisplayFrame = nil
		frozenDisplayImage = nil
		frozenBaseImage = nil
		captureFrameSource = .unknown
		captureFrameWindowID = nil
		scrollMinimapPreview = nil
		frozenOverlay.reset()
		annotationStyle = FrozenAnnotationStyleState()
	}

	mutating func endHostLocalFrozenSelecting() {
		hostLocalFrozenSelecting = false
	}

	mutating func resetFrozenChrome() {
		hostLocalFrozenSelecting = false
		frozenSelectionSnapshot = nil
		frozenSelectionEditable = false
		frozenSelectionInteraction = nil
		frozenDisplayFrame = nil
		frozenDisplayImage = nil
		frozenBaseImage = nil
		captureFrameSource = .unknown
		captureFrameWindowID = nil
		scrollMinimapPreview = nil
		frozenOverlay.reset()
		annotationStyle = FrozenAnnotationStyleState()
	}
}

struct NativeScrollCaptureState {
	let stitcher: RsnapScrollCaptureSession
	let viewportRect: CGRect
	let viewportPixelRect: CGRect
	let viewportSamplingRect: CGRect
	let captureSource: CaptureSessionController.FrozenCaptureJobSource
	let viewportPixelsPerPointY: Double
	var sampleLoopScheduled = false
	var sampleDrainProcessing = false
	var sampleProcessing = false
	var toolbarBackdropLoopScheduled = false
	var sampleSequence: UInt64 = 0
	var sampleDrainSequence: UInt64 = 0
	var observedWheelCount: UInt64 = 0
	var committedSampleCount: UInt64 = 0
	var exportRevision: UInt64 = 0
	var lastStreamFrameSequence: UInt64 = 0
	var lastQueuedStreamFrameSequence: UInt64 = 0
	var pendingSampleFrames: [NativeScrollCaptureSampleFrame] = []
	var lastMissingSampleStatusUptime: TimeInterval = 0
	var lastForwardedWheelUptime: TimeInterval = 0
	var lastObservedWheelUptime: TimeInterval = 0
	var controlledScrollInFlight = false
	var queuedForwardedWheelDeltaY: Double = 0
	var queuedForwardedWheelPrecise = true
	var queuedForwardedWheelTargetPoint: CGPoint?
	var lastFallbackCaptureUptime: TimeInterval = 0
	var lastPreviewRefreshUptime: TimeInterval = 0
	var lastWheelInterceptTelemetryUptime: TimeInterval = 0
	var lastWheelObservedTelemetryUptime: TimeInterval = 0
	var lastWheelForwardedTelemetryUptime: TimeInterval = 0
	var sampleUntilUptime: TimeInterval = 0
	var pendingDownwardMotionHintRows: Double = 0
}

struct ScrollCaptureMinimapSnapshot {
	let image: CGImage
	let exportSizePixels: CGSize
	let viewportTopYPixels: CGFloat
	let viewportHeightPixels: CGFloat
}

final class FrozenOverlayState {
	private let session: RsnapFrozenOverlayEditSession
	private var snapshot: FrozenOverlayEditSnapshot

	init() {
		do {
			let session = try RsnapFrozenOverlayEditSession()
			self.session = session
			self.snapshot = try session.snapshot()
		} catch {
			fatalError("Failed to create Rust frozen overlay edit session: \(error)")
		}
	}

	var canUndo: Bool { snapshot.canUndo }
	var canRedo: Bool { snapshot.canRedo }
	var keepsFrozenSelectionFixed: Bool { snapshot.keepsFrozenSelectionFixed }
	var isMovingMovableAnnotation: Bool { snapshot.isMovingMovableAnnotation }
	var hasActiveInteraction: Bool { snapshot.hasActiveInteraction }
	var hasRecognizeTextBlockingEdits: Bool {
		snapshot.hasActiveInteraction || snapshot.activeTextEdit != nil
			|| snapshot.elements.isEmpty == false
	}
	var activeTextEdit: FrozenTextEditState? {
		snapshot.activeTextEdit.map { FrozenTextEditState(anchor: $0.anchor, text: $0.text) }
	}
	var exportElements: [FrozenOverlayExportElement] { snapshot.elements }

	var penStrokes: [FrozenBrushStroke] {
		snapshot.elements.compactMap(Self.penStroke(from:))
	}

	var arrowAnnotations: [FrozenArrowAnnotation] {
		snapshot.elements.compactMap(Self.arrowAnnotation(from:))
	}

	var mosaicRects: [CGRect] {
		snapshot.elements.compactMap(Self.mosaicRect(from:))
	}

	var spotlightAnnotations: [FrozenSpotlightAnnotation] {
		snapshot.elements.compactMap(Self.spotlightAnnotation(from:))
	}

	var textAnnotations: [FrozenTextAnnotation] {
		snapshot.elements.compactMap(Self.textAnnotation(from:))
	}

	var previewPenStroke: FrozenBrushStroke? {
		snapshot.previewPen.flatMap(Self.penStroke(from:))
	}

	var previewArrow: FrozenArrowAnnotation? {
		snapshot.previewArrow.flatMap(Self.arrowAnnotation(from:))
	}

	var previewMosaicRect: CGRect? {
		snapshot.previewMosaic.flatMap(Self.mosaicRect(from:))
	}

	var previewTextAnnotation: FrozenTextAnnotation? {
		snapshot.previewText.flatMap(Self.textAnnotation(from:))
	}

	var previewSpotlightAnnotation: FrozenSpotlightAnnotation? {
		snapshot.previewSpotlight.flatMap(Self.spotlightAnnotation(from:))
	}

	func reset() {
		do {
			try session.reset()
			try refreshSnapshot()
		} catch {
			fatalError("Failed to reset Rust frozen overlay state: \(error)")
		}
	}

	func begin(
		tool: ToolbarItemKind,
		at point: CGPoint,
		selection: CGRect,
		style: FrozenAnnotationStyleState
	) -> Bool {
		performRefreshingWhenChanged {
			try session.begin(tool: tool, at: point, selection: selection, style: style.editStyle)
		}
	}

	func update(to point: CGPoint, selection: CGRect) -> Bool {
		performRefreshingWhenChanged {
			try session.update(to: point, selection: selection)
		}
	}

	func finish(selection: CGRect) -> Bool {
		performRefreshingAlways {
			try session.finish(selection: selection)
		}
	}

	func appendText(_ text: String) -> Bool {
		performRefreshingWhenChanged {
			try session.appendText(text)
		}
	}

	func backspaceText() -> Bool {
		performRefreshingWhenChanged {
			try session.backspaceText()
		}
	}

	func commitTextEdit(style: FrozenTextStyle) -> Bool {
		performRefreshingAlways {
			try session.commitText(
				style: FrozenOverlayEditStyle(
					strokeWidthPoints: 3,
					strokeColor: .blue,
					spotlightBorderWidthPoints: 0,
					spotlightColor: .blue,
					textFontSizePoints: style.fontSizePoints,
					textColor: style.color.exportColor
				)
			)
		}
	}

	func undo() -> Bool {
		performRefreshingAlways {
			try session.undo()
		}
	}

	func redo() -> Bool {
		performRefreshingAlways {
			try session.redo()
		}
	}

	func containsMovableAnnotation(at point: CGPoint) -> Bool {
		do {
			return try session.containsMovableAnnotation(at: point)
		} catch {
			fatalError("Failed to hit-test Rust frozen overlay annotation: \(error)")
		}
	}

	private func performRefreshingWhenChanged(_ operation: () throws -> Bool) -> Bool {
		do {
			let changed = try operation()
			if changed {
				try refreshSnapshot()
			}
			return changed
		} catch {
			fatalError("Failed to update Rust frozen overlay state: \(error)")
		}
	}

	private func performRefreshingAlways(_ operation: () throws -> Bool) -> Bool {
		do {
			let changed = try operation()
			try refreshSnapshot()
			return changed
		} catch {
			fatalError("Failed to update Rust frozen overlay state: \(error)")
		}
	}

	private func refreshSnapshot() throws {
		snapshot = try session.snapshot()
	}

	private static func penStroke(from element: FrozenOverlayExportElement) -> FrozenBrushStroke? {
		guard case .pen(let points, let style) = element else {
			return nil
		}
		return FrozenBrushStroke(points: points, style: style.frozenBrushStyle)
	}

	private static func arrowAnnotation(from element: FrozenOverlayExportElement)
		-> FrozenArrowAnnotation?
	{
		guard case .arrow(let start, let end, let style) = element else {
			return nil
		}
		return FrozenArrowAnnotation(start: start, end: end, style: style.frozenBrushStyle)
	}

	private static func mosaicRect(from element: FrozenOverlayExportElement) -> CGRect? {
		guard case .mosaic(let rect) = element else {
			return nil
		}
		return rect
	}

	private static func spotlightAnnotation(from element: FrozenOverlayExportElement)
		-> FrozenSpotlightAnnotation?
	{
		guard case .spotlight(let rect, let style) = element else {
			return nil
		}
		return FrozenSpotlightAnnotation(rect: rect, style: style.frozenSpotlightStyle)
	}

	private static func textAnnotation(from element: FrozenOverlayExportElement)
		-> FrozenTextAnnotation?
	{
		guard case .text(let anchor, let text, let style) = element else {
			return nil
		}
		return FrozenTextAnnotation(anchor: anchor, text: text, style: style.frozenTextStyle)
	}
}

package struct FrozenToolbarItemLayout: Equatable {
	package let kind: ToolbarItemKind
	package let frame: CGRect
	package let enabled: Bool
	package let selected: Bool
}

package struct FrozenAnnotationColorSwatchLayout: Equatable {
	package let color: FrozenAnnotationColor
	package let frame: CGRect
	package let selected: Bool
}

package struct FrozenAnnotationStyleLayout: Equatable {
	package let kind: FrozenAnnotationStyleToolbarKind
	package let scale: CGFloat
	package let frame: CGRect
	package let sizeControlFrame: CGRect
	package let decreaseFrame: CGRect
	package let increaseFrame: CGRect
	package let displayFrame: CGRect
	package let swatches: [FrozenAnnotationColorSwatchLayout]
}

package struct FrozenToolbarLayout {
	package let scale: CGFloat
	package let frame: CGRect
	package let items: [FrozenToolbarItemLayout]
	package let annotationStyle: FrozenAnnotationStyleLayout?
}
