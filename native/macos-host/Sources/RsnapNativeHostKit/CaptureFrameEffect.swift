import AppKit
import CoreGraphics
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

	package static func backgroundPlan(
		for background: CaptureFrameBackgroundPreference
	) -> CaptureFrameBackgroundPlan? {
		try? RsnapCaptureFramePlanner.backgroundPlan(for: background.planKind)
	}

	package static func systemWallpaperPath(screen: NSScreen?) -> String? {
		guard
			let screen = screen ?? NSScreen.main,
			let url = NSWorkspace.shared.desktopImageURL(for: screen)
		else {
			return nil
		}

		return url.standardizedFileURL.path
	}

	private static func renderWithRust(
		image: CGImage,
		background: CaptureFrameBackgroundPreference,
		screen: NSScreen?,
		source: CaptureFrameSource,
		renderKind: CaptureFrameRenderKind
	) -> CGImage? {
		guard
			let sourceSnapshot = NativeHostImageBridge.rgbaSnapshot(
				from: image,
				interpolationQuality: .high,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
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

		return NativeHostImageBridge.cgImage(from: rendered, shouldInterpolate: true)
	}

	private static func systemWallpaperPath(
		for background: CaptureFrameBackgroundPreference,
		screen: NSScreen?
	) -> String? {
		guard background == .systemWallpaper else {
			return nil
		}
		return systemWallpaperPath(screen: screen)
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
