import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
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

	var recognizeTextActionEnabled: Bool {
		scene.mode == .frozen
			&& currentFrozenSelection() != nil
			&& scene.toolbarItems.contains { $0.kind == .ocr && $0.enabled }
			&& chromeState.frozenOverlay.hasRecognizeTextBlockingEdits == false
	}

	func recognizeText() {
		guard recognizeTextActionEnabled else {
			try? setHostStatusMessage(recognizeTextBlockedMessage())
			refreshOverlay()
			return
		}
		let _ = chromeState.frozenOverlay.commitTextEdit(
			style: chromeState.annotationStyle.textStyle)
		sendFrozenAction(.recognizeTextRequested, exitAfter: .recognizeText)
	}

	func recognizeTextBlockedMessage() -> String {
		if chromeState.frozenOverlay.hasRecognizeTextBlockingEdits {
			return "Text recognition is unavailable after annotation edits."
		}
		return "Text recognition is not available for this selection."
	}

	func startScrollCapture(source: String = "unknown") {
		let _ = chromeState.frozenOverlay.commitTextEdit(
			style: chromeState.annotationStyle.textStyle)
		guard scrollCaptureToolbarEnabled else {
			let reason = scrollCaptureEntryBlockedReason()
			NativeHostTelemetry.captureEvent(
				"capture.scroll_capture_entry",
				captureID: currentCaptureTelemetryID,
				outcome: "blocked",
				detail: scrollCaptureEntryDetail(source: source, reason: reason)
			)
			try? setHostStatusMessage(scrollCaptureEntryBlockedMessage(reason: reason))
			refreshOverlay()
			return
		}
		NativeHostTelemetry.captureEvent(
			"capture.scroll_capture_entry",
			captureID: currentCaptureTelemetryID,
			outcome: "requested",
			detail: scrollCaptureEntryDetail(source: source, reason: "ready")
		)
		do {
			try beginNativeScrollCapture()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.scroll_capture_entry_failed",
				captureID: currentCaptureTelemetryID,
				stage: source,
				error: String(describing: error)
			)
			try? setHostStatusMessage("Scroll Capture could not start.")
			refreshOverlay()
		}
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
			guard recognizeTextActionEnabled else {
				try? setHostStatusMessage(recognizeTextBlockedMessage())
				refreshOverlay()
				return
			}
			sendFrozenAction(.toolbarItemInvoked(item), exitAfter: .recognizeText)
		case .scroll:
			startScrollCapture(source: "toolbar")
		default:
			sendFrozenAction(.toolbarItemInvoked(item))
		}
	}

	private func scrollCaptureEntryBlockedReason() -> String {
		if Self.scrollCaptureEnabled == false {
			return "disabled"
		}
		if scene.mode != .frozen {
			return "requires_frozen"
		}
		if scrollCaptureState != nil {
			return "already_active"
		}
		if currentFrozenSelection() == nil {
			return "no_selection"
		}
		if chromeState.frozenSelectionEditable == false {
			return "not_dragged_region"
		}
		if let selection = currentFrozenSelection(),
			scrollCaptureSelectionHasSufficientHeight(selection) == false
		{
			return "selection_too_short"
		}
		return "unavailable"
	}

	private func scrollCaptureEntryBlockedMessage(reason: String) -> String {
		switch reason {
		case "disabled":
			return "Scroll Capture is disabled."
		case "already_active":
			return "Scroll Capture is already running."
		case "not_dragged_region":
			return "Scroll Capture requires a dragged region selection."
		case "no_selection", "requires_frozen":
			return "Select a dragged region before starting Scroll Capture."
		case "selection_too_short":
			return "Select a taller region before starting Scroll Capture."
		default:
			return "Scroll Capture is not available for this selection."
		}
	}

	private func scrollCaptureEntryDetail(source: String, reason: String) -> String {
		let selection = currentFrozenSelection()

		return [
			"source=\(source)",
			"reason=\(reason)",
			"scene=\(scene.mode)",
			"editable=\(chromeState.frozenSelectionEditable)",
			"has_selection=\(selection != nil)",
			"selection_height_px=\(selection.map { scrollCaptureSelectionHeightPixels($0) } ?? 0)",
			"minimum_height_px=\(Self.scrollCaptureMinimumSelectionHeightPixels)",
			"active=\(scrollCaptureState != nil)",
		].joined(separator: " ")
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
			schedulePreparedFrozenAnnotationExport(reason: "annotation_finish")
			return
		}
		pointerMoved(to: point)
	}

	func currentFrozenSelection() -> CGRect? {
		chromeState.frozenSelectionSnapshot ?? scene.frozenSelection
	}

	func beginFrozenSelectionTransformIfPossible(
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

	func updateFrozenSelectionTransform(to point: CGPoint) -> Bool {
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

	func completeFrozenSelectionTransform(at point: CGPoint) -> Bool {
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
		invalidatePreparedFrozenExport()
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
				self.schedulePreparedFrozenExport(reason: "selection_transform")
				self.schedulePreparedRecognizeTextImage(reason: "selection_transform")
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

	func transformedFrozenSelection(
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
		schedulePreparedFrozenAnnotationExport(reason: "annotation_undo")
	}

	func performFrozenRedo() {
		guard chromeState.frozenOverlay.redo() else {
			return
		}
		refreshOverlay()
		schedulePreparedFrozenAnnotationExport(reason: "annotation_redo")
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
			invalidatePreparedFrozenExport()
			chromeState.frozenSelectionSnapshot = nextSelection
			chromeState.frozenBaseImage =
				nextBaseImage ?? frozenBaseImageFromDisplay(for: nextSelection)
			try session?.send(report: .freezeSnapshotCommitted(selection: nextSelection))
			try syncCore()
			schedulePreparedFrozenExport(reason: "auto_center")
			schedulePreparedRecognizeTextImage(reason: "auto_center")
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
				schedulePreparedFrozenAnnotationExport(reason: "annotation_text_commit")
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

	func primeLoupePatchForToggle(at point: CGPoint) {
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

	func sendFrozenAction(
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

	func beginHostLocalFrozenSelectingIfPossible(at point: CGPoint) {
		guard scene.mode == .live else {
			return
		}
		guard chromeState.hostLocalFrozenSelecting == false else {
			return
		}
		chromeState.beginHostLocalFrozenSelecting()
	}
	static func clampedSelectionRect(
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

	static func autoCenterMarginBalanceShiftPoints(
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

	static func detectAutoCenterContentBounds(in image: CGImage) -> CGRect? {
		guard let snapshot = NativeHostImageBridge.rgbaSnapshot(from: image) else {
			return nil
		}
		return try? RsnapAutoCenterPlanner.contentBounds(in: snapshot)
	}
}
