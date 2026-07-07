import AppKit
import CoreGraphics
import Foundation

extension CaptureHostView {
	func seedLivePointerPreview(
		_ globalPoint: CGPoint?,
		recordsInputLatency: Bool = true
	) {
		guard let globalPoint else {
			resetLivePointerPreview()
			return
		}
		livePointerPreview.seed(globalPoint, recordsInputLatency: recordsInputLatency)
	}

	@discardableResult
	func setLivePointerPreview(
		to globalPoint: CGPoint,
		recordsInputLatency: Bool = true
	) -> Bool {
		livePointerPreview.set(to: globalPoint, recordsInputLatency: recordsInputLatency)
	}

	func resetLivePointerPreview() {
		finishLivePresentationTelemetry(reason: "reset")
		liveInputTelemetry.reset()
		livePointerPreview.reset()
	}

	func markLivePrimaryInteractionReleased(at point: CGPoint) {
		guard scene.mode == .live, livePrimaryInteraction.hasInteraction else {
			return
		}
		let wasDragSelection = livePrimaryInteraction.dragExceededThreshold
		let completionPoint = liveDragCompletionPoint(for: point)
		logLivePrimaryInputEvent(
			"capture.live_primary_release_marked",
			point: completionPoint,
			detail: "dragExceeded=\(wasDragSelection)"
		)
		livePrimaryInteraction.markReleased(at: point)
		removeLiveMouseUpMonitor()
		cancelQueuedPointerDispatch()
		updateLivePointerPreview(
			to: completionPoint,
			rendersImmediately: true,
			rendersFullPreview: wasDragSelection
		)
	}

	var hasLivePrimaryInteraction: Bool {
		scene.mode == .live && livePrimaryInteraction.hasInteraction
	}

	func completeOwnedLivePrimaryInteraction(at point: CGPoint) {
		guard scene.mode == .live, livePrimaryInteraction.canCompleteInteraction else {
			return
		}
		let completionPoint = liveDragCompletionPoint(for: point)
		logLivePrimaryInputEvent(
			"capture.live_primary_complete_owned",
			point: completionPoint,
			detail: "dragExceeded=\(livePrimaryInteraction.dragExceededThreshold)"
		)
		markLivePrimaryInteractionReleased(at: point)
		if let controller {
			controller.completePrimaryInteraction(at: completionPoint)
		} else {
			clearLivePrimaryInteractionState(rendersImmediately: true)
		}
	}

	@discardableResult
	func recoverReleasedLivePrimaryInteractionIfNeeded(at point: CGPoint) -> Bool {
		guard
			scene.mode == .live,
			livePrimaryInteraction.canCompleteInteraction,
			!isPrimaryMouseButtonPressed()
		else {
			return false
		}
		logLivePrimaryInputEvent("capture.live_primary_release_recovered", point: point)
		controller?.completeLivePrimaryInteraction(from: self, at: point)
		return true
	}

	@discardableResult
	func recoverReleasedFrozenInteractionIfNeeded(at point: CGPoint) -> Bool {
		guard
			scene.mode == .frozen,
			controller?.hasFrozenOverlayActiveInteraction == true,
			!isPrimaryMouseButtonPressed()
		else {
			return false
		}
		cancelFrozenMouseReleaseWatchdog()
		cancelQueuedPointerDispatch()
		controller?.completeFrozenInteraction(at: point)
		syncVisibleCursor()
		return true
	}

	func clearLivePrimaryInteractionState(rendersImmediately: Bool) {
		cancelQueuedPointerDispatch()
		livePrimaryInteraction.reset()
		removeLiveMouseUpMonitor()
		if rendersImmediately, scene.mode == .live {
			liveRenderer.renderNow()
		}
	}

	func installLiveMouseUpMonitor() {
		mouseReleaseRecovery.installLiveMouseUpMonitor { [weak self] event in
			self?.completeLivePrimaryInteractionFromMouseUp(event)
		}
	}

	func installLiveMouseReleaseWatchdog() {
		mouseReleaseRecovery.installLiveMouseReleaseWatchdog { [weak self] in
			self?.pollLiveMouseReleaseWatchdog() ?? false
		}
	}

	func installFrozenMouseReleaseWatchdog() {
		mouseReleaseRecovery.installFrozenMouseReleaseWatchdog { [weak self] in
			self?.pollFrozenMouseReleaseWatchdog() ?? false
		}
	}

