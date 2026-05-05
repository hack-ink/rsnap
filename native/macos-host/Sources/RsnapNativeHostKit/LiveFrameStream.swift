import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

final class LiveFrameStreamBroker {
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
	private var activeTelemetryCaptureID: UInt64 = 0
	private var telemetryStartedAtUptime: TimeInterval?
	private var didEmitFirstRgbSample = false
	private var didEmitEmptyDiagnosticSample = false
	private var didEmitRgbDiagnosticSample = false
	private var issueDiagnosticSamplesRemaining = 0

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

	func updateSelfCaptureExceptionWindowIDs(_ windowIDs: Set<CGWindowID>) {
		stateLock.lock()
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
		try? oldSampler?.reset()
	}

	func start(for screens: [NSScreen], prewarmPoint: CGPoint? = nil, captureID: UInt64 = 0) {
		let startedAt = ProcessInfo.processInfo.systemUptime
		stateLock.lock()
		if captureID != 0,
			activeTelemetryCaptureID != captureID || telemetryStartedAtUptime == nil
		{
			activeTelemetryCaptureID = captureID
			telemetryStartedAtUptime = startedAt
			didEmitFirstRgbSample = false
			didEmitEmptyDiagnosticSample = false
			didEmitRgbDiagnosticSample = false
			issueDiagnosticSamplesRemaining = 4
		}
		if sampler == nil {
			sampler = Self.makeSampler(exceptionWindowIDs: selfCaptureExceptionWindowIDs)
		}
		let mainDisplayHeight = Self.mainDisplayHeight(for: screens)
		self.mainDisplayHeight = mainDisplayHeight
		let nextMonitors = screens.compactMap {
			Self.monitorSnapshot(for: $0, mainDisplayHeight: mainDisplayHeight)
		}
		let targetMonitor = prewarmPoint.flatMap { point in
			nextMonitors.first(where: { $0.appKitFrame.contains(point) })
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
		activeTelemetryCaptureID = 0
		telemetryStartedAtUptime = nil
		didEmitFirstRgbSample = false
		didEmitEmptyDiagnosticSample = false
		didEmitRgbDiagnosticSample = false
		issueDiagnosticSamplesRemaining = 0
		let sampler = self.sampler
		stateLock.unlock()
		guard let sampler else {
			return
		}
		try? sampler.reset()
	}

	func sample(at point: CGPoint, sidePixels: Int) -> LiveChromeSample? {
		let sampleStartedAt = ProcessInfo.processInfo.systemUptime
		stateLock.lock()
		let sampler = self.sampler
		let captureID = activeTelemetryCaptureID
		stateLock.unlock()
		guard let sampler, let monitor = monitor(containing: point) else {
			emitDiagnosticSampleIfNeeded(
				captureID: captureID,
				startedAt: sampleStartedAt,
				outcome: "inactive",
				frameAgeMilliseconds: -1,
				hasPatch: false
			)
			return nil
		}
		let samplerPoint = Self.appKitPointToQuartz(point, mainDisplayHeight: mainDisplayHeight)
		guard
			let sample = try? sampler.sampleCursor(
				monitor: samplerMonitorSnapshot(for: monitor),
				point: samplerPoint,
				patchSidePixels: sidePixels
			)
		else {
			emitDiagnosticSampleIfNeeded(
				captureID: captureID,
				startedAt: sampleStartedAt,
				outcome: "empty",
				frameAgeMilliseconds: -1,
				hasPatch: false
			)
			return nil
		}
		guard let capturedAtUptime = sample.capturedAtUptime else {
			emitDiagnosticSampleIfNeeded(
				captureID: captureID,
				startedAt: sampleStartedAt,
				outcome: "missing_metadata",
				frameAgeMilliseconds: -1,
				hasPatch: sample.patchRGBA != nil
			)
			return nil
		}
		let frameAge = ProcessInfo.processInfo.systemUptime - capturedAtUptime
		let frameAgeMilliseconds = frameAge * 1_000
		guard frameAge <= LiveRgbSample.maximumDisplayAge else {
			emitDiagnosticSampleIfNeeded(
				captureID: captureID,
				startedAt: sampleStartedAt,
				outcome: "stale",
				frameAgeMilliseconds: frameAgeMilliseconds,
				hasPatch: sample.patchRGBA != nil
			)
			return nil
		}
		guard sample.rgb != nil else {
			emitDiagnosticSampleIfNeeded(
				captureID: captureID,
				startedAt: sampleStartedAt,
				outcome: "no_rgb",
				frameAgeMilliseconds: frameAgeMilliseconds,
				hasPatch: sample.patchRGBA != nil
			)
			return nil
		}

		let chromeSample = LiveChromeSample(
			rgbSample: sample.rgb,
			rgbCapturedAtUptime: capturedAtUptime,
			rgbSource: "live_stream",
			loupePatch: cgImage(from: sample)
		)
		emitDiagnosticSampleIfNeeded(
			captureID: captureID,
			startedAt: sampleStartedAt,
			outcome: "rgb",
			frameAgeMilliseconds: frameAgeMilliseconds,
			hasPatch: chromeSample.loupePatch != nil
		)
		emitFirstRgbTelemetryIfNeeded(chromeSample)
		return chromeSample
	}

	func rgbSample(at point: CGPoint) -> LiveRgbSample? {
		sample(at: point, sidePixels: 0)?.rgb
	}

	func patch(in rect: CGRect) -> CGImage? {
		let point = CGPoint(x: rect.midX, y: rect.midY)
		let sidePixels = max(Int(rect.width.rounded()), Int(rect.height.rounded()), 1)
		return sample(at: point, sidePixels: sidePixels)?.loupePatch
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

		return cgImage(
			width: snapshot.width,
			height: snapshot.height,
			rgba: snapshot.rgba
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
		return monitors.first(where: { $0.appKitFrame.contains(point) })
	}

	private static func makeSampler(exceptionWindowIDs: Set<CGWindowID>) -> RsnapLiveSampler? {
		try? RsnapLiveSampler(selfCaptureExceptionWindowIDs: exceptionWindowIDs.sorted())
	}

	private func emitFirstRgbTelemetryIfNeeded(_ sample: LiveChromeSample) {
		guard sample.rgbSample != nil else {
			return
		}
		let captureID: UInt64
		let totalMilliseconds: Double
		stateLock.lock()
		guard !didEmitFirstRgbSample, activeTelemetryCaptureID != 0 else {
			stateLock.unlock()
			return
		}
		didEmitFirstRgbSample = true
		captureID = activeTelemetryCaptureID
		totalMilliseconds =
			telemetryStartedAtUptime.map {
				NativeHostTelemetry.milliseconds(since: $0)
			} ?? 0
		stateLock.unlock()
		NativeHostTelemetry.liveStreamFirstRgbSample(
			captureID: captureID,
			totalMilliseconds: totalMilliseconds,
			hasPatch: sample.loupePatch != nil
		)
	}

	private func emitDiagnosticSampleIfNeeded(
		captureID: UInt64,
		startedAt: TimeInterval,
		outcome: String,
		frameAgeMilliseconds: Double,
		hasPatch: Bool
	) {
		guard captureID != 0 else {
			return
		}
		stateLock.lock()
		let shouldEmit: Bool
		switch outcome {
		case "empty", "inactive":
			shouldEmit = !didEmitEmptyDiagnosticSample
			didEmitEmptyDiagnosticSample = true
		case "rgb":
			shouldEmit = !didEmitRgbDiagnosticSample
			didEmitRgbDiagnosticSample = true
		default:
			shouldEmit = issueDiagnosticSamplesRemaining > 0
			if shouldEmit {
				issueDiagnosticSamplesRemaining -= 1
			}
		}
		stateLock.unlock()
		guard shouldEmit else {
			return
		}
		NativeHostTelemetry.liveStreamSample(
			captureID: captureID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAt),
			outcome: outcome,
			frameAgeMilliseconds: frameAgeMilliseconds,
			hasPatch: hasPatch
		)
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
		guard let displayID = screen.displayID else {
			return nil
		}
		let appKitFrame = screen.frame
		return SamplerMonitor(
			id: displayID,
			appKitFrame: appKitFrame,
			quartzFrame: appKitRectToQuartz(appKitFrame, mainDisplayHeight: mainDisplayHeight),
			scaleFactorX1000: UInt32(max((screen.backingScaleFactor * 1000).rounded(), 1000))
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

	private static func appKitPointToQuartz(_ point: CGPoint, mainDisplayHeight: CGFloat) -> CGPoint
	{
		CGPoint(x: point.x, y: mainDisplayHeight - point.y - 1)
	}

	private func cgImage(from sample: LiveSampleSnapshot?) -> CGImage? {
		guard
			let sample,
			let patchRGBA = sample.patchRGBA,
			sample.patchWidth > 0,
			sample.patchHeight > 0
		else {
			return nil
		}

		return cgImage(
			width: sample.patchWidth,
			height: sample.patchHeight,
			rgba: patchRGBA
		)
	}

	private func cgImage(width: Int, height: Int, rgba: Data) -> CGImage? {
		guard width > 0, height > 0 else {
			return nil
		}
		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
		let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue)
		guard
			let provider = CGDataProvider(data: rgba as CFData),
			let image = CGImage(
				width: width,
				height: height,
				bitsPerComponent: 8,
				bitsPerPixel: 32,
				bytesPerRow: width * 4,
				space: colorSpace,
				bitmapInfo: bitmapInfo,
				provider: provider,
				decode: nil,
				shouldInterpolate: false,
				intent: .defaultIntent
			)
		else {
			return nil
		}

		return image
	}
}

extension NSScreen {
	fileprivate var displayID: CGDirectDisplayID? {
		(deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
	}
}
