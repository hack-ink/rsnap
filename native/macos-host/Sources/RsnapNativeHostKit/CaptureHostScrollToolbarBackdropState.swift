import CoreGraphics
import Foundation

package struct CaptureHostGlassPatchCache {
	package let frame: CGRect
	package let capturedAt: TimeInterval
	package let image: CGImage

	package init(frame: CGRect, capturedAt: TimeInterval, image: CGImage) {
		self.frame = frame
		self.capturedAt = capturedAt
		self.image = image
	}
}

package struct CaptureHostScrollToolbarBackdropCaptureStart: Equatable {
	package let generation: UInt64
	package let afterFrameSequence: UInt64
	package let previousSignature: UInt64?
	package let fallbackPermitted: Bool
}

package struct CaptureHostScrollToolbarBackdropChange: Equatable {
	package let count: UInt64
	package let gapMilliseconds: Double?

	package init(count: UInt64, gapMilliseconds: Double?) {
		self.count = count
		self.gapMilliseconds = gapMilliseconds
	}
}

package struct CaptureHostScrollToolbarBackdropRefresh: Equatable {
	package let gapMilliseconds: Double?
	package let activeFrame: CGRect?
	package let activeGlobalFrame: CGRect?

	package init(
		gapMilliseconds: Double?,
		activeFrame: CGRect?,
		activeGlobalFrame: CGRect?
	) {
		self.gapMilliseconds = gapMilliseconds
		self.activeFrame = activeFrame
		self.activeGlobalFrame = activeGlobalFrame
	}
}

package struct CaptureHostScrollToolbarBackdropState {
	package private(set) var captureInFlight = false
	package private(set) var captureGeneration: UInt64 = 0
	package private(set) var seedFrame: CGRect?
	package private(set) var seedImage: CGImage?
	package private(set) var seedPatchCache: CaptureHostGlassPatchCache?
	package private(set) var lastFrameSequence: UInt64 = 0
	package private(set) var lastSignature: UInt64?
	package private(set) var changedCount: UInt64 = 0
	package private(set) var activeFrame: CGRect?
	package private(set) var activeGlobalFrame: CGRect?
	package private(set) var lastChangedUptime: TimeInterval = 0
	package private(set) var lastFallbackCaptureStartedUptime: TimeInterval = 0
	package private(set) var lastCaptureStartedUptime: TimeInterval = 0
	package private(set) var lastRefreshUptime: TimeInterval = 0

	package init() {}

	package mutating func resetTracking(seedFrame: CGRect? = nil, seedImage: CGImage? = nil) {
		self.seedFrame = seedFrame
		self.seedImage = seedImage
		seedPatchCache = nil
		lastFrameSequence = 0
		lastSignature = nil
		changedCount = 0
		activeFrame = nil
		activeGlobalFrame = nil
		lastChangedUptime = 0
		lastFallbackCaptureStartedUptime = 0
		lastRefreshUptime = 0
	}

	package mutating func resetAndInvalidateCaptures() {
		captureGeneration &+= 1
		captureInFlight = false
		lastCaptureStartedUptime = 0
		resetTracking()
	}

	package mutating func clearActiveFrame() {
		lastCaptureStartedUptime = 0
		activeFrame = nil
		activeGlobalFrame = nil
	}

	package mutating func updateActiveFrame(_ frame: CGRect, globalFrame: CGRect) {
		activeFrame = frame
		activeGlobalFrame = globalFrame
	}

	package func cachedSeedPatch(matching globalFrame: CGRect) -> CGImage? {
		guard let seedPatchCache,
			abs(seedPatchCache.frame.minX - globalFrame.minX) < 1,
			abs(seedPatchCache.frame.minY - globalFrame.minY) < 1,
			abs(seedPatchCache.frame.width - globalFrame.width) < 1,
			abs(seedPatchCache.frame.height - globalFrame.height) < 1
		else {
			return nil
		}
		return seedPatchCache.image
	}

	package mutating func storeSeedPatch(
		frame: CGRect,
		capturedAt: TimeInterval,
		image: CGImage
	) {
		seedPatchCache = CaptureHostGlassPatchCache(
			frame: frame,
			capturedAt: capturedAt,
			image: image
		)
	}

	package mutating func beginCapture(
		now: TimeInterval,
		minimumInterval: TimeInterval,
		fallbackMinimumInterval: TimeInterval
	) -> CaptureHostScrollToolbarBackdropCaptureStart? {
		guard captureInFlight == false else {
			return nil
		}
		guard
			lastCaptureStartedUptime == 0
				|| now - lastCaptureStartedUptime >= minimumInterval
		else {
			return nil
		}
		lastCaptureStartedUptime = now
		let fallbackPermitted =
			lastFallbackCaptureStartedUptime == 0
			|| now - lastFallbackCaptureStartedUptime >= fallbackMinimumInterval
		if fallbackPermitted {
			lastFallbackCaptureStartedUptime = now
		}
		captureInFlight = true
		captureGeneration &+= 1
		return CaptureHostScrollToolbarBackdropCaptureStart(
			generation: captureGeneration,
			afterFrameSequence: lastFrameSequence,
			previousSignature: lastSignature,
			fallbackPermitted: fallbackPermitted
		)
	}

	package mutating func clearInFlightForAbandonedCapture() {
		captureInFlight = false
	}

	package mutating func finishCapture(
		generation: UInt64,
		frameSequence: UInt64?
	) -> Bool {
		guard captureGeneration == generation else {
			return false
		}
		captureInFlight = false
		if let frameSequence {
			lastFrameSequence = max(lastFrameSequence, frameSequence)
		}
		return true
	}

	package mutating func recordChange(
		signature: UInt64?,
		now: TimeInterval
	) -> CaptureHostScrollToolbarBackdropChange? {
		guard let signature else {
			return nil
		}
		let previousSignature = lastSignature
		lastSignature = signature
		guard previousSignature != signature else {
			return nil
		}
		changedCount &+= 1
		let gapMilliseconds =
			lastChangedUptime > 0 ? (now - lastChangedUptime) * 1_000 : nil
		lastChangedUptime = now
		return CaptureHostScrollToolbarBackdropChange(
			count: changedCount,
			gapMilliseconds: gapMilliseconds
		)
	}

	package mutating func beginRefresh(
		now: TimeInterval,
		interval: TimeInterval
	) -> CaptureHostScrollToolbarBackdropRefresh? {
		guard now - lastRefreshUptime >= interval else {
			return nil
		}
		let gapMilliseconds = lastRefreshUptime > 0 ? (now - lastRefreshUptime) * 1_000 : nil
		lastRefreshUptime = now
		return CaptureHostScrollToolbarBackdropRefresh(
			gapMilliseconds: gapMilliseconds,
			activeFrame: activeFrame,
			activeGlobalFrame: activeGlobalFrame
		)
	}
}
