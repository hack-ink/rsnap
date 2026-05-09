import CRsnapHostFFI
import CoreGraphics
import Foundation

public struct SessionConfiguration: Equatable, Sendable {
	public var allowTextInput: Bool
	public var prefersToolbarAboveSelection: Bool

	public init(
		allowTextInput: Bool = true,
		prefersToolbarAboveSelection: Bool = false
	) {
		self.allowTextInput = allowTextInput
		self.prefersToolbarAboveSelection = prefersToolbarAboveSelection
	}
}

public struct RGBSample: Equatable, Sendable {
	public var r: UInt8
	public var g: UInt8
	public var b: UInt8

	public init(r: UInt8, g: UInt8, b: UInt8) {
		self.r = r
		self.g = g
		self.b = b
	}
}

public struct LiveSampleSnapshot: Equatable, Sendable {
	public var rgb: RGBSample?
	public var capturedAtUptime: TimeInterval?
	public var frameAgeMicroseconds: UInt64?
	public var frameSequence: UInt64?
	public var streamGeneration: UInt64?
	public var patchWidth: Int
	public var patchHeight: Int
	public var patchRGBA: Data?

	public init(
		rgb: RGBSample?,
		capturedAtUptime: TimeInterval? = nil,
		frameAgeMicroseconds: UInt64? = nil,
		frameSequence: UInt64? = nil,
		streamGeneration: UInt64? = nil,
		patchWidth: Int = 0,
		patchHeight: Int = 0,
		patchRGBA: Data? = nil
	) {
		self.rgb = rgb
		self.capturedAtUptime = capturedAtUptime
		self.frameAgeMicroseconds = frameAgeMicroseconds
		self.frameSequence = frameSequence
		self.streamGeneration = streamGeneration
		self.patchWidth = patchWidth
		self.patchHeight = patchHeight
		self.patchRGBA = patchRGBA
	}
}

public struct RGBARegionSnapshot: Equatable, Sendable {
	public var width: Int
	public var height: Int
	public var rgba: Data

	public init(width: Int, height: Int, rgba: Data) {
		self.width = width
		self.height = height
		self.rgba = rgba
	}
}

public enum FrozenOverlayExportColor: UInt32, Equatable {
	case white = 0
	case yellow = 1
	case green = 2
	case blue = 3
	case red = 4
	case black = 5

	fileprivate var ffiColor: RsnapFrozenAnnotationColor {
		switch self {
		case .white:
			RSNAP_FROZEN_ANNOTATION_COLOR_WHITE
		case .yellow:
			RSNAP_FROZEN_ANNOTATION_COLOR_YELLOW
		case .green:
			RSNAP_FROZEN_ANNOTATION_COLOR_GREEN
		case .blue:
			RSNAP_FROZEN_ANNOTATION_COLOR_BLUE
		case .red:
			RSNAP_FROZEN_ANNOTATION_COLOR_RED
		case .black:
			RSNAP_FROZEN_ANNOTATION_COLOR_BLACK
		}
	}
}

public struct FrozenOverlayExportStrokeStyle: Equatable {
	public var strokeWidthPoints: CGFloat
	public var color: FrozenOverlayExportColor

	public init(strokeWidthPoints: CGFloat, color: FrozenOverlayExportColor) {
		self.strokeWidthPoints = strokeWidthPoints
		self.color = color
	}
}

public struct FrozenOverlayExportSpotlightStyle: Equatable {
	public var borderWidthPoints: CGFloat
	public var borderColor: FrozenOverlayExportColor

	public init(borderWidthPoints: CGFloat, borderColor: FrozenOverlayExportColor) {
		self.borderWidthPoints = borderWidthPoints
		self.borderColor = borderColor
	}
}

public struct FrozenOverlayExportTextStyle: Equatable {
	public var fontSizePoints: CGFloat
	public var color: FrozenOverlayExportColor

	public init(fontSizePoints: CGFloat, color: FrozenOverlayExportColor) {
		self.fontSizePoints = fontSizePoints
		self.color = color
	}
}

public enum FrozenOverlayExportElement: Equatable {
	case pen(points: [CGPoint], style: FrozenOverlayExportStrokeStyle)
	case arrow(start: CGPoint, end: CGPoint, style: FrozenOverlayExportStrokeStyle)
	case mosaic(rect: CGRect)
	case spotlight(rect: CGRect, style: FrozenOverlayExportSpotlightStyle)
	case text(anchor: CGPoint, text: String, style: FrozenOverlayExportTextStyle)
}

private final class FrozenOverlayExportFFIStorage {
	var elements: [RsnapFrozenOverlayExportElement] = []
	private var pointBuffers: [UnsafeMutableBufferPointer<RsnapFloatPoint>] = []
	private var textBuffers: [UnsafeMutableBufferPointer<CChar>] = []

	init(_ elements: [FrozenOverlayExportElement]) {
		self.elements = elements.map { element in
			switch element {
			case .pen(let points, let style):
				return encodePen(points: points, style: style)
			case .arrow(let start, let end, let style):
				return encodeArrow(start: start, end: end, style: style)
			case .mosaic(let rect):
				return encodeMosaic(rect: rect)
			case .spotlight(let rect, let style):
				return encodeSpotlight(rect: rect, style: style)
			case .text(let anchor, let text, let style):
				return encodeText(anchor: anchor, text: text, style: style)
			}
		}
	}

	deinit {
		for buffer in pointBuffers {
			buffer.baseAddress?.deinitialize(count: buffer.count)
			buffer.baseAddress?.deallocate()
		}
		for buffer in textBuffers {
			buffer.baseAddress?.deinitialize(count: buffer.count)
			buffer.baseAddress?.deallocate()
		}
	}

	private func encodePen(
		points: [CGPoint],
		style: FrozenOverlayExportStrokeStyle
	) -> RsnapFrozenOverlayExportElement {
		let buffer = allocatePoints(points)
		return element(
			kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_PEN,
			points: buffer.baseAddress,
			pointsLen: buffer.count,
			strokeWidthPoints: style.strokeWidthPoints,
			color: style.color
		)
	}

	private func encodeArrow(
		start: CGPoint,
		end: CGPoint,
		style: FrozenOverlayExportStrokeStyle
	) -> RsnapFrozenOverlayExportElement {
		element(
			kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_ARROW,
			start: Self.encode(point: start),
			end: Self.encode(point: end),
			strokeWidthPoints: style.strokeWidthPoints,
			color: style.color
		)
	}

	private func encodeMosaic(rect: CGRect) -> RsnapFrozenOverlayExportElement {
		element(kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_MOSAIC, rect: Self.encode(rect: rect))
	}

	private func encodeSpotlight(
		rect: CGRect,
		style: FrozenOverlayExportSpotlightStyle
	) -> RsnapFrozenOverlayExportElement {
		element(
			kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_SPOTLIGHT,
			rect: Self.encode(rect: rect),
			borderWidthPoints: style.borderWidthPoints,
			color: style.borderColor
		)
	}

	private func encodeText(
		anchor: CGPoint,
		text: String,
		style: FrozenOverlayExportTextStyle
	) -> RsnapFrozenOverlayExportElement {
		let buffer = allocateText(text)
		return element(
			kind: RSNAP_FROZEN_OVERLAY_EXPORT_ELEMENT_TEXT,
			start: Self.encode(point: anchor),
			text: buffer.baseAddress,
			fontSizePoints: style.fontSizePoints,
			color: style.color
		)
	}

