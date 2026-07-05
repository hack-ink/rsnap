import CRsnapHostFFI
import CoreGraphics
import Foundation

func rsnapOwnedRgbaSnapshot(from outRegion: RsnapOwnedRgbaRegion)
	-> RGBARegionSnapshot?
{
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

func rsnapRequireOk(_ status: RsnapStatus, context: String) throws {
	let code = rsnap_status_code(status)
	if code != 0 {
		throw HostBridgeError.ffiStatus(context: context, code: code)
	}
}

func rsnapFloatPoint(from point: CGPoint) -> RsnapFloatPoint {
	RsnapFloatPoint(x: Double(point.x), y: Double(point.y))
}

func cgPoint(from point: RsnapFloatPoint) -> CGPoint {
	CGPoint(x: point.x, y: point.y)
}

func rsnapFloatRect(from rect: CGRect) -> RsnapFloatRect {
	RsnapFloatRect(
		x: Double(rect.origin.x),
		y: Double(rect.origin.y),
		width: Double(rect.width),
		height: Double(rect.height)
	)
}

func cgRect(from rect: RsnapFloatRect) -> CGRect {
	CGRect(
		x: rect.x,
		y: rect.y,
		width: rect.width,
		height: rect.height
	)
}

func cgRect(from rect: RsnapPixelRect) -> CGRect {
	CGRect(
		x: Int(rect.x),
		y: Int(rect.y),
		width: Int(rect.width),
		height: Int(rect.height)
	)
}
