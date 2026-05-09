import AppKit
import CoreGraphics
import Foundation

enum CaptureChromeTheme: Equatable {
	case dark
	case light
}

struct CaptureChromePalette {
	let foregrounds: CaptureChromeForegroundPalette
	let bodyFill: NSColor
	let outerStroke: NSColor
	let shadow: NSColor
	let swatchStroke: NSColor
	let keycapFill: NSColor
	let keycapStroke: NSColor
	let toolbarHoverBackground: NSColor
	let toolbarSelectedBackground: NSColor

	var labelText: NSColor { foregrounds.primary }
	var secondaryText: NSColor { foregrounds.secondary }
	var keycapText: NSColor { foregrounds.secondary }
	var toolbarIcon: NSColor { foregrounds.control }
	var toolbarHoverIcon: NSColor { foregrounds.controlHover }
	var toolbarSelectedIcon: NSColor { foregrounds.controlSelected }
	var toolbarDisabledIcon: NSColor { foregrounds.controlDisabled }
}

struct CaptureChromeForegroundPalette {
	let primary: NSColor
	let secondary: NSColor
	let control: NSColor
	let controlHover: NSColor
	let controlSelected: NSColor
	let controlDisabled: NSColor
}

enum CaptureChrome {
	struct ToolbarMetrics {
		let scale: CGFloat
		let buttonSize: CGFloat
		let itemSpacing: CGFloat
		let horizontalPadding: CGFloat
		let verticalPadding: CGFloat
		let gap: CGFloat
		let annotationStyleRowHeight: CGFloat
		let annotationStyleControlGap: CGFloat
		let annotationSizeButtonWidth: CGFloat
		let annotationSwatchSize: CGFloat
		let annotationSwatchGap: CGFloat
	}

	private static let liquidGlassBodyOpacity: CGFloat = 0.5

	static let hudInnerMarginX: CGFloat = 12
	static let hudInnerMarginY: CGFloat = 8
	static let hudGroupSpacing: CGFloat = 12
	static let hudColorItemSpacing: CGFloat = 6
	static let hudSwatchSize = CGSize(width: 10, height: 10)
	static let hudCornerRadius: CGFloat = 18
	static let hudLoupeGap: CGFloat = 8
	static let loupeCellSize: CGFloat = 10
	static let liveScrimAlpha: CGFloat = 176.0 / 255.0
	static let frozenScrimAlpha: CGFloat = 176.0 / 255.0
	static let liveDashedBorderWidth: CGFloat = 1.55
	static let frozenDashedBorderWidth: CGFloat = 1.55
	static let dashedBorderDashLength: CGFloat = 8.0
	static let dashedBorderGapLength: CGFloat = 4.2
	static let selectionCornerRadius: CGFloat = 18
	static let liveSelectionCornerRadius: CGFloat = 20
	static let frozenSelectionMinimumSize: CGFloat = 1
	static let resizeHandleHitSize: CGFloat = 24
	static let resizeHandleStrokeWidth: CGFloat = 1.3
	static let resizeHandleLegLength: CGFloat = 8
	static let resizeHandleOffset: CGFloat = 2.5
	static let toolbarButtonSize: CGFloat = 24
	static let toolbarItemSpacing: CGFloat = 4
	static let toolbarVerticalPadding: CGFloat = 5
	static let toolbarGlyphSize: CGFloat = 18
	static let toolbarControlFontSize: CGFloat = 13
	static let toolbarControlCornerRadius: CGFloat = 8
	// Keep the toolbar visually closer to the slim live HUD chrome.
	static let toolbarTargetHeight: CGFloat = 30
	static let toolbarGap: CGFloat = 10
	static let toolbarScreenMargin: CGFloat = 10
	static let scrollMinimapPreferredWidth: CGFloat = 96
	static let scrollMinimapMinimumWidth: CGFloat = 44
	static let scrollMinimapGap: CGFloat = 10
	static let scrollMinimapScreenMargin: CGFloat = 10
	static let scrollMinimapImageInset: CGFloat = 3
	static let scrollMinimapCornerRadius: CGFloat = 9
	static let annotationStyleRowHeight: CGFloat = 24
	static let annotationStyleControlGap: CGFloat = 4
	static let annotationSizeButtonWidth: CGFloat = 20
	static let annotationSwatchSize: CGFloat = 16
	static let annotationSwatchGap: CGFloat = 6
	static let annotationPenPreviewLength: CGFloat = 18
	static let annotationSizePreviewGap: CGFloat = 8
	static let selectionSizeBadgeGap: CGFloat = 8
	static let selectionSizeBadgeInset: CGFloat = 8
	static let selectionSizeBadgeToolbarAvoidance: CGFloat = 4

