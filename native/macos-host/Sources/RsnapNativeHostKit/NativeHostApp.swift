import AppKit
import CoreGraphics
import CoreImage
import CoreText
import Darwin
import Foundation
import QuartzCore
import RsnapHostBridge
import Vision

struct LiveRgbSample: Sendable {
	// SCStream may stop emitting while the captured display is static; FrozenFrameAuthority
	// applies its own strict age budget for authoritative screenshot frames.
	static let maximumDisplayAge: TimeInterval = 60.0
	static let maximumReusableAge: TimeInterval = 0.04

	let rgb: RGBSample
	let capturedAtUptime: TimeInterval
	let source: String

	func ageMilliseconds(now: TimeInterval = ProcessInfo.processInfo.systemUptime) -> Double {
		max(0, now - capturedAtUptime) * 1_000
	}

	func isFresh(
		maximumAge: TimeInterval = Self.maximumDisplayAge,
		now: TimeInterval = ProcessInfo.processInfo.systemUptime
	) -> Bool {
		now - capturedAtUptime <= maximumAge
	}
}

struct LiveChromeSample {
	let rgb: LiveRgbSample?
	let loupePatch: CGImage?

	var rgbSample: RGBSample? {
		rgb?.rgb
	}

	init(rgb: LiveRgbSample?, loupePatch: CGImage?) {
		self.rgb = rgb
		self.loupePatch = loupePatch
	}

	init(
		rgbSample: RGBSample?,
		rgbCapturedAtUptime: TimeInterval? = nil,
		rgbSource: String = "unqualified",
		loupePatch: CGImage?
	) {
		if let rgbSample, let rgbCapturedAtUptime {
			rgb = LiveRgbSample(
				rgb: rgbSample,
				capturedAtUptime: rgbCapturedAtUptime,
				source: rgbSource
			)
		} else {
			rgb = nil
		}
		self.loupePatch = loupePatch
	}
}

enum LiveSamplingBudget {
	static let hoverWindowCacheRefreshInterval: TimeInterval = 1.0 / 15.0
}

struct LiveColorSampleSource: Equatable, Sendable {
	let referenceWindowID: CGWindowID
	let desktopFrame: CGRect
	let screenFrame: CGRect
	let displayID: CGDirectDisplayID
	let scaleFactor: CGFloat
}

private struct NativeHostFeedbackSound {
	let sound: NSSound?
	let playFailedEvent: String

	static func load(
		candidatePaths: [String],
		loadedEvent: String,
		loadFailedEvent: String,
		playFailedEvent: String
	) -> Self {
		for path in candidatePaths {
			if let sound = NSSound(contentsOfFile: path, byReference: true) {
				NativeHostTelemetry.lifecycleEvent(
					loadedEvent,
					detail: "path=\(path)"
				)
				return Self(sound: sound, playFailedEvent: playFailedEvent)
			}
		}

		let candidates = candidatePaths.joined(separator: ",")
		NativeHostTelemetry.lifecycleWarning(
			loadFailedEvent,
			detail: "candidates=\(candidates)"
		)
		return Self(sound: nil, playFailedEvent: playFailedEvent)
	}

	func play() {
		guard let sound else {
			return
		}
		sound.stop()
		sound.currentTime = 0
		if sound.play() == false {
			NativeHostTelemetry.lifecycleWarning(playFailedEvent)
		}
	}
}

private enum CaptureSuccessSound {
	private static let candidatePaths = [
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Screen Capture.aif",
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Shutter.aif",
	]

	static func load() -> NativeHostFeedbackSound {
		NativeHostFeedbackSound.load(
			candidatePaths: candidatePaths,
			loadedEvent: "native_host.capture_success_sound_loaded",
			loadFailedEvent: "native_host.capture_success_sound_load_failed",
			playFailedEvent: "native_host.capture_success_sound_play_failed"
		)
	}
}

private enum OcrCompletionSound {
	private static let candidatePaths = [
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/accessibility/Sticky Keys ON.aif",
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/siri/jbl_confirm.caf",
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Volume Mount.aif",
		"/System/Library/Sounds/Glass.aiff",
	]

	static func load() -> NativeHostFeedbackSound {
		NativeHostFeedbackSound.load(
			candidatePaths: candidatePaths,
			loadedEvent: "native_host.ocr_completion_sound_loaded",
			loadFailedEvent: "native_host.ocr_completion_sound_load_failed",
			playFailedEvent: "native_host.ocr_completion_sound_play_failed"
		)
	}
}

package func scrollCaptureMinimapPlan(
	for selection: CGRect,
	exportSize: CGSize,
	in bounds: CGRect,
	preferredWidth: CGFloat,
	minimumWidth: CGFloat,
	gap: CGFloat,
	margin: CGFloat,
	imageInset: CGFloat,
	viewportTopPixels: CGFloat,
	viewportHeightPixels: CGFloat
) -> ScrollMinimapLayoutPlan? {
	try? RsnapScrollMinimapPlanner.plan(
		selection: selection,
		exportSize: exportSize,
		bounds: bounds,
		preferredWidth: preferredWidth,
		minimumWidth: minimumWidth,
		gap: gap,
		margin: margin,
		imageInset: imageInset,
		viewportTopPixels: viewportTopPixels,
		viewportHeightPixels: viewportHeightPixels
	)
}

@MainActor
public final class NativeHostApplicationController: NSObject, NSApplicationDelegate {
	private let settingsStore = NativeHostSettingsStore()
	private let hotKeyCoordinator = HotKeyBindingCoordinator()
	private var lifecycleActivity: NSObjectProtocol?
	private var selfCaptureRegistrationWindow: NSWindow?
	private var didBootstrap = false
	private var didPresentLaunchPermissionOnboarding = false
	private let softwareUpdater = NativeHostSoftwareUpdater()
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
	private lazy var permissionRecoveryWindowController = PermissionRecoveryGuideWindowController()
	private lazy var settingsWindowController = SettingsWindowController(
		settingsStore: settingsStore,
		softwareUpdater: softwareUpdater,
		onClose: { [weak self] in
			self?.settingsWindowDidClose()
		})

