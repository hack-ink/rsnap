import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	func prepareLiveFrameStreamSampler(reason: String) {
		liveFrameStream.prepareAuthority(reason: reason)
	}

	func prepareLaunchCaptureStreams(reason: String) {
		liveFrameStream.prepareAuthority(reason: reason)
		guard NativePermissions.screenRecordingGranted else {
			return
		}
		_ = warmLiveSamplingIfPossible(
			at: NSEvent.mouseLocation,
			source: reason,
			excludeSelfFromFrozenAuthority: true
		)
		releaseScreenCaptureStreams()
	}

	func allocateCaptureTelemetryID() -> UInt64 {
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

	func startCaptureSession(
		captureID: UInt64,
		captureStartedAt: TimeInterval,
		capturableOwnWindowIDs: Set<CGWindowID>
	) throws {
		let startPoint = NSEvent.mouseLocation
		let desktopFrame = CaptureOverlayController.desktopFrame
		frozenFrameLatchToken = nil
		// The native frame authority treats these IDs as current-process windows to include
		// through the app-level exclusion. Overlay windows must stay out of this list so color
		// sampling sees the desktop under the capture UI.
		cancelPendingScreenCaptureStreamRelease(reason: "start_capture")
		liveFrameStream.updateSelfCaptureExceptionWindowIDs(
			capturableOwnWindowIDs,
			captureID: captureID
		)
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
		let initialWindowReport = WindowSnapshotFeed.snapshotReport(desktopFrame: desktopFrame)
		let initialWindowSnapshots = initialWindowReport.snapshots
		let windowSnapshotMilliseconds =
			NativeHostTelemetry.milliseconds(since: windowSnapshotStartedAt)
		NativeHostTelemetry.liveChromeWindowSnapshotRefresh(
			captureID: captureID,
			source: "start_capture",
			totalMilliseconds: windowSnapshotMilliseconds,
			candidateWindowCount: initialWindowReport.candidateWindowCount,
			targetableWindowCount: initialWindowReport.snapshots.count,
			ownWindowCount: initialWindowReport.ownWindowCount,
			ownTargetableWindowCount: initialWindowReport.ownTargetableWindowCount,
			highLayerWindowCount: initialWindowReport.highLayerWindowCount,
			tinyWindowCount: initialWindowReport.tinyWindowCount,
			transparentWindowCount: initialWindowReport.transparentWindowCount
		)
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
			},
			frameRegionSampler: { [frozenFrameAuthority] rect in
				frozenFrameAuthority.regionImage(in: rect)
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
				self.prepareOverlayCaptureStreams(
					overlayController: overlayController,
					startPoint: startPoint,
					captureID: captureID,
					capturableOwnWindowIDs: capturableOwnWindowIDs
				)
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

	private func prepareOverlayCaptureStreams(
		overlayController: CaptureOverlayController,
		startPoint: CGPoint,
		captureID: UInt64,
		capturableOwnWindowIDs: Set<CGWindowID>
	) {
		let selfCaptureExceptionWindowIDs =
			overlayController.selfCaptureExceptionWindowIDs
		liveFrameStream.start(
			for: NSScreen.screens,
			prewarmPoint: startPoint,
			captureID: captureID
		)
		if capturableOwnWindowIDs.isEmpty,
			frozenFrameAuthority.hasSelfCaptureCompleteFrame(containing: startPoint)
		{
			NativeHostTelemetry.captureEvent(
				"capture.self_capture_rebuild_skipped",
				captureID: captureID,
				detail: "start_capture_complete_filter"
			)
		} else if capturableOwnWindowIDs.isEmpty,
			frozenFrameAuthority.hasSelfCaptureCompleteStream(containing: startPoint)
		{
			NativeHostTelemetry.captureEvent(
				"capture.self_capture_rebuild_skipped",
				captureID: captureID,
				detail: "start_capture_complete_stream"
			)
		} else {
			NativeHostTelemetry.captureEvent(
				"capture.self_capture_rebuild_requested",
				captureID: captureID,
				detail:
					"overlayWindowCount=\(selfCaptureExceptionWindowIDs.count) capturableOwnWindowCount=\(capturableOwnWindowIDs.count)"
			)
			_ = warmLiveSamplingIfPossible(
				at: startPoint,
				source: "capture_overlay_preflight",
				captureID: captureID,
				excludeSelfFromFrozenAuthority: true,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: capturableOwnWindowIDs
			)
		}
	}

	func ensureCapturePermissions() -> Bool {
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

	func cachedRegionImage(in rect: CGRect) -> CGImage? {
		overlayController?.cachedRegionImage(in: rect)
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
}
