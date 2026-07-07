import CoreGraphics
import Foundation

package struct AnnotationStyleWheelGate: Equatable {
	package static let deadZone: CGFloat = 0.05
	package static let preciseStepInterval: TimeInterval = 0.18
	package static let discreteStepInterval: TimeInterval = 0.04

	private var lastStepTimestamp: TimeInterval?

	package init() {}

	package mutating func steps(
		timestamp: TimeInterval,
		deltaY: CGFloat,
		hasPreciseScrollingDeltas: Bool,
		phaseActive: Bool,
		phaseEndedOrCancelled: Bool,
		momentumActive: Bool
	) -> Int {
		guard momentumActive == false else {
			return 0
		}
		if phaseEndedOrCancelled {
			reset()
			return 0
		}
		guard abs(deltaY) > .ulpOfOne else {
			return 0
		}
		guard abs(deltaY) >= Self.deadZone else {
			return 0
		}

		let minimumInterval =
			hasPreciseScrollingDeltas || phaseActive
			? Self.preciseStepInterval
			: Self.discreteStepInterval
		if let lastStepTimestamp, timestamp - lastStepTimestamp < minimumInterval {
			return 0
		}
		lastStepTimestamp = timestamp
		return deltaY > 0 ? 1 : -1
	}

	package mutating func reset() {
		lastStepTimestamp = nil
	}
}