	public func finishLaunching() {
		guard didBootstrap == false else {
			return
		}
		didBootstrap = true
		NativeHostTelemetry.lifecycleEvent("native_host.finish_launching_begin")
		NSApp.setActivationPolicy(.accessory)
		ProcessInfo.processInfo.disableAutomaticTermination("Rsnap menubar host")
		ProcessInfo.processInfo.disableSuddenTermination()
		lifecycleActivity = ProcessInfo.processInfo.beginActivity(
			options: [.automaticTerminationDisabled, .suddenTerminationDisabled],
			reason: "Rsnap menubar host"
		)
		Self.applyApplicationIcon()
		configureStatusItem()
		configureGlobalHotKeys()
		showSelfCaptureRegistrationWindow()
		NotificationCenter.default.addObserver(
			self,
			selector: #selector(settingsDidChange),
			name: NativeHostSettingsStore.didChangeNotification,
			object: settingsStore
		)
		refreshHotKeyBindings(for: sessionController.currentSceneMode)
		refreshStatusMenuState()
		sessionController.prepareLiveFrameStreamSampler(reason: "launch")
		scheduleLaunchPermissionOnboardingIfNeeded()
		scheduleLaunchUpdateCheckIfEnabled()
		DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(250)) { [weak self] in
			self?.sessionController.refreshShareableContentCacheIfPermitted(source: "launch")
		}
		NativeHostTelemetry.lifecycleEvent(
			"native_host.finish_launching_end",
			detail: "statusItemPresent=\(statusItem != nil)"
		)
	}

	public func applicationDidFinishLaunching(_ notification: Notification) {
		finishLaunching()
	}

	public func applicationWillTerminate(_ notification: Notification) {
		hotKeyCoordinator.invalidate()
		sessionController.releaseScreenCaptureStreams(immediate: true)
	}

	private func showSelfCaptureRegistrationWindow() {
		guard selfCaptureRegistrationWindow == nil else {
			return
		}
		let screenFrame = NSScreen.main?.frame ?? CGRect(x: 0, y: 0, width: 1, height: 1)
		let window = NSWindow(
			contentRect: CGRect(x: screenFrame.minX, y: screenFrame.minY, width: 1, height: 1),
			styleMask: [.borderless],
			backing: .buffered,
			defer: false
		)
		window.alphaValue = 0.001
		window.backgroundColor = .clear
		window.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]
		window.hasShadow = false
		window.ignoresMouseEvents = true
		window.isOpaque = false
		window.isReleasedWhenClosed = false
		window.level = .normal
		window.sharingType = .readOnly
		window.orderFrontRegardless()
		selfCaptureRegistrationWindow = window
		NativeHostTelemetry.lifecycleDebug("native_host.self_capture_registration_window_visible")
	}

	public func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
		false
	}

	public func applicationShouldHandleReopen(
		_ sender: NSApplication,
		hasVisibleWindows flag: Bool
	) -> Bool {
		openSettings(nil)
		return false
	}

	deinit {
		NotificationCenter.default.removeObserver(self)
	}

	@objc
	private func startCapture(_ sender: Any?) {
		if presentPermissionRecoveryIfNeeded(source: "start_capture") {
			return
		}
		sessionController.startCapture(
			capturableOwnWindowIDs: settingsWindowController.captureExceptionWindowIDs)
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
	private func openScreenshotsFolder(_ sender: Any?) {
		let outputDirectory = settingsStore.settings.outputDirectory
		do {
			try FileManager.default.createDirectory(
				at: outputDirectory,
				withIntermediateDirectories: true)
			NSWorkspace.shared.open(outputDirectory)
			NativeHostTelemetry.lifecycleEvent("native_host.output_directory_opened")
		} catch {
			NativeHostTelemetry.lifecycleWarning(
				"native_host.output_directory_open_failed",
				detail: "reason=create_or_open_failed")
		}
	}

	@objc
	private func checkForUpdates(_ sender: Any?) {
		softwareUpdater.checkForUpdates(sender)
	}

	private func scheduleLaunchPermissionOnboardingIfNeeded() {
		DispatchQueue.main.async { [weak self] in
			_ = self?.presentPermissionRecoveryIfNeeded(
				source: "launch",
				oncePerLaunch: true
			)
		}
	}

	private func scheduleLaunchUpdateCheckIfEnabled() {
		Task { @MainActor [weak self] in
			self?.softwareUpdater.checkForUpdatesInBackgroundOnLaunchIfEnabled()
		}
	}

	@discardableResult
	private func presentPermissionRecoveryIfNeeded(
		source: String,
		oncePerLaunch: Bool = false
	) -> Bool {
		guard NativePermissions.screenRecordingGranted == false else {
			permissionRecoveryWindowController.close()
			return false
		}
		if oncePerLaunch {
			guard didPresentLaunchPermissionOnboarding == false else {
				return true
			}
			didPresentLaunchPermissionOnboarding = true
		}
		permissionRecoveryWindowController.present()
		NativeHostTelemetry.lifecycleEvent(
			"native_host.permission_recovery_presented",
			detail: "source=\(source)"
		)
		return true
	}

	private func settingsWindowDidClose() {
		DispatchQueue.main.async { [weak self] in
			guard let self,
				self.settingsWindowController.window?.isVisible != true
			else {
				return
			}
			NSApp.setActivationPolicy(.accessory)
		}
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

		let menu = NSMenu(title: NativeHostBrand.displayName)
		let captureItem = menu.addItem(
			withTitle: "New Capture",
			action: #selector(startCapture(_:)),
			keyEquivalent: ""
		)
		menu.addItem(.separator())
		menu.addItem(
			withTitle: "Open Screenshots Folder",
			action: #selector(openScreenshotsFolder(_:)),
			keyEquivalent: "")
		menu.addItem(
			withTitle: "Check for Updates…",
			action: #selector(checkForUpdates(_:)),
			keyEquivalent: "")
		menu.addItem(.separator())
		menu.addItem(
			withTitle: "Settings…", action: #selector(openSettings(_:)), keyEquivalent: ",")
		menu.addItem(.separator())
		menu.addItem(withTitle: "Quit", action: #selector(quit(_:)), keyEquivalent: "q")
		for menuItem in menu.items {
			menuItem.target = self
		}

		item.menu = menu
		statusItem = item
		captureMenuItem = captureItem
		updateCaptureMenuShortcut()
		NativeHostTelemetry.lifecycleEvent(
			"native_host.status_item_installed",
			detail: "visible=\(item.isVisible),hasMenu=\(item.menu != nil)"
		)
	}

	private func configureGlobalHotKeys() {
		hotKeyCoordinator.onCaptureRequested = { [weak self] in
			self?.startCapture(nil)
		}
		hotKeyCoordinator.onCancelRequested = { [weak self] in
			self?.cancelCapture(nil)
		}
		hotKeyCoordinator.onToggleLoupeRequested = { [weak self] in
			self?.sessionController.toggleLoupe()
		}
		hotKeyCoordinator.onSaveRequested = { [weak self] in
			self?.sessionController.saveSelection()
		}
	}

	fileprivate func refreshStatusMenuState() {
		let isCaptureActive = sessionController.isCaptureActive
		captureMenuItem?.isEnabled = !isCaptureActive
	}

	private func updateCaptureMenuShortcut() {
		guard let captureMenuItem else {
			return
		}
		let shortcut = NativeHostSettings.captureHotKeyPresentation(
			for: settingsStore.settings.captureHotkey)
		captureMenuItem.keyEquivalent = shortcut.keyEquivalent
		captureMenuItem.keyEquivalentModifierMask = shortcut.modifierMask
	}

	private func refreshHotKeyBindings(for mode: SceneKind) {
		hotKeyCoordinator.update(
			captureHotKey: settingsStore.settings.captureHotkey,
			sceneMode: mode
		)
	}

	@objc
	private func settingsDidChange() {
		refreshHotKeyBindings(for: sessionController.currentSceneMode)
		updateCaptureMenuShortcut()
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
	struct FrozenCaptureJobSource: Sendable {
		let referenceWindowID: CGWindowID
		let desktopFrame: CGRect
	}

	private struct PendingFrozenCommit: Sendable {
		let id: UInt64
		let captureID: UInt64
		let generation: UInt64
		let selection: CGRect
		let editable: Bool
		let token: FrozenFrameLatchToken?
		let startedAtUptime: TimeInterval
		let snapshotStartedAtUptime: TimeInterval
		let hadLatchToken: Bool
	}

	private static let autoCenterMaxIterations = 6
	private static let displayFirstFrameWait: TimeInterval = 0.025
	private static let coldSelfCaptureRecoveryWait: TimeInterval = 3.5
	private static let scrollCaptureEnabled = false
	private static let scrollCaptureForwardingPassthrough: TimeInterval = 0.055
	private static let scrollCaptureSampleDelay: TimeInterval = 0.04
	private static let liveFrameStreamReleaseGrace: TimeInterval = 1.5

	private let settingsStore: NativeHostSettingsStore
	private let liveFrameStream = LiveFrameStreamBroker()
	private let frozenFrameAuthority = FrozenFrameAuthority()
	private let frozenCommitQueue = DispatchQueue(
		label: "ink.hack.rsnap.frozen-commit",
		qos: .userInitiated
	)
	private let captureSuccessSound = CaptureSuccessSound.load()
	private let ocrCompletionSound = OcrCompletionSound.load()
	private var session: RsnapHostSession?
	private var overlayController: CaptureOverlayController?
	private var frozenFrameLatchToken: FrozenFrameLatchToken?
	private var pendingFrozenCommit: PendingFrozenCommit?
	private var nextPendingFrozenCommitID: UInt64 = 1
	private var frozenSnapshotGeneration: UInt64 = 0
	private var completedHostEffect: HostEffectKind?
	private var scrollCaptureState: NativeScrollCaptureState?
	private var scrollCaptureGlobalMonitor: Any?
	private var nextCaptureTelemetryID: UInt64 = 1
	private var activeCaptureTelemetryID: UInt64?
	private var pendingLiveFrameStreamRelease: DispatchWorkItem?
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

	private func pointTelemetryDetail(_ point: CGPoint) -> String {
		"x=\(Int(point.x.rounded())) y=\(Int(point.y.rounded()))"
	}

	func prepareLiveFrameStreamSampler(reason: String) {
		liveFrameStream.prepareSampler(reason: reason)
	}

	private func allocateCaptureTelemetryID() -> UInt64 {
		let captureID = nextCaptureTelemetryID
		nextCaptureTelemetryID &+= 1
		if nextCaptureTelemetryID == 0 {
			nextCaptureTelemetryID = 1
		}
		return captureID
	}

	func refreshShareableContentCacheIfPermitted(source: String) {
		guard session == nil else {
			DispatchQueue.main.asyncAfter(deadline: .now() + .seconds(2)) { [weak self] in
				self?.refreshShareableContentCacheIfPermitted(source: source)
			}
			return
		}
		guard NativePermissions.screenRecordingGranted else {
			return
		}
		frozenFrameAuthority.refreshShareableContentCache(
			captureID: currentCaptureTelemetryID,
			source: source
		)
	}

	func hasFreshShareableContentCache() -> Bool {
		frozenFrameAuthority.hasFreshShareableContentCache()
	}

	@discardableResult
	func warmLiveSamplingIfPossible(
		at point: CGPoint,
		source: String = "capture",
		captureID: UInt64 = 0,
		excludeSelfFromFrozenAuthority: Bool = false,
		selfCaptureExceptionWindowIDs: Set<CGWindowID> = [],
		includedCurrentProcessWindowIDs: Set<CGWindowID> = []
	) -> LiveChromeSample? {
		let warmStartedAt = ProcessInfo.processInfo.systemUptime
		let screenCount = NSScreen.screens.count
		guard NativePermissions.screenRecordingGranted else {
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
		frozenFrameAuthority.start(
			for: screens,
			captureID: captureID,
			source: source,
			rebuildContentFilter: excludeSelfFromFrozenAuthority,
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
		)
		let frozenAuthorityStartMilliseconds =
			NativeHostTelemetry.milliseconds(since: frozenAuthorityStartedAt)
		NativeHostTelemetry.liveSamplingWarmTiming(
			captureID: captureID,
			source: source,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: warmStartedAt),
			frozenAuthorityStartMilliseconds: frozenAuthorityStartMilliseconds,
			liveStreamStartMilliseconds: 0,
			seedSampleMilliseconds: 0,
			sampleReady: false,
			screenCount: screenCount
		)
		return nil
	}

	func startCapture(capturableOwnWindowIDs: Set<CGWindowID> = []) {
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
			try startCaptureSession(
				captureID: captureID,
				captureStartedAt: captureStartedAt,
				capturableOwnWindowIDs: capturableOwnWindowIDs
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

	private func startCaptureSession(
		captureID: UInt64,
		captureStartedAt: TimeInterval,
		capturableOwnWindowIDs: Set<CGWindowID>
	) throws {
		let startPoint = NSEvent.mouseLocation
		let desktopFrame = CaptureOverlayController.desktopFrame
		frozenFrameLatchToken = nil
		// The Rust live sampler treats these IDs as current-process windows to
		// include through the app-level exclusion. Overlay windows must stay out
		// of this list so color sampling sees the desktop under the capture UI.
		pendingLiveFrameStreamRelease?.cancel()
		pendingLiveFrameStreamRelease = nil
		liveFrameStream.updateSelfCaptureExceptionWindowIDs(capturableOwnWindowIDs)
		let warmStartedAt = ProcessInfo.processInfo.systemUptime
		let initialSample = warmLiveSamplingIfPossible(
			at: startPoint,
			source: "start_capture",
			captureID: captureID,
			includedCurrentProcessWindowIDs: capturableOwnWindowIDs
		)
		let initialRgbSample =
			initialSample?.rgbSample
			?? frozenFrameAuthority.rgbSample(containing: startPoint)
		let warmMilliseconds = NativeHostTelemetry.milliseconds(since: warmStartedAt)
		liveFrameStream.start(
			for: NSScreen.screens,
			prewarmPoint: startPoint,
			captureID: captureID
		)
		let windowSnapshotStartedAt = ProcessInfo.processInfo.systemUptime
		let initialWindowSnapshots = WindowSnapshotFeed.snapshots(desktopFrame: desktopFrame)
		let windowSnapshotMilliseconds =
			NativeHostTelemetry.milliseconds(since: windowSnapshotStartedAt)
		let initialHighlightedWindow = WindowSnapshotFeed.window(
			at: startPoint, in: initialWindowSnapshots)
		chromeState.rgbSample = initialRgbSample
		let sessionSetupStartedAt = ProcessInfo.processInfo.systemUptime
		let session = try RsnapHostSession(configuration: settingsStore.sessionConfiguration)
		self.session = session

		try session.enterLive()
		try session.send(
			event: .pointerMoved(
				point: startPoint,
				rgb: initialRgbSample,
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
			liveFrameStream: liveFrameStream,
			frameRgbSampler: { [frozenFrameAuthority] point in
				frozenFrameAuthority.liveRgbSample(containing: point)
			},
			framePatchSampler: { [frozenFrameAuthority] point, sidePixels in
				frozenFrameAuthority.loupePatch(containing: point, sidePixels: sidePixels)
			}
		)
		self.overlayController = overlayController
		let overlayShowStartedAt = ProcessInfo.processInfo.systemUptime
		overlayController.show(
			initialScene: initialScene,
			chrome: chromeState,
			settings: settingsStore.settings,
			focusPoint: startPoint,
			initialWindowSnapshots: initialWindowSnapshots,
			prepareCaptureStreams: { [weak self, weak overlayController] in
				guard let self, let overlayController else {
					return
				}
				let selfCaptureExceptionWindowIDs =
					overlayController.selfCaptureExceptionWindowIDs
				self.liveFrameStream.start(
					for: NSScreen.screens,
					prewarmPoint: startPoint,
					captureID: captureID
				)
				if self.frozenFrameAuthority.hasSelfCaptureCompleteFrame(
					containing: startPoint)
				{
					NativeHostTelemetry.captureEvent(
						"capture.self_capture_rebuild_skipped",
						captureID: captureID,
						detail: "start_capture_complete_filter"
					)
				} else {
					_ = self.warmLiveSamplingIfPossible(
						at: startPoint,
						source: "capture_overlay_preflight",
						captureID: captureID,
						excludeSelfFromFrozenAuthority: true,
						selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
						includedCurrentProcessWindowIDs: capturableOwnWindowIDs
					)
				}
			}
		)
		overlayController.prepareCaptureStreamsNow(trigger: "overlay_show")
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
			initialSampleReady: initialRgbSample != nil,
			screenCount: NSScreen.screens.count,
			windowCount: initialWindowSnapshots.count
		)
	}

	private func ensureCapturePermissions() -> Bool {
		guard NativePermissions.screenRecordingGranted == false else {
			return true
		}
		return NativePermissions.requestScreenRecording()
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
		guard pendingFrozenCommit == nil else {
			return
		}

		do {
			overlayController?.prepareCaptureStreamsNow(trigger: "primary_interaction")
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
			chromeState.endHostLocalFrozenSelecting()
			refreshOverlay()
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
		guard pendingFrozenCommit == nil else {
			return
		}

		do {
			liveFrameStream.prime(at: point)
			if frozenFrameLatchToken == nil {
				frozenFrameLatchToken = frozenFrameAuthority.latchToken(containing: point)
			}
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
		guard pendingFrozenCommit == nil else {
			return
		}

		overlayController?.markLivePrimaryInteractionReleased(at: point)
		do {
			NativeHostTelemetry.captureEvent(
				"capture.live_primary_complete_requested",
				captureID: currentCaptureTelemetryID,
				detail: pointTelemetryDetail(point)
			)
			liveFrameStream.prime(at: point)
			if frozenFrameLatchToken == nil {
				frozenFrameLatchToken = frozenFrameAuthority.latchToken(containing: point)
			}
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
			NativeHostTelemetry.captureEvent(
				"capture.live_primary_complete_synced",
				captureID: currentCaptureTelemetryID,
				detail: "mode=\(scene.mode)"
			)
			if scene.mode == .live {
				if pendingFrozenCommit == nil {
					chromeState.endHostLocalFrozenSelecting()
					refreshOverlay()
				}
			}
		} catch {
			chromeState.endHostLocalFrozenSelecting()
			refreshOverlay()
			NativeHostTelemetry.captureWarning(
				"capture.primary_interaction_complete_failed",
				captureID: currentCaptureTelemetryID,
				stage: "send_or_sync",
				error: String(describing: error)
			)
		}
	}

	func registerLivePrimaryInteractionOwner(_ owner: CaptureHostView) {
		overlayController?.registerLivePrimaryInteractionOwner(owner)
	}

	func completeLivePrimaryInteraction(from sender: CaptureHostView, at point: CGPoint) {
		overlayController?.completeLivePrimaryInteraction(from: sender, at: point)
	}

	func copySelection() {
		let _ = chromeState.frozenOverlay.commitTextEdit(
			style: chromeState.annotationStyle.textStyle)
		sendFrozenAction(.copyRequested, exitAfter: .copyCapture)
	}

	func saveSelection() {
		let _ = chromeState.frozenOverlay.commitTextEdit(
			style: chromeState.annotationStyle.textStyle)
		sendFrozenAction(.saveRequested, exitAfter: .saveCapture)
	}

	func recognizeText() {
		let _ = chromeState.frozenOverlay.commitTextEdit(
			style: chromeState.annotationStyle.textStyle)
		sendFrozenAction(.recognizeTextRequested, exitAfter: .recognizeText)
	}

	func startScrollCapture() {
		guard Self.scrollCaptureEnabled else {
			return
		}
		let _ = chromeState.frozenOverlay.commitTextEdit(
			style: chromeState.annotationStyle.textStyle)
		sendFrozenAction(.toolbarItemInvoked(.scroll))
	}

	func invokeToolbarItem(_ item: ToolbarItemKind) {
		if item != .text {
			let _ = chromeState.frozenOverlay.commitTextEdit(
				style: chromeState.annotationStyle.textStyle)
		}
		switch item {
		case .copy:
			sendFrozenAction(.toolbarItemInvoked(item), exitAfter: .copyCapture)
		case .save:
			sendFrozenAction(.toolbarItemInvoked(item), exitAfter: .saveCapture)
		case .ocr:
			sendFrozenAction(.toolbarItemInvoked(item), exitAfter: .recognizeText)
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
		if chromeState.frozenOverlay.begin(
			tool: selectedTool,
			at: point,
			selection: selection,
			style: chromeState.annotationStyle
		) {
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
		guard chromeState.frozenSelectionTransformAllowed else {
			return false
		}
		guard
			let monitorFrame = screen(containing: CGPoint(x: selection.midX, y: selection.midY))?
				.frame
		else {
			return false
		}
		guard
			let kind = try? RsnapFrozenSelectionTransformPlanner.hitTest(
				point: point,
				selection: selection,
				handleRadius: 12,
				edgeTolerance: 4
			)
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
				NativeHostTelemetry.captureEvent(
					"capture.frozen_selection_transform_commit",
					captureID: captureID
				)
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
		try? RsnapFrozenSelectionTransformPlanner.transformedRect(
			kind: interaction.kind,
			initialSelection: interaction.initialSelection,
			monitorFrame: interaction.monitorFrame,
			initialPointer: interaction.initialPointer,
			point: point,
			minimumSize: CaptureChrome.frozenSelectionMinimumSize
		)
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

	func performFrozenAnnotationStyleAction(_ action: FrozenAnnotationStyleAction) {
		let selectedTool = scene.toolbarItems.first(where: { $0.selected })?.kind ?? .pointer
		guard chromeState.annotationStyle.apply(action, selectedTool: selectedTool) else {
			return
		}
		refreshOverlay()
	}

	func performFrozenAnnotationSizeSteps(_ steps: Int) {
		let selectedTool = scene.toolbarItems.first(where: { $0.selected })?.kind ?? .pointer
		guard chromeState.annotationStyle.applySizeSteps(steps, selectedTool: selectedTool)
		else {
			return
		}
		refreshOverlay()
	}

	func performFrozenAutoCenter() {
		guard let selection = currentFrozenSelection() else {
			return
		}
		if chromeState.frozenOverlay.keepsFrozenSelectionFixed {
			return
		}
		guard let screen = screen(containing: CGPoint(x: selection.midX, y: selection.midY)) else {
			return
		}

		var nextSelection = selection
		var nextBaseImage =
			(chromeState.frozenSelectionSnapshot == selection) ? chromeState.frozenBaseImage : nil
		if nextBaseImage == nil {
			nextBaseImage = frozenBaseImageFromDisplay(for: selection)
		}

		for _ in 0..<Self.autoCenterMaxIterations {
			guard
				let baseImage = nextBaseImage,
				let contentBounds = Self.detectAutoCenterContentBounds(in: baseImage)
			else {
				break
			}

			let deltaX = Self.autoCenterMarginBalanceShiftPoints(
				contentOriginPx: contentBounds.minX,
				contentSizePx: contentBounds.width,
				cropSizePx: CGFloat(baseImage.width),
				captureSizePoints: nextSelection.width
			)
			let deltaY = Self.autoCenterMarginBalanceShiftPoints(
				contentOriginPx: contentBounds.minY,
				contentSizePx: contentBounds.height,
				cropSizePx: CGFloat(baseImage.height),
				captureSizePoints: nextSelection.height
			)
			guard deltaX != 0 || deltaY != 0 else {
				break
			}

			let candidateSelection = Self.clampedSelectionRect(
				width: nextSelection.width,
				height: nextSelection.height,
				x: nextSelection.minX + deltaX,
				// Content bounds are in top-down CGImage coordinates; AppKit screen coordinates are bottom-up.
				y: nextSelection.minY - deltaY,
				monitorFrame: screen.frame
			)
			guard candidateSelection != nextSelection else {
				break
			}

			nextSelection = candidateSelection
			nextBaseImage = frozenBaseImageFromDisplay(for: nextSelection)
		}

		guard nextSelection != selection else {
			return
		}

		do {
			frozenSnapshotGeneration &+= 1
			chromeState.frozenSelectionSnapshot = nextSelection
			chromeState.frozenBaseImage =
				nextBaseImage ?? frozenBaseImageFromDisplay(for: nextSelection)
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
			if chromeState.frozenOverlay.commitTextEdit(
				style: chromeState.annotationStyle.textStyle)
			{
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
		guard flags.contains(.command) == false, flags.contains(.control) == false,
			flags.contains(.option) == false
		else {
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
			let shouldPrimeLoupePatch = scene.mode == .live && !scene.loupeVisible
			let loupePoint = scene.pointer ?? NSEvent.mouseLocation
			try session?.send(event: .toggleLoupe)
			if shouldPrimeLoupePatch {
				primeLoupePatchForToggle(at: loupePoint)
			}
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

	private func primeLoupePatchForToggle(at point: CGPoint) {
		let sample = overlayController?.immediateLiveChromeSample(
			point: point,
			settings: currentSettings,
			includeLoupePatch: true
		)
		if let rgbSample = sample?.rgbSample {
			chromeState.rgbSample = rgbSample
		}
		if let loupePatch = sample?.loupePatch {
			chromeState.loupePatch = loupePatch
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
		guard chromeState.hostLocalFrozenSelecting == false else {
			return
		}
		chromeState.beginHostLocalFrozenSelecting()
	}

	private func syncCore() throws {
		guard let session else {
			return
		}

		var pendingRequests = try session.drainRequests()
		while pendingRequests.isEmpty == false {
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
			if chromeState.hostLocalFrozenSelecting == false {
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
		case .requestFreezeSnapshot(let selection, let selectionEditable):
			NativeHostTelemetry.captureEvent(
				"capture.freeze_snapshot_requested",
				captureID: currentCaptureTelemetryID,
				detail:
					"editable=\(selectionEditable) x=\(Int(selection.minX.rounded())) y=\(Int(selection.minY.rounded())) w=\(Int(selection.width.rounded())) h=\(Int(selection.height.rounded()))"
			)
			try commitFrozenSelection(
				selection,
				editable: selectionEditable
			)
		case .startScrollCapture:
			guard Self.scrollCaptureEnabled else {
				try setHostStatusMessage("Scroll capture is temporarily disabled.")
				refreshOverlay()
				return
			}
			try beginNativeScrollCapture()
		case .copyCapture:
			try performCopy()
		case .saveCapture:
			try performSave()
		case .recognizeText:
			try performRecognizeText()
		case .requestScreenRecordingPermission:
			let granted = NativePermissions.requestScreenRecording()
			try session?.send(report: .permissionChanged(.screenRecording, granted: granted))
			if granted == false {
				try sendHostStatusMessage("Screen recording permission is required.")
			}
		}
	}

	private func commitFrozenSelection(_ selection: CGRect, editable: Bool) throws {
		guard session != nil else {
			return
		}
		let captureID = currentCaptureTelemetryID
		let commitStartedAt = ProcessInfo.processInfo.systemUptime
		frozenSnapshotGeneration &+= 1
		let generation = frozenSnapshotGeneration
		let selectionCenter = CGPoint(x: selection.midX, y: selection.midY)
		let hadLatchToken = frozenFrameLatchToken != nil
		let token =
			frozenFrameLatchToken ?? frozenFrameAuthority.latchToken(containing: selectionCenter)
		let snapshotStartedAt = ProcessInfo.processInfo.systemUptime
		let snapshotResolution = frozenFrameAuthority.resolveSnapshot(
			containing: selectionCenter,
			after: token,
			maxWait: frozenFrameLatchWait(containing: selectionCenter)
		)
		let snapshotWaitMilliseconds =
			NativeHostTelemetry.milliseconds(since: snapshotStartedAt)
		switch snapshotResolution {
		case .resolved(let frozenFrame):
			try finishFrozenCommit(
				captureID: captureID,
				selection: selection,
				editable: editable,
				frozenFrame: frozenFrame,
				commitStartedAt: commitStartedAt,
				snapshotWaitMilliseconds: snapshotWaitMilliseconds,
				hadLatchToken: hadLatchToken,
				syncAfterReport: false
			)
		case .pendingSelfCaptureFrame:
			let pendingCommit = PendingFrozenCommit(
				id: nextPendingFrozenCommitID,
				captureID: captureID,
				generation: generation,
				selection: selection,
				editable: editable,
				token: token,
				startedAtUptime: commitStartedAt,
				snapshotStartedAtUptime: snapshotStartedAt,
				hadLatchToken: hadLatchToken
			)
			nextPendingFrozenCommitID &+= 1
			schedulePendingFrozenCommit(
				pendingCommit,
				selectionCenter: selectionCenter
			)
		case .noFreshFrame:
			try failFrozenCommit(
				captureID: captureID,
				commitStartedAt: commitStartedAt,
				snapshotWaitMilliseconds: snapshotWaitMilliseconds,
				hadLatchToken: hadLatchToken
			)
		}
	}

	private func schedulePendingFrozenCommit(
		_ pendingCommit: PendingFrozenCommit,
		selectionCenter: CGPoint
	) {
		pendingFrozenCommit = pendingCommit
		refreshOverlay()
		let authority = frozenFrameAuthority
		let remainingWait = max(
			0,
			Self.coldSelfCaptureRecoveryWait
				- (ProcessInfo.processInfo.systemUptime - pendingCommit.snapshotStartedAtUptime)
		)
		frozenCommitQueue.async { [weak self] in
			let snapshotResolution = authority.resolveSnapshot(
				containing: selectionCenter,
				after: pendingCommit.token,
				maxWait: remainingWait
			)
			DispatchQueue.main.async {
				self?.finishPendingFrozenCommit(
					pendingCommit,
					snapshotResolution: snapshotResolution
				)
			}
		}
	}

	private func finishPendingFrozenCommit(
		_ pendingCommit: PendingFrozenCommit,
		snapshotResolution: FrozenFrameAuthority.SnapshotResolution
	) {
		guard
			let currentPending = pendingFrozenCommit,
			currentPending.id == pendingCommit.id,
			currentPending.generation == pendingCommit.generation,
			scene.mode == .live
		else {
			return
		}
		let snapshotWaitMilliseconds =
			NativeHostTelemetry.milliseconds(since: pendingCommit.snapshotStartedAtUptime)
		switch snapshotResolution {
		case .resolved(let frozenFrame):
			do {
				try finishFrozenCommit(
					captureID: pendingCommit.captureID,
					selection: pendingCommit.selection,
					editable: pendingCommit.editable,
					frozenFrame: frozenFrame,
					commitStartedAt: pendingCommit.startedAtUptime,
					snapshotWaitMilliseconds: snapshotWaitMilliseconds,
					hadLatchToken: pendingCommit.hadLatchToken,
					syncAfterReport: true
				)
			} catch {
				NativeHostTelemetry.captureWarning(
					"capture.freeze_commit_failed",
					captureID: pendingCommit.captureID,
					stage: "finish_pending_commit",
					error: String(describing: error)
				)
				tearDownCapture()
			}
		case .pendingSelfCaptureFrame, .noFreshFrame:
			do {
				try failFrozenCommit(
					captureID: pendingCommit.captureID,
					commitStartedAt: pendingCommit.startedAtUptime,
					snapshotWaitMilliseconds: snapshotWaitMilliseconds,
					hadLatchToken: pendingCommit.hadLatchToken
				)
			} catch {
				NativeHostTelemetry.captureWarning(
					"capture.freeze_commit_failed",
					captureID: pendingCommit.captureID,
					stage: "authority_snapshot_status",
					error: String(describing: error)
				)
			}
		}
	}

	private func failFrozenCommit(
		captureID: UInt64,
		commitStartedAt: TimeInterval,
		snapshotWaitMilliseconds: Double,
		hadLatchToken: Bool
	) throws {
		pendingFrozenCommit = nil
		frozenFrameLatchToken = nil
		chromeState.endHostLocalFrozenSelecting()
		refreshOverlay()
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
	}

	private func finishFrozenCommit(
		captureID: UInt64,
		selection: CGRect,
		editable: Bool,
		frozenFrame: FrozenFrameSnapshot,
		commitStartedAt: TimeInterval,
		snapshotWaitMilliseconds: Double,
		hadLatchToken: Bool,
		syncAfterReport: Bool
	) throws {
		guard let session else {
			return
		}
		pendingFrozenCommit = nil
		frozenFrameLatchToken = nil
		chromeState.resetFrozenChrome()
		chromeState.frozenSelectionSnapshot = selection
		chromeState.frozenSelectionEditable = editable
		chromeState.frozenSelectionInteraction = nil
		let frameSource = captureFrameSource(
			for: selection,
			editable: editable
		)
		chromeState.captureFrameSource = frameSource
		chromeState.captureFrameWindowID =
			frameSource == .window ? scene.highlightedWindow?.windowID : nil
		chromeState.frozenDisplayFrame = frozenFrame.displayFrame
		chromeState.frozenDisplayImage = frozenFrame.image
		let hostOwnedFrozenScene = hostOwnedFrozenPresentationScene(
			for: selection,
			editable: editable
		)
		let presentStartedAt = ProcessInfo.processInfo.systemUptime
		overlayController?.presentFrozenFirstFrame(
			scene: hostOwnedFrozenScene,
			chrome: chromeState,
			settings: settingsStore.settings
		)
		let presentMilliseconds = NativeHostTelemetry.milliseconds(since: presentStartedAt)
		let baseImageStartedAt = ProcessInfo.processInfo.systemUptime
		chromeState.frozenBaseImage = frozenBaseImageFromDisplay(for: selection)
		let baseImageMilliseconds =
			NativeHostTelemetry.milliseconds(since: baseImageStartedAt)
		NativeHostTelemetry.freezeCommitTiming(
			captureID: captureID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: commitStartedAt),
			snapshotWaitMilliseconds: snapshotWaitMilliseconds,
			baseImageMilliseconds: baseImageMilliseconds,
			presentMilliseconds: presentMilliseconds,
			frameAgeMilliseconds: frozenFrame.ageMilliseconds(),
			displayID: frozenFrame.displayID,
			sequence: frozenFrame.sequence,
			snapshotSource: frozenFrame.source,
			snapshotGeneration: frozenFrame.generation,
			selfCaptureSafe: frozenFrame.selfCaptureSafe,
			selfCaptureFilterComplete: frozenFrame.selfCaptureFilterComplete,
			hadLatchToken: hadLatchToken,
			baseReady: chromeState.frozenBaseImage != nil
		)
		try session.send(report: .freezeSnapshotCommitted(selection: selection))
		if syncAfterReport {
			try syncCore()
		}
	}

	private func frozenFrameLatchWait(containing _: CGPoint) -> TimeInterval {
		Self.displayFirstFrameWait
	}

	private func hostOwnedFrozenPresentationScene(for selection: CGRect, editable: Bool)
		-> SceneSnapshot
	{
		SceneSnapshot(
			mode: .frozen,
			cursorIntent: editable ? .grab : .default,
			pointer: scene.pointer,
			activeMonitor: nil,
			highlightedWindow: nil,
			liveSelectionPreview: nil,
			frozenSelection: selection,
			rgb: scene.rgb,
			loupeVisible: false,
			toolbarItems: hostOwnedFrozenToolbarItems(scrollEnabled: editable),
			statusMessage: nil
		)
	}

	private func captureFrameSource(for selection: CGRect, editable: Bool) -> CaptureFrameSource {
		if editable {
			return .dragRegion
		}
		if scene.highlightedWindow != nil {
			return .window
		}
		if let activeMonitor = scene.activeMonitor,
			Self.rectNearlyMatches(selection, activeMonitor.frame, tolerance: 2)
		{
			return .fullScreen
		}
		if NSScreen.screens.contains(where: { screen in
			Self.rectNearlyMatches(selection, screen.frame, tolerance: 2)
		}) {
			return .fullScreen
		}
		return .unknown
	}

	private static func rectNearlyMatches(
		_ lhs: CGRect,
		_ rhs: CGRect,
		tolerance: CGFloat
	) -> Bool {
		abs(lhs.minX - rhs.minX) <= tolerance
			&& abs(lhs.minY - rhs.minY) <= tolerance
			&& abs(lhs.width - rhs.width) <= tolerance
			&& abs(lhs.height - rhs.height) <= tolerance
	}

	private func hostOwnedFrozenToolbarItems(scrollEnabled: Bool) -> [ToolbarItem] {
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
		]
		if Self.scrollCaptureEnabled {
			items.append(ToolbarItem(kind: .scroll, enabled: scrollEnabled, selected: false))
		}
		if allowTextInput {
			items.append(ToolbarItem(kind: .ocr, enabled: true, selected: false))
		}
		items.append(ToolbarItem(kind: .copy, enabled: true, selected: false))
		items.append(ToolbarItem(kind: .save, enabled: true, selected: false))
		return items
	}

	var scrollCaptureToolbarEnabled: Bool {
		Self.scrollCaptureEnabled
			&& scene.mode == .frozen
			&& scrollCaptureState == nil
			&& currentFrozenSelection() != nil
	}

	func handleScrollCaptureWheel(_ event: NSEvent, at point: CGPoint) -> Bool {
		guard Self.scrollCaptureEnabled else {
			return false
		}
		guard var state = scrollCaptureState else {
			return false
		}
		guard state.viewportRect.contains(point) else {
			return false
		}

		let targetPoint = CGPoint(
			x: point.x.clamped(to: state.viewportRect.minX...state.viewportRect.maxX),
			y: point.y.clamped(to: state.viewportRect.minY...state.viewportRect.maxY)
		)
		let posted =
			overlayController?.withPrimaryMousePassthrough(
				duration: Self.scrollCaptureForwardingPassthrough
			) {
				Self.postScrollWheelEvent(matching: event, at: targetPoint)
			} ?? Self.postScrollWheelEvent(matching: event, at: targetPoint)

		guard posted else {
			try? setHostStatusMessage("Could not forward scroll input.")
			refreshOverlay()
			return true
		}

		state.sampleGeneration &+= 1
		let generation = state.sampleGeneration
		scrollCaptureState = state
		DispatchQueue.main.asyncAfter(deadline: .now() + Self.scrollCaptureSampleDelay) {
			[weak self] in
			self?.observeNativeScrollCaptureFrame(generation: generation)
		}

		return true
	}

	private func installNativeScrollCaptureMonitor() {
		removeNativeScrollCaptureMonitor()
		scrollCaptureGlobalMonitor = NSEvent.addGlobalMonitorForEvents(matching: .scrollWheel) {
			[weak self] _ in
			DispatchQueue.main.async { [weak self] in
				self?.scheduleNativeScrollCaptureSampleIfPointerIsInViewport()
			}
		}
	}

	private func removeNativeScrollCaptureMonitor() {
		if let monitor = scrollCaptureGlobalMonitor {
			NSEvent.removeMonitor(monitor)
			scrollCaptureGlobalMonitor = nil
		}
		overlayController?.setScrollCaptureMousePassthroughActive(false)
	}

	private func scheduleNativeScrollCaptureSampleIfPointerIsInViewport() {
		guard let state = scrollCaptureState else {
			return
		}
		guard state.viewportRect.contains(NSEvent.mouseLocation) else {
			return
		}
		scheduleNativeScrollCaptureSample()
	}

	private func scheduleNativeScrollCaptureSample() {
		guard var state = scrollCaptureState else {
			return
		}
		state.sampleGeneration &+= 1
		let generation = state.sampleGeneration
		scrollCaptureState = state
		DispatchQueue.main.asyncAfter(deadline: .now() + Self.scrollCaptureSampleDelay) {
			[weak self] in
			self?.observeNativeScrollCaptureFrame(generation: generation)
		}
	}

	private func beginNativeScrollCapture() throws {
		guard Self.scrollCaptureEnabled else {
			try setHostStatusMessage("Scroll capture is temporarily disabled.")
			refreshOverlay()
			return
		}
		guard scrollCaptureState == nil else {
			try setHostStatusMessage("Scroll capture is already active.")
			refreshOverlay()
			return
		}
		guard scene.mode == .frozen, let selection = currentFrozenSelection() else {
			try setHostStatusMessage("Scroll capture requires a frozen selection.")
			refreshOverlay()
			return
		}
		guard chromeState.frozenSelectionEditable else {
			try setHostStatusMessage("Scroll capture requires a dragged region selection.")
			refreshOverlay()
			return
		}

		ensureFrozenBaseImageFromDisplayIfNeeded(for: selection)
		let baseImage = chromeState.frozenBaseImage ?? frozenBaseImageFromDisplay(for: selection)
		guard let baseImage, let baseSnapshot = NativeHostImageBridge.rgbaSnapshot(from: baseImage)
		else {
			try setHostStatusMessage("Scroll capture could not read the selected region.")
			refreshOverlay()
			return
		}

		let stitcher = try RsnapScrollCaptureSession(
			baseImage: baseSnapshot,
			previewWidthPixels: baseSnapshot.width
		)
		scrollCaptureState = NativeScrollCaptureState(
			stitcher: stitcher,
			viewportRect: selection
		)
		installNativeScrollCaptureMonitor()
		overlayController?.setScrollCaptureMousePassthroughActive(true)
		chromeState.frozenOverlay.reset()
		chromeState.frozenSelectionEditable = false
		chromeState.frozenSelectionInteraction = nil
		chromeState.frozenSelectionSnapshot = selection
		chromeState.captureFrameSource = .scrollCapture
		chromeState.captureFrameWindowID = nil
		chromeState.frozenDisplayFrame = nil
		chromeState.frozenDisplayImage = nil
		chromeState.frozenBaseImage = baseImage
		chromeState.scrollMinimapPreview = ScrollCaptureMinimapSnapshot(
			image: baseImage,
			exportSizePixels: CGSize(
				width: CGFloat(baseSnapshot.width),
				height: CGFloat(baseSnapshot.height)
			),
			viewportTopYPixels: 0,
			viewportHeightPixels: CGFloat(baseSnapshot.height)
		)
		try setHostStatusMessage(
			"Scroll capture started. Scroll inside the selection, then copy or save.")
		refreshOverlay()
	}

	private func observeNativeScrollCaptureFrame(generation: UInt64) {
		guard let state = scrollCaptureState, generation <= state.sampleGeneration else {
			return
		}
		guard
			let sampleImage = overlayController?.backgroundPatch(in: state.viewportRect),
			let sample = NativeHostImageBridge.rgbaSnapshot(from: sampleImage)
		else {
			try? setHostStatusMessage("Scroll capture could not sample the scrolled region.")
			refreshOverlay()
			return
		}

		do {
			let result = try state.stitcher.observeDownwardFrame(sample)
			try refreshNativeScrollCapturePreview(
				result: result,
				currentViewportSnapshot: sample
			)
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.scroll_observe_failed",
				captureID: currentCaptureTelemetryID,
				stage: "observe_frame",
				error: String(describing: error)
			)
			try? setHostStatusMessage("Scroll capture could not stitch that frame.")
			refreshOverlay()
		}
	}

	private func refreshNativeScrollCapturePreview(
		result: ScrollObserveResult,
		currentViewportSnapshot: RGBARegionSnapshot
	) throws {
		guard let state = scrollCaptureState else {
			return
		}
		guard
			let export = try state.stitcher.exportImage(),
			let exportImage = NativeHostImageBridge.cgImage(from: export)
		else {
			try setHostStatusMessage("Scroll capture could not render the stitched image.")
			refreshOverlay()
			return
		}

		chromeState.frozenSelectionSnapshot = state.viewportRect
		chromeState.frozenSelectionEditable = false
		chromeState.frozenSelectionInteraction = nil
		chromeState.frozenDisplayFrame = nil
		chromeState.frozenDisplayImage = nil
		chromeState.scrollMinimapPreview = ScrollCaptureMinimapSnapshot(
			image: exportImage,
			exportSizePixels: CGSize(width: CGFloat(export.width), height: CGFloat(export.height)),
			viewportTopYPixels: CGFloat(result.currentViewportTopY),
			viewportHeightPixels: CGFloat(currentViewportSnapshot.height)
		)

		if result.outcome == .committed {
			try setHostStatusMessage(
				"Scroll capture appended \(result.growthRows) px. Copy or save exports the stitched image."
			)
		} else if result.outcome == .unsupportedDirection {
			try setHostStatusMessage("Scroll capture only appends downward motion.")
		}
		refreshOverlay()
	}

	private func performCopy() throws {
		guard let session else {
			return
		}
		let copyStartedAt = ProcessInfo.processInfo.systemUptime
		let captureImageStartedAt = ProcessInfo.processInfo.systemUptime
		guard let cgImage = try captureFrozenSelectionImage(applyingCaptureFrameEffect: true)
		else {
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

		let makeImageStartedAt = ProcessInfo.processInfo.systemUptime
		guard let pngData = try Self.losslessPNGData(from: cgImage) else {
			let makeImageMilliseconds = NativeHostTelemetry.milliseconds(since: makeImageStartedAt)
			NativeHostTelemetry.copyCaptureTiming(
				captureID: currentCaptureTelemetryID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: copyStartedAt),
				captureImageMilliseconds: captureImageMilliseconds,
				clearPasteboardMilliseconds: 0,
				makeImageMilliseconds: makeImageMilliseconds,
				writePasteboardMilliseconds: 0,
				success: false,
				failureStage: "encode_image",
				width: cgImage.width,
				height: cgImage.height
			)
			try sendHostStatusMessage("Could not encode the captured image.")
			return
		}
		let makeImageMilliseconds = NativeHostTelemetry.milliseconds(since: makeImageStartedAt)

		let pasteboard = NSPasteboard.general
		let clearPasteboardStartedAt = ProcessInfo.processInfo.systemUptime
		pasteboard.clearContents()
		let clearPasteboardMilliseconds =
			NativeHostTelemetry.milliseconds(since: clearPasteboardStartedAt)
		let writePasteboardStartedAt = ProcessInfo.processInfo.systemUptime
		let pasteboardItem = NSPasteboardItem()
		let didWritePasteboard =
			pasteboardItem.setData(pngData, forType: .png)
			&& pasteboard.writeObjects([pasteboardItem])
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

		captureSuccessSound.play()

		try session.send(report: .hostEffectCompleted(.copyCapture))
		try session.send(report: .statusMessage("Copied capture to clipboard."))
		completedHostEffect = .copyCapture
	}

	private func performSave() throws {
		guard let session else {
			return
		}
		guard let cgImage = try captureFrozenSelectionImage(applyingCaptureFrameEffect: true)
		else {
			try sendHostStatusMessage("Could not capture the frozen selection.")
			return
		}
		guard let pngData = try Self.losslessPNGData(from: cgImage) else {
			try sendHostStatusMessage("Could not encode the captured image.")
			return
		}

		let outputURL = try nextOutputURL()
		try pngData.write(to: outputURL, options: .atomic)

		captureSuccessSound.play()

		try session.send(report: .hostEffectCompleted(.saveCapture))
		try session.send(report: .statusMessage("Saved capture to \(outputURL.lastPathComponent)."))
		completedHostEffect = .saveCapture
	}

	private struct RecognizeTextRun {
		let captureID: UInt64
		let startedAt: TimeInterval
		let recognitionLevel: String
		let usesLanguageCorrection: Bool
		let automaticallyDetectsLanguage: Bool
	}

	private struct RecognizeTextResult {
		let observations: [VNRecognizedTextObservation]
		let recognizedLines: [String]
		let text: String
		let processingMilliseconds: Double
	}

	private struct RecognizeTextPasteboardTiming {
		let clearMilliseconds: Double
		let writeMilliseconds: Double
	}

	private func performRecognizeText() throws {
		guard let session else {
			return
		}
		let run = RecognizeTextRun(
			captureID: currentCaptureTelemetryID,
			startedAt: ProcessInfo.processInfo.systemUptime,
			recognitionLevel: "accurate",
			usesLanguageCorrection: true,
			automaticallyDetectsLanguage: true
		)
		let captureImageStartedAt = ProcessInfo.processInfo.systemUptime
		guard
			let cgImage = try recognizeTextCaptureImage(
				run: run,
				captureImageStartedAt: captureImageStartedAt
			)
		else {
			return
		}
		let captureImageMilliseconds =
			NativeHostTelemetry.milliseconds(since: captureImageStartedAt)
		let request = recognizeTextRequest(run: run)
		let visionRequestMilliseconds = try performRecognizeTextRequest(
			request,
			cgImage: cgImage,
			run: run,
			captureImageMilliseconds: captureImageMilliseconds
		)
		let result = recognizeTextResult(from: request)
		guard
			let pasteboardTiming = try writeRecognizedTextIfNeeded(
				result.text,
				run: run,
				cgImage: cgImage,
				captureImageMilliseconds: captureImageMilliseconds,
				visionRequestMilliseconds: visionRequestMilliseconds,
				result: result
			)
		else {
			return
		}

		recordRecognizeTextTiming(
			run: run,
			captureImageMilliseconds: captureImageMilliseconds,
			visionRequestMilliseconds: visionRequestMilliseconds,
			resultProcessingMilliseconds: result.processingMilliseconds,
			clearPasteboardMilliseconds: pasteboardTiming.clearMilliseconds,
			writePasteboardMilliseconds: pasteboardTiming.writeMilliseconds,
			success: true,
			outcome: result.text.isEmpty ? "no_text" : "text_ready",
			failureStage: "none",
			width: cgImage.width,
			height: cgImage.height,
			observationCount: result.observations.count,
			recognizedLines: result.recognizedLines.count,
			recognizedCharacters: result.text.count
		)

		if result.text.isEmpty == false {
			ocrCompletionSound.play()
		}

		try session.send(report: .hostEffectCompleted(.recognizeText))
		let message =
			result.text.isEmpty
			? "No text was recognized."
			: "Recognized text copied to clipboard."
		try session.send(report: .statusMessage(message))
		completedHostEffect = .recognizeText
	}

	private func recognizeTextCaptureImage(
		run: RecognizeTextRun,
		captureImageStartedAt: TimeInterval
	) throws -> CGImage? {
		guard let cgImage = try captureFrozenSelectionImage() else {
			recordRecognizeTextTiming(
				run: run,
				captureImageMilliseconds: NativeHostTelemetry.milliseconds(
					since: captureImageStartedAt),
				visionRequestMilliseconds: 0,
				resultProcessingMilliseconds: 0,
				clearPasteboardMilliseconds: 0,
				writePasteboardMilliseconds: 0,
				success: false,
				outcome: "recognize_error",
				failureStage: "capture_image",
				width: 0,
				height: 0,
				observationCount: 0,
				recognizedLines: 0,
				recognizedCharacters: 0
			)
			try sendHostStatusMessage("Could not capture the frozen selection.")
			return nil
		}
		return cgImage
	}

	private func recognizeTextRequest(run: RecognizeTextRun) -> VNRecognizeTextRequest {
		let request = VNRecognizeTextRequest()
		request.recognitionLevel = .accurate
		request.usesLanguageCorrection = run.usesLanguageCorrection
		request.automaticallyDetectsLanguage = run.automaticallyDetectsLanguage
		return request
	}

	private func performRecognizeTextRequest(
		_ request: VNRecognizeTextRequest,
		cgImage: CGImage,
		run: RecognizeTextRun,
		captureImageMilliseconds: Double
	) throws -> Double {
		let handler = VNImageRequestHandler(cgImage: cgImage)
		let visionStartedAt = ProcessInfo.processInfo.systemUptime
		do {
			try handler.perform([request])
		} catch {
			let visionRequestMilliseconds = NativeHostTelemetry.milliseconds(since: visionStartedAt)
			recordRecognizeTextTiming(
				run: run,
				captureImageMilliseconds: captureImageMilliseconds,
				visionRequestMilliseconds: visionRequestMilliseconds,
				resultProcessingMilliseconds: 0,
				clearPasteboardMilliseconds: 0,
				writePasteboardMilliseconds: 0,
				success: false,
				outcome: "recognize_error",
				failureStage: "vision_request",
				width: cgImage.width,
				height: cgImage.height,
				observationCount: 0,
				recognizedLines: 0,
				recognizedCharacters: 0
			)
			throw error
		}
		return NativeHostTelemetry.milliseconds(since: visionStartedAt)
	}

	private func recognizeTextResult(from request: VNRecognizeTextRequest) -> RecognizeTextResult {
		let resultProcessingStartedAt = ProcessInfo.processInfo.systemUptime
		let observations = request.results ?? []
		let recognizedLines = observations.compactMap { observation -> String? in
			guard let line = observation.topCandidates(1).first?.string,
				line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
			else {
				return nil
			}
			return line
		}
		return RecognizeTextResult(
			observations: observations,
			recognizedLines: recognizedLines,
			text: recognizedLines.joined(separator: "\n"),
			processingMilliseconds: NativeHostTelemetry.milliseconds(
				since: resultProcessingStartedAt)
		)
	}

	private func writeRecognizedTextIfNeeded(
		_ text: String,
		run: RecognizeTextRun,
		cgImage: CGImage,
		captureImageMilliseconds: Double,
		visionRequestMilliseconds: Double,
		result: RecognizeTextResult
	) throws -> RecognizeTextPasteboardTiming? {
		guard text.isEmpty == false else {
			return RecognizeTextPasteboardTiming(clearMilliseconds: 0, writeMilliseconds: 0)
		}
		let pasteboard = NSPasteboard.general
		let clearPasteboardStartedAt = ProcessInfo.processInfo.systemUptime
		pasteboard.clearContents()
		let clearPasteboardMilliseconds =
			NativeHostTelemetry.milliseconds(since: clearPasteboardStartedAt)
		let writePasteboardStartedAt = ProcessInfo.processInfo.systemUptime
		guard pasteboard.setString(text, forType: .string) else {
			let writePasteboardMilliseconds =
				NativeHostTelemetry.milliseconds(since: writePasteboardStartedAt)
			recordRecognizeTextTiming(
				run: run,
				captureImageMilliseconds: captureImageMilliseconds,
				visionRequestMilliseconds: visionRequestMilliseconds,
				resultProcessingMilliseconds: result.processingMilliseconds,
				clearPasteboardMilliseconds: clearPasteboardMilliseconds,
				writePasteboardMilliseconds: writePasteboardMilliseconds,
				success: false,
				outcome: "recognize_error",
				failureStage: "pasteboard_write",
				width: cgImage.width,
				height: cgImage.height,
				observationCount: result.observations.count,
				recognizedLines: result.recognizedLines.count,
				recognizedCharacters: text.count
			)
			try sendHostStatusMessage("Could not copy recognized text.")
			return nil
		}
		return RecognizeTextPasteboardTiming(
			clearMilliseconds: clearPasteboardMilliseconds,
			writeMilliseconds: NativeHostTelemetry.milliseconds(since: writePasteboardStartedAt)
		)
	}

	private func recordRecognizeTextTiming(
		run: RecognizeTextRun,
		captureImageMilliseconds: Double,
		visionRequestMilliseconds: Double,
		resultProcessingMilliseconds: Double,
		clearPasteboardMilliseconds: Double,
		writePasteboardMilliseconds: Double,
		success: Bool,
		outcome: String,
		failureStage: String,
		width: Int,
		height: Int,
		observationCount: Int,
		recognizedLines: Int,
		recognizedCharacters: Int
	) {
		NativeHostTelemetry.recognizeTextTiming(
			captureID: run.captureID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: run.startedAt),
			captureImageMilliseconds: captureImageMilliseconds,
			visionRequestMilliseconds: visionRequestMilliseconds,
			resultProcessingMilliseconds: resultProcessingMilliseconds,
			clearPasteboardMilliseconds: clearPasteboardMilliseconds,
			writePasteboardMilliseconds: writePasteboardMilliseconds,
			success: success,
			outcome: outcome,
			failureStage: failureStage,
			width: width,
			height: height,
			observationCount: observationCount,
			recognizedLines: recognizedLines,
			recognizedCharacters: recognizedCharacters,
			recognitionLevel: run.recognitionLevel,
			languageCorrection: run.usesLanguageCorrection,
			automaticLanguageDetection: run.automaticallyDetectsLanguage
		)
	}

	private func activeScrollCaptureExportImage() throws -> CGImage? {
		guard Self.scrollCaptureEnabled else {
			return nil
		}
		guard let state = scrollCaptureState else {
			return nil
		}
		guard
			let export = try state.stitcher.exportImage(),
			let exportImage = NativeHostImageBridge.cgImage(from: export)
		else {
			return nil
		}
		return exportImage
	}

	private func captureFrozenSelectionImage(applyingCaptureFrameEffect: Bool = false) throws
		-> CGImage?
	{
		let captureStartedAt = ProcessInfo.processInfo.systemUptime
		guard let selection = currentFrozenSelection() else {
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

		if let scrollExport = try activeScrollCaptureExportImage() {
			NativeHostTelemetry.frozenSelectionImageTiming(
				captureID: currentCaptureTelemetryID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
				ensureMilliseconds: 0,
				refreshMilliseconds: 0,
				compositeMilliseconds: 0,
				source: "scroll_capture_export",
				success: true,
				width: scrollExport.width,
				height: scrollExport.height,
				hasOverlayEdits: false
			)
			return scrollExport
		}

		let snapshotMatchedBefore = chromeState.frozenSelectionSnapshot == selection
		let hadBaseImageBefore = chromeState.frozenBaseImage != nil
		let hadFrozenDisplayImageBefore = chromeState.frozenDisplayImage != nil
		let hasOverlayEdits =
			chromeState.frozenOverlay.canUndo || chromeState.frozenOverlay.hasActiveInteraction
		let ensureStartedAt = ProcessInfo.processInfo.systemUptime
		ensureFrozenBaseImageFromDisplayIfNeeded(for: selection)
		let ensureMilliseconds = NativeHostTelemetry.milliseconds(since: ensureStartedAt)
		var refreshedFromFrozenDisplay = false
		var refreshMilliseconds = 0.0
		if chromeState.frozenSelectionSnapshot != selection || chromeState.frozenBaseImage == nil {
			let refreshStartedAt = ProcessInfo.processInfo.systemUptime
			refreshedFromFrozenDisplay = refreshFrozenBaseImageFromDisplay(for: selection)
			refreshMilliseconds = NativeHostTelemetry.milliseconds(since: refreshStartedAt)
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
		let composited = try compositeFrozenOverlay(on: baseImage, selection: selection)
		let result =
			applyingCaptureFrameEffect
			? applyCaptureFrameEffectIfNeeded(
				to: composited,
				selection: selection,
				hasOverlayEdits: hasOverlayEdits
			)
			: composited
		let compositeMilliseconds = NativeHostTelemetry.milliseconds(since: compositeStartedAt)
		let imageSource: String
		if refreshedFromFrozenDisplay {
			imageSource = "frozen_display_refresh"
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

	private func applyCaptureFrameEffectIfNeeded(
		to image: CGImage,
		selection: CGRect,
		hasOverlayEdits: Bool
	) -> CGImage {
		let settings = settingsStore.settings
		guard settings.shouldApplyCaptureFrameEffect(to: chromeState.captureFrameSource) else {
			return image
		}
		let selectionCenter = CGPoint(x: selection.midX, y: selection.midY)
		let screen = screen(containing: selectionCenter)
		if hasOverlayEdits == false,
			chromeState.captureFrameSource == .window,
			let windowImage = captureFrameWindowImage()
		{
			return CaptureFrameEffectRenderer.renderWindowSnapshot(
				image: windowImage,
				background: settings.captureFrameBackground,
				screen: screen
			) ?? image
		}
		return CaptureFrameEffectRenderer.render(
			image: image,
			background: settings.captureFrameBackground,
			screen: screen,
			source: chromeState.captureFrameSource
		) ?? image
	}

	private func captureFrameWindowImage() -> CGImage? {
		guard let windowID = chromeState.captureFrameWindowID else {
			return nil
		}
		guard let createImage = Self.captureFrameWindowListCreateImage else {
			return nil
		}
		return createImage(
			CGRect.null,
			CGWindowListOption.optionIncludingWindow.rawValue,
			windowID,
			CGWindowImageOption.bestResolution.rawValue
		)?
		.takeRetainedValue()
	}

	private typealias CaptureFrameWindowListCreateImage =
		@convention(c) (
			CGRect,
			UInt32,
			CGWindowID,
			UInt32
		) -> Unmanaged<CGImage>?

	nonisolated private static let captureFrameWindowListCreateImage:
		CaptureFrameWindowListCreateImage? = {
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
			return unsafeBitCast(symbol, to: CaptureFrameWindowListCreateImage.self)
		}()

	@discardableResult
	private func refreshFrozenBaseImageFromDisplay(for selection: CGRect) -> Bool {
		// Export must stay tied to the latched frozen display, not the live desktop.
		let baseImage = frozenBaseImageFromDisplay(for: selection)
		chromeState.frozenSelectionSnapshot = selection
		chromeState.frozenBaseImage = baseImage
		return baseImage != nil
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
		guard
			let cropRect = try? RsnapExportEncoder.frozenDisplayCropRect(
				imageWidth: image.width,
				imageHeight: image.height,
				displayFrame: displayFrame,
				selection: selection
			)
		else {
			return nil
		}
		return image.cropping(to: cropRect)
	}

	private static func losslessPNGData(from image: CGImage) throws -> Data? {
		guard let snapshot = NativeHostImageBridge.rgbaSnapshot(from: image) else {
			return nil
		}

		return try RsnapExportEncoder.pngData(from: snapshot)
	}

	private static func postScrollWheelEvent(matching event: NSEvent, at point: CGPoint) -> Bool {
		let deltaX = Int32(event.scrollingDeltaX.rounded())
		let deltaY = Int32(event.scrollingDeltaY.rounded())
		guard deltaX != 0 || deltaY != 0 else {
			return false
		}

		let units: CGScrollEventUnit = event.hasPreciseScrollingDeltas ? .pixel : .line
		let wheelCount: UInt32 = deltaX == 0 ? 1 : 2
		guard
			let source = CGEventSource(stateID: .hidSystemState),
			let scrollEvent = CGEvent(
				scrollWheelEvent2Source: source,
				units: units,
				wheelCount: wheelCount,
				wheel1: deltaY,
				wheel2: deltaX,
				wheel3: 0
			)
		else {
			return false
		}

		scrollEvent.location = point
		scrollEvent.post(tap: .cghidEventTap)
		return true
	}

	private func screen(containing point: CGPoint) -> NSScreen? {
		NSScreen.screens.first(where: { $0.frame.contains(point) })
	}

	private func activeMonitor(at point: CGPoint) -> MonitorSnapshot? {
		guard let screen = screen(containing: point) else {
			return nil
		}
		return MonitorSnapshot(
			id: Self.displayID(for: screen) ?? 0,
			frame: screen.frame,
			scaleFactorX1000: UInt32((screen.backingScaleFactor * 1_000).rounded())
		)
	}

	private static func displayID(for screen: NSScreen) -> CGDirectDisplayID? {
		(screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
			.uint32Value
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
		let rgbSample =
			chromeSample?.rgbSample
			?? frozenFrameAuthority.rgbSample(containing: point)
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

	private func setHostStatusMessage(_ message: String) throws {
		try sendHostStatusMessage(message)
		scene.statusMessage = message
	}

	private func compositeFrozenOverlay(on image: CGImage, selection: CGRect) throws -> CGImage {
		let elements = chromeState.frozenOverlay.exportElements
		guard elements.isEmpty == false else {
			return image
		}

		guard
			let snapshot = NativeHostImageBridge.rgbaSnapshot(from: image),
			let rendered = NativeHostImageBridge.cgImage(
				from: try RsnapExportEncoder.frozenOverlayExportImage(
					from: snapshot,
					selection: selection,
					elements: elements
				))
		else {
			throw HostBridgeError.ffiStatus(
				context: "converting frozen overlay export image",
				code: 4)
		}

		return rendered
	}

	private func drawExportText(
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
			offset: CGSize(width: 0, height: 1 * scale), blur: 4 * scale,
			color: style.color.textShadowColor.cgColor)
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
		sceneDidChange?(scene)
	}

	private func tearDownCapture() {
		let captureID = currentCaptureTelemetryID
		releaseScreenCaptureStreams()
		pendingFrozenCommit = nil
		frozenFrameLatchToken = nil
		frozenSnapshotGeneration &+= 1
		completedHostEffect = nil
		removeNativeScrollCaptureMonitor()
		scrollCaptureState = nil
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

	fileprivate func releaseScreenCaptureStreams(immediate: Bool = false) {
		pendingLiveFrameStreamRelease?.cancel()
		pendingLiveFrameStreamRelease = nil
		let releaseScreenCaptureStreams = { [weak self] in
			guard let self else {
				return
			}
			self.frozenFrameAuthority.stop()
			self.liveFrameStream.stop()
			self.pendingLiveFrameStreamRelease = nil
		}
		if immediate {
			releaseScreenCaptureStreams()
			return
		}
		let workItem = DispatchWorkItem(block: releaseScreenCaptureStreams)
		pendingLiveFrameStreamRelease = workItem
		DispatchQueue.main.asyncAfter(
			deadline: .now() + Self.liveFrameStreamReleaseGrace,
			execute: workItem
		)
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

	private static func autoCenterMarginBalanceShiftPoints(
		contentOriginPx: CGFloat,
		contentSizePx: CGFloat,
		cropSizePx: CGFloat,
		captureSizePoints: CGFloat
	) -> CGFloat {
		RsnapAutoCenterPlanner.marginBalanceShiftPoints(
			contentOriginPixels: contentOriginPx,
			contentSizePixels: contentSizePx,
			cropSizePixels: cropSizePx,
			captureSizePoints: captureSizePoints
		)
	}

	private static func detectAutoCenterContentBounds(in image: CGImage) -> CGRect? {
		guard let snapshot = NativeHostImageBridge.rgbaSnapshot(from: image) else {
			return nil
		}
		return try? RsnapAutoCenterPlanner.contentBounds(in: snapshot)
	}
}

@MainActor
final class CaptureOverlayWindow: NSPanel {
	let hostView: CaptureHostView

	override var canBecomeKey: Bool { true }
	override var canBecomeMain: Bool { false }

	init(
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
		sharingType = .readOnly
		titleVisibility = .hidden
		titlebarAppearsTransparent = true
	}
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
	struct ToolbarMetrics {
		let scale: CGFloat
		let buttonSize: CGFloat
		let itemSpacing: CGFloat
		let horizontalPadding: CGFloat
		let verticalPadding: CGFloat
		let gap: CGFloat
		let annotationStyleRowHeight: CGFloat
		let annotationStyleControlGap: CGFloat
		let annotationSizeButtonWidth: CGFloat
		let annotationSwatchSize: CGFloat
		let annotationSwatchGap: CGFloat
	}

	private static let liquidGlassBodyOpacity: CGFloat = 0.5

	static let hudInnerMarginX: CGFloat = 12
	static let hudInnerMarginY: CGFloat = 8
	static let hudGroupSpacing: CGFloat = 12
	static let hudColorItemSpacing: CGFloat = 6
	static let hudSwatchSize = CGSize(width: 10, height: 10)
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
	static let toolbarGlyphSize: CGFloat = 18
	static let toolbarControlFontSize: CGFloat = 13
	static let toolbarControlCornerRadius: CGFloat = 8
	// Keep the toolbar visually closer to the slim live HUD chrome.
	static let toolbarTargetHeight: CGFloat = 30
	static let toolbarGap: CGFloat = 10
	static let toolbarScreenMargin: CGFloat = 10
	static let scrollMinimapPreferredWidth: CGFloat = 96
	static let scrollMinimapMinimumWidth: CGFloat = 44
	static let scrollMinimapGap: CGFloat = 10
	static let scrollMinimapScreenMargin: CGFloat = 10
	static let scrollMinimapImageInset: CGFloat = 3
	static let scrollMinimapCornerRadius: CGFloat = 9
	static let annotationStyleRowHeight: CGFloat = 24
	static let annotationStyleControlGap: CGFloat = 4
	static let annotationSizeButtonWidth: CGFloat = 20
	static let annotationSwatchSize: CGFloat = 16
	static let annotationSwatchGap: CGFloat = 6
	static let annotationPenPreviewLength: CGFloat = 18
	static let annotationSizePreviewGap: CGFloat = 8
	static let selectionSizeBadgeGap: CGFloat = 8
	static let selectionSizeBadgeInset: CGFloat = 8
	static let selectionSizeBadgeToolbarAvoidance: CGFloat = 4

	static func toolbarMetrics() -> ToolbarMetrics {
		let baseHeight =
			toolbarVerticalPadding * 2
			+ toolbarButtonSize
		let targetHeight = toolbarTargetHeight
		let scale = min(1, targetHeight / max(baseHeight, 1))
		return ToolbarMetrics(
			scale: scale,
			buttonSize: toolbarButtonSize * scale,
			itemSpacing: toolbarItemSpacing * scale,
			horizontalPadding: hudInnerMarginX * scale,
			verticalPadding: toolbarVerticalPadding * scale,
			gap: toolbarGap * scale,
			annotationStyleRowHeight: annotationStyleRowHeight * scale,
			annotationStyleControlGap: annotationStyleControlGap * scale,
			annotationSizeButtonWidth: annotationSizeButtonWidth * scale,
			annotationSwatchSize: annotationSwatchSize * scale,
			annotationSwatchGap: annotationSwatchGap * scale
		)
	}

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
		let saturation = CGFloat(settings.hudTintSaturation.clamped(to: 0...1))
		let brightness = CGFloat(settings.hudTintBrightness.clamped(to: 0...1))
		return NSColor(
			calibratedHue: hue,
			saturation: saturation,
			brightness: brightness,
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
