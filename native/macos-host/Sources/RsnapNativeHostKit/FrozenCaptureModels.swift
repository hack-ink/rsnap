import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

enum FrozenAnnotationColor: CaseIterable, Equatable {
	case white
	case yellow
	case green
	case blue
	case red
	case black

	func nsColor(alpha: CGFloat = 1) -> NSColor {
		let color =
			switch self {
			case .white:
				NSColor(srgbRed: 255 / 255, green: 255 / 255, blue: 255 / 255, alpha: 1)
			case .yellow:
				NSColor(srgbRed: 255 / 255, green: 219 / 255, blue: 77 / 255, alpha: 1)
			case .green:
				NSColor(srgbRed: 92 / 255, green: 214 / 255, blue: 149 / 255, alpha: 1)
			case .blue:
				NSColor(srgbRed: 102 / 255, green: 178 / 255, blue: 255 / 255, alpha: 1)
			case .red:
				NSColor(srgbRed: 255 / 255, green: 107 / 255, blue: 107 / 255, alpha: 1)
			case .black:
				NSColor(srgbRed: 24 / 255, green: 24 / 255, blue: 24 / 255, alpha: 1)
			}
		return color.withAlphaComponent(alpha)
	}

	var textShadowColor: NSColor {
		switch self {
		case .black:
			return NSColor.white.withAlphaComponent(0.48)
		case .white, .yellow, .green, .blue, .red:
			return NSColor.black.withAlphaComponent(0.45)
		}
	}
}

struct FrozenBrushStyle: Equatable {
	private static let defaultStrokeWidth: CGFloat = 3.0
	private static let minStrokeWidth: CGFloat = 1.0
	private static let maxStrokeWidth: CGFloat = 24.0
	private static let strokeWidthStep: CGFloat = 0.25

	var strokeWidthPoints = defaultStrokeWidth
	var color: FrozenAnnotationColor = .blue

	mutating func applySizeSteps(_ steps: Int) -> Bool {
		guard steps != 0 else {
			return false
		}
		let direction = steps.signum()
		var changed = false
		for _ in 0..<abs(steps) {
			changed =
				setStrokeWidth(strokeWidthPoints + CGFloat(direction) * Self.strokeWidthStep)
				|| changed
		}
		return changed
	}

	private mutating func setStrokeWidth(_ value: CGFloat) -> Bool {
		let clamped = value.clamped(to: Self.minStrokeWidth...Self.maxStrokeWidth)
		guard abs(clamped - strokeWidthPoints) > .ulpOfOne else {
			return false
		}
		strokeWidthPoints = clamped
		return true
	}
}

struct FrozenSpotlightStyle: Equatable {
	private static let defaultBorderWidth: CGFloat = 0.0
	private static let minBorderWidth: CGFloat = 0.0
	private static let maxBorderWidth: CGFloat = 24.0
	private static let borderWidthStep: CGFloat = 0.25

	var borderWidthPoints = defaultBorderWidth
	var borderColor: FrozenAnnotationColor = .blue

	mutating func applySizeSteps(_ steps: Int) -> Bool {
		guard steps != 0 else {
			return false
		}
		let direction = steps.signum()
		var changed = false
		for _ in 0..<abs(steps) {
			changed =
				setBorderWidth(borderWidthPoints + CGFloat(direction) * Self.borderWidthStep)
				|| changed
		}
		return changed
	}

	private mutating func setBorderWidth(_ value: CGFloat) -> Bool {
		let clamped = value.clamped(to: Self.minBorderWidth...Self.maxBorderWidth)
		guard abs(clamped - borderWidthPoints) > .ulpOfOne else {
			return false
		}
		borderWidthPoints = clamped
		return true
	}
}

struct FrozenTextStyle: Equatable {
	private static let defaultFontSize: CGFloat = 16.0
	private static let minFontSize: CGFloat = 12.0
	private static let maxFontSize: CGFloat = 72.0

	var fontSizePoints = defaultFontSize
	var color: FrozenAnnotationColor = .blue

