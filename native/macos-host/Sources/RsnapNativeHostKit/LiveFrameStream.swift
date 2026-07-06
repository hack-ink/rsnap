import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

final class LiveFrameStreamBroker: @unchecked Sendable {
	private struct SamplerMonitor: Equatable {
		let id: UInt32
		let appKitFrame: CGRect
		let quartzFrame: CGRect
		let scaleFactorX1000: UInt32
	}

	private static let primeThrottleInterval: TimeInterval = 1.0 / 120.0

	private let stateLock = NSLock()
	private var sampler: RsnapLiveSampler?
	private var selfCaptureExceptionWindowIDs: Set<CGWindowID> = []
	private var monitors: [SamplerMonitor] = []
	private var mainDisplayHeight: CGFloat = 0
	private var streamGeneration: UInt64 = 0
	private var lastPrimedMonitorID: UInt32?
	private var lastPrimeGeneration: UInt64 = 0
	private var lastPrimeUptime: TimeInterval = 0

	func prepareSampler(reason: String = "unspecified") {
		let startedAt = ProcessInfo.processInfo.systemUptime
		stateLock.lock()
		let created: Bool
		if sampler == nil {
			sampler = Self.makeSampler(exceptionWindowIDs: selfCaptureExceptionWindowIDs)
			created = sampler != nil
		} else {
			created = false
		}
		stateLock.unlock()
		NativeHostTelemetry.liveStreamSamplerPrepared(
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAt),
			created: created,
			reason: reason
		)
	}

	func updateSelfCaptureExceptionWindowIDs(
		_ windowIDs: Set<CGWindowID>,
		captureID: UInt64 = 0
	) {
		stateLock.lock()
		let previousWindowCount = selfCaptureExceptionWindowIDs.count
		guard windowIDs != selfCaptureExceptionWindowIDs else {
			stateLock.unlock()
			return
		}
		selfCaptureExceptionWindowIDs = windowIDs
		streamGeneration &+= 1
		monitors.removeAll()
		mainDisplayHeight = 0
		lastPrimedMonitorID = nil
		lastPrimeGeneration = 0
		lastPrimeUptime = 0
		let oldSampler = sampler
		if oldSampler != nil {
			sampler = Self.makeSampler(exceptionWindowIDs: windowIDs)
		}
		stateLock.unlock()
		NativeHostTelemetry.liveStreamSelfCaptureExceptionUpdate(
			captureID: captureID,
			previousWindowCount: previousWindowCount,
			nextWindowCount: windowIDs.count,
			samplerRebuilt: oldSampler != nil
		)
		try? oldSampler?.reset()
	}

	func start(for screens: [NSScreen], prewarmPoint: CGPoint? = nil, captureID: UInt64 = 0) {
		stateLock.lock()
		if sampler == nil {
			sampler = Self.makeSampler(exceptionWindowIDs: selfCaptureExceptionWindowIDs)
		}
		let mainDisplayHeight = Self.mainDisplayHeight(for: screens)
		self.mainDisplayHeight = mainDisplayHeight
		let nextMonitors = screens.compactMap {
			Self.monitorSnapshot(for: $0, mainDisplayHeight: mainDisplayHeight)
		}
		let targetMonitor = prewarmPoint.flatMap { point in
			nextMonitors.first(where: { $0.appKitFrame.inclusivelyContains(point) })
		}
		let monitorsUnchanged = nextMonitors == monitors
		monitors = nextMonitors
		if monitorsUnchanged {
			stateLock.unlock()
			if let targetMonitor {
				prime(monitor: targetMonitor)
			}
			return
		}
		streamGeneration &+= 1
		stateLock.unlock()
		if let targetMonitor {
			prime(monitor: targetMonitor)
		}
	}

	func stop() {
		stateLock.lock()
		streamGeneration &+= 1
		monitors.removeAll()
		mainDisplayHeight = 0
		lastPrimedMonitorID = nil
		lastPrimeGeneration = 0
		lastPrimeUptime = 0
		let sampler = self.sampler
		stateLock.unlock()
		guard let sampler else {
			return
		}
		try? sampler.reset()
	}

	func patch(in rect: CGRect) -> CGImage? {
		region(in: rect)
	}

	func region(in rect: CGRect) -> CGImage? {
		guard let monitor = monitor(containing: CGPoint(x: rect.midX, y: rect.midY)) else {
			return nil
		}
		stateLock.lock()
		let sampler = self.sampler
		let mainDisplayHeight = self.mainDisplayHeight
		let encodedMonitor = samplerMonitorSnapshot(for: monitor)
		stateLock.unlock()
		guard let sampler else {
			return nil
		}
		let quartzRect = Self.appKitRectToQuartz(rect, mainDisplayHeight: mainDisplayHeight)
		guard let snapshot = try? sampler.peekRegion(monitor: encodedMonitor, rect: quartzRect)
		else {
			return nil
		}

		return NativeHostImageBridge.cgImage(
			width: snapshot.width,
			height: snapshot.height,
			rgba: snapshot.rgba
		)
	}

	func nextRegionFrame(
		in rect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		guard let monitor = monitor(containing: CGPoint(x: rect.midX, y: rect.midY)) else {
			return nil
		}
		stateLock.lock()
		let sampler = self.sampler
		let mainDisplayHeight = self.mainDisplayHeight
		let encodedMonitor = samplerMonitorSnapshot(for: monitor)
		stateLock.unlock()
		guard let sampler else {
			return nil
		}
		let quartzRect = Self.appKitRectToQuartz(rect, mainDisplayHeight: mainDisplayHeight)
		return try? sampler.nextRegionFrame(
			monitor: encodedMonitor,
			rect: quartzRect,
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
		guard let monitor = monitor(containing: CGPoint(x: rect.midX, y: rect.midY)) else {
			return nil
		}
		stateLock.lock()
		let sampler = self.sampler
		let encodedMonitor = samplerMonitorSnapshot(for: monitor)
		stateLock.unlock()
		guard let sampler else {
			return nil
		}
		return try? sampler.nextRegionFrame(
			monitor: encodedMonitor,
			pixelRect: pixelRect,
			afterFrameSequence: afterFrameSequence,
			waitForFresh: waitForFresh
		)
	}

	func prime(at point: CGPoint?) {
		guard let point, let monitor = monitor(containing: point) else {
			return
		}
		prime(monitor: monitor)
	}

	private func monitor(containing point: CGPoint) -> SamplerMonitor? {
		stateLock.lock()
		let monitors = self.monitors
		stateLock.unlock()
		return monitors.first(where: { $0.appKitFrame.inclusivelyContains(point) })
	}

	private static func makeSampler(exceptionWindowIDs: Set<CGWindowID>) -> RsnapLiveSampler? {
		try? RsnapLiveSampler(selfCaptureExceptionWindowIDs: exceptionWindowIDs.sorted())
	}

	private func prime(monitor: SamplerMonitor) {
		stateLock.lock()
		let sampler = self.sampler
		let generation = streamGeneration
		let now = ProcessInfo.processInfo.systemUptime
		if lastPrimedMonitorID == monitor.id,
			lastPrimeGeneration == generation,
			now - lastPrimeUptime < Self.primeThrottleInterval
		{
			stateLock.unlock()
			return
		}
		lastPrimedMonitorID = monitor.id
		lastPrimeGeneration = generation
		lastPrimeUptime = now
		stateLock.unlock()
		guard let sampler else {
			return
		}
		try? sampler.primeMonitor(samplerMonitorSnapshot(for: monitor))
	}

	private func samplerMonitorSnapshot(for monitor: SamplerMonitor) -> MonitorSnapshot {
		MonitorSnapshot(
			id: monitor.id,
			frame: monitor.quartzFrame,
			scaleFactorX1000: monitor.scaleFactorX1000
		)
	}

	private static func monitorSnapshot(
		for screen: NSScreen,
		mainDisplayHeight: CGFloat
	) -> SamplerMonitor? {
		guard let displayID = screen.nativeDisplayID else {
			return nil
		}
		let appKitFrame = screen.frame
		return SamplerMonitor(
			id: displayID,
			appKitFrame: appKitFrame,
			quartzFrame: appKitRectToQuartz(appKitFrame, mainDisplayHeight: mainDisplayHeight),
			scaleFactorX1000: UInt32(max((screen.backingScaleFactor * 1_000).rounded(), 1_000))
		)
	}

	private static func mainDisplayHeight(for screens: [NSScreen]) -> CGFloat {
		screens
			.first(where: { $0.frame.origin.x.rounded() == 0 && $0.frame.origin.y.rounded() == 0 })?
			.frame.height
			.rounded()
			?? screens.first?.frame.height.rounded()
			?? 0
	}

	private static func appKitRectToQuartz(_ rect: CGRect, mainDisplayHeight: CGFloat) -> CGRect {
		CGRect(
			x: rect.minX,
			y: mainDisplayHeight - rect.maxY,
			width: rect.width,
			height: rect.height
		)
	}

}
