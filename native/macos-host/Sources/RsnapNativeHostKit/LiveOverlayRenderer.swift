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
		return snapshots.first(where: { $0.frame.inclusivelyContains(point) })
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
			if isOnScreen == false {
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
		snapshots.first(where: { $0.frame.inclusivelyContains(point) })
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
		if let latestSamplePoint, Self.pointsEquivalent(latestSamplePoint, point) {
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
			?? Self.recentPatchSample(
				previousSample: previousSample,
				canReuseRecentPatch: canReuseRecentPatch
			)
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

	private static func recentPatchSample(
		previousSample: LiveChromeSample?,
		canReuseRecentPatch: Bool
	) -> LiveChromeSample? {
		guard canReuseRecentPatch, let loupePatch = previousSample?.loupePatch else {
			return nil
		}
		return LiveChromeSample(rgb: nil, loupePatch: loupePatch)
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
		guard alreadyRunning == false else {
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
	private final class FlowPassLayers {
		let containerLayer = CALayer()
		let gradientLayer = CAGradientLayer()
		let maskLayer = CAShapeLayer()
		let alphaScale: CGFloat

		init(alphaScale: CGFloat) {
			self.alphaScale = alphaScale
		}
	}

	private static let pathOutset: CGFloat = 1.0
	private static let darkLineWidth: CGFloat = 1.8
	private static let lightLineWidth: CGFloat = 1.9
	private static let darkGlowLineWidth: CGFloat = 5.0
	private static let lightGlowLineWidth: CGFloat = 5.25
	private static let flowAnimationKey = "rsnap.selection-flow.rotation"
	private static let flowAnimationDuration: CFTimeInterval = 2.45
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

	private let glowPass = FlowPassLayers(alphaScale: 0.24)
	private let linePass = FlowPassLayers(alphaScale: 1.0)
	private let cornerAccentLayer = CAShapeLayer()
	private var focusRect: CGRect = .null
	private var theme: CaptureChromeTheme = .dark
	private var flowAnimating = false

	override init() {
		super.init()
		contentsScale = NSScreen.main?.backingScaleFactor ?? 2
		isOpaque = false
		allowsEdgeAntialiasing = true
		masksToBounds = false
		configureLayers()
	}

	override init(layer: Any) {
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
		guard isHidden == false || focusRect.isNull == false else {
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
		animates: Bool,
		roundedExclusions _: [OverlayMaskGeometry.RoundedExclusion]
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

	func updateRoundedExclusions(_: [OverlayMaskGeometry.RoundedExclusion]) {}

	private func configureLayers() {
		for pass in [glowPass, linePass] {
			pass.containerLayer.masksToBounds = false
			pass.containerLayer.allowsEdgeAntialiasing = true
			pass.containerLayer.addSublayer(pass.gradientLayer)
			pass.containerLayer.mask = pass.maskLayer
			addSublayer(pass.containerLayer)

			pass.gradientLayer.type = .conic
			pass.gradientLayer.startPoint = CGPoint(x: 0.5, y: 0.5)
			pass.gradientLayer.endPoint = CGPoint(x: 1.0, y: 0.5)
			pass.gradientLayer.allowsEdgeAntialiasing = true

			pass.maskLayer.fillColor = NSColor.clear.cgColor
			pass.maskLayer.strokeColor = NSColor.white.cgColor
			pass.maskLayer.lineCap = .butt
			pass.maskLayer.lineJoin = .miter
			pass.maskLayer.allowsEdgeAntialiasing = true
		}
		glowPass.containerLayer.opacity = selectionFlowGlowOpacity()

		cornerAccentLayer.fillColor = NSColor.clear.cgColor
		cornerAccentLayer.lineCap = .butt
		cornerAccentLayer.lineJoin = .miter
		cornerAccentLayer.allowsEdgeAntialiasing = true
		addSublayer(cornerAccentLayer)
	}

	private func updateAppearance() {
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		let strokeRect = focusRect.insetBy(dx: -Self.pathOutset, dy: -Self.pathOutset)
		update(glowPass, strokeRect: strokeRect, lineWidth: selectionFlowGlowLineWidth())
		update(linePass, strokeRect: strokeRect, lineWidth: selectionFlowLineWidth())
		updateCornerAccent(strokeRect: strokeRect)
		CATransaction.commit()
	}

	private func installFlowAnimation(restartsAnimation: Bool) {
		let hasAnimations = linePass.gradientLayer.animation(forKey: Self.flowAnimationKey) != nil
		if restartsAnimation == false, hasAnimations {
			return
		}
		removeFlowAnimation()
		installFlowAnimation(on: linePass.gradientLayer)
	}

	private func installFlowAnimation(on layer: CALayer) {
		let keyPath = "transform.rotation.z"
		let currentRotation =
			(layer.presentation()?.value(forKeyPath: keyPath) as? CGFloat) ?? 0
		let animation = CABasicAnimation(keyPath: keyPath)
		animation.fromValue = currentRotation
		animation.toValue = currentRotation + CGFloat.pi * 2
		animation.duration = Self.flowAnimationDuration
		animation.repeatCount = .infinity
		animation.timingFunction = CAMediaTimingFunction(name: .linear)
		layer.add(animation, forKey: Self.flowAnimationKey)
	}

	private func removeFlowAnimation() {
		for pass in [glowPass, linePass] {
			pass.gradientLayer.removeAnimation(forKey: Self.flowAnimationKey)
		}
	}

	private func update(_ pass: FlowPassLayers, strokeRect: CGRect, lineWidth: CGFloat) {
		let layerBounds = bounds
		pass.containerLayer.frame = layerBounds
		pass.containerLayer.isHidden = layerBounds.width <= 0 || layerBounds.height <= 0
		pass.containerLayer.opacity = pass === glowPass ? selectionFlowGlowOpacity() : 1.0
		pass.gradientLayer.frame = pixelAligned(conicGradientFrame(in: layerBounds))
		pass.gradientLayer.colors = gradientColors(alphaScale: pass.alphaScale)
		pass.gradientLayer.locations = gradientLocations()

		pass.maskLayer.frame = layerBounds
		pass.maskLayer.contentsScale = contentsScale
		pass.maskLayer.lineWidth = lineWidth
		pass.maskLayer.path = NSBezierPath(rect: strokeRect).cgPath
	}

	private func conicGradientFrame(in layerBounds: CGRect) -> CGRect {
		let side = max(hypot(layerBounds.width, layerBounds.height), 1)
		return CGRect(
			x: layerBounds.midX - side / 2,
			y: layerBounds.midY - side / 2,
			width: side,
			height: side
		)
	}

	private func updateCornerAccent(strokeRect: CGRect) {
		cornerAccentLayer.frame = bounds
		cornerAccentLayer.contentsScale = contentsScale
		cornerAccentLayer.lineWidth = selectionFlowLineWidth()
		cornerAccentLayer.opacity = theme == .dark ? 0.86 : 0.72
		cornerAccentLayer.strokeColor = cgColor(
			from: (theme == .dark ? Self.darkPalette[0] : Self.lightPalette[0]),
			alphaScale: 0.90
		)
		cornerAccentLayer.path = selectionFlowCornerAccentPath(for: strokeRect)
	}

	private func selectionFlowCornerAccentPath(for rect: CGRect) -> CGPath {
		let overhang = selectionFlowCornerOverhang()
		let inset = overhang * 1.4
		let path = CGMutablePath()
		path.move(to: CGPoint(x: rect.minX - overhang, y: rect.minY))
		path.addLine(to: CGPoint(x: rect.minX + inset, y: rect.minY))
		path.move(to: CGPoint(x: rect.maxX, y: rect.minY - overhang))
		path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY + inset))
		path.move(to: CGPoint(x: rect.maxX + overhang, y: rect.maxY))
		path.addLine(to: CGPoint(x: rect.maxX - inset, y: rect.maxY))
		path.move(to: CGPoint(x: rect.minX, y: rect.maxY + overhang))
		path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY - inset))
		path.move(to: CGPoint(x: rect.maxX + overhang, y: rect.minY))
		path.addLine(to: CGPoint(x: rect.maxX - inset, y: rect.minY))
		path.move(to: CGPoint(x: rect.maxX, y: rect.maxY + overhang))
		path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY - inset))
		path.move(to: CGPoint(x: rect.minX - overhang, y: rect.maxY))
		path.addLine(to: CGPoint(x: rect.minX + inset, y: rect.maxY))
		path.move(to: CGPoint(x: rect.minX, y: rect.minY - overhang))
		path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + inset))
		return path
	}

	private func gradientColors(alphaScale: CGFloat) -> [CGColor] {
		let palette = theme == .dark ? Self.darkPalette : Self.lightPalette
		var colors = palette.map { cgColor(from: $0, alphaScale: alphaScale) }
		if let first = palette.first {
			colors.append(cgColor(from: first, alphaScale: alphaScale))
		}
		return colors
	}

	private func gradientLocations() -> [NSNumber] {
		let paletteCount = max((theme == .dark ? Self.darkPalette : Self.lightPalette).count, 1)
		return (0...paletteCount).map { index in
			NSNumber(value: Double(index) / Double(paletteCount))
		}
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

	private func selectionFlowCornerOverhang() -> CGFloat {
		max(selectionFlowGlowLineWidth() / 2, 3)
	}
}

