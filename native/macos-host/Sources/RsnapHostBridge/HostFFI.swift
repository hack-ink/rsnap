import CRsnapHostFFI
import CoreGraphics
import Foundation

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

public struct RGBARegionFrameSnapshot: Equatable, Sendable {
	public var frameSequence: UInt64
	public var frameAgeMicroseconds: UInt64
	public var region: RGBARegionSnapshot

	public init(
		frameSequence: UInt64,
		frameAgeMicroseconds: UInt64,
		region: RGBARegionSnapshot
	) {
		self.frameSequence = frameSequence
		self.frameAgeMicroseconds = frameAgeMicroseconds
		self.region = region
	}
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
			rsnapFloatRect(from: selection),
			Double(exportSize.width),
			Double(exportSize.height),
			rsnapFloatRect(from: bounds),
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
		try rsnapRequireOk(status, context: "resolving scroll minimap layout plan")
		let viewportFrame =
			outPlan.has_viewport_frame != 0 ? cgRect(from: outPlan.viewport_frame) : nil

		return ScrollMinimapLayoutPlan(
			frame: cgRect(from: outPlan.frame),
			imageFrame: cgRect(from: outPlan.image_frame),
			viewportFrame: viewportFrame
		)
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
			rsnapFloatRect(from: selection),
			Double(handleRadius),
			Double(edgeTolerance),
			&outKind
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "hit-testing frozen selection transform")

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
			rsnapFloatRect(from: initialSelection),
			rsnapFloatRect(from: monitorFrame),
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
		try rsnapRequireOk(status, context: "resolving frozen selection transform")

		return cgRect(from: outRect)
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
		try rsnapRequireOk(status, context: "detecting auto-center content bounds")

		return cgRect(from: outRect)
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
			rsnapFloatRect(from: displayFrame),
			Double(point.x),
			Double(point.y),
			&outRGB
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "sampling BGRA frame RGB")

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
			rsnapFloatRect(from: displayFrame),
			Double(point.x),
			Double(point.y),
			UInt32(max(sidePixels, 0)),
			&outRegion
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "sampling BGRA frame loupe patch")

		return rsnapOwnedRgbaSnapshot(from: outRegion)
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
		try rsnapRequireOk(status, context: "encoding export PNG")

		return try data(from: outPNG, context: "taking encoded export PNG")
	}

	public static func pngData(
		from image: RGBARegionSnapshot,
		screenScaleFactor: CGFloat
	) throws -> Data {
		let scaleFactorX1000 = encode(screenScaleFactor: screenScaleFactor)
		var outPNG = RsnapOwnedBytes()
		let status = image.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_export_rgba_to_png_with_screen_scale(
				UInt32(max(image.width, 0)),
				UInt32(max(image.height, 0)),
				baseAddress,
				image.rgba.count,
				scaleFactorX1000,
				&outPNG
			)
		}
		try rsnapRequireOk(status, context: "encoding export PNG with screen scale")

		return try data(from: outPNG, context: "taking encoded export PNG with screen scale")
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
		try rsnapRequireOk(status, context: "encoding cropped export PNG")

		return try data(from: outPNG, context: "taking encoded cropped export PNG")
	}

	public static func pngData(
		from image: RGBARegionSnapshot,
		crop: CGRect,
		screenScaleFactor: CGFloat
	) throws -> Data {
		let cropRect = try encode(crop: crop)
		let scaleFactorX1000 = encode(screenScaleFactor: screenScaleFactor)
		var outPNG = RsnapOwnedBytes()
		let status = image.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_export_rgba_crop_to_png_with_screen_scale(
				UInt32(max(image.width, 0)),
				UInt32(max(image.height, 0)),
				baseAddress,
				image.rgba.count,
				cropRect,
				scaleFactorX1000,
				&outPNG
			)
		}
		try rsnapRequireOk(status, context: "encoding cropped export PNG with screen scale")

		return try data(
			from: outPNG, context: "taking encoded cropped export PNG with screen scale")
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
			rsnapFloatRect(from: displayFrame),
			rsnapFloatRect(from: selection),
			&outRect
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "resolving frozen display export crop")

		return cgRect(from: outRect)
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
			rsnapFloatRect(from: sourceRect),
			&outRegion
		)
		let code = rsnap_status_code(status)
		if code == RSNAP_STATUS_EMPTY.rawValue {
			return nil
		}
		try rsnapRequireOk(status, context: "rendering frozen mosaic privacy patch")

		return rsnapOwnedRgbaSnapshot(from: outRegion)
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
					rsnapFloatRect(from: selection),
					elementBuffer.baseAddress,
					elementBuffer.count,
					&outRegion
				)
			}
		}
		try rsnapRequireOk(status, context: "rendering frozen overlay export image")
		guard let snapshot = rsnapOwnedRgbaSnapshot(from: outRegion) else {
			throw HostBridgeError.ffiStatus(
				context: "taking frozen overlay export image",
				code: RSNAP_STATUS_EMPTY.rawValue)
		}

		return snapshot
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

	private static func encode(screenScaleFactor: CGFloat) -> UInt32 {
		let scale = screenScaleFactor.isFinite ? max(screenScaleFactor, 1) : 1
		let scaled = min((scale * 1_000).rounded(), CGFloat(UInt32.max))

		return UInt32(scaled)
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
		try rsnapRequireOk(status, context: "observing scroll-capture frame")

		return try decode(result: outResult)
	}

	public func observeDownwardFrame(
		_ frame: RGBARegionSnapshot,
		motionRowsHint: Int?,
		allowBurstSearch: Bool = true
	) throws -> ScrollObserveResult {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outResult = RsnapScrollObserveResult()
		let hint = UInt32(max(motionRowsHint ?? 0, 0))
		let status = frame.rgba.withUnsafeBytes { buffer -> RsnapStatus in
			guard let baseAddress = buffer.bindMemory(to: UInt8.self).baseAddress else {
				return RSNAP_STATUS_INVALID_INPUT
			}
			return rsnap_scroll_session_observe_downward_frame_with_motion_hint(
				handle,
				UInt32(max(frame.width, 0)),
				UInt32(max(frame.height, 0)),
				baseAddress,
				frame.rgba.count,
				hint,
				allowBurstSearch ? 1 : 0,
				&outResult
			)
		}
		try rsnapRequireOk(status, context: "observing scroll-capture frame with motion hint")

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
		return rsnapOwnedRgbaSnapshot(from: outRegion)
	}

	public func previewImage() throws -> RGBARegionSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		var outRegion = RsnapOwnedRgbaRegion()
		let status = rsnap_scroll_session_take_preview_rgba(handle, &outRegion)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		if code != 0 {
			throw HostBridgeError.ffiStatus(
				context: "taking scroll-capture preview RGBA", code: code)
		}
		return rsnapOwnedRgbaSnapshot(from: outRegion)
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
		return rsnapOwnedRgbaSnapshot(from: ownedRegion)
	}

	public func nextRegionFrame(
		monitor: MonitorSnapshot,
		rect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) throws -> RGBARegionFrameSnapshot? {
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
		var frameSequence: UInt64 = 0
		var frameAgeMicroseconds: UInt64 = 0
		var ownedRegion = RsnapOwnedRgbaRegion()
		let status = rsnap_live_sampler_take_next_region_rgba_after_seq(
			handle,
			encodedMonitor,
			encodedRect,
			afterFrameSequence,
			UInt8(waitForFresh ? 1 : 0),
			&frameSequence,
			&frameAgeMicroseconds,
			&ownedRegion
		)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		if code != 0 {
			throw HostBridgeError.ffiStatus(
				context: "taking next live RGBA region frame",
				code: code
			)
		}
		guard let region = rsnapOwnedRgbaSnapshot(from: ownedRegion) else {
			return nil
		}
		return RGBARegionFrameSnapshot(
			frameSequence: frameSequence,
			frameAgeMicroseconds: frameAgeMicroseconds,
			region: region
		)
	}

	public func nextRegionFrame(
		monitor: MonitorSnapshot,
		pixelRect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) throws -> RGBARegionFrameSnapshot? {
		stateLock.lock()
		defer { stateLock.unlock() }

		let encodedPixelRect = try Self.encode(
			pixelRect: pixelRect,
			context: "encoding live RGBA pixel region")
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
		var frameSequence: UInt64 = 0
		var frameAgeMicroseconds: UInt64 = 0
		var ownedRegion = RsnapOwnedRgbaRegion()
		let status = rsnap_live_sampler_take_next_region_rgba_pixels_after_seq(
			handle,
			encodedMonitor,
			encodedPixelRect,
			afterFrameSequence,
			UInt8(waitForFresh ? 1 : 0),
			&frameSequence,
			&frameAgeMicroseconds,
			&ownedRegion
		)
		let code = rsnap_status_code(status)
		if code == 3 {
			return nil
		}
		if code != 0 {
			throw HostBridgeError.ffiStatus(
				context: "taking next live RGBA pixel region frame",
				code: code
			)
		}
		guard let region = rsnapOwnedRgbaSnapshot(from: ownedRegion) else {
			return nil
		}
		return RGBARegionFrameSnapshot(
			frameSequence: frameSequence,
			frameAgeMicroseconds: frameAgeMicroseconds,
			region: region
		)
	}

	private static func encode(pixelRect: CGRect, context: String) throws -> RsnapPixelRect {
		let x = pixelRect.origin.x.rounded()
		let y = pixelRect.origin.y.rounded()
		let width = pixelRect.width.rounded()
		let height = pixelRect.height.rounded()
		let maxValue = CGFloat(UInt32.max)

		guard
			x >= 0,
			y >= 0,
			width > 0,
			height > 0,
			x <= maxValue,
			y <= maxValue,
			width <= maxValue,
			height <= maxValue
		else {
			throw HostBridgeError.ffiStatus(
				context: context,
				code: RSNAP_STATUS_INVALID_INPUT.rawValue)
		}

		return RsnapPixelRect(
			x: UInt32(x),
			y: UInt32(y),
			width: UInt32(width),
			height: UInt32(height)
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
		return rsnapOwnedRgbaSnapshot(from: outRegion)
	}
}
