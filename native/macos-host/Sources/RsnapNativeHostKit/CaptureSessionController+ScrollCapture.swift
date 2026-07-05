import AppKit
@preconcurrency import CoreGraphics
import Foundation
import RsnapHostBridge

package func scrollCaptureViewportPoint(
	for point: CGPoint,
	in viewportRect: CGRect,
	desktopFrame: CGRect? = nil
) -> CGPoint? {
	for candidate in scrollCaptureViewportPointCandidates(for: point, desktopFrame: desktopFrame)
	where viewportRect.inclusivelyContains(candidate) {
		return candidate
	}
	return nil
}

private func scrollCaptureViewportPointCandidates(
	for point: CGPoint,
	desktopFrame: CGRect?
) -> [CGPoint] {
	let desktop =
		desktopFrame
		?? NSScreen.screens.reduce(CGRect.null) { partial, screen in
			partial.union(screen.frame)
		}
	guard desktop.isNull == false else {
		return [point]
	}
	let flippedPoint = CGPoint(
		x: point.x,
		y: desktop.minY + desktop.maxY - point.y
	)
	if abs(flippedPoint.y - point.y) <= 0.5 {
		return [point]
	}
	return [point, flippedPoint]
}

package struct ScrollCaptureObservedInputPoint: Equatable {
	package let viewportPoint: CGPoint
	package let inputSource: String
	package let insideViewport: Bool
}

package func scrollCaptureObservedInputPoint(
	for point: CGPoint,
	viewportRect: CGRect,
	sourceFrame: CGRect,
	desktopFrame: CGRect? = nil,
	padding: CGFloat
) -> ScrollCaptureObservedInputPoint? {
	if let viewportPoint = scrollCaptureViewportPoint(
		for: point,
		in: viewportRect,
		desktopFrame: desktopFrame
	) {
		return ScrollCaptureObservedInputPoint(
			viewportPoint: viewportPoint,
			inputSource: "viewport",
			insideViewport: true
		)
	}

	let expandedViewport = CGRect(
		x: viewportRect.minX - padding,
		y: viewportRect.minY - padding,
		width: viewportRect.width + padding * 2,
		height: viewportRect.height + padding * 2
	)
	if expandedViewport.inclusivelyContains(point) {
		let viewportPoint = CGPoint(
			x: point.x.clamped(to: viewportRect.minX...viewportRect.maxX),
			y: point.y.clamped(to: viewportRect.minY...viewportRect.maxY)
		)
		return ScrollCaptureObservedInputPoint(
			viewportPoint: viewportPoint,
			inputSource: "near_viewport",
			insideViewport: false
		)
	}
	let expandedSource = sourceFrame.isNull ? CGRect.null : sourceFrame.insetBy(dx: -8, dy: -8)
	for candidate in scrollCaptureViewportPointCandidates(
		for: point,
		desktopFrame: desktopFrame
	) {
		let sourceContainsCandidate =
			expandedSource.isNull == false && expandedSource.inclusivelyContains(candidate)
		let nearViewport = expandedViewport.inclusivelyContains(candidate)
		guard sourceContainsCandidate || nearViewport else {
			continue
		}
		let viewportPoint = CGPoint(
			x: candidate.x.clamped(to: viewportRect.minX...viewportRect.maxX),
			y: candidate.y.clamped(to: viewportRect.minY...viewportRect.maxY)
		)
		return ScrollCaptureObservedInputPoint(
			viewportPoint: viewportPoint,
			inputSource: sourceContainsCandidate ? "capture_source" : "near_viewport",
			insideViewport: false
		)
	}

	return nil
}

private struct NativeScrollCaptureGeometry {
	let baseImage: CGImage
	let baseSnapshot: RGBARegionSnapshot
	let pixelRect: CGRect
	let samplingRect: CGRect
}

extension CaptureSessionController {
	var scrollCaptureToolbarEnabled: Bool {
		guard Self.scrollCaptureEnabled,
			scene.mode == .frozen,
			scrollCaptureState == nil,
			chromeState.frozenSelectionEditable,
			let selection = currentFrozenSelection()
		else {
			return false
		}
		return scrollCaptureSelectionHasSufficientHeight(selection)
	}