	private func allocatePoints(_ points: [CGPoint]) -> UnsafeMutableBufferPointer<RsnapFloatPoint>
	{
		guard !points.isEmpty else {
			return UnsafeMutableBufferPointer(start: nil, count: 0)
		}
		let encoded = points.map(Self.encode(point:))
		let pointer = UnsafeMutablePointer<RsnapFloatPoint>.allocate(capacity: encoded.count)
		pointer.initialize(from: encoded, count: encoded.count)
		let buffer = UnsafeMutableBufferPointer(start: pointer, count: encoded.count)
		pointBuffers.append(buffer)
		return buffer
	}

	private func allocateText(_ text: String) -> UnsafeMutableBufferPointer<CChar> {
		let encoded = Array(text.utf8CString)
		let pointer = UnsafeMutablePointer<CChar>.allocate(capacity: encoded.count)
		pointer.initialize(from: encoded, count: encoded.count)
		let buffer = UnsafeMutableBufferPointer(start: pointer, count: encoded.count)
		textBuffers.append(buffer)
		return buffer
	}

	private func element(
		kind: RsnapFrozenOverlayExportElementKind,
		rect: RsnapFloatRect = RsnapFloatRect(),
		start: RsnapFloatPoint = RsnapFloatPoint(),
		end: RsnapFloatPoint = RsnapFloatPoint(),
		points: UnsafePointer<RsnapFloatPoint>? = nil,
		pointsLen: Int = 0,
		text: UnsafePointer<CChar>? = nil,
		strokeWidthPoints: CGFloat = 0,
		borderWidthPoints: CGFloat = 0,
		fontSizePoints: CGFloat = 0,
		color: FrozenOverlayExportColor = .blue
	) -> RsnapFrozenOverlayExportElement {
		RsnapFrozenOverlayExportElement(
			kind: kind,
			rect: rect,
			start: start,
			end: end,
			points: points,
			points_len: pointsLen,
			text: text,
			stroke_width_points: Double(strokeWidthPoints),
			border_width_points: Double(borderWidthPoints),
			font_size_points: Double(fontSizePoints),
			color: color.ffiColor
		)
	}

	private static func encode(point: CGPoint) -> RsnapFloatPoint {
		RsnapFloatPoint(x: Double(point.x), y: Double(point.y))
	}

	private static func encode(rect: CGRect) -> RsnapFloatRect {
		RsnapFloatRect(
			x: Double(rect.origin.x),
			y: Double(rect.origin.y),
			width: Double(rect.width),
			height: Double(rect.height)
		)
	}
}

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

public struct ScrollMinimapLayoutPlan: Equatable, Sendable {
	public var frame: CGRect
	public var imageFrame: CGRect
	public var viewportFrame: CGRect?
}

public enum FrozenSelectionTransformKind: UInt32, Equatable, Sendable {
	case move = 0
	case resizeLeft = 1
	case resizeRight = 2
	case resizeTop = 3
	case resizeBottom = 4
	case resizeTopLeft = 5
	case resizeTopRight = 6
	case resizeBottomLeft = 7
	case resizeBottomRight = 8

	fileprivate var ffiKind: RsnapFrozenSelectionTransformKind {
		switch self {
		case .move:
			RSNAP_FROZEN_SELECTION_TRANSFORM_MOVE
		case .resizeLeft:
			RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_LEFT
		case .resizeRight:
			RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_RIGHT
		case .resizeTop:
			RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_TOP
		case .resizeBottom:
			RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_BOTTOM
		case .resizeTopLeft:
			RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_TOP_LEFT
		case .resizeTopRight:
			RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_TOP_RIGHT
		case .resizeBottomLeft:
			RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_BOTTOM_LEFT
		case .resizeBottomRight:
			RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_BOTTOM_RIGHT
		}
	}
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
		try requireOk(status, context: "resolving capture frame layout plan")

		return CaptureFrameLayoutPlan(
			canvasSize: CGSize(width: outPlan.canvas_width, height: outPlan.canvas_height),
			imageRect: decode(rect: outPlan.image_rect),
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
		try requireOk(status, context: "resolving capture frame aspect-fill crop")

		return decode(rect: outRect)
	}

	public static func backgroundPlan(
		for background: CaptureFrameBackgroundKind
	) throws -> CaptureFrameBackgroundPlan {
		var outPlan = RsnapCaptureFrameBackgroundPlan()
		let status = rsnap_capture_frame_background_plan(background.ffiKind, &outPlan)
		try requireOk(status, context: "resolving capture frame background plan")

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
		try requireOk(status, context: "resolving capture frame wallpaper request")

		return CaptureFrameWallpaperRequest(
			targetPixelSize: Int(outRequest.target_pixel_size),
			overlayAlpha: CGFloat(outRequest.overlay_alpha)
		)
	}

	private static func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private static func decode(rect: RsnapFloatRect) -> CGRect {
		CGRect(x: rect.x, y: rect.y, width: rect.width, height: rect.height)
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
		try requireOk(status, context: "rendering capture frame")

		return rgbaSnapshot(from: outRegion)
	}

	private static func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private static func rgbaSnapshot(from outRegion: RsnapOwnedRgbaRegion) -> RGBARegionSnapshot? {
		guard outRegion.len > 0, let rgba = outRegion.rgba else {
			return nil
		}

		let ownedRegion = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		ownedRegion.initialize(to: outRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: outRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(ownedRegion)
				ownedRegion.deinitialize(count: 1)
				ownedRegion.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(outRegion.width),
			height: Int(outRegion.height),
			rgba: data
		)
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
		try requireOk(status, context: "decoding PNG wallpaper thumbnail")

		return rgbaSnapshot(from: outRegion)
	}

	private static func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private static func rgbaSnapshot(from outRegion: RsnapOwnedRgbaRegion) -> RGBARegionSnapshot? {
		guard outRegion.len > 0, let rgba = outRegion.rgba else {
			return nil
		}

		let ownedRegion = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		ownedRegion.initialize(to: outRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: outRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(ownedRegion)
				ownedRegion.deinitialize(count: 1)
				ownedRegion.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(outRegion.width),
			height: Int(outRegion.height),
			rgba: data
		)
	}
}

