import AppKit
import CoreGraphics
import Foundation
import QuartzCore
import RsnapHostBridge

@MainActor
final class CaptureHostMaterialViewCoordinator {
	private static let scrollToolbarBackdropCaptureMinimumInterval: TimeInterval = 1.0 / 60.0
	private static let scrollToolbarBackdropFallbackMinimumInterval: TimeInterval = 1.0 / 20.0
	private static let liveChromeLiquidGlassZ: CGFloat = 200
	private static let frozenToolbarLiquidGlassBackdropZ: CGFloat = 295
	private static let frozenToolbarLiquidGlassZ: CGFloat = 300
	private static let frozenToolbarContentZ: CGFloat = 320

	private unowned let hostView: CaptureHostView
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
	private var glassPatchResolver = CaptureHostGlassPatchResolver()

	init(hostView: CaptureHostView) {
		self.hostView = hostView
	}

	var isFrozenToolbarLiquidGlassVisible: Bool {
		frozenToolbarLiquidGlassVisible
	}

	var isFrozenToolbarLiquidGlassContentDrawn: Bool {
		frozenToolbarLiquidGlassContentDrawn
	}

	func resetScrollToolbarBackdropTracking(
		seedFrame: CGRect? = nil,
		seedImage: CGImage? = nil
	) {
		scrollToolbarBackdropState.resetTracking(seedFrame: seedFrame, seedImage: seedImage)
	}

	private var scene: SceneSnapshot { hostView.scene }
	private var chrome: CaptureChromeState { hostView.chrome }
	private var settings: NativeHostSettings { hostView.settings }
	private var controller: CaptureSessionController? { hostView.controller }
	private var toolbarHoverState: CaptureHostToolbarHoverState { hostView.toolbarHoverState }

	private func globalRect(from localRect: CGRect) -> CGRect? {
		hostView.globalRect(from: localRect)
	}

	private func currentDisplayTargetFramesPerSecond() -> Int {
		hostView.currentDisplayTargetFramesPerSecond()
	}

	private func localFrozenSelectionRect() -> CGRect? {
		hostView.localFrozenSelectionRect()
	}

	private func toolbarLayout(for selection: CGRect) -> FrozenToolbarLayout? {
		hostView.toolbarLayout(for: selection)
	}

	private func updateLiveChromeBackdrops() {
		hostView.updateLiveChromeBackdrops()
	}
	func glassPatch(
		for surfaceKind: CaptureHostGlassSurfaceKind,
		frame: CGRect
	) -> CGImage? {
		guard let globalFrame = globalRect(from: frame) else {
			return nil
		}
		return glassPatchResolver.patch(
			for: surfaceKind,
			frame: frame,
			globalFrame: globalFrame,
			now: ProcessInfo.processInfo.systemUptime,
			cacheInterval: glassPatchCacheInterval(),
			theme: chromeTheme(),
			settings: settings
		) { [weak self] globalFrame in
			self?.glassSourcePatch(in: globalFrame)
		}
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
		CaptureHostGlassPatchResolver.frozenDisplayPatch(
			in: globalFrame,
			displayFrame: displayFrame,
			image: image
		)
	}

	private func chromeTheme() -> CaptureChromeTheme {
		hostView.chromeTheme()
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

	func updateChromeMaterialViews() {
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

	func updateLiveLiquidGlassViews(hudFrame: CGRect?, loupeFrame: CGRect?) {
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

	func moveExistingLiveLiquidGlassViews(hudFrame: CGRect?, loupeFrame: CGRect?) {
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
			hostView.addSubview(createdView, positioned: .below, relativeTo: nil)
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
		hostView.addSubview(createdView, positioned: .below, relativeTo: nil)
		toolbarLiquidGlassView = createdView
		ensureFrozenToolbarContentView(above: createdView)
	}

	@discardableResult
	private func ensureFrozenToolbarBackdropView(below glassView: NSView) -> NSImageView {
		if let toolbarLiquidGlassBackdropView {
			if toolbarLiquidGlassBackdropView.superview !== hostView {
				hostView.addSubview(
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
		hostView.addSubview(backdropView, positioned: .below, relativeTo: glassView)
		toolbarLiquidGlassBackdropView = backdropView
		return backdropView
	}

	@discardableResult
	private func ensureFrozenToolbarContentView(above glassView: NSView) -> FrozenToolbarRenderView
	{
		if let toolbarLiquidGlassContentView {
			if toolbarLiquidGlassContentView.superview !== hostView {
				hostView.addSubview(
					toolbarLiquidGlassContentView, positioned: .above, relativeTo: glassView)
			}
			toolbarLiquidGlassContentView.layer?.zPosition = Self.frozenToolbarContentZ
			return toolbarLiquidGlassContentView
		}
		let contentView = FrozenToolbarRenderView(frame: .zero)
		configureFrozenToolbarContentView(contentView)
		hostView.addSubview(contentView, positioned: .above, relativeTo: glassView)
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

	func hideLiveLiquidGlassViews(removing: Bool = true) {
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
				hostView.needsDisplay = true
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
			hostView.needsDisplay = true
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
			hostView.needsDisplay = true
		}
	}
}
