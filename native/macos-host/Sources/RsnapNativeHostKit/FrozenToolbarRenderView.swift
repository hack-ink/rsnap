import AppKit
import CoreGraphics
import CoreText
import RsnapHostBridge

@MainActor
final class FrozenToolbarRenderView: NSView {
	struct Item: Equatable {
		let kind: ToolbarItemKind
		let frame: CGRect
		let enabled: Bool
		let selected: Bool
	}

	private var theme: CaptureChromeTheme = .dark
	private var settings = NativeHostSettings.defaults
	private var hoveredToolbarAction: ToolbarItemKind?
	private var hoveredAnnotationStyleAction: FrozenAnnotationStyleAction?
	private var toolbarScale: CGFloat = 1
	private var annotationStyleState = FrozenAnnotationStyleState()
	private var annotationStyleLayout: FrozenAnnotationStyleLayout?
	private var items: [Item] = []

	override var isOpaque: Bool { false }

	override func hitTest(_ point: NSPoint) -> NSView? {
		nil
	}

	@discardableResult
	func update(
		theme: CaptureChromeTheme,
		settings: NativeHostSettings,
		hoveredToolbarAction: ToolbarItemKind?,
		hoveredAnnotationStyleAction: FrozenAnnotationStyleAction?,
		toolbarScale: CGFloat,
		annotationStyleState: FrozenAnnotationStyleState,
		annotationStyleLayout: FrozenAnnotationStyleLayout?,
		items: [Item]
	) -> Bool {
		let changed =
			self.theme != theme || self.settings != settings
			|| self.hoveredToolbarAction != hoveredToolbarAction
			|| self.hoveredAnnotationStyleAction != hoveredAnnotationStyleAction
			|| self.toolbarScale != toolbarScale
			|| self.annotationStyleState != annotationStyleState
			|| self.annotationStyleLayout != annotationStyleLayout || self.items != items
		self.theme = theme
		self.settings = settings
		self.hoveredToolbarAction = hoveredToolbarAction
		self.hoveredAnnotationStyleAction = hoveredAnnotationStyleAction
		self.toolbarScale = toolbarScale
		self.annotationStyleState = annotationStyleState
		self.annotationStyleLayout = annotationStyleLayout
		self.items = items
		if changed {
			needsDisplay = true
		}
		return changed
	}

	override func draw(_ dirtyRect: NSRect) {
		super.draw(dirtyRect)
		guard let context = NSGraphicsContext.current?.cgContext else {
			return
		}
		context.clear(bounds)
		drawToolbarContent(in: context)
	}

	private func drawToolbarContent(in context: CGContext) {
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let pillPath = NSBezierPath(
			roundedRect: bounds,
			xRadius: CaptureChrome.hudCornerRadius,
			yRadius: CaptureChrome.hudCornerRadius
		)
		context.setStrokeColor(palette.outerStroke.cgColor)
		context.setLineWidth(1)
		pillPath.stroke()

		for item in items {
			if hoveredToolbarAction == item.kind, item.enabled, !item.selected {
				context.setFillColor(palette.toolbarHoverBackground.cgColor)
				let radius = CaptureChrome.toolbarControlCornerRadius * toolbarScale
				let hoverPath = NSBezierPath(
					roundedRect: item.frame,
					xRadius: radius,
					yRadius: radius
				)
				hoverPath.fill()
			}
			if item.selected {
				context.setFillColor(palette.toolbarSelectedBackground.cgColor)
				let radius = CaptureChrome.toolbarControlCornerRadius * toolbarScale
				let selectedPath = NSBezierPath(
					roundedRect: item.frame,
					xRadius: radius,
					yRadius: radius
				)
				selectedPath.fill()
			}

			let symbolColor =
				item.enabled
				? (item.selected ? palette.toolbarSelectedIcon : palette.toolbarIcon)
				: palette.toolbarDisabledIcon
			drawToolbarGlyph(
				item.kind,
				selected: item.selected,
				in: item.frame,
				scale: toolbarScale,
				color: symbolColor,
				context: context
			)
		}

		if let annotationStyleLayout {
			FrozenToolbarDrawing.drawAnnotationStyleControls(
				annotationStyleLayout,
				state: annotationStyleState,
				hoveredAction: hoveredAnnotationStyleAction,
				palette: palette,
				in: context
			)
		}
	}

