import AppKit
import CoreGraphics
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
	private lazy var windowSnapshotFeed = WindowSnapshotFeed()
	private let liveChromePipeline: LiveChromePipeline
	private var pendingCaptureStreamPreparation: (() -> Void)?
	private var primaryMousePassthroughToken: UInt64 = 0
	private var allMousePassthroughActive = false

	init(
		controller: CaptureSessionController,
		liveFrameStream: LiveFrameStreamBroker,
		frameRgbSampler: @escaping ChromeSampleFeed.FrameRgbSampler,
		framePatchSampler: @escaping ChromeSampleFeed.FramePatchSampler,
		frameRegionSampler: @escaping LiveChromePipeline.FrameRegionSampler
	) {
		self.controller = controller
		self.liveChromePipeline = LiveChromePipeline(
			liveFrameStream: liveFrameStream,
			frameRgbSampler: frameRgbSampler,
			framePatchSampler: framePatchSampler,
			frameRegionSampler: frameRegionSampler,
			sampleUpdated: { [weak controller] in
				(controller?.overlayController?.primaryWindow as? CaptureOverlayWindow)?.hostView
					.refreshSampleUpdatedLiveChromeNow()
			}
		)
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
		NSApp.activate(ignoringOtherApps: true)
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
		let captureID = controller?.activeTelemetryCaptureID ?? 0
		liveChromePipeline.startFrameStream(
			for: NSScreen.screens,
			focusPoint: focusPoint,
			captureID: captureID
		)
		for window in windows {
			window.displayIfNeeded()
		}
		windowSnapshotFeed.start(
			desktopFrame: Self.desktopFrame,
			initialSnapshots: initialWindowSnapshots,
			captureID: captureID)
		liveChromePipeline.startSampling(
			focusPoint: focusPoint,
			captureID: captureID,
			source: liveColorSampleSource(near: focusPoint)
		)
		if let focusedWindow {
			focusedWindow.hostView.refreshLivePresentationNow()
			focusedWindow.displayIfNeeded()
			focusedWindow.hostView.forceVisibleCursorRefresh()
		}
	}

	func showFrozenFirstFrame(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings,
		focusPoint: CGPoint
	) {
		close()
		NSApp.activate(ignoringOtherApps: true)
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
		NSApp.activate(ignoringOtherApps: true)
		targetWindow.makeKey()
		targetWindow.makeFirstResponder(targetWindow.hostView)
		focusedWindowNumber = targetWindow.windowNumber
		(NSApp.delegate as? NativeHostApplicationController)?.window = targetWindow
		for window in windows where window !== targetWindow {
			window.hostView.clearVisibleCursorOverride()
		}
		liveChromePipeline.hideBackdrops()
		targetWindow.hostView.refreshLivePresentationNow()
		targetWindow.displayIfNeeded()
		targetWindow.hostView.forceVisibleCursorRefresh()
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
		liveChromePipeline.stop()
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
			window.hostView.clearVisibleCursorOverride()
			window.hostView.clearPrimaryInteractionState(rendersImmediately: false)
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
		liveChromePipeline.backgroundPatch(in: rect) { [self] in
			captureImageBelowOverlay(in: rect, near: CGPoint(x: rect.midX, y: rect.midY))
		}
	}

	func streamPatch(in rect: CGRect) -> CGImage? {
		liveChromePipeline.streamPatch(in: rect)
	}

	func cachedRegionImage(in rect: CGRect) -> CGImage? {
		liveChromePipeline.cachedRegionImage(in: rect)
	}

	func nextRegionFrame(
		in rect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		liveChromePipeline.nextRegionFrame(
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
		liveChromePipeline.nextRegionFrame(
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
		liveChromePipeline.updateLivePreviewDemand(
			point: point,
			settings: settings,
			includeLoupePatch: includeLoupePatch,
			source: point.flatMap { liveColorSampleSource(near: $0) }
		)
	}

	func liveChromeSnapshot(
		point: CGPoint?,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		liveChromePipeline.liveChromeSnapshot(
			point: point,
			settings: settings,
			includeLoupePatch: includeLoupePatch
		)
	}

	func immediateLiveChromeSample(
		point: CGPoint,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		liveChromePipeline.immediateLiveChromeSample(
			point: point,
			settings: settings,
			includeLoupePatch: includeLoupePatch
		)
	}

	func updateLiveChromeBackdrops(
		_ snapshot: LiveChromeBackdropSnapshot?
	) {
		liveChromePipeline.updateLiveChromeBackdrops(
			snapshot,
			focusedWindowNumber: focusedWindowNumber
		)
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
		return OverlayImageSampler.captureBelowOverlay(in: rect, source: source)
	}

	static var desktopFrame: CGRect {
		NSScreen.screens.map(\.frame).reduce(.null) { frame, next in
			frame.isNull ? next : frame.union(next)
		}
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
		liveChromePipeline.stop()

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
			window.hostView.clearVisibleCursorOverride()
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
