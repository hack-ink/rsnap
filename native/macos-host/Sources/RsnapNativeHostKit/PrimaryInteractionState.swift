import CoreGraphics

package struct PrimaryInteractionState: Equatable {
	package private(set) var dragStartGlobal: CGPoint?
	package private(set) var dragReleasedGlobal: CGPoint?
	package private(set) var dragExceededThreshold = false
	package private(set) var completionInFlight = false
	package private(set) var hoverChromeSuppressed = false

	package init() {}

	package var hasInteraction: Bool {
		dragStartGlobal != nil
	}

	package var canCompleteInteraction: Bool {
		hasInteraction && completionInFlight == false
	}

	package mutating func begin(at point: CGPoint) {
		dragStartGlobal = point
		dragReleasedGlobal = nil
		dragExceededThreshold = false
		completionInFlight = false
	}

	@discardableResult
	package mutating func suppressHoverChrome() -> Bool {
		guard hoverChromeSuppressed == false else {
			return false
		}
		hoverChromeSuppressed = true
		return true
	}

	package mutating func clearHoverChromeSuppression() {
		hoverChromeSuppressed = false
	}

	package func dragDistance(from point: CGPoint) -> CGFloat {
		guard let dragStartGlobal else {
			return 0
		}
		return max(abs(point.x - dragStartGlobal.x), abs(point.y - dragStartGlobal.y))
	}

	@discardableResult
	package mutating func updateDragThreshold(
		from point: CGPoint,
		threshold: CGFloat
	) -> Bool {
		guard dragExceededThreshold == false, dragDistance(from: point) >= threshold else {
			return false
		}
		dragExceededThreshold = true
		return true
	}

	package func completionPoint(for point: CGPoint) -> CGPoint {
		dragExceededThreshold ? point : dragStartGlobal ?? point
	}

	@discardableResult
	package mutating func markReleased(at point: CGPoint) -> CGPoint {
		let completionPoint = completionPoint(for: point)
		completionInFlight = true
		dragReleasedGlobal = completionPoint
		hoverChromeSuppressed = false
		return completionPoint
	}

	package func immediateDragSelectionGlobal(
		current: CGPoint?,
		in windowFrame: CGRect
	) -> CGRect? {
		guard let dragStartGlobal, dragExceededThreshold else {
			return nil
		}
		let current = dragReleasedGlobal ?? current ?? dragStartGlobal
		guard windowFrame.contains(dragStartGlobal) else {
			return nil
		}
		let normalized = windowFrame.normalizedRect(anchor: dragStartGlobal, current: current)
		guard max(normalized.width, normalized.height) >= 1 else {
			return nil
		}
		return CGRect(
			x: normalized.minX,
			y: normalized.minY,
			width: max(normalized.width, 1),
			height: max(normalized.height, 1)
		)
	}

	package mutating func reset() {
		dragStartGlobal = nil
		dragReleasedGlobal = nil
		dragExceededThreshold = false
		completionInFlight = false
		hoverChromeSuppressed = false
	}
}
