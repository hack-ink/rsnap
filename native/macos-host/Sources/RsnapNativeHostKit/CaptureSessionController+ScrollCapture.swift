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

private let nativeScrollCaptureMinimumNonzeroWheelMotionHintRows = 12.0

private struct NativeScrollCaptureGeometry {
	let baseImage: CGImage
	let baseSnapshot: RGBARegionSnapshot
	let pixelRect: CGRect
	let samplingRect: CGRect
}

extension CaptureSessionController {
	private static let scrollCaptureForwardedEventMarker: Int64 = 0x5253_4E41_5053_4352
	private static let scrollCapturePreciseWheelDeltaLimit = 72.0
	private static let scrollCapturePreciseWheelDeltaMinimum = 36.0
	private static let scrollCaptureLineWheelDeltaLimit = 12.0
	private static let scrollCaptureLineWheelDeltaMinimum = 1.0
	private static let scrollCaptureQueuedWheelDeltaLimitMultiplier = 32.0

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
		if state.controlledScrollInFlight {
			queueNativeScrollCaptureWheel(event, at: targetPoint)
			return true
		}
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
		let rawDeltaY = Double(event.scrollingDeltaY)
		guard rawDeltaY != 0 else {
			if nativeScrollCaptureShouldLogWheelTelemetry(
				\.lastWheelObservedTelemetryUptime
			) {
				NativeHostTelemetry.captureEvent(
					"capture.scroll_wheel_observed",
					captureID: currentCaptureTelemetryID,
					outcome: "zero_delta_ignored",
					detail:
						"source=\(source),deltaX=\(Int(event.scrollingDeltaX.rounded())),deltaY=0,x=\(Int(point.x.rounded())),y=\(Int(point.y.rounded()))"
				)
			}
			return
		}
		guard
			let inputPoint = scrollCaptureObservedInputPoint(
				for: point,
				viewportRect: state.viewportRect,
				sourceFrame: state.captureSource.referenceFrame,
				desktopFrame: state.captureSource.desktopFrame,
				padding: Self.scrollCaptureInputViewportPaddingPoints
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
						"source=\(source),deltaX=\(Int(event.scrollingDeltaX.rounded())),deltaY=\(Int(event.scrollingDeltaY.rounded())),x=\(Int(point.x.rounded())),y=\(Int(point.y.rounded())),viewportX=\(Int(state.viewportRect.minX.rounded())),viewportY=\(Int(state.viewportRect.minY.rounded())),viewportWidth=\(Int(state.viewportRect.width.rounded())),viewportHeight=\(Int(state.viewportRect.height.rounded())),inputPadding=\(Int(Self.scrollCaptureInputViewportPaddingPoints.rounded())),sourceX=\(Int(state.captureSource.referenceFrame.minX.rounded())),sourceY=\(Int(state.captureSource.referenceFrame.minY.rounded())),sourceWidth=\(Int(state.captureSource.referenceFrame.width.rounded())),sourceHeight=\(Int(state.captureSource.referenceFrame.height.rounded()))"
				)
			}
			return
		}
		let viewportPoint = inputPoint.viewportPoint
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
		state.lastObservedWheelUptime = ProcessInfo.processInfo.systemUptime
		scrollCaptureState = state
		let forwardedByRsnap = Self.scrollCaptureEventWasForwardedByRsnap(event)
		if forwardedByRsnap == false {
			recordNativeScrollCaptureMotionHint(
				deltaY: abs(rawDeltaY),
				multiplier: Self.scrollCapturePassthroughWheelMotionHintMultiplier
			)
		}
		if nativeScrollCaptureShouldLogWheelTelemetry(
			\.lastWheelObservedTelemetryUptime
		) {
			NativeHostTelemetry.captureEvent(
				"capture.scroll_wheel_observed",
				captureID: currentCaptureTelemetryID,
				outcome: inputPoint.insideViewport ? "success" : "accepted_outside_viewport",
				detail:
					"source=\(source),inputSource=\(inputPoint.inputSource),count=\(state.observedWheelCount),deltaX=\(Int(event.scrollingDeltaX.rounded())),deltaY=\(Int(event.scrollingDeltaY.rounded())),x=\(Int(point.x.rounded())),y=\(Int(point.y.rounded())),viewportX=\(Int(viewportPoint.x.rounded())),viewportY=\(Int(viewportPoint.y.rounded())),forwardedByRsnap=\(forwardedByRsnap)"
			)
		}
		overlayController?.refreshScrollCaptureToolbarBackdropNow()
		scheduleNativeScrollCaptureSample(
			delay: Self.scrollCaptureActiveInputSampleInterval
		)
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

		let precise = event.hasPreciseScrollingDeltas
		let totalForwardedDeltaY = Self.forwardedScrollQueuedDelta(rawDeltaY, precise: precise)
		let forwardedDeltaY = Self.scrollCaptureCommandDelta(
			totalForwardedDeltaY,
			precise: precise
		)
		let overflowDeltaY = totalForwardedDeltaY - forwardedDeltaY
		if overflowDeltaY != 0 {
			queueNativeScrollCaptureForwardedDelta(
				overflowDeltaY,
				precise: precise,
				at: targetPoint,
				source: "overflow"
			)
		}
		if nativeScrollCaptureShouldLogWheelTelemetry(
			\.lastWheelForwardedTelemetryUptime
		) {
			NativeHostTelemetry.captureEvent(
				"capture.scroll_wheel_forwarded",
				captureID: currentCaptureTelemetryID,
				detail:
					"rawDeltaX=\(Int(rawDeltaX.rounded())),rawDeltaY=\(Int(rawDeltaY.rounded())),totalForwardedDeltaY=\(Int(totalForwardedDeltaY.rounded())),forwardedDeltaY=\(Int(forwardedDeltaY.rounded())),queuedOverflowDeltaY=\(Int(overflowDeltaY.rounded())),postedDeltaY=\(Int((-forwardedDeltaY).rounded())),precise=\(precise)"
			)
		}
		return postNativeScrollCaptureForwardedDelta(
			forwardedDeltaY,
			precise: precise,
			at: targetPoint
		)
	}

	func postNativeScrollCaptureForwardedDelta(
		_ forwardedDeltaY: Double,
		precise: Bool,
		at targetPoint: CGPoint
	) -> Bool {
		let postedDeltaY = -forwardedDeltaY
		guard postedDeltaY != 0, var state = scrollCaptureState else {
			return false
		}

		state.lastForwardedWheelUptime = ProcessInfo.processInfo.systemUptime
		state.controlledScrollInFlight = true
		scrollCaptureState = state

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
				postWheelEvent()
			}
			?? postWheelEvent()
		if posted {
			recordNativeScrollCaptureMotionHint(deltaY: abs(postedDeltaY))
			scheduleNativeScrollCaptureSample(
				delay: Self.scrollCaptureControlledScrollSettleDelay
			)
		} else if var latestState = scrollCaptureState {
			latestState.controlledScrollInFlight = false
			scrollCaptureState = latestState
		}
		return posted
	}

	func queueNativeScrollCaptureWheel(_ event: NSEvent, at targetPoint: CGPoint) {
		let rawDeltaY = Double(event.scrollingDeltaY)
		guard rawDeltaY != 0 else {
			return
		}
		let precise = event.hasPreciseScrollingDeltas
		let forwardedDeltaY = Self.forwardedScrollQueuedDelta(rawDeltaY, precise: precise)
		queueNativeScrollCaptureForwardedDelta(
			forwardedDeltaY,
			precise: precise,
			at: targetPoint,
			source: "in_flight"
		)
	}

	func queueNativeScrollCaptureForwardedDelta(
		_ forwardedDeltaY: Double,
		precise: Bool,
		at targetPoint: CGPoint,
		source: String
	) {
		guard forwardedDeltaY != 0, var state = scrollCaptureState else {
			return
		}
		let limit = Self.scrollCaptureQueuedWheelDeltaLimit(precise: precise)

		state.queuedForwardedWheelDeltaY = (state.queuedForwardedWheelDeltaY + forwardedDeltaY)
			.clamped(to: -limit...limit)
		state.queuedForwardedWheelPrecise = precise
		state.queuedForwardedWheelTargetPoint = targetPoint
		scrollCaptureState = state
		if nativeScrollCaptureShouldLogWheelTelemetry(
			\.lastWheelInterceptTelemetryUptime
		) {
			NativeHostTelemetry.captureEvent(
				"capture.scroll_wheel_queued",
				captureID: currentCaptureTelemetryID,
				detail:
					"source=\(source),forwardedDeltaY=\(Int(forwardedDeltaY.rounded())),queuedDeltaY=\(Int(state.queuedForwardedWheelDeltaY.rounded())),precise=\(precise)"
			)
		}
	}

	func drainNativeScrollCaptureQueuedWheelIfNeeded() {
		guard var state = scrollCaptureState,
			state.controlledScrollInFlight == false,
			state.queuedForwardedWheelDeltaY != 0
		else {
			return
		}

		let precise = state.queuedForwardedWheelPrecise
		let limit = Self.scrollCaptureCommandWheelDeltaLimit(precise: precise)
		let forwardedDeltaY = state.queuedForwardedWheelDeltaY.clamped(to: -limit...limit)
		let targetPoint =
			state.queuedForwardedWheelTargetPoint
			?? CGPoint(x: state.viewportRect.midX, y: state.viewportRect.midY)

		state.queuedForwardedWheelDeltaY -= forwardedDeltaY
		if abs(state.queuedForwardedWheelDeltaY) < 1 {
			state.queuedForwardedWheelDeltaY = 0
			state.queuedForwardedWheelTargetPoint = nil
		}
		scrollCaptureState = state

		_ = postNativeScrollCaptureForwardedDelta(
			forwardedDeltaY,
			precise: precise,
			at: targetPoint
		)
	}

	func recordNativeScrollCaptureMotionHint(deltaY: Double, multiplier: Double = 1) {
		guard deltaY > 0, var state = scrollCaptureState else {
			return
		}
		let viewportHeightRows =
			state.viewportRect.height * CGFloat(state.viewportPixelsPerPointY)
		let maxHintRows = max(Double(viewportHeightRows) * 0.85, 1)
		let scaledHintRows = deltaY * state.viewportPixelsPerPointY * max(multiplier, 0)
		let hintRows =
			if scaledHintRows > 0 {
				max(scaledHintRows, nativeScrollCaptureMinimumNonzeroWheelMotionHintRows)
			} else {
				0.0
			}
		state.pendingDownwardMotionHintRows = min(
			state.pendingDownwardMotionHintRows + hintRows,
			maxHintRows
		)
		scrollCaptureState = state
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
			DispatchQueue.main.async { [weak self] in
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

	private static func scrollCaptureCommandWheelDeltaLimit(precise: Bool) -> Double {
		precise ? scrollCapturePreciseWheelDeltaLimit : scrollCaptureLineWheelDeltaLimit
	}

	private static func scrollCaptureQueuedWheelDeltaLimit(precise: Bool) -> Double {
		scrollCaptureCommandWheelDeltaLimit(precise: precise)
			* scrollCaptureQueuedWheelDeltaLimitMultiplier
	}

	private static func scrollCaptureCommandDelta(_ delta: Double, precise: Bool) -> Double {
		let commandDelta = forwardedScrollDelta(
			delta,
			limit: scrollCaptureCommandWheelDeltaLimit(precise: precise)
		)
		let minimum =
			precise ? scrollCapturePreciseWheelDeltaMinimum : scrollCaptureLineWheelDeltaMinimum
		guard commandDelta != 0, abs(commandDelta) < minimum else {
			return commandDelta
		}
		return commandDelta < 0 ? -minimum : minimum
	}

	private static func forwardedScrollQueuedDelta(_ delta: Double, precise: Bool) -> Double {
		forwardedScrollDelta(
			delta,
			limit: scrollCaptureQueuedWheelDeltaLimit(precise: precise)
		)
	}

	private static func forwardedScrollDelta(_ delta: Double, limit: Double) -> Double {
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
