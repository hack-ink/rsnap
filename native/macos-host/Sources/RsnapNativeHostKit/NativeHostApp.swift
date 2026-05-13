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
		sessionController.prepareLaunchCaptureStreams(reason: "launch")
		scheduleLaunchPermissionOnboardingIfNeeded()
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