	static func toolbarMetrics() -> ToolbarMetrics {
		let baseHeight =
			toolbarVerticalPadding * 2
			+ toolbarButtonSize
		let targetHeight = toolbarTargetHeight
		let scale = min(1, targetHeight / max(baseHeight, 1))
		return ToolbarMetrics(
			scale: scale,
			buttonSize: toolbarButtonSize * scale,
			itemSpacing: toolbarItemSpacing * scale,
			horizontalPadding: hudInnerMarginX * scale,
			verticalPadding: toolbarVerticalPadding * scale,
			gap: toolbarGap * scale,
			annotationStyleRowHeight: annotationStyleRowHeight * scale,
			annotationStyleControlGap: annotationStyleControlGap * scale,
			annotationSizeButtonWidth: annotationSizeButtonWidth * scale,
			annotationSwatchSize: annotationSwatchSize * scale,
			annotationSwatchGap: annotationSwatchGap * scale
		)
	}

	static func dashedBorderOutset(strokeWidth: CGFloat, pixelsPerPoint: CGFloat) -> CGFloat {
		let feathering = 1.0 / max(pixelsPerPoint, .leastNonzeroMagnitude)
		return (strokeWidth + feathering) * 0.5
	}

	static func selectionSizeBadgeFrame(
		for selection: CGRect,
		textSize: CGSize,
		in bounds: CGRect,
		avoiding toolbarFrame: CGRect? = nil
	) -> CGRect {
		let size = CGSize(width: ceil(textSize.width), height: ceil(textSize.height))
		let bottomOutside = CGRect(
			x: selection.maxX - size.width,
			y: selection.minY - selectionSizeBadgeGap - size.height,
			width: size.width,
			height: size.height
		)
		if fitsSelectionSizeBadge(bottomOutside, in: bounds),
			!selectionSizeBadge(bottomOutside, conflictsWith: toolbarFrame)
		{
			return bottomOutside
		}

		if selectionSizeBadge(bottomOutside, conflictsWith: toolbarFrame) {
			let topOutside = CGRect(
				x: selection.maxX - size.width,
				y: selection.maxY + selectionSizeBadgeGap,
				width: size.width,
				height: size.height
			)
			if fitsSelectionSizeBadge(topOutside, in: bounds),
				!selectionSizeBadge(topOutside, conflictsWith: toolbarFrame)
			{
				return topOutside
			}
		}

		return selectionSizeBadgeInsideBottomRight(
			selection: selection,
			size: size,
			bounds: bounds
		)
	}

	private static func fitsSelectionSizeBadge(_ frame: CGRect, in bounds: CGRect) -> Bool {
		frame.minX >= bounds.minX + selectionSizeBadgeGap
			&& frame.maxX <= bounds.maxX - selectionSizeBadgeGap
			&& frame.minY >= bounds.minY + selectionSizeBadgeGap
			&& frame.maxY <= bounds.maxY - selectionSizeBadgeGap
	}

	private static func selectionSizeBadge(
		_ frame: CGRect,
		conflictsWith toolbarFrame: CGRect?
	) -> Bool {
		guard let toolbarFrame else {
			return false
		}
		return frame.insetBy(
			dx: -selectionSizeBadgeToolbarAvoidance,
			dy: -selectionSizeBadgeToolbarAvoidance
		).intersects(toolbarFrame)
	}

	private static func selectionSizeBadgeInsideBottomRight(
		selection: CGRect,
		size: CGSize,
		bounds: CGRect
	) -> CGRect {
		let minX = bounds.minX + selectionSizeBadgeGap
		let maxX = max(minX, bounds.maxX - selectionSizeBadgeGap - size.width)
		let minY = bounds.minY + selectionSizeBadgeGap
		let maxY = max(minY, bounds.maxY - selectionSizeBadgeGap - size.height)
		let targetX = min(
			selection.maxX - selectionSizeBadgeInset - size.width,
			bounds.maxX - selectionSizeBadgeGap - size.width)
		let targetY = max(
			selection.minY + selectionSizeBadgeInset, bounds.minY + selectionSizeBadgeGap)
		return CGRect(
			x: targetX.clamped(to: minX...maxX),
			y: targetY.clamped(to: minY...maxY),
			width: size.width,
			height: size.height
		)
	}

