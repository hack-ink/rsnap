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
