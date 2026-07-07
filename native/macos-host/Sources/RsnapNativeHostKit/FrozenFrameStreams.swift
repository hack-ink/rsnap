import AppKit
import CoreGraphics
import Foundation
import ScreenCaptureKit

extension FrozenFrameAuthority {
	func start(
		for screens: [NSScreen],
		captureID: UInt64 = 0,
		source: String = "capture",
		rebuildContentFilter: Bool = false,
		selfCaptureExceptionWindowIDs: Set<CGWindowID> = [],
		includedCurrentProcessWindowIDs: Set<CGWindowID> = []
	) {
		let setupStartedAt = ProcessInfo.processInfo.systemUptime
		let targets = screens.compactMap(ContentFilterPlanner.displayTarget(for:))
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
		orderedFrameHistory = orderedFrameHistory.filter { targetIDs.contains($0.key) }
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
		targets: [FrozenDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		requestID: UInt64
	) {
		ShareableContentLookup.resolveCompleteFilters(
			targets: targets,
			targetIDs: targetIDs,
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
			captureID: captureID,
			source: source,
			startedAtUptime: startedAtUptime,
			retryUntilUptime: retryUntilUptime
		) { [weak self] in
			self?.isCurrentSetupRequest(requestID, targetIDs: targetIDs) == true
		} completion: { [weak self] outcome in
			switch outcome {
			case .prepared(let preparedFilters):
				self?.replaceStreamsFromPreparedFilters(
					targets: targets,
					targetIDs: targetIDs,
					preparedFilters: preparedFilters,
					captureID: captureID,
					source: source,
					startedAtUptime: startedAtUptime,
					requestID: requestID
				)
			case .unavailable:
				self?.finishSetup(targetIDs: targetIDs)
			}
		}
	}

	private func replaceStreamsFromPreparedFilters(
		targets: [FrozenDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		preparedFilters: [CGDirectDisplayID: PreparedContentFilter],
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
		orderedFrameHistory = orderedFrameHistory.filter { targetIDs.contains($0.key) }
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
		targets: [FrozenDisplayTarget],
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		generation requestGeneration: UInt64,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval
	) {
		ShareableContentLookup.resolveFilters(
			targets: targets,
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
			captureID: captureID,
			source: source,
			startedAtUptime: startedAtUptime,
			retryUntilUptime: startedAtUptime + Self.selfCaptureFilterRetryWindow
		) { [weak self] in
			self?.isCurrentGeneration(requestGeneration) == true
		} completion: { [weak self] outcome in
			switch outcome {
			case .prepared(let preparedFilters):
				self?.configureStreams(
					targets: targets,
					preparedFilters: preparedFilters,
					generation: requestGeneration,
					captureID: captureID,
					source: source
				)
			case .unavailable:
				self?.finishSetup(generation: requestGeneration)
			}
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
		orderedFrameHistory.removeAll()
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
		targets: [FrozenDisplayTarget],
		preparedFilters: [CGDirectDisplayID: PreparedContentFilter],
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
			let configuration = ContentFilterPlanner.streamConfiguration(for: target)
			let stream = SCStream(
				filter: preparedFilter.filter,
				configuration: configuration,
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
					selfCaptureFilterComplete: preparedFilter.selfCaptureFilterComplete,
					displayFrame: target.frame,
					filter: preparedFilter.filter,
					configuration: configuration
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
}
