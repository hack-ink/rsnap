import AppKit
@preconcurrency import CoreGraphics
import Darwin
import Foundation
import QuartzCore

struct QuickScreenshotDisplayFrame {
	let displayID: CGDirectDisplayID
	let frame: CGRect
	let image: CGImage
	let capturedAtUptime: TimeInterval
}

struct QuickScreenshotSelection {
	let anchor: CGPoint
	let current: CGPoint
	let rect: CGRect
	let displayFrame: QuickScreenshotDisplayFrame
}

@MainActor
final class QuickScreenshotController {
	private var acquisitionController: QuickScreenshotAcquisitionController?

	var onStateChanged: (() -> Void)?

	var isActive: Bool {
		acquisitionController != nil
	}

	func startInteractiveFrozenCapture(
		captureController: CaptureSessionController,
		capturableOwnWindowIDs: Set<CGWindowID>,
		source: String
	) {
		guard acquisitionController == nil else {
			NativeHostTelemetry.lifecycleEvent(
				"native_host.quick_screenshot_already_active",
				detail: "source=\(source)"
			)
			return
		}
		guard captureController.isCaptureActive == false else {
			NativeHostTelemetry.lifecycleEvent(
				"native_host.quick_screenshot_ignored",
				detail: "source=\(source),reason=capture_active"
			)
			return
		}

		let controller = QuickScreenshotAcquisitionController(
			source: source,
			onComplete: { [weak self, weak captureController] selection in
				self?.acquisitionController = nil
				self?.onStateChanged?()
				captureController?.startQuickScreenshotFrozenCapture(
					selection: selection,
					capturableOwnWindowIDs: capturableOwnWindowIDs
				)
			},
			onCancel: { [weak self] reason in
				self?.acquisitionController = nil
				self?.onStateChanged?()
				NativeHostTelemetry.lifecycleEvent(
					"native_host.quick_screenshot_canceled",
					detail: "source=\(source),reason=\(reason)"
				)
			}
		)
		guard controller.start() else {
			return
		}
		acquisitionController = controller
		onStateChanged?()
	}

	func cancel() {
		acquisitionController?.cancel(reason: "app_terminate")
		acquisitionController = nil
		onStateChanged?()
	}
}

@MainActor
private final class QuickScreenshotAcquisitionController {
	private enum State {
		case armed
		case selecting(
			anchor: CGPoint,
			current: CGPoint,
			displayFrame: QuickScreenshotDisplayFrame
		)
		case finishing
		case canceled

		var isInterceptingEvents: Bool {
			switch self {
			case .armed, .selecting:
				return true
			case .finishing, .canceled:
				return false
			}
		}
	}

	private static let minimumSelectionSide: CGFloat = 2
	private static let eventMask =
		(CGEventMask(1) << CGEventType.leftMouseDown.rawValue)
		| (CGEventMask(1) << CGEventType.leftMouseDragged.rawValue)
		| (CGEventMask(1) << CGEventType.mouseMoved.rawValue)
		| (CGEventMask(1) << CGEventType.leftMouseUp.rawValue)
		| (CGEventMask(1) << CGEventType.rightMouseDown.rawValue)
		| (CGEventMask(1) << CGEventType.keyDown.rawValue)

	private let source: String
	private let onComplete: (QuickScreenshotSelection) -> Void
	private let onCancel: (String) -> Void
	private var state = State.armed
	private var eventTap: CFMachPort?
	private var eventTapSource: CFRunLoopSource?
	private var preparedDisplayFrames: [QuickScreenshotDisplayFrame] = []
	private var overlayController: QuickScreenshotSelectionOverlayController?

	init(
		source: String,
		onComplete: @escaping (QuickScreenshotSelection) -> Void,
		onCancel: @escaping (String) -> Void
	) {
		self.source = source
		self.onComplete = onComplete
		self.onCancel = onCancel
	}

