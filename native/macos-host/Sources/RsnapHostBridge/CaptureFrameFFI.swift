import CRsnapHostFFI
import CoreGraphics
import Foundation

public enum CaptureFrameSourceKind: UInt32, Equatable, Sendable {
	case dragRegion = 0
	case window = 1
	case fullScreen = 2
	case scrollCapture = 3
	case unknown = 4

	fileprivate var ffiKind: RsnapCaptureFrameSourceKind {
		switch self {
		case .dragRegion:
			RSNAP_CAPTURE_FRAME_SOURCE_DRAG_REGION
		case .window:
			RSNAP_CAPTURE_FRAME_SOURCE_WINDOW
		case .fullScreen:
			RSNAP_CAPTURE_FRAME_SOURCE_FULL_SCREEN
		case .scrollCapture:
			RSNAP_CAPTURE_FRAME_SOURCE_SCROLL_CAPTURE
		case .unknown:
			RSNAP_CAPTURE_FRAME_SOURCE_UNKNOWN
		}
	}
}

public enum CaptureFrameBackgroundKind: UInt32, Equatable, Sendable {
	case systemWallpaper = 0
	case aurora = 1
	case graphite = 2
	case linen = 3

	fileprivate var ffiKind: RsnapCaptureFrameBackgroundKind {
		switch self {
		case .systemWallpaper:
			RSNAP_CAPTURE_FRAME_BACKGROUND_SYSTEM_WALLPAPER
		case .aurora:
			RSNAP_CAPTURE_FRAME_BACKGROUND_AURORA
		case .graphite:
			RSNAP_CAPTURE_FRAME_BACKGROUND_GRAPHITE
		case .linen:
			RSNAP_CAPTURE_FRAME_BACKGROUND_LINEN
		}
	}
}

public enum CaptureFrameRenderKind: UInt32, Equatable, Sendable {
	case framedCapture = 0
	case windowSnapshot = 1

	fileprivate var ffiKind: RsnapCaptureFrameRenderKind {
		switch self {
		case .framedCapture:
			RSNAP_CAPTURE_FRAME_RENDER_FRAMED_CAPTURE
		case .windowSnapshot:
			RSNAP_CAPTURE_FRAME_RENDER_WINDOW_SNAPSHOT
		}
	}
}

public struct CaptureFrameColorStop: Equatable, Sendable {
	public var red: CGFloat
	public var green: CGFloat
	public var blue: CGFloat
	public var alpha: CGFloat

	public init(red: CGFloat, green: CGFloat, blue: CGFloat, alpha: CGFloat) {
		self.red = red
		self.green = green
		self.blue = blue
		self.alpha = alpha
	}
}

public struct CaptureFrameBackgroundPlan: Equatable, Sendable {
	public var colorStops: [CaptureFrameColorStop]
	public var locations: [CGFloat]
	public var prefersWallpaper: Bool
	public var wallpaperOverlayAlpha: CGFloat

	public init(
		colorStops: [CaptureFrameColorStop],
		locations: [CGFloat],
		prefersWallpaper: Bool,
		wallpaperOverlayAlpha: CGFloat
	) {
		self.colorStops = colorStops
		self.locations = locations
		self.prefersWallpaper = prefersWallpaper
		self.wallpaperOverlayAlpha = wallpaperOverlayAlpha
	}
}

public struct CaptureFrameWallpaperRequest: Equatable, Sendable {
	public var targetPixelSize: Int
	public var overlayAlpha: CGFloat

	public init(targetPixelSize: Int, overlayAlpha: CGFloat) {
		self.targetPixelSize = targetPixelSize
		self.overlayAlpha = overlayAlpha
	}
}

public struct CaptureFrameShadowPlan: Equatable, Sendable {
	public var offset: CGSize
	public var blur: CGFloat
	public var alpha: CGFloat
}

public struct CaptureFrameLayoutPlan: Equatable, Sendable {
	public var canvasSize: CGSize
	public var imageRect: CGRect
	public var cornerRadius: CGFloat
	public var shadows: [CaptureFrameShadowPlan]
}

public enum RsnapCaptureFramePlanner {
	public static func plan(
		imageWidth: Int,
		imageHeight: Int,
		screenScaleFactor: CGFloat,
		source: CaptureFrameSourceKind
	) throws -> CaptureFrameLayoutPlan? {
		var outPlan = RsnapCaptureFramePlan()
		let status = rsnap_capture_frame_plan(
			UInt32(max(imageWidth, 0)),
			UInt32(max(imageHeight, 0)),
			Double(screenScaleFactor),
			source.ffiKind,
			&outPlan
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_INVALID_INPUT.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "resolving capture frame layout plan")

		return CaptureFrameLayoutPlan(
			canvasSize: CGSize(width: outPlan.canvas_width, height: outPlan.canvas_height),
			imageRect: cgRect(from: outPlan.image_rect),
			cornerRadius: CGFloat(outPlan.corner_radius),
			shadows: [
				decode(shadow: outPlan.shadows.0),
				decode(shadow: outPlan.shadows.1),
				decode(shadow: outPlan.shadows.2),
			]
		)
	}

