import AppKit
import CoreGraphics

@MainActor
struct SelectionChromeRenderer {
	static func drawSelectionScrim(
		for focusRect: CGRect,
		bounds: CGRect,
		in context: CGContext,
		alpha: CGFloat,
		excluding exclusionPath: CGPath? = nil
	) {
		let scrimColor = NSColor(calibratedWhite: 0, alpha: alpha)
		let visibleFocusRect = focusRect.intersection(bounds)
		if visibleFocusRect.isNull || visibleFocusRect.width <= 0 || visibleFocusRect.height <= 0 {
			context.setFillColor(scrimColor.cgColor)
			context.fill(bounds)
			return
		}

		context.saveGState()
		OverlayMaskGeometry.drawScrim(
			in: context,
			bounds: bounds,
			focusRect: visibleFocusRect,
			color: scrimColor.cgColor,
			pathExclusions: [exclusionPath].compactMap { $0 }
		)
		context.restoreGState()
	}

	static func drawDashedSelectionBorder(
		around rect: CGRect,
		in context: CGContext,
		lineWidth: CGFloat,
		pixelsPerPoint: CGFloat
	) {
		let outlineColor = NSColor(
			calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
		let strokeColor = NSColor(
			calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 248 / 255)
		let borderOutset = CaptureChrome.dashedBorderOutset(
			strokeWidth: lineWidth,
			pixelsPerPoint: pixelsPerPoint
		)
		let borderRect = rect.insetBy(dx: -borderOutset, dy: -borderOutset)
		let path = CaptureChrome.dashedBorderPath(
			for: borderRect
		)

		context.saveGState()
		context.setLineCap(.butt)
		context.setLineJoin(.miter)

		context.addPath(path)
		context.setStrokeColor(outlineColor.cgColor)
		context.setLineWidth(lineWidth + 0.75)
		context.strokePath()

		context.addPath(path)
		context.setStrokeColor(strokeColor.cgColor)
		context.setLineWidth(lineWidth)
		context.strokePath()
		context.restoreGState()
	}

	static func drawFrozenResizeHandles(
		for rect: CGRect,
		orientation: FrozenResizeHandleOrientationPreference,
		in context: CGContext
	) {
		let outlineColor = NSColor(
			calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 124 / 255)
		let strokeColor = NSColor(
			calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 246 / 255)
		let leg = CaptureChrome.resizeHandleLegLength
		let offset = CaptureChrome.resizeHandleOffset
		let handles: [(CGPoint, CGPoint, CGPoint)]
		switch orientation {
		case .outward:
			handles = [
				(
					CGPoint(x: rect.minX - offset - leg, y: rect.maxY + offset + leg),
					CGPoint(x: rect.minX - offset, y: rect.maxY + offset + leg),
					CGPoint(x: rect.minX - offset - leg, y: rect.maxY + offset)
				),
				(
					CGPoint(x: rect.maxX + offset + leg, y: rect.maxY + offset + leg),
					CGPoint(x: rect.maxX + offset, y: rect.maxY + offset + leg),
					CGPoint(x: rect.maxX + offset + leg, y: rect.maxY + offset)
				),
				(
					CGPoint(x: rect.minX - offset - leg, y: rect.minY - offset - leg),
					CGPoint(x: rect.minX - offset, y: rect.minY - offset - leg),
					CGPoint(x: rect.minX - offset - leg, y: rect.minY - offset)
				),
				(
					CGPoint(x: rect.maxX + offset + leg, y: rect.minY - offset - leg),
					CGPoint(x: rect.maxX + offset, y: rect.minY - offset - leg),
					CGPoint(x: rect.maxX + offset + leg, y: rect.minY - offset)
				),
			]
		case .inward:
			handles = [
				(
					CGPoint(x: rect.minX - offset, y: rect.maxY + offset),
					CGPoint(x: rect.minX - offset - leg, y: rect.maxY + offset),
					CGPoint(x: rect.minX - offset, y: rect.maxY + offset + leg)
				),
				(
					CGPoint(x: rect.maxX + offset, y: rect.maxY + offset),
					CGPoint(x: rect.maxX + offset + leg, y: rect.maxY + offset),
					CGPoint(x: rect.maxX + offset, y: rect.maxY + offset + leg)
				),
				(
					CGPoint(x: rect.minX - offset, y: rect.minY - offset),
					CGPoint(x: rect.minX - offset - leg, y: rect.minY - offset),
					CGPoint(x: rect.minX - offset, y: rect.minY - offset - leg)
				),
				(
					CGPoint(x: rect.maxX + offset, y: rect.minY - offset),
					CGPoint(x: rect.maxX + offset + leg, y: rect.minY - offset),
					CGPoint(x: rect.maxX + offset, y: rect.minY - offset - leg)
				),
			]
		}

		context.saveGState()
		context.setLineCap(.butt)
		context.setLineJoin(.miter)
		for (elbow, horizontal, vertical) in handles {
			let path = CGMutablePath()
			path.move(to: horizontal)
			path.addLine(to: elbow)
			path.addLine(to: vertical)

			context.addPath(path)
			context.setStrokeColor(outlineColor.cgColor)
			context.setLineWidth(CaptureChrome.resizeHandleStrokeWidth + 0.8)
			context.strokePath()

			context.addPath(path)
			context.setStrokeColor(strokeColor.cgColor)
			context.setLineWidth(CaptureChrome.resizeHandleStrokeWidth)
			context.strokePath()
		}
		context.restoreGState()
	}

	static func drawSelectionSizeBadge(
		for rect: CGRect,
		text: String,
		bounds: CGRect,
		avoiding toolbarFrame: CGRect?,
		in context: CGContext
	) {
		let font = LiveChromePlacementPlanner.metrics.font
		let textSize = text.size(using: font)
		let badgeFrame = CaptureChrome.selectionSizeBadgeFrame(
			for: rect,
			textSize: textSize,
			in: bounds,
			avoiding: toolbarFrame
		)
		let anchor = badgeFrame.origin

		drawText(
			text, at: CGPoint(x: anchor.x, y: anchor.y - 1),
			color: NSColor.black.withAlphaComponent(0.6), font: font)
		drawText(
			text, at: CGPoint(x: anchor.x - 1, y: anchor.y),
			color: NSColor.black.withAlphaComponent(0.75), font: font)
		drawText(
			text, at: CGPoint(x: anchor.x + 1, y: anchor.y),
			color: NSColor.black.withAlphaComponent(0.75), font: font)
		drawText(
			text, at: CGPoint(x: anchor.x, y: anchor.y + 1),
			color: NSColor.black.withAlphaComponent(0.75), font: font)
		drawText(
			text, at: CGPoint(x: anchor.x, y: anchor.y),
			color: NSColor.white.withAlphaComponent(0.98), font: font)
	}

	private static func drawText(_ text: String, at point: CGPoint, color: NSColor, font: NSFont) {
		(text as NSString).draw(
			at: point,
			withAttributes: [
				.font: font,
				.foregroundColor: color,
			])
	}
}
