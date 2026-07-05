import AppKit
import CoreGraphics
import CoreVideo
import Foundation
import RsnapHostBridge
import ScreenCaptureKit

struct FrozenFrameLatchToken: Sendable {
	let displayID: CGDirectDisplayID
	let generation: UInt64
	let minSequence: UInt64
	let startedAtUptime: TimeInterval
}

struct FrozenFrameSnapshot: @unchecked Sendable {
	let displayID: CGDirectDisplayID
	let displayFrame: CGRect
	let image: CGImage
	let generation: UInt64
	let sequence: UInt64
	let capturedAtUptime: TimeInterval
	let source: String
	let selfCaptureSafe: Bool
	let selfCaptureFilterComplete: Bool

	func ageMilliseconds(now: TimeInterval = ProcessInfo.processInfo.systemUptime) -> Double {
		max(0, now - capturedAtUptime) * 1_000
	}
}

/// Owns the screenshot consistency protocol around ScreenCaptureKit's asynchronous frame stream.
///
/// The controller asks this type to prepare an overlay-safe filter, latch a commit point, and
/// resolve that latch into either a fresh frame, a pending self-capture-safe frame, or failure.
/// Keeping those states here prevents cached stream frames from being treated as authoritative
/// screenshots just because a pixel buffer happens to exist.
final class FrozenFrameAuthority: @unchecked Sendable {
	private static let maximumSnapshotAgeMilliseconds = 150.0
	private static let maximumLiveRgbAgeMilliseconds =
		LiveRgbSample.maximumDisplayAge * 1_000
	private static let selfCaptureFilterRetryInterval: TimeInterval = 0.035
	private static let selfCaptureFilterRetryWindow: TimeInterval = 2.5

	struct FrameRecord: @unchecked Sendable {
		let displayID: CGDirectDisplayID
		let displayFrame: CGRect
		let pixelBuffer: CVPixelBuffer
		let generation: UInt64
		let sequence: UInt64
		let capturedAtUptime: TimeInterval
		let selfCaptureFilterComplete: Bool

		func ageMilliseconds(now: TimeInterval = ProcessInfo.processInfo.systemUptime) -> Double {
			max(0, now - capturedAtUptime) * 1_000
		}
	}

	enum SnapshotResolution: Sendable {
		case resolved(FrozenFrameSnapshot)
		case pendingSelfCaptureFrame
		case noFreshFrame
	}

	private final class DisplayStream: @unchecked Sendable {
		let stream: SCStream
		let output: FrozenFrameStreamOutput
		let selfCaptureFilterComplete: Bool

		init(
			stream: SCStream,
			output: FrozenFrameStreamOutput,
			selfCaptureFilterComplete: Bool
		) {
			self.stream = stream
			self.output = output
			self.selfCaptureFilterComplete = selfCaptureFilterComplete
		}

		func stop() {
			stream.stopCapture(completionHandler: nil)
		}
	}

	private struct TelemetryContext {
		let captureID: UInt64
		let source: String
		let startedAtUptime: TimeInterval
	}