	mutating func applySizeSteps(_ steps: Int) -> Bool {
		guard steps != 0 else {
			return false
		}
		var nextSize = fontSizePoints
		for _ in 0..<abs(steps) {
			if steps > 0 {
				nextSize =
					abs(nextSize - nextSize.rounded()) <= .ulpOfOne
					? nextSize + 1
					: ceil(nextSize)
			} else {
				nextSize =
					abs(nextSize - nextSize.rounded()) <= .ulpOfOne
					? nextSize - 1
					: floor(nextSize)
			}
		}
		return setFontSize(nextSize)
	}

	private mutating func setFontSize(_ value: CGFloat) -> Bool {
		let clamped = value.clamped(to: Self.minFontSize...Self.maxFontSize)
		guard abs(clamped - fontSizePoints) > .ulpOfOne else {
			return false
		}
		fontSizePoints = clamped
		return true
	}
}

extension FrozenAnnotationColor {
	fileprivate var exportColor: FrozenOverlayExportColor {
		switch self {
		case .white:
			.white
		case .yellow:
			.yellow
		case .green:
			.green
		case .blue:
			.blue
		case .red:
			.red
		case .black:
			.black
		}
	}
}

extension FrozenBrushStyle {
	fileprivate var exportStrokeStyle: FrozenOverlayExportStrokeStyle {
		FrozenOverlayExportStrokeStyle(
			strokeWidthPoints: strokeWidthPoints,
			color: color.exportColor
		)
	}
}

extension FrozenSpotlightStyle {
	fileprivate var exportSpotlightStyle: FrozenOverlayExportSpotlightStyle {
		FrozenOverlayExportSpotlightStyle(
			borderWidthPoints: borderWidthPoints,
			borderColor: borderColor.exportColor
		)
	}
}

extension FrozenTextStyle {
	fileprivate var exportTextStyle: FrozenOverlayExportTextStyle {
		FrozenOverlayExportTextStyle(
			fontSizePoints: fontSizePoints,
			color: color.exportColor
		)
	}
}

extension FrozenOverlayExportColor {
	fileprivate var annotationColor: FrozenAnnotationColor {
		switch self {
		case .white:
			.white
		case .yellow:
			.yellow
		case .green:
			.green
		case .blue:
			.blue
		case .red:
			.red
		case .black:
			.black
		}
	}
}

extension FrozenOverlayExportStrokeStyle {
	fileprivate var frozenBrushStyle: FrozenBrushStyle {
		FrozenBrushStyle(strokeWidthPoints: strokeWidthPoints, color: color.annotationColor)
	}
}

extension FrozenOverlayExportSpotlightStyle {
	fileprivate var frozenSpotlightStyle: FrozenSpotlightStyle {
		FrozenSpotlightStyle(
			borderWidthPoints: borderWidthPoints,
			borderColor: borderColor.annotationColor
		)
	}
}

extension FrozenOverlayExportTextStyle {
	fileprivate var frozenTextStyle: FrozenTextStyle {
		FrozenTextStyle(fontSizePoints: fontSizePoints, color: color.annotationColor)
	}
}

extension FrozenAnnotationStyleState {
	fileprivate var editStyle: FrozenOverlayEditStyle {
		FrozenOverlayEditStyle(
			strokeWidthPoints: brushStyle.strokeWidthPoints,
			strokeColor: brushStyle.color.exportColor,
			spotlightBorderWidthPoints: spotlightStyle.borderWidthPoints,
			spotlightColor: spotlightStyle.borderColor.exportColor,
			textFontSizePoints: textStyle.fontSizePoints,
			textColor: textStyle.color.exportColor
		)
	}
}

enum FrozenAnnotationStyleAction: Equatable {
	case decreaseSize
	case increaseSize
	case color(FrozenAnnotationColor)
}

enum FrozenAnnotationStyleToolbarKind: Equatable {
	case brush
	case spotlight
	case text

	init?(selectedTool: ToolbarItemKind) {
		switch selectedTool {
		case .pen, .arrow:
			self = .brush
		case .spotlight:
			self = .spotlight
		case .text:
			self = .text
		case .pointer, .mosaic, .undo, .redo, .autoCenter, .scroll, .ocr, .copy, .save:
			return nil
		}
	}

	private var baseSizeDisplayWidth: CGFloat {
		switch self {
		case .brush:
			return 84
		case .spotlight:
			return 58
		case .text:
			return 58
		}
	}

	func sizeDisplayWidth(scale: CGFloat) -> CGFloat {
		baseSizeDisplayWidth * scale
	}

