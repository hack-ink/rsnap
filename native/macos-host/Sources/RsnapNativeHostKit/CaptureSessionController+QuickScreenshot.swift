import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	func startQuickScreenshotFrozenCapture(
		selection: QuickScreenshotSelection,
		capturableOwnWindowIDs: Set<CGWindowID>
	) {
		if session != nil {
			overlayController?.focusWindow(at: selection.current)
			return
		}

		let captureID = allocateCaptureTelemetryID()
		activeCaptureTelemetryID = captureID
		let captureStartedAt = ProcessInfo.processInfo.systemUptime
		guard ensureCapturePermissions() else {
			NativeHostTelemetry.captureWarning(
				"capture.quick_screenshot_start_blocked",
				captureID: captureID,
				stage: "screen_recording_permission",
				error: "permission_denied"
			)
			activeCaptureTelemetryID = nil
			captureStateDidChange?()
			return
		}

		do {
			try startQuickScreenshotSession(
				captureID: captureID,
				captureStartedAt: captureStartedAt,
				selection: selection,
				capturableOwnWindowIDs: capturableOwnWindowIDs
			)
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.quick_screenshot_start_failed",
				captureID: captureID,
				stage: "exception",
				error: String(describing: error)
			)
			tearDownCapture()
		}
	}

	private func startQuickScreenshotSession(
		captureID: UInt64,
		captureStartedAt: TimeInterval,
		selection: QuickScreenshotSelection,
		capturableOwnWindowIDs: Set<CGWindowID>
	) throws {
		let startPoint = selection.anchor
		let endPoint = selection.current
		let sessionSetupStartedAt = ProcessInfo.processInfo.systemUptime
		let session = try RsnapHostSession(configuration: settingsStore.sessionConfiguration)
		self.session = session
		liveFrameStream.updateSelfCaptureExceptionWindowIDs(capturableOwnWindowIDs)

		try session.enterLive()
		let startInputs = currentLiveInputs(at: startPoint)
		try session.send(
			event: .pointerMoved(
				point: startPoint,
				rgb: startInputs.rgb,
				activeMonitor: startInputs.activeMonitor,
				highlightedWindow: nil
			)
		)
		try session.send(
			event: .primaryInteractionStarted(
				point: startPoint,
				activeMonitor: startInputs.activeMonitor,
				highlightedWindow: nil
			)
		)
		let endInputs = currentLiveInputs(at: endPoint)
		try session.send(
			event: .primaryInteractionUpdated(
				point: endPoint,
				activeMonitor: endInputs.activeMonitor,
				highlightedWindow: nil
			)
		)
		try session.send(
			event: .primaryInteractionCompleted(
				point: endPoint,
				activeMonitor: endInputs.activeMonitor,
				highlightedWindow: nil
			)
		)
		let pendingRequests = try session.drainRequests()
		let initialScene = try session.currentScene()
		scene = initialScene
		chromeState.rgbSample = endInputs.rgb
		guard
			let freezeRequest = quickScreenshotFreezeRequest(in: pendingRequests)
		else {
			NativeHostTelemetry.captureWarning(
				"capture.quick_screenshot_commit_failed",
				captureID: captureID,
				stage: "host_request",
				error: "missing_freeze_request"
			)
			tearDownCapture()
			return
		}
		let frozenFrame = quickFrozenFrame(from: selection)
		let frozenScene = prepareQuickScreenshotFrozenPresentation(
			selection: freezeRequest.selection,
			editable: freezeRequest.editable,
			frozenFrame: frozenFrame
		)
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
		overlayController.showFrozenFirstFrame(
			scene: frozenScene,
			chrome: chromeState,
			settings: settingsStore.settings,
			focusPoint: CGPoint(
				x: freezeRequest.selection.midX,
				y: freezeRequest.selection.midY
			)
		)
		(NSApp.delegate as? NativeHostApplicationController)?.window =
			overlayController.primaryWindow
		captureStateDidChange?()

		try finishQuickScreenshotRequests(
			pendingRequests,
			frozenFrame: frozenFrame,
			captureID: captureID,
			commitStartedAt: captureStartedAt
		)
		NativeHostTelemetry.captureStartTiming(
			captureID: captureID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
			warmMilliseconds: 0,
			windowSnapshotMilliseconds: 0,
			sessionSetupMilliseconds: sessionSetupMilliseconds,
			overlayShowMilliseconds: 0,
			initialSampleReady: endInputs.rgb != nil,
			screenCount: NSScreen.screens.count,
			windowCount: 0
		)
	}

	private func finishQuickScreenshotRequests(
		_ requests: [HostRequest],
		frozenFrame: FrozenFrameSnapshot,
		captureID: UInt64,
		commitStartedAt: TimeInterval
	) throws {
		for request in requests {
			switch request {
			case .requestFreezeSnapshot(let requestedSelection, let selectionEditable):
				try finishFrozenCommit(
					captureID: captureID,
					selection: requestedSelection,
					editable: selectionEditable,
					frozenFrame: frozenFrame,
					commitStartedAt: commitStartedAt,
					snapshotWaitMilliseconds: 0,
					hadLatchToken: false,
					syncAfterReport: true
				)
				NativeHostTelemetry.captureEvent(
					"capture.quick_screenshot_committed",
					captureID: captureID,
					detail:
						"x=\(Int(requestedSelection.minX.rounded())) y=\(Int(requestedSelection.minY.rounded())) w=\(Int(requestedSelection.width.rounded())) h=\(Int(requestedSelection.height.rounded()))"
				)
				return
			default:
				try handle(request: request)
			}
		}

		NativeHostTelemetry.captureWarning(
			"capture.quick_screenshot_commit_failed",
			captureID: captureID,
			stage: "host_request",
			error: "missing_freeze_request"
		)
		tearDownCapture()
	}

	private func quickScreenshotFreezeRequest(in requests: [HostRequest])
		-> (selection: CGRect, editable: Bool)?
	{
		for request in requests {
			if case .requestFreezeSnapshot(let selection, let selectionEditable) = request {
				return (selection, selectionEditable)
			}
		}
		return nil
	}

	private func prepareQuickScreenshotFrozenPresentation(
		selection: CGRect,
		editable: Bool,
		frozenFrame: FrozenFrameSnapshot
	) -> SceneSnapshot {
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
		let frozenScene = hostOwnedFrozenPresentationScene(
			for: selection,
			editable: editable
		)
		scene = frozenScene
		return frozenScene
	}

	private func quickFrozenFrame(from selection: QuickScreenshotSelection)
		-> FrozenFrameSnapshot
	{
		frozenSnapshotGeneration &+= 1
		return FrozenFrameSnapshot(
			displayID: selection.displayFrame.displayID,
			displayFrame: selection.displayFrame.frame,
			image: selection.displayFrame.image,
			generation: frozenSnapshotGeneration,
			sequence: 0,
			capturedAtUptime: selection.displayFrame.capturedAtUptime,
			source: "quick_screenshot_mouse_down",
			selfCaptureSafe: true,
			selfCaptureFilterComplete: true
		)
	}
}
