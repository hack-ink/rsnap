import CoreGraphics
import Foundation

@MainActor
final class LivePointerPreviewState {
	private(set) var globalPoint: CGPoint?
	private(set) var inputUptime: TimeInterval?
	private(set) var inputSequence: UInt64 = 0

	func currentPoint(fallback: CGPoint?) -> CGPoint? {
		globalPoint ?? fallback
	}

	func seed(
		_ point: CGPoint,
		recordsInputLatency: Bool = true
	) {
		globalPoint = point
		if recordsInputLatency {
			inputUptime = ProcessInfo.processInfo.systemUptime
			inputSequence &+= 1
		} else {
			inputUptime = nil
			inputSequence = 0
		}
	}

	@discardableResult
	func set(
		to point: CGPoint,
		recordsInputLatency: Bool = true
	) -> Bool {
		if let globalPoint, hypot(globalPoint.x - point.x, globalPoint.y - point.y) < 0.05 {
			return false
		}
		seed(point, recordsInputLatency: recordsInputLatency)
		return true
	}

	func reset() {
		globalPoint = nil
		inputUptime = nil
		inputSequence = 0
	}
}