public enum RsnapScrollMinimapPlanner {
	public static func plan(
		selection: CGRect,
		exportSize: CGSize,
		bounds: CGRect,
		preferredWidth: CGFloat,
		minimumWidth: CGFloat,
		gap: CGFloat,
		margin: CGFloat,
		imageInset: CGFloat,
		viewportTopPixels: CGFloat,
		viewportHeightPixels: CGFloat
	) throws -> ScrollMinimapLayoutPlan? {
		var outPlan = RsnapScrollMinimapPlan()
		let status = rsnap_scroll_minimap_plan(
			encode(rect: selection),
			Double(exportSize.width),
			Double(exportSize.height),
			encode(rect: bounds),
			Double(preferredWidth),
			Double(minimumWidth),
			Double(gap),
			Double(margin),
			Double(imageInset),
			Double(viewportTopPixels),
			Double(viewportHeightPixels),
			&outPlan
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try requireOk(status, context: "resolving scroll minimap layout plan")
		let viewportFrame =
			outPlan.has_viewport_frame != 0 ? decode(rect: outPlan.viewport_frame) : nil

		return ScrollMinimapLayoutPlan(
			frame: decode(rect: outPlan.frame),
			imageFrame: decode(rect: outPlan.image_frame),
			viewportFrame: viewportFrame
		)
	}

	private static func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private static func encode(rect: CGRect) -> RsnapFloatRect {
		RsnapFloatRect(
			x: Double(rect.minX),
			y: Double(rect.minY),
			width: Double(rect.width),
			height: Double(rect.height)
		)
	}

	private static func decode(rect: RsnapFloatRect) -> CGRect {
		CGRect(x: rect.x, y: rect.y, width: rect.width, height: rect.height)
	}
}

public enum RsnapFrozenSelectionTransformPlanner {
	public static func hitTest(
		point: CGPoint,
		selection: CGRect,
		handleRadius: CGFloat,
		edgeTolerance: CGFloat
	) throws -> FrozenSelectionTransformKind? {
		var outKind = RSNAP_FROZEN_SELECTION_TRANSFORM_MOVE
		let status = rsnap_frozen_selection_transform_hit_test(
			Double(point.x),
			Double(point.y),
			encode(rect: selection),
			Double(handleRadius),
			Double(edgeTolerance),
			&outKind
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try requireOk(status, context: "hit-testing frozen selection transform")

		return decode(kind: outKind)
	}

	public static func transformedRect(
		kind: FrozenSelectionTransformKind,
		initialSelection: CGRect,
		monitorFrame: CGRect,
		initialPointer: CGPoint,
		point: CGPoint,
		minimumSize: CGFloat
	) throws -> CGRect? {
		var outRect = RsnapFloatRect()
		let status = rsnap_frozen_selection_transform_rect(
			kind.ffiKind,
			encode(rect: initialSelection),
			encode(rect: monitorFrame),
			Double(initialPointer.x),
			Double(initialPointer.y),
			Double(point.x),
			Double(point.y),
			Double(minimumSize),
			&outRect
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try requireOk(status, context: "resolving frozen selection transform")

		return decode(rect: outRect)
	}

	private static func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private static func encode(rect: CGRect) -> RsnapFloatRect {
		RsnapFloatRect(
			x: Double(rect.minX),
			y: Double(rect.minY),
			width: Double(rect.width),
			height: Double(rect.height)
		)
	}

	private static func decode(rect: RsnapFloatRect) -> CGRect {
		CGRect(x: rect.x, y: rect.y, width: rect.width, height: rect.height)
	}

	private static func decode(kind: RsnapFrozenSelectionTransformKind)
		-> FrozenSelectionTransformKind
	{
		return switch kind {
		case RSNAP_FROZEN_SELECTION_TRANSFORM_MOVE:
			.move
		case RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_LEFT:
			.resizeLeft
		case RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_RIGHT:
			.resizeRight
		case RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_TOP:
			.resizeTop
		case RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_BOTTOM:
			.resizeBottom
		case RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_TOP_LEFT:
			.resizeTopLeft
		case RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_TOP_RIGHT:
			.resizeTopRight
		case RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_BOTTOM_LEFT:
			.resizeBottomLeft
		case RSNAP_FROZEN_SELECTION_TRANSFORM_RESIZE_BOTTOM_RIGHT:
			.resizeBottomRight
		default:
			.move
		}
	}
}

public enum RsnapAutoCenterPlanner {
	public static func contentBounds(in image: RGBARegionSnapshot) throws -> CGRect? {
		var outRect = RsnapPixelRect()
		let status = image.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_auto_center_content_bounds_rgba(
				UInt32(max(image.width, 0)),
				UInt32(max(image.height, 0)),
				baseAddress,
				image.rgba.count,
				&outRect
			)
		}
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try requireOk(status, context: "detecting auto-center content bounds")

		return decode(pixelRect: outRect)
	}

	public static func marginBalanceShiftPoints(
		contentOriginPixels: CGFloat,
		contentSizePixels: CGFloat,
		cropSizePixels: CGFloat,
		captureSizePoints: CGFloat
	) -> CGFloat {
		CGFloat(
			rsnap_auto_center_margin_balance_shift_points(
				Double(contentOriginPixels),
				Double(contentSizePixels),
				Double(cropSizePixels),
				Double(captureSizePoints)
			)
		)
	}

	private static func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private static func decode(pixelRect: RsnapPixelRect) -> CGRect {
		CGRect(
			x: Int(pixelRect.x),
			y: Int(pixelRect.y),
			width: Int(pixelRect.width),
			height: Int(pixelRect.height)
		)
	}
}

public enum RsnapBgraFrameSampler {
	public static func rgbSample(
		width: Int,
		height: Int,
		bytesPerRow: Int,
		baseAddress: UnsafeRawPointer,
		byteCount: Int,
		displayFrame: CGRect,
		point: CGPoint
	) throws -> RGBSample? {
		var outRGB = RsnapRgb()
		let status = rsnap_bgra_frame_sample_rgb(
			UInt32(max(width, 0)),
			UInt32(max(height, 0)),
			max(bytesPerRow, 0),
			baseAddress.assumingMemoryBound(to: UInt8.self),
			max(byteCount, 0),
			encode(rect: displayFrame),
			Double(point.x),
			Double(point.y),
			&outRGB
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try requireOk(status, context: "sampling BGRA frame RGB")

		return RGBSample(r: outRGB.r, g: outRGB.g, b: outRGB.b)
	}

	public static func loupePatch(
		width: Int,
		height: Int,
		bytesPerRow: Int,
		baseAddress: UnsafeRawPointer,
		byteCount: Int,
		displayFrame: CGRect,
		point: CGPoint,
		sidePixels: Int
	) throws -> RGBARegionSnapshot? {
		var outRegion = RsnapOwnedRgbaRegion()
		let status = rsnap_bgra_frame_loupe_patch_rgba(
			UInt32(max(width, 0)),
			UInt32(max(height, 0)),
			max(bytesPerRow, 0),
			baseAddress.assumingMemoryBound(to: UInt8.self),
			max(byteCount, 0),
			encode(rect: displayFrame),
			Double(point.x),
			Double(point.y),
			UInt32(max(sidePixels, 0)),
			&outRegion
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try requireOk(status, context: "sampling BGRA frame loupe patch")

		return rgbaSnapshot(from: outRegion)
	}

	private static func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private static func encode(rect: CGRect) -> RsnapFloatRect {
		RsnapFloatRect(
			x: Double(rect.origin.x),
			y: Double(rect.origin.y),
			width: Double(rect.width),
			height: Double(rect.height)
		)
	}

	private static func rgbaSnapshot(from outRegion: RsnapOwnedRgbaRegion) -> RGBARegionSnapshot? {
		guard outRegion.len > 0, let rgba = outRegion.rgba else {
			return nil
		}

		let ownedRegion = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		ownedRegion.initialize(to: outRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: outRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(ownedRegion)
				ownedRegion.deinitialize(count: 1)
				ownedRegion.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(outRegion.width),
			height: Int(outRegion.height),
			rgba: data
		)
	}
}

public enum RsnapExportEncoder {
	public static func pngData(from image: RGBARegionSnapshot) throws -> Data {
		var outPNG = RsnapOwnedBytes()
		let status = image.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_export_rgba_to_png(
				UInt32(max(image.width, 0)),
				UInt32(max(image.height, 0)),
				baseAddress,
				image.rgba.count,
				&outPNG
			)
		}
		try requireOk(status, context: "encoding export PNG")

		return try data(from: outPNG, context: "taking encoded export PNG")
	}

	public static func pngData(from image: RGBARegionSnapshot, crop: CGRect) throws -> Data {
		let cropRect = try encode(crop: crop)
		var outPNG = RsnapOwnedBytes()
		let status = image.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_export_rgba_crop_to_png(
				UInt32(max(image.width, 0)),
				UInt32(max(image.height, 0)),
				baseAddress,
				image.rgba.count,
				cropRect,
				&outPNG
			)
		}
		try requireOk(status, context: "encoding cropped export PNG")

		return try data(from: outPNG, context: "taking encoded cropped export PNG")
	}

