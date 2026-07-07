import Foundation

package struct DisplayHandoffCompletion: Equatable {
	package let startedAt: TimeInterval?
	package let pendingFrameDisplayed: Bool
	package let deferredClassicToolbarGlass: Bool

	package init(
		startedAt: TimeInterval?,
		pendingFrameDisplayed: Bool,
		deferredClassicToolbarGlass: Bool
	) {
		self.startedAt = startedAt
		self.pendingFrameDisplayed = pendingFrameDisplayed
		self.deferredClassicToolbarGlass = deferredClassicToolbarGlass
	}
}

package struct DisplayHandoffState: Equatable {
	package private(set) var pending = false
	package private(set) var completionQueued = false
	package private(set) var startedAt: TimeInterval?
	package private(set) var pendingFrameDisplayed = false
	package private(set) var defersClassicToolbarGlassUntilAfterFirstDisplay = false

	package init() {}

	package var allowsClassicToolbarGlass: Bool {
		!defersClassicToolbarGlassUntilAfterFirstDisplay
	}

	package mutating func reset() {
		pending = false
		completionQueued = false
		startedAt = nil
		pendingFrameDisplayed = false
		defersClassicToolbarGlassUntilAfterFirstDisplay = false
	}

	package mutating func beginTransitionToFrozen(now: TimeInterval) {
		pending = true
		completionQueued = false
		startedAt = now
		pendingFrameDisplayed = false
		defersClassicToolbarGlassUntilAfterFirstDisplay = false
	}

	package mutating func beginFrozenFirstFrameInstall(
		pending: Bool,
		defersClassicToolbarGlass: Bool,
		now: TimeInterval
	) {
		self.pending = pending
		completionQueued = false
		startedAt = pending ? now : nil
		pendingFrameDisplayed = false
		defersClassicToolbarGlassUntilAfterFirstDisplay = defersClassicToolbarGlass
	}

	package mutating func markPendingFrameDisplayed() {
		pendingFrameDisplayed = true
	}

	package mutating func queueCompletionIfNeeded() -> Bool {
		guard pending, !completionQueued else {
			return false
		}
		completionQueued = true
		return true
	}

	package mutating func finish() -> DisplayHandoffCompletion? {
		guard pending else {
			return nil
		}
		let completion = DisplayHandoffCompletion(
			startedAt: startedAt,
			pendingFrameDisplayed: pendingFrameDisplayed,
			deferredClassicToolbarGlass: defersClassicToolbarGlassUntilAfterFirstDisplay
		)
		pending = false
		completionQueued = false
		startedAt = nil
		pendingFrameDisplayed = false
		return completion
	}

	package mutating func clearDeferredClassicToolbarGlass() {
		defersClassicToolbarGlassUntilAfterFirstDisplay = false
	}
}
