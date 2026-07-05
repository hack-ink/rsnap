import AppKit
import CoreGraphics
import CoreImage
import Foundation
import QuartzCore
import RsnapHostBridge

@MainActor private let frozenEffectCIContext = CIContext(options: nil)

@MainActor
final class CaptureHostView: NSView {
	private static let liveDragIntentThreshold: CGFloat = 3
	private static let scrollToolbarBackdropCaptureMinimumInterval: TimeInterval = 1.0 / 60.0
	private static let scrollToolbarBackdropFallbackMinimumInterval: TimeInterval = 1.0 / 20.0

	private enum GlassSurfaceKind: Hashable {
		case hud
		case loupe
		case toolbar
	}

	private static let liveChromeLiquidGlassZ: CGFloat = 200
	private static let frozenToolbarLiquidGlassBackdropZ: CGFloat = 295
	private static let frozenToolbarLiquidGlassZ: CGFloat = 300
	private static let frozenToolbarContentZ: CGFloat = 320

	weak var controller: CaptureSessionController?

	private var scene = SceneSnapshot(
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
	private var chrome = CaptureChromeState()
	private var settings = NativeHostSettings.defaults
	private var hudLiquidGlassView: NSView?
	private var loupeLiquidGlassView: NSView?
	private var toolbarLiquidGlassBackdropView: NSImageView?
	private var toolbarLiquidGlassView: NSView?
	private var toolbarLiquidGlassContentView: FrozenToolbarRenderView?
	private var frozenToolbarLiquidGlassVisible = false
	private var frozenToolbarLiquidGlassContentDrawn = false
	private let scrollToolbarBackdropRefreshGapMetric = NativeHostTelemetry.distribution(
		"scroll_capture.toolbar_backdrop_refresh_gap",
		category: "Capture",
		batchSize: 30
	)
	private let scrollToolbarBackdropRefreshDurationMetric = NativeHostTelemetry.distribution(
		"scroll_capture.toolbar_backdrop_refresh_duration",
		category: "Capture",
		batchSize: 30
	)
	private let scrollToolbarBackdropChangedGapMetric = NativeHostTelemetry.distribution(
		"scroll_capture.toolbar_backdrop_changed_gap",
		category: "Capture",
		batchSize: 30
	)
	private let scrollToolbarBackdropCaptureQueue = DispatchQueue(
		label: "ink.hack.rsnap.scroll-toolbar-backdrop-capture",
		qos: .userInitiated
	)
	private var scrollToolbarBackdropState = CaptureHostScrollToolbarBackdropState()
	private var trackingAreaRef: NSTrackingArea?
	private var toolbarHoverState = CaptureHostToolbarHoverState()
	private var annotationStyleWheelGate = CaptureHostAnnotationStyleWheelGate()
	private var lastCursorPresentation: CaptureHostCursorPresentation?
	private var lastAppliedCursorPresentation: CaptureHostCursorPresentation?
	private var livePrimaryInteraction = CaptureHostLivePrimaryInteractionState()
	private var liveMouseUpMonitor: Any?
	private var liveMouseReleaseWatchdog: DispatchWorkItem?
	private var frozenMouseReleaseWatchdog: DispatchWorkItem?
	private var livePointerPreviewGlobal: CGPoint?
	private var livePointerPreviewInputUptime: TimeInterval?
	private var livePointerPreviewInputSequence: UInt64 = 0
	private var lastLivePointerEventUptime: TimeInterval?
	private var liveHighlightedWindowPreview: WindowSnapshot?
	private var sampleUpdatedLiveChromeRenderInProgress = false
	private var frozenFirstDisplayHandoff = CaptureHostFrozenFirstDisplayHandoffState()
	private var lastLivePreviewSnapshot: LivePreviewSnapshot?
	private var liveSampleCache = CaptureHostLiveSampleCache()
	private var glassPatchCache: [GlassSurfaceKind: CaptureHostGlassPatchCache] = [:]
	private lazy var pointerDispatchQueue = CaptureHostPointerDispatchQueue(
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
	private lazy var liveRenderer = LiveOverlayRenderer(hostView: self)
	private var liveRendererInstalled = false
	private var deferredLiveShutdownWorkItem: DispatchWorkItem?
	private var loggedLiveRefreshTarget: LiveChromeRefreshTelemetryKey?
	private let livePointerEventGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.pointer_event_gap",
		category: "LiveChromeTelemetry"
	)
	private var liveChromeMouseEventCount = 0
	private var didEmitLiveChromeInputSummary = false

	override var acceptsFirstResponder: Bool { true }
	override var isOpaque: Bool { false }

	override func hitTest(_ point: NSPoint) -> NSView? {
		guard scene.mode == .frozen, chrome.scrollMinimapPreview != nil,
			let selection = localFrozenSelectionRect(), selection.contains(point),
			!toolbarFrameContains(point), annotationStyleAction(at: point) == nil
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
			scrollToolbarBackdropState.resetTracking()
		} else if previousChrome.scrollMinimapPreview == nil, chrome.scrollMinimapPreview != nil {
			scrollToolbarBackdropState.resetTracking(
				seedFrame: previousChrome.frozenDisplayFrame,
				seedImage: previousChrome.frozenDisplayImage
			)
		} else if previousChrome.scrollMinimapPreview != nil, chrome.scrollMinimapPreview == nil {
			scrollToolbarBackdropState.resetTracking()
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
				resetLiveChromeInputTelemetry()
				seedLiveChromeSampleCache(from: chrome, point: scene.pointer)
			}
			if livePointerPreviewGlobal == nil {
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
		refreshHoveredToolbarAction()
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
			toolbarItemCount: visibleToolbarItems().count,
			usesLiquidHudGlass: settings.usesLiquidHudGlass,
			usesClassicHudGlass: settings.usesClassicHudGlass,
			liquidGlassAvailable: LiveChromeGlassMaterialSupport.isLiquidGlassAvailable,
			frozenToolbarLiquidGlassVisible: frozenToolbarLiquidGlassVisible,
			frozenToolbarLiquidGlassContentDrawn: frozenToolbarLiquidGlassContentDrawn,
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
		clearHoveredToolbarAction()
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
			refreshHoveredToolbarAction(for: event.locationInWindow)
		}
		applyVisibleCursorIfNeeded(currentCursorPresentation())
	}

	override func mouseMoved(with event: NSEvent) {
		let point = globalPoint(from: event)
		if scene.mode == .frozen {
			refreshHoveredToolbarAction(for: event.locationInWindow)
			if recoverReleasedFrozenInteractionIfNeeded(at: point) {
				return
			}
		}
		if scene.mode == .live {
			if recoverReleasedLivePrimaryInteractionIfNeeded(at: point) {
				return
			}
			liveChromeMouseEventCount += 1
			updateLivePointerPreview(to: point, rendersImmediately: true)
			return
		}
		updateLivePointerPreview(to: point, rendersImmediately: false)
		queuePointerEvent(.moved(point))
	}

