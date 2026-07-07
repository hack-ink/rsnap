import CoreGraphics
import Foundation

package enum CaptureHostPointerDispatchTrack: Equatable {
	case hover
	case drag
}

package enum CaptureHostPointerDispatchEvent: Equatable {
	case moved(CGPoint)
	case liveDragged(CGPoint)
	case frozenSelectionDragged(CGPoint)

	package var track: CaptureHostPointerDispatchTrack {
		switch self {
		case .moved:
			return .hover
		case .liveDragged, .frozenSelectionDragged:
			return .drag
		}
	}
}

package enum CaptureHostPointerDispatchTiming {
	package static func delay(
		now: TimeInterval,
		targetInterval: TimeInterval,
		lastDispatchUptime: TimeInterval
	) -> TimeInterval {
		max(0, targetInterval - (now - lastDispatchUptime))
	}
}

@MainActor
package final class CaptureHostPointerDispatchQueue {
	private final class TrackState {
		var queuedEvent: CaptureHostPointerDispatchEvent?
		var queuedWorkItem: DispatchWorkItem?
		var lastDispatchUptime: TimeInterval = 0
	}

	private let hoverState = TrackState()
	private let dragState = TrackState()
	private let targetInterval: () -> TimeInterval
	private let dispatchEvent: (CaptureHostPointerDispatchEvent) -> Void
	private let schedule: (TimeInterval, DispatchWorkItem) -> Void

	package init(
		targetInterval: @escaping () -> TimeInterval,
		dispatchEvent: @escaping (CaptureHostPointerDispatchEvent) -> Void,
		schedule: @escaping (TimeInterval, DispatchWorkItem) -> Void = { delay, workItem in
			if delay <= 0 {
				DispatchQueue.main.async(execute: workItem)
			} else {
				DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: workItem)
			}
		}
	) {
		self.targetInterval = targetInterval
		self.dispatchEvent = dispatchEvent
		self.schedule = schedule
	}

	package func enqueue(_ event: CaptureHostPointerDispatchEvent) {
		if event.track == .drag {
			cancel(hoverState)
		}
		let state = state(for: event.track)
		let now = ProcessInfo.processInfo.systemUptime
		let delay = CaptureHostPointerDispatchTiming.delay(
			now: now,
			targetInterval: targetInterval(),
			lastDispatchUptime: state.lastDispatchUptime
		)

		state.queuedEvent = event
		guard state.queuedWorkItem == nil else {
			return
		}

		var scheduledWorkItem: DispatchWorkItem?
		let workItem = DispatchWorkItem { [weak self, weak state] in
			guard let self else {
				return
			}
			guard let state else {
				return
			}
			guard state.queuedWorkItem === scheduledWorkItem else {
				return
			}
			state.queuedWorkItem = nil
			guard let event = state.queuedEvent else {
				return
			}
			state.queuedEvent = nil
			self.dispatchEvent(event)
			state.lastDispatchUptime = ProcessInfo.processInfo.systemUptime
		}
		scheduledWorkItem = workItem
		state.queuedWorkItem = workItem
		schedule(delay, workItem)
	}

	package func cancel() {
		cancel(hoverState)
		cancel(dragState)
	}

	private func state(for track: CaptureHostPointerDispatchTrack) -> TrackState {
		switch track {
		case .hover:
			return hoverState
		case .drag:
			return dragState
		}
	}

	private func cancel(_ state: TrackState) {
		state.queuedWorkItem?.cancel()
		state.queuedWorkItem = nil
		state.queuedEvent = nil
	}
}
