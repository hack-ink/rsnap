import AppKit
import CoreGraphics
import Foundation
import QuartzCore
import RsnapHostBridge

enum LiveGlassSurfaceKind: Hashable {
	case hud
	case loupe
}

struct LivePreviewSnapshot {
	let bounds: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let frozenPending: Bool
	let frozenDisplayFrame: CGRect?
	let frozenDisplayImage: CGImage?
	let pointerLocal: CGPoint?
	let dragSelectionLocal: CGRect?
	let hoverSelectionLocal: CGRect?
	let selectionSizeText: String?
	let hudFrame: CGRect?
	let loupeFrame: CGRect?
	let positionDisplay: LivePositionDisplay
	let colorDisplay: LiveColorDisplay
	let rgbSample: RGBSample?
	let keycapVisible: Bool
	let inputUptime: TimeInterval?
	let loupePatch: CGImage?
	let glassPatches: [LiveGlassSurfaceKind: CGImage]
}

@MainActor
final class LiveOverlayRenderer {
	weak var hostView: NSView?
	let rootLayer = CALayer()
	private let chromeRootLayer = CALayer()
	let frozenDisplayLayer = CALayer()
	let scrimLayer = LiveScrimLayer()
	let topScrimLayer = CALayer()
	let leftScrimLayer = CALayer()
	let rightScrimLayer = CALayer()
	let bottomScrimLayer = CALayer()
	let hoverGlowLayer = CAShapeLayer()
	let hoverFlowLayer = SelectionFlowBandLayer()
	let dragBorderOutlineLayer = CAShapeLayer()
	let dragBorderLayer = CAShapeLayer()
	let selectionSizeLayer = CATextLayer()
	private let hudLayer = CALayer()
	private let hudGlassLayer = CALayer()
	private let hudFillLayer = CALayer()
	private let hudStrokeLayer = CAShapeLayer()
	private let hudPositionLayer = CATextLayer()
	private let hudHexLayer = CATextLayer()
	private let hudHexRollLayer = CALayer()
	private let hudSwatchLayer = CALayer()
	private let hudKeycapLayer = CALayer()
	private let hudKeycapTextLayer = CATextLayer()
	private let loupeLayer = CALayer()
	private let loupeGlassLayer = CALayer()
	private let loupeFillLayer = CALayer()
	private let loupeStrokeLayer = CAShapeLayer()
	private let loupePatchLayer = CALayer()
	private let loupeCenterLayer = CAShapeLayer()
	private let pointerLayer = PointerAccentLayer()
	private let frameClock = LiveFrameClockDriver()
	private let layerRenderDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.layer_render_duration",
		category: "LiveChromeTelemetry"
	)
	private let layerChromeRenderDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.layer_chrome_render_duration",
		category: "LiveChromeTelemetry"
	)
	private let snapshotDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.snapshot_duration",
		category: "LiveChromeTelemetry"
	)
	private let layerChromeRenderGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.layer_chrome_render_gap",
		category: "LiveChromeTelemetry"
	)
	private let activeLayerChromeRenderGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.active_layer_chrome_render_gap",
		category: "LiveChromeTelemetry"
	)
	private static let activeInputWindow: TimeInterval = 0.25
	private enum LayerZ {
		static let root: CGFloat = 100
		static let chromeRoot: CGFloat = 300
		static let frozenDisplay: CGFloat = 0
		static let scrim: CGFloat = 10
		static let selectionChrome: CGFloat = 30
		static let selectionSize: CGFloat = 40
		static let hudChrome: CGFloat = 1_000
		static let pointer: CGFloat = 1_200
	}

	private var snapshotProvider: (() -> LivePreviewSnapshot?)?
	var lastRenderedFocusRect: CGRect?
	private var lastRenderedFocusFlowAnimates = false
	private var lastChromeRenderUptime: TimeInterval?
	private var lastActiveChromeRenderUptime: TimeInterval?
	private lazy var hudColorRollCoordinator = ColorRollCoordinator(
		hudHexLayer: hudHexLayer,
		hudHexRollLayer: hudHexRollLayer,
		hudSwatchLayer: hudSwatchLayer,
		backingScaleProvider: { [weak self] in
			self?.hostView?.window?.backingScaleFactor ?? 2
		}
	)

	init(hostView: NSView) {
		self.hostView = hostView
		configureLayers()
		frameClock.onTick = { [weak self] in
			self?.renderFrameTick()
		}
	}

	func install(snapshotProvider: @escaping () -> LivePreviewSnapshot?) {
		self.snapshotProvider = snapshotProvider
		guard let hostView else {
			return
		}
		if hostView.layer == nil {
			hostView.wantsLayer = true
		}
		hostView.layer?.addSublayer(rootLayer)
		hostView.layer?.addSublayer(chromeRootLayer)
		rootLayer.isHidden = true
		chromeRootLayer.isHidden = true
	}

	func updateDisplayID(_ displayID: CGDirectDisplayID?, targetFramesPerSecond: Int) {
		guard displayID != nil else {
			stop()
			return
		}
		frameClock.start(targetFramesPerSecond: targetFramesPerSecond)
	}

	func stop() {
		frameClock.stop()
		hideRootAndResetRenderState()
	}

	func suspend() {
		hideRootAndResetRenderState()
	}

	func renderNow() {
		renderCurrentSnapshot()
	}

	func renderLiveChromeNow() {
		renderChromeSnapshot()
	}

	func moveLiveChrome(
		hudFrame: CGRect?,
		loupeFrame: CGRect?,
		chromeExclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		if let hudFrame, !hudLayer.isHidden, layerFrameNeedsUpdate(hudLayer.frame, hudFrame) {
			hudLayer.frame = hudFrame
		}
		if let loupeFrame, !loupeLayer.isHidden,
			layerFrameNeedsUpdate(loupeLayer.frame, loupeFrame)
		{
			loupeLayer.frame = loupeFrame
		}
		updateLiveScrimExclusions(excluding: chromeExclusions)
		updateLiveFlowExclusions(excluding: chromeExclusions)
		CATransaction.commit()
	}

	private func configureLayers() {
		rootLayer.zPosition = LayerZ.root
		rootLayer.masksToBounds = true
		chromeRootLayer.zPosition = LayerZ.chromeRoot
		chromeRootLayer.masksToBounds = true
		frozenDisplayLayer.isHidden = true
		frozenDisplayLayer.zPosition = LayerZ.frozenDisplay
		rootLayer.addSublayer(frozenDisplayLayer)
		scrimLayer.isHidden = true
		scrimLayer.zPosition = LayerZ.scrim
		rootLayer.addSublayer(scrimLayer)
		for scrimLayer in [topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer] {
			rootLayer.addSublayer(scrimLayer)
			scrimLayer.isHidden = true
			scrimLayer.zPosition = LayerZ.scrim
		}
		hoverGlowLayer.fillColor = NSColor.clear.cgColor
		hoverGlowLayer.lineWidth = 2.25
		hoverGlowLayer.shadowOffset = .zero
		hoverGlowLayer.shadowRadius = 12
		hoverGlowLayer.zPosition = LayerZ.selectionChrome
		rootLayer.addSublayer(hoverGlowLayer)

		hoverFlowLayer.zPosition = LayerZ.selectionChrome
		rootLayer.addSublayer(hoverFlowLayer)

		dragBorderOutlineLayer.fillColor = NSColor.clear.cgColor
		dragBorderOutlineLayer.zPosition = LayerZ.selectionChrome
		rootLayer.addSublayer(dragBorderOutlineLayer)

		dragBorderLayer.fillColor = NSColor.clear.cgColor
		dragBorderLayer.zPosition = LayerZ.selectionChrome
		rootLayer.addSublayer(dragBorderLayer)

		selectionSizeLayer.contentsScale = 2
		selectionSizeLayer.zPosition = LayerZ.selectionSize
		rootLayer.addSublayer(selectionSizeLayer)

		for chromeLayer in [hudLayer, loupeLayer] {
			chromeLayer.masksToBounds = false
			chromeLayer.zPosition = LayerZ.hudChrome
			chromeRootLayer.addSublayer(chromeLayer)
		}
		pointerLayer.zPosition = LayerZ.pointer
		chromeRootLayer.addSublayer(pointerLayer)
		for hudSublayer in [
			hudGlassLayer, hudFillLayer, hudStrokeLayer, hudSwatchLayer, hudPositionLayer,
			hudHexLayer, hudHexRollLayer, hudKeycapLayer, hudKeycapTextLayer,
		] {
			hudLayer.addSublayer(hudSublayer)
		}
		hudHexRollLayer.masksToBounds = false
		hudHexRollLayer.isHidden = true
		for loupeSublayer in [
			loupeGlassLayer, loupeFillLayer, loupeStrokeLayer, loupePatchLayer, loupeCenterLayer,
		] {
			loupeLayer.addSublayer(loupeSublayer)
		}
		for chromeLayer in [hudLayer, loupeLayer] {
			chromeLayer.isHidden = true
		}
	}

	private func renderCurrentSnapshot() {
		guard let snapshot = currentSnapshot() else {
			hideRootAndResetRenderState()
			return
		}
		renderFullSnapshot(snapshot)
	}

	private func renderFrameTick() {
		guard let snapshot = currentSnapshot() else {
			hideRootAndResetRenderState()
			return
		}
		let focusRect = snapshot.dragSelectionLocal ?? snapshot.hoverSelectionLocal
		let focusFlowAnimates = shouldAnimateSelectionFlow(snapshot)
		if snapshot.frozenPending || snapshot.dragSelectionLocal != nil
			|| focusRect != lastRenderedFocusRect
			|| focusFlowAnimates != lastRenderedFocusFlowAnimates
		{
			renderFullSnapshot(snapshot)
		} else {
			renderChromeSnapshot(snapshot)
		}
	}

	private func renderFullSnapshot(_ snapshot: LivePreviewSnapshot) {
		let renderStart = ProcessInfo.processInfo.systemUptime
		recordChromeRenderGap(at: renderStart, snapshot: snapshot)
		defer {
			layerRenderDurationMetric.recordMillisecondsSince(renderStart)
		}
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		rootLayer.isHidden = false
		rootLayer.frame = snapshot.bounds
		chromeRootLayer.isHidden = false
		chromeRootLayer.frame = snapshot.bounds
		renderFrozenDisplay(snapshot)
		renderFocus(snapshot)
		lastRenderedFocusRect = snapshot.dragSelectionLocal ?? snapshot.hoverSelectionLocal
		lastRenderedFocusFlowAnimates = shouldAnimateSelectionFlow(snapshot)
		renderHud(snapshot)
		renderLoupe(snapshot)
		renderPointer(snapshot)
		CATransaction.commit()
	}

	private func renderChromeSnapshot() {
		guard let snapshot = currentSnapshot() else {
			hideRootAndResetRenderState()
			return
		}
		renderChromeSnapshot(snapshot)
	}

	private func hideRootAndResetRenderState() {
		rootLayer.isHidden = true
		chromeRootLayer.isHidden = true
		lastRenderedFocusRect = nil
		lastRenderedFocusFlowAnimates = false
		lastChromeRenderUptime = nil
		lastActiveChromeRenderUptime = nil
		hudColorRollCoordinator.reset()
		hoverFlowLayer.hide()
		pointerLayer.hide()
	}

	private func currentSnapshot() -> LivePreviewSnapshot? {
		let snapshotStart = ProcessInfo.processInfo.systemUptime
		defer {
			snapshotDurationMetric.recordMillisecondsSince(snapshotStart)
		}
		return snapshotProvider?()
	}

	private func renderChromeSnapshot(_ snapshot: LivePreviewSnapshot) {
		let renderStart = ProcessInfo.processInfo.systemUptime
		recordChromeRenderGap(at: renderStart, snapshot: snapshot)
		defer {
			layerChromeRenderDurationMetric.recordMillisecondsSince(renderStart)
		}
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		rootLayer.isHidden = false
		rootLayer.frame = snapshot.bounds
		chromeRootLayer.isHidden = false
		chromeRootLayer.frame = snapshot.bounds
		let chromeExclusions = liveChromeRoundedExclusions(for: snapshot)
		updateLiveScrimExclusions(excluding: chromeExclusions)
		updateLiveFlowExclusions(excluding: chromeExclusions)
		renderHud(snapshot)
		renderLoupe(snapshot)
		renderPointer(snapshot)
		CATransaction.commit()
	}

	private func layerFrameNeedsUpdate(_ current: CGRect, _ next: CGRect) -> Bool {
		abs(current.minX - next.minX) > 0.001
			|| abs(current.minY - next.minY) > 0.001
			|| abs(current.width - next.width) > 0.001
			|| abs(current.height - next.height) > 0.001
	}

	private func recordChromeRenderGap(at now: TimeInterval, snapshot: LivePreviewSnapshot) {
		if let lastChromeRenderUptime {
			let gapMilliseconds = (now - lastChromeRenderUptime) * 1_000
			if gapMilliseconds >= 0, gapMilliseconds < 250 {
				layerChromeRenderGapMetric.record(gapMilliseconds)
			}
		}
		lastChromeRenderUptime = now
		guard let inputUptime = snapshot.inputUptime,
			now - inputUptime <= Self.activeInputWindow
		else {
			lastActiveChromeRenderUptime = nil
			return
		}
		if let lastActiveChromeRenderUptime {
			let activeGapMilliseconds = (now - lastActiveChromeRenderUptime) * 1_000
			if activeGapMilliseconds >= 0, activeGapMilliseconds < 250 {
				activeLayerChromeRenderGapMetric.record(activeGapMilliseconds)
			}
		}
		lastActiveChromeRenderUptime = now
	}

	private func renderHud(_ snapshot: LivePreviewSnapshot) {
		guard let hudFrame = snapshot.hudFrame else {
			hudLayer.isHidden = true
			hudColorRollCoordinator.reset()
			return
		}
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		hudLayer.isHidden = false
		hudLayer.frame = hudFrame
		applySurfaceStyle(
			container: hudLayer,
			glassLayer: hudGlassLayer,
			fillLayer: hudFillLayer,
			strokeLayer: hudStrokeLayer,
			frame: hudLayer.bounds,
			palette: palette,
			settings: snapshot.settings,
			glassImage: snapshot.glassPatches[.hud]
		)

		let font = LiveOverlayTypography.font
		let swatchSize = CaptureChrome.hudSwatchSize
		let positionText =
			"x=\(snapshot.positionDisplay.xValueText),y=\(snapshot.positionDisplay.yValueText)"
		let positionSize = CGSize(
			width: snapshot.positionDisplay.xSlotWidth
				+ LiveOverlayTypography.commaWidth
				+ snapshot.positionDisplay.ySlotWidth,
			height: LiveOverlayTypography.lineHeight
		)
		var cursorX = CaptureChrome.hudInnerMarginX
		let baselineY = (hudLayer.bounds.height - positionSize.height) / 2
		applyText(
			hudPositionLayer,
			text: positionText,
			font: font,
			color: palette.labelText,
			frame: CGRect(
				x: cursorX, y: baselineY, width: ceil(positionSize.width),
				height: ceil(positionSize.height)),
			alignment: .left
		)
		cursorX += positionSize.width + CaptureChrome.hudGroupSpacing

		let swatchFrame = CGRect(
			x: cursorX,
			y: hudLayer.bounds.midY - swatchSize.height / 2,
			width: swatchSize.width,
			height: swatchSize.height
		)
		cursorX += swatchSize.width + CaptureChrome.hudColorItemSpacing

		let hexFrame = CGRect(
			x: cursorX, y: baselineY, width: ceil(snapshot.colorDisplay.hexSlotWidth),
			height: ceil(LiveOverlayTypography.lineHeight))
		hudColorRollCoordinator.render(
			colorDisplay: snapshot.colorDisplay,
			rgbSample: snapshot.rgbSample,
			palette: palette,
			swatchFrame: swatchFrame,
			hexFrame: hexFrame,
			font: font
		)
		cursorX += snapshot.colorDisplay.hexSlotWidth + CaptureChrome.hudGroupSpacing

		if snapshot.keycapVisible {
			let keycapText = "Tab"
			let keycapFont = font
			let keycapFrame = CGRect(
				x: cursorX,
				y: hudLayer.bounds.midY - LiveOverlayTypography.keycapFrameSize.height / 2,
				width: LiveOverlayTypography.keycapFrameSize.width,
				height: LiveOverlayTypography.keycapFrameSize.height
			)
			hudKeycapLayer.isHidden = false
			hudKeycapTextLayer.isHidden = false
			hudKeycapLayer.frame = keycapFrame
			hudKeycapLayer.cornerRadius = 6
			hudKeycapLayer.backgroundColor = palette.keycapFill.cgColor
			hudKeycapLayer.borderColor = palette.keycapStroke.cgColor
			hudKeycapLayer.borderWidth = 1
			applyText(
				hudKeycapTextLayer, text: keycapText, font: keycapFont, color: palette.keycapText,
				frame: centeredTextFrame(for: keycapText, font: keycapFont, in: keycapFrame),
				alignment: .center)
		} else {
			hudKeycapLayer.isHidden = true
			hudKeycapTextLayer.isHidden = true
		}
	}

	private func renderLoupe(_ snapshot: LivePreviewSnapshot) {
		guard let loupeFrame = snapshot.loupeFrame, let loupePatch = snapshot.loupePatch else {
			loupeLayer.isHidden = true
			return
		}
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		loupeLayer.isHidden = false
		loupeLayer.frame = loupeFrame
		applySurfaceStyle(
			container: loupeLayer,
			glassLayer: loupeGlassLayer,
			fillLayer: loupeFillLayer,
			strokeLayer: loupeStrokeLayer,
			frame: loupeLayer.bounds,
			palette: palette,
			settings: snapshot.settings,
			glassImage: snapshot.glassPatches[.loupe]
		)
		loupePatchLayer.frame = loupeLayer.bounds.insetBy(dx: 10, dy: 10)
		loupePatchLayer.contentsGravity = .resizeAspectFill
		loupePatchLayer.minificationFilter = .nearest
		loupePatchLayer.magnificationFilter = .nearest
		loupePatchLayer.contents = loupePatch
		let centerRect = CGRect(
			x: loupePatchLayer.frame.midX - CaptureChrome.loupeCellSize / 2,
			y: loupePatchLayer.frame.midY - CaptureChrome.loupeCellSize / 2,
			width: CaptureChrome.loupeCellSize,
			height: CaptureChrome.loupeCellSize
		).insetBy(dx: 1, dy: 1)
		loupeCenterLayer.path = CGPath(rect: centerRect, transform: nil)
		loupeCenterLayer.fillColor = NSColor.clear.cgColor
		loupeCenterLayer.strokeColor = NSColor.white.withAlphaComponent(0.9).cgColor
		loupeCenterLayer.lineWidth = 2
	}

	private func renderPointer(_ snapshot: LivePreviewSnapshot) {
		let scale = hostView?.window?.backingScaleFactor ?? 2
		pointerLayer.update(
			pointer: snapshot.pointerLocal,
			in: snapshot.bounds,
			contentsScale: scale
		)
	}

	private func applySurfaceStyle(
		container: CALayer,
		glassLayer: CALayer,
		fillLayer: CALayer,
		strokeLayer: CAShapeLayer,
		frame: CGRect,
		palette: CaptureChromePalette,
		settings: NativeHostSettings,
		glassImage: CGImage?
	) {
		let cornerRadius = CaptureChrome.hudCornerRadius
		let boundsPath = CGPath(
			roundedRect: frame,
			cornerWidth: cornerRadius,
			cornerHeight: cornerRadius,
			transform: nil
		)
		let glassEnabled = settings.usesClassicHudGlass
		let hasNativeLiquidGlass = settings.usesLiquidHudGlass
		let opacity = CaptureChrome.effectiveHudOpacity(settings: settings)
		let hasInlineGlass = glassEnabled && glassImage != nil
		let hasGlass = hasInlineGlass || glassEnabled || hasNativeLiquidGlass

		container.cornerRadius = cornerRadius
		if hasNativeLiquidGlass {
			container.shadowOpacity = 0
			container.shadowPath = nil
		} else {
			container.shadowColor = palette.shadow.cgColor
			container.shadowOffset = .zero
			container.shadowRadius = 10
			container.shadowOpacity = Float(max(0.12, opacity * 0.75))
			container.shadowPath = boundsPath
		}

		glassLayer.frame = frame
		glassLayer.cornerRadius = cornerRadius
		glassLayer.masksToBounds = true
		glassLayer.contentsGravity = .resizeAspectFill
		glassLayer.contents = glassImage
		glassLayer.opacity = hasInlineGlass ? CaptureChrome.glassOpacity(settings: settings) : 0
		glassLayer.isHidden = !hasInlineGlass

		let usesNativeLiquidGlass = settings.usesLiquidHudGlass
		fillLayer.frame = frame
		fillLayer.cornerRadius = cornerRadius
		fillLayer.isHidden = usesNativeLiquidGlass
		fillLayer.backgroundColor =
			usesNativeLiquidGlass
			? NSColor.clear.cgColor
			: CaptureChrome.effectiveBodyFill(
				palette: palette,
				settings: settings,
				hasGlass: hasGlass
			).cgColor

		strokeLayer.frame = frame
		strokeLayer.path = boundsPath
		strokeLayer.fillColor = NSColor.clear.cgColor
		strokeLayer.strokeColor = palette.outerStroke.cgColor
		strokeLayer.lineWidth = 1
	}

	func applyText(
		_ layer: CATextLayer,
		text: String,
		font: NSFont,
		color: NSColor,
		frame: CGRect,
		alignment: CATextLayerAlignmentMode
	) {
		layer.contentsScale = hostView?.window?.backingScaleFactor ?? 2
		layer.string = text
		layer.font = font
		layer.fontSize = font.pointSize
		layer.foregroundColor = color.cgColor
		layer.alignmentMode = alignment
		layer.frame = frame
		layer.isWrapped = false
	}

	private func centeredTextFrame(for text: String, font: NSFont, in frame: CGRect) -> CGRect {
		let textSize = text.size(using: font)
		let width = ceil(textSize.width)
		let height = ceil(textSize.height)
		return CGRect(
			x: frame.midX - width / 2,
			y: frame.midY - height / 2,
			width: width,
			height: height
		)
	}
}