	public static func frozenDisplayCropRect(
		imageWidth: Int,
		imageHeight: Int,
		displayFrame: CGRect,
		selection: CGRect
	) throws -> CGRect? {
		var outRect = RsnapPixelRect()
		let status = rsnap_frozen_display_crop_rect(
			UInt32(max(imageWidth, 0)),
			UInt32(max(imageHeight, 0)),
			encode(rect: displayFrame),
			encode(rect: selection),
			&outRect
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try requireOk(status, context: "resolving frozen display export crop")

		return decode(pixelRect: outRect)
	}

	public static func frozenMosaicLightPrivacyPatch(
		imageWidth: Int,
		imageHeight: Int,
		sourceRect: CGRect
	) throws -> RGBARegionSnapshot? {
		var outRegion = RsnapOwnedRgbaRegion()
		let status = rsnap_frozen_mosaic_light_privacy_patch_rgba(
			UInt32(max(imageWidth, 0)),
			UInt32(max(imageHeight, 0)),
			encode(rect: sourceRect),
			&outRegion
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try requireOk(status, context: "rendering frozen mosaic privacy patch")

		return rgbaSnapshot(from: outRegion)
	}

	public static func frozenOverlayExportImage(
		from image: RGBARegionSnapshot,
		selection: CGRect,
		elements: [FrozenOverlayExportElement]
	) throws -> RGBARegionSnapshot {
		let storage = FrozenOverlayExportFFIStorage(elements)
		var outRegion = RsnapOwnedRgbaRegion()
		let status = image.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return storage.elements.withUnsafeBufferPointer { elementBuffer in
				rsnap_frozen_overlay_export_render_rgba(
					UInt32(max(image.width, 0)),
					UInt32(max(image.height, 0)),
					baseAddress,
					image.rgba.count,
					encode(rect: selection),
					elementBuffer.baseAddress,
					elementBuffer.count,
					&outRegion
				)
			}
		}
		try requireOk(status, context: "rendering frozen overlay export image")
		guard let snapshot = rgbaSnapshot(from: outRegion) else {
			throw HostBridgeError.ffiStatus(
				context: "taking frozen overlay export image",
				code: RSNAP_STATUS_EMPTY.rawValue)
		}

		return snapshot
	}

	private static func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private static func encode(crop: CGRect) throws -> RsnapPixelRect {
		let x = crop.origin.x.rounded()
		let y = crop.origin.y.rounded()
		let width = crop.width.rounded()
		let height = crop.height.rounded()
		let maxValue = CGFloat(UInt32.max)

		guard
			x >= 0,
			y >= 0,
			width >= 0,
			height >= 0,
			x <= maxValue,
			y <= maxValue,
			width <= maxValue,
			height <= maxValue
		else {
			throw HostBridgeError.ffiStatus(
				context: "encoding export crop rectangle",
				code: RSNAP_STATUS_INVALID_INPUT.rawValue)
		}

		return RsnapPixelRect(
			x: UInt32(x),
			y: UInt32(y),
			width: UInt32(width),
			height: UInt32(height)
		)
	}

	private static func encode(rect: CGRect) -> RsnapFloatRect {
		RsnapFloatRect(
			x: Double(rect.origin.x),
			y: Double(rect.origin.y),
			width: Double(rect.width),
			height: Double(rect.height)
		)
	}

	private static func decode(pixelRect: RsnapPixelRect) -> CGRect {
		CGRect(
			x: Int(pixelRect.x),
			y: Int(pixelRect.y),
			width: Int(pixelRect.width),
			height: Int(pixelRect.height)
		)
	}

	private static func data(from outPNG: RsnapOwnedBytes, context: String) throws -> Data {
		guard outPNG.len > 0, let bytes = outPNG.bytes else {
			throw HostBridgeError.ffiStatus(context: context, code: RSNAP_STATUS_EMPTY.rawValue)
		}

		let ownedBytes = UnsafeMutablePointer<RsnapOwnedBytes>.allocate(capacity: 1)
		ownedBytes.initialize(to: outPNG)
		return Data(
			bytesNoCopy: bytes,
			count: outPNG.len,
			deallocator: .custom { _, _ in
				rsnap_owned_bytes_release(ownedBytes)
				ownedBytes.deinitialize(count: 1)
				ownedBytes.deallocate()
			}
		)
	}

	private static func rgbaSnapshot(from outRegion: RsnapOwnedRgbaRegion) -> RGBARegionSnapshot? {
		guard outRegion.len > 0, let rgba = outRegion.rgba else {
			return nil
		}

		let ownedRegion = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		ownedRegion.initialize(to: outRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: outRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(ownedRegion)
				ownedRegion.deinitialize(count: 1)
				ownedRegion.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(outRegion.width), height: Int(outRegion.height), rgba: data)
	}
}

public enum ScrollObserveOutcome: UInt32, Equatable, Sendable {
	case noChange = 0
	case previewUpdated = 1
	case committed = 2
	case unsupportedDirection = 3
}

public struct ScrollObserveResult: Equatable, Sendable {
	public var outcome: ScrollObserveOutcome
	public var growthRows: Int
	public var exportWidth: Int
	public var exportHeight: Int
	public var currentViewportTopY: Int

	public init(
		outcome: ScrollObserveOutcome,
		growthRows: Int,
		exportWidth: Int,
		exportHeight: Int,
		currentViewportTopY: Int
	) {
		self.outcome = outcome
		self.growthRows = growthRows
		self.exportWidth = exportWidth
		self.exportHeight = exportHeight
		self.currentViewportTopY = currentViewportTopY
	}
}

public struct MonitorSnapshot: Equatable, Sendable {
	public var id: UInt32
	public var frame: CGRect
	public var scaleFactorX1000: UInt32

	public init(id: UInt32, frame: CGRect, scaleFactorX1000: UInt32) {
		self.id = id
		self.frame = frame
		self.scaleFactorX1000 = scaleFactorX1000
	}
}

public struct WindowSnapshot: Equatable, Sendable {
	public var windowID: UInt32?
	public var frame: CGRect

	public init(windowID: UInt32?, frame: CGRect) {
		self.windowID = windowID
		self.frame = frame
	}
}

public enum SceneKind: UInt32, Equatable, Sendable {
	case hidden = 0
	case live = 1
	case frozen = 2
}

public enum CursorIntent: UInt32, Equatable, Sendable {
	case `default` = 0
	case crosshair = 1
	case grab = 2
	case grabbing = 3
	case resizeNorth = 4
	case resizeSouth = 5
	case resizeEast = 6
	case resizeWest = 7
	case resizeNorthEast = 8
	case resizeNorthWest = 9
	case resizeSouthEast = 10
	case resizeSouthWest = 11
	case text = 12
}

public enum ToolbarItemKind: UInt32, Equatable, Sendable {
	case pointer = 0
	case pen = 1
	case arrow = 2
	case text = 3
	case mosaic = 4
	case spotlight = 5
	case undo = 6
	case redo = 7
	case autoCenter = 8
	case scroll = 9
	case ocr = 10
	case copy = 11
	case save = 12

