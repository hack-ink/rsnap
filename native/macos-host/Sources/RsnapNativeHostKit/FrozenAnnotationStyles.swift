import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

package enum FrozenAnnotationColor: CaseIterable, Equatable {
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
	var exportColor: FrozenOverlayExportColor {
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
	var exportStrokeStyle: FrozenOverlayExportStrokeStyle {
		FrozenOverlayExportStrokeStyle(
			strokeWidthPoints: strokeWidthPoints,
			color: color.exportColor
		)
	}
}

extension FrozenSpotlightStyle {
	var exportSpotlightStyle: FrozenOverlayExportSpotlightStyle {
		FrozenOverlayExportSpotlightStyle(
			borderWidthPoints: borderWidthPoints,
			borderColor: borderColor.exportColor
		)
	}
}

extension FrozenTextStyle {
	var exportTextStyle: FrozenOverlayExportTextStyle {
		FrozenOverlayExportTextStyle(
			fontSizePoints: fontSizePoints,
			color: color.exportColor
		)
	}
}

extension FrozenOverlayExportColor {
	var annotationColor: FrozenAnnotationColor {
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
	var frozenBrushStyle: FrozenBrushStyle {
		FrozenBrushStyle(strokeWidthPoints: strokeWidthPoints, color: color.annotationColor)
	}
}

extension FrozenOverlayExportSpotlightStyle {
	var frozenSpotlightStyle: FrozenSpotlightStyle {
		FrozenSpotlightStyle(
			borderWidthPoints: borderWidthPoints,
			borderColor: borderColor.annotationColor
		)
	}
}

extension FrozenOverlayExportTextStyle {
	var frozenTextStyle: FrozenTextStyle {
		FrozenTextStyle(fontSizePoints: fontSizePoints, color: color.annotationColor)
	}
}

extension FrozenAnnotationStyleState {
	var editStyle: FrozenOverlayEditStyle {
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

package enum FrozenAnnotationStyleAction: Equatable {
	case decreaseSize
	case increaseSize
	case color(FrozenAnnotationColor)
}

package enum FrozenAnnotationStyleToolbarKind: Equatable {
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

package struct FrozenAnnotationStyleState: Equatable {
	var brushStyle = FrozenBrushStyle()
	var spotlightStyle = FrozenSpotlightStyle()
	var textStyle = FrozenTextStyle()

	package init() {}

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
