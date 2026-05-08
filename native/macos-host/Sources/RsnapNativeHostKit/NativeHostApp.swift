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

private struct LiveChromeRefreshTelemetryKey: Equatable {
	let targetHz: Int
	let hudGlassEnabled: Bool
	let hudGlassMode: String
	let liquidGlassStyle: String
	let liquidGlassAvailable: Bool
}

@MainActor private let frozenEffectCIContext = CIContext(options: nil)

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
		if !sound.play() {
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

private let frozenMosaicBlockSizePixels: CGFloat = 10.0

package func frozenExportOverlayPoint(
	_ point: CGPoint,
	selection: CGRect,
	imageSize: CGSize
) -> CGPoint {
	let scaleX = imageSize.width / max(selection.width, 1)
	let scaleY = imageSize.height / max(selection.height, 1)
	return CGPoint(
		x: (point.x - selection.minX) * scaleX,
		y: (point.y - selection.minY) * scaleY
	)
}

package func frozenExportOverlayRect(
	_ rect: CGRect,
	selection: CGRect,
	imageSize: CGSize
) -> CGRect {
	let scaleX = imageSize.width / max(selection.width, 1)
	let scaleY = imageSize.height / max(selection.height, 1)
	return CGRect(
		x: (rect.minX - selection.minX) * scaleX,
		y: (rect.minY - selection.minY) * scaleY,
		width: rect.width * scaleX,
		height: rect.height * scaleY
	)
}

package func frozenExportSourceImageRect(
	_ rect: CGRect,
	selection: CGRect,
	imageSize: CGSize
) -> CGRect {
	let scaleX = imageSize.width / max(selection.width, 1)
	let scaleY = imageSize.height / max(selection.height, 1)
	return CGRect(
		x: (rect.minX - selection.minX) * scaleX,
		y: (selection.maxY - rect.maxY) * scaleY,
		width: rect.width * scaleX,
		height: rect.height * scaleY
	)
}

package func scrollCaptureMinimapFrame(
	for selection: CGRect,
	exportSize: CGSize,
	in bounds: CGRect,
	preferredWidth: CGFloat,
	minimumWidth: CGFloat,
	gap: CGFloat,
	margin: CGFloat
) -> CGRect? {
	guard exportSize.width > 0, exportSize.height > 0, bounds.width > margin * 2,
		bounds.height > margin * 2
	else {
		return nil
	}

	let rightSpace = bounds.maxX - selection.maxX - gap - margin
	let leftSpace = selection.minX - bounds.minX - gap - margin
	let useRight: Bool
	let sideSpace: CGFloat
	if rightSpace >= minimumWidth {
		useRight = true
		sideSpace = rightSpace
	} else if leftSpace >= minimumWidth {
		useRight = false
		sideSpace = leftSpace
	} else {
		useRight = rightSpace >= leftSpace
		sideSpace = max(rightSpace, leftSpace)
	}

	let maxHeight = bounds.height - margin * 2
	let aspectHeightPerWidth = exportSize.height / exportSize.width
	let heightLimitedWidth = maxHeight / max(aspectHeightPerWidth, .leastNonzeroMagnitude)
	let width = min(preferredWidth, sideSpace, heightLimitedWidth)
	guard width >= min(minimumWidth, preferredWidth) * 0.55 else {
		return nil
	}

	let height = width * aspectHeightPerWidth
	let maxY = max(margin, bounds.maxY - margin - height)
	let y = (selection.midY - height / 2).clamped(to: margin...maxY)
	let x = useRight ? selection.maxX + gap : selection.minX - gap - width
	return CGRect(x: x, y: y, width: width, height: height)
}

private func makeFrozenMosaicPatch(from image: CGImage, sourceRect: CGRect) -> CGImage? {
	let imageRect = CGRect(x: 0, y: 0, width: image.width, height: image.height)
	let cropRect = sourceRect.integral.intersection(imageRect)
	guard
		!cropRect.isNull,
		cropRect.width >= 1,
		cropRect.height >= 1
	else {
		return nil
	}

	let pixelWidth = max(1, Int(ceil(cropRect.width / frozenMosaicBlockSizePixels)))
	let pixelHeight = max(1, Int(ceil(cropRect.height / frozenMosaicBlockSizePixels)))
	let bytesPerRow = pixelWidth * 4
	let seedX = Int(floor(cropRect.minX / frozenMosaicBlockSizePixels))
	let seedY = Int(floor(cropRect.minY / frozenMosaicBlockSizePixels))
	guard
		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
		let context = CGContext(
			data: nil,
			width: pixelWidth,
			height: pixelHeight,
			bitsPerComponent: 8,
			bytesPerRow: bytesPerRow,
			space: colorSpace,
			bitmapInfo: CGBitmapInfo.byteOrder32Big
				.union(CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue))
				.rawValue
		)
	else {
		return nil
	}

	if let rawData = context.data {
		let pixels = rawData.assumingMemoryBound(to: UInt8.self)
		for y in 0..<pixelHeight {
			for x in 0..<pixelWidth {
				let offset = y * bytesPerRow + x * 4
				let color = frozenMosaicLightPrivacyColor(
					x: x + seedX,
					y: y + seedY,
					width: pixelWidth,
					height: pixelHeight
				)
				pixels[offset] = color.red
				pixels[offset + 1] = color.green
				pixels[offset + 2] = color.blue
				pixels[offset + 3] = 255
			}
		}
	}
	return context.makeImage()
}

private func frozenMosaicLightPrivacyColor(
	x: Int,
	y: Int,
	width: Int,
	height: Int
) -> (red: UInt8, green: UInt8, blue: UInt8) {
	let hash = frozenMosaicHash(x: x, y: y, width: width, height: height)
	let groupHash = frozenMosaicHash(x: x / 2, y: y / 2, width: width, height: height)
	let base: CGFloat = 0.74 + CGFloat(Int(groupHash & 3)) * 0.035
	let variation = (CGFloat(Int((hash >> 8) & 3)) - 1.5) * 0.012
	let warmth = CGFloat(Int((groupHash >> 3) & 1)) * 0.012
	return (
		frozenMosaicByte(base + variation + warmth),
		frozenMosaicByte(base + variation + warmth * 0.5),
		frozenMosaicByte(base + variation)
	)
}

private func frozenMosaicHash(x: Int, y: Int, width: Int, height: Int) -> UInt32 {
	var hash =
		UInt32(truncatingIfNeeded: x) &* 0x45d9_f3b
		^ UInt32(truncatingIfNeeded: y) &* 0x119d_e1f3
		^ UInt32(truncatingIfNeeded: width) &* 0x27d4_eb2d
		^ UInt32(truncatingIfNeeded: height) &* 0x1656_67b1
	hash ^= hash >> 16
	hash &*= 0x7feb_352d
	hash ^= hash >> 15
	hash &*= 0x846c_a68b
	hash ^= hash >> 16
	return hash
}

