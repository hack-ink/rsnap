import AppKit
import CoreGraphics
import CoreImage
import CoreVideo
import Foundation
import QuartzCore
import RsnapHostBridge

enum LiveGlassSurfaceKind: Hashable {
	case hud
	case loupe
	case status
}

struct GlassPatchRequest {
	let kind: LiveGlassSurfaceKind
	let globalRect: CGRect
	let blurAmount: CGFloat
	let tintAmount: CGFloat
	let brightnessBias: CGFloat
}

struct LivePreviewSnapshot {
	let bounds: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let pointerLocal: CGPoint?
	let dragSelectionLocal: CGRect?
	let hoverSelectionLocal: CGRect?
	let selectionSizeText: String?
	let hudFrame: CGRect?
	let loupeFrame: CGRect?
	let statusFrame: CGRect?
	let positionText: String
	let hexText: String
	let rgbText: String
	let rgbSample: RGBSample?
	let keycapVisible: Bool
	let statusMessage: String?
	let loupePatch: CGImage?
	let glassPatches: [LiveGlassSurfaceKind: CGImage]
}

final class WindowSnapshotFeed {
	private let ownPID = ProcessInfo.processInfo.processIdentifier
	private let queue = DispatchQueue(label: "ink.hack.rsnap.native-host.window-snapshot-feed", qos: .userInitiated)
	private let stateLock = NSLock()
	private var timer: DispatchSourceTimer?
	private var desktopFrame: CGRect = .null
	private var latestSnapshots: [WindowSnapshot] = []

	func start(desktopFrame: CGRect) {
		stop()
		stateLock.lock()
		self.desktopFrame = desktopFrame
		stateLock.unlock()
		refresh()
		let timer = DispatchSource.makeTimerSource(queue: queue)
		timer.schedule(deadline: .now() + LiveSamplingBudget.hoverWindowCacheRefreshInterval, repeating: LiveSamplingBudget.hoverWindowCacheRefreshInterval)
		timer.setEventHandler { [weak self] in
			self?.refresh()
		}
		self.timer = timer
		timer.resume()
	}

	func stop() {
		timer?.cancel()
		timer = nil
		stateLock.lock()
		latestSnapshots.removeAll()
		stateLock.unlock()
	}

	func window(at point: CGPoint) -> WindowSnapshot? {
		stateLock.lock()
		let snapshots = latestSnapshots
		stateLock.unlock()
		return snapshots.first(where: { $0.frame.contains(point) })
	}