	func scheduleNativeScrollCaptureSample(
		extendingWindowBy window: TimeInterval =
			CaptureSessionController.scrollCaptureInputSampleWindow,
		delay: TimeInterval = CaptureSessionController.scrollCaptureSampleInterval
	) {
		guard var state = scrollCaptureState else {
			return
		}

		let now = ProcessInfo.processInfo.systemUptime
		state.sampleUntilUptime = max(state.sampleUntilUptime, now + max(window, 0))
		guard state.sampleLoopScheduled == false, state.sampleDrainProcessing == false else {
			scrollCaptureState = state
			if nativeScrollCaptureToolbarBackdropShouldLoop(state: state) {
				scheduleNativeScrollCaptureToolbarBackdropRefresh()
			}
			return
		}

		state.sampleLoopScheduled = true
		scrollCaptureState = state
		if nativeScrollCaptureToolbarBackdropShouldLoop(state: state) {
			scheduleNativeScrollCaptureToolbarBackdropRefresh()
		}
		DispatchQueue.main.asyncAfter(deadline: .now() + max(delay, 0)) {
			[weak self] in
			self?.observeNativeScrollCaptureFrame()
		}
	}

	func scheduleNativeScrollCaptureToolbarBackdropRefresh() {
		guard var state = scrollCaptureState,
			state.toolbarBackdropLoopScheduled == false,
			nativeScrollCaptureToolbarBackdropShouldLoop(state: state)
		else {
			return
		}
		state.toolbarBackdropLoopScheduled = true
		scrollCaptureState = state
		DispatchQueue.main.asyncAfter(
			deadline: .now() + Self.scrollCaptureToolbarBackdropRefreshInterval
		) { [weak self] in
			self?.refreshNativeScrollCaptureToolbarBackdrop()
		}
	}

	func refreshNativeScrollCaptureToolbarBackdrop() {
		guard var state = scrollCaptureState else {
			return
		}
		state.toolbarBackdropLoopScheduled = false
		scrollCaptureState = state
		guard nativeScrollCaptureToolbarBackdropShouldLoop(state: state) else {
			return
		}
		overlayController?.refreshScrollCaptureToolbarBackdropNow()
		guard let latestState = scrollCaptureState,
			nativeScrollCaptureToolbarBackdropShouldLoop(state: latestState)
		else {
			return
		}
		scheduleNativeScrollCaptureToolbarBackdropRefresh()
	}

	func scheduleNativeScrollCaptureSampleIfNeeded() {
		guard let latestState = scrollCaptureState,
			nativeScrollCaptureShouldKeepSampling(state: latestState)
		else {
			return
		}
		scheduleNativeScrollCaptureSample(
			extendingWindowBy: 0,
			delay: nativeScrollCaptureNextSampleDelay(state: latestState)
		)
	}