	static func dashedBorderPath(
		for rect: CGRect,
		dashLength: CGFloat = dashedBorderDashLength,
		gapLength: CGFloat = dashedBorderGapLength,
		cornerKeepout: CGFloat = 0
	) -> CGPath {
		let path = CGMutablePath()
		for (start, end) in dashedBorderSegments(
			for: rect,
			dashLength: dashLength,
			gapLength: gapLength,
			cornerKeepout: cornerKeepout
		) {
			path.move(to: start)
			path.addLine(to: end)
		}
		return path
	}

	private static func dashedBorderSegments(
		for rect: CGRect,
		dashLength: CGFloat,
		gapLength: CGFloat,
		cornerKeepout: CGFloat
	) -> [(CGPoint, CGPoint)] {
		if cornerKeepout > 0 {
			let horizontalRanges = dashedBorderEdgeRanges(
				edgeLength: rect.width,
				cornerKeepout: cornerKeepout,
				dashLength: dashLength,
				gapLength: gapLength
			)
			let verticalRanges = dashedBorderEdgeRanges(
				edgeLength: rect.height,
				cornerKeepout: cornerKeepout,
				dashLength: dashLength,
				gapLength: gapLength
			)
			var segments: [(CGPoint, CGPoint)] = []
			for (start, end) in horizontalRanges {
				segments.append(
					(
						CGPoint(x: rect.minX + start, y: rect.minY),
						CGPoint(x: rect.minX + end, y: rect.minY)
					))
			}
			for (start, end) in verticalRanges {
				segments.append(
					(
						CGPoint(x: rect.maxX, y: rect.minY + start),
						CGPoint(x: rect.maxX, y: rect.minY + end)
					))
			}
			for (start, end) in horizontalRanges {
				segments.append(
					(
						CGPoint(x: rect.minX + start, y: rect.maxY),
						CGPoint(x: rect.minX + end, y: rect.maxY)
					))
			}
			for (start, end) in verticalRanges {
				segments.append(
					(
						CGPoint(x: rect.minX, y: rect.minY + start),
						CGPoint(x: rect.minX, y: rect.minY + end)
					))
			}
			return segments
		}

		let perimeter = dashedBorderPerimeter(for: rect)
		guard perimeter > 0 else {
			return []
		}

		var segments: [(CGPoint, CGPoint)] = []
		for (dashStart, dashEnd) in dashedBorderDashRanges(
			perimeter: perimeter,
			dashLength: dashLength,
			gapLength: gapLength
		) {
			appendDashedBorderSegments(
				for: rect,
				dashStart: dashStart,
				dashEnd: dashEnd,
				into: &segments
			)
		}
		return segments
	}

	private static func dashedBorderEdgeRanges(
		edgeLength: CGFloat,
		cornerKeepout: CGFloat,
		dashLength: CGFloat,
		gapLength: CGFloat
	) -> [(CGFloat, CGFloat)] {
		let usableLength = edgeLength - cornerKeepout * 2
		guard usableLength > 0 else {
			return []
		}
		if usableLength <= dashLength {
			return [(cornerKeepout, edgeLength - cornerKeepout)]
		}

		let clampedDashLength = min(dashLength, usableLength)
		let cycleSpan = max(dashLength + gapLength, .leastNonzeroMagnitude)
		let dashCount = max(Int(floor((usableLength + gapLength) / cycleSpan)), 1)
		if dashCount == 1 {
			return [(cornerKeepout, edgeLength - cornerKeepout)]
		}

		let occupiedLength =
			CGFloat(dashCount) * clampedDashLength + CGFloat(dashCount - 1) * gapLength
		let gapCount = max(dashCount - 1, 0)
		let resolvedGapLength: CGFloat =
			if gapCount == 0 {
				gapLength
			} else {
				gapLength + max(usableLength - occupiedLength, 0) / CGFloat(gapCount)
			}

		return (0..<dashCount).map { index in
			let start = cornerKeepout + CGFloat(index) * (clampedDashLength + resolvedGapLength)
			return (start, start + clampedDashLength)
		}
	}

	private static func dashedBorderDashRanges(
		perimeter: CGFloat,
		dashLength: CGFloat,
		gapLength: CGFloat
	) -> [(CGFloat, CGFloat)] {
		guard perimeter > 0 else {
			return []
		}
		let targetCycle = max(dashLength + gapLength, .leastNonzeroMagnitude)
		let cycleCount = max(Int((perimeter / targetCycle).rounded()), 1)
		let cycleSpan = perimeter / CGFloat(cycleCount)
		let resolvedDashLength = min(dashLength, cycleSpan)

		return (0..<cycleCount).map { index in
			let start = CGFloat(index) * cycleSpan
			return (start, start + resolvedDashLength)
		}
	}

