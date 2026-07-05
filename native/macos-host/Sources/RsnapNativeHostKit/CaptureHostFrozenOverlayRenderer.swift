import AppKit
import CoreGraphics

@MainActor
struct CaptureHostFrozenOverlayRenderer {
	static func render(
		selection: CGRect,
		chrome: CaptureChromeState,
		windowFrame: CGRect,
		bounds: CGRect,
		in context: CGContext
	) {
		drawFrozenMosaics(
			selection: selection,
			chrome: chrome,
			windowFrame: windowFrame,
			bounds: bounds,
			in: context
		)
		drawFrozenSpotlights(
			selection: selection,
			chrome: chrome,
			windowFrame: windowFrame,
			bounds: bounds,
			in: context
		)
		drawFrozenPenStrokes(
			chrome: chrome,
			windowFrame: windowFrame,
			bounds: bounds,
			in: context
		)
		drawFrozenArrows(
			chrome: chrome,
			windowFrame: windowFrame,
			bounds: bounds,
			in: context
		)
		drawFrozenTextAnnotations(
			chrome: chrome,
			windowFrame: windowFrame,
			bounds: bounds,
			in: context
		)
	}

	private static func drawFrozenMosaics(
		selection: CGRect,
		chrome: CaptureChromeState,
		windowFrame: CGRect,
		bounds: CGRect,
		in context: CGContext
	) {
		let mosaicRects = chrome.frozenOverlay.mosaicRects.compactMap {
			localRect(from: $0, windowFrame: windowFrame, bounds: bounds)
		}
		let previewRect = chrome.frozenOverlay.previewMosaicRect.flatMap {
			localRect(from: $0, windowFrame: windowFrame, bounds: bounds)
		}
		let allRects = mosaicRects + (previewRect.map { [$0] } ?? [])
		guard allRects.isEmpty == false, let baseImage = chrome.frozenBaseImage else {
			return
		}
		let imageSize = CGSize(width: CGFloat(baseImage.width), height: CGFloat(baseImage.height))

		context.saveGState()
		context.interpolationQuality = .none
		for rect in allRects {
			let imageRect = CGRect(
				x: ((rect.minX - selection.minX) / max(selection.width, 1))
					* imageSize.width,
				y: ((selection.maxY - rect.maxY) / max(selection.height, 1))
					* imageSize.height,
				width: (rect.width / max(selection.width, 1)) * imageSize.width,
				height: (rect.height / max(selection.height, 1)) * imageSize.height
			)
			guard let patch = makeFrozenMosaicPatch(from: baseImage, sourceRect: imageRect)
			else {
				continue
			}
			context.draw(patch, in: rect)
		}
		context.restoreGState()
	}

	private static func drawFrozenSpotlights(
		selection: CGRect,
		chrome: CaptureChromeState,
		windowFrame: CGRect,
		bounds: CGRect,
		in context: CGContext
	) {
		let spotlightAnnotations: [(rect: CGRect, style: FrozenSpotlightStyle)] =
			chrome.frozenOverlay.spotlightAnnotations.compactMap { annotation in
				guard
					let rect = localRect(
						from: annotation.rect, windowFrame: windowFrame, bounds: bounds)
				else {
					return nil
				}
				return (rect: rect, style: annotation.style)
			}
		let previewAnnotation =
			chrome.frozenOverlay.previewSpotlightAnnotation.flatMap { annotation in
				localRect(from: annotation.rect, windowFrame: windowFrame, bounds: bounds).map {
					rect in
					(rect: rect, style: annotation.style)
				}
			}
		let allAnnotations = spotlightAnnotations + (previewAnnotation.map { [$0] } ?? [])
		guard allAnnotations.isEmpty == false else {
			return
		}

		context.saveGState()
		context.setFillColor(NSColor.black.withAlphaComponent(0.32).cgColor)
		context.fill(selection)
		context.setBlendMode(.clear)
		for annotation in allAnnotations {
			context.fill(annotation.rect)
		}
		context.restoreGState()

		for annotation in allAnnotations {
			drawFrozenSpotlightBorder(
				for: annotation.rect,
				style: annotation.style,
				scale: 1,
				alpha: 0.92,
				in: context
			)
		}
	}

