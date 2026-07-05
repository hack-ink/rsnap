import AppKit
import CoreGraphics
import Darwin
import Foundation
import RsnapHostBridge

@MainActor
final class CaptureOverlayController {
	private weak var controller: CaptureSessionController?
	private var windows: [CaptureOverlayWindow] = []
	private var retiringWindows: [CaptureOverlayWindow] = []
	private weak var livePrimaryInteractionOwner: CaptureHostView?
	private var focusedWindowNumber: Int?
	private var collapsedForFrozen = false
	private let liveFrameStream: LiveFrameStreamBroker
	private let frameRgbSampler: ChromeSampleFeed.FrameRgbSampler
	private let framePatchSampler: ChromeSampleFeed.FramePatchSampler
	private lazy var windowSnapshotFeed = WindowSnapshotFeed()
	private lazy var chromeSampleFeed = ChromeSampleFeed(
		broker: liveFrameStream,
		frameRgbSampler: frameRgbSampler,
		framePatchSampler: framePatchSampler,
		backgroundSampler: Self.chromeSampleAtDisplayPoint,
		sampleUpdated: { [weak self] in
			DispatchQueue.main.async { [weak self] in
				(self?.primaryWindow as? CaptureOverlayWindow)?.hostView
					.refreshSampleUpdatedLiveChromeNow()
			}
		}
	)
	private let liveChromeBackdrops = LiveChromeBackdropWindowController()
	private var pendingCaptureStreamPreparation: (() -> Void)?
	private var primaryMousePassthroughToken: UInt64 = 0
	private var allMousePassthroughActive = false

	init(
		controller: CaptureSessionController,
		liveFrameStream: LiveFrameStreamBroker,
		frameRgbSampler: @escaping ChromeSampleFeed.FrameRgbSampler,
		framePatchSampler: @escaping ChromeSampleFeed.FramePatchSampler
	) {
		self.controller = controller
		self.liveFrameStream = liveFrameStream
		self.frameRgbSampler = frameRgbSampler
		self.framePatchSampler = framePatchSampler
	}

	var primaryWindow: NSWindow? {
		windows.first(where: { $0.windowNumber == focusedWindowNumber }) ?? windows.first
	}

	var selfCaptureExceptionWindowIDs: Set<CGWindowID> {
		Set(windows.map { CGWindowID($0.windowNumber) })
	}

	func show(
		initialScene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings,
		focusPoint: CGPoint,
		initialWindowSnapshots: [WindowSnapshot],
		prepareCaptureStreams: (() -> Void)? = nil
	) {
		close()
		var targetWindow: CaptureOverlayWindow?
		for screen in NSScreen.screens {
			let window = CaptureOverlayWindow(
				screen: screen,
				controller: controller,
				initialScene: initialScene,
				initialChrome: chrome,
				initialSettings: settings
			)
			window.hostView.update(
				scene: initialScene,
				chrome: chrome,
				settings: settings
			)
			windows.append(window)
			if targetWindow == nil, screen.frame.inclusivelyContains(focusPoint) {
				targetWindow = window
			}
		}

		let focusedWindow = targetWindow ?? windows.first
		for window in windows {
			window.orderFrontRegardless()
			if window === focusedWindow {
				window.makeKey()
				window.makeFirstResponder(window.hostView)
				focusedWindowNumber = window.windowNumber
				(NSApp.delegate as? NativeHostApplicationController)?.window = window
			}
		}
		collapsedForFrozen = false
		if let prepareCaptureStreams {
			pendingCaptureStreamPreparation = prepareCaptureStreams
		}
		liveFrameStream.start(
			for: NSScreen.screens,
			prewarmPoint: focusPoint,
			captureID: controller?.activeTelemetryCaptureID ?? 0
		)
		for window in windows {
			window.displayIfNeeded()
		}
		let captureID = controller?.activeTelemetryCaptureID ?? 0
		windowSnapshotFeed.start(
			desktopFrame: Self.desktopFrame,
			initialSnapshots: initialWindowSnapshots,
			captureID: captureID)
		chromeSampleFeed.start(
			targetFramesPerSecond: NativeHostDisplayRefresh.samplingFramesPerSecond(),
			captureID: captureID)
		chromeSampleFeed.updateDemand(
			point: focusPoint,
			sidePixels: 1,
			includeLoupePatch: false,
			source: liveColorSampleSource(near: focusPoint)
		)
		if let focusedWindow {
			focusedWindow.hostView.refreshLivePresentationNow()
			focusedWindow.displayIfNeeded()
		}
	}

