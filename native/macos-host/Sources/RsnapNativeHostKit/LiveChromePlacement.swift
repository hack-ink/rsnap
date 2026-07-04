import AppKit
import CoreGraphics
import Foundation

struct LiveChromeFloatingPlacement {
	let frame: CGRect
	let flippedHorizontally: Bool
}

@MainActor
struct LiveChromeLayoutMetrics {
	let font: NSFont
	let lineHeight: CGFloat
	let commaWidth: CGFloat
	let xPrefixWidth: CGFloat
	let yPrefixWidth: CGFloat
	let digitWidth: CGFloat
	let minusWidth: CGFloat
	let keycapTextSize: CGSize
	let keycapFrameSize: CGSize
	let hexSlotWidth: CGFloat
	let placeholderXSlotWidth: CGFloat
	let placeholderYSlotWidth: CGFloat

	static let standard: LiveChromeLayoutMetrics = {
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let keycapTextSize = "Tab".size(using: font)
		return LiveChromeLayoutMetrics(
			font: font,
			lineHeight: ceil("x=0".size(using: font).height),
			commaWidth: ",".size(using: font).width,
			xPrefixWidth: "x=".size(using: font).width,
			yPrefixWidth: "y=".size(using: font).width,
			digitWidth: "0".size(using: font).width,
			minusWidth: "-".size(using: font).width,
			keycapTextSize: keycapTextSize,
			keycapFrameSize: CGSize(
				width: keycapTextSize.width + 12, height: keycapTextSize.height + 4),
			hexSlotWidth: "#FFFFFF".size(using: font).width,
			placeholderXSlotWidth: "x=?".size(using: font).width,
			placeholderYSlotWidth: "y=?".size(using: font).width
		)
	}()

	func coordinateSlotWidth(prefixWidth: CGFloat, valueText: String) -> CGFloat {
		prefixWidth
			+ valueText.reduce(CGFloat(0)) { width, character in
				width + (character == "-" ? minusWidth : digitWidth)
			}
	}
}

@MainActor
enum LiveChromePlacementPlanner {
	static let metrics = LiveChromeLayoutMetrics.standard
	private static let screenMargin: CGFloat = 6

	static func hudSize(
		positionDisplay: LivePositionDisplay,
		keycapVisible: Bool
	) -> CGSize {
		let swatchSize = CaptureChrome.hudSwatchSize
		let keycapFrame = keycapVisible ? metrics.keycapFrameSize : .zero
		let contentHeight = max(metrics.lineHeight, swatchSize.height, keycapFrame.height)
		let contentWidth =
			positionDisplay.xSlotWidth
			+ metrics.commaWidth
			+ positionDisplay.ySlotWidth
			+ CaptureChrome.hudGroupSpacing
			+ swatchSize.width
			+ CaptureChrome.hudColorItemSpacing
			+ metrics.hexSlotWidth
			+ (keycapVisible
				? CaptureChrome.hudGroupSpacing + keycapFrame.width
				: 0)
		return CGSize(
			width: contentWidth + CaptureChrome.hudInnerMarginX * 2,
			height: contentHeight + CaptureChrome.hudInnerMarginY * 2
		)
	}

	static func hudPlacement(
		bounds: CGRect,
		anchor: CGPoint,
		positionDisplay: LivePositionDisplay,
		keycapVisible: Bool
	) -> LiveChromeFloatingPlacement {
		floatingPlacement(
			bounds: bounds,
			anchor: anchor,
			size: hudSize(positionDisplay: positionDisplay, keycapVisible: keycapVisible),
			offsetX: 48,
			offsetY: 24,
			preferBelow: true
		)
	}

	static func loupeFrame(
		bounds: CGRect,
		hudFrame: CGRect,
		patch: CGImage?,
		alignTrailing: Bool
	) -> CGRect? {
		guard let patch else {
			return nil
		}
		let innerSide = CGFloat(patch.width) * CaptureChrome.loupeCellSize
		let size = CGSize(width: innerSide + 20, height: innerSide + 20)
		return stackedRect(
			bounds: bounds,
			referenceFrame: hudFrame,
			size: size,
			gap: CaptureChrome.hudLoupeGap,
			preferBelow: true,
			alignTrailing: alignTrailing
		)
	}

	private static func floatingPlacement(
		bounds: CGRect,
		anchor: CGPoint,
		size: CGSize,
		offsetX: CGFloat,
		offsetY: CGFloat,
		preferBelow: Bool
	) -> LiveChromeFloatingPlacement {
		let minX = screenMargin
		let minY = screenMargin
		let maxX = max(bounds.width - size.width - screenMargin, minX)
		let maxY = max(bounds.height - size.height - screenMargin, minY)

		var x = anchor.x + offsetX
		var flippedHorizontally = false
		if x + size.width > bounds.width - screenMargin {
			x = anchor.x - offsetX - size.width
			flippedHorizontally = true
		}
		x = x.clamped(to: minX...maxX)

		let preferredBelowY = anchor.y - offsetY - size.height
		let preferredAboveY = anchor.y + offsetY
		var y = preferBelow ? preferredBelowY : preferredAboveY
		if preferBelow {
			if y < minY {
				y = preferredAboveY
			}
		} else if y + size.height > bounds.height - screenMargin {
			y = preferredBelowY
		}
		y = y.clamped(to: minY...maxY)

		return LiveChromeFloatingPlacement(
			frame: CGRect(origin: CGPoint(x: x, y: y), size: size),
			flippedHorizontally: flippedHorizontally
		)
	}

	private static func stackedRect(
		bounds: CGRect,
		referenceFrame: CGRect,
		size: CGSize,
		gap: CGFloat,
		preferBelow: Bool,
		alignTrailing: Bool
	) -> CGRect {
		let minX = screenMargin
		let minY = screenMargin
		let maxX = max(bounds.width - size.width - screenMargin, minX)
		let maxY = max(bounds.height - size.height - screenMargin, minY)

		var x = alignTrailing ? (referenceFrame.maxX - size.width) : referenceFrame.minX
		if alignTrailing == false, x + size.width > bounds.width - screenMargin {
			x = referenceFrame.maxX - size.width
		}
		x = x.clamped(to: minX...maxX)

		let preferredBelowY = referenceFrame.minY - gap - size.height
		let preferredAboveY = referenceFrame.maxY + gap
		var y = preferBelow ? preferredBelowY : preferredAboveY
		if preferBelow {
			if y < minY {
				y = preferredAboveY
			}
		} else if y + size.height > bounds.height - screenMargin {
			y = preferredBelowY
		}
		y = y.clamped(to: minY...maxY)

		return CGRect(origin: CGPoint(x: x, y: y), size: size)
	}
}

@MainActor
enum LiveChromePendingColorText {
	private static let wheel = Array("0123456789ABCDEF")

	static func hexText(uptime: TimeInterval = ProcessInfo.processInfo.systemUptime) -> String {
		let digits = (0..<6).map { index -> Character in
			let rate = 9 + ((index * 7) % 6)
			let phase = Double((index * 23) % 31) / 31.0
			let tick = Int(((uptime + phase) * Double(rate)).rounded(.down))
			var seed =
				UInt64(tick + 1) &* 1_099_511_628_211
				^ UInt64(index + 1) &* 0x9E37_79B9_7F4A_7C15
			seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
			return wheel[Int((seed >> 58) & 0xF)]
		}
		return "#" + String(digits)
	}
}