	private static func appendDashedBorderSegments(
		for rect: CGRect,
		dashStart: CGFloat,
		dashEnd: CGFloat,
		into segments: inout [(CGPoint, CGPoint)]
	) {
		var segmentStart = dashStart
		for cornerDistance in dashedBorderCornerDistances(for: rect) {
			if segmentStart >= dashEnd {
				break
			}
			if cornerDistance <= segmentStart || cornerDistance >= dashEnd {
				continue
			}
			pushDashedBorderSegment(
				for: rect, start: segmentStart, end: cornerDistance, into: &segments)
			segmentStart = cornerDistance
		}
		if segmentStart < dashEnd {
			pushDashedBorderSegment(for: rect, start: segmentStart, end: dashEnd, into: &segments)
		}
	}

	private static func pushDashedBorderSegment(
		for rect: CGRect,
		start: CGFloat,
		end: CGFloat,
		into segments: inout [(CGPoint, CGPoint)]
	) {
		let startPoint = dashedBorderPoint(for: rect, distance: start)
		let endPoint = dashedBorderPoint(for: rect, distance: end)
		guard startPoint != endPoint else {
			return
		}
		segments.append((startPoint, endPoint))
	}

	private static func dashedBorderPoint(for rect: CGRect, distance: CGFloat) -> CGPoint {
		let width = rect.width
		let height = rect.height
		let perimeter = dashedBorderPerimeter(for: rect)
		let normalizedDistance = distance.truncatingRemainder(dividingBy: perimeter)
		let resolvedDistance =
			normalizedDistance < 0 ? normalizedDistance + perimeter : normalizedDistance

		if resolvedDistance < width {
			return CGPoint(x: rect.minX + resolvedDistance, y: rect.minY)
		}
		if resolvedDistance < width + height {
			return CGPoint(x: rect.maxX, y: rect.minY + (resolvedDistance - width))
		}
		if resolvedDistance < width * 2 + height {
			return CGPoint(x: rect.maxX - (resolvedDistance - width - height), y: rect.maxY)
		}
		return CGPoint(x: rect.minX, y: rect.maxY - (resolvedDistance - width * 2 - height))
	}

	private static func dashedBorderCornerDistances(for rect: CGRect) -> [CGFloat] {
		let width = rect.width
		let height = rect.height
		return [width, width + height, width * 2 + height, dashedBorderPerimeter(for: rect)]
	}

	private static func dashedBorderPerimeter(for rect: CGRect) -> CGFloat {
		guard rect.width > 0, rect.height > 0 else {
			return 0
		}
		return (rect.width + rect.height) * 2
	}

