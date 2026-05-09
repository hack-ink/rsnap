import AppKit
import CoreGraphics
import Foundation
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
		renderWithRust(
			image: image,
			background: background,
			screen: screen,
			source: source,
			renderKind: .framedCapture
		)
	}

	package static func renderWindowSnapshot(
		image: CGImage,
		background: CaptureFrameBackgroundPreference,
		screen: NSScreen?
	) -> CGImage? {
		renderWithRust(
			image: image,
			background: background,
			screen: screen,
			source: .window,
			renderKind: .windowSnapshot
		)
	}

	package static func canvasSize(for imageSize: CGSize) -> CGSize {
		captureFramePlan(for: imageSize, screen: nil, source: .unknown)?.canvasSize ?? .zero
	}

	package static func imageRect(for imageSize: CGSize) -> CGRect {
		captureFramePlan(for: imageSize, screen: nil, source: .unknown)?.imageRect ?? .zero
	}

	private static func renderWithRust(
		image: CGImage,
		background: CaptureFrameBackgroundPreference,
		screen: NSScreen?,
		source: CaptureFrameSource,
		renderKind: CaptureFrameRenderKind
	) -> CGImage? {
		guard let sourceSnapshot = rgbaSnapshot(from: image) else {
			return nil
		}
		guard
			let rendered = try? RsnapCaptureFrameRenderer.render(
				source: sourceSnapshot,
				background: background.planKind,
				screenScaleFactor: screen?.backingScaleFactor ?? 2,
				sourceKind: source.planKind,
				renderKind: renderKind,
				wallpaperPath: systemWallpaperPath(for: background, screen: screen)
			)
		else {
			return nil
		}

		return cgImage(from: rendered)
	}

	private static func rgbaSnapshot(from image: CGImage) -> RGBARegionSnapshot? {
		let width = image.width
		let height = image.height
		guard
			width > 0,
			height > 0,
			let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)
		else {
			return nil
		}

		let bytesPerRow = width * 4
		var rgba = Data(count: bytesPerRow * height)
		let didDraw = rgba.withUnsafeMutableBytes { buffer -> Bool in
			guard
				let baseAddress = buffer.baseAddress,
				let context = CGContext(
					data: baseAddress,
					width: width,
					height: height,
					bitsPerComponent: 8,
					bytesPerRow: bytesPerRow,
					space: colorSpace,
					bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
				)
			else {
				return false
			}

			context.interpolationQuality = .high
			context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
			return true
		}
		guard didDraw else {
			return nil
		}

		return RGBARegionSnapshot(width: width, height: height, rgba: rgba)
	}

	private static func cgImage(from snapshot: RGBARegionSnapshot) -> CGImage? {
		let expectedByteCount = snapshot.width * snapshot.height * 4
		guard
			snapshot.width > 0,
			snapshot.height > 0,
			snapshot.rgba.count == expectedByteCount,
			let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
			let provider = CGDataProvider(data: snapshot.rgba as CFData)
		else {
			return nil
		}

		return CGImage(
			width: snapshot.width,
			height: snapshot.height,
			bitsPerComponent: 8,
			bitsPerPixel: 32,
			bytesPerRow: snapshot.width * 4,
			space: colorSpace,
			bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
			provider: provider,
			decode: nil,
			shouldInterpolate: true,
			intent: .defaultIntent
		)
	}

	private static func systemWallpaperPath(
		for background: CaptureFrameBackgroundPreference,
		screen: NSScreen?
	) -> String? {
		guard background == .systemWallpaper else {
			return nil
		}
		guard
			let screen = screen ?? NSScreen.main,
			let url = NSWorkspace.shared.desktopImageURL(for: screen)
		else {
			return nil
		}

		return url.standardizedFileURL.path
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

extension NativeHostSettings {
	func shouldApplyCaptureFrameEffect(to source: CaptureFrameSource) -> Bool {
		captureFrameEffectEnabled && captureFrameApplicability.includes(source)
	}
}