	func beginNativeScrollCapture() throws {
		guard Self.scrollCaptureEnabled else {
			try setHostStatusMessage("Scroll capture is temporarily disabled.")
			refreshOverlay()
			return
		}
		guard scrollCaptureState == nil else {
			try setHostStatusMessage("Scroll capture is already active.")
			refreshOverlay()
			return
		}
		guard scene.mode == .frozen, let selection = currentFrozenSelection() else {
			try setHostStatusMessage("Scroll capture requires a frozen selection.")
			refreshOverlay()
			return
		}
		guard chromeState.frozenSelectionEditable else {
			try setHostStatusMessage("Scroll capture requires a dragged region selection.")
			refreshOverlay()
			return
		}
		guard scrollCaptureSelectionHasSufficientHeight(selection) else {
			try setHostStatusMessage("Select a taller region before starting Scroll Capture.")
			refreshOverlay()
			return
		}

		guard
			let captureSource = overlayController?.scrollCaptureFallbackSource(
				near: CGPoint(x: selection.midX, y: selection.midY)
			)
		else {
			try setHostStatusMessage("Scroll capture could not locate the overlay window.")
			refreshOverlay()
			return
		}

		guard let geometry = nativeScrollCaptureGeometry(for: selection)
		else {
			try setHostStatusMessage("Scroll capture could not read the selected region.")
			refreshOverlay()
			return
		}
		let baseImage = geometry.baseImage
		let baseSnapshot = geometry.baseSnapshot
		let baseSource = "frozen_display_region"
		debugDumpNativeScrollCaptureSnapshot(baseSnapshot, name: "base-\(baseSource)")
		let stitcher = try RsnapScrollCaptureSession(
			baseImage: baseSnapshot,
			previewWidthPixels: min(
				baseSnapshot.width,
				Self.scrollCapturePreviewImageWidthPixels
			)
		)
		var initialState = NativeScrollCaptureState(
			stitcher: stitcher,
			viewportRect: selection,
			viewportPixelRect: geometry.pixelRect,
			viewportSamplingRect: geometry.samplingRect,
			captureSource: captureSource,
			viewportPixelsPerPointY: Double(geometry.pixelRect.height)
				/ max(Double(selection.height), 1)
		)
		initialState.sampleUntilUptime =
			ProcessInfo.processInfo.systemUptime + Self.scrollCaptureInitialSampleWindow
		scrollCaptureState = initialState
		installNativeScrollCaptureMonitor()
		overlayController?.prepareCaptureStreamsNow(trigger: "scroll_capture_start")
		prepareNativeScrollCaptureLiveStream(for: selection)
		configureNativeScrollCaptureChrome(
			baseImage: baseImage,
			baseSnapshot: baseSnapshot,
			selection: selection
		)
		try setHostStatusMessage("Scroll capture started. Scroll inside the selection.")
		NativeHostTelemetry.captureEvent(
			"capture.scroll_capture_started",
			captureID: currentCaptureTelemetryID,
			detail:
				"width=\(baseSnapshot.width),height=\(baseSnapshot.height),x=\(Int(selection.minX.rounded())),y=\(Int(selection.minY.rounded())),pixelX=\(Int(geometry.pixelRect.minX.rounded())),pixelY=\(Int(geometry.pixelRect.minY.rounded())),pixelWidth=\(Int(geometry.pixelRect.width.rounded())),pixelHeight=\(Int(geometry.pixelRect.height.rounded())),samplingX=\(Int(geometry.samplingRect.minX.rounded())),samplingY=\(Int(geometry.samplingRect.minY.rounded())),samplingWidth=\(Int(geometry.samplingRect.width.rounded())),samplingHeight=\(Int(geometry.samplingRect.height.rounded())),mode=manual_universal,baseSource=\(baseSource),liveBaseMatched=false"
		)
		NativeHostTelemetry.captureEvent(
			"capture.scroll_capture_mode",
			captureID: currentCaptureTelemetryID,
			outcome: "manual_universal",
			detail:
				"input=selection_passthrough_global_monitor,permission=screen_recording,accessibility_required=false"
		)
		overlayController?.focusWindow(at: CGPoint(x: selection.midX, y: selection.midY))
		overlayController?.setScrollCaptureMousePassthroughActive(true)
		NativeHostTelemetry.captureEvent(
			"capture.scroll_input_ready",
			captureID: currentCaptureTelemetryID,
			detail:
				"input=selection_passthrough_global_monitor,overlay=focused,passthrough=window"
		)
		DispatchQueue.main.async { [weak self] in
			self?.refreshOverlay()
		}
		scheduleNativeScrollCaptureSample(
			extendingWindowBy: Self.scrollCaptureInitialSampleWindow
		)
	}

	private func prepareNativeScrollCaptureLiveStream(for selection: CGRect) {
		cancelPendingScreenCaptureStreamRelease(reason: "scroll_capture_start")
		let prewarmPoint = CGPoint(x: selection.midX, y: selection.midY)
		liveFrameStream.start(
			for: NSScreen.screens,
			prewarmPoint: prewarmPoint,
			captureID: currentCaptureTelemetryID
		)
		liveFrameStream.prime(at: prewarmPoint)
	}

	private func nativeScrollCaptureGeometry(
		for selection: CGRect
	) -> NativeScrollCaptureGeometry? {
		guard
			let displayFrame = chromeState.frozenDisplayFrame,
			let displayImage = chromeState.frozenDisplayImage,
			let pixelRect = try? RsnapExportEncoder.frozenDisplayCropRect(
				imageWidth: displayImage.width,
				imageHeight: displayImage.height,
				displayFrame: displayFrame,
				selection: selection
			),
			let baseImage = displayImage.cropping(to: pixelRect),
			let baseSnapshot = NativeHostImageBridge.rgbaSnapshot(from: baseImage)
		else {
			return nil
		}
		let samplingRect = Self.nativeScrollCaptureSamplingRect(
			pixelRect: pixelRect,
			displayFrame: displayFrame,
			displayImageSize: CGSize(
				width: CGFloat(displayImage.width),
				height: CGFloat(displayImage.height)
			)
		)

		return NativeScrollCaptureGeometry(
			baseImage: baseImage,
			baseSnapshot: baseSnapshot,
			pixelRect: pixelRect,
			samplingRect: samplingRect
		)
	}

	private static func nativeScrollCaptureSamplingRect(
		pixelRect: CGRect,
		displayFrame: CGRect,
		displayImageSize: CGSize
	) -> CGRect {
		let pointsPerPixelX = displayFrame.width / max(displayImageSize.width, 1)
		let pointsPerPixelY = displayFrame.height / max(displayImageSize.height, 1)
		let height = pixelRect.height * pointsPerPixelY
		let maxY = displayFrame.maxY - pixelRect.minY * pointsPerPixelY
		return CGRect(
			x: displayFrame.minX + pixelRect.minX * pointsPerPixelX,
			y: maxY - height,
			width: pixelRect.width * pointsPerPixelX,
			height: height
		)
	}

