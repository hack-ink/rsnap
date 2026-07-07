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
				?? SelectionImageRenderer.renderCopyCaptureJob(request: request)
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
			let result = SelectionImageRenderer.renderSaveCaptureJob(
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
		let result = try SelectionImageRenderer.renderFrozenSelectionImage(
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
		return SelectionImageRenderer.captureFrameWindowImage(windowID: windowID)
	}

	nonisolated static func captureFrameWindowImage(windowID: CGWindowID) -> CGImage? {
		SelectionImageRenderer.captureFrameWindowImage(windowID: windowID)
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
		return SelectionImageRenderer.cropFrozenDisplayImage(
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
		SelectionImageRenderer.cropFrozenDisplayImage(
			image,
			displayFrame: displayFrame,
			selection: selection
		)
	}

	nonisolated static func losslessPNGData(
		from image: CGImage,
		screenScaleFactor: CGFloat
	) throws -> Data? {
		try SelectionImageRenderer.losslessPNGData(
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