private final class LiveScrimLayer: CAShapeLayer {
	private let exclusionMaskLayer = CAShapeLayer()
	private var renderedBounds = CGRect.null
	private var focusRect = CGRect.null
	private var roundedExclusions: [OverlayMaskGeometry.RoundedExclusion] = []
	var scrimColor: CGColor =
		NSColor(calibratedWhite: 0, alpha: CGFloat(CaptureChrome.liveScrimAlpha)).cgColor

	override init() {
		super.init()
		configureShape()
	}

	override init(layer: Any) {
		if let layer = layer as? LiveScrimLayer {
			renderedBounds = layer.renderedBounds
			focusRect = layer.focusRect
			roundedExclusions = layer.roundedExclusions
			scrimColor = layer.scrimColor
		}
		super.init(layer: layer)
		configureShape()
	}

	private func configureShape() {
		isOpaque = false
		fillRule = .evenOdd
		fillColor = scrimColor
		strokeColor = nil
		needsDisplayOnBoundsChange = false
		exclusionMaskLayer.fillRule = .evenOdd
		exclusionMaskLayer.fillColor = NSColor.black.cgColor
		exclusionMaskLayer.strokeColor = nil
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func update(
		focusRect: CGRect,
		color: CGColor,
		roundedExclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		let currentBounds = bounds
		guard
			renderedBounds != currentBounds
				|| self.focusRect != focusRect
				|| !CFEqual(scrimColor, color)
				|| self.roundedExclusions != roundedExclusions
		else {
			return
		}
		renderedBounds = currentBounds
		self.focusRect = focusRect
		self.scrimColor = color
		self.roundedExclusions = roundedExclusions
		fillColor = color
		path = OverlayMaskGeometry.scrimPath(
			bounds: currentBounds,
			focusRect: focusRect
		)
		updateExclusionMask(bounds: currentBounds, roundedExclusions: roundedExclusions)
	}

	private func updateExclusionMask(
		bounds: CGRect,
		roundedExclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		guard roundedExclusions.isEmpty == false else {
			mask = nil
			return
		}
		exclusionMaskLayer.frame = bounds
		exclusionMaskLayer.contentsScale = contentsScale
		exclusionMaskLayer.path = OverlayMaskGeometry.evenOddMaskPath(
			bounds: bounds,
			roundedExclusions: roundedExclusions
		)
		mask = exclusionMaskLayer
	}
}

@MainActor
final class LiveOverlayRenderer {
	private weak var hostView: NSView?
	private let rootLayer = CALayer()
	private let chromeRootLayer = CALayer()
	private let frozenDisplayLayer = CALayer()
	private let scrimLayer = LiveScrimLayer()
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
	private let hudHexRollLayer = CALayer()
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
	private static let hudColorResolveAnimationKey = "rsnap.hud.color.resolve"
	private static let hudColorResolveBackgroundAnimationKey = "rsnap.hud.color.resolve.background"
	private static let hudColorRollAnimationKey = "rsnap.hud.color.roll"
	private static let hudColorPendingRollAnimationKey = "rsnap.hud.color.pending.roll"
	private static let hudColorRollDuration: TimeInterval = 0.40
	private static let hudColorRollDigitStagger: TimeInterval = 0.024
	private static let hexWheel = Array("0123456789ABCDEF")
	private static let pendingHexRollBaseSeed: UInt64 = 0x5EED_71A5_C01D
	private struct HudHexPendingRollColumnState {
		let digits: [Character]
		let scrollsUp: Bool
		let contentLayer: CALayer
	}
	private enum LayerZ {
		static let root: CGFloat = 100
		static let chromeRoot: CGFloat = 300
		static let frozenDisplay: CGFloat = 0
		static let scrim: CGFloat = 10
		static let selectionChrome: CGFloat = 30
		static let selectionSize: CGFloat = 40
		static let hudChrome: CGFloat = 1_000
	}