	static func palette(for theme: CaptureChromeTheme, settings: NativeHostSettings)
		-> CaptureChromePalette
	{
		let opacity = effectiveHudOpacity(settings: settings)
		let tint = CGFloat(settings.hudTint.clamped(to: 0...1))
		let foregrounds = foregroundPalette(for: theme)
		let bodyAlphaFloor: CGFloat = theme == .dark ? 0.06 : 0.08
		let fillOpacity: CGFloat =
			settings.hudGlassEnabled
			? max(bodyAlphaFloor, opacity * 0.20)
			: opacity
		let tintColor = glassTintColor(for: theme, settings: settings)

		switch theme {
		case .dark:
			let baseFill = NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 1)
			let bodyFill =
				baseFill
				.mixed(with: tintColor, fraction: tint * 0.72)
				.withAlphaComponent(fillOpacity)
			return CaptureChromePalette(
				foregrounds: foregrounds,
				bodyFill: bodyFill,
				outerStroke: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.12, 0.14 + opacity * 0.10)),
				shadow: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.16, 0.12 + opacity * 0.18)),
				swatchStroke: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 36 / 255),
				keycapFill: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.06, opacity * 0.18)),
				keycapStroke: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.10, opacity * 0.22)),
				toolbarHoverBackground: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.08, opacity * 0.18)),
				toolbarSelectedBackground: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.12, opacity * 0.24))
			)
		case .light:
			let baseFill = NSColor(srgbRed: 232 / 255, green: 236 / 255, blue: 243 / 255, alpha: 1)
			let bodyFill =
				baseFill
				.mixed(with: tintColor, fraction: tint * 0.62)
				.withAlphaComponent(fillOpacity)
			return CaptureChromePalette(
				foregrounds: foregrounds,
				bodyFill: bodyFill,
				outerStroke: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.12, 0.16 + opacity * 0.12)),
				shadow: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.10, 0.06 + opacity * 0.14)),
				swatchStroke: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: 44 / 255),
				keycapFill: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.05, opacity * 0.12)),
				keycapStroke: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.10, opacity * 0.20)),
				toolbarHoverBackground: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.08, opacity * 0.16)),
				toolbarSelectedBackground: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.10, opacity * 0.22))
			)
		}
	}

	private static func foregroundPalette(for theme: CaptureChromeTheme)
		-> CaptureChromeForegroundPalette
	{
		switch theme {
		case .dark:
			let primary = NSColor(
				srgbRed: 235 / 255, green: 235 / 255, blue: 245 / 255, alpha: 235 / 255)
			let secondary = NSColor(
				srgbRed: 235 / 255, green: 235 / 255, blue: 245 / 255, alpha: 150 / 255)
			let controlBase = NSColor.white
			return CaptureChromeForegroundPalette(
				primary: primary,
				secondary: secondary,
				control: controlBase.withAlphaComponent(160 / 255),
				controlHover: controlBase.withAlphaComponent(222 / 255),
				controlSelected: controlBase,
				controlDisabled: controlBase.withAlphaComponent(72 / 255)
			)
		case .light:
			let primary = NSColor(
				srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 235 / 255)
			let secondary = NSColor(
				srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 160 / 255)
			let controlBase = NSColor.black
			return CaptureChromeForegroundPalette(
				primary: primary,
				secondary: secondary,
				control: controlBase.withAlphaComponent(182 / 255),
				controlHover: controlBase.withAlphaComponent(220 / 255),
				controlSelected: controlBase,
				controlDisabled: controlBase.withAlphaComponent(82 / 255)
			)
		}
	}

	static func glassOpacity(settings: NativeHostSettings) -> Float {
		Float(0.88 + settings.hudBlur.clamped(to: 0...1) * 0.12)
	}

	static func effectiveHudOpacity(settings: NativeHostSettings) -> CGFloat {
		if settings.usesLiquidHudGlass {
			return liquidGlassBodyOpacity
		}
		return CGFloat(settings.hudOpacity.clamped(to: 0...1))
	}

	static func effectiveBodyFill(
		palette: CaptureChromePalette,
		settings: NativeHostSettings,
		hasGlass: Bool
	) -> NSColor {
		let opacity = effectiveHudOpacity(settings: settings)
		if hasGlass {
			return palette.bodyFill.withAlphaComponent(
				max(palette.bodyFill.alphaComponent, max(0.22, opacity * 0.42)))
		}
		return palette.bodyFill.withAlphaComponent(max(0.42, opacity * 0.82))
	}

	private static func glassTintColor(
		for theme: CaptureChromeTheme, settings: NativeHostSettings
	) -> NSColor {
		let hue = CGFloat(settings.hudTintHue.clamped(to: 0...1))
		let saturation = CGFloat(settings.hudTintSaturation.clamped(to: 0...1))
		let brightness = CGFloat(settings.hudTintBrightness.clamped(to: 0...1))
		return NSColor(
			calibratedHue: hue,
			saturation: saturation,
			brightness: brightness,
			alpha: 1
		)
	}
}

extension CGFloat {
	func clamped(to range: ClosedRange<CGFloat>) -> CGFloat {
		Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
	}
}

extension Double {
	func clamped(to range: ClosedRange<Double>) -> Double {
		Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
	}
}

extension NSColor {
	fileprivate func mixed(with other: NSColor, fraction: CGFloat) -> NSColor {
		let amount = fraction.clamped(to: 0...1)
		guard
			let lhs = usingColorSpace(.sRGB),
			let rhs = other.usingColorSpace(.sRGB)
		else {
			return self
		}
		return NSColor(
			srgbRed: lhs.redComponent + (rhs.redComponent - lhs.redComponent) * amount,
			green: lhs.greenComponent + (rhs.greenComponent - lhs.greenComponent) * amount,
			blue: lhs.blueComponent + (rhs.blueComponent - lhs.blueComponent) * amount,
			alpha: lhs.alphaComponent + (rhs.alphaComponent - lhs.alphaComponent) * amount
		)
	}
}

extension NSImage {
	fileprivate func tinted(with color: NSColor) -> NSImage {
		let tinted = copy() as? NSImage ?? self
		tinted.isTemplate = true
		let image = NSImage(size: tinted.size)
		image.lockFocus()
		color.set()
		let rect = CGRect(origin: .zero, size: tinted.size)
		tinted.draw(in: rect, from: rect, operation: .sourceOver, fraction: 1.0)
		image.unlockFocus()
		return image
	}
}