private func frozenMosaicByte(_ value: CGFloat) -> UInt8 {
	UInt8((min(max(value, 0), 1) * 255).rounded())
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
		guard !didBootstrap else {
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
		guard !NativePermissions.screenRecordingGranted else {
			permissionRecoveryWindowController.close()
			return false
		}
		if oncePerLaunch {
			guard !didPresentLaunchPermissionOnboarding else {
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
	fileprivate struct FrozenCaptureJobSource: Sendable {
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
		guard !NativePermissions.screenRecordingGranted else {
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

	fileprivate func performFrozenAnnotationStyleAction(_ action: FrozenAnnotationStyleAction) {
		let selectedTool = scene.toolbarItems.first(where: { $0.selected })?.kind ?? .pointer
		guard chromeState.annotationStyle.apply(action, selectedTool: selectedTool) else {
			return
		}
		refreshOverlay()
	}

	fileprivate func performFrozenAnnotationSizeSteps(_ steps: Int) {
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
			if !granted {
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
		guard let baseImage, let baseSnapshot = Self.rgbaSnapshot(from: baseImage) else {
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
			let sample = Self.rgbaSnapshot(from: sampleImage)
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
			let exportImage = Self.cgImage(from: export)
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

	private func performRecognizeText() throws {
		guard let session else {
			return
		}
		let captureID = currentCaptureTelemetryID
		let recognizeStartedAt = ProcessInfo.processInfo.systemUptime
		let captureImageStartedAt = ProcessInfo.processInfo.systemUptime
		let recognitionLevel = "accurate"
		let usesLanguageCorrection = true
		let automaticallyDetectsLanguage = true
		guard let cgImage = try captureFrozenSelectionImage() else {
			NativeHostTelemetry.recognizeTextTiming(
				captureID: captureID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: recognizeStartedAt),
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
				recognizedCharacters: 0,
				recognitionLevel: recognitionLevel,
				languageCorrection: usesLanguageCorrection,
				automaticLanguageDetection: automaticallyDetectsLanguage
			)
			try sendHostStatusMessage("Could not capture the frozen selection.")
			return
		}
		let captureImageMilliseconds =
			NativeHostTelemetry.milliseconds(since: captureImageStartedAt)

		let request = VNRecognizeTextRequest()
		request.recognitionLevel = .accurate
		request.usesLanguageCorrection = usesLanguageCorrection
		request.automaticallyDetectsLanguage = automaticallyDetectsLanguage
		let handler = VNImageRequestHandler(cgImage: cgImage)
		let visionStartedAt = ProcessInfo.processInfo.systemUptime
		do {
			try handler.perform([request])
		} catch {
			NativeHostTelemetry.recognizeTextTiming(
				captureID: captureID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: recognizeStartedAt),
				captureImageMilliseconds: captureImageMilliseconds,
				visionRequestMilliseconds: NativeHostTelemetry.milliseconds(
					since: visionStartedAt),
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
				recognizedCharacters: 0,
				recognitionLevel: recognitionLevel,
				languageCorrection: usesLanguageCorrection,
				automaticLanguageDetection: automaticallyDetectsLanguage
			)
			throw error
		}
		let visionRequestMilliseconds = NativeHostTelemetry.milliseconds(since: visionStartedAt)

		let resultProcessingStartedAt = ProcessInfo.processInfo.systemUptime
		let observations = request.results ?? []
		let recognizedLines =
			observations
			.compactMap { observation -> String? in
				guard let line = observation.topCandidates(1).first?.string,
					!line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
				else {
					return nil
				}
				return line
			}
		let text = recognizedLines.joined(separator: "\n")
		let resultProcessingMilliseconds =
			NativeHostTelemetry.milliseconds(since: resultProcessingStartedAt)

		var clearPasteboardMilliseconds = 0.0
		var writePasteboardMilliseconds = 0.0
		if !text.isEmpty {
			let pasteboard = NSPasteboard.general
			let clearPasteboardStartedAt = ProcessInfo.processInfo.systemUptime
			pasteboard.clearContents()
			clearPasteboardMilliseconds =
				NativeHostTelemetry.milliseconds(since: clearPasteboardStartedAt)
			let writePasteboardStartedAt = ProcessInfo.processInfo.systemUptime
			guard pasteboard.setString(text, forType: .string) else {
				writePasteboardMilliseconds =
					NativeHostTelemetry.milliseconds(since: writePasteboardStartedAt)
				NativeHostTelemetry.recognizeTextTiming(
					captureID: captureID,
					totalMilliseconds: NativeHostTelemetry.milliseconds(
						since: recognizeStartedAt),
					captureImageMilliseconds: captureImageMilliseconds,
					visionRequestMilliseconds: visionRequestMilliseconds,
					resultProcessingMilliseconds: resultProcessingMilliseconds,
					clearPasteboardMilliseconds: clearPasteboardMilliseconds,
					writePasteboardMilliseconds: writePasteboardMilliseconds,
					success: false,
					outcome: "recognize_error",
					failureStage: "pasteboard_write",
					width: cgImage.width,
					height: cgImage.height,
					observationCount: observations.count,
					recognizedLines: recognizedLines.count,
					recognizedCharacters: text.count,
					recognitionLevel: recognitionLevel,
					languageCorrection: usesLanguageCorrection,
					automaticLanguageDetection: automaticallyDetectsLanguage
				)
				try sendHostStatusMessage("Could not copy recognized text.")
				return
			}
			writePasteboardMilliseconds =
				NativeHostTelemetry.milliseconds(since: writePasteboardStartedAt)
		}

		NativeHostTelemetry.recognizeTextTiming(
			captureID: captureID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: recognizeStartedAt),
			captureImageMilliseconds: captureImageMilliseconds,
			visionRequestMilliseconds: visionRequestMilliseconds,
			resultProcessingMilliseconds: resultProcessingMilliseconds,
			clearPasteboardMilliseconds: clearPasteboardMilliseconds,
			writePasteboardMilliseconds: writePasteboardMilliseconds,
			success: true,
			outcome: text.isEmpty ? "no_text" : "text_ready",
			failureStage: "none",
			width: cgImage.width,
			height: cgImage.height,
			observationCount: observations.count,
			recognizedLines: recognizedLines.count,
			recognizedCharacters: text.count,
			recognitionLevel: recognitionLevel,
			languageCorrection: usesLanguageCorrection,
			automaticLanguageDetection: automaticallyDetectsLanguage
		)

		if !text.isEmpty {
			ocrCompletionSound.play()
		}

		try session.send(report: .hostEffectCompleted(.recognizeText))
		let message =
			text.isEmpty
			? "No text was recognized."
			: "Recognized text copied to clipboard."
		try session.send(report: .statusMessage(message))
		completedHostEffect = .recognizeText
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
			let exportImage = Self.cgImage(from: export)
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
			chromeState.frozenOverlay.canUndo || chromeState.frozenOverlay.activeInteraction != nil
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
		let composited = compositeFrozenOverlay(on: baseImage, selection: selection) ?? baseImage
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
		if !hasOverlayEdits,
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

	private static func rgbaSnapshot(from image: CGImage) -> RGBARegionSnapshot? {
		let width = image.width
		let height = image.height
		guard width > 0, height > 0 else {
			return nil
		}

		let bytesPerPixel = 4
		let bytesPerRow = width * bytesPerPixel
		var rgba = Data(count: bytesPerRow * height)
		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
		let bitmapInfo =
			CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
		let rendered = rgba.withUnsafeMutableBytes { buffer -> Bool in
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
				return false
			}
			context.interpolationQuality = .none
			context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
			return true
		}
		guard rendered else {
			return nil
		}

		return RGBARegionSnapshot(width: width, height: height, rgba: rgba)
	}

	private static func losslessPNGData(from image: CGImage) throws -> Data? {
		guard let snapshot = rgbaSnapshot(from: image) else {
			return nil
		}

		return try RsnapExportEncoder.pngData(from: snapshot)
	}

	private static func cgImage(from snapshot: RGBARegionSnapshot) -> CGImage? {
		guard snapshot.width > 0, snapshot.height > 0 else {
			return nil
		}
		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
		let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue)
		guard
			let provider = CGDataProvider(data: snapshot.rgba as CFData),
			let image = CGImage(
				width: snapshot.width,
				height: snapshot.height,
				bitsPerComponent: 8,
				bitsPerPixel: 32,
				bytesPerRow: snapshot.width * 4,
				space: colorSpace,
				bitmapInfo: bitmapInfo,
				provider: provider,
				decode: nil,
				shouldInterpolate: false,
				intent: .defaultIntent
			)
		else {
			return nil
		}

		return image
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
		let imageSize = CGSize(width: CGFloat(width), height: CGFloat(height))
		let scaleX = imageSize.width / max(selection.width, 1)
		let scaleY = imageSize.height / max(selection.height, 1)
		context.draw(image, in: imageRect)

		func mapPoint(_ point: CGPoint) -> CGPoint {
			frozenExportOverlayPoint(
				point,
				selection: selection,
				imageSize: imageSize
			)
		}
		func mapRect(_ rect: CGRect) -> CGRect {
			frozenExportOverlayRect(
				rect,
				selection: selection,
				imageSize: imageSize
			)
		}
		func sourceImageRect(_ rect: CGRect) -> CGRect {
			frozenExportSourceImageRect(
				rect,
				selection: selection,
				imageSize: imageSize
			)
		}

		let mosaicRects = chromeState.frozenOverlay.mosaicRects.map {
			(source: sourceImageRect($0), destination: mapRect($0))
		}
		if !mosaicRects.isEmpty {
			context.saveGState()
			context.interpolationQuality = .high
			for rect in mosaicRects {
				if let mosaicPatch = makeFrozenMosaicPatch(from: image, sourceRect: rect.source) {
					context.draw(mosaicPatch, in: rect.destination.integral.intersection(imageRect))
				}
			}
			context.restoreGState()
		}

		let spotlightAnnotations = chromeState.frozenOverlay.spotlightAnnotations.map {
			(rect: mapRect($0.rect), style: $0.style)
		}
		let averageScale = (scaleX + scaleY) / 2
		if !spotlightAnnotations.isEmpty {
			context.saveGState()
			context.setFillColor(NSColor.black.withAlphaComponent(0.32).cgColor)
			context.fill(imageRect)
			for annotation in spotlightAnnotations {
				context.saveGState()
				context.clip(to: annotation.rect)
				context.draw(image, in: imageRect)
				context.restoreGState()
			}
			context.restoreGState()

			for annotation in spotlightAnnotations {
				drawFrozenSpotlightBorder(
					for: annotation.rect,
					style: annotation.style,
					scale: averageScale,
					alpha: 0.96,
					in: context
				)
			}
		}

		for stroke in chromeState.frozenOverlay.penStrokes {
			guard let first = stroke.points.first else {
				continue
			}
			context.setStrokeColor(stroke.style.color.nsColor(alpha: 0.96).cgColor)
			context.setLineWidth(stroke.style.strokeWidthPoints * averageScale)
			context.setLineCap(.round)
			context.setLineJoin(.round)
			context.beginPath()
			context.move(to: mapPoint(first))
			for point in stroke.points.dropFirst() {
				context.addLine(to: mapPoint(point))
			}
			context.strokePath()
		}
		for annotation in chromeState.frozenOverlay.arrowAnnotations {
			drawFrozenArrow(
				from: mapPoint(annotation.start),
				to: mapPoint(annotation.end),
				style: annotation.style,
				scale: averageScale,
				in: context
			)
		}
		for annotation in chromeState.frozenOverlay.textAnnotations {
			drawExportText(
				annotation.text,
				at: mapPoint(annotation.anchor),
				style: annotation.style,
				scale: averageScale,
				in: context
			)
		}

		return context.makeImage()
	}

	private func drawExportText(
		_ text: String,
		at point: CGPoint,
		style: FrozenTextStyle,
		scale: CGFloat,
		in context: CGContext
	) {
		guard !text.isEmpty else {
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
		guard cropSizePx > 0, captureSizePoints > 0 else {
			return 0
		}
		let leadingMarginPx = contentOriginPx
		let trailingMarginPx = cropSizePx - (contentOriginPx + contentSizePx)
		let deltaPx = (leadingMarginPx - trailingMarginPx) * 0.5
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

	fileprivate var selfCaptureExceptionWindowIDs: Set<CGWindowID> {
		Set(windows.map { CGWindowID($0.windowNumber) })
	}

	fileprivate func show(
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
		windowSnapshotFeed.start(
			desktopFrame: Self.desktopFrame, initialSnapshots: initialWindowSnapshots)
		let captureID = controller?.activeTelemetryCaptureID ?? 0
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

	fileprivate func prepareCaptureStreamsNow(trigger: String) {
		guard let prepareCaptureStreams = pendingCaptureStreamPreparation else {
			return
		}
		pendingCaptureStreamPreparation = nil
		NativeHostTelemetry.captureEvent(
			"capture.stream_prepare_started",
			captureID: controller?.activeTelemetryCaptureID ?? 0,
			detail: "trigger=\(trigger)"
		)
		prepareCaptureStreams()
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

	fileprivate func markLivePrimaryInteractionReleased(at point: CGPoint) {
		if let owner = livePrimaryInteractionOwner, owner.hasLivePrimaryInteraction {
			owner.markLivePrimaryInteractionReleased(at: point)
			return
		}
		for window in windows where window.hostView.hasLivePrimaryInteraction {
			window.hostView.markLivePrimaryInteractionReleased(at: point)
		}
	}

	fileprivate func registerLivePrimaryInteractionOwner(_ owner: CaptureHostView) {
		livePrimaryInteractionOwner = owner
	}

	fileprivate func completeLivePrimaryInteraction(from sender: CaptureHostView, at point: CGPoint)
	{
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

	fileprivate func presentFrozenFirstFrame(
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
		targetWindow.hostView.refreshLivePresentationNow()
		targetWindow.displayIfNeeded()
	}

	func withPrimaryMousePassthrough<T>(duration: TimeInterval, perform: () -> T) -> T {
		guard let window = primaryWindow as? CaptureOverlayWindow else {
			return perform()
		}
		let previousIgnoresMouseEvents = window.ignoresMouseEvents
		window.ignoresMouseEvents = true
		let result = perform()
		DispatchQueue.main.asyncAfter(deadline: .now() + duration) { [weak window] in
			window?.ignoresMouseEvents = previousIgnoresMouseEvents
		}
		return result
	}

	func setScrollCaptureMousePassthroughActive(_ active: Bool) {
		for window in windows {
			window.ignoresMouseEvents = active
		}
	}

	func close() {
		pendingCaptureStreamPreparation = nil
		windowSnapshotFeed.stop()
		chromeSampleFeed.stop()
		liveChromeBackdrops.hideAll()
		guard !windows.isEmpty else {
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

	fileprivate func immediateLiveChromeSample(
		point: CGPoint,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		let samplePixels = includeLoupePatch ? settings.loupeSampleSize.sidePixels : 1
		return liveFrameStream.sample(at: point, sidePixels: samplePixels)
			?? chromeSampleFeed.snapshot(for: point)
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
		(screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
			.uint32Value
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
		guard !sampleRect.isNull, sampleRect.width > 0, sampleRect.height > 0 else {
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
		guard !sampleRect.isNull, sampleRect.width > 0, sampleRect.height > 0 else {
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
		guard !displayRect.isNull, displayRect.width > 0, displayRect.height > 0 else {
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
		sharingType = .readOnly
		titleVisibility = .hidden
		titlebarAppearsTransparent = true
	}
}

@MainActor
final class CaptureHostView: NSView {
	private static let liveDragIntentThreshold: CGFloat = 3

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
	private var toolbarLiquidGlassView: NSView?
	private var toolbarLiquidGlassContentView: FrozenToolbarRenderView?
	private var frozenToolbarLiquidGlassVisible = false
	private var frozenToolbarLiquidGlassContentDrawn = false
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
		return self
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
		let hostLocalFrozenSelectingEnded =
			previousChrome.hostLocalFrozenSelecting && !chrome.hostLocalFrozenSelecting
		if scene.mode != .frozen {
			frozenFirstDisplayCompletionQueued = false
			frozenFirstDisplayHandoffStartedAt = nil
			frozenFirstDisplayPendingFrameDisplayed = false
			defersFrozenToolbarClassicGlassUntilAfterFirstDisplay = false
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

	fileprivate func refreshLivePresentationNow() {
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

	fileprivate func refreshSampleUpdatedLiveChromeNow() {
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

	fileprivate func installFrozenFirstFrame(
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

	fileprivate func finishFrozenFirstFrameInstall() {
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
		if scene.mode == .frozen {
			refreshHoveredToolbarAction(for: event.locationInWindow)
		}
		let point = globalPoint(from: event)
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
			if !liveDragExceededThreshold,
				liveDragDistance(from: point) >= Self.liveDragIntentThreshold
			{
				liveDragExceededThreshold = true
				logLivePrimaryInputEvent("capture.live_primary_drag_threshold", point: point)
			}
			updateLivePointerPreview(to: point, rendersImmediately: false)
			queuePointerEvent(liveDragExceededThreshold ? .liveDragged(point) : .moved(point))
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
			controller?.beginFrozenInteraction(at: point)
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
			&& !flags.contains(.shift)
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
			let frame = scrollCaptureMinimapFrame(
				for: selection,
				exportSize: preview.exportSizePixels,
				in: bounds,
				preferredWidth: CaptureChrome.scrollMinimapPreferredWidth,
				minimumWidth: CaptureChrome.scrollMinimapMinimumWidth,
				gap: CaptureChrome.scrollMinimapGap,
				margin: CaptureChrome.scrollMinimapScreenMargin
			)
		else {
			return
		}

		let theme = chromeTheme()
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let imageFrame = frame.insetBy(
			dx: CaptureChrome.scrollMinimapImageInset,
			dy: CaptureChrome.scrollMinimapImageInset
		)
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

		if let viewportFrame = scrollCaptureMinimapViewportFrame(
			for: preview,
			in: imageFrame
		) {
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

	private func scrollCaptureMinimapViewportFrame(
		for preview: ScrollCaptureMinimapSnapshot,
		in frame: CGRect
	) -> CGRect? {
		let exportHeight = max(preview.exportSizePixels.height, 1)
		let viewportHeight = preview.viewportHeightPixels.clamped(to: 1...exportHeight)
		let maxTop = max(exportHeight - viewportHeight, 0)
		let viewportTop = preview.viewportTopYPixels.clamped(to: 0...maxTop)
		let markerHeight = max(2, frame.height * viewportHeight / exportHeight)
		let markerY =
			frame.maxY - frame.height * (viewportTop + viewportHeight) / exportHeight
		let marker = CGRect(
			x: frame.minX,
			y: markerY,
			width: frame.width,
			height: markerHeight
		)
		let clippedMarker = marker.intersection(frame)
		return clippedMarker.isNull ? nil : clippedMarker
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

	fileprivate func markLivePrimaryInteractionReleased(at point: CGPoint) {
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

	fileprivate var hasLivePrimaryInteraction: Bool {
		scene.mode == .live && liveDragStartGlobal != nil
	}

	fileprivate func completeOwnedLivePrimaryInteraction(at point: CGPoint) {
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

	private func liveDragCompletionPoint(for point: CGPoint) -> CGPoint {
		liveDragExceededThreshold ? point : liveDragStartGlobal ?? point
	}

	private func isPrimaryMouseButtonPressed() -> Bool {
		(NSEvent.pressedMouseButtons & 1) == 1
	}

	fileprivate func clearLivePrimaryInteractionState(rendersImmediately: Bool) {
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
			!livePrimaryCompletionInFlight
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
		if !isPrimaryMouseButtonPressed() {
			let point = NSEvent.mouseLocation
			logLivePrimaryInputEvent("capture.live_primary_release_watchdog", point: point)
			completeLivePrimaryInteractionFromSystemMouseUp(at: point, source: "watchdog")
			return
		}
		scheduleLiveMouseReleaseWatchdog()
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
					if !chrome.frozenSelectionTransformAllowed {
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
		guard !allRects.isEmpty, let baseImage = chrome.frozenBaseImage else {
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
		guard !allAnnotations.isEmpty else {
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
		guard !allStrokes.isEmpty else {
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
		guard !arrows.isEmpty else {
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
		guard !text.isEmpty else {
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
		guard !items.isEmpty else {
			return nil
		}

		var styleKind: FrozenAnnotationStyleToolbarKind?
		for item in items where item.selected {
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
			frozenToolbarLiquidGlassVisible,
			frozenToolbarLiquidGlassContentDrawn,
			let toolbarFrame = toolbarLayout(for: selection)?.frame
		else {
			return nil
		}
		let visibleSelection = selection.intersection(bounds)
		if !visibleSelection.isNull, toolbarFrame.intersects(visibleSelection) {
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
		for originalItem in scene.toolbarItems {
			var item = originalItem
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
					&& !chrome.frozenOverlay.keepsFrozenSelectionFixed
			case .scroll:
				guard controller?.scrollCaptureToolbarEnabled == true else {
					continue
				}
				item.enabled = controller?.scrollCaptureToolbarEnabled ?? false
			default:
				break
			}
			items.append(item)
		}
		return items
	}

	private func toolbarItem(_ kind: ToolbarItemKind) -> ToolbarItem? {
		scene.toolbarItems.first(where: { $0.kind == kind })
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

		if !livePrimaryCompletionInFlight {
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
		return !Self.windowSnapshotsEquivalent(previousPreview, liveHighlightedWindowPreview)
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
		return "\(Int(round(rect.width * scale)))x\(Int(round(rect.height * scale)))"
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
		LiveChromeLiquidGlassBridge.update(activeView, settings: settings)
		if activeView.frame != frame {
			activeView.frame = frame
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
	private func ensureFrozenToolbarContentView(above glassView: NSView) -> FrozenToolbarRenderView
	{
		if let toolbarLiquidGlassContentView {
			toolbarLiquidGlassContentView.layer?.zPosition = Self.frozenToolbarContentZ
			return toolbarLiquidGlassContentView
		}
		let contentView = FrozenToolbarRenderView(frame: .zero)
		configureFrozenToolbarContentView(contentView)
		addSubview(contentView, positioned: .above, relativeTo: glassView)
		toolbarLiquidGlassContentView = contentView
		return contentView
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
		let contentView = ensureFrozenToolbarContentView(above: toolbarLiquidGlassView)
		let frameChanged = contentView.frame != layout.frame
		if contentView.frame != layout.frame {
			contentView.frame = layout.frame
			contentView.needsDisplay = true
		}
		contentView.isHidden = false
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
		frozenToolbarLiquidGlassVisible = true
		frozenToolbarLiquidGlassContentDrawn = true
		if !wasVisible {
			needsDisplay = true
		}
	}

	private func hideFrozenToolbarLiquidGlassView() {
		let wasVisible = frozenToolbarLiquidGlassVisible
		frozenToolbarLiquidGlassVisible = false
		frozenToolbarLiquidGlassContentDrawn = false
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

private enum FrozenAnnotationColor: CaseIterable, Equatable {
	case white
	case yellow
	case green
	case blue
	case red
	case black

	func nsColor(alpha: CGFloat = 1) -> NSColor {
		let color =
			switch self {
			case .white:
				NSColor(srgbRed: 255 / 255, green: 255 / 255, blue: 255 / 255, alpha: 1)
			case .yellow:
				NSColor(srgbRed: 255 / 255, green: 219 / 255, blue: 77 / 255, alpha: 1)
			case .green:
				NSColor(srgbRed: 92 / 255, green: 214 / 255, blue: 149 / 255, alpha: 1)
			case .blue:
				NSColor(srgbRed: 102 / 255, green: 178 / 255, blue: 255 / 255, alpha: 1)
			case .red:
				NSColor(srgbRed: 255 / 255, green: 107 / 255, blue: 107 / 255, alpha: 1)
			case .black:
				NSColor(srgbRed: 24 / 255, green: 24 / 255, blue: 24 / 255, alpha: 1)
			}
		return color.withAlphaComponent(alpha)
	}

	var textShadowColor: NSColor {
		switch self {
		case .black:
			return NSColor.white.withAlphaComponent(0.48)
		case .white, .yellow, .green, .blue, .red:
			return NSColor.black.withAlphaComponent(0.45)
		}
	}
}

private struct FrozenBrushStyle: Equatable {
	private static let defaultStrokeWidth: CGFloat = 3.0
	private static let minStrokeWidth: CGFloat = 1.0
	private static let maxStrokeWidth: CGFloat = 24.0
	private static let strokeWidthStep: CGFloat = 0.25

	var strokeWidthPoints = defaultStrokeWidth
	var color: FrozenAnnotationColor = .blue

	mutating func applySizeSteps(_ steps: Int) -> Bool {
		guard steps != 0 else {
			return false
		}
		let direction = steps.signum()
		var changed = false
		for _ in 0..<abs(steps) {
			changed =
				setStrokeWidth(strokeWidthPoints + CGFloat(direction) * Self.strokeWidthStep)
				|| changed
		}
		return changed
	}

	private mutating func setStrokeWidth(_ value: CGFloat) -> Bool {
		let clamped = value.clamped(to: Self.minStrokeWidth...Self.maxStrokeWidth)
		guard abs(clamped - strokeWidthPoints) > .ulpOfOne else {
			return false
		}
		strokeWidthPoints = clamped
		return true
	}
}

private struct FrozenSpotlightStyle: Equatable {
	private static let defaultBorderWidth: CGFloat = 0.0
	private static let minBorderWidth: CGFloat = 0.0
	private static let maxBorderWidth: CGFloat = 24.0
	private static let borderWidthStep: CGFloat = 0.25

	var borderWidthPoints = defaultBorderWidth
	var borderColor: FrozenAnnotationColor = .blue

	mutating func applySizeSteps(_ steps: Int) -> Bool {
		guard steps != 0 else {
			return false
		}
		let direction = steps.signum()
		var changed = false
		for _ in 0..<abs(steps) {
			changed =
				setBorderWidth(borderWidthPoints + CGFloat(direction) * Self.borderWidthStep)
				|| changed
		}
		return changed
	}

	private mutating func setBorderWidth(_ value: CGFloat) -> Bool {
		let clamped = value.clamped(to: Self.minBorderWidth...Self.maxBorderWidth)
		guard abs(clamped - borderWidthPoints) > .ulpOfOne else {
			return false
		}
		borderWidthPoints = clamped
		return true
	}
}

private struct FrozenTextStyle: Equatable {
	private static let defaultFontSize: CGFloat = 16.0
	private static let minFontSize: CGFloat = 12.0
	private static let maxFontSize: CGFloat = 72.0

	var fontSizePoints = defaultFontSize
	var color: FrozenAnnotationColor = .blue

	mutating func applySizeSteps(_ steps: Int) -> Bool {
		guard steps != 0 else {
			return false
		}
		var nextSize = fontSizePoints
		for _ in 0..<abs(steps) {
			if steps > 0 {
				nextSize =
					abs(nextSize - nextSize.rounded()) <= .ulpOfOne
					? nextSize + 1
					: ceil(nextSize)
			} else {
				nextSize =
					abs(nextSize - nextSize.rounded()) <= .ulpOfOne
					? nextSize - 1
					: floor(nextSize)
			}
		}
		return setFontSize(nextSize)
	}

	private mutating func setFontSize(_ value: CGFloat) -> Bool {
		let clamped = value.clamped(to: Self.minFontSize...Self.maxFontSize)
		guard abs(clamped - fontSizePoints) > .ulpOfOne else {
			return false
		}
		fontSizePoints = clamped
		return true
	}
}

private enum FrozenAnnotationStyleAction: Equatable {
	case decreaseSize
	case increaseSize
	case color(FrozenAnnotationColor)
}

private enum FrozenAnnotationStyleToolbarKind: Equatable {
	case brush
	case spotlight
	case text

	init?(selectedTool: ToolbarItemKind) {
		switch selectedTool {
		case .pen, .arrow:
			self = .brush
		case .spotlight:
			self = .spotlight
		case .text:
			self = .text
		case .pointer, .mosaic, .undo, .redo, .autoCenter, .scroll, .ocr, .copy, .save:
			return nil
		}
	}

	private var baseSizeDisplayWidth: CGFloat {
		switch self {
		case .brush:
			return 84
		case .spotlight:
			return 58
		case .text:
			return 58
		}
	}

	func sizeDisplayWidth(scale: CGFloat) -> CGFloat {
		baseSizeDisplayWidth * scale
	}

	func sizeControlWidth(scale: CGFloat) -> CGFloat {
		sizeDisplayWidth(scale: scale)
			+ CaptureChrome.annotationSizeButtonWidth * scale * 2
	}

	func selectedColor(in state: FrozenAnnotationStyleState) -> FrozenAnnotationColor {
		switch self {
		case .brush:
			return state.brushStyle.color
		case .spotlight:
			return state.spotlightStyle.borderColor
		case .text:
			return state.textStyle.color
		}
	}

	func sizeLabel(in state: FrozenAnnotationStyleState) -> String {
		switch self {
		case .brush:
			return Self.trimmedDecimalLabel(state.brushStyle.strokeWidthPoints)
		case .spotlight:
			return Self.trimmedDecimalLabel(state.spotlightStyle.borderWidthPoints)
		case .text:
			let size = state.textStyle.fontSizePoints
			let text =
				abs(size - size.rounded()) <= .ulpOfOne
				? "\(Int(size.rounded()))"
				: String(format: "%.1f", Double(size))
			return "\(text) pt"
		}
	}

	private static func trimmedDecimalLabel(_ value: CGFloat) -> String {
		var text = String(format: "%.2f", Double(value))
		while text.contains(".") && text.hasSuffix("0") {
			text.removeLast()
		}
		if text.hasSuffix(".") {
			text.removeLast()
		}
		return text
	}
}

private struct FrozenAnnotationStyleState: Equatable {
	var brushStyle = FrozenBrushStyle()
	var spotlightStyle = FrozenSpotlightStyle()
	var textStyle = FrozenTextStyle()

	mutating func apply(
		_ action: FrozenAnnotationStyleAction,
		selectedTool: ToolbarItemKind
	) -> Bool {
		guard let kind = FrozenAnnotationStyleToolbarKind(selectedTool: selectedTool) else {
			return false
		}
		switch (kind, action) {
		case (.brush, .decreaseSize):
			return brushStyle.applySizeSteps(-1)
		case (.brush, .increaseSize):
			return brushStyle.applySizeSteps(1)
		case (.brush, .color(let color)):
			guard brushStyle.color != color else {
				return false
			}
			brushStyle.color = color
			return true
		case (.spotlight, .decreaseSize):
			return spotlightStyle.applySizeSteps(-1)
		case (.spotlight, .increaseSize):
			return spotlightStyle.applySizeSteps(1)
		case (.spotlight, .color(let color)):
			guard spotlightStyle.borderColor != color else {
				return false
			}
			spotlightStyle.borderColor = color
			return true
		case (.text, .decreaseSize):
			return textStyle.applySizeSteps(-1)
		case (.text, .increaseSize):
			return textStyle.applySizeSteps(1)
		case (.text, .color(let color)):
			guard textStyle.color != color else {
				return false
			}
			textStyle.color = color
			return true
		}
	}

	mutating func applySizeSteps(_ steps: Int, selectedTool: ToolbarItemKind) -> Bool {
		guard let kind = FrozenAnnotationStyleToolbarKind(selectedTool: selectedTool) else {
			return false
		}
		switch kind {
		case .brush:
			return brushStyle.applySizeSteps(steps)
		case .spotlight:
			return spotlightStyle.applySizeSteps(steps)
		case .text:
			return textStyle.applySizeSteps(steps)
		}
	}
}

private struct FrozenBrushStroke: Equatable {
	var points: [CGPoint]
	var style: FrozenBrushStyle
}

private struct FrozenArrowAnnotation: Equatable {
	var start: CGPoint
	var end: CGPoint
	var style: FrozenBrushStyle
}

private struct FrozenSpotlightAnnotation: Equatable {
	var rect: CGRect
	var style: FrozenSpotlightStyle
}

private struct FrozenTextAnnotation: Equatable {
	var anchor: CGPoint
	var text: String
	var style: FrozenTextStyle
}

private struct FrozenTextEditState {
	var anchor: CGPoint
	var text: String
}

private func drawFrozenSpotlightBorder(
	for rect: CGRect,
	style: FrozenSpotlightStyle,
	scale: CGFloat,
	alpha: CGFloat,
	in context: CGContext
) {
	let lineWidth = style.borderWidthPoints * scale
	guard lineWidth > .ulpOfOne else {
		return
	}
	context.saveGState()
	context.setStrokeColor(style.borderColor.nsColor(alpha: alpha).cgColor)
	context.setLineWidth(lineWidth)
	context.stroke(rect.insetBy(dx: lineWidth / 2, dy: lineWidth / 2))
	context.restoreGState()
}

private func drawFrozenArrow(
	from start: CGPoint,
	to end: CGPoint,
	style: FrozenBrushStyle,
	scale: CGFloat,
	in context: CGContext
) {
	let distance = hypot(end.x - start.x, end.y - start.y)
	guard distance > .ulpOfOne else {
		return
	}
	let strokeWidth = style.strokeWidthPoints * 1.4 * scale
	let headLength = min(max(strokeWidth * 4.2, 16 * scale), distance * 0.9)
	let headSpread: CGFloat = .pi / 7
	let angle = atan2(end.y - start.y, end.x - start.x)
	let direction = CGPoint(x: cos(angle), y: sin(angle))
	let shaftEnd = CGPoint(
		x: end.x - direction.x * headLength * 0.72,
		y: end.y - direction.y * headLength * 0.72
	)
	let left = CGPoint(
		x: end.x - cos(angle - headSpread) * headLength,
		y: end.y - sin(angle - headSpread) * headLength
	)
	let right = CGPoint(
		x: end.x - cos(angle + headSpread) * headLength,
		y: end.y - sin(angle + headSpread) * headLength
	)

	context.saveGState()
	context.setStrokeColor(style.color.nsColor(alpha: 0.96).cgColor)
	context.setLineWidth(strokeWidth)
	context.setLineCap(.round)
	context.setLineJoin(.round)
	context.beginPath()
	context.move(to: start)
	context.addLine(to: shaftEnd)
	context.strokePath()
	context.beginPath()
	context.move(to: end)
	context.addLine(to: left)
	context.move(to: end)
	context.addLine(to: right)
	context.strokePath()
	context.restoreGState()
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
	var captureFrameSource: CaptureFrameSource = .unknown
	var captureFrameWindowID: CGWindowID?
	var scrollMinimapPreview: ScrollCaptureMinimapSnapshot?
	var frozenOverlay = FrozenOverlayState()
	var annotationStyle = FrozenAnnotationStyleState()

	var frozenSelectionTransformAllowed: Bool {
		frozenSelectionEditable && !frozenOverlay.keepsFrozenSelectionFixed
	}

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
		captureFrameSource = .unknown
		captureFrameWindowID = nil
		scrollMinimapPreview = nil
		frozenOverlay.reset()
		annotationStyle = FrozenAnnotationStyleState()
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
		captureFrameSource = .unknown
		captureFrameWindowID = nil
		scrollMinimapPreview = nil
		frozenOverlay.reset()
		annotationStyle = FrozenAnnotationStyleState()
	}
}

private struct NativeScrollCaptureState {
	let stitcher: RsnapScrollCaptureSession
	let viewportRect: CGRect
	var sampleGeneration: UInt64 = 0
}

private struct ScrollCaptureMinimapSnapshot {
	let image: CGImage
	let exportSizePixels: CGSize
	let viewportTopYPixels: CGFloat
	let viewportHeightPixels: CGFloat
}

private struct FrozenOverlayState {
	enum Edit {
		case pen(FrozenBrushStroke)
		case arrow(FrozenArrowAnnotation)
		case mosaic(CGRect)
		case spotlight(FrozenSpotlightAnnotation)
		case text(FrozenTextAnnotation)
	}

	enum ActiveInteraction {
		case pen(points: [CGPoint], style: FrozenBrushStyle)
		case arrow(start: CGPoint, current: CGPoint, style: FrozenBrushStyle)
		case mosaic(anchor: CGPoint, current: CGPoint)
		case mosaicMove(index: Int, currentRect: CGRect, dragOffset: CGSize)
		case textMove(index: Int, currentAnnotation: FrozenTextAnnotation, dragOffset: CGSize)
		case spotlight(anchor: CGPoint, current: CGPoint, style: FrozenSpotlightStyle)
	}

	private enum MoveTarget {
		case mosaic(index: Int, rect: CGRect)
		case text(index: Int, annotation: FrozenTextAnnotation)
	}

	var edits: [Edit] = []
	var redoEdits: [Edit] = []
	var activeInteraction: ActiveInteraction?
	var activeTextEdit: FrozenTextEditState?

	var canUndo: Bool { !edits.isEmpty }
	var canRedo: Bool { !redoEdits.isEmpty }
	var keepsFrozenSelectionFixed: Bool {
		!edits.isEmpty || !redoEdits.isEmpty || activeInteraction != nil || activeTextEdit != nil
	}
	var isMovingMovableAnnotation: Bool {
		switch activeInteraction {
		case .mosaicMove?, .textMove?:
			return true
		case nil, .pen?, .arrow?, .mosaic?, .spotlight?:
			return false
		}
	}

	mutating func reset() {
		edits.removeAll()
		redoEdits.removeAll()
		activeInteraction = nil
		activeTextEdit = nil
	}

	mutating func begin(
		tool: ToolbarItemKind,
		at point: CGPoint,
		selection: CGRect,
		style: FrozenAnnotationStyleState
	) -> Bool {
		guard selection.contains(point) else {
			return false
		}

		switch tool {
		case .pen:
			activeInteraction = .pen(points: [point], style: style.brushStyle)
		case .arrow:
			activeInteraction = .arrow(start: point, current: point, style: style.brushStyle)
		case .mosaic:
			activeInteraction = .mosaic(anchor: point, current: point)
		case .pointer:
			guard let target = Self.moveTarget(in: edits, at: point) else {
				return false
			}
			switch target {
			case .mosaic(let index, let rect):
				activeInteraction = .mosaicMove(
					index: index,
					currentRect: rect,
					dragOffset: CGSize(width: point.x - rect.minX, height: point.y - rect.minY)
				)
			case .text(let index, let annotation):
				activeInteraction = .textMove(
					index: index,
					currentAnnotation: annotation,
					dragOffset: CGSize(
						width: point.x - annotation.anchor.x,
						height: point.y - annotation.anchor.y
					)
				)
			}
		case .spotlight:
			activeInteraction = .spotlight(
				anchor: point,
				current: point,
				style: style.spotlightStyle
			)
		case .text:
			let _ = commitTextEdit(style: style.textStyle)
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
		case .pen(var points, let style):
			let clamped = selection.clamp(point)
			if let lastPoint = points.last,
				hypot(lastPoint.x - clamped.x, lastPoint.y - clamped.y) < 1.5
			{
				return false
			}
			points.append(clamped)
			self.activeInteraction = .pen(points: points, style: style)
		case .arrow(let start, _, let style):
			self.activeInteraction = .arrow(
				start: start, current: selection.clamp(point), style: style)
		case .mosaic(let anchor, _):
			self.activeInteraction = .mosaic(anchor: anchor, current: selection.clamp(point))
		case .mosaicMove(let index, let currentRect, let dragOffset):
			self.activeInteraction = .mosaicMove(
				index: index,
				currentRect: Self.movedMosaicRect(
					rect: currentRect,
					dragOffset: dragOffset,
					point: point,
					selection: selection
				),
				dragOffset: dragOffset
			)
		case .textMove(let index, let currentAnnotation, let dragOffset):
			self.activeInteraction = .textMove(
				index: index,
				currentAnnotation: Self.movedTextAnnotation(
					currentAnnotation,
					dragOffset: dragOffset,
					point: point,
					selection: selection
				),
				dragOffset: dragOffset
			)
		case .spotlight(let anchor, _, let style):
			self.activeInteraction = .spotlight(
				anchor: anchor,
				current: selection.clamp(point),
				style: style
			)
		}

		return true
	}

	mutating func finish(selection: CGRect) -> Bool {
		guard let activeInteraction else {
			return false
		}
		defer { self.activeInteraction = nil }

		var changed = true
		switch activeInteraction {
		case .pen(let points, let style):
			guard points.count >= 2 else {
				return false
			}
			edits.append(.pen(FrozenBrushStroke(points: points, style: style)))
		case .arrow(let start, let current, let style):
			guard hypot(start.x - current.x, start.y - current.y) >= 6 else {
				return false
			}
			edits.append(.arrow(FrozenArrowAnnotation(start: start, end: current, style: style)))
		case .mosaic(let anchor, let current):
			let rect = selection.normalizedRect(anchor: anchor, current: current)
			guard rect.width >= 6, rect.height >= 6 else {
				return false
			}
			edits.append(.mosaic(rect))
		case .mosaicMove(let index, let currentRect, _):
			guard edits.indices.contains(index), case .mosaic(let oldRect) = edits[index] else {
				return false
			}
			if oldRect == currentRect {
				changed = false
			} else {
				edits[index] = .mosaic(currentRect)
			}
		case .textMove(let index, let currentAnnotation, _):
			guard edits.indices.contains(index),
				case .text(let oldAnnotation) = edits[index]
			else {
				return false
			}
			if oldAnnotation == currentAnnotation {
				changed = false
			} else {
				edits[index] = .text(currentAnnotation)
			}
		case .spotlight(let anchor, let current, let style):
			let rect = selection.normalizedRect(anchor: anchor, current: current)
			guard rect.width >= 6, rect.height >= 6 else {
				return false
			}
			edits.append(.spotlight(FrozenSpotlightAnnotation(rect: rect, style: style)))
		}

		if changed {
			redoEdits.removeAll()
		}
		return true
	}

	private static func moveTarget(in edits: [Edit], at point: CGPoint) -> MoveTarget? {
		for index in edits.indices.reversed() {
			switch edits[index] {
			case .mosaic(let rect) where rect.contains(point):
				return .mosaic(index: index, rect: rect)
			case .text(let annotation) where textHitBounds(for: annotation).contains(point):
				return .text(index: index, annotation: annotation)
			case .pen, .arrow, .mosaic, .spotlight, .text:
				continue
			}
		}
		return nil
	}

	private static func mosaicMoveTarget(
		in edits: [Edit],
		at point: CGPoint
	) -> (index: Int, rect: CGRect)? {
		for index in edits.indices.reversed() {
			if case .mosaic(let rect) = edits[index], rect.contains(point) {
				return (index, rect)
			}
		}
		return nil
	}

	func containsMovableAnnotation(at point: CGPoint) -> Bool {
		Self.moveTarget(in: edits, at: point) != nil
	}

	private static func movedMosaicRect(
		rect: CGRect,
		dragOffset: CGSize,
		point: CGPoint,
		selection: CGRect
	) -> CGRect {
		let maxMinX = max(selection.minX, selection.maxX - rect.width)
		let maxMinY = max(selection.minY, selection.maxY - rect.height)
		return CGRect(
			x: min(max(point.x - dragOffset.width, selection.minX), maxMinX),
			y: min(max(point.y - dragOffset.height, selection.minY), maxMinY),
			width: rect.width,
			height: rect.height
		)
	}

	private static func movedTextAnnotation(
		_ annotation: FrozenTextAnnotation,
		dragOffset: CGSize,
		point: CGPoint,
		selection: CGRect
	) -> FrozenTextAnnotation {
		let size = textBounds(for: annotation).size
		let maxAnchorX = max(selection.minX, selection.maxX - size.width)
		let maxAnchorY = max(selection.minY, selection.maxY - size.height)
		let anchor = CGPoint(
			x: min(max(point.x - dragOffset.width, selection.minX), maxAnchorX),
			y: min(max(point.y - dragOffset.height, selection.minY), maxAnchorY)
		)
		return FrozenTextAnnotation(anchor: anchor, text: annotation.text, style: annotation.style)
	}

	private static func textHitBounds(for annotation: FrozenTextAnnotation) -> CGRect {
		textBounds(for: annotation).insetBy(dx: -4, dy: -4)
	}

	private static func textBounds(for annotation: FrozenTextAnnotation) -> CGRect {
		let font = NSFont.systemFont(
			ofSize: max(1, annotation.style.fontSizePoints), weight: .medium)
		let attributed = NSAttributedString(
			string: annotation.text,
			attributes: [.font: font]
		)
		let size = attributed.boundingRect(
			with: CGSize(
				width: CGFloat.greatestFiniteMagnitude,
				height: CGFloat.greatestFiniteMagnitude
			),
			options: [.usesLineFragmentOrigin, .usesFontLeading]
		).size
		return CGRect(
			x: annotation.anchor.x,
			y: annotation.anchor.y,
			width: max(1, ceil(size.width)),
			height: max(ceil(font.ascender - font.descender + font.leading), ceil(size.height))
		)
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

	mutating func commitTextEdit(style: FrozenTextStyle) -> Bool {
		guard let activeTextEdit else {
			return false
		}
		self.activeTextEdit = nil
		let trimmed = activeTextEdit.text.trimmingCharacters(in: .whitespacesAndNewlines)
		guard !trimmed.isEmpty else {
			return false
		}
		edits.append(
			.text(
				FrozenTextAnnotation(
					anchor: activeTextEdit.anchor,
					text: activeTextEdit.text,
					style: style
				)))
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

	var penStrokes: [FrozenBrushStroke] {
		edits.compactMap {
			if case .pen(let stroke) = $0 {
				return stroke
			}
			return nil
		}
	}

	var arrowAnnotations: [FrozenArrowAnnotation] {
		edits.compactMap {
			if case .arrow(let annotation) = $0 {
				return annotation
			}
			return nil
		}
	}

	var mosaicRects: [CGRect] {
		let movingIndex = movingMosaicEditIndex
		return edits.indices.compactMap { index in
			if index == movingIndex {
				return nil
			}
			if case .mosaic(let rect) = edits[index] {
				return rect
			}
			return nil
		}
	}

	var spotlightAnnotations: [FrozenSpotlightAnnotation] {
		edits.compactMap {
			if case .spotlight(let annotation) = $0 {
				return annotation
			}
			return nil
		}
	}

	var textAnnotations: [FrozenTextAnnotation] {
		let movingIndex = movingTextEditIndex
		return edits.indices.compactMap { index in
			if index == movingIndex {
				return nil
			}
			if case .text(let annotation) = edits[index] {
				return annotation
			}
			return nil
		}
	}

	var previewPenStroke: FrozenBrushStroke? {
		if case .pen(let points, let style)? = activeInteraction {
			return FrozenBrushStroke(points: points, style: style)
		}
		return nil
	}

	var previewArrow: FrozenArrowAnnotation? {
		if case .arrow(let start, let current, let style)? = activeInteraction {
			return FrozenArrowAnnotation(start: start, end: current, style: style)
		}
		return nil
	}

	var movingMosaicEditIndex: Int? {
		if case .mosaicMove(let index, _, _)? = activeInteraction {
			return index
		}
		return nil
	}

	var movingTextEditIndex: Int? {
		if case .textMove(let index, _, _)? = activeInteraction {
			return index
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
		if case .mosaicMove(_, let currentRect, _)? = activeInteraction {
			return currentRect
		}
		return nil
	}

	var previewTextAnnotation: FrozenTextAnnotation? {
		if case .textMove(_, let annotation, _)? = activeInteraction {
			return annotation
		}
		return nil
	}

	var previewSpotlightAnnotation: FrozenSpotlightAnnotation? {
		if case .spotlight(let anchor, let current, let style)? = activeInteraction {
			return FrozenSpotlightAnnotation(
				rect: CGRect(
					x: min(anchor.x, current.x),
					y: min(anchor.y, current.y),
					width: abs(current.x - anchor.x),
					height: abs(current.y - anchor.y)
				),
				style: style
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

private struct FrozenAnnotationColorSwatchLayout: Equatable {
	let color: FrozenAnnotationColor
	let frame: CGRect
	let selected: Bool
}

private struct FrozenAnnotationStyleLayout: Equatable {
	let kind: FrozenAnnotationStyleToolbarKind
	let scale: CGFloat
	let frame: CGRect
	let sizeControlFrame: CGRect
	let decreaseFrame: CGRect
	let increaseFrame: CGRect
	let displayFrame: CGRect
	let swatches: [FrozenAnnotationColorSwatchLayout]
}

private struct FrozenToolbarLayout {
	let scale: CGFloat
	let frame: CGRect
	let items: [FrozenToolbarItemLayout]
	let annotationStyle: FrozenAnnotationStyleLayout?
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