	public var isModeTool: Bool {
		switch self {
		case .pointer, .pen, .arrow, .text, .mosaic, .spotlight:
			return true
		case .undo, .redo, .autoCenter, .scroll, .ocr, .copy, .save:
			return false
		}
	}
}

public struct ToolbarItem: Equatable, Sendable {
	public var kind: ToolbarItemKind
	public var enabled: Bool
	public var selected: Bool

	public init(kind: ToolbarItemKind, enabled: Bool, selected: Bool) {
		self.kind = kind
		self.enabled = enabled
		self.selected = selected
	}
}

public struct SceneSnapshot: Equatable, Sendable {
	public var mode: SceneKind
	public var cursorIntent: CursorIntent
	public var pointer: CGPoint?
	public var activeMonitor: MonitorSnapshot?
	public var highlightedWindow: WindowSnapshot?
	public var liveSelectionPreview: CGRect?
	public var frozenSelection: CGRect?
	public var rgb: RGBSample?
	public var loupeVisible: Bool
	public var toolbarItems: [ToolbarItem]
	public var statusMessage: String?

	public init(
		mode: SceneKind,
		cursorIntent: CursorIntent,
		pointer: CGPoint?,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?,
		liveSelectionPreview: CGRect?,
		frozenSelection: CGRect?,
		rgb: RGBSample?,
		loupeVisible: Bool,
		toolbarItems: [ToolbarItem],
		statusMessage: String?
	) {
		self.mode = mode
		self.cursorIntent = cursorIntent
		self.pointer = pointer
		self.activeMonitor = activeMonitor
		self.highlightedWindow = highlightedWindow
		self.liveSelectionPreview = liveSelectionPreview
		self.frozenSelection = frozenSelection
		self.rgb = rgb
		self.loupeVisible = loupeVisible
		self.toolbarItems = toolbarItems
		self.statusMessage = statusMessage
	}
}

public enum HostRequest: Equatable, Sendable {
	case startLiveCapture
	case stopLiveCapture
	case requestFreezeSnapshot(selection: CGRect, selectionEditable: Bool)
	case startScrollCapture
	case copyCapture
	case saveCapture
	case recognizeText
	case requestScreenRecordingPermission
}

public enum HostEffectKind: UInt32, Equatable, Sendable {
	case copyCapture = 0
	case saveCapture = 1
	case recognizeText = 2
}

public enum PermissionKind: UInt32, Equatable, Sendable {
	case screenRecording = 0
}

public enum HostEvent: Sendable {
	case sessionActivated
	case pointerMoved(
		point: CGPoint,
		rgb: RGBSample?,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?
	)
	case primaryInteractionStarted(
		point: CGPoint,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?
	)
	case primaryInteractionUpdated(
		point: CGPoint,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?
	)
	case primaryInteractionCompleted(
		point: CGPoint,
		activeMonitor: MonitorSnapshot?,
		highlightedWindow: WindowSnapshot?
	)
	case cancelRequested
	case copyRequested
	case saveRequested
	case recognizeTextRequested
	case toggleLoupe
	case toolbarItemInvoked(ToolbarItemKind)
}

public enum HostReport: Sendable {
	case freezeSnapshotCommitted(selection: CGRect)
	case hostEffectCompleted(HostEffectKind)
	case permissionChanged(PermissionKind, granted: Bool)
	case statusMessage(String)
}

public enum HostBridgeError: Error, CustomStringConvertible {
	case abiVersionMismatch(expected: UInt32, actual: UInt32)
	case sessionCreationFailed
	case ffiStatus(context: String, code: UInt32)
	case invalidSceneKind(UInt32)
	case invalidCursorIntent(UInt32)
	case invalidRequestKind(UInt32)

	public var description: String {
		switch self {
		case .abiVersionMismatch(let expected, let actual):
			return "ABI mismatch: expected \(expected), got \(actual)"
		case .sessionCreationFailed:
			return "Failed to create rsnap host session."
		case .ffiStatus(let context, let code):
			return "FFI status \(code) while \(context)"
		case .invalidSceneKind(let rawValue):
			return "Unknown scene kind \(rawValue)"
		case .invalidCursorIntent(let rawValue):
			return "Unknown cursor intent \(rawValue)"
		case .invalidRequestKind(let rawValue):
			return "Unknown host request kind \(rawValue)"
		}
	}
}

public final class RsnapHostSession {
	private let handle: OpaquePointer
	public let configuration: SessionConfiguration

	public init(configuration: SessionConfiguration = .init()) throws {
		let actualAbi = rsnap_host_ffi_abi_version()
		if actualAbi != RSNAP_HOST_FFI_ABI_VERSION {
			throw HostBridgeError.abiVersionMismatch(
				expected: RSNAP_HOST_FFI_ABI_VERSION,
				actual: actualAbi
			)
		}

		let config = RsnapSessionConfig(
			platform: RSNAP_PLATFORM_MACOS,
			allow_text_input: configuration.allowTextInput ? 1 : 0,
			prefers_toolbar_above_selection: configuration.prefersToolbarAboveSelection ? 1 : 0
		)
		guard let handle = rsnap_session_create(config) else {
			throw HostBridgeError.sessionCreationFailed
		}

		self.handle = handle
		self.configuration = configuration
	}

	deinit {
		rsnap_session_destroy(handle)
	}

	public func enterLive() throws {
		try requireOk(
			rsnap_session_enter_live(handle),
			context: "entering live mode"
		)
	}

	public func send(event: HostEvent) throws {
		try requireOk(
			rsnap_session_handle_host_event(handle, encode(event: event)),
			context: "sending host event"
		)
	}

	public func send(report: HostReport) throws {
		try requireOk(
			rsnap_session_handle_host_report(handle, encode(report: report)),
			context: "sending host report"
		)
	}

	public func currentScene() throws -> SceneSnapshot {
		var outScene = RsnapSceneModel()
		try requireOk(
			rsnap_session_copy_scene_model(handle, &outScene),
			context: "copying scene model"
		)

		return try decode(scene: outScene)
	}

	public func takeNextRequest() throws -> HostRequest? {
		var outRequest = RsnapHostRequestValue()
		let status = rsnap_session_take_next_request(handle, &outRequest)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		try requireOk(status, context: "draining queued host request")

		return try decode(request: outRequest)
	}

	public func drainRequests() throws -> [HostRequest] {
		var requests: [HostRequest] = []
		while let request = try takeNextRequest() {
			requests.append(request)
		}
		return requests
	}

