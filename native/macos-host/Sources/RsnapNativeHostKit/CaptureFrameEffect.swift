import AppKit
import CoreGraphics
import Foundation
import ImageIO

enum CaptureFrameSource: Equatable {
	case dragRegion
	case window
	case fullScreen
	case scrollCapture
	case unknown
}

package enum CaptureFrameEffectRenderer {
	package static func render(
		image: CGImage,
		background: CaptureFrameBackgroundPreference,
		screen: NSScreen?
	) -> CGImage? {
		let imageSize = CGSize(width: image.width, height: image.height)
		guard imageSize.width > 0, imageSize.height > 0 else {
			return nil
		}
		let canvasSize = canvasSize(for: imageSize)
		guard
			let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
			let context = CGContext(
				data: nil,
				width: Int(canvasSize.width.rounded()),
				height: Int(canvasSize.height.rounded()),
				bitsPerComponent: 8,
				bytesPerRow: 0,
				space: colorSpace,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			return nil
		}

		let canvasRect = CGRect(origin: .zero, size: canvasSize)
		drawBackground(background, screen: screen, in: canvasRect, context: context)
		drawFramedCapture(image, imageSize: imageSize, in: canvasRect, context: context)
		return context.makeImage()
	}

	package static func canvasSize(for imageSize: CGSize) -> CGSize {
		let padding = padding(for: imageSize)
		return CGSize(
			width: ceil(imageSize.width + padding * 2),
			height: ceil(imageSize.height + padding * 2)
		)
	}

	package static func imageRect(for imageSize: CGSize) -> CGRect {
		let padding = padding(for: imageSize)
		return CGRect(
			x: padding,
			y: padding,
			width: imageSize.width,
			height: imageSize.height
		)
	}

	private static func padding(for imageSize: CGSize) -> CGFloat {
		let shortSide = min(imageSize.width, imageSize.height)
		let longSide = max(imageSize.width, imageSize.height)
		let visualPadding = shortSide * 0.115
		let maximumPadding = max(72, longSide * 0.18)
		return min(max(visualPadding, 48), maximumPadding)
	}

	private static func cornerRadius(for imageSize: CGSize) -> CGFloat {
		min(32, max(10, min(imageSize.width, imageSize.height) * 0.035))
	}

	private static func drawBackground(
		_ background: CaptureFrameBackgroundPreference,
		screen: NSScreen?,
		in rect: CGRect,
		context: CGContext
	) {
		if background == .systemWallpaper,
			let wallpaper = systemWallpaperImage(
				screen: screen,
				targetPixelSize: Int(max(rect.width, rect.height).rounded(.up))
			)
		{
			drawAspectFill(wallpaper, in: rect, context: context)
			context.setFillColor(NSColor.black.withAlphaComponent(0.10).cgColor)
			context.fill(rect)
			return
		}

		let colors = gradientColors(for: background)
		guard
			let gradient = CGGradient(
				colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
				colors: colors as CFArray,
				locations: [0, 0.54, 1]
			)
		else {
			context.setFillColor(colors.first ?? NSColor.windowBackgroundColor.cgColor)
			context.fill(rect)
			return
		}
		context.drawLinearGradient(
			gradient,
			start: CGPoint(x: rect.minX, y: rect.maxY),
			end: CGPoint(x: rect.maxX, y: rect.minY),
			options: [.drawsBeforeStartLocation, .drawsAfterEndLocation]
		)
	}

	private static func gradientColors(for background: CaptureFrameBackgroundPreference)
		-> [CGColor]
	{
		switch background {
		case .systemWallpaper, .aurora:
			return [
				NSColor(calibratedRed: 0.10, green: 0.16, blue: 0.28, alpha: 1).cgColor,
				NSColor(calibratedRed: 0.30, green: 0.47, blue: 0.71, alpha: 1).cgColor,
				NSColor(calibratedRed: 0.95, green: 0.61, blue: 0.43, alpha: 1).cgColor,
			]
		case .graphite:
			return [
				NSColor(calibratedRed: 0.08, green: 0.09, blue: 0.11, alpha: 1).cgColor,
				NSColor(calibratedRed: 0.24, green: 0.26, blue: 0.30, alpha: 1).cgColor,
				NSColor(calibratedRed: 0.56, green: 0.59, blue: 0.64, alpha: 1).cgColor,
			]
		case .linen:
			return [
				NSColor(calibratedRed: 0.83, green: 0.87, blue: 0.82, alpha: 1).cgColor,
				NSColor(calibratedRed: 0.58, green: 0.70, blue: 0.71, alpha: 1).cgColor,
				NSColor(calibratedRed: 0.24, green: 0.36, blue: 0.47, alpha: 1).cgColor,
			]
		}
	}

	private static func drawFramedCapture(
		_ image: CGImage,
		imageSize: CGSize,
		in canvasRect: CGRect,
		context: CGContext
	) {
		let imageRect = imageRect(for: imageSize)
		let cornerRadius = cornerRadius(for: imageSize)
		let capturePath = CGPath(
			roundedRect: imageRect,
			cornerWidth: cornerRadius,
			cornerHeight: cornerRadius,
			transform: nil
		)

		context.saveGState()
		context.addPath(capturePath)
		context.setShadow(
			offset: .zero,
			blur: max(48, min(canvasRect.width, canvasRect.height) * 0.065),
			color: NSColor.black.withAlphaComponent(0.34).cgColor
		)
		context.setFillColor(NSColor.black.withAlphaComponent(0.24).cgColor)
		context.fillPath()
		context.restoreGState()

		context.saveGState()
		context.addPath(capturePath)
		context.setShadow(
			offset: CGSize(width: 0, height: -max(18, canvasRect.height * 0.026)),
			blur: max(28, min(canvasRect.width, canvasRect.height) * 0.04),
			color: NSColor.black.withAlphaComponent(0.40).cgColor
		)
		context.setFillColor(NSColor.black.withAlphaComponent(0.28).cgColor)
		context.fillPath()
		context.restoreGState()

		context.saveGState()
		context.addPath(capturePath)
		context.clip()
		context.interpolationQuality = .high
		context.draw(image, in: imageRect)
		context.restoreGState()

		context.saveGState()
		context.addPath(capturePath)
		context.setStrokeColor(NSColor.black.withAlphaComponent(0.22).cgColor)
		context.setLineWidth(max(1, min(imageSize.width, imageSize.height) * 0.0015))
		context.strokePath()
		context.restoreGState()
	}

	private static func systemWallpaperImage(
		screen: NSScreen?,
		targetPixelSize: Int
	) -> CGImage? {
		guard
			let screen = screen ?? NSScreen.main,
			let url = NSWorkspace.shared.desktopImageURL(for: screen)
		else {
			return nil
		}
		guard
			let source = CGImageSourceCreateWithURL(
				url as CFURL,
				[kCGImageSourceShouldCache: false] as CFDictionary
			)
		else {
			return nil
		}
		let maxPixelSize = max(1, targetPixelSize)
		let options =
			[
				kCGImageSourceCreateThumbnailFromImageAlways: true,
				kCGImageSourceCreateThumbnailWithTransform: true,
				kCGImageSourceShouldCacheImmediately: true,
				kCGImageSourceThumbnailMaxPixelSize: maxPixelSize,
			] as CFDictionary
		return CGImageSourceCreateThumbnailAtIndex(source, 0, options)
	}

	private static func drawAspectFill(
		_ image: CGImage,
		in destination: CGRect,
		context: CGContext
	) {
		let imageSize = CGSize(width: image.width, height: image.height)
		let source = aspectFillCropRect(sourceSize: imageSize, destinationSize: destination.size)
		let cropped = image.cropping(to: source.integral) ?? image
		context.interpolationQuality = .high
		context.draw(cropped, in: destination)
	}

	private static func aspectFillCropRect(
		sourceSize: CGSize,
		destinationSize: CGSize
	) -> CGRect {
		let sourceAspect = sourceSize.width / max(sourceSize.height, 1)
		let destinationAspect = destinationSize.width / max(destinationSize.height, 1)
		if sourceAspect > destinationAspect {
			let width = sourceSize.height * destinationAspect
			return CGRect(
				x: (sourceSize.width - width) / 2,
				y: 0,
				width: width,
				height: sourceSize.height
			)
		}
		let height = sourceSize.width / max(destinationAspect, .leastNonzeroMagnitude)
		return CGRect(
			x: 0,
			y: (sourceSize.height - height) / 2,
			width: sourceSize.width,
			height: height
		)
	}
}

extension NativeHostSettings {
	func shouldApplyCaptureFrameEffect(to source: CaptureFrameSource) -> Bool {
		captureFrameEffectEnabled && captureFrameApplicability.includes(source)
	}
}
