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

private struct NativeScrollCaptureSampleFrame: Sendable {
	let region: RGBARegionSnapshot
	let source: String
	let frameSequence: UInt64
	let frameAgeMicroseconds: UInt64
	let prefersPairwiseRegistration: Bool
}

private struct NativeScrollCaptureFallbackRequest: Sendable {
	let rect: CGRect
	let source: CaptureSessionController.FrozenCaptureJobSource
	let frameSequence: UInt64
}

private struct NativeScrollCaptureObservation: Sendable {
	let sampledFrame: NativeScrollCaptureSampleFrame
	let registrationStrategy: String
	let result: ScrollObserveResult?
	let errorDescription: String?
}

private let nativeScrollCaptureMinimumHintRowsForHintedRegistration = 1

private struct NativeScrollCapturePreviewUpdate: @unchecked Sendable {
	let image: CGImage
	let exportWidth: Int
	let exportHeight: Int
	let result: ScrollObserveResult
	let viewportHeightPixels: Int
}

private struct NativeScrollCaptureObservationBatch: Sendable {
	let observations: [NativeScrollCaptureObservation]
	let preview: NativeScrollCapturePreviewUpdate?
	let previewErrorDescription: String?
	let previewExportMilliseconds: Double?
}

private func writeNativeScrollCaptureDebugDump(_ snapshot: RGBARegionSnapshot, name: String) {
	guard
		let rawDirectory = ProcessInfo.processInfo.environment["RSNAP_SCROLL_CAPTURE_DUMP_DIR"],
		rawDirectory.isEmpty == false,
		let pngData = try? RsnapExportEncoder.pngData(from: snapshot)
	else {
		return
	}
	let directory = URL(fileURLWithPath: rawDirectory, isDirectory: true)
	try? FileManager.default.createDirectory(
		at: directory,
		withIntermediateDirectories: true
	)
	let safeName = name.replacingOccurrences(of: "/", with: "_")
	try? pngData.write(to: directory.appendingPathComponent("\(safeName).png"))
}

extension CaptureSessionController {
	private static let scrollCaptureForwardedEventMarker: Int64 = 0x5253_4E41_5053_4352
	private static let scrollCapturePreciseWheelDeltaLimit = 120.0
	private static let scrollCaptureLineWheelDeltaLimit = 12.0

	var scrollCaptureToolbarEnabled: Bool {
		Self.scrollCaptureEnabled
			&& scene.mode == .frozen
			&& scrollCaptureState == nil
			&& chromeState.frozenSelectionEditable
			&& currentFrozenSelection() != nil
	}

	func handleScrollCaptureWheel(_ event: NSEvent, at point: CGPoint) -> Bool {
		guard Self.scrollCaptureEnabled else {
			return false
		}
		guard let state = scrollCaptureState else {
			return false
		}
		guard
			let viewportPoint = scrollCaptureViewportPoint(
				for: point,
				in: state.viewportRect
			)
		else {
			return false
		}

		let forwardedByRsnap = Self.scrollCaptureEventWasForwardedByRsnap(event)
		if forwardedByRsnap {
			if nativeScrollCaptureShouldLogWheelTelemetry(
				\.lastWheelInterceptTelemetryUptime
			) {
				NativeHostTelemetry.captureEvent(
					"capture.scroll_wheel_intercepted",
					captureID: currentCaptureTelemetryID,
					outcome: "forwarded_echo",
					detail:
						"source=overlay,deltaX=\(Int(event.scrollingDeltaX.rounded())),deltaY=\(Int(event.scrollingDeltaY.rounded())),x=\(Int(point.x.rounded())),y=\(Int(point.y.rounded()))"
				)
			}
			scheduleNativeScrollCaptureSample()
			return true
		}

		let targetPoint = CGPoint(
			x: viewportPoint.x.clamped(to: state.viewportRect.minX...state.viewportRect.maxX),
			y: viewportPoint.y.clamped(to: state.viewportRect.minY...state.viewportRect.maxY)
		)
		guard nativeScrollCaptureAcceptsManualInput(state: state) else {
			return true
		}
		if nativeScrollCaptureShouldLogWheelTelemetry(
			\.lastWheelInterceptTelemetryUptime
		) {
			NativeHostTelemetry.captureEvent(
				"capture.scroll_wheel_intercepted",
				captureID: currentCaptureTelemetryID,
				detail:
					"source=overlay,deltaX=\(Int(event.scrollingDeltaX.rounded())),deltaY=\(Int(event.scrollingDeltaY.rounded())),x=\(Int(point.x.rounded())),y=\(Int(point.y.rounded())),viewportX=\(Int(viewportPoint.x.rounded())),viewportY=\(Int(viewportPoint.y.rounded()))"
			)
		}
		let posted = forwardNativeScrollCaptureWheel(
			event,
			at: Self.scrollCapturePostPoint(for: event, fallbackAppKitPoint: targetPoint)
		)

		guard posted else {
			try? setHostStatusMessage("Could not forward scroll input.")
			refreshOverlay()
			return true
		}

		scheduleNativeScrollCaptureSample()

		return true
	}