	private func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private func encode(event: HostEvent) -> RsnapHostEvent {
		switch event {
		case .sessionActivated:
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_SESSION_ACTIVATED.rawValue,
				point: RsnapPoint(),
				has_point: 0,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: RsnapMonitorRect(),
				has_active_monitor: 0,
				highlighted_window: RsnapWindowRect(),
				has_highlighted_window: 0,
				toolbar_item_kind: 0
			)
		case .pointerMoved(let point, let rgb, let activeMonitor, let highlightedWindow):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_POINTER_MOVED.rawValue,
				point: encode(point: point),
				has_point: 1,
				rgb: encode(rgb: rgb),
				has_rgb: rgb == nil ? 0 : 1,
				active_monitor: encode(monitor: activeMonitor),
				has_active_monitor: activeMonitor == nil ? 0 : 1,
				highlighted_window: encode(window: highlightedWindow),
				has_highlighted_window: highlightedWindow == nil ? 0 : 1,
				toolbar_item_kind: 0
			)
		case .primaryInteractionStarted(let point, let activeMonitor, let highlightedWindow):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_PRIMARY_INTERACTION_STARTED.rawValue,
				point: encode(point: point),
				has_point: 1,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: encode(monitor: activeMonitor),
				has_active_monitor: activeMonitor == nil ? 0 : 1,
				highlighted_window: encode(window: highlightedWindow),
				has_highlighted_window: highlightedWindow == nil ? 0 : 1,
				toolbar_item_kind: 0
			)
		case .primaryInteractionUpdated(let point, let activeMonitor, let highlightedWindow):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_PRIMARY_INTERACTION_UPDATED.rawValue,
				point: encode(point: point),
				has_point: 1,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: encode(monitor: activeMonitor),
				has_active_monitor: activeMonitor == nil ? 0 : 1,
				highlighted_window: encode(window: highlightedWindow),
				has_highlighted_window: highlightedWindow == nil ? 0 : 1,
				toolbar_item_kind: 0
			)
		case .primaryInteractionCompleted(let point, let activeMonitor, let highlightedWindow):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_PRIMARY_INTERACTION_COMPLETED.rawValue,
				point: encode(point: point),
				has_point: 1,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: encode(monitor: activeMonitor),
				has_active_monitor: activeMonitor == nil ? 0 : 1,
				highlighted_window: encode(window: highlightedWindow),
				has_highlighted_window: highlightedWindow == nil ? 0 : 1,
				toolbar_item_kind: 0
			)
		case .cancelRequested:
			return eventWith(kind: RSNAP_HOST_EVENT_CANCEL_REQUESTED.rawValue)
		case .copyRequested:
			return eventWith(kind: RSNAP_HOST_EVENT_COPY_REQUESTED.rawValue)
		case .saveRequested:
			return eventWith(kind: RSNAP_HOST_EVENT_SAVE_REQUESTED.rawValue)
		case .recognizeTextRequested:
			return eventWith(kind: RSNAP_HOST_EVENT_RECOGNIZE_TEXT_REQUESTED.rawValue)
		case .toggleLoupe:
			return eventWith(kind: RSNAP_HOST_EVENT_TOGGLE_LOUPE.rawValue)
		case .toolbarItemInvoked(let item):
			return RsnapHostEvent(
				kind: RSNAP_HOST_EVENT_TOOLBAR_ITEM_INVOKED.rawValue,
				point: RsnapPoint(),
				has_point: 0,
				rgb: RsnapRgb(),
				has_rgb: 0,
				active_monitor: RsnapMonitorRect(),
				has_active_monitor: 0,
				highlighted_window: RsnapWindowRect(),
				has_highlighted_window: 0,
				toolbar_item_kind: item.rawValue
			)
		}
	}

	private func encode(report: HostReport) -> RsnapHostReport {
		var reportValue = RsnapHostReport()

		switch report {
		case .freezeSnapshotCommitted(let selection):
			reportValue.kind = RSNAP_HOST_REPORT_FREEZE_SNAPSHOT_COMMITTED.rawValue
			reportValue.selection = encode(rect: selection)
			reportValue.has_selection = 1
		case .hostEffectCompleted(let effect):
			reportValue.kind = RSNAP_HOST_REPORT_HOST_EFFECT_COMPLETED.rawValue
			reportValue.effect_kind = effect.rawValue
		case .permissionChanged(let permission, let granted):
			reportValue.kind = RSNAP_HOST_REPORT_PERMISSION_CHANGED.rawValue
			reportValue.permission_kind = permission.rawValue
			reportValue.granted = granted ? 1 : 0
		case .statusMessage(let message):
			reportValue.kind = RSNAP_HOST_REPORT_STATUS_MESSAGE.rawValue
			encodeStatusMessage(message, into: &reportValue)
		}

		return reportValue
	}

	private func decode(scene: RsnapSceneModel) throws -> SceneSnapshot {
		guard let mode = SceneKind(rawValue: scene.scene_kind) else {
			throw HostBridgeError.invalidSceneKind(scene.scene_kind)
		}
		guard let cursorIntent = CursorIntent(rawValue: scene.cursor_intent) else {
			throw HostBridgeError.invalidCursorIntent(scene.cursor_intent)
		}

		return SceneSnapshot(
			mode: mode,
			cursorIntent: cursorIntent,
			pointer: scene.has_pointer == 0 ? nil : decode(point: scene.pointer),
			activeMonitor: scene.has_active_monitor == 0
				? nil : decode(monitor: scene.active_monitor),
			highlightedWindow: scene.has_highlighted_window == 0
				? nil : decode(window: scene.highlighted_window),
			liveSelectionPreview: scene.has_live_selection_preview == 0
				? nil : decode(rect: scene.live_selection_preview),
			frozenSelection: scene.has_frozen_selection == 0
				? nil : decode(rect: scene.frozen_selection),
			rgb: scene.has_rgb == 0 ? nil : decode(rgb: scene.rgb),
			loupeVisible: scene.loupe_visible != 0,
			toolbarItems: decodeToolbarItems(scene),
			statusMessage: decodeStatusMessage(scene)
		)
	}

	private func eventWith(kind: UInt32) -> RsnapHostEvent {
		RsnapHostEvent(
			kind: kind,
			point: RsnapPoint(),
			has_point: 0,
			rgb: RsnapRgb(),
			has_rgb: 0,
			active_monitor: RsnapMonitorRect(),
			has_active_monitor: 0,
			highlighted_window: RsnapWindowRect(),
			has_highlighted_window: 0,
			toolbar_item_kind: 0
		)
	}

	private func decodeToolbarItems(_ scene: RsnapSceneModel) -> [ToolbarItem] {
		let count = min(Int(scene.toolbar_item_count), Int(RSNAP_TOOLBAR_ITEM_CAPACITY))
		return withUnsafeBytes(of: scene.toolbar_items) { rawBuffer in
			let buffer = rawBuffer.bindMemory(to: RsnapToolbarItem.self)
			return buffer.prefix(count).compactMap { item in
				guard item.present != 0, let kind = ToolbarItemKind(rawValue: item.kind) else {
					return nil
				}
				return ToolbarItem(
					kind: kind, enabled: item.enabled != 0, selected: item.selected != 0)
			}
		}
	}

	private func decodeStatusMessage(_ scene: RsnapSceneModel) -> String? {
		let count = min(Int(scene.status_message_len), Int(RSNAP_STATUS_MESSAGE_CAPACITY))
		guard count > 0 else {
			return nil
		}
		return withUnsafeBytes(of: scene.status_message) { rawBuffer in
			String(bytes: rawBuffer.prefix(count), encoding: .utf8)
		}
	}

	private func decode(request: RsnapHostRequestValue) throws -> HostRequest {
		switch request.kind {
		case RSNAP_HOST_REQUEST_START_LIVE_CAPTURE.rawValue:
			return .startLiveCapture
		case RSNAP_HOST_REQUEST_STOP_LIVE_CAPTURE.rawValue:
			return .stopLiveCapture
		case RSNAP_HOST_REQUEST_REQUEST_FREEZE_SNAPSHOT.rawValue:
			guard request.has_selection != 0 else {
				throw HostBridgeError.invalidRequestKind(request.kind)
			}
			return .requestFreezeSnapshot(
				selection: decode(rect: request.selection),
				selectionEditable: request.selection_editable != 0
			)
		case RSNAP_HOST_REQUEST_COPY_CAPTURE.rawValue:
			return .copyCapture
		case RSNAP_HOST_REQUEST_SAVE_CAPTURE.rawValue:
			return .saveCapture
		case RSNAP_HOST_REQUEST_RECOGNIZE_TEXT.rawValue:
			return .recognizeText
		case RSNAP_HOST_REQUEST_REQUEST_SCREEN_RECORDING_PERMISSION.rawValue:
			return .requestScreenRecordingPermission
		case RSNAP_HOST_REQUEST_START_SCROLL_CAPTURE.rawValue:
			return .startScrollCapture
		default:
			throw HostBridgeError.invalidRequestKind(request.kind)
		}
	}

	private func encodeStatusMessage(_ message: String, into report: inout RsnapHostReport) {
		let data = Array(message.utf8.prefix(Int(RSNAP_STATUS_MESSAGE_CAPACITY)))
		report.status_message_len = UInt32(data.count)
		withUnsafeMutableBytes(of: &report.status_message) { rawBuffer in
			rawBuffer.initializeMemory(as: UInt8.self, repeating: 0)
			rawBuffer.prefix(data.count).copyBytes(from: data)
		}
	}

	private func encode(point: CGPoint) -> RsnapPoint {
		RsnapPoint(x: Int32(point.x.rounded()), y: Int32(point.y.rounded()))
	}

	private func decode(point: RsnapPoint) -> CGPoint {
		CGPoint(x: Int(point.x), y: Int(point.y))
	}

	private func encode(rgb: RGBSample?) -> RsnapRgb {
		guard let rgb else {
			return RsnapRgb()
		}
		return RsnapRgb(r: rgb.r, g: rgb.g, b: rgb.b)
	}

	private func decode(rgb: RsnapRgb) -> RGBSample {
		RGBSample(r: rgb.r, g: rgb.g, b: rgb.b)
	}

	private func encode(rect: CGRect) -> RsnapRect {
		RsnapRect(
			x: Int32(rect.origin.x.rounded()),
			y: Int32(rect.origin.y.rounded()),
			width: UInt32(max(rect.width.rounded(), 0)),
			height: UInt32(max(rect.height.rounded(), 0))
		)
	}

	private func decode(rect: RsnapRect) -> CGRect {
		CGRect(
			x: Int(rect.x),
			y: Int(rect.y),
			width: Int(rect.width),
			height: Int(rect.height)
		)
	}

	private func encode(monitor: MonitorSnapshot?) -> RsnapMonitorRect {
		guard let monitor else {
			return RsnapMonitorRect()
		}
		return RsnapMonitorRect(
			id: monitor.id,
			origin: encode(point: monitor.frame.origin),
			width: UInt32(max(monitor.frame.width.rounded(), 0)),
			height: UInt32(max(monitor.frame.height.rounded(), 0)),
			scale_factor_x1000: monitor.scaleFactorX1000
		)
	}

	private func decode(monitor: RsnapMonitorRect) -> MonitorSnapshot {
		MonitorSnapshot(
			id: monitor.id,
			frame: CGRect(
				x: Int(monitor.origin.x),
				y: Int(monitor.origin.y),
				width: Int(monitor.width),
				height: Int(monitor.height)
			),
			scaleFactorX1000: monitor.scale_factor_x1000
		)
	}

	private func encode(window: WindowSnapshot?) -> RsnapWindowRect {
		guard let window else {
			return RsnapWindowRect()
		}
		return RsnapWindowRect(
			window_id: window.windowID ?? 0,
			has_window_id: window.windowID == nil ? 0 : 1,
			x: Int64(window.frame.origin.x.rounded()),
			y: Int64(window.frame.origin.y.rounded()),
			width: Int64(window.frame.width.rounded()),
			height: Int64(window.frame.height.rounded())
		)
	}

	private func decode(window: RsnapWindowRect) -> WindowSnapshot {
		WindowSnapshot(
			windowID: window.has_window_id == 0 ? nil : window.window_id,
			frame: CGRect(
				x: Int(window.x),
				y: Int(window.y),
				width: Int(window.width),
				height: Int(window.height)
			)
		)
	}
}

