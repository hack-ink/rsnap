import AppKit
import CoreGraphics
import Foundation
import QuartzCore
import RsnapHostBridge

@MainActor
final class CaptureHostView: NSView {
	private static let liveDragIntentThreshold: CGFloat = 3

	weak var controller: CaptureSessionController?

	private(set) var scene = SceneSnapshot(
		mode: .hidden,
		cursorIntent: .default,
		pointer: nil,
		activeMonitor: nil,
		highlightedWindow: nil,
		liveSelectionPreview: nil,
		frozenSelection: nil,
		rgb: nil,
		loupeVisible: false,
		toolbarItems: [],
		statusMessage: nil
	)
	private(set) var chrome = CaptureChromeState()
	private(set) var settings = NativeHostSettings.defaults
	private var trackingAreaRef: NSTrackingArea?
	private var annotationStyleWheelGate = CaptureHostAnnotationStyleWheelGate()
	private var lastCursorPresentation: CaptureHostCursorPresentation?
	private var lastAppliedCursorPresentation: CaptureHostCursorPresentation?
	var livePrimaryInteraction = CaptureHostLivePrimaryInteractionState()
	let mouseReleaseRecovery = CaptureHostMouseReleaseRecovery()
	let livePointerPreview = CaptureHostLivePointerPreviewState()
	var liveHighlightedWindowPreview: WindowSnapshot?
	var sampleUpdatedLiveChromeRenderInProgress = false
	var frozenFirstDisplayHandoff = CaptureHostFrozenFirstDisplayHandoffState()
	var lastLivePreviewSnapshot: LivePreviewSnapshot?
	var liveSampleCache = CaptureHostLiveSampleCache()
	private lazy var frozenToolbar = CaptureHostFrozenToolbarCoordinator(hostView: self)
	private lazy var materialViews = CaptureHostMaterialViewCoordinator(hostView: self)
	lazy var pointerDispatchQueue = CaptureHostPointerDispatchQueue(
		targetInterval: { [weak self] in
			guard let self else {
				return NativeHostDisplayRefresh.frameInterval(
					forTargetFramesPerSecond:
						NativeHostDisplayRefresh.maximumTargetFramesPerSecond)
			}
			return self.pointerDispatchInterval()
		},
		dispatchEvent: { [weak self] event in
			self?.dispatchPointerEvent(event)
		}
	)
	lazy var liveRenderer = LiveOverlayRenderer(hostView: self)
	private var liveRendererInstalled = false
	private var deferredLiveShutdownWorkItem: DispatchWorkItem?
	private var loggedLiveRefreshTarget: LiveChromeRefreshTelemetryKey?
	let liveInputTelemetry = CaptureHostLiveInputTelemetry()

	override var acceptsFirstResponder: Bool { true }
	override var isOpaque: Bool { false }
	var toolbarHoverState: CaptureHostToolbarHoverState { frozenToolbar.hoverState }

	override func hitTest(_ point: NSPoint) -> NSView? {
		guard scene.mode == .frozen, chrome.scrollMinimapPreview != nil,
			let selection = localFrozenSelectionRect(), selection.contains(point),
			!frozenToolbar.frameContains(point),
			frozenToolbar.annotationStyleAction(at: point) == nil
		else {
			return super.hitTest(point)
		}
		return nil
	}