	func start() -> Bool {
		let callback: CGEventTapCallBack = { _, type, event, userInfo in
			guard let userInfo else {
				return Unmanaged.passUnretained(event)
			}
			return MainActor.assumeIsolated {
				let controller = Unmanaged<QuickScreenshotAcquisitionController>
					.fromOpaque(userInfo)
					.takeUnretainedValue()
				return controller.handleEventTap(type: type, event: event)
			}
		}
		guard
			let eventTap = CGEvent.tapCreate(
				tap: .cgSessionEventTap,
				place: .headInsertEventTap,
				options: .defaultTap,
				eventsOfInterest: Self.eventMask,
				callback: callback,
				userInfo: Unmanaged.passUnretained(self).toOpaque()
			)
		else {
			NativeHostTelemetry.lifecycleWarning(
				"native_host.quick_screenshot_acquisition_failed",
				detail: "source=\(source),stage=event_tap"
			)
			return false
		}

		let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, eventTap, 0)
		CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
		CGEvent.tapEnable(tap: eventTap, enable: true)
		self.eventTap = eventTap
		eventTapSource = source
		NativeHostTelemetry.lifecycleEvent(
			"native_host.quick_screenshot_armed",
			detail: "source=\(self.source)"
		)
		prepareSelectionOverlay()
		return true
	}

	func cancel(reason: String) {
		guard case .canceled = state else {
			state = .canceled
			close()
			onCancel(reason)
			return
		}
	}

	private func handleEventTap(type: CGEventType, event: CGEvent) -> Unmanaged<CGEvent>? {
		guard eventTap != nil, state.isInterceptingEvents else {
			return Unmanaged.passUnretained(event)
		}

		switch type {
		case .tapDisabledByTimeout, .tapDisabledByUserInput:
			if let eventTap {
				CGEvent.tapEnable(tap: eventTap, enable: true)
			}
			return Unmanaged.passUnretained(event)
		case .keyDown:
			if event.getIntegerValueField(.keyboardEventKeycode) == 53 {
				cancel(reason: "escape")
				return nil
			}
			return nil
		case .rightMouseDown:
			cancel(reason: "right_mouse")
			return nil
		case .mouseMoved:
			let point = Self.appKitPoint(from: event)
			overlayController?.updatePointer(point)
			return Unmanaged.passUnretained(event)
		case .leftMouseDown:
			let point = Self.appKitPoint(from: event)
			overlayController?.updatePointer(point)
			beginSelection(at: point)
			return nil
		case .leftMouseDragged:
			let point = Self.appKitPoint(from: event)
			overlayController?.updatePointer(point)
			updateSelection(to: point)
			return nil
		case .leftMouseUp:
			finishSelection(at: Self.appKitPoint(from: event))
			return nil
		default:
			return nil
		}
	}

	private func beginSelection(at point: CGPoint) {
		guard case .armed = state else {
			return
		}
		guard
			let displayFrame = preparedDisplayFrame(containing: point)
				?? Self.captureDisplayFrame(containing: point)
		else {
			cancel(reason: "capture_failed")
			return
		}
		state = .selecting(anchor: point, current: point, displayFrame: displayFrame)
		let overlayController =
			self.overlayController
			?? QuickScreenshotSelectionOverlayController(displayFrames: [displayFrame])
		self.overlayController = overlayController
		overlayController.prepare()
		let initialSelection = Self.normalizedRect(
			anchor: point,
			current: point,
			in: displayFrame.frame
		)
		overlayController.show(initialSelection: initialSelection)
		updateSelection(to: point)
		let frameAgeMilliseconds =
			(ProcessInfo.processInfo.systemUptime - displayFrame.capturedAtUptime)
			* 1_000
		NativeHostTelemetry.lifecycleEvent(
			"native_host.quick_screenshot_selection_started",
			detail:
				"source=\(source),displayID=\(displayFrame.displayID),frameAgeMs=\(String(format: "%.2f", frameAgeMilliseconds)),x=\(Int(point.x.rounded())),y=\(Int(point.y.rounded()))"
		)
	}

	private func updateSelection(to point: CGPoint) {
		guard case .selecting(let anchor, _, let displayFrame) = state else {
			return
		}
		let clampedPoint = Self.clamped(point, to: displayFrame.frame)
		let rect = Self.normalizedRect(
			anchor: anchor,
			current: clampedPoint,
			in: displayFrame.frame
		)
		state = .selecting(anchor: anchor, current: clampedPoint, displayFrame: displayFrame)
		overlayController?.update(selection: rect)
	}

	private func finishSelection(at point: CGPoint) {
		guard case .selecting(let anchor, _, let displayFrame) = state else {
			cancel(reason: "mouse_up_without_selection")
			return
		}
		state = .finishing
		let clampedPoint = Self.clamped(point, to: displayFrame.frame)
		let rect = Self.normalizedRect(
			anchor: anchor,
			current: clampedPoint,
			in: displayFrame.frame
		)
		guard
			rect.width >= Self.minimumSelectionSide,
			rect.height >= Self.minimumSelectionSide
		else {
			cancel(reason: "selection_too_small")
			return
		}

		close()
		onComplete(
			QuickScreenshotSelection(
				anchor: anchor,
				current: clampedPoint,
				rect: rect,
				displayFrame: displayFrame
			)
		)
	}

	private func close() {
		let activeEventTap = eventTap
		let activeEventTapSource = eventTapSource
		self.eventTap = nil
		self.eventTapSource = nil

		if let activeEventTap {
			CGEvent.tapEnable(tap: activeEventTap, enable: false)
		}
		if let activeEventTapSource {
			CFRunLoopRemoveSource(CFRunLoopGetMain(), activeEventTapSource, .commonModes)
		}
		if let activeEventTap {
			CFMachPortInvalidate(activeEventTap)
		}
		overlayController?.close()
		overlayController = nil
		preparedDisplayFrames.removeAll()
	}

	private static func appKitPoint(from event: CGEvent) -> CGPoint {
		let quartzPoint = event.location
		let desktopFrame = CaptureOverlayController.desktopFrame
		return CGPoint(
			x: quartzPoint.x,
			y: desktopFrame.maxY - quartzPoint.y
		)
	}

	private func prepareSelectionOverlay() {
		let prepareStartedAt = ProcessInfo.processInfo.systemUptime
		preparedDisplayFrames = Self.captureDisplayFrames()
		if preparedDisplayFrames.isEmpty == false {
			let overlayController = QuickScreenshotSelectionOverlayController(
				displayFrames: preparedDisplayFrames
			)
			overlayController.prepare()
			overlayController.showArmed(pointer: NSEvent.mouseLocation)
			self.overlayController = overlayController
		}
		let prepareMilliseconds = NativeHostTelemetry.milliseconds(since: prepareStartedAt)
		NativeHostTelemetry.lifecycleEvent(
			"native_host.quick_screenshot_prewarmed",
			detail:
				"source=\(source),frames=\(preparedDisplayFrames.count),prepareMs=\(String(format: "%.2f", prepareMilliseconds))"
		)
	}

	private func preparedDisplayFrame(containing point: CGPoint)
		-> QuickScreenshotDisplayFrame?
	{
		preparedDisplayFrames.first { $0.frame.inclusivelyContains(point) }
	}

	private static func captureDisplayFrames() -> [QuickScreenshotDisplayFrame] {
		let desktopFrame = CaptureOverlayController.desktopFrame
		let capturedAtUptime = ProcessInfo.processInfo.systemUptime
		return NSScreen.screens.compactMap { screen in
			guard
				let displayID = CaptureSessionController.displayID(for: screen),
				let image = captureDisplayImage(
					displayID: displayID,
					rect: screen.frame,
					desktopFrame: desktopFrame
				)
			else {
				return nil
			}
			return QuickScreenshotDisplayFrame(
				displayID: displayID,
				frame: screen.frame,
				image: image,
				capturedAtUptime: capturedAtUptime
			)
		}
	}

	private static func captureDisplayFrame(containing point: CGPoint)
		-> QuickScreenshotDisplayFrame?
	{
		let desktopFrame = CaptureOverlayController.desktopFrame
		let capturedAtUptime = ProcessInfo.processInfo.systemUptime
		guard
			let screen = NSScreen.screens.first(where: {
				$0.frame.inclusivelyContains(point)
			}),
			let displayID = CaptureSessionController.displayID(for: screen),
			let image = captureDisplayImage(
				displayID: displayID,
				rect: screen.frame,
				desktopFrame: desktopFrame
			)
		else {
			return nil
		}
		return QuickScreenshotDisplayFrame(
			displayID: displayID,
			frame: screen.frame,
			image: image,
			capturedAtUptime: capturedAtUptime
		)
	}

	private static func captureDisplayImage(
		displayID: CGDirectDisplayID,
		rect: CGRect,
		desktopFrame: CGRect
	) -> CGImage? {
		let quartzRect = CGRect(
			x: rect.minX,
			y: desktopFrame.maxY - rect.maxY,
			width: rect.width,
			height: rect.height
		)
		guard quartzRect.isNull == false, quartzRect.width > 0, quartzRect.height > 0 else {
			return nil
		}
		return displayCreateImageForRect?(displayID, quartzRect)?
			.takeRetainedValue()
	}

	private static func clamped(_ point: CGPoint, to frame: CGRect) -> CGPoint {
		CGPoint(
			x: point.x.clamped(to: frame.minX...frame.maxX),
			y: point.y.clamped(to: frame.minY...frame.maxY)
		)
	}

	private static func normalizedRect(
		anchor: CGPoint,
		current: CGPoint,
		in frame: CGRect
	) -> CGRect {
		let clampedAnchor = clamped(anchor, to: frame)
		let clampedCurrent = clamped(current, to: frame)
		return CGRect(
			x: min(clampedAnchor.x, clampedCurrent.x),
			y: min(clampedAnchor.y, clampedCurrent.y),
			width: abs(clampedCurrent.x - clampedAnchor.x),
			height: abs(clampedCurrent.y - clampedAnchor.y)
		)
	}

	private typealias DisplayCreateImageForRect =
		@convention(c) (
			CGDirectDisplayID,
			CGRect
		) -> Unmanaged<CGImage>?

	private static let displayCreateImageForRect: DisplayCreateImageForRect? = {
		guard
			let coreGraphics = dlopen(
				"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",
				RTLD_LAZY
			)
		else {
			return nil
		}
		guard let symbol = dlsym(coreGraphics, "CGDisplayCreateImageForRect") else {
			dlclose(coreGraphics)
			return nil
		}
		return unsafeBitCast(symbol, to: DisplayCreateImageForRect.self)
	}()
}

