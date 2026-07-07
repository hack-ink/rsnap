import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	func frozenSelectionImageRenderRequest() throws -> FrozenSelectionImageRenderRequest? {
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
				let result = SelectionImageRenderer.renderPreparedRecognizeTextImageJob(
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

		let result = SelectionImageRenderer.renderPreparedRecognizeTextImageJob(
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
			let result = SelectionImageRenderer.renderCopyCaptureJob(request: request)
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
}
