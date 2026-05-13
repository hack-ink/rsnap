import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	func syncCore() throws {
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

	func handle(request: HostRequest) throws {
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

	func commitFrozenSelection(_ selection: CGRect, editable: Bool) throws {
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

	func schedulePendingFrozenCommit(
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

	func finishPendingFrozenCommit(
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

	func failFrozenCommit(
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

	func finishFrozenCommit(
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
		invalidatePreparedFrozenExport()
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
		schedulePreparedFrozenExport(reason: "freeze_commit")
		schedulePreparedRecognizeTextImage(reason: "freeze_commit")
	}

	func frozenFrameLatchWait(containing _: CGPoint) -> TimeInterval {
		Self.displayFirstFrameWait
	}

	func hostOwnedFrozenPresentationScene(for selection: CGRect, editable: Bool)
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

	func captureFrameSource(for selection: CGRect, editable: Bool) -> CaptureFrameSource {
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

	static func rectNearlyMatches(
		_ lhs: CGRect,
		_ rhs: CGRect,
		tolerance: CGFloat
	) -> Bool {
		abs(lhs.minX - rhs.minX) <= tolerance
			&& abs(lhs.minY - rhs.minY) <= tolerance
			&& abs(lhs.width - rhs.width) <= tolerance
			&& abs(lhs.height - rhs.height) <= tolerance
	}

	func hostOwnedFrozenToolbarItems(scrollEnabled: Bool) -> [ToolbarItem] {
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
}
