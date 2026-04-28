import AppKit
import CoreGraphics
import CoreImage
import CoreText
import Darwin
import Foundation
import RsnapHostBridge
import Vision

struct LiveChromeSample {
	let rgbSample: RGBSample?
	let loupePatch: CGImage?
}

enum LiveSamplingBudget {
	static let hoverWindowCacheRefreshInterval: TimeInterval = 1.0 / 15.0
}

struct LiveColorSampleSource: Equatable, Sendable {
	let referenceWindowID: CGWindowID
	let desktopFrame: CGRect
	let screenFrame: CGRect
	let scaleFactor: CGFloat
}

private struct LiveChromeRefreshTelemetryKey: Equatable {
	let targetHz: Int
	let hudGlassEnabled: Bool
	let hudGlassMode: String
	let liquidGlassStyle: String
	let liquidGlassAvailable: Bool
}

@MainActor private let frozenEffectCIContext = CIContext(options: nil)

private enum CaptureSuccessSound {
	private static let candidatePaths = [
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Screen Capture.aif",
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Shutter.aif",
	]

	static func load() -> NSSound? {
		for path in candidatePaths {
			if let sound = NSSound(contentsOfFile: path, byReference: true) {
				NativeHostTelemetry.lifecycleEvent(
					"native_host.capture_success_sound_loaded",
					detail: "path=\(path)"
				)
				return sound
			}
		}

		let candidates = candidatePaths.joined(separator: ",")
		NativeHostTelemetry.lifecycleWarning(
			"native_host.capture_success_sound_load_failed",
			detail: "candidates=\(candidates)"
		)
		return nil
	}

	static func play(_ sound: NSSound?) {
		guard let sound else {
			return
		}
		sound.stop()
		sound.currentTime = 0
		if !sound.play() {
			NativeHostTelemetry.lifecycleWarning(
				"native_host.capture_success_sound_play_failed")
		}
	}
}

@MainActor private func makeFrozenMosaicImage(from image: CGImage) -> CGImage? {
	let ciImage = CIImage(cgImage: image)
	guard let filter = CIFilter(name: "CIPixellate") else {
		return nil
	}
	filter.setValue(ciImage, forKey: kCIInputImageKey)
	filter.setValue(18.0, forKey: kCIInputScaleKey)
	guard let outputImage = filter.outputImage?.cropped(to: ciImage.extent) else {
		return nil
	}
	return frozenEffectCIContext.createCGImage(outputImage, from: outputImage.extent)
}

@MainActor
public final class NativeHostApplicationController: NSObject, NSApplicationDelegate {
	private let settingsStore = NativeHostSettingsStore()
	private let globalHotKeys = GlobalHotKeyCenter()
	private var lifecycleActivity: NSObjectProtocol?
	private var liveSamplingPrewarmWorkItem: DispatchWorkItem?
	private var didBootstrap = false
	@objc public dynamic var window: NSWindow?
	private lazy var sessionController: CaptureSessionController = {
		let controller = CaptureSessionController(settingsStore: settingsStore)
		controller.captureStateDidChange = { [weak self] in
			self?.refreshStatusMenuState()
		}
		controller.sceneDidChange = { [weak self] scene in
			self?.refreshHotKeyBindings(for: scene.mode)
		}
		return controller
	}()
	private var statusItem: NSStatusItem?
	private weak var captureMenuItem: NSMenuItem?
	private weak var cancelCaptureMenuItem: NSMenuItem?
	private lazy var settingsWindowController = SettingsWindowController(
		settingsStore: settingsStore)
	private lazy var permissionsWindowController = PermissionsWindowController()

	public func finishLaunching() {
		guard !didBootstrap else {
			return
		}
		didBootstrap = true
		NativeHostTelemetry.lifecycleEvent("native_host.finish_launching_begin")
		NSApp.setActivationPolicy(.accessory)
		ProcessInfo.processInfo.disableAutomaticTermination("rsnap menubar host")
		ProcessInfo.processInfo.disableSuddenTermination()
		lifecycleActivity = ProcessInfo.processInfo.beginActivity(
			options: [.automaticTerminationDisabled, .suddenTerminationDisabled],
			reason: "rsnap menubar host"
		)
		Self.applyApplicationIcon()
		configureStatusItem()
		configureGlobalHotKeys()
		NotificationCenter.default.addObserver(
			self,
			selector: #selector(settingsDidChange),
			name: NativeHostSettingsStore.didChangeNotification,
			object: settingsStore
		)
		refreshHotKeyBindings(for: sessionController.currentSceneMode)
		refreshStatusMenuState()
		scheduleLiveSamplingPrewarm()
		NativeHostTelemetry.lifecycleEvent(
			"native_host.finish_launching_end",
			detail: "statusItemPresent=\(statusItem != nil)"
		)
	}

	public func applicationDidFinishLaunching(_ notification: Notification) {
		finishLaunching()
	}