	func cancelFrozenMouseReleaseWatchdog() {
		mouseReleaseRecovery.cancelFrozenMouseReleaseWatchdog()
	}

	func updateLivePointerPreview(
		to globalPoint: CGPoint,
		rendersImmediately: Bool,
		rendersFullPreview: Bool = false
	) {
		guard scene.mode == .live else {
			return
		}
		liveInputTelemetry.recordPointerEvent()
		let pointerChanged = setLivePointerPreview(to: globalPoint)
		let hoverTargetChanged = refreshLiveHighlightedWindowPreviewForFastPath(at: globalPoint)
		if pointerChanged || rendersImmediately || hoverTargetChanged {
			updateLivePreviewSampleDemand()
			moveLiveChromeLayers()
			if rendersFullPreview || hoverTargetChanged {
				liveRenderer.renderNow()
			} else {
				liveRenderer.renderLiveChromeNow()
			}
		}
	}

	func finishLivePresentationTelemetry(reason: String) {
		liveInputTelemetry.emitInputSummary(
			reason: reason,
			captureID: controller?.activeTelemetryCaptureID ?? 0,
			pointerInputSequence: livePointerPreview.inputSequence
		)
	}

	private func liveDragCompletionPoint(for point: CGPoint) -> CGPoint {
		livePrimaryInteraction.completionPoint(for: point)
	}

	private func isPrimaryMouseButtonPressed() -> Bool {
		mouseReleaseRecovery.isPrimaryMouseButtonPressed
	}

	private func removeLiveMouseUpMonitor() {
		mouseReleaseRecovery.removeLiveMouseUpMonitor()
	}

	private func completeLivePrimaryInteractionFromMouseUp(_ event: NSEvent) {
		completeLivePrimaryInteractionFromSystemMouseUp(
			at: globalPoint(from: event),
			source: "local"
		)
	}

	private func completeLivePrimaryInteractionFromSystemMouseUp(
		at point: CGPoint,
		source: String
	) {
		guard
			scene.mode == .live,
			livePrimaryInteraction.canCompleteInteraction
		else {
			return
		}
		logLivePrimaryInputEvent(
			"capture.live_primary_mouse_up_monitor",
			point: point,
			detail: "source=\(source)"
		)
		controller?.completeLivePrimaryInteraction(
			from: self,
			at: point
		)
	}

	private func pollLiveMouseReleaseWatchdog() -> Bool {
		guard
			scene.mode == .live,
			livePrimaryInteraction.canCompleteInteraction
		else {
			return false
		}
		if isPrimaryMouseButtonPressed() == false {
			let point = NSEvent.mouseLocation
			logLivePrimaryInputEvent("capture.live_primary_release_watchdog", point: point)
			completeLivePrimaryInteractionFromSystemMouseUp(at: point, source: "watchdog")
			return false
		}
		return true
	}

	private func pollFrozenMouseReleaseWatchdog() -> Bool {
		guard
			scene.mode == .frozen,
			controller?.hasFrozenOverlayActiveInteraction == true
		else {
			return false
		}
		if isPrimaryMouseButtonPressed() == false {
			let point = currentGlobalMousePoint() ?? NSEvent.mouseLocation
			NativeHostTelemetry.captureEvent(
				"capture.frozen_primary_release_watchdog",
				captureID: controller?.activeTelemetryCaptureID ?? 0,
				detail: "x=\(Int(point.x.rounded())) y=\(Int(point.y.rounded()))"
			)
			cancelQueuedPointerDispatch()
			controller?.completeFrozenInteraction(at: point)
			syncVisibleCursor()
			return false
		}
		return true
	}

	func logLivePrimaryInputEvent(
		_ event: String,
		point: CGPoint,
		detail: String = "none"
	) {
		NativeHostTelemetry.captureEvent(
			event,
			captureID: controller?.activeTelemetryCaptureID ?? 0,
			detail:
				"\(detail) x=\(Int(point.x.rounded())) y=\(Int(point.y.rounded())) inFlight=\(livePrimaryInteraction.completionInFlight)"
		)
	}

	func cancelQueuedPointerDispatch() {
		pointerDispatchQueue.cancel()
	}
}
