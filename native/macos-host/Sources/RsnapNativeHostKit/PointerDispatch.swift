import CoreGraphics
import Foundation

package enum PointerDispatchTrack: Equatable {
	case hover
	case drag
}

package enum PointerDispatchEvent: Equatable {
	case moved(CGPoint)
	case liveDragged(CGPoint)
	case frozenSelectionDragged(CGPoint)

	package var track: PointerDispatchTrack {
		switch self {
		case .moved:
			return .hover
		case .liveDragged, .frozenSelectionDragged:
			return .drag
		}
	}
}

package enum PointerDispatchTiming {
	package static func delay(
		now: TimeInterval,
		targetInterval: TimeInterval,
		lastDispatchUptime: TimeInterval
	) -> TimeInterval {
		max(0, targetInterval - (now - lastDispatchUptime))
	}
}

@MainActor
package final class PointerDispatchQueue {
	private final class TrackState {
		var queuedEvent: PointerDispatchEvent?
		var queuedWorkItem: DispatchWorkItem?
		var lastDispatchUptime: TimeInterval = 0
	}

	private let hoverState = TrackState()
	private let dragState = TrackState()
	private let targetInterval: () -> TimeInterval
	private let dispatchEvent: (PointerDispatchEvent) -> Void
	private let schedule: (TimeInterval, DispatchWorkItem) -> Void

	package init(
		targetInterval: @escaping () -> TimeInterval,
		dispatchEvent: @escaping (PointerDispatchEvent) -> Void,
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

	package func enqueue(_ event: PointerDispatchEvent) {
		if event.track == .drag {
			cancel(hoverState)
		}
		let state = state(for: event.track)
		let now = ProcessInfo.processInfo.systemUptime
		let delay = PointerDispatchTiming.delay(
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

	private func state(for track: PointerDispatchTrack) -> TrackState {
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
