import AppKit
import CoreGraphics
import Foundation
import QuartzCore
import RsnapHostBridge

enum LiveGlassSurfaceKind: Hashable {
	case hud
	case loupe
}

struct LivePreviewSnapshot {
	let bounds: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let frozenPending: Bool
	let frozenDisplayFrame: CGRect?
	let frozenDisplayImage: CGImage?
	let pointerLocal: CGPoint?
	let dragSelectionLocal: CGRect?
	let hoverSelectionLocal: CGRect?
	let selectionSizeText: String?
	let hudFrame: CGRect?
	let loupeFrame: CGRect?
	let positionDisplay: LivePositionDisplay
	let colorDisplay: LiveColorDisplay
	let rgbSample: RGBSample?
	let keycapVisible: Bool
	let inputUptime: TimeInterval?
	let loupePatch: CGImage?
	let glassPatches: [LiveGlassSurfaceKind: CGImage]
}

@MainActor
private enum LiveOverlayTypography {
	static let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
	static let lineHeight = ceil("x=0".size(using: font).height)
	static let commaWidth = ",".size(using: font).width
	static let keycapTextSize = "Tab".size(using: font)
	static let keycapFrameSize = CGSize(
		width: keycapTextSize.width + 12, height: keycapTextSize.height + 4)
}

final class WindowSnapshotFeed {
	private static let ownPID = ProcessInfo.processInfo.processIdentifier
	private static let maxWindowLayerForTargeting = 3
	private let queue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.window-snapshot-feed", qos: .userInitiated)
	private let stateLock = NSLock()
	private var timer: DispatchSourceTimer?
	private var desktopFrame: CGRect = .null
	private var latestSnapshots: [WindowSnapshot] = []

	func start(desktopFrame: CGRect, initialSnapshots: [WindowSnapshot] = []) {
		stop()
		stateLock.lock()
		self.desktopFrame = desktopFrame
		latestSnapshots = initialSnapshots
		stateLock.unlock()
		let timer = DispatchSource.makeTimerSource(queue: queue)
		timer.schedule(
			deadline: .now(), repeating: LiveSamplingBudget.hoverWindowCacheRefreshInterval)
		timer.setEventHandler { [weak self] in
			self?.refresh()
		}
		self.timer = timer
		timer.resume()
	}

	func stop() {
		timer?.cancel()
		timer = nil
		stateLock.lock()
		latestSnapshots.removeAll()
		stateLock.unlock()
	}

	func window(at point: CGPoint) -> WindowSnapshot? {
		stateLock.lock()
		let snapshots = latestSnapshots
		stateLock.unlock()
		return snapshots.first(where: { $0.frame.contains(point) })
	}