	private func drawToolbarGlyph(
		_ kind: ToolbarItemKind,
		selected: Bool,
		in rect: CGRect,
		scale: CGFloat,
		color: NSColor,
		context: CGContext
	) {
		let glyph = PhosphorToolbarIcons.cachedGlyph(
			for: kind,
			selected: selected,
			size: CaptureChrome.toolbarGlyphSize * scale
		)
		let origin = CGPoint(
			x: rect.midX - glyph.bounds.width * 0.5 - glyph.bounds.origin.x,
			y: rect.midY - glyph.bounds.height * 0.5 - glyph.bounds.origin.y
		)
		context.saveGState()
		context.setFillColor(color.cgColor)
		context.textMatrix = .identity
		context.textPosition = origin
		CTLineDraw(glyph.line, context)
		context.restoreGState()
	}
}

@MainActor
enum FrozenToolbarDrawing {
	static func drawAnnotationStyleControls(
		_ layout: FrozenAnnotationStyleLayout,
		state: FrozenAnnotationStyleState,
		hoveredAction: FrozenAnnotationStyleAction?,
		palette: CaptureChromePalette,
		in context: CGContext
	) {
		drawSizeControl(
			layout,
			state: state,
			hoveredAction: hoveredAction,
			palette: palette,
			in: context
		)
		for swatch in layout.swatches {
			drawColorSwatch(swatch, palette: palette, in: context)
		}
	}

	private static func drawSizeControl(
		_ layout: FrozenAnnotationStyleLayout,
		state: FrozenAnnotationStyleState,
		hoveredAction: FrozenAnnotationStyleAction?,
		palette: CaptureChromePalette,
		in context: CGContext
	) {
		let sizeHovered = hoveredAction == .decreaseSize || hoveredAction == .increaseSize
		let scale = layout.scale
		let capsuleRect = layout.sizeControlFrame.insetBy(dx: 0, dy: 3 * scale)
		let capsulePath = NSBezierPath(
			roundedRect: capsuleRect,
			xRadius: CaptureChrome.toolbarControlCornerRadius * scale,
			yRadius: CaptureChrome.toolbarControlCornerRadius * scale
		)
		context.setFillColor(
			(sizeHovered
				? palette.toolbarHoverBackground.withAlphaComponent(0.72)
				: palette.toolbarHoverBackground.withAlphaComponent(0.42)).cgColor)
		capsulePath.fill()
		context.setStrokeColor(
			palette.outerStroke.withAlphaComponent(sizeHovered ? 0.52 : 0.36).cgColor)
		context.setLineWidth(max(0.5, scale))
		capsulePath.stroke()

		for (action, frame) in [
			(FrozenAnnotationStyleAction.decreaseSize, layout.decreaseFrame),
			(FrozenAnnotationStyleAction.increaseSize, layout.increaseFrame),
		] where hoveredAction == action {
			context.setFillColor(palette.toolbarHoverBackground.cgColor)
			NSBezierPath(
				roundedRect: frame.insetBy(dx: 2 * scale, dy: 4 * scale),
				xRadius: 6 * scale,
				yRadius: 6 * scale
			).fill()
		}

		context.setStrokeColor(palette.outerStroke.withAlphaComponent(0.34).cgColor)
		context.setLineWidth(max(0.5, scale))
		for dividerX in [layout.displayFrame.minX, layout.displayFrame.maxX] {
			context.beginPath()
			context.move(to: CGPoint(x: dividerX, y: capsuleRect.minY + 5 * scale))
			context.addLine(to: CGPoint(x: dividerX, y: capsuleRect.maxY - 5 * scale))
			context.strokePath()
		}

		let font = NSFont.monospacedSystemFont(
			ofSize: max(1, CaptureChrome.toolbarControlFontSize * scale),
			weight: .medium
		)
		drawCenteredText(
			"-",
			in: layout.decreaseFrame,
			font: font,
			color: palette.toolbarIcon,
			context: context
		)
		drawCenteredText(
			"+",
			in: layout.increaseFrame,
			font: font,
			color: palette.toolbarIcon,
			context: context
		)

		switch layout.kind {
		case .brush:
			drawBrushSizeDisplay(
				in: layout.displayFrame,
				state: state,
				scale: scale,
				font: font,
				color: palette.labelText,
				context: context
			)
		case .spotlight, .text:
			drawCenteredText(
				layout.kind.sizeLabel(in: state),
				in: layout.displayFrame,
				font: font,
				color: palette.labelText,
				context: context
			)
		}
	}

