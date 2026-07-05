import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	func performCopy() throws {
		guard session != nil else {
			return
		}
		let copyStartedAt = ProcessInfo.processInfo.systemUptime
		guard let request = try frozenSelectionImageRenderRequest() else {
			logCopyCaptureFailure(
				copyStartedAt: copyStartedAt,
				captureImageMilliseconds: 0,
				makeImageMilliseconds: 0,
				failureStage: "capture_image",
				width: 0,
				height: 0
			)
			try setHostStatusMessage("Could not capture the frozen selection.")
			refreshOverlay()
			return
		}

		hostEffectJobGeneration &+= 1
		let jobGeneration = hostEffectJobGeneration
		try setHostStatusMessage("Copying capture...")
		refreshOverlay()

		if let preparedResult = frozenPreparedExportStore.result(matching: request) {
			finishCopyCaptureJob(
				preparedResult,
				copyStartedAt: copyStartedAt,
				jobGeneration: jobGeneration
			)
			return
		}

		let preparedExportStore = frozenPreparedExportStore
		frozenImageRenderQueue.async { [weak self] in
			let result =
				preparedExportStore.result(matching: request)
				?? FrozenSelectionImageRenderer.renderCopyCaptureJob(request: request)
			DispatchQueue.main.async {
				self?.finishCopyCaptureJob(
					result,
					copyStartedAt: copyStartedAt,
					jobGeneration: jobGeneration
				)
			}
		}
	}

	func performSave() throws {
		guard session != nil else {
			return
		}
		guard let request = try frozenSelectionImageRenderRequest() else {
			try setHostStatusMessage("Could not capture the frozen selection.")
			refreshOverlay()
			return
		}
		let saveStartedAt = ProcessInfo.processInfo.systemUptime
		let outputURL = try nextOutputURL()
		hostEffectJobGeneration &+= 1
		let jobGeneration = hostEffectJobGeneration
		try setHostStatusMessage("Saving capture...")
		refreshOverlay()

		let preparedExportStore = frozenPreparedExportStore
		frozenImageRenderQueue.async { [weak self] in
			let result = FrozenSelectionImageRenderer.renderSaveCaptureJob(
				request: request,
				outputURL: outputURL,
				preparedExportStore: preparedExportStore
			)
			DispatchQueue.main.async {
				self?.finishSaveCaptureJob(
					result,
					saveStartedAt: saveStartedAt,
					jobGeneration: jobGeneration
				)
			}
		}
	}

	private func frozenSelectionImageRenderRequest() throws -> FrozenSelectionImageRenderRequest? {
		guard let selection = currentFrozenSelection() else {
			return nil
		}
		let scrollExportSnapshot = try activeScrollCaptureExportSnapshot()
		let settings = settingsStore.settings
		let selectionCenter = CGPoint(x: selection.midX, y: selection.midY)
		let screen = screen(containing: selectionCenter)
		let wallpaperPath =
			settings.captureFrameBackground == .systemWallpaper
			? CaptureFrameEffectRenderer.systemWallpaperPath(screen: screen)
			: nil
		return FrozenSelectionImageRenderRequest(
			captureID: currentCaptureTelemetryID,
			selection: selection,
			scrollExportSnapshot: scrollExportSnapshot?.snapshot,
			scrollExportRevision: scrollExportSnapshot?.revision ?? 0,
			frozenDisplayFrame: chromeState.frozenDisplayFrame,
			frozenDisplayImage: chromeState.frozenDisplayImage,
			frozenBaseImage: chromeState.frozenBaseImage,
			frozenSelectionSnapshot: chromeState.frozenSelectionSnapshot,
			overlayElements: chromeState.frozenOverlay.exportElements,
			hasOverlayEdits: chromeState.frozenOverlay.canUndo
				|| chromeState.frozenOverlay.hasActiveInteraction,
			captureFrameSource: chromeState.captureFrameSource,
			captureFrameWindowID: chromeState.captureFrameWindowID,
			captureFrameEffectEnabled: settings.captureFrameEffectEnabled,
			captureFrameBackground: settings.captureFrameBackground,
			captureFrameApplicability: settings.captureFrameApplicability,
			captureFrameEnvironment: CaptureFrameRenderEnvironment(
				screenScaleFactor: screen?.backingScaleFactor ?? 2,
				wallpaperPath: wallpaperPath
			)
		)
	}

	func invalidatePreparedFrozenExport() {
		pendingScrollCapturePreparedExport?.cancel()
		pendingScrollCapturePreparedExport = nil
		pendingFrozenAnnotationPreparedExport?.cancel()
		pendingFrozenAnnotationPreparedExport = nil
		pendingFrozenRecognizeTextImagePreparation?.cancel()
		pendingFrozenRecognizeTextImagePreparation = nil
		frozenPreparedExportStore.invalidate()
		frozenPreparedRecognizeTextImageStore.invalidate()
	}

	func schedulePreparedFrozenAnnotationExport(reason: String) {
		pendingFrozenAnnotationPreparedExport?.cancel()
		let workItem = DispatchWorkItem { [weak self] in
			guard let self else {
				return
			}
			self.pendingFrozenAnnotationPreparedExport = nil
			self.schedulePreparedFrozenExport(reason: reason)
			self.schedulePreparedRecognizeTextImage(reason: reason)
		}
		pendingFrozenAnnotationPreparedExport = workItem
		DispatchQueue.main.asyncAfter(
			deadline: .now() + Self.frozenAnnotationPreparedExportDelay,
			execute: workItem
		)
	}

	func schedulePreparedScrollCaptureExport(reason: String, revision: UInt64) {
		schedulePreparedScrollCaptureExport(
			reason: reason,
			revision: revision,
			delay: Self.scrollCapturePreparedExportDelay
		)
	}

	private func schedulePreparedScrollCaptureExport(
		reason: String,
		revision: UInt64,
		delay: TimeInterval
	) {
		pendingScrollCapturePreparedExport?.cancel()
		let workItem = DispatchWorkItem { [weak self] in
			guard let self else {
				return
			}
			self.pendingScrollCapturePreparedExport = nil
			guard let state = self.scrollCaptureState,
				state.exportRevision == revision
			else {
				return
			}
			let remainingDelay = self.remainingPreparedScrollExportQuietDelay(state: state)
			if remainingDelay > 0 {
				self.schedulePreparedScrollCaptureExport(
					reason: reason,
					revision: revision,
					delay: remainingDelay
				)
				return
			}
			self.schedulePreparedFrozenExport(reason: reason)
			self.schedulePreparedRecognizeTextImage(reason: reason)
		}
		pendingScrollCapturePreparedExport = workItem
		DispatchQueue.main.asyncAfter(
			deadline: .now() + delay,
			execute: workItem
		)
	}

	private func remainingPreparedScrollExportQuietDelay(
		state: NativeScrollCaptureState
	) -> TimeInterval {
		let lastInputUptime = max(
			state.lastObservedWheelUptime,
			state.lastForwardedWheelUptime
		)
		guard lastInputUptime > 0 else {
			return 0
		}
		let elapsed = ProcessInfo.processInfo.systemUptime - lastInputUptime
		return max(0, Self.scrollCapturePreparedExportDelay - elapsed)
	}

	private var canPrepareRecognizeTextImage: Bool {
		let allowTextInput =
			session?.configuration.allowTextInput
			?? settingsStore.sessionConfiguration.allowTextInput
		return allowTextInput
			&& currentFrozenSelection() != nil
			&& chromeState.frozenOverlay.hasRecognizeTextBlockingEdits == false
	}

	func schedulePreparedRecognizeTextImage(reason: String) {
		guard canPrepareRecognizeTextImage else {
			pendingFrozenRecognizeTextImagePreparation?.cancel()
			pendingFrozenRecognizeTextImagePreparation = nil
			frozenPreparedRecognizeTextImageStore.invalidate()
			return
		}
		schedulePreparedRecognizeTextImage(
			reason: reason,
			delay: Self.frozenRecognizeTextImagePreparationDelay
		)
	}

	private func schedulePreparedRecognizeTextImage(
		reason: String,
		delay: TimeInterval
	) {
		pendingFrozenRecognizeTextImagePreparation?.cancel()
		let workItem = DispatchWorkItem { [weak self] in
			guard let self else {
				return
			}
			self.pendingFrozenRecognizeTextImagePreparation = nil
			guard
				let request = try? self.frozenSelectionImageRenderRequest(),
				request.canPrepareExportInBackground,
				let preparationGeneration =
					self.frozenPreparedRecognizeTextImageStore.beginPreparing(for: request)
			else {
				return
			}

			let preparedImageStore = self.frozenPreparedRecognizeTextImageStore
			self.frozenImageRenderQueue.async {
				guard
					preparedImageStore.preparationIsCurrent(
						for: request,
						generation: preparationGeneration
					)
				else {
					return
				}
				let startedAt = ProcessInfo.processInfo.systemUptime
				let result = FrozenSelectionImageRenderer.renderPreparedRecognizeTextImageJob(
					request: request)
				preparedImageStore.finishPreparing(
					for: request,
					generation: preparationGeneration,
					result: result
				)
				NativeHostTelemetry.preparedRecognizeTextImageTiming(
					captureID: request.captureID,
					totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAt),
					captureImageMilliseconds: result.captureImageMilliseconds,
					success: result.image != nil,
					reason: reason,
					width: result.width,
					height: result.height
				)
			}
		}
		pendingFrozenRecognizeTextImagePreparation = workItem
		DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: workItem)
	}

	func preparedRecognizeTextCaptureImage(
		captureImageStartedAt: TimeInterval
	) throws -> PreparedRecognizeTextCaptureImage? {
		guard let request = try frozenSelectionImageRenderRequest() else {
			return nil
		}
		if let image = frozenPreparedRecognizeTextImageStore.result(matching: request) {
			return PreparedRecognizeTextCaptureImage(
				image: image,
				captureImageMilliseconds: 0,
				cacheHit: true
			)
		}

		let result = FrozenSelectionImageRenderer.renderPreparedRecognizeTextImageJob(
			request: request)
		if let baseImage = result.baseImage,
			chromeState.frozenSelectionSnapshot == request.selection,
			chromeState.frozenBaseImage == nil
		{
			chromeState.frozenBaseImage = baseImage
		}
		guard let image = result.image else {
			return nil
		}
		return PreparedRecognizeTextCaptureImage(
			image: image,
			captureImageMilliseconds: NativeHostTelemetry.milliseconds(
				since: captureImageStartedAt),
			cacheHit: false
		)
	}

	func schedulePreparedFrozenExport(reason: String) {
		guard let request = try? frozenSelectionImageRenderRequest(),
			request.canPrepareExportInBackground,
			let preparationGeneration = frozenPreparedExportStore.beginPreparing(for: request)
		else {
			return
		}

		let preparedExportStore = frozenPreparedExportStore
		frozenImageRenderQueue.async {
			guard
				preparedExportStore.preparationIsCurrent(
					for: request,
					generation: preparationGeneration
				)
			else {
				return
			}
			let startedAt = ProcessInfo.processInfo.systemUptime
			let result = FrozenSelectionImageRenderer.renderCopyCaptureJob(request: request)
			preparedExportStore.finishPreparing(
				for: request,
				generation: preparationGeneration,
				result: result
			)
			NativeHostTelemetry.preparedFrozenExportTiming(
				captureID: request.captureID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAt),
				captureImageMilliseconds: result.captureImageMilliseconds,
				makeImageMilliseconds: result.makeImageMilliseconds,
				success: result.pngData != nil,
				reason: reason,
				width: result.width,
				height: result.height
			)
		}
	}

	private func finishCopyCaptureJob(
		_ result: CopyCaptureJobResult,
		copyStartedAt: TimeInterval,
		jobGeneration: UInt64
	) {
		guard hostEffectJobGeneration == jobGeneration, session != nil else {
			return
		}
		guard let pngData = result.pngData else {
			logCopyCaptureFailure(
				copyStartedAt: copyStartedAt,
				captureImageMilliseconds: result.captureImageMilliseconds,
				makeImageMilliseconds: result.makeImageMilliseconds,
				failureStage: result.failureStage,
				width: result.width,
				height: result.height
			)
			try? setHostStatusMessage(result.failureMessage)
			refreshOverlay()
			return
		}

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
				captureImageMilliseconds: result.captureImageMilliseconds,
				clearPasteboardMilliseconds: clearPasteboardMilliseconds,
				makeImageMilliseconds: result.makeImageMilliseconds,
				writePasteboardMilliseconds: writePasteboardMilliseconds,
				success: false,
				failureStage: "pasteboard_write",
				width: result.width,
				height: result.height,
				cacheHit: result.cacheHit
			)
			try? setHostStatusMessage("Could not copy the captured image.")
			refreshOverlay()
			return
		}
		NativeHostTelemetry.copyCaptureTiming(
			captureID: currentCaptureTelemetryID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: copyStartedAt),
			captureImageMilliseconds: result.captureImageMilliseconds,
			clearPasteboardMilliseconds: clearPasteboardMilliseconds,
			makeImageMilliseconds: result.makeImageMilliseconds,
			writePasteboardMilliseconds: writePasteboardMilliseconds,
			success: true,
			failureStage: "none",
			width: result.width,
			height: result.height,
			cacheHit: result.cacheHit
		)
		completeHostEffect(.copyCapture, statusMessage: "Copied capture to clipboard.")
	}

	private func finishSaveCaptureJob(
		_ result: SaveCaptureJobResult,
		saveStartedAt: TimeInterval,
		jobGeneration: UInt64
	) {
		guard hostEffectJobGeneration == jobGeneration, session != nil else {
			return
		}
		NativeHostTelemetry.saveCaptureTiming(
			captureID: currentCaptureTelemetryID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: saveStartedAt),
			captureImageMilliseconds: result.captureImageMilliseconds,
			makeImageMilliseconds: result.makeImageMilliseconds,
			writeFileMilliseconds: result.writeFileMilliseconds,
			success: result.outputURL != nil,
			failureStage: result.failureStage,
			width: result.width,
			height: result.height,
			cacheHit: result.cacheHit
		)
		guard let outputURL = result.outputURL else {
			try? setHostStatusMessage(result.failureMessage)
			refreshOverlay()
			return
		}
		completeHostEffect(
			.saveCapture,
			statusMessage: "Saved capture to \(outputURL.lastPathComponent)."
		)
	}

	private func completeHostEffect(_ effect: HostEffectKind, statusMessage: String) {
		do {
			captureSuccessSound.play()
			try session?.send(report: .hostEffectCompleted(effect))
			try session?.send(report: .statusMessage(statusMessage))
			completedHostEffect = effect
			tearDownCapture()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.host_effect_complete_failed",
				captureID: currentCaptureTelemetryID,
				stage: String(describing: effect),
				error: String(describing: error)
			)
			try? setHostStatusMessage(statusMessage)
			refreshOverlay()
		}
	}

	private func logCopyCaptureFailure(
		copyStartedAt: TimeInterval,
		captureImageMilliseconds: Double,
		makeImageMilliseconds: Double,
		failureStage: String,
		width: Int,
		height: Int
	) {
		NativeHostTelemetry.copyCaptureTiming(
			captureID: currentCaptureTelemetryID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: copyStartedAt),
			captureImageMilliseconds: captureImageMilliseconds,
			clearPasteboardMilliseconds: 0,
			makeImageMilliseconds: makeImageMilliseconds,
			writePasteboardMilliseconds: 0,
			success: false,
			failureStage: failureStage,
			width: width,
			height: height
		)
	}

	private func activeScrollCaptureExportSnapshot() throws -> ActiveScrollCaptureExportSnapshot? {
		guard Self.scrollCaptureEnabled else {
			return nil
		}
		guard let state = scrollCaptureState else {
			return nil
		}
		guard let snapshot = try state.stitcher.exportImage() else {
			return nil
		}
		return ActiveScrollCaptureExportSnapshot(
			snapshot: snapshot,
			revision: state.exportRevision
		)
	}

	func captureFrozenSelectionImage(applyingCaptureFrameEffect: Bool = false) throws
		-> CGImage?
	{
		let captureStartedAt = ProcessInfo.processInfo.systemUptime
		guard let request = try frozenSelectionImageRenderRequest() else {
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
		let result = try FrozenSelectionImageRenderer.renderFrozenSelectionImage(
			from: request,
			applyingCaptureFrameEffect: applyingCaptureFrameEffect,
			prefersPixelSnapshot: false
		)
		if let baseImage = result.baseImage,
			chromeState.frozenSelectionSnapshot == request.selection,
			chromeState.frozenBaseImage == nil
		{
			chromeState.frozenBaseImage = baseImage
		}
		return result.image
	}

	func applyCaptureFrameEffectIfNeeded(
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

	func captureFrameWindowImage() -> CGImage? {
		guard let windowID = chromeState.captureFrameWindowID else {
			return nil
		}
		return FrozenSelectionImageRenderer.captureFrameWindowImage(windowID: windowID)
	}

	nonisolated static func captureFrameWindowImage(windowID: CGWindowID) -> CGImage? {
		FrozenSelectionImageRenderer.captureFrameWindowImage(windowID: windowID)
	}

	@discardableResult
	func refreshFrozenBaseImageFromDisplay(for selection: CGRect) -> Bool {
		// Export must stay tied to the latched frozen display, not the live desktop.
		let baseImage = frozenBaseImageFromDisplay(for: selection)
		chromeState.frozenSelectionSnapshot = selection
		chromeState.frozenBaseImage = baseImage
		return baseImage != nil
	}

	func ensureFrozenBaseImageFromDisplayIfNeeded(for selection: CGRect) {
		guard chromeState.frozenSelectionSnapshot == selection, chromeState.frozenBaseImage == nil
		else {
			return
		}
		chromeState.frozenBaseImage = frozenBaseImageFromDisplay(for: selection)
	}

	func frozenBaseImageFromDisplay(for selection: CGRect) -> CGImage? {
		guard
			let displayFrame = chromeState.frozenDisplayFrame,
			let displayImage = chromeState.frozenDisplayImage
		else {
			return nil
		}
		return FrozenSelectionImageRenderer.cropFrozenDisplayImage(
			displayImage,
			displayFrame: displayFrame,
			selection: selection
		)
	}

	nonisolated static func cropFrozenDisplayImage(
		_ image: CGImage,
		displayFrame: CGRect,
		selection: CGRect
	) -> CGImage? {
		FrozenSelectionImageRenderer.cropFrozenDisplayImage(
			image,
			displayFrame: displayFrame,
			selection: selection
		)
	}

	nonisolated static func losslessPNGData(
		from image: CGImage,
		screenScaleFactor: CGFloat
	) throws -> Data? {
		try FrozenSelectionImageRenderer.losslessPNGData(
			from: image,
			screenScaleFactor: screenScaleFactor
		)
	}
	func compositeFrozenOverlay(on image: CGImage, selection: CGRect) throws -> CGImage {
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

	func drawExportText(
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
	func nextOutputURL() throws -> URL {
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
}