	static func snapshots(desktopFrame: CGRect) -> [WindowSnapshot] {
		let candidateWindows =
			(CGWindowListCopyWindowInfo(
				[.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
				as? [[String: Any]])
			?? []
		var snapshots: [WindowSnapshot] = []
		for info in candidateWindows {
			let isOnScreen = (info[kCGWindowIsOnscreen as String] as? NSNumber)?.boolValue ?? false
			let ownerPID = (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value ?? -1
			if !isOnScreen {
				continue
			}
			let alpha = (info[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 1
			if alpha < 0.05 {
				continue
			}
			let layer = (info[kCGWindowLayer as String] as? NSNumber)?.intValue ?? 0
			if layer < 0 || layer > maxWindowLayerForTargeting {
				continue
			}
			if ownerPID == ownPID && !Self.isTargetableOwnWindow(info, layer: layer) {
				continue
			}
			guard let boundsDictionary = info[kCGWindowBounds as String] as? NSDictionary else {
				continue
			}
			var quartzBounds = CGRect.null
			guard CGRectMakeWithDictionaryRepresentation(boundsDictionary, &quartzBounds) else {
				continue
			}
			let appKitBounds = CGRect(
				x: quartzBounds.minX,
				y: desktopFrame.maxY - quartzBounds.maxY,
				width: quartzBounds.width,
				height: quartzBounds.height
			)
			if appKitBounds.width < 40 || appKitBounds.height < 40 {
				continue
			}
			let windowID = (info[kCGWindowNumber as String] as? NSNumber)?.uint32Value
			snapshots.append(WindowSnapshot(windowID: windowID, frame: appKitBounds))
		}
		return snapshots
	}

	private static func isTargetableOwnWindow(_ info: [String: Any], layer: Int) -> Bool {
		guard layer == 0 else {
			return false
		}
		let name = (info[kCGWindowName as String] as? String) ?? ""
		return name == "Settings"
	}

	static func window(at point: CGPoint, in snapshots: [WindowSnapshot]) -> WindowSnapshot? {
		snapshots.first(where: { $0.frame.contains(point) })
	}

	private func refresh() {
		stateLock.lock()
		let desktopFrame = self.desktopFrame
		stateLock.unlock()
		let snapshots = Self.snapshots(desktopFrame: desktopFrame)
		stateLock.lock()
		latestSnapshots = snapshots
		stateLock.unlock()
	}
}

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
					* NativeHostDisplayRefresh.timerWakeupLeadRatio
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
		let running = self.running
		stateLock.unlock()
		guard running else {
			return nil
		}
		guard let point else {
			return latestSample
		}
		if let latestSamplePoint, Self.pointsEquivalent(latestSamplePoint, point) {
			return latestSample
		}
		guard let rgbSample = frameRgbSampler(point) else {
			return nil
		}
		let sample = LiveChromeSample(rgb: rgbSample, loupePatch: nil)
		stateLock.lock()
		self.latestSample = sample
		self.latestSamplePoint = point
		stateLock.unlock()
		return sample
	}

	private func enqueueRefresh() {
		stateLock.lock()
		guard !refreshQueued else {
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
			stateLock.unlock()
			return
		}
		let frameRgbSample = frameRgbSampler(point)
		let framePatchSample =
			includeLoupePatch
			? framePatchSampler(point, sidePixels)
			: nil
		let streamSample =
			includeLoupePatch && framePatchSample == nil
			? broker.sample(at: point, sidePixels: sidePixels)
			: nil
		let streamRgbSample =
			frameRgbSample == nil
			? (streamSample?.rgb ?? broker.rgbSample(at: point))
			: nil
		let reusableRgbSample = Self.reusableRgbSample(
			previousSample: previousSample, previousPoint: previousPoint, point: point, now: now)
		let rgbSample =
			frameRgbSample
			?? streamRgbSample
			?? reusableRgbSample
		let rgbSource =
			Self.rgbSampleSource(
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
			Self.sampleWithUpdatedPatch(rgb: nil, loupePatch: framePatchSample)
			?? streamSample
			?? Self.reusablePatchSample(
				previousSample: previousSample,
				previousPoint: previousPoint,
				point: point,
				includeLoupePatch: includeLoupePatch
			)
		let sample = Self.sampleWithUpdatedPatch(
			rgb: rgbSample,
			patchSample: patchSample
		)
		stateLock.lock()
		latestSample = sample
		latestSamplePoint = sample == nil ? nil : point
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

	private static func rgbSampleSource(
		frameRgbSample: LiveRgbSample?,
		streamRgbSample: LiveRgbSample?,
		reusableRgbSample: LiveRgbSample?
	) -> String {
		if frameRgbSample != nil {
			return "frame_authority"
		}
		if streamRgbSample != nil {
			return "live_stream"
		}
		if reusableRgbSample != nil {
			return "reusable_cache"
		}
		return "none"
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

	private static func reusableRgbSample(
		previousSample: LiveChromeSample?,
		previousPoint: CGPoint?,
		point: CGPoint,
		now: TimeInterval
	) -> LiveRgbSample? {
		reusableRgbSample(
			rgbSample: previousSample?.rgb,
			previousPoint: previousPoint,
			point: point,
			now: now
		)
	}

	private static func reusableRgbSample(
		rgbSample: LiveRgbSample?,
		previousPoint: CGPoint?,
		point: CGPoint,
		now: TimeInterval
	) -> LiveRgbSample? {
		guard let previousPoint, pointsEquivalent(previousPoint, point) else {
			return nil
		}
		guard rgbSample?.isFresh(maximumAge: LiveRgbSample.maximumReusableAge, now: now) == true
		else {
			return nil
		}
		return rgbSample
	}

	private static func reusablePatchSample(
		previousSample: LiveChromeSample?,
		previousPoint: CGPoint?,
		point: CGPoint,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		guard includeLoupePatch, let previousPoint, pointsEquivalent(previousPoint, point) else {
			return nil
		}
		return previousSample
	}

	private static func pointsEquivalent(_ lhs: CGPoint, _ rhs: CGPoint) -> Bool {
		abs(lhs.x - rhs.x) <= 0.5 && abs(lhs.y - rhs.y) <= 0.5
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
		if let streamRgbSample, !Self.isLikelyOverlayWhite(streamRgbSample) {
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
		} else if !whiteStreamRunHasProbed,
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
			Self.pointsEquivalent(desiredPoint, point)
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
			backgroundCorrectionMode = !Self.isLikelyOverlayWhite(rgbSample)
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
				outcome: Self.backgroundSampleOutcome(
					hasRgb: sampleRgbValue != nil,
					hasPatch: sampleLoupePatch != nil
				),
				source: sampleRgb?.source ?? "background_sample",
				includeLoupePatch: includeLoupePatch,
				immediate: shouldNotifyImmediately
			) : nil
		let shouldNotify =
			shouldNotifyImmediately
			|| Self.shouldNotifySampleUpdated(
				now: ProcessInfo.processInfo.systemUptime,
				lastPointChangeUptime: lastPointChangeUptime
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

	private static func backgroundSampleOutcome(hasRgb: Bool, hasPatch: Bool) -> String {
		if hasRgb, hasPatch {
			return "rgb_patch"
		}
		if hasRgb {
			return "rgb"
		}
		if hasPatch {
			return "patch"
		}
		return "empty"
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

	private static func shouldNotifySampleUpdated(
		now: TimeInterval,
		lastPointChangeUptime: TimeInterval
	) -> Bool {
		now - lastPointChangeUptime >= sampleUpdatedNotificationIdleDelay
	}

	private static func isLikelyOverlayWhite(_ sample: RGBSample) -> Bool {
		sample.r >= 250 && sample.g >= 250 && sample.b >= 250
	}

	private static func sampleWithUpdatedPatch(
		rgb: LiveRgbSample?,
		patchSample: LiveChromeSample?
	) -> LiveChromeSample? {
		sampleWithUpdatedPatch(rgb: rgb, loupePatch: patchSample?.loupePatch)
	}

	private static func sampleWithUpdatedPatch(
		rgb: LiveRgbSample?,
		loupePatch: CGImage?
	) -> LiveChromeSample? {
		guard rgb != nil || loupePatch != nil else {
			return nil
		}
		return LiveChromeSample(
			rgb: rgb,
			loupePatch: loupePatch
		)
	}

}

final class LiveFrameClockDriver: @unchecked Sendable {
	var onTick: (() -> Void)?
	private let stateLock = NSLock()
	private let tickGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.frame_tick_gap",
		category: "LiveChromeTelemetry"
	)
	private var timer: DispatchSourceTimer?
	private var currentTargetFramesPerSecond: Int?
	private var lastTickUptime: TimeInterval?

	func start(targetFramesPerSecond: Int) {
		let sanitizedTarget = max(1, targetFramesPerSecond)
		stateLock.lock()
		let alreadyRunning = timer != nil && currentTargetFramesPerSecond == sanitizedTarget
		stateLock.unlock()
		guard !alreadyRunning else {
			return
		}

		stop()
		let timer = DispatchSource.makeTimerSource(queue: .main)
		let intervalNanoseconds = max(
			1,
			Int(
				(NativeHostDisplayRefresh.timerInterval(
					forTargetFramesPerSecond: sanitizedTarget) * 1_000_000_000.0)
					.rounded())
		)
		timer.schedule(
			deadline: .now(),
			repeating: .nanoseconds(intervalNanoseconds),
			leeway: .nanoseconds(0)
		)
		timer.setEventHandler { [weak self] in
			self?.tick()
		}
		stateLock.lock()
		self.timer = timer
		currentTargetFramesPerSecond = sanitizedTarget
		lastTickUptime = nil
		stateLock.unlock()
		timer.resume()
	}

	private func tick() {
		let now = ProcessInfo.processInfo.systemUptime
		if let lastTickUptime {
			let gapMilliseconds = (now - lastTickUptime) * 1_000
			if gapMilliseconds >= 0, gapMilliseconds < 250 {
				tickGapMetric.record(gapMilliseconds)
			}
		}
		lastTickUptime = now
		onTick?()
	}

	func stop() {
		stateLock.lock()
		guard let timer else {
			currentTargetFramesPerSecond = nil
			lastTickUptime = nil
			stateLock.unlock()
			return
		}
		self.timer = nil
		currentTargetFramesPerSecond = nil
		lastTickUptime = nil
		stateLock.unlock()
		timer.cancel()
	}

	deinit {
		stop()
	}
}

private final class SelectionFlowBandLayer: CALayer {
	private enum Edge: CaseIterable {
		case top
		case right
		case bottom
		case left

		var isHorizontal: Bool {
			self == .top || self == .bottom
		}

		var animationKeyPath: String {
			isHorizontal ? "transform.translation.x" : "transform.translation.y"
		}

		var flowDirection: CGFloat {
			switch self {
			case .top, .right:
				return 1
			case .bottom, .left:
				return -1
			}
		}

		var startPoint: CGPoint {
			isHorizontal ? CGPoint(x: 0, y: 0.5) : CGPoint(x: 0.5, y: 0)
		}

		var endPoint: CGPoint {
			isHorizontal ? CGPoint(x: 1, y: 0.5) : CGPoint(x: 0.5, y: 1)
		}
	}

	private final class EdgeFlowLayers {
		let edge: Edge
		let clipLayer = CALayer()
		let glowLayer = CAGradientLayer()
		let lineLayer = CAGradientLayer()

		init(edge: Edge) {
			self.edge = edge
		}
	}

	private static let pathOutset: CGFloat = 1.0
	private static let darkLineWidth: CGFloat = 1.8
	private static let lightLineWidth: CGFloat = 1.9
	private static let darkGlowLineWidth: CGFloat = 5.0
	private static let lightGlowLineWidth: CGFloat = 5.25
	private static let flowAnimationKey = "rsnap.selection-flow.edge-translation"
	private static let flowAnimationDuration: CFTimeInterval = 2.45
	private static let gradientPeriod: CGFloat = 260
	private static let darkPalette: [(CGFloat, CGFloat, CGFloat, CGFloat)] = [
		(112.0 / 255.0, 215.0 / 255.0, 1.0, 0.98),
		(176.0 / 255.0, 154.0 / 255.0, 1.0, 0.94),
		(110.0 / 255.0, 245.0 / 255.0, 215.0 / 255.0, 0.90),
		(65.0 / 255.0, 150.0 / 255.0, 1.0, 0.96),
	]
	private static let lightPalette: [(CGFloat, CGFloat, CGFloat, CGFloat)] = [
		(0.0 / 255.0, 76.0 / 255.0, 196.0 / 255.0, 1.0),
		(83.0 / 255.0, 44.0 / 255.0, 194.0 / 255.0, 0.98),
		(0.0 / 255.0, 113.0 / 255.0, 98.0 / 255.0, 0.98),
		(196.0 / 255.0, 82.0 / 255.0, 0.0 / 255.0, 0.96),
	]

	private let edgeLayers: [EdgeFlowLayers]
	private var focusRect: CGRect = .null
	private var theme: CaptureChromeTheme = .dark
	private var flowAnimating = false

	override init() {
		edgeLayers = Edge.allCases.map { EdgeFlowLayers(edge: $0) }
		super.init()
		contentsScale = NSScreen.main?.backingScaleFactor ?? 2
		isOpaque = false
		allowsEdgeAntialiasing = true
		masksToBounds = false
		configureLayers()
	}

	override init(layer: Any) {
		edgeLayers = Edge.allCases.map { EdgeFlowLayers(edge: $0) }
		super.init(layer: layer)
		if let layer = layer as? SelectionFlowBandLayer {
			focusRect = layer.focusRect
			theme = layer.theme
			flowAnimating = layer.flowAnimating
		}
		configureLayers()
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func hide() {
		guard !isHidden || !focusRect.isNull else {
			return
		}
		isHidden = true
		focusRect = .null
		flowAnimating = false
		removeFlowAnimation()
	}

	func update(
		frame: CGRect,
		focusRect: CGRect,
		theme: CaptureChromeTheme,
		timestamp _: CFTimeInterval,
		contentsScale: CGFloat,
		animates: Bool
	) {
		let focusChanged = self.focusRect != focusRect
		let themeChanged = self.theme != theme
		let frameChanged = self.frame != frame
		let scaleChanged = self.contentsScale != contentsScale
		let animationChanged = flowAnimating != animates
		let wasHidden = isHidden
		self.frame = frame
		self.contentsScale = contentsScale
		self.focusRect = focusRect
		self.theme = theme
		flowAnimating = animates
		if wasHidden || focusChanged || themeChanged || frameChanged || scaleChanged {
			updateAppearance()
		}
		if animates {
			isHidden = false
			installFlowAnimation(restartsAnimation: wasHidden || animationChanged)
		} else {
			isHidden = true
			removeFlowAnimation()
		}
	}

	private func configureLayers() {
		for edgeLayer in edgeLayers {
			edgeLayer.clipLayer.masksToBounds = true
			edgeLayer.clipLayer.allowsEdgeAntialiasing = true
			addSublayer(edgeLayer.clipLayer)

			for gradientLayer in [edgeLayer.glowLayer, edgeLayer.lineLayer] {
				gradientLayer.type = .axial
				gradientLayer.startPoint = edgeLayer.edge.startPoint
				gradientLayer.endPoint = edgeLayer.edge.endPoint
				gradientLayer.allowsEdgeAntialiasing = true
				edgeLayer.clipLayer.addSublayer(gradientLayer)
			}
			edgeLayer.glowLayer.opacity = selectionFlowGlowOpacity()
		}
	}

	private func updateAppearance() {
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		let strokeRect = focusRect.insetBy(dx: -Self.pathOutset, dy: -Self.pathOutset)
		for edgeLayer in edgeLayers {
			update(edgeLayer, strokeRect: strokeRect)
		}
		CATransaction.commit()
	}

	private func installFlowAnimation(restartsAnimation: Bool) {
		let hasAnimations = edgeLayers.allSatisfy {
			$0.lineLayer.animation(forKey: Self.flowAnimationKey) != nil
				&& $0.glowLayer.animation(forKey: Self.flowAnimationKey) != nil
		}
		if !restartsAnimation, hasAnimations {
			return
		}
		removeFlowAnimation()
		for edgeLayer in edgeLayers {
			installFlowAnimation(on: edgeLayer.lineLayer, edge: edgeLayer.edge)
			installFlowAnimation(on: edgeLayer.glowLayer, edge: edgeLayer.edge)
		}
	}

	private func installFlowAnimation(on layer: CALayer, edge: Edge) {
		let keyPath = edge.animationKeyPath
		let currentOffset = (layer.presentation()?.value(forKeyPath: keyPath) as? CGFloat) ?? 0
		let travel = edge.flowDirection * Self.gradientPeriod
		let animation = CABasicAnimation(keyPath: keyPath)
		animation.fromValue = currentOffset
		animation.toValue = currentOffset + travel
		animation.duration = Self.flowAnimationDuration
		animation.repeatCount = .infinity
		animation.timingFunction = CAMediaTimingFunction(name: .linear)
		layer.add(animation, forKey: Self.flowAnimationKey)
	}

	private func removeFlowAnimation() {
		for edgeLayer in edgeLayers {
			edgeLayer.lineLayer.removeAnimation(forKey: Self.flowAnimationKey)
			edgeLayer.glowLayer.removeAnimation(forKey: Self.flowAnimationKey)
		}
	}

	private func update(_ edgeLayer: EdgeFlowLayers, strokeRect: CGRect) {
		let lineWidth = selectionFlowLineWidth()
		let glowWidth = selectionFlowGlowLineWidth()
		let frame = edgeFrame(for: edgeLayer.edge, strokeRect: strokeRect, glowWidth: glowWidth)
		edgeLayer.clipLayer.frame = pixelAligned(frame)
		edgeLayer.clipLayer.isHidden = frame.width <= 0 || frame.height <= 0
		edgeLayer.glowLayer.opacity = selectionFlowGlowOpacity()
		updateGradients(edgeLayer, lineWidth: lineWidth, glowWidth: glowWidth)
	}

	private func updateGradients(
		_ edgeLayer: EdgeFlowLayers,
		lineWidth: CGFloat,
		glowWidth: CGFloat
	) {
		let clipBounds = edgeLayer.clipLayer.bounds
		let gradientLength =
			(edgeLayer.edge.isHorizontal ? clipBounds.width : clipBounds.height)
			+ Self.gradientPeriod * 2
		let gradientFrame: CGRect
		let lineFrame: CGRect
		if edgeLayer.edge.isHorizontal {
			gradientFrame = CGRect(
				x: -Self.gradientPeriod,
				y: 0,
				width: gradientLength,
				height: glowWidth
			)
			lineFrame = CGRect(
				x: -Self.gradientPeriod,
				y: (glowWidth - lineWidth) / 2,
				width: gradientLength,
				height: lineWidth
			)
		} else {
			gradientFrame = CGRect(
				x: 0,
				y: -Self.gradientPeriod,
				width: glowWidth,
				height: gradientLength
			)
			lineFrame = CGRect(
				x: (glowWidth - lineWidth) / 2,
				y: -Self.gradientPeriod,
				width: lineWidth,
				height: gradientLength
			)
		}
		edgeLayer.glowLayer.frame = pixelAligned(gradientFrame)
		edgeLayer.lineLayer.frame = pixelAligned(lineFrame)
		configureGradient(edgeLayer.glowLayer, length: gradientLength, alphaScale: 0.24)
		configureGradient(edgeLayer.lineLayer, length: gradientLength, alphaScale: 1.0)
	}

	private func configureGradient(
		_ gradientLayer: CAGradientLayer,
		length: CGFloat,
		alphaScale: CGFloat
	) {
		let stops = gradientStops(length: length, alphaScale: alphaScale)
		gradientLayer.colors = stops.colors
		gradientLayer.locations = stops.locations
	}

	private func gradientStops(
		length: CGFloat,
		alphaScale: CGFloat
	) -> (colors: [CGColor], locations: [NSNumber]) {
		let palette = theme == .dark ? Self.darkPalette : Self.lightPalette
		let step = Self.gradientPeriod / CGFloat(palette.count)
		let safeLength = max(length, 1)
		var colors: [CGColor] = []
		var locations: [NSNumber] = []
		var distance: CGFloat = 0
		var index = 0
		while distance < safeLength {
			let color = palette[index % palette.count]
			colors.append(cgColor(from: color, alphaScale: alphaScale))
			locations.append(NSNumber(value: Double(distance / safeLength)))
			distance += step
			index += 1
		}
		let color = palette[index % palette.count]
		colors.append(cgColor(from: color, alphaScale: alphaScale))
		locations.append(1)
		return (colors, locations)
	}

	private func cgColor(
		from color: (CGFloat, CGFloat, CGFloat, CGFloat),
		alphaScale: CGFloat
	) -> CGColor {
		NSColor(
			calibratedRed: color.0,
			green: color.1,
			blue: color.2,
			alpha: min(max(color.3 * alphaScale, 0), 1)
		).cgColor
	}

	private func edgeFrame(
		for edge: Edge,
		strokeRect: CGRect,
		glowWidth: CGFloat
	) -> CGRect {
		let half = glowWidth / 2
		switch edge {
		case .top:
			return CGRect(
				x: strokeRect.minX - half,
				y: strokeRect.minY - half,
				width: strokeRect.width + glowWidth,
				height: glowWidth
			)
		case .right:
			return CGRect(
				x: strokeRect.maxX - half,
				y: strokeRect.minY - half,
				width: glowWidth,
				height: strokeRect.height + glowWidth
			)
		case .bottom:
			return CGRect(
				x: strokeRect.minX - half,
				y: strokeRect.maxY - half,
				width: strokeRect.width + glowWidth,
				height: glowWidth
			)
		case .left:
			return CGRect(
				x: strokeRect.minX - half,
				y: strokeRect.minY - half,
				width: glowWidth,
				height: strokeRect.height + glowWidth
			)
		}
	}

	private func pixelAligned(_ rect: CGRect) -> CGRect {
		let scale = max(contentsScale, 1)
		return CGRect(
			x: floor(rect.minX * scale) / scale,
			y: floor(rect.minY * scale) / scale,
			width: ceil(rect.width * scale) / scale,
			height: ceil(rect.height * scale) / scale
		)
	}

	private func selectionFlowLineWidth() -> CGFloat {
		theme == .dark ? Self.darkLineWidth : Self.lightLineWidth
	}

	private func selectionFlowGlowLineWidth() -> CGFloat {
		theme == .dark ? Self.darkGlowLineWidth : Self.lightGlowLineWidth
	}

	private func selectionFlowGlowOpacity() -> Float {
		theme == .dark ? 0.30 : 0.34
	}
}

@MainActor
final class LiveOverlayRenderer {
	private weak var hostView: NSView?
	private let rootLayer = CALayer()
	private let frozenDisplayLayer = CALayer()
	private let scrimLayer = CAShapeLayer()
	private let topScrimLayer = CALayer()
	private let leftScrimLayer = CALayer()
	private let rightScrimLayer = CALayer()
	private let bottomScrimLayer = CALayer()
	private let hoverGlowLayer = CAShapeLayer()
	private let hoverFlowLayer = SelectionFlowBandLayer()
	private let dragBorderOutlineLayer = CAShapeLayer()
	private let dragBorderLayer = CAShapeLayer()
	private let selectionSizeLayer = CATextLayer()
	private let hudLayer = CALayer()
	private let hudGlassLayer = CALayer()
	private let hudFillLayer = CALayer()
	private let hudStrokeLayer = CAShapeLayer()
	private let hudPositionLayer = CATextLayer()
	private let hudHexLayer = CATextLayer()
	private let hudSwatchLayer = CALayer()
	private let hudKeycapLayer = CALayer()
	private let hudKeycapTextLayer = CATextLayer()
	private let loupeLayer = CALayer()
	private let loupeGlassLayer = CALayer()
	private let loupeFillLayer = CALayer()
	private let loupeStrokeLayer = CAShapeLayer()
	private let loupePatchLayer = CALayer()
	private let loupeCenterLayer = CAShapeLayer()
	private let frameClock = LiveFrameClockDriver()
	private let layerRenderDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.layer_render_duration",
		category: "LiveChromeTelemetry"
	)
	private let layerChromeRenderDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.layer_chrome_render_duration",
		category: "LiveChromeTelemetry"
	)
	private let snapshotDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.snapshot_duration",
		category: "LiveChromeTelemetry"
	)
	private let layerChromeRenderGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.layer_chrome_render_gap",
		category: "LiveChromeTelemetry"
	)
	private let activeLayerChromeRenderGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.active_layer_chrome_render_gap",
		category: "LiveChromeTelemetry"
	)
	private static let activeInputWindow: TimeInterval = 0.25
	private static let hudColorPendingAnimationKey = "rsnap.hud.color.pending"
	private static let hudColorResolveAnimationKey = "rsnap.hud.color.resolve"
	private static let hudColorResolveBackgroundAnimationKey = "rsnap.hud.color.resolve.background"
	private enum LayerZ {
		static let frozenDisplay: CGFloat = 0
		static let scrim: CGFloat = 10
		static let selectionChrome: CGFloat = 30
		static let selectionSize: CGFloat = 40
		static let hudChrome: CGFloat = 1000
	}

	private var snapshotProvider: (() -> LivePreviewSnapshot?)?
	private var lastRenderedFocusRect: CGRect?
	private var lastRenderedFocusFlowAnimates = false
	private var lastChromeRenderUptime: TimeInterval?
	private var lastActiveChromeRenderUptime: TimeInterval?
	private var lastHudColorPending: Bool?

	init(hostView: NSView) {
		self.hostView = hostView
		configureLayers()
		frameClock.onTick = { [weak self] in
			self?.renderFrameTick()
		}
	}

	func install(snapshotProvider: @escaping () -> LivePreviewSnapshot?) {
		self.snapshotProvider = snapshotProvider
		guard let hostView else {
			return
		}
		if hostView.layer == nil {
			hostView.wantsLayer = true
		}
		hostView.layer?.addSublayer(rootLayer)
		rootLayer.isHidden = true
	}

	func updateDisplayID(_ displayID: CGDirectDisplayID?, targetFramesPerSecond: Int) {
		guard displayID != nil else {
			stop()
			return
		}
		frameClock.start(targetFramesPerSecond: targetFramesPerSecond)
	}

	func stop() {
		frameClock.stop()
		hideRootAndResetRenderState()
	}

	func suspend() {
		hideRootAndResetRenderState()
	}

	func renderNow() {
		renderCurrentSnapshot()
	}

	func renderLiveChromeNow() {
		renderChromeSnapshot()
	}

	func moveLiveChrome(hudFrame: CGRect?, loupeFrame: CGRect?) {
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		if let hudFrame, !hudLayer.isHidden, layerFrameNeedsUpdate(hudLayer.frame, hudFrame) {
			hudLayer.frame = hudFrame
		}
		if let loupeFrame, !loupeLayer.isHidden,
			layerFrameNeedsUpdate(loupeLayer.frame, loupeFrame)
		{
			loupeLayer.frame = loupeFrame
		}
		CATransaction.commit()
	}

	private func configureLayers() {
		rootLayer.zPosition = 100
		rootLayer.masksToBounds = true
		frozenDisplayLayer.isHidden = true
		frozenDisplayLayer.zPosition = LayerZ.frozenDisplay
		rootLayer.addSublayer(frozenDisplayLayer)
		scrimLayer.fillRule = .evenOdd
		scrimLayer.isHidden = true
		scrimLayer.zPosition = LayerZ.scrim
		rootLayer.addSublayer(scrimLayer)
		for scrimLayer in [topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer] {
			rootLayer.addSublayer(scrimLayer)
			scrimLayer.isHidden = true
			scrimLayer.zPosition = LayerZ.scrim
		}
		hoverGlowLayer.fillColor = NSColor.clear.cgColor
		hoverGlowLayer.lineWidth = 2.25
		hoverGlowLayer.shadowOffset = .zero
		hoverGlowLayer.shadowRadius = 12
		hoverGlowLayer.zPosition = LayerZ.selectionChrome
		rootLayer.addSublayer(hoverGlowLayer)

		hoverFlowLayer.zPosition = LayerZ.selectionChrome
		rootLayer.addSublayer(hoverFlowLayer)

		dragBorderOutlineLayer.fillColor = NSColor.clear.cgColor
		dragBorderOutlineLayer.zPosition = LayerZ.selectionChrome
		rootLayer.addSublayer(dragBorderOutlineLayer)

		dragBorderLayer.fillColor = NSColor.clear.cgColor
		dragBorderLayer.zPosition = LayerZ.selectionChrome
		rootLayer.addSublayer(dragBorderLayer)

		selectionSizeLayer.contentsScale = 2
		selectionSizeLayer.zPosition = LayerZ.selectionSize
		rootLayer.addSublayer(selectionSizeLayer)

		for chromeLayer in [hudLayer, loupeLayer] {
			chromeLayer.masksToBounds = false
			chromeLayer.zPosition = LayerZ.hudChrome
			rootLayer.addSublayer(chromeLayer)
		}
		for hudSublayer in [
			hudGlassLayer, hudFillLayer, hudStrokeLayer, hudSwatchLayer, hudPositionLayer,
			hudHexLayer, hudKeycapLayer, hudKeycapTextLayer,
		] {
			hudLayer.addSublayer(hudSublayer)
		}
		for loupeSublayer in [
			loupeGlassLayer, loupeFillLayer, loupeStrokeLayer, loupePatchLayer, loupeCenterLayer,
		] {
			loupeLayer.addSublayer(loupeSublayer)
		}
		for chromeLayer in [hudLayer, loupeLayer] {
			chromeLayer.isHidden = true
		}
	}

	private func renderCurrentSnapshot() {
		guard let snapshot = currentSnapshot() else {
			hideRootAndResetRenderState()
			return
		}
		renderFullSnapshot(snapshot)
	}

	private func renderFrameTick() {
		guard let snapshot = currentSnapshot() else {
			hideRootAndResetRenderState()
			return
		}
		let focusRect = snapshot.dragSelectionLocal ?? snapshot.hoverSelectionLocal
		let focusFlowAnimates = shouldAnimateSelectionFlow(snapshot)
		if snapshot.frozenPending || snapshot.dragSelectionLocal != nil
			|| focusRect != lastRenderedFocusRect
			|| focusFlowAnimates != lastRenderedFocusFlowAnimates
		{
			renderFullSnapshot(snapshot)
		} else {
			renderChromeSnapshot(snapshot)
		}
	}

	private func renderFullSnapshot(_ snapshot: LivePreviewSnapshot) {
		let renderStart = ProcessInfo.processInfo.systemUptime
		recordChromeRenderGap(at: renderStart, snapshot: snapshot)
		defer {
			layerRenderDurationMetric.recordMillisecondsSince(renderStart)
		}
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		rootLayer.isHidden = false
		rootLayer.frame = snapshot.bounds
		renderFrozenDisplay(snapshot)
		renderFocus(snapshot)
		lastRenderedFocusRect = snapshot.dragSelectionLocal ?? snapshot.hoverSelectionLocal
		lastRenderedFocusFlowAnimates = shouldAnimateSelectionFlow(snapshot)
		renderHud(snapshot)
		renderLoupe(snapshot)
		CATransaction.commit()
	}

	private func renderChromeSnapshot() {
		guard let snapshot = currentSnapshot() else {
			hideRootAndResetRenderState()
			return
		}
		renderChromeSnapshot(snapshot)
	}

	private func hideRootAndResetRenderState() {
		rootLayer.isHidden = true
		lastRenderedFocusRect = nil
		lastRenderedFocusFlowAnimates = false
		lastChromeRenderUptime = nil
		lastActiveChromeRenderUptime = nil
		resetHudColorAnimationState()
		hoverFlowLayer.hide()
	}

	private func currentSnapshot() -> LivePreviewSnapshot? {
		let snapshotStart = ProcessInfo.processInfo.systemUptime
		defer {
			snapshotDurationMetric.recordMillisecondsSince(snapshotStart)
		}
		return snapshotProvider?()
	}

	private func renderChromeSnapshot(_ snapshot: LivePreviewSnapshot) {
		let renderStart = ProcessInfo.processInfo.systemUptime
		recordChromeRenderGap(at: renderStart, snapshot: snapshot)
		defer {
			layerChromeRenderDurationMetric.recordMillisecondsSince(renderStart)
		}
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		rootLayer.isHidden = false
		rootLayer.frame = snapshot.bounds
		renderHud(snapshot)
		renderLoupe(snapshot)
		CATransaction.commit()
	}

	private func layerFrameNeedsUpdate(_ current: CGRect, _ next: CGRect) -> Bool {
		abs(current.minX - next.minX) > 0.001
			|| abs(current.minY - next.minY) > 0.001
			|| abs(current.width - next.width) > 0.001
			|| abs(current.height - next.height) > 0.001
	}

	private func recordChromeRenderGap(at now: TimeInterval, snapshot: LivePreviewSnapshot) {
		if let lastChromeRenderUptime {
			let gapMilliseconds = (now - lastChromeRenderUptime) * 1_000
			if gapMilliseconds >= 0, gapMilliseconds < 250 {
				layerChromeRenderGapMetric.record(gapMilliseconds)
			}
		}
		lastChromeRenderUptime = now
		guard let inputUptime = snapshot.inputUptime,
			now - inputUptime <= Self.activeInputWindow
		else {
			lastActiveChromeRenderUptime = nil
			return
		}
		if let lastActiveChromeRenderUptime {
			let activeGapMilliseconds = (now - lastActiveChromeRenderUptime) * 1_000
			if activeGapMilliseconds >= 0, activeGapMilliseconds < 250 {
				activeLayerChromeRenderGapMetric.record(activeGapMilliseconds)
			}
		}
		lastActiveChromeRenderUptime = now
	}

	private func renderFrozenDisplay(_ snapshot: LivePreviewSnapshot) {
		guard let image = snapshot.frozenDisplayImage, let frame = snapshot.frozenDisplayFrame
		else {
			frozenDisplayLayer.isHidden = true
			frozenDisplayLayer.contents = nil
			return
		}
		frozenDisplayLayer.contentsGravity = .resize
		frozenDisplayLayer.contentsScale = hostView?.window?.screen?.backingScaleFactor ?? 2
		frozenDisplayLayer.frame = frame
		frozenDisplayLayer.contents = image
		frozenDisplayLayer.isHidden = false
	}

	private func renderFocus(_ snapshot: LivePreviewSnapshot) {
		let focusRect = snapshot.dragSelectionLocal ?? snapshot.hoverSelectionLocal
		guard let focusRect else {
			scrimLayer.isHidden = true
			for scrimLayer in [topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer] {
				scrimLayer.isHidden = true
			}
			hoverGlowLayer.isHidden = true
			hoverFlowLayer.hide()
			dragBorderOutlineLayer.isHidden = true
			dragBorderLayer.isHidden = true
			selectionSizeLayer.isHidden = true
			return
		}

		let scrimAlpha = CGFloat(CaptureChrome.liveScrimAlpha)
		let scrimColor = NSColor(calibratedWhite: 0, alpha: scrimAlpha).cgColor
		let bounds = snapshot.bounds
		for legacyScrimLayer in [topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer] {
			legacyScrimLayer.isHidden = true
		}
		updateScrimLayer(bounds: bounds, focusRect: focusRect, color: scrimColor)

		if snapshot.frozenPending {
			hoverGlowLayer.isHidden = true
			hoverFlowLayer.hide()
			dragBorderOutlineLayer.isHidden = false
			dragBorderLayer.isHidden = false
			selectionSizeLayer.isHidden = true
			let pixelsPerPoint = hostView?.window?.screen?.backingScaleFactor ?? 1
			let borderOutset = CaptureChrome.dashedBorderOutset(
				strokeWidth: CaptureChrome.frozenDashedBorderWidth,
				pixelsPerPoint: pixelsPerPoint
			)
			let borderRect = focusRect.insetBy(dx: -borderOutset, dy: -borderOutset)
			let layerFrame = dashedBorderLayerFrame(
				for: borderRect,
				lineWidth: CaptureChrome.frozenDashedBorderWidth + 0.75
			)
			let localBorderRect = borderRect.offsetBy(
				dx: -layerFrame.minX,
				dy: -layerFrame.minY
			)
			let frozenPath = CaptureChrome.dashedBorderPath(for: localBorderRect)
			for layer in [dragBorderOutlineLayer, dragBorderLayer] {
				layer.frame = layerFrame
				layer.masksToBounds = true
			}
			dragBorderOutlineLayer.path = frozenPath
			dragBorderOutlineLayer.strokeColor =
				NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
				.cgColor
			dragBorderOutlineLayer.lineWidth = CaptureChrome.frozenDashedBorderWidth + 0.75
			dragBorderOutlineLayer.lineCap = .butt
			dragBorderOutlineLayer.lineJoin = .miter
			dragBorderLayer.path = frozenPath
			dragBorderLayer.strokeColor =
				NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 248 / 255)
				.cgColor
			dragBorderLayer.lineWidth = CaptureChrome.frozenDashedBorderWidth
			dragBorderLayer.lineCap = .butt
			dragBorderLayer.lineJoin = .miter
			return
		}

		if let dragSelection = snapshot.dragSelectionLocal {
			hoverGlowLayer.isHidden = true
			hoverFlowLayer.hide()
			dragBorderOutlineLayer.isHidden = false
			dragBorderLayer.isHidden = false
			let pixelsPerPoint = hostView?.window?.screen?.backingScaleFactor ?? 1
			let borderOutset = CaptureChrome.dashedBorderOutset(
				strokeWidth: CaptureChrome.liveDashedBorderWidth,
				pixelsPerPoint: pixelsPerPoint
			)
			let borderRect = dragSelection.insetBy(dx: -borderOutset, dy: -borderOutset)
			let layerFrame = dashedBorderLayerFrame(
				for: borderRect,
				lineWidth: CaptureChrome.liveDashedBorderWidth + 0.75
			)
			let localBorderRect = borderRect.offsetBy(
				dx: -layerFrame.minX,
				dy: -layerFrame.minY
			)
			let dragPath = CaptureChrome.dashedBorderPath(for: localBorderRect)
			for layer in [dragBorderOutlineLayer, dragBorderLayer] {
				layer.frame = layerFrame
				layer.masksToBounds = true
			}
			dragBorderOutlineLayer.path = dragPath
			dragBorderOutlineLayer.strokeColor =
				NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
				.cgColor
			dragBorderOutlineLayer.lineWidth = CaptureChrome.liveDashedBorderWidth + 0.75
			dragBorderOutlineLayer.lineCap = .butt
			dragBorderOutlineLayer.lineJoin = .miter
			dragBorderLayer.path = dragPath
			dragBorderLayer.strokeColor =
				NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor
			dragBorderLayer.lineWidth = CaptureChrome.liveDashedBorderWidth
			dragBorderLayer.lineCap = .butt
			dragBorderLayer.lineJoin = .miter
			if let selectionSizeText = snapshot.selectionSizeText {
				let font = LiveOverlayTypography.font
				let textSize = selectionSizeText.size(using: font)
				let frame = CaptureChrome.selectionSizeBadgeFrame(
					for: dragSelection,
					textSize: textSize,
					in: bounds
				)
				applyText(
					selectionSizeLayer,
					text: selectionSizeText,
					font: font,
					color: NSColor.white.withAlphaComponent(0.98),
					frame: frame,
					alignment: .left
				)
				selectionSizeLayer.isHidden = false
			} else {
				selectionSizeLayer.isHidden = true
			}
			return
		}

		dragBorderOutlineLayer.isHidden = true
		dragBorderLayer.isHidden = true
		selectionSizeLayer.isHidden = true
		let hoverPath = NSBezierPath(
			roundedRect: focusRect,
			xRadius: CaptureChrome.liveSelectionCornerRadius,
			yRadius: CaptureChrome.liveSelectionCornerRadius
		).cgPath
		hoverGlowLayer.path = hoverPath
		hoverGlowLayer.isHidden = true
		let contentsScale = hostView?.window?.screen?.backingScaleFactor ?? 2
		let animatesFlow = shouldAnimateSelectionFlow(snapshot)
		let flowFrame = flowLayerFrame(for: focusRect, scale: contentsScale)
		hoverFlowLayer.update(
			frame: flowFrame,
			focusRect: focusRect.offsetBy(dx: -flowFrame.minX, dy: -flowFrame.minY),
			theme: snapshot.theme,
			timestamp: CACurrentMediaTime(),
			contentsScale: contentsScale,
			animates: animatesFlow
		)
	}

	private func dashedBorderLayerFrame(for borderRect: CGRect, lineWidth: CGFloat) -> CGRect {
		let padding = max(lineWidth + 2, 4)
		return borderRect.insetBy(dx: -padding, dy: -padding)
	}

	private func updateScrimLayer(bounds: CGRect, focusRect: CGRect, color: CGColor) {
		let path = CGMutablePath()
		path.addRect(bounds)
		let visibleFocusRect = focusRect.intersection(bounds)
		if !visibleFocusRect.isNull, visibleFocusRect.width > 0, visibleFocusRect.height > 0 {
			path.addRect(visibleFocusRect)
		}
		scrimLayer.frame = bounds
		scrimLayer.path = path
		scrimLayer.fillColor = color
		scrimLayer.isHidden = false
	}

	private func shouldAnimateSelectionFlow(_ snapshot: LivePreviewSnapshot) -> Bool {
		guard snapshot.dragSelectionLocal == nil, snapshot.hoverSelectionLocal != nil,
			!snapshot.frozenPending
		else {
			return false
		}
		return true
	}

	private func flowLayerFrame(for focusRect: CGRect, scale: CGFloat) -> CGRect {
		let outset: CGFloat = 24
		let expanded = focusRect.insetBy(dx: -outset, dy: -outset)
		let safeScale = max(scale, 1)
		return CGRect(
			x: floor(expanded.minX * safeScale) / safeScale,
			y: floor(expanded.minY * safeScale) / safeScale,
			width: ceil(expanded.width * safeScale) / safeScale,
			height: ceil(expanded.height * safeScale) / safeScale
		)
	}

	private func renderHud(_ snapshot: LivePreviewSnapshot) {
		guard let hudFrame = snapshot.hudFrame else {
			hudLayer.isHidden = true
			resetHudColorAnimationState()
			return
		}
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		hudLayer.isHidden = false
		hudLayer.frame = hudFrame
		applySurfaceStyle(
			container: hudLayer,
			glassLayer: hudGlassLayer,
			fillLayer: hudFillLayer,
			strokeLayer: hudStrokeLayer,
			frame: hudLayer.bounds,
			palette: palette,
			settings: snapshot.settings,
			glassImage: snapshot.glassPatches[.hud]
		)

		let font = LiveOverlayTypography.font
		let swatchSize = CaptureChrome.hudSwatchSize
		let positionText =
			"x=\(snapshot.positionDisplay.xValueText),y=\(snapshot.positionDisplay.yValueText)"
		let positionSize = CGSize(
			width: snapshot.positionDisplay.xSlotWidth
				+ LiveOverlayTypography.commaWidth
				+ snapshot.positionDisplay.ySlotWidth,
			height: LiveOverlayTypography.lineHeight
		)
		var cursorX = CaptureChrome.hudInnerMarginX
		let baselineY = (hudLayer.bounds.height - positionSize.height) / 2
		applyText(
			hudPositionLayer,
			text: positionText,
			font: font,
			color: palette.labelText,
			frame: CGRect(
				x: cursorX, y: baselineY, width: ceil(positionSize.width),
				height: ceil(positionSize.height)),
			alignment: .left
		)
		cursorX += positionSize.width + CaptureChrome.hudGroupSpacing

		hudSwatchLayer.frame = CGRect(
			x: cursorX,
			y: hudLayer.bounds.midY - swatchSize.height / 2,
			width: swatchSize.width,
			height: swatchSize.height
		)
		hudSwatchLayer.cornerRadius = 0
		let pendingSwatchColor = palette.labelText.withAlphaComponent(0.16)
		let swatchColor =
			snapshot.rgbSample.map {
				NSColor(
					calibratedRed: CGFloat($0.r) / 255, green: CGFloat($0.g) / 255,
					blue: CGFloat($0.b) / 255, alpha: 1)
			} ?? pendingSwatchColor
		hudSwatchLayer.backgroundColor = swatchColor.cgColor
		hudSwatchLayer.borderColor = palette.swatchStroke.cgColor
		hudSwatchLayer.borderWidth = 1
		cursorX += swatchSize.width + CaptureChrome.hudColorItemSpacing

		let hexTextColor =
			snapshot.colorDisplay.isPending
			? palette.labelText.withAlphaComponent(0.46) : palette.labelText
		applyText(
			hudHexLayer, text: snapshot.colorDisplay.hexText, font: font, color: hexTextColor,
			frame: CGRect(
				x: cursorX, y: baselineY, width: ceil(snapshot.colorDisplay.hexSlotWidth),
				height: ceil(LiveOverlayTypography.lineHeight)), alignment: .left)
		updateHudColorAnimation(
			isPending: snapshot.colorDisplay.isPending,
			pendingSwatchColor: pendingSwatchColor,
			resolvedSwatchColor: swatchColor
		)
		cursorX += snapshot.colorDisplay.hexSlotWidth + CaptureChrome.hudGroupSpacing

		if snapshot.keycapVisible {
			let keycapText = "Tab"
			let keycapFont = font
			let keycapFrame = CGRect(
				x: cursorX,
				y: hudLayer.bounds.midY - LiveOverlayTypography.keycapFrameSize.height / 2,
				width: LiveOverlayTypography.keycapFrameSize.width,
				height: LiveOverlayTypography.keycapFrameSize.height
			)
			hudKeycapLayer.isHidden = false
			hudKeycapTextLayer.isHidden = false
			hudKeycapLayer.frame = keycapFrame
			hudKeycapLayer.cornerRadius = 6
			hudKeycapLayer.backgroundColor = palette.keycapFill.cgColor
			hudKeycapLayer.borderColor = palette.keycapStroke.cgColor
			hudKeycapLayer.borderWidth = 1
			applyText(
				hudKeycapTextLayer, text: keycapText, font: keycapFont, color: palette.keycapText,
				frame: centeredTextFrame(for: keycapText, font: keycapFont, in: keycapFrame),
				alignment: .center)
		} else {
			hudKeycapLayer.isHidden = true
			hudKeycapTextLayer.isHidden = true
		}
	}

	private func updateHudColorAnimation(
		isPending: Bool,
		pendingSwatchColor: NSColor,
		resolvedSwatchColor: NSColor
	) {
		if isPending {
			hudSwatchLayer.removeAnimation(forKey: Self.hudColorResolveAnimationKey)
			hudSwatchLayer.removeAnimation(forKey: Self.hudColorResolveBackgroundAnimationKey)
			hudHexLayer.removeAnimation(forKey: Self.hudColorResolveAnimationKey)
			ensureHudColorPendingPulse(on: hudSwatchLayer, from: 0.44, to: 0.78)
			ensureHudColorPendingPulse(on: hudHexLayer, from: 0.48, to: 0.86)
			lastHudColorPending = true
			return
		}

		let wasPending = lastHudColorPending == true
		let priorSwatchColor =
			wasPending ? hudSwatchLayer.presentation()?.backgroundColor : nil
		let priorSwatchOpacity =
			wasPending ? hudSwatchLayer.presentation()?.opacity : nil
		let priorHexOpacity = wasPending ? hudHexLayer.presentation()?.opacity : nil
		hudSwatchLayer.removeAnimation(forKey: Self.hudColorPendingAnimationKey)
		hudHexLayer.removeAnimation(forKey: Self.hudColorPendingAnimationKey)
		lastHudColorPending = false

		guard wasPending else {
			return
		}

		addHudColorResolveAnimation(
			to: hudSwatchLayer,
			fromOpacity: priorSwatchOpacity.map(CGFloat.init) ?? 0.62
		)
		addHudColorResolveAnimation(
			to: hudHexLayer,
			fromOpacity: priorHexOpacity.map(CGFloat.init) ?? 0.62
		)
		let colorAnimation = CABasicAnimation(keyPath: "backgroundColor")
		colorAnimation.fromValue = priorSwatchColor ?? pendingSwatchColor.cgColor
		colorAnimation.toValue = resolvedSwatchColor.cgColor
		colorAnimation.duration = 0.16
		colorAnimation.timingFunction = CAMediaTimingFunction(name: .easeOut)
		hudSwatchLayer.add(
			colorAnimation,
			forKey: Self.hudColorResolveBackgroundAnimationKey
		)
	}

	private func ensureHudColorPendingPulse(
		on layer: CALayer,
		from fromOpacity: Float,
		to toOpacity: Float
	) {
		guard layer.animation(forKey: Self.hudColorPendingAnimationKey) == nil else {
			return
		}
		let animation = CABasicAnimation(keyPath: "opacity")
		animation.fromValue = fromOpacity
		animation.toValue = toOpacity
		animation.duration = 0.42
		animation.autoreverses = true
		animation.repeatCount = .infinity
		animation.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
		layer.add(animation, forKey: Self.hudColorPendingAnimationKey)
	}

	private func addHudColorResolveAnimation(to layer: CALayer, fromOpacity: CGFloat) {
		let animation = CABasicAnimation(keyPath: "opacity")
		animation.fromValue = fromOpacity
		animation.toValue = 1
		animation.duration = 0.16
		animation.timingFunction = CAMediaTimingFunction(name: .easeOut)
		layer.add(animation, forKey: Self.hudColorResolveAnimationKey)
	}

	private func resetHudColorAnimationState() {
		lastHudColorPending = nil
		hudSwatchLayer.removeAnimation(forKey: Self.hudColorPendingAnimationKey)
		hudSwatchLayer.removeAnimation(forKey: Self.hudColorResolveAnimationKey)
		hudSwatchLayer.removeAnimation(forKey: Self.hudColorResolveBackgroundAnimationKey)
		hudHexLayer.removeAnimation(forKey: Self.hudColorPendingAnimationKey)
		hudHexLayer.removeAnimation(forKey: Self.hudColorResolveAnimationKey)
	}

	private func renderLoupe(_ snapshot: LivePreviewSnapshot) {
		guard let loupeFrame = snapshot.loupeFrame, let loupePatch = snapshot.loupePatch else {
			loupeLayer.isHidden = true
			return
		}
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		loupeLayer.isHidden = false
		loupeLayer.frame = loupeFrame
		applySurfaceStyle(
			container: loupeLayer,
			glassLayer: loupeGlassLayer,
			fillLayer: loupeFillLayer,
			strokeLayer: loupeStrokeLayer,
			frame: loupeLayer.bounds,
			palette: palette,
			settings: snapshot.settings,
			glassImage: snapshot.glassPatches[.loupe]
		)
		loupePatchLayer.frame = loupeLayer.bounds.insetBy(dx: 10, dy: 10)
		loupePatchLayer.contentsGravity = .resizeAspectFill
		loupePatchLayer.minificationFilter = .nearest
		loupePatchLayer.magnificationFilter = .nearest
		loupePatchLayer.contents = loupePatch
		let centerRect = CGRect(
			x: loupePatchLayer.frame.midX - CaptureChrome.loupeCellSize / 2,
			y: loupePatchLayer.frame.midY - CaptureChrome.loupeCellSize / 2,
			width: CaptureChrome.loupeCellSize,
			height: CaptureChrome.loupeCellSize
		).insetBy(dx: 1, dy: 1)
		loupeCenterLayer.path = CGPath(rect: centerRect, transform: nil)
		loupeCenterLayer.fillColor = NSColor.clear.cgColor
		loupeCenterLayer.strokeColor = NSColor.white.withAlphaComponent(0.9).cgColor
		loupeCenterLayer.lineWidth = 2
	}

	private func applySurfaceStyle(
		container: CALayer,
		glassLayer: CALayer,
		fillLayer: CALayer,
		strokeLayer: CAShapeLayer,
		frame: CGRect,
		palette: CaptureChromePalette,
		settings: NativeHostSettings,
		glassImage: CGImage?
	) {
		let cornerRadius = CaptureChrome.hudCornerRadius
		let boundsPath = CGPath(
			roundedRect: frame,
			cornerWidth: cornerRadius,
			cornerHeight: cornerRadius,
			transform: nil
		)
		let glassEnabled = settings.usesClassicHudGlass
		let hasNativeLiquidGlass = settings.usesLiquidHudGlass
		let opacity = CaptureChrome.effectiveHudOpacity(settings: settings)
		let hasInlineGlass = glassEnabled && glassImage != nil
		let hasGlass = hasInlineGlass || glassEnabled || hasNativeLiquidGlass

		container.cornerRadius = cornerRadius
		container.shadowColor = palette.shadow.cgColor
		container.shadowOffset = .zero
		container.shadowRadius = 10
		container.shadowOpacity = Float(max(0.12, opacity * 0.75))
		container.shadowPath = boundsPath

		glassLayer.frame = frame
		glassLayer.cornerRadius = cornerRadius
		glassLayer.masksToBounds = true
		glassLayer.contentsGravity = .resizeAspectFill
		glassLayer.contents = glassImage
		glassLayer.opacity = hasInlineGlass ? CaptureChrome.glassOpacity(settings: settings) : 0
		glassLayer.isHidden = !hasInlineGlass

		let usesNativeLiquidGlass = settings.usesLiquidHudGlass
		fillLayer.frame = frame
		fillLayer.cornerRadius = cornerRadius
		fillLayer.backgroundColor =
			usesNativeLiquidGlass
			? NSColor.clear.cgColor
			: CaptureChrome.effectiveBodyFill(
				palette: palette,
				settings: settings,
				hasGlass: hasGlass
			).cgColor

		strokeLayer.frame = frame
		strokeLayer.path = boundsPath
		strokeLayer.fillColor = NSColor.clear.cgColor
		strokeLayer.strokeColor = palette.outerStroke.cgColor
		strokeLayer.lineWidth = 1
	}

	private func applyText(
		_ layer: CATextLayer,
		text: String,
		font: NSFont,
		color: NSColor,
		frame: CGRect,
		alignment: CATextLayerAlignmentMode
	) {
		layer.contentsScale = hostView?.window?.backingScaleFactor ?? 2
		layer.string = text
		layer.font = font
		layer.fontSize = font.pointSize
		layer.foregroundColor = color.cgColor
		layer.alignmentMode = alignment
		layer.frame = frame
		layer.isWrapped = false
	}

	private func centeredTextFrame(for text: String, font: NSFont, in frame: CGRect) -> CGRect {
		let textSize = text.size(using: font)
		let width = ceil(textSize.width)
		let height = ceil(textSize.height)
		return CGRect(
			x: frame.midX - width / 2,
			y: frame.midY - height / 2,
			width: width,
			height: height
		)
	}
}