	func installNativeScrollCaptureMonitor() {
		removeNativeScrollCaptureMonitor()
		NativeHostTelemetry.captureEvent(
			"capture.scroll_input_tap",
			captureID: currentCaptureTelemetryID,
			outcome: "not_used",
			detail: "input=selection_passthrough_global_monitor,accessibility_required=false"
		)
		scrollCaptureGlobalMonitor = NSEvent.addGlobalMonitorForEvents(matching: .scrollWheel) {
			[weak self] event in
			DispatchQueue.main.async { [weak self] in
				self?.recordNativeScrollWheelObserved(event, source: "global_monitor")
			}
		}
	}

	func removeNativeScrollCaptureMonitor() {
		if let monitor = scrollCaptureGlobalMonitor {
			NSEvent.removeMonitor(monitor)
			scrollCaptureGlobalMonitor = nil
		}
		overlayController?.setScrollCaptureMousePassthroughActive(false)
	}

	func recordNativeScrollWheelObserved(_ event: NSEvent, source: String) {
		guard var state = scrollCaptureState else {
			return
		}
		let point = NSEvent.mouseLocation
		guard
			let viewportPoint = scrollCaptureViewportPoint(
				for: point,
				in: state.viewportRect
			)
		else {
			if nativeScrollCaptureShouldLogWheelTelemetry(
				\.lastWheelObservedTelemetryUptime
			) {
				NativeHostTelemetry.captureEvent(
					"capture.scroll_wheel_observed",
					captureID: currentCaptureTelemetryID,
					outcome: "outside_viewport",
					detail:
						"source=\(source),deltaX=\(Int(event.scrollingDeltaX.rounded())),deltaY=\(Int(event.scrollingDeltaY.rounded())),x=\(Int(point.x.rounded())),y=\(Int(point.y.rounded())),viewportX=\(Int(state.viewportRect.minX.rounded())),viewportY=\(Int(state.viewportRect.minY.rounded())),viewportWidth=\(Int(state.viewportRect.width.rounded())),viewportHeight=\(Int(state.viewportRect.height.rounded()))"
				)
			}
			return
		}
		guard nativeScrollCaptureAcceptsManualInput(state: state) else {
			if nativeScrollCaptureShouldLogWheelTelemetry(
				\.lastWheelObservedTelemetryUptime
			) {
				NativeHostTelemetry.captureEvent(
					"capture.scroll_wheel_observed",
					captureID: currentCaptureTelemetryID,
					outcome: "manual_input_ignored",
					detail:
						"source=\(source),deltaX=\(Int(event.scrollingDeltaX.rounded())),deltaY=\(Int(event.scrollingDeltaY.rounded())),x=\(Int(point.x.rounded())),y=\(Int(point.y.rounded())),viewportX=\(Int(viewportPoint.x.rounded())),viewportY=\(Int(viewportPoint.y.rounded()))"
				)
			}
			return
		}
		state.observedWheelCount &+= 1
		scrollCaptureState = state
		let forwardedByRsnap = Self.scrollCaptureEventWasForwardedByRsnap(event)
		if forwardedByRsnap == false {
			recordNativeScrollCaptureMotionHint(
				deltaY: abs(Double(event.scrollingDeltaY)),
				multiplier: Self.scrollCapturePassthroughWheelMotionHintMultiplier
			)
		}
		if nativeScrollCaptureShouldLogWheelTelemetry(
			\.lastWheelObservedTelemetryUptime
		) {
			NativeHostTelemetry.captureEvent(
				"capture.scroll_wheel_observed",
				captureID: currentCaptureTelemetryID,
				detail:
					"source=\(source),count=\(state.observedWheelCount),deltaX=\(Int(event.scrollingDeltaX.rounded())),deltaY=\(Int(event.scrollingDeltaY.rounded())),x=\(Int(point.x.rounded())),y=\(Int(point.y.rounded())),viewportX=\(Int(viewportPoint.x.rounded())),viewportY=\(Int(viewportPoint.y.rounded())),forwardedByRsnap=\(forwardedByRsnap)"
			)
		}
		overlayController?.refreshScrollCaptureToolbarBackdropNow()
		scheduleNativeScrollCaptureSample()
	}

