import AppKit
import Foundation

enum NativeHostDisplayRefresh {
	static let preferredMaximumFramesPerSecond = 120
	static let fallbackFramesPerSecond = 60

	static func displayFramesPerSecond(for screen: NSScreen?) -> Int {
		max(
			1,
			screen?.maximumFramesPerSecond ?? NSScreen.main?.maximumFramesPerSecond
				?? fallbackFramesPerSecond)
	}

	static func effectiveFramesPerSecond(for screen: NSScreen?) -> Int {
		min(displayFramesPerSecond(for: screen), preferredMaximumFramesPerSecond)
	}

	static func frameInterval(for screen: NSScreen?) -> TimeInterval {
		1.0 / Double(effectiveFramesPerSecond(for: screen))
	}

	static func frameBudgetMilliseconds(for screen: NSScreen?) -> Double {
		frameInterval(for: screen) * 1_000
	}
}