	private func configureNativeScrollCaptureChrome(
		baseImage: CGImage,
		baseSnapshot: RGBARegionSnapshot,
		selection: CGRect
	) {
		chromeState.frozenOverlay.reset()
		chromeState.frozenSelectionEditable = false
		chromeState.frozenSelectionInteraction = nil
		chromeState.frozenSelectionSnapshot = selection
		chromeState.captureFrameSource = .scrollCapture
		chromeState.captureFrameWindowID = nil
		chromeState.frozenDisplayFrame = nil
		chromeState.frozenDisplayImage = nil
		chromeState.frozenBaseImage = baseImage
		chromeState.scrollMinimapPreview = ScrollCaptureMinimapSnapshot(
			image: baseImage,
			exportSizePixels: CGSize(
				width: CGFloat(baseSnapshot.width),
				height: CGFloat(baseSnapshot.height)
			),
			viewportTopYPixels: 0,
			viewportHeightPixels: CGFloat(baseSnapshot.height)
		)
	}

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

		let liveFrameRequest = NativeScrollCaptureLiveFrameRequest(
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
			? NativeScrollCaptureFallbackRequest(
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
		liveFrameRequest: NativeScrollCaptureLiveFrameRequest?,
		fallbackRequest: NativeScrollCaptureFallbackRequest?,
		captureID: UInt64,
		sampleDrainSequence: UInt64
	) {
		scrollCaptureSampleQueue.async { [liveFrameRequest, fallbackRequest] in
			let batch = NativeScrollCaptureObservationPipeline.sampleBatch(
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
		_ batch: NativeScrollCaptureSampleBatch,
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
		startNativeScrollCaptureObservationsIfNeeded(captureID: captureID)
		scheduleNativeScrollCaptureSampleIfNeeded()
	}

	private func startNativeScrollCaptureObservationsIfNeeded(captureID: UInt64) {
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

		enqueueNativeScrollCaptureObservations(
			sampledFrames: sampledFrames,
			stitcher: stitcher,
			motionRowsHint: motionRowsHint,
			previewRefreshDue: previewRefreshDue,
			captureID: captureID,
			sampleSequence: sampleSequence,
			observedWheelCount: observedWheelCount
		)
	}

	private func enqueueNativeScrollCaptureObservations(
		sampledFrames: [NativeScrollCaptureSampleFrame],
		stitcher: RsnapScrollCaptureSession,
		motionRowsHint: Int?,
		previewRefreshDue: Bool,
		captureID: UInt64,
		sampleSequence: UInt64,
		observedWheelCount: UInt64
	) {
		scrollCaptureStitchQueue.async {
			[sampledFrames, stitcher, motionRowsHint, previewRefreshDue] in
			let batch = NativeScrollCaptureObservationPipeline.makeBatch(
				sampledFrames: sampledFrames,
				stitcher: stitcher,
				motionRowsHint: motionRowsHint,
				previewRefreshDue: previewRefreshDue
			)
			DispatchQueue.main.async { [weak self = self] in
				self?.finishNativeScrollCaptureObservations(
					batch,
					captureID: captureID,
					sampleSequence: sampleSequence,
					observedWheelCount: observedWheelCount,
					motionRowsHint: motionRowsHint
				)
			}
		}
	}

	private func finishNativeScrollCaptureObservations(
		_ batch: NativeScrollCaptureObservationBatch,
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
			startNativeScrollCaptureObservationsIfNeeded(captureID: captureID)
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
		_ preview: NativeScrollCapturePreviewUpdate
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
	func scrollCaptureSelectionHasSufficientHeight(_ selection: CGRect) -> Bool {
		scrollCaptureSelectionHeightPixels(selection)
			>= Self.scrollCaptureMinimumSelectionHeightPixels
	}

	func scrollCaptureSelectionHeightPixels(_ selection: CGRect) -> Int {
		if chromeState.frozenSelectionSnapshot == selection,
			let frozenBaseImage = chromeState.frozenBaseImage
		{
			return frozenBaseImage.height
		}
		let point = CGPoint(x: selection.midX, y: selection.midY)
		let scale =
			screen(containing: point)?.backingScaleFactor
			?? NSScreen.main?.backingScaleFactor
			?? 1
		return Int((selection.height * scale).rounded())
	}
}