@MainActor
private final class QuickScreenshotSelectionOverlayController {
	private let displayFrames: [QuickScreenshotDisplayFrame]
	private var windows: [QuickScreenshotSelectionWindow] = []
	private var activeSelectionWindow: QuickScreenshotSelectionWindow?

	init(displayFrames: [QuickScreenshotDisplayFrame]) {
		self.displayFrames = displayFrames
	}

	func prepare() {
		guard windows.isEmpty else {
			return
		}
		for displayFrame in displayFrames {
			let window = QuickScreenshotSelectionWindow(displayFrame: displayFrame)
			window.contentView?.displayIfNeeded()
			windows.append(window)
		}
	}

	func show(initialSelection: CGRect?) {
		prepare()
		let focusedWindow =
			initialSelection.flatMap { selection in
				windows.first { $0.frame.intersects(selection) }
			} ?? windows.first
		activeSelectionWindow = focusedWindow
		for window in windows {
			window.selectionView.updateSelection(initialSelection)
			window.orderFrontRegardless()
			window.displayIfNeeded()
		}
	}

	func showArmed(pointer: CGPoint?) {
		prepare()
		activeSelectionWindow = nil
		for window in windows {
			window.selectionView.updateSelection(nil)
			window.selectionView.updatePointer(pointer)
			window.orderFrontRegardless()
			window.displayIfNeeded()
		}
	}

