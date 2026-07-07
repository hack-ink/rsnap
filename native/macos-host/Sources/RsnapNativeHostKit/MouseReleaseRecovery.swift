import AppKit
import Foundation

@MainActor
final class MouseReleaseRecovery {
	typealias WatchdogPoll = () -> Bool

	private var liveMouseUpMonitor: Any?
	private var liveMouseReleaseWatchdog: DispatchWorkItem?
	private var frozenMouseReleaseWatchdog: DispatchWorkItem?

	var isPrimaryMouseButtonPressed: Bool {
		(NSEvent.pressedMouseButtons & 1) == 1
	}

	func installLiveMouseUpMonitor(onMouseUp: @escaping (NSEvent) -> Void) {
		removeLiveMouseUpMonitor()
		liveMouseUpMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseUp]) { event in
			onMouseUp(event)
			return event
		}
	}

	func removeLiveMouseUpMonitor() {
		cancelLiveMouseReleaseWatchdog()
		if let liveMouseUpMonitor {
			NSEvent.removeMonitor(liveMouseUpMonitor)
			self.liveMouseUpMonitor = nil
		}
	}

	func installLiveMouseReleaseWatchdog(onPoll: @escaping WatchdogPoll) {
		cancelLiveMouseReleaseWatchdog()
		scheduleLiveMouseReleaseWatchdog(onPoll: onPoll)
	}

	func cancelLiveMouseReleaseWatchdog() {
		liveMouseReleaseWatchdog?.cancel()
		liveMouseReleaseWatchdog = nil
	}

	func installFrozenMouseReleaseWatchdog(onPoll: @escaping WatchdogPoll) {
		cancelFrozenMouseReleaseWatchdog()
		scheduleFrozenMouseReleaseWatchdog(onPoll: onPoll)
	}

	func cancelFrozenMouseReleaseWatchdog() {
		frozenMouseReleaseWatchdog?.cancel()
		frozenMouseReleaseWatchdog = nil
	}

	private func scheduleLiveMouseReleaseWatchdog(onPoll: @escaping WatchdogPoll) {
		let workItem = DispatchWorkItem { [weak self] in
			self?.pollLiveMouseReleaseWatchdog(onPoll: onPoll)
		}
		liveMouseReleaseWatchdog = workItem
		DispatchQueue.main.asyncAfter(deadline: .now() + Self.watchdogInterval, execute: workItem)
	}

	private func pollLiveMouseReleaseWatchdog(onPoll: @escaping WatchdogPoll) {
		liveMouseReleaseWatchdog = nil
		if onPoll() {
			scheduleLiveMouseReleaseWatchdog(onPoll: onPoll)
		}
	}

	private func scheduleFrozenMouseReleaseWatchdog(onPoll: @escaping WatchdogPoll) {
		let workItem = DispatchWorkItem { [weak self] in
			self?.pollFrozenMouseReleaseWatchdog(onPoll: onPoll)
		}
		frozenMouseReleaseWatchdog = workItem
		DispatchQueue.main.asyncAfter(deadline: .now() + Self.watchdogInterval, execute: workItem)
	}

	private func pollFrozenMouseReleaseWatchdog(onPoll: @escaping WatchdogPoll) {
		frozenMouseReleaseWatchdog = nil
		if onPoll() {
			scheduleFrozenMouseReleaseWatchdog(onPoll: onPoll)
		}
	}

	private static var watchdogInterval: TimeInterval {
		NativeHostDisplayRefresh.frameInterval(
			forTargetFramesPerSecond: NativeHostDisplayRefresh.maximumTargetFramesPerSecond)
	}
}
