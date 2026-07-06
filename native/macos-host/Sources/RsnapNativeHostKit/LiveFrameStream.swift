import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

package struct RGBARegionFrameSnapshot: Equatable, Sendable {
	package let frameSequence: UInt64
	package let frameAgeMicroseconds: UInt64
	package let region: RGBARegionSnapshot

	package init(frameSequence: UInt64, frameAgeMicroseconds: UInt64, region: RGBARegionSnapshot) {
		self.frameSequence = frameSequence
		self.frameAgeMicroseconds = frameAgeMicroseconds
		self.region = region
	}
}

final class LiveFrameStreamBroker: @unchecked Sendable {
	private static let primeThrottleInterval: TimeInterval = 1.0 / 120.0

	private let stateLock = NSLock()
	private let frozenFrameAuthority: FrozenFrameAuthority
	private var includedCurrentProcessWindowIDs: Set<CGWindowID> = []
	private var screens: [NSScreen] = []
	private var lastPrimedDisplayID: CGDirectDisplayID?
	private var lastPrimeUptime: TimeInterval = 0

	init(frozenFrameAuthority: FrozenFrameAuthority) {
		self.frozenFrameAuthority = frozenFrameAuthority
	}

	func prepareAuthority(reason: String = "unspecified") {
		let startedAt = ProcessInfo.processInfo.systemUptime
		NativeHostTelemetry.liveStreamAuthorityPrepared(
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAt),
			reason: reason
		)
	}

	func updateSelfCaptureExceptionWindowIDs(
		_ windowIDs: Set<CGWindowID>,
		captureID: UInt64 = 0
	) {
		stateLock.lock()
		let previousWindowCount = includedCurrentProcessWindowIDs.count
		guard windowIDs != includedCurrentProcessWindowIDs else {
			stateLock.unlock()
			return
		}
		includedCurrentProcessWindowIDs = windowIDs
		let screens = self.screens
		lastPrimedDisplayID = nil
		lastPrimeUptime = 0
		stateLock.unlock()
		NativeHostTelemetry.liveStreamSelfCaptureExceptionUpdate(
			captureID: captureID,
			previousWindowCount: previousWindowCount,
			nextWindowCount: windowIDs.count,
			samplerRebuilt: false
		)
		guard screens.isEmpty == false else {
			return
		}
		start(
			for: screens,
			prewarmPoint: nil,
			captureID: captureID
		)
	}

	func start(for screens: [NSScreen], prewarmPoint: CGPoint? = nil, captureID: UInt64 = 0) {
		stateLock.lock()
		self.screens = screens
		let includedWindowIDs = includedCurrentProcessWindowIDs
		stateLock.unlock()
		frozenFrameAuthority.start(
			for: screens,
			captureID: captureID,
			source: "live_region_stream",
			rebuildContentFilter: true,
			includedCurrentProcessWindowIDs: includedWindowIDs
		)
		prime(at: prewarmPoint)
	}

	func stop() {
		stateLock.lock()
		screens.removeAll()
		lastPrimedDisplayID = nil
		lastPrimeUptime = 0
		stateLock.unlock()
		frozenFrameAuthority.stop()
	}

	func nextRegionFrame(
		in rect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		frozenFrameAuthority.nextRegionFrame(
			in: rect,
			afterFrameSequence: afterFrameSequence,
			waitForFresh: waitForFresh
		)
	}

	func nextRegionFrame(
		in rect: CGRect,
		pixelRect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		frozenFrameAuthority.nextRegionFrame(
			in: rect,
			pixelRect: pixelRect,
			afterFrameSequence: afterFrameSequence,
			waitForFresh: waitForFresh
		)
	}

	func prime(at point: CGPoint?) {
		guard let point, let displayID = displayID(containing: point) else {
			return
		}
		stateLock.lock()
		let now = ProcessInfo.processInfo.systemUptime
		if lastPrimedDisplayID == displayID,
			now - lastPrimeUptime < Self.primeThrottleInterval
		{
			stateLock.unlock()
			return
		}
		lastPrimedDisplayID = displayID
		lastPrimeUptime = now
		let screens = self.screens
		let includedWindowIDs = includedCurrentProcessWindowIDs
		stateLock.unlock()
		guard screens.isEmpty == false else {
			return
		}
		frozenFrameAuthority.start(
			for: screens,
			captureID: 0,
			source: "live_region_prime",
			rebuildContentFilter: false,
			includedCurrentProcessWindowIDs: includedWindowIDs
		)
	}

	private func displayID(containing point: CGPoint) -> CGDirectDisplayID? {
		stateLock.lock()
		let screens = self.screens
		stateLock.unlock()
		return screens.first(where: { $0.frame.inclusivelyContains(point) })?.nativeDisplayID
	}
}