	override init(frame frameRect: NSRect) {
		super.init(frame: frameRect)
		wantsLayer = true
		layerContentsRedrawPolicy = .duringViewResize
		liveRenderer.install { [weak self] in
			self?.currentRendererPreviewSnapshot()
		}
		liveRendererInstalled = true
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func update(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) {
		let previousScene = self.scene
		let previousChrome = self.chrome
		let previousSettings = self.settings
		let previousMode = self.scene.mode
		let transitioningToFrozen = previousMode == .live && scene.mode == .frozen
		let hostLocalFrozenSelectingEnded =
			previousChrome.hostLocalFrozenSelecting && !chrome.hostLocalFrozenSelecting
		if scene.mode != .frozen {
			frozenFirstDisplayHandoff.reset()
			materialViews.resetScrollToolbarBackdropTracking()
		} else if previousChrome.scrollMinimapPreview == nil, chrome.scrollMinimapPreview != nil {
			materialViews.resetScrollToolbarBackdropTracking(
				seedFrame: previousChrome.frozenDisplayFrame,
				seedImage: previousChrome.frozenDisplayImage
			)
		} else if previousChrome.scrollMinimapPreview != nil, chrome.scrollMinimapPreview == nil {
			materialViews.resetScrollToolbarBackdropTracking()
		}
		self.scene = scene
		self.chrome = chrome
		self.settings = settings
		if hostLocalFrozenSelectingEnded {
			clearLivePrimaryInteractionState(rendersImmediately: false)
		}
		if previousMode != scene.mode {
			window?.acceptsMouseMovedEvents = true
			updateTrackingAreas()
		}
		if scene.mode == .live {
			frozenFirstDisplayHandoff.reset()
			if previousMode != .live {
				livePrimaryInteraction.clearHoverChromeSuppression()
				liveInputTelemetry.reset()
				seedLiveChromeSampleCache(from: chrome, point: scene.pointer)
			}
			if livePointerPreview.globalPoint == nil {
				seedLivePointerPreview(scene.pointer, recordsInputLatency: false)
			}
			if liveHighlightedWindowPreview == nil {
				liveHighlightedWindowPreview = scene.highlightedWindow
			}
		} else {
			clearLivePrimaryInteractionState(rendersImmediately: false)
			if scene.mode == .hidden {
				livePrimaryInteraction.clearHoverChromeSuppression()
				frozenFirstDisplayHandoff.reset()
				lastLivePreviewSnapshot = nil
				liveSampleCache.reset()
			}
			resetLivePointerPreview()
			liveHighlightedWindowPreview = nil
			if transitioningToFrozen {
				frozenFirstDisplayHandoff.beginTransitionToFrozen(
					now: ProcessInfo.processInfo.systemUptime)
			}
		}
		frozenToolbar.refreshHoveredAction()
		syncVisibleCursor()
		updateChromeMaterialViews()
		updateLiveRendererState()
		if scene.mode == .live {
			updateLivePreviewDemands()
			if shouldRenderFullLiveOverlay(
				previousScene: previousScene,
				previousChrome: previousChrome,
				previousSettings: previousSettings,
				previousMode: previousMode
			) {
				liveRenderer.renderNow()
			} else {
				liveRenderer.renderLiveChromeNow()
			}
		} else {
			if transitioningToFrozen {
				liveRenderer.renderNow()
				needsDisplay = true
				completeFrozenFirstDisplayHandoff()
			} else {
				if previousMode == .live {
					stopLivePresentationNow()
				}
				needsDisplay = true
			}
		}
	}

	private func shouldRenderFullLiveOverlay(
		previousScene: SceneSnapshot,
		previousChrome: CaptureChromeState,
		previousSettings: NativeHostSettings,
		previousMode: SceneKind
	) -> Bool {
		guard scene.mode == .live else {
			return false
		}
		return previousMode != .live
			|| previousScene.liveSelectionPreview != scene.liveSelectionPreview
			|| previousScene.highlightedWindow != scene.highlightedWindow
			|| previousChrome.hostLocalFrozenSelecting != chrome.hostLocalFrozenSelecting
			|| previousSettings != settings
	}

	private func completeFrozenFirstDisplayHandoff() {
		guard frozenFirstDisplayHandoff.pending else {
			return
		}
		window?.disableScreenUpdatesUntilFlush()
		finishFrozenFirstDisplayHandoff()
	}

	private func finishFrozenFirstDisplayHandoff() {
		guard let completion = frozenFirstDisplayHandoff.finish() else {
			return
		}
		let materialStartedAt = ProcessInfo.processInfo.systemUptime
		updateChromeMaterialViews()
		let materialMilliseconds = NativeHostTelemetry.milliseconds(since: materialStartedAt)
		let shouldStopLiveRenderer = scene.mode != .live
		lastLivePreviewSnapshot = nil
		window?.disableScreenUpdatesUntilFlush()
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		let liveRendererStopStartedAt = ProcessInfo.processInfo.systemUptime
		if shouldStopLiveRenderer {
			liveRenderer.stop()
		}
		let liveRendererStopMilliseconds =
			NativeHostTelemetry.milliseconds(since: liveRendererStopStartedAt)
		needsDisplay = true
		let displayStartedAt = ProcessInfo.processInfo.systemUptime
		displayIfNeeded()
		let displayMilliseconds = NativeHostTelemetry.milliseconds(since: displayStartedAt)
		CATransaction.commit()
		if completion.deferredClassicToolbarGlass {
			DispatchQueue.main.async { [weak self] in
				guard let self else {
					return
				}
				self.frozenFirstDisplayHandoff.clearDeferredClassicToolbarGlass()
				self.needsDisplay = true
			}
		}
		if let handoffStartedAt = completion.startedAt {
			emitFrozenFirstDisplayHandoffTiming(
				startedAt: handoffStartedAt,
				materialMilliseconds: materialMilliseconds,
				liveRendererStopMilliseconds: liveRendererStopMilliseconds,
				displayMilliseconds: displayMilliseconds,
				pendingFrameDisplayed: completion.pendingFrameDisplayed
			)
		}
	}

	private func emitFrozenFirstDisplayHandoffTiming(
		startedAt: TimeInterval,
		materialMilliseconds: Double,
		liveRendererStopMilliseconds: Double,
		displayMilliseconds: Double,
		pendingFrameDisplayed: Bool
	) {
		NativeHostTelemetry.frozenFirstDisplayHandoffTiming(
			captureID: controller?.activeTelemetryCaptureID ?? 0,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAt),
			materialMilliseconds: materialMilliseconds,
			liveRendererStopMilliseconds: liveRendererStopMilliseconds,
			displayMilliseconds: displayMilliseconds,
			toolbarVisible: frozenToolbarVisibleForContract(),
			toolbarItemCount: frozenToolbar.visibleItems().count,
			usesLiquidHudGlass: settings.usesLiquidHudGlass,
			usesClassicHudGlass: settings.usesClassicHudGlass,
			liquidGlassAvailable: LiveChromeGlassMaterialSupport.isLiquidGlassAvailable,
			frozenToolbarLiquidGlassVisible: materialViews.isFrozenToolbarLiquidGlassVisible,
			frozenToolbarLiquidGlassContentDrawn: materialViews
				.isFrozenToolbarLiquidGlassContentDrawn,
			frozenSelectionEditable: chrome.frozenSelectionEditable,
			pendingFrameDisplayed: pendingFrameDisplayed
		)
	}