	func sizeControlWidth(scale: CGFloat) -> CGFloat {
		sizeDisplayWidth(scale: scale)
			+ CaptureChrome.annotationSizeButtonWidth * scale * 2
	}

	func selectedColor(in state: FrozenAnnotationStyleState) -> FrozenAnnotationColor {
		switch self {
		case .brush:
			return state.brushStyle.color
		case .spotlight:
			return state.spotlightStyle.borderColor
		case .text:
			return state.textStyle.color
		}
	}

	func sizeLabel(in state: FrozenAnnotationStyleState) -> String {
		switch self {
		case .brush:
			return Self.trimmedDecimalLabel(state.brushStyle.strokeWidthPoints)
		case .spotlight:
			return Self.trimmedDecimalLabel(state.spotlightStyle.borderWidthPoints)
		case .text:
			let size = state.textStyle.fontSizePoints
			let text =
				abs(size - size.rounded()) <= .ulpOfOne
				? "\(Int(size.rounded()))"
				: String(format: "%.1f", Double(size))
			return "\(text) pt"
		}
	}

	private static func trimmedDecimalLabel(_ value: CGFloat) -> String {
		var text = String(format: "%.2f", Double(value))
		while text.contains(".") && text.hasSuffix("0") {
			text.removeLast()
		}
		if text.hasSuffix(".") {
			text.removeLast()
		}
		return text
	}
}

struct FrozenAnnotationStyleState: Equatable {
	var brushStyle = FrozenBrushStyle()
	var spotlightStyle = FrozenSpotlightStyle()
	var textStyle = FrozenTextStyle()

	mutating func apply(
		_ action: FrozenAnnotationStyleAction,
		selectedTool: ToolbarItemKind
	) -> Bool {
		guard let kind = FrozenAnnotationStyleToolbarKind(selectedTool: selectedTool) else {
			return false
		}
		switch (kind, action) {
		case (.brush, .decreaseSize):
			return brushStyle.applySizeSteps(-1)
		case (.brush, .increaseSize):
			return brushStyle.applySizeSteps(1)
		case (.brush, .color(let color)):
			guard brushStyle.color != color else {
				return false
			}
			brushStyle.color = color
			return true
		case (.spotlight, .decreaseSize):
			return spotlightStyle.applySizeSteps(-1)
		case (.spotlight, .increaseSize):
			return spotlightStyle.applySizeSteps(1)
		case (.spotlight, .color(let color)):
			guard spotlightStyle.borderColor != color else {
				return false
			}
			spotlightStyle.borderColor = color
			return true
		case (.text, .decreaseSize):
			return textStyle.applySizeSteps(-1)
		case (.text, .increaseSize):
			return textStyle.applySizeSteps(1)
		case (.text, .color(let color)):
			guard textStyle.color != color else {
				return false
			}
			textStyle.color = color
			return true
		}
	}

	mutating func applySizeSteps(_ steps: Int, selectedTool: ToolbarItemKind) -> Bool {
		guard let kind = FrozenAnnotationStyleToolbarKind(selectedTool: selectedTool) else {
			return false
		}
		switch kind {
		case .brush:
			return brushStyle.applySizeSteps(steps)
		case .spotlight:
			return spotlightStyle.applySizeSteps(steps)
		case .text:
			return textStyle.applySizeSteps(steps)
		}
	}
}

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

struct FrozenToolbarItemLayout: Equatable {
	let kind: ToolbarItemKind
	let frame: CGRect
	let enabled: Bool
	let selected: Bool
}

struct FrozenAnnotationColorSwatchLayout: Equatable {
	let color: FrozenAnnotationColor
	let frame: CGRect
	let selected: Bool
}

struct FrozenAnnotationStyleLayout: Equatable {
	let kind: FrozenAnnotationStyleToolbarKind
	let scale: CGFloat
	let frame: CGRect
	let sizeControlFrame: CGRect
	let decreaseFrame: CGRect
	let increaseFrame: CGRect
	let displayFrame: CGRect
	let swatches: [FrozenAnnotationColorSwatchLayout]
}

struct FrozenToolbarLayout {
	let scale: CGFloat
	let frame: CGRect
	let items: [FrozenToolbarItemLayout]
	let annotationStyle: FrozenAnnotationStyleLayout?
}
