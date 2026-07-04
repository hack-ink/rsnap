import CoreGraphics
import Foundation

package enum CaptureHostPointerDispatchTrack: Equatable {
	case hover
	case drag
}

package enum CaptureHostPointerDispatchEvent: Equatable {
	case moved(CGPoint)
	case liveDragged(CGPoint)

	package var track: CaptureHostPointerDispatchTrack {
		switch self {
		case .moved:
			return .hover
		case .liveDragged:
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
final class CaptureHostPointerDispatchQueue {
	private var queuedEvent: CaptureHostPointerDispatchEvent?
	private var queuedWorkItem: DispatchWorkItem?
	private var lastHoverDispatchUptime: TimeInterval = 0
	private var lastDragDispatchUptime: TimeInterval = 0
	private let targetInterval: () -> TimeInterval
	private let dispatchEvent: (CaptureHostPointerDispatchEvent) -> Void

	init(
		targetInterval: @escaping () -> TimeInterval,
		dispatchEvent: @escaping (CaptureHostPointerDispatchEvent) -> Void
	) {
		self.targetInterval = targetInterval
		self.dispatchEvent = dispatchEvent
	}

	func enqueue(_ event: CaptureHostPointerDispatchEvent) {
		let now = ProcessInfo.processInfo.systemUptime
		let delay = CaptureHostPointerDispatchTiming.delay(
			now: now,
			targetInterval: targetInterval(),
			lastDispatchUptime: lastDispatchUptime(for: event)
		)

		queuedEvent = event
		guard queuedWorkItem == nil else {
			return
		}

		let workItem = DispatchWorkItem { [weak self] in
			guard let self else {
				return
			}
			self.queuedWorkItem = nil
			guard let event = self.queuedEvent else {
				return
			}
			self.queuedEvent = nil
			self.setLastDispatchUptime(ProcessInfo.processInfo.systemUptime, for: event)
			self.dispatchEvent(event)
		}
		queuedWorkItem = workItem
		if delay <= 0 {
			DispatchQueue.main.async(execute: workItem)
		} else {
			DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: workItem)
		}
	}

	func cancel() {
		queuedWorkItem?.cancel()
		queuedWorkItem = nil
		queuedEvent = nil
	}

	private func lastDispatchUptime(for event: CaptureHostPointerDispatchEvent) -> TimeInterval {
		switch event.track {
		case .hover:
			return lastHoverDispatchUptime
		case .drag:
			return lastDragDispatchUptime
		}
	}

	private func setLastDispatchUptime(
		_ uptime: TimeInterval,
		for event: CaptureHostPointerDispatchEvent
	) {
		switch event.track {
		case .hover:
			lastHoverDispatchUptime = uptime
		case .drag:
			lastDragDispatchUptime = uptime
		}
	}
}
