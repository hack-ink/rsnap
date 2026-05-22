import AppKit
@preconcurrency import CoreGraphics
import Darwin
import Foundation

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
	}

	private static let minimumSelectionSide: CGFloat = 2
	private static let eventMask =
		(CGEventMask(1) << CGEventType.leftMouseDown.rawValue)
		| (CGEventMask(1) << CGEventType.leftMouseDragged.rawValue)
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
		switch type {
		case .tapDisabledByTimeout, .tapDisabledByUserInput:
			if let eventTap {
				CGEvent.tapEnable(tap: eventTap, enable: true)
			}
			return nil
		case .keyDown:
			if event.getIntegerValueField(.keyboardEventKeycode) == 53 {
				cancel(reason: "escape")
				return nil
			}
			return nil
		case .rightMouseDown:
			cancel(reason: "right_mouse")
			return nil
		case .leftMouseDown:
			beginSelection(at: Self.appKitPoint(from: event))
			return nil
		case .leftMouseDragged:
			updateSelection(to: Self.appKitPoint(from: event))
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
		if let eventTap {
			CGEvent.tapEnable(tap: eventTap, enable: false)
		}
		if let eventTapSource {
			CFRunLoopRemoveSource(CFRunLoopGetMain(), eventTapSource, .commonModes)
		}
		eventTap = nil
		eventTapSource = nil
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
		for window in windows {
			window.selectionView.selection = initialSelection
			window.selectionView.needsDisplay = true
			window.orderFrontRegardless()
			window.displayIfNeeded()
		}
	}

	func update(selection: CGRect) {
		for window in windows {
			window.selectionView.selection = selection
			window.selectionView.needsDisplay = true
			window.displayIfNeeded()
		}
	}

	func close() {
		for window in windows {
			window.orderOut(nil)
		}
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
	var selection: CGRect?

	init(displayFrame: QuickScreenshotDisplayFrame) {
		self.displayFrame = displayFrame
		super.init(frame: CGRect(origin: .zero, size: displayFrame.frame.size))
		wantsLayer = true
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		nil
	}

	override var isFlipped: Bool { false }
	override var isOpaque: Bool { false }

	override func draw(_ dirtyRect: NSRect) {
		super.draw(dirtyRect)
		NSGraphicsContext.current?.cgContext.clear(dirtyRect)
		drawDimmedMask()
	}

	private func drawDimmedMask() {
		let maskPath = NSBezierPath(rect: bounds)
		if let selection {
			maskPath.append(NSBezierPath(rect: localRect(from: selection)))
			maskPath.windingRule = .evenOdd
		}
		NSColor(calibratedWhite: 0, alpha: CaptureChrome.liveScrimAlpha).setFill()
		maskPath.fill()

		guard let selection else {
			return
		}
		let selectionRect = localRect(from: selection)
		NSColor.white.withAlphaComponent(0.96).setStroke()
		let stroke = NSBezierPath(rect: selectionRect)
		stroke.lineWidth = 1.5
		stroke.stroke()
		NSColor.systemBlue.withAlphaComponent(0.95).setStroke()
		let innerStroke = NSBezierPath(rect: selectionRect.insetBy(dx: 1.5, dy: 1.5))
		innerStroke.lineWidth = 1
		innerStroke.stroke()
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