	func showFrozenFirstFrame(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings,
		focusPoint: CGPoint
	) {
		close()
		var targetWindow: CaptureOverlayWindow?
		for screen in NSScreen.screens {
			let window = CaptureOverlayWindow(
				screen: screen,
				controller: controller,
				initialScene: scene,
				initialChrome: chrome,
				initialSettings: settings
			)
			window.hostView.update(
				scene: scene,
				chrome: chrome,
				settings: settings
			)
			windows.append(window)
			if targetWindow == nil, screen.frame.inclusivelyContains(focusPoint) {
				targetWindow = window
			}
		}

		let focusedWindow = targetWindow ?? windows.first
		for window in windows {
			window.orderFrontRegardless()
			if window === focusedWindow {
				window.makeKey()
				window.makeFirstResponder(window.hostView)
				focusedWindowNumber = window.windowNumber
				(NSApp.delegate as? NativeHostApplicationController)?.window = window
			}
		}
		collapsedForFrozen = false
		for window in windows {
			window.displayIfNeeded()
		}
		presentFrozenFirstFrame(
			scene: scene,
			chrome: chrome,
			settings: settings
		)
		primaryWindow?.displayIfNeeded()
	}

	func prepareCaptureStreamsNow(trigger: String) {
		guard let prepareCaptureStreams = pendingCaptureStreamPreparation else {
			return
		}
		pendingCaptureStreamPreparation = nil
		NativeHostTelemetry.captureEvent(
			"capture.stream_prepare_started",
			captureID: controller?.activeTelemetryCaptureID ?? 0,
			detail: "trigger=\(trigger) overlayWindowCount=\(selfCaptureExceptionWindowIDs.count)"
		)
		prepareCaptureStreams()
	}

	func update(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) {
		if scene.mode == .frozen, let selection = scene.frozenSelection {
			prepareFrozenPresentation(for: selection)
		}
		for window in windows {
			window.hostView.update(
				scene: scene,
				chrome: chrome,
				settings: settings
			)
		}
	}

	func markLivePrimaryInteractionReleased(at point: CGPoint) {
		if let owner = livePrimaryInteractionOwner, owner.hasLivePrimaryInteraction {
			owner.markLivePrimaryInteractionReleased(at: point)
			return
		}
		for window in windows where window.hostView.hasLivePrimaryInteraction {
			window.hostView.markLivePrimaryInteractionReleased(at: point)
		}
	}

	func registerLivePrimaryInteractionOwner(_ owner: CaptureHostView) {
		livePrimaryInteractionOwner = owner
	}

	func completeLivePrimaryInteraction(from sender: CaptureHostView, at point: CGPoint) {
		guard
			let owner = livePrimaryInteractionOwner,
			owner.hasLivePrimaryInteraction
		else {
			if sender.hasLivePrimaryInteraction {
				sender.completeOwnedLivePrimaryInteraction(at: point)
				livePrimaryInteractionOwner = nil
			}
			return
		}
		owner.completeOwnedLivePrimaryInteraction(at: point)
		livePrimaryInteractionOwner = nil
	}

	func presentFrozenFirstFrame(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) {
		guard
			scene.mode == .frozen,
			let selection = scene.frozenSelection
		else {
			update(scene: scene, chrome: chrome, settings: settings)
			return
		}
		prepareFrozenPresentation(for: selection)
		guard
			let primaryWindow = windows.first(where: {
				$0.frame.contains(CGPoint(x: selection.midX, y: selection.midY))
			}) ?? windows.first
		else {
			update(scene: scene, chrome: chrome, settings: settings)
			return
		}

		primaryWindow.hostView.installFrozenFirstFrame(
			scene: scene,
			chrome: chrome,
			settings: settings,
			rendersPendingFrame: false
		)
		primaryWindow.hostView.finishFrozenFirstFrameInstall()
	}

