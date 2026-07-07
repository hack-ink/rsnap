import CoreGraphics
import Darwin
import Foundation
import RsnapHostBridge

enum OverlayImageSampler {
	nonisolated static func captureBelowOverlay(
		in rect: CGRect,
		source: CaptureSessionController.FrozenCaptureJobSource
	) -> CGImage? {
		let quartzRect = appKitRectToQuartz(rect, desktopFrame: source.desktopFrame)
		return legacyWindowListImage(
			quartzRect: quartzRect,
			windowListOption: .optionOnScreenBelowWindow,
			windowID: source.referenceWindowID,
			imageOption: [.boundsIgnoreFraming, .bestResolution]
		)
	}

	nonisolated static func chromeSampleAtDisplayPoint(
		_ point: CGPoint,
		source: LiveColorSampleSource,
		sidePixels: Int,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		guard displayPointSampleGate.wait(timeout: .now()) == .success else {
			return nil
		}
		defer {
			displayPointSampleGate.signal()
		}
		let rgbSample = rgbSampleAtDisplayPoint(point, source: source)
		let loupePatch =
			includeLoupePatch
			? loupePatchAtDisplayPoint(point, source: source, sidePixels: sidePixels)
			: nil
		guard rgbSample != nil || loupePatch != nil else {
			return nil
		}
		return LiveChromeSample(
			rgbSample: rgbSample,
			rgbCapturedAtUptime: ProcessInfo.processInfo.systemUptime,
			rgbSource: "display_point",
			loupePatch: loupePatch
		)
	}

	nonisolated private static func rgbSampleAtDisplayPoint(
		_ point: CGPoint,
		source: LiveColorSampleSource
	) -> RGBSample? {
		let scaleFactor = max(source.scaleFactor, 1)
		let sampleSide = max(3 / scaleFactor, 1)
		let sampleRect = CGRect(
			x: point.x - sampleSide / 2,
			y: point.y - sampleSide / 2,
			width: sampleSide,
			height: sampleSide
		).intersection(source.screenFrame)
		guard sampleRect.isNull == false, sampleRect.width > 0, sampleRect.height > 0 else {
			return nil
		}
		guard
			let image = captureImageOnDisplay(in: sampleRect, source: source)
		else {
			return nil
		}
		return rgbSample(from: image)
	}

	nonisolated private static func loupePatchAtDisplayPoint(
		_ point: CGPoint,
		source: LiveColorSampleSource,
		sidePixels: Int
	) -> CGImage? {
		let scaleFactor = max(source.scaleFactor, 1)
		let sidePixels = max(sidePixels, 1)
		let sampleSide = max(CGFloat(sidePixels) / scaleFactor, 1 / scaleFactor)
		let sampleRect = CGRect(
			x: point.x - sampleSide / 2,
			y: point.y - sampleSide / 2,
			width: sampleSide,
			height: sampleSide
		).intersection(source.screenFrame)
		guard sampleRect.isNull == false, sampleRect.width > 0, sampleRect.height > 0 else {
			return nil
		}
		guard
			let image = captureBelowOverlay(
				in: sampleRect,
				source: source,
				imageOption: [.boundsIgnoreFraming, .bestResolution]
			)
		else {
			return nil
		}
		return normalizedPatchImage(image, sidePixels: sidePixels)
	}

	nonisolated private static func captureBelowOverlay(
		in rect: CGRect,
		source: LiveColorSampleSource,
		imageOption: CGWindowImageOption
	) -> CGImage? {
		let quartzRect = appKitRectToQuartz(rect, desktopFrame: source.desktopFrame)
		return legacyWindowListImage(
			quartzRect: quartzRect,
			windowListOption: .optionOnScreenBelowWindow,
			windowID: source.referenceWindowID,
			imageOption: imageOption
		)
	}

	nonisolated private static func captureImageOnDisplay(
		in rect: CGRect,
		source: LiveColorSampleSource
	) -> CGImage? {
		let displayRect = appKitRectToQuartz(rect, desktopFrame: source.desktopFrame)
		guard displayRect.isNull == false, displayRect.width > 0, displayRect.height > 0 else {
			return nil
		}
		return displayCreateImageForRect?(source.displayID, displayRect)?
			.takeRetainedValue()
	}

