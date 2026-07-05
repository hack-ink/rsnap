import CRsnapHostFFI
import CoreGraphics
import Foundation

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
