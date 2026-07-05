import AppKit
import CoreGraphics
import QuartzCore

struct LiveHudColorRollTextLayerFactory {
	let backingScaleProvider: () -> CGFloat

	func makeTextLayer(
		text: String,
		font: NSFont,
		color: NSColor,
		frame: CGRect
	) -> CATextLayer {
		let layer = CATextLayer()
		layer.contentsScale = backingScaleProvider()
		layer.string = text
		layer.font = font
		layer.fontSize = font.pointSize
		layer.foregroundColor = color.cgColor
		layer.alignmentMode = .left
		layer.frame = frame
		layer.isWrapped = false
		return layer
	}

	func makeMultilineTextLayer(
		text: String,
		font: NSFont,
		color: NSColor,
		lineHeight: CGFloat,
		frame: CGRect
	) -> CATextLayer {
		let paragraphStyle = NSMutableParagraphStyle()
		paragraphStyle.alignment = .left
		paragraphStyle.lineBreakMode = .byClipping
		paragraphStyle.minimumLineHeight = lineHeight
		paragraphStyle.maximumLineHeight = lineHeight
		let attributedString = NSAttributedString(
			string: text,
			attributes: [
				.font: font,
				.foregroundColor: color,
				.paragraphStyle: paragraphStyle,
			]
		)
		let layer = CATextLayer()
		layer.contentsScale = backingScaleProvider()
		layer.string = attributedString
		layer.alignmentMode = .left
		layer.frame = frame
		layer.isWrapped = true
		layer.truncationMode = .none
		return layer
	}

	func applyText(
		_ layer: CATextLayer,
		text: String,
		font: NSFont,
		color: NSColor,
		frame: CGRect,
		alignment: CATextLayerAlignmentMode
	) {
		layer.contentsScale = backingScaleProvider()
		layer.string = text
		layer.font = font
		layer.fontSize = font.pointSize
		layer.foregroundColor = color.cgColor
		layer.alignmentMode = alignment
		layer.frame = frame
		layer.isWrapped = false
	}

	static func removeAnimationsRecursively(from layer: CALayer) {
		layer.removeAllAnimations()
		for sublayer in layer.sublayers ?? [] {
			removeAnimationsRecursively(from: sublayer)
		}
	}
}