	func focusWindow(at point: CGPoint) {
		guard
			let targetWindow = windows.first(where: { $0.frame.inclusivelyContains(point) })
				?? windows.first
		else {
			return
		}
		if focusedWindowNumber == targetWindow.windowNumber, targetWindow.isKeyWindow {
			return
		}

		targetWindow.orderFrontRegardless()
		targetWindow.makeKey()
		targetWindow.makeFirstResponder(targetWindow.hostView)
		focusedWindowNumber = targetWindow.windowNumber
		(NSApp.delegate as? NativeHostApplicationController)?.window = targetWindow
		liveChromeBackdrops.hideAll()
		targetWindow.hostView.refreshLivePresentationNow()
		targetWindow.displayIfNeeded()
	}

	func withPrimaryMousePassthrough<T>(duration: TimeInterval, perform: () -> T) -> T {
		guard let window = primaryWindow as? CaptureOverlayWindow else {
			return perform()
		}
		primaryMousePassthroughToken &+= 1
		let token = primaryMousePassthroughToken
		window.ignoresMouseEvents = true
		let result = perform()
		DispatchQueue.main.asyncAfter(deadline: .now() + duration) { [weak self, weak window] in
			guard let self, self.primaryMousePassthroughToken == token else {
				return
			}
			window?.ignoresMouseEvents = false
		}
		return result
	}

	func withAllMousePassthrough<T>(duration: TimeInterval, perform: () -> T) -> T {
		let visibleWindows = windows.filter(\.isVisible)
		guard visibleWindows.isEmpty == false else {
			return perform()
		}
		primaryMousePassthroughToken &+= 1
		let token = primaryMousePassthroughToken
		if allMousePassthroughActive == false {
			allMousePassthroughActive = true
			for window in visibleWindows where window.ignoresMouseEvents == false {
				window.ignoresMouseEvents = true
			}
			NSApp.updateWindows()
		}
		let result = perform()
		DispatchQueue.main.asyncAfter(deadline: .now() + duration) { [weak self] in
			guard let self, self.primaryMousePassthroughToken == token else {
				return
			}
			self.allMousePassthroughActive = false
			for window in visibleWindows {
				window.ignoresMouseEvents = false
			}
		}
		return result
	}

	func setScrollCaptureMousePassthroughActive(_ active: Bool) {
		primaryMousePassthroughToken &+= 1
		allMousePassthroughActive = active
		for window in windows {
			window.ignoresMouseEvents = active
		}
		guard active == false, let window = primaryWindow as? CaptureOverlayWindow else {
			return
		}
		window.orderFrontRegardless()
		window.makeKey()
		window.makeFirstResponder(window.hostView)
	}

	func refreshScrollCaptureToolbarBackdropNow() {
		for window in windows where window.isVisible {
			window.hostView.refreshScrollCaptureToolbarBackdropNow()
		}
	}