	func updatePointer(_ pointer: CGPoint?) {
		for window in windows {
			window.selectionView.updatePointer(pointer)
		}
	}

	func update(selection: CGRect) {
		let targetWindow =
			activeSelectionWindow
			?? windows.first { $0.frame.intersects(selection) }
			?? windows.first
		activeSelectionWindow = targetWindow
		guard let targetWindow else {
			return
		}
		targetWindow.selectionView.updateSelection(selection)
		targetWindow.displayIfNeeded()
	}

	func close() {
		for window in windows {
			window.selectionView.clearPresentation()
			window.orderOut(nil)
		}
		activeSelectionWindow = nil
		windows.removeAll()
	}
}

@MainActor
private final class QuickScreenshotSelectionWindow: NSPanel {
	let selectionView: QuickScreenshotSelectionView
	private let rootView: NSView

	override var canBecomeKey: Bool { false }
	override var canBecomeMain: Bool { false }

	init(displayFrame: QuickScreenshotDisplayFrame) {
		let contentFrame = CGRect(origin: .zero, size: displayFrame.frame.size)
		selectionView = QuickScreenshotSelectionView(displayFrame: displayFrame)
		rootView = NSView(frame: contentFrame)
		super.init(
			contentRect: displayFrame.frame,
			styleMask: [.borderless, .nonactivatingPanel],
			backing: .buffered,
			defer: false
		)
		setFrame(displayFrame.frame, display: false)
		let imageView = NSImageView(frame: contentFrame)
		imageView.autoresizingMask = [.width, .height]
		imageView.image = NSImage(
			cgImage: displayFrame.image,
			size: displayFrame.frame.size
		)
		imageView.imageScaling = .scaleAxesIndependently
		selectionView.frame = contentFrame
		selectionView.autoresizingMask = [.width, .height]
		rootView.addSubview(imageView)
		rootView.addSubview(selectionView)
		contentView = rootView
		animationBehavior = .none
		backgroundColor = .clear
		collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
		hasShadow = false
		hidesOnDeactivate = false
		ignoresMouseEvents = true
		isFloatingPanel = true
		isMovable = false
		isOpaque = false
		isReleasedWhenClosed = false
		level = .screenSaver
		sharingType = .readOnly
		titleVisibility = .hidden
		titlebarAppearsTransparent = true
	}
}