public final class RsnapScrollCaptureSession: @unchecked Sendable {
	private let handle: OpaquePointer
	private let stateLock = NSLock()

	public init(baseImage: RGBARegionSnapshot, previewWidthPixels: Int) throws {
		let actualAbi = rsnap_host_ffi_abi_version()
		if actualAbi != RSNAP_HOST_FFI_ABI_VERSION {
			throw HostBridgeError.abiVersionMismatch(
				expected: RSNAP_HOST_FFI_ABI_VERSION,
				actual: actualAbi
			)
		}

		let width = UInt32(max(baseImage.width, 0))
		let height = UInt32(max(baseImage.height, 0))
		let previewWidth = UInt32(max(previewWidthPixels, 1))
		let maybeHandle = baseImage.rgba.withUnsafeBytes { buffer -> OpaquePointer? in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return nil
			}
			return rsnap_scroll_session_create(
				width,
				height,
				baseAddress,
				baseImage.rgba.count,
				previewWidth
			)
		}
		guard let handle = maybeHandle else {
			throw HostBridgeError.sessionCreationFailed
		}
		self.handle = handle
	}

	deinit {
		rsnap_scroll_session_destroy(handle)
	}

	public func observeDownwardFrame(_ frame: RGBARegionSnapshot) throws -> ScrollObserveResult {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outResult = RsnapScrollObserveResult()
		let status = frame.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_scroll_session_observe_downward_frame(
				handle,
				UInt32(max(frame.width, 0)),
				UInt32(max(frame.height, 0)),
				baseAddress,
				frame.rgba.count,
				&outResult
			)
		}
		try requireOk(status, context: "observing scroll-capture frame")

		return try decode(result: outResult)
	}

	public func exportImage() throws -> RGBARegionSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outRegion = RsnapOwnedRgbaRegion()
		let status = rsnap_scroll_session_take_export_rgba(handle, &outRegion)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		if code != 0 {
			throw HostBridgeError.ffiStatus(
				context: "taking scroll-capture export RGBA", code: code)
		}
		guard outRegion.len > 0, let rgba = outRegion.rgba else {
			return nil
		}
		let ownedRegion = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		ownedRegion.initialize(to: outRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: outRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(ownedRegion)
				ownedRegion.deinitialize(count: 1)
				ownedRegion.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(outRegion.width),
			height: Int(outRegion.height),
			rgba: data
		)
	}

	private func requireOk(_ status: RsnapStatus, context: String) throws {
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: context, code: code)
		}
	}

	private func decode(result: RsnapScrollObserveResult) throws -> ScrollObserveResult {
		guard let outcome = ScrollObserveOutcome(rawValue: result.kind) else {
			throw HostBridgeError.ffiStatus(
				context: "decoding scroll observation", code: result.kind)
		}
		return ScrollObserveResult(
			outcome: outcome,
			growthRows: Int(result.growth_rows),
			exportWidth: Int(result.export_width),
			exportHeight: Int(result.export_height),
			currentViewportTopY: Int(result.current_viewport_top_y)
		)
	}
}

public final class RsnapLiveSampler: @unchecked Sendable {
	private let handle: OpaquePointer
	private let stateLock = NSLock()

