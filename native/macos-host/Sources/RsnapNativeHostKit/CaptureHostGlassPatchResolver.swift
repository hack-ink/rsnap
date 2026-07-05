import CoreGraphics
import CoreImage
import Foundation

@MainActor private let captureHostGlassPatchCIContext = CIContext(options: nil)

enum CaptureHostGlassSurfaceKind: Hashable {
	case hud
	case loupe
	case toolbar
}

@MainActor
struct CaptureHostGlassPatchResolver {
	private var cache: [CaptureHostGlassSurfaceKind: CaptureHostGlassPatchCache] = [:]

	mutating func patch(
		for surfaceKind: CaptureHostGlassSurfaceKind,
		frame: CGRect,
		globalFrame: CGRect,
		now: TimeInterval,
		cacheInterval: TimeInterval,
		theme: CaptureChromeTheme,
		settings: NativeHostSettings,
		sourcePatch: (CGRect) -> CGImage?
	) -> CGImage? {
		if let cached = cache[surfaceKind],
			now - cached.capturedAt < cacheInterval,
			framesMatch(cached.frame, frame)
		{
			return cached.image
		}

		guard let patch = sourcePatch(globalFrame) else {
			return nil
		}
		guard
			let image = Self.blurredPatch(
				from: patch,
				surfaceKind: surfaceKind,
				theme: theme,
				settings: settings
			)
		else {
			return nil
		}

		cache[surfaceKind] = CaptureHostGlassPatchCache(
			frame: frame,
			capturedAt: now,
			image: image
		)
		return image
	}

	static func frozenDisplayPatch(
		in globalFrame: CGRect,
		displayFrame: CGRect?,
		image: CGImage?
	) -> CGImage? {
		guard
			let displayFrame,
			let image
		else {
			return nil
		}
		let cropRect = CGRect(
			x: ((globalFrame.minX - displayFrame.minX) / max(displayFrame.width, 1))
				* CGFloat(image.width),
			y: ((displayFrame.maxY - globalFrame.maxY) / max(displayFrame.height, 1))
				* CGFloat(image.height),
			width: (globalFrame.width / max(displayFrame.width, 1)) * CGFloat(image.width),
			height: (globalFrame.height / max(displayFrame.height, 1)) * CGFloat(image.height)
		).integral.intersection(CGRect(x: 0, y: 0, width: image.width, height: image.height))
		guard cropRect.width > 0, cropRect.height > 0 else {
			return nil
		}
		return image.cropping(to: cropRect)
	}

	private static func blurredPatch(
		from image: CGImage,
		surfaceKind: CaptureHostGlassSurfaceKind,
		theme: CaptureChromeTheme,
		settings: NativeHostSettings
	) -> CGImage? {
		let ciImage = CIImage(cgImage: image)
		let clampedImage = ciImage.clampedToExtent()
		guard let filter = CIFilter(name: "CIGaussianBlur") else {
			return image
		}
		let blurAmount = CGFloat(settings.hudBlur.clamped(to: 0...1))
		let blurRadius: CGFloat =
			switch surfaceKind {
			case .hud, .loupe, .toolbar:
				14 + blurAmount * 32.0
			}
		filter.setValue(clampedImage, forKey: kCIInputImageKey)
		filter.setValue(blurRadius, forKey: kCIInputRadiusKey)
		guard let blurredImage = filter.outputImage?.cropped(to: ciImage.extent) else {
			return image
		}
		let colorAdjustedImage: CIImage
		if let colorControls = CIFilter(name: "CIColorControls") {
			colorControls.setValue(blurredImage, forKey: kCIInputImageKey)
			switch surfaceKind {
			case .hud, .loupe, .toolbar:
				colorControls.setValue(
					1.18 + settings.hudTint.clamped(to: 0...1) * 0.42,
					forKey: kCIInputSaturationKey
				)
				colorControls.setValue(1.04, forKey: kCIInputContrastKey)
				colorControls.setValue(
					themeBrightnessBias(for: theme),
					forKey: kCIInputBrightnessKey
				)
			}
			colorAdjustedImage =
				colorControls.outputImage?.cropped(to: ciImage.extent) ?? blurredImage
		} else {
			colorAdjustedImage = blurredImage
		}
		return captureHostGlassPatchCIContext.createCGImage(
			colorAdjustedImage,
			from: colorAdjustedImage.extent
		) ?? image
	}

	private func framesMatch(_ lhs: CGRect, _ rhs: CGRect) -> Bool {
		abs(lhs.minX - rhs.minX) < 1
			&& abs(lhs.minY - rhs.minY) < 1
			&& abs(lhs.width - rhs.width) < 1
			&& abs(lhs.height - rhs.height) < 1
	}

	private static func themeBrightnessBias(for theme: CaptureChromeTheme) -> Double {
		theme == .dark ? 0.015 : -0.01
	}
}