@MainActor
private final class QuickScreenshotSelectionView: NSView {
	private let displayFrame: QuickScreenshotDisplayFrame
	private let rootLayer = CALayer()
	private let scrimLayer = LiveScrimLayer()
	private let dragBorderOutlineLayer = CAShapeLayer()
	private let dragBorderLayer = CAShapeLayer()
	private let selectionSizeLayer = CATextLayer()
	private let pointerLayer = PointerAccentLayer()
	private var selection: CGRect?
	private var pointer: CGPoint?

	init(displayFrame: QuickScreenshotDisplayFrame) {
		self.displayFrame = displayFrame
		super.init(frame: CGRect(origin: .zero, size: displayFrame.frame.size))
		wantsLayer = true
		layer = rootLayer
		configureLayers()
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		nil
	}

	override var isFlipped: Bool { false }
	override var isOpaque: Bool { false }

	override func layout() {
		super.layout()
		rootLayer.frame = bounds
		if let selection {
			updateSelectionLayers(selection)
		}
		updateCrosshairLayers(pointer)
	}

	func updateSelection(_ selection: CGRect?) {
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		self.selection = selection
		if let selection {
			updateSelectionLayers(selection)
		} else {
			hideSelectionLayers()
		}
		CATransaction.commit()
	}

	func updatePointer(_ pointer: CGPoint?) {
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		defer { CATransaction.commit() }

		self.pointer = pointer
		updateCrosshairLayers(pointer)
	}

	func clearPresentation() {
		updateSelection(nil)
		updatePointer(nil)
	}

	private func updateCrosshairLayers(_ pointer: CGPoint?) {
		guard let pointer else {
			hideCrosshairLayers()
			return
		}
		let localPoint = CGPoint(
			x: pointer.x - displayFrame.frame.minX,
			y: pointer.y - displayFrame.frame.minY
		)
		guard bounds.contains(localPoint) else {
			hideCrosshairLayers()
			return
		}
		let scale = window?.screen?.backingScaleFactor ?? 1
		pointerLayer.update(pointer: localPoint, in: bounds, contentsScale: scale)
	}