	private var snapshotProvider: (() -> LivePreviewSnapshot?)?
	private var lastRenderedFocusRect: CGRect?
	private var lastRenderedFocusFlowAnimates = false
	private var lastChromeRenderUptime: TimeInterval?
	private var lastActiveChromeRenderUptime: TimeInterval?
	private var lastHudColorPending: Bool?
	private var hudColorRevealArmed = true
	private var hasResolvedHudColor = false
	private var lastResolvedHudHexText: String?
	private var lastResolvedHudSwatchColor: CGColor?
	private var activeHudHexRollTarget: String?
	private var activeHudHexRollSwatchColor: CGColor?
	private var hudHexRollAnimationEndUptime: TimeInterval?
	private var hudHexPendingRollActive = false
	private var hudHexPendingRollColumns: [HudHexPendingRollColumnState] = []

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
		hostView.layer?.addSublayer(chromeRootLayer)
		rootLayer.isHidden = true
		chromeRootLayer.isHidden = true
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

	func moveLiveChrome(
		hudFrame: CGRect?,
		loupeFrame: CGRect?,
		chromeExclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
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
		updateLiveScrimExclusions(excluding: chromeExclusions)
		updateLiveFlowExclusions(excluding: chromeExclusions)
		CATransaction.commit()
	}

	private func configureLayers() {
		rootLayer.zPosition = LayerZ.root
		rootLayer.masksToBounds = true
		chromeRootLayer.zPosition = LayerZ.chromeRoot
		chromeRootLayer.masksToBounds = true
		frozenDisplayLayer.isHidden = true
		frozenDisplayLayer.zPosition = LayerZ.frozenDisplay
		rootLayer.addSublayer(frozenDisplayLayer)
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
			chromeRootLayer.addSublayer(chromeLayer)
		}
		for hudSublayer in [
			hudGlassLayer, hudFillLayer, hudStrokeLayer, hudSwatchLayer, hudPositionLayer,
			hudHexLayer, hudHexRollLayer, hudKeycapLayer, hudKeycapTextLayer,
		] {
			hudLayer.addSublayer(hudSublayer)
		}
		hudHexRollLayer.masksToBounds = false
		hudHexRollLayer.isHidden = true
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
		chromeRootLayer.isHidden = false
		chromeRootLayer.frame = snapshot.bounds
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
		chromeRootLayer.isHidden = true
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
		chromeRootLayer.isHidden = false
		chromeRootLayer.frame = snapshot.bounds
		let chromeExclusions = liveChromeRoundedExclusions(for: snapshot)
		updateLiveScrimExclusions(excluding: chromeExclusions)
		updateLiveFlowExclusions(excluding: chromeExclusions)
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
			hideFocusLayers()
			return
		}

		let scrimAlpha = CGFloat(CaptureChrome.liveScrimAlpha)
		let scrimColor = NSColor(calibratedWhite: 0, alpha: scrimAlpha).cgColor
		let bounds = snapshot.bounds
		let chromeExclusions = liveChromeRoundedExclusions(for: snapshot)
		hideLegacyScrimLayers()
		updateScrimLayer(
			bounds: bounds,
			focusRect: focusRect,
			color: scrimColor,
			excluding: chromeExclusions
		)

		if snapshot.frozenPending {
			renderFrozenPendingFocus(focusRect)
			return
		}

		if let dragSelection = snapshot.dragSelectionLocal {
			renderDragSelectionFocus(dragSelection, snapshot: snapshot, bounds: bounds)
			return
		}

