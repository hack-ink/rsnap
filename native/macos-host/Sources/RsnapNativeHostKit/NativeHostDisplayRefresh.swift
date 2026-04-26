import Foundation

enum NativeHostDisplayRefresh {
	static let targetFramesPerSecond = 120

	static var frameInterval: TimeInterval {
		1.0 / Double(targetFramesPerSecond)
	}

	static var followClockInterval: TimeInterval {
		frameInterval * 0.92
	}

	static var frameBudgetMilliseconds: Double {
		frameInterval * 1_000
	}
}
