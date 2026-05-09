import AppKit
import CoreGraphics
import CoreImage
import CoreText
import Darwin
import Foundation
import QuartzCore
import RsnapHostBridge
import Vision

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

	func releaseScreenCaptureStreams(immediate: Bool = false) {
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
