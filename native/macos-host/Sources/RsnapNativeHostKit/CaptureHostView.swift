import AppKit
import CoreGraphics
import CoreImage
import CoreText
import Foundation
import QuartzCore
import RsnapHostBridge

@MainActor private let frozenEffectCIContext = CIContext(options: nil)

struct LiveChromeRefreshTelemetryKey: Equatable {
	let targetHz: Int
	let hudGlassEnabled: Bool
	let hudGlassMode: String
	let liquidGlassStyle: String
	let liquidGlassAvailable: Bool
}

func makeFrozenMosaicPatch(from image: CGImage, sourceRect: CGRect) -> CGImage? {
	guard
		let patch = try? RsnapExportEncoder.frozenMosaicLightPrivacyPatch(
			imageWidth: image.width,
			imageHeight: image.height,
			sourceRect: sourceRect
		)
	else {
		return nil
	}

	let bitmapInfo =
		CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
	return NativeHostImageBridge.cgImage(
		width: patch.width,
		height: patch.height,
		rgba: patch.rgba,
		bitmapInfo: CGBitmapInfo(rawValue: bitmapInfo)
	)
}

@MainActor
final class CaptureHostView: NSView {
	private static let liveDragIntentThreshold: CGFloat = 3
	private static let scrollToolbarBackdropCaptureMinimumInterval: TimeInterval = 1.0 / 60.0
	private static let scrollToolbarBackdropFallbackMinimumInterval: TimeInterval = 1.0 / 20.0

	private final class FrozenToolbarRenderView: NSView {
		struct Item: Equatable {
			let kind: ToolbarItemKind
			let frame: CGRect
			let enabled: Bool
			let selected: Bool
		}

		private var theme: CaptureChromeTheme = .dark
		private var settings = NativeHostSettings.defaults
		private var hoveredToolbarAction: ToolbarItemKind?
		private var hoveredAnnotationStyleAction: FrozenAnnotationStyleAction?
		private var toolbarScale: CGFloat = 1
		private var annotationStyleState = FrozenAnnotationStyleState()
		private var annotationStyleLayout: FrozenAnnotationStyleLayout?
		private var items: [Item] = []

		override var isOpaque: Bool { false }

		override func hitTest(_ point: NSPoint) -> NSView? {
			nil
		}

		@discardableResult
		func update(
			theme: CaptureChromeTheme,
			settings: NativeHostSettings,
			hoveredToolbarAction: ToolbarItemKind?,
			hoveredAnnotationStyleAction: FrozenAnnotationStyleAction?,
			toolbarScale: CGFloat,
			annotationStyleState: FrozenAnnotationStyleState,
			annotationStyleLayout: FrozenAnnotationStyleLayout?,
			items: [Item]
		) -> Bool {
			let changed =
				self.theme != theme || self.settings != settings
				|| self.hoveredToolbarAction != hoveredToolbarAction
				|| self.hoveredAnnotationStyleAction != hoveredAnnotationStyleAction
				|| self.toolbarScale != toolbarScale
				|| self.annotationStyleState != annotationStyleState
				|| self.annotationStyleLayout != annotationStyleLayout || self.items != items
			self.theme = theme
			self.settings = settings
			self.hoveredToolbarAction = hoveredToolbarAction
			self.hoveredAnnotationStyleAction = hoveredAnnotationStyleAction
			self.toolbarScale = toolbarScale
			self.annotationStyleState = annotationStyleState
			self.annotationStyleLayout = annotationStyleLayout
			self.items = items
			if changed {
				needsDisplay = true
			}
			return changed
		}

		override func draw(_ dirtyRect: NSRect) {
			super.draw(dirtyRect)
			guard let context = NSGraphicsContext.current?.cgContext else {
				return
			}
			context.clear(bounds)
			drawToolbarContent(in: context)
		}

		private func drawToolbarContent(in context: CGContext) {
			let palette = CaptureChrome.palette(for: theme, settings: settings)
			let pillPath = NSBezierPath(
				roundedRect: bounds,
				xRadius: CaptureChrome.hudCornerRadius,
				yRadius: CaptureChrome.hudCornerRadius
			)
			context.setStrokeColor(palette.outerStroke.cgColor)
			context.setLineWidth(1)
			pillPath.stroke()

			for item in items {
				if hoveredToolbarAction == item.kind, item.enabled, !item.selected {
					context.setFillColor(palette.toolbarHoverBackground.cgColor)
					let radius = CaptureChrome.toolbarControlCornerRadius * toolbarScale
					let hoverPath = NSBezierPath(
						roundedRect: item.frame,
						xRadius: radius,
						yRadius: radius
					)
					hoverPath.fill()
				}
				if item.selected {
					context.setFillColor(palette.toolbarSelectedBackground.cgColor)
					let radius = CaptureChrome.toolbarControlCornerRadius * toolbarScale
					let selectedPath = NSBezierPath(
						roundedRect: item.frame,
						xRadius: radius,
						yRadius: radius
					)
					selectedPath.fill()
				}

				let symbolColor =
					item.enabled
					? (item.selected ? palette.toolbarSelectedIcon : palette.toolbarIcon)
					: palette.toolbarDisabledIcon
				drawToolbarGlyph(
					item.kind,
					selected: item.selected,
					in: item.frame,
					scale: toolbarScale,
					color: symbolColor,
					context: context
				)
			}

			if let annotationStyleLayout {
				FrozenToolbarDrawing.drawAnnotationStyleControls(
					annotationStyleLayout,
					state: annotationStyleState,
					hoveredAction: hoveredAnnotationStyleAction,
					palette: palette,
					in: context
				)
			}
		}

		private func drawToolbarGlyph(
			_ kind: ToolbarItemKind,
			selected: Bool,
			in rect: CGRect,
			scale: CGFloat,
			color: NSColor,
			context: CGContext
		) {
			let glyph = PhosphorToolbarIcons.cachedGlyph(
				for: kind,
				selected: selected,
				size: CaptureChrome.toolbarGlyphSize * scale
			)
			let origin = CGPoint(
				x: rect.midX - glyph.bounds.width * 0.5 - glyph.bounds.origin.x,
				y: rect.midY - glyph.bounds.height * 0.5 - glyph.bounds.origin.y
			)
			context.saveGState()
			context.setFillColor(color.cgColor)
			context.textMatrix = .identity
			context.textPosition = origin
			CTLineDraw(glyph.line, context)
			context.restoreGState()
		}
	}

	private enum QueuedPointerEvent {
		case moved(CGPoint)
		case liveDragged(CGPoint)
	}

	private enum GlassSurfaceKind: Hashable {
		case hud
		case loupe
		case toolbar
	}

	private struct GlassPatchCache {
		let frame: CGRect
		let capturedAt: TimeInterval
		let image: CGImage
	}

	private struct LiveFloatingPlacement {
		let frame: CGRect
		let flippedHorizontally: Bool
	}

	private enum CursorPresentation: Equatable {
		case arrow
		case crosshair
		case openHand
		case closedHand
		case resizeUpDown
		case resizeLeftRight
		case resizeTopLeft
		case resizeTopRight
		case resizeBottomLeft
		case resizeBottomRight
		case iBeam
	}

	private struct HudLayoutMetrics {
		let font: NSFont
		let lineHeight: CGFloat
		let commaWidth: CGFloat
		let xPrefixWidth: CGFloat
		let yPrefixWidth: CGFloat
		let digitWidth: CGFloat
		let minusWidth: CGFloat
		let keycapTextSize: CGSize
		let keycapFrameSize: CGSize
		let hexSlotWidth: CGFloat
		let placeholderXSlotWidth: CGFloat
		let placeholderYSlotWidth: CGFloat
	}