	override func mouseDragged(with event: NSEvent) {
		if scene.mode == .frozen {
			refreshHoveredToolbarAction(for: event.locationInWindow)
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
			refreshHoveredToolbarAction(for: localPoint)
			if let styleAction = annotationStyleAction(at: localPoint) {
				performAnnotationStyleAction(styleAction)
				return
			}
			if let action = toolbarAction(at: localPoint) {
				performToolbarAction(action)
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
		guard annotationStyleSizeControlContains(localPoint) else {
			resetAnnotationStyleWheelGate()
			super.scrollWheel(with: event)
			return
		}
		let steps = annotationStyleWheelSteps(from: event)
		guard steps != 0 else {
			return
		}
		controller?.performFrozenAnnotationSizeSteps(steps)
		refreshHoveredToolbarAction(for: localPoint)
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
					guard toolbarItem(.redo)?.enabled == true else {
						return
					}
					controller?.performFrozenRedo()
				} else {
					guard toolbarItem(.undo)?.enabled == true else {
						return
					}
					controller?.performFrozenUndo()
				}
				return
			case "s":
				guard toolbarItem(.save)?.enabled == true else {
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
				guard toolbarItem(.copy)?.enabled == true else {
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
					guard toolbarItem(.autoCenter)?.enabled == true else {
						return
					}
					controller?.performFrozenAutoCenter()
					return
				case "r":
					guard toolbarItem(.ocr)?.enabled == true else {
						return
					}
					controller?.recognizeText()
					return
				case "s":
					guard toolbarItem(.scroll)?.enabled == true else {
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
			if let selection = localFrozenSelectionRect().map(pixelAlignedSelectionRect) {
				drawFrozenDisplaySurface(in: context)
				let toolbarScrimExclusionPath = frozenToolbarScrimExclusionPath(for: selection)
				CaptureHostFrozenSelectionChromeRenderer.drawSelectionScrim(
					for: selection,
					bounds: bounds,
					in: context,
					alpha: CaptureChrome.frozenScrimAlpha,
					excluding: toolbarScrimExclusionPath
				)
				CaptureHostFrozenSelectionChromeRenderer.drawDashedSelectionBorder(
					around: selection,
					in: context,
					lineWidth: CaptureChrome.frozenDashedBorderWidth,
					pixelsPerPoint: window?.screen?.backingScaleFactor ?? 1
				)
				if chrome.frozenSelectionTransformAllowed {
					CaptureHostFrozenSelectionChromeRenderer.drawFrozenResizeHandles(
						for: selection,
						orientation: settings.frozenResizeHandleOrientation,
						in: context
					)
				}
				drawFrozenOverlays(for: selection, in: context)
				drawScrollCaptureMinimap(for: selection, in: context)
				CaptureHostFrozenSelectionChromeRenderer.drawSelectionSizeBadge(
					for: selection,
					text: selectionSizeText(for: selection),
					bounds: bounds,
					avoiding: toolbarLayout(for: selection)?.frame,
					in: context
				)
				drawFrozenToolbar(for: selection, in: context)
			}
			scheduleFrozenFirstFrameInstallCompletionIfNeeded()
		}

	}

	private func pixelAlignedSelectionRect(_ rect: CGRect) -> CGRect {
		let scale = max(window?.screen?.backingScaleFactor ?? 1, 1)
		let minX = floor(rect.minX * scale) / scale
		let minY = floor(rect.minY * scale) / scale
		let maxX = ceil(rect.maxX * scale) / scale
		let maxY = ceil(rect.maxY * scale) / scale
		return CGRect(
			x: minX,
			y: minY,
			width: max(0, maxX - minX),
			height: max(0, maxY - minY)
		)
	}

	private func scheduleFrozenFirstFrameInstallCompletionIfNeeded() {
		guard frozenFirstDisplayHandoff.queueCompletionIfNeeded() else {
			return
		}
		DispatchQueue.main.async { [weak self] in
			self?.finishFrozenFirstFrameInstall()
		}
	}

	private func drawFrozenDisplaySurface(in context: CGContext) {
		guard scene.mode == .frozen else {
			return
		}
		guard let frame = localFrozenDisplayFrame(), let image = chrome.frozenDisplayImage else {
			return
		}

		context.saveGState()
		context.interpolationQuality = .high
		context.clip(to: bounds)
		context.draw(image, in: frame)
		context.restoreGState()
	}

	private func drawScrollCaptureMinimap(for selection: CGRect, in context: CGContext) {
		guard let preview = chrome.scrollMinimapPreview else {
			return
		}
		let theme = chromeTheme()
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		CaptureHostScrollMinimapRenderer.render(
			preview: preview,
			selection: selection,
			bounds: bounds,
			palette: palette,
			in: context
		)
	}

	private func localFrozenDisplayFrame() -> CGRect? {
		localRect(from: chrome.frozenDisplayFrame)
	}

	private func currentImmediateLiveDragSelectionLocal() -> CGRect? {
		guard scene.mode == .live, let window else {
			return nil
		}
		guard
			let globalRect = livePrimaryInteraction.immediateDragSelectionGlobal(
				current: livePointerPreviewGlobal ?? scene.pointer,
				in: window.frame
			)
		else {
			return nil
		}
		return localRect(from: globalRect)
	}

	private func liveDragDistance(from point: CGPoint) -> CGFloat {
		livePrimaryInteraction.dragDistance(from: point)
	}

	private func localPointer() -> CGPoint? {
		guard let globalPoint = livePointerPreviewGlobal ?? scene.pointer else {
			return nil
		}
		return localPoint(from: globalPoint)
	}

	private func seedLivePointerPreview(
		_ globalPoint: CGPoint?,
		recordsInputLatency: Bool = true
	) {
		guard let globalPoint else {
			resetLivePointerPreview()
			return
		}
		livePointerPreviewGlobal = globalPoint
		if recordsInputLatency {
			livePointerPreviewInputUptime = ProcessInfo.processInfo.systemUptime
			livePointerPreviewInputSequence &+= 1
		} else {
			livePointerPreviewInputUptime = nil
			livePointerPreviewInputSequence = 0
		}
	}

	@discardableResult
	private func setLivePointerPreview(
		to globalPoint: CGPoint,
		recordsInputLatency: Bool = true
	) -> Bool {
		if let current = livePointerPreviewGlobal,
			hypot(current.x - globalPoint.x, current.y - globalPoint.y) < 0.05
		{
			return false
		}
		seedLivePointerPreview(globalPoint, recordsInputLatency: recordsInputLatency)
		return true
	}

	private func resetLivePointerPreview() {
		emitLiveChromeInputSummary(reason: "reset")
		resetLiveChromeInputTelemetry()
		livePointerPreviewGlobal = nil
		livePointerPreviewInputUptime = nil
		livePointerPreviewInputSequence = 0
		lastLivePointerEventUptime = nil
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
	private func recoverReleasedLivePrimaryInteractionIfNeeded(at point: CGPoint) -> Bool {
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
	private func recoverReleasedFrozenInteractionIfNeeded(at point: CGPoint) -> Bool {
		guard
			scene.mode == .frozen,
			controller?.hasFrozenOverlayActiveInteraction == true,
			!isPrimaryMouseButtonPressed()
		else {
			return false
		}
		cancelFrozenMouseReleaseWatchdog()
		controller?.completeFrozenInteraction(at: point)
		syncVisibleCursor()
		return true
	}

	private func liveDragCompletionPoint(for point: CGPoint) -> CGPoint {
		livePrimaryInteraction.completionPoint(for: point)
	}

	private func isPrimaryMouseButtonPressed() -> Bool {
		(NSEvent.pressedMouseButtons & 1) == 1
	}

	func clearLivePrimaryInteractionState(rendersImmediately: Bool) {
		cancelQueuedPointerDispatch()
		livePrimaryInteraction.reset()
		removeLiveMouseUpMonitor()
		if rendersImmediately, scene.mode == .live {
			liveRenderer.renderNow()
		}
	}

	private func installLiveMouseUpMonitor() {
		removeLiveMouseUpMonitor()
		liveMouseUpMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseUp]) {
			[weak self] event in
			self?.completeLivePrimaryInteractionFromMouseUp(event)
			return event
		}
	}

	private func removeLiveMouseUpMonitor() {
		cancelLiveMouseReleaseWatchdog()
		if let liveMouseUpMonitor {
			NSEvent.removeMonitor(liveMouseUpMonitor)
			self.liveMouseUpMonitor = nil
		}
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

	private func installLiveMouseReleaseWatchdog() {
		cancelLiveMouseReleaseWatchdog()
		scheduleLiveMouseReleaseWatchdog()
	}

	private func scheduleLiveMouseReleaseWatchdog() {
		let workItem = DispatchWorkItem { [weak self] in
			self?.pollLiveMouseReleaseWatchdog()
		}
		liveMouseReleaseWatchdog = workItem
		DispatchQueue.main.asyncAfter(
			deadline: .now()
				+ NativeHostDisplayRefresh.frameInterval(
					forTargetFramesPerSecond: NativeHostDisplayRefresh.maximumTargetFramesPerSecond),
			execute: workItem
		)
	}

	private func pollLiveMouseReleaseWatchdog() {
		liveMouseReleaseWatchdog = nil
		guard
			scene.mode == .live,
			livePrimaryInteraction.canCompleteInteraction
		else {
			return
		}
		if isPrimaryMouseButtonPressed() == false {
			let point = NSEvent.mouseLocation
			logLivePrimaryInputEvent("capture.live_primary_release_watchdog", point: point)
			completeLivePrimaryInteractionFromSystemMouseUp(at: point, source: "watchdog")
			return
		}
		scheduleLiveMouseReleaseWatchdog()
	}

	private func installFrozenMouseReleaseWatchdog() {
		cancelFrozenMouseReleaseWatchdog()
		scheduleFrozenMouseReleaseWatchdog()
	}

	private func scheduleFrozenMouseReleaseWatchdog() {
		let workItem = DispatchWorkItem { [weak self] in
			self?.pollFrozenMouseReleaseWatchdog()
		}
		frozenMouseReleaseWatchdog = workItem
		DispatchQueue.main.asyncAfter(
			deadline: .now()
				+ NativeHostDisplayRefresh.frameInterval(
					forTargetFramesPerSecond: NativeHostDisplayRefresh.maximumTargetFramesPerSecond),
			execute: workItem
		)
	}

	private func pollFrozenMouseReleaseWatchdog() {
		frozenMouseReleaseWatchdog = nil
		guard
			scene.mode == .frozen,
			controller?.hasFrozenOverlayActiveInteraction == true
		else {
			return
		}
		if isPrimaryMouseButtonPressed() == false {
			let point = currentGlobalMousePoint() ?? NSEvent.mouseLocation
			NativeHostTelemetry.captureEvent(
				"capture.frozen_primary_release_watchdog",
				captureID: controller?.activeTelemetryCaptureID ?? 0,
				detail: "x=\(Int(point.x.rounded())) y=\(Int(point.y.rounded()))"
			)
			controller?.completeFrozenInteraction(at: point)
			syncVisibleCursor()
			return
		}
		scheduleFrozenMouseReleaseWatchdog()
	}

	private func logLivePrimaryInputEvent(
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

	private func cancelLiveMouseReleaseWatchdog() {
		liveMouseReleaseWatchdog?.cancel()
		liveMouseReleaseWatchdog = nil
	}

	private func cancelFrozenMouseReleaseWatchdog() {
		frozenMouseReleaseWatchdog?.cancel()
		frozenMouseReleaseWatchdog = nil
	}

	private func cancelQueuedPointerDispatch() {
		pointerDispatchQueue.cancel()
	}

	private func updateLivePointerPreview(
		to globalPoint: CGPoint,
		rendersImmediately: Bool,
		rendersFullPreview: Bool = false
	) {
		guard scene.mode == .live else {
			return
		}
		recordLivePointerEventGap()
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

	private func recordLivePointerEventGap() {
		let now = ProcessInfo.processInfo.systemUptime
		if let lastLivePointerEventUptime {
			let gapMilliseconds = (now - lastLivePointerEventUptime) * 1_000
			if gapMilliseconds >= 0, gapMilliseconds < 250 {
				livePointerEventGapMetric.record(gapMilliseconds)
			}
		}
		lastLivePointerEventUptime = now
	}

	func finishLivePresentationTelemetry(reason: String) {
		emitLiveChromeInputSummary(reason: reason)
	}

	private func resetLiveChromeInputTelemetry() {
		liveChromeMouseEventCount = 0
		didEmitLiveChromeInputSummary = false
	}

	private func emitLiveChromeInputSummary(reason: String) {
		guard didEmitLiveChromeInputSummary == false else {
			return
		}
		let observedMouseEvents = max(
			liveChromeMouseEventCount,
			Int(min(livePointerPreviewInputSequence, UInt64(Int.max)))
		)
		guard observedMouseEvents > 0 else {
			return
		}
		didEmitLiveChromeInputSummary = true
		NativeHostTelemetry.liveChromeInputSummary(
			captureID: controller?.activeTelemetryCaptureID ?? 0,
			reason: reason,
			mouseEvents: observedMouseEvents,
			followTicks: 0,
			fastMoveAttempts: 0,
			fastMoveSuccesses: 0,
			loupeFastMoveAttempts: 0,
			loupeFastMoveSuccesses: 0,
			predictedMoves: 0,
			fallbackRefreshes: 0,
			immediateRefreshes: 0
		)
	}

	private func localFrozenSelectionRect() -> CGRect? {
		localRect(from: chrome.frozenSelectionSnapshot ?? scene.frozenSelection)
	}

	private func localRect(from globalRect: CGRect?) -> CGRect? {
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

	private func globalRect(from localRect: CGRect) -> CGRect? {
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

	private func localPoint(from globalPoint: CGPoint) -> CGPoint? {
		guard let window else {
			return nil
		}
		return captureOverlayLocalPoint(
			from: globalPoint,
			windowFrame: window.frame,
			bounds: bounds
		)
	}

	private func currentLocalMousePoint() -> CGPoint? {
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
				let selectedModeTool = visibleToolbarItems().first(where: { $0.selected })?.kind
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

	private func globalPoint(from event: NSEvent) -> CGPoint {
		guard let window else {
			return NSEvent.mouseLocation
		}
		return window.convertPoint(toScreen: event.locationInWindow)
	}

	private func currentGlobalMousePoint() -> CGPoint? {
		guard let window else {
			return NSEvent.mouseLocation
		}
		let localPoint = window.mouseLocationOutsideOfEventStream
		let globalPoint = window.convertPoint(toScreen: localPoint)
		return NSScreen.screens.contains(where: { $0.frame.inclusivelyContains(globalPoint) })
			? globalPoint : nil
	}

	private func drawFrozenOverlays(for selection: CGRect, in context: CGContext) {
		guard let window else {
			return
		}
		CaptureHostFrozenOverlayRenderer.render(
			selection: selection,
			chrome: chrome,
			windowFrame: window.frame,
			bounds: bounds,
			in: context
		)
	}

	private func toolbarLayout(for selection: CGRect) -> FrozenToolbarLayout? {
		FrozenToolbarLayoutPlanner.layout(
			selection: selection,
			bounds: bounds,
			prefersTopPlacement: settings.toolbarPlacement == .top,
			items: visibleToolbarItems(),
			annotationStyle: chrome.annotationStyle
		)
	}

	private func frozenToolbarScrimExclusionPath(for selection: CGRect) -> CGPath? {
		guard settings.usesLiquidHudGlass,
			let toolbarFrame = toolbarLayout(for: selection)?.frame
		else {
			return nil
		}
		guard
			chrome.scrollMinimapPreview != nil
				|| (frozenToolbarLiquidGlassVisible && frozenToolbarLiquidGlassContentDrawn)
		else {
			return nil
		}
		let visibleSelection = selection.intersection(bounds)
		if visibleSelection.isNull == false, toolbarFrame.intersects(visibleSelection) {
			return nil
		}
		return CGPath(
			roundedRect: toolbarFrame,
			cornerWidth: CaptureChrome.hudCornerRadius,
			cornerHeight: CaptureChrome.hudCornerRadius,
			transform: nil
		)
	}

	private func frozenToolbarVisibleForContract() -> Bool {
		guard scene.mode == .frozen,
			let selection = localFrozenSelectionRect(),
			toolbarLayout(for: selection) != nil
		else {
			return false
		}
		if settings.usesLiquidHudGlass {
			return frozenToolbarLiquidGlassVisible && frozenToolbarLiquidGlassContentDrawn
		}
		return true
	}

	private func visibleToolbarItems() -> [ToolbarItem] {
		FrozenToolbarLayoutPlanner.visibleItems(
			from: scene.toolbarItems,
			availability: FrozenToolbarAvailability(
				scrollCaptureActive: chrome.scrollMinimapPreview != nil,
				canUndo: chrome.frozenOverlay.canUndo,
				canRedo: chrome.frozenOverlay.canRedo,
				frozenSelectionAvailable: scene.frozenSelection != nil,
				keepsFrozenSelectionFixed: chrome.frozenOverlay.keepsFrozenSelectionFixed,
				scrollToolbarEnabled: controller?.scrollCaptureToolbarEnabled ?? false,
				hasRecognizeTextBlockingEdits: chrome.frozenOverlay.hasRecognizeTextBlockingEdits
			)
		)
	}

	private func toolbarItem(_ kind: ToolbarItemKind) -> ToolbarItem? {
		visibleToolbarItems().first(where: { $0.kind == kind })
	}

	private func toolbarAction(at point: CGPoint) -> ToolbarItemKind? {
		frozenToolbarHitState(at: point).toolbarAction
	}

	private func annotationStyleAction(at point: CGPoint) -> FrozenAnnotationStyleAction? {
		frozenToolbarHitState(at: point).annotationStyleAction
	}

	private func annotationStyleSizeControlContains(_ point: CGPoint) -> Bool {
		guard scene.mode == .frozen, let selection = localFrozenSelectionRect(),
			let styleLayout = toolbarLayout(for: selection)?.annotationStyle
		else {
			return false
		}
		return styleLayout.sizeControlFrame.contains(point)
	}

	private func toolbarFrameContains(_ point: CGPoint) -> Bool {
		frozenToolbarHitState(at: point).pointerOverToolbar
	}

	private func performToolbarAction(_ action: ToolbarItemKind) {
		switch action {
		case .undo:
			controller?.performFrozenUndo()
		case .redo:
			controller?.performFrozenRedo()
		case .autoCenter:
			controller?.performFrozenAutoCenter()
		default:
			controller?.invokeToolbarItem(action)
		}
	}

	private func performAnnotationStyleAction(_ action: FrozenAnnotationStyleAction) {
		controller?.performFrozenAnnotationStyleAction(action)
	}

	private func frozenToolbarHitState(at point: CGPoint) -> FrozenToolbarHitState {
		guard scene.mode == .frozen, let selection = localFrozenSelectionRect() else {
			return FrozenToolbarHitState(
				pointerOverToolbar: false,
				toolbarAction: nil,
				annotationStyleAction: nil
			)
		}
		return FrozenToolbarLayoutPlanner.hitState(at: point, in: toolbarLayout(for: selection))
	}

	private func clearHoveredToolbarAction() {
		guard toolbarHoverState.clear() else {
			return
		}
	}

	private func refreshHoveredToolbarAction(for localPoint: CGPoint? = nil) {
		let probePoint = scene.mode == .frozen ? (localPoint ?? currentLocalMousePoint()) : nil
		let hitState: FrozenToolbarHitState
		if let probePoint {
			hitState = frozenToolbarHitState(at: probePoint)
		} else {
			hitState = FrozenToolbarHitState(
				pointerOverToolbar: false,
				toolbarAction: nil,
				annotationStyleAction: nil
			)
		}
		if toolbarHoverState.update(to: hitState) {
			syncVisibleCursor()
			updateChromeMaterialViews()
			needsDisplay = true
		}
	}

	private func drawFrozenToolbar(for selection: CGRect, in context: CGContext) {
		guard
			!settings.usesLiquidHudGlass || !frozenToolbarLiquidGlassVisible
				|| !frozenToolbarLiquidGlassContentDrawn
		else {
			return
		}
		guard let layout = toolbarLayout(for: selection) else {
			return
		}
		let theme = chromeTheme()
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		drawPill(
			in: layout.frame,
			context: context,
			theme: theme,
			strongShadow: false,
			surfaceKind: .toolbar,
			allowsClassicGlass: frozenFirstDisplayHandoff.allowsClassicToolbarGlass
		)
		FrozenToolbarDrawing.drawToolbarContent(
			items: layout.items,
			hoveredToolbarAction: toolbarHoverState.toolbarAction,
			toolbarScale: layout.scale,
			annotationStyleState: chrome.annotationStyle,
			annotationStyleLayout: layout.annotationStyle,
			hoveredAnnotationStyleAction: toolbarHoverState.annotationStyleAction,
			palette: palette,
			in: context
		)
	}

	private func syncVisibleCursor() {
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

	private func currentHudPlacement() -> LiveChromeFloatingPlacement? {
		guard scene.mode == .live, let anchor = localPointer() else {
			return nil
		}
		return LiveChromePlacementPlanner.hudPlacement(
			bounds: bounds,
			anchor: anchor,
			positionDisplay: currentPositionDisplay(),
			keycapVisible: settings.showAltHintKeycap
		)
	}

	private func currentLoupeFrame(
		hudFrame: CGRect,
		patch: CGImage?,
		alignTrailing: Bool
	) -> CGRect? {
		LiveChromePlacementPlanner.loupeFrame(
			bounds: bounds,
			hudFrame: hudFrame,
			patch: patch,
			alignTrailing: alignTrailing
		)
	}

	private func currentLoupeFrame(hudFrame: CGRect) -> CGRect? {
		currentLoupeFrame(
			hudFrame: hudFrame,
			patch: reusableLiveLoupePatch(),
			alignTrailing: currentHudPlacement()?.flippedHorizontally ?? false
		)
	}

	private func currentRendererPreviewSnapshot() -> LivePreviewSnapshot? {
		if scene.mode == .live {
			let snapshot: LivePreviewSnapshot?
			if chrome.hostLocalFrozenSelecting {
				snapshot =
					currentHostLocalFrozenSelectingPreviewSnapshot()
					?? lastLivePreviewSnapshot
					?? currentLivePreviewSnapshot(usesSceneDragPreview: false)
			} else {
				snapshot = currentLivePreviewSnapshot()
			}
			lastLivePreviewSnapshot = snapshot
			return snapshot
		}
		if frozenFirstDisplayHandoff.pending {
			return currentPendingFrozenPreviewSnapshot() ?? lastLivePreviewSnapshot
		}
		return nil
	}

	private func currentHostLocalFrozenSelectingPreviewSnapshot() -> LivePreviewSnapshot? {
		guard scene.mode == .live, chrome.hostLocalFrozenSelecting else {
			return nil
		}

		guard let dragSelectionLocal = currentImmediateLiveDragSelectionLocal() else {
			return nil
		}
		let rgbSample = liveSampleCache.rgbSample(
			matching: livePointerPreviewGlobal ?? scene.pointer)?
			.rgb
		return LivePreviewSnapshot(
			bounds: bounds,
			theme: chromeTheme(),
			settings: settings,
			frozenPending: false,
			frozenDisplayFrame: localFrozenDisplayFrame(),
			frozenDisplayImage: chrome.frozenDisplayImage,
			pointerLocal: nil,
			dragSelectionLocal: dragSelectionLocal,
			hoverSelectionLocal: nil,
			selectionSizeText: selectionSizeText(for: dragSelectionLocal),
			hudFrame: nil,
			loupeFrame: nil,
			positionDisplay: currentPositionDisplay(),
			colorDisplay: currentLiveColorDisplay(for: rgbSample),
			rgbSample: rgbSample,
			keycapVisible: false,
			inputUptime: nil,
			loupePatch: nil,
			glassPatches: [:]
		)
	}

	private func currentPendingFrozenPreviewSnapshot() -> LivePreviewSnapshot? {
		guard frozenFirstDisplayHandoff.pending else {
			return nil
		}
		let frozenSelectionLocal =
			localFrozenSelectionRect()
			?? lastLivePreviewSnapshot?.dragSelectionLocal
			?? lastLivePreviewSnapshot?.hoverSelectionLocal
		guard let frozenSelectionLocal else {
			return nil
		}
		return LivePreviewSnapshot(
			bounds: bounds,
			theme: chromeTheme(),
			settings: settings,
			frozenPending: true,
			frozenDisplayFrame: localFrozenDisplayFrame(),
			frozenDisplayImage: chrome.frozenDisplayImage,
			pointerLocal: nil,
			dragSelectionLocal: frozenSelectionLocal,
			hoverSelectionLocal: nil,
			selectionSizeText: nil,
			hudFrame: nil,
			loupeFrame: nil,
			positionDisplay: currentPositionDisplay(),
			colorDisplay: currentLiveColorDisplay(for: liveSampleCache.latestRgb?.rgb),
			rgbSample: liveSampleCache.latestRgb?.rgb,
			keycapVisible: false,
			inputUptime: nil,
			loupePatch: nil,
			glassPatches: [:]
		)
	}

	private func currentLivePreviewSnapshot(
		usesSceneDragPreview: Bool = true
	) -> LivePreviewSnapshot? {
		guard scene.mode == .live else {
			return nil
		}

		if livePrimaryInteraction.completionInFlight == false {
			let polledPoint = currentGlobalMousePoint() ?? NSEvent.mouseLocation
			if let currentPreview = livePointerPreviewGlobal {
				if hypot(currentPreview.x - polledPoint.x, currentPreview.y - polledPoint.y)
					>= 0.5
				{
					applyPolledLivePointerPreview(polledPoint)
				}
			} else {
				applyPolledLivePointerPreview(polledPoint, recordsInputLatency: false)
			}
		}

		refreshLiveHighlightedWindowPreview(at: livePointerPreviewGlobal ?? scene.pointer)
		updateLivePreviewDemands()

		let point = livePointerPreviewGlobal ?? scene.pointer
		let chromeSample = currentLiveChromeSample(at: point)
		let rgbSample = liveRgbSample(from: chromeSample, at: point)
		let loupePatch = scene.loupeVisible ? chromeSample?.loupePatch : nil
		let dragSelectionLocal =
			currentImmediateLiveDragSelectionLocal()
			?? (usesSceneDragPreview && livePrimaryInteraction.hasInteraction
				&& livePrimaryInteraction.dragExceededThreshold
				? localRect(from: scene.liveSelectionPreview) : nil)
		let hoverSelectionLocal =
			dragSelectionLocal == nil
			? localRect(from: liveHighlightedWindowPreview?.frame)
			: nil
		let positionDisplay = currentPositionDisplay()
		let colorDisplay = currentLiveColorDisplay(for: rgbSample)
		let hudPlacement =
			livePrimaryInteraction.hoverChromeSuppressed ? nil : currentHudPlacement()
		let hudFrame = hudPlacement?.frame
		let loupeFrame =
			!livePrimaryInteraction.hoverChromeSuppressed && scene.loupeVisible
			? hudPlacement.flatMap {
				currentLoupeFrame(
					hudFrame: $0.frame,
					patch: chromeSample?.loupePatch,
					alignTrailing: $0.flippedHorizontally
				)
			}
			: nil
		updateLiveLiquidGlassViews(hudFrame: hudFrame, loupeFrame: loupeFrame)

		return LivePreviewSnapshot(
			bounds: bounds,
			theme: chromeTheme(),
			settings: settings,
			frozenPending: false,
			frozenDisplayFrame: nil,
			frozenDisplayImage: nil,
			pointerLocal: localPointer(),
			dragSelectionLocal: dragSelectionLocal,
			hoverSelectionLocal: hoverSelectionLocal,
			selectionSizeText: dragSelectionLocal.map(selectionSizeText(for:)),
			hudFrame: hudFrame,
			loupeFrame: loupeFrame,
			positionDisplay: positionDisplay,
			colorDisplay: colorDisplay,
			rgbSample: rgbSample,
			keycapVisible: settings.showAltHintKeycap,
			inputUptime: sampleUpdatedLiveChromeRenderInProgress
				? nil : livePointerPreviewInputUptime,
			loupePatch: loupePatch,
			glassPatches: [:]
		)
	}

	private func applyPolledLivePointerPreview(
		_ globalPoint: CGPoint,
		recordsInputLatency: Bool = true
	) {
		_ = setLivePointerPreview(
			to: globalPoint,
			recordsInputLatency: recordsInputLatency
		)
	}

	private func refreshLiveHighlightedWindowPreview(at globalPoint: CGPoint?) {
		guard let globalPoint else {
			liveHighlightedWindowPreview = nil
			return
		}
		liveHighlightedWindowPreview = controller?.previewHighlightedWindow(at: globalPoint)
	}

	private func refreshLiveHighlightedWindowPreviewForFastPath(at globalPoint: CGPoint) -> Bool {
		guard
			livePrimaryInteraction.hasInteraction == false,
			livePrimaryInteraction.hoverChromeSuppressed == false
		else {
			return false
		}
		let previousPreview = liveHighlightedWindowPreview
		refreshLiveHighlightedWindowPreview(at: globalPoint)
		return Self.windowSnapshotsEquivalent(previousPreview, liveHighlightedWindowPreview)
			== false
	}

	private static func windowSnapshotsEquivalent(_ lhs: WindowSnapshot?, _ rhs: WindowSnapshot?)
		-> Bool
	{
		switch (lhs, rhs) {
		case (nil, nil):
			return true
		case (let lhs?, let rhs?):
			return lhs.windowID == rhs.windowID && windowFramesEquivalent(lhs.frame, rhs.frame)
		default:
			return false
		}
	}

	private static func windowFramesEquivalent(_ lhs: CGRect, _ rhs: CGRect) -> Bool {
		abs(lhs.minX - rhs.minX) <= 0.5
			&& abs(lhs.minY - rhs.minY) <= 0.5
			&& abs(lhs.width - rhs.width) <= 0.5
			&& abs(lhs.height - rhs.height) <= 0.5
	}

	private func updateLiveChromeBackdrops() {
		let frames = currentLiveChromeLayerFrames()
		updateLiveChromeBackdrops(hudFrame: frames.hud, loupeFrame: frames.loupe)
	}

	private func updateLiveChromeBackdrops(hudFrame: CGRect?, loupeFrame: CGRect?) {
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

	private func moveLiveChromeLayers() {
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

	private func updateLivePreviewDemands() {
		guard scene.mode == .live else {
			controller?.updateLivePreviewDemand(
				point: nil, settings: settings, includeLoupePatch: false)
			controller?.updateLiveChromeBackdrops(nil)
			return
		}
		updateLivePreviewSampleDemand()
		updateLiveChromeBackdrops()
	}

	private func updateLivePreviewSampleDemand() {
		guard scene.mode == .live else {
			controller?.updateLivePreviewDemand(
				point: nil, settings: settings, includeLoupePatch: false)
			return
		}
		let point = livePointerPreviewGlobal ?? scene.pointer
		controller?.updateLivePreviewDemand(
			point: point,
			settings: settings,
			includeLoupePatch: scene.loupeVisible && !livePrimaryInteraction.hoverChromeSuppressed
		)
	}

	private func currentDisplayID() -> CGDirectDisplayID? {
		(window?.screen?.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
			.uint32Value
	}

	private func currentDisplayTargetFramesPerSecond() -> Int {
		NativeHostDisplayRefresh.targetFramesPerSecond(for: window?.screen)
	}

	private func currentPointerFollowFramesPerSecond() -> Int {
		NativeHostDisplayRefresh.pointerFollowFramesPerSecond(for: window?.screen)
	}

	private func currentLiveChromeSample(at point: CGPoint?) -> LiveChromeSample? {
		CaptureHostLiveSampleResolver.currentLiveChromeSample(
			at: point,
			scenePointer: scene.pointer,
			loupeVisible: scene.loupeVisible,
			hoverChromeSuppressed: livePrimaryInteraction.hoverChromeSuppressed,
			settings: settings,
			chrome: chrome,
			cache: &liveSampleCache
		) { wantsLoupePatch in
			controller?.liveChromeSnapshot(
				point: point,
				settings: settings,
				includeLoupePatch: wantsLoupePatch
			)
		}
	}

	private func reusableLiveLoupePatch() -> CGImage? {
		CaptureHostLiveSampleResolver.reusableLiveLoupePatch(
			cache: liveSampleCache,
			chrome: chrome,
			settings: settings
		)
	}

	private func liveRgbSample(from sample: LiveChromeSample?, at point: CGPoint?) -> RGBSample? {
		CaptureHostLiveSampleResolver.liveRgbSample(
			from: sample,
			at: point,
			cache: &liveSampleCache
		)
	}

	private func seedLiveChromeSampleCache(from chrome: CaptureChromeState, point: CGPoint?) {
		CaptureHostLiveSampleResolver.seedChromeSample(
			from: chrome,
			point: point,
			cache: &liveSampleCache
		)
	}

	private func selectionSizeText(for rect: CGRect) -> String {
		let scale = window?.screen?.backingScaleFactor ?? 1
		let sizeText = "\(Int(round(rect.width * scale)))x\(Int(round(rect.height * scale)))px"

		if abs(scale - 1) <= 0.005 {
			return sizeText
		}

		return "\(sizeText) @\(String(format: "%g", Double(scale)))x"
	}

	private func currentPositionDisplay() -> LivePositionDisplay {
		let metrics = LiveChromePlacementPlanner.metrics
		guard let pointer = livePointerPreviewGlobal ?? scene.pointer else {
			return LivePositionDisplay(
				xValueText: "?",
				yValueText: "?",
				xSlotWidth: metrics.placeholderXSlotWidth,
				ySlotWidth: metrics.placeholderYSlotWidth
			)
		}
		let xValueText = String(Int(pointer.x.rounded()))
		let yValueText = String(Int(pointer.y.rounded()))
		return LivePositionDisplay(
			xValueText: xValueText,
			yValueText: yValueText,
			xSlotWidth: metrics.coordinateSlotWidth(
				prefixWidth: metrics.xPrefixWidth,
				valueText: xValueText
			),
			ySlotWidth: metrics.coordinateSlotWidth(
				prefixWidth: metrics.yPrefixWidth,
				valueText: yValueText
			)
		)
	}

	private func currentLiveColorDisplay(for sample: RGBSample?) -> LiveColorDisplay {
		let hexText =
			sample.map { String(format: "#%02X%02X%02X", $0.r, $0.g, $0.b) }
			?? pendingLiveColorHexText()
		return LiveColorDisplay(
			hexText: hexText,
			hexSlotWidth: LiveChromePlacementPlanner.metrics.hexSlotWidth,
			isPending: sample == nil
		)
	}

	private func pendingLiveColorHexText() -> String {
		LiveChromePendingColorText.hexText()
	}

	private func drawPill(
		in frame: CGRect,
		context: CGContext,
		theme: CaptureChromeTheme,
		strongShadow: Bool,
		surfaceKind: GlassSurfaceKind,
		allowsLiquidGlassClearFill: Bool = true,
		allowsClassicGlass: Bool = true
	) {
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let pillPath = NSBezierPath(
			roundedRect: frame,
			xRadius: CaptureChrome.hudCornerRadius,
			yRadius: CaptureChrome.hudCornerRadius
		)
		let glassImage =
			settings.usesClassicHudGlass && allowsClassicGlass
			? glassPatch(for: surfaceKind, frame: frame) : nil
		let hasGlass = glassImage != nil
		context.saveGState()
		if strongShadow {
			context.setShadow(offset: .zero, blur: 10, color: palette.shadow.cgColor)
		}
		if hasGlass,
			let clipPath = pillPath.copy() as? NSBezierPath,
			let glassImage
		{
			clipPath.addClip()
			context.saveGState()
			context.setAlpha(CGFloat(CaptureChrome.glassOpacity(settings: settings)))
			context.draw(glassImage, in: frame)
			context.restoreGState()
		}
		let usesLiquidGlass = allowsLiquidGlassClearFill && settings.usesLiquidHudGlass
		let fillColor =
			usesLiquidGlass
			? NSColor.clear
			: CaptureChrome.effectiveBodyFill(
				palette: palette,
				settings: settings,
				hasGlass: hasGlass
			)
		context.setFillColor(fillColor.cgColor)
		pillPath.fill()
		context.restoreGState()

		context.setStrokeColor(palette.outerStroke.cgColor)
		context.setLineWidth(1)
		pillPath.stroke()
	}

	private func glassPatch(for surfaceKind: GlassSurfaceKind, frame: CGRect) -> CGImage? {
		let now = ProcessInfo.processInfo.systemUptime
		if let cached = glassPatchCache[surfaceKind],
			now - cached.capturedAt < glassPatchCacheInterval(),
			abs(cached.frame.minX - frame.minX) < 1,
			abs(cached.frame.minY - frame.minY) < 1,
			abs(cached.frame.width - frame.width) < 1,
			abs(cached.frame.height - frame.height) < 1
		{
			return cached.image
		}

		guard let globalFrame = globalRect(from: frame) else {
			return nil
		}
		guard let patch = glassSourcePatch(in: globalFrame) else {
			return nil
		}
		guard let image = blurredGlassPatch(from: patch, surfaceKind: surfaceKind) else {
			return nil
		}

		glassPatchCache[surfaceKind] = CaptureHostGlassPatchCache(
			frame: frame,
			capturedAt: now,
			image: image
		)
		return image
	}

	private func glassPatchCacheInterval() -> TimeInterval {
		NativeHostDisplayRefresh.frameInterval(
			forTargetFramesPerSecond: currentDisplayTargetFramesPerSecond())
	}

	private func glassSourcePatch(in globalFrame: CGRect) -> CGImage? {
		switch scene.mode {
		case .live:
			return controller?.backgroundPatch(in: globalFrame)
		case .frozen:
			if chrome.scrollMinimapPreview != nil {
				return controller?.backgroundPatch(in: globalFrame)
					?? frozenDisplayPatch(in: globalFrame)
			}
			return frozenDisplayPatch(in: globalFrame)
		case .hidden:
			return nil
		}
	}

	private func frozenDisplayPatch(in globalFrame: CGRect) -> CGImage? {
		frozenDisplayPatch(
			in: globalFrame,
			displayFrame: chrome.frozenDisplayFrame,
			image: chrome.frozenDisplayImage
		)
	}

	private func scrollToolbarBackdropSeedPatch(in globalFrame: CGRect) -> CGImage? {
		if let cached = scrollToolbarBackdropState.cachedSeedPatch(matching: globalFrame) {
			return cached
		}
		guard
			let image = frozenDisplayPatch(
				in: globalFrame,
				displayFrame: scrollToolbarBackdropState.seedFrame,
				image: scrollToolbarBackdropState.seedImage
			)
		else {
			return nil
		}
		scrollToolbarBackdropState.storeSeedPatch(
			frame: globalFrame,
			capturedAt: ProcessInfo.processInfo.systemUptime,
			image: image
		)
		return image
	}

	private func frozenDisplayPatch(
		in globalFrame: CGRect,
		displayFrame: CGRect?,
		image: CGImage?
	) -> CGImage? {
		guard
			let displayFrame,
			let image
		else {
			return nil
		}
		let cropRect = CGRect(
			x: ((globalFrame.minX - displayFrame.minX) / max(displayFrame.width, 1))
				* CGFloat(image.width),
			y: ((displayFrame.maxY - globalFrame.maxY) / max(displayFrame.height, 1))
				* CGFloat(image.height),
			width: (globalFrame.width / max(displayFrame.width, 1)) * CGFloat(image.width),
			height: (globalFrame.height / max(displayFrame.height, 1)) * CGFloat(image.height)
		).integral.intersection(CGRect(x: 0, y: 0, width: image.width, height: image.height))
		guard cropRect.width > 0, cropRect.height > 0 else {
			return nil
		}
		return image.cropping(to: cropRect)
	}

	private func blurredGlassPatch(from image: CGImage, surfaceKind: GlassSurfaceKind) -> CGImage? {
		let ciImage = CIImage(cgImage: image)
		let clampedImage = ciImage.clampedToExtent()
		guard let filter = CIFilter(name: "CIGaussianBlur") else {
			return image
		}
		let blurAmount = CGFloat(settings.hudBlur.clamped(to: 0...1))
		let blurRadius: CGFloat =
			switch surfaceKind {
			case .hud, .loupe, .toolbar:
				14 + blurAmount * 32.0
			}
		filter.setValue(clampedImage, forKey: kCIInputImageKey)
		filter.setValue(blurRadius, forKey: kCIInputRadiusKey)
		guard let blurredImage = filter.outputImage?.cropped(to: ciImage.extent) else {
			return image
		}
		let colorAdjustedImage: CIImage
		if let colorControls = CIFilter(name: "CIColorControls") {
			colorControls.setValue(blurredImage, forKey: kCIInputImageKey)
			switch surfaceKind {
			case .hud, .loupe, .toolbar:
				colorControls.setValue(
					1.18 + settings.hudTint.clamped(to: 0...1) * 0.42, forKey: kCIInputSaturationKey
				)
				colorControls.setValue(1.04, forKey: kCIInputContrastKey)
				colorControls.setValue(themeBrightnessBias(), forKey: kCIInputBrightnessKey)
			}
			colorAdjustedImage =
				colorControls.outputImage?.cropped(to: ciImage.extent) ?? blurredImage
		} else {
			colorAdjustedImage = blurredImage
		}
		return frozenEffectCIContext.createCGImage(
			colorAdjustedImage, from: colorAdjustedImage.extent) ?? image
	}

	private func chromeTheme() -> CaptureChromeTheme {
		effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .aqua ? .light : .dark
	}

	private func configureChromeLiquidGlassView(_ view: NSView, zPosition: CGFloat) {
		view.isHidden = true
		view.wantsLayer = true
		view.layer?.cornerRadius = CaptureChrome.hudCornerRadius
		view.layer?.masksToBounds = true
		view.layer?.shadowOpacity = 0
		view.layer?.shadowPath = nil
		view.layer?.zPosition = zPosition
	}

	private func configureFrozenToolbarBackdropView(_ view: NSImageView) {
		view.isHidden = true
		view.imageScaling = .scaleAxesIndependently
		view.wantsLayer = true
		view.layer?.backgroundColor = NSColor.clear.cgColor
		view.layer?.cornerRadius = CaptureChrome.hudCornerRadius
		view.layer?.masksToBounds = true
		view.layer?.isOpaque = false
		view.layer?.zPosition = Self.frozenToolbarLiquidGlassBackdropZ
	}

	private func configureFrozenToolbarContentView(_ view: FrozenToolbarRenderView) {
		view.isHidden = true
		view.wantsLayer = true
		view.layer?.backgroundColor = NSColor.clear.cgColor
		view.layer?.isOpaque = false
		view.layer?.zPosition = Self.frozenToolbarContentZ
	}

	private func updateChromeMaterialViews() {
		if scene.mode != .live || !settings.usesLiquidHudGlass || chrome.hostLocalFrozenSelecting {
			hideLiveLiquidGlassViews(removing: false)
		}
		if scene.mode == .frozen {
			updateFrozenToolbarLiquidGlassView()
		} else if frozenToolbarLiquidGlassVisible {
			hideFrozenToolbarLiquidGlassView()
		} else if scene.mode == .live, settings.usesLiquidHudGlass {
			prewarmFrozenToolbarLiquidGlassViewIfNeeded()
		}
		if scene.mode == .live {
			updateLiveChromeBackdrops()
		} else {
			controller?.updateLiveChromeBackdrops(nil)
		}
	}

	private func updateLiveLiquidGlassViews(hudFrame: CGRect?, loupeFrame: CGRect?) {
		guard scene.mode == .live, settings.usesLiquidHudGlass, !chrome.hostLocalFrozenSelecting
		else {
			hideLiveLiquidGlassViews(removing: false)
			return
		}
		updateLiveLiquidGlassView(
			&hudLiquidGlassView,
			frame: hudFrame,
			zPosition: Self.liveChromeLiquidGlassZ
		)
		updateLiveLiquidGlassView(
			&loupeLiquidGlassView,
			frame: loupeFrame,
			zPosition: Self.liveChromeLiquidGlassZ
		)
	}

	private func moveExistingLiveLiquidGlassViews(hudFrame: CGRect?, loupeFrame: CGRect?) {
		guard scene.mode == .live, settings.usesLiquidHudGlass, !chrome.hostLocalFrozenSelecting
		else {
			hideLiveLiquidGlassViews(removing: false)
			return
		}
		moveExistingLiveLiquidGlassView(hudLiquidGlassView, frame: hudFrame)
		moveExistingLiveLiquidGlassView(loupeLiquidGlassView, frame: loupeFrame)
	}

	private func moveExistingLiveLiquidGlassView(_ view: NSView?, frame: CGRect?) {
		guard let view else {
			return
		}
		guard let frame else {
			view.isHidden = true
			return
		}
		if view.frame != frame {
			view.frame = frame
		}
		view.isHidden = false
	}

	private func updateLiveLiquidGlassView(
		_ view: inout NSView?,
		frame: CGRect?,
		zPosition: CGFloat
	) {
		guard let frame else {
			view?.isHidden = true
			return
		}
		if view == nil {
			guard let createdView = LiveChromeLiquidGlassBridge.makeGlassView() else {
				return
			}
			configureChromeLiquidGlassView(createdView, zPosition: zPosition)
			addSubview(createdView, positioned: .below, relativeTo: nil)
			view = createdView
		}
		guard let activeView = view else {
			return
		}
		activeView.layer?.zPosition = zPosition
		let settingsChanged = LiveChromeLiquidGlassBridge.update(activeView, settings: settings)
		let frameChanged = activeView.frame != frame
		let wasHidden = activeView.isHidden
		if frameChanged {
			activeView.frame = frame
		}
		if settingsChanged || frameChanged || wasHidden {
			activeView.needsLayout = true
			activeView.layoutSubtreeIfNeeded()
		}
		activeView.isHidden = false
	}

	private func prewarmFrozenToolbarLiquidGlassViewIfNeeded() {
		if let toolbarLiquidGlassView {
			LiveChromeLiquidGlassBridge.update(toolbarLiquidGlassView, settings: settings)
			ensureFrozenToolbarContentView(above: toolbarLiquidGlassView)
			return
		}
		guard let createdView = LiveChromeLiquidGlassBridge.makeGlassView() else {
			return
		}
		configureChromeLiquidGlassView(
			createdView,
			zPosition: Self.frozenToolbarLiquidGlassZ
		)
		LiveChromeLiquidGlassBridge.update(createdView, settings: settings)
		createdView.frame = .zero
		createdView.isHidden = true
		addSubview(createdView, positioned: .below, relativeTo: nil)
		toolbarLiquidGlassView = createdView
		ensureFrozenToolbarContentView(above: createdView)
	}

	@discardableResult
	private func ensureFrozenToolbarBackdropView(below glassView: NSView) -> NSImageView {
		if let toolbarLiquidGlassBackdropView {
			if toolbarLiquidGlassBackdropView.superview !== self {
				addSubview(
					toolbarLiquidGlassBackdropView,
					positioned: .below,
					relativeTo: glassView
				)
			}
			toolbarLiquidGlassBackdropView.layer?.zPosition = Self.frozenToolbarLiquidGlassBackdropZ
			return toolbarLiquidGlassBackdropView
		}
		let backdropView = NSImageView(frame: .zero)
		configureFrozenToolbarBackdropView(backdropView)
		addSubview(backdropView, positioned: .below, relativeTo: glassView)
		toolbarLiquidGlassBackdropView = backdropView
		return backdropView
	}

	@discardableResult
	private func ensureFrozenToolbarContentView(above glassView: NSView) -> FrozenToolbarRenderView
	{
		if let toolbarLiquidGlassContentView {
			if toolbarLiquidGlassContentView.superview !== self {
				addSubview(toolbarLiquidGlassContentView, positioned: .above, relativeTo: glassView)
			}
			toolbarLiquidGlassContentView.layer?.zPosition = Self.frozenToolbarContentZ
			return toolbarLiquidGlassContentView
		}
		let contentView = FrozenToolbarRenderView(frame: .zero)
		configureFrozenToolbarContentView(contentView)
		addSubview(contentView, positioned: .above, relativeTo: glassView)
		toolbarLiquidGlassContentView = contentView
		return contentView
	}

	private func updateFrozenToolbarBackdrop(
		for toolbarFrame: CGRect,
		preparingFirstVisibleToolbar: Bool
	) {
		guard chrome.scrollMinimapPreview != nil,
			let globalFrame = globalRect(from: toolbarFrame)
		else {
			scrollToolbarBackdropState.clearActiveFrame()
			toolbarLiquidGlassBackdropView?.isHidden = true
			toolbarLiquidGlassBackdropView?.image = nil
			return
		}
		scrollToolbarBackdropState.updateActiveFrame(toolbarFrame, globalFrame: globalFrame)
		let existingFrameMatches = toolbarLiquidGlassBackdropView?.frame == toolbarFrame
		let existingHasImage = toolbarLiquidGlassBackdropView?.image != nil
		if existingHasImage {
			if existingFrameMatches == false {
				toolbarLiquidGlassBackdropView?.frame = toolbarFrame
			}
			toolbarLiquidGlassBackdropView?.isHidden = preparingFirstVisibleToolbar
		} else {
			toolbarLiquidGlassBackdropView?.isHidden = true
			toolbarLiquidGlassBackdropView?.image = nil
		}
		if existingHasImage == false,
			let toolbarLiquidGlassView,
			let seedPatch = scrollToolbarBackdropSeedPatch(in: globalFrame)
		{
			let backdropView = ensureFrozenToolbarBackdropView(below: toolbarLiquidGlassView)
			if backdropView.frame != toolbarFrame {
				backdropView.frame = toolbarFrame
			}
			backdropView.image = NSImage(cgImage: seedPatch, size: toolbarFrame.size)
			backdropView.isHidden = preparingFirstVisibleToolbar
		}
		scheduleScrollToolbarBackdropCapture(
			toolbarFrame: toolbarFrame,
			globalFrame: globalFrame
		)
	}

	private func scheduleScrollToolbarBackdropCapture(
		toolbarFrame: CGRect,
		globalFrame: CGRect
	) {
		guard let scrollCaptureState = controller?.scrollCaptureState,
			let liveFrameStream = controller?.liveFrameStream
		else {
			return
		}
		let now = ProcessInfo.processInfo.systemUptime
		guard
			let capture = scrollToolbarBackdropState.beginCapture(
				now: now,
				minimumInterval: Self.scrollToolbarBackdropCaptureMinimumInterval,
				fallbackMinimumInterval: Self.scrollToolbarBackdropFallbackMinimumInterval
			)
		else {
			return
		}
		let fallbackSource = scrollCaptureState.captureSource
		let maximumLiveFrameAgeMicroseconds = UInt64(
			CaptureSessionController.scrollCaptureActiveInputLiveFrameMaxAge * 1_000_000
		)
		DispatchQueue.main.async { [weak self] in
			guard let self, self.scrollToolbarBackdropState.captureGeneration == capture.generation
			else {
				self?.scrollToolbarBackdropState.clearInFlightForAbandonedCapture()
				return
			}
			self.scrollToolbarBackdropCaptureQueue.async {
				[
					toolbarFrame, globalFrame, liveFrameStream, fallbackSource,
					maximumLiveFrameAgeMicroseconds, capture,
				] in
				let rawFrame = liveFrameStream.nextRegionFrame(
					in: globalFrame,
					afterFrameSequence: capture.afterFrameSequence,
					waitForFresh: false
				)
				let nonblockingFrame: RGBARegionFrameSnapshot? =
					if let rawFrame,
						Self.scrollToolbarBackdropFrameIsFresh(
							rawFrame,
							maximumAgeMicroseconds: maximumLiveFrameAgeMicroseconds
						)
					{
						rawFrame
					} else {
						nil
					}
				let freshFrame = nonblockingFrame
				let frameSequence = max(
					rawFrame?.frameSequence ?? 0,
					freshFrame?.frameSequence ?? 0
				)
				let livePatch = freshFrame.flatMap {
					NativeHostImageBridge.cgImage(from: $0.region)
				}
				let liveSignature = freshFrame.map {
					Self.scrollToolbarBackdropSignature($0.region)
				}
				let liveWouldRemainStatic =
					liveSignature == nil
					|| (capture.previousSignature != nil
						&& liveSignature == capture.previousSignature)
				let fallbackPatch =
					liveWouldRemainStatic && capture.fallbackPermitted
					? CaptureOverlayController.captureImageBelowOverlay(
						in: globalFrame,
						source: fallbackSource
					) : nil
				let fallbackSnapshot = fallbackPatch.flatMap {
					NativeHostImageBridge.rgbaSnapshot(from: $0)
				}
				let fallbackSignature = fallbackSnapshot.map {
					Self.scrollToolbarBackdropSignature($0)
				}
				let shouldUseFallback =
					fallbackPatch != nil
					&& (capture.previousSignature == nil
						|| fallbackSignature != capture.previousSignature)
				let patch = shouldUseFallback ? fallbackPatch : (livePatch ?? fallbackPatch)
				let signature =
					shouldUseFallback ? fallbackSignature : (liveSignature ?? fallbackSignature)
				DispatchQueue.main.async { [weak self = self] in
					self?.finishScrollToolbarBackdropCapture(
						patch,
						toolbarFrame: toolbarFrame,
						generation: capture.generation,
						frameSequence: frameSequence > 0 ? frameSequence : nil,
						signature: signature
					)
				}
			}
		}
	}

	nonisolated private static func scrollToolbarBackdropFrameIsFresh(
		_ frame: RGBARegionFrameSnapshot,
		maximumAgeMicroseconds: UInt64
	) -> Bool {
		frame.frameAgeMicroseconds <= maximumAgeMicroseconds
	}

	private func finishScrollToolbarBackdropCapture(
		_ patch: CGImage?,
		toolbarFrame: CGRect,
		generation: UInt64,
		frameSequence: UInt64?,
		signature: UInt64?
	) {
		guard
			scrollToolbarBackdropState.finishCapture(
				generation: generation,
				frameSequence: frameSequence
			)
		else {
			return
		}
		guard
			scene.mode == .frozen,
			chrome.scrollMinimapPreview != nil,
			let patch,
			let toolbarLiquidGlassView,
			let selection = localFrozenSelectionRect(),
			let layout = toolbarLayout(for: selection),
			layout.frame == toolbarFrame
		else {
			return
		}
		let backdropView = ensureFrozenToolbarBackdropView(below: toolbarLiquidGlassView)
		if backdropView.frame != toolbarFrame {
			backdropView.frame = toolbarFrame
		}
		backdropView.image = NSImage(cgImage: patch, size: toolbarFrame.size)
		backdropView.isHidden = false
		recordScrollToolbarBackdropChangeIfNeeded(signature: signature)
	}

	private func recordScrollToolbarBackdropChangeIfNeeded(signature: UInt64?) {
		let now = ProcessInfo.processInfo.systemUptime
		guard let change = scrollToolbarBackdropState.recordChange(signature: signature, now: now)
		else {
			return
		}
		if let gapMilliseconds = change.gapMilliseconds {
			scrollToolbarBackdropChangedGapMetric.record(gapMilliseconds)
			if change.count == 2 || change.count.isMultiple(of: 30) {
				NativeHostTelemetry.captureEvent(
					"capture.scroll_toolbar_backdrop_changed",
					captureID: controller?.activeTelemetryCaptureID ?? 0,
					detail:
						"count=\(change.count),gapMs=\(String(format: "%.2f", gapMilliseconds))"
				)
			}
		}
	}

	nonisolated private static func scrollToolbarBackdropSignature(
		_ region: RGBARegionSnapshot
	) -> UInt64 {
		var hash: UInt64 = 14_695_981_039_346_656_037
		let stride = max(region.rgba.count / 256, 4)
		region.rgba.withUnsafeBytes { rawBuffer in
			guard let bytes = rawBuffer.bindMemory(to: UInt8.self).baseAddress else {
				return
			}
			var index = 0
			while index < region.rgba.count {
				hash ^= UInt64(bytes[index])
				hash &*= 1_099_511_628_211
				index += stride
			}
		}
		hash ^= UInt64(max(region.width, 0))
		hash &*= 1_099_511_628_211
		hash ^= UInt64(max(region.height, 0))
		return hash
	}

	func refreshScrollCaptureToolbarBackdropNow() {
		guard settings.usesLiquidHudGlass, chrome.scrollMinimapPreview != nil else {
			return
		}
		guard let state = controller?.scrollCaptureState,
			controller?.nativeScrollCaptureToolbarBackdropShouldLoop(state: state) == true
		else {
			return
		}
		let now = ProcessInfo.processInfo.systemUptime
		let interval = NativeHostDisplayRefresh.frameInterval(
			forTargetFramesPerSecond: currentDisplayTargetFramesPerSecond())
		guard let refresh = scrollToolbarBackdropState.beginRefresh(now: now, interval: interval)
		else {
			return
		}
		if let gapMilliseconds = refresh.gapMilliseconds {
			scrollToolbarBackdropRefreshGapMetric.record(gapMilliseconds)
		}
		let refreshStartedAt = now
		if let toolbarFrame = refresh.activeFrame, let globalFrame = refresh.activeGlobalFrame {
			scheduleScrollToolbarBackdropCapture(
				toolbarFrame: toolbarFrame,
				globalFrame: globalFrame
			)
		} else {
			_ = refreshFrozenToolbarBackdropOnly()
		}
		scrollToolbarBackdropRefreshDurationMetric.recordMillisecondsSince(refreshStartedAt)
	}

	private func refreshFrozenToolbarBackdropOnly() -> Bool {
		guard
			scene.mode == .frozen,
			settings.usesLiquidHudGlass,
			frozenToolbarLiquidGlassVisible,
			frozenToolbarLiquidGlassContentDrawn,
			let toolbarLiquidGlassView,
			toolbarLiquidGlassView.isHidden == false,
			let selection = localFrozenSelectionRect(),
			let layout = toolbarLayout(for: selection),
			toolbarLiquidGlassView.frame == layout.frame
		else {
			return false
		}
		updateFrozenToolbarBackdrop(
			for: layout.frame,
			preparingFirstVisibleToolbar: false
		)
		return true
	}

	private func hideLiveLiquidGlassViews(removing: Bool = true) {
		if removing {
			hudLiquidGlassView?.removeFromSuperview()
			loupeLiquidGlassView?.removeFromSuperview()
			hudLiquidGlassView = nil
			loupeLiquidGlassView = nil
		} else {
			hudLiquidGlassView?.isHidden = true
			loupeLiquidGlassView?.isHidden = true
		}
	}

	private func updateFrozenToolbarLiquidGlassView() {
		let wasVisible = frozenToolbarLiquidGlassVisible
		guard
			scene.mode == .frozen,
			settings.usesLiquidHudGlass,
			let selection = localFrozenSelectionRect(),
			let layout = toolbarLayout(for: selection)
		else {
			hideFrozenToolbarLiquidGlassView()
			return
		}
		updateLiveLiquidGlassView(
			&toolbarLiquidGlassView,
			frame: layout.frame,
			zPosition: Self.frozenToolbarLiquidGlassZ
		)
		guard let toolbarLiquidGlassView else {
			frozenToolbarLiquidGlassVisible = false
			frozenToolbarLiquidGlassContentDrawn = false
			toolbarLiquidGlassContentView?.isHidden = true
			if wasVisible {
				needsDisplay = true
			}
			return
		}
		toolbarLiquidGlassView.layer?.zPosition = Self.frozenToolbarLiquidGlassZ
		let preparingFirstVisibleToolbar =
			!wasVisible || !frozenToolbarLiquidGlassVisible || !frozenToolbarLiquidGlassContentDrawn
		if preparingFirstVisibleToolbar {
			toolbarLiquidGlassView.isHidden = true
		}
		updateFrozenToolbarBackdrop(
			for: layout.frame,
			preparingFirstVisibleToolbar: preparingFirstVisibleToolbar
		)
		let contentView = ensureFrozenToolbarContentView(above: toolbarLiquidGlassView)
		let frameChanged = contentView.frame != layout.frame
		if contentView.frame != layout.frame {
			contentView.frame = layout.frame
			contentView.needsDisplay = true
		}
		contentView.isHidden = preparingFirstVisibleToolbar
		let changed = contentView.update(
			theme: chromeTheme(),
			settings: settings,
			hoveredToolbarAction: toolbarHoverState.toolbarAction,
			hoveredAnnotationStyleAction: toolbarHoverState.annotationStyleAction,
			toolbarScale: layout.scale,
			annotationStyleState: chrome.annotationStyle,
			annotationStyleLayout: layout.annotationStyle.map {
				FrozenToolbarLayoutPlanner.localAnnotationStyleLayout(
					$0,
					relativeTo: layout.frame
				)
			},
			items: layout.items.map { item in
				FrozenToolbarItemLayout(
					kind: item.kind,
					frame: item.frame.offsetBy(dx: -layout.frame.minX, dy: -layout.frame.minY),
					enabled: item.enabled,
					selected: item.selected
				)
			}
		)
		if changed {
			contentView.needsDisplay = true
		}
		if frameChanged || changed || !wasVisible || !frozenToolbarLiquidGlassContentDrawn {
			contentView.display()
		}
		if preparingFirstVisibleToolbar {
			CATransaction.begin()
			CATransaction.setDisableActions(true)
			toolbarLiquidGlassView.isHidden = false
			if chrome.scrollMinimapPreview != nil, toolbarLiquidGlassBackdropView?.image != nil {
				toolbarLiquidGlassBackdropView?.isHidden = false
			}
			contentView.isHidden = false
			CATransaction.commit()
		}
		frozenToolbarLiquidGlassVisible = true
		frozenToolbarLiquidGlassContentDrawn = true
		if wasVisible == false {
			needsDisplay = true
		}
	}

	private func hideFrozenToolbarLiquidGlassView() {
		let wasVisible = frozenToolbarLiquidGlassVisible
		frozenToolbarLiquidGlassVisible = false
		frozenToolbarLiquidGlassContentDrawn = false
		scrollToolbarBackdropState.resetAndInvalidateCaptures()
		toolbarLiquidGlassBackdropView?.isHidden = true
		toolbarLiquidGlassBackdropView?.image = nil
		toolbarLiquidGlassView?.isHidden = true
		toolbarLiquidGlassContentView?.isHidden = true
		if wasVisible {
			needsDisplay = true
		}
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

	private func themeBrightnessBias(for theme: CaptureChromeTheme) -> Double {
		theme == .dark ? 0.015 : -0.01
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