	private func refresh() {
		stateLock.lock()
		let desktopFrame = self.desktopFrame
		stateLock.unlock()
		let candidateWindows =
			(CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
				as? [[String: Any]])
			?? []
		var snapshots: [WindowSnapshot] = []
		for info in candidateWindows {
			let ownerPID = (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value ?? -1
			if ownerPID == ownPID {
				continue
			}
			let alpha = (info[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 1
			if alpha < 0.05 {
				continue
			}
			let layer = (info[kCGWindowLayer as String] as? NSNumber)?.intValue ?? 0
			if layer < 0 {
				continue
			}
			guard let boundsDictionary = info[kCGWindowBounds as String] as? NSDictionary else {
				continue
			}
			var quartzBounds = CGRect.null
			guard CGRectMakeWithDictionaryRepresentation(boundsDictionary, &quartzBounds) else {
				continue
			}
			let appKitBounds = CGRect(
				x: quartzBounds.minX,
				y: desktopFrame.maxY - quartzBounds.maxY,
				width: quartzBounds.width,
				height: quartzBounds.height
			)
			if appKitBounds.width < 40 || appKitBounds.height < 40 {
				continue
			}
			let windowID = (info[kCGWindowNumber as String] as? NSNumber)?.uint32Value
			snapshots.append(WindowSnapshot(windowID: windowID, frame: appKitBounds))
		}
		stateLock.lock()
		latestSnapshots = snapshots
		stateLock.unlock()
	}
}

final class ChromeSampleFeed {
	private let broker: LiveFrameStreamBroker
	private let queue = DispatchQueue(label: "ink.hack.rsnap.native-host.chrome-sample-feed", qos: .userInteractive)
	private let stateLock = NSLock()
	private var timer: DispatchSourceTimer?
	private var desiredPoint: CGPoint?
	private var desiredSidePixels: Int = 1
	private var latestSample: LiveChromeSample?

	init(broker: LiveFrameStreamBroker) {
		self.broker = broker
	}

	func start() {
		stop()
		let timer = DispatchSource.makeTimerSource(queue: queue)
		let interval = TimeInterval(1.0 / 60.0)
		timer.schedule(deadline: .now(), repeating: interval)
		timer.setEventHandler { [weak self] in
			self?.refresh()
		}
		self.timer = timer
		timer.resume()
	}

	func stop() {
		timer?.cancel()
		timer = nil
		stateLock.lock()
		desiredPoint = nil
		latestSample = nil
		stateLock.unlock()
	}

	func updateDemand(point: CGPoint?, sidePixels: Int) {
		stateLock.lock()
		let nextSidePixels = max(1, sidePixels)
		if nextSidePixels != desiredSidePixels {
			latestSample = nil
		}
		desiredPoint = point
		desiredSidePixels = nextSidePixels
		stateLock.unlock()
	}

	func snapshot() -> LiveChromeSample? {
		stateLock.lock()
		let latestSample = self.latestSample
		stateLock.unlock()
		return latestSample
	}

	private func refresh() {
		stateLock.lock()
		let point = desiredPoint
		let sidePixels = desiredSidePixels
		stateLock.unlock()
		guard let point else {
			stateLock.lock()
			latestSample = nil
			stateLock.unlock()
			return
		}
		let sample = broker.sample(at: point, sidePixels: sidePixels)
		stateLock.lock()
		latestSample = sample
		stateLock.unlock()
	}
}

final class GlassPatchFeed {
	private struct CachedPatch {
		let request: GlassPatchRequest
		let capturedAt: TimeInterval
		let image: CGImage
	}

	private let queue = DispatchQueue(label: "ink.hack.rsnap.native-host.glass-patch-feed", qos: .utility)
	private let stateLock = NSLock()
	private let capturePatch: (CGRect) -> CGImage?
	private let ciContext = CIContext(options: nil)
	private var timer: DispatchSourceTimer?
	private var requests: [LiveGlassSurfaceKind: GlassPatchRequest] = [:]
	private var cachedPatches: [LiveGlassSurfaceKind: CachedPatch] = [:]

	init(capturePatch: @escaping (CGRect) -> CGImage?) {
		self.capturePatch = capturePatch
	}

	func start() {
		stop()
		let timer = DispatchSource.makeTimerSource(queue: queue)
		let interval = TimeInterval(1.0 / 12.0)
		timer.schedule(deadline: .now() + interval, repeating: interval)
		timer.setEventHandler { [weak self] in
			self?.refresh()
		}
		self.timer = timer
		timer.resume()
	}

	func stop() {
		timer?.cancel()
		timer = nil
		stateLock.lock()
		requests.removeAll()
		cachedPatches.removeAll()
		stateLock.unlock()
	}

	func updateRequests(_ requests: [GlassPatchRequest]) {
		let nextRequests = Dictionary(uniqueKeysWithValues: requests.map { ($0.kind, $0) })
		stateLock.lock()
		self.requests = nextRequests
		cachedPatches = cachedPatches.filter { nextRequests[$0.key] != nil }
		stateLock.unlock()
	}

	func snapshot() -> [LiveGlassSurfaceKind: CGImage] {
		stateLock.lock()
		let result = cachedPatches.mapValues(\.image)
		stateLock.unlock()
		return result
	}

	private func refresh() {
		stateLock.lock()
		let requests = self.requests
		let cachedPatches = self.cachedPatches
		stateLock.unlock()

		var nextCachedPatches = cachedPatches
		let now = ProcessInfo.processInfo.systemUptime

		for (kind, request) in requests {
			if let cachedPatch = cachedPatches[kind] {
				let cachedCenter = CGPoint(x: cachedPatch.request.globalRect.midX, y: cachedPatch.request.globalRect.midY)
				let nextCenter = CGPoint(x: request.globalRect.midX, y: request.globalRect.midY)
				let distance = hypot(cachedCenter.x - nextCenter.x, cachedCenter.y - nextCenter.y)
				if distance < 24, now - cachedPatch.capturedAt < 0.08 {
					continue
				}
			}

			guard
				let patch = capturePatch(request.globalRect),
				let blurred = blurredImage(
					from: patch,
					blurAmount: request.blurAmount,
					tintAmount: request.tintAmount,
					brightnessBias: request.brightnessBias
				)
			else {
				continue
			}
			nextCachedPatches[kind] = CachedPatch(request: request, capturedAt: now, image: blurred)
		}

		stateLock.lock()
		self.cachedPatches = nextCachedPatches
		stateLock.unlock()
	}

	private func blurredImage(from image: CGImage, blurAmount: CGFloat) -> CGImage? {
		blurredImage(from: image, blurAmount: blurAmount, tintAmount: 0, brightnessBias: 0)
	}

	private func blurredImage(
		from image: CGImage,
		blurAmount: CGFloat,
		tintAmount: CGFloat,
		brightnessBias: CGFloat
	) -> CGImage? {
		let ciImage = CIImage(cgImage: image)
		let clampedImage = ciImage.clampedToExtent()
		guard let filter = CIFilter(name: "CIGaussianBlur") else {
			return image
		}
		filter.setValue(clampedImage, forKey: kCIInputImageKey)
		let normalizedBlur = blurAmount.clamped(to: 0...1)
		let blurRadius = 4 + pow(normalizedBlur, 0.82) * 44
		filter.setValue(blurRadius, forKey: kCIInputRadiusKey)
		guard let outputImage = filter.outputImage?.cropped(to: ciImage.extent) else {
			return image
		}
		let tunedImage: CIImage
		if let colorControls = CIFilter(name: "CIColorControls") {
			colorControls.setValue(outputImage, forKey: kCIInputImageKey)
			colorControls.setValue(1.08 + tintAmount.clamped(to: 0...1) * 0.34, forKey: kCIInputSaturationKey)
			colorControls.setValue(1.03, forKey: kCIInputContrastKey)
			colorControls.setValue(brightnessBias, forKey: kCIInputBrightnessKey)
			tunedImage = colorControls.outputImage?.cropped(to: ciImage.extent) ?? outputImage
		} else {
			tunedImage = outputImage
		}
		return ciContext.createCGImage(tunedImage, from: tunedImage.extent) ?? image
	}
}

final class LiveDisplayLinkDriver: @unchecked Sendable {
	var onTick: (() -> Void)?
	private var displayLink: CVDisplayLink?

	func start(displayID: CGDirectDisplayID) {
		stop()
		var link: CVDisplayLink?
		guard CVDisplayLinkCreateWithActiveCGDisplays(&link) == kCVReturnSuccess, let link else {
			return
		}
		CVDisplayLinkSetCurrentCGDisplay(link, displayID)
		CVDisplayLinkSetOutputHandler(link) { [weak self] _, _, _, _, _ in
			guard let self else {
				return kCVReturnSuccess
			}
			DispatchQueue.main.async {
				self.onTick?()
			}
			return kCVReturnSuccess
		}
		displayLink = link
		CVDisplayLinkStart(link)
	}

	func stop() {
		guard let displayLink else {
			return
		}
		CVDisplayLinkStop(displayLink)
		self.displayLink = nil
	}

	deinit {
		stop()
	}
}

@MainActor
final class LiveOverlayRenderer {
	private weak var hostView: NSView?
	var onTick: (() -> Void)?
	private let rootLayer = CALayer()
	private let topScrimLayer = CALayer()
	private let leftScrimLayer = CALayer()
	private let rightScrimLayer = CALayer()
	private let bottomScrimLayer = CALayer()
	private let hoverGlowLayer = CAShapeLayer()
	private let dragBorderLayer = CAShapeLayer()
	private let selectionSizeLayer = CATextLayer()
	private let hudLayer = CALayer()
	private let hudGlassLayer = CALayer()
	private let hudFillLayer = CALayer()
	private let hudStrokeLayer = CAShapeLayer()
	private let hudPositionLayer = CATextLayer()
	private let hudHexLayer = CATextLayer()
	private let hudRGBLayer = CATextLayer()
	private let hudSwatchLayer = CALayer()
	private let hudKeycapLayer = CALayer()
	private let hudKeycapTextLayer = CATextLayer()
	private let loupeLayer = CALayer()
	private let loupeGlassLayer = CALayer()
	private let loupeFillLayer = CALayer()
	private let loupeStrokeLayer = CAShapeLayer()
	private let loupePatchLayer = CALayer()
	private let loupeCenterLayer = CAShapeLayer()
	private let statusLayer = CALayer()
	private let statusGlassLayer = CALayer()
	private let statusFillLayer = CALayer()
	private let statusStrokeLayer = CAShapeLayer()
	private let statusTextLayer = CATextLayer()
	private let displayLink = LiveDisplayLinkDriver()
	private var snapshotProvider: (() -> LivePreviewSnapshot?)?

	init(hostView: NSView) {
		self.hostView = hostView
		configureLayers()
		displayLink.onTick = { [weak self] in
			self?.renderCurrentSnapshot()
			self?.onTick?()
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
		rootLayer.isHidden = true
	}

	func updateDisplayID(_ displayID: CGDirectDisplayID?) {
		guard let displayID else {
			stop()
			return
		}
		displayLink.start(displayID: displayID)
	}

	func stop() {
		displayLink.stop()
		rootLayer.isHidden = true
	}

	func renderNow() {
		renderCurrentSnapshot()
		onTick?()
	}

	private func configureLayers() {
		rootLayer.zPosition = 100
		rootLayer.masksToBounds = false
		[topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer].forEach {
			rootLayer.addSublayer($0)
			$0.isHidden = true
		}
		hoverGlowLayer.fillColor = NSColor.clear.cgColor
		hoverGlowLayer.lineWidth = 2.25
		hoverGlowLayer.shadowOffset = .zero
		hoverGlowLayer.shadowRadius = 12
		rootLayer.addSublayer(hoverGlowLayer)

		dragBorderLayer.fillColor = NSColor.clear.cgColor
		dragBorderLayer.lineDashPattern = [12, 8]
		rootLayer.addSublayer(dragBorderLayer)

		selectionSizeLayer.contentsScale = 2
		rootLayer.addSublayer(selectionSizeLayer)

		[hudLayer, loupeLayer, statusLayer].forEach {
			$0.masksToBounds = false
			rootLayer.addSublayer($0)
		}
		[hudGlassLayer, hudFillLayer, hudStrokeLayer, hudSwatchLayer, hudPositionLayer, hudHexLayer, hudRGBLayer, hudKeycapLayer, hudKeycapTextLayer].forEach {
			hudLayer.addSublayer($0)
		}
		[loupeGlassLayer, loupeFillLayer, loupeStrokeLayer, loupePatchLayer, loupeCenterLayer].forEach {
			loupeLayer.addSublayer($0)
		}
		[statusGlassLayer, statusFillLayer, statusStrokeLayer, statusTextLayer].forEach {
			statusLayer.addSublayer($0)
		}
		[hudLayer, loupeLayer, statusLayer].forEach { $0.isHidden = true }
	}

	private func renderCurrentSnapshot() {
		guard let snapshot = snapshotProvider?() else {
			rootLayer.isHidden = true
			return
		}
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		rootLayer.isHidden = false
		rootLayer.frame = snapshot.bounds
		renderFocus(snapshot)
		renderHud(snapshot)
		renderLoupe(snapshot)
		renderStatus(snapshot)
		CATransaction.commit()
	}

	private func renderFocus(_ snapshot: LivePreviewSnapshot) {
		let focusRect = snapshot.dragSelectionLocal ?? snapshot.hoverSelectionLocal
		guard let focusRect else {
			[topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer].forEach { $0.isHidden = true }
			hoverGlowLayer.isHidden = true
			dragBorderLayer.isHidden = true
			selectionSizeLayer.isHidden = true
			return
		}

		let scrimAlpha = CGFloat(CaptureChrome.liveScrimAlpha)
		let scrimColor = NSColor(calibratedWhite: 0, alpha: scrimAlpha).cgColor
		let bounds = snapshot.bounds
		let rects = [
			CGRect(x: bounds.minX, y: bounds.minY, width: bounds.width, height: max(0, focusRect.minY - bounds.minY)),
			CGRect(x: bounds.minX, y: focusRect.minY, width: max(0, focusRect.minX - bounds.minX), height: focusRect.height),
			CGRect(x: focusRect.maxX, y: focusRect.minY, width: max(0, bounds.maxX - focusRect.maxX), height: focusRect.height),
			CGRect(x: bounds.minX, y: focusRect.maxY, width: bounds.width, height: max(0, bounds.maxY - focusRect.maxY)),
		]
		for (layer, rect) in zip([topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer], rects) {
			layer.backgroundColor = scrimColor
			layer.frame = rect
			layer.isHidden = rect.width <= 0 || rect.height <= 0
		}

		if let dragSelection = snapshot.dragSelectionLocal {
			hoverGlowLayer.isHidden = true
			dragBorderLayer.isHidden = false
			let dragPath = NSBezierPath(
				roundedRect: dragSelection,
				xRadius: CaptureChrome.selectionCornerRadius,
				yRadius: CaptureChrome.selectionCornerRadius
			).cgPath
			dragBorderLayer.path = dragPath
			dragBorderLayer.strokeColor = NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor
			dragBorderLayer.lineWidth = CaptureChrome.liveDashedBorderWidth
			if let selectionSizeText = snapshot.selectionSizeText {
				let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
				let textSize = selectionSizeText.size(using: font)
				let x = min(dragSelection.maxX - textSize.width, bounds.maxX - 8 - textSize.width)
				let preferredY = dragSelection.maxY + 8
				let y = preferredY + textSize.height <= bounds.maxY - 8
					? preferredY
					: max(bounds.minY + 8, dragSelection.maxY - 8 - textSize.height)
				applyText(
					selectionSizeLayer,
					text: selectionSizeText,
					font: font,
					color: NSColor.white.withAlphaComponent(0.98),
					frame: CGRect(x: x, y: y, width: ceil(textSize.width), height: ceil(textSize.height)),
					alignment: .left
				)
				selectionSizeLayer.isHidden = false
			} else {
				selectionSizeLayer.isHidden = true
			}
			return
		}

		dragBorderLayer.isHidden = true
		selectionSizeLayer.isHidden = true
		let hoverPath = NSBezierPath(
			roundedRect: focusRect,
			xRadius: CaptureChrome.liveSelectionCornerRadius,
			yRadius: CaptureChrome.liveSelectionCornerRadius
		).cgPath
		hoverGlowLayer.path = hoverPath
		hoverGlowLayer.strokeColor = NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 0.45).cgColor
		hoverGlowLayer.shadowColor = NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.55).cgColor
		hoverGlowLayer.isHidden = false
	}

	private func renderHud(_ snapshot: LivePreviewSnapshot) {
		guard let hudFrame = snapshot.hudFrame else {
			hudLayer.isHidden = true
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

		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let positionSize = snapshot.positionText.size(using: font)
		let hexSize = snapshot.hexText.size(using: font)
		let rgbSize = snapshot.rgbText.size(using: font)
		var cursorX = CaptureChrome.hudInnerMarginX
		let baselineY = (hudLayer.bounds.height - positionSize.height) / 2
		applyText(hudPositionLayer, text: snapshot.positionText, font: font, color: palette.labelText, frame: CGRect(x: cursorX, y: baselineY, width: ceil(positionSize.width), height: ceil(positionSize.height)), alignment: .left)
		cursorX += positionSize.width + 10

		hudSwatchLayer.frame = CGRect(x: cursorX, y: hudLayer.bounds.midY - 5, width: 10, height: 10)
		hudSwatchLayer.cornerRadius = 5
		let swatchColor = snapshot.rgbSample.map {
			NSColor(calibratedRed: CGFloat($0.r) / 255, green: CGFloat($0.g) / 255, blue: CGFloat($0.b) / 255, alpha: 1)
		} ?? NSColor(calibratedWhite: 1, alpha: 0.12)
		hudSwatchLayer.backgroundColor = swatchColor.cgColor
		hudSwatchLayer.borderColor = palette.swatchStroke.cgColor
		hudSwatchLayer.borderWidth = 1
		cursorX += 20

		applyText(hudHexLayer, text: snapshot.hexText, font: font, color: palette.labelText, frame: CGRect(x: cursorX, y: baselineY, width: ceil(hexSize.width), height: ceil(hexSize.height)), alignment: .left)
		cursorX += hexSize.width + 10
		applyText(hudRGBLayer, text: snapshot.rgbText, font: font, color: palette.secondaryText, frame: CGRect(x: cursorX, y: baselineY, width: ceil(rgbSize.width), height: ceil(rgbSize.height)), alignment: .left)
		cursorX += rgbSize.width + 10

		if snapshot.keycapVisible {
			let keycapText = "Tab"
			let keycapFont = font
			let keycapTextSize = keycapText.size(using: keycapFont)
			let keycapFrame = CGRect(x: cursorX, y: hudLayer.bounds.midY - (keycapTextSize.height + 4) / 2, width: keycapTextSize.width + 12, height: keycapTextSize.height + 4)
			hudKeycapLayer.isHidden = false
			hudKeycapTextLayer.isHidden = false
			hudKeycapLayer.frame = keycapFrame
			hudKeycapLayer.cornerRadius = 6
			hudKeycapLayer.backgroundColor = palette.keycapFill.cgColor
			hudKeycapLayer.borderColor = palette.keycapStroke.cgColor
			hudKeycapLayer.borderWidth = 1
			applyText(hudKeycapTextLayer, text: keycapText, font: keycapFont, color: palette.keycapText, frame: keycapFrame, alignment: .center)
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

	private func renderStatus(_ snapshot: LivePreviewSnapshot) {
		guard let statusMessage = snapshot.statusMessage, let statusFrame = snapshot.statusFrame else {
			statusLayer.isHidden = true
			return
		}
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		let font = NSFont.systemFont(ofSize: 12, weight: .medium)
		statusLayer.isHidden = false
		statusLayer.frame = statusFrame
		applySurfaceStyle(
			container: statusLayer,
			glassLayer: statusGlassLayer,
			fillLayer: statusFillLayer,
			strokeLayer: statusStrokeLayer,
			frame: statusLayer.bounds,
			palette: palette,
			settings: snapshot.settings,
			glassImage: snapshot.glassPatches[.status]
		)
		applyText(statusTextLayer, text: statusMessage, font: font, color: palette.labelText, frame: statusLayer.bounds.insetBy(dx: CaptureChrome.hudInnerMarginX, dy: CaptureChrome.hudInnerMarginY - 1), alignment: .left)
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
		let glassEnabled = settings.hudGlassEnabled && settings.hudBlur > 0.01
		let opacity = CGFloat(settings.hudOpacity.clamped(to: 0...1))
		let hasGlass = glassEnabled && glassImage != nil

		container.cornerRadius = cornerRadius
		container.shadowColor = palette.shadow.cgColor
		container.shadowOffset = .zero
		container.shadowRadius = 10
		container.shadowOpacity = Float(max(0.12, opacity * 0.75))

		glassLayer.frame = frame
		glassLayer.cornerRadius = cornerRadius
		glassLayer.masksToBounds = true
		glassLayer.contentsGravity = .resizeAspectFill
		glassLayer.contents = glassImage
		glassLayer.opacity = hasGlass ? Float(0.88 + settings.hudBlur.clamped(to: 0...1) * 0.12) : 0
		glassLayer.isHidden = !hasGlass

		fillLayer.frame = frame
		fillLayer.cornerRadius = cornerRadius
		fillLayer.backgroundColor = effectiveBodyFill(
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

	private func effectiveBodyFill(
		palette: CaptureChromePalette,
		settings: NativeHostSettings,
		hasGlass: Bool
	) -> NSColor {
		let opacity = CGFloat(settings.hudOpacity.clamped(to: 0...1))
		if hasGlass {
			return palette.bodyFill.withAlphaComponent(max(palette.bodyFill.alphaComponent, max(0.18, opacity * 0.34)))
		}
		return palette.bodyFill.withAlphaComponent(max(0.42, opacity * 0.82))
	}

	private func applyText(
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
}
