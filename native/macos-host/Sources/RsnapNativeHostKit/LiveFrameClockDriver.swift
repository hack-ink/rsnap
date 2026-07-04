import Foundation

final class LiveFrameClockDriver: @unchecked Sendable {
	var onTick: (() -> Void)?
	private let stateLock = NSLock()
	private let tickGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.frame_tick_gap",
		category: "LiveChromeTelemetry"
	)
	private var timer: DispatchSourceTimer?
	private var currentTargetFramesPerSecond: Int?
	private var lastTickUptime: TimeInterval?

	func start(targetFramesPerSecond: Int) {
		let sanitizedTarget = max(1, targetFramesPerSecond)
		stateLock.lock()
		let alreadyRunning = timer != nil && currentTargetFramesPerSecond == sanitizedTarget
		stateLock.unlock()
		guard alreadyRunning == false else {
			return
		}

		stop()
		let timer = DispatchSource.makeTimerSource(queue: .main)
		let intervalNanoseconds = max(
			1,
			Int(
				(NativeHostDisplayRefresh.timerInterval(
					forTargetFramesPerSecond: sanitizedTarget) * 1_000_000_000.0)
					.rounded())
		)
		timer.schedule(
			deadline: .now(),
			repeating: .nanoseconds(intervalNanoseconds),
			leeway: .nanoseconds(0)
		)
		timer.setEventHandler { [weak self] in
			self?.tick()
		}
		stateLock.lock()
		self.timer = timer
		currentTargetFramesPerSecond = sanitizedTarget
		lastTickUptime = nil
		stateLock.unlock()
		timer.resume()
	}

	private func tick() {
		let now = ProcessInfo.processInfo.systemUptime
		if let lastTickUptime {
			let gapMilliseconds = (now - lastTickUptime) * 1_000
			if gapMilliseconds >= 0, gapMilliseconds < 250 {
				tickGapMetric.record(gapMilliseconds)
			}
		}
		lastTickUptime = now
		onTick?()
	}

	func stop() {
		stateLock.lock()
		guard let timer else {
			currentTargetFramesPerSecond = nil
			lastTickUptime = nil
			stateLock.unlock()
			return
		}
		self.timer = nil
		currentTargetFramesPerSecond = nil
		lastTickUptime = nil
		stateLock.unlock()
		timer.cancel()
	}

	deinit {
		stop()
	}
}
