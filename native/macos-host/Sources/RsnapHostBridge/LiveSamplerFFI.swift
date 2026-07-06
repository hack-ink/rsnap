import CRsnapHostFFI
import CoreGraphics
import Foundation

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

}
