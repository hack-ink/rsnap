import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureHostView {
	func routeCursorUpdate(with event: NSEvent) {
		if scene.mode == .frozen {
			frozenToolbar.refreshHoveredAction(for: event.locationInWindow)
		}
		applyVisibleCursorIfNeeded(currentCursorPresentation())
	}

	func routeMouseMoved(with event: NSEvent) {
		let point = globalPoint(from: event)
		if scene.mode == .frozen {
			frozenToolbar.refreshHoveredAction(for: event.locationInWindow)
			if recoverReleasedFrozenInteractionIfNeeded(at: point) {
				return
			}
		}
		if scene.mode == .live {
			if recoverReleasedLivePrimaryInteractionIfNeeded(at: point) {
				return
			}
			liveInputTelemetry.recordMouseEvent()
			updateLivePointerPreview(to: point, rendersImmediately: true)
			return
		}
		updateLivePointerPreview(to: point, rendersImmediately: false)
		queuePointerEvent(.moved(point))
	}

	func routeMouseDragged(with event: NSEvent) {
		if scene.mode == .frozen {
			frozenToolbar.refreshHoveredAction(for: event.locationInWindow)
		}

		if scene.mode == .live {
			let point = globalPoint(from: event)
			if recoverReleasedLivePrimaryInteractionIfNeeded(at: point) {
				return
			}
			if livePrimaryInteraction.updateDragThreshold(
				from: point,
				threshold: Self.liveDragIntentThreshold
			) {
				logLivePrimaryInputEvent("capture.live_primary_drag_threshold", point: point)
			}
			updateLivePointerPreview(to: point, rendersImmediately: false)
			queuePointerEvent(
				livePrimaryInteraction.dragExceededThreshold ? .liveDragged(point) : .moved(point))
		} else {
			let point = globalPoint(from: event)
			if recoverReleasedFrozenInteractionIfNeeded(at: point) {
				return
			}
			controller?.continueFrozenInteraction(to: point)
			syncVisibleCursor()
		}
	}

	func routeMouseDown(with event: NSEvent) {
		let localPoint = event.locationInWindow
		let point = globalPoint(from: event)
		switch scene.mode {
		case .hidden:
			break
		case .live:
			suppressLiveHoverChrome()
			livePrimaryInteraction.begin(at: point)
			logLivePrimaryInputEvent("capture.live_primary_mouse_down", point: point)
			controller?.registerLivePrimaryInteractionOwner(self)
			installLiveMouseUpMonitor()
			installLiveMouseReleaseWatchdog()
			updateLivePointerPreview(to: point, rendersImmediately: true)
			controller?.beginPrimaryInteraction(at: point)
		case .frozen:
			frozenToolbar.refreshHoveredAction(for: localPoint)
			if let styleAction = frozenToolbar.annotationStyleAction(at: localPoint) {
				frozenToolbar.performAnnotationStyleAction(styleAction)
				return
			}
			if let action = frozenToolbar.toolbarAction(at: localPoint) {
				frozenToolbar.performToolbarAction(action)
				return
			}
			guard chrome.scrollMinimapPreview == nil else {
				return
			}
			controller?.beginFrozenInteraction(at: point)
			if controller?.hasFrozenOverlayActiveInteraction == true {
				installFrozenMouseReleaseWatchdog()
			}
			syncVisibleCursor()
		}
	}

	func routeScrollWheel(with event: NSEvent) -> Bool {
		guard scene.mode == .frozen else {
			resetAnnotationStyleWheelGate()
			return false
		}
		if controller?.handleScrollCaptureWheel(event, at: globalPoint(from: event)) == true {
			resetAnnotationStyleWheelGate()
			return true
		}
		let localPoint = event.locationInWindow
		guard frozenToolbar.annotationStyleSizeControlContains(localPoint) else {
			resetAnnotationStyleWheelGate()
			return false
		}
		let steps = annotationStyleWheelSteps(from: event)
		guard steps != 0 else {
			return true
		}
		controller?.performFrozenAnnotationSizeSteps(steps)
		frozenToolbar.refreshHoveredAction(for: localPoint)
		return true
	}

	func routeMouseUp(with event: NSEvent) {
		let point = globalPoint(from: event)
		if scene.mode == .live {
			logLivePrimaryInputEvent("capture.live_primary_mouse_up", point: point)
			controller?.completeLivePrimaryInteraction(from: self, at: point)
		} else if scene.mode == .frozen {
			cancelFrozenMouseReleaseWatchdog()
			controller?.completeFrozenInteraction(at: point)
			syncVisibleCursor()
		}
	}

	func routeKeyDown(with event: NSEvent) -> Bool {
		if controller?.handleFrozenTextKey(event) == true {
			return true
		}

		if scene.mode == .frozen, event.modifierFlags.contains(.command),
			routeFrozenCommandShortcut(with: event)
		{
			return true
		}

		switch event.keyCode {
		case 53:
			controller?.cancelCapture()
			return true
		case 48:
			controller?.toggleLoupe()
			return true
		case 49:
			return routeSpaceShortcut()
		default:
			if scene.mode == .frozen, plainFrozenShortcutAvailable(event),
				routePlainFrozenShortcut(with: event)
			{
				return true
			}
			return false
		}
	}

	func syncVisibleCursor() {
		let cursorPresentation = currentCursorPresentation()
		guard window?.isKeyWindow == true else {
			clearVisibleCursorOverride()
			lastCursorPresentation = cursorPresentation
			return
		}
		guard cursorPresentation != lastCursorPresentation else {
			return
		}
		lastCursorPresentation = cursorPresentation
		window?.invalidateCursorRects(for: self)
		if scene.mode == .frozen {
			applyVisibleCursorIfNeeded(cursorPresentation)
		}
	}

	func pointerDispatchInterval() -> TimeInterval {
		NativeHostDisplayRefresh.frameInterval(
			forTargetFramesPerSecond: currentDisplayTargetFramesPerSecond())
	}

	private func routeFrozenCommandShortcut(with event: NSEvent) -> Bool {
		switch event.charactersIgnoringModifiers?.lowercased() {
		case "z":
			if event.modifierFlags.contains(.shift) {
				guard frozenToolbar.item(.redo)?.enabled == true else {
					return true
				}
				controller?.performFrozenRedo()
			} else {
				guard frozenToolbar.item(.undo)?.enabled == true else {
					return true
				}
				controller?.performFrozenUndo()
			}
			return true
		case "s":
			guard frozenToolbar.item(.save)?.enabled == true else {
				return true
			}
			controller?.saveSelection()
			return true
		default:
			return false
		}
	}

	private func routeSpaceShortcut() -> Bool {
		if scene.mode == .frozen {
			guard frozenToolbar.item(.copy)?.enabled == true else {
				return true
			}
			controller?.copySelection()
			return true
		}
		if scene.mode == .live {
			controller?.completePrimaryInteraction(at: scene.pointer ?? NSEvent.mouseLocation)
			return true
		}
		return true
	}

	private func routePlainFrozenShortcut(with event: NSEvent) -> Bool {
		switch event.charactersIgnoringModifiers?.lowercased() {
		case "c":
			guard frozenToolbar.item(.autoCenter)?.enabled == true else {
				return true
			}
			controller?.performFrozenAutoCenter()
			return true
		case "r":
			guard frozenToolbar.item(.ocr)?.enabled == true else {
				return true
			}
			controller?.recognizeText()
			return true
		case "s":
			guard frozenToolbar.item(.scroll)?.enabled == true else {
				return true
			}
			controller?.startScrollCapture(source: "keyboard_s")
			return true
		default:
			return false
		}
	}

	private func plainFrozenShortcutAvailable(_ event: NSEvent) -> Bool {
		let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
		return flags.contains(.command) == false
			&& flags.contains(.control) == false
			&& flags.contains(.option) == false
			&& flags.contains(.shift) == false
	}

	private func annotationStyleWheelSteps(from event: NSEvent) -> Int {
		let phase = event.phase
		return annotationStyleWheelGate.steps(
			timestamp: event.timestamp,
			deltaY: event.scrollingDeltaY,
			hasPreciseScrollingDeltas: event.hasPreciseScrollingDeltas,
			phaseActive: phase != [],
			phaseEndedOrCancelled: phase.contains(.ended) || phase.contains(.cancelled),
			momentumActive: event.momentumPhase != []
		)
	}

	private func resetAnnotationStyleWheelGate() {
		annotationStyleWheelGate.reset()
	}

	func currentCursorPresentation() -> CaptureHostCursorPresentation {
		if toolbarHoverState.pointerOverToolbar || toolbarHoverState.toolbarAction != nil {
			return .arrow
		}
		if scene.mode == .frozen {
			if let interaction = chrome.frozenSelectionInteraction {
				return CaptureHostCursorSupport.presentation(
					for: CaptureHostCursorSupport.cursorIntent(for: interaction.kind, active: true))
			}
			if let selection = chrome.frozenSelectionSnapshot ?? scene.frozenSelection,
				let selectedModeTool = frozenToolbar.visibleItems().first(where: { $0.selected })?
					.kind
			{
				if [ToolbarItemKind.pen, .arrow, .mosaic, .spotlight].contains(selectedModeTool) {
					return .crosshair
				}
				if selectedModeTool == .pointer {
					if chrome.frozenOverlay.isMovingMovableAnnotation {
						return .closedHand
					}
					if let pointer = currentGlobalMousePoint(),
						chrome.frozenOverlay.containsMovableAnnotation(at: pointer)
					{
						return .openHand
					}
					if chrome.frozenSelectionTransformAllowed == false {
						return .arrow
					}
					if let pointer = currentGlobalMousePoint(),
						let intent = CaptureHostCursorSupport.editableFrozenCursorIntent(
							at: pointer,
							selection: selection
						)
					{
						return CaptureHostCursorSupport.presentation(for: intent)
					}
				}
			}
		}

		return CaptureHostCursorSupport.presentation(for: scene.cursorIntent)
	}

	private func applyVisibleCursorIfNeeded(_ cursorPresentation: CaptureHostCursorPresentation) {
		guard cursorPresentation != lastAppliedCursorPresentation else {
			return
		}
		clearVisibleCursorOverride()
		lastAppliedCursorPresentation = cursorPresentation
		CaptureHostCursorSupport.cursor(for: cursorPresentation).push()
		pushedCursorPresentation = cursorPresentation
	}

	func forceVisibleCursorRefresh() {
		lastCursorPresentation = nil
		lastAppliedCursorPresentation = nil
		clearVisibleCursorOverride()
		syncVisibleCursor()
	}

	func clearVisibleCursorOverride() {
		guard pushedCursorPresentation != nil else {
			return
		}
		NSCursor.pop()
		pushedCursorPresentation = nil
		lastAppliedCursorPresentation = nil
	}

	private func suppressLiveHoverChrome() {
		guard scene.mode == .live, livePrimaryInteraction.suppressHoverChrome() else {
			return
		}
		updateLivePreviewDemands()
		liveRenderer.renderNow()
	}

	private func queuePointerEvent(_ event: CaptureHostPointerDispatchEvent) {
		pointerDispatchQueue.enqueue(event)
	}

	func dispatchPointerEvent(_ event: CaptureHostPointerDispatchEvent) {
		switch event {
		case .moved(let point):
			controller?.pointerMoved(to: point)
		case .liveDragged(let point):
			if recoverReleasedLivePrimaryInteractionIfNeeded(at: point) {
				return
			}
			controller?.continuePrimaryInteraction(to: point)
		}
	}
}
