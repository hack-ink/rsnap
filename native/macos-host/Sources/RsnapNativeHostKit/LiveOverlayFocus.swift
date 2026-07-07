import AppKit
import CoreGraphics
import QuartzCore

extension LiveOverlayRenderer {
	func renderFrozenDisplay(_ snapshot: LivePreviewSnapshot) {
		guard let image = snapshot.frozenDisplayImage, let frame = snapshot.frozenDisplayFrame
		else {
			frozenDisplayLayer.isHidden = true
			frozenDisplayLayer.contents = nil
			return
		}
		frozenDisplayLayer.contentsGravity = .resize
		frozenDisplayLayer.contentsScale = hostView?.window?.screen?.backingScaleFactor ?? 2
		frozenDisplayLayer.frame = frame
		frozenDisplayLayer.contents = image
		frozenDisplayLayer.isHidden = false
	}

	func renderFocus(_ snapshot: LivePreviewSnapshot) {
		let focusRect = snapshot.dragSelectionLocal ?? snapshot.hoverSelectionLocal
		guard let focusRect else {
			hideFocusLayers()
			return
		}

		let scrimAlpha = CGFloat(CaptureChrome.liveScrimAlpha)
		let scrimColor = NSColor(calibratedWhite: 0, alpha: scrimAlpha).cgColor
		let bounds = snapshot.bounds
		let chromeExclusions = liveChromeRoundedExclusions(for: snapshot)
		hideLegacyScrimLayers()
		updateScrimLayer(
			bounds: bounds,
			focusRect: focusRect,
			color: scrimColor,
			excluding: chromeExclusions
		)

		if snapshot.frozenPending {
			renderFrozenPendingFocus(focusRect)
			return
		}

		if let dragSelection = snapshot.dragSelectionLocal {
			renderDragSelectionFocus(dragSelection, snapshot: snapshot, bounds: bounds)
			return
		}

		renderHoverFocus(focusRect, snapshot: snapshot, chromeExclusions: chromeExclusions)
	}

	private func hideFocusLayers() {
		scrimLayer.isHidden = true
		hideLegacyScrimLayers()
		hoverGlowLayer.isHidden = true
		hoverFlowLayer.hide()
		dragBorderOutlineLayer.isHidden = true
		dragBorderLayer.isHidden = true
		selectionSizeLayer.isHidden = true
	}

	private func hideLegacyScrimLayers() {
		for scrimLayer in [topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer] {
			scrimLayer.isHidden = true
		}
	}