	nonisolated private static func rgbSample(from image: CGImage) -> RGBSample? {
		let width = max(image.width, 1)
		let height = max(image.height, 1)
		let bytesPerPixel = 4
		let bytesPerRow = width * bytesPerPixel
		var pixels = [UInt8](repeating: 0, count: bytesPerRow * height)
		let colorSpace = CGColorSpaceCreateDeviceRGB()
		let bitmapInfo =
			CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
		return pixels.withUnsafeMutableBytes { buffer -> RGBSample? in
			guard
				let baseAddress = buffer.baseAddress,
				let context = CGContext(
					data: baseAddress,
					width: width,
					height: height,
					bitsPerComponent: 8,
					bytesPerRow: bytesPerRow,
					space: colorSpace,
					bitmapInfo: bitmapInfo
				)
			else {
				return nil
			}
			context.interpolationQuality = .none
			context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
			let bytes = buffer.bindMemory(to: UInt8.self)
			let centerOffset = ((height / 2) * bytesPerRow) + ((width / 2) * bytesPerPixel)
			return RGBSample(
				r: bytes[centerOffset],
				g: bytes[centerOffset + 1],
				b: bytes[centerOffset + 2]
			)
		}
	}

	nonisolated private static func normalizedPatchImage(
		_ image: CGImage,
		sidePixels: Int
	) -> CGImage? {
		let sidePixels = max(sidePixels, 1)
		if image.width == sidePixels, image.height == sidePixels {
			return image
		}
		let bytesPerPixel = 4
		let bytesPerRow = sidePixels * bytesPerPixel
		var pixels = [UInt8](repeating: 0, count: bytesPerRow * sidePixels)
		let colorSpace = CGColorSpaceCreateDeviceRGB()
		let bitmapInfo =
			CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
		return pixels.withUnsafeMutableBytes { buffer -> CGImage? in
			guard
				let baseAddress = buffer.baseAddress,
				let context = CGContext(
					data: baseAddress,
					width: sidePixels,
					height: sidePixels,
					bitsPerComponent: 8,
					bytesPerRow: bytesPerRow,
					space: colorSpace,
					bitmapInfo: bitmapInfo
				)
			else {
				return nil
			}
			context.interpolationQuality = .none
			context.draw(
				image,
				in: CGRect(x: 0, y: 0, width: sidePixels, height: sidePixels)
			)
			return context.makeImage()
		}
	}

	private typealias LegacyWindowListCreateImage =
		@convention(c) (
			CGRect,
			UInt32,
			CGWindowID,
			UInt32
		) -> Unmanaged<CGImage>?

	private typealias DisplayCreateImageForRect =
		@convention(c) (
			CGDirectDisplayID,
			CGRect
		) -> Unmanaged<CGImage>?

	nonisolated private static let displayPointSampleGate = DispatchSemaphore(value: 1)

	nonisolated private static let displayCreateImageForRect: DisplayCreateImageForRect? = {
		guard
			let coreGraphics = dlopen(
				"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",
				RTLD_LAZY
			)
		else {
			return nil
		}
		guard let symbol = dlsym(coreGraphics, "CGDisplayCreateImageForRect") else {
			dlclose(coreGraphics)
			return nil
		}
		return unsafeBitCast(symbol, to: DisplayCreateImageForRect.self)
	}()

	nonisolated private static let legacyWindowListCreateImage: LegacyWindowListCreateImage? = {
		guard
			let coreGraphics = dlopen(
				"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",
				RTLD_LAZY
			)
		else {
			return nil
		}
		guard let symbol = dlsym(coreGraphics, "CGWindowListCreateImage") else {
			dlclose(coreGraphics)
			return nil
		}
		return unsafeBitCast(symbol, to: LegacyWindowListCreateImage.self)
	}()

	nonisolated private static func legacyWindowListImage(
		quartzRect: CGRect,
		windowListOption: CGWindowListOption,
		windowID: CGWindowID,
		imageOption: CGWindowImageOption
	) -> CGImage? {
		guard let createImage = legacyWindowListCreateImage else {
			return nil
		}
		return createImage(
			quartzRect,
			windowListOption.rawValue,
			windowID,
			imageOption.rawValue
		)?
		.takeRetainedValue()
	}

	nonisolated private static func appKitRectToQuartz(
		_ rect: CGRect,
		desktopFrame: CGRect
	) -> CGRect {
		CGRect(
			x: rect.minX,
			y: desktopFrame.maxY - rect.maxY,
			width: rect.width,
			height: rect.height
		)
	}
}