	public static func aspectFillCropRect(
		sourceWidth: Int,
		sourceHeight: Int,
		destinationSize: CGSize
	) throws -> CGRect? {
		var outRect = RsnapFloatRect()
		let status = rsnap_capture_frame_aspect_fill_crop_rect(
			UInt32(max(sourceWidth, 0)),
			UInt32(max(sourceHeight, 0)),
			Double(destinationSize.width),
			Double(destinationSize.height),
			&outRect
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_INVALID_INPUT.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "resolving capture frame aspect-fill crop")

		return cgRect(from: outRect)
	}

	public static func backgroundPlan(
		for background: CaptureFrameBackgroundKind
	) throws -> CaptureFrameBackgroundPlan {
		var outPlan = RsnapCaptureFrameBackgroundPlan()
		let status = rsnap_capture_frame_background_plan(background.ffiKind, &outPlan)
		try rsnapRequireOk(status, context: "resolving capture frame background plan")

		return CaptureFrameBackgroundPlan(
			colorStops: [
				decode(color: outPlan.colors.0),
				decode(color: outPlan.colors.1),
				decode(color: outPlan.colors.2),
			],
			locations: [
				CGFloat(outPlan.locations.0),
				CGFloat(outPlan.locations.1),
				CGFloat(outPlan.locations.2),
			],
			prefersWallpaper: outPlan.prefers_wallpaper != 0,
			wallpaperOverlayAlpha: CGFloat(outPlan.wallpaper_overlay_alpha)
		)
	}

	public static func wallpaperRequestPlan(
		for background: CaptureFrameBackgroundKind,
		destinationSize: CGSize
	) throws -> CaptureFrameWallpaperRequest? {
		var outRequest = RsnapCaptureFrameWallpaperRequest()
		let status = rsnap_capture_frame_wallpaper_request_plan(
			background.ffiKind,
			Double(destinationSize.width),
			Double(destinationSize.height),
			&outRequest
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "resolving capture frame wallpaper request")

		return CaptureFrameWallpaperRequest(
			targetPixelSize: Int(outRequest.target_pixel_size),
			overlayAlpha: CGFloat(outRequest.overlay_alpha)
		)
	}

	private static func decode(color: RsnapCaptureFrameColorStop) -> CaptureFrameColorStop {
		CaptureFrameColorStop(
			red: CGFloat(color.red),
			green: CGFloat(color.green),
			blue: CGFloat(color.blue),
			alpha: CGFloat(color.alpha)
		)
	}

	private static func decode(shadow: RsnapCaptureFrameShadow) -> CaptureFrameShadowPlan {
		CaptureFrameShadowPlan(
			offset: CGSize(width: shadow.offset_x, height: shadow.offset_y),
			blur: CGFloat(shadow.blur),
			alpha: CGFloat(shadow.alpha)
		)
	}
}

public enum RsnapCaptureFrameRenderer {
	public static func render(
		source: RGBARegionSnapshot,
		background: CaptureFrameBackgroundKind,
		screenScaleFactor: CGFloat,
		sourceKind: CaptureFrameSourceKind,
		renderKind: CaptureFrameRenderKind,
		wallpaperPath: String?
	) throws -> RGBARegionSnapshot? {
		var outRegion = RsnapOwnedRgbaRegion()
		let status = source.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}

			if let wallpaperPath {
				return wallpaperPath.withCString { wallpaperPathPointer in
					rsnap_capture_frame_render_rgba(
						UInt32(max(source.width, 0)),
						UInt32(max(source.height, 0)),
						baseAddress,
						source.rgba.count,
						Double(screenScaleFactor),
						sourceKind.ffiKind,
						background.ffiKind,
						renderKind.ffiKind,
						wallpaperPathPointer,
						&outRegion
					)
				}
			}

			return rsnap_capture_frame_render_rgba(
				UInt32(max(source.width, 0)),
				UInt32(max(source.height, 0)),
				baseAddress,
				source.rgba.count,
				Double(screenScaleFactor),
				sourceKind.ffiKind,
				background.ffiKind,
				renderKind.ffiKind,
				nil,
				&outRegion
			)
		}
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_INVALID_INPUT.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "rendering capture frame")

		return rsnapOwnedRgbaSnapshot(from: outRegion)
	}

}

public enum RsnapWallpaperThumbnailDecoder {
	public static func pngThumbnail(
		path: String,
		targetPixelSize: Int
	) throws -> RGBARegionSnapshot? {
		let clampedTarget = min(max(targetPixelSize, 0), Int(UInt32.max))
		if clampedTarget == 0 {
			return nil
		}

		var outRegion = RsnapOwnedRgbaRegion()
		let status = path.withCString { pathPointer in
			rsnap_capture_frame_wallpaper_png_thumbnail(
				pathPointer,
				UInt32(clampedTarget),
				&outRegion
			)
		}
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "decoding PNG wallpaper thumbnail")

		return rsnapOwnedRgbaSnapshot(from: outRegion)
	}

}
