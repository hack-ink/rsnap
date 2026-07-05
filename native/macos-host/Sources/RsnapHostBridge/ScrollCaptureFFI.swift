import CRsnapHostFFI
import CoreGraphics
import Foundation

public struct ScrollMinimapLayoutPlan: Equatable, Sendable {
	public var frame: CGRect
	public var imageFrame: CGRect
	public var viewportFrame: CGRect?
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
