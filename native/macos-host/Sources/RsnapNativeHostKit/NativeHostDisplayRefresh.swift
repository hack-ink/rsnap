import AppKit
import Foundation

enum NativeHostDisplayRefresh {
	static let maximumTargetFramesPerSecond = 120
	static let fallbackFramesPerSecond = 60
	static let targetFramesPerSecond = maximumTargetFramesPerSecond
	static let timerWakeupLeadRatio = 0.97

	static var frameInterval: TimeInterval {
		frameInterval(forTargetFramesPerSecond: targetFramesPerSecond)
	}

	static var frameBudgetMilliseconds: Double {
		frameInterval * 1_000
	}

	static func targetFramesPerSecond(for screen: NSScreen?) -> Int {
		min(maximumTargetFramesPerSecond, screenMaximumFramesPerSecond(screen))
	}

	static func pointerFollowFramesPerSecond(for screen: NSScreen?) -> Int {
		let screenFramesPerSecond = screenMaximumFramesPerSecond(screen)
		return min(
			maximumTargetFramesPerSecond, max(screenFramesPerSecond * 2, screenFramesPerSecond))
	}

	static func frameInterval(for screen: NSScreen?) -> TimeInterval {
		frameInterval(forTargetFramesPerSecond: targetFramesPerSecond(for: screen))
	}

	static func frameBudgetMilliseconds(forTargetFramesPerSecond framesPerSecond: Int) -> Double {
		frameInterval(forTargetFramesPerSecond: framesPerSecond) * 1_000
	}

	static func frameInterval(forTargetFramesPerSecond framesPerSecond: Int) -> TimeInterval {
		1.0 / Double(max(1, framesPerSecond))
	}

	static func timerInterval(forTargetFramesPerSecond framesPerSecond: Int) -> TimeInterval {
		frameInterval(forTargetFramesPerSecond: framesPerSecond) * timerWakeupLeadRatio
	}

	private static func screenMaximumFramesPerSecond(_ screen: NSScreen?) -> Int {
		guard let framesPerSecond = screen?.maximumFramesPerSecond, framesPerSecond > 0 else {
			return fallbackFramesPerSecond
		}
		return framesPerSecond
	}
}
