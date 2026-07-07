import AppKit
import CoreGraphics
import Foundation
import QuartzCore
import RsnapHostBridge

@MainActor
final class CaptureHostView: NSView {
	static let liveDragIntentThreshold: CGFloat = 3

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
	var annotationStyleWheelGate = CaptureHostAnnotationStyleWheelGate()
	var lastCursorPresentation: CaptureHostCursorPresentation?
	let cursorOwner = CaptureHostCursorOwner()
	var livePrimaryInteraction = CaptureHostLivePrimaryInteractionState()
	let mouseReleaseRecovery = CaptureHostMouseReleaseRecovery()
	let livePointerPreview = CaptureHostLivePointerPreviewState()
	var liveHighlightedWindowPreview: WindowSnapshot?
	var sampleUpdatedLiveChromeRenderInProgress = false
	var frozenFirstDisplayHandoff = CaptureHostFrozenFirstDisplayHandoffState()
	var lastLivePreviewSnapshot: LivePreviewSnapshot?
	var liveSampleCache = CaptureHostLiveSampleCache()
	lazy var frozenToolbar = CaptureHostFrozenToolbarCoordinator(hostView: self)
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

	isolated deinit {
		clearVisibleCursorOverride()
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
		let frozenTransformDirtyRect = frozenSelectionTransformDirtyRect(
			previousScene: previousScene,
			previousChrome: previousChrome,
			previousSettings: previousSettings,
			nextScene: scene,
			nextChrome: chrome,
			nextSettings: settings
		)
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
		if chrome.frozenSelectionInteraction == nil {
			frozenToolbar.refreshHoveredAction()
		}
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
				if let frozenTransformDirtyRect {
					setNeedsDisplay(frozenTransformDirtyRect)
				} else {
					needsDisplay = true
				}
			}
		}
	}

	private func frozenSelectionTransformDirtyRect(
		previousScene: SceneSnapshot,
		previousChrome: CaptureChromeState,
		previousSettings: NativeHostSettings,
		nextScene: SceneSnapshot,
		nextChrome: CaptureChromeState,
		nextSettings: NativeHostSettings
	) -> CGRect? {
		guard previousScene.mode == .frozen, nextScene.mode == .frozen else {
			return nil
		}
		guard previousSettings == nextSettings else {
			return nil
		}
		guard
			previousChrome.frozenSelectionInteraction != nil
				|| nextChrome.frozenSelectionInteraction != nil
		else {
			return nil
		}
		guard previousChrome.frozenDisplayFrame == nextChrome.frozenDisplayFrame else {
			return nil
		}
		guard
			let previousSelection = localRect(
				from: previousChrome.frozenSelectionSnapshot ?? previousScene.frozenSelection),
			let nextSelection = localRect(
				from: nextChrome.frozenSelectionSnapshot ?? nextScene.frozenSelection)
		else {
			return nil
		}

		var dirtyRect = previousSelection.union(nextSelection)
		if let previousToolbarFrame = frozenToolbarFrame(
			for: previousSelection,
			scene: previousScene,
			chrome: previousChrome,
			settings: previousSettings
		) {
			dirtyRect = dirtyRect.union(previousToolbarFrame)
		}
		if let nextToolbarFrame = frozenToolbarFrame(
			for: nextSelection,
			scene: nextScene,
			chrome: nextChrome,
			settings: nextSettings
		) {
			dirtyRect = dirtyRect.union(nextToolbarFrame)
		}
		let clippedDirtyRect = dirtyRect.insetBy(dx: -96, dy: -96).intersection(bounds)
		return clippedDirtyRect.isNull ? nil : clippedDirtyRect
	}

	private func frozenToolbarFrame(
		for selection: CGRect,
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) -> CGRect? {
		FrozenToolbarLayoutPlanner.layout(
			selection: selection,
			bounds: bounds,
			prefersTopPlacement: settings.toolbarPlacement == .top,
			items: FrozenToolbarLayoutPlanner.visibleItems(
				from: scene.toolbarItems,
				availability: FrozenToolbarAvailability(
					scrollCaptureActive: chrome.scrollMinimapPreview != nil,
					canUndo: chrome.frozenOverlay.canUndo,
					canRedo: chrome.frozenOverlay.canRedo,
					frozenSelectionAvailable: scene.frozenSelection != nil,
					keepsFrozenSelectionFixed: chrome.frozenOverlay.keepsFrozenSelectionFixed,
					scrollToolbarEnabled: controller?.scrollCaptureToolbarEnabled ?? false,
					hasRecognizeTextBlockingEdits: chrome.frozenOverlay
						.hasRecognizeTextBlockingEdits
				)
			),
			annotationStyle: chrome.annotationStyle
		)?.frame
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
		routeCursorUpdate(with: event)
	}

	override func mouseMoved(with event: NSEvent) {
		routeMouseMoved(with: event)
	}

	override func mouseDragged(with event: NSEvent) {
		routeMouseDragged(with: event)
	}

	override func mouseDown(with event: NSEvent) {
		routeMouseDown(with: event)
	}

	override func scrollWheel(with event: NSEvent) {
		if routeScrollWheel(with: event) == false {
			super.scrollWheel(with: event)
		}
	}

	override func rightMouseDown(with event: NSEvent) {
		controller?.cancelCapture()
	}

	override func mouseUp(with event: NSEvent) {
		routeMouseUp(with: event)
	}

	override func keyDown(with event: NSEvent) {
		if routeKeyDown(with: event) == false {
			super.keyDown(with: event)
		}
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

	private func themeBrightnessBias() -> Double {
		chromeTheme() == .dark ? 0.015 : -0.01
	}

}