	public func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
		false
	}

	deinit {
		NotificationCenter.default.removeObserver(self)
	}

	@objc
	private func startCapture(_ sender: Any?) {
		sessionController.startCapture()
	}

	@objc
	private func cancelCapture(_ sender: Any?) {
		sessionController.cancelCapture()
	}

	@objc
	private func openSettings(_ sender: Any?) {
		settingsWindowController.present()
	}

	@objc
	private func openPermissions(_ sender: Any?) {
		permissionsWindowController.present()
	}

	@objc
	private func quit(_ sender: Any?) {
		NSApp.terminate(nil)
	}

	private func configureStatusItem() {
		let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
		item.isVisible = true
		NativeHostTelemetry.lifecycleEvent(
			"native_host.status_item_created",
			detail: "buttonPresent=\(item.button != nil)"
		)
		if let button = item.button {
			if let image = Self.statusItemImage() {
				button.image = image
				button.imagePosition = .imageOnly
				button.imageScaling = .scaleProportionallyDown
				button.title = ""
				NativeHostTelemetry.lifecycleEvent(
					"native_host.status_item_image_configured",
					detail: "width=\(Int(image.size.width)),height=\(Int(image.size.height))"
				)
			} else {
				button.title = "RS"
				NativeHostTelemetry.lifecycleEvent(
					"native_host.status_item_text_fallback_configured")
			}
		}

		let menu = NSMenu(title: "Rsnap Native Host")
		let captureItem = menu.addItem(
			withTitle: "Capture",
			action: #selector(startCapture(_:)),
			keyEquivalent: "n"
		)
		let cancelItem = menu.addItem(
			withTitle: "Cancel Capture",
			action: #selector(cancelCapture(_:)),
			keyEquivalent: "\u{1b}"
		)
		menu.addItem(.separator())
		menu.addItem(
			withTitle: "Settings…", action: #selector(openSettings(_:)), keyEquivalent: ",")
		menu.addItem(
			withTitle: "Permissions…", action: #selector(openPermissions(_:)), keyEquivalent: "")
		menu.addItem(.separator())
		menu.addItem(withTitle: "Quit", action: #selector(quit(_:)), keyEquivalent: "q")
		for menuItem in menu.items {
			menuItem.target = self
		}

		item.menu = menu
		statusItem = item
		captureMenuItem = captureItem
		cancelCaptureMenuItem = cancelItem
		NativeHostTelemetry.lifecycleEvent(
			"native_host.status_item_installed",
			detail: "visible=\(item.isVisible),hasMenu=\(item.menu != nil)"
		)
	}

	private func configureGlobalHotKeys() {
		globalHotKeys.onCaptureRequested = { [weak self] in
			self?.startCapture(nil)
		}
		globalHotKeys.onCancelRequested = { [weak self] in
			self?.cancelCapture(nil)
		}
		globalHotKeys.onToggleLoupeRequested = { [weak self] in
			self?.sessionController.toggleLoupe()
		}
	}

	fileprivate func refreshStatusMenuState() {
		let isCaptureActive = sessionController.isCaptureActive
		captureMenuItem?.isEnabled = !isCaptureActive
		cancelCaptureMenuItem?.isEnabled = isCaptureActive
	}

	private func refreshHotKeyBindings(for mode: SceneKind) {
		globalHotKeys.updateBindings(
			captureHotKey: settingsStore.settings.captureHotkey,
			sceneMode: mode
		)
	}

	private func scheduleLiveSamplingPrewarm() {
		liveSamplingPrewarmWorkItem?.cancel()
		let workItem = DispatchWorkItem { [weak self] in
			self?.liveSamplingPrewarmWorkItem = nil
			self?.prewarmLiveSamplingIfPossible()
		}
		liveSamplingPrewarmWorkItem = workItem
		DispatchQueue.main.asyncAfter(deadline: .now() + 0.35, execute: workItem)
	}

	private func prewarmLiveSamplingIfPossible() {
		guard !sessionController.isCaptureActive else {
			return
		}
		let point = NSEvent.mouseLocation
		let sample = sessionController.warmLiveSamplingIfPossible(
			at: point, source: "startup_prewarm", captureID: 0)
		NativeHostTelemetry.lifecycleDebug(
			"native_host.live_sampling_prewarm",
			detail: "sampleReady=\(sample?.rgbSample != nil)"
		)
	}

	@objc
	private func settingsDidChange() {
		refreshHotKeyBindings(for: sessionController.currentSceneMode)
	}

	private static func statusItemImage() -> NSImage? {
		let directResourceURL = Bundle.main.resourceURL?.appendingPathComponent("StatusBarIcon.png")
		let fallbackResourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("assets/tray-icon/generated/tray-icon-template.png")

		let imageURL = [directResourceURL, fallbackResourceURL]
			.compactMap { $0 }
			.first(where: { FileManager.default.fileExists(atPath: $0.path) })
		guard let imageURL, let image = NSImage(contentsOf: imageURL) else {
			return nil
		}
		image.isTemplate = true
		image.size = NSSize(width: 18, height: 18)
		return image
	}

	private static func applyApplicationIcon() {
		let directResourceURL = Bundle.main.resourceURL?.appendingPathComponent("AppIcon.icns")
		let fallbackResourceURL = URL(fileURLWithPath: #filePath)
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.deletingLastPathComponent()
			.appendingPathComponent("assets/app-icon/generated/app-icon.icns")

		let imageURL = [directResourceURL, fallbackResourceURL]
			.compactMap { $0 }
			.first(where: { FileManager.default.fileExists(atPath: $0.path) })
		guard let imageURL, let image = NSImage(contentsOf: imageURL) else {
			return
		}
		NSApp.applicationIconImage = image
	}
}

@MainActor
final class CaptureSessionController: NSObject {
	fileprivate struct FrozenCaptureJobSource: Sendable {
		let referenceWindowID: CGWindowID
		let desktopFrame: CGRect
	}

	private let settingsStore: NativeHostSettingsStore
	private let liveFrameStream = LiveFrameStreamBroker()
	private let frozenFrameAuthority = FrozenFrameAuthority()
	private let captureSuccessSound = CaptureSuccessSound.load()
	private var session: RsnapHostSession?
	private var overlayController: CaptureOverlayController?
	private var frozenFrameLatchToken: FrozenFrameLatchToken?
	private var frozenSnapshotGeneration: UInt64 = 0
	private var completedHostEffect: HostEffectKind?
	private var nextCaptureTelemetryID: UInt64 = 1
	private var activeCaptureTelemetryID: UInt64?
	var captureStateDidChange: (() -> Void)?
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
	private var chromeState = CaptureChromeState()
	var sceneDidChange: ((SceneSnapshot) -> Void)?

	init(settingsStore: NativeHostSettingsStore) {
		self.settingsStore = settingsStore
		super.init()
		NotificationCenter.default.addObserver(
			self,
			selector: #selector(settingsDidChange),
			name: NativeHostSettingsStore.didChangeNotification,
			object: settingsStore
		)
	}

	deinit {
		NotificationCenter.default.removeObserver(self)
	}

	var isCaptureActive: Bool {
		session != nil
	}

	var currentSceneMode: SceneKind {
		scene.mode
	}

	fileprivate var currentSettings: NativeHostSettings {
		settingsStore.settings
	}

	private var currentCaptureTelemetryID: UInt64 {
		activeCaptureTelemetryID ?? 0
	}

	var activeTelemetryCaptureID: UInt64 {
		currentCaptureTelemetryID
	}

	private func allocateCaptureTelemetryID() -> UInt64 {
		let captureID = nextCaptureTelemetryID
		nextCaptureTelemetryID &+= 1
		if nextCaptureTelemetryID == 0 {
			nextCaptureTelemetryID = 1
		}
		return captureID
	}

	@discardableResult
	func warmLiveSamplingIfPossible(
		at point: CGPoint,
		source: String = "capture",
		captureID: UInt64 = 0
	) -> LiveChromeSample? {
		let warmStartedAt = ProcessInfo.processInfo.systemUptime
		let screenCount = NSScreen.screens.count
		guard NativePermissions.status(for: .screenRecording) else {
			NativeHostTelemetry.liveSamplingWarmTiming(
				captureID: captureID,
				source: source,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: warmStartedAt),
				frozenAuthorityStartMilliseconds: 0,
				liveStreamStartMilliseconds: 0,
				seedSampleMilliseconds: 0,
				sampleReady: false,
				screenCount: screenCount
			)
			return nil
		}
		let screens = NSScreen.screens
		let frozenAuthorityStartedAt = ProcessInfo.processInfo.systemUptime
		frozenFrameAuthority.start(for: screens, captureID: captureID, source: source)
		let frozenAuthorityStartMilliseconds =
			NativeHostTelemetry.milliseconds(since: frozenAuthorityStartedAt)
		let liveStreamStartedAt = ProcessInfo.processInfo.systemUptime
		liveFrameStream.start(for: screens, prewarmPoint: point)
		let liveStreamStartMilliseconds =
			NativeHostTelemetry.milliseconds(since: liveStreamStartedAt)
		let seedStartedAt = ProcessInfo.processInfo.systemUptime
		let sample = liveFrameStream.seedSample(at: point, sidePixels: 1)
		let seedSampleMilliseconds = NativeHostTelemetry.milliseconds(since: seedStartedAt)
		NativeHostTelemetry.liveSamplingWarmTiming(
			captureID: captureID,
			source: source,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: warmStartedAt),
			frozenAuthorityStartMilliseconds: frozenAuthorityStartMilliseconds,
			liveStreamStartMilliseconds: liveStreamStartMilliseconds,
			seedSampleMilliseconds: seedSampleMilliseconds,
			sampleReady: sample?.rgbSample != nil,
			screenCount: screenCount
		)
		return sample
	}

	func startCapture() {
		if session != nil {
			NativeHostTelemetry.captureEvent(
				"capture.focus_existing",
				captureID: currentCaptureTelemetryID
			)
			overlayController?.focusWindow(at: NSEvent.mouseLocation)
			return
		}
		let captureID = allocateCaptureTelemetryID()
		activeCaptureTelemetryID = captureID
		let captureStartedAt = ProcessInfo.processInfo.systemUptime
		guard ensureCapturePermissions() else {
			NativeHostTelemetry.captureWarning(
				"capture.start_blocked",
				captureID: captureID,
				stage: "screen_recording_permission",
				error: "permission_denied"
			)
			NativeHostTelemetry.captureStartFailureTiming(
				captureID: captureID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
				failureStage: "screen_recording_permission"
			)
			activeCaptureTelemetryID = nil
			captureStateDidChange?()
			return
		}

		do {
			let startPoint = NSEvent.mouseLocation
			let warmStartedAt = ProcessInfo.processInfo.systemUptime
			let initialSample = warmLiveSamplingIfPossible(
				at: startPoint, source: "start_capture", captureID: captureID)
			let warmMilliseconds = NativeHostTelemetry.milliseconds(since: warmStartedAt)
			frozenFrameLatchToken = nil
			let desktopFrame = CaptureOverlayController.desktopFrame
			let windowSnapshotStartedAt = ProcessInfo.processInfo.systemUptime
			let initialWindowSnapshots = WindowSnapshotFeed.snapshots(desktopFrame: desktopFrame)
			let windowSnapshotMilliseconds =
				NativeHostTelemetry.milliseconds(since: windowSnapshotStartedAt)
			let initialHighlightedWindow = WindowSnapshotFeed.window(
				at: startPoint, in: initialWindowSnapshots)
			chromeState.rgbSample = initialSample?.rgbSample
			let sessionSetupStartedAt = ProcessInfo.processInfo.systemUptime
			let session = try RsnapHostSession(configuration: settingsStore.sessionConfiguration)
			self.session = session

			try session.enterLive()
			try session.send(
				event: .pointerMoved(
					point: startPoint,
					rgb: initialSample?.rgbSample,
					activeMonitor: activeMonitor(at: startPoint),
					highlightedWindow: initialHighlightedWindow
				)
			)
			let initialScene = try session.currentScene()
			self.scene = initialScene
			let sessionSetupMilliseconds =
				NativeHostTelemetry.milliseconds(since: sessionSetupStartedAt)

			let overlayController = CaptureOverlayController(
				controller: self,
				liveFrameStream: liveFrameStream
			)
			self.overlayController = overlayController
			let overlayShowStartedAt = ProcessInfo.processInfo.systemUptime
			overlayController.show(
				initialScene: initialScene,
				chrome: chromeState,
				settings: settingsStore.settings,
				focusPoint: startPoint,
				initialWindowSnapshots: initialWindowSnapshots
			)
			let overlayShowMilliseconds =
				NativeHostTelemetry.milliseconds(since: overlayShowStartedAt)
			(NSApp.delegate as? NativeHostApplicationController)?.window =
				overlayController.primaryWindow
			sceneDidChange?(initialScene)

			captureStateDidChange?()
			NativeHostTelemetry.captureStartTiming(
				captureID: captureID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
				warmMilliseconds: warmMilliseconds,
				windowSnapshotMilliseconds: windowSnapshotMilliseconds,
				sessionSetupMilliseconds: sessionSetupMilliseconds,
				overlayShowMilliseconds: overlayShowMilliseconds,
				initialSampleReady: initialSample?.rgbSample != nil,
				screenCount: NSScreen.screens.count,
				windowCount: initialWindowSnapshots.count
			)
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.start_failed",
				captureID: captureID,
				stage: "exception",
				error: String(describing: error)
			)
			NativeHostTelemetry.captureStartFailureTiming(
				captureID: captureID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
				failureStage: "exception"
			)
			tearDownCapture()
		}
	}

	private func ensureCapturePermissions() -> Bool {
		let granted = NativePermissions.status(for: .screenRecording)
		guard !granted else {
			return true
		}
		return NativePermissions.request(.screenRecording)
	}

	func backgroundPatch(in rect: CGRect) -> CGImage? {
		overlayController?.backgroundPatch(in: rect)
	}

	func streamPatch(in rect: CGRect) -> CGImage? {
		overlayController?.streamPatch(in: rect)
	}

	func updateLivePreviewDemand(
		point: CGPoint?,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) {
		overlayController?.updateLivePreviewDemand(
			point: point,
			settings: settings,
			includeLoupePatch: includeLoupePatch
		)
	}

	func liveChromeSnapshot(
		point: CGPoint?,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		overlayController?.liveChromeSnapshot(
			point: point,
			settings: settings,
			includeLoupePatch: includeLoupePatch
		)
	}

	func updateLiveChromeBackdrops(_ snapshot: LiveChromeBackdropSnapshot?) {
		overlayController?.updateLiveChromeBackdrops(snapshot)
	}

	func updateLiveChromeVisuals(_ snapshot: LiveChromeVisualSnapshot?) {
		overlayController?.updateLiveChromeVisuals(snapshot)
	}

	func previewHighlightedWindow(at point: CGPoint) -> WindowSnapshot? {
		overlayController?.hoverWindowPreview(at: point)
	}

	func cancelCapture() {
		do {
			try session?.send(event: .cancelRequested)
			try syncCore()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.cancel_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
			tearDownCapture()
		}
	}

	func pointerMoved(to point: CGPoint) {
		do {
			let liveInputs = currentLiveInputs(at: point)
			try session?.send(
				event: .pointerMoved(
					point: point,
					rgb: liveInputs.rgb,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try syncCore()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.pointer_update_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
		}
	}

	func beginPrimaryInteraction(at point: CGPoint) {
		guard scene.mode == .live else {
			pointerMoved(to: point)
			return
		}

		do {
			liveFrameStream.prime(at: point)
			frozenFrameLatchToken = frozenFrameAuthority.latchToken(containing: point)
			beginHostLocalFrozenSelectingIfPossible(at: point)
			let liveInputs = currentLiveInputs(at: point)
			try session?.send(
				event: .pointerMoved(
					point: point,
					rgb: liveInputs.rgb,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try session?.send(
				event: .primaryInteractionStarted(
					point: point,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try syncCore()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.primary_interaction_begin_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
		}
	}

	func continuePrimaryInteraction(to point: CGPoint) {
		guard scene.mode == .live else {
			pointerMoved(to: point)
			return
		}

		do {
			liveFrameStream.prime(at: point)
			if frozenFrameLatchToken == nil {
				frozenFrameLatchToken = frozenFrameAuthority.latchToken(containing: point)
			}
			beginHostLocalFrozenSelectingIfPossible(at: point)
			let liveInputs = currentLiveInputs(at: point)
			try session?.send(
				event: .pointerMoved(
					point: point,
					rgb: liveInputs.rgb,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try session?.send(
				event: .primaryInteractionUpdated(
					point: point,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try syncCore()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.primary_interaction_update_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
		}
	}

	func completePrimaryInteraction(at point: CGPoint) {
		guard scene.mode == .live else {
			pointerMoved(to: point)
			return
		}

		do {
			liveFrameStream.prime(at: point)
			if frozenFrameLatchToken == nil {
				frozenFrameLatchToken = frozenFrameAuthority.latchToken(containing: point)
			}
			beginHostLocalFrozenSelectingIfPossible(at: point)
			let liveInputs = currentLiveInputs(at: point)
			try session?.send(
				event: .pointerMoved(
					point: point,
					rgb: liveInputs.rgb,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try session?.send(
				event: .primaryInteractionCompleted(
					point: point,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try syncCore()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.primary_interaction_complete_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
		}
	}

	func copySelection() {
		let _ = chromeState.frozenOverlay.commitTextEdit()
		sendFrozenAction(.copyRequested, exitAfter: .copyCapture)
	}

	func saveSelection() {
		let _ = chromeState.frozenOverlay.commitTextEdit()
		sendFrozenAction(.saveRequested, exitAfter: .saveCapture)
	}

	func recognizeText() {
		let _ = chromeState.frozenOverlay.commitTextEdit()
		sendFrozenAction(.recognizeTextRequested)
	}

	func startScrollCapture() {
		let _ = chromeState.frozenOverlay.commitTextEdit()
		do {
			try sendHostStatusMessage("Scroll capture is not available in the native host yet.")
			try syncCore()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.scroll_unavailable_report_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
		}
	}

	func invokeToolbarItem(_ item: ToolbarItemKind) {
		if item != .text {
			let _ = chromeState.frozenOverlay.commitTextEdit()
		}
		switch item {
		case .copy:
			sendFrozenAction(.toolbarItemInvoked(item), exitAfter: .copyCapture)
		case .save:
			sendFrozenAction(.toolbarItemInvoked(item), exitAfter: .saveCapture)
		case .scroll:
			startScrollCapture()
		default:
			sendFrozenAction(.toolbarItemInvoked(item))
		}
	}

	func beginFrozenInteraction(at point: CGPoint) {
		guard scene.mode == .frozen else {
			pointerMoved(to: point)
			return
		}
		guard let selection = currentFrozenSelection() else {
			pointerMoved(to: point)
			return
		}
		let selectedTool = scene.toolbarItems.first(where: { $0.selected })?.kind ?? .pointer
		if selectedTool == .pointer,
			beginFrozenSelectionTransformIfPossible(at: point, selection: selection)
		{
			refreshOverlay()
			return
		}
		if chromeState.frozenOverlay.begin(tool: selectedTool, at: point, selection: selection) {
			refreshOverlay()
			return
		}
		pointerMoved(to: point)
	}

	func continueFrozenInteraction(to point: CGPoint) {
		guard scene.mode == .frozen, let selection = currentFrozenSelection() else {
			pointerMoved(to: point)
			return
		}
		if updateFrozenSelectionTransform(to: point) {
			refreshOverlay()
			return
		}
		if chromeState.frozenOverlay.update(to: point, selection: selection) {
			refreshOverlay()
			return
		}
		pointerMoved(to: point)
	}

	func completeFrozenInteraction(at point: CGPoint) {
		guard scene.mode == .frozen, let selection = currentFrozenSelection() else {
			pointerMoved(to: point)
			return
		}
		if completeFrozenSelectionTransform(at: point) {
			return
		}
		let _ = chromeState.frozenOverlay.update(to: point, selection: selection)
		if chromeState.frozenOverlay.finish(selection: selection) {
			refreshOverlay()
			return
		}
		pointerMoved(to: point)
	}

	private func currentFrozenSelection() -> CGRect? {
		chromeState.frozenSelectionSnapshot ?? scene.frozenSelection
	}

	private func beginFrozenSelectionTransformIfPossible(
		at point: CGPoint,
		selection: CGRect
	) -> Bool {
		guard chromeState.frozenSelectionEditable else {
			return false
		}
		guard
			let monitorFrame = screen(containing: CGPoint(x: selection.midX, y: selection.midY))?
				.frame
		else {
			return false
		}
		guard let kind = FrozenSelectionTransformKind.hitTest(at: point, selection: selection)
		else {
			return false
		}
		chromeState.frozenSelectionInteraction = FrozenSelectionInteractionState(
			kind: kind,
			initialPointer: point,
			initialSelection: selection,
			monitorFrame: monitorFrame
		)
		chromeState.frozenSelectionSnapshot = selection
		return true
	}

	private func updateFrozenSelectionTransform(to point: CGPoint) -> Bool {
		guard let interaction = chromeState.frozenSelectionInteraction else {
			return false
		}
		guard let nextSelection = transformedFrozenSelection(interaction: interaction, point: point)
		else {
			return false
		}
		guard chromeState.frozenSelectionSnapshot != nextSelection else {
			return true
		}
		chromeState.frozenSelectionSnapshot = nextSelection
		return true
	}

	private func completeFrozenSelectionTransform(at point: CGPoint) -> Bool {
		guard let interaction = chromeState.frozenSelectionInteraction else {
			return false
		}
		chromeState.frozenSelectionInteraction = nil
		let nextSelection =
			transformedFrozenSelection(interaction: interaction, point: point)
			?? interaction.initialSelection
		chromeState.frozenSelectionSnapshot = nextSelection
		guard nextSelection != scene.frozenSelection else {
			refreshOverlay()
			return true
		}

		frozenSnapshotGeneration &+= 1
		let generation = frozenSnapshotGeneration
		let captureID = currentCaptureTelemetryID
		chromeState.frozenBaseImage = nil
		chromeState.frozenMosaicImage = nil
		ensureFrozenBaseImageFromDisplayIfNeeded(for: nextSelection)
		refreshOverlay()
		DispatchQueue.main.async { [weak self] in
			guard let self else {
				return
			}
			guard generation == self.frozenSnapshotGeneration else {
				return
			}
			do {
				try self.session?.send(report: .freezeSnapshotCommitted(selection: nextSelection))
				try self.syncCore()
			} catch {
				NativeHostTelemetry.captureWarning(
					"capture.frozen_selection_transform_commit_failed",
					captureID: captureID,
					stage: "send_or_sync",
					error: String(describing: error)
				)
				self.chromeState.frozenSelectionSnapshot = self.scene.frozenSelection
				self.refreshOverlay()
			}
		}
		return true
	}

	private func transformedFrozenSelection(
		interaction: FrozenSelectionInteractionState,
		point: CGPoint
	) -> CGRect? {
		let minSize = CaptureChrome.frozenSelectionMinimumSize
		let selection = interaction.initialSelection
		let monitor = interaction.monitorFrame
		let deltaX = point.x - interaction.initialPointer.x
		let deltaY = point.y - interaction.initialPointer.y

		switch interaction.kind {
		case .move:
			return Self.clampedSelectionRect(
				width: selection.width,
				height: selection.height,
				x: selection.minX + deltaX,
				y: selection.minY + deltaY,
				monitorFrame: monitor
			)
		case .resizeLeft:
			let newMinX = (selection.minX + deltaX).clamped(
				to: monitor.minX...(selection.maxX - minSize))
			return CGRect(
				x: newMinX, y: selection.minY, width: selection.maxX - newMinX,
				height: selection.height)
		case .resizeRight:
			let newMaxX = (selection.maxX + deltaX).clamped(
				to: (selection.minX + minSize)...monitor.maxX)
			return CGRect(
				x: selection.minX, y: selection.minY, width: newMaxX - selection.minX,
				height: selection.height)
		case .resizeTop:
			let newMaxY = (selection.maxY + deltaY).clamped(
				to: (selection.minY + minSize)...monitor.maxY)
			return CGRect(
				x: selection.minX, y: selection.minY, width: selection.width,
				height: newMaxY - selection.minY)
		case .resizeBottom:
			let newMinY = (selection.minY + deltaY).clamped(
				to: monitor.minY...(selection.maxY - minSize))
			return CGRect(
				x: selection.minX, y: newMinY, width: selection.width,
				height: selection.maxY - newMinY)
		case .resizeTopLeft:
			let newMinX = (selection.minX + deltaX).clamped(
				to: monitor.minX...(selection.maxX - minSize))
			let newMaxY = (selection.maxY + deltaY).clamped(
				to: (selection.minY + minSize)...monitor.maxY)
			return CGRect(
				x: newMinX, y: selection.minY, width: selection.maxX - newMinX,
				height: newMaxY - selection.minY)
		case .resizeTopRight:
			let newMaxX = (selection.maxX + deltaX).clamped(
				to: (selection.minX + minSize)...monitor.maxX)
			let newMaxY = (selection.maxY + deltaY).clamped(
				to: (selection.minY + minSize)...monitor.maxY)
			return CGRect(
				x: selection.minX, y: selection.minY, width: newMaxX - selection.minX,
				height: newMaxY - selection.minY)
		case .resizeBottomLeft:
			let newMinX = (selection.minX + deltaX).clamped(
				to: monitor.minX...(selection.maxX - minSize))
			let newMinY = (selection.minY + deltaY).clamped(
				to: monitor.minY...(selection.maxY - minSize))
			return CGRect(
				x: newMinX, y: newMinY, width: selection.maxX - newMinX,
				height: selection.maxY - newMinY)
		case .resizeBottomRight:
			let newMaxX = (selection.maxX + deltaX).clamped(
				to: (selection.minX + minSize)...monitor.maxX)
			let newMinY = (selection.minY + deltaY).clamped(
				to: monitor.minY...(selection.maxY - minSize))
			return CGRect(
				x: selection.minX, y: newMinY, width: newMaxX - selection.minX,
				height: selection.maxY - newMinY)
		}
	}

	func performFrozenUndo() {
		guard chromeState.frozenOverlay.undo() else {
			return
		}
		refreshOverlay()
	}

	func performFrozenRedo() {
		guard chromeState.frozenOverlay.redo() else {
			return
		}
		refreshOverlay()
	}

	func performFrozenAutoCenter() {
		guard let selection = currentFrozenSelection() else {
			return
		}
		if chromeState.frozenOverlay.canUndo || chromeState.frozenOverlay.activeTextEdit != nil {
			return
		}
		if chromeState.frozenSelectionSnapshot != selection || chromeState.frozenBaseImage == nil {
			chromeState.frozenSelectionSnapshot = selection
			chromeState.frozenBaseImage = frozenBaseImageFromDisplay(for: selection)
			chromeState.frozenMosaicImage = nil
		}
		guard
			let baseImage = chromeState.frozenBaseImage,
			let contentBounds = Self.detectAutoCenterContentBounds(in: baseImage),
			let screen = screen(containing: CGPoint(x: selection.midX, y: selection.midY))
		else {
			return
		}

		let deltaX = Self.autoCenterShiftPoints(
			contentOriginPx: contentBounds.minX,
			contentSizePx: contentBounds.width,
			cropSizePx: CGFloat(baseImage.width),
			captureSizePoints: selection.width
		)
		let deltaY = Self.autoCenterShiftPoints(
			contentOriginPx: contentBounds.minY,
			contentSizePx: contentBounds.height,
			cropSizePx: CGFloat(baseImage.height),
			captureSizePoints: selection.height
		)
		let nextSelection = Self.clampedSelectionRect(
			width: selection.width,
			height: selection.height,
			x: selection.minX + deltaX,
			// Content bounds are in top-down CGImage coordinates; AppKit screen coordinates are bottom-up.
			y: selection.minY - deltaY,
			monitorFrame: screen.frame
		)
		guard nextSelection != selection else {
			return
		}

		do {
			frozenSnapshotGeneration &+= 1
			chromeState.frozenSelectionSnapshot = nextSelection
			chromeState.frozenBaseImage = frozenBaseImageFromDisplay(for: nextSelection)
			chromeState.frozenMosaicImage = nil
			try session?.send(report: .freezeSnapshotCommitted(selection: nextSelection))
			try syncCore()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.frozen_auto_center_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
		}
	}

	func handleFrozenTextKey(_ event: NSEvent) -> Bool {
		guard scene.mode == .frozen else {
			return false
		}

		switch event.keyCode {
		case 36, 76:
			if chromeState.frozenOverlay.commitTextEdit() {
				refreshOverlay()
				return true
			}
			return false
		case 51:
			if chromeState.frozenOverlay.backspaceText() {
				refreshOverlay()
				return true
			}
			return false
		default:
			break
		}

		let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
		guard !flags.contains(.command), !flags.contains(.control), !flags.contains(.option) else {
			return false
		}
		guard let characters = event.characters else {
			return false
		}
		if chromeState.frozenOverlay.appendText(characters) {
			refreshOverlay()
			return true
		}

		return false
	}

	func toggleLoupe() {
		do {
			try session?.send(event: .toggleLoupe)
			try syncCore()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.toggle_loupe_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
		}
	}

	private func sendFrozenAction(
		_ event: HostEvent, exitAfter expectedEffect: HostEffectKind? = nil
	) {
		do {
			completedHostEffect = nil
			try session?.send(event: event)
			try syncCore()
			if let expectedEffect, completedHostEffect == expectedEffect {
				tearDownCapture()
			}
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.frozen_action_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
		}
	}

	private func beginHostLocalFrozenSelectingIfPossible(at point: CGPoint) {
		guard scene.mode == .live else {
			return
		}
		guard !chromeState.hostLocalFrozenSelecting else {
			return
		}
		chromeState.beginHostLocalFrozenSelecting()
	}

	private func syncCore() throws {
		guard let session else {
			return
		}

		var pendingRequests = try session.drainRequests()
		while !pendingRequests.isEmpty {
			for request in pendingRequests {
				try handle(request: request)
			}
			pendingRequests = try session.drainRequests()
		}

		let previousMode = self.scene.mode
		let scene = try session.currentScene()
		self.scene = scene

		if scene.mode != .live {
			chromeState.resetLiveChrome()
		}
		if scene.mode != .frozen {
			if !chromeState.hostLocalFrozenSelecting {
				chromeState.resetFrozenChrome()
			}
		} else if previousMode != .frozen
			&& chromeState.frozenSelectionSnapshot == nil
			&& chromeState.frozenDisplayImage == nil
			&& chromeState.frozenBaseImage == nil
		{
			chromeState.resetFrozenChrome()
		}

		if scene.mode == .hidden {
			tearDownCapture()
			return
		}

		overlayController?.update(
			scene: scene,
			chrome: chromeState,
			settings: settingsStore.settings
		)
		sceneDidChange?(scene)
	}

	private func handle(request: HostRequest) throws {
		switch request {
		case .startLiveCapture:
			break
		case .stopLiveCapture:
			tearDownCapture()
		case .requestFreezeSnapshot(let selection):
			try commitFrozenSelection(
				selection,
				editable: scene.liveSelectionPreview == selection
			)
		case .copyCapture:
			try performCopy()
		case .saveCapture:
			try performSave()
		case .recognizeText:
			try performRecognizeText()
		case .requestScreenRecordingPermission:
			let granted = NativePermissions.request(.screenRecording)
			try session?.send(report: .permissionChanged(.screenRecording, granted: granted))
			if !granted {
				try sendHostStatusMessage("Screen recording permission is required.")
			}
		case .requestAccessibilityPermission:
			guard NativePermissions.requiredForCurrentNativeHost(.accessibility) else {
				try session?.send(
					report: .permissionChanged(
						.accessibility, granted: NativePermissions.status(for: .accessibility)))
				try sendHostStatusMessage(
					"Accessibility is not required by the current native host.")
				return
			}
			let granted = NativePermissions.request(.accessibility)
			try session?.send(report: .permissionChanged(.accessibility, granted: granted))
			if !granted {
				try sendHostStatusMessage("Accessibility permission is required.")
			}
		case .requestInputMonitoringPermission:
			guard NativePermissions.requiredForCurrentNativeHost(.inputMonitoring) else {
				try session?.send(
					report: .permissionChanged(
						.inputMonitoring, granted: NativePermissions.status(for: .inputMonitoring)))
				try sendHostStatusMessage(
					"Input Monitoring is not required by the current native host.")
				return
			}
			let granted = NativePermissions.request(.inputMonitoring)
			try session?.send(report: .permissionChanged(.inputMonitoring, granted: granted))
			if !granted {
				try sendHostStatusMessage("Input monitoring permission is required.")
			}
		}
	}

	private func commitFrozenSelection(_ selection: CGRect, editable: Bool) throws {
		guard let session else {
			return
		}
		let captureID = currentCaptureTelemetryID
		let commitStartedAt = ProcessInfo.processInfo.systemUptime
		frozenSnapshotGeneration &+= 1
		let selectionCenter = CGPoint(x: selection.midX, y: selection.midY)
		let hadLatchToken = frozenFrameLatchToken != nil
		let token =
			frozenFrameLatchToken ?? frozenFrameAuthority.latchToken(containing: selectionCenter)
		let snapshotStartedAt = ProcessInfo.processInfo.systemUptime
		let frozenFrame = frozenFrameAuthority.snapshot(
			containing: selectionCenter,
			after: token,
			maxWait: frozenFrameLatchWait()
		)
		let snapshotWaitMilliseconds =
			NativeHostTelemetry.milliseconds(since: snapshotStartedAt)
		guard let frozenFrame else {
			NativeHostTelemetry.captureWarning(
				"capture.freeze_commit_failed",
				captureID: captureID,
				stage: "authority_snapshot",
				error: "no_fresh_frame"
			)
			NativeHostTelemetry.freezeCommitFailureTiming(
				captureID: captureID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: commitStartedAt),
				snapshotWaitMilliseconds: snapshotWaitMilliseconds,
				hadLatchToken: hadLatchToken
			)
			try sendHostStatusMessage("Could not freeze the current frame.")
			return
		}
		frozenFrameLatchToken = nil
		chromeState.resetFrozenChrome()
		chromeState.frozenSelectionSnapshot = selection
		chromeState.frozenSelectionEditable = editable
		chromeState.frozenSelectionInteraction = nil
		chromeState.frozenDisplayFrame = frozenFrame.displayFrame
		chromeState.frozenDisplayImage = frozenFrame.image
		let baseImageStartedAt = ProcessInfo.processInfo.systemUptime
		chromeState.frozenBaseImage = frozenBaseImageFromDisplay(for: selection)
		let baseImageMilliseconds =
			NativeHostTelemetry.milliseconds(since: baseImageStartedAt)
		let hostOwnedFrozenScene = hostOwnedFrozenPresentationScene(for: selection)
		let presentStartedAt = ProcessInfo.processInfo.systemUptime
		overlayController?.presentFrozenFirstFrame(
			scene: hostOwnedFrozenScene,
			chrome: chromeState,
			settings: settingsStore.settings
		)
		let presentMilliseconds = NativeHostTelemetry.milliseconds(since: presentStartedAt)
		NativeHostTelemetry.freezeCommitTiming(
			captureID: captureID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: commitStartedAt),
			snapshotWaitMilliseconds: snapshotWaitMilliseconds,
			baseImageMilliseconds: baseImageMilliseconds,
			presentMilliseconds: presentMilliseconds,
			frameAgeMilliseconds: frozenFrame.ageMilliseconds(),
			displayID: frozenFrame.displayID,
			sequence: frozenFrame.sequence,
			hadLatchToken: hadLatchToken,
			baseReady: chromeState.frozenBaseImage != nil
		)
		try session.send(report: .freezeSnapshotCommitted(selection: selection))
	}

	private func frozenFrameLatchWait() -> TimeInterval {
		min(0.040, max(0.018, NativeHostDisplayRefresh.frameInterval * 2.5))
	}

	private func hostOwnedFrozenPresentationScene(for selection: CGRect) -> SceneSnapshot {
		SceneSnapshot(
			mode: .frozen,
			cursorIntent: .grab,
			pointer: scene.pointer,
			activeMonitor: nil,
			highlightedWindow: nil,
			liveSelectionPreview: nil,
			frozenSelection: selection,
			rgb: scene.rgb,
			loupeVisible: false,
			toolbarItems: hostOwnedFrozenToolbarItems(),
			statusMessage: nil
		)
	}

	private func hostOwnedFrozenToolbarItems() -> [ToolbarItem] {
		let allowTextInput =
			session?.configuration.allowTextInput
			?? settingsStore.sessionConfiguration.allowTextInput
		var items: [ToolbarItem] = [
			ToolbarItem(kind: .pointer, enabled: true, selected: true),
			ToolbarItem(kind: .pen, enabled: true, selected: false),
			ToolbarItem(kind: .arrow, enabled: true, selected: false),
			ToolbarItem(kind: .text, enabled: allowTextInput, selected: false),
			ToolbarItem(kind: .mosaic, enabled: true, selected: false),
			ToolbarItem(kind: .spotlight, enabled: true, selected: false),
			ToolbarItem(kind: .undo, enabled: false, selected: false),
			ToolbarItem(kind: .redo, enabled: false, selected: false),
			ToolbarItem(kind: .autoCenter, enabled: true, selected: false),
			ToolbarItem(kind: .scroll, enabled: false, selected: false),
		]
		if allowTextInput {
			items.append(ToolbarItem(kind: .ocr, enabled: true, selected: false))
		}
		items.append(ToolbarItem(kind: .copy, enabled: true, selected: false))
		items.append(ToolbarItem(kind: .save, enabled: true, selected: false))
		return items
	}

	private func performCopy() throws {
		guard let session else {
			return
		}
		let copyStartedAt = ProcessInfo.processInfo.systemUptime
		let captureImageStartedAt = ProcessInfo.processInfo.systemUptime
		guard let cgImage = try captureFrozenSelectionImage() else {
			NativeHostTelemetry.copyCaptureTiming(
				captureID: currentCaptureTelemetryID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: copyStartedAt),
				captureImageMilliseconds: NativeHostTelemetry.milliseconds(
					since: captureImageStartedAt),
				clearPasteboardMilliseconds: 0,
				makeImageMilliseconds: 0,
				writePasteboardMilliseconds: 0,
				success: false,
				failureStage: "capture_image",
				width: 0,
				height: 0
			)
			try sendHostStatusMessage("Could not capture the frozen selection.")
			return
		}
		let captureImageMilliseconds =
			NativeHostTelemetry.milliseconds(since: captureImageStartedAt)

		let pasteboard = NSPasteboard.general
		let clearPasteboardStartedAt = ProcessInfo.processInfo.systemUptime
		pasteboard.clearContents()
		let clearPasteboardMilliseconds =
			NativeHostTelemetry.milliseconds(since: clearPasteboardStartedAt)
		let makeImageStartedAt = ProcessInfo.processInfo.systemUptime
		let image = NSImage(cgImage: cgImage, size: .zero)
		let makeImageMilliseconds = NativeHostTelemetry.milliseconds(since: makeImageStartedAt)
		let writePasteboardStartedAt = ProcessInfo.processInfo.systemUptime
		let didWritePasteboard = pasteboard.writeObjects([image])
		let writePasteboardMilliseconds =
			NativeHostTelemetry.milliseconds(since: writePasteboardStartedAt)
		guard didWritePasteboard else {
			NativeHostTelemetry.copyCaptureTiming(
				captureID: currentCaptureTelemetryID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: copyStartedAt),
				captureImageMilliseconds: captureImageMilliseconds,
				clearPasteboardMilliseconds: clearPasteboardMilliseconds,
				makeImageMilliseconds: makeImageMilliseconds,
				writePasteboardMilliseconds: writePasteboardMilliseconds,
				success: false,
				failureStage: "pasteboard_write",
				width: cgImage.width,
				height: cgImage.height
			)
			try sendHostStatusMessage("Could not copy the captured image.")
			return
		}
		NativeHostTelemetry.copyCaptureTiming(
			captureID: currentCaptureTelemetryID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: copyStartedAt),
			captureImageMilliseconds: captureImageMilliseconds,
			clearPasteboardMilliseconds: clearPasteboardMilliseconds,
			makeImageMilliseconds: makeImageMilliseconds,
			writePasteboardMilliseconds: writePasteboardMilliseconds,
			success: true,
			failureStage: "none",
			width: cgImage.width,
			height: cgImage.height
		)

		CaptureSuccessSound.play(captureSuccessSound)

		try session.send(report: .hostEffectCompleted(.copyCapture))
		try session.send(report: .statusMessage("Copied capture to clipboard."))
		completedHostEffect = .copyCapture
	}

	private func performSave() throws {
		guard let session else {
			return
		}
		guard let cgImage = try captureFrozenSelectionImage() else {
			try sendHostStatusMessage("Could not capture the frozen selection.")
			return
		}
		let bitmap = NSBitmapImageRep(cgImage: cgImage)
		guard let pngData = bitmap.representation(using: .png, properties: [:]) else {
			try sendHostStatusMessage("Could not encode the captured image.")
			return
		}

		let outputURL = try nextOutputURL()
		try pngData.write(to: outputURL, options: .atomic)

		CaptureSuccessSound.play(captureSuccessSound)

		try session.send(report: .hostEffectCompleted(.saveCapture))
		try session.send(report: .statusMessage("Saved capture to \(outputURL.lastPathComponent)."))
		completedHostEffect = .saveCapture
	}

	private func performRecognizeText() throws {
		guard let session else {
			return
		}
		guard let cgImage = try captureFrozenSelectionImage() else {
			try sendHostStatusMessage("Could not capture the frozen selection.")
			return
		}

		let request = VNRecognizeTextRequest()
		request.recognitionLevel = .accurate
		let handler = VNImageRequestHandler(cgImage: cgImage)
		try handler.perform([request])

		let text = (request.results ?? [])
			.compactMap { $0.topCandidates(1).first?.string }
			.joined(separator: "\n")

		let pasteboard = NSPasteboard.general
		pasteboard.clearContents()
		pasteboard.setString(text, forType: .string)

		try session.send(report: .hostEffectCompleted(.recognizeText))
		let message =
			text.isEmpty
			? "No text was recognized."
			: "Recognized text copied to clipboard."
		try session.send(report: .statusMessage(message))
	}

	private func captureFrozenSelectionImage() throws -> CGImage? {
		let captureStartedAt = ProcessInfo.processInfo.systemUptime
		guard let selection = try session?.currentScene().frozenSelection else {
			NativeHostTelemetry.frozenSelectionImageTiming(
				captureID: currentCaptureTelemetryID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
				ensureMilliseconds: 0,
				refreshMilliseconds: 0,
				compositeMilliseconds: 0,
				source: "no_selection",
				success: false,
				width: 0,
				height: 0,
				hasOverlayEdits: false
			)
			return nil
		}

		let snapshotMatchedBefore = chromeState.frozenSelectionSnapshot == selection
		let hadBaseImageBefore = chromeState.frozenBaseImage != nil
		let hadFrozenDisplayImageBefore = chromeState.frozenDisplayImage != nil
		let hasOverlayEdits =
			chromeState.frozenOverlay.canUndo || chromeState.frozenOverlay.activeInteraction != nil
		let ensureStartedAt = ProcessInfo.processInfo.systemUptime
		ensureFrozenBaseImageFromDisplayIfNeeded(for: selection)
		let ensureMilliseconds = NativeHostTelemetry.milliseconds(since: ensureStartedAt)
		var refreshedFromBelowOverlay = false
		var refreshMilliseconds = 0.0
		if chromeState.frozenSelectionSnapshot != selection || chromeState.frozenBaseImage == nil {
			let refreshStartedAt = ProcessInfo.processInfo.systemUptime
			refreshFrozenCaptureSnapshot(for: selection)
			refreshMilliseconds = NativeHostTelemetry.milliseconds(since: refreshStartedAt)
			refreshedFromBelowOverlay = true
		}
		guard let baseImage = chromeState.frozenBaseImage else {
			NativeHostTelemetry.frozenSelectionImageTiming(
				captureID: currentCaptureTelemetryID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
				ensureMilliseconds: ensureMilliseconds,
				refreshMilliseconds: refreshMilliseconds,
				compositeMilliseconds: 0,
				source: "missing_base",
				success: false,
				width: 0,
				height: 0,
				hasOverlayEdits: hasOverlayEdits
			)
			return nil
		}

		let compositeStartedAt = ProcessInfo.processInfo.systemUptime
		let result = compositeFrozenOverlay(on: baseImage, selection: selection) ?? baseImage
		let compositeMilliseconds = NativeHostTelemetry.milliseconds(since: compositeStartedAt)
		let imageSource: String
		if refreshedFromBelowOverlay {
			imageSource = "below_overlay_refresh"
		} else if snapshotMatchedBefore, hadBaseImageBefore {
			imageSource = "cached_base"
		} else if hadFrozenDisplayImageBefore {
			imageSource = "frozen_display_crop"
		} else {
			imageSource = "unknown_base"
		}
		NativeHostTelemetry.frozenSelectionImageTiming(
			captureID: currentCaptureTelemetryID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
			ensureMilliseconds: ensureMilliseconds,
			refreshMilliseconds: refreshMilliseconds,
			compositeMilliseconds: compositeMilliseconds,
			source: imageSource,
			success: true,
			width: result.width,
			height: result.height,
			hasOverlayEdits: hasOverlayEdits
		)
		return result
	}

	private func refreshFrozenCaptureSnapshot(for selection: CGRect) {
		guard let overlayController else {
			chromeState.frozenSelectionSnapshot = selection
			chromeState.frozenBaseImage = nil
			chromeState.frozenMosaicImage = nil
			return
		}

		let baseImage = overlayController.captureImageBelowOverlay(
			in: selection,
			near: CGPoint(x: selection.midX, y: selection.midY)
		)
		chromeState.frozenSelectionSnapshot = selection
		chromeState.frozenBaseImage = baseImage
		chromeState.frozenMosaicImage = nil
	}

	private func ensureFrozenBaseImageFromDisplayIfNeeded(for selection: CGRect) {
		guard chromeState.frozenSelectionSnapshot == selection, chromeState.frozenBaseImage == nil
		else {
			return
		}
		chromeState.frozenBaseImage = frozenBaseImageFromDisplay(for: selection)
	}

	private func frozenBaseImageFromDisplay(for selection: CGRect) -> CGImage? {
		guard
			let displayFrame = chromeState.frozenDisplayFrame,
			let displayImage = chromeState.frozenDisplayImage
		else {
			return nil
		}
		return Self.cropFrozenDisplayImage(
			displayImage,
			displayFrame: displayFrame,
			selection: selection
		)
	}

	private static func cropFrozenDisplayImage(
		_ image: CGImage,
		displayFrame: CGRect,
		selection: CGRect
	) -> CGImage? {
		let cropRect = CGRect(
			x: ((selection.minX - displayFrame.minX) / max(displayFrame.width, 1))
				* CGFloat(image.width),
			y: ((displayFrame.maxY - selection.maxY) / max(displayFrame.height, 1))
				* CGFloat(image.height),
			width: (selection.width / max(displayFrame.width, 1)) * CGFloat(image.width),
			height: (selection.height / max(displayFrame.height, 1)) * CGFloat(image.height)
		).integral.intersection(CGRect(x: 0, y: 0, width: image.width, height: image.height))
		guard cropRect.width > 0, cropRect.height > 0 else {
			return nil
		}
		return image.cropping(to: cropRect)
	}

	private func screen(containing point: CGPoint) -> NSScreen? {
		NSScreen.screens.first(where: { $0.frame.contains(point) })
	}

	private func activeMonitor(at point: CGPoint) -> MonitorSnapshot? {
		guard let screen = screen(containing: point) else {
			return nil
		}
		let screenNumber =
			(screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
			.uint32Value
			?? 0
		return MonitorSnapshot(
			id: screenNumber,
			frame: screen.frame,
			scaleFactorX1000: UInt32((screen.backingScaleFactor * 1_000).rounded())
		)
	}

	private func highlightedWindow(at point: CGPoint) -> WindowSnapshot? {
		overlayController?.hoverWindow(at: point)
	}

	private func currentLiveInputs(at point: CGPoint) -> (
		rgb: RGBSample?, activeMonitor: MonitorSnapshot?, highlightedWindow: WindowSnapshot?
	) {
		let chromeSample = overlayController?.liveChromeSnapshot(
			point: point,
			settings: currentSettings,
			includeLoupePatch: scene.loupeVisible
		)
		let rgbSample = chromeSample?.rgbSample ?? chromeState.rgbSample ?? scene.rgb
		let highlightedWindow = highlightedWindow(at: point)
		chromeState.rgbSample = rgbSample
		chromeState.loupePatch = scene.loupeVisible ? chromeSample?.loupePatch : nil
		return (
			rgb: rgbSample,
			activeMonitor: activeMonitor(at: point),
			highlightedWindow: highlightedWindow
		)
	}

	private func sendHostStatusMessage(_ message: String) throws {
		guard let session else {
			return
		}
		try session.send(report: .statusMessage(message))
	}

	private func compositeFrozenOverlay(on image: CGImage, selection: CGRect) -> CGImage? {
		guard
			chromeState.frozenOverlay.canUndo || chromeState.frozenOverlay.activeInteraction != nil
		else {
			return image
		}

		let width = image.width
		let height = image.height
		guard
			let colorSpace = image.colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB),
			let context = CGContext(
				data: nil,
				width: width,
				height: height,
				bitsPerComponent: 8,
				bytesPerRow: 0,
				space: colorSpace,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			return image
		}

		let imageRect = CGRect(x: 0, y: 0, width: width, height: height)
		context.draw(image, in: imageRect)

		let scaleX = CGFloat(width) / max(selection.width, 1)
		let scaleY = CGFloat(height) / max(selection.height, 1)
		func mapPoint(_ point: CGPoint) -> CGPoint {
			CGPoint(
				x: (point.x - selection.minX) * scaleX,
				y: (point.y - selection.minY) * scaleY
			)
		}
		func mapRect(_ rect: CGRect) -> CGRect {
			CGRect(
				x: (rect.minX - selection.minX) * scaleX,
				y: (rect.minY - selection.minY) * scaleY,
				width: rect.width * scaleX,
				height: rect.height * scaleY
			)
		}

		let mosaicRects = chromeState.frozenOverlay.mosaicRects.map(mapRect)
		if chromeState.frozenMosaicImage == nil, !mosaicRects.isEmpty {
			chromeState.frozenMosaicImage = makeFrozenMosaicImage(from: image)
		}
		if let mosaicImage = chromeState.frozenMosaicImage, !mosaicRects.isEmpty {
			for rect in mosaicRects {
				if let mosaicPatch = mosaicImage.cropping(to: rect.integral.intersection(imageRect))
				{
					context.draw(mosaicPatch, in: rect)
				}
			}
		}

		let spotlightRects = chromeState.frozenOverlay.spotlightRects.map(mapRect)
		if !spotlightRects.isEmpty {
			context.saveGState()
			context.setFillColor(NSColor.black.withAlphaComponent(0.32).cgColor)
			context.fill(imageRect)
			for rect in spotlightRects {
				context.saveGState()
				context.clip(to: rect)
				context.draw(image, in: imageRect)
				context.restoreGState()
			}
			context.restoreGState()

			context.setStrokeColor(
				NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor)
			context.setLineWidth(2 * ((scaleX + scaleY) / 2))
			for rect in spotlightRects {
				context.stroke(rect.insetBy(dx: scaleX, dy: scaleY))
			}
		}

		context.setStrokeColor(
			NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor)
		context.setLineWidth(3 * ((scaleX + scaleY) / 2))
		context.setLineCap(.round)
		context.setLineJoin(.round)
		for stroke in chromeState.frozenOverlay.penStrokes {
			guard let first = stroke.first else {
				continue
			}
			context.beginPath()
			context.move(to: mapPoint(first))
			for point in stroke.dropFirst() {
				context.addLine(to: mapPoint(point))
			}
			context.strokePath()
		}
		for (start, end) in chromeState.frozenOverlay.arrowAnnotations {
			drawArrow(
				from: mapPoint(start),
				to: mapPoint(end),
				in: context
			)
		}
		for annotation in chromeState.frozenOverlay.textAnnotations {
			drawExportText(
				annotation.text,
				at: mapPoint(annotation.anchor),
				scale: (scaleX + scaleY) / 2,
				in: context
			)
		}

		return context.makeImage()
	}

	private func drawArrow(from start: CGPoint, to end: CGPoint, in context: CGContext) {
		context.beginPath()
		context.move(to: start)
		context.addLine(to: end)
		context.strokePath()

		let angle = atan2(end.y - start.y, end.x - start.x)
		let headLength: CGFloat = 10
		let headSpread: CGFloat = .pi / 7
		let left = CGPoint(
			x: end.x - cos(angle - headSpread) * headLength,
			y: end.y - sin(angle - headSpread) * headLength
		)
		let right = CGPoint(
			x: end.x - cos(angle + headSpread) * headLength,
			y: end.y - sin(angle + headSpread) * headLength
		)
		context.beginPath()
		context.move(to: end)
		context.addLine(to: left)
		context.move(to: end)
		context.addLine(to: right)
		context.strokePath()
	}

	private func drawExportText(
		_ text: String, at point: CGPoint, scale: CGFloat, in context: CGContext
	) {
		guard !text.isEmpty else {
			return
		}

		let font = NSFont.systemFont(ofSize: max(14, 16 * scale), weight: .medium)
		let attributes: [NSAttributedString.Key: Any] = [
			.font: font,
			.foregroundColor: NSColor.white,
		]
		let attributed = NSAttributedString(string: text, attributes: attributes)
		context.saveGState()
		context.setShadow(
			offset: CGSize(width: 0, height: 1 * scale), blur: 4 * scale,
			color: NSColor.black.withAlphaComponent(0.45).cgColor)
		let graphicsContext = NSGraphicsContext(cgContext: context, flipped: false)
		NSGraphicsContext.saveGraphicsState()
		NSGraphicsContext.current = graphicsContext
		attributed.draw(at: point)
		NSGraphicsContext.restoreGraphicsState()
		context.restoreGState()
	}

	private func refreshOverlay() {
		overlayController?.update(
			scene: scene,
			chrome: chromeState,
			settings: settingsStore.settings
		)
	}

	private func tearDownCapture() {
		let captureID = currentCaptureTelemetryID
		liveFrameStream.stop()
		frozenFrameLatchToken = nil
		frozenSnapshotGeneration &+= 1
		completedHostEffect = nil
		chromeState = CaptureChromeState()
		overlayController?.close()
		overlayController = nil
		if let appController = NSApp.delegate as? NativeHostApplicationController {
			appController.window = nil
		}
		session = nil
		scene = SceneSnapshot(
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
		sceneDidChange?(scene)
		captureStateDidChange?()
		if captureID != 0 {
			NativeHostTelemetry.captureEvent("capture.teardown", captureID: captureID)
		}
		activeCaptureTelemetryID = nil
	}

	@objc
	private func settingsDidChange() {
		overlayController?.update(
			scene: scene,
			chrome: chromeState,
			settings: settingsStore.settings
		)
	}

	private func nextOutputURL() throws -> URL {
		let settings = settingsStore.settings
		let fileManager = FileManager.default
		try fileManager.createDirectory(
			at: settings.outputDirectory, withIntermediateDirectories: true)
		switch settings.outputNaming {
		case .timestamp:
			let timestamp = ISO8601DateFormatter().string(from: .init()).replacingOccurrences(
				of: ":", with: "-")
			return settings.outputDirectory
				.appendingPathComponent("\(settings.outputFilenamePrefix)-\(timestamp)")
				.appendingPathExtension("png")
		case .sequence:
			let existingFiles = try fileManager.contentsOfDirectory(
				at: settings.outputDirectory,
				includingPropertiesForKeys: nil
			)
			let prefix = "\(settings.outputFilenamePrefix)-"
			let nextSequence =
				existingFiles.compactMap { url -> Int? in
					guard url.pathExtension.lowercased() == "png" else {
						return nil
					}
					let stem = url.deletingPathExtension().lastPathComponent
					guard stem.hasPrefix(prefix) else {
						return nil
					}
					return Int(stem.dropFirst(prefix.count))
				}.max().map { $0 + 1 } ?? 1
			return settings.outputDirectory
				.appendingPathComponent(
					"\(settings.outputFilenamePrefix)-\(String(format: "%04d", nextSequence))"
				)
				.appendingPathExtension("png")
		}
	}

	private static func clampedSelectionRect(
		width: CGFloat,
		height: CGFloat,
		x: CGFloat,
		y: CGFloat,
		monitorFrame: CGRect
	) -> CGRect {
		let maxX = max(monitorFrame.minX, monitorFrame.maxX - width)
		let maxY = max(monitorFrame.minY, monitorFrame.maxY - height)
		return CGRect(
			x: x.clamped(to: monitorFrame.minX...maxX),
			y: y.clamped(to: monitorFrame.minY...maxY),
			width: width,
			height: height
		)
	}

	private static func autoCenterShiftPoints(
		contentOriginPx: CGFloat,
		contentSizePx: CGFloat,
		cropSizePx: CGFloat,
		captureSizePoints: CGFloat
	) -> CGFloat {
		guard cropSizePx > 0, captureSizePoints > 0 else {
			return 0
		}
		let contentCenterPx = contentOriginPx + (contentSizePx * 0.5)
		let cropCenterPx = cropSizePx * 0.5
		let deltaPx = contentCenterPx - cropCenterPx
		return (deltaPx * captureSizePoints / cropSizePx).rounded()
	}

	private static func detectAutoCenterContentBounds(in image: CGImage) -> CGRect? {
		let bitmap = NSBitmapImageRep(cgImage: image)
		let width = bitmap.pixelsWide
		let height = bitmap.pixelsHigh
		guard width >= 2, height >= 2 else {
			return nil
		}
		guard
			bitmap.bitsPerSample == 8,
			!bitmap.isPlanar,
			bitmap.samplesPerPixel >= 3,
			!bitmap.bitmapFormat.contains(.floatingPointSamples),
			let bitmapData = bitmap.bitmapData
		else {
			return nil
		}

		let edgeStrip = max(1, min(24, Int((CGFloat(min(width, height)) * 0.08).rounded())))
		guard
			let topMean = regionRGBMean(
				bitmapData, bitmap: bitmap, x0: 0, x1: width, y0: 0, y1: edgeStrip),
			let bottomMean = regionRGBMean(
				bitmapData, bitmap: bitmap, x0: 0, x1: width, y0: height - edgeStrip, y1: height),
			let leftMean = regionRGBMean(
				bitmapData, bitmap: bitmap, x0: 0, x1: edgeStrip, y0: 0, y1: height),
			let rightMean = regionRGBMean(
				bitmapData, bitmap: bitmap, x0: width - edgeStrip, x1: width, y0: 0, y1: height)
		else {
			return nil
		}

		let threshold = max(
			24,
			min(
				96,
				Int(
					round(
						max(
							regionRGBMeanDistance(
								bitmapData, bitmap: bitmap, x0: 0, x1: width, y0: 0, y1: edgeStrip,
								mean: topMean),
							regionRGBMeanDistance(
								bitmapData, bitmap: bitmap, x0: 0, x1: width,
								y0: height - edgeStrip, y1: height, mean: bottomMean),
							regionRGBMeanDistance(
								bitmapData, bitmap: bitmap, x0: 0, x1: edgeStrip, y0: 0, y1: height,
								mean: leftMean),
							regionRGBMeanDistance(
								bitmapData, bitmap: bitmap, x0: width - edgeStrip, x1: width, y0: 0,
								y1: height, mean: rightMean)
						) * 3
					)
				)
			)
		)
		let minSalientPerRow = max(1, width / 64)
		let minSalientPerColumn = max(1, height / 64)
		var rowCounts = Array(repeating: 0, count: height)
		var columnCounts = Array(repeating: 0, count: width)

		for y in 0..<height {
			for x in 0..<width {
				let rgb = rgbComponents(bitmapData, bitmap: bitmap, x: x, y: y)
				let salientDistance = min(
					rgbDistanceToMean(rgb, mean: topMean),
					rgbDistanceToMean(rgb, mean: bottomMean),
					rgbDistanceToMean(rgb, mean: leftMean),
					rgbDistanceToMean(rgb, mean: rightMean)
				)
				guard salientDistance >= CGFloat(threshold) else {
					continue
				}
				rowCounts[y] += 1
				columnCounts[x] += 1
			}
		}

		guard
			let top = rowCounts.firstIndex(where: { $0 >= minSalientPerRow }),
			let bottom = rowCounts.lastIndex(where: { $0 >= minSalientPerRow }),
			let left = columnCounts.firstIndex(where: { $0 >= minSalientPerColumn }),
			let right = columnCounts.lastIndex(where: { $0 >= minSalientPerColumn }),
			left <= right,
			top <= bottom
		else {
			return nil
		}

		let bounds = CGRect(
			x: left,
			y: top,
			width: right - left + 1,
			height: bottom - top + 1
		)
		let fillsCropWidth = bounds.width * 100 >= CGFloat(width) * 92
		let fillsCropHeight = bounds.height * 100 >= CGFloat(height) * 92
		return (fillsCropWidth && fillsCropHeight) ? nil : bounds
	}

	private static func regionRGBMean(
		_ bitmapData: UnsafeMutablePointer<UInt8>,
		bitmap: NSBitmapImageRep,
		x0: Int,
		x1: Int,
		y0: Int,
		y1: Int
	) -> [CGFloat]? {
		guard x0 < x1, y0 < y1 else {
			return nil
		}
		var rTotal: CGFloat = 0
		var gTotal: CGFloat = 0
		var bTotal: CGFloat = 0
		var count: CGFloat = 0
		for y in y0..<y1 {
			for x in x0..<x1 {
				let rgb = rgbComponents(bitmapData, bitmap: bitmap, x: x, y: y)
				rTotal += rgb.r
				gTotal += rgb.g
				bTotal += rgb.b
				count += 1
			}
		}
		guard count > 0 else {
			return nil
		}
		return [rTotal / count, gTotal / count, bTotal / count]
	}

	private static func regionRGBMeanDistance(
		_ bitmapData: UnsafeMutablePointer<UInt8>,
		bitmap: NSBitmapImageRep,
		x0: Int,
		x1: Int,
		y0: Int,
		y1: Int,
		mean: [CGFloat]
	) -> CGFloat {
		guard x0 < x1, y0 < y1 else {
			return 0
		}
		var total: CGFloat = 0
		var count: CGFloat = 0
		for y in y0..<y1 {
			for x in x0..<x1 {
				total += rgbDistanceToMean(
					rgbComponents(bitmapData, bitmap: bitmap, x: x, y: y),
					mean: mean
				)
				count += 1
			}
		}
		return count == 0 ? 0 : total / count
	}

	private static func rgbComponents(
		_ bitmapData: UnsafeMutablePointer<UInt8>,
		bitmap: NSBitmapImageRep,
		x: Int,
		y: Int
	) -> (r: CGFloat, g: CGFloat, b: CGFloat) {
		let bytesPerPixel = max(3, bitmap.bitsPerPixel / 8)
		let offset = y * bitmap.bytesPerRow + x * bytesPerPixel
		if bitmap.bitmapFormat.contains(.alphaFirst), bytesPerPixel >= 4 {
			return (
				r: CGFloat(bitmapData[offset + 1]),
				g: CGFloat(bitmapData[offset + 2]),
				b: CGFloat(bitmapData[offset + 3])
			)
		}
		return (
			r: CGFloat(bitmapData[offset]),
			g: CGFloat(bitmapData[offset + 1]),
			b: CGFloat(bitmapData[offset + 2])
		)
	}

	private static func rgbDistanceToMean(
		_ rgb: (r: CGFloat, g: CGFloat, b: CGFloat),
		mean: [CGFloat]
	) -> CGFloat {
		abs(rgb.r - mean[0]).rounded()
			+ abs(rgb.g - mean[1]).rounded()
			+ abs(rgb.b - mean[2]).rounded()
	}
}

@MainActor
final class CaptureOverlayController {
	private weak var controller: CaptureSessionController?
	private var windows: [CaptureOverlayWindow] = []
	private var retiringWindows: [CaptureOverlayWindow] = []
	private var focusedWindowNumber: Int?
	private var collapsedForFrozen = false
	private let liveFrameStream: LiveFrameStreamBroker
	private lazy var windowSnapshotFeed = WindowSnapshotFeed()
	private lazy var chromeSampleFeed = ChromeSampleFeed(
		broker: liveFrameStream,
		backgroundSampler: Self.rgbSampleAtDisplayPoint
	)
	private let liveChromeWindows = LiveChromeVisualWindowController()
	private let liveChromeBackdrops = LiveChromeBackdropWindowController()

	init(controller: CaptureSessionController, liveFrameStream: LiveFrameStreamBroker) {
		self.controller = controller
		self.liveFrameStream = liveFrameStream
	}

	var primaryWindow: NSWindow? {
		windows.first(where: { $0.windowNumber == focusedWindowNumber }) ?? windows.first
	}

	fileprivate func show(
		initialScene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings,
		focusPoint: CGPoint,
		initialWindowSnapshots: [WindowSnapshot]
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
			if targetWindow == nil, screen.frame.contains(focusPoint) {
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
		liveFrameStream.start(for: NSScreen.screens, prewarmPoint: focusPoint)
		windowSnapshotFeed.start(
			desktopFrame: Self.desktopFrame, initialSnapshots: initialWindowSnapshots)
		chromeSampleFeed.start(
			targetFramesPerSecond: NativeHostDisplayRefresh.pointerFollowFramesPerSecond(
				for: focusedWindow?.screen
					?? NSScreen.screens.first(where: { $0.frame.contains(focusPoint) })
			))
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

	fileprivate func update(
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

	fileprivate func presentFrozenFirstFrame(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) {
		guard
			scene.mode == .frozen,
			let selection = scene.frozenSelection,
			let primaryWindow = windows.first(where: {
				$0.frame.contains(CGPoint(x: selection.midX, y: selection.midY))
			}) ?? windows.first
		else {
			update(scene: scene, chrome: chrome, settings: settings)
			return
		}

		primaryWindow.disableScreenUpdatesUntilFlush()
		liveChromeWindows.hideLiveWindows()
		primaryWindow.hostView.installFrozenFirstFrame(
			scene: scene,
			chrome: chrome,
			settings: settings
		)
		primaryWindow.displayIfNeeded()
		prepareFrozenPresentation(for: selection)
	}

	func focusWindow(at point: CGPoint) {
		guard let targetWindow = windows.first(where: { $0.frame.contains(point) }) ?? windows.first
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
		liveChromeWindows.hideLiveWindows()
		targetWindow.hostView.refreshLivePresentationNow()
		targetWindow.displayIfNeeded()
	}

	func close() {
		windowSnapshotFeed.stop()
		chromeSampleFeed.stop()
		liveChromeBackdrops.hideAll()
		liveChromeWindows.hideAll()
		guard !windows.isEmpty else {
			focusedWindowNumber = nil
			collapsedForFrozen = false
			return
		}

		let windowsToRetire = windows
		windows.removeAll()
		focusedWindowNumber = nil
		collapsedForFrozen = false
		(NSApp.delegate as? NativeHostApplicationController)?.window = nil

		for window in windowsToRetire {
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
		guard NSScreen.screens.contains(where: { $0.frame.contains(point) }) else {
			return nil
		}
		return windowSnapshotFeed.window(at: point)
	}

	func hoverWindowPreview(at point: CGPoint) -> WindowSnapshot? {
		guard NSScreen.screens.contains(where: { $0.frame.contains(point) }) else {
			return nil
		}
		return windowSnapshotFeed.window(at: point)
	}

	func backgroundPatch(in rect: CGRect) -> CGImage? {
		captureImageBelowOverlay(in: rect, near: CGPoint(x: rect.midX, y: rect.midY))
			?? liveFrameStream.patch(in: rect)
	}

	func streamPatch(in rect: CGRect) -> CGImage? {
		liveFrameStream.patch(in: rect)
	}

	func cachedRegionImage(in rect: CGRect) -> CGImage? {
		liveFrameStream.region(in: rect)
	}

	fileprivate func latestMonitorImage(
		near point: CGPoint
	) -> (frame: CGRect, image: CGImage)? {
		liveFrameStream.latestMonitorImage(containing: point)
	}

	fileprivate func currentMonitorImageBelowOverlay(
		near point: CGPoint
	) -> (frame: CGRect, image: CGImage)? {
		guard
			let source = frozenCaptureJobSource(near: point),
			let monitorFrame = NSScreen.screens.first(where: { $0.frame.contains(point) })?.frame,
			let image = Self.captureImageBelowOverlay(in: monitorFrame, source: source)
		else {
			return nil
		}
		return (frame: monitorFrame, image: image)
	}

	fileprivate func updateLivePreviewDemand(
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

	fileprivate func liveChromeSnapshot(
		point: CGPoint?,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		let latestSample = chromeSampleFeed.snapshot()
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

		let _ = point
		if wantsLoupePatch, let latestSample {
			return LiveChromeSample(rgbSample: latestSample.rgbSample, loupePatch: nil)
		}
		return latestSample
	}

	fileprivate func updateLiveChromeVisuals(
		_ snapshot: LiveChromeVisualSnapshot?
	) {
		liveChromeWindows.update(snapshot: snapshot, focusedWindowNumber: focusedWindowNumber)
	}

	fileprivate func updateLiveChromeBackdrops(
		_ snapshot: LiveChromeBackdropSnapshot?
	) {
		liveChromeBackdrops.update(snapshot: snapshot, focusedWindowNumber: focusedWindowNumber)
	}

	fileprivate func frozenCaptureJobSource(
		near point: CGPoint
	) -> CaptureSessionController.FrozenCaptureJobSource? {
		guard
			let referenceWindow = windows.first(where: { $0.frame.contains(point) })
				?? windows.first
		else {
			return nil
		}
		return CaptureSessionController.FrozenCaptureJobSource(
			referenceWindowID: CGWindowID(referenceWindow.windowNumber),
			desktopFrame: Self.desktopFrame
		)
	}

	fileprivate func liveColorSampleSource(near point: CGPoint) -> LiveColorSampleSource? {
		guard
			let referenceWindow = windows.first(where: { $0.frame.contains(point) })
				?? windows.first
		else {
			return nil
		}
		let screen =
			NSScreen.screens.first(where: { $0.frame.contains(point) })
			?? referenceWindow.screen
		return LiveColorSampleSource(
			referenceWindowID: CGWindowID(referenceWindow.windowNumber),
			desktopFrame: Self.desktopFrame,
			screenFrame: screen?.frame ?? referenceWindow.frame,
			scaleFactor: screen?.backingScaleFactor ?? 1
		)
	}

	func captureImageBelowOverlay(in rect: CGRect, near point: CGPoint) -> CGImage? {
		guard let source = frozenCaptureJobSource(near: point) else {
			return nil
		}
		return Self.captureImageBelowOverlay(in: rect, source: source)
	}

	nonisolated fileprivate static func captureImageBelowOverlay(
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
		guard !sampleRect.isNull, sampleRect.width > 0, sampleRect.height > 0 else {
			return nil
		}
		if let image = captureImageBelowOverlay(in: sampleRect, source: source),
			let sample = rgbSample(from: image)
		{
			return sample
		}
		return nil
	}

	nonisolated private static func captureImageBelowOverlay(
		in rect: CGRect,
		source: LiveColorSampleSource
	) -> CGImage? {
		let quartzRect = appKitRectToQuartz(rect, desktopFrame: source.desktopFrame)
		return legacyWindowListImage(
			quartzRect: quartzRect,
			windowListOption: .optionOnScreenBelowWindow,
			windowID: source.referenceWindowID,
			imageOption: [.boundsIgnoreFraming, .bestResolution]
		)
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

	private typealias LegacyWindowListCreateImage =
		@convention(c) (
			CGRect,
			UInt32,
			CGWindowID,
			UInt32
		) -> Unmanaged<CGImage>?

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

	fileprivate static var desktopFrame: CGRect {
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
		guard !collapsedForFrozen else {
			return
		}
		collapsedForFrozen = true
		guard collapsedForFrozen, !windows.isEmpty else {
			return
		}
		windowSnapshotFeed.stop()
		chromeSampleFeed.stop()
		liveChromeBackdrops.hideAll()
		liveChromeWindows.hideLiveWindows()

		guard windows.count > 1 else {
			return
		}

		let focusPoint = CGPoint(x: selection.midX, y: selection.midY)
		guard
			let primaryWindow = windows.first(where: { $0.frame.contains(focusPoint) })
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

@MainActor
final class CaptureOverlayWindow: NSPanel {
	let hostView: CaptureHostView

	override var canBecomeKey: Bool { true }
	override var canBecomeMain: Bool { false }

	fileprivate init(
		screen: NSScreen,
		controller: CaptureSessionController?,
		initialScene: SceneSnapshot,
		initialChrome: CaptureChromeState,
		initialSettings: NativeHostSettings
	) {
		hostView = CaptureHostView(frame: screen.frame)
		super.init(
			contentRect: screen.frame,
			styleMask: [.borderless, .nonactivatingPanel],
			backing: .buffered,
			defer: false
		)

		setFrame(screen.frame, display: false)
		hostView.controller = controller
		hostView.seedInitialState(
			scene: initialScene,
			chrome: initialChrome,
			settings: initialSettings
		)
		contentView = hostView
		acceptsMouseMovedEvents = true
		animationBehavior = .none
		backgroundColor = .clear
		collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
		hasShadow = false
		hidesOnDeactivate = false
		ignoresMouseEvents = false
		isFloatingPanel = true
		isMovable = false
		isOpaque = false
		level = .screenSaver
		sharingType = .none
		titleVisibility = .hidden
		titlebarAppearsTransparent = true
	}
}

@MainActor
final class CaptureHostView: NSView {
	private final class PassthroughVisualEffectView: NSVisualEffectView {
		override func hitTest(_ point: NSPoint) -> NSView? {
			nil
		}
	}

	private enum QueuedPointerEvent {
		case moved(CGPoint)
		case liveDragged(CGPoint)
	}

	private enum GlassSurfaceKind: Hashable {
		case hud
		case loupe
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

	private struct PositionSlotWidthKey: Hashable {
		let minX: Int
		let maxX: Int
		let minY: Int
		let maxY: Int
	}

	private struct HudLayoutMetrics {
		let font: NSFont
		let lineHeight: CGFloat
		let commaWidth: CGFloat
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
			keycapTextSize: keycapTextSize,
			keycapFrameSize: CGSize(
				width: keycapTextSize.width + 12, height: keycapTextSize.height + 4),
			hexSlotWidth: "#FFFFFF".size(using: font).width,
			placeholderXSlotWidth: "x=?".size(using: font).width,
			placeholderYSlotWidth: "y=?".size(using: font).width
		)
	}()

	private static var positionSlotWidthCache: [PositionSlotWidthKey: (x: CGFloat, y: CGFloat)] =
		[:]

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
	private let hudMaterialView = PassthroughVisualEffectView(frame: .zero)
	private let loupeMaterialView = PassthroughVisualEffectView(frame: .zero)
	private var hudLiquidGlassView: NSView?
	private var loupeLiquidGlassView: NSView?
	private var trackingAreaRef: NSTrackingArea?
	private var hoveredToolbarAction: ToolbarItemKind?
	private var lastCursorPresentation: CursorPresentation?
	private var queuedPointerEvent: QueuedPointerEvent?
	private var queuedPointerWorkItem: DispatchWorkItem?
	private var lastHoverPointerDispatchUptime: TimeInterval = 0
	private var lastDragPointerDispatchUptime: TimeInterval = 0
	private var livePointerPreviewGlobal: CGPoint?
	private var livePointerPreviewInputUptime: TimeInterval?
	private var livePointerPreviewInputSequence: UInt64 = 0
	private var lastLivePointerEventUptime: TimeInterval?
	private var liveHighlightedWindowPreview: WindowSnapshot?
	private var liveHoverChromeSuppressed = false
	private var pendingFrozenFirstDisplay = false
	private var frozenFirstDisplayCompletionQueued = false
	private var lastLivePreviewSnapshot: LivePreviewSnapshot?
	private var latestLiveChromeSample: LiveChromeSample?
	private var latestLiveRgbSample: RGBSample?
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

	override init(frame frameRect: NSRect) {
		super.init(frame: frameRect)
		wantsLayer = true
		layerContentsRedrawPolicy = .duringViewResize
		for materialView in [hudMaterialView, loupeMaterialView] {
			configureChromeMaterialView(materialView)
			addSubview(materialView, positioned: .below, relativeTo: nil)
		}
		liveRenderer.install { [weak self] in
			self?.currentRendererPreviewSnapshot()
		}
		liveRenderer.onTick = { [weak self] in
			guard let self else {
				return
			}
			guard self.scene.mode != .live else {
				return
			}
			self.controller?.updateLiveChromeVisuals(self.currentChromeVisualSnapshot())
		}
		liveRendererInstalled = true
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	fileprivate func update(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) {
		let previousScene = self.scene
		let previousChrome = self.chrome
		let previousSettings = self.settings
		let previousMode = self.scene.mode
		let transitioningToFrozen = previousMode == .live && scene.mode == .frozen
		if scene.mode != .frozen {
			frozenFirstDisplayCompletionQueued = false
		}
		self.scene = scene
		self.chrome = chrome
		self.settings = settings
		if scene.mode == .live {
			pendingFrozenFirstDisplay = false
			if previousMode != .live {
				liveHoverChromeSuppressed = false
				resetLiveChromeInputTelemetry()
				controller?.updateLiveChromeVisuals(nil)
			}
			if livePointerPreviewGlobal == nil {
				seedLivePointerPreview(scene.pointer, recordsInputLatency: false)
			}
			if liveHighlightedWindowPreview == nil {
				liveHighlightedWindowPreview = scene.highlightedWindow
			}
		} else {
			if scene.mode == .hidden {
				liveHoverChromeSuppressed = false
				pendingFrozenFirstDisplay = false
				lastLivePreviewSnapshot = nil
				latestLiveChromeSample = nil
				latestLiveRgbSample = nil
			}
			resetLivePointerPreview()
			liveHighlightedWindowPreview = nil
			if transitioningToFrozen {
				pendingFrozenFirstDisplay = true
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
				needsDisplay = true
				completeFrozenFirstDisplayHandoff()
				controller?.updateLiveChromeVisuals(currentChromeVisualSnapshot())
			} else {
				if previousMode == .live {
					stopLivePresentationNow()
				}
				needsDisplay = true
				controller?.updateLiveChromeVisuals(currentChromeVisualSnapshot())
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
		window?.displayIfNeeded()
		pendingFrozenFirstDisplay = false
		frozenFirstDisplayCompletionQueued = false
		lastLivePreviewSnapshot = nil
		if scene.mode != .live {
			liveRenderer.stop()
		}
	}

	fileprivate func seedInitialState(
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
		lastLivePreviewSnapshot = nil
		if scene.mode == .live {
			seedLivePointerPreview(scene.pointer, recordsInputLatency: false)
			liveHighlightedWindowPreview = scene.highlightedWindow
		} else {
			resetLivePointerPreview()
			liveHighlightedWindowPreview = nil
		}
		lastCursorPresentation = currentCursorPresentation()
		updateChromeMaterialViews()
		updateLiveRendererState()
	}

	fileprivate func refreshLivePresentationNow() {
		guard scene.mode == .live else {
			return
		}
		updateLivePreviewDemands()
		liveRenderer.renderNow()
	}

	fileprivate func installFrozenFirstFrame(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) {
		let retainedLivePreview = lastLivePreviewSnapshot ?? currentLivePreviewSnapshot()
		self.scene = scene
		self.chrome = chrome
		self.settings = settings
		liveHoverChromeSuppressed = false
		pendingFrozenFirstDisplay = retainedLivePreview != nil || scene.frozenSelection != nil
		frozenFirstDisplayCompletionQueued = false
		lastLivePreviewSnapshot = retainedLivePreview
		resetLivePointerPreview()
		liveHighlightedWindowPreview = nil
		refreshHoveredToolbarAction()
		syncVisibleCursor()
		updateChromeMaterialViews()
		needsDisplay = true
		controller?.updateLivePreviewDemand(
			point: nil, settings: settings, includeLoupePatch: false)
		controller?.updateLiveChromeVisuals(currentChromeVisualSnapshot())
		liveRenderer.renderNow()
	}

	fileprivate func finishFrozenFirstFrameInstall() {
		guard pendingFrozenFirstDisplay else {
			return
		}
		window?.disableScreenUpdatesUntilFlush()
		frozenFirstDisplayCompletionQueued = false
		pendingFrozenFirstDisplay = false
		lastLivePreviewSnapshot = nil
		needsDisplay = true
		displayIfNeeded()
		if scene.mode != .live {
			liveRenderer.stop()
		}
		controller?.updateLiveChromeVisuals(currentChromeVisualSnapshot())
	}

	override func layout() {
		super.layout()
		updateChromeMaterialViews()
		updateLiveRendererState()
		if scene.mode == .live {
			updateLivePreviewDemands()
			controller?.updateLiveChromeVisuals(currentChromeVisualSnapshot())
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

		let trackingAreaRef = NSTrackingArea(
			rect: bounds,
			options: [
				.activeAlways, .cursorUpdate, .inVisibleRect, .mouseMoved, .enabledDuringMouseDrag,
			],
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
		cursor(for: currentCursorPresentation()).set()
	}

	override func mouseMoved(with event: NSEvent) {
		if scene.mode == .frozen {
			refreshHoveredToolbarAction(for: event.locationInWindow)
		}
		let point = globalPoint(from: event)
		if scene.mode == .live {
			liveChromeMouseEventCount += 1
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
			updateLivePointerPreview(to: point, rendersImmediately: false)
			queuePointerEvent(.liveDragged(point))
		} else {
			controller?.continueFrozenInteraction(to: globalPoint(from: event))
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
			updateLivePointerPreview(to: point, rendersImmediately: true)
			controller?.beginPrimaryInteraction(at: point)
		case .frozen:
			if let action = toolbarAction(at: localPoint) {
				performToolbarAction(action)
				return
			}
			controller?.beginFrozenInteraction(at: point)
			syncVisibleCursor()
		}
	}

	override func mouseUp(with event: NSEvent) {
		let point = globalPoint(from: event)
		if scene.mode == .live {
			updateLivePointerPreview(to: point, rendersImmediately: true)
			controller?.completePrimaryInteraction(at: point)
		} else if scene.mode == .frozen {
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
					controller?.performFrozenRedo()
				} else {
					controller?.performFrozenUndo()
				}
				return
			case "s":
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
				controller?.copySelection()
			} else if scene.mode == .live {
				controller?.completePrimaryInteraction(at: scene.pointer ?? NSEvent.mouseLocation)
			}
		default:
			if scene.mode == .frozen, plainFrozenShortcutAvailable(event) {
				switch event.charactersIgnoringModifiers?.lowercased() {
				case "c":
					controller?.performFrozenAutoCenter()
					return
				case "s":
					controller?.startScrollCapture()
					return
				case "r":
					guard toolbarItem(.ocr)?.enabled == true else {
						return
					}
					controller?.recognizeText()
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
		return !flags.contains(.command)
			&& !flags.contains(.control)
			&& !flags.contains(.option)
	}

	override func draw(_ dirtyRect: NSRect) {
		super.draw(dirtyRect)
		guard let context = NSGraphicsContext.current?.cgContext else {
			return
		}

		switch scene.mode {
		case .hidden:
			break
		case .live:
			break
		case .frozen:
			if pendingFrozenFirstDisplay {
				scheduleFrozenFirstFrameInstallCompletionIfNeeded()
				return
			}
			drawFrozenDisplaySurface(in: context)
			if let selection = localFrozenSelectionRect() {
				drawSelectionScrim(
					for: selection, in: context, alpha: CaptureChrome.frozenScrimAlpha)
				drawDashedSelectionBorder(
					around: selection,
					in: context,
					lineWidth: CaptureChrome.frozenDashedBorderWidth
				)
				if chrome.frozenSelectionEditable {
					drawFrozenResizeHandles(for: selection, in: context)
				}
				drawFrozenOverlays(for: selection, in: context)
				drawSelectionSizeBadge(for: selection, in: context)
			}
			scheduleFrozenFirstFrameInstallCompletionIfNeeded()
		}

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
		context.draw(image, in: frame)
		context.restoreGState()
	}

	private func drawHud(in context: CGContext) {
		guard scene.mode == .live, let anchor = localPointer() else {
			return
		}
		let theme = chromeTheme()
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let metrics = Self.hudLayoutMetrics
		let font = metrics.font
		let positionDisplay = currentPositionDisplay()
		let rgbSample = latestLiveRgbSample
		let colorDisplay = currentLiveColorDisplay(for: rgbSample)
		let itemSpacing: CGFloat = 8
		let swatchSize = CGSize(width: 10, height: 10)
		let commaSeparator = ","
		let xGroupText = "x=\(positionDisplay.xValueText)"
		let yGroupText = "y=\(positionDisplay.yValueText)"
		let positionHeight = metrics.lineHeight
		let keycapVisible = settings.showAltHintKeycap
		let keycapFrame = keycapVisible ? metrics.keycapFrameSize : .zero
		let contentHeight = max(positionHeight, swatchSize.height, keycapFrame.height)
		let contentWidth =
			positionDisplay.xSlotWidth
			+ metrics.commaWidth
			+ positionDisplay.ySlotWidth
			+ swatchSize.width
			+ colorDisplay.hexSlotWidth
			+ keycapFrame.width
			+ itemSpacing * (keycapVisible ? 3 : 2)
		let hudFrame = CGRect(
			x: (anchor.x + 14).clamped(
				to: 6...(bounds.width - contentWidth - CaptureChrome.hudInnerMarginX * 2 - 6)),
			y: (anchor.y + 14).clamped(
				to: 6...(bounds.height - contentHeight - CaptureChrome.hudInnerMarginY * 2 - 6)),
			width: contentWidth + CaptureChrome.hudInnerMarginX * 2,
			height: contentHeight + CaptureChrome.hudInnerMarginY * 2
		)

		drawPill(
			in: hudFrame, context: context, theme: theme, strongShadow: true, surfaceKind: .hud)

		var cursorX = hudFrame.minX + CaptureChrome.hudInnerMarginX
		let baselineY = hudFrame.midY - positionHeight / 2
		drawText(
			xGroupText, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += positionDisplay.xSlotWidth
		drawText(
			commaSeparator, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText,
			font: font)
		cursorX += metrics.commaWidth
		drawText(
			yGroupText, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += positionDisplay.ySlotWidth + itemSpacing

		let swatchRect = CGRect(
			x: cursorX,
			y: hudFrame.midY - swatchSize.height / 2,
			width: swatchSize.width,
			height: swatchSize.height
		)
		let swatchColor =
			rgbSample.map {
				NSColor(
					calibratedRed: CGFloat($0.r) / 255,
					green: CGFloat($0.g) / 255,
					blue: CGFloat($0.b) / 255,
					alpha: 1
				)
			} ?? NSColor(calibratedWhite: 1, alpha: 0.12)
		context.setFillColor(swatchColor.cgColor)
		context.fill(swatchRect)
		context.setStrokeColor(palette.swatchStroke.cgColor)
		context.setLineWidth(1)
		context.stroke(swatchRect)
		cursorX += swatchSize.width + itemSpacing

		drawText(
			colorDisplay.hexText,
			at: CGPoint(x: cursorX, y: baselineY),
			color: palette.labelText,
			font: font
		)
		cursorX += colorDisplay.hexSlotWidth + itemSpacing

		if keycapVisible {
			let keycapRect = CGRect(
				x: cursorX,
				y: hudFrame.midY - keycapFrame.height / 2,
				width: keycapFrame.width,
				height: keycapFrame.height
			)
			context.setFillColor(palette.keycapFill.cgColor)
			let keycapPath = NSBezierPath(roundedRect: keycapRect, xRadius: 6, yRadius: 6)
			keycapPath.fill()
			context.setStrokeColor(palette.keycapStroke.cgColor)
			context.setLineWidth(1)
			keycapPath.stroke()
			drawText(
				"Tab",
				at: CGPoint(
					x: keycapRect.midX - metrics.keycapTextSize.width / 2,
					y: keycapRect.midY - metrics.keycapTextSize.height / 2
				),
				color: palette.keycapText,
				font: font
			)
		}
	}

	private func localFrozenDisplayFrame() -> CGRect? {
		localRect(from: chrome.frozenDisplayFrame)
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

	private func updateLivePointerPreview(
		to globalPoint: CGPoint,
		rendersImmediately: Bool
	) {
		guard scene.mode == .live else {
			return
		}
		recordLivePointerEventGap()
		let pointerChanged = setLivePointerPreview(to: globalPoint)
		if pointerChanged || rendersImmediately {
			updateLivePreviewDemands()
			liveRenderer.renderLiveChromeNow()
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

	fileprivate func finishLivePresentationTelemetry(reason: String) {
		emitLiveChromeInputSummary(reason: reason)
	}

	private func resetLiveChromeInputTelemetry() {
		liveChromeMouseEventCount = 0
		didEmitLiveChromeInputSummary = false
	}

	private func emitLiveChromeInputSummary(reason: String) {
		guard !didEmitLiveChromeInputSummary else {
			return
		}
		guard liveChromeMouseEventCount > 0 else {
			return
		}
		didEmitLiveChromeInputSummary = true
		NativeHostTelemetry.liveChromeInputSummary(
			captureID: controller?.activeTelemetryCaptureID ?? 0,
			reason: reason,
			mouseEvents: liveChromeMouseEventCount,
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

	private func currentCursorPresentation() -> CursorPresentation {
		if hoveredToolbarAction != nil {
			return .arrow
		}
		if scene.mode == .frozen {
			if let interaction = chrome.frozenSelectionInteraction {
				return cursorPresentation(for: cursorIntent(for: interaction.kind, active: true))
			}
			if let selectedModeTool = visibleToolbarItems().first(where: { $0.selected })?.kind,
				selectedModeTool == .pointer,
				!chrome.frozenSelectionEditable
			{
				return .arrow
			}
			if let selection = chrome.frozenSelectionSnapshot ?? scene.frozenSelection,
				let selectedModeTool = visibleToolbarItems().first(where: { $0.selected })?.kind
			{
				if [ToolbarItemKind.pen, .arrow, .mosaic, .spotlight].contains(selectedModeTool) {
					return .crosshair
				}
				if selectedModeTool == .pointer, chrome.frozenSelectionEditable,
					let pointer = currentGlobalMousePoint(),
					let intent = editableFrozenCursorIntent(at: pointer, selection: selection)
				{
					return cursorPresentation(for: intent)
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
		guard let kind = FrozenSelectionTransformKind.hitTest(at: point, selection: selection)
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

	private func drawLoupe(in context: CGContext) {
		guard
			scene.mode == .live,
			scene.loupeVisible,
			let hudFrame = currentHudFrame(),
			let patch = chrome.loupePatch,
			let frame = currentLoupeFrame(hudFrame: hudFrame)
		else {
			return
		}

		let theme = chromeTheme()
		drawPill(in: frame, context: context, theme: theme, strongShadow: true, surfaceKind: .loupe)

		let imageRect = frame.insetBy(dx: 10, dy: 10)
		context.saveGState()
		context.interpolationQuality = .none
		context.draw(patch, in: imageRect)
		context.restoreGState()

		let centerX = imageRect.minX + floor(CGFloat(patch.width) / 2) * CaptureChrome.loupeCellSize
		let centerY =
			imageRect.minY + floor(CGFloat(patch.height) / 2) * CaptureChrome.loupeCellSize
		let centerRect = CGRect(
			x: centerX,
			y: centerY,
			width: CaptureChrome.loupeCellSize,
			height: CaptureChrome.loupeCellSize
		).insetBy(dx: 1, dy: 1)
		context.setStrokeColor(NSColor.white.withAlphaComponent(0.9).cgColor)
		context.setLineWidth(2)
		context.stroke(centerRect)
	}

	private func drawSelectionScrim(for focusRect: CGRect, in context: CGContext, alpha: CGFloat) {
		let scrimColor = NSColor(calibratedWhite: 0, alpha: alpha)
		context.setFillColor(scrimColor.cgColor)

		for rect in [
			CGRect(
				x: bounds.minX, y: bounds.minY, width: bounds.width,
				height: max(0, focusRect.minY - bounds.minY)),
			CGRect(
				x: bounds.minX, y: focusRect.minY, width: max(0, focusRect.minX - bounds.minX),
				height: focusRect.height),
			CGRect(
				x: focusRect.maxX, y: focusRect.minY, width: max(0, bounds.maxX - focusRect.maxX),
				height: focusRect.height),
			CGRect(
				x: bounds.minX, y: focusRect.maxY, width: bounds.width,
				height: max(0, bounds.maxY - focusRect.maxY)),
		] where rect.width > 0 && rect.height > 0 {
			context.fill(rect)
		}
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
		let scale = window?.screen?.backingScaleFactor ?? 1
		let text = "\(Int(round(rect.width * scale)))x\(Int(round(rect.height * scale)))"
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
		if chrome.frozenMosaicImage == nil, !allRects.isEmpty,
			let baseImage = chrome.frozenBaseImage
		{
			chrome.frozenMosaicImage = makeFrozenMosaicImage(from: baseImage)
		}
		guard !allRects.isEmpty, let mosaicImage = chrome.frozenMosaicImage else {
			return
		}

		for rect in allRects {
			let imageRect = CGRect(
				x: ((rect.minX - selection.minX) / max(selection.width, 1))
					* CGFloat(mosaicImage.width),
				y: ((rect.minY - selection.minY) / max(selection.height, 1))
					* CGFloat(mosaicImage.height),
				width: (rect.width / max(selection.width, 1)) * CGFloat(mosaicImage.width),
				height: (rect.height / max(selection.height, 1)) * CGFloat(mosaicImage.height)
			).integral
			guard let patch = mosaicImage.cropping(to: imageRect) else {
				continue
			}
			context.draw(patch, in: rect)
			context.setStrokeColor(
				NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.84).cgColor)
			context.setLineWidth(1.5)
			context.stroke(rect.insetBy(dx: 1, dy: 1))
		}
	}

	private func drawFrozenSpotlights(for selection: CGRect, in context: CGContext) {
		let spotlightRects = chrome.frozenOverlay.spotlightRects.compactMap(localRect(from:))
		let previewRect = chrome.frozenOverlay.previewSpotlightRect.flatMap(localRect(from:))
		let allRects = spotlightRects + (previewRect.map { [$0] } ?? [])
		guard !allRects.isEmpty else {
			return
		}

		context.saveGState()
		context.setFillColor(NSColor.black.withAlphaComponent(0.32).cgColor)
		context.fill(selection)
		context.setBlendMode(.clear)
		for rect in allRects {
			context.fill(rect)
		}
		context.restoreGState()

		context.setStrokeColor(
			NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.92).cgColor)
		context.setLineWidth(2)
		for rect in allRects {
			context.stroke(rect.insetBy(dx: 1, dy: 1))
		}
	}

	private func drawFrozenPenStrokes(in context: CGContext) {
		let allStrokes =
			chrome.frozenOverlay.penStrokes
			+ (chrome.frozenOverlay.previewPenStroke.map { [$0] } ?? [])
		guard !allStrokes.isEmpty else {
			return
		}

		context.saveGState()
		context.setStrokeColor(
			NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor)
		context.setLineWidth(3)
		context.setLineCap(.round)
		context.setLineJoin(.round)
		for stroke in allStrokes {
			guard let first = stroke.first.flatMap(localPoint(from:)) else {
				continue
			}
			context.beginPath()
			context.move(to: first)
			for point in stroke.dropFirst() {
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
		guard !arrows.isEmpty else {
			return
		}

		context.saveGState()
		context.setStrokeColor(
			NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor)
		context.setLineWidth(3)
		context.setLineCap(.round)
		context.setLineJoin(.round)
		for (start, end) in arrows {
			guard let localStart = localPoint(from: start), let localEnd = localPoint(from: end)
			else {
				continue
			}
			drawArrow(from: localStart, to: localEnd, in: context)
		}
		context.restoreGState()
	}

	private func drawFrozenTextAnnotations(in context: CGContext) {
		for annotation in chrome.frozenOverlay.textAnnotations {
			guard let point = localPoint(from: annotation.anchor) else {
				continue
			}
			drawFrozenText(annotation.text, at: point, scale: 1, in: context)
		}
		if let activeTextEdit = chrome.frozenOverlay.activeTextEdit,
			let point = localPoint(from: activeTextEdit.anchor)
		{
			drawFrozenText(activeTextEdit.text + "│", at: point, scale: 1, in: context)
		}
	}

	private func drawFrozenText(
		_ text: String, at point: CGPoint, scale: CGFloat, in context: CGContext
	) {
		guard !text.isEmpty else {
			return
		}

		let font = NSFont.systemFont(ofSize: max(14, 16 * scale), weight: .medium)
		let attributes: [NSAttributedString.Key: Any] = [
			.font: font,
			.foregroundColor: NSColor.white,
		]
		let attributed = NSAttributedString(string: text, attributes: attributes)
		context.saveGState()
		context.setShadow(
			offset: CGSize(width: 0, height: 1), blur: 4,
			color: NSColor.black.withAlphaComponent(0.45).cgColor)
		let graphicsContext = NSGraphicsContext(cgContext: context, flipped: false)
		NSGraphicsContext.saveGraphicsState()
		NSGraphicsContext.current = graphicsContext
		attributed.draw(at: point)
		NSGraphicsContext.restoreGraphicsState()
		context.restoreGState()
	}

	private func drawArrow(from start: CGPoint, to end: CGPoint, in context: CGContext) {
		context.beginPath()
		context.move(to: start)
		context.addLine(to: end)
		context.strokePath()

		let angle = atan2(end.y - start.y, end.x - start.x)
		let headLength: CGFloat = 10
		let headSpread: CGFloat = .pi / 7
		let left = CGPoint(
			x: end.x - cos(angle - headSpread) * headLength,
			y: end.y - sin(angle - headSpread) * headLength
		)
		let right = CGPoint(
			x: end.x - cos(angle + headSpread) * headLength,
			y: end.y - sin(angle + headSpread) * headLength
		)
		context.beginPath()
		context.move(to: end)
		context.addLine(to: left)
		context.move(to: end)
		context.addLine(to: right)
		context.strokePath()
	}

	private func toolbarLayout(for selection: CGRect) -> FrozenToolbarLayout? {
		let items = visibleToolbarItems()
		guard !items.isEmpty else {
			return nil
		}

		let itemCount = CGFloat(items.count)
		let width =
			itemCount * CaptureChrome.toolbarButtonSize
			+ max(0, itemCount - 1) * CaptureChrome.toolbarItemSpacing
			+ CaptureChrome.hudInnerMarginX * 2
		let height = CaptureChrome.toolbarButtonSize + CaptureChrome.toolbarVerticalPadding * 2
		let desiredY = selection.maxY + CaptureChrome.toolbarGap
		let wantsTop = settings.toolbarPlacement == .top
		let placedAbove =
			wantsTop || desiredY + height > bounds.maxY - CaptureChrome.toolbarScreenMargin
		let y =
			placedAbove
			? max(
				bounds.minY + CaptureChrome.toolbarScreenMargin,
				selection.minY - CaptureChrome.toolbarGap - height)
			: min(bounds.maxY - CaptureChrome.toolbarScreenMargin - height, desiredY)
		let x = (selection.midX - width / 2).clamped(
			to: CaptureChrome
				.toolbarScreenMargin...(bounds.maxX - CaptureChrome.toolbarScreenMargin - width)
		)
		let frame = CGRect(x: x, y: y, width: width, height: height)
		var itemFrames: [FrozenToolbarItemLayout] = []
		var cursorX = frame.minX + CaptureChrome.hudInnerMarginX
		for item in items {
			let itemFrame = CGRect(
				x: cursorX,
				y: frame.midY - CaptureChrome.toolbarButtonSize / 2,
				width: CaptureChrome.toolbarButtonSize,
				height: CaptureChrome.toolbarButtonSize
			)
			itemFrames.append(
				FrozenToolbarItemLayout(
					kind: item.kind,
					frame: itemFrame,
					enabled: item.enabled,
					selected: item.selected
				)
			)
			cursorX += CaptureChrome.toolbarButtonSize + CaptureChrome.toolbarItemSpacing
		}

		return FrozenToolbarLayout(frame: frame, items: itemFrames)
	}

	private func visibleToolbarItems() -> [ToolbarItem] {
		scene.toolbarItems.map { item in
			var item = item
			switch item.kind {
			case .pen, .arrow, .mosaic, .spotlight, .text:
				item.enabled = true
			case .undo:
				item.enabled = chrome.frozenOverlay.canUndo
			case .redo:
				item.enabled = chrome.frozenOverlay.canRedo
			case .autoCenter:
				item.enabled =
					scene.frozenSelection != nil
					&& chrome.frozenOverlay.activeTextEdit == nil
					&& !chrome.frozenOverlay.canUndo
			case .scroll:
				item.enabled = false
			default:
				break
			}
			return item
		}
	}

	private func toolbarItem(_ kind: ToolbarItemKind) -> ToolbarItem? {
		scene.toolbarItems.first(where: { $0.kind == kind })
	}

	private func toolbarAction(at point: CGPoint) -> ToolbarItemKind? {
		guard scene.mode == .frozen, let selection = localFrozenSelectionRect(),
			let layout = toolbarLayout(for: selection)
		else {
			return nil
		}
		return layout.items.first(where: { $0.frame.contains(point) && $0.enabled })?.kind
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

	private func refreshHoveredToolbarAction(for localPoint: CGPoint? = nil) {
		let hoveredAction = localPoint.flatMap(toolbarAction(at:))
		if hoveredToolbarAction != hoveredAction {
			hoveredToolbarAction = hoveredAction
			syncVisibleCursor()
			updateChromeMaterialViews()
			needsDisplay = true
		}
	}

	private func syncVisibleCursor() {
		let cursorPresentation = currentCursorPresentation()
		guard cursorPresentation != lastCursorPresentation else {
			return
		}
		lastCursorPresentation = cursorPresentation
		window?.invalidateCursorRects(for: self)
		if scene.mode == .frozen {
			cursor(for: cursorPresentation).set()
		}
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
		let itemSpacing: CGFloat = 8
		let swatchSize = CGSize(width: 10, height: 10)
		let keycapVisible = settings.showAltHintKeycap
		let keycapFrame = keycapVisible ? metrics.keycapFrameSize : .zero
		let contentHeight = max(metrics.lineHeight, swatchSize.height, keycapFrame.height)
		let screenFrame = window?.screen?.frame ?? NSScreen.main?.frame ?? bounds
		let positionSlotWidths = Self.cachedPositionSlotWidths(for: screenFrame)
		let contentWidth =
			positionSlotWidths.x
			+ metrics.commaWidth
			+ positionSlotWidths.y
			+ swatchSize.width
			+ metrics.hexSlotWidth
			+ keycapFrame.width
			+ itemSpacing * (keycapVisible ? 3 : 2)
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
			patch: chrome.loupePatch,
			alignTrailing: currentHudPlacement()?.flippedHorizontally ?? false
		)
	}

	private func currentRendererPreviewSnapshot() -> LivePreviewSnapshot? {
		if scene.mode == .live {
			let snapshot =
				chrome.hostLocalFrozenSelecting
				? currentHostLocalFrozenSelectingPreviewSnapshot()
				: currentLivePreviewSnapshot()
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

		let dragSelectionLocal = localRect(from: scene.liveSelectionPreview)
		let hoverSelectionLocal =
			dragSelectionLocal == nil
			? localRect(from: liveHighlightedWindowPreview?.frame ?? scene.highlightedWindow?.frame)
			: nil
		let rgbSample = latestLiveRgbSample
		return LivePreviewSnapshot(
			bounds: bounds,
			theme: chromeTheme(),
			settings: settings,
			frozenPending: false,
			frozenDisplayFrame: localFrozenDisplayFrame(),
			frozenDisplayImage: chrome.frozenDisplayImage,
			pointerLocal: nil,
			dragSelectionLocal: dragSelectionLocal,
			hoverSelectionLocal: hoverSelectionLocal,
			selectionSizeText: dragSelectionLocal.map(selectionSizeText(for:)),
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
			colorDisplay: currentLiveColorDisplay(for: latestLiveRgbSample),
			rgbSample: latestLiveRgbSample,
			keycapVisible: false,
			inputUptime: nil,
			loupePatch: nil,
			glassPatches: [:]
		)
	}

	private func currentLivePreviewSnapshot() -> LivePreviewSnapshot? {
		guard scene.mode == .live else {
			return nil
		}

		let polledPoint = currentGlobalMousePoint() ?? NSEvent.mouseLocation
		if let currentPreview = livePointerPreviewGlobal {
			if hypot(currentPreview.x - polledPoint.x, currentPreview.y - polledPoint.y) >= 0.5 {
				applyPolledLivePointerPreview(polledPoint)
			}
		} else {
			applyPolledLivePointerPreview(polledPoint, recordsInputLatency: false)
		}

		refreshLiveHighlightedWindowPreview(at: livePointerPreviewGlobal ?? scene.pointer)
		updateLivePreviewDemands()

		let chromeSample = currentLiveChromeSample()
		let rgbSample = liveRgbSample(from: chromeSample)
		let loupePatch = scene.loupeVisible ? chromeSample?.loupePatch : nil
		let dragSelectionLocal = localRect(from: scene.liveSelectionPreview)
		let hoverSelectionLocal =
			dragSelectionLocal == nil
			? localRect(from: liveHighlightedWindowPreview?.frame ?? scene.highlightedWindow?.frame)
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
			inputUptime: livePointerPreviewInputUptime,
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
			return
		}
		liveHighlightedWindowPreview =
			controller?.previewHighlightedWindow(at: globalPoint) ?? liveHighlightedWindowPreview
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

	private func currentLiveChromeLayerFrames() -> (hud: CGRect?, loupe: CGRect?) {
		let hudPlacement = liveHoverChromeSuppressed ? nil : currentHudPlacement()
		let hudFrame = hudPlacement?.frame
		let loupeFrame =
			!liveHoverChromeSuppressed && scene.loupeVisible
			? hudPlacement.flatMap {
				currentLoupeFrame(
					hudFrame: $0.frame,
					patch: chrome.loupePatch,
					alignTrailing: $0.flippedHorizontally
				)
			}
			: nil
		return (hudFrame, loupeFrame)
	}

	private func currentChromeVisualSnapshot() -> LiveChromeVisualSnapshot? {
		switch scene.mode {
		case .live:
			return nil
		case .frozen:
			return currentFrozenChromeVisualSnapshot()
		case .hidden:
			return nil
		}
	}

	private func currentFrozenChromeVisualSnapshot() -> LiveChromeVisualSnapshot? {
		let toolbarSnapshot: FrozenToolbarVisualSnapshot? = {
			guard
				let sourceWindowNumber = window?.windowNumber,
				let selection = localFrozenSelectionRect(),
				let layout = toolbarLayout(for: selection),
				let frame = globalRect(from: layout.frame)
			else {
				return nil
			}

			let items = layout.items.map { item in
				FrozenToolbarVisualItemSnapshot(
					kind: item.kind,
					frame: CGRect(
						x: item.frame.minX - layout.frame.minX,
						y: item.frame.minY - layout.frame.minY,
						width: item.frame.width,
						height: item.frame.height
					),
					enabled: item.enabled,
					selected: item.selected
				)
			}

			return FrozenToolbarVisualSnapshot(
				sourceWindowNumber: sourceWindowNumber,
				frame: frame,
				theme: chromeTheme(),
				settings: settings,
				items: items
			)
		}()

		return LiveChromeVisualSnapshot(
			sourceWindowNumber: window?.windowNumber,
			hud: nil,
			loupe: nil,
			toolbar: toolbarSnapshot
		)
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
		if !alignTrailing, x + size.width > bounds.width - 6 {
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
		lastLivePreviewSnapshot = nil
		hideLiveLiquidGlassViews()
		guard scene.mode != .live else {
			return
		}
		liveRenderer.stop()
		controller?.updateLiveChromeVisuals(currentChromeVisualSnapshot())
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

	private func currentLiveChromeSample() -> LiveChromeSample? {
		let sample = controller?.liveChromeSnapshot(
			point: livePointerPreviewGlobal ?? scene.pointer,
			settings: settings,
			includeLoupePatch: scene.loupeVisible && !liveHoverChromeSuppressed
		)
		if let sample {
			latestLiveChromeSample = sample
			if let rgbSample = sample.rgbSample {
				latestLiveRgbSample = rgbSample
			}
			return sample
		}
		return latestLiveChromeSample
	}

	private func liveRgbSample(from sample: LiveChromeSample?) -> RGBSample? {
		if let rgbSample = sample?.rgbSample {
			latestLiveRgbSample = rgbSample
			return rgbSample
		}
		return latestLiveRgbSample
	}

	private func selectionSizeText(for rect: CGRect) -> String {
		let scale = window?.screen?.backingScaleFactor ?? 1
		return "\(Int(round(rect.width * scale)))x\(Int(round(rect.height * scale)))"
	}

	private static func cachedPositionSlotWidths(for screenFrame: CGRect) -> (
		x: CGFloat, y: CGFloat
	) {
		let minX = Int(screenFrame.minX.rounded())
		let maxX = Int(screenFrame.maxX.rounded()) - 1
		let minY = Int(screenFrame.minY.rounded())
		let maxY = Int(screenFrame.maxY.rounded()) - 1
		let key = PositionSlotWidthKey(minX: minX, maxX: maxX, minY: minY, maxY: maxY)
		if let cached = positionSlotWidthCache[key] {
			return cached
		}
		let font = hudLayoutMetrics.font
		let slotWidths = (
			x: ["x=\(minX)", "x=\(maxX)"].map { $0.size(using: font).width }.max() ?? 0,
			y: ["y=\(minY)", "y=\(maxY)"].map { $0.size(using: font).width }.max() ?? 0
		)
		positionSlotWidthCache[key] = slotWidths
		return slotWidths
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
		let screenFrame = window?.screen?.frame ?? .zero
		let slotWidths = Self.cachedPositionSlotWidths(for: screenFrame)
		return LivePositionDisplay(
			xValueText: String(Int(pointer.x.rounded())),
			yValueText: String(Int(pointer.y.rounded())),
			xSlotWidth: slotWidths.x,
			ySlotWidth: slotWidths.y
		)
	}

	private func currentLiveColorDisplay(for sample: RGBSample?) -> LiveColorDisplay {
		let placeholderHex = ""
		let hexText =
			sample.map { String(format: "#%02X%02X%02X", $0.r, $0.g, $0.b) } ?? placeholderHex
		return LiveColorDisplay(
			hexText: hexText,
			hexSlotWidth: Self.hudLayoutMetrics.hexSlotWidth
		)
	}

	private func drawPill(
		in frame: CGRect,
		context: CGContext,
		theme: CaptureChromeTheme,
		strongShadow: Bool,
		surfaceKind: GlassSurfaceKind
	) {
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let pillPath = NSBezierPath(
			roundedRect: frame,
			xRadius: CaptureChrome.hudCornerRadius,
			yRadius: CaptureChrome.hudCornerRadius
		)
		let glassImage =
			settings.usesClassicHudGlass ? glassPatch(for: surfaceKind, frame: frame) : nil
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
		let usesLiquidGlass = scene.mode == .live && settings.usesLiquidHudGlass
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
			return frozenDisplayPatch(in: globalFrame)
		case .hidden:
			return nil
		}
	}

	private func frozenDisplayPatch(in globalFrame: CGRect) -> CGImage? {
		guard
			let displayFrame = chrome.frozenDisplayFrame,
			let image = chrome.frozenDisplayImage
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
			case .hud, .loupe:
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
			case .hud, .loupe:
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

	private func configureChromeMaterialView(_ view: NSVisualEffectView) {
		view.blendingMode = .behindWindow
		view.state = .active
		view.isHidden = true
		view.wantsLayer = true
		view.layer?.cornerRadius = CaptureChrome.hudCornerRadius
		view.layer?.masksToBounds = true
	}

	private func configureChromeLiquidGlassView(_ view: NSView) {
		view.isHidden = true
		view.wantsLayer = true
		view.layer?.cornerRadius = CaptureChrome.hudCornerRadius
		view.layer?.masksToBounds = true
		view.layer?.zPosition = 50
	}

	private func updateChromeMaterialViews() {
		for materialView in [hudMaterialView, loupeMaterialView] {
			materialView.isHidden = true
		}
		if scene.mode != .live || !settings.usesLiquidHudGlass || chrome.hostLocalFrozenSelecting {
			hideLiveLiquidGlassViews()
		}
		updateLiveChromeBackdrops()
	}

	private func updateLiveLiquidGlassViews(hudFrame: CGRect?, loupeFrame: CGRect?) {
		guard scene.mode == .live, settings.usesLiquidHudGlass, !chrome.hostLocalFrozenSelecting
		else {
			hideLiveLiquidGlassViews()
			return
		}
		updateLiveLiquidGlassView(&hudLiquidGlassView, frame: hudFrame)
		updateLiveLiquidGlassView(&loupeLiquidGlassView, frame: loupeFrame)
	}

	private func updateLiveLiquidGlassView(_ view: inout NSView?, frame: CGRect?) {
		guard let frame else {
			view?.isHidden = true
			return
		}
		if view == nil {
			guard let createdView = LiveChromeLiquidGlassBridge.makeGlassView() else {
				return
			}
			configureChromeLiquidGlassView(createdView)
			addSubview(createdView, positioned: .below, relativeTo: nil)
			view = createdView
		}
		guard let activeView = view else {
			return
		}
		LiveChromeLiquidGlassBridge.update(activeView, settings: settings)
		if activeView.frame != frame {
			activeView.frame = frame
		}
		activeView.isHidden = false
	}

	private func hideLiveLiquidGlassViews() {
		hudLiquidGlassView?.removeFromSuperview()
		loupeLiquidGlassView?.removeFromSuperview()
		hudLiquidGlassView = nil
		loupeLiquidGlassView = nil
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

private struct FrozenTextAnnotation {
	var anchor: CGPoint
	var text: String
}

private struct FrozenTextEditState {
	var anchor: CGPoint
	var text: String
}

private enum FrozenSelectionTransformKind {
	case move
	case resizeLeft
	case resizeRight
	case resizeTop
	case resizeBottom
	case resizeTopLeft
	case resizeTopRight
	case resizeBottomLeft
	case resizeBottomRight
}

extension FrozenSelectionTransformKind {
	fileprivate static func hitTest(
		at point: CGPoint,
		selection: CGRect
	) -> FrozenSelectionTransformKind? {
		let handleRadius = CGFloat(12)
		let edgeTolerance = CGFloat(4)
		let left = selection.minX
		let right = selection.maxX
		let top = selection.maxY
		let bottom = selection.minY

		if abs(point.x - left) <= handleRadius, abs(point.y - top) <= handleRadius {
			return .resizeTopLeft
		}
		if abs(point.x - right) <= handleRadius, abs(point.y - top) <= handleRadius {
			return .resizeTopRight
		}
		if abs(point.x - left) <= handleRadius, abs(point.y - bottom) <= handleRadius {
			return .resizeBottomLeft
		}
		if abs(point.x - right) <= handleRadius, abs(point.y - bottom) <= handleRadius {
			return .resizeBottomRight
		}
		if point.y >= bottom, point.y <= top, abs(point.x - left) <= edgeTolerance {
			return .resizeLeft
		}
		if point.y >= bottom, point.y <= top, abs(point.x - right) <= edgeTolerance {
			return .resizeRight
		}
		if point.x >= left, point.x <= right, abs(point.y - top) <= edgeTolerance {
			return .resizeTop
		}
		if point.x >= left, point.x <= right, abs(point.y - bottom) <= edgeTolerance {
			return .resizeBottom
		}
		if selection.contains(point) {
			return .move
		}
		return nil
	}
}

private struct FrozenSelectionInteractionState {
	let kind: FrozenSelectionTransformKind
	let initialPointer: CGPoint
	let initialSelection: CGRect
	let monitorFrame: CGRect
}

private struct CaptureChromeState {
	var loupePatch: CGImage?
	var rgbSample: RGBSample?
	var hostLocalFrozenSelecting = false
	var frozenSelectionSnapshot: CGRect?
	var frozenSelectionEditable = false
	var frozenSelectionInteraction: FrozenSelectionInteractionState?
	var frozenDisplayFrame: CGRect?
	var frozenDisplayImage: CGImage?
	var frozenBaseImage: CGImage?
	var frozenMosaicImage: CGImage?
	var frozenOverlay = FrozenOverlayState()

	mutating func resetLiveChrome() {
		loupePatch = nil
	}

	mutating func beginHostLocalFrozenSelecting() {
		hostLocalFrozenSelecting = true
		frozenSelectionSnapshot = nil
		frozenSelectionEditable = false
		frozenSelectionInteraction = nil
		frozenDisplayFrame = nil
		frozenDisplayImage = nil
		frozenBaseImage = nil
		frozenMosaicImage = nil
		frozenOverlay.reset()
	}

	mutating func endHostLocalFrozenSelecting() {
		hostLocalFrozenSelecting = false
	}

	mutating func resetFrozenChrome() {
		hostLocalFrozenSelecting = false
		frozenSelectionSnapshot = nil
		frozenSelectionEditable = false
		frozenSelectionInteraction = nil
		frozenDisplayFrame = nil
		frozenDisplayImage = nil
		frozenBaseImage = nil
		frozenMosaicImage = nil
		frozenOverlay.reset()
	}
}

private struct FrozenOverlayState {
	enum Edit {
		case pen([CGPoint])
		case arrow(start: CGPoint, end: CGPoint)
		case mosaic(CGRect)
		case spotlight(CGRect)
		case text(FrozenTextAnnotation)
	}

	enum ActiveInteraction {
		case pen(points: [CGPoint])
		case arrow(start: CGPoint, current: CGPoint)
		case mosaic(anchor: CGPoint, current: CGPoint)
		case spotlight(anchor: CGPoint, current: CGPoint)
	}

	var edits: [Edit] = []
	var redoEdits: [Edit] = []
	var activeInteraction: ActiveInteraction?
	var activeTextEdit: FrozenTextEditState?

	var canUndo: Bool { !edits.isEmpty }
	var canRedo: Bool { !redoEdits.isEmpty }

	mutating func reset() {
		edits.removeAll()
		redoEdits.removeAll()
		activeInteraction = nil
		activeTextEdit = nil
	}

	mutating func begin(tool: ToolbarItemKind, at point: CGPoint, selection: CGRect) -> Bool {
		guard selection.contains(point) else {
			return false
		}

		switch tool {
		case .pen:
			activeInteraction = .pen(points: [point])
		case .arrow:
			activeInteraction = .arrow(start: point, current: point)
		case .mosaic:
			activeInteraction = .mosaic(anchor: point, current: point)
		case .spotlight:
			activeInteraction = .spotlight(anchor: point, current: point)
		case .text:
			let _ = commitTextEdit()
			activeTextEdit = FrozenTextEditState(anchor: selection.clamp(point), text: "")
			return true
		default:
			return false
		}

		return true
	}

	mutating func update(to point: CGPoint, selection: CGRect) -> Bool {
		guard let activeInteraction else {
			return false
		}

		switch activeInteraction {
		case .pen(var points):
			let clamped = selection.clamp(point)
			if let lastPoint = points.last,
				hypot(lastPoint.x - clamped.x, lastPoint.y - clamped.y) < 1.5
			{
				return false
			}
			points.append(clamped)
			self.activeInteraction = .pen(points: points)
		case .arrow(let start, _):
			self.activeInteraction = .arrow(start: start, current: selection.clamp(point))
		case .mosaic(let anchor, _):
			self.activeInteraction = .mosaic(anchor: anchor, current: selection.clamp(point))
		case .spotlight(let anchor, _):
			self.activeInteraction = .spotlight(anchor: anchor, current: selection.clamp(point))
		}

		return true
	}

	mutating func finish(selection: CGRect) -> Bool {
		guard let activeInteraction else {
			return false
		}
		defer { self.activeInteraction = nil }

		switch activeInteraction {
		case .pen(let points):
			guard points.count >= 2 else {
				return false
			}
			edits.append(.pen(points))
		case .arrow(let start, let current):
			guard hypot(start.x - current.x, start.y - current.y) >= 6 else {
				return false
			}
			edits.append(.arrow(start: start, end: current))
		case .mosaic(let anchor, let current):
			let rect = selection.normalizedRect(anchor: anchor, current: current)
			guard rect.width >= 6, rect.height >= 6 else {
				return false
			}
			edits.append(.mosaic(rect))
		case .spotlight(let anchor, let current):
			let rect = selection.normalizedRect(anchor: anchor, current: current)
			guard rect.width >= 6, rect.height >= 6 else {
				return false
			}
			edits.append(.spotlight(rect))
		}

		redoEdits.removeAll()
		return true
	}

	mutating func appendText(_ text: String) -> Bool {
		guard var activeTextEdit else {
			return false
		}
		let sanitized = text.replacingOccurrences(of: "\r", with: "")
		guard !sanitized.isEmpty else {
			return false
		}
		activeTextEdit.text.append(sanitized)
		self.activeTextEdit = activeTextEdit
		return true
	}

	mutating func backspaceText() -> Bool {
		guard var activeTextEdit else {
			return false
		}
		guard activeTextEdit.text.popLast() != nil else {
			return false
		}
		self.activeTextEdit = activeTextEdit
		return true
	}

	mutating func commitTextEdit() -> Bool {
		guard let activeTextEdit else {
			return false
		}
		self.activeTextEdit = nil
		let trimmed = activeTextEdit.text.trimmingCharacters(in: .whitespacesAndNewlines)
		guard !trimmed.isEmpty else {
			return false
		}
		edits.append(
			.text(FrozenTextAnnotation(anchor: activeTextEdit.anchor, text: activeTextEdit.text)))
		redoEdits.removeAll()
		return true
	}

	mutating func cancelTextEdit() {
		activeTextEdit = nil
	}

	mutating func undo() -> Bool {
		activeTextEdit = nil
		guard let edit = edits.popLast() else {
			return false
		}
		redoEdits.append(edit)
		return true
	}

	mutating func redo() -> Bool {
		activeTextEdit = nil
		guard let edit = redoEdits.popLast() else {
			return false
		}
		edits.append(edit)
		return true
	}

	var penStrokes: [[CGPoint]] {
		edits.compactMap {
			if case .pen(let points) = $0 {
				return points
			}
			return nil
		}
	}

	var arrowAnnotations: [(CGPoint, CGPoint)] {
		edits.compactMap {
			if case .arrow(let start, let end) = $0 {
				return (start, end)
			}
			return nil
		}
	}

	var mosaicRects: [CGRect] {
		edits.compactMap {
			if case .mosaic(let rect) = $0 {
				return rect
			}
			return nil
		}
	}

	var spotlightRects: [CGRect] {
		edits.compactMap {
			if case .spotlight(let rect) = $0 {
				return rect
			}
			return nil
		}
	}

	var textAnnotations: [FrozenTextAnnotation] {
		edits.compactMap {
			if case .text(let annotation) = $0 {
				return annotation
			}
			return nil
		}
	}

	var previewPenStroke: [CGPoint]? {
		if case .pen(let points)? = activeInteraction {
			return points
		}
		return nil
	}

	var previewArrow: (CGPoint, CGPoint)? {
		if case .arrow(let start, let current)? = activeInteraction {
			return (start, current)
		}
		return nil
	}

	var previewMosaicRect: CGRect? {
		if case .mosaic(let anchor, let current)? = activeInteraction {
			return CGRect(
				x: min(anchor.x, current.x),
				y: min(anchor.y, current.y),
				width: abs(current.x - anchor.x),
				height: abs(current.y - anchor.y)
			)
		}
		return nil
	}

	var previewSpotlightRect: CGRect? {
		if case .spotlight(let anchor, let current)? = activeInteraction {
			return CGRect(
				x: min(anchor.x, current.x),
				y: min(anchor.y, current.y),
				width: abs(current.x - anchor.x),
				height: abs(current.y - anchor.y)
			)
		}
		return nil
	}
}

private struct FrozenToolbarItemLayout: Equatable {
	let kind: ToolbarItemKind
	let frame: CGRect
	let enabled: Bool
	let selected: Bool
}

private struct FrozenToolbarLayout {
	let frame: CGRect
	let items: [FrozenToolbarItemLayout]
}

enum CaptureChromeTheme: Equatable {
	case dark
	case light
}

struct CaptureChromePalette {
	let foregrounds: CaptureChromeForegroundPalette
	let bodyFill: NSColor
	let outerStroke: NSColor
	let shadow: NSColor
	let swatchStroke: NSColor
	let keycapFill: NSColor
	let keycapStroke: NSColor
	let toolbarHoverBackground: NSColor
	let toolbarSelectedBackground: NSColor

	var labelText: NSColor { foregrounds.primary }
	var secondaryText: NSColor { foregrounds.secondary }
	var keycapText: NSColor { foregrounds.secondary }
	var toolbarIcon: NSColor { foregrounds.control }
	var toolbarHoverIcon: NSColor { foregrounds.controlHover }
	var toolbarSelectedIcon: NSColor { foregrounds.controlSelected }
	var toolbarDisabledIcon: NSColor { foregrounds.controlDisabled }
}

struct CaptureChromeForegroundPalette {
	let primary: NSColor
	let secondary: NSColor
	let control: NSColor
	let controlHover: NSColor
	let controlSelected: NSColor
	let controlDisabled: NSColor
}

enum CaptureChrome {
	private static let liquidGlassBodyOpacity: CGFloat = 0.5

	static let hudInnerMarginX: CGFloat = 12
	static let hudInnerMarginY: CGFloat = 8
	static let hudCornerRadius: CGFloat = 18
	static let hudLoupeGap: CGFloat = 8
	static let loupeCellSize: CGFloat = 10
	static let liveScrimAlpha: CGFloat = 176.0 / 255.0
	static let frozenScrimAlpha: CGFloat = 176.0 / 255.0
	static let liveDashedBorderWidth: CGFloat = 1.55
	static let frozenDashedBorderWidth: CGFloat = 1.55
	static let dashedBorderDashLength: CGFloat = 8.0
	static let dashedBorderGapLength: CGFloat = 4.2
	static let selectionCornerRadius: CGFloat = 18
	static let liveSelectionCornerRadius: CGFloat = 20
	static let frozenSelectionMinimumSize: CGFloat = 1
	static let resizeHandleHitSize: CGFloat = 24
	static let resizeHandleStrokeWidth: CGFloat = 1.3
	static let resizeHandleLegLength: CGFloat = 8
	static let resizeHandleOffset: CGFloat = 2.5
	static let toolbarButtonSize: CGFloat = 24
	static let toolbarItemSpacing: CGFloat = 4
	static let toolbarVerticalPadding: CGFloat = 5
	static let toolbarGap: CGFloat = 10
	static let toolbarScreenMargin: CGFloat = 10
	static let selectionSizeBadgeGap: CGFloat = 8
	static let selectionSizeBadgeInset: CGFloat = 8
	static let selectionSizeBadgeToolbarAvoidance: CGFloat = 4

	static func dashedBorderOutset(strokeWidth: CGFloat, pixelsPerPoint: CGFloat) -> CGFloat {
		let feathering = 1.0 / max(pixelsPerPoint, .leastNonzeroMagnitude)
		return (strokeWidth + feathering) * 0.5
	}

	static func selectionSizeBadgeFrame(
		for selection: CGRect,
		textSize: CGSize,
		in bounds: CGRect,
		avoiding toolbarFrame: CGRect? = nil
	) -> CGRect {
		let size = CGSize(width: ceil(textSize.width), height: ceil(textSize.height))
		let bottomOutside = CGRect(
			x: selection.maxX - size.width,
			y: selection.minY - selectionSizeBadgeGap - size.height,
			width: size.width,
			height: size.height
		)
		if fitsSelectionSizeBadge(bottomOutside, in: bounds),
			!selectionSizeBadge(bottomOutside, conflictsWith: toolbarFrame)
		{
			return bottomOutside
		}

		if selectionSizeBadge(bottomOutside, conflictsWith: toolbarFrame) {
			let topOutside = CGRect(
				x: selection.maxX - size.width,
				y: selection.maxY + selectionSizeBadgeGap,
				width: size.width,
				height: size.height
			)
			if fitsSelectionSizeBadge(topOutside, in: bounds),
				!selectionSizeBadge(topOutside, conflictsWith: toolbarFrame)
			{
				return topOutside
			}
		}

		return selectionSizeBadgeInsideBottomRight(
			selection: selection,
			size: size,
			bounds: bounds
		)
	}

	private static func fitsSelectionSizeBadge(_ frame: CGRect, in bounds: CGRect) -> Bool {
		frame.minX >= bounds.minX + selectionSizeBadgeGap
			&& frame.maxX <= bounds.maxX - selectionSizeBadgeGap
			&& frame.minY >= bounds.minY + selectionSizeBadgeGap
			&& frame.maxY <= bounds.maxY - selectionSizeBadgeGap
	}

	private static func selectionSizeBadge(
		_ frame: CGRect,
		conflictsWith toolbarFrame: CGRect?
	) -> Bool {
		guard let toolbarFrame else {
			return false
		}
		return frame.insetBy(
			dx: -selectionSizeBadgeToolbarAvoidance,
			dy: -selectionSizeBadgeToolbarAvoidance
		).intersects(toolbarFrame)
	}

	private static func selectionSizeBadgeInsideBottomRight(
		selection: CGRect,
		size: CGSize,
		bounds: CGRect
	) -> CGRect {
		let minX = bounds.minX + selectionSizeBadgeGap
		let maxX = max(minX, bounds.maxX - selectionSizeBadgeGap - size.width)
		let minY = bounds.minY + selectionSizeBadgeGap
		let maxY = max(minY, bounds.maxY - selectionSizeBadgeGap - size.height)
		let targetX = min(
			selection.maxX - selectionSizeBadgeInset - size.width,
			bounds.maxX - selectionSizeBadgeGap - size.width)
		let targetY = max(
			selection.minY + selectionSizeBadgeInset, bounds.minY + selectionSizeBadgeGap)
		return CGRect(
			x: targetX.clamped(to: minX...maxX),
			y: targetY.clamped(to: minY...maxY),
			width: size.width,
			height: size.height
		)
	}

	static func dashedBorderPath(
		for rect: CGRect,
		dashLength: CGFloat = dashedBorderDashLength,
		gapLength: CGFloat = dashedBorderGapLength,
		cornerKeepout: CGFloat = 0
	) -> CGPath {
		let path = CGMutablePath()
		for (start, end) in dashedBorderSegments(
			for: rect,
			dashLength: dashLength,
			gapLength: gapLength,
			cornerKeepout: cornerKeepout
		) {
			path.move(to: start)
			path.addLine(to: end)
		}
		return path
	}

	private static func dashedBorderSegments(
		for rect: CGRect,
		dashLength: CGFloat,
		gapLength: CGFloat,
		cornerKeepout: CGFloat
	) -> [(CGPoint, CGPoint)] {
		if cornerKeepout > 0 {
			let horizontalRanges = dashedBorderEdgeRanges(
				edgeLength: rect.width,
				cornerKeepout: cornerKeepout,
				dashLength: dashLength,
				gapLength: gapLength
			)
			let verticalRanges = dashedBorderEdgeRanges(
				edgeLength: rect.height,
				cornerKeepout: cornerKeepout,
				dashLength: dashLength,
				gapLength: gapLength
			)
			var segments: [(CGPoint, CGPoint)] = []
			for (start, end) in horizontalRanges {
				segments.append(
					(
						CGPoint(x: rect.minX + start, y: rect.minY),
						CGPoint(x: rect.minX + end, y: rect.minY)
					))
			}
			for (start, end) in verticalRanges {
				segments.append(
					(
						CGPoint(x: rect.maxX, y: rect.minY + start),
						CGPoint(x: rect.maxX, y: rect.minY + end)
					))
			}
			for (start, end) in horizontalRanges {
				segments.append(
					(
						CGPoint(x: rect.minX + start, y: rect.maxY),
						CGPoint(x: rect.minX + end, y: rect.maxY)
					))
			}
			for (start, end) in verticalRanges {
				segments.append(
					(
						CGPoint(x: rect.minX, y: rect.minY + start),
						CGPoint(x: rect.minX, y: rect.minY + end)
					))
			}
			return segments
		}

		let perimeter = dashedBorderPerimeter(for: rect)
		guard perimeter > 0 else {
			return []
		}

		var segments: [(CGPoint, CGPoint)] = []
		for (dashStart, dashEnd) in dashedBorderDashRanges(
			perimeter: perimeter,
			dashLength: dashLength,
			gapLength: gapLength
		) {
			appendDashedBorderSegments(
				for: rect,
				dashStart: dashStart,
				dashEnd: dashEnd,
				into: &segments
			)
		}
		return segments
	}

	private static func dashedBorderEdgeRanges(
		edgeLength: CGFloat,
		cornerKeepout: CGFloat,
		dashLength: CGFloat,
		gapLength: CGFloat
	) -> [(CGFloat, CGFloat)] {
		let usableLength = edgeLength - cornerKeepout * 2
		guard usableLength > 0 else {
			return []
		}
		if usableLength <= dashLength {
			return [(cornerKeepout, edgeLength - cornerKeepout)]
		}

		let clampedDashLength = min(dashLength, usableLength)
		let cycleSpan = max(dashLength + gapLength, .leastNonzeroMagnitude)
		let dashCount = max(Int(floor((usableLength + gapLength) / cycleSpan)), 1)
		if dashCount == 1 {
			return [(cornerKeepout, edgeLength - cornerKeepout)]
		}

		let occupiedLength =
			CGFloat(dashCount) * clampedDashLength + CGFloat(dashCount - 1) * gapLength
		let gapCount = max(dashCount - 1, 0)
		let resolvedGapLength: CGFloat =
			if gapCount == 0 {
				gapLength
			} else {
				gapLength + max(usableLength - occupiedLength, 0) / CGFloat(gapCount)
			}

		return (0..<dashCount).map { index in
			let start = cornerKeepout + CGFloat(index) * (clampedDashLength + resolvedGapLength)
			return (start, start + clampedDashLength)
		}
	}

	private static func dashedBorderDashRanges(
		perimeter: CGFloat,
		dashLength: CGFloat,
		gapLength: CGFloat
	) -> [(CGFloat, CGFloat)] {
		guard perimeter > 0 else {
			return []
		}
		let targetCycle = max(dashLength + gapLength, .leastNonzeroMagnitude)
		let cycleCount = max(Int((perimeter / targetCycle).rounded()), 1)
		let cycleSpan = perimeter / CGFloat(cycleCount)
		let resolvedDashLength = min(dashLength, cycleSpan)

		return (0..<cycleCount).map { index in
			let start = CGFloat(index) * cycleSpan
			return (start, start + resolvedDashLength)
		}
	}

	private static func appendDashedBorderSegments(
		for rect: CGRect,
		dashStart: CGFloat,
		dashEnd: CGFloat,
		into segments: inout [(CGPoint, CGPoint)]
	) {
		var segmentStart = dashStart
		for cornerDistance in dashedBorderCornerDistances(for: rect) {
			if segmentStart >= dashEnd {
				break
			}
			if cornerDistance <= segmentStart || cornerDistance >= dashEnd {
				continue
			}
			pushDashedBorderSegment(
				for: rect, start: segmentStart, end: cornerDistance, into: &segments)
			segmentStart = cornerDistance
		}
		if segmentStart < dashEnd {
			pushDashedBorderSegment(for: rect, start: segmentStart, end: dashEnd, into: &segments)
		}
	}

	private static func pushDashedBorderSegment(
		for rect: CGRect,
		start: CGFloat,
		end: CGFloat,
		into segments: inout [(CGPoint, CGPoint)]
	) {
		let startPoint = dashedBorderPoint(for: rect, distance: start)
		let endPoint = dashedBorderPoint(for: rect, distance: end)
		guard startPoint != endPoint else {
			return
		}
		segments.append((startPoint, endPoint))
	}

	private static func dashedBorderPoint(for rect: CGRect, distance: CGFloat) -> CGPoint {
		let width = rect.width
		let height = rect.height
		let perimeter = dashedBorderPerimeter(for: rect)
		let normalizedDistance = distance.truncatingRemainder(dividingBy: perimeter)
		let resolvedDistance =
			normalizedDistance < 0 ? normalizedDistance + perimeter : normalizedDistance

		if resolvedDistance < width {
			return CGPoint(x: rect.minX + resolvedDistance, y: rect.minY)
		}
		if resolvedDistance < width + height {
			return CGPoint(x: rect.maxX, y: rect.minY + (resolvedDistance - width))
		}
		if resolvedDistance < width * 2 + height {
			return CGPoint(x: rect.maxX - (resolvedDistance - width - height), y: rect.maxY)
		}
		return CGPoint(x: rect.minX, y: rect.maxY - (resolvedDistance - width * 2 - height))
	}

	private static func dashedBorderCornerDistances(for rect: CGRect) -> [CGFloat] {
		let width = rect.width
		let height = rect.height
		return [width, width + height, width * 2 + height, dashedBorderPerimeter(for: rect)]
	}

	private static func dashedBorderPerimeter(for rect: CGRect) -> CGFloat {
		guard rect.width > 0, rect.height > 0 else {
			return 0
		}
		return (rect.width + rect.height) * 2
	}

	static func palette(for theme: CaptureChromeTheme, settings: NativeHostSettings)
		-> CaptureChromePalette
	{
		let opacity = effectiveHudOpacity(settings: settings)
		let tint = CGFloat(settings.hudTint.clamped(to: 0...1))
		let foregrounds = foregroundPalette(for: theme)
		let bodyAlphaFloor: CGFloat = theme == .dark ? 0.06 : 0.08
		let fillOpacity: CGFloat =
			settings.hudGlassEnabled
			? max(bodyAlphaFloor, opacity * 0.20)
			: opacity
		let tintColor = glassTintColor(for: theme, settings: settings)

		switch theme {
		case .dark:
			let baseFill = NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 1)
			let bodyFill =
				baseFill
				.mixed(with: tintColor, fraction: tint * 0.72)
				.withAlphaComponent(fillOpacity)
			return CaptureChromePalette(
				foregrounds: foregrounds,
				bodyFill: bodyFill,
				outerStroke: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.12, 0.14 + opacity * 0.10)),
				shadow: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.16, 0.12 + opacity * 0.18)),
				swatchStroke: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 36 / 255),
				keycapFill: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.06, opacity * 0.18)),
				keycapStroke: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.10, opacity * 0.22)),
				toolbarHoverBackground: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.08, opacity * 0.18)),
				toolbarSelectedBackground: NSColor(
					srgbRed: 1, green: 1, blue: 1, alpha: max(0.12, opacity * 0.24))
			)
		case .light:
			let baseFill = NSColor(srgbRed: 232 / 255, green: 236 / 255, blue: 243 / 255, alpha: 1)
			let bodyFill =
				baseFill
				.mixed(with: tintColor, fraction: tint * 0.62)
				.withAlphaComponent(fillOpacity)
			return CaptureChromePalette(
				foregrounds: foregrounds,
				bodyFill: bodyFill,
				outerStroke: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.12, 0.16 + opacity * 0.12)),
				shadow: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.10, 0.06 + opacity * 0.14)),
				swatchStroke: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: 44 / 255),
				keycapFill: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.05, opacity * 0.12)),
				keycapStroke: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.10, opacity * 0.20)),
				toolbarHoverBackground: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.08, opacity * 0.16)),
				toolbarSelectedBackground: NSColor(
					srgbRed: 0, green: 0, blue: 0, alpha: max(0.10, opacity * 0.22))
			)
		}
	}

	private static func foregroundPalette(for theme: CaptureChromeTheme)
		-> CaptureChromeForegroundPalette
	{
		switch theme {
		case .dark:
			let primary = NSColor(
				srgbRed: 235 / 255, green: 235 / 255, blue: 245 / 255, alpha: 235 / 255)
			let secondary = NSColor(
				srgbRed: 235 / 255, green: 235 / 255, blue: 245 / 255, alpha: 150 / 255)
			let controlBase = NSColor.white
			return CaptureChromeForegroundPalette(
				primary: primary,
				secondary: secondary,
				control: controlBase.withAlphaComponent(160 / 255),
				controlHover: controlBase.withAlphaComponent(222 / 255),
				controlSelected: controlBase,
				controlDisabled: controlBase.withAlphaComponent(72 / 255)
			)
		case .light:
			let primary = NSColor(
				srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 235 / 255)
			let secondary = NSColor(
				srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 160 / 255)
			let controlBase = NSColor.black
			return CaptureChromeForegroundPalette(
				primary: primary,
				secondary: secondary,
				control: controlBase.withAlphaComponent(182 / 255),
				controlHover: controlBase.withAlphaComponent(220 / 255),
				controlSelected: controlBase,
				controlDisabled: controlBase.withAlphaComponent(82 / 255)
			)
		}
	}

	static func glassOpacity(settings: NativeHostSettings) -> Float {
		Float(0.88 + settings.hudBlur.clamped(to: 0...1) * 0.12)
	}

	static func effectiveHudOpacity(settings: NativeHostSettings) -> CGFloat {
		if settings.usesLiquidHudGlass {
			return liquidGlassBodyOpacity
		}
		return CGFloat(settings.hudOpacity.clamped(to: 0...1))
	}

	static func effectiveBodyFill(
		palette: CaptureChromePalette,
		settings: NativeHostSettings,
		hasGlass: Bool
	) -> NSColor {
		let opacity = effectiveHudOpacity(settings: settings)
		if hasGlass {
			return palette.bodyFill.withAlphaComponent(
				max(palette.bodyFill.alphaComponent, max(0.22, opacity * 0.42)))
		}
		return palette.bodyFill.withAlphaComponent(max(0.42, opacity * 0.82))
	}

	private static func glassTintColor(
		for theme: CaptureChromeTheme, settings: NativeHostSettings
	) -> NSColor {
		let hue = CGFloat(settings.hudTintHue.clamped(to: 0...1))
		return NSColor(
			calibratedHue: hue,
			saturation: theme == .dark ? 0.48 : 0.34,
			brightness: theme == .dark ? 0.62 : 0.94,
			alpha: 1
		)
	}
}

extension CGFloat {
	func clamped(to range: ClosedRange<CGFloat>) -> CGFloat {
		Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
	}
}

extension Double {
	func clamped(to range: ClosedRange<Double>) -> Double {
		Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
	}
}

extension NSColor {
	fileprivate func mixed(with other: NSColor, fraction: CGFloat) -> NSColor {
		let amount = fraction.clamped(to: 0...1)
		guard
			let lhs = usingColorSpace(.sRGB),
			let rhs = other.usingColorSpace(.sRGB)
		else {
			return self
		}
		return NSColor(
			srgbRed: lhs.redComponent + (rhs.redComponent - lhs.redComponent) * amount,
			green: lhs.greenComponent + (rhs.greenComponent - lhs.greenComponent) * amount,
			blue: lhs.blueComponent + (rhs.blueComponent - lhs.blueComponent) * amount,
			alpha: lhs.alphaComponent + (rhs.alphaComponent - lhs.alphaComponent) * amount
		)
	}
}

extension String {
	func size(using font: NSFont) -> CGSize {
		(self as NSString).size(withAttributes: [.font: font])
	}
}

extension NSImage {
	fileprivate func tinted(with color: NSColor) -> NSImage {
		let tinted = copy() as? NSImage ?? self
		tinted.isTemplate = true
		let image = NSImage(size: tinted.size)
		image.lockFocus()
		color.set()
		let rect = CGRect(origin: .zero, size: tinted.size)
		tinted.draw(in: rect, from: rect, operation: .sourceOver, fraction: 1.0)
		image.unlockFocus()
		return image
	}
}