	private static let hudLayoutMetrics: HudLayoutMetrics = {
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let keycapTextSize = "Tab".size(using: font)
		return HudLayoutMetrics(
			font: font,
			lineHeight: ceil("x=0".size(using: font).height),
			commaWidth: ",".size(using: font).width,
			xPrefixWidth: "x=".size(using: font).width,
			yPrefixWidth: "y=".size(using: font).width,
			digitWidth: "0".size(using: font).width,
			minusWidth: "-".size(using: font).width,
			keycapTextSize: keycapTextSize,
			keycapFrameSize: CGSize(
				width: keycapTextSize.width + 12, height: keycapTextSize.height + 4),
			hexSlotWidth: "#FFFFFF".size(using: font).width,
			placeholderXSlotWidth: "x=?".size(using: font).width,
			placeholderYSlotWidth: "y=?".size(using: font).width
		)
	}()
	private static let pendingHudHexWheel = Array("0123456789ABCDEF")
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
	private var lastScrollCaptureToolbarBackdropRefreshUptime: TimeInterval = 0
	private var lastScrollToolbarBackdropCaptureStartedUptime: TimeInterval = 0
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
	private var scrollToolbarBackdropCaptureInFlight = false
	private var scrollToolbarBackdropCaptureGeneration: UInt64 = 0
	private var scrollToolbarBackdropSeedFrame: CGRect?
	private var scrollToolbarBackdropSeedImage: CGImage?
	private var scrollToolbarBackdropSeedPatchCache: GlassPatchCache?
	private var scrollToolbarBackdropLastFrameSequence: UInt64 = 0
	private var scrollToolbarBackdropLastSignature: UInt64?
	private var scrollToolbarBackdropChangedCount: UInt64 = 0
	private var scrollToolbarBackdropFrame: CGRect?
	private var scrollToolbarBackdropGlobalFrame: CGRect?
	private var lastScrollToolbarBackdropChangedUptime: TimeInterval = 0
	private var lastScrollToolbarBackdropFallbackCaptureStartedUptime: TimeInterval = 0
	private var trackingAreaRef: NSTrackingArea?
	private var pointerOverFrozenToolbar = false
	private var hoveredToolbarAction: ToolbarItemKind?
	private var hoveredAnnotationStyleAction: FrozenAnnotationStyleAction?
	private var annotationStyleWheelLastStepTimestamp: TimeInterval?
	private var lastCursorPresentation: CursorPresentation?
	private var lastAppliedCursorPresentation: CursorPresentation?
	private var queuedPointerEvent: QueuedPointerEvent?
	private var queuedPointerWorkItem: DispatchWorkItem?
	private var lastHoverPointerDispatchUptime: TimeInterval = 0
	private var lastDragPointerDispatchUptime: TimeInterval = 0
	private var liveDragStartGlobal: CGPoint?
	private var liveDragReleasedGlobal: CGPoint?
	private var liveDragExceededThreshold = false
	private var livePrimaryCompletionInFlight = false
	private var liveMouseUpMonitor: Any?
	private var liveMouseReleaseWatchdog: DispatchWorkItem?
	private var frozenMouseReleaseWatchdog: DispatchWorkItem?
	private var livePointerPreviewGlobal: CGPoint?
	private var livePointerPreviewInputUptime: TimeInterval?
	private var livePointerPreviewInputSequence: UInt64 = 0
	private var lastLivePointerEventUptime: TimeInterval?
	private var liveHighlightedWindowPreview: WindowSnapshot?
	private var liveHoverChromeSuppressed = false
	private var sampleUpdatedLiveChromeRenderInProgress = false
	private var pendingFrozenFirstDisplay = false
	private var frozenFirstDisplayCompletionQueued = false
	private var frozenFirstDisplayHandoffStartedAt: TimeInterval?
	private var frozenFirstDisplayPendingFrameDisplayed = false
	private var defersFrozenToolbarClassicGlassUntilAfterFirstDisplay = false
	private var lastLivePreviewSnapshot: LivePreviewSnapshot?
	private var latestLiveChromeSample: LiveChromeSample?
	private var latestLiveChromeSamplePoint: CGPoint?
	private var latestLiveRgbSample: LiveRgbSample?
	private var latestLiveRgbSamplePoint: CGPoint?
	private var glassPatchCache: [GlassSurfaceKind: GlassPatchCache] = [:]
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
			frozenFirstDisplayCompletionQueued = false
			frozenFirstDisplayHandoffStartedAt = nil
			frozenFirstDisplayPendingFrameDisplayed = false
			defersFrozenToolbarClassicGlassUntilAfterFirstDisplay = false
			scrollToolbarBackdropSeedFrame = nil
			scrollToolbarBackdropSeedImage = nil
			scrollToolbarBackdropSeedPatchCache = nil
			scrollToolbarBackdropLastFrameSequence = 0
			scrollToolbarBackdropLastSignature = nil
			scrollToolbarBackdropChangedCount = 0
			scrollToolbarBackdropFrame = nil
			scrollToolbarBackdropGlobalFrame = nil
			lastScrollToolbarBackdropChangedUptime = 0
			lastScrollToolbarBackdropFallbackCaptureStartedUptime = 0
			lastScrollCaptureToolbarBackdropRefreshUptime = 0
		} else if previousChrome.scrollMinimapPreview == nil, chrome.scrollMinimapPreview != nil {
			scrollToolbarBackdropSeedFrame = previousChrome.frozenDisplayFrame
			scrollToolbarBackdropSeedImage = previousChrome.frozenDisplayImage
			scrollToolbarBackdropSeedPatchCache = nil
			scrollToolbarBackdropLastFrameSequence = 0
			scrollToolbarBackdropLastSignature = nil
			scrollToolbarBackdropChangedCount = 0
			scrollToolbarBackdropFrame = nil
			scrollToolbarBackdropGlobalFrame = nil
			lastScrollToolbarBackdropChangedUptime = 0
			lastScrollToolbarBackdropFallbackCaptureStartedUptime = 0
			lastScrollCaptureToolbarBackdropRefreshUptime = 0
		} else if previousChrome.scrollMinimapPreview != nil, chrome.scrollMinimapPreview == nil {
			scrollToolbarBackdropSeedFrame = nil
			scrollToolbarBackdropSeedImage = nil
			scrollToolbarBackdropSeedPatchCache = nil
			scrollToolbarBackdropLastFrameSequence = 0
			scrollToolbarBackdropLastSignature = nil
			scrollToolbarBackdropChangedCount = 0
			scrollToolbarBackdropFrame = nil
			scrollToolbarBackdropGlobalFrame = nil
			lastScrollToolbarBackdropChangedUptime = 0
			lastScrollToolbarBackdropFallbackCaptureStartedUptime = 0
			lastScrollCaptureToolbarBackdropRefreshUptime = 0
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
			pendingFrozenFirstDisplay = false
			if previousMode != .live {
				liveHoverChromeSuppressed = false
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
				liveHoverChromeSuppressed = false
				pendingFrozenFirstDisplay = false
				lastLivePreviewSnapshot = nil
				latestLiveChromeSample = nil
				latestLiveChromeSamplePoint = nil
				latestLiveRgbSample = nil
				latestLiveRgbSamplePoint = nil
			}
			resetLivePointerPreview()
			liveHighlightedWindowPreview = nil
			if transitioningToFrozen {
				pendingFrozenFirstDisplay = true
				frozenFirstDisplayHandoffStartedAt = ProcessInfo.processInfo.systemUptime
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
		guard pendingFrozenFirstDisplay else {
			return
		}
		window?.disableScreenUpdatesUntilFlush()
		finishFrozenFirstDisplayHandoff()
	}

	private func finishFrozenFirstDisplayHandoff() {
		let handoffStartedAt = frozenFirstDisplayHandoffStartedAt
		pendingFrozenFirstDisplay = false
		frozenFirstDisplayCompletionQueued = false
		frozenFirstDisplayHandoffStartedAt = nil
		let pendingFrameDisplayed = frozenFirstDisplayPendingFrameDisplayed
		frozenFirstDisplayPendingFrameDisplayed = false
		let deferredClassicToolbarGlass =
			defersFrozenToolbarClassicGlassUntilAfterFirstDisplay
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
		if deferredClassicToolbarGlass {
			DispatchQueue.main.async { [weak self] in
				guard let self else {
					return
				}
				self.defersFrozenToolbarClassicGlassUntilAfterFirstDisplay = false
				self.needsDisplay = true
			}
		}
		if let handoffStartedAt {
			emitFrozenFirstDisplayHandoffTiming(
				startedAt: handoffStartedAt,
				materialMilliseconds: materialMilliseconds,
				liveRendererStopMilliseconds: liveRendererStopMilliseconds,
				displayMilliseconds: displayMilliseconds,
				pendingFrameDisplayed: pendingFrameDisplayed
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
		liveHoverChromeSuppressed = false
		pendingFrozenFirstDisplay = false
		frozenFirstDisplayCompletionQueued = false
		frozenFirstDisplayHandoffStartedAt = nil
		frozenFirstDisplayPendingFrameDisplayed = false
		defersFrozenToolbarClassicGlassUntilAfterFirstDisplay = false
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
		liveHoverChromeSuppressed = false
		pendingFrozenFirstDisplay = retainedLivePreview != nil || scene.frozenSelection != nil
		frozenFirstDisplayCompletionQueued = false
		frozenFirstDisplayHandoffStartedAt =
			pendingFrozenFirstDisplay ? ProcessInfo.processInfo.systemUptime : nil
		frozenFirstDisplayPendingFrameDisplayed = false
		defersFrozenToolbarClassicGlassUntilAfterFirstDisplay = settings.usesClassicHudGlass
		lastLivePreviewSnapshot = retainedLivePreview
		clearLivePrimaryInteractionState(rendersImmediately: false)
		resetLivePointerPreview()
		liveHighlightedWindowPreview = nil
		clearHoveredToolbarAction()
		syncVisibleCursor()
		needsDisplay = true
		controller?.updateLivePreviewDemand(
			point: nil, settings: settings, includeLoupePatch: false)
		if rendersPendingFrame, pendingFrozenFirstDisplay {
			frozenFirstDisplayPendingFrameDisplayed = true
			liveRenderer.renderNow()
		}
	}

	func finishFrozenFirstFrameInstall() {
		guard pendingFrozenFirstDisplay else {
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
		addCursorRect(bounds, cursor: cursor(for: currentCursorPresentation()))
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
			if liveDragExceededThreshold == false,
				liveDragDistance(from: point) >= Self.liveDragIntentThreshold
			{
				liveDragExceededThreshold = true
				logLivePrimaryInputEvent("capture.live_primary_drag_threshold", point: point)
			}
			updateLivePointerPreview(to: point, rendersImmediately: false)
			queuePointerEvent(liveDragExceededThreshold ? .liveDragged(point) : .moved(point))
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
			liveDragStartGlobal = point
			liveDragReleasedGlobal = nil
			liveDragExceededThreshold = false
			livePrimaryCompletionInFlight = false
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

	private static let annotationStyleWheelDeadZone: CGFloat = 0.05
	private static let annotationStylePreciseWheelStepInterval: TimeInterval = 0.18
	private static let annotationStyleDiscreteWheelStepInterval: TimeInterval = 0.04

	private func annotationStyleWheelSteps(from event: NSEvent) -> Int {
		guard event.momentumPhase == [] else {
			return 0
		}
		let phase = event.phase
		if phase.contains(.ended) || phase.contains(.cancelled) {
			resetAnnotationStyleWheelGate()
			return 0
		}
		let deltaY = event.scrollingDeltaY
		guard abs(deltaY) > .ulpOfOne else {
			return 0
		}
		guard abs(deltaY) >= Self.annotationStyleWheelDeadZone else {
			return 0
		}
		let direction = deltaY > 0 ? 1 : -1
		let isSmoothScroll = event.hasPreciseScrollingDeltas || phase != []
		let minimumInterval =
			isSmoothScroll
			? Self.annotationStylePreciseWheelStepInterval
			: Self.annotationStyleDiscreteWheelStepInterval
		if let lastStepTimestamp = annotationStyleWheelLastStepTimestamp,
			event.timestamp - lastStepTimestamp < minimumInterval
		{
			return 0
		}
		annotationStyleWheelLastStepTimestamp = event.timestamp
		return direction
	}

	private func resetAnnotationStyleWheelGate() {
		annotationStyleWheelLastStepTimestamp = nil
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
			if pendingFrozenFirstDisplay {
				frozenFirstDisplayPendingFrameDisplayed = true
				scheduleFrozenFirstFrameInstallCompletionIfNeeded()
				return
			}
			if let selection = localFrozenSelectionRect().map(pixelAlignedSelectionRect) {
				drawFrozenDisplaySurface(in: context)
				let toolbarScrimExclusionPath = frozenToolbarScrimExclusionPath(for: selection)
				drawSelectionScrim(
					for: selection,
					in: context,
					alpha: CaptureChrome.frozenScrimAlpha,
					excluding: toolbarScrimExclusionPath
				)
				drawDashedSelectionBorder(
					around: selection,
					in: context,
					lineWidth: CaptureChrome.frozenDashedBorderWidth
				)
				if chrome.frozenSelectionTransformAllowed {
					drawFrozenResizeHandles(for: selection, in: context)
				}
				drawFrozenOverlays(for: selection, in: context)
				drawScrollCaptureMinimap(for: selection, in: context)
				drawSelectionSizeBadge(for: selection, in: context)
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
		guard pendingFrozenFirstDisplay, !frozenFirstDisplayCompletionQueued else {
			return
		}
		frozenFirstDisplayCompletionQueued = true
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
		guard
			let minimapPlan = scrollCaptureMinimapPlan(
				for: selection,
				exportSize: preview.exportSizePixels,
				in: bounds,
				preferredWidth: CaptureChrome.scrollMinimapPreferredWidth,
				minimumWidth: CaptureChrome.scrollMinimapMinimumWidth,
				gap: CaptureChrome.scrollMinimapGap,
				margin: CaptureChrome.scrollMinimapScreenMargin,
				imageInset: CaptureChrome.scrollMinimapImageInset,
				viewportTopPixels: preview.viewportTopYPixels,
				viewportHeightPixels: preview.viewportHeightPixels
			)
		else {
			return
		}

		let theme = chromeTheme()
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let frame = minimapPlan.frame
		let imageFrame = minimapPlan.imageFrame
		let backgroundPath = NSBezierPath(
			roundedRect: frame,
			xRadius: CaptureChrome.scrollMinimapCornerRadius,
			yRadius: CaptureChrome.scrollMinimapCornerRadius
		)

		context.saveGState()
		context.setShadow(
			offset: CGSize(width: 0, height: -2),
			blur: 12,
			color: NSColor.black.withAlphaComponent(0.32).cgColor
		)
		context.setFillColor(NSColor.black.withAlphaComponent(0.72).cgColor)
		backgroundPath.fill()
		context.restoreGState()

		context.saveGState()
		let imageClipPath = NSBezierPath(
			roundedRect: imageFrame,
			xRadius: max(CaptureChrome.scrollMinimapCornerRadius - 3, 1),
			yRadius: max(CaptureChrome.scrollMinimapCornerRadius - 3, 1)
		)
		imageClipPath.addClip()
		context.interpolationQuality = .high
		context.draw(preview.image, in: imageFrame)
		context.restoreGState()

		if let viewportFrame = minimapPlan.viewportFrame {
			context.setFillColor(NSColor.white.withAlphaComponent(0.13).cgColor)
			context.fill(viewportFrame)
			context.setStrokeColor(NSColor.white.withAlphaComponent(0.88).cgColor)
			context.setLineWidth(1)
			context.stroke(viewportFrame)
		}

		context.setStrokeColor(palette.keycapStroke.withAlphaComponent(0.88).cgColor)
		context.setLineWidth(1)
		backgroundPath.stroke()
	}

	private func localFrozenDisplayFrame() -> CGRect? {
		localRect(from: chrome.frozenDisplayFrame)
	}

	private func currentImmediateLiveDragSelectionLocal() -> CGRect? {
		guard scene.mode == .live, let dragStart = liveDragStartGlobal, let window else {
			return nil
		}
		guard liveDragExceededThreshold else {
			return nil
		}
		let current =
			liveDragReleasedGlobal ?? livePointerPreviewGlobal ?? scene.pointer ?? dragStart
		let windowFrame = window.frame
		guard windowFrame.contains(dragStart) else {
			return nil
		}
		let normalized = windowFrame.normalizedRect(anchor: dragStart, current: current)
		guard max(normalized.width, normalized.height) >= 1 else {
			return nil
		}
		let globalRect = CGRect(
			x: normalized.minX,
			y: normalized.minY,
			width: max(normalized.width, 1),
			height: max(normalized.height, 1)
		)
		return localRect(from: globalRect)
	}

	private func liveDragDistance(from point: CGPoint) -> CGFloat {
		guard let dragStart = liveDragStartGlobal else {
			return 0
		}
		return max(abs(point.x - dragStart.x), abs(point.y - dragStart.y))
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
		guard scene.mode == .live, liveDragStartGlobal != nil else {
			return
		}
		let completionPoint = liveDragCompletionPoint(for: point)
		logLivePrimaryInputEvent(
			"capture.live_primary_release_marked",
			point: completionPoint,
			detail: "dragExceeded=\(liveDragExceededThreshold)"
		)
		livePrimaryCompletionInFlight = true
		liveDragReleasedGlobal = completionPoint
		liveHoverChromeSuppressed = false
		removeLiveMouseUpMonitor()
		cancelQueuedPointerDispatch()
		updateLivePointerPreview(
			to: completionPoint,
			rendersImmediately: true,
			rendersFullPreview: liveDragExceededThreshold
		)
	}

	var hasLivePrimaryInteraction: Bool {
		scene.mode == .live && liveDragStartGlobal != nil
	}

	func completeOwnedLivePrimaryInteraction(at point: CGPoint) {
		guard scene.mode == .live, liveDragStartGlobal != nil, !livePrimaryCompletionInFlight else {
			return
		}
		let completionPoint = liveDragCompletionPoint(for: point)
		logLivePrimaryInputEvent(
			"capture.live_primary_complete_owned",
			point: completionPoint,
			detail: "dragExceeded=\(liveDragExceededThreshold)"
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
			liveDragStartGlobal != nil,
			!livePrimaryCompletionInFlight,
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
		liveDragExceededThreshold ? point : liveDragStartGlobal ?? point
	}

	private func isPrimaryMouseButtonPressed() -> Bool {
		(NSEvent.pressedMouseButtons & 1) == 1
	}

	func clearLivePrimaryInteractionState(rendersImmediately: Bool) {
		cancelQueuedPointerDispatch()
		liveHoverChromeSuppressed = false
		liveDragStartGlobal = nil
		liveDragReleasedGlobal = nil
		liveDragExceededThreshold = false
		livePrimaryCompletionInFlight = false
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
			liveDragStartGlobal != nil,
			livePrimaryCompletionInFlight == false
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
			liveDragStartGlobal != nil,
			!livePrimaryCompletionInFlight
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
				"\(detail) x=\(Int(point.x.rounded())) y=\(Int(point.y.rounded())) inFlight=\(livePrimaryCompletionInFlight)"
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
		queuedPointerWorkItem?.cancel()
		queuedPointerWorkItem = nil
		queuedPointerEvent = nil
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
		let local = CGPoint(
			x: globalPoint.x - window.frame.minX,
			y: globalPoint.y - window.frame.minY
		)
		return bounds.contains(local) ? local : nil
	}

	private func currentLocalMousePoint() -> CGPoint? {
		guard let window else {
			return nil
		}
		let localPoint = window.mouseLocationOutsideOfEventStream
		return bounds.contains(localPoint) ? localPoint : nil
	}

	private func currentCursorPresentation() -> CursorPresentation {
		if pointerOverFrozenToolbar || hoveredToolbarAction != nil {
			return .arrow
		}
		if scene.mode == .frozen {
			if let interaction = chrome.frozenSelectionInteraction {
				return cursorPresentation(for: cursorIntent(for: interaction.kind, active: true))
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
						let intent = editableFrozenCursorIntent(at: pointer, selection: selection)
					{
						return cursorPresentation(for: intent)
					}
				}
			}
		}

		return cursorPresentation(for: scene.cursorIntent)
	}

	private func cursorPresentation(for intent: CursorIntent) -> CursorPresentation {
		switch intent {
		case .default:
			return .arrow
		case .crosshair:
			return .crosshair
		case .grab:
			return .openHand
		case .grabbing:
			return .closedHand
		case .resizeNorth, .resizeSouth:
			return .resizeUpDown
		case .resizeEast, .resizeWest:
			return .resizeLeftRight
		case .resizeNorthEast:
			return .resizeTopRight
		case .resizeNorthWest:
			return .resizeTopLeft
		case .resizeSouthEast:
			return .resizeBottomRight
		case .resizeSouthWest:
			return .resizeBottomLeft
		case .text:
			return .iBeam
		}
	}

	private func cursorIntent(
		for interactionKind: FrozenSelectionTransformKind,
		active: Bool
	) -> CursorIntent {
		switch interactionKind {
		case .move:
			return active ? .grabbing : .grab
		case .resizeLeft:
			return .resizeWest
		case .resizeRight:
			return .resizeEast
		case .resizeTop:
			return .resizeNorth
		case .resizeBottom:
			return .resizeSouth
		case .resizeTopLeft:
			return .resizeNorthWest
		case .resizeTopRight:
			return .resizeNorthEast
		case .resizeBottomLeft:
			return .resizeSouthWest
		case .resizeBottomRight:
			return .resizeSouthEast
		}
	}

	private func editableFrozenCursorIntent(at point: CGPoint, selection: CGRect) -> CursorIntent? {
		guard
			let kind = try? RsnapFrozenSelectionTransformPlanner.hitTest(
				point: point,
				selection: selection,
				handleRadius: 12,
				edgeTolerance: 4
			)
		else {
			return nil
		}
		return cursorIntent(for: kind, active: false)
	}

	private func cursor(for presentation: CursorPresentation) -> NSCursor {
		switch presentation {
		case .arrow:
			return .arrow
		case .crosshair:
			return .crosshair
		case .openHand:
			return .openHand
		case .closedHand:
			return .closedHand
		case .resizeUpDown:
			return .resizeUpDown
		case .resizeLeftRight:
			return .resizeLeftRight
		case .resizeTopLeft:
			return ._windowResizeTopLeft
		case .resizeTopRight:
			return ._windowResizeTopRight
		case .resizeBottomLeft:
			return ._windowResizeBottomLeft
		case .resizeBottomRight:
			return ._windowResizeBottomRight
		case .iBeam:
			return .iBeam
		}
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
		return NSScreen.screens.contains(where: { $0.frame.contains(globalPoint) })
			? globalPoint : nil
	}

	private func drawSelectionScrim(
		for focusRect: CGRect,
		in context: CGContext,
		alpha: CGFloat,
		excluding exclusionPath: CGPath? = nil
	) {
		let scrimColor = NSColor(calibratedWhite: 0, alpha: alpha)
		let visibleFocusRect = focusRect.intersection(bounds)
		if visibleFocusRect.isNull || visibleFocusRect.width <= 0 || visibleFocusRect.height <= 0 {
			context.setFillColor(scrimColor.cgColor)
			context.fill(bounds)
			return
		}

		context.saveGState()
		OverlayMaskGeometry.drawScrim(
			in: context,
			bounds: bounds,
			focusRect: visibleFocusRect,
			color: scrimColor.cgColor,
			pathExclusions: [exclusionPath].compactMap { $0 }
		)
		context.restoreGState()
	}

	private func drawLiveSelectionGlow(around rect: CGRect, in context: CGContext) {
		context.saveGState()
		context.setShadow(
			offset: .zero,
			blur: 12,
			color: NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.55).cgColor
		)
		let path = NSBezierPath(
			roundedRect: rect,
			xRadius: CaptureChrome.liveSelectionCornerRadius,
			yRadius: CaptureChrome.liveSelectionCornerRadius
		)
		context.setStrokeColor(
			NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 0.45).cgColor)
		context.setLineWidth(2.25)
		path.stroke()
		context.restoreGState()
	}

	private func drawDashedSelectionBorder(
		around rect: CGRect,
		in context: CGContext,
		lineWidth: CGFloat
	) {
		let outlineColor = NSColor(
			calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
		let strokeColor = NSColor(
			calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 248 / 255)
		let pixelsPerPoint = window?.screen?.backingScaleFactor ?? 1
		let borderOutset = CaptureChrome.dashedBorderOutset(
			strokeWidth: lineWidth,
			pixelsPerPoint: pixelsPerPoint
		)
		let borderRect = rect.insetBy(dx: -borderOutset, dy: -borderOutset)
		let path = CaptureChrome.dashedBorderPath(
			for: borderRect
		)

		context.saveGState()
		context.setLineCap(.butt)
		context.setLineJoin(.miter)

		context.addPath(path)
		context.setStrokeColor(outlineColor.cgColor)
		context.setLineWidth(lineWidth + 0.75)
		context.strokePath()

		context.addPath(path)
		context.setStrokeColor(strokeColor.cgColor)
		context.setLineWidth(lineWidth)
		context.strokePath()
		context.restoreGState()
	}

	private func drawFrozenResizeHandles(for rect: CGRect, in context: CGContext) {
		let outlineColor = NSColor(
			calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 124 / 255)
		let strokeColor = NSColor(
			calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 246 / 255)
		let leg = CaptureChrome.resizeHandleLegLength
		let offset = CaptureChrome.resizeHandleOffset
		let handles: [(CGPoint, CGPoint, CGPoint)]
		switch settings.frozenResizeHandleOrientation {
		case .outward:
			handles = [
				(
					CGPoint(x: rect.minX - offset - leg, y: rect.maxY + offset + leg),
					CGPoint(x: rect.minX - offset, y: rect.maxY + offset + leg),
					CGPoint(x: rect.minX - offset - leg, y: rect.maxY + offset)
				),
				(
					CGPoint(x: rect.maxX + offset + leg, y: rect.maxY + offset + leg),
					CGPoint(x: rect.maxX + offset, y: rect.maxY + offset + leg),
					CGPoint(x: rect.maxX + offset + leg, y: rect.maxY + offset)
				),
				(
					CGPoint(x: rect.minX - offset - leg, y: rect.minY - offset - leg),
					CGPoint(x: rect.minX - offset, y: rect.minY - offset - leg),
					CGPoint(x: rect.minX - offset - leg, y: rect.minY - offset)
				),
				(
					CGPoint(x: rect.maxX + offset + leg, y: rect.minY - offset - leg),
					CGPoint(x: rect.maxX + offset, y: rect.minY - offset - leg),
					CGPoint(x: rect.maxX + offset + leg, y: rect.minY - offset)
				),
			]
		case .inward:
			handles = [
				(
					CGPoint(x: rect.minX - offset, y: rect.maxY + offset),
					CGPoint(x: rect.minX - offset - leg, y: rect.maxY + offset),
					CGPoint(x: rect.minX - offset, y: rect.maxY + offset + leg)
				),
				(
					CGPoint(x: rect.maxX + offset, y: rect.maxY + offset),
					CGPoint(x: rect.maxX + offset + leg, y: rect.maxY + offset),
					CGPoint(x: rect.maxX + offset, y: rect.maxY + offset + leg)
				),
				(
					CGPoint(x: rect.minX - offset, y: rect.minY - offset),
					CGPoint(x: rect.minX - offset - leg, y: rect.minY - offset),
					CGPoint(x: rect.minX - offset, y: rect.minY - offset - leg)
				),
				(
					CGPoint(x: rect.maxX + offset, y: rect.minY - offset),
					CGPoint(x: rect.maxX + offset + leg, y: rect.minY - offset),
					CGPoint(x: rect.maxX + offset, y: rect.minY - offset - leg)
				),
			]
		}

		context.saveGState()
		context.setLineCap(.butt)
		context.setLineJoin(.miter)
		for (elbow, horizontal, vertical) in handles {
			let path = CGMutablePath()
			path.move(to: horizontal)
			path.addLine(to: elbow)
			path.addLine(to: vertical)

			context.addPath(path)
			context.setStrokeColor(outlineColor.cgColor)
			context.setLineWidth(CaptureChrome.resizeHandleStrokeWidth + 0.8)
			context.strokePath()

			context.addPath(path)
			context.setStrokeColor(strokeColor.cgColor)
			context.setLineWidth(CaptureChrome.resizeHandleStrokeWidth)
			context.strokePath()
		}
		context.restoreGState()
	}

	private func drawSelectionSizeBadge(for rect: CGRect, in context: CGContext) {
		let text = selectionSizeText(for: rect)
		let font = Self.hudLayoutMetrics.font
		let textSize = text.size(using: font)
		let badgeFrame = CaptureChrome.selectionSizeBadgeFrame(
			for: rect,
			textSize: textSize,
			in: bounds,
			avoiding: toolbarLayout(for: rect)?.frame
		)
		let anchor = badgeFrame.origin

		drawText(
			text, at: CGPoint(x: anchor.x, y: anchor.y - 1),
			color: NSColor.black.withAlphaComponent(0.6), font: font)
		drawText(
			text, at: CGPoint(x: anchor.x - 1, y: anchor.y),
			color: NSColor.black.withAlphaComponent(0.75), font: font)
		drawText(
			text, at: CGPoint(x: anchor.x + 1, y: anchor.y),
			color: NSColor.black.withAlphaComponent(0.75), font: font)
		drawText(
			text, at: CGPoint(x: anchor.x, y: anchor.y + 1),
			color: NSColor.black.withAlphaComponent(0.75), font: font)
		drawText(
			text, at: CGPoint(x: anchor.x, y: anchor.y),
			color: NSColor.white.withAlphaComponent(0.98), font: font)
	}

	private func drawFrozenOverlays(for selection: CGRect, in context: CGContext) {
		drawFrozenMosaics(for: selection, in: context)
		drawFrozenSpotlights(for: selection, in: context)
		drawFrozenPenStrokes(in: context)
		drawFrozenArrows(in: context)
		drawFrozenTextAnnotations(in: context)
	}

	private func drawFrozenMosaics(for selection: CGRect, in context: CGContext) {
		let mosaicRects = chrome.frozenOverlay.mosaicRects.compactMap(localRect(from:))
		let previewRect = chrome.frozenOverlay.previewMosaicRect.flatMap(localRect(from:))
		let allRects = mosaicRects + (previewRect.map { [$0] } ?? [])
		guard allRects.isEmpty == false, let baseImage = chrome.frozenBaseImage else {
			return
		}
		let imageSize = CGSize(width: CGFloat(baseImage.width), height: CGFloat(baseImage.height))

		context.saveGState()
		context.interpolationQuality = .none
		for rect in allRects {
			let imageRect = CGRect(
				x: ((rect.minX - selection.minX) / max(selection.width, 1))
					* imageSize.width,
				y: ((selection.maxY - rect.maxY) / max(selection.height, 1))
					* imageSize.height,
				width: (rect.width / max(selection.width, 1)) * imageSize.width,
				height: (rect.height / max(selection.height, 1)) * imageSize.height
			)
			guard let patch = makeFrozenMosaicPatch(from: baseImage, sourceRect: imageRect)
			else {
				continue
			}
			context.draw(patch, in: rect)
		}
		context.restoreGState()
	}

	private func drawFrozenSpotlights(for selection: CGRect, in context: CGContext) {
		let spotlightAnnotations: [(rect: CGRect, style: FrozenSpotlightStyle)] =
			chrome.frozenOverlay.spotlightAnnotations.compactMap { annotation in
				guard let rect = localRect(from: annotation.rect) else {
					return nil
				}
				return (rect: rect, style: annotation.style)
			}
		let previewAnnotation =
			chrome.frozenOverlay.previewSpotlightAnnotation.flatMap { annotation in
				localRect(from: annotation.rect).map { rect in
					(rect: rect, style: annotation.style)
				}
			}
		let allAnnotations = spotlightAnnotations + (previewAnnotation.map { [$0] } ?? [])
		guard allAnnotations.isEmpty == false else {
			return
		}

		context.saveGState()
		context.setFillColor(NSColor.black.withAlphaComponent(0.32).cgColor)
		context.fill(selection)
		context.setBlendMode(.clear)
		for annotation in allAnnotations {
			context.fill(annotation.rect)
		}
		context.restoreGState()

		for annotation in allAnnotations {
			drawFrozenSpotlightBorder(
				for: annotation.rect,
				style: annotation.style,
				scale: 1,
				alpha: 0.92,
				in: context
			)
		}
	}

	private func drawFrozenPenStrokes(in context: CGContext) {
		let allStrokes =
			chrome.frozenOverlay.penStrokes
			+ (chrome.frozenOverlay.previewPenStroke.map { [$0] } ?? [])
		guard allStrokes.isEmpty == false else {
			return
		}

		context.saveGState()
		context.setLineCap(.round)
		context.setLineJoin(.round)
		for stroke in allStrokes {
			guard let first = stroke.points.first.flatMap(localPoint(from:)) else {
				continue
			}
			context.setStrokeColor(stroke.style.color.nsColor(alpha: 0.96).cgColor)
			context.setLineWidth(stroke.style.strokeWidthPoints)
			context.beginPath()
			context.move(to: first)
			for point in stroke.points.dropFirst() {
				guard let localPoint = localPoint(from: point) else {
					continue
				}
				context.addLine(to: localPoint)
			}
			context.strokePath()
		}
		context.restoreGState()
	}

	private func drawFrozenArrows(in context: CGContext) {
		let arrows =
			chrome.frozenOverlay.arrowAnnotations
			+ (chrome.frozenOverlay.previewArrow.map { [$0] } ?? [])
		guard arrows.isEmpty == false else {
			return
		}

		for annotation in arrows {
			guard
				let localStart = localPoint(from: annotation.start),
				let localEnd = localPoint(from: annotation.end)
			else {
				continue
			}
			drawFrozenArrow(
				from: localStart,
				to: localEnd,
				style: annotation.style,
				scale: 1,
				in: context
			)
		}
	}

	private func drawFrozenTextAnnotations(in context: CGContext) {
		for annotation in chrome.frozenOverlay.textAnnotations {
			guard let point = localPoint(from: annotation.anchor) else {
				continue
			}
			drawFrozenText(
				annotation.text, at: point, style: annotation.style, scale: 1, in: context)
		}
		if let previewText = chrome.frozenOverlay.previewTextAnnotation,
			let point = localPoint(from: previewText.anchor)
		{
			drawFrozenText(
				previewText.text, at: point, style: previewText.style, scale: 1, in: context)
		}
		if let activeTextEdit = chrome.frozenOverlay.activeTextEdit,
			let point = localPoint(from: activeTextEdit.anchor)
		{
			drawFrozenText(
				activeTextEdit.text + "│",
				at: point,
				style: chrome.annotationStyle.textStyle,
				scale: 1,
				in: context
			)
		}
	}

	private func drawFrozenText(
		_ text: String,
		at point: CGPoint,
		style: FrozenTextStyle,
		scale: CGFloat,
		in context: CGContext
	) {
		guard text.isEmpty == false else {
			return
		}

		let font = NSFont.systemFont(ofSize: max(1, style.fontSizePoints * scale), weight: .medium)
		let attributes: [NSAttributedString.Key: Any] = [
			.font: font,
			.foregroundColor: style.color.nsColor(),
		]
		let attributed = NSAttributedString(string: text, attributes: attributes)
		context.saveGState()
		context.setShadow(
			offset: CGSize(width: 0, height: 1), blur: 4,
			color: style.color.textShadowColor.cgColor)
		let graphicsContext = NSGraphicsContext(cgContext: context, flipped: false)
		NSGraphicsContext.saveGraphicsState()
		NSGraphicsContext.current = graphicsContext
		attributed.draw(at: point)
		NSGraphicsContext.restoreGraphicsState()
		context.restoreGState()
	}

	private func toolbarLayout(for selection: CGRect) -> FrozenToolbarLayout? {
		let items = visibleToolbarItems()
		guard items.isEmpty == false else {
			return nil
		}

		var styleKind: FrozenAnnotationStyleToolbarKind?
		for item in items where item.enabled && item.selected {
			if let kind = FrozenAnnotationStyleToolbarKind(selectedTool: item.kind) {
				styleKind = kind
				break
			}
		}
		let metrics = CaptureChrome.toolbarMetrics()
		let itemCount = CGFloat(items.count)
		let primaryContentWidth =
			itemCount * metrics.buttonSize
			+ max(0, itemCount - 1) * metrics.itemSpacing
		let styleContentWidth =
			styleKind.map { annotationStyleContentWidth(for: $0, metrics: metrics) } ?? 0
		let contentWidth = max(primaryContentWidth, styleContentWidth)
		let width = contentWidth + metrics.horizontalPadding * 2
		let primaryRowHeight = metrics.verticalPadding * 2 + metrics.buttonSize
		let height = styleKind == nil ? primaryRowHeight : primaryRowHeight * 2
		let desiredY = selection.maxY + metrics.gap
		let wantsTop = settings.toolbarPlacement == .top
		let placedAbove =
			wantsTop || desiredY + height > bounds.maxY - CaptureChrome.toolbarScreenMargin
		let y =
			placedAbove
			? max(
				bounds.minY + CaptureChrome.toolbarScreenMargin,
				selection.minY - metrics.gap - height)
			: min(bounds.maxY - CaptureChrome.toolbarScreenMargin - height, desiredY)
		let minX = bounds.minX + CaptureChrome.toolbarScreenMargin
		let maxX = max(minX, bounds.maxX - CaptureChrome.toolbarScreenMargin - width)
		let x = (selection.midX - width / 2).clamped(to: minX...maxX)
		let frame = CGRect(x: x, y: y, width: width, height: height)
		let toolbarAboveSelection = frame.midY >= selection.midY
		let primaryY =
			if styleKind == nil {
				frame.midY - metrics.buttonSize / 2
			} else if toolbarAboveSelection {
				frame.minY + metrics.verticalPadding
			} else {
				frame.maxY - metrics.verticalPadding - metrics.buttonSize
			}
		var itemFrames: [FrozenToolbarItemLayout] = []
		var cursorX = frame.midX - primaryContentWidth / 2
		for item in items {
			let itemFrame = CGRect(
				x: cursorX,
				y: primaryY,
				width: metrics.buttonSize,
				height: metrics.buttonSize
			)
			itemFrames.append(
				FrozenToolbarItemLayout(
					kind: item.kind,
					frame: itemFrame,
					enabled: item.enabled,
					selected: item.selected
				)
			)
			cursorX += metrics.buttonSize + metrics.itemSpacing
		}

		let styleLayout: FrozenAnnotationStyleLayout?
		if let styleKind {
			styleLayout = annotationStyleLayout(
				for: styleKind,
				in: frame,
				contentWidth: styleContentWidth,
				metrics: metrics,
				toolbarAboveSelection: toolbarAboveSelection
			)
		} else {
			styleLayout = nil
		}

		return FrozenToolbarLayout(
			scale: metrics.scale,
			frame: frame,
			items: itemFrames,
			annotationStyle: styleLayout
		)
	}

	private func annotationStyleContentWidth(
		for kind: FrozenAnnotationStyleToolbarKind,
		metrics: CaptureChrome.ToolbarMetrics
	) -> CGFloat {
		let swatchCount = CGFloat(FrozenAnnotationColor.allCases.count)
		let swatchesWidth =
			swatchCount * metrics.annotationSwatchSize
			+ max(0, swatchCount - 1) * metrics.annotationSwatchGap
		return kind.sizeControlWidth(scale: metrics.scale)
			+ metrics.annotationStyleControlGap
			+ swatchesWidth
	}

	private func annotationStyleLayout(
		for kind: FrozenAnnotationStyleToolbarKind,
		in frame: CGRect,
		contentWidth: CGFloat,
		metrics: CaptureChrome.ToolbarMetrics,
		toolbarAboveSelection: Bool
	) -> FrozenAnnotationStyleLayout {
		let rowY =
			toolbarAboveSelection
			? frame.maxY - metrics.verticalPadding - metrics.annotationStyleRowHeight
			: frame.minY + metrics.verticalPadding
		let rowFrame = CGRect(
			x: frame.midX - contentWidth / 2,
			y: rowY,
			width: contentWidth,
			height: metrics.annotationStyleRowHeight
		)
		let sizeControlFrame = CGRect(
			x: rowFrame.minX,
			y: rowFrame.minY,
			width: kind.sizeControlWidth(scale: metrics.scale),
			height: rowFrame.height
		)
		let decreaseFrame = CGRect(
			x: sizeControlFrame.minX,
			y: sizeControlFrame.minY,
			width: metrics.annotationSizeButtonWidth,
			height: sizeControlFrame.height
		)
		let increaseFrame = CGRect(
			x: sizeControlFrame.maxX - metrics.annotationSizeButtonWidth,
			y: sizeControlFrame.minY,
			width: metrics.annotationSizeButtonWidth,
			height: sizeControlFrame.height
		)
		let displayFrame = CGRect(
			x: decreaseFrame.maxX,
			y: sizeControlFrame.minY,
			width: max(0, increaseFrame.minX - decreaseFrame.maxX),
			height: sizeControlFrame.height
		)
		var swatches: [FrozenAnnotationColorSwatchLayout] = []
		var swatchX = sizeControlFrame.maxX + metrics.annotationStyleControlGap
		for color in FrozenAnnotationColor.allCases {
			let swatchFrame = CGRect(
				x: swatchX,
				y: rowFrame.midY - metrics.annotationSwatchSize / 2,
				width: metrics.annotationSwatchSize,
				height: metrics.annotationSwatchSize
			)
			swatches.append(
				FrozenAnnotationColorSwatchLayout(
					color: color,
					frame: swatchFrame,
					selected: kind.selectedColor(in: chrome.annotationStyle) == color
				))
			swatchX += metrics.annotationSwatchSize + metrics.annotationSwatchGap
		}
		return FrozenAnnotationStyleLayout(
			kind: kind,
			scale: metrics.scale,
			frame: rowFrame,
			sizeControlFrame: sizeControlFrame,
			decreaseFrame: decreaseFrame,
			increaseFrame: increaseFrame,
			displayFrame: displayFrame,
			swatches: swatches
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
		var items: [ToolbarItem] = []
		let scrollCaptureActive = chrome.scrollMinimapPreview != nil
		for originalItem in scene.toolbarItems {
			var item = originalItem
			switch item.kind {
			case .pointer, .pen, .arrow, .mosaic, .spotlight, .text:
				item.enabled = originalItem.enabled && !scrollCaptureActive
			case .undo:
				item.enabled = chrome.frozenOverlay.canUndo && !scrollCaptureActive
			case .redo:
				item.enabled = chrome.frozenOverlay.canRedo && !scrollCaptureActive
			case .autoCenter:
				item.enabled =
					scene.frozenSelection != nil
					&& !chrome.frozenOverlay.keepsFrozenSelectionFixed
					&& !scrollCaptureActive
			case .scroll:
				item.enabled = controller?.scrollCaptureToolbarEnabled ?? false
			case .ocr:
				item.enabled =
					originalItem.enabled && !chrome.frozenOverlay.hasRecognizeTextBlockingEdits
			case .copy, .save:
				item.enabled = originalItem.enabled
			}
			items.append(item)
		}
		return items
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

	private func frozenToolbarHitState(at point: CGPoint) -> (
		pointerOverToolbar: Bool,
		toolbarAction: ToolbarItemKind?,
		annotationStyleAction: FrozenAnnotationStyleAction?
	) {
		guard scene.mode == .frozen, let selection = localFrozenSelectionRect(),
			let layout = toolbarLayout(for: selection)
		else {
			return (false, nil, nil)
		}

		var hoveredAction: ToolbarItemKind?
		for item in layout.items where item.enabled {
			if item.frame.contains(point) {
				hoveredAction = item.kind
				break
			}
		}

		var hoveredStyleAction: FrozenAnnotationStyleAction?
		if let styleLayout = layout.annotationStyle {
			if styleLayout.decreaseFrame.contains(point) {
				hoveredStyleAction = .decreaseSize
			} else if styleLayout.increaseFrame.contains(point) {
				hoveredStyleAction = .increaseSize
			} else {
				for swatch in styleLayout.swatches where swatch.frame.contains(point) {
					hoveredStyleAction = .color(swatch.color)
					break
				}
			}
		}

		return (layout.frame.contains(point), hoveredAction, hoveredStyleAction)
	}

	private func clearHoveredToolbarAction() {
		guard
			pointerOverFrozenToolbar || hoveredToolbarAction != nil
				|| hoveredAnnotationStyleAction != nil
		else {
			return
		}
		pointerOverFrozenToolbar = false
		hoveredToolbarAction = nil
		hoveredAnnotationStyleAction = nil
	}

	private func refreshHoveredToolbarAction(for localPoint: CGPoint? = nil) {
		let probePoint = scene.mode == .frozen ? (localPoint ?? currentLocalMousePoint()) : nil
		let hitState:
			(
				pointerOverToolbar: Bool,
				toolbarAction: ToolbarItemKind?,
				annotationStyleAction: FrozenAnnotationStyleAction?
			)
		if let probePoint {
			hitState = frozenToolbarHitState(at: probePoint)
		} else {
			hitState = (false, nil, nil)
		}
		let pointerOverToolbar = hitState.pointerOverToolbar
		let hoveredAction = hitState.toolbarAction
		let hoveredStyleAction = hitState.annotationStyleAction
		if hoveredToolbarAction != hoveredAction
			|| hoveredAnnotationStyleAction != hoveredStyleAction
			|| pointerOverFrozenToolbar != pointerOverToolbar
		{
			pointerOverFrozenToolbar = pointerOverToolbar
			hoveredToolbarAction = hoveredAction
			hoveredAnnotationStyleAction = hoveredStyleAction
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
			allowsClassicGlass: !defersFrozenToolbarClassicGlassUntilAfterFirstDisplay
		)

		for item in layout.items {
			if hoveredToolbarAction == item.kind, item.enabled, !item.selected {
				context.setFillColor(palette.toolbarHoverBackground.cgColor)
				let radius = CaptureChrome.toolbarControlCornerRadius * layout.scale
				let hoverPath = NSBezierPath(
					roundedRect: item.frame,
					xRadius: radius,
					yRadius: radius
				)
				hoverPath.fill()
			}
			if item.selected {
				context.setFillColor(palette.toolbarSelectedBackground.cgColor)
				let radius = CaptureChrome.toolbarControlCornerRadius * layout.scale
				let selectedPath = NSBezierPath(
					roundedRect: item.frame,
					xRadius: radius,
					yRadius: radius
				)
				selectedPath.fill()
			}

			let symbolColor =
				item.enabled
				? (item.selected ? palette.toolbarSelectedIcon : palette.toolbarIcon)
				: palette.toolbarDisabledIcon
			drawToolbarGlyph(
				item.kind,
				selected: item.selected,
				in: item.frame,
				scale: layout.scale,
				color: symbolColor,
				context: context
			)
		}

		if let styleLayout = layout.annotationStyle {
			FrozenToolbarDrawing.drawAnnotationStyleControls(
				styleLayout,
				state: chrome.annotationStyle,
				hoveredAction: hoveredAnnotationStyleAction,
				palette: palette,
				in: context
			)
		}
	}

	private func drawToolbarGlyph(
		_ kind: ToolbarItemKind,
		selected: Bool,
		in rect: CGRect,
		scale: CGFloat,
		color: NSColor,
		context: CGContext
	) {
		let glyph = PhosphorToolbarIcons.cachedGlyph(
			for: kind,
			selected: selected,
			size: CaptureChrome.toolbarGlyphSize * scale
		)
		let origin = CGPoint(
			x: rect.midX - glyph.bounds.width * 0.5 - glyph.bounds.origin.x,
			y: rect.midY - glyph.bounds.height * 0.5 - glyph.bounds.origin.y
		)
		context.saveGState()
		context.setFillColor(color.cgColor)
		context.textMatrix = .identity
		context.textPosition = origin
		CTLineDraw(glyph.line, context)
		context.restoreGState()
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

	private func applyVisibleCursorIfNeeded(_ cursorPresentation: CursorPresentation) {
		guard cursorPresentation != lastAppliedCursorPresentation else {
			return
		}
		lastAppliedCursorPresentation = cursorPresentation
		cursor(for: cursorPresentation).set()
	}

	private func currentHudPlacement() -> LiveFloatingPlacement? {
		guard scene.mode == .live, let anchor = localPointer() else {
			return nil
		}
		return liveFloatingPlacement(
			anchor: anchor,
			size: currentHudSize(),
			offsetX: 48,
			offsetY: 24,
			preferBelow: true
		)
	}

	private func currentHudSize() -> CGSize {
		let metrics = Self.hudLayoutMetrics
		let swatchSize = CaptureChrome.hudSwatchSize
		let keycapVisible = settings.showAltHintKeycap
		let keycapFrame = keycapVisible ? metrics.keycapFrameSize : .zero
		let contentHeight = max(metrics.lineHeight, swatchSize.height, keycapFrame.height)
		let positionDisplay = currentPositionDisplay()
		let contentWidth =
			positionDisplay.xSlotWidth
			+ metrics.commaWidth
			+ positionDisplay.ySlotWidth
			+ CaptureChrome.hudGroupSpacing
			+ swatchSize.width
			+ CaptureChrome.hudColorItemSpacing
			+ metrics.hexSlotWidth
			+ (keycapVisible
				? CaptureChrome.hudGroupSpacing + keycapFrame.width
				: 0)
		let size = CGSize(
			width: contentWidth + CaptureChrome.hudInnerMarginX * 2,
			height: contentHeight + CaptureChrome.hudInnerMarginY * 2
		)
		return size
	}

	private func currentHudFrame() -> CGRect? {
		currentHudPlacement()?.frame
	}

	private func currentLoupeFrame(
		hudFrame: CGRect,
		patch: CGImage?,
		alignTrailing: Bool
	) -> CGRect? {
		guard let patch else {
			return nil
		}
		let innerSide = CGFloat(patch.width) * CaptureChrome.loupeCellSize
		let size = CGSize(width: innerSide + 20, height: innerSide + 20)
		return liveStackedRect(
			referenceFrame: hudFrame,
			size: size,
			gap: CaptureChrome.hudLoupeGap,
			preferBelow: true,
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
		if pendingFrozenFirstDisplay {
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
		let rgbSample = cachedLiveRgbSample(matching: livePointerPreviewGlobal ?? scene.pointer)?
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
		guard pendingFrozenFirstDisplay else {
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
			colorDisplay: currentLiveColorDisplay(for: latestLiveRgbSample?.rgb),
			rgbSample: latestLiveRgbSample?.rgb,
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

		if livePrimaryCompletionInFlight == false {
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
			?? (usesSceneDragPreview && liveDragStartGlobal != nil && liveDragExceededThreshold
				? localRect(from: scene.liveSelectionPreview) : nil)
		let hoverSelectionLocal =
			dragSelectionLocal == nil
			? localRect(from: liveHighlightedWindowPreview?.frame)
			: nil
		let positionDisplay = currentPositionDisplay()
		let colorDisplay = currentLiveColorDisplay(for: rgbSample)
		let hudPlacement = liveHoverChromeSuppressed ? nil : currentHudPlacement()
		let hudFrame = hudPlacement?.frame
		let loupeFrame =
			!liveHoverChromeSuppressed && scene.loupeVisible
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
		guard liveDragStartGlobal == nil, !liveHoverChromeSuppressed else {
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
		let hudPlacement = liveHoverChromeSuppressed ? nil : currentHudPlacement()
		let hudFrame = hudPlacement?.frame
		let loupeFrame =
			!liveHoverChromeSuppressed && scene.loupeVisible
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

	private func liveFloatingPlacement(
		anchor: CGPoint,
		size: CGSize,
		offsetX: CGFloat,
		offsetY: CGFloat,
		preferBelow: Bool
	) -> LiveFloatingPlacement {
		let minX: CGFloat = 6
		let minY: CGFloat = 6
		let maxX = max(bounds.width - size.width - 6, minX)
		let maxY = max(bounds.height - size.height - 6, minY)

		var x = anchor.x + offsetX
		var flippedHorizontally = false
		if x + size.width > bounds.width - 6 {
			x = anchor.x - offsetX - size.width
			flippedHorizontally = true
		}
		x = x.clamped(to: minX...maxX)

		let preferredBelowY = anchor.y - offsetY - size.height
		let preferredAboveY = anchor.y + offsetY
		var y = preferBelow ? preferredBelowY : preferredAboveY
		if preferBelow {
			if y < minY {
				y = preferredAboveY
			}
		} else if y + size.height > bounds.height - 6 {
			y = preferredBelowY
		}
		y = y.clamped(to: minY...maxY)

		return LiveFloatingPlacement(
			frame: CGRect(origin: CGPoint(x: x, y: y), size: size),
			flippedHorizontally: flippedHorizontally
		)
	}

	private func liveStackedRect(
		referenceFrame: CGRect,
		size: CGSize,
		gap: CGFloat,
		preferBelow: Bool,
		alignTrailing: Bool = false
	) -> CGRect {
		let minX: CGFloat = 6
		let minY: CGFloat = 6
		let maxX = max(bounds.width - size.width - 6, minX)
		let maxY = max(bounds.height - size.height - 6, minY)

		var x = alignTrailing ? (referenceFrame.maxX - size.width) : referenceFrame.minX
		if alignTrailing == false, x + size.width > bounds.width - 6 {
			x = referenceFrame.maxX - size.width
		}
		x = x.clamped(to: minX...maxX)

		let preferredBelowY = referenceFrame.minY - gap - size.height
		let preferredAboveY = referenceFrame.maxY + gap
		var y = preferBelow ? preferredBelowY : preferredAboveY
		if preferBelow {
			if y < minY {
				y = preferredAboveY
			}
		} else if y + size.height > bounds.height - 6 {
			y = preferredBelowY
		}
		y = y.clamped(to: minY...maxY)

		return CGRect(origin: CGPoint(x: x, y: y), size: size)
	}

	private func updateLiveRendererState() {
		guard liveRendererInstalled else {
			return
		}
		guard scene.mode == .live || pendingFrozenFirstDisplay else {
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
		pendingFrozenFirstDisplay = false
		frozenFirstDisplayHandoffStartedAt = nil
		frozenFirstDisplayPendingFrameDisplayed = false
		defersFrozenToolbarClassicGlassUntilAfterFirstDisplay = false
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
			includeLoupePatch: scene.loupeVisible && !liveHoverChromeSuppressed
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
		let wantsLoupePatch = scene.loupeVisible && !liveHoverChromeSuppressed
		let sample = controller?.liveChromeSnapshot(
			point: point,
			settings: settings,
			includeLoupePatch: wantsLoupePatch
		)
		if let sample {
			let resolvedSample = sampleWithCachedLoupePatch(
				sample,
				point: point,
				wantsLoupePatch: wantsLoupePatch
			)
			seedLiveChromeSampleCache(resolvedSample, point: point)
			if let rgbSample = resolvedSample.rgb {
				seedLiveRgbSampleCache(rgbSample, point: point)
			}
			return resolvedSample
		}
		if let cachedSample = cachedLiveChromeSample(matching: point) {
			return cachedSample
		}
		if chrome.loupePatch != nil,
			liveSamplePoint(scene.pointer, matches: point)
		{
			seedLiveChromeSampleCache(from: chrome, point: scene.pointer)
			return cachedLiveChromeSample(matching: point)
		}
		if wantsLoupePatch, let cachedPatch = reusableLiveLoupePatch() {
			return LiveChromeSample(rgb: nil, loupePatch: cachedPatch)
		}
		return nil
	}

	private func sampleWithCachedLoupePatch(
		_ sample: LiveChromeSample,
		point: CGPoint?,
		wantsLoupePatch: Bool
	) -> LiveChromeSample {
		guard wantsLoupePatch, sample.loupePatch == nil else {
			return sample
		}
		if let cachedSample = cachedLiveChromeSample(matching: point),
			let cachedPatch = cachedSample.loupePatch
		{
			return LiveChromeSample(
				rgb: sample.rgb,
				loupePatch: cachedPatch
			)
		}
		if let cachedPatch = reusableLiveLoupePatch() {
			return LiveChromeSample(
				rgb: sample.rgb,
				loupePatch: cachedPatch
			)
		}
		if liveSamplePoint(scene.pointer, matches: point), let chromePatch = chrome.loupePatch,
			liveLoupePatchMatchesCurrentSize(chromePatch)
		{
			return LiveChromeSample(
				rgb: sample.rgb,
				loupePatch: chromePatch
			)
		}
		return sample
	}

	private func reusableLiveLoupePatch() -> CGImage? {
		if let patch = latestLiveChromeSample?.loupePatch,
			liveLoupePatchMatchesCurrentSize(patch)
		{
			return patch
		}
		if let patch = chrome.loupePatch,
			liveLoupePatchMatchesCurrentSize(patch)
		{
			return patch
		}
		return nil
	}

	private func liveLoupePatchMatchesCurrentSize(_ patch: CGImage) -> Bool {
		let sidePixels = settings.loupeSampleSize.sidePixels
		return patch.width == sidePixels && patch.height == sidePixels
	}

	private func liveRgbSample(from sample: LiveChromeSample?, at point: CGPoint?) -> RGBSample? {
		if let rgbSample = sample?.rgb,
			rgbSample.isFresh()
		{
			seedLiveRgbSampleCache(rgbSample, point: point)
			return rgbSample.rgb
		}
		return cachedLiveRgbSample(matching: point)?.rgb
	}

	private func seedLiveChromeSampleCache(from chrome: CaptureChromeState, point: CGPoint?) {
		guard chrome.loupePatch != nil else {
			return
		}
		seedLiveChromeSampleCache(
			LiveChromeSample(
				rgb: nil,
				loupePatch: chrome.loupePatch
			),
			point: point
		)
	}

	private func seedLiveChromeSampleCache(_ sample: LiveChromeSample, point: CGPoint?) {
		latestLiveChromeSample = sample
		latestLiveChromeSamplePoint = point
	}

	private func seedLiveRgbSampleCache(_ rgbSample: LiveRgbSample, point: CGPoint?) {
		latestLiveRgbSample = rgbSample
		latestLiveRgbSamplePoint = point
	}

	private func cachedLiveChromeSample(matching point: CGPoint?) -> LiveChromeSample? {
		guard liveSamplePoint(latestLiveChromeSamplePoint, matches: point) else {
			return nil
		}
		guard let latestLiveChromeSample else {
			return nil
		}
		guard latestLiveChromeSample.rgb == nil || latestLiveChromeSample.rgb?.isFresh() == true
		else {
			return LiveChromeSample(rgb: nil, loupePatch: latestLiveChromeSample.loupePatch)
		}
		return latestLiveChromeSample
	}

	private func cachedLiveRgbSample(matching point: CGPoint?) -> LiveRgbSample? {
		guard liveSamplePoint(latestLiveRgbSamplePoint, matches: point) else {
			return nil
		}
		guard latestLiveRgbSample?.isFresh(maximumAge: LiveRgbSample.maximumReusableAge) == true
		else {
			return nil
		}
		return latestLiveRgbSample
	}

	private func liveSamplePoint(_ samplePoint: CGPoint?, matches point: CGPoint?) -> Bool {
		switch (samplePoint, point) {
		case (nil, nil):
			return true
		case (let samplePoint?, let point?):
			return Self.liveSamplePointsEquivalent(samplePoint, point)
		default:
			return false
		}
	}

	private static func liveSamplePointsEquivalent(_ lhs: CGPoint, _ rhs: CGPoint) -> Bool {
		abs(lhs.x - rhs.x) <= 0.5 && abs(lhs.y - rhs.y) <= 0.5
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
		let metrics = Self.hudLayoutMetrics
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
			xSlotWidth: Self.coordinateSlotWidth(
				prefixWidth: metrics.xPrefixWidth,
				valueText: xValueText,
				metrics: metrics
			),
			ySlotWidth: Self.coordinateSlotWidth(
				prefixWidth: metrics.yPrefixWidth,
				valueText: yValueText,
				metrics: metrics
			)
		)
	}

	private static func coordinateSlotWidth(
		prefixWidth: CGFloat,
		valueText: String,
		metrics: HudLayoutMetrics
	) -> CGFloat {
		prefixWidth
			+ valueText.reduce(CGFloat(0)) { width, character in
				width + (character == "-" ? metrics.minusWidth : metrics.digitWidth)
			}
	}

	private func currentLiveColorDisplay(for sample: RGBSample?) -> LiveColorDisplay {
		let hexText =
			sample.map { String(format: "#%02X%02X%02X", $0.r, $0.g, $0.b) }
			?? pendingLiveColorHexText()
		return LiveColorDisplay(
			hexText: hexText,
			hexSlotWidth: Self.hudLayoutMetrics.hexSlotWidth,
			isPending: sample == nil
		)
	}

	private func pendingLiveColorHexText() -> String {
		let uptime = ProcessInfo.processInfo.systemUptime
		let digits = (0..<6).map { index -> Character in
			let rate = 9 + ((index * 7) % 6)
			let phase = Double((index * 23) % 31) / 31.0
			let tick = Int(((uptime + phase) * Double(rate)).rounded(.down))
			var seed =
				UInt64(tick + 1) &* 1_099_511_628_211
				^ UInt64(index + 1) &* 0x9E37_79B9_7F4A_7C15
			seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
			return Self.pendingHudHexWheel[Int((seed >> 58) & 0xF)]
		}
		return "#" + String(digits)
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

		glassPatchCache[surfaceKind] = GlassPatchCache(frame: frame, capturedAt: now, image: image)
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
		if let cached = scrollToolbarBackdropSeedPatchCache,
			abs(cached.frame.minX - globalFrame.minX) < 1,
			abs(cached.frame.minY - globalFrame.minY) < 1,
			abs(cached.frame.width - globalFrame.width) < 1,
			abs(cached.frame.height - globalFrame.height) < 1
		{
			return cached.image
		}
		guard
			let image = frozenDisplayPatch(
				in: globalFrame,
				displayFrame: scrollToolbarBackdropSeedFrame,
				image: scrollToolbarBackdropSeedImage
			)
		else {
			return nil
		}
		scrollToolbarBackdropSeedPatchCache = GlassPatchCache(
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

	private func drawText(_ text: String, at point: CGPoint, color: NSColor, font: NSFont) {
		(text as NSString).draw(
			at: point,
			withAttributes: [
				.font: font,
				.foregroundColor: color,
			])
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
			lastScrollToolbarBackdropCaptureStartedUptime = 0
			scrollToolbarBackdropFrame = nil
			scrollToolbarBackdropGlobalFrame = nil
			toolbarLiquidGlassBackdropView?.isHidden = true
			toolbarLiquidGlassBackdropView?.image = nil
			return
		}
		scrollToolbarBackdropFrame = toolbarFrame
		scrollToolbarBackdropGlobalFrame = globalFrame
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
		guard scrollToolbarBackdropCaptureInFlight == false,
			let scrollCaptureState = controller?.scrollCaptureState,
			let liveFrameStream = controller?.liveFrameStream
		else {
			return
		}
		let now = ProcessInfo.processInfo.systemUptime
		guard
			lastScrollToolbarBackdropCaptureStartedUptime == 0
				|| now - lastScrollToolbarBackdropCaptureStartedUptime
					>= Self.scrollToolbarBackdropCaptureMinimumInterval
		else {
			return
		}
		lastScrollToolbarBackdropCaptureStartedUptime = now
		let fallbackPermitted =
			lastScrollToolbarBackdropFallbackCaptureStartedUptime == 0
			|| now - lastScrollToolbarBackdropFallbackCaptureStartedUptime
				>= Self.scrollToolbarBackdropFallbackMinimumInterval
		if fallbackPermitted {
			lastScrollToolbarBackdropFallbackCaptureStartedUptime = now
		}
		scrollToolbarBackdropCaptureInFlight = true
		scrollToolbarBackdropCaptureGeneration &+= 1
		let generation = scrollToolbarBackdropCaptureGeneration
		let afterFrameSequence = scrollToolbarBackdropLastFrameSequence
		let previousSignature = scrollToolbarBackdropLastSignature
		let fallbackSource = scrollCaptureState.captureSource
		let maximumLiveFrameAgeMicroseconds = UInt64(
			CaptureSessionController.scrollCaptureActiveInputLiveFrameMaxAge * 1_000_000
		)
		DispatchQueue.main.async { [weak self] in
			guard let self, self.scrollToolbarBackdropCaptureGeneration == generation else {
				self?.scrollToolbarBackdropCaptureInFlight = false
				return
			}
			self.scrollToolbarBackdropCaptureQueue.async {
				[
					toolbarFrame, globalFrame, liveFrameStream, afterFrameSequence, fallbackSource,
					maximumLiveFrameAgeMicroseconds, previousSignature, fallbackPermitted,
				] in
				let rawFrame = liveFrameStream.nextRegionFrame(
					in: globalFrame,
					afterFrameSequence: afterFrameSequence,
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
					|| (previousSignature != nil && liveSignature == previousSignature)
				let fallbackPatch =
					liveWouldRemainStatic && fallbackPermitted
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
					&& (previousSignature == nil || fallbackSignature != previousSignature)
				let patch = shouldUseFallback ? fallbackPatch : (livePatch ?? fallbackPatch)
				let signature =
					shouldUseFallback ? fallbackSignature : (liveSignature ?? fallbackSignature)
				DispatchQueue.main.async { [weak self] in
					self?.finishScrollToolbarBackdropCapture(
						patch,
						toolbarFrame: toolbarFrame,
						generation: generation,
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
		guard scrollToolbarBackdropCaptureGeneration == generation else {
			return
		}
		scrollToolbarBackdropCaptureInFlight = false
		if let frameSequence {
			scrollToolbarBackdropLastFrameSequence = max(
				scrollToolbarBackdropLastFrameSequence,
				frameSequence
			)
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
		guard let signature else {
			return
		}
		let previousSignature = scrollToolbarBackdropLastSignature
		scrollToolbarBackdropLastSignature = signature
		guard previousSignature != signature else {
			return
		}
		let now = ProcessInfo.processInfo.systemUptime
		scrollToolbarBackdropChangedCount &+= 1
		if lastScrollToolbarBackdropChangedUptime > 0 {
			let gapMilliseconds = (now - lastScrollToolbarBackdropChangedUptime) * 1_000
			scrollToolbarBackdropChangedGapMetric.record(gapMilliseconds)
			if scrollToolbarBackdropChangedCount == 2
				|| scrollToolbarBackdropChangedCount.isMultiple(of: 30)
			{
				NativeHostTelemetry.captureEvent(
					"capture.scroll_toolbar_backdrop_changed",
					captureID: controller?.activeTelemetryCaptureID ?? 0,
					detail:
						"count=\(scrollToolbarBackdropChangedCount),gapMs=\(String(format: "%.2f", gapMilliseconds))"
				)
			}
		}
		lastScrollToolbarBackdropChangedUptime = now
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
		guard now - lastScrollCaptureToolbarBackdropRefreshUptime >= interval else {
			return
		}
		if lastScrollCaptureToolbarBackdropRefreshUptime > 0 {
			scrollToolbarBackdropRefreshGapMetric.record(
				(now - lastScrollCaptureToolbarBackdropRefreshUptime) * 1_000)
		}
		lastScrollCaptureToolbarBackdropRefreshUptime = now
		let refreshStartedAt = now
		if let scrollToolbarBackdropFrame, let scrollToolbarBackdropGlobalFrame {
			scheduleScrollToolbarBackdropCapture(
				toolbarFrame: scrollToolbarBackdropFrame,
				globalFrame: scrollToolbarBackdropGlobalFrame
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

	private func localAnnotationStyleLayout(
		_ layout: FrozenAnnotationStyleLayout,
		relativeTo toolbarFrame: CGRect
	) -> FrozenAnnotationStyleLayout {
		FrozenAnnotationStyleLayout(
			kind: layout.kind,
			scale: layout.scale,
			frame: layout.frame.offsetBy(dx: -toolbarFrame.minX, dy: -toolbarFrame.minY),
			sizeControlFrame: layout.sizeControlFrame.offsetBy(
				dx: -toolbarFrame.minX,
				dy: -toolbarFrame.minY
			),
			decreaseFrame: layout.decreaseFrame.offsetBy(
				dx: -toolbarFrame.minX,
				dy: -toolbarFrame.minY
			),
			increaseFrame: layout.increaseFrame.offsetBy(
				dx: -toolbarFrame.minX,
				dy: -toolbarFrame.minY
			),
			displayFrame: layout.displayFrame.offsetBy(
				dx: -toolbarFrame.minX,
				dy: -toolbarFrame.minY
			),
			swatches: layout.swatches.map { swatch in
				FrozenAnnotationColorSwatchLayout(
					color: swatch.color,
					frame: swatch.frame.offsetBy(
						dx: -toolbarFrame.minX,
						dy: -toolbarFrame.minY
					),
					selected: swatch.selected
				)
			}
		)
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
			hoveredToolbarAction: hoveredToolbarAction,
			hoveredAnnotationStyleAction: hoveredAnnotationStyleAction,
			toolbarScale: layout.scale,
			annotationStyleState: chrome.annotationStyle,
			annotationStyleLayout: layout.annotationStyle.map {
				localAnnotationStyleLayout($0, relativeTo: layout.frame)
			},
			items: layout.items.map { item in
				FrozenToolbarRenderView.Item(
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
		scrollToolbarBackdropCaptureGeneration &+= 1
		scrollToolbarBackdropCaptureInFlight = false
		lastScrollToolbarBackdropCaptureStartedUptime = 0
		scrollToolbarBackdropSeedFrame = nil
		scrollToolbarBackdropSeedImage = nil
		scrollToolbarBackdropSeedPatchCache = nil
		scrollToolbarBackdropLastFrameSequence = 0
		scrollToolbarBackdropLastSignature = nil
		scrollToolbarBackdropChangedCount = 0
		scrollToolbarBackdropFrame = nil
		scrollToolbarBackdropGlobalFrame = nil
		lastScrollToolbarBackdropChangedUptime = 0
		lastScrollToolbarBackdropFallbackCaptureStartedUptime = 0
		lastScrollCaptureToolbarBackdropRefreshUptime = 0
		toolbarLiquidGlassBackdropView?.isHidden = true
		toolbarLiquidGlassBackdropView?.image = nil
		toolbarLiquidGlassView?.isHidden = true
		toolbarLiquidGlassContentView?.isHidden = true
		if wasVisible {
			needsDisplay = true
		}
	}

	private func suppressLiveHoverChrome() {
		guard scene.mode == .live, !liveHoverChromeSuppressed else {
			return
		}
		liveHoverChromeSuppressed = true
		updateLivePreviewDemands()
		liveRenderer.renderNow()
	}

	private func themeBrightnessBias() -> Double {
		chromeTheme() == .dark ? 0.015 : -0.01
	}

	private func themeBrightnessBias(for theme: CaptureChromeTheme) -> Double {
		theme == .dark ? 0.015 : -0.01
	}

	private func queuePointerEvent(_ event: QueuedPointerEvent) {
		let now = ProcessInfo.processInfo.systemUptime
		let targetInterval = pointerDispatchInterval()
		let elapsed = now - lastPointerDispatchUptime(for: event)

		queuedPointerEvent = event
		guard queuedPointerWorkItem == nil else {
			return
		}

		let delay = max(0, targetInterval - elapsed)
		let workItem = DispatchWorkItem { [weak self] in
			guard let self else {
				return
			}
			self.queuedPointerWorkItem = nil
			guard let event = self.queuedPointerEvent else {
				return
			}
			self.queuedPointerEvent = nil
			self.setLastPointerDispatchUptime(ProcessInfo.processInfo.systemUptime, for: event)
			self.dispatchPointerEvent(event)
		}
		queuedPointerWorkItem = workItem
		if delay <= 0 {
			DispatchQueue.main.async(execute: workItem)
		} else {
			DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: workItem)
		}
	}

	private func dispatchPointerEvent(_ event: QueuedPointerEvent) {
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

	private func lastPointerDispatchUptime(for event: QueuedPointerEvent) -> TimeInterval {
		switch event {
		case .moved:
			return lastHoverPointerDispatchUptime
		case .liveDragged:
			return lastDragPointerDispatchUptime
		}
	}

	private func setLastPointerDispatchUptime(_ uptime: TimeInterval, for event: QueuedPointerEvent)
	{
		switch event {
		case .moved:
			lastHoverPointerDispatchUptime = uptime
		case .liveDragged:
			lastDragPointerDispatchUptime = uptime
		}
	}

}

extension NSCursor {
	private static func frozenDiagonalCursor(
		from baseCursor: NSCursor
	) -> NSCursor {
		NSCursor(image: baseCursor.image, hotSpot: baseCursor.hotSpot)
	}

	private static var _diagonalTopLeftBottomRight: NSCursor {
		if #available(macOS 15.0, *) {
			return frozenDiagonalCursor(
				from: .frameResize(position: .topLeft, directions: [.inward, .outward])
			)
		}
		return .crosshair
	}

	private static var _diagonalTopRightBottomLeft: NSCursor {
		if #available(macOS 15.0, *) {
			return frozenDiagonalCursor(
				from: .frameResize(position: .topRight, directions: [.inward, .outward])
			)
		}
		return .crosshair
	}

	fileprivate static var _windowResizeTopRight: NSCursor {
		_diagonalTopRightBottomLeft
	}

	fileprivate static var _windowResizeTopLeft: NSCursor {
		_diagonalTopLeftBottomRight
	}

	fileprivate static var _windowResizeBottomLeft: NSCursor {
		_diagonalTopRightBottomLeft
	}

	fileprivate static var _windowResizeBottomRight: NSCursor {
		_diagonalTopLeftBottomRight
	}
}

extension CGRect {
	fileprivate func clamp(_ point: CGPoint) -> CGPoint {
		CGPoint(
			x: point.x.clamped(to: minX...maxX),
			y: point.y.clamped(to: minY...maxY)
		)
	}

	fileprivate func normalizedRect(anchor: CGPoint, current: CGPoint) -> CGRect {
		let clampedAnchor = clamp(anchor)
		let clampedCurrent = clamp(current)
		return CGRect(
			x: min(clampedAnchor.x, clampedCurrent.x),
			y: min(clampedAnchor.y, clampedCurrent.y),
			width: abs(clampedCurrent.x - clampedAnchor.x),
			height: abs(clampedCurrent.y - clampedAnchor.y)
		)
	}
}

@MainActor
private enum FrozenToolbarDrawing {
	static func drawAnnotationStyleControls(
		_ layout: FrozenAnnotationStyleLayout,
		state: FrozenAnnotationStyleState,
		hoveredAction: FrozenAnnotationStyleAction?,
		palette: CaptureChromePalette,
		in context: CGContext
	) {
		drawSizeControl(
			layout,
			state: state,
			hoveredAction: hoveredAction,
			palette: palette,
			in: context
		)
		for swatch in layout.swatches {
			drawColorSwatch(swatch, palette: palette, in: context)
		}
	}

	private static func drawSizeControl(
		_ layout: FrozenAnnotationStyleLayout,
		state: FrozenAnnotationStyleState,
		hoveredAction: FrozenAnnotationStyleAction?,
		palette: CaptureChromePalette,
		in context: CGContext
	) {
		let sizeHovered = hoveredAction == .decreaseSize || hoveredAction == .increaseSize
		let scale = layout.scale
		let capsuleRect = layout.sizeControlFrame.insetBy(dx: 0, dy: 3 * scale)
		let capsulePath = NSBezierPath(
			roundedRect: capsuleRect,
			xRadius: CaptureChrome.toolbarControlCornerRadius * scale,
			yRadius: CaptureChrome.toolbarControlCornerRadius * scale
		)
		context.setFillColor(
			(sizeHovered
				? palette.toolbarHoverBackground.withAlphaComponent(0.72)
				: palette.toolbarHoverBackground.withAlphaComponent(0.42)).cgColor)
		capsulePath.fill()
		context.setStrokeColor(
			palette.outerStroke.withAlphaComponent(sizeHovered ? 0.52 : 0.36).cgColor)
		context.setLineWidth(max(0.5, scale))
		capsulePath.stroke()

		for (action, frame) in [
			(FrozenAnnotationStyleAction.decreaseSize, layout.decreaseFrame),
			(FrozenAnnotationStyleAction.increaseSize, layout.increaseFrame),
		] where hoveredAction == action {
			context.setFillColor(palette.toolbarHoverBackground.cgColor)
			NSBezierPath(
				roundedRect: frame.insetBy(dx: 2 * scale, dy: 4 * scale),
				xRadius: 6 * scale,
				yRadius: 6 * scale
			).fill()
		}

		context.setStrokeColor(palette.outerStroke.withAlphaComponent(0.34).cgColor)
		context.setLineWidth(max(0.5, scale))
		for dividerX in [layout.displayFrame.minX, layout.displayFrame.maxX] {
			context.beginPath()
			context.move(to: CGPoint(x: dividerX, y: capsuleRect.minY + 5 * scale))
			context.addLine(to: CGPoint(x: dividerX, y: capsuleRect.maxY - 5 * scale))
			context.strokePath()
		}

		let font = NSFont.monospacedSystemFont(
			ofSize: max(1, CaptureChrome.toolbarControlFontSize * scale),
			weight: .medium
		)
		drawCenteredText(
			"-",
			in: layout.decreaseFrame,
			font: font,
			color: palette.toolbarIcon,
			context: context
		)
		drawCenteredText(
			"+",
			in: layout.increaseFrame,
			font: font,
			color: palette.toolbarIcon,
			context: context
		)

		switch layout.kind {
		case .brush:
			drawBrushSizeDisplay(
				in: layout.displayFrame,
				state: state,
				scale: scale,
				font: font,
				color: palette.labelText,
				context: context
			)
		case .spotlight, .text:
			drawCenteredText(
				layout.kind.sizeLabel(in: state),
				in: layout.displayFrame,
				font: font,
				color: palette.labelText,
				context: context
			)
		}
	}

	private static func drawBrushSizeDisplay(
		in frame: CGRect,
		state: FrozenAnnotationStyleState,
		scale: CGFloat,
		font: NSFont,
		color: NSColor,
		context: CGContext
	) {
		let previewColor = state.brushStyle.color.nsColor(alpha: 0.96)
		let previewWidth = (state.brushStyle.strokeWidthPoints * scale).clamped(to: 0.5...10)
		let previewHalfLength = CaptureChrome.annotationPenPreviewLength * scale / 2
		let previewCenter = CGPoint(x: frame.minX + 10 * scale + previewHalfLength, y: frame.midY)
		let previewStart = CGPoint(x: previewCenter.x - previewHalfLength, y: previewCenter.y)
		let previewEnd = CGPoint(x: previewCenter.x + previewHalfLength, y: previewCenter.y)

		context.saveGState()
		context.setStrokeColor(previewColor.cgColor)
		context.setLineWidth(previewWidth)
		context.setLineCap(.round)
		context.beginPath()
		context.move(to: previewStart)
		context.addLine(to: previewEnd)
		context.strokePath()
		context.restoreGState()

		let label = FrozenAnnotationStyleToolbarKind.brush.sizeLabel(in: state)
		let labelSize = label.size(using: font)
		drawText(
			label,
			at: CGPoint(
				x: previewEnd.x + CaptureChrome.annotationSizePreviewGap * scale,
				y: frame.midY - labelSize.height / 2
			),
			font: font,
			color: color,
			context: context
		)
	}

	private static func drawColorSwatch(
		_ swatch: FrozenAnnotationColorSwatchLayout,
		palette: CaptureChromePalette,
		in context: CGContext
	) {
		let radius = swatch.frame.width / 2 - 1
		let center = CGPoint(x: swatch.frame.midX, y: swatch.frame.midY)
		let rect = CGRect(
			x: center.x - radius,
			y: center.y - radius,
			width: radius * 2,
			height: radius * 2
		)
		let path = NSBezierPath(ovalIn: rect)
		context.setFillColor(swatch.color.nsColor().cgColor)
		path.fill()
		context.setStrokeColor(
			(swatch.selected ? palette.toolbarSelectedIcon : palette.toolbarIcon)
				.withAlphaComponent(swatch.selected ? 0.95 : 0.56).cgColor)
		let scale = max(0.5, swatch.frame.width / max(CaptureChrome.annotationSwatchSize, 1))
		context.setLineWidth(swatch.selected ? 2 * scale : scale)
		path.stroke()
	}

	private static func drawCenteredText(
		_ text: String,
		in frame: CGRect,
		font: NSFont,
		color: NSColor,
		context: CGContext
	) {
		let size = text.size(using: font)
		drawText(
			text,
			at: CGPoint(x: frame.midX - size.width / 2, y: frame.midY - size.height / 2),
			font: font,
			color: color,
			context: context
		)
	}

	private static func drawText(
		_ text: String,
		at point: CGPoint,
		font: NSFont,
		color: NSColor,
		context: CGContext
	) {
		let graphicsContext = NSGraphicsContext(cgContext: context, flipped: false)
		NSGraphicsContext.saveGraphicsState()
		NSGraphicsContext.current = graphicsContext
		(text as NSString).draw(
			at: point,
			withAttributes: [
				.font: font,
				.foregroundColor: color,
			])
		NSGraphicsContext.restoreGraphicsState()
	}
}

extension String {
	func size(using font: NSFont) -> CGSize {
		(self as NSString).size(withAttributes: [.font: font])
	}
}