	func seedInitialState(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) {
		self.scene = scene
		self.chrome = chrome
		self.settings = settings
		livePrimaryInteraction.clearHoverChromeSuppression()
		frozenFirstDisplayHandoff.reset()
		lastLivePreviewSnapshot = nil
		if scene.mode == .live {
			seedLivePointerPreview(scene.pointer, recordsInputLatency: false)
			liveHighlightedWindowPreview = scene.highlightedWindow
		} else {
			clearLivePrimaryInteractionState(rendersImmediately: false)
			resetLivePointerPreview()
			liveHighlightedWindowPreview = nil
		}
		lastCursorPresentation = currentCursorPresentation()
		updateChromeMaterialViews()
		updateLiveRendererState()
		if scene.mode == .live {
			seedLiveChromeSampleCache(from: chrome, point: scene.pointer)
		}
	}

	func refreshLivePresentationNow() {
		guard scene.mode == .live else {
			return
		}
		updateLivePreviewDemands()
		liveRenderer.renderNow()
	}

	fileprivate func refreshLiveChromeNow() {
		guard scene.mode == .live else {
			return
		}
		updateLivePreviewSampleDemand()
		liveRenderer.renderLiveChromeNow()
	}

	func refreshSampleUpdatedLiveChromeNow() {
		if scene.mode == .frozen, chrome.scrollMinimapPreview != nil {
			guard let state = controller?.scrollCaptureState,
				controller?.nativeScrollCaptureToolbarBackdropShouldLoop(state: state) == true
			else {
				return
			}
			refreshScrollCaptureToolbarBackdropNow()
			return
		}
		guard scene.mode == .live else {
			return
		}
		sampleUpdatedLiveChromeRenderInProgress = true
		defer {
			sampleUpdatedLiveChromeRenderInProgress = false
		}
		updateLivePreviewSampleDemand()
		liveRenderer.renderLiveChromeNow()
	}

