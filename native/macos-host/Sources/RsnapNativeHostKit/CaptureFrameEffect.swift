import AppKit
import CoreGraphics
import Foundation
import ImageIO
import RsnapHostBridge

package enum CaptureFrameSource: Equatable {
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
		screen: NSScreen?,
		source: CaptureFrameSource
	) -> CGImage? {
		let imageSize = CGSize(width: image.width, height: image.height)
		guard imageSize.width > 0, imageSize.height > 0 else {
			return nil
		}
		guard let plan = captureFramePlan(for: imageSize, screen: screen, source: source) else {
			return nil
		}
		let canvasSize = plan.canvasSize
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
		drawFramedCapture(image, plan: plan, context: context)
		return context.makeImage()
	}

	package static func renderWindowSnapshot(
		image: CGImage,
		background: CaptureFrameBackgroundPreference,
		screen: NSScreen?
	) -> CGImage? {
		let imageSize = CGSize(width: image.width, height: image.height)
		guard imageSize.width > 0, imageSize.height > 0 else {
			return nil
		}
		guard let plan = captureFramePlan(for: imageSize, screen: screen, source: .window) else {
			return nil
		}
		let canvasSize = plan.canvasSize
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
		drawFloatingWindowSnapshot(image, plan: plan, context: context)
		return context.makeImage()
	}

	package static func canvasSize(for imageSize: CGSize) -> CGSize {
		captureFramePlan(for: imageSize, screen: nil, source: .unknown)?.canvasSize ?? .zero
	}

	package static func imageRect(for imageSize: CGSize) -> CGRect {
		captureFramePlan(for: imageSize, screen: nil, source: .unknown)?.imageRect ?? .zero
	}

	private static func drawBackground(
		_ background: CaptureFrameBackgroundPreference,
		screen: NSScreen?,
		in rect: CGRect,
		context: CGContext
	) {
		guard let plan = captureFrameBackgroundPlan(for: background) else {
			context.setFillColor(NSColor.windowBackgroundColor.cgColor)
			context.fill(rect)
			return
		}

		if let wallpaperRequest = captureFrameWallpaperRequest(
			for: background,
			destinationSize: rect.size
		),
			let wallpaper = systemWallpaperImage(
				screen: screen,
				targetPixelSize: wallpaperRequest.targetPixelSize
			)
		{
			drawAspectFill(wallpaper, in: rect, context: context)
			context.setFillColor(
				NSColor.black.withAlphaComponent(wallpaperRequest.overlayAlpha).cgColor)
			context.fill(rect)
			return
		}

		let colors = plan.colorStops.map(\.cgColor)
		guard
			let gradient = CGGradient(
				colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
				colors: colors as CFArray,
				locations: plan.locations
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

	private static func drawFramedCapture(
		_ image: CGImage,
		plan: CaptureFrameLayoutPlan,
		context: CGContext
	) {
		let capturePath = CGPath(
			roundedRect: plan.imageRect,
			cornerWidth: plan.cornerRadius,
			cornerHeight: plan.cornerRadius,
			transform: nil
		)

		for shadow in plan.shadows {
			drawShadow(
				path: capturePath,
				offset: shadow.offset,
				blur: shadow.blur,
				alpha: shadow.alpha,
				context: context
			)
		}

		context.saveGState()
		context.addPath(capturePath)
		context.clip()
		context.interpolationQuality = .high
		context.draw(image, in: plan.imageRect)
		context.restoreGState()
	}

	private static func drawFloatingWindowSnapshot(
		_ image: CGImage,
		plan: CaptureFrameLayoutPlan,
		context: CGContext
	) {
		context.saveGState()
		context.interpolationQuality = .high
		context.draw(image, in: plan.imageRect)
		context.restoreGState()
	}

	private static func drawShadow(
		path: CGPath,
		offset: CGSize,
		blur: CGFloat,
		alpha: CGFloat,
		context: CGContext
	) {
		context.saveGState()
		context.addPath(path)
		context.setShadow(
			offset: offset,
			blur: blur,
			color: NSColor.black.withAlphaComponent(alpha).cgColor
		)
		context.setFillColor(NSColor.black.cgColor)
		context.fillPath()
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
		let source: CGRect?
		do {
			source = try RsnapCaptureFramePlanner.aspectFillCropRect(
				sourceWidth: image.width,
				sourceHeight: image.height,
				destinationSize: destination.size
			)
		} catch {
			source = nil
		}
		guard let source else {
			context.interpolationQuality = .high
			context.draw(image, in: destination)
			return
		}
		let cropped = image.cropping(to: source.integral) ?? image
		context.interpolationQuality = .high
		context.draw(cropped, in: destination)
	}

	private static func captureFramePlan(
		for imageSize: CGSize,
		screen: NSScreen?,
		source: CaptureFrameSource
	) -> CaptureFrameLayoutPlan? {
		try? RsnapCaptureFramePlanner.plan(
			imageWidth: Int(max(imageSize.width.rounded(), 0)),
			imageHeight: Int(max(imageSize.height.rounded(), 0)),
			screenScaleFactor: screen?.backingScaleFactor ?? 2,
			source: source.planKind
		)
	}

	private static func captureFrameBackgroundPlan(
		for background: CaptureFrameBackgroundPreference
	) -> CaptureFrameBackgroundPlan? {
		try? RsnapCaptureFramePlanner.backgroundPlan(for: background.planKind)
	}

	private static func captureFrameWallpaperRequest(
		for background: CaptureFrameBackgroundPreference,
		destinationSize: CGSize
	) -> CaptureFrameWallpaperRequest? {
		try? RsnapCaptureFramePlanner.wallpaperRequestPlan(
			for: background.planKind,
			destinationSize: destinationSize
		)
	}
}

extension CaptureFrameSource {
	fileprivate var planKind: CaptureFrameSourceKind {
		switch self {
		case .dragRegion:
			return .dragRegion
		case .window:
			return .window
		case .fullScreen:
			return .fullScreen
		case .scrollCapture:
			return .scrollCapture
		case .unknown:
			return .unknown
		}
	}
}

extension CaptureFrameBackgroundPreference {
	fileprivate var planKind: CaptureFrameBackgroundKind {
		switch self {
		case .systemWallpaper:
			return .systemWallpaper
		case .aurora:
			return .aurora
		case .graphite:
			return .graphite
		case .linen:
			return .linen
		}
	}
}

extension CaptureFrameColorStop {
	fileprivate var cgColor: CGColor {
		NSColor(
			calibratedRed: red,
			green: green,
			blue: blue,
			alpha: alpha
		).cgColor
	}
}

extension NativeHostSettings {
	func shouldApplyCaptureFrameEffect(to source: CaptureFrameSource) -> Bool {
		captureFrameEffectEnabled && captureFrameApplicability.includes(source)
	}
}