	private static func drawBrushSizeDisplay(
		in frame: CGRect,
		state: FrozenAnnotationStyleState,
		scale: CGFloat,
		font: NSFont,
		color: NSColor,
		context: CGContext
	) {
		let previewColor = state.brushStyle.color.nsColor(alpha: 0.96)
		let previewWidth = (state.brushStyle.strokeWidthPoints * scale).clamped(to: 0.5...10)
		let previewHalfLength = CaptureChrome.annotationPenPreviewLength * scale / 2
		let previewCenter = CGPoint(x: frame.minX + 10 * scale + previewHalfLength, y: frame.midY)
		let previewStart = CGPoint(x: previewCenter.x - previewHalfLength, y: previewCenter.y)
		let previewEnd = CGPoint(x: previewCenter.x + previewHalfLength, y: previewCenter.y)

		context.saveGState()
		context.setStrokeColor(previewColor.cgColor)
		context.setLineWidth(previewWidth)
		context.setLineCap(.round)
		context.beginPath()
		context.move(to: previewStart)
		context.addLine(to: previewEnd)
		context.strokePath()
		context.restoreGState()

		let label = FrozenAnnotationStyleToolbarKind.brush.sizeLabel(in: state)
		let labelSize = label.size(using: font)
		drawText(
			label,
			at: CGPoint(
				x: previewEnd.x + CaptureChrome.annotationSizePreviewGap * scale,
				y: frame.midY - labelSize.height / 2
			),
			font: font,
			color: color,
			context: context
		)
	}

	private static func drawColorSwatch(
		_ swatch: FrozenAnnotationColorSwatchLayout,
		palette: CaptureChromePalette,
		in context: CGContext
	) {
		let radius = swatch.frame.width / 2 - 1
		let center = CGPoint(x: swatch.frame.midX, y: swatch.frame.midY)
		let rect = CGRect(
			x: center.x - radius,
			y: center.y - radius,
			width: radius * 2,
			height: radius * 2
		)
		let path = NSBezierPath(ovalIn: rect)
		context.setFillColor(swatch.color.nsColor().cgColor)
		path.fill()
		context.setStrokeColor(
			(swatch.selected ? palette.toolbarSelectedIcon : palette.toolbarIcon)
				.withAlphaComponent(swatch.selected ? 0.95 : 0.56).cgColor)
		let scale = max(0.5, swatch.frame.width / max(CaptureChrome.annotationSwatchSize, 1))
		context.setLineWidth(swatch.selected ? 2 * scale : scale)
		path.stroke()
	}

	private static func drawCenteredText(
		_ text: String,
		in frame: CGRect,
		font: NSFont,
		color: NSColor,
		context: CGContext
	) {
		let size = text.size(using: font)
		drawText(
			text,
			at: CGPoint(x: frame.midX - size.width / 2, y: frame.midY - size.height / 2),
			font: font,
			color: color,
			context: context
		)
	}

	private static func drawText(
		_ text: String,
		at point: CGPoint,
		font: NSFont,
		color: NSColor,
		context: CGContext
	) {
		let graphicsContext = NSGraphicsContext(cgContext: context, flipped: false)
		NSGraphicsContext.saveGraphicsState()
		NSGraphicsContext.current = graphicsContext
		(text as NSString).draw(
			at: point,
			withAttributes: [
				.font: font,
				.foregroundColor: color,
			])
		NSGraphicsContext.restoreGraphicsState()
	}
}
