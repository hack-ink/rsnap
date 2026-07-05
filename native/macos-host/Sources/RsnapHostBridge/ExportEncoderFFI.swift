import CRsnapHostFFI
import CoreGraphics
import Foundation

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