	private func renderFrozenPendingFocus(_ focusRect: CGRect) {
		hoverGlowLayer.isHidden = true
		hoverFlowLayer.hide()
		dragBorderOutlineLayer.isHidden = false
		dragBorderLayer.isHidden = false
		selectionSizeLayer.isHidden = true
		let pixelsPerPoint = hostView?.window?.screen?.backingScaleFactor ?? 1
		let borderOutset = CaptureChrome.dashedBorderOutset(
			strokeWidth: CaptureChrome.frozenDashedBorderWidth,
			pixelsPerPoint: pixelsPerPoint
		)
		let borderRect = focusRect.insetBy(dx: -borderOutset, dy: -borderOutset)
		let layerFrame = dashedBorderLayerFrame(
			for: borderRect,
			lineWidth: CaptureChrome.frozenDashedBorderWidth + 0.75
		)
		let localBorderRect = borderRect.offsetBy(dx: -layerFrame.minX, dy: -layerFrame.minY)
		let frozenPath = CaptureChrome.dashedBorderPath(for: localBorderRect)
		for layer in [dragBorderOutlineLayer, dragBorderLayer] {
			layer.frame = layerFrame
			layer.masksToBounds = true
		}
		dragBorderOutlineLayer.path = frozenPath
		dragBorderOutlineLayer.strokeColor =
			NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
			.cgColor
		dragBorderOutlineLayer.lineWidth = CaptureChrome.frozenDashedBorderWidth + 0.75
		dragBorderOutlineLayer.lineCap = .butt
		dragBorderOutlineLayer.lineJoin = .miter
		dragBorderLayer.path = frozenPath
		dragBorderLayer.strokeColor =
			NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 248 / 255)
			.cgColor
		dragBorderLayer.lineWidth = CaptureChrome.frozenDashedBorderWidth
		dragBorderLayer.lineCap = .butt
		dragBorderLayer.lineJoin = .miter
	}

	private func renderDragSelectionFocus(
		_ dragSelection: CGRect,
		snapshot: LivePreviewSnapshot,
		bounds: CGRect
	) {
		hoverGlowLayer.isHidden = true
		hoverFlowLayer.hide()
		dragBorderOutlineLayer.isHidden = false
		dragBorderLayer.isHidden = false
		let pixelsPerPoint = hostView?.window?.screen?.backingScaleFactor ?? 1
		let borderOutset = CaptureChrome.dashedBorderOutset(
			strokeWidth: CaptureChrome.liveDashedBorderWidth,
			pixelsPerPoint: pixelsPerPoint
		)
		let borderRect = dragSelection.insetBy(dx: -borderOutset, dy: -borderOutset)
		let layerFrame = dashedBorderLayerFrame(
			for: borderRect,
			lineWidth: CaptureChrome.liveDashedBorderWidth + 0.75
		)
		let localBorderRect = borderRect.offsetBy(dx: -layerFrame.minX, dy: -layerFrame.minY)
		let dragPath = CaptureChrome.dashedBorderPath(for: localBorderRect)
		for layer in [dragBorderOutlineLayer, dragBorderLayer] {
			layer.frame = layerFrame
			layer.masksToBounds = true
		}
		dragBorderOutlineLayer.path = dragPath
		dragBorderOutlineLayer.strokeColor =
			NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
			.cgColor
		dragBorderOutlineLayer.lineWidth = CaptureChrome.liveDashedBorderWidth + 0.75
		dragBorderOutlineLayer.lineCap = .butt
		dragBorderOutlineLayer.lineJoin = .miter
		dragBorderLayer.path = dragPath
		dragBorderLayer.strokeColor =
			NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor
		dragBorderLayer.lineWidth = CaptureChrome.liveDashedBorderWidth
		dragBorderLayer.lineCap = .butt
		dragBorderLayer.lineJoin = .miter
		renderSelectionSizeBadge(
			snapshot.selectionSizeText, selection: dragSelection, bounds: bounds)
	}

	private func renderSelectionSizeBadge(
		_ selectionSizeText: String?,
		selection: CGRect,
		bounds: CGRect
	) {
		guard let selectionSizeText else {
			selectionSizeLayer.isHidden = true
			return
		}
		let font = LiveOverlayTypography.font
		let textSize = selectionSizeText.size(using: font)
		let frame = CaptureChrome.selectionSizeBadgeFrame(
			for: selection,
			textSize: textSize,
			in: bounds
		)
		applyText(
			selectionSizeLayer,
			text: selectionSizeText,
			font: font,
			color: NSColor.white.withAlphaComponent(0.98),
			frame: frame,
			alignment: .left
		)
		selectionSizeLayer.isHidden = false
	}

	private func renderHoverFocus(
		_ focusRect: CGRect,
		snapshot: LivePreviewSnapshot,
		chromeExclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		dragBorderOutlineLayer.isHidden = true
		dragBorderLayer.isHidden = true
		selectionSizeLayer.isHidden = true
		let hoverPath = NSBezierPath(
			roundedRect: focusRect,
			xRadius: CaptureChrome.liveSelectionCornerRadius,
			yRadius: CaptureChrome.liveSelectionCornerRadius
		).cgPath
		hoverGlowLayer.path = hoverPath
		hoverGlowLayer.isHidden = true
		let contentsScale = hostView?.window?.screen?.backingScaleFactor ?? 2
		let animatesFlow = shouldAnimateSelectionFlow(snapshot)
		let flowFrame = flowLayerFrame(for: focusRect, scale: contentsScale)
		hoverFlowLayer.update(
			frame: flowFrame,
			focusRect: focusRect.offsetBy(dx: -flowFrame.minX, dy: -flowFrame.minY),
			theme: snapshot.theme,
			timestamp: CACurrentMediaTime(),
			contentsScale: contentsScale,
			animates: animatesFlow,
			roundedExclusions: chromeExclusions
		)
	}

	private func dashedBorderLayerFrame(for borderRect: CGRect, lineWidth: CGFloat) -> CGRect {
		let padding = max(lineWidth + 2, 4)
		return borderRect.insetBy(dx: -padding, dy: -padding)
	}

	private func updateScrimLayer(
		bounds: CGRect,
		focusRect: CGRect,
		color: CGColor,
		excluding roundedExclusions: [OverlayMaskGeometry.RoundedExclusion] = []
	) {
		let effectiveExclusions = Self.visibleScrimExclusions(
			roundedExclusions,
			bounds: bounds,
			focusRect: focusRect
		)
		scrimLayer.frame = bounds
		scrimLayer.contentsScale = hostView?.window?.screen?.backingScaleFactor ?? 2
		scrimLayer.update(
			focusRect: focusRect,
			color: color,
			roundedExclusions: effectiveExclusions
		)
		scrimLayer.isHidden = false
	}

	private static func visibleScrimExclusions(
		_ roundedExclusions: [OverlayMaskGeometry.RoundedExclusion],
		bounds: CGRect,
		focusRect: CGRect
	) -> [OverlayMaskGeometry.RoundedExclusion] {
		roundedExclusions.compactMap { exclusion in
			let visibleRect = exclusion.rect.intersection(bounds)
			guard visibleRect.isNull == false, visibleRect.width > 0, visibleRect.height > 0,
				focusRect.contains(visibleRect) == false
			else {
				return nil
			}
			return OverlayMaskGeometry.RoundedExclusion(
				rect: visibleRect,
				cornerRadius: exclusion.cornerRadius
			)
		}
	}

	func liveChromeRoundedExclusions(
		for snapshot: LivePreviewSnapshot
	) -> [OverlayMaskGeometry.RoundedExclusion] {
		guard snapshot.settings.hudGlassEnabled else {
			return []
		}
		return [snapshot.hudFrame, snapshot.loupeFrame].compactMap { frame in
			frame.map {
				OverlayMaskGeometry.RoundedExclusion(
					rect: $0,
					cornerRadius: CaptureChrome.hudCornerRadius
				)
			}
		}
	}

	func updateLiveScrimExclusions(
		excluding exclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		guard scrimLayer.isHidden == false, let focusRect = lastRenderedFocusRect else {
			return
		}
		updateScrimLayer(
			bounds: rootLayer.bounds,
			focusRect: focusRect,
			color: scrimLayer.scrimColor,
			excluding: exclusions
		)
	}

	func updateLiveFlowExclusions(
		excluding exclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		guard hoverFlowLayer.isHidden == false else {
			return
		}
		hoverFlowLayer.updateRoundedExclusions(exclusions)
	}

	func shouldAnimateSelectionFlow(_ snapshot: LivePreviewSnapshot) -> Bool {
		guard snapshot.dragSelectionLocal == nil, snapshot.hoverSelectionLocal != nil,
			!snapshot.frozenPending
		else {
			return false
		}
		return true
	}

	private func flowLayerFrame(for focusRect: CGRect, scale: CGFloat) -> CGRect {
		let outset: CGFloat = 24
		let expanded = focusRect.insetBy(dx: -outset, dy: -outset)
		let safeScale = max(scale, 1)
		return CGRect(
			x: floor(expanded.minX * safeScale) / safeScale,
			y: floor(expanded.minY * safeScale) / safeScale,
			width: ceil(expanded.width * safeScale) / safeScale,
			height: ceil(expanded.height * safeScale) / safeScale
		)
	}
}
