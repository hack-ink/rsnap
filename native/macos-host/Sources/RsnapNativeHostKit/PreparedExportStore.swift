import CoreGraphics
import Foundation
import RsnapHostBridge

struct ActiveScrollCaptureExportSnapshot: @unchecked Sendable {
	let snapshot: RGBARegionSnapshot
	let revision: UInt64
}

struct FrozenSelectionImageRenderRequest: @unchecked Sendable {
	let captureID: UInt64
	let selection: CGRect
	let scrollExportSnapshot: RGBARegionSnapshot?
	let scrollExportRevision: UInt64
	let frozenDisplayFrame: CGRect?
	let frozenDisplayImage: CGImage?
	let frozenBaseImage: CGImage?
	let frozenSelectionSnapshot: CGRect?
	let overlayElements: [FrozenOverlayExportElement]
	let hasOverlayEdits: Bool
	let captureFrameSource: CaptureFrameSource
	let captureFrameWindowID: CGWindowID?
	let captureFrameEffectEnabled: Bool
	let captureFrameBackground: CaptureFrameBackgroundPreference
	let captureFrameApplicability: CaptureFrameApplicabilityPreference
	let captureFrameEnvironment: CaptureFrameRenderEnvironment

	var preparedExportKey: FrozenPreparedExportKey {
		FrozenPreparedExportKey(
			captureID: captureID,
			selection: selection,
			scrollExportWidth: scrollExportSnapshot?.width ?? 0,
			scrollExportHeight: scrollExportSnapshot?.height ?? 0,
			scrollExportRevision: scrollExportRevision,
			frozenDisplayFrame: frozenDisplayFrame,
			frozenBaseWidth: frozenBaseImage?.width ?? 0,
			frozenBaseHeight: frozenBaseImage?.height ?? 0,
			frozenSelectionSnapshot: frozenSelectionSnapshot,
			overlayElements: overlayElements,
			hasOverlayEdits: hasOverlayEdits,
			captureFrameSource: captureFrameSource,
			captureFrameWindowID: captureFrameWindowID,
			captureFrameEffectEnabled: captureFrameEffectEnabled,
			captureFrameBackground: captureFrameBackground,
			captureFrameApplicability: captureFrameApplicability,
			captureFrameEnvironment: captureFrameEnvironment
		)
	}

	var canPrepareExportInBackground: Bool {
		true
	}
}

struct FrozenPreparedExportKey: Equatable, @unchecked Sendable {
	let captureID: UInt64
	let selection: CGRect
	let scrollExportWidth: Int
	let scrollExportHeight: Int
	let scrollExportRevision: UInt64
	let frozenDisplayFrame: CGRect?
	let frozenBaseWidth: Int
	let frozenBaseHeight: Int
	let frozenSelectionSnapshot: CGRect?
	let overlayElements: [FrozenOverlayExportElement]
	let hasOverlayEdits: Bool
	let captureFrameSource: CaptureFrameSource
	let captureFrameWindowID: CGWindowID?
	let captureFrameEffectEnabled: Bool
	let captureFrameBackground: CaptureFrameBackgroundPreference
	let captureFrameApplicability: CaptureFrameApplicabilityPreference
	let captureFrameEnvironment: CaptureFrameRenderEnvironment
}

struct FrozenSelectionImageRenderResult: @unchecked Sendable {
	let image: CGImage?
	let rgbaSnapshot: RGBARegionSnapshot?
	let baseImage: CGImage?
	let failureStage: String?
	let ensureMilliseconds: Double
	let refreshMilliseconds: Double
	let compositeMilliseconds: Double
	let source: String
	let hasOverlayEdits: Bool
	let width: Int
	let height: Int
}

struct FrozenRenderedImage: @unchecked Sendable {
	let image: CGImage?
	let rgbaSnapshot: RGBARegionSnapshot?

	var width: Int {
		rgbaSnapshot?.width ?? image?.width ?? 0
	}

	var height: Int {
		rgbaSnapshot?.height ?? image?.height ?? 0
	}
}

struct CopyCaptureJobResult: @unchecked Sendable {
	let pngData: Data?
	let failureStage: String
	let failureMessage: String
	let captureImageMilliseconds: Double
	let makeImageMilliseconds: Double
	let width: Int
	let height: Int
	let cacheHit: Bool

	var preparedCacheHit: Self {
		Self(
			pngData: pngData,
			failureStage: failureStage,
			failureMessage: failureMessage,
			captureImageMilliseconds: 0,
			makeImageMilliseconds: 0,
			width: width,
			height: height,
			cacheHit: true
		)
	}
}

