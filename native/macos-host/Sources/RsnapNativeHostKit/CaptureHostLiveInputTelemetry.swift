import Foundation

@MainActor
final class CaptureHostLiveInputTelemetry {
	private let pointerEventGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.pointer_event_gap",
		category: "LiveChromeTelemetry"
	)
	private var mouseEventCount = 0
	private var lastPointerEventUptime: TimeInterval?
	private var didEmitInputSummary = false

	func recordMouseEvent() {
		mouseEventCount += 1
	}

	func recordPointerEvent() {
		let now = ProcessInfo.processInfo.systemUptime
		if let lastPointerEventUptime {
			let gapMilliseconds = (now - lastPointerEventUptime) * 1_000
			if gapMilliseconds >= 0, gapMilliseconds < 250 {
				pointerEventGapMetric.record(gapMilliseconds)
			}
		}
		lastPointerEventUptime = now
	}

	func reset() {
		mouseEventCount = 0
		lastPointerEventUptime = nil
		didEmitInputSummary = false
	}

	func emitInputSummary(
		reason: String,
		captureID: UInt64,
		pointerInputSequence: UInt64
	) {
		guard didEmitInputSummary == false else {
			return
		}
		let observedMouseEvents = max(
			mouseEventCount,
			Int(min(pointerInputSequence, UInt64(Int.max)))
		)
		guard observedMouseEvents > 0 else {
			return
		}
		didEmitInputSummary = true
		NativeHostTelemetry.liveChromeInputSummary(
			captureID: captureID,
			reason: reason,
			mouseEvents: observedMouseEvents,
			followTicks: 0,
			fastMoveAttempts: 0,
			fastMoveSuccesses: 0,
			loupeFastMoveAttempts: 0,
			loupeFastMoveSuccesses: 0,
			predictedMoves: 0,
			fallbackRefreshes: 0,
			immediateRefreshes: 0
		)
	}
}
