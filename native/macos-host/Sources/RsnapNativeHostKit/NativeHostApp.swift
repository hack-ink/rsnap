import AppKit
import CoreImage
import CoreGraphics
import Foundation
import OSLog
import RsnapHostBridge
import Vision

struct LiveChromeSample {
	let rgbSample: RGBSample?
	let loupePatch: CGImage?
}

enum LiveSamplingBudget {
	static let hoverWindowCacheRefreshInterval: TimeInterval = 1.0 / 15.0
}

private let frozenEffectCIContext = CIContext(options: nil)
private let menuBarLogger = Logger(
	subsystem: Bundle.main.bundleIdentifier ?? "ink.hack.rsnap",
	category: "MenuBar"
)

@MainActor
public final class NativeHostApplicationController: NSObject, NSApplicationDelegate {
	private let settingsStore = NativeHostSettingsStore()
	private let globalHotKeys = GlobalHotKeyCenter()
	private var lifecycleActivity: NSObjectProtocol?
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
	private lazy var settingsWindowController = SettingsWindowController(settingsStore: settingsStore)
	private lazy var permissionsWindowController = PermissionsWindowController()

	public func finishLaunching() {
		guard !didBootstrap else {
			return
		}
		didBootstrap = true
		menuBarLogger.info("finishLaunching begin")
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
		sessionController.warmLiveSamplingIfPossible(at: NSEvent.mouseLocation)
		menuBarLogger.info("finishLaunching end statusItemPresent=\(self.statusItem != nil, privacy: .public)")
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
		menuBarLogger.info("created status item buttonPresent=\(item.button != nil, privacy: .public)")
		if let button = item.button {
			if let image = Self.statusItemImage() {
				button.image = image
				button.imagePosition = .imageOnly
				button.imageScaling = .scaleProportionallyDown
				button.title = ""
				menuBarLogger.info("configured status item with image size=\(Int(image.size.width), privacy: .public)x\(Int(image.size.height), privacy: .public)")
			} else {
				button.title = "RS"
				menuBarLogger.info("configured status item with text fallback")
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
		menu.addItem(withTitle: "Settings…", action: #selector(openSettings(_:)), keyEquivalent: ",")
		menu.addItem(withTitle: "Permissions…", action: #selector(openPermissions(_:)), keyEquivalent: "")
		menu.addItem(.separator())
		menu.addItem(withTitle: "Quit", action: #selector(quit(_:)), keyEquivalent: "q")
		menu.items.forEach { $0.target = self }

		item.menu = menu
		statusItem = item
		captureMenuItem = captureItem
		cancelCaptureMenuItem = cancelItem
		menuBarLogger.info("status item installed visible=\(item.isVisible, privacy: .public) hasMenu=\(item.menu != nil, privacy: .public)")
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
	private let settingsStore: NativeHostSettingsStore
	private let liveFrameStream = LiveFrameStreamBroker()
	private var session: RsnapHostSession?
	private var overlayController: CaptureOverlayController?
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

	func warmLiveSamplingIfPossible(at point: CGPoint) {
		guard NativePermissions.status(for: .screenRecording) else {
			return
		}
		liveFrameStream.start(for: NSScreen.screens, prewarmPoint: point)
	}

	func startCapture() {
		NSLog("RsnapNativeHost startCapture requested; sessionActive=\(session != nil)")
		if session != nil {
			overlayController?.focusWindow(at: NSEvent.mouseLocation)
			return
		}
		guard ensureCapturePermissions() else {
			NSLog("RsnapNativeHost startCapture blocked by screen recording permission")
			captureStateDidChange?()
			return
		}

		do {
			warmLiveSamplingIfPossible(at: NSEvent.mouseLocation)
			let session = try RsnapHostSession(configuration: settingsStore.sessionConfiguration)
			self.session = session

			try session.enterLive()
			let initialScene = try session.currentScene()
			self.scene = initialScene

			let overlayController = CaptureOverlayController(
				controller: self,
				liveFrameStream: liveFrameStream
			)
			self.overlayController = overlayController
			overlayController.show(
				initialScene: initialScene,
				chrome: chromeState,
				settings: settingsStore.settings,
				focusPoint: NSEvent.mouseLocation
			)
			(NSApp.delegate as? NativeHostApplicationController)?.window = overlayController.primaryWindow
			sceneDidChange?(initialScene)
			NSLog("RsnapNativeHost overlay shown")

			pointerMoved(to: NSEvent.mouseLocation)
			captureStateDidChange?()
		} catch {
			NSLog("Failed to start native rsnap host: \(error)")
			tearDownCapture()
		}
	}

	private func ensureCapturePermissions() -> Bool {
		let granted = NativePermissions.status(for: .screenRecording)
		NSLog("RsnapNativeHost screen recording preflight=\(granted)")
		guard !granted else {
			return true
		}
		return NativePermissions.request(.screenRecording)
	}

	func backgroundPatch(in rect: CGRect) -> CGImage? {
		overlayController?.backgroundPatch(in: rect)
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

	func updateLiveGlassRequests(_ requests: [GlassPatchRequest]) {
		overlayController?.updateLiveGlassRequests(requests)
	}

	func liveGlassPatches() -> [LiveGlassSurfaceKind: CGImage] {
		overlayController?.liveGlassPatches() ?? [:]
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
			NSLog("Failed to cancel capture: \(error)")
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
			NSLog("Failed to send pointer update: \(error)")
		}
	}

	func beginPrimaryInteraction(at point: CGPoint) {
		guard scene.mode == .live else {
			pointerMoved(to: point)
			return
		}

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
			try session?.send(
				event: .primaryInteractionStarted(
					point: point,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try syncCore()
		} catch {
			NSLog("Failed to begin primary interaction: \(error)")
		}
	}

	func continuePrimaryInteraction(to point: CGPoint) {
		guard scene.mode == .live else {
			pointerMoved(to: point)
			return
		}

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
			try session?.send(
				event: .primaryInteractionUpdated(
					point: point,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try syncCore()
		} catch {
			NSLog("Failed to update primary interaction: \(error)")
		}
	}

	func completePrimaryInteraction(at point: CGPoint) {
		guard scene.mode == .live else {
			pointerMoved(to: point)
			return
		}

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
			try session?.send(
				event: .primaryInteractionCompleted(
					point: point,
					activeMonitor: liveInputs.activeMonitor,
					highlightedWindow: liveInputs.highlightedWindow
				)
			)
			try syncCore()
		} catch {
			NSLog("Failed to freeze selection: \(error)")
		}
	}

	func copySelection() {
		let _ = chromeState.frozenOverlay.commitTextEdit()
		sendFrozenAction(.copyRequested)
	}

	func saveSelection() {
		let _ = chromeState.frozenOverlay.commitTextEdit()
		sendFrozenAction(.saveRequested)
	}

	func recognizeText() {
		let _ = chromeState.frozenOverlay.commitTextEdit()
		sendFrozenAction(.recognizeTextRequested)
	}

	func invokeToolbarItem(_ item: ToolbarItemKind) {
		if item != .text {
			let _ = chromeState.frozenOverlay.commitTextEdit()
		}
		sendFrozenAction(.toolbarItemInvoked(item))
	}

	func beginFrozenInteraction(at point: CGPoint) {
		guard scene.mode == .frozen else {
			pointerMoved(to: point)
			return
		}
		guard let selection = scene.frozenSelection else {
			pointerMoved(to: point)
			return
		}
		let selectedTool = scene.toolbarItems.first(where: { $0.selected })?.kind ?? .pointer
		if chromeState.frozenOverlay.begin(tool: selectedTool, at: point, selection: selection) {
			refreshOverlay()
			return
		}
		pointerMoved(to: point)
	}

	func continueFrozenInteraction(to point: CGPoint) {
		guard scene.mode == .frozen, let selection = scene.frozenSelection else {
			pointerMoved(to: point)
			return
		}
		if chromeState.frozenOverlay.update(to: point, selection: selection) {
			refreshOverlay()
			return
		}
		pointerMoved(to: point)
	}

	func completeFrozenInteraction(at point: CGPoint) {
		guard scene.mode == .frozen, let selection = scene.frozenSelection else {
			pointerMoved(to: point)
			return
		}
		let _ = chromeState.frozenOverlay.update(to: point, selection: selection)
		if chromeState.frozenOverlay.finish(selection: selection) {
			refreshOverlay()
			return
		}
		pointerMoved(to: point)
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
		guard let selection = scene.frozenSelection else {
			return
		}
		if chromeState.frozenOverlay.canUndo || chromeState.frozenOverlay.activeTextEdit != nil {
			return
		}
		if chromeState.frozenSelectionSnapshot != selection || chromeState.frozenBaseImage == nil {
			refreshFrozenCaptureSnapshot(for: selection)
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
			y: selection.minY + deltaY,
			monitorFrame: screen.frame
		)
		guard nextSelection != selection else {
			return
		}

		do {
			refreshFrozenCaptureSnapshot(for: nextSelection)
			try session?.send(report: .freezeSnapshotCommitted(selection: nextSelection))
			try syncCore()
		} catch {
			NSLog("Failed to auto-center frozen selection: \(error)")
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
			NSLog("Failed to toggle loupe: \(error)")
		}
	}

	private func sendFrozenAction(_ event: HostEvent) {
		do {
			try session?.send(event: event)
			try syncCore()
		} catch {
			NSLog("Failed to send frozen action: \(error)")
		}
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
			chromeState.resetFrozenChrome()
		} else if previousMode != .frozen {
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

	private func handle(request: HostRequestKind) throws {
		switch request {
		case .startLiveCapture:
			break
		case .stopLiveCapture:
			tearDownCapture()
		case .requestFreezeSnapshot:
			try commitFrozenSelection()
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
			let granted = NativePermissions.request(.accessibility)
			try session?.send(report: .permissionChanged(.accessibility, granted: granted))
			if !granted {
				try sendHostStatusMessage("Accessibility permission is required.")
			}
		case .requestInputMonitoringPermission:
			let granted = NativePermissions.request(.inputMonitoring)
			try session?.send(report: .permissionChanged(.inputMonitoring, granted: granted))
			if !granted {
				try sendHostStatusMessage("Input monitoring permission is required.")
			}
		}
	}

	private func commitFrozenSelection() throws {
		guard let session else {
			return
		}
		guard let selection = try session.currentScene().frozenSelection else {
			try sendHostStatusMessage("No frozen selection is available.")
			return
		}
		refreshFrozenCaptureSnapshot(for: selection)
		try session.send(report: .freezeSnapshotCommitted(selection: selection))
	}

	private func performCopy() throws {
		guard let session else {
			return
		}
		guard let cgImage = try captureFrozenSelectionImage() else {
			try sendHostStatusMessage("Could not capture the frozen selection.")
			return
		}

		let pasteboard = NSPasteboard.general
		pasteboard.clearContents()
		let image = NSImage(cgImage: cgImage, size: .zero)
		pasteboard.writeObjects([image])

		try session.send(report: .hostEffectCompleted(.copyCapture))
		try session.send(report: .statusMessage("Copied capture to clipboard."))
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

		try session.send(report: .hostEffectCompleted(.saveCapture))
		try session.send(report: .statusMessage("Saved capture to \(outputURL.lastPathComponent)."))
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
		let message = text.isEmpty
			? "No text was recognized."
			: "Recognized text copied to clipboard."
		try session.send(report: .statusMessage(message))
	}

	private func captureFrozenSelectionImage() throws -> CGImage? {
		guard let selection = try session?.currentScene().frozenSelection else {
			return nil
		}

		if chromeState.frozenSelectionSnapshot != selection || chromeState.frozenBaseImage == nil {
			refreshFrozenCaptureSnapshot(for: selection)
		}
		guard let baseImage = chromeState.frozenBaseImage else {
			return nil
		}

		return compositeFrozenOverlay(on: baseImage, selection: selection) ?? baseImage
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
		chromeState.frozenMosaicImage = baseImage.flatMap(Self.makeFrozenMosaicImage)
	}

	private static func makeFrozenMosaicImage(from image: CGImage) -> CGImage? {
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

	private func screen(containing point: CGPoint) -> NSScreen? {
		NSScreen.screens.first(where: { $0.frame.contains(point) })
	}

	private func activeMonitor(at point: CGPoint) -> MonitorSnapshot? {
		guard let screen = screen(containing: point) else {
			return nil
		}
		let screenNumber = (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
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

	private func currentLiveInputs(at point: CGPoint) -> (rgb: RGBSample?, activeMonitor: MonitorSnapshot?, highlightedWindow: WindowSnapshot?) {
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
		if let mosaicImage = chromeState.frozenMosaicImage, !mosaicRects.isEmpty {
			for rect in mosaicRects {
				if let mosaicPatch = mosaicImage.cropping(to: rect.integral.intersection(imageRect)) {
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

			context.setStrokeColor(NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor)
			context.setLineWidth(2 * ((scaleX + scaleY) / 2))
			for rect in spotlightRects {
				context.stroke(rect.insetBy(dx: scaleX, dy: scaleY))
			}
		}

		context.setStrokeColor(NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor)
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

	private func drawExportText(_ text: String, at point: CGPoint, scale: CGFloat, in context: CGContext) {
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
		context.setShadow(offset: CGSize(width: 0, height: 1 * scale), blur: 4 * scale, color: NSColor.black.withAlphaComponent(0.45).cgColor)
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
		try fileManager.createDirectory(at: settings.outputDirectory, withIntermediateDirectories: true)
		switch settings.outputNaming {
		case .timestamp:
			let timestamp = ISO8601DateFormatter().string(from: .init()).replacingOccurrences(of: ":", with: "-")
			return settings.outputDirectory
				.appendingPathComponent("\(settings.outputFilenamePrefix)-\(timestamp)")
				.appendingPathExtension("png")
		case .sequence:
			let existingFiles = try fileManager.contentsOfDirectory(
				at: settings.outputDirectory,
				includingPropertiesForKeys: nil
			)
			let prefix = "\(settings.outputFilenamePrefix)-"
			let nextSequence = existingFiles.compactMap { url -> Int? in
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
				.appendingPathComponent("\(settings.outputFilenamePrefix)-\(String(format: "%04d", nextSequence))")
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

		let edgeStrip = max(1, min(24, Int((CGFloat(min(width, height)) * 0.08).rounded())))
		guard
			let topMean = regionRGBMean(bitmap, x0: 0, x1: width, y0: 0, y1: edgeStrip),
			let bottomMean = regionRGBMean(bitmap, x0: 0, x1: width, y0: height - edgeStrip, y1: height),
			let leftMean = regionRGBMean(bitmap, x0: 0, x1: edgeStrip, y0: 0, y1: height),
			let rightMean = regionRGBMean(bitmap, x0: width - edgeStrip, x1: width, y0: 0, y1: height)
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
							regionRGBMeanDistance(bitmap, x0: 0, x1: width, y0: 0, y1: edgeStrip, mean: topMean),
							regionRGBMeanDistance(bitmap, x0: 0, x1: width, y0: height - edgeStrip, y1: height, mean: bottomMean),
							regionRGBMeanDistance(bitmap, x0: 0, x1: edgeStrip, y0: 0, y1: height, mean: leftMean),
							regionRGBMeanDistance(bitmap, x0: width - edgeStrip, x1: width, y0: 0, y1: height, mean: rightMean)
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
				guard let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else {
					continue
				}
				let distances = [
					rgbDistanceToMean(color, mean: topMean),
					rgbDistanceToMean(color, mean: bottomMean),
					rgbDistanceToMean(color, mean: leftMean),
					rgbDistanceToMean(color, mean: rightMean),
				]
				guard let salientDistance = distances.min(), salientDistance >= CGFloat(threshold) else {
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
		_ bitmap: NSBitmapImageRep,
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
				guard let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else {
					continue
				}
				rTotal += color.redComponent * 255
				gTotal += color.greenComponent * 255
				bTotal += color.blueComponent * 255
				count += 1
			}
		}
		guard count > 0 else {
			return nil
		}
		return [rTotal / count, gTotal / count, bTotal / count]
	}

	private static func regionRGBMeanDistance(
		_ bitmap: NSBitmapImageRep,
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
				guard let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else {
					continue
				}
				total += rgbDistanceToMean(color, mean: mean)
				count += 1
			}
		}
		return count == 0 ? 0 : total / count
	}

	private static func rgbDistanceToMean(_ color: NSColor, mean: [CGFloat]) -> CGFloat {
		abs(color.redComponent * 255 - mean[0]).rounded()
			+ abs(color.greenComponent * 255 - mean[1]).rounded()
			+ abs(color.blueComponent * 255 - mean[2]).rounded()
	}
}

@MainActor
final class CaptureOverlayController {
	private weak var controller: CaptureSessionController?
	private var windows: [CaptureOverlayWindow] = []
	private var retiringWindows: [CaptureOverlayWindow] = []
	private var focusedWindowNumber: Int?
	private let liveFrameStream: LiveFrameStreamBroker
	private lazy var windowSnapshotFeed = WindowSnapshotFeed()
	private lazy var chromeSampleFeed = ChromeSampleFeed(broker: liveFrameStream)
	private let liveChromeWindows = LiveChromeVisualWindowController()

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
		focusPoint: CGPoint
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
			if targetWindow == nil, screen.frame.contains(focusPoint) {
				targetWindow = window
			}
		}

		let focusedWindow = targetWindow ?? windows.first
		for window in windows {
			if window === focusedWindow {
				window.makeKeyAndOrderFront(nil)
				window.makeFirstResponder(window.hostView)
				focusedWindowNumber = window.windowNumber
				(NSApp.delegate as? NativeHostApplicationController)?.window = window
			} else {
				window.orderFrontRegardless()
			}
		}
		liveFrameStream.start(for: NSScreen.screens, prewarmPoint: focusPoint)
		windowSnapshotFeed.start(desktopFrame: Self.desktopFrame)
		chromeSampleFeed.start()
		chromeSampleFeed.prime(point: focusPoint, sidePixels: 1)
	}

	fileprivate func update(
		scene: SceneSnapshot,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) {
		for window in windows {
			window.hostView.update(
				scene: scene,
				chrome: chrome,
				settings: settings
			)
		}
	}

	func focusWindow(at point: CGPoint) {
		guard let targetWindow = windows.first(where: { $0.frame.contains(point) }) ?? windows.first else {
			return
		}
		if focusedWindowNumber == targetWindow.windowNumber, targetWindow.isKeyWindow {
			return
		}

		targetWindow.makeKeyAndOrderFront(nil)
		targetWindow.makeFirstResponder(targetWindow.hostView)
		focusedWindowNumber = targetWindow.windowNumber
		(NSApp.delegate as? NativeHostApplicationController)?.window = targetWindow
	}

	func close() {
		windowSnapshotFeed.stop()
		chromeSampleFeed.stop()
		liveChromeWindows.hideAll()
		guard !windows.isEmpty else {
			focusedWindowNumber = nil
			return
		}

		let windowsToRetire = windows
		windows.removeAll()
		focusedWindowNumber = nil
		(NSApp.delegate as? NativeHostApplicationController)?.window = nil

		for window in windowsToRetire {
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

	fileprivate func updateLivePreviewDemand(
		point: CGPoint?,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) {
		let samplePixels = includeLoupePatch ? settings.loupeSampleSize.sidePixels : 1
		chromeSampleFeed.updateDemand(point: point, sidePixels: samplePixels)
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
			latestSample?.loupePatch.map { $0.width == wantsLoupePatchSide && $0.height == wantsLoupePatchSide }
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

	fileprivate func updateLiveGlassRequests(_ requests: [GlassPatchRequest]) {
		let _ = requests
	}

	fileprivate func liveGlassPatches() -> [LiveGlassSurfaceKind: CGImage] {
		[:]
	}

	fileprivate func updateLiveChromeVisuals(
		_ snapshot: LiveChromeVisualSnapshot?
	) {
		liveChromeWindows.update(snapshot: snapshot, focusedWindowNumber: focusedWindowNumber)
	}

	func captureImageBelowOverlay(in rect: CGRect, near point: CGPoint) -> CGImage? {
		guard let referenceWindow = windows.first(where: { $0.frame.contains(point) }) ?? windows.first else {
			return nil
		}

		let desktopFrame = Self.desktopFrame
		let quartzRect = Self.appKitRectToQuartz(rect, desktopFrame: desktopFrame)

		return CGWindowListCreateImage(
			quartzRect,
			.optionOnScreenBelowWindow,
			CGWindowID(referenceWindow.windowNumber),
			[.boundsIgnoreFraming, .bestResolution]
		)
	}

	private static var desktopFrame: CGRect {
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

	private static func appKitRectToQuartz(_ rect: CGRect, desktopFrame: CGRect) -> CGRect {
		CGRect(
			x: rect.minX,
			y: desktopFrame.maxY - rect.maxY,
			width: rect.width,
			height: rect.height
		)
	}

}

@MainActor
final class CaptureOverlayWindow: NSWindow {
	let hostView: CaptureHostView

	override var canBecomeKey: Bool { true }
	override var canBecomeMain: Bool { true }

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
			styleMask: [.borderless],
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
		backgroundColor = .clear
		collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
		hasShadow = false
		ignoresMouseEvents = false
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
	private enum QueuedPointerEvent {
		case moved(CGPoint)
		case liveDragged(CGPoint)
	}

	private enum GlassSurfaceKind: Hashable {
		case hud
		case loupe
		case status
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
		case resizeNorthEastSouthWest
		case resizeNorthWestSouthEast
		case iBeam
	}

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
	private let hudMaterialView = NSVisualEffectView(frame: .zero)
	private let loupeMaterialView = NSVisualEffectView(frame: .zero)
	private let toolbarMaterialView = NSVisualEffectView(frame: .zero)
	private let statusMaterialView = NSVisualEffectView(frame: .zero)
	private var trackingAreaRef: NSTrackingArea?
	private var hoveredToolbarAction: ToolbarItemKind?
	private var lastCursorPresentation: CursorPresentation?
	private var queuedPointerEvent: QueuedPointerEvent?
	private var queuedPointerWorkItem: DispatchWorkItem?
	private var lastHoverPointerDispatchUptime: TimeInterval = 0
	private var lastDragPointerDispatchUptime: TimeInterval = 0
	private var livePointerPreviewGlobal: CGPoint?
	private var liveHighlightedWindowPreview: WindowSnapshot?
	private var glassPatchCache: [GlassSurfaceKind: GlassPatchCache] = [:]
	private lazy var liveRenderer = LiveOverlayRenderer(hostView: self)
	private var liveRendererInstalled = false

	override var acceptsFirstResponder: Bool { true }

	override init(frame frameRect: NSRect) {
		super.init(frame: frameRect)
		wantsLayer = true
		layerContentsRedrawPolicy = .duringViewResize
		[hudMaterialView, loupeMaterialView, toolbarMaterialView, statusMaterialView].forEach {
			configureChromeMaterialView($0)
			addSubview($0, positioned: .below, relativeTo: nil)
		}
		liveRenderer.install { [weak self] in
			self?.currentLivePreviewSnapshot()
		}
		liveRenderer.onTick = { [weak self] in
			guard let self else {
				return
			}
			self.controller?.updateLiveChromeVisuals(self.currentLiveChromeVisualSnapshot())
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
		self.scene = scene
		self.chrome = chrome
		self.settings = settings
		if scene.mode == .live {
			if livePointerPreviewGlobal == nil {
				livePointerPreviewGlobal = scene.pointer
			}
			if liveHighlightedWindowPreview == nil {
				liveHighlightedWindowPreview = scene.highlightedWindow
			}
		} else {
			livePointerPreviewGlobal = nil
			liveHighlightedWindowPreview = nil
		}
		refreshHoveredToolbarAction()
		let cursorPresentation = currentCursorPresentation()
		if cursorPresentation != lastCursorPresentation {
			lastCursorPresentation = cursorPresentation
			window?.invalidateCursorRects(for: self)
		}
		updateChromeMaterialViews()
		updateLiveRendererState()
		if scene.mode == .live {
			updateLivePreviewDemands()
			liveRenderer.renderNow()
			controller?.updateLiveChromeVisuals(currentLiveChromeVisualSnapshot())
		} else {
			controller?.updateLiveChromeVisuals(nil)
			needsDisplay = true
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
		if scene.mode == .live {
			livePointerPreviewGlobal = scene.pointer
			liveHighlightedWindowPreview = scene.highlightedWindow
		} else {
			livePointerPreviewGlobal = nil
			liveHighlightedWindowPreview = nil
		}
		lastCursorPresentation = currentCursorPresentation()
		updateChromeMaterialViews()
		updateLiveRendererState()
	}

	override func layout() {
		super.layout()
		updateChromeMaterialViews()
		updateLiveRendererState()
		if scene.mode == .live {
			updateLivePreviewDemands()
			controller?.updateLiveChromeVisuals(currentLiveChromeVisualSnapshot())
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
			options: [.activeInKeyWindow, .cursorUpdate, .inVisibleRect, .mouseMoved, .enabledDuringMouseDrag],
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
		refreshHoveredToolbarAction(for: event.locationInWindow)
		let point = globalPoint(from: event)
		updateLivePointerPreview(to: point)
		queuePointerEvent(.moved(point))
	}

	override func mouseDragged(with event: NSEvent) {
		refreshHoveredToolbarAction(for: event.locationInWindow)

		if scene.mode == .live {
			let point = globalPoint(from: event)
			updateLivePointerPreview(to: point)
			queuePointerEvent(.liveDragged(point))
		} else {
			controller?.continueFrozenInteraction(to: globalPoint(from: event))
		}
	}

	override func mouseDown(with event: NSEvent) {
		let localPoint = event.locationInWindow
		let point = globalPoint(from: event)
		switch scene.mode {
		case .hidden:
			break
		case .live:
			updateLivePointerPreview(to: point)
			controller?.beginPrimaryInteraction(at: point)
		case .frozen:
			if let action = toolbarAction(at: localPoint) {
				performToolbarAction(action)
				return
			}
			controller?.beginFrozenInteraction(at: point)
		}
	}

	override func mouseUp(with event: NSEvent) {
		if scene.mode == .live {
			let point = globalPoint(from: event)
			updateLivePointerPreview(to: point)
			controller?.completePrimaryInteraction(at: point)
		} else if scene.mode == .frozen {
			controller?.completeFrozenInteraction(at: globalPoint(from: event))
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
			if scene.mode == .live {
				controller?.completePrimaryInteraction(at: scene.pointer ?? NSEvent.mouseLocation)
			}
		default:
			switch event.charactersIgnoringModifiers?.lowercased() {
			case "a":
				if scene.mode == .frozen {
					controller?.performFrozenAutoCenter()
					return
				}
			case "c":
				controller?.copySelection()
			case "s":
				controller?.saveSelection()
			case "r":
				guard toolbarItem(.ocr)?.enabled == true else {
					return
				}
				controller?.recognizeText()
			default:
				super.keyDown(with: event)
			}
		}
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
			if let selection = localFrozenSelectionRect() {
				drawSelectionScrim(for: selection, in: context, alpha: CaptureChrome.frozenScrimAlpha)
				drawDashedSelectionBorder(
					around: selection,
					in: context,
					lineWidth: CaptureChrome.frozenDashedBorderWidth,
					excludeResizeHandleCorners: true
				)
				drawFrozenResizeHandles(for: selection, in: context)
				drawFrozenOverlays(for: selection, in: context)
				drawSelectionSizeBadge(for: selection, in: context)
				drawFrozenToolbar(for: selection, in: context)
			}
		}

		if scene.mode != .live {
			drawStatusMessage(in: context)
		}
	}

	private func drawHud(in context: CGContext) {
		guard scene.mode == .live, let anchor = localPointer() else {
			return
		}
		let theme = chromeTheme()
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let positionDisplay = currentPositionDisplay(font: font)
		let colorDisplay = currentLiveColorDisplay(for: chrome.rgbSample, font: font)
		let itemSpacing: CGFloat = 8
		let swatchSize = CGSize(width: 10, height: 10)
		let commaSeparator = ","
		let xGroupText = "x=\(positionDisplay.xValueText)"
		let yGroupText = "y=\(positionDisplay.yValueText)"
		let positionHeight = max(
			xGroupText.size(using: font).height,
			yGroupText.size(using: font).height
		)
		let keycapVisible = settings.showAltHintKeycap
		let keycapSize = keycapVisible ? "Tab".size(using: font) : .zero
		let keycapFrame = keycapVisible ? CGSize(width: keycapSize.width + 12, height: keycapSize.height + 4) : .zero
		let contentHeight = max(positionHeight, swatchSize.height, font.pointSize, keycapFrame.height)
		let contentWidth = positionDisplay.xSlotWidth
			+ commaSeparator.size(using: font).width
			+ positionDisplay.ySlotWidth
			+ swatchSize.width
			+ colorDisplay.hexSlotWidth
			+ keycapFrame.width
			+ itemSpacing * (keycapVisible ? 3 : 2)
		let hudFrame = CGRect(
			x: (anchor.x + 14).clamped(to: 6...(bounds.width - contentWidth - CaptureChrome.hudInnerMarginX * 2 - 6)),
			y: (anchor.y + 14).clamped(to: 6...(bounds.height - contentHeight - CaptureChrome.hudInnerMarginY * 2 - 6)),
			width: contentWidth + CaptureChrome.hudInnerMarginX * 2,
			height: contentHeight + CaptureChrome.hudInnerMarginY * 2
		)

		drawPill(in: hudFrame, context: context, theme: theme, strongShadow: true, surfaceKind: .hud)

		var cursorX = hudFrame.minX + CaptureChrome.hudInnerMarginX
		let baselineY = hudFrame.midY - positionHeight / 2
		drawText(xGroupText, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += positionDisplay.xSlotWidth
		drawText(commaSeparator, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += commaSeparator.size(using: font).width
		drawText(yGroupText, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += positionDisplay.ySlotWidth + itemSpacing

		let swatchRect = CGRect(
			x: cursorX,
			y: hudFrame.midY - swatchSize.height / 2,
			width: swatchSize.width,
			height: swatchSize.height
		)
		let swatchColor = chrome.rgbSample.map {
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
					x: keycapRect.midX - keycapSize.width / 2,
					y: keycapRect.midY - keycapSize.height / 2
				),
				color: palette.keycapText,
				font: font
			)
		}
	}

	private func drawStatusMessage(in context: CGContext) {
		guard
			let hostMessage = scene.statusMessage,
			!hostMessage.isEmpty,
			let frame = currentStatusMessageFrame()
		else {
			return
		}

		let theme = chromeTheme()
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let font = NSFont.systemFont(ofSize: 12, weight: .medium)

		drawPill(in: frame, context: context, theme: theme, strongShadow: true, surfaceKind: .status)
		drawText(
			hostMessage,
			at: CGPoint(
				x: frame.minX + CaptureChrome.hudInnerMarginX,
				y: frame.minY + CaptureChrome.hudInnerMarginY - 1
			),
			color: palette.labelText,
			font: font
		)
	}

	private func localPointer() -> CGPoint? {
		guard let globalPoint = livePointerPreviewGlobal ?? scene.pointer else {
			return nil
		}
		return localPoint(from: globalPoint)
	}

	private func updateLivePointerPreview(to globalPoint: CGPoint) {
		guard scene.mode == .live else {
			return
		}
		livePointerPreviewGlobal = globalPoint
		liveHighlightedWindowPreview = controller?.previewHighlightedWindow(at: globalPoint) ?? scene.highlightedWindow
		updateLivePreviewDemands()
		liveRenderer.renderNow()
		controller?.updateLiveChromeVisuals(currentLiveChromeVisualSnapshot())
	}

	private func localFrozenSelectionRect() -> CGRect? {
		localRect(from: scene.frozenSelection)
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
		if scene.mode == .frozen,
			let selection = scene.frozenSelection,
			let pointer = scene.pointer,
			selection.contains(CGPoint(x: pointer.x, y: pointer.y)),
			let selectedModeTool = visibleToolbarItems().first(where: { $0.selected })?.kind,
			[ToolbarItemKind.pen, .arrow, .mosaic, .spotlight].contains(selectedModeTool)
		{
			return .crosshair
		}

		switch scene.cursorIntent {
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
		case .resizeNorthEast, .resizeSouthWest:
			return .resizeNorthEastSouthWest
		case .resizeNorthWest, .resizeSouthEast:
			return .resizeNorthWestSouthEast
		case .text:
			return .iBeam
		}
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
		case .resizeNorthEastSouthWest:
			return ._windowResizeNorthEastSouthWest
		case .resizeNorthWestSouthEast:
			return ._windowResizeNorthWestSouthEast
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
		let centerY = imageRect.minY + floor(CGFloat(patch.height) / 2) * CaptureChrome.loupeCellSize
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
			CGRect(x: bounds.minX, y: bounds.minY, width: bounds.width, height: max(0, focusRect.minY - bounds.minY)),
			CGRect(x: bounds.minX, y: focusRect.minY, width: max(0, focusRect.minX - bounds.minX), height: focusRect.height),
			CGRect(x: focusRect.maxX, y: focusRect.minY, width: max(0, bounds.maxX - focusRect.maxX), height: focusRect.height),
			CGRect(x: bounds.minX, y: focusRect.maxY, width: bounds.width, height: max(0, bounds.maxY - focusRect.maxY)),
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
		context.setStrokeColor(NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 0.45).cgColor)
		context.setLineWidth(2.25)
		path.stroke()
		context.restoreGState()
	}

	private func drawDashedSelectionBorder(
		around rect: CGRect,
		in context: CGContext,
		lineWidth: CGFloat,
		excludeResizeHandleCorners: Bool
	) {
		let outlineColor = NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
		let strokeColor = NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 248 / 255)
		let pixelsPerPoint = window?.screen?.backingScaleFactor ?? 1
		let borderOutset = CaptureChrome.dashedBorderOutset(
			strokeWidth: lineWidth,
			pixelsPerPoint: pixelsPerPoint
		)
		let borderRect = rect.insetBy(dx: -borderOutset, dy: -borderOutset)
		let path = CaptureChrome.dashedBorderPath(
			for: borderRect,
			cornerKeepout: excludeResizeHandleCorners ? CaptureChrome.resizeHandleOuterRadius : 0
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
		let outlineColor = NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 124 / 255)
		let strokeColor = NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 246 / 255)
		let dotColor = NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 252 / 255)

		for point in [rect.origin, CGPoint(x: rect.maxX, y: rect.minY), CGPoint(x: rect.minX, y: rect.maxY), CGPoint(x: rect.maxX, y: rect.maxY)] {
			let center = point
			context.setStrokeColor(outlineColor.cgColor)
			context.setLineWidth(CaptureChrome.resizeHandleStrokeWidth + 0.6)
			context.strokeEllipse(
				in: CGRect(
					x: center.x - CaptureChrome.resizeHandleOuterRadius,
					y: center.y - CaptureChrome.resizeHandleOuterRadius,
					width: CaptureChrome.resizeHandleOuterRadius * 2,
					height: CaptureChrome.resizeHandleOuterRadius * 2
				)
			)
			context.setStrokeColor(strokeColor.cgColor)
			context.setLineWidth(CaptureChrome.resizeHandleStrokeWidth)
			context.strokeEllipse(
				in: CGRect(
					x: center.x - CaptureChrome.resizeHandleOuterRadius,
					y: center.y - CaptureChrome.resizeHandleOuterRadius,
					width: CaptureChrome.resizeHandleOuterRadius * 2,
					height: CaptureChrome.resizeHandleOuterRadius * 2
				)
			)
			context.setFillColor(dotColor.cgColor)
			context.fillEllipse(
				in: CGRect(
					x: center.x - CaptureChrome.resizeHandleCenterDotRadius,
					y: center.y - CaptureChrome.resizeHandleCenterDotRadius,
					width: CaptureChrome.resizeHandleCenterDotRadius * 2,
					height: CaptureChrome.resizeHandleCenterDotRadius * 2
				)
			)
		}
	}

	private func drawSelectionSizeBadge(for rect: CGRect, in context: CGContext) {
		let scale = window?.screen?.backingScaleFactor ?? 1
		let text = "\(Int(round(rect.width * scale)))x\(Int(round(rect.height * scale)))"
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let textSize = text.size(using: font)
		let x = min(rect.maxX - textSize.width, bounds.maxX - 8 - textSize.width)
		let preferredY = rect.maxY + 8
		let y = preferredY + textSize.height <= bounds.maxY - 8
			? preferredY
			: max(bounds.minY + 8, rect.maxY - 8 - textSize.height)
		let anchor = CGPoint(x: x, y: y)

		drawText(text, at: CGPoint(x: anchor.x, y: anchor.y - 1), color: NSColor.black.withAlphaComponent(0.6), font: font)
		drawText(text, at: CGPoint(x: anchor.x - 1, y: anchor.y), color: NSColor.black.withAlphaComponent(0.75), font: font)
		drawText(text, at: CGPoint(x: anchor.x + 1, y: anchor.y), color: NSColor.black.withAlphaComponent(0.75), font: font)
		drawText(text, at: CGPoint(x: anchor.x, y: anchor.y + 1), color: NSColor.black.withAlphaComponent(0.75), font: font)
		drawText(text, at: CGPoint(x: anchor.x, y: anchor.y), color: NSColor.white.withAlphaComponent(0.98), font: font)
	}

	private func drawFrozenToolbar(for selection: CGRect, in context: CGContext) {
		guard let layout = toolbarLayout(for: selection) else {
			return
		}

		drawPill(in: layout.frame, context: context, theme: chromeTheme(), strongShadow: false, surfaceKind: .toolbar)

		for item in layout.items {
		let palette = CaptureChrome.palette(for: chromeTheme(), settings: settings)
			let hovered = item.kind == hoveredToolbarAction && item.enabled
			let selected = item.selected
			if hovered || selected {
				context.setFillColor((selected ? palette.toolbarSelectedBackground : palette.toolbarHoverBackground).cgColor)
				let hoverPath = NSBezierPath(roundedRect: item.frame, xRadius: 8, yRadius: 8)
				hoverPath.fill()
			}

			let symbolColor = item.enabled
				? (selected
					? palette.toolbarSelectedIcon
					: (hovered ? palette.toolbarHoverIcon : palette.toolbarIcon))
				: palette.toolbarDisabledIcon
			drawToolbarGlyph(item.kind, in: item.frame, color: symbolColor, context: context)
		}
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
		guard !allRects.isEmpty, let mosaicImage = chrome.frozenMosaicImage else {
			return
		}

		for rect in allRects {
			let imageRect = CGRect(
				x: ((rect.minX - selection.minX) / max(selection.width, 1)) * CGFloat(mosaicImage.width),
				y: ((rect.minY - selection.minY) / max(selection.height, 1)) * CGFloat(mosaicImage.height),
				width: (rect.width / max(selection.width, 1)) * CGFloat(mosaicImage.width),
				height: (rect.height / max(selection.height, 1)) * CGFloat(mosaicImage.height)
			).integral
			guard let patch = mosaicImage.cropping(to: imageRect) else {
				continue
			}
			context.draw(patch, in: rect)
			context.setStrokeColor(NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.84).cgColor)
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

		context.setStrokeColor(NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.92).cgColor)
		context.setLineWidth(2)
		for rect in allRects {
			context.stroke(rect.insetBy(dx: 1, dy: 1))
		}
	}

	private func drawFrozenPenStrokes(in context: CGContext) {
		let allStrokes = chrome.frozenOverlay.penStrokes
			+ (chrome.frozenOverlay.previewPenStroke.map { [$0] } ?? [])
		guard !allStrokes.isEmpty else {
			return
		}

		context.saveGState()
		context.setStrokeColor(NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor)
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
		let arrows = chrome.frozenOverlay.arrowAnnotations
			+ (chrome.frozenOverlay.previewArrow.map { [$0] } ?? [])
		guard !arrows.isEmpty else {
			return
		}

		context.saveGState()
		context.setStrokeColor(NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor)
		context.setLineWidth(3)
		context.setLineCap(.round)
		context.setLineJoin(.round)
		for (start, end) in arrows {
			guard let localStart = localPoint(from: start), let localEnd = localPoint(from: end) else {
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

	private func drawFrozenText(_ text: String, at point: CGPoint, scale: CGFloat, in context: CGContext) {
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
		context.setShadow(offset: CGSize(width: 0, height: 1), blur: 4, color: NSColor.black.withAlphaComponent(0.45).cgColor)
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
		let width = itemCount * CaptureChrome.toolbarButtonSize
			+ max(0, itemCount - 1) * CaptureChrome.toolbarItemSpacing
			+ CaptureChrome.hudInnerMarginX * 2
		let height = CaptureChrome.toolbarButtonSize + CaptureChrome.toolbarVerticalPadding * 2
		let desiredY = selection.maxY + CaptureChrome.toolbarGap
		let wantsTop = settings.toolbarPlacement == .top
		let placedAbove = wantsTop || desiredY + height > bounds.maxY - CaptureChrome.toolbarScreenMargin
		let y = placedAbove
			? max(bounds.minY + CaptureChrome.toolbarScreenMargin, selection.minY - CaptureChrome.toolbarGap - height)
			: min(bounds.maxY - CaptureChrome.toolbarScreenMargin - height, desiredY)
		let x = (selection.midX - width / 2).clamped(
			to: CaptureChrome.toolbarScreenMargin...(bounds.maxX - CaptureChrome.toolbarScreenMargin - width)
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
		guard scene.mode == .frozen, let selection = localFrozenSelectionRect(), let layout = toolbarLayout(for: selection) else {
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
			let cursorPresentation = currentCursorPresentation()
			if cursorPresentation != lastCursorPresentation {
				lastCursorPresentation = cursorPresentation
			}
			window?.invalidateCursorRects(for: self)
			needsDisplay = true
		}
	}

	private func currentHudPlacement() -> LiveFloatingPlacement? {
		guard scene.mode == .live, let anchor = localPointer() else {
			return nil
		}
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let positionDisplay = currentPositionDisplay(font: font)
		let colorDisplay = currentLiveColorDisplay(for: chrome.rgbSample ?? scene.rgb, font: font)
		let itemSpacing: CGFloat = 8
		let swatchSize = CGSize(width: 10, height: 10)
		let commaSeparator = ","
		let xGroupText = "x=\(positionDisplay.xValueText)"
		let yGroupText = "y=\(positionDisplay.yValueText)"
		let positionHeight = max(
			xGroupText.size(using: font).height,
			yGroupText.size(using: font).height
		)
		let keycapVisible = settings.showAltHintKeycap
		let keycapSize = keycapVisible ? "Tab".size(using: font) : .zero
		let keycapFrame = keycapVisible ? CGSize(width: keycapSize.width + 12, height: keycapSize.height + 4) : .zero
		let contentHeight = max(positionHeight, swatchSize.height, font.pointSize, keycapFrame.height)
		let contentWidth = positionDisplay.xSlotWidth
			+ commaSeparator.size(using: font).width
			+ positionDisplay.ySlotWidth
			+ swatchSize.width
			+ colorDisplay.hexSlotWidth
			+ keycapFrame.width
			+ itemSpacing * (keycapVisible ? 3 : 2)
		let size = CGSize(
			width: contentWidth + CaptureChrome.hudInnerMarginX * 2,
			height: contentHeight + CaptureChrome.hudInnerMarginY * 2
		)
		return liveFloatingPlacement(
			anchor: anchor,
			size: size,
			offsetX: 48,
			offsetY: 24,
			preferBelow: true
		)
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

	private func currentStatusMessageFrame() -> CGRect? {
		guard let hostMessage = scene.statusMessage, !hostMessage.isEmpty else {
			return nil
		}
		let font = NSFont.systemFont(ofSize: 12, weight: .medium)
		let textSize = hostMessage.size(using: font)
		return CGRect(
			x: (bounds.midX - (textSize.width + CaptureChrome.hudInnerMarginX * 2) / 2).rounded(),
			y: 24,
			width: ceil(textSize.width + CaptureChrome.hudInnerMarginX * 2),
			height: ceil(textSize.height + CaptureChrome.hudInnerMarginY * 2)
		)
	}

	private func currentLivePreviewSnapshot() -> LivePreviewSnapshot? {
		guard scene.mode == .live else {
			return nil
		}

		let polledPoint = NSEvent.mouseLocation
		if let currentPreview = livePointerPreviewGlobal {
			if hypot(currentPreview.x - polledPoint.x, currentPreview.y - polledPoint.y) >= 0.5 {
				livePointerPreviewGlobal = polledPoint
				liveHighlightedWindowPreview = controller?.previewHighlightedWindow(at: polledPoint) ?? liveHighlightedWindowPreview
			}
		} else {
			livePointerPreviewGlobal = polledPoint
			liveHighlightedWindowPreview = controller?.previewHighlightedWindow(at: polledPoint) ?? liveHighlightedWindowPreview
		}

		updateLivePreviewDemands()

		let chromeSample = currentLiveChromeSample()
		let rgbSample = chromeSample?.rgbSample
			?? chrome.rgbSample
			?? scene.rgb
		let loupePatch = scene.loupeVisible ? chromeSample?.loupePatch : nil
		let dragSelectionLocal = localRect(from: scene.liveSelectionPreview)
		let hoverSelectionLocal = dragSelectionLocal == nil
			? localRect(from: liveHighlightedWindowPreview?.frame ?? scene.highlightedWindow?.frame)
			: nil
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let positionDisplay = currentPositionDisplay(font: font)
		let colorDisplay = currentLiveColorDisplay(for: rgbSample, font: font)

		return LivePreviewSnapshot(
			bounds: bounds,
			theme: chromeTheme(),
			settings: settings,
			pointerLocal: localPointer(),
			dragSelectionLocal: dragSelectionLocal,
			hoverSelectionLocal: hoverSelectionLocal,
			selectionSizeText: dragSelectionLocal.map(selectionSizeText(for:)),
			hudFrame: nil,
			loupeFrame: nil,
			statusFrame: nil,
			positionDisplay: positionDisplay,
			colorDisplay: colorDisplay,
			rgbSample: rgbSample,
			keycapVisible: settings.showAltHintKeycap,
			statusMessage: scene.statusMessage,
			loupePatch: loupePatch,
			glassPatches: [:]
		)
	}

	private func currentLiveChromeVisualSnapshot() -> LiveChromeVisualSnapshot? {
		guard scene.mode == .live, let sourceWindowNumber = window?.windowNumber else {
			return nil
		}

		let chromeSample = currentLiveChromeSample()
		let rgbSample = chromeSample?.rgbSample
			?? chrome.rgbSample
			?? scene.rgb
		let hudPlacement = currentHudPlacement()
		let hudFrameLocal = hudPlacement?.frame
		let hudFrame = hudFrameLocal.flatMap(globalRect(from:))
		let loupeFrame = scene.loupeVisible
			? hudFrameLocal
				.flatMap {
					currentLoupeFrame(
						hudFrame: $0,
						patch: chromeSample?.loupePatch,
						alignTrailing: hudPlacement?.flippedHorizontally ?? false
					)
				}
				.flatMap(globalRect(from:))
			: nil
		let statusFrame = currentStatusMessageFrame().flatMap(globalRect(from:))
		let theme = chromeTheme()
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let positionDisplay = currentPositionDisplay(font: font)
		let colorDisplay = currentLiveColorDisplay(for: rgbSample, font: font)

		let hudSnapshot = hudFrame.map {
			LiveHudVisualSnapshot(
				sourceWindowNumber: sourceWindowNumber,
				frame: $0,
				theme: theme,
				settings: settings,
				positionDisplay: positionDisplay,
				colorDisplay: colorDisplay,
				rgbSample: rgbSample,
				keycapVisible: settings.showAltHintKeycap
			)
		}
		let loupeSnapshot: LiveLoupeVisualSnapshot? = {
			guard let frame = loupeFrame, let patch = chromeSample?.loupePatch else {
				return nil
			}
			return LiveLoupeVisualSnapshot(
				sourceWindowNumber: sourceWindowNumber,
				frame: frame,
				theme: theme,
				settings: settings,
				patch: patch
			)
		}()
		let statusSnapshot: LiveStatusVisualSnapshot? = {
			guard let frame = statusFrame, let message = scene.statusMessage, !message.isEmpty else {
				return nil
			}
			return LiveStatusVisualSnapshot(
				sourceWindowNumber: sourceWindowNumber,
				frame: frame,
				theme: theme,
				settings: settings,
				message: message
			)
		}()

		return LiveChromeVisualSnapshot(
			hud: hudSnapshot,
			loupe: loupeSnapshot,
			status: statusSnapshot
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
		guard scene.mode == .live else {
			liveRenderer.stop()
			return
		}
		liveRenderer.updateDisplayID(currentDisplayID())
	}

	private func updateLivePreviewDemands() {
		guard scene.mode == .live else {
			controller?.updateLivePreviewDemand(point: nil, settings: settings, includeLoupePatch: false)
			return
		}
		let point = livePointerPreviewGlobal ?? scene.pointer
		controller?.updateLivePreviewDemand(
			point: point,
			settings: settings,
			includeLoupePatch: scene.loupeVisible
		)
	}

	private func currentDisplayID() -> CGDirectDisplayID? {
		(window?.screen?.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
			.uint32Value
	}

	private func currentLiveChromeSample() -> LiveChromeSample? {
		controller?.liveChromeSnapshot(
			point: livePointerPreviewGlobal ?? scene.pointer,
			settings: settings,
			includeLoupePatch: scene.loupeVisible
		)
	}

	private func selectionSizeText(for rect: CGRect) -> String {
		let scale = window?.screen?.backingScaleFactor ?? 1
		return "\(Int(round(rect.width * scale)))x\(Int(round(rect.height * scale)))"
	}

	private func currentPositionDisplay(font: NSFont) -> LivePositionDisplay {
		guard let pointer = livePointerPreviewGlobal ?? scene.pointer else {
			let placeholder = "?"
			let slotWidth = placeholder.size(using: font).width
			return LivePositionDisplay(
				xValueText: placeholder,
				yValueText: placeholder,
				xSlotWidth: slotWidth,
				ySlotWidth: slotWidth
			)
		}
		let screenFrame = window?.screen?.frame ?? .zero
		let maxX = Int(screenFrame.maxX.rounded()) - 1
		let maxY = Int(screenFrame.maxY.rounded()) - 1
		let minX = Int(screenFrame.minX.rounded())
		let minY = Int(screenFrame.minY.rounded())
		let xCandidates = ["x=\(minX)", "x=\(maxX)", "x=\(Int(pointer.x.rounded()))"]
		let yCandidates = ["y=\(minY)", "y=\(maxY)", "y=\(Int(pointer.y.rounded()))"]
		return LivePositionDisplay(
			xValueText: String(Int(pointer.x.rounded())),
			yValueText: String(Int(pointer.y.rounded())),
			xSlotWidth: xCandidates.map { $0.size(using: font).width }.max() ?? 0,
			ySlotWidth: yCandidates.map { $0.size(using: font).width }.max() ?? 0
		)
	}

	private func currentLiveColorDisplay(for sample: RGBSample?, font: NSFont) -> LiveColorDisplay {
		let placeholderHex = "Sampling…"
		let hexText = sample.map { String(format: "#%02X%02X%02X", $0.r, $0.g, $0.b) } ?? placeholderHex
		let hexSlotWidth = hexText.size(using: font).width
		if let sample {
			let componentSlotWidth = "255".size(using: font).width
			return LiveColorDisplay(
				hexText: hexText,
				hexSlotWidth: hexSlotWidth,
				rgbValueDisplay: .sample(
					rText: "\(sample.r)",
					gText: "\(sample.g)",
					bText: "\(sample.b)",
					componentSlotWidth: componentSlotWidth
				)
			)
		}
		return LiveColorDisplay(
			hexText: hexText,
			hexSlotWidth: hexSlotWidth,
			rgbValueDisplay: .placeholder(text: "Preparing color")
		)
	}

	private func liveRGBLayoutWidth(for colorDisplay: LiveColorDisplay, font: NSFont) -> CGFloat {
		let prefixWidth = "RGB(".size(using: font).width
		let commaWidth = ",".size(using: font).width
		let suffixWidth = ")".size(using: font).width
		let sampleWidth: CGFloat
		switch colorDisplay.rgbValueDisplay {
		case let .sample(_, _, _, componentSlotWidth):
			sampleWidth = prefixWidth + componentSlotWidth * 3 + commaWidth * 2 + suffixWidth
		case .placeholder:
			let componentSlotWidth = "255".size(using: font).width
			sampleWidth = prefixWidth + componentSlotWidth * 3 + commaWidth * 2 + suffixWidth
		}
		let placeholderWidth: CGFloat
		switch colorDisplay.rgbValueDisplay {
		case let .placeholder(text):
			placeholderWidth = text.size(using: font).width
		case .sample:
			placeholderWidth = "Preparing color".size(using: font).width
		}
		return max(sampleWidth, placeholderWidth)
	}

	private func formatPositionText() -> String {
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let positionDisplay = currentPositionDisplay(font: font)
		return "x=\(positionDisplay.xValueText), y=\(positionDisplay.yValueText)"
	}

	private func formatRGBText(for sample: RGBSample?) -> (String, String) {
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		let colorDisplay = currentLiveColorDisplay(for: sample, font: font)
		let rgbText: String = {
			switch colorDisplay.rgbValueDisplay {
			case let .sample(rText, gText, bText, _):
				return "RGB(\(rText), \(gText), \(bText))"
			case let .placeholder(text):
				return text
			}
		}()
		return (colorDisplay.hexText, rgbText)
	}

	private func legacyFormatPositionText() -> String {
		guard let pointer = livePointerPreviewGlobal ?? scene.pointer else {
			return "x=?, y=?"
		}
		let screenFrame = window?.screen?.frame ?? .zero
		let maxX = Int(screenFrame.maxX.rounded()) - 1
		let maxY = Int(screenFrame.maxY.rounded()) - 1
		let minX = Int(screenFrame.minX.rounded())
		let minY = Int(screenFrame.minY.rounded())
		let xWidth = max(String(minX).count, String(maxX).count, 1)
		let yWidth = max(String(minY).count, String(maxY).count, 1)
		let x = Int(pointer.x.rounded())
		let y = Int(pointer.y.rounded())
		return String(format: "x=%\(xWidth)d, y=%\(yWidth)d", x, y)
	}

	private func formatRGBText() -> (String, String) {
		formatRGBText(for: chrome.rgbSample)
	}

	private func formatLiveRGBText(for sample: RGBSample?) -> (String, String) {
		guard sample != nil else {
			return ("Sampling…", "Preparing color")
		}
		return formatRGBText(for: sample)
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
		context.saveGState()
		if strongShadow {
			context.setShadow(offset: .zero, blur: 10, color: palette.shadow.cgColor)
		}
		if
			settings.hudGlassEnabled,
			settings.hudBlur > 0.01,
			let clipPath = pillPath.copy() as? NSBezierPath,
			let glassImage = glassPatch(for: surfaceKind, frame: frame)
		{
			clipPath.addClip()
			context.draw(glassImage, in: frame)
		}
		context.setFillColor(palette.bodyFill.cgColor)
		pillPath.fill()
		context.restoreGState()

		context.setStrokeColor(palette.outerStroke.cgColor)
		context.setLineWidth(1)
		pillPath.stroke()
	}

	private func glassPatch(for surfaceKind: GlassSurfaceKind, frame: CGRect) -> CGImage? {
		let now = ProcessInfo.processInfo.systemUptime
		if let cached = glassPatchCache[surfaceKind],
			now - cached.capturedAt < (scene.mode == .live ? 1.0 / 12.0 : 1.0 / 30.0),
			abs(cached.frame.minX - frame.minX) < (scene.mode == .live ? 18 : 1),
			abs(cached.frame.minY - frame.minY) < (scene.mode == .live ? 18 : 1),
			abs(cached.frame.width - frame.width) < 1,
			abs(cached.frame.height - frame.height) < 1
		{
			return cached.image
		}

		guard
			let globalFrame = globalRect(from: frame),
			let patch = controller?.backgroundPatch(in: globalFrame),
			let image = blurredGlassPatch(from: patch)
		else {
			return nil
		}

		glassPatchCache[surfaceKind] = GlassPatchCache(frame: frame, capturedAt: now, image: image)
		return image
	}

	private func blurredGlassPatch(from image: CGImage) -> CGImage? {
		let ciImage = CIImage(cgImage: image)
		let clampedImage = ciImage.clampedToExtent()
		guard let filter = CIFilter(name: "CIGaussianBlur") else {
			return image
		}
		let blurRadius = CGFloat(14 + settings.hudBlur.clamped(to: 0...1) * 32)
		filter.setValue(clampedImage, forKey: kCIInputImageKey)
		filter.setValue(blurRadius, forKey: kCIInputRadiusKey)
		guard let blurredImage = filter.outputImage?.cropped(to: ciImage.extent) else {
			return image
		}
		let colorAdjustedImage: CIImage
		if let colorControls = CIFilter(name: "CIColorControls") {
			colorControls.setValue(blurredImage, forKey: kCIInputImageKey)
			colorControls.setValue(1.18 + settings.hudTint.clamped(to: 0...1) * 0.42, forKey: kCIInputSaturationKey)
			colorControls.setValue(1.04, forKey: kCIInputContrastKey)
			colorControls.setValue(themeBrightnessBias(), forKey: kCIInputBrightnessKey)
			colorAdjustedImage = colorControls.outputImage?.cropped(to: ciImage.extent) ?? blurredImage
		} else {
			colorAdjustedImage = blurredImage
		}
		return frozenEffectCIContext.createCGImage(colorAdjustedImage, from: colorAdjustedImage.extent) ?? image
	}

	private func drawText(_ text: String, at point: CGPoint, color: NSColor, font: NSFont) {
		(text as NSString).draw(at: point, withAttributes: [
			.font: font,
			.foregroundColor: color,
		])
	}

	private func drawToolbarGlyph(
		_ kind: ToolbarItemKind,
		in rect: CGRect,
		color: NSColor,
		context: CGContext
	) {
		context.saveGState()
		context.setStrokeColor(color.cgColor)
		context.setFillColor(color.cgColor)
		context.setLineWidth(1.7)
		context.setLineCap(.round)
		context.setLineJoin(.round)

		let insetRect = rect.insetBy(dx: 5.5, dy: 5.5)
		switch kind {
		case .pointer:
			let path = NSBezierPath()
			path.move(to: CGPoint(x: insetRect.minX, y: insetRect.minY))
			path.line(to: CGPoint(x: insetRect.maxX - 2, y: insetRect.midY - 1))
			path.line(to: CGPoint(x: insetRect.midX + 0.5, y: insetRect.midY + 0.5))
			path.line(to: CGPoint(x: insetRect.maxX, y: insetRect.maxY))
			path.lineWidth = 1.6
			path.stroke()
		case .pen:
			context.move(to: CGPoint(x: insetRect.minX + 1, y: insetRect.minY + 1))
			context.addLine(to: CGPoint(x: insetRect.maxX - 2, y: insetRect.maxY - 2))
			context.strokePath()
			context.fillEllipse(in: CGRect(x: insetRect.maxX - 3.5, y: insetRect.maxY - 3.5, width: 3, height: 3))
		case .arrow:
			drawArrow(
				from: CGPoint(x: insetRect.minX, y: insetRect.minY + 1),
				to: CGPoint(x: insetRect.maxX, y: insetRect.maxY),
				in: context
			)
		case .text:
			let font = NSFont.systemFont(ofSize: 13, weight: .semibold)
			drawText("T", at: CGPoint(x: rect.midX - 4, y: rect.midY - 7), color: color, font: font)
		case .mosaic:
			let size = insetRect.width / 3
			for row in 0..<3 {
				for column in 0..<3 {
					if (row + column).isMultiple(of: 2) {
						let cell = CGRect(
							x: insetRect.minX + CGFloat(column) * size,
							y: insetRect.minY + CGFloat(row) * size,
							width: size - 1,
							height: size - 1
						)
						context.fill(cell)
					}
				}
			}
		case .spotlight:
			let outer = insetRect
			let inner = outer.insetBy(dx: 3, dy: 3)
			context.stroke(outer)
			context.clear(inner)
			context.stroke(inner)
		case .undo:
			drawCurvedArrow(in: insetRect, clockwise: false, context: context)
		case .redo:
			drawCurvedArrow(in: insetRect, clockwise: true, context: context)
		case .autoCenter:
			let center = CGPoint(x: insetRect.midX, y: insetRect.midY)
			for target in [
				CGPoint(x: insetRect.minX, y: insetRect.midY),
				CGPoint(x: insetRect.maxX, y: insetRect.midY),
				CGPoint(x: insetRect.midX, y: insetRect.minY),
				CGPoint(x: insetRect.midX, y: insetRect.maxY),
			] {
				context.move(to: target)
				context.addLine(to: center)
				context.strokePath()
			}
		case .scroll:
			context.move(to: CGPoint(x: insetRect.midX, y: insetRect.minY))
			context.addLine(to: CGPoint(x: insetRect.midX, y: insetRect.maxY))
			context.strokePath()
			drawArrow(
				from: CGPoint(x: insetRect.midX, y: insetRect.minY + 3),
				to: CGPoint(x: insetRect.midX, y: insetRect.minY),
				in: context
			)
			drawArrow(
				from: CGPoint(x: insetRect.midX, y: insetRect.maxY - 3),
				to: CGPoint(x: insetRect.midX, y: insetRect.maxY),
				in: context
			)
		case .ocr:
			let corner: CGFloat = 4
			context.move(to: CGPoint(x: insetRect.minX, y: insetRect.minY + corner))
			context.addLine(to: CGPoint(x: insetRect.minX, y: insetRect.minY))
			context.addLine(to: CGPoint(x: insetRect.minX + corner, y: insetRect.minY))
			context.move(to: CGPoint(x: insetRect.maxX - corner, y: insetRect.minY))
			context.addLine(to: CGPoint(x: insetRect.maxX, y: insetRect.minY))
			context.addLine(to: CGPoint(x: insetRect.maxX, y: insetRect.minY + corner))
			context.move(to: CGPoint(x: insetRect.minX, y: insetRect.maxY - corner))
			context.addLine(to: CGPoint(x: insetRect.minX, y: insetRect.maxY))
			context.addLine(to: CGPoint(x: insetRect.minX + corner, y: insetRect.maxY))
			context.move(to: CGPoint(x: insetRect.maxX - corner, y: insetRect.maxY))
			context.addLine(to: CGPoint(x: insetRect.maxX, y: insetRect.maxY))
			context.addLine(to: CGPoint(x: insetRect.maxX, y: insetRect.maxY - corner))
			context.strokePath()
		case .copy:
			context.stroke(insetRect.offsetBy(dx: -2, dy: 2))
			context.stroke(insetRect.offsetBy(dx: 1, dy: -1))
		case .save:
			let tray = CGRect(x: insetRect.minX, y: insetRect.maxY - 4, width: insetRect.width, height: 4)
			context.stroke(tray)
			drawArrow(
				from: CGPoint(x: insetRect.midX, y: insetRect.minY),
				to: CGPoint(x: insetRect.midX, y: insetRect.maxY - 3),
				in: context
			)
		}

		context.restoreGState()
	}

	private func drawCurvedArrow(in rect: CGRect, clockwise: Bool, context: CGContext) {
		let radius = min(rect.width, rect.height) * 0.42
		let center = CGPoint(x: rect.midX, y: rect.midY)
		let startAngle: CGFloat = clockwise ? .pi * 0.15 : .pi * 0.85
		let endAngle: CGFloat = clockwise ? .pi * 1.55 : -.pi * 0.55
		context.addArc(
			center: center,
			radius: radius,
			startAngle: startAngle,
			endAngle: endAngle,
			clockwise: !clockwise
		)
		context.strokePath()
		let headPoint = CGPoint(
			x: center.x + cos(endAngle) * radius,
			y: center.y + sin(endAngle) * radius
		)
		let tailPoint = CGPoint(
			x: headPoint.x + (clockwise ? -4 : 4),
			y: headPoint.y + 2
		)
		drawArrow(from: tailPoint, to: headPoint, in: context)
	}

	private func chromeTheme() -> CaptureChromeTheme {
		effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .aqua ? .light : .dark
	}

	private func configureChromeMaterialView(_ view: NSVisualEffectView) {
		view.blendingMode = .behindWindow
		view.state = .active
		view.isHidden = true
	}

	private func updateChromeMaterialViews() {
		[hudMaterialView, loupeMaterialView, toolbarMaterialView, statusMaterialView].forEach {
			$0.isHidden = true
		}
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
		let screenRefreshRate = window?.screen?.maximumFramesPerSecond ?? 60
		let cappedRefreshRate = max(1, min(screenRefreshRate, 120))
		return 1.0 / Double(cappedRefreshRate)
	}

	private func lastPointerDispatchUptime(for event: QueuedPointerEvent) -> TimeInterval {
		switch event {
		case .moved:
			return lastHoverPointerDispatchUptime
		case .liveDragged:
			return lastDragPointerDispatchUptime
		}
	}

	private func setLastPointerDispatchUptime(_ uptime: TimeInterval, for event: QueuedPointerEvent) {
		switch event {
		case .moved:
			lastHoverPointerDispatchUptime = uptime
		case .liveDragged:
			lastDragPointerDispatchUptime = uptime
		}
	}

}

private extension NSCursor {
	static var _windowResizeNorthEastSouthWest: NSCursor {
		if #available(macOS 15.0, *) {
			return .frameResize(position: .topRight, directions: [.inward, .outward])
		}
		return .crosshair
	}

	static var _windowResizeNorthWestSouthEast: NSCursor {
		if #available(macOS 15.0, *) {
			return .frameResize(position: .topLeft, directions: [.inward, .outward])
		}
		return .crosshair
	}
}

private extension CGRect {
	func clamp(_ point: CGPoint) -> CGPoint {
		CGPoint(
			x: point.x.clamped(to: minX...maxX),
			y: point.y.clamped(to: minY...maxY)
		)
	}

	func normalizedRect(anchor: CGPoint, current: CGPoint) -> CGRect {
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

private struct CaptureChromeState {
	var loupePatch: CGImage?
	var rgbSample: RGBSample?
	var frozenSelectionSnapshot: CGRect?
	var frozenBaseImage: CGImage?
	var frozenMosaicImage: CGImage?
	var frozenOverlay = FrozenOverlayState()

	mutating func resetLiveChrome() {
		loupePatch = nil
	}

	mutating func resetFrozenChrome() {
		frozenSelectionSnapshot = nil
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
				if let lastPoint = points.last, hypot(lastPoint.x - clamped.x, lastPoint.y - clamped.y) < 1.5 {
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
		edits.append(.text(FrozenTextAnnotation(anchor: activeTextEdit.anchor, text: activeTextEdit.text)))
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

private struct FrozenToolbarItemLayout {
	let kind: ToolbarItemKind
	let frame: CGRect
	let enabled: Bool
	let selected: Bool
}

private struct FrozenToolbarLayout {
	let frame: CGRect
	let items: [FrozenToolbarItemLayout]
}

enum CaptureChromeTheme {
	case dark
	case light
}

struct CaptureChromePalette {
	let bodyFill: NSColor
	let outerStroke: NSColor
	let shadow: NSColor
	let labelText: NSColor
	let secondaryText: NSColor
	let swatchStroke: NSColor
	let keycapFill: NSColor
	let keycapStroke: NSColor
	let keycapText: NSColor
	let toolbarIcon: NSColor
	let toolbarHoverIcon: NSColor
	let toolbarSelectedIcon: NSColor
	let toolbarDisabledIcon: NSColor
	let toolbarHoverBackground: NSColor
	let toolbarSelectedBackground: NSColor
}

enum CaptureChrome {
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
	static let resizeHandleHitSize: CGFloat = 24
	static let resizeHandleOuterRadius: CGFloat = 4.25
	static let resizeHandleCenterDotRadius: CGFloat = 1.15
	static let resizeHandleStrokeWidth: CGFloat = 1.3
	static let toolbarButtonSize: CGFloat = 24
	static let toolbarItemSpacing: CGFloat = 4
	static let toolbarVerticalPadding: CGFloat = 6
	static let toolbarGap: CGFloat = 10
	static let toolbarScreenMargin: CGFloat = 10

	static func dashedBorderOutset(strokeWidth: CGFloat, pixelsPerPoint: CGFloat) -> CGFloat {
		let feathering = 1.0 / max(pixelsPerPoint, .leastNonzeroMagnitude)
		return (strokeWidth + feathering) * 0.5
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
				segments.append((
					CGPoint(x: rect.minX + start, y: rect.minY),
					CGPoint(x: rect.minX + end, y: rect.minY)
				))
			}
			for (start, end) in verticalRanges {
				segments.append((
					CGPoint(x: rect.maxX, y: rect.minY + start),
					CGPoint(x: rect.maxX, y: rect.minY + end)
				))
			}
			for (start, end) in horizontalRanges {
				segments.append((
					CGPoint(x: rect.minX + start, y: rect.maxY),
					CGPoint(x: rect.minX + end, y: rect.maxY)
				))
			}
			for (start, end) in verticalRanges {
				segments.append((
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

		let occupiedLength = CGFloat(dashCount) * clampedDashLength + CGFloat(dashCount - 1) * gapLength
		let gapCount = max(dashCount - 1, 0)
		let resolvedGapLength: CGFloat = if gapCount == 0 {
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
			pushDashedBorderSegment(for: rect, start: segmentStart, end: cornerDistance, into: &segments)
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
		let resolvedDistance = normalizedDistance < 0 ? normalizedDistance + perimeter : normalizedDistance

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

	static func palette(for theme: CaptureChromeTheme, settings: NativeHostSettings) -> CaptureChromePalette {
		let opacity = CGFloat(settings.hudOpacity.clamped(to: 0...1))
		let tint = CGFloat(settings.hudTint.clamped(to: 0...1))
		let hue = CGFloat(settings.hudTintHue.clamped(to: 0...1))
		let bodyAlphaFloor: CGFloat = theme == .dark ? 0.06 : 0.08
		let fillOpacity: CGFloat = settings.hudGlassEnabled
			? max(bodyAlphaFloor, opacity * 0.20)
			: opacity
		let tintColor = NSColor(
			calibratedHue: hue,
			saturation: theme == .dark ? (0.08 + 0.22 * tint) : (0.04 + 0.16 * tint),
			brightness: theme == .dark ? (0.30 + 0.12 * tint) : (0.93 - 0.06 * tint),
			alpha: 1
		)

		switch theme {
			case .dark:
				let baseFill = NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 1)
				let bodyFill = baseFill
					.mixed(with: tintColor, fraction: tint * 0.55)
					.withAlphaComponent(fillOpacity)
				return CaptureChromePalette(
					bodyFill: bodyFill,
				outerStroke: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: max(0.12, 0.14 + opacity * 0.10)),
				shadow: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: max(0.16, 0.12 + opacity * 0.18)),
				labelText: NSColor(srgbRed: 235 / 255, green: 235 / 255, blue: 245 / 255, alpha: 235 / 255),
				secondaryText: NSColor(srgbRed: 235 / 255, green: 235 / 255, blue: 245 / 255, alpha: 150 / 255),
				swatchStroke: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 36 / 255),
				keycapFill: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: max(0.06, opacity * 0.18)),
				keycapStroke: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: max(0.10, opacity * 0.22)),
				keycapText: NSColor(srgbRed: 235 / 255, green: 235 / 255, blue: 245 / 255, alpha: 150 / 255),
				toolbarIcon: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 160 / 255),
				toolbarHoverIcon: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 222 / 255),
				toolbarSelectedIcon: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 1),
				toolbarDisabledIcon: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 72 / 255),
				toolbarHoverBackground: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: max(0.08, opacity * 0.18)),
				toolbarSelectedBackground: NSColor(srgbRed: 1, green: 1, blue: 1, alpha: max(0.12, opacity * 0.24))
			)
			case .light:
				let baseFill = NSColor(srgbRed: 232 / 255, green: 236 / 255, blue: 243 / 255, alpha: 1)
				let bodyFill = baseFill
					.mixed(with: tintColor, fraction: tint * 0.45)
					.withAlphaComponent(fillOpacity)
				return CaptureChromePalette(
					bodyFill: bodyFill,
				outerStroke: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: max(0.12, 0.16 + opacity * 0.12)),
				shadow: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: max(0.10, 0.06 + opacity * 0.14)),
				labelText: NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 235 / 255),
				secondaryText: NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 160 / 255),
				swatchStroke: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: 44 / 255),
				keycapFill: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: max(0.05, opacity * 0.12)),
				keycapStroke: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: max(0.10, opacity * 0.20)),
				keycapText: NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 160 / 255),
				toolbarIcon: NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 182 / 255),
				toolbarHoverIcon: NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 220 / 255),
				toolbarSelectedIcon: NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 1),
				toolbarDisabledIcon: NSColor(srgbRed: 28 / 255, green: 28 / 255, blue: 32 / 255, alpha: 82 / 255),
				toolbarHoverBackground: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: max(0.08, opacity * 0.16)),
				toolbarSelectedBackground: NSColor(srgbRed: 0, green: 0, blue: 0, alpha: max(0.10, opacity * 0.22))
			)
		}
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

private extension NSColor {
	func mixed(with other: NSColor, fraction: CGFloat) -> NSColor {
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

private extension NSImage {
	func tinted(with color: NSColor) -> NSImage {
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