struct SaveCaptureJobResult: @unchecked Sendable {
	let outputURL: URL?
	let failureMessage: String
	let failureStage: String
	let captureImageMilliseconds: Double
	let makeImageMilliseconds: Double
	let writeFileMilliseconds: Double
	let width: Int
	let height: Int
	let cacheHit: Bool
}

struct PreparedRecognizeTextCaptureImage: @unchecked Sendable {
	let image: CGImage
	let captureImageMilliseconds: Double
	let cacheHit: Bool
}

private struct FrozenPreparedExportEntry: @unchecked Sendable {
	let key: FrozenPreparedExportKey
	let result: CopyCaptureJobResult
}

struct PreparedRecognizeTextImageJobResult: @unchecked Sendable {
	let image: CGImage?
	let baseImage: CGImage?
	let captureImageMilliseconds: Double
	let width: Int
	let height: Int
}

private struct FrozenPreparedRecognizeTextImageEntry: @unchecked Sendable {
	let key: FrozenPreparedExportKey
	let image: CGImage
	let width: Int
	let height: Int
}

final class PreparedExportStore: @unchecked Sendable {
	private let lock = NSLock()
	private var generation: UInt64 = 0
	private var inFlightKey: FrozenPreparedExportKey?
	private var entry: FrozenPreparedExportEntry?

	func reset() {
		invalidate()
	}

	func invalidate() {
		lock.lock()
		generation &+= 1
		inFlightKey = nil
		entry = nil
		lock.unlock()
	}

	func beginPreparing(for request: FrozenSelectionImageRenderRequest) -> UInt64? {
		let key = request.preparedExportKey
		lock.lock()
		defer {
			lock.unlock()
		}
		if entry?.key == key || inFlightKey == key {
			return nil
		}
		inFlightKey = key
		return generation
	}

	func preparationIsCurrent(
		for request: FrozenSelectionImageRenderRequest,
		generation preparedGeneration: UInt64
	) -> Bool {
		let key = request.preparedExportKey
		lock.lock()
		let isCurrent = generation == preparedGeneration && inFlightKey == key
		lock.unlock()
		return isCurrent
	}

	func finishPreparing(
		for request: FrozenSelectionImageRenderRequest,
		generation preparedGeneration: UInt64,
		result: CopyCaptureJobResult
	) {
		let key = request.preparedExportKey
		lock.lock()
		defer {
			lock.unlock()
		}
		guard generation == preparedGeneration, inFlightKey == key else {
			return
		}
		inFlightKey = nil
		guard result.pngData != nil else {
			entry = nil
			return
		}
		entry = FrozenPreparedExportEntry(key: key, result: result)
	}

	func result(matching request: FrozenSelectionImageRenderRequest) -> CopyCaptureJobResult? {
		let key = request.preparedExportKey
		lock.lock()
		let result = entry?.key == key ? entry?.result.preparedCacheHit : nil
		lock.unlock()
		return result
	}
}

final class FrozenPreparedRecognizeTextImageStore: @unchecked Sendable {
	private let lock = NSLock()
	private var generation: UInt64 = 0
	private var inFlightKey: FrozenPreparedExportKey?
	private var entry: FrozenPreparedRecognizeTextImageEntry?

	func reset() {
		invalidate()
	}

	func invalidate() {
		lock.lock()
		generation &+= 1
		inFlightKey = nil
		entry = nil
		lock.unlock()
	}

	func beginPreparing(for request: FrozenSelectionImageRenderRequest) -> UInt64? {
		let key = request.preparedExportKey
		lock.lock()
		defer {
			lock.unlock()
		}
		if entry?.key == key || inFlightKey == key {
			return nil
		}
		inFlightKey = key
		return generation
	}

	func preparationIsCurrent(
		for request: FrozenSelectionImageRenderRequest,
		generation preparedGeneration: UInt64
	) -> Bool {
		let key = request.preparedExportKey
		lock.lock()
		let isCurrent = generation == preparedGeneration && inFlightKey == key
		lock.unlock()
		return isCurrent
	}

	func finishPreparing(
		for request: FrozenSelectionImageRenderRequest,
		generation preparedGeneration: UInt64,
		result: PreparedRecognizeTextImageJobResult
	) {
		let key = request.preparedExportKey
		lock.lock()
		defer {
			lock.unlock()
		}
		guard generation == preparedGeneration, inFlightKey == key else {
			return
		}
		inFlightKey = nil
		guard let image = result.image else {
			entry = nil
			return
		}
		entry = FrozenPreparedRecognizeTextImageEntry(
			key: key,
			image: image,
			width: result.width,
			height: result.height
		)
	}

	func result(matching request: FrozenSelectionImageRenderRequest) -> CGImage? {
		let key = request.preparedExportKey
		lock.lock()
		let image = entry?.key == key ? entry?.image : nil
		lock.unlock()
		return image
	}
}