	func installFrozenFirstFrame(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings,
		rendersPendingFrame: Bool = true
	) {
		let retainedLivePreview =
			rendersPendingFrame ? (lastLivePreviewSnapshot ?? currentLivePreviewSnapshot()) : nil
		self.scene = scene
		self.chrome = chrome
		self.settings = settings
		livePrimaryInteraction.clearHoverChromeSuppression()
		frozenFirstDisplayHandoff.beginFrozenFirstFrameInstall(
			pending: retainedLivePreview != nil || scene.frozenSelection != nil,
			defersClassicToolbarGlass: settings.usesClassicHudGlass,
			now: ProcessInfo.processInfo.systemUptime
		)
		lastLivePreviewSnapshot = retainedLivePreview
		clearLivePrimaryInteractionState(rendersImmediately: false)
		resetLivePointerPreview()
		liveHighlightedWindowPreview = nil
		frozenToolbar.clearHoveredAction()
		syncVisibleCursor()
		needsDisplay = true
		controller?.updateLivePreviewDemand(
			point: nil, settings: settings, includeLoupePatch: false)
		if rendersPendingFrame, frozenFirstDisplayHandoff.pending {
			frozenFirstDisplayHandoff.markPendingFrameDisplayed()
			liveRenderer.renderNow()
		}
	}

	func finishFrozenFirstFrameInstall() {
		guard frozenFirstDisplayHandoff.pending else {
			return
		}
		window?.disableScreenUpdatesUntilFlush()
		finishFrozenFirstDisplayHandoff()
	}

	override func layout() {
		super.layout()
		updateChromeMaterialViews()
		updateLiveRendererState()
		if scene.mode == .live {
			updateLivePreviewDemands()
		}
	}

	override func viewDidMoveToWindow() {
		super.viewDidMoveToWindow()
		window?.makeFirstResponder(self)
		updateTrackingAreas()
		updateLiveRendererState()
	}

	override func updateTrackingAreas() {
		if let trackingAreaRef {
			removeTrackingArea(trackingAreaRef)
		}

		let options: NSTrackingArea.Options = [
			.activeAlways, .cursorUpdate, .inVisibleRect, .mouseMoved, .enabledDuringMouseDrag,
		]
		let trackingAreaRef = NSTrackingArea(
			rect: bounds,
			options: options,
			owner: self,
			userInfo: nil
		)
		addTrackingArea(trackingAreaRef)
		self.trackingAreaRef = trackingAreaRef
	}

	override func resetCursorRects() {
		super.resetCursorRects()
		addCursorRect(
			bounds,
			cursor: CaptureHostCursorSupport.cursor(for: currentCursorPresentation())
		)
	}

	override func cursorUpdate(with event: NSEvent) {
		if scene.mode == .frozen {
			frozenToolbar.refreshHoveredAction(for: event.locationInWindow)
		}
		applyVisibleCursorIfNeeded(currentCursorPresentation())
	}