	func forwardNativeScrollCaptureWheel(_ event: NSEvent, at targetPoint: CGPoint) -> Bool {
		let rawDeltaX = Double(event.scrollingDeltaX)
		let rawDeltaY = Double(event.scrollingDeltaY)
		let maxMagnitude = max(abs(rawDeltaX), abs(rawDeltaY))
		guard maxMagnitude > 0 else {
			return false
		}
		guard rawDeltaY != 0 else {
			NativeHostTelemetry.captureEvent(
				"capture.scroll_wheel_forwarded",
				captureID: currentCaptureTelemetryID,
				outcome: "horizontal_ignored",
				detail:
					"rawDeltaX=\(Int(rawDeltaX.rounded())),rawDeltaY=\(Int(rawDeltaY.rounded()))"
			)
			return true
		}

		guard var state = scrollCaptureState else {
			return false
		}
		state.lastForwardedWheelUptime = ProcessInfo.processInfo.systemUptime
		scrollCaptureState = state

		let precise = event.hasPreciseScrollingDeltas
		let forwardedDeltaY = Self.forwardedScrollDelta(rawDeltaY, precise: precise)
		let postedDeltaY = -forwardedDeltaY
		if nativeScrollCaptureShouldLogWheelTelemetry(
			\.lastWheelForwardedTelemetryUptime
		) {
			NativeHostTelemetry.captureEvent(
				"capture.scroll_wheel_forwarded",
				captureID: currentCaptureTelemetryID,
				detail:
					"rawDeltaX=\(Int(rawDeltaX.rounded())),rawDeltaY=\(Int(rawDeltaY.rounded())),forwardedDeltaY=\(Int(forwardedDeltaY.rounded())),postedDeltaY=\(Int(postedDeltaY.rounded())),precise=\(precise)"
			)
		}
		let postWheelEvent = {
			Self.postScrollWheelEvent(
				deltaX: 0,
				deltaY: postedDeltaY,
				precise: precise,
				at: targetPoint
			)
		}
		let posted =
			overlayController?.withAllMousePassthrough(
				duration: Self.scrollCaptureForwardingPassthrough
			) {
				DispatchQueue.main.async {
					_ = postWheelEvent()
				}
				return true
			}
			?? postWheelEvent()
		if posted {
			recordNativeScrollCaptureMotionHint(deltaY: abs(postedDeltaY))
			scheduleNativeScrollCaptureSample()
		}
		return posted
	}

	func recordNativeScrollCaptureMotionHint(deltaY: Double, multiplier: Double = 1) {
		guard deltaY > 0, var state = scrollCaptureState else {
			return
		}
		let viewportHeightRows =
			state.viewportRect.height * CGFloat(state.viewportPixelsPerPointY)
		let maxHintRows = max(Double(viewportHeightRows) * 0.85, 1)
		let hintRows = deltaY * state.viewportPixelsPerPointY * max(multiplier, 0)
		state.pendingDownwardMotionHintRows = min(
			state.pendingDownwardMotionHintRows + hintRows,
			maxHintRows
		)
		scrollCaptureState = state
	}

	func scheduleNativeScrollCaptureSample(
		extendingWindowBy window: TimeInterval =
			CaptureSessionController.scrollCaptureInputSampleWindow
	) {
		guard var state = scrollCaptureState else {
			return
		}

		let now = ProcessInfo.processInfo.systemUptime
		state.sampleUntilUptime = max(state.sampleUntilUptime, now + max(window, 0))
		guard state.sampleLoopScheduled == false, state.sampleProcessing == false else {
			scrollCaptureState = state
			scheduleNativeScrollCaptureToolbarBackdropRefresh()
			return
		}

		state.sampleLoopScheduled = true
		scrollCaptureState = state
		scheduleNativeScrollCaptureToolbarBackdropRefresh()
		DispatchQueue.main.asyncAfter(deadline: .now() + Self.scrollCaptureSampleInterval) {
			[weak self] in
			self?.observeNativeScrollCaptureFrame()
		}
	}

