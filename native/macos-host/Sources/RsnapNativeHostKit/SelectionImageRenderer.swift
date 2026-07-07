import AppKit
import CoreGraphics
import Darwin
import Foundation
import RsnapHostBridge

enum SelectionImageRenderer {
	static func renderPreparedRecognizeTextImageJob(
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

	static func renderCopyCaptureJob(
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

	static func renderSaveCaptureJob(
		request: FrozenSelectionImageRenderRequest,
		outputURL: URL,
		preparedExportStore: PreparedExportStore
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

	static func renderSaveCaptureJob(
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

	static func renderFrozenSelectionImage(
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

	private static func renderScrollExportImage(
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

	private static func renderDisplayFrozenSelectionImage(
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

	private static func frozenBaseImageFromDisplay(
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

	private static func logFrozenSelectionImageTiming(
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

	private static func applyCaptureFrameEffectIfNeeded(
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

	private static func applyCaptureFrameEffectIfNeeded(
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

	private static func resolvedRenderedImage(
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

	private static func compositeFrozenOverlay(
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

	static func captureFrameWindowImage(windowID: CGWindowID) -> CGImage? {
		guard let createImage = captureFrameWindowListCreateImage else {
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

	private static let captureFrameWindowListCreateImage: CaptureFrameWindowListCreateImage? = {
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

	static func losslessPNGData(
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
}