	public init(selfCaptureExceptionWindowIDs: [UInt32] = []) throws {
		let actualAbi = rsnap_host_ffi_abi_version()
		if actualAbi != RSNAP_HOST_FFI_ABI_VERSION {
			throw HostBridgeError.abiVersionMismatch(
				expected: RSNAP_HOST_FFI_ABI_VERSION,
				actual: actualAbi
			)
		}
		let handle: OpaquePointer?
		if selfCaptureExceptionWindowIDs.isEmpty {
			handle = rsnap_live_sampler_create()
		} else {
			handle = selfCaptureExceptionWindowIDs.withUnsafeBufferPointer { buffer in
				rsnap_live_sampler_create_with_self_capture_exception_window_ids(
					buffer.baseAddress,
					buffer.count
				)
			}
		}
		guard let handle else {
			throw HostBridgeError.sessionCreationFailed
		}
		self.handle = handle
	}

	deinit {
		rsnap_live_sampler_destroy(handle)
	}

	public func sampleCursor(
		monitor: MonitorSnapshot,
		point: CGPoint,
		patchSidePixels: Int
	) throws -> LiveSampleSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outSample = RsnapLiveSample()
		let status = rsnap_live_sampler_sample_cursor(
			handle,
			RsnapMonitorRect(
				id: monitor.id,
				origin: RsnapPoint(
					x: Int32(monitor.frame.origin.x.rounded()),
					y: Int32(monitor.frame.origin.y.rounded())
				),
				width: UInt32(max(monitor.frame.width.rounded(), 0)),
				height: UInt32(max(monitor.frame.height.rounded(), 0)),
				scale_factor_x1000: monitor.scaleFactorX1000
			),
			RsnapPoint(x: Int32(point.x.rounded()), y: Int32(point.y.rounded())),
			UInt32(max(patchSidePixels, 0)),
			UInt32(max(patchSidePixels, 0)),
			&outSample
		)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: "sampling live cursor", code: code)
		}

		let patchData: Data? = withUnsafeBytes(of: outSample.patch_rgba) { rawBuffer in
			let count = min(Int(outSample.patch_len), rawBuffer.count)
			guard count > 0 else {
				return nil
			}
			return Data(rawBuffer.prefix(count))
		}

		let frameAgeMicroseconds =
			outSample.has_frame_metadata == 0 ? nil : UInt64(outSample.frame_age_micros)
		let capturedAtUptime = frameAgeMicroseconds.map {
			ProcessInfo.processInfo.systemUptime - (Double($0) / 1_000_000.0)
		}

		return LiveSampleSnapshot(
			rgb: outSample.has_rgb == 0
				? nil : RGBSample(r: outSample.rgb.r, g: outSample.rgb.g, b: outSample.rgb.b),
			capturedAtUptime: capturedAtUptime,
			frameAgeMicroseconds: frameAgeMicroseconds,
			frameSequence: outSample.has_frame_metadata == 0
				? nil : UInt64(outSample.frame_seq),
			streamGeneration: outSample.has_frame_metadata == 0
				? nil : UInt64(outSample.stream_generation),
			patchWidth: Int(outSample.patch_width),
			patchHeight: Int(outSample.patch_height),
			patchRGBA: patchData
		)
	}

	public func primeMonitor(_ monitor: MonitorSnapshot) throws {
		stateLock.lock()
		defer { stateLock.unlock() }

		let status = rsnap_live_sampler_prime_monitor(
			handle,
			RsnapMonitorRect(
				id: monitor.id,
				origin: RsnapPoint(
					x: Int32(monitor.frame.origin.x.rounded()),
					y: Int32(monitor.frame.origin.y.rounded())
				),
				width: UInt32(max(monitor.frame.width.rounded(), 0)),
				height: UInt32(max(monitor.frame.height.rounded(), 0)),
				scale_factor_x1000: monitor.scaleFactorX1000
			)
		)
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: "priming live monitor", code: code)
		}
	}

	public func reset() throws {
		stateLock.lock()
		defer { stateLock.unlock() }

		let status = rsnap_live_sampler_reset(handle)
		let code = rsnap_status_code(status)
		if code != 0 {
			throw HostBridgeError.ffiStatus(context: "resetting live monitor sampler", code: code)
		}
	}

	public func peekRegion(
		monitor: MonitorSnapshot,
		rect: CGRect
	) throws -> RGBARegionSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		let encodedMonitor = RsnapMonitorRect(
			id: monitor.id,
			origin: RsnapPoint(
				x: Int32(monitor.frame.origin.x.rounded()),
				y: Int32(monitor.frame.origin.y.rounded())
			),
			width: UInt32(max(monitor.frame.width.rounded(), 0)),
			height: UInt32(max(monitor.frame.height.rounded(), 0)),
			scale_factor_x1000: monitor.scaleFactorX1000
		)
		let encodedRect = RsnapRect(
			x: Int32(rect.origin.x.rounded()),
			y: Int32(rect.origin.y.rounded()),
			width: UInt32(max(rect.width.rounded(), 0)),
			height: UInt32(max(rect.height.rounded(), 0))
		)
		var ownedRegion = RsnapOwnedRgbaRegion()
		let takeStatus = rsnap_live_sampler_take_region_rgba(
			handle,
			encodedMonitor,
			encodedRect,
			&ownedRegion
		)
		let takeCode = rsnap_status_code(takeStatus)
		if takeCode == 3 {
			return nil
		}
		if takeCode != 0 {
			throw HostBridgeError.ffiStatus(context: "taking live RGBA region", code: takeCode)
		}
		guard ownedRegion.len > 0, let rgba = ownedRegion.rgba else {
			return nil
		}
		let regionHandle = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		regionHandle.initialize(to: ownedRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: ownedRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(regionHandle)
				regionHandle.deinitialize(count: 1)
				regionHandle.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(ownedRegion.width),
			height: Int(ownedRegion.height),
			rgba: data
		)
	}

	/// Returns the live sampler's cache-only full-monitor snapshot.
	///
	/// This API does not expose the original frame capture time or stream sequence. Do not use it
	/// as a frozen screenshot source unless the FFI contract is extended to prove freshness.
	public func peekLatestMonitorImage(
		monitor: MonitorSnapshot
	) throws -> RGBARegionSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outRegion = RsnapOwnedRgbaRegion()
		let encodedMonitor = RsnapMonitorRect(
			id: monitor.id,
			origin: RsnapPoint(
				x: Int32(monitor.frame.origin.x.rounded()),
				y: Int32(monitor.frame.origin.y.rounded())
			),
			width: UInt32(max(monitor.frame.width.rounded(), 0)),
			height: UInt32(max(monitor.frame.height.rounded(), 0)),
			scale_factor_x1000: monitor.scaleFactorX1000
		)
		let status = rsnap_live_sampler_take_latest_monitor_rgba(
			handle,
			encodedMonitor,
			&outRegion
		)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		if code != 0 {
			throw HostBridgeError.ffiStatus(
				context: "peeking latest monitor RGBA snapshot", code: code)
		}
		guard outRegion.len > 0, let rgba = outRegion.rgba else {
			return nil
		}
		let ownedRegion = UnsafeMutablePointer<RsnapOwnedRgbaRegion>.allocate(capacity: 1)
		ownedRegion.initialize(to: outRegion)
		let data = Data(
			bytesNoCopy: rgba,
			count: outRegion.len,
			deallocator: .custom { _, _ in
				rsnap_owned_rgba_region_release(ownedRegion)
				ownedRegion.deinitialize(count: 1)
				ownedRegion.deallocate()
			}
		)
		return RGBARegionSnapshot(
			width: Int(outRegion.width),
			height: Int(outRegion.height),
			rgba: data
		)
	}
}