	private func configureLayers() {
		rootLayer.masksToBounds = true
		rootLayer.isOpaque = false
		rootLayer.backgroundColor = NSColor.clear.cgColor

		scrimLayer.isHidden = true
		rootLayer.addSublayer(scrimLayer)

		for layer in [dragBorderOutlineLayer, dragBorderLayer] {
			layer.fillColor = NSColor.clear.cgColor
			layer.lineCap = .butt
			layer.lineJoin = .miter
			layer.isHidden = true
			rootLayer.addSublayer(layer)
		}
		dragBorderOutlineLayer.strokeColor =
			NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
			.cgColor
		dragBorderOutlineLayer.lineWidth = CaptureChrome.liveDashedBorderWidth + 0.75
		dragBorderLayer.strokeColor =
			NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor
		dragBorderLayer.lineWidth = CaptureChrome.liveDashedBorderWidth

		selectionSizeLayer.isHidden = true
		selectionSizeLayer.alignmentMode = .left
		selectionSizeLayer.foregroundColor = NSColor.white.withAlphaComponent(0.98).cgColor
		selectionSizeLayer.font = LiveOverlayTypography.font
		selectionSizeLayer.fontSize = LiveOverlayTypography.font.pointSize
		rootLayer.addSublayer(selectionSizeLayer)

		rootLayer.addSublayer(pointerLayer)
	}

	private func updateSelectionLayers(_ globalSelection: CGRect) {
		let selectionRect = localRect(from: globalSelection)
		let scale = window?.screen?.backingScaleFactor ?? 1
		scrimLayer.frame = bounds
		scrimLayer.contentsScale = scale
		scrimLayer.update(
			focusRect: selectionRect,
			color: NSColor(calibratedWhite: 0, alpha: CaptureChrome.liveScrimAlpha).cgColor,
			roundedExclusions: []
		)
		scrimLayer.isHidden = false

		let borderOutset = CaptureChrome.dashedBorderOutset(
			strokeWidth: CaptureChrome.liveDashedBorderWidth,
			pixelsPerPoint: scale
		)
		let borderRect = selectionRect.insetBy(dx: -borderOutset, dy: -borderOutset)
		let layerFrame = dashedBorderLayerFrame(
			for: borderRect,
			lineWidth: CaptureChrome.liveDashedBorderWidth + 0.75
		)
		let localBorderRect = borderRect.offsetBy(dx: -layerFrame.minX, dy: -layerFrame.minY)
		let path = CaptureChrome.dashedBorderPath(for: localBorderRect)
		for layer in [dragBorderOutlineLayer, dragBorderLayer] {
			layer.frame = layerFrame
			layer.contentsScale = scale
			layer.path = path
			layer.isHidden = selectionRect.intersects(bounds) == false
		}

		renderSelectionSizeBadge(selectionRect, scale: scale)
	}

	private func hideSelectionLayers() {
		scrimLayer.isHidden = true
		dragBorderOutlineLayer.isHidden = true
		dragBorderLayer.isHidden = true
		selectionSizeLayer.isHidden = true
	}

	private func hideCrosshairLayers() {
		pointerLayer.hide()
	}

	private func renderSelectionSizeBadge(_ selectionRect: CGRect, scale: CGFloat) {
		guard selectionRect.intersects(bounds) else {
			selectionSizeLayer.isHidden = true
			return
		}
		let text = selectionSizeText(for: selectionRect)
		let font = LiveOverlayTypography.font
		let textSize = text.size(using: font)
		selectionSizeLayer.contentsScale = scale
		selectionSizeLayer.string = text
		selectionSizeLayer.frame = CaptureChrome.selectionSizeBadgeFrame(
			for: selectionRect,
			textSize: textSize,
			in: bounds
		)
		selectionSizeLayer.isHidden = false
	}

	private func dashedBorderLayerFrame(for borderRect: CGRect, lineWidth: CGFloat) -> CGRect {
		let padding = max(lineWidth + 2, 4)
		return borderRect.insetBy(dx: -padding, dy: -padding)
	}

	private func selectionSizeText(for rect: CGRect) -> String {
		let scale = window?.screen?.backingScaleFactor ?? 1
		let sizeText = "\(Int(round(rect.width * scale)))x\(Int(round(rect.height * scale)))px"

		if abs(scale - 1) <= 0.005 {
			return sizeText
		}

		return "\(sizeText) @\(String(format: "%g", Double(scale)))x"
	}

	private func localRect(from globalRect: CGRect) -> CGRect {
		CGRect(
			x: globalRect.minX - displayFrame.frame.minX,
			y: globalRect.minY - displayFrame.frame.minY,
			width: globalRect.width,
			height: globalRect.height
		)
	}
}
