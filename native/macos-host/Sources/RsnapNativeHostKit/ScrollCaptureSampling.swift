import AppKit
@preconcurrency import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	func observeNativeScrollCaptureFrame() {
		guard var state = scrollCaptureState else {
			return
		}
		guard state.sampleDrainProcessing == false else {
			state.sampleLoopScheduled = false
			scrollCaptureState = state
			scheduleNativeScrollCaptureSampleIfNeeded()
			return
		}
		if state.sampleProcessing,
			state.pendingSampleFrames.count >= Self.scrollCaptureMaxPendingSampleFrames
		{
			state.sampleLoopScheduled = false
			scrollCaptureState = state
			scheduleNativeScrollCaptureSampleIfNeeded()
			return
		}
		state.sampleLoopScheduled = false
		if let settleDelay = nativeScrollCaptureControlledSettleDelayRemaining(state: state) {
			scrollCaptureState = state
			scheduleNativeScrollCaptureSample(
				extendingWindowBy: settleDelay + Self.scrollCaptureSampleInterval,
				delay: settleDelay
			)
			return
		}
		state.sampleDrainProcessing = true
		state.sampleDrainSequence &+= 1
		let sampleDrainSequence = state.sampleDrainSequence
		let sampleUptime = ProcessInfo.processInfo.systemUptime
		let captureID = currentCaptureTelemetryID
		scrollCaptureState = state

		let liveFrameRequest = ScrollCaptureLiveFrameRequest(
			stream: liveFrameStream,
			rect: state.viewportRect,
			pixelRect: state.viewportPixelRect,
			afterFrameSequence: state.lastQueuedStreamFrameSequence,
			maximumFrameAgeMicroseconds: nativeScrollCaptureMaximumStreamFrameAge(state: state),
			maxFrames: Self.scrollCaptureMaxFramesPerSample,
			waitForFresh: false
		)
		let fallbackRequest =
			nativeScrollCaptureFallbackReadyForInput(state: state)
				&& nativeScrollCaptureFallbackAllowed(at: sampleUptime)
			? ScrollCaptureFallbackRequest(
				rect: state.viewportSamplingRect,
				pixelRect: state.viewportPixelRect,
				source: state.captureSource,
				frameSequence: state.lastQueuedStreamFrameSequence &+ 1
			) : nil

		enqueueNativeScrollCaptureSampleDrain(
			liveFrameRequest: liveFrameRequest,
			fallbackRequest: fallbackRequest,
			captureID: captureID,
			sampleDrainSequence: sampleDrainSequence
		)
	}

	private func enqueueNativeScrollCaptureSampleDrain(
		liveFrameRequest: ScrollCaptureLiveFrameRequest?,
		fallbackRequest: ScrollCaptureFallbackRequest?,
		captureID: UInt64,
		sampleDrainSequence: UInt64
	) {
		scrollCaptureSampleQueue.async { [liveFrameRequest, fallbackRequest] in
			let batch = ScrollCapturePipeline.sampleBatch(
				liveFrameRequest: liveFrameRequest,
				fallbackRequest: fallbackRequest
			)
			DispatchQueue.main.async { [weak self = self] in
				self?.finishNativeScrollCaptureSampleDrain(
					batch,
					captureID: captureID,
					sampleDrainSequence: sampleDrainSequence
				)
			}
		}
	}

	private func finishNativeScrollCaptureSampleDrain(
		_ batch: ScrollCaptureSampleBatch,
		captureID: UInt64,
		sampleDrainSequence: UInt64
	) {
		guard var state = scrollCaptureState,
			currentCaptureTelemetryID == captureID,
			state.sampleDrainSequence == sampleDrainSequence
		else {
			return
		}
		state.sampleDrainProcessing = false
		if let latestFrameSequence = batch.latestFrameSequence {
			state.lastQueuedStreamFrameSequence = max(
				state.lastQueuedStreamFrameSequence,
				latestFrameSequence
			)
		}
		if batch.frames.isEmpty {
			scrollCaptureState = state
			recordNativeScrollCaptureMissingSample(
				state: state,
				sampleSequence: sampleDrainSequence
			)
			scheduleNativeScrollCaptureSampleIfNeeded()
			return
		}
		let availablePendingFrameSlots = max(
			Self.scrollCaptureMaxPendingSampleFrames - state.pendingSampleFrames.count,
			0
		)
		let acceptedFrames = Array(batch.frames.prefix(availablePendingFrameSlots))
		state.pendingSampleFrames.append(contentsOf: acceptedFrames)
		scrollCaptureState = state
		startScrollCaptureObservationsIfNeeded(captureID: captureID)
		scheduleNativeScrollCaptureSampleIfNeeded()
	}

	private func startScrollCaptureObservationsIfNeeded(captureID: UInt64) {
		guard var state = scrollCaptureState,
			currentCaptureTelemetryID == captureID,
			state.sampleProcessing == false,
			state.pendingSampleFrames.isEmpty == false
		else {
			return
		}
		state.sampleProcessing = true
		state.sampleSequence &+= 1
		let sampleSequence = state.sampleSequence
		let observedWheelCount = state.observedWheelCount
		let sampleUptime = ProcessInfo.processInfo.systemUptime
		let previewRefreshDue =
			state.lastPreviewRefreshUptime == 0
			|| sampleUptime - state.lastPreviewRefreshUptime
				>= Self.scrollCapturePreviewRefreshInterval
		let motionRowsHint =
			state.pendingDownwardMotionHintRows > 0
			? Int(state.pendingDownwardMotionHintRows.rounded())
			: nil
		let sampledFrames = Array(
			state.pendingSampleFrames.prefix(Self.scrollCaptureMaxFramesPerSample)
		)
		state.pendingSampleFrames.removeFirst(sampledFrames.count)
		let stitcher = state.stitcher
		scrollCaptureState = state

		enqueueScrollCaptureObservations(
			sampledFrames: sampledFrames,
			stitcher: stitcher,
			motionRowsHint: motionRowsHint,
			previewRefreshDue: previewRefreshDue,
			captureID: captureID,
			sampleSequence: sampleSequence,
			observedWheelCount: observedWheelCount
		)
	}

	private func enqueueScrollCaptureObservations(
		sampledFrames: [ScrollCaptureSampleFrame],
		stitcher: RsnapScrollCaptureSession,
		motionRowsHint: Int?,
		previewRefreshDue: Bool,
		captureID: UInt64,
		sampleSequence: UInt64,
		observedWheelCount: UInt64
	) {
		scrollCaptureStitchQueue.async {
			[sampledFrames, stitcher, motionRowsHint, previewRefreshDue] in
			let batch = ScrollCapturePipeline.makeBatch(
				sampledFrames: sampledFrames,
				stitcher: stitcher,
				motionRowsHint: motionRowsHint,
				previewRefreshDue: previewRefreshDue
			)
			DispatchQueue.main.async { [weak self = self] in
				self?.finishScrollCaptureObservations(
					batch,
					captureID: captureID,
					sampleSequence: sampleSequence,
					observedWheelCount: observedWheelCount,
					motionRowsHint: motionRowsHint
				)
			}
		}
	}

	private func finishScrollCaptureObservations(
		_ batch: ScrollCaptureObservationBatch,
		captureID: UInt64,
		sampleSequence: UInt64,
		observedWheelCount: UInt64,
		motionRowsHint: Int?
	) {
		guard var state = scrollCaptureState,
			currentCaptureTelemetryID == captureID,
			state.sampleSequence == sampleSequence
		else {
			return
		}
		state.sampleProcessing = false
		scrollCaptureState = state
		defer {
			completeNativeScrollCaptureCommandIfNeeded()
			startScrollCaptureObservationsIfNeeded(captureID: captureID)
			scheduleNativeScrollCaptureSampleIfNeeded()
		}
		guard batch.observations.isEmpty == false else {
			recordNativeScrollCaptureMissingSample(state: state, sampleSequence: sampleSequence)
			return
		}

		var latestCommittedExportRevision: UInt64?
		for observation in batch.observations {
			let sampledFrame = observation.sampledFrame
			if let errorDescription = observation.errorDescription {
				NativeHostTelemetry.captureWarning(
					"capture.scroll_observe_failed",
					captureID: captureID,
					stage: "observe_frame",
					error: errorDescription
				)
				try? setHostStatusMessage("Scroll capture could not stitch that frame.")
				refreshOverlay()
				continue
			}
			guard let result = observation.result else {
				continue
			}
			if var latestState = scrollCaptureState {
				latestState.lastStreamFrameSequence = sampledFrame.frameSequence
				if result.outcome == .committed {
					latestState.committedSampleCount &+= 1
					latestState.exportRevision &+= 1
					latestCommittedExportRevision = latestState.exportRevision
					latestState.pendingDownwardMotionHintRows = 0
				} else if result.outcome == .noChange, latestState.controlledScrollInFlight {
					latestState.pendingDownwardMotionHintRows = 0
				} else if result.outcome == .unsupportedDirection {
					latestState.pendingDownwardMotionHintRows = 0
				}
				scrollCaptureState = latestState
			}
			NativeHostTelemetry.captureEvent(
				"capture.scroll_sample_observed",
				captureID: captureID,
				outcome: scrollObserveOutcomeName(result.outcome),
				detail:
					"seq=\(sampleSequence),source=\(sampledFrame.source),registration=\(observation.registrationStrategy),frameSeq=\(sampledFrame.frameSequence),frameAgeMicros=\(sampledFrame.frameAgeMicroseconds),motionRowsHint=\(motionRowsHint ?? 0),growthRows=\(result.growthRows),exportHeight=\(result.exportHeight),viewportTopY=\(result.currentViewportTopY),wheelCount=\(observedWheelCount)"
			)
			guard result.outcome != .noChange else {
				continue
			}
			if result.outcome == .unsupportedDirection {
				try? setHostStatusMessage("Scroll capture only appends downward motion.")
				refreshOverlay()
			}
		}

		if let preview = batch.preview {
			do {
				try refreshNativeScrollCapturePreview(preview)
				if var latestState = scrollCaptureState {
					latestState.lastPreviewRefreshUptime = ProcessInfo.processInfo.systemUptime
					scrollCaptureState = latestState
				}
				let exportMs =
					batch.previewExportMilliseconds.map {
						String(format: "%.2f", $0)
					} ?? "0.00"
				NativeHostTelemetry.captureEvent(
					"capture.scroll_preview_refreshed",
					captureID: captureID,
					detail:
						"seq=\(sampleSequence),exportWidth=\(preview.exportWidth),exportHeight=\(preview.exportHeight),exportMs=\(exportMs)"
				)
			} catch {
				NativeHostTelemetry.captureWarning(
					"capture.scroll_observe_failed",
					captureID: captureID,
					stage: "refresh_preview",
					error: String(describing: error)
				)
				try? setHostStatusMessage("Scroll capture could not stitch that frame.")
				refreshOverlay()
			}
		} else if let previewErrorDescription = batch.previewErrorDescription {
			NativeHostTelemetry.captureWarning(
				"capture.scroll_observe_failed",
				captureID: captureID,
				stage: "refresh_preview",
				error: previewErrorDescription
			)
		}

		if let latestCommittedExportRevision {
			schedulePreparedScrollCaptureExport(
				reason: "scroll_capture_revision_\(latestCommittedExportRevision)",
				revision: latestCommittedExportRevision
			)
		}
	}

	func debugDumpNativeScrollCaptureSnapshot(_ snapshot: RGBARegionSnapshot, name: String) {
		writeNativeScrollCaptureDebugDump(snapshot, name: name)
	}

	func nativeScrollCaptureAcceptsManualInput(state: NativeScrollCaptureState) -> Bool {
		state.controlledScrollInFlight == false
	}

	func nativeScrollCaptureControlledSettleDelayRemaining(
		state: NativeScrollCaptureState
	) -> TimeInterval? {
		guard state.controlledScrollInFlight, state.lastForwardedWheelUptime > 0 else {
			return nil
		}
		let elapsed = ProcessInfo.processInfo.systemUptime - state.lastForwardedWheelUptime
		let remaining = Self.scrollCaptureControlledScrollSettleDelay - elapsed
		return remaining > 0 ? remaining : nil
	}

	func completeNativeScrollCaptureCommandIfNeeded() {
		guard var state = scrollCaptureState, state.controlledScrollInFlight else {
			drainNativeScrollCaptureQueuedWheelIfNeeded()
			return
		}

		state.controlledScrollInFlight = false
		scrollCaptureState = state
		drainNativeScrollCaptureQueuedWheelIfNeeded()
	}

	func nativeScrollCaptureShouldKeepSampling(state: NativeScrollCaptureState) -> Bool {
		ProcessInfo.processInfo.systemUptime < state.sampleUntilUptime
			|| state.controlledScrollInFlight
			|| state.queuedForwardedWheelDeltaY != 0
	}

	func nativeScrollCaptureToolbarBackdropShouldLoop(state: NativeScrollCaptureState) -> Bool {
		nativeScrollCaptureShouldKeepSampling(state: state)
			&& (state.observedWheelCount > 0
				|| state.lastObservedWheelUptime > 0
				|| state.lastForwardedWheelUptime > 0
				|| state.queuedForwardedWheelDeltaY != 0)
	}

	func nativeScrollCaptureNextSampleDelay(state: NativeScrollCaptureState) -> TimeInterval {
		let now = ProcessInfo.processInfo.systemUptime
		if nativeScrollCaptureActiveInputOngoing(state: state, at: now) {
			return Self.scrollCaptureActiveInputSampleInterval
		}
		return Self.scrollCaptureSampleInterval
	}

	func nativeScrollCaptureActiveInputOngoing(
		state: NativeScrollCaptureState,
		at uptime: TimeInterval
	) -> Bool {
		state.lastObservedWheelUptime > 0
			&& uptime - state.lastObservedWheelUptime <= Self.scrollCaptureActiveInputTail
	}

	func recordNativeScrollCaptureMissingSample(
		state: NativeScrollCaptureState,
		sampleSequence: UInt64
	) {
		let now = ProcessInfo.processInfo.systemUptime
		guard now - state.lastMissingSampleStatusUptime > 0.75 else {
			return
		}
		NativeHostTelemetry.captureEvent(
			"capture.scroll_sample_missing",
			captureID: currentCaptureTelemetryID,
			outcome: "no_live_stream_region",
			detail: "seq=\(sampleSequence)"
		)
		if var latestState = scrollCaptureState {
			latestState.lastMissingSampleStatusUptime = now
			scrollCaptureState = latestState
		}
		try? setHostStatusMessage("Scroll capture is waiting for a stable live screen frame.")
		refreshOverlay()
	}

	private func nativeScrollCaptureMaximumStreamFrameAge(
		state: NativeScrollCaptureState
	) -> UInt64? {
		if nativeScrollCaptureFallbackReadyForInput(state: state) {
			return UInt64(Self.scrollCaptureActiveInputLiveFrameMaxAge * 1_000_000)
		}
		return UInt64(Self.scrollCaptureInputLiveFrameMaxAge * 1_000_000)
	}

	private func nativeScrollCaptureFallbackAllowed(at uptime: TimeInterval) -> Bool {
		guard var state = scrollCaptureState else {
			return false
		}
		guard
			state.lastFallbackCaptureUptime == 0
				|| uptime - state.lastFallbackCaptureUptime
					>= Self.scrollCaptureFallbackCaptureInterval
		else {
			return false
		}
		state.lastFallbackCaptureUptime = uptime
		scrollCaptureState = state
		return true
	}

	private func nativeScrollCaptureFallbackReadyForInput(
		state: NativeScrollCaptureState
	) -> Bool {
		state.observedWheelCount > 0
			|| state.pendingDownwardMotionHintRows > 0
			|| state.committedSampleCount > 0
	}

	func scrollObserveOutcomeName(_ outcome: ScrollObserveOutcome) -> String {
		switch outcome {
		case .noChange:
			return "no_change"
		case .previewUpdated:
			return "preview_updated"
		case .committed:
			return "committed"
		case .unsupportedDirection:
			return "unsupported_direction"
		}
	}

	private func refreshNativeScrollCapturePreview(
		_ preview: ScrollCapturePreviewUpdate
	) throws {
		guard let state = scrollCaptureState else {
			return
		}

		chromeState.frozenSelectionSnapshot = state.viewportRect
		chromeState.frozenSelectionEditable = false
		chromeState.frozenSelectionInteraction = nil
		chromeState.frozenDisplayFrame = nil
		chromeState.frozenDisplayImage = nil
		chromeState.scrollMinimapPreview = ScrollCaptureMinimapSnapshot(
			image: preview.image,
			exportSizePixels: CGSize(
				width: CGFloat(preview.exportWidth),
				height: CGFloat(preview.exportHeight)
			),
			viewportTopYPixels: CGFloat(preview.viewportTopYPixels),
			viewportHeightPixels: CGFloat(preview.viewportHeightPixels)
		)

		if preview.result.outcome == .committed {
			try setHostStatusMessage(
				"Scroll capture appended \(preview.result.growthRows) px. Copy or save exports the stitched image."
			)
		} else if preview.result.outcome == .unsupportedDirection {
			try setHostStatusMessage("Scroll capture only appends downward motion.")
		}
		refreshOverlay()
	}
}
