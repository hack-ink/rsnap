import AppKit
import CoreGraphics
import Darwin
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
				?? Self.renderCopyCaptureJob(request: request)
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
			let result = Self.renderSaveCaptureJob(
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
				let result = Self.renderPreparedRecognizeTextImageJob(request: request)
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

		let result = Self.renderPreparedRecognizeTextImageJob(request: request)
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
			let result = Self.renderCopyCaptureJob(request: request)
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

	nonisolated private static func renderPreparedRecognizeTextImageJob(
		request: FrozenSelectionImageRenderRequest
	) -> PreparedRecognizeTextImageJobResult {
		let captureImageStartedAt = ProcessInfo.processInfo.systemUptime
		let renderResult: FrozenSelectionImageRenderResult
		do {
			renderResult = try renderFrozenSelectionImage(
				from: request,
				applyingCaptureFrameEffect: false,
				prefersPixelSnapshot: false
			)
		} catch {
			return PreparedRecognizeTextImageJobResult(
				image: nil,
				baseImage: nil,
				captureImageMilliseconds: NativeHostTelemetry.milliseconds(
					since: captureImageStartedAt),
				width: 0,
				height: 0
			)
		}

		let captureImageMilliseconds =
			NativeHostTelemetry.milliseconds(since: captureImageStartedAt)
		let image =
			renderResult.image
			?? renderResult.rgbaSnapshot.flatMap { NativeHostImageBridge.cgImage(from: $0) }
		return PreparedRecognizeTextImageJobResult(
			image: image,
			baseImage: renderResult.baseImage,
			captureImageMilliseconds: captureImageMilliseconds,
			width: renderResult.width,
			height: renderResult.height
		)
	}

	nonisolated private static func renderCopyCaptureJob(
		request: FrozenSelectionImageRenderRequest
	) -> CopyCaptureJobResult {
		let captureImageStartedAt = ProcessInfo.processInfo.systemUptime
		let renderResult: FrozenSelectionImageRenderResult
		do {
			renderResult = try renderFrozenSelectionImage(
				from: request,
				applyingCaptureFrameEffect: true,
				prefersPixelSnapshot: true
			)
		} catch {
			let captureImageMilliseconds =
				NativeHostTelemetry.milliseconds(since: captureImageStartedAt)
			return CopyCaptureJobResult(
				pngData: nil,
				failureStage: "capture_image",
				failureMessage: "Could not capture the frozen selection.",
				captureImageMilliseconds: captureImageMilliseconds,
				makeImageMilliseconds: 0,
				width: 0,
				height: 0,
				cacheHit: false
			)
		}
		let captureImageMilliseconds =
			NativeHostTelemetry.milliseconds(since: captureImageStartedAt)
		guard renderResult.rgbaSnapshot != nil || renderResult.image != nil else {
			return CopyCaptureJobResult(
				pngData: nil,
				failureStage: renderResult.failureStage ?? "capture_image",
				failureMessage: "Could not capture the frozen selection.",
				captureImageMilliseconds: captureImageMilliseconds,
				makeImageMilliseconds: 0,
				width: renderResult.width,
				height: renderResult.height,
				cacheHit: false
			)
		}

		let makeImageStartedAt = ProcessInfo.processInfo.systemUptime
		let pngData: Data?
		let screenScaleFactor = request.captureFrameEnvironment.screenScaleFactor
		if let snapshot = renderResult.rgbaSnapshot {
			pngData = try? RsnapExportEncoder.pngData(
				from: snapshot,
				screenScaleFactor: screenScaleFactor
			)
		} else if let cgImage = renderResult.image {
			pngData = try? losslessPNGData(
				from: cgImage,
				screenScaleFactor: screenScaleFactor
			)
		} else {
			pngData = nil
		}
		let makeImageMilliseconds = NativeHostTelemetry.milliseconds(since: makeImageStartedAt)
		let width = renderResult.rgbaSnapshot?.width ?? renderResult.image?.width ?? 0
		let height = renderResult.rgbaSnapshot?.height ?? renderResult.image?.height ?? 0
		guard let pngData else {
			return CopyCaptureJobResult(
				pngData: nil,
				failureStage: "encode_image",
				failureMessage: "Could not encode the captured image.",
				captureImageMilliseconds: captureImageMilliseconds,
				makeImageMilliseconds: makeImageMilliseconds,
				width: width,
				height: height,
				cacheHit: false
			)
		}
		return CopyCaptureJobResult(
			pngData: pngData,
			failureStage: "none",
			failureMessage: "",
			captureImageMilliseconds: captureImageMilliseconds,
			makeImageMilliseconds: makeImageMilliseconds,
			width: width,
			height: height,
			cacheHit: false
		)
	}

	nonisolated private static func renderSaveCaptureJob(
		request: FrozenSelectionImageRenderRequest,
		outputURL: URL,
		preparedExportStore: FrozenPreparedExportStore
	) -> SaveCaptureJobResult {
		if let preparedResult = preparedExportStore.result(matching: request),
			let pngData = preparedResult.pngData
		{
			let writeStartedAt = ProcessInfo.processInfo.systemUptime
			do {
				try pngData.write(to: outputURL, options: .atomic)
				return SaveCaptureJobResult(
					outputURL: outputURL,
					failureMessage: "",
					failureStage: "none",
					captureImageMilliseconds: 0,
					makeImageMilliseconds: 0,
					writeFileMilliseconds: NativeHostTelemetry.milliseconds(
						since: writeStartedAt),
					width: preparedResult.width,
					height: preparedResult.height,
					cacheHit: true
				)
			} catch {
				return SaveCaptureJobResult(
					outputURL: nil,
					failureMessage: "Could not save the capture.",
					failureStage: "file_write",
					captureImageMilliseconds: 0,
					makeImageMilliseconds: 0,
					writeFileMilliseconds: NativeHostTelemetry.milliseconds(
						since: writeStartedAt),
					width: preparedResult.width,
					height: preparedResult.height,
					cacheHit: true
				)
			}
		}
		return renderSaveCaptureJob(request: request, outputURL: outputURL)
	}

	nonisolated private static func renderSaveCaptureJob(
		request: FrozenSelectionImageRenderRequest,
		outputURL: URL
	) -> SaveCaptureJobResult {
		do {
			let captureImageStartedAt = ProcessInfo.processInfo.systemUptime
			let renderResult = try renderFrozenSelectionImage(
				from: request,
				applyingCaptureFrameEffect: true,
				prefersPixelSnapshot: true
			)
			let captureImageMilliseconds =
				NativeHostTelemetry.milliseconds(since: captureImageStartedAt)
			guard renderResult.rgbaSnapshot != nil || renderResult.image != nil else {
				return SaveCaptureJobResult(
					outputURL: nil,
					failureMessage: "Could not capture the frozen selection.",
					failureStage: renderResult.failureStage ?? "capture_image",
					captureImageMilliseconds: captureImageMilliseconds,
					makeImageMilliseconds: 0,
					writeFileMilliseconds: 0,
					width: renderResult.width,
					height: renderResult.height,
					cacheHit: false
				)
			}
			let makeImageStartedAt = ProcessInfo.processInfo.systemUptime
			let pngData: Data?
			let screenScaleFactor = request.captureFrameEnvironment.screenScaleFactor
			if let snapshot = renderResult.rgbaSnapshot {
				pngData = try RsnapExportEncoder.pngData(
					from: snapshot,
					screenScaleFactor: screenScaleFactor
				)
			} else if let cgImage = renderResult.image {
				pngData = try losslessPNGData(
					from: cgImage,
					screenScaleFactor: screenScaleFactor
				)
			} else {
				pngData = nil
			}
			let makeImageMilliseconds = NativeHostTelemetry.milliseconds(since: makeImageStartedAt)
			let width = renderResult.rgbaSnapshot?.width ?? renderResult.image?.width ?? 0
			let height = renderResult.rgbaSnapshot?.height ?? renderResult.image?.height ?? 0
			guard let pngData else {
				return SaveCaptureJobResult(
					outputURL: nil,
					failureMessage: "Could not encode the captured image.",
					failureStage: "encode_image",
					captureImageMilliseconds: captureImageMilliseconds,
					makeImageMilliseconds: makeImageMilliseconds,
					writeFileMilliseconds: 0,
					width: width,
					height: height,
					cacheHit: false
				)
			}
			let writeStartedAt = ProcessInfo.processInfo.systemUptime
			try pngData.write(to: outputURL, options: .atomic)
			return SaveCaptureJobResult(
				outputURL: outputURL,
				failureMessage: "",
				failureStage: "none",
				captureImageMilliseconds: captureImageMilliseconds,
				makeImageMilliseconds: makeImageMilliseconds,
				writeFileMilliseconds: NativeHostTelemetry.milliseconds(since: writeStartedAt),
				width: width,
				height: height,
				cacheHit: false
			)
		} catch {
			return SaveCaptureJobResult(
				outputURL: nil,
				failureMessage: "Could not save the capture.",
				failureStage: "file_write",
				captureImageMilliseconds: 0,
				makeImageMilliseconds: 0,
				writeFileMilliseconds: 0,
				width: 0,
				height: 0,
				cacheHit: false
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
		let result = try Self.renderFrozenSelectionImage(
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

	nonisolated private static func renderFrozenSelectionImage(
		from request: FrozenSelectionImageRenderRequest,
		applyingCaptureFrameEffect: Bool,
		prefersPixelSnapshot: Bool = false
	) throws -> FrozenSelectionImageRenderResult {
		let captureStartedAt = ProcessInfo.processInfo.systemUptime
		if let scrollExport = request.scrollExportSnapshot {
			return renderScrollExportImage(
				scrollExport,
				request: request,
				captureStartedAt: captureStartedAt,
				applyingCaptureFrameEffect: applyingCaptureFrameEffect,
				prefersPixelSnapshot: prefersPixelSnapshot
			)
		}
		return try renderDisplayFrozenSelectionImage(
			from: request,
			captureStartedAt: captureStartedAt,
			applyingCaptureFrameEffect: applyingCaptureFrameEffect,
			prefersPixelSnapshot: prefersPixelSnapshot
		)
	}

	nonisolated private static func renderScrollExportImage(
		_ scrollExport: RGBARegionSnapshot,
		request: FrozenSelectionImageRenderRequest,
		captureStartedAt: TimeInterval,
		applyingCaptureFrameEffect: Bool,
		prefersPixelSnapshot: Bool
	) -> FrozenSelectionImageRenderResult {
		let base = FrozenRenderedImage(image: nil, rgbaSnapshot: scrollExport)
		let result =
			applyingCaptureFrameEffect
			? applyCaptureFrameEffectIfNeeded(
				to: base,
				request: request,
				hasOverlayEdits: false,
				prefersPixelSnapshot: prefersPixelSnapshot
			)
			: resolvedRenderedImage(base, prefersPixelSnapshot: prefersPixelSnapshot)
		logFrozenSelectionImageTiming(
			request: request,
			captureStartedAt: captureStartedAt,
			ensureMilliseconds: 0,
			refreshMilliseconds: 0,
			compositeMilliseconds: 0,
			source: "scroll_capture_export",
			success: true,
			width: result.width,
			height: result.height,
			hasOverlayEdits: false
		)
		return FrozenSelectionImageRenderResult(
			image: result.image,
			rgbaSnapshot: result.rgbaSnapshot,
			baseImage: nil,
			failureStage: nil,
			ensureMilliseconds: 0,
			refreshMilliseconds: 0,
			compositeMilliseconds: 0,
			source: "scroll_capture_export",
			hasOverlayEdits: false,
			width: result.width,
			height: result.height
		)
	}

	nonisolated private static func renderDisplayFrozenSelectionImage(
		from request: FrozenSelectionImageRenderRequest,
		captureStartedAt: TimeInterval,
		applyingCaptureFrameEffect: Bool,
		prefersPixelSnapshot: Bool
	) throws -> FrozenSelectionImageRenderResult {
		let snapshotMatchedBefore = request.frozenSelectionSnapshot == request.selection
		let hadBaseImageBefore = request.frozenBaseImage != nil
		let hadFrozenDisplayImageBefore = request.frozenDisplayImage != nil
		let ensureStartedAt = ProcessInfo.processInfo.systemUptime
		let baseImage =
			snapshotMatchedBefore
			? request.frozenBaseImage
				?? frozenBaseImageFromDisplay(
					displayFrame: request.frozenDisplayFrame,
					displayImage: request.frozenDisplayImage,
					selection: request.selection
				)
			: frozenBaseImageFromDisplay(
				displayFrame: request.frozenDisplayFrame,
				displayImage: request.frozenDisplayImage,
				selection: request.selection
			)
		let ensureMilliseconds = NativeHostTelemetry.milliseconds(since: ensureStartedAt)
		guard let baseImage else {
			logFrozenSelectionImageTiming(
				request: request,
				captureStartedAt: captureStartedAt,
				ensureMilliseconds: ensureMilliseconds,
				refreshMilliseconds: 0,
				compositeMilliseconds: 0,
				source: "missing_base",
				success: false,
				width: 0,
				height: 0,
				hasOverlayEdits: request.hasOverlayEdits
			)
			return FrozenSelectionImageRenderResult(
				image: nil,
				rgbaSnapshot: nil,
				baseImage: nil,
				failureStage: "capture_image",
				ensureMilliseconds: ensureMilliseconds,
				refreshMilliseconds: 0,
				compositeMilliseconds: 0,
				source: "missing_base",
				hasOverlayEdits: request.hasOverlayEdits,
				width: 0,
				height: 0
			)
		}

		let compositeStartedAt = ProcessInfo.processInfo.systemUptime
		let composited = try compositeFrozenOverlay(
			on: baseImage,
			selection: request.selection,
			elements: request.overlayElements,
			prefersPixelSnapshot: prefersPixelSnapshot || applyingCaptureFrameEffect
		)
		let result =
			applyingCaptureFrameEffect
			? applyCaptureFrameEffectIfNeeded(
				to: composited,
				request: request,
				hasOverlayEdits: request.hasOverlayEdits,
				prefersPixelSnapshot: prefersPixelSnapshot
			)
			: composited
		let compositeMilliseconds = NativeHostTelemetry.milliseconds(since: compositeStartedAt)
		let imageSource: String
		if snapshotMatchedBefore, hadBaseImageBefore {
			imageSource = "cached_base"
		} else if hadFrozenDisplayImageBefore {
			imageSource = "frozen_display_crop"
		} else {
			imageSource = "unknown_base"
		}
		logFrozenSelectionImageTiming(
			request: request,
			captureStartedAt: captureStartedAt,
			ensureMilliseconds: ensureMilliseconds,
			refreshMilliseconds: 0,
			compositeMilliseconds: compositeMilliseconds,
			source: imageSource,
			success: true,
			width: result.width,
			height: result.height,
			hasOverlayEdits: request.hasOverlayEdits
		)
		return FrozenSelectionImageRenderResult(
			image: result.image,
			rgbaSnapshot: result.rgbaSnapshot,
			baseImage: baseImage,
			failureStage: nil,
			ensureMilliseconds: ensureMilliseconds,
			refreshMilliseconds: 0,
			compositeMilliseconds: compositeMilliseconds,
			source: imageSource,
			hasOverlayEdits: request.hasOverlayEdits,
			width: result.width,
			height: result.height
		)
	}

	nonisolated private static func frozenBaseImageFromDisplay(
		displayFrame: CGRect?,
		displayImage: CGImage?,
		selection: CGRect
	) -> CGImage? {
		guard let displayFrame, let displayImage else {
			return nil
		}
		return cropFrozenDisplayImage(
			displayImage,
			displayFrame: displayFrame,
			selection: selection
		)
	}

	nonisolated private static func logFrozenSelectionImageTiming(
		request: FrozenSelectionImageRenderRequest,
		captureStartedAt: TimeInterval,
		ensureMilliseconds: Double,
		refreshMilliseconds: Double,
		compositeMilliseconds: Double,
		source: String,
		success: Bool,
		width: Int,
		height: Int,
		hasOverlayEdits: Bool
	) {
		NativeHostTelemetry.frozenSelectionImageTiming(
			captureID: request.captureID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: captureStartedAt),
			ensureMilliseconds: ensureMilliseconds,
			refreshMilliseconds: refreshMilliseconds,
			compositeMilliseconds: compositeMilliseconds,
			source: source,
			success: success,
			width: width,
			height: height,
			hasOverlayEdits: hasOverlayEdits
		)
	}

	nonisolated private static func applyCaptureFrameEffectIfNeeded(
		to image: CGImage,
		request: FrozenSelectionImageRenderRequest,
		hasOverlayEdits: Bool,
		prefersPixelSnapshot: Bool
	) -> FrozenRenderedImage {
		applyCaptureFrameEffectIfNeeded(
			to: FrozenRenderedImage(image: image, rgbaSnapshot: nil),
			request: request,
			hasOverlayEdits: hasOverlayEdits,
			prefersPixelSnapshot: prefersPixelSnapshot
		)
	}

	nonisolated private static func applyCaptureFrameEffectIfNeeded(
		to image: FrozenRenderedImage,
		request: FrozenSelectionImageRenderRequest,
		hasOverlayEdits: Bool,
		prefersPixelSnapshot: Bool
	) -> FrozenRenderedImage {
		guard request.captureFrameEffectEnabled,
			request.captureFrameApplicability.includes(request.captureFrameSource)
		else {
			return resolvedRenderedImage(image, prefersPixelSnapshot: prefersPixelSnapshot)
		}
		if hasOverlayEdits == false,
			request.captureFrameSource == .window,
			let windowID = request.captureFrameWindowID,
			let windowImage = captureFrameWindowImage(windowID: windowID)
		{
			guard
				let snapshot = CaptureFrameEffectRenderer.renderWindowSnapshotSnapshot(
					image: windowImage,
					background: request.captureFrameBackground,
					environment: request.captureFrameEnvironment
				)
			else {
				return resolvedRenderedImage(image, prefersPixelSnapshot: prefersPixelSnapshot)
			}

			let renderedImage =
				prefersPixelSnapshot
				? nil
				: NativeHostImageBridge.cgImage(from: snapshot, shouldInterpolate: true)
			if prefersPixelSnapshot == false, renderedImage == nil {
				return resolvedRenderedImage(image, prefersPixelSnapshot: prefersPixelSnapshot)
			}
			return FrozenRenderedImage(
				image: renderedImage,
				rgbaSnapshot: snapshot
			)
		}
		let renderedSnapshot: RGBARegionSnapshot?
		if let sourceSnapshot = image.rgbaSnapshot {
			renderedSnapshot = CaptureFrameEffectRenderer.renderSnapshot(
				source: sourceSnapshot,
				background: request.captureFrameBackground,
				sourceKind: request.captureFrameSource,
				environment: request.captureFrameEnvironment
			)
		} else if let sourceImage = image.image {
			renderedSnapshot = CaptureFrameEffectRenderer.renderSnapshot(
				image: sourceImage,
				background: request.captureFrameBackground,
				source: request.captureFrameSource,
				environment: request.captureFrameEnvironment
			)
		} else {
			renderedSnapshot = nil
		}
		guard let renderedSnapshot else {
			return resolvedRenderedImage(image, prefersPixelSnapshot: prefersPixelSnapshot)
		}

		let renderedImage =
			prefersPixelSnapshot
			? nil
			: NativeHostImageBridge.cgImage(from: renderedSnapshot, shouldInterpolate: true)
		if prefersPixelSnapshot == false, renderedImage == nil {
			return resolvedRenderedImage(image, prefersPixelSnapshot: prefersPixelSnapshot)
		}
		return FrozenRenderedImage(
			image: renderedImage,
			rgbaSnapshot: renderedSnapshot
		)
	}

	nonisolated private static func resolvedRenderedImage(
		_ image: FrozenRenderedImage,
		prefersPixelSnapshot: Bool
	) -> FrozenRenderedImage {
		if prefersPixelSnapshot || image.image != nil {
			return image
		}
		guard let snapshot = image.rgbaSnapshot else {
			return image
		}
		return FrozenRenderedImage(
			image: NativeHostImageBridge.cgImage(from: snapshot),
			rgbaSnapshot: snapshot
		)
	}

	nonisolated private static func compositeFrozenOverlay(
		on image: CGImage,
		selection: CGRect,
		elements: [FrozenOverlayExportElement],
		prefersPixelSnapshot: Bool
	) throws -> FrozenRenderedImage {
		guard elements.isEmpty == false else {
			return FrozenRenderedImage(image: image, rgbaSnapshot: nil)
		}

		guard
			let snapshot = NativeHostImageBridge.rgbaSnapshot(from: image),
			let renderedSnapshot = try? RsnapExportEncoder.frozenOverlayExportImage(
				from: snapshot,
				selection: selection,
				elements: elements
			)
		else {
			throw HostBridgeError.ffiStatus(
				context: "converting frozen overlay export image",
				code: 4)
		}

		return FrozenRenderedImage(
			image: prefersPixelSnapshot
				? nil
				: NativeHostImageBridge.cgImage(from: renderedSnapshot),
			rgbaSnapshot: renderedSnapshot
		)
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
		return Self.captureFrameWindowImage(windowID: windowID)
	}

	nonisolated static func captureFrameWindowImage(windowID: CGWindowID) -> CGImage? {
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

	nonisolated static func cropFrozenDisplayImage(
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

	nonisolated static func losslessPNGData(
		from image: CGImage,
		screenScaleFactor: CGFloat
	) throws -> Data? {
		guard let snapshot = NativeHostImageBridge.rgbaSnapshot(from: image) else {
			return nil
		}

		return try RsnapExportEncoder.pngData(
			from: snapshot,
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