	func withOverlayHiddenForScrollTargetAcquisition<T>(perform: () -> T) -> T {
		let visibleWindows = windows.filter(\.isVisible)
		let previousIgnoresMouseEvents = visibleWindows.map { $0.ignoresMouseEvents }
		let previousFocusedWindowNumber = focusedWindowNumber

		for window in visibleWindows {
			window.ignoresMouseEvents = true
			window.orderOut(nil)
		}
		NSApp.updateWindows()
		RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.08))

		let result = perform()

		for (index, window) in visibleWindows.enumerated() {
			window.orderFrontRegardless()
			if previousFocusedWindowNumber == window.windowNumber {
				window.makeKey()
				window.makeFirstResponder(window.hostView)
				(NSApp.delegate as? NativeHostApplicationController)?.window = window
			}
			window.ignoresMouseEvents = previousIgnoresMouseEvents[index]
		}

		return result
	}

	func close() {
		pendingCaptureStreamPreparation = nil
		windowSnapshotFeed.stop()
		chromeSampleFeed.stop()
		liveChromeBackdrops.hideAll()
		guard windows.isEmpty == false else {
			focusedWindowNumber = nil
			collapsedForFrozen = false
			return
		}

		let windowsToRetire = windows
		windows.removeAll()
		livePrimaryInteractionOwner = nil
		focusedWindowNumber = nil
		collapsedForFrozen = false
		(NSApp.delegate as? NativeHostApplicationController)?.window = nil

		for window in windowsToRetire {
			window.hostView.clearLivePrimaryInteractionState(rendersImmediately: false)
			window.hostView.finishLivePresentationTelemetry(reason: "close")
			window.hostView.controller = nil
			window.ignoresMouseEvents = true
			window.orderOut(nil)
		}

		retiringWindows.append(contentsOf: windowsToRetire)
		DispatchQueue.main.async { [weak self] in
			self?.retiringWindows.removeAll()
		}
	}

	func hoverWindow(at point: CGPoint) -> WindowSnapshot? {
		guard NSScreen.screens.contains(where: { $0.frame.inclusivelyContains(point) }) else {
			return nil
		}
		return windowSnapshotFeed.window(at: point)
	}

	func hoverWindowPreview(at point: CGPoint) -> WindowSnapshot? {
		guard NSScreen.screens.contains(where: { $0.frame.inclusivelyContains(point) }) else {
			return nil
		}
		return windowSnapshotFeed.window(at: point)
	}

	func backgroundPatch(in rect: CGRect) -> CGImage? {
		liveFrameStream.region(in: rect)
			?? captureImageBelowOverlay(in: rect, near: CGPoint(x: rect.midX, y: rect.midY))
			?? liveFrameStream.patch(in: rect)
	}

	func streamPatch(in rect: CGRect) -> CGImage? {
		liveFrameStream.patch(in: rect)
	}

	func cachedRegionImage(in rect: CGRect) -> CGImage? {
		liveFrameStream.region(in: rect)
	}

	func nextRegionFrame(
		in rect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		liveFrameStream.nextRegionFrame(
			in: rect,
			afterFrameSequence: afterFrameSequence,
			waitForFresh: waitForFresh
		)
	}

	func nextRegionFrame(
		in rect: CGRect,
		pixelRect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		liveFrameStream.nextRegionFrame(
			in: rect,
			pixelRect: pixelRect,
			afterFrameSequence: afterFrameSequence,
			waitForFresh: waitForFresh
		)
	}

	func updateLivePreviewDemand(
		point: CGPoint?,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) {
		let samplePixels = includeLoupePatch ? settings.loupeSampleSize.sidePixels : 1
		chromeSampleFeed.updateDemand(
			point: point,
			sidePixels: samplePixels,
			includeLoupePatch: includeLoupePatch,
			source: point.flatMap { liveColorSampleSource(near: $0) }
		)
	}

	func liveChromeSnapshot(
		point: CGPoint?,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		let latestSample = chromeSampleFeed.snapshot(for: point)
		let wantsLoupePatch = includeLoupePatch
		let wantsLoupePatchSide = settings.loupeSampleSize.sidePixels
		let latestLoupePatchSatisfiesDemand =
			latestSample?.loupePatch.map {
				$0.width == wantsLoupePatchSide && $0.height == wantsLoupePatchSide
			}
			?? false
		let latestSampleSatisfiesDemand =
			latestSample?.rgbSample != nil
			&& (!wantsLoupePatch || latestLoupePatchSatisfiesDemand)
		if latestSampleSatisfiesDemand {
			return latestSample
		}
		if wantsLoupePatch, latestLoupePatchSatisfiesDemand {
			return latestSample
		}

		let _ = point
		if wantsLoupePatch, let latestSample {
			return LiveChromeSample(rgb: latestSample.rgb, loupePatch: nil)
		}
		return latestSample
	}

	func immediateLiveChromeSample(
		point: CGPoint,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		let samplePixels = includeLoupePatch ? settings.loupeSampleSize.sidePixels : 1
		return liveFrameStream.sample(at: point, sidePixels: samplePixels)
			?? chromeSampleFeed.snapshot(for: point)
	}

	func updateLiveChromeBackdrops(
		_ snapshot: LiveChromeBackdropSnapshot?
	) {
		liveChromeBackdrops.update(snapshot: snapshot, focusedWindowNumber: focusedWindowNumber)
	}

	fileprivate func frozenCaptureJobSource(
		near point: CGPoint
	) -> CaptureSessionController.FrozenCaptureJobSource? {
		guard
			let referenceWindow = windows.first(where: { $0.frame.inclusivelyContains(point) })
				?? windows.first
		else {
			return nil
		}
		return CaptureSessionController.FrozenCaptureJobSource(
			referenceWindowID: CGWindowID(referenceWindow.windowNumber),
			desktopFrame: Self.desktopFrame,
			referenceFrame: referenceWindow.frame
		)
	}

	func scrollCaptureFallbackSource(
		near point: CGPoint
	) -> CaptureSessionController.FrozenCaptureJobSource? {
		frozenCaptureJobSource(near: point)
	}

	fileprivate func liveColorSampleSource(near point: CGPoint) -> LiveColorSampleSource? {
		guard
			let referenceWindow = windows.first(where: { $0.frame.inclusivelyContains(point) })
				?? windows.first
		else {
			return nil
		}
		let screen =
			NSScreen.screens.first(where: { $0.frame.inclusivelyContains(point) })
			?? referenceWindow.screen
		guard let displayID = screen.flatMap(Self.displayID) else {
			return nil
		}
		return LiveColorSampleSource(
			referenceWindowID: CGWindowID(referenceWindow.windowNumber),
			desktopFrame: Self.desktopFrame,
			screenFrame: screen?.frame ?? referenceWindow.frame,
			displayID: displayID,
			scaleFactor: screen?.backingScaleFactor ?? 1
		)
	}

	private static func displayID(for screen: NSScreen) -> CGDirectDisplayID? {
		screen.nativeDisplayID
	}

	func captureImageBelowOverlay(in rect: CGRect, near point: CGPoint) -> CGImage? {
		guard let source = frozenCaptureJobSource(near: point) else {
			return nil
		}
		return Self.captureImageBelowOverlay(in: rect, source: source)
	}

	nonisolated static func captureImageBelowOverlay(
		in rect: CGRect,
		source: CaptureSessionController.FrozenCaptureJobSource
	) -> CGImage? {
		let quartzRect = appKitRectToQuartz(rect, desktopFrame: source.desktopFrame)
		return legacyWindowListImage(
			quartzRect: quartzRect,
			windowListOption: .optionOnScreenBelowWindow,
			windowID: source.referenceWindowID,
			imageOption: [.boundsIgnoreFraming, .bestResolution]
		)
	}

	nonisolated private static func chromeSampleAtDisplayPoint(
		_ point: CGPoint,
		source: LiveColorSampleSource,
		sidePixels: Int,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		guard displayPointSampleGate.wait(timeout: .now()) == .success else {
			return nil
		}
		defer {
			displayPointSampleGate.signal()
		}
		let rgbSample = rgbSampleAtDisplayPoint(point, source: source)
		let loupePatch =
			includeLoupePatch
			? loupePatchAtDisplayPoint(point, source: source, sidePixels: sidePixels)
			: nil
		guard rgbSample != nil || loupePatch != nil else {
			return nil
		}
		return LiveChromeSample(
			rgbSample: rgbSample,
			rgbCapturedAtUptime: ProcessInfo.processInfo.systemUptime,
			rgbSource: "display_point",
			loupePatch: loupePatch
		)
	}

	nonisolated private static func rgbSampleAtDisplayPoint(
		_ point: CGPoint,
		source: LiveColorSampleSource
	) -> RGBSample? {
		let scaleFactor = max(source.scaleFactor, 1)
		let sampleSide = max(3 / scaleFactor, 1)
		let sampleRect = CGRect(
			x: point.x - sampleSide / 2,
			y: point.y - sampleSide / 2,
			width: sampleSide,
			height: sampleSide
		).intersection(source.screenFrame)
		guard sampleRect.isNull == false, sampleRect.width > 0, sampleRect.height > 0 else {
			return nil
		}
		guard
			let image = captureImageOnDisplay(in: sampleRect, source: source)
		else {
			return nil
		}
		return rgbSample(from: image)
	}

	nonisolated private static func loupePatchAtDisplayPoint(
		_ point: CGPoint,
		source: LiveColorSampleSource,
		sidePixels: Int
	) -> CGImage? {
		let scaleFactor = max(source.scaleFactor, 1)
		let sidePixels = max(sidePixels, 1)
		let sampleSide = max(CGFloat(sidePixels) / scaleFactor, 1 / scaleFactor)
		let sampleRect = CGRect(
			x: point.x - sampleSide / 2,
			y: point.y - sampleSide / 2,
			width: sampleSide,
			height: sampleSide
		).intersection(source.screenFrame)
		guard sampleRect.isNull == false, sampleRect.width > 0, sampleRect.height > 0 else {
			return nil
		}
		guard
			let image = captureImageBelowOverlay(
				in: sampleRect,
				source: source,
				imageOption: [.boundsIgnoreFraming, .bestResolution]
			)
		else {
			return nil
		}
		return normalizedPatchImage(image, sidePixels: sidePixels)
	}

	nonisolated private static func captureImageBelowOverlay(
		in rect: CGRect,
		source: LiveColorSampleSource,
		imageOption: CGWindowImageOption
	) -> CGImage? {
		let quartzRect = appKitRectToQuartz(rect, desktopFrame: source.desktopFrame)
		return legacyWindowListImage(
			quartzRect: quartzRect,
			windowListOption: .optionOnScreenBelowWindow,
			windowID: source.referenceWindowID,
			imageOption: imageOption
		)
	}

	nonisolated private static func captureImageOnDisplay(
		in rect: CGRect,
		source: LiveColorSampleSource
	) -> CGImage? {
		let displayRect = appKitRectToQuartz(rect, desktopFrame: source.desktopFrame)
		guard displayRect.isNull == false, displayRect.width > 0, displayRect.height > 0 else {
			return nil
		}
		return displayCreateImageForRect?(source.displayID, displayRect)?
			.takeRetainedValue()
	}

	nonisolated private static func rgbSample(from image: CGImage) -> RGBSample? {
		let width = max(image.width, 1)
		let height = max(image.height, 1)
		let bytesPerPixel = 4
		let bytesPerRow = width * bytesPerPixel
		var pixels = [UInt8](repeating: 0, count: bytesPerRow * height)
		let colorSpace = CGColorSpaceCreateDeviceRGB()
		let bitmapInfo =
			CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
		return pixels.withUnsafeMutableBytes { buffer -> RGBSample? in
			guard
				let baseAddress = buffer.baseAddress,
				let context = CGContext(
					data: baseAddress,
					width: width,
					height: height,
					bitsPerComponent: 8,
					bytesPerRow: bytesPerRow,
					space: colorSpace,
					bitmapInfo: bitmapInfo
				)
			else {
				return nil
			}
			context.interpolationQuality = .none
			context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
			let bytes = buffer.bindMemory(to: UInt8.self)
			let centerOffset = ((height / 2) * bytesPerRow) + ((width / 2) * bytesPerPixel)
			return RGBSample(
				r: bytes[centerOffset],
				g: bytes[centerOffset + 1],
				b: bytes[centerOffset + 2]
			)
		}
	}

	nonisolated private static func normalizedPatchImage(
		_ image: CGImage,
		sidePixels: Int
	) -> CGImage? {
		let sidePixels = max(sidePixels, 1)
		if image.width == sidePixels, image.height == sidePixels {
			return image
		}
		let bytesPerPixel = 4
		let bytesPerRow = sidePixels * bytesPerPixel
		var pixels = [UInt8](repeating: 0, count: bytesPerRow * sidePixels)
		let colorSpace = CGColorSpaceCreateDeviceRGB()
		let bitmapInfo =
			CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
		return pixels.withUnsafeMutableBytes { buffer -> CGImage? in
			guard
				let baseAddress = buffer.baseAddress,
				let context = CGContext(
					data: baseAddress,
					width: sidePixels,
					height: sidePixels,
					bitsPerComponent: 8,
					bytesPerRow: bytesPerRow,
					space: colorSpace,
					bitmapInfo: bitmapInfo
				)
			else {
				return nil
			}
			context.interpolationQuality = .none
			context.draw(
				image,
				in: CGRect(x: 0, y: 0, width: sidePixels, height: sidePixels)
			)
			return context.makeImage()
		}
	}

	private typealias LegacyWindowListCreateImage =
		@convention(c) (
			CGRect,
			UInt32,
			CGWindowID,
			UInt32
		) -> Unmanaged<CGImage>?

	private typealias DisplayCreateImageForRect =
		@convention(c) (
			CGDirectDisplayID,
			CGRect
		) -> Unmanaged<CGImage>?

	nonisolated private static let displayPointSampleGate = DispatchSemaphore(value: 1)

	nonisolated private static let displayCreateImageForRect: DisplayCreateImageForRect? = {
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

	nonisolated private static let legacyWindowListCreateImage: LegacyWindowListCreateImage? = {
		guard
			let coreGraphics = dlopen(
				"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",
				RTLD_LAZY
			)
		else {
			return nil
		}
		guard let symbol = dlsym(coreGraphics, "CGWindowListCreateImage") else {
			dlclose(coreGraphics)
			return nil
		}
		return unsafeBitCast(symbol, to: LegacyWindowListCreateImage.self)
	}()

	nonisolated private static func legacyWindowListImage(
		quartzRect: CGRect,
		windowListOption: CGWindowListOption,
		windowID: CGWindowID,
		imageOption: CGWindowImageOption
	) -> CGImage? {
		guard let createImage = legacyWindowListCreateImage else {
			return nil
		}
		return createImage(
			quartzRect,
			windowListOption.rawValue,
			windowID,
			imageOption.rawValue
		)?
		.takeRetainedValue()
	}

	static var desktopFrame: CGRect {
		NSScreen.screens.map(\.frame).reduce(.null) { frame, next in
			frame.isNull ? next : frame.union(next)
		}
	}

	private static func quartzRectToAppKit(_ rect: CGRect, desktopFrame: CGRect) -> CGRect {
		CGRect(
			x: rect.minX,
			y: desktopFrame.maxY - rect.maxY,
			width: rect.width,
			height: rect.height
		)
	}

	nonisolated private static func appKitRectToQuartz(_ rect: CGRect, desktopFrame: CGRect)
		-> CGRect
	{
		CGRect(
			x: rect.minX,
			y: desktopFrame.maxY - rect.maxY,
			width: rect.width,
			height: rect.height
		)
	}

	private func prepareFrozenPresentation(for selection: CGRect) {
		guard collapsedForFrozen == false else {
			return
		}
		collapsedForFrozen = true
		guard collapsedForFrozen, windows.isEmpty == false else {
			return
		}
		windowSnapshotFeed.stop()
		chromeSampleFeed.stop()
		liveChromeBackdrops.hideAll()

		guard windows.count > 1 else {
			return
		}

		let focusPoint = CGPoint(x: selection.midX, y: selection.midY)
		guard
			let primaryWindow = windows.first(where: { $0.frame.inclusivelyContains(focusPoint) })
				?? windows.first
		else {
			return
		}

		let secondaryWindows = windows.filter { $0 !== primaryWindow }
		windows = [primaryWindow]
		focusedWindowNumber = primaryWindow.windowNumber
		(NSApp.delegate as? NativeHostApplicationController)?.window = primaryWindow
		primaryWindow.makeFirstResponder(primaryWindow.hostView)

		for window in secondaryWindows {
			window.hostView.controller = nil
			window.ignoresMouseEvents = true
			window.orderOut(nil)
		}

		retiringWindows.append(contentsOf: secondaryWindows)
		DispatchQueue.main.async { [weak self] in
			self?.retiringWindows.removeAll()
		}
	}

}
