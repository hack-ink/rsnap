import AppKit
import CoreGraphics
import Foundation
import ScreenCaptureKit

extension FrozenFrameAuthority {
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
}
