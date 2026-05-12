import AppKit
import CoreGraphics
import RsnapHostBridge

package struct CaptureFrameRenderEnvironment: Equatable, Sendable {
	let screenScaleFactor: CGFloat
	let wallpaperPath: String?
}

package enum CaptureFrameSource: Equatable, Sendable {
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
			source: source,
			renderKind: .framedCapture,
			environment: environment(for: background, screen: screen)
		)
	}

	package static func render(
		image: CGImage,
		background: CaptureFrameBackgroundPreference,
		source: CaptureFrameSource,
		environment: CaptureFrameRenderEnvironment
	) -> CGImage? {
		guard
			let rendered = renderSnapshot(
				image: image,
				background: background,
				source: source,
				environment: environment
			)
		else {
			return nil
		}

		return NativeHostImageBridge.cgImage(from: rendered, shouldInterpolate: true)
	}

	package static func renderSnapshot(
		image: CGImage,
		background: CaptureFrameBackgroundPreference,
		source: CaptureFrameSource,
		environment: CaptureFrameRenderEnvironment
	) -> RGBARegionSnapshot? {
		guard
			let sourceSnapshot = NativeHostImageBridge.rgbaSnapshot(
				from: image,
				interpolationQuality: .high,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			return nil
		}

		return renderWithRust(
			sourceSnapshot: sourceSnapshot,
			background: background,
			sourceKind: source,
			renderKind: .framedCapture,
			environment: environment
		)
	}

	package static func renderSnapshot(
		source: RGBARegionSnapshot,
		background: CaptureFrameBackgroundPreference,
		sourceKind: CaptureFrameSource,
		environment: CaptureFrameRenderEnvironment
	) -> RGBARegionSnapshot? {
		renderWithRust(
			sourceSnapshot: source,
			background: background,
			sourceKind: sourceKind,
			renderKind: .framedCapture,
			environment: environment
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
			source: .window,
			renderKind: .windowSnapshot,
			environment: environment(for: background, screen: screen)
		)
	}

	package static func renderWindowSnapshot(
		image: CGImage,
		background: CaptureFrameBackgroundPreference,
		environment: CaptureFrameRenderEnvironment
	) -> CGImage? {
		guard
			let rendered = renderWindowSnapshotSnapshot(
				image: image,
				background: background,
				environment: environment
			)
		else {
			return nil
		}

		return NativeHostImageBridge.cgImage(from: rendered, shouldInterpolate: true)
	}

	package static func renderWindowSnapshotSnapshot(
		image: CGImage,
		background: CaptureFrameBackgroundPreference,
		environment: CaptureFrameRenderEnvironment
	) -> RGBARegionSnapshot? {
		guard
			let sourceSnapshot = NativeHostImageBridge.rgbaSnapshot(
				from: image,
				interpolationQuality: .high,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			return nil
		}

		return renderWithRust(
			sourceSnapshot: sourceSnapshot,
			background: background,
			sourceKind: .window,
			renderKind: .windowSnapshot,
			environment: environment
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
		source: CaptureFrameSource,
		renderKind: CaptureFrameRenderKind,
		environment: CaptureFrameRenderEnvironment
	) -> CGImage? {
		guard
			let rendered = renderSnapshot(
				from: image,
				background: background,
				source: source,
				renderKind: renderKind,
				environment: environment
			)
		else {
			return nil
		}

		return NativeHostImageBridge.cgImage(from: rendered, shouldInterpolate: true)
	}

	private static func renderSnapshot(
		from image: CGImage,
		background: CaptureFrameBackgroundPreference,
		source: CaptureFrameSource,
		renderKind: CaptureFrameRenderKind,
		environment: CaptureFrameRenderEnvironment
	) -> RGBARegionSnapshot? {
		guard
			let sourceSnapshot = NativeHostImageBridge.rgbaSnapshot(
				from: image,
				interpolationQuality: .high,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			return nil
		}

		return renderWithRust(
			sourceSnapshot: sourceSnapshot,
			background: background,
			sourceKind: source,
			renderKind: renderKind,
			environment: environment
		)
	}

	private static func renderWithRust(
		sourceSnapshot: RGBARegionSnapshot,
		background: CaptureFrameBackgroundPreference,
		sourceKind: CaptureFrameSource,
		renderKind: CaptureFrameRenderKind,
		environment: CaptureFrameRenderEnvironment
	) -> RGBARegionSnapshot? {
		try? RsnapCaptureFrameRenderer.render(
			source: sourceSnapshot,
			background: background.planKind,
			screenScaleFactor: environment.screenScaleFactor,
			sourceKind: sourceKind.planKind,
			renderKind: renderKind,
			wallpaperPath: environment.wallpaperPath
		)
	}

	private static func environment(
		for background: CaptureFrameBackgroundPreference,
		screen: NSScreen?
	) -> CaptureFrameRenderEnvironment {
		CaptureFrameRenderEnvironment(
			screenScaleFactor: screen?.backingScaleFactor ?? 2,
			wallpaperPath: systemWallpaperPath(for: background, screen: screen)
		)
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