	private static func drawFrozenPenStrokes(
		chrome: CaptureChromeState,
		windowFrame: CGRect,
		bounds: CGRect,
		in context: CGContext
	) {
		let allStrokes =
			chrome.frozenOverlay.penStrokes
			+ (chrome.frozenOverlay.previewPenStroke.map { [$0] } ?? [])
		guard allStrokes.isEmpty == false else {
			return
		}

		context.saveGState()
		context.setLineCap(.round)
		context.setLineJoin(.round)
		for stroke in allStrokes {
			guard
				let first = stroke.points.first.flatMap({
					localPoint(from: $0, windowFrame: windowFrame, bounds: bounds)
				})
			else {
				continue
			}
			context.setStrokeColor(stroke.style.color.nsColor(alpha: 0.96).cgColor)
			context.setLineWidth(stroke.style.strokeWidthPoints)
			context.beginPath()
			context.move(to: first)
			for point in stroke.points.dropFirst() {
				guard
					let localPoint = localPoint(
						from: point, windowFrame: windowFrame, bounds: bounds)
				else {
					continue
				}
				context.addLine(to: localPoint)
			}
			context.strokePath()
		}
		context.restoreGState()
	}

	private static func drawFrozenArrows(
		chrome: CaptureChromeState,
		windowFrame: CGRect,
		bounds: CGRect,
		in context: CGContext
	) {
		let arrows =
			chrome.frozenOverlay.arrowAnnotations
			+ (chrome.frozenOverlay.previewArrow.map { [$0] } ?? [])
		guard arrows.isEmpty == false else {
			return
		}

		for annotation in arrows {
			guard
				let localStart = localPoint(
					from: annotation.start, windowFrame: windowFrame, bounds: bounds),
				let localEnd = localPoint(
					from: annotation.end, windowFrame: windowFrame, bounds: bounds)
			else {
				continue
			}
			drawFrozenArrow(
				from: localStart,
				to: localEnd,
				style: annotation.style,
				scale: 1,
				in: context
			)
		}
	}

	private static func drawFrozenTextAnnotations(
		chrome: CaptureChromeState,
		windowFrame: CGRect,
		bounds: CGRect,
		in context: CGContext
	) {
		for annotation in chrome.frozenOverlay.textAnnotations {
			guard
				let point = localPoint(
					from: annotation.anchor, windowFrame: windowFrame, bounds: bounds)
			else {
				continue
			}
			drawFrozenText(
				annotation.text, at: point, style: annotation.style, scale: 1, in: context)
		}
		if let previewText = chrome.frozenOverlay.previewTextAnnotation,
			let point = localPoint(
				from: previewText.anchor, windowFrame: windowFrame, bounds: bounds)
		{
			drawFrozenText(
				previewText.text, at: point, style: previewText.style, scale: 1, in: context)
		}
		if let activeTextEdit = chrome.frozenOverlay.activeTextEdit,
			let point = localPoint(
				from: activeTextEdit.anchor, windowFrame: windowFrame, bounds: bounds)
		{
			drawFrozenText(
				activeTextEdit.text + "│",
				at: point,
				style: chrome.annotationStyle.textStyle,
				scale: 1,
				in: context
			)
		}
	}

	private static func drawFrozenText(
		_ text: String,
		at point: CGPoint,
		style: FrozenTextStyle,
		scale: CGFloat,
		in context: CGContext
	) {
		guard text.isEmpty == false else {
			return
		}

		let font = NSFont.systemFont(ofSize: max(1, style.fontSizePoints * scale), weight: .medium)
		let attributes: [NSAttributedString.Key: Any] = [
			.font: font,
			.foregroundColor: style.color.nsColor(),
		]
		let attributed = NSAttributedString(string: text, attributes: attributes)
		context.saveGState()
		context.setShadow(
			offset: CGSize(width: 0, height: 1), blur: 4,
			color: style.color.textShadowColor.cgColor)
		let graphicsContext = NSGraphicsContext(cgContext: context, flipped: false)
		NSGraphicsContext.saveGraphicsState()
		NSGraphicsContext.current = graphicsContext
		attributed.draw(at: point)
		NSGraphicsContext.restoreGraphicsState()
		context.restoreGState()
	}

	private static func localRect(
		from globalRect: CGRect,
		windowFrame: CGRect,
		bounds: CGRect
	) -> CGRect? {
		let localRect = CGRect(
			x: globalRect.minX - windowFrame.minX,
			y: globalRect.minY - windowFrame.minY,
			width: globalRect.width,
			height: globalRect.height
		)
		return localRect.intersects(bounds) ? localRect : nil
	}

	private static func localPoint(
		from globalPoint: CGPoint,
		windowFrame: CGRect,
		bounds: CGRect
	) -> CGPoint? {
		captureOverlayLocalPoint(
			from: globalPoint,
			windowFrame: windowFrame,
			bounds: bounds
		)
	}
}
