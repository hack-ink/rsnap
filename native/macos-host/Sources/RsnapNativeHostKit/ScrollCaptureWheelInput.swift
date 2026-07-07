import AppKit
@preconcurrency import CoreGraphics
import Foundation
import RsnapHostBridge

private let nativeScrollCaptureMinimumNonzeroWheelMotionHintRows = 12.0

extension CaptureSessionController {
	private static let scrollCaptureForwardedEventMarker: Int64 = 0x5253_4E41_5053_4352
	private static let scrollCapturePreciseWheelDeltaLimit = 72.0
	private static let scrollCapturePreciseWheelDeltaMinimum = 36.0
	private static let scrollCaptureLineWheelDeltaLimit = 12.0
	private static let scrollCaptureLineWheelDeltaMinimum = 1.0
	private static let scrollCaptureQueuedWheelDeltaLimitMultiplier = 32.0

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
