import AppKit
import CoreGraphics

@MainActor
enum CaptureHostScrollMinimapRenderer {
	static func render(
		preview: ScrollCaptureMinimapSnapshot,
		selection: CGRect,
		bounds: CGRect,
		palette: CaptureChromePalette,
		in context: CGContext
	) {
		guard
			let minimapPlan = scrollCaptureMinimapPlan(
				for: selection,
				exportSize: preview.exportSizePixels,
				in: bounds,
				preferredWidth: CaptureChrome.scrollMinimapPreferredWidth,
				minimumWidth: CaptureChrome.scrollMinimapMinimumWidth,
				gap: CaptureChrome.scrollMinimapGap,
				margin: CaptureChrome.scrollMinimapScreenMargin,
				imageInset: CaptureChrome.scrollMinimapImageInset,
				viewportTopPixels: preview.viewportTopYPixels,
				viewportHeightPixels: preview.viewportHeightPixels
			)
		else {
			return
		}

		let frame = minimapPlan.frame
		let imageFrame = minimapPlan.imageFrame
		let backgroundPath = NSBezierPath(
			roundedRect: frame,
			xRadius: CaptureChrome.scrollMinimapCornerRadius,
			yRadius: CaptureChrome.scrollMinimapCornerRadius
		)

		context.saveGState()
		context.setShadow(
			offset: CGSize(width: 0, height: -2),
			blur: 12,
			color: NSColor.black.withAlphaComponent(0.32).cgColor
		)
		context.setFillColor(NSColor.black.withAlphaComponent(0.72).cgColor)
		backgroundPath.fill()
		context.restoreGState()

		context.saveGState()
		let imageClipPath = NSBezierPath(
			roundedRect: imageFrame,
			xRadius: max(CaptureChrome.scrollMinimapCornerRadius - 3, 1),
			yRadius: max(CaptureChrome.scrollMinimapCornerRadius - 3, 1)
		)
		imageClipPath.addClip()
		context.interpolationQuality = .high
		context.draw(preview.image, in: imageFrame)
		context.restoreGState()

		if let viewportFrame = minimapPlan.viewportFrame {
			context.setFillColor(NSColor.white.withAlphaComponent(0.13).cgColor)
			context.fill(viewportFrame)
			context.setStrokeColor(NSColor.white.withAlphaComponent(0.88).cgColor)
			context.setLineWidth(1)
			context.stroke(viewportFrame)
		}

		context.setStrokeColor(palette.keycapStroke.withAlphaComponent(0.88).cgColor)
		context.setLineWidth(1)
		backgroundPath.stroke()
	}
}