	override func mouseMoved(with event: NSEvent) {
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

	override func mouseDragged(with event: NSEvent) {
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

	override func mouseDown(with event: NSEvent) {
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

	override func scrollWheel(with event: NSEvent) {
		guard scene.mode == .frozen else {
			resetAnnotationStyleWheelGate()
			super.scrollWheel(with: event)
			return
		}
		if controller?.handleScrollCaptureWheel(event, at: globalPoint(from: event)) == true {
			resetAnnotationStyleWheelGate()
			return
		}
		let localPoint = event.locationInWindow
		guard frozenToolbar.annotationStyleSizeControlContains(localPoint) else {
			resetAnnotationStyleWheelGate()
			super.scrollWheel(with: event)
			return
		}
		let steps = annotationStyleWheelSteps(from: event)
		guard steps != 0 else {
			return
		}
		controller?.performFrozenAnnotationSizeSteps(steps)
		frozenToolbar.refreshHoveredAction(for: localPoint)
	}

	override func rightMouseDown(with event: NSEvent) {
		controller?.cancelCapture()
	}

	override func mouseUp(with event: NSEvent) {
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

	override func keyDown(with event: NSEvent) {
		if controller?.handleFrozenTextKey(event) == true {
			return
		}

		if scene.mode == .frozen, event.modifierFlags.contains(.command) {
			switch event.charactersIgnoringModifiers?.lowercased() {
			case "z":
				if event.modifierFlags.contains(.shift) {
					guard frozenToolbar.item(.redo)?.enabled == true else {
						return
					}
					controller?.performFrozenRedo()
				} else {
					guard frozenToolbar.item(.undo)?.enabled == true else {
						return
					}
					controller?.performFrozenUndo()
				}
				return
			case "s":
				guard frozenToolbar.item(.save)?.enabled == true else {
					return
				}
				controller?.saveSelection()
				return
			default:
				break
			}
		}

		switch event.keyCode {
		case 53:
			controller?.cancelCapture()
		case 48:
			controller?.toggleLoupe()
		case 49:
			if scene.mode == .frozen {
				guard frozenToolbar.item(.copy)?.enabled == true else {
					return
				}
				controller?.copySelection()
			} else if scene.mode == .live {
				controller?.completePrimaryInteraction(at: scene.pointer ?? NSEvent.mouseLocation)
			}
		default:
			if scene.mode == .frozen, plainFrozenShortcutAvailable(event) {
				switch event.charactersIgnoringModifiers?.lowercased() {
				case "c":
					guard frozenToolbar.item(.autoCenter)?.enabled == true else {
						return
					}
					controller?.performFrozenAutoCenter()
					return
				case "r":
					guard frozenToolbar.item(.ocr)?.enabled == true else {
						return
					}
					controller?.recognizeText()
					return
				case "s":
					guard frozenToolbar.item(.scroll)?.enabled == true else {
						return
					}
					controller?.startScrollCapture(source: "keyboard_s")
					return
				default:
					break
				}
			}
			super.keyDown(with: event)
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

	override func draw(_ dirtyRect: NSRect) {
		super.draw(dirtyRect)
		guard let context = NSGraphicsContext.current?.cgContext else {
			return
		}
		context.clear(bounds)

		switch scene.mode {
		case .hidden:
			break
		case .live:
			break
		case .frozen:
			if frozenFirstDisplayHandoff.pending {
				frozenFirstDisplayHandoff.markPendingFrameDisplayed()
				scheduleFrozenFirstFrameInstallCompletionIfNeeded()
				return
			}
			if let selection = localFrozenSelectionRect().map({
				CaptureHostFrozenPresentationRenderer.pixelAlignedSelectionRect(
					$0,
					backingScaleFactor: window?.screen?.backingScaleFactor ?? 1
				)
			}) {
				let toolbarLayout = toolbarLayout(for: selection)
				CaptureHostFrozenPresentationRenderer.render(
					selection: selection,
					bounds: bounds,
					backingScaleFactor: window?.screen?.backingScaleFactor ?? 1,
					theme: chromeTheme(),
					settings: settings,
					chrome: chrome,
					toolbarLayout: toolbarLayout,
					toolbarHoverState: toolbarHoverState,
					materialState: CaptureHostFrozenPresentationMaterialState(
						toolbarLiquidGlassVisible: materialViews.isFrozenToolbarLiquidGlassVisible,
						toolbarLiquidGlassContentDrawn: materialViews
							.isFrozenToolbarLiquidGlassContentDrawn,
						allowsClassicToolbarGlass: frozenFirstDisplayHandoff
							.allowsClassicToolbarGlass
					),
					frozenDisplayFrame: localFrozenDisplayFrame(),
					frozenDisplayImage: chrome.frozenDisplayImage,
					windowFrame: window?.frame,
					selectionSizeText: selectionSizeText(for: selection),
					glassPatch: { [weak self] surfaceKind, frame in
						self?.materialViews.glassPatch(for: surfaceKind, frame: frame)
					},
					in: context
				)
			}
			scheduleFrozenFirstFrameInstallCompletionIfNeeded()
		}

	}

	private func scheduleFrozenFirstFrameInstallCompletionIfNeeded() {
		guard frozenFirstDisplayHandoff.queueCompletionIfNeeded() else {
			return
		}
		DispatchQueue.main.async { [weak self] in
			self?.finishFrozenFirstFrameInstall()
		}
	}

	func localFrozenDisplayFrame() -> CGRect? {
		localRect(from: chrome.frozenDisplayFrame)
	}

	func currentImmediateLiveDragSelectionLocal() -> CGRect? {
		guard scene.mode == .live, let window else {
			return nil
		}
		guard
			let globalRect = livePrimaryInteraction.immediateDragSelectionGlobal(
				current: livePointerPreview.currentPoint(fallback: scene.pointer),
				in: window.frame
			)
		else {
			return nil
		}
		return localRect(from: globalRect)
	}

	func localPointer() -> CGPoint? {
		guard let globalPoint = livePointerPreview.currentPoint(fallback: scene.pointer) else {
			return nil
		}
		return localPoint(from: globalPoint)
	}

	func localFrozenSelectionRect() -> CGRect? {
		localRect(from: chrome.frozenSelectionSnapshot ?? scene.frozenSelection)
	}

	func localRect(from globalRect: CGRect?) -> CGRect? {
		guard let selection = globalRect, let window else {
			return nil
		}
		let localRect = CGRect(
			x: selection.minX - window.frame.minX,
			y: selection.minY - window.frame.minY,
			width: selection.width,
			height: selection.height
		)
		return localRect.intersects(bounds) ? localRect : nil
	}

	func globalRect(from localRect: CGRect) -> CGRect? {
		guard let window else {
			return nil
		}
		return CGRect(
			x: localRect.minX + window.frame.minX,
			y: localRect.minY + window.frame.minY,
			width: localRect.width,
			height: localRect.height
		)
	}

	func localPoint(from globalPoint: CGPoint) -> CGPoint? {
		guard let window else {
			return nil
		}
		return captureOverlayLocalPoint(
			from: globalPoint,
			windowFrame: window.frame,
			bounds: bounds
		)
	}

	func currentLocalMousePoint() -> CGPoint? {
		guard let window else {
			return nil
		}
		let localPoint = window.mouseLocationOutsideOfEventStream
		return bounds.clampedInclusivePoint(localPoint)
	}

	private func currentCursorPresentation() -> CaptureHostCursorPresentation {
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

	func globalPoint(from event: NSEvent) -> CGPoint {
		guard let window else {
			return NSEvent.mouseLocation
		}
		return window.convertPoint(toScreen: event.locationInWindow)
	}

	func currentGlobalMousePoint() -> CGPoint? {
		guard let window else {
			return NSEvent.mouseLocation
		}
		let localPoint = window.mouseLocationOutsideOfEventStream
		let globalPoint = window.convertPoint(toScreen: localPoint)
		return NSScreen.screens.contains(where: { $0.frame.inclusivelyContains(globalPoint) })
			? globalPoint : nil
	}

	func toolbarLayout(for selection: CGRect) -> FrozenToolbarLayout? {
		frozenToolbar.layout(for: selection)
	}

	private func frozenToolbarVisibleForContract() -> Bool {
		guard scene.mode == .frozen,
			let selection = localFrozenSelectionRect(),
			toolbarLayout(for: selection) != nil
		else {
			return false
		}
		if settings.usesLiquidHudGlass {
			return materialViews.isFrozenToolbarLiquidGlassVisible
				&& materialViews.isFrozenToolbarLiquidGlassContentDrawn
		}
		return true
	}

	func syncVisibleCursor() {
		let cursorPresentation = currentCursorPresentation()
		guard cursorPresentation != lastCursorPresentation else {
			return
		}
		lastCursorPresentation = cursorPresentation
		window?.invalidateCursorRects(for: self)
		if scene.mode == .frozen {
			applyVisibleCursorIfNeeded(cursorPresentation)
		}
	}

	private func applyVisibleCursorIfNeeded(_ cursorPresentation: CaptureHostCursorPresentation) {
		guard cursorPresentation != lastAppliedCursorPresentation else {
			return
		}
		lastAppliedCursorPresentation = cursorPresentation
		CaptureHostCursorSupport.cursor(for: cursorPresentation).set()
	}

	func updateLiveChromeBackdrops() {
		let frames = currentLiveChromeLayerFrames()
		updateLiveChromeBackdrops(hudFrame: frames.hud, loupeFrame: frames.loupe)
	}

	func updateLiveChromeBackdrops(hudFrame: CGRect?, loupeFrame: CGRect?) {
		guard scene.mode == .live, settings.usesClassicHudGlass else {
			controller?.updateLiveChromeBackdrops(nil)
			return
		}
		controller?.updateLiveChromeBackdrops(
			LiveChromeBackdropSnapshot(
				sourceWindowNumber: window?.windowNumber,
				hudFrame: hudFrame.flatMap(globalRect(from:)),
				loupeFrame: loupeFrame.flatMap(globalRect(from:)),
				theme: chromeTheme(),
				settings: settings
			)
		)
	}

	func moveLiveChromeLayers() {
		let frames = currentLiveChromeLayerFrames()
		updateLiveChromeBackdrops(hudFrame: frames.hud, loupeFrame: frames.loupe)
		moveExistingLiveLiquidGlassViews(hudFrame: frames.hud, loupeFrame: frames.loupe)
		liveRenderer.moveLiveChrome(
			hudFrame: frames.hud,
			loupeFrame: frames.loupe,
			chromeExclusions: liveChromeRoundedExclusions(
				hudFrame: frames.hud,
				loupeFrame: frames.loupe
			)
		)
	}

	private func liveChromeRoundedExclusions(
		hudFrame: CGRect?,
		loupeFrame: CGRect?
	) -> [OverlayMaskGeometry.RoundedExclusion] {
		guard settings.hudGlassEnabled else {
			return []
		}
		return [hudFrame, loupeFrame].compactMap { frame in
			frame.map {
				OverlayMaskGeometry.RoundedExclusion(
					rect: $0,
					cornerRadius: CaptureChrome.hudCornerRadius
				)
			}
		}
	}

	private func currentLiveChromeLayerFrames() -> (hud: CGRect?, loupe: CGRect?) {
		let hudPlacement =
			livePrimaryInteraction.hoverChromeSuppressed ? nil : currentHudPlacement()
		let hudFrame = hudPlacement?.frame
		let loupeFrame =
			!livePrimaryInteraction.hoverChromeSuppressed && scene.loupeVisible
			? hudPlacement.flatMap {
				currentLoupeFrame(
					hudFrame: $0.frame,
					patch: reusableLiveLoupePatch(),
					alignTrailing: $0.flippedHorizontally
				)
			}
			: nil
		return (hudFrame, loupeFrame)
	}

	private func updateLiveRendererState() {
		guard liveRendererInstalled else {
			return
		}
		guard scene.mode == .live || frozenFirstDisplayHandoff.pending else {
			liveRenderer.suspend()
			loggedLiveRefreshTarget = nil
			return
		}
		deferredLiveShutdownWorkItem?.cancel()
		deferredLiveShutdownWorkItem = nil
		let displayTargetHz = currentDisplayTargetFramesPerSecond()
		let refreshTarget = LiveChromeRefreshTelemetryKey(
			targetHz: displayTargetHz,
			hudGlassEnabled: settings.hudGlassEnabled,
			hudGlassMode: settings.resolvedHudGlassMode.rawValue,
			liquidGlassStyle: settings.liquidGlassStyle.rawValue,
			liquidGlassAvailable: LiveChromeGlassMaterialSupport.isLiquidGlassAvailable
		)
		if loggedLiveRefreshTarget != refreshTarget {
			loggedLiveRefreshTarget = refreshTarget
			NativeHostTelemetry.liveChromeRefreshTarget(
				captureID: controller?.activeTelemetryCaptureID ?? 0,
				targetHz: displayTargetHz,
				frameBudgetMilliseconds: NativeHostDisplayRefresh.frameBudgetMilliseconds(
					forTargetFramesPerSecond: displayTargetHz),
				hudGlassEnabled: refreshTarget.hudGlassEnabled,
				hudGlassMode: refreshTarget.hudGlassMode,
				liquidGlassStyle: refreshTarget.liquidGlassStyle,
				liquidGlassAvailable: refreshTarget.liquidGlassAvailable
			)
		}
		if scene.mode == .live {
			liveRenderer.updateDisplayID(
				currentDisplayID(), targetFramesPerSecond: currentPointerFollowFramesPerSecond())
			return
		}
		liveRenderer.updateDisplayID(currentDisplayID(), targetFramesPerSecond: displayTargetHz)
	}

	private func stopLivePresentationNow() {
		deferredLiveShutdownWorkItem?.cancel()
		deferredLiveShutdownWorkItem = nil
		frozenFirstDisplayHandoff.reset()
		lastLivePreviewSnapshot = nil
		hideLiveLiquidGlassViews()
		guard scene.mode != .live else {
			return
		}
		liveRenderer.stop()
	}

	private func currentDisplayID() -> CGDirectDisplayID? {
		window?.screen?.nativeDisplayID
	}

	func currentDisplayTargetFramesPerSecond() -> Int {
		NativeHostDisplayRefresh.targetFramesPerSecond(for: window?.screen)
	}

	private func currentPointerFollowFramesPerSecond() -> Int {
		NativeHostDisplayRefresh.pointerFollowFramesPerSecond(for: window?.screen)
	}

	func chromeTheme() -> CaptureChromeTheme {
		effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .aqua ? .light : .dark
	}

	func updateChromeMaterialViews() {
		materialViews.updateChromeMaterialViews()
	}

	func updateLiveLiquidGlassViews(hudFrame: CGRect?, loupeFrame: CGRect?) {
		materialViews.updateLiveLiquidGlassViews(hudFrame: hudFrame, loupeFrame: loupeFrame)
	}

	private func moveExistingLiveLiquidGlassViews(hudFrame: CGRect?, loupeFrame: CGRect?) {
		materialViews.moveExistingLiveLiquidGlassViews(hudFrame: hudFrame, loupeFrame: loupeFrame)
	}

	private func hideLiveLiquidGlassViews(removing: Bool = true) {
		materialViews.hideLiveLiquidGlassViews(removing: removing)
	}

	func refreshScrollCaptureToolbarBackdropNow() {
		materialViews.refreshScrollCaptureToolbarBackdropNow()
	}

	private func suppressLiveHoverChrome() {
		guard scene.mode == .live, livePrimaryInteraction.suppressHoverChrome() else {
			return
		}
		updateLivePreviewDemands()
		liveRenderer.renderNow()
	}

	private func themeBrightnessBias() -> Double {
		chromeTheme() == .dark ? 0.015 : -0.01
	}

	private func queuePointerEvent(_ event: CaptureHostPointerDispatchEvent) {
		pointerDispatchQueue.enqueue(event)
	}

	private func dispatchPointerEvent(_ event: CaptureHostPointerDispatchEvent) {
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

	private func pointerDispatchInterval() -> TimeInterval {
		NativeHostDisplayRefresh.frameInterval(
			forTargetFramesPerSecond: currentDisplayTargetFramesPerSecond())
	}

}