		renderHoverFocus(focusRect, snapshot: snapshot, chromeExclusions: chromeExclusions)
	}

	private func hideFocusLayers() {
		scrimLayer.isHidden = true
		hideLegacyScrimLayers()
		hoverGlowLayer.isHidden = true
		hoverFlowLayer.hide()
		dragBorderOutlineLayer.isHidden = true
		dragBorderLayer.isHidden = true
		selectionSizeLayer.isHidden = true
	}

	private func hideLegacyScrimLayers() {
		for scrimLayer in [topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer] {
			scrimLayer.isHidden = true
		}
	}

	private func renderFrozenPendingFocus(_ focusRect: CGRect) {
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
		let localBorderRect = borderRect.offsetBy(dx: -layerFrame.minX, dy: -layerFrame.minY)
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
	}

	private func renderDragSelectionFocus(
		_ dragSelection: CGRect,
		snapshot: LivePreviewSnapshot,
		bounds: CGRect
	) {
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
		let localBorderRect = borderRect.offsetBy(dx: -layerFrame.minX, dy: -layerFrame.minY)
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
		renderSelectionSizeBadge(
			snapshot.selectionSizeText, selection: dragSelection, bounds: bounds)
	}

	private func renderSelectionSizeBadge(
		_ selectionSizeText: String?,
		selection: CGRect,
		bounds: CGRect
	) {
		guard let selectionSizeText else {
			selectionSizeLayer.isHidden = true
			return
		}
		let font = LiveOverlayTypography.font
		let textSize = selectionSizeText.size(using: font)
		let frame = CaptureChrome.selectionSizeBadgeFrame(
			for: selection,
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
	}

	private func renderHoverFocus(
		_ focusRect: CGRect,
		snapshot: LivePreviewSnapshot,
		chromeExclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
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
			animates: animatesFlow,
			roundedExclusions: chromeExclusions
		)
	}

	private func dashedBorderLayerFrame(for borderRect: CGRect, lineWidth: CGFloat) -> CGRect {
		let padding = max(lineWidth + 2, 4)
		return borderRect.insetBy(dx: -padding, dy: -padding)
	}

	private func updateScrimLayer(
		bounds: CGRect,
		focusRect: CGRect,
		color: CGColor,
		excluding roundedExclusions: [OverlayMaskGeometry.RoundedExclusion] = []
	) {
		let effectiveExclusions = Self.visibleScrimExclusions(
			roundedExclusions,
			bounds: bounds,
			focusRect: focusRect
		)
		scrimLayer.frame = bounds
		scrimLayer.contentsScale = hostView?.window?.screen?.backingScaleFactor ?? 2
		scrimLayer.update(
			focusRect: focusRect,
			color: color,
			roundedExclusions: effectiveExclusions
		)
		scrimLayer.isHidden = false
	}

	private static func visibleScrimExclusions(
		_ roundedExclusions: [OverlayMaskGeometry.RoundedExclusion],
		bounds: CGRect,
		focusRect: CGRect
	) -> [OverlayMaskGeometry.RoundedExclusion] {
		roundedExclusions.compactMap { exclusion in
			let visibleRect = exclusion.rect.intersection(bounds)
			guard visibleRect.isNull == false, visibleRect.width > 0, visibleRect.height > 0,
				focusRect.contains(visibleRect) == false
			else {
				return nil
			}
			return OverlayMaskGeometry.RoundedExclusion(
				rect: visibleRect,
				cornerRadius: exclusion.cornerRadius
			)
		}
	}

	private func liveChromeRoundedExclusions(
		for snapshot: LivePreviewSnapshot
	) -> [OverlayMaskGeometry.RoundedExclusion] {
		guard snapshot.settings.hudGlassEnabled else {
			return []
		}
		return [snapshot.hudFrame, snapshot.loupeFrame].compactMap { frame in
			frame.map {
				OverlayMaskGeometry.RoundedExclusion(
					rect: $0,
					cornerRadius: CaptureChrome.hudCornerRadius
				)
			}
		}
	}

	private func updateLiveScrimExclusions(
		excluding exclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		guard scrimLayer.isHidden == false, let focusRect = lastRenderedFocusRect else {
			return
		}
		updateScrimLayer(
			bounds: rootLayer.bounds,
			focusRect: focusRect,
			color: scrimLayer.scrimColor,
			excluding: exclusions
		)
	}

	private func updateLiveFlowExclusions(
		excluding exclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		guard hoverFlowLayer.isHidden == false else {
			return
		}
		hoverFlowLayer.updateRoundedExclusions(exclusions)
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
		let hexFrame = CGRect(
			x: cursorX, y: baselineY, width: ceil(snapshot.colorDisplay.hexSlotWidth),
			height: ceil(LiveOverlayTypography.lineHeight))
		applyText(
			hudHexLayer, text: snapshot.colorDisplay.hexText, font: font, color: hexTextColor,
			frame: hexFrame, alignment: .left)
		updateHudColorAnimation(
			isPending: snapshot.colorDisplay.isPending,
			pendingSwatchColor: pendingSwatchColor,
			resolvedSwatchColor: swatchColor,
			resolvedHexText: snapshot.colorDisplay.hexText,
			hexFrame: hexFrame,
			font: font,
			textColor: palette.labelText
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
		resolvedSwatchColor: NSColor,
		resolvedHexText: String,
		hexFrame: CGRect,
		font: NSFont,
		textColor: NSColor
	) {
		if isPending {
			hudSwatchLayer.removeAnimation(forKey: Self.hudColorResolveAnimationKey)
			hudSwatchLayer.removeAnimation(forKey: Self.hudColorResolveBackgroundAnimationKey)
			hudSwatchLayer.opacity = 1
			hudHexLayer.removeAnimation(forKey: Self.hudColorResolveAnimationKey)
			if hasResolvedHudColor {
				clearHudHexRollAnimation()
				if let lastResolvedHudSwatchColor {
					hudSwatchLayer.backgroundColor = lastResolvedHudSwatchColor
				}
				if let lastResolvedHudHexText {
					applyText(
						hudHexLayer,
						text: lastResolvedHudHexText,
						font: font,
						color: textColor,
						frame: hexFrame,
						alignment: .left
					)
				}
				hudHexLayer.isHidden = false
				lastHudColorPending = false
				hudColorRevealArmed = false
				return
			}
			beginOrUpdateHudHexPendingRollAnimation(
				frame: hexFrame,
				font: font,
				textColor: textColor
			)
			lastHudColorPending = true
			return
		}

		let wasPending = lastHudColorPending == true
		let shouldAnimateReveal = wasPending && hudColorRevealArmed && !hasResolvedHudColor
		let priorSwatchColor =
			wasPending ? hudSwatchLayer.presentation()?.backgroundColor : nil
		let priorSwatchOpacity =
			wasPending ? hudSwatchLayer.presentation()?.opacity : nil
		let priorHexOpacity = wasPending ? hudHexLayer.presentation()?.opacity : nil
		lastResolvedHudHexText = resolvedHexText
		lastResolvedHudSwatchColor = resolvedSwatchColor.cgColor
		hasResolvedHudColor = true
		lastHudColorPending = false
		hudColorRevealArmed = false

		guard shouldAnimateReveal else {
			updateHudHexRollVisibility(
				target: resolvedHexText,
				frame: hexFrame,
				font: font,
				textColor: textColor
			)
			return
		}

		addHudColorResolveAnimation(
			to: hudSwatchLayer,
			fromOpacity: priorSwatchOpacity.map(CGFloat.init) ?? 0.62
		)
		beginHudHexRollAnimation(
			target: resolvedHexText,
			frame: hexFrame,
			font: font,
			textColor: textColor,
			initialOpacity: priorHexOpacity.map(CGFloat.init) ?? 0.62,
			targetSwatchColor: resolvedSwatchColor
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

	private func addHudColorResolveAnimation(to layer: CALayer, fromOpacity: CGFloat) {
		let animation = CABasicAnimation(keyPath: "opacity")
		animation.fromValue = fromOpacity
		animation.toValue = 1
		animation.duration = 0.16
		animation.timingFunction = CAMediaTimingFunction(name: .easeOut)
		layer.add(animation, forKey: Self.hudColorResolveAnimationKey)
	}

	private func updateHudHexRollVisibility(
		target: String,
		frame: CGRect,
		font: NSFont,
		textColor: NSColor
	) {
		guard let activeTarget = activeHudHexRollTarget else {
			clearHudHexRollAnimation()
			hudHexLayer.isHidden = false
			return
		}
		let now = ProcessInfo.processInfo.systemUptime
		if activeTarget != target {
			if let animationEnd = hudHexRollAnimationEndUptime {
				if now < animationEnd {
					if let activeHudHexRollSwatchColor {
						hudSwatchLayer.backgroundColor = activeHudHexRollSwatchColor
					}
					hudHexLayer.isHidden = true
					hudHexRollLayer.isHidden = false
					hudHexRollLayer.frame = frame
					return
				}
				finishHudHexRollAnimation()
			}
			clearHudHexRollAnimation()
			hudHexLayer.isHidden = false
			return
		}
		if let animationEnd = hudHexRollAnimationEndUptime,
			now >= animationEnd
		{
			finishHudHexRollAnimation()
		}

		hudHexLayer.isHidden = true
		hudHexRollLayer.isHidden = false
		hudHexRollLayer.frame = frame
	}

	private func finishHudHexRollAnimation() {
		hudHexRollAnimationEndUptime = nil
		activeHudHexRollSwatchColor = nil
		hudHexPendingRollActive = false
		hudHexPendingRollColumns.removeAll(keepingCapacity: true)
		removeHudHexRollLayerAnimations()
	}

	private func beginOrUpdateHudHexPendingRollAnimation(
		frame: CGRect,
		font: NSFont,
		textColor: NSColor
	) {
		hudHexLayer.isHidden = true
		hudHexRollLayer.isHidden = false
		hudHexRollLayer.frame = frame
		guard hudHexPendingRollActive == false else {
			return
		}

		clearHudHexRollAnimation()
		hudHexPendingRollActive = true
		hudHexPendingRollColumns.removeAll(keepingCapacity: true)
		hudHexLayer.isHidden = true
		hudHexRollLayer.isHidden = false
		hudHexRollLayer.frame = frame

		let lineHeight = ceil(LiveOverlayTypography.lineHeight)
		let characterFrames = hudHexCharacterFrames(
			for: "#FFFFFF",
			font: font,
			lineHeight: lineHeight
		)
		let hashLayer = makeHudHexRollTextLayer(
			text: "#",
			font: font,
			color: textColor.withAlphaComponent(0.72),
			frame: characterFrames.first ?? CGRect(x: 0, y: 0, width: 0, height: lineHeight)
		)
		hudHexRollLayer.addSublayer(hashLayer)

		for index in 0..<6 {
			let characterFrame =
				index + 1 < characterFrames.count
				? characterFrames[index + 1]
				: CGRect(x: 0, y: 0, width: 0, height: lineHeight)
			let columnLayer = CALayer()
			columnLayer.masksToBounds = true
			columnLayer.frame = characterFrame
			hudHexRollLayer.addSublayer(columnLayer)
			let columnState = addHudHexPendingRollColumn(
				to: columnLayer,
				index: index,
				font: font,
				textColor: textColor,
				lineHeight: lineHeight,
				digitWidth: characterFrame.width
			)
			hudHexPendingRollColumns.append(columnState)
		}
	}
	private static func pendingHexRollSeed(index: Int) -> UInt64 {
		let uptimeBucket = UInt64((ProcessInfo.processInfo.systemUptime * 1_000).rounded(.down))
		let mixedIndex = UInt64(index + 1) &* 0x9E37_79B9_7F4A_7C15
		return pendingHexRollBaseSeed ^ uptimeBucket ^ mixedIndex
	}

	private static func pendingHexRollSequence(index: Int) -> [Character] {
		var seed = pendingHexRollSeed(index: index)
		seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
		let visibleRows = 47 + Int((seed >> 57) & 0x1F) + index * 3
		var digits: [Character] = []
		digits.reserveCapacity(visibleRows + 1)
		var previous: Character?
		for offset in 0..<visibleRows {
			seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
			var wheelIndex = Int((seed >> 58) & 0xF)
			if let previousDigit = previous,
				Self.hexWheel[wheelIndex] == previousDigit
			{
				wheelIndex = (wheelIndex + offset + index + 1) % Self.hexWheel.count
			}
			let digit = Self.hexWheel[wheelIndex]
			digits.append(digit)
			previous = digit
		}
		if let first = digits.first {
			digits.append(first)
		}
		return digits
	}

	private static func pendingHexRollColumnDuration(index: Int) -> TimeInterval {
		let seed =
			pendingHexRollSeed(index: index)
			&* 2_862_933_555_777_941_757
			&+ 3_037_000_493
		return 1.58 + Double((seed >> 56) & 0x1F) * 0.031
	}

	private static func pendingHexRollColumnPhase(index: Int, duration: TimeInterval)
		-> TimeInterval
	{
		let seed =
			pendingHexRollSeed(index: index)
			&* 11_400_714_819_323_198_485
			&+ 12_829_314
		let ratio = Double((seed >> 40) & 0xFFFF) / 65_535.0
		return duration * ratio
	}

	private static func pendingHexRollColumnScrollsUp(index: Int) -> Bool {
		let uptimeBucket = UInt64((ProcessInfo.processInfo.systemUptime * 1_000).rounded(.down))
		let startsUp = ((pendingHexRollBaseSeed ^ uptimeBucket) & 1) == 0
		if index <= 1 {
			return index == 0 ? startsUp : !startsUp
		}
		let seed =
			pendingHexRollSeed(index: index)
			&* 3_202_034_522_624_059_733
			&+ 1_029
		return ((seed >> 63) & 1) == 0
	}

	private static func resolveHexRollColumnScrollsUp(
		index: Int,
		startDigit: Character,
		targetDigit: Character
	) -> Bool {
		let startValue = UInt64(startDigit.unicodeScalars.first?.value ?? 0)
		let targetValue = UInt64(targetDigit.unicodeScalars.first?.value ?? 0)
		let seed =
			pendingHexRollSeed(index: index)
			^ (startValue &* 1_099_511_628_211)
			^ (targetValue &* 2_862_933_555_777_941_757)
		let startsUp = ((seed >> 63) & 1) == 0
		if index <= 1 {
			return index == 0 ? startsUp : !startsUp
		}
		return ((seed >> 59) & 1) == 0
	}

	private static func resolveHexRollExtraLoops(index: Int, targetDigit: Character) -> Int {
		let targetValue = UInt64(targetDigit.unicodeScalars.first?.value ?? 0)
		let seed =
			pendingHexRollSeed(index: index)
			^ (targetValue &* 11_400_714_819_323_198_485)
		return 1 + Int((seed >> 60) & 1)
	}

	private static func resolveHexRollSequence(
		from startDigit: Character,
		to targetDigit: Character,
		index: Int,
		scrollsUp: Bool
	) -> [Character] {
		let wheelCount = max(hexWheel.count, 1)
		let startIndex = hexWheel.firstIndex(of: startDigit) ?? 0
		let targetIndex = hexWheel.firstIndex(of: targetDigit) ?? startIndex
		let directedDistance =
			scrollsUp
			? (targetIndex - startIndex + wheelCount) % wheelCount
			: (startIndex - targetIndex + wheelCount) % wheelCount
		let extraSteps =
			resolveHexRollExtraLoops(index: index, targetDigit: targetDigit)
			* wheelCount
		let totalSteps =
			directedDistance + extraSteps
		return (0...totalSteps).map { offset in
			let wheelIndex =
				scrollsUp
				? (startIndex + offset) % wheelCount
				: (startIndex - offset + (totalSteps + wheelCount) * wheelCount) % wheelCount
			return hexWheel[wheelIndex]
		}
	}

	private func beginHudHexRollAnimation(
		target: String,
		frame: CGRect,
		font: NSFont,
		textColor: NSColor,
		initialOpacity: CGFloat,
		targetSwatchColor: NSColor? = nil
	) {
		let lineHeight = ceil(LiveOverlayTypography.lineHeight)
		let startDigits = currentPendingHudHexDigits(lineHeight: lineHeight)
		let pendingDirections = hudHexPendingRollColumns.map(\.scrollsUp)
		clearHudHexRollAnimation()
		activeHudHexRollTarget = target
		activeHudHexRollSwatchColor = targetSwatchColor?.cgColor
		let now = ProcessInfo.processInfo.systemUptime
		let targetDigits = Array(target.dropFirst())
		var rollEndOffset: TimeInterval = 0
		hudHexLayer.isHidden = true
		hudHexRollLayer.isHidden = false
		hudHexRollLayer.frame = frame

		let characterFrames = hudHexCharacterFrames(
			for: target,
			font: font,
			lineHeight: lineHeight
		)
		let hashLayer = makeHudHexRollTextLayer(
			text: "#",
			font: font,
			color: textColor.withAlphaComponent(0.72),
			frame: characterFrames.first ?? CGRect(x: 0, y: 0, width: 0, height: lineHeight)
		)
		hudHexRollLayer.addSublayer(hashLayer)

		for (index, targetDigit) in targetDigits.enumerated() {
			let characterFrame =
				index + 1 < characterFrames.count
				? characterFrames[index + 1]
				: CGRect(x: 0, y: 0, width: 0, height: lineHeight)
			let columnLayer = CALayer()
			columnLayer.masksToBounds = true
			columnLayer.frame = characterFrame
			hudHexRollLayer.addSublayer(columnLayer)
			let startDigit =
				index < startDigits.count
				? startDigits[index]
				: nil
			let resolvedStartDigit = startDigit ?? Self.hexWheel.first ?? targetDigit
			let scrollsUp =
				index < pendingDirections.count
				? pendingDirections[index]
				: Self.resolveHexRollColumnScrollsUp(
					index: index,
					startDigit: resolvedStartDigit,
					targetDigit: targetDigit
				)
			let columnEndOffset = addHudHexRollDigit(
				to: columnLayer,
				startDigit: resolvedStartDigit,
				targetDigit: targetDigit,
				index: index,
				font: font,
				textColor: textColor,
				initialOpacity: initialOpacity,
				lineHeight: lineHeight,
				digitWidth: characterFrame.width,
				scrollsUp: scrollsUp
			)
			hudHexPendingRollColumns.append(columnEndOffset.state)
			rollEndOffset = max(rollEndOffset, columnEndOffset.endOffset)
		}
		hudHexRollAnimationEndUptime = now + rollEndOffset + 0.03
	}

	private func hudHexCharacterFrames(
		for text: String,
		font: NSFont,
		lineHeight: CGFloat
	) -> [CGRect] {
		let characters = Array(text)
		return characters.indices.map { index in
			let prefixStart = String(characters.prefix(index)).size(using: font).width
			let prefixEnd = String(characters.prefix(index + 1)).size(using: font).width
			return CGRect(
				x: prefixStart,
				y: 0,
				width: max(prefixEnd - prefixStart, 1),
				height: lineHeight
			)
		}
	}

	private func addHudHexPendingRollColumn(
		to columnLayer: CALayer,
		index: Int,
		font: NSFont,
		textColor: NSColor,
		lineHeight: CGFloat,
		digitWidth: CGFloat
	) -> HudHexPendingRollColumnState {
		var digits = Self.pendingHexRollSequence(index: index)
		let scrollsUp = Self.pendingHexRollColumnScrollsUp(index: index)
		if scrollsUp == false {
			digits.reverse()
		}
		let contentText = digits.map(String.init).joined(separator: "\n")
		let contentLayer = CALayer()
		contentLayer.frame = CGRect(
			x: 0,
			y: 0,
			width: digitWidth,
			height: lineHeight * CGFloat(digits.count)
		)
		columnLayer.addSublayer(contentLayer)

		let digitLayer = makeHudHexRollMultilineTextLayer(
			text: contentText,
			font: font,
			color: textColor.withAlphaComponent(0.72),
			lineHeight: lineHeight,
			frame: contentLayer.bounds
		)
		contentLayer.addSublayer(digitLayer)

		let animation = CABasicAnimation(keyPath: "transform.translation.y")
		let travel = lineHeight * CGFloat(max(digits.count - 1, 1))
		animation.fromValue = scrollsUp ? 0 : -travel
		animation.toValue = scrollsUp ? -travel : 0
		let duration = Self.pendingHexRollColumnDuration(index: index)
		animation.duration = duration
		animation.beginTime =
			CACurrentMediaTime()
			- Self.pendingHexRollColumnPhase(index: index, duration: duration)
		animation.repeatCount = .infinity
		animation.timingFunction = CAMediaTimingFunction(name: .linear)
		animation.isRemovedOnCompletion = false
		contentLayer.add(animation, forKey: Self.hudColorPendingRollAnimationKey)
		return HudHexPendingRollColumnState(
			digits: digits,
			scrollsUp: scrollsUp,
			contentLayer: contentLayer
		)
	}

	private func currentPendingHudHexDigits(lineHeight: CGFloat) -> [Character?] {
		hudHexPendingRollColumns.map { column in
			guard column.digits.isEmpty == false else {
				return nil
			}
			let presentationLayer = column.contentLayer.presentation() ?? column.contentLayer
			let translationY = presentationLayer.transform.m42
			let rawIndex = Int((-translationY / lineHeight).rounded())
			let visibleIndex = min(max(rawIndex, 0), column.digits.count - 1)
			return column.digits[visibleIndex]
		}
	}

	private func addHudHexRollDigit(
		to columnLayer: CALayer,
		startDigit: Character,
		targetDigit: Character,
		index: Int,
		font: NSFont,
		textColor: NSColor,
		initialOpacity: CGFloat,
		lineHeight: CGFloat,
		digitWidth: CGFloat,
		scrollsUp: Bool
	) -> (state: HudHexPendingRollColumnState, endOffset: TimeInterval) {
		let rollDigits = Self.resolveHexRollSequence(
			from: startDigit,
			to: targetDigit,
			index: index,
			scrollsUp: scrollsUp
		)
		let terminalPaddingRows = 2
		let contentDigits: [Character]
		let startRowIndex: Int
		let targetRowIndex: Int
		if scrollsUp {
			contentDigits =
				rollDigits
				+ Array(repeating: targetDigit, count: terminalPaddingRows)
			startRowIndex = 0
			targetRowIndex = max(rollDigits.count - 1, 0)
		} else {
			contentDigits =
				Array(repeating: targetDigit, count: terminalPaddingRows)
				+ Array(rollDigits.reversed())
			startRowIndex = max(contentDigits.count - 1, 0)
			targetRowIndex = terminalPaddingRows
		}
		let contentLayer = CALayer()
		contentLayer.opacity = Float(max(initialOpacity, 0.72))
		contentLayer.frame = CGRect(
			x: 0,
			y: 0,
			width: digitWidth,
			height: lineHeight * CGFloat(contentDigits.count)
		)
		columnLayer.addSublayer(contentLayer)

		addHudHexRollDigitStack(
			to: contentLayer,
			digits: contentDigits,
			font: font,
			color: textColor,
			lineHeight: lineHeight,
			digitWidth: digitWidth
		)

		let fromY = -lineHeight * CGFloat(startRowIndex)
		let toY = -lineHeight * CGFloat(targetRowIndex)
		contentLayer.transform = CATransform3DMakeTranslation(0, toY, 0)

		let stagger = Double(index) * Self.hudColorRollDigitStagger
		let duration =
			Self.hudColorRollDuration
			+ Double(Self.resolveHexRollExtraLoops(index: index, targetDigit: targetDigit)) * 0.035
		let animation = CABasicAnimation(keyPath: "transform.translation.y")
		animation.fromValue = fromY
		animation.toValue = toY
		animation.beginTime = CACurrentMediaTime() + stagger
		animation.duration = duration
		animation.timingFunction = CAMediaTimingFunction(name: .easeOut)
		animation.fillMode = .both
		animation.isRemovedOnCompletion = false
		contentLayer.add(animation, forKey: Self.hudColorRollAnimationKey)
		let columnState = HudHexPendingRollColumnState(
			digits: contentDigits,
			scrollsUp: scrollsUp,
			contentLayer: contentLayer
		)
		return (columnState, stagger + duration)
	}

	private func addHudHexRollDigitStack(
		to contentLayer: CALayer,
		digits: [Character],
		font: NSFont,
		color: NSColor,
		lineHeight: CGFloat,
		digitWidth: CGFloat
	) {
		for (row, digit) in digits.enumerated() {
			let digitLayer = makeHudHexRollTextLayer(
				text: String(digit),
				font: font,
				color: color,
				frame: CGRect(
					x: 0,
					y: CGFloat(row) * lineHeight,
					width: digitWidth,
					height: lineHeight
				)
			)
			contentLayer.addSublayer(digitLayer)
		}
	}

	private func removeHudHexRollLayerAnimations() {
		hudHexRollLayer.removeAllAnimations()
		for sublayer in hudHexRollLayer.sublayers ?? [] {
			removeAnimationsRecursively(from: sublayer)
		}
	}

	private func removeAnimationsRecursively(from layer: CALayer) {
		layer.removeAllAnimations()
		for sublayer in layer.sublayers ?? [] {
			removeAnimationsRecursively(from: sublayer)
		}
	}

	private func makeHudHexRollTextLayer(
		text: String,
		font: NSFont,
		color: NSColor,
		frame: CGRect
	) -> CATextLayer {
		let layer = CATextLayer()
		layer.contentsScale = hostView?.window?.backingScaleFactor ?? 2
		layer.string = text
		layer.font = font
		layer.fontSize = font.pointSize
		layer.foregroundColor = color.cgColor
		layer.alignmentMode = .left
		layer.frame = frame
		layer.isWrapped = false
		return layer
	}

	private func makeHudHexRollMultilineTextLayer(
		text: String,
		font: NSFont,
		color: NSColor,
		lineHeight: CGFloat,
		frame: CGRect
	) -> CATextLayer {
		let paragraphStyle = NSMutableParagraphStyle()
		paragraphStyle.alignment = .left
		paragraphStyle.lineBreakMode = .byClipping
		paragraphStyle.minimumLineHeight = lineHeight
		paragraphStyle.maximumLineHeight = lineHeight
		let attributedString = NSAttributedString(
			string: text,
			attributes: [
				.font: font,
				.foregroundColor: color,
				.paragraphStyle: paragraphStyle,
			]
		)
		let layer = CATextLayer()
		layer.contentsScale = hostView?.window?.backingScaleFactor ?? 2
		layer.string = attributedString
		layer.alignmentMode = .left
		layer.frame = frame
		layer.isWrapped = true
		layer.truncationMode = .none
		return layer
	}

	private func addHudRollAnimation(
		to layer: CALayer,
		fromY: CGFloat,
		toY: CGFloat,
		opacityValues: [CGFloat],
		keyTimes: [CGFloat],
		beginOffset: TimeInterval,
		duration: TimeInterval,
		timing: CAMediaTimingFunctionName = .easeInEaseOut
	) {
		let translation = CABasicAnimation(keyPath: "transform.translation.y")
		translation.fromValue = fromY
		translation.toValue = toY

		let opacity = CAKeyframeAnimation(keyPath: "opacity")
		opacity.values = opacityValues.map { NSNumber(value: Double($0)) }
		opacity.keyTimes = keyTimes.map { NSNumber(value: Double($0)) }

		let group = CAAnimationGroup()
		group.animations = [translation, opacity]
		group.beginTime = CACurrentMediaTime() + beginOffset
		group.duration = duration
		group.timingFunction = CAMediaTimingFunction(name: timing)
		group.fillMode = .both
		group.isRemovedOnCompletion = false
		layer.add(group, forKey: Self.hudColorRollAnimationKey)
	}

	private func clearHudHexRollAnimation() {
		activeHudHexRollTarget = nil
		activeHudHexRollSwatchColor = nil
		hudHexRollAnimationEndUptime = nil
		hudHexPendingRollActive = false
		hudHexPendingRollColumns.removeAll(keepingCapacity: true)
		removeHudHexRollLayerAnimations()
		for sublayer in hudHexRollLayer.sublayers ?? [] {
			sublayer.removeFromSuperlayer()
		}
		hudHexRollLayer.isHidden = true
	}

	private func resetHudColorAnimationState() {
		lastHudColorPending = nil
		hudColorRevealArmed = true
		hasResolvedHudColor = false
		lastResolvedHudHexText = nil
		lastResolvedHudSwatchColor = nil
		activeHudHexRollTarget = nil
		activeHudHexRollSwatchColor = nil
		hudHexRollAnimationEndUptime = nil
		hudHexPendingRollActive = false
		hudHexPendingRollColumns.removeAll(keepingCapacity: true)
		hudSwatchLayer.removeAnimation(forKey: Self.hudColorResolveAnimationKey)
		hudSwatchLayer.removeAnimation(forKey: Self.hudColorResolveBackgroundAnimationKey)
		hudHexLayer.removeAnimation(forKey: Self.hudColorResolveAnimationKey)
		hudHexLayer.isHidden = false
		clearHudHexRollAnimation()
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
		if hasNativeLiquidGlass {
			container.shadowOpacity = 0
			container.shadowPath = nil
		} else {
			container.shadowColor = palette.shadow.cgColor
			container.shadowOffset = .zero
			container.shadowRadius = 10
			container.shadowOpacity = Float(max(0.12, opacity * 0.75))
			container.shadowPath = boundsPath
		}

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
		fillLayer.isHidden = usesNativeLiquidGlass
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
