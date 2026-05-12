import AppKit
import CoreGraphics
import Darwin
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	func performCopy() throws {
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

	func performSave() throws {
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
	func activeScrollCaptureExportImage() throws -> CGImage? {
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

	func captureFrozenSelectionImage(applyingCaptureFrameEffect: Bool = false) throws
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
			let result =
				applyingCaptureFrameEffect
				? applyCaptureFrameEffectIfNeeded(
					to: scrollExport,
					selection: selection,
					hasOverlayEdits: false
				)
				: scrollExport
			NativeHostTelemetry.frozenSelectionImageTiming(
				captureID: currentCaptureTelemetryID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
				ensureMilliseconds: 0,
				refreshMilliseconds: 0,
				compositeMilliseconds: 0,
				source: "scroll_capture_export",
				success: true,
				width: result.width,
				height: result.height,
				hasOverlayEdits: false
			)
			return result
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

	typealias CaptureFrameWindowListCreateImage =
		@convention(c) (
			CGRect,
			UInt32,
			CGWindowID,
			UInt32
		) -> Unmanaged<CGImage>?

	nonisolated static let captureFrameWindowListCreateImage: CaptureFrameWindowListCreateImage? = {
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
		return Self.cropFrozenDisplayImage(
			displayImage,
			displayFrame: displayFrame,
			selection: selection
		)
	}

	static func cropFrozenDisplayImage(
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

	static func losslessPNGData(from image: CGImage) throws -> Data? {
		guard let snapshot = NativeHostImageBridge.rgbaSnapshot(from: image) else {
			return nil
		}

		return try RsnapExportEncoder.pngData(from: snapshot)
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