	private let stateLock = NSCondition()
	private let outputQueue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.frozen-frame-authority-output",
		qos: .userInteractive
	)
	private static let shareableContentCacheMaxAge: TimeInterval = 3_600
	private static let shareableContentCache = FrozenFrameShareableContentCache()
	private var generation: UInt64 = 0
	private var setupRequestID: UInt64 = 0
	private var setupDisplayIDs: Set<CGDirectDisplayID>?
	private var selfCaptureFilterRequired = false
	private var selfCaptureUnsafeAfterUptime: TimeInterval?
	private var activeDisplayIDs: Set<CGDirectDisplayID> = []
	private var displayTargets: [CGDirectDisplayID: FrozenFrameDisplayTarget] = [:]
	private var streams: [CGDirectDisplayID: DisplayStream] = [:]
	private var latestFrames: [CGDirectDisplayID: FrameRecord] = [:]
	private var firstFrameStartUptimes: [CGDirectDisplayID: TimeInterval] = [:]
	private var firstFrameLoggedDisplayIDs: Set<CGDirectDisplayID> = []
	private var telemetryContext = TelemetryContext(
		captureID: 0, source: "capture", startedAtUptime: 0)

	func refreshShareableContentCache(captureID: UInt64 = 0, source: String = "cache") {
		let startedAtUptime = ProcessInfo.processInfo.systemUptime
		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) {
			content, error in
			guard let content else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.content_cache_refresh_failed",
					captureID: captureID,
					source: source,
					displayID: 0,
					error: String(describing: error)
				)
				return
			}
			guard FrozenFrameContentFilterPlanner.shareableContentHasDisplays(content) else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.content_cache_refresh_invalid",
					captureID: captureID,
					source: source,
					displayID: 0,
					error: FrozenFrameContentFilterPlanner.shareableContentDisplayDetail(
						content,
						requiredDisplayIDs: []
					)
				)
				NativeHostTelemetry.frozenAuthorityContentLookupTiming(
					captureID: captureID,
					source: source,
					totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
					success: false,
					displayCount: content.displays.count,
					windowCount: content.windows.count
				)
				return
			}
			Self.shareableContentCache.store(content)
			NativeHostTelemetry.frozenAuthorityContentLookupTiming(
				captureID: captureID,
				source: source,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
				success: true,
				displayCount: content.displays.count,
				windowCount: content.windows.count
			)
		}
	}

	private static func cachedShareableContent(
		covering displayIDs: Set<CGDirectDisplayID>? = nil
	) -> SCShareableContent? {
		shareableContentCache.fresh(
			maxAge: shareableContentCacheMaxAge,
			covering: displayIDs
		)
	}

	func hasFreshShareableContentCache() -> Bool {
		Self.cachedShareableContent() != nil
	}

	func start(
		for screens: [NSScreen],
		captureID: UInt64 = 0,
		source: String = "capture",
		rebuildContentFilter: Bool = false,
		selfCaptureExceptionWindowIDs: Set<CGWindowID> = [],
		includedCurrentProcessWindowIDs: Set<CGWindowID> = []
	) {
		let setupStartedAt = ProcessInfo.processInfo.systemUptime
		let targets = screens.compactMap(FrozenFrameContentFilterPlanner.displayTarget(for:))
		guard targets.isEmpty == false else {
			stop()
			return
		}
		let targetIDs = Set(targets.map(\.displayID))
		let nextTargets = Dictionary(uniqueKeysWithValues: targets.map { ($0.displayID, $0) })

		stateLock.lock()
		let unchanged = activeDisplayIDs == targetIDs && displayTargets == nextTargets
		let streamsCoverTargets = Set(streams.keys) == targetIDs
		let setupInProgressForTargets = setupDisplayIDs == targetIDs
		displayTargets = nextTargets
		if rebuildContentFilter {
			selfCaptureFilterRequired = true
			selfCaptureUnsafeAfterUptime = setupStartedAt
		}
		// When overlay windows become visible, discard pre-overlay streams instead of updating
		// filters in place. Keep the old stream running until the replacement is configured so a
		// fast click can still freeze a fresh frame, but only if that frame came from a complete
		// self-capture-excluding filter.
		if rebuildContentFilter, unchanged, streamsCoverTargets {
			setupRequestID &+= 1
			let requestID = setupRequestID
			setupDisplayIDs = targetIDs
			updateTelemetryContextLocked(
				captureID: captureID,
				source: source,
				startedAtUptime: setupStartedAt,
				targetIDs: targetIDs
			)
			stateLock.unlock()
			rebuildStreamsFromShareableContent(
				targets: targets,
				targetIDs: targetIDs,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
				captureID: captureID,
				source: source,
				startedAtUptime: setupStartedAt,
				retryUntilUptime: setupStartedAt + Self.selfCaptureFilterRetryWindow,
				requestID: requestID
			)
			return
		}
		if unchanged, streamsCoverTargets || setupInProgressForTargets,
			rebuildContentFilter == false
		{
			updateTelemetryContextLocked(
				captureID: captureID,
				source: source,
				startedAtUptime: setupStartedAt,
				targetIDs: targetIDs
			)
			stateLock.unlock()
			return
		}
		generation &+= 1
		setupRequestID &+= 1
		let requestGeneration = generation
		activeDisplayIDs = targetIDs
		setupDisplayIDs = targetIDs
		if rebuildContentFilter == false {
			selfCaptureFilterRequired = false
			selfCaptureUnsafeAfterUptime = nil
		}
		latestFrames = latestFrames.filter { targetIDs.contains($0.key) }
		updateTelemetryContextLocked(
			captureID: captureID,
			source: source,
			startedAtUptime: setupStartedAt,
			targetIDs: targetIDs
		)
		let staleStreams = streams.values
		streams.removeAll()
		stateLock.unlock()

		for staleStream in staleStreams {
			staleStream.stop()
		}

		configureStreamsFromShareableContent(
			targets: targets,
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
			generation: requestGeneration,
			captureID: captureID,
			source: source,
			startedAtUptime: setupStartedAt
		)
	}

	private func rebuildStreamsFromShareableContent(
		targets: [FrozenFrameDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		requestID: UInt64
	) {
		if configureStreamsFromCachedShareableContent(
			targets: targets,
			targetIDs: targetIDs,
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
			captureID: captureID,
			source: source,
			startedAtUptime: startedAtUptime,
			requestID: requestID
		) {
			return
		}

		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) {
			[weak self] content, error in
			self?.handleShareableContentLookup(
				content,
				error: error,
				targets: targets,
				targetIDs: targetIDs,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				retryUntilUptime: retryUntilUptime,
				requestID: requestID
			)
		}
	}

	private func configureStreamsFromCachedShareableContent(
		targets: [FrozenFrameDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		requestID: UInt64
	) -> Bool {
		guard let content = Self.cachedShareableContent(covering: targetIDs) else {
			return false
		}
		let preparedFilters = FrozenFrameContentFilterPlanner.contentFilters(
			for: targets,
			in: content,
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
		)
		guard FrozenFrameContentFilterPlanner.filtersAreComplete(preparedFilters, for: targets)
		else {
			return false
		}
		NativeHostTelemetry.frozenAuthorityContentLookupTiming(
			captureID: captureID,
			source: source,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
			success: true,
			displayCount: content.displays.count,
			windowCount: content.windows.count
		)
		replaceStreamsFromPreparedFilters(
			targets: targets,
			targetIDs: targetIDs,
			preparedFilters: preparedFilters,
			captureID: captureID,
			source: source,
			startedAtUptime: startedAtUptime,
			requestID: requestID
		)
		return true
	}

	private func handleShareableContentLookup(
		_ content: SCShareableContent?,
		error: Error?,
		targets: [FrozenFrameDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		requestID: UInt64
	) {
		guard let content else {
			NativeHostTelemetry.frozenAuthorityWarning(
				"frozen_authority.content_lookup_failed",
				captureID: captureID,
				source: source,
				displayID: 0,
				error: String(describing: error)
			)
			NativeHostTelemetry.frozenAuthorityContentLookupTiming(
				captureID: captureID,
				source: source,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
				success: false,
				displayCount: targets.count,
				windowCount: 0
			)
			finishSetup(targetIDs: targetIDs)
			return
		}

		let contentCoversTargets = FrozenFrameContentFilterPlanner.shareableContent(
			content, covers: targetIDs)
		NativeHostTelemetry.frozenAuthorityContentLookupTiming(
			captureID: captureID,
			source: source,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
			success: contentCoversTargets,
			displayCount: content.displays.count,
			windowCount: content.windows.count
		)
		guard isCurrentSetupRequest(requestID, targetIDs: targetIDs) else {
			return
		}
		guard contentCoversTargets else {
			NativeHostTelemetry.frozenAuthorityWarning(
				"frozen_authority.content_lookup_invalid",
				captureID: captureID,
				source: source,
				displayID: 0,
				error: FrozenFrameContentFilterPlanner.shareableContentDisplayDetail(
					content, requiredDisplayIDs: targetIDs)
			)
			if ProcessInfo.processInfo.systemUptime < retryUntilUptime {
				retryRebuildStreamsFromShareableContent(
					targets: targets,
					targetIDs: targetIDs,
					selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
					includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
					captureID: captureID,
					source: source,
					startedAtUptime: startedAtUptime,
					retryUntilUptime: retryUntilUptime,
					requestID: requestID
				)
				return
			}
			finishSetup(targetIDs: targetIDs)
			return
		}
		Self.shareableContentCache.store(content)
		let preparedFilters = FrozenFrameContentFilterPlanner.contentFilters(
			for: targets,
			in: content,
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
		)
		guard FrozenFrameContentFilterPlanner.filtersAreComplete(preparedFilters, for: targets)
		else {
			if ProcessInfo.processInfo.systemUptime < retryUntilUptime {
				retryRebuildStreamsFromShareableContent(
					targets: targets,
					targetIDs: targetIDs,
					selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
					includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
					captureID: captureID,
					source: source,
					startedAtUptime: startedAtUptime,
					retryUntilUptime: retryUntilUptime,
					requestID: requestID
				)
				return
			}
			logIncompleteFilters(
				preparedFilters, targets: targets, captureID: captureID, source: source)
			finishSetup(targetIDs: targetIDs)
			return
		}
		replaceStreamsFromPreparedFilters(
			targets: targets,
			targetIDs: targetIDs,
			preparedFilters: preparedFilters,
			captureID: captureID,
			source: source,
			startedAtUptime: startedAtUptime,
			requestID: requestID
		)
	}

	private func retryRebuildStreamsFromShareableContent(
		targets: [FrozenFrameDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		requestID: UInt64
	) {
		DispatchQueue.global(qos: .userInteractive).asyncAfter(
			deadline: .now() + Self.selfCaptureFilterRetryInterval
		) { [weak self] in
			self?.rebuildStreamsFromShareableContent(
				targets: targets,
				targetIDs: targetIDs,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				retryUntilUptime: retryUntilUptime,
				requestID: requestID
			)
		}
	}

	private func replaceStreamsFromPreparedFilters(
		targets: [FrozenFrameDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		preparedFilters: [CGDirectDisplayID: FrozenFramePreparedContentFilter],
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		requestID: UInt64
	) {
		stateLock.lock()
		guard setupRequestID == requestID, activeDisplayIDs == targetIDs else {
			stateLock.unlock()
			return
		}
		generation &+= 1
		let requestGeneration = generation
		setupDisplayIDs = targetIDs
		latestFrames = latestFrames.filter { targetIDs.contains($0.key) }
		updateTelemetryContextLocked(
			captureID: captureID,
			source: source,
			startedAtUptime: startedAtUptime,
			targetIDs: targetIDs
		)
		let staleStreams = streams.values
		streams.removeAll()
		stateLock.unlock()

		for staleStream in staleStreams {
			staleStream.stop()
		}

		configureStreams(
			targets: targets,
			preparedFilters: preparedFilters,
			generation: requestGeneration,
			captureID: captureID,
			source: source
		)
	}

	private func configureStreamsFromShareableContent(
		targets: [FrozenFrameDisplayTarget],
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		generation requestGeneration: UInt64,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval
	) {
		let targetIDs = Set(targets.map(\.displayID))
		if let content = Self.cachedShareableContent(covering: targetIDs) {
			NativeHostTelemetry.frozenAuthorityContentLookupTiming(
				captureID: captureID,
				source: source,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
				success: true,
				displayCount: content.displays.count,
				windowCount: content.windows.count
			)
			let preparedFilters = FrozenFrameContentFilterPlanner.contentFilters(
				for: targets,
				in: content,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
			)
			configureStreams(
				targets: targets,
				preparedFilters: preparedFilters,
				generation: requestGeneration,
				captureID: captureID,
				source: source
			)
			return
		}
		let retryUntilUptime = startedAtUptime + Self.selfCaptureFilterRetryWindow
		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) {
			[weak self] content, error in
			guard let self else {
				return
			}
			guard self.isCurrentGeneration(requestGeneration) else {
				return
			}
			guard let content else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.content_lookup_failed",
					captureID: captureID,
					source: source,
					displayID: 0,
					error: String(describing: error)
				)
				NativeHostTelemetry.frozenAuthorityContentLookupTiming(
					captureID: captureID,
					source: source,
					totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
					success: false,
					displayCount: targets.count,
					windowCount: 0
				)
				self.finishSetup(generation: requestGeneration)
				return
			}
			let contentCoversTargets = FrozenFrameContentFilterPlanner.shareableContent(
				content, covers: targetIDs)
			NativeHostTelemetry.frozenAuthorityContentLookupTiming(
				captureID: captureID,
				source: source,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
				success: contentCoversTargets,
				displayCount: content.displays.count,
				windowCount: content.windows.count
			)
			guard contentCoversTargets else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.content_lookup_invalid",
					captureID: captureID,
					source: source,
					displayID: 0,
					error: FrozenFrameContentFilterPlanner.shareableContentDisplayDetail(
						content,
						requiredDisplayIDs: targetIDs
					)
				)
				if ProcessInfo.processInfo.systemUptime < retryUntilUptime {
					DispatchQueue.global(qos: .userInteractive).asyncAfter(
						deadline: .now() + Self.selfCaptureFilterRetryInterval
					) { [weak self] in
						self?.configureStreamsFromShareableContent(
							targets: targets,
							selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
							includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
							generation: requestGeneration,
							captureID: captureID,
							source: source,
							startedAtUptime: startedAtUptime
						)
					}
					return
				}
				self.finishSetup(generation: requestGeneration)
				return
			}
			Self.shareableContentCache.store(content)
			let preparedFilters = FrozenFrameContentFilterPlanner.contentFilters(
				for: targets,
				in: content,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
			)
			self.configureStreams(
				targets: targets,
				preparedFilters: preparedFilters,
				generation: requestGeneration,
				captureID: captureID,
				source: source
			)
		}
	}

	func stop() {
		stateLock.lock()
		generation &+= 1
		setupRequestID &+= 1
		activeDisplayIDs.removeAll()
		setupDisplayIDs = nil
		selfCaptureFilterRequired = false
		selfCaptureUnsafeAfterUptime = nil
		displayTargets.removeAll()
		latestFrames.removeAll()
		firstFrameStartUptimes.removeAll()
		firstFrameLoggedDisplayIDs.removeAll()
		telemetryContext = TelemetryContext(
			captureID: 0, source: "capture", startedAtUptime: 0)
		let staleStreams = streams.values
		streams.removeAll()
		stateLock.unlock()

		for staleStream in staleStreams {
			staleStream.stop()
		}
	}

	func latchToken(containing point: CGPoint) -> FrozenFrameLatchToken? {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard
			let displayID = displayTargets.first(where: {
				$0.value.frame.inclusivelyContains(point)
			})?.key
		else {
			return nil
		}
		let latestRecord = latestFrames[displayID]
		let tokenRecord =
			latestRecord.flatMap { snapshotEligibleRecordLocked($0) }
		return FrozenFrameLatchToken(
			displayID: displayID,
			generation: tokenRecord?.generation ?? 0,
			minSequence: tokenRecord?.sequence ?? 0,
			startedAtUptime: ProcessInfo.processInfo.systemUptime
		)
	}

	func needsSelfCaptureCompleteFrame(containing point: CGPoint) -> Bool {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard selfCaptureFilterRequired else {
			return false
		}
		guard
			let displayID = displayTargets.first(where: {
				$0.value.frame.inclusivelyContains(point)
			})?.key
		else {
			return false
		}
		guard let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record)
		else {
			return true
		}
		return eligibleRecord.selfCaptureFilterComplete == false
	}

	func hasSelfCaptureCompleteFrame(containing point: CGPoint) -> Bool {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard
			let displayID = displayTargets.first(where: {
				$0.value.frame.inclusivelyContains(point)
			})?.key,
			let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record)
		else {
			return false
		}
		return eligibleRecord.selfCaptureFilterComplete
	}

	func hasSelfCaptureCompleteStream(containing point: CGPoint) -> Bool {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard selfCaptureFilterRequired else {
			return false
		}
		guard
			let displayID = displayTargets.first(where: {
				$0.value.frame.inclusivelyContains(point)
			})?.key,
			let stream = streams[displayID]
		else {
			return false
		}
		return stream.selfCaptureFilterComplete
	}

	func rgbSample(containing point: CGPoint) -> RGBSample? {
		liveRgbSample(containing: point)?.rgb
	}

	func liveRgbSample(containing point: CGPoint) -> LiveRgbSample? {
		stateLock.lock()
		let displayID = displayTargets.first(where: { $0.value.frame.inclusivelyContains(point) })?
			.key
		let record = displayID.flatMap { latestFrames[$0] }.flatMap(snapshotEligibleRecordLocked)
		stateLock.unlock()
		guard let record else {
			return nil
		}
		guard record.ageMilliseconds() <= Self.maximumLiveRgbAgeMilliseconds else {
			return nil
		}
		guard
			let rgb = FrozenFramePixelBufferBridge.rgbSample(
				from: record.pixelBuffer,
				point: point,
				displayFrame: record.displayFrame
			)
		else {
			return nil
		}
		return LiveRgbSample(
			rgb: rgb,
			capturedAtUptime: record.capturedAtUptime,
			source: "frame_authority"
		)
	}

	func loupePatch(containing point: CGPoint, sidePixels: Int) -> CGImage? {
		stateLock.lock()
		let displayID = displayTargets.first(where: { $0.value.frame.inclusivelyContains(point) })?
			.key
		let record = displayID.flatMap { latestFrames[$0] }.flatMap(snapshotEligibleRecordLocked)
		stateLock.unlock()
		guard let record else {
			return nil
		}
		return FrozenFramePixelBufferBridge.loupePatch(
			from: record.pixelBuffer,
			point: point,
			displayFrame: record.displayFrame,
			sidePixels: sidePixels
		)
	}

	func snapshot(
		containing point: CGPoint,
		after token: FrozenFrameLatchToken?,
		maxWait: TimeInterval
	) -> FrozenFrameSnapshot? {
		let deadline = Date(timeIntervalSinceNow: max(0, maxWait))
		stateLock.lock()
		let displayID =
			token?.displayID
			?? displayTargets.first(where: { $0.value.frame.inclusivelyContains(point) })?.key
		guard let displayID else {
			stateLock.unlock()
			return nil
		}
		var source = "post_token"
		var record = freshRecordLocked(displayID: displayID, token: token)
		if record == nil,
			let fallbackRecord = unchangedRecordLocked(
				displayID: displayID,
				token: token
			)
		{
			record = fallbackRecord
			source = "latest_unchanged"
		}
		while record == nil, Date() < deadline {
			stateLock.wait(until: deadline)
			record = freshRecordLocked(displayID: displayID, token: token)
			if record == nil,
				let fallbackRecord = unchangedRecordLocked(
					displayID: displayID,
					token: token
				)
			{
				record = fallbackRecord
				source = "latest_unchanged"
			}
		}
		stateLock.unlock()

		guard let record,
			let image = FrozenFramePixelBufferBridge.makeImage(from: record.pixelBuffer)
		else {
			return nil
		}
		return FrozenFrameSnapshot(
			displayID: record.displayID,
			displayFrame: record.displayFrame,
			image: image,
			generation: record.generation,
			sequence: record.sequence,
			capturedAtUptime: record.capturedAtUptime,
			source: source,
			selfCaptureSafe: true,
			selfCaptureFilterComplete: record.selfCaptureFilterComplete
		)
	}

	func resolveSnapshot(
		containing point: CGPoint,
		after token: FrozenFrameLatchToken?,
		maxWait: TimeInterval
	) -> SnapshotResolution {
		if let snapshot = snapshot(containing: point, after: token, maxWait: maxWait) {
			return .resolved(snapshot)
		}
		if needsSelfCaptureCompleteFrame(containing: point) {
			return .pendingSelfCaptureFrame
		}
		return .noFreshFrame
	}

	private func freshRecordLocked(displayID: CGDirectDisplayID, token: FrozenFrameLatchToken?)
		-> FrameRecord?
	{
		guard let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record)
		else {
			return nil
		}
		guard Self.isFreshForSnapshot(eligibleRecord) else {
			return nil
		}
		guard let token else {
			return eligibleRecord
		}
		if eligibleRecord.capturedAtUptime >= token.startedAtUptime {
			return eligibleRecord
		}
		if eligibleRecord.generation == token.generation,
			eligibleRecord.sequence > token.minSequence
		{
			return eligibleRecord
		}
		if token.minSequence == 0 {
			return eligibleRecord
		}
		return nil
	}

	private func unchangedRecordLocked(displayID: CGDirectDisplayID, token: FrozenFrameLatchToken?)
		-> FrameRecord?
	{
		guard let token, token.minSequence > 0, let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record),
			eligibleRecord.generation == token.generation,
			eligibleRecord.sequence == token.minSequence
		else {
			return nil
		}
		guard Self.isFreshForSnapshot(eligibleRecord) else {
			return nil
		}
		// ScreenCaptureKit display streams may not emit another frame while the display is
		// visually unchanged. Even then, same-sequence frames must stay inside the freshness
		// budget; a complete self-capture-excluding filter proves visibility safety, not age.
		return eligibleRecord
	}

	private func snapshotEligibleRecordLocked(_ record: FrameRecord) -> FrameRecord? {
		if isSelfCaptureSafeLocked(record) == false {
			return nil
		}
		return record
	}

	private func isSelfCaptureSafeLocked(_ record: FrameRecord) -> Bool {
		if record.selfCaptureFilterComplete {
			return true
		}
		guard selfCaptureFilterRequired else {
			return true
		}
		guard let unsafeAfterUptime = selfCaptureUnsafeAfterUptime else {
			return false
		}
		return record.capturedAtUptime < unsafeAfterUptime
	}

	private static func isFreshForSnapshot(_ record: FrameRecord) -> Bool {
		record.ageMilliseconds() <= maximumSnapshotAgeMilliseconds
	}

	private func configureStreams(
		targets: [FrozenFrameDisplayTarget],
		preparedFilters: [CGDirectDisplayID: FrozenFramePreparedContentFilter],
		generation requestGeneration: UInt64,
		captureID: UInt64,
		source: String
	) {
		for target in targets {
			guard let preparedFilter = preparedFilters[target.displayID] else {
				continue
			}

			let output = FrozenFrameStreamOutput(
				displayID: target.displayID,
				displayFrame: target.frame,
				generation: requestGeneration,
				selfCaptureFilterComplete: preparedFilter.selfCaptureFilterComplete
			) { [weak self] frame in
				self?.store(frame: frame, generation: requestGeneration)
			} onStop: { [weak self] displayID, generation in
				self?.handleStreamStopped(displayID: displayID, generation: generation)
			} telemetrySnapshot: { [weak self] in
				self?.currentTelemetrySnapshot() ?? (captureID: captureID, source: source)
			}
			let stream = SCStream(
				filter: preparedFilter.filter,
				configuration: FrozenFrameContentFilterPlanner.streamConfiguration(for: target),
				delegate: output)
			do {
				try stream.addStreamOutput(
					output, type: SCStreamOutputType.screen, sampleHandlerQueue: outputQueue)
			} catch {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.output_install_failed",
					captureID: captureID,
					source: source,
					displayID: target.displayID,
					error: String(describing: error)
				)
				continue
			}

			stateLock.lock()
			let shouldStart =
				generation == requestGeneration
				&& (!selfCaptureFilterRequired || preparedFilter.selfCaptureFilterComplete)
			if shouldStart {
				streams[target.displayID] = DisplayStream(
					stream: stream,
					output: output,
					selfCaptureFilterComplete: preparedFilter.selfCaptureFilterComplete
				)
			}
			stateLock.unlock()
			if selfCaptureFilterRequired, !preparedFilter.selfCaptureFilterComplete {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.self_capture_filter_incomplete",
					captureID: captureID,
					source: source,
					displayID: target.displayID,
					error:
						"expectedWindowCount=\(preparedFilter.expectedWindowCount) matchedWindowCount=\(preparedFilter.matchedWindowCount)"
				)
			}
			guard shouldStart else {
				continue
			}

			stream.startCapture { [weak self] error in
				if let error {
					NativeHostTelemetry.frozenAuthorityWarning(
						"frozen_authority.stream_start_failed",
						captureID: captureID,
						source: source,
						displayID: target.displayID,
						error: String(describing: error)
					)
					self?.handleStreamStopped(
						displayID: target.displayID,
						generation: requestGeneration
					)
				}
			}
		}
		finishSetup(generation: requestGeneration)
	}

	private func logIncompleteFilters(
		_ preparedFilters: [CGDirectDisplayID: FrozenFramePreparedContentFilter],
		targets: [FrozenFrameDisplayTarget],
		captureID: UInt64,
		source: String
	) {
		for target in targets {
			guard let preparedFilter = preparedFilters[target.displayID] else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.self_capture_filter_incomplete",
					captureID: captureID,
					source: source,
					displayID: target.displayID,
					error: "missingFilter"
				)
				continue
			}
			guard preparedFilter.selfCaptureFilterComplete == false else {
				continue
			}
			NativeHostTelemetry.frozenAuthorityWarning(
				"frozen_authority.self_capture_filter_incomplete",
				captureID: captureID,
				source: source,
				displayID: target.displayID,
				error:
					"expectedWindowCount=\(preparedFilter.expectedWindowCount) matchedWindowCount=\(preparedFilter.matchedWindowCount)"
			)
		}
	}

	private func store(
		frame: FrameRecord,
		generation requestGeneration: UInt64
	) {
		var firstFrameTelemetry: TelemetryContext?
		stateLock.lock()
		if generation == requestGeneration, activeDisplayIDs.contains(frame.displayID),
			isSelfCaptureSafeLocked(frame)
		{
			if firstFrameLoggedDisplayIDs.contains(frame.displayID) == false {
				firstFrameLoggedDisplayIDs.insert(frame.displayID)
				let startedAt =
					firstFrameStartUptimes[frame.displayID] ?? telemetryContext.startedAtUptime
				firstFrameTelemetry = TelemetryContext(
					captureID: telemetryContext.captureID,
					source: telemetryContext.source,
					startedAtUptime: startedAt
				)
			}
			latestFrames[frame.displayID] = frame
			stateLock.broadcast()
		}
		stateLock.unlock()
		if let firstFrameTelemetry {
			NativeHostTelemetry.frozenAuthorityFirstFrameTiming(
				captureID: firstFrameTelemetry.captureID,
				source: firstFrameTelemetry.source,
				displayID: frame.displayID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(
					since: firstFrameTelemetry.startedAtUptime),
				frameAgeMilliseconds: frame.ageMilliseconds(),
				sequence: frame.sequence,
				generation: frame.generation,
				selfCaptureSafe: true,
				selfCaptureFilterComplete: frame.selfCaptureFilterComplete
			)
		}
	}

	private func handleStreamStopped(
		displayID: CGDirectDisplayID,
		generation stoppedGeneration: UInt64
	) {
		stateLock.lock()
		if generation == stoppedGeneration {
			streams.removeValue(forKey: displayID)
			latestFrames.removeValue(forKey: displayID)
			firstFrameLoggedDisplayIDs.remove(displayID)
			stateLock.broadcast()
		}
		stateLock.unlock()
	}

	private func finishSetup(generation requestGeneration: UInt64) {
		stateLock.lock()
		if generation == requestGeneration {
			setupDisplayIDs = nil
			stateLock.broadcast()
		}
		stateLock.unlock()
	}

	private func finishSetup(targetIDs: Set<CGDirectDisplayID>) {
		stateLock.lock()
		if setupDisplayIDs == targetIDs {
			setupDisplayIDs = nil
			stateLock.broadcast()
		}
		stateLock.unlock()
	}

	private func isCurrentGeneration(_ requestGeneration: UInt64) -> Bool {
		stateLock.lock()
		let isCurrent = generation == requestGeneration
		stateLock.unlock()
		return isCurrent
	}

	private func isCurrentSetupRequest(_ requestID: UInt64, targetIDs: Set<CGDirectDisplayID>)
		-> Bool
	{
		stateLock.lock()
		let isCurrent = setupRequestID == requestID && activeDisplayIDs == targetIDs
		stateLock.unlock()
		return isCurrent
	}

	private func updateTelemetryContextLocked(
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		targetIDs: Set<CGDirectDisplayID>
	) {
		telemetryContext = TelemetryContext(
			captureID: captureID,
			source: source,
			startedAtUptime: startedAtUptime
		)
		firstFrameStartUptimes = firstFrameStartUptimes.filter { targetIDs.contains($0.key) }
		firstFrameLoggedDisplayIDs.removeAll(keepingCapacity: true)
		for targetID in targetIDs {
			firstFrameStartUptimes[targetID] = startedAtUptime
		}
	}

	private func currentTelemetrySnapshot() -> (captureID: UInt64, source: String) {
		stateLock.lock()
		let snapshot = (captureID: telemetryContext.captureID, source: telemetryContext.source)
		stateLock.unlock()
		return snapshot
	}

}
