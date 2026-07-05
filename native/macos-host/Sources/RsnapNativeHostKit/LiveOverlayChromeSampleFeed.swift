import CoreGraphics
import Foundation
import RsnapHostBridge

final class ChromeSampleFeed: @unchecked Sendable {
	private struct FirstRgbTelemetry {
		let captureID: UInt64
		let totalMilliseconds: Double
		let refreshCount: UInt64
		let source: String
		let hasPatch: Bool
		let includeLoupePatch: Bool
	}

	private struct BackgroundSampleTelemetry {
		let captureID: UInt64
		let totalMilliseconds: Double
		let outcome: String
		let source: String
		let includeLoupePatch: Bool
		let immediate: Bool
	}

	typealias FrameRgbSampler = @Sendable (CGPoint) -> LiveRgbSample?
	typealias FramePatchSampler = @Sendable (CGPoint, Int) -> CGImage?
	typealias BackgroundSampler =
		@Sendable (CGPoint, LiveColorSampleSource, Int, Bool) -> LiveChromeSample?
	typealias FirstRgbSampled = @Sendable () -> Void
	typealias SampleUpdated = () -> Void

	private let broker: LiveFrameStreamBroker
	private let frameRgbSampler: FrameRgbSampler
	private let framePatchSampler: FramePatchSampler
	private let backgroundSampler: BackgroundSampler
	private let firstRgbSampled: FirstRgbSampled
	private let sampleUpdated: SampleUpdated
	private let queue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.chrome-sample-feed", qos: .userInteractive)
	private let backgroundQueue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.chrome-sample-feed.background",
		qos: .userInteractive,
		attributes: .concurrent
	)
	private let stateLock = NSLock()
	private let sampleRefreshGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.sample_refresh_gap",
		category: "LiveChromeTelemetry",
		batchSize: 60
	)
	private let sampleRefreshDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.sample_refresh_duration",
		category: "LiveChromeTelemetry",
		batchSize: 60
	)
	private let backgroundSampleDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.background_sample_duration",
		category: "LiveChromeTelemetry",
		batchSize: 20
	)
	private static let backgroundProbeMinimumInterval: TimeInterval = 0.25
	private static let backgroundProbeIdleDelay: TimeInterval = 0.08
	private static let maximumBackgroundSamplesInFlight = 1
	private static let sampleTimerWakeupLeadRatio = 0.94
	private static let loupePatchSampleMinimumInterval =
		NativeHostDisplayRefresh.frameInterval(
			forTargetFramesPerSecond: NativeHostDisplayRefresh.fallbackFramesPerSecond)
	private static let sampleUpdatedNotificationIdleDelay =
		NativeHostDisplayRefresh.frameInterval(
			forTargetFramesPerSecond: NativeHostDisplayRefresh.fallbackFramesPerSecond)
	private var timer: DispatchSourceTimer?
	private var refreshQueued = false
	private var backgroundSamplesInFlight = 0
	private var backgroundRefreshPending = false
	private var backgroundCorrectionMode = false
	private var whiteStreamRunHasProbed = false
	private var lastBackgroundProbeUptime: TimeInterval = 0
	private var lastLoupePatchRefreshUptime: TimeInterval = 0
	private var desiredPoint: CGPoint?
	private var desiredSidePixels: Int = 1
	private var desiredIncludesLoupePatch = false
	private var desiredSource: LiveColorSampleSource?
	private var latestSample: LiveChromeSample?
	private var latestSamplePoint: CGPoint?
	private var lastRefreshUptime: TimeInterval?
	private var lastPointChangeUptime = ProcessInfo.processInfo.systemUptime
	private var running = false
	private var activeCaptureID: UInt64 = 0
	private var activationStartedAtUptime: TimeInterval?
	private var generation: UInt64 = 0
	private var refreshCount: UInt64 = 0
	private var didEmitFirstRgbSample = false
	private var didEmitEmptyBackgroundSample = false
	private var liveStreamRgbReady = false

	init(
		broker: LiveFrameStreamBroker,
		frameRgbSampler: @escaping FrameRgbSampler = { _ in nil },
		framePatchSampler: @escaping FramePatchSampler = { _, _ in nil },
		backgroundSampler: @escaping BackgroundSampler,
		firstRgbSampled: @escaping FirstRgbSampled = {},
		sampleUpdated: @escaping SampleUpdated = {}
	) {
		self.broker = broker
		self.frameRgbSampler = frameRgbSampler
		self.framePatchSampler = framePatchSampler
		self.backgroundSampler = backgroundSampler
		self.firstRgbSampled = firstRgbSampled
		self.sampleUpdated = sampleUpdated
	}

	func start(
		targetFramesPerSecond: Int = NativeHostDisplayRefresh.targetFramesPerSecond,
		captureID: UInt64 = 0
	) {
		stop()
		let startedAt = ProcessInfo.processInfo.systemUptime
		stateLock.lock()
		generation &+= 1
		running = true
		activeCaptureID = captureID
		activationStartedAtUptime = startedAt
		refreshCount = 0
		didEmitFirstRgbSample = false
		didEmitEmptyBackgroundSample = false
		liveStreamRgbReady = false
		stateLock.unlock()
		let timer = DispatchSource.makeTimerSource(queue: queue)
		let intervalNanoseconds = max(
			1,
			Int(
				(NativeHostDisplayRefresh.frameInterval(
					forTargetFramesPerSecond: targetFramesPerSecond)
					* Self.sampleTimerWakeupLeadRatio
					* 1_000_000_000.0).rounded())
		)
		timer.schedule(
			deadline: .now(),
			repeating: .nanoseconds(intervalNanoseconds),
			leeway: .nanoseconds(0)
		)
		timer.setEventHandler { [weak self] in
			self?.refresh()
		}
		self.timer = timer
		timer.resume()
		NativeHostTelemetry.liveChromeSampleFeedStarted(
			captureID: captureID,
			targetHz: targetFramesPerSecond
		)
	}

	func stop() {
		timer?.cancel()
		timer = nil
		stateLock.lock()
		generation &+= 1
		running = false
		desiredPoint = nil
		desiredIncludesLoupePatch = false
		desiredSource = nil
		latestSample = nil
		latestSamplePoint = nil
		refreshQueued = false
		backgroundSamplesInFlight = 0
		backgroundRefreshPending = false
		backgroundCorrectionMode = false
		whiteStreamRunHasProbed = false
		lastBackgroundProbeUptime = 0
		lastLoupePatchRefreshUptime = 0
		lastRefreshUptime = nil
		lastPointChangeUptime = ProcessInfo.processInfo.systemUptime
		activeCaptureID = 0
		activationStartedAtUptime = nil
		refreshCount = 0
		didEmitFirstRgbSample = false
		didEmitEmptyBackgroundSample = false
		liveStreamRgbReady = false
		stateLock.unlock()
	}

	func updateDemand(
		point: CGPoint?,
		sidePixels: Int,
		includeLoupePatch: Bool,
		source: LiveColorSampleSource?
	) {
		stateLock.lock()
		let nextSidePixels = max(1, sidePixels)
		let sidePixelsChanged = nextSidePixels != desiredSidePixels
		let patchDemandChanged = includeLoupePatch != desiredIncludesLoupePatch
		let sourceChanged = desiredSource != source
		let activating = desiredPoint == nil && point != nil
		let pointChanged =
			desiredPoint.map { current in
				guard let point else {
					return true
				}
				return abs(current.x - point.x) > 0.5 || abs(current.y - point.y) > 0.5
			} ?? (point != nil)
		if sidePixelsChanged || patchDemandChanged {
			latestSample = latestSample.map {
				LiveChromeSample(rgb: $0.rgb, loupePatch: nil)
			}
		}
		if pointChanged || sourceChanged {
			backgroundCorrectionMode = false
			whiteStreamRunHasProbed = false
			lastPointChangeUptime = ProcessInfo.processInfo.systemUptime
		}
		desiredPoint = point
		desiredSidePixels = nextSidePixels
		desiredIncludesLoupePatch = includeLoupePatch
		desiredSource = source
		let shouldRefresh =
			pointChanged || activating || sourceChanged || sidePixelsChanged || patchDemandChanged
		let running = self.running
		stateLock.unlock()
		if shouldRefresh, running {
			enqueueRefresh()
		}
	}

	func snapshot(for point: CGPoint?) -> LiveChromeSample? {
		stateLock.lock()
		let latestSample = self.latestSample
		let latestSamplePoint = self.latestSamplePoint
		let includeLoupePatch = desiredIncludesLoupePatch
		let running = self.running
		stateLock.unlock()
		guard running else {
			return nil
		}
		guard let point else {
			return latestSample
		}
		if let latestSamplePoint,
			LiveOverlayChromeSamplePolicy.pointsEquivalent(latestSamplePoint, point)
		{
			return latestSample
		}
		guard let rgbSample = frameRgbSampler(point) else {
			return nil
		}
		let sample = LiveChromeSample(
			rgb: rgbSample,
			loupePatch: includeLoupePatch ? latestSample?.loupePatch : nil
		)
		stateLock.lock()
		self.latestSample = sample
		self.latestSamplePoint = point
		stateLock.unlock()
		return sample
	}

	private func enqueueRefresh() {
		stateLock.lock()
		guard refreshQueued == false else {
			stateLock.unlock()
			return
		}
		refreshQueued = true
		stateLock.unlock()
		queue.async { [weak self] in
			self?.refresh()
		}
	}

	private func refresh() {
		let now = ProcessInfo.processInfo.systemUptime
		let refreshStartedAt = now
		stateLock.lock()
		guard running else {
			refreshQueued = false
			stateLock.unlock()
			return
		}
		refreshQueued = false
		let point = desiredPoint
		let sidePixels = desiredSidePixels
		let includeLoupePatch = desiredIncludesLoupePatch
		let source = desiredSource
		let previousSample = latestSample
		let previousPoint = latestSamplePoint
		let lastRefreshUptime = self.lastRefreshUptime
		let lastLoupePatchRefreshUptime = self.lastLoupePatchRefreshUptime
		let pointIdleDuration = now - lastPointChangeUptime
		refreshCount &+= 1
		let currentRefreshCount = refreshCount
		self.lastRefreshUptime = now
		stateLock.unlock()
		if let lastRefreshUptime {
			let gapMilliseconds = (now - lastRefreshUptime) * 1_000
			if gapMilliseconds >= 0, gapMilliseconds < 250 {
				sampleRefreshGapMetric.record(gapMilliseconds)
			}
		}
		guard let point else {
			stateLock.lock()
			latestSample = nil
			latestSamplePoint = nil
			self.lastLoupePatchRefreshUptime = 0
			stateLock.unlock()
			return
		}
		let frameRgbSample = frameRgbSampler(point)
		let canReuseRecentPatch =
			includeLoupePatch
			&& previousSample?.loupePatch != nil
			&& now - lastLoupePatchRefreshUptime < Self.loupePatchSampleMinimumInterval
		let shouldRefreshLoupePatch =
			includeLoupePatch && !canReuseRecentPatch
		let streamSample =
			shouldRefreshLoupePatch
			? broker.sample(at: point, sidePixels: sidePixels)
			: nil
		let framePatchSample =
			shouldRefreshLoupePatch && streamSample?.loupePatch == nil
			? framePatchSampler(point, sidePixels)
			: nil
		let streamRgbSample =
			frameRgbSample == nil
			? (streamSample?.rgb ?? broker.rgbSample(at: point))
			: nil
		let reusableRgbSample = LiveOverlayChromeSamplePolicy.reusableRgbSample(
			previousSample: previousSample, previousPoint: previousPoint, point: point, now: now)
		let rgbSample =
			frameRgbSample
			?? streamRgbSample
			?? reusableRgbSample
		let rgbSource =
			LiveOverlayChromeSamplePolicy.rgbSampleSource(
				frameRgbSample: frameRgbSample,
				streamRgbSample: streamRgbSample,
				reusableRgbSample: reusableRgbSample
			)
		let shouldUseDisplayPointSampler: Bool
		stateLock.lock()
		if frameRgbSample != nil || streamRgbSample != nil {
			liveStreamRgbReady = true
		}
		shouldUseDisplayPointSampler = !liveStreamRgbReady
		stateLock.unlock()
		if frameRgbSample == nil, let source, shouldUseDisplayPointSampler {
			enqueueBackgroundSampleIfNeeded(
				point: point,
				source: source,
				sidePixels: sidePixels,
				includeLoupePatch: includeLoupePatch,
				streamRgbSample: streamRgbSample?.rgb,
				pointIdleDuration: pointIdleDuration
			)
		}
		let patchSample =
			LiveOverlayChromeSamplePolicy.sampleWithUpdatedPatch(
				rgb: nil, loupePatch: framePatchSample)
			?? streamSample
			?? LiveOverlayChromeSamplePolicy.recentPatchSample(
				previousSample: previousSample,
				canReuseRecentPatch: canReuseRecentPatch
			)
			?? LiveOverlayChromeSamplePolicy.reusablePatchSample(
				previousSample: previousSample,
				previousPoint: previousPoint,
				point: point,
				includeLoupePatch: includeLoupePatch
			)
		let sample = LiveOverlayChromeSamplePolicy.sampleWithUpdatedPatch(
			rgb: rgbSample,
			patchSample: patchSample
		)
		stateLock.lock()
		latestSample = sample
		latestSamplePoint = sample == nil ? nil : point
		if framePatchSample != nil || streamSample?.loupePatch != nil {
			self.lastLoupePatchRefreshUptime = now
		}
		let firstRgbTelemetry = makeFirstRgbTelemetryLocked(
			rgbSample: rgbSample?.rgb,
			source: rgbSource,
			refreshCount: currentRefreshCount,
			hasPatch: sample?.loupePatch != nil,
			includeLoupePatch: includeLoupePatch
		)
		stateLock.unlock()
		emit(firstRgbTelemetry)
		sampleRefreshDurationMetric.recordMillisecondsSince(refreshStartedAt)
	}

	private func makeFirstRgbTelemetryLocked(
		rgbSample: RGBSample?,
		source: String,
		refreshCount: UInt64,
		hasPatch: Bool,
		includeLoupePatch: Bool
	) -> FirstRgbTelemetry? {
		guard rgbSample != nil, !didEmitFirstRgbSample else {
			return nil
		}
		didEmitFirstRgbSample = true
		return FirstRgbTelemetry(
			captureID: activeCaptureID,
			totalMilliseconds: activationStartedAtUptime.map {
				NativeHostTelemetry.milliseconds(since: $0)
			} ?? 0,
			refreshCount: refreshCount,
			source: source,
			hasPatch: hasPatch,
			includeLoupePatch: includeLoupePatch
		)
	}

	private func emit(_ telemetry: FirstRgbTelemetry?) {
		guard let telemetry else {
			return
		}
		NativeHostTelemetry.liveChromeFirstRgbSample(
			captureID: telemetry.captureID,
			totalMilliseconds: telemetry.totalMilliseconds,
			refreshCount: telemetry.refreshCount,
			source: telemetry.source,
			hasPatch: telemetry.hasPatch,
			includeLoupePatch: telemetry.includeLoupePatch
		)
		firstRgbSampled()
	}

	private func emit(_ telemetry: BackgroundSampleTelemetry?) {
		guard let telemetry else {
			return
		}
		NativeHostTelemetry.liveChromeBackgroundSample(
			captureID: telemetry.captureID,
			totalMilliseconds: telemetry.totalMilliseconds,
			outcome: telemetry.outcome,
			source: telemetry.source,
			includeLoupePatch: telemetry.includeLoupePatch,
			immediate: telemetry.immediate
		)
	}

	private func enqueueBackgroundSampleIfNeeded(
		point: CGPoint,
		source: LiveColorSampleSource,
		sidePixels: Int,
		includeLoupePatch: Bool,
		streamRgbSample: RGBSample?,
		pointIdleDuration: TimeInterval
	) {
		let now = ProcessInfo.processInfo.systemUptime
		let shouldProbe: Bool
		let shouldNotifyImmediately: Bool
		let sampleSidePixels: Int
		let sampleIncludesLoupePatch: Bool
		let sampleGeneration: UInt64
		stateLock.lock()
		if let streamRgbSample, !LiveOverlayChromeSamplePolicy.isLikelyOverlayWhite(streamRgbSample)
		{
			backgroundCorrectionMode = false
			whiteStreamRunHasProbed = false
			stateLock.unlock()
			return
		}
		if streamRgbSample == nil {
			stateLock.unlock()
			return
		} else if backgroundCorrectionMode {
			guard pointIdleDuration >= Self.backgroundProbeIdleDelay else {
				stateLock.unlock()
				return
			}
			shouldProbe = false
			shouldNotifyImmediately = false
			sampleSidePixels = sidePixels
			sampleIncludesLoupePatch = includeLoupePatch
		} else if whiteStreamRunHasProbed == false,
			now - lastBackgroundProbeUptime >= Self.backgroundProbeMinimumInterval
		{
			guard pointIdleDuration >= Self.backgroundProbeIdleDelay else {
				stateLock.unlock()
				return
			}
			whiteStreamRunHasProbed = true
			lastBackgroundProbeUptime = now
			shouldProbe = true
			shouldNotifyImmediately = false
			sampleSidePixels = sidePixels
			sampleIncludesLoupePatch = includeLoupePatch
		} else {
			stateLock.unlock()
			return
		}
		guard backgroundSamplesInFlight < Self.maximumBackgroundSamplesInFlight else {
			backgroundRefreshPending = true
			stateLock.unlock()
			return
		}
		backgroundSamplesInFlight += 1
		sampleGeneration = generation
		stateLock.unlock()
		backgroundQueue.async { [weak self] in
			self?.refreshBackgroundSample(
				point: point,
				source: source,
				sidePixels: sampleSidePixels,
				includeLoupePatch: sampleIncludesLoupePatch,
				shouldProbeForCorrection: shouldProbe,
				shouldNotifyImmediately: shouldNotifyImmediately,
				generation: sampleGeneration
			)
		}
	}

	private func refreshBackgroundSample(
		point: CGPoint,
		source: LiveColorSampleSource,
		sidePixels: Int,
		includeLoupePatch: Bool,
		shouldProbeForCorrection: Bool,
		shouldNotifyImmediately: Bool,
		generation sampleGeneration: UInt64
	) {
		let startedAt = ProcessInfo.processInfo.systemUptime
		let sample = backgroundSampler(point, source, sidePixels, includeLoupePatch)
		let sampleRgb = sample?.rgb
		let sampleRgbValue = sampleRgb?.rgb
		let sampleLoupePatch = sample?.loupePatch
		let sampleMilliseconds = NativeHostTelemetry.milliseconds(since: startedAt)
		backgroundSampleDurationMetric.record(sampleMilliseconds)
		stateLock.lock()
		guard generation == sampleGeneration else {
			stateLock.unlock()
			return
		}
		let shouldRefreshPending = finishBackgroundSampleLocked()
		let currentRefreshCount = refreshCount
		if liveStreamRgbReady, sampleLoupePatch == nil {
			stateLock.unlock()
			if shouldRefreshPending {
				enqueueRefresh()
			}
			return
		}
		guard let desiredPoint,
			sample != nil,
			desiredSource == source,
			LiveOverlayChromeSamplePolicy.pointsEquivalent(desiredPoint, point)
		else {
			let shouldLogEmptyBackgroundSample =
				sample == nil && (!didEmitEmptyBackgroundSample || sampleMilliseconds >= 20)
			if shouldLogEmptyBackgroundSample {
				didEmitEmptyBackgroundSample = true
			}
			let backgroundSampleTelemetry =
				shouldLogEmptyBackgroundSample
				? BackgroundSampleTelemetry(
					captureID: activeCaptureID,
					totalMilliseconds: sampleMilliseconds,
					outcome: "empty",
					source: "display_point",
					includeLoupePatch: includeLoupePatch,
					immediate: shouldNotifyImmediately
				) : nil
			stateLock.unlock()
			emit(backgroundSampleTelemetry)
			if shouldRefreshPending {
				enqueueRefresh()
			}
			return
		}
		if shouldProbeForCorrection, let rgbSample = sampleRgbValue {
			backgroundCorrectionMode = !LiveOverlayChromeSamplePolicy.isLikelyOverlayWhite(
				rgbSample)
		}
		let previousRgb = latestSample?.rgb
		let previousLoupePatch = latestSample?.loupePatch
		latestSample = LiveChromeSample(
			rgb: sampleRgb ?? previousRgb,
			loupePatch: sampleLoupePatch ?? previousLoupePatch
		)
		latestSamplePoint = point
		let firstRgbTelemetry = makeFirstRgbTelemetryLocked(
			rgbSample: sampleRgbValue,
			source: sampleRgb?.source ?? "background_sample",
			refreshCount: currentRefreshCount,
			hasPatch: sampleLoupePatch != nil,
			includeLoupePatch: includeLoupePatch
		)
		let shouldLogBackgroundSample =
			firstRgbTelemetry != nil || sampleMilliseconds >= 20
		let backgroundSampleTelemetry =
			shouldLogBackgroundSample
			? BackgroundSampleTelemetry(
				captureID: activeCaptureID,
				totalMilliseconds: sampleMilliseconds,
				outcome: LiveOverlayChromeSamplePolicy.backgroundSampleOutcome(
					hasRgb: sampleRgbValue != nil,
					hasPatch: sampleLoupePatch != nil
				),
				source: sampleRgb?.source ?? "background_sample",
				includeLoupePatch: includeLoupePatch,
				immediate: shouldNotifyImmediately
			) : nil
		let shouldNotify =
			shouldNotifyImmediately
			|| LiveOverlayChromeSamplePolicy.shouldNotifySampleUpdated(
				now: ProcessInfo.processInfo.systemUptime,
				lastPointChangeUptime: lastPointChangeUptime,
				idleDelay: Self.sampleUpdatedNotificationIdleDelay
			)
		stateLock.unlock()
		emit(backgroundSampleTelemetry)
		emit(firstRgbTelemetry)
		if shouldNotify {
			sampleUpdated()
		}
		if shouldRefreshPending {
			enqueueRefresh()
		}
	}

	private func finishBackgroundSampleLocked() -> Bool {
		backgroundSamplesInFlight = max(0, backgroundSamplesInFlight - 1)
		guard backgroundRefreshPending,
			backgroundSamplesInFlight < Self.maximumBackgroundSamplesInFlight,
			desiredPoint != nil
		else {
			return false
		}
		backgroundRefreshPending = false
		return true
	}

}