	func scheduleNativeScrollCaptureToolbarBackdropRefresh() {
		guard var state = scrollCaptureState, state.toolbarBackdropLoopScheduled == false else {
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
		overlayController?.refreshScrollCaptureToolbarBackdropNow()
		guard let latestState = scrollCaptureState,
			nativeScrollCaptureShouldKeepSampling(state: latestState)
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
		scheduleNativeScrollCaptureSample()
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

		guard
			let captureSource = overlayController?.scrollCaptureFallbackSource(
				near: CGPoint(x: selection.midX, y: selection.midY)
			)
		else {
			try setHostStatusMessage("Scroll capture could not locate the overlay window.")
			refreshOverlay()
			return
		}

		ensureFrozenBaseImageFromDisplayIfNeeded(for: selection)
		let frozenBaseImage = chromeState.frozenBaseImage ?? frozenBaseImageFromDisplay(for: selection)
		guard let frozenBaseImage,
			let frozenBaseSnapshot = NativeHostImageBridge.rgbaSnapshot(from: frozenBaseImage)
		else {
			try setHostStatusMessage("Scroll capture could not read the selected region.")
			refreshOverlay()
			return
		}
		let liveBaseImage = CaptureOverlayController.captureImageBelowOverlay(
			in: selection,
			source: captureSource
		)
		let liveBaseSnapshot = liveBaseImage.flatMap {
			NativeHostImageBridge.rgbaSnapshot(from: $0)
		}
		let liveBaseMatchesFrozenSize =
			liveBaseSnapshot?.width == frozenBaseSnapshot.width
			&& liveBaseSnapshot?.height == frozenBaseSnapshot.height
		let baseImage: CGImage
		let baseSnapshot: RGBARegionSnapshot
		let baseSource: String
		if let liveBaseImage, let liveBaseSnapshot, liveBaseMatchesFrozenSize {
			baseImage = liveBaseImage
			baseSnapshot = liveBaseSnapshot
			baseSource = "below_overlay_capture_region"
		} else {
			baseImage = frozenBaseImage
			baseSnapshot = frozenBaseSnapshot
			baseSource = "frozen_display_region"
		}
		debugDumpNativeScrollCaptureSnapshot(baseSnapshot, name: "base-\(baseSource)")
		let stitcher = try RsnapScrollCaptureSession(
			baseImage: baseSnapshot,
			previewWidthPixels: baseSnapshot.width
		)
		var initialState = NativeScrollCaptureState(
			stitcher: stitcher,
			viewportRect: selection,
			captureSource: captureSource,
			viewportPixelsPerPointY: Double(baseSnapshot.height) / max(Double(selection.height), 1)
		)
		initialState.sampleUntilUptime =
			ProcessInfo.processInfo.systemUptime + Self.scrollCaptureInitialSampleWindow
		scrollCaptureState = initialState
		installNativeScrollCaptureMonitor()
		overlayController?.prepareCaptureStreamsNow(trigger: "scroll_capture_start")
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
		try setHostStatusMessage("Scroll capture started. Scroll inside the selection.")
		NativeHostTelemetry.captureEvent(
			"capture.scroll_capture_started",
			captureID: currentCaptureTelemetryID,
			detail:
				"width=\(baseSnapshot.width),height=\(baseSnapshot.height),x=\(Int(selection.minX.rounded())),y=\(Int(selection.minY.rounded())),mode=manual_universal,baseSource=\(baseSource),liveBaseMatched=\(liveBaseMatchesFrozenSize)"
		)
		NativeHostTelemetry.captureEvent(
			"capture.scroll_capture_mode",
			captureID: currentCaptureTelemetryID,
			outcome: "manual_universal",
			detail:
				"input=selection_passthrough_global_monitor,permission=screen_recording,accessibility_required=false"
		)
		refreshOverlay()
		overlayController?.focusWindow(at: CGPoint(x: selection.midX, y: selection.midY))
		overlayController?.setScrollCaptureMousePassthroughActive(true)
		NativeHostTelemetry.captureEvent(
			"capture.scroll_input_ready",
			captureID: currentCaptureTelemetryID,
			detail:
				"input=selection_passthrough_global_monitor,overlay=focused,passthrough=window"
		)
		scheduleNativeScrollCaptureSample(
			extendingWindowBy: Self.scrollCaptureInitialSampleWindow
		)
		scheduleNativeScrollCaptureToolbarBackdropRefresh()
	}

	func observeNativeScrollCaptureFrame() {
		guard var state = scrollCaptureState else {
			return
		}
		guard state.sampleProcessing == false else {
			state.sampleLoopScheduled = false
			scrollCaptureState = state
			scheduleNativeScrollCaptureSampleIfNeeded()
			return
		}
		state.sampleLoopScheduled = false
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
		let stitcher = state.stitcher
		let captureID = currentCaptureTelemetryID
		scrollCaptureState = state
		overlayController?.refreshScrollCaptureToolbarBackdropNow()

		let sampledFrames = nativeScrollCaptureSampleFrames(
			in: state.viewportRect,
			afterFrameSequence: state.lastStreamFrameSequence
		)
		let fallbackRequest =
			sampledFrames.isEmpty
				&& nativeScrollCaptureFallbackReadyForInput(state: state)
				&& nativeScrollCaptureFallbackAllowed(at: sampleUptime)
			? NativeScrollCaptureFallbackRequest(
				rect: state.viewportRect,
				source: state.captureSource,
				frameSequence: state.lastStreamFrameSequence &+ 1
			) : nil
		guard sampledFrames.isEmpty == false || fallbackRequest != nil else {
			if var latestState = scrollCaptureState,
				latestState.sampleSequence == sampleSequence
			{
				latestState.sampleProcessing = false
				scrollCaptureState = latestState
			}
			recordNativeScrollCaptureMissingSample(state: state, sampleSequence: sampleSequence)
			scheduleNativeScrollCaptureSampleIfNeeded()
			return
		}

		for sampledFrame in sampledFrames {
			debugDumpNativeScrollCaptureSnapshot(
				sampledFrame.region,
				name: "sample-\(sampleSequence)-\(sampledFrame.frameSequence)"
			)
		}

		enqueueNativeScrollCaptureObservations(
			sampledFrames: sampledFrames,
			fallbackRequest: fallbackRequest,
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
		fallbackRequest: NativeScrollCaptureFallbackRequest?,
		stitcher: RsnapScrollCaptureSession,
		motionRowsHint: Int?,
		previewRefreshDue: Bool,
		captureID: UInt64,
		sampleSequence: UInt64,
		observedWheelCount: UInt64
	) {
		scrollCaptureStitchQueue.async {
			[sampledFrames, fallbackRequest, stitcher, motionRowsHint, previewRefreshDue] in
			var sampledFrames = sampledFrames
			if sampledFrames.isEmpty,
				let fallbackRequest,
				let image = CaptureOverlayController.captureImageBelowOverlay(
					in: fallbackRequest.rect,
					source: fallbackRequest.source
				),
				let snapshot = NativeHostImageBridge.rgbaSnapshot(from: image)
			{
				writeNativeScrollCaptureDebugDump(
					snapshot,
					name: "fallback-\(fallbackRequest.frameSequence)"
				)
				sampledFrames.append(
					NativeScrollCaptureSampleFrame(
						region: snapshot,
						source: "below_overlay_capture_region",
						frameSequence: fallbackRequest.frameSequence,
						frameAgeMicroseconds: 0,
						prefersPairwiseRegistration: true
					))
			}
			let batch = Self.nativeScrollCaptureObservationBatch(
				sampledFrames: sampledFrames,
				stitcher: stitcher,
				motionRowsHint: motionRowsHint,
				previewRefreshDue: previewRefreshDue
			)
			DispatchQueue.main.async { [weak self] in
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

	nonisolated private static func nativeScrollCaptureObservationBatch(
		sampledFrames: [NativeScrollCaptureSampleFrame],
		stitcher: RsnapScrollCaptureSession,
		motionRowsHint: Int?,
		previewRefreshDue: Bool
	) -> NativeScrollCaptureObservationBatch {
		var observations: [NativeScrollCaptureObservation] = []
		var latestPreviewCandidate: NativeScrollCaptureObservation?
		for sampledFrame in sampledFrames {
			let observation = nativeScrollCaptureObservation(
				sampledFrame,
				stitcher: stitcher,
				motionRowsHint: motionRowsHint
			)
			if observation.result?.outcome != .noChange {
				latestPreviewCandidate = observation
			}
			observations.append(observation)
		}
		let preview = nativeScrollCapturePreviewUpdate(
			stitcher: stitcher,
			candidate: latestPreviewCandidate,
			previewRefreshDue: previewRefreshDue
		)
		return NativeScrollCaptureObservationBatch(
			observations: observations,
			preview: preview.update,
			previewErrorDescription: preview.errorDescription,
			previewExportMilliseconds: preview.exportMilliseconds
		)
	}

	nonisolated private static func nativeScrollCaptureObservation(
		_ sampledFrame: NativeScrollCaptureSampleFrame,
		stitcher: RsnapScrollCaptureSession,
		motionRowsHint: Int?
	) -> NativeScrollCaptureObservation {
		let usesHintedRegistration = nativeScrollCaptureUsesHintedRegistration(
			for: sampledFrame,
			motionRowsHint: motionRowsHint
		)
		let registrationStrategy = usesHintedRegistration ? "hinted_motion" : "pairwise"
		do {
			let result =
				if usesHintedRegistration, let motionRowsHint {
					try stitcher.observeDownwardFrame(
						sampledFrame.region,
						motionRowsHint: motionRowsHint,
						allowBurstSearch: true
					)
				} else {
					try stitcher.observeDownwardFrame(sampledFrame.region)
				}
			return NativeScrollCaptureObservation(
				sampledFrame: sampledFrame,
				registrationStrategy: registrationStrategy,
				result: result,
				errorDescription: nil
			)
		} catch {
			return NativeScrollCaptureObservation(
				sampledFrame: sampledFrame,
				registrationStrategy: registrationStrategy,
				result: nil,
				errorDescription: String(describing: error)
			)
		}
	}

	nonisolated private static func nativeScrollCaptureUsesHintedRegistration(
		for sampledFrame: NativeScrollCaptureSampleFrame,
		motionRowsHint: Int?
	) -> Bool {
		guard sampledFrame.prefersPairwiseRegistration == false,
			let motionRowsHint,
			motionRowsHint >= nativeScrollCaptureMinimumHintRowsForHintedRegistration
		else {
			return false
		}
		return true
	}

	nonisolated private static func nativeScrollCapturePreviewUpdate(
		stitcher: RsnapScrollCaptureSession,
		candidate: NativeScrollCaptureObservation?,
		previewRefreshDue: Bool
	) -> (
		update: NativeScrollCapturePreviewUpdate?,
		errorDescription: String?,
		exportMilliseconds: Double?
	) {
		guard previewRefreshDue, let candidate, let result = candidate.result else {
			return (nil, nil, nil)
		}
		let previewStartedAt = ProcessInfo.processInfo.systemUptime
		do {
			if let export = try stitcher.exportImage(),
				let exportImage = NativeHostImageBridge.cgImage(from: export)
			{
				return (
					NativeScrollCapturePreviewUpdate(
						image: exportImage,
						exportWidth: export.width,
						exportHeight: export.height,
						result: result,
						viewportHeightPixels: candidate.sampledFrame.region.height
					),
					nil,
					NativeHostTelemetry.milliseconds(since: previewStartedAt)
				)
			}
			return (
				nil,
				"scroll preview export returned no image",
				NativeHostTelemetry.milliseconds(since: previewStartedAt)
			)
		} catch {
			return (
				nil,
				String(describing: error),
				NativeHostTelemetry.milliseconds(since: previewStartedAt)
			)
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
			scheduleNativeScrollCaptureSampleIfNeeded()
		}
		guard batch.observations.isEmpty == false else {
			recordNativeScrollCaptureMissingSample(state: state, sampleSequence: sampleSequence)
			return
		}

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
	}

	func debugDumpNativeScrollCaptureSnapshot(_ snapshot: RGBARegionSnapshot, name: String) {
		writeNativeScrollCaptureDebugDump(snapshot, name: name)
	}

	func nativeScrollCaptureAcceptsManualInput(state _: NativeScrollCaptureState) -> Bool {
		true
	}

	func nativeScrollCaptureShouldKeepSampling(state: NativeScrollCaptureState) -> Bool {
		ProcessInfo.processInfo.systemUptime < state.sampleUntilUptime
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

	private func nativeScrollCaptureSampleFrames(
		in rect: CGRect,
		afterFrameSequence: UInt64
	) -> [NativeScrollCaptureSampleFrame] {
		var frames: [NativeScrollCaptureSampleFrame] = []
		var nextAfterFrameSequence = afterFrameSequence

		for _ in 0..<Self.scrollCaptureMaxFramesPerSample {
			guard
				let frame = overlayController?.nextRegionFrame(
					in: rect,
					afterFrameSequence: nextAfterFrameSequence,
					waitForFresh: false
				)
			else {
				break
			}
			frames.append(
				NativeScrollCaptureSampleFrame(
					region: frame.region,
					source: "ordered_live_stream_region",
					frameSequence: frame.frameSequence,
					frameAgeMicroseconds: frame.frameAgeMicroseconds,
					prefersPairwiseRegistration: false
				))
			nextAfterFrameSequence = frame.frameSequence
		}
		return frames
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

	private func nativeScrollCaptureShouldLogWheelTelemetry(
		_ keyPath: WritableKeyPath<NativeScrollCaptureState, TimeInterval>
	) -> Bool {
		guard var state = scrollCaptureState else {
			return false
		}
		let uptime = ProcessInfo.processInfo.systemUptime
		guard
			state[keyPath: keyPath] == 0
				|| uptime - state[keyPath: keyPath]
					>= Self.scrollCaptureWheelTelemetryInterval
		else {
			return false
		}
		state[keyPath: keyPath] = uptime
		scrollCaptureState = state
		return true
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
			viewportTopYPixels: CGFloat(preview.result.currentViewportTopY),
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
	static func postScrollWheelEvent(
		deltaX rawDeltaX: Double,
		deltaY rawDeltaY: Double,
		precise: Bool,
		at point: CGPoint
	) -> Bool {
		let deltaX = Int32(rawDeltaX.rounded())
		let deltaY = Int32(rawDeltaY.rounded())
		guard deltaX != 0 || deltaY != 0 else {
			return false
		}

		let units: CGScrollEventUnit = precise ? .pixel : .line
		let wheelCount: UInt32 = deltaX == 0 ? 1 : 2
		guard
			let source = CGEventSource(stateID: .hidSystemState),
			let scrollEvent = CGEvent(
				scrollWheelEvent2Source: source,
				units: units,
				wheelCount: wheelCount,
				wheel1: deltaY,
				wheel2: deltaX,
				wheel3: 0
			)
		else {
			return false
		}

		scrollEvent.setIntegerValueField(
			.eventSourceUserData,
			value: Self.scrollCaptureForwardedEventMarker
		)
		scrollEvent.location = point
		scrollEvent.post(tap: .cghidEventTap)
		return true
	}

	private static func forwardedScrollDelta(_ delta: Double, precise: Bool) -> Double {
		let limit = precise ? scrollCapturePreciseWheelDeltaLimit : scrollCaptureLineWheelDeltaLimit
		let clamped = delta.clamped(to: -limit...limit)
		let rounded = clamped.rounded()
		if abs(rounded) >= 1 {
			return rounded
		}
		if abs(delta) > 0 {
			return delta > 0 ? 1 : -1
		}
		return 0
	}

	private static func scrollCaptureEventWasForwardedByRsnap(_ event: NSEvent) -> Bool {
		event.cgEvent?.getIntegerValueField(.eventSourceUserData)
			== Self.scrollCaptureForwardedEventMarker
	}

	private static func scrollCapturePostPoint(
		for event: NSEvent,
		fallbackAppKitPoint: CGPoint
	) -> CGPoint {
		if let point = event.cgEvent?.location {
			return point
		}
		return scrollCaptureFlippedDesktopPoint(fallbackAppKitPoint)
	}

}

private func scrollCaptureFlippedDesktopPoint(_ point: CGPoint) -> CGPoint {
	let desktop = NSScreen.screens.reduce(CGRect.null) { partial, screen in
		partial.union(screen.frame)
	}
	guard desktop.isNull == false else {
		return point
	}
	return CGPoint(
		x: point.x,
		y: desktop.minY + desktop.maxY - point.y
	)
}

extension CGRect {
	fileprivate func inclusivelyContains(_ point: CGPoint) -> Bool {
		point.x >= minX && point.x <= maxX && point.y >= minY && point.y <= maxY
	}
}
