import AppKit
@preconcurrency import CoreGraphics
import Foundation
import RsnapHostBridge

package func scrollCaptureViewportPoint(
	for point: CGPoint,
	in viewportRect: CGRect,
	desktopFrame: CGRect? = nil
) -> CGPoint? {
	for candidate in scrollCaptureViewportPointCandidates(for: point, desktopFrame: desktopFrame)
	where viewportRect.inclusivelyContains(candidate) {
		return candidate
	}
	return nil
}

private func scrollCaptureViewportPointCandidates(
	for point: CGPoint,
	desktopFrame: CGRect?
) -> [CGPoint] {
	let desktop =
		desktopFrame
		?? NSScreen.screens.reduce(CGRect.null) { partial, screen in
			partial.union(screen.frame)
		}
	guard desktop.isNull == false else {
		return [point]
	}
	let flippedPoint = CGPoint(
		x: point.x,
		y: desktop.minY + desktop.maxY - point.y
	)
	if abs(flippedPoint.y - point.y) <= 0.5 {
		return [point]
	}
	return [point, flippedPoint]
}

package struct ScrollCaptureObservedInputPoint: Equatable {
	package let viewportPoint: CGPoint
	package let inputSource: String
	package let insideViewport: Bool
}

package func scrollCaptureObservedInputPoint(
	for point: CGPoint,
	viewportRect: CGRect,
	sourceFrame: CGRect,
	desktopFrame: CGRect? = nil,
	padding: CGFloat
) -> ScrollCaptureObservedInputPoint? {
	if let viewportPoint = scrollCaptureViewportPoint(
		for: point,
		in: viewportRect,
		desktopFrame: desktopFrame
	) {
		return ScrollCaptureObservedInputPoint(
			viewportPoint: viewportPoint,
			inputSource: "viewport",
			insideViewport: true
		)
	}

	let expandedViewport = CGRect(
		x: viewportRect.minX - padding,
		y: viewportRect.minY - padding,
		width: viewportRect.width + padding * 2,
		height: viewportRect.height + padding * 2
	)
	if expandedViewport.inclusivelyContains(point) {
		let viewportPoint = CGPoint(
			x: point.x.clamped(to: viewportRect.minX...viewportRect.maxX),
			y: point.y.clamped(to: viewportRect.minY...viewportRect.maxY)
		)
		return ScrollCaptureObservedInputPoint(
			viewportPoint: viewportPoint,
			inputSource: "near_viewport",
			insideViewport: false
		)
	}
	let expandedSource = sourceFrame.isNull ? CGRect.null : sourceFrame.insetBy(dx: -8, dy: -8)
	for candidate in scrollCaptureViewportPointCandidates(
		for: point,
		desktopFrame: desktopFrame
	) {
		let sourceContainsCandidate =
			expandedSource.isNull == false && expandedSource.inclusivelyContains(candidate)
		let nearViewport = expandedViewport.inclusivelyContains(candidate)
		guard sourceContainsCandidate || nearViewport else {
			continue
		}
		let viewportPoint = CGPoint(
			x: candidate.x.clamped(to: viewportRect.minX...viewportRect.maxX),
			y: candidate.y.clamped(to: viewportRect.minY...viewportRect.maxY)
		)
		return ScrollCaptureObservedInputPoint(
			viewportPoint: viewportPoint,
			inputSource: sourceContainsCandidate ? "capture_source" : "near_viewport",
			insideViewport: false
		)
	}

	return nil
}

private struct NativeScrollCaptureGeometry {
	let baseImage: CGImage
	let baseSnapshot: RGBARegionSnapshot
	let pixelRect: CGRect
	let samplingRect: CGRect
}

extension CaptureSessionController {
	var scrollCaptureToolbarEnabled: Bool {
		guard Self.scrollCaptureEnabled,
			scene.mode == .frozen,
			scrollCaptureState == nil,
			chromeState.frozenSelectionEditable,
			let selection = currentFrozenSelection()
		else {
			return false
		}
		return scrollCaptureSelectionHasSufficientHeight(selection)
	}

	func scheduleNativeScrollCaptureSample(
		extendingWindowBy window: TimeInterval =
			CaptureSessionController.scrollCaptureInputSampleWindow,
		delay: TimeInterval = CaptureSessionController.scrollCaptureSampleInterval
	) {
		guard var state = scrollCaptureState else {
			return
		}

		let now = ProcessInfo.processInfo.systemUptime
		state.sampleUntilUptime = max(state.sampleUntilUptime, now + max(window, 0))
		guard state.sampleLoopScheduled == false, state.sampleDrainProcessing == false else {
			scrollCaptureState = state
			if nativeScrollCaptureToolbarBackdropShouldLoop(state: state) {
				scheduleNativeScrollCaptureToolbarBackdropRefresh()
			}
			return
		}

		state.sampleLoopScheduled = true
		scrollCaptureState = state
		if nativeScrollCaptureToolbarBackdropShouldLoop(state: state) {
			scheduleNativeScrollCaptureToolbarBackdropRefresh()
		}
		DispatchQueue.main.asyncAfter(deadline: .now() + max(delay, 0)) {
			[weak self] in
			self?.observeNativeScrollCaptureFrame()
		}
	}

	func scheduleNativeScrollCaptureToolbarBackdropRefresh() {
		guard var state = scrollCaptureState,
			state.toolbarBackdropLoopScheduled == false,
			nativeScrollCaptureToolbarBackdropShouldLoop(state: state)
		else {
			return
		}
		state.toolbarBackdropLoopScheduled = true
		scrollCaptureState = state
		DispatchQueue.main.asyncAfter(
			deadline: .now() + Self.scrollCaptureToolbarBackdropRefreshInterval
		) { [weak self] in
			self?.refreshNativeScrollCaptureToolbarBackdrop()
		}
	}

	func refreshNativeScrollCaptureToolbarBackdrop() {
		guard var state = scrollCaptureState else {
			return
		}
		state.toolbarBackdropLoopScheduled = false
		scrollCaptureState = state
		guard nativeScrollCaptureToolbarBackdropShouldLoop(state: state) else {
			return
		}
		overlayController?.refreshScrollCaptureToolbarBackdropNow()
		guard let latestState = scrollCaptureState,
			nativeScrollCaptureToolbarBackdropShouldLoop(state: latestState)
		else {
			return
		}
		scheduleNativeScrollCaptureToolbarBackdropRefresh()
	}

	func scheduleNativeScrollCaptureSampleIfNeeded() {
		guard let latestState = scrollCaptureState,
			nativeScrollCaptureShouldKeepSampling(state: latestState)
		else {
			return
		}
		scheduleNativeScrollCaptureSample(
			extendingWindowBy: 0,
			delay: nativeScrollCaptureNextSampleDelay(state: latestState)
		)
	}

	func beginNativeScrollCapture() throws {
		guard Self.scrollCaptureEnabled else {
			try setHostStatusMessage("Scroll capture is temporarily disabled.")
			refreshOverlay()
			return
		}
		guard scrollCaptureState == nil else {
			try setHostStatusMessage("Scroll capture is already active.")
			refreshOverlay()
			return
		}
		guard scene.mode == .frozen, let selection = currentFrozenSelection() else {
			try setHostStatusMessage("Scroll capture requires a frozen selection.")
			refreshOverlay()
			return
		}
		guard chromeState.frozenSelectionEditable else {
			try setHostStatusMessage("Scroll capture requires a dragged region selection.")
			refreshOverlay()
			return
		}
		guard scrollCaptureSelectionHasSufficientHeight(selection) else {
			try setHostStatusMessage("Select a taller region before starting Scroll Capture.")
			refreshOverlay()
			return
		}

		guard
			let captureSource = overlayController?.scrollCaptureFallbackSource(
				near: CGPoint(x: selection.midX, y: selection.midY)
			)
		else {
			try setHostStatusMessage("Scroll capture could not locate the overlay window.")
			refreshOverlay()
			return
		}

		guard let geometry = nativeScrollCaptureGeometry(for: selection)
		else {
			try setHostStatusMessage("Scroll capture could not read the selected region.")
			refreshOverlay()
			return
		}
		let baseImage = geometry.baseImage
		let baseSnapshot = geometry.baseSnapshot
		let baseSource = "frozen_display_region"
		debugDumpNativeScrollCaptureSnapshot(baseSnapshot, name: "base-\(baseSource)")
		let stitcher = try RsnapScrollCaptureSession(
			baseImage: baseSnapshot,
			previewWidthPixels: min(
				baseSnapshot.width,
				Self.scrollCapturePreviewImageWidthPixels
			)
		)
		var initialState = NativeScrollCaptureState(
			stitcher: stitcher,
			viewportRect: selection,
			viewportPixelRect: geometry.pixelRect,
			viewportSamplingRect: geometry.samplingRect,
			captureSource: captureSource,
			viewportPixelsPerPointY: Double(geometry.pixelRect.height)
				/ max(Double(selection.height), 1)
		)
		initialState.sampleUntilUptime =
			ProcessInfo.processInfo.systemUptime + Self.scrollCaptureInitialSampleWindow
		scrollCaptureState = initialState
		installNativeScrollCaptureMonitor()
		overlayController?.prepareCaptureStreamsNow(trigger: "scroll_capture_start")
		prepareNativeScrollCaptureLiveStream(for: selection)
		configureNativeScrollCaptureChrome(
			baseImage: baseImage,
			baseSnapshot: baseSnapshot,
			selection: selection
		)
		try setHostStatusMessage("Scroll capture started. Scroll inside the selection.")
		NativeHostTelemetry.captureEvent(
			"capture.scroll_capture_started",
			captureID: currentCaptureTelemetryID,
			detail:
				"width=\(baseSnapshot.width),height=\(baseSnapshot.height),x=\(Int(selection.minX.rounded())),y=\(Int(selection.minY.rounded())),pixelX=\(Int(geometry.pixelRect.minX.rounded())),pixelY=\(Int(geometry.pixelRect.minY.rounded())),pixelWidth=\(Int(geometry.pixelRect.width.rounded())),pixelHeight=\(Int(geometry.pixelRect.height.rounded())),samplingX=\(Int(geometry.samplingRect.minX.rounded())),samplingY=\(Int(geometry.samplingRect.minY.rounded())),samplingWidth=\(Int(geometry.samplingRect.width.rounded())),samplingHeight=\(Int(geometry.samplingRect.height.rounded())),mode=manual_universal,baseSource=\(baseSource),liveBaseMatched=false"
		)
		NativeHostTelemetry.captureEvent(
			"capture.scroll_capture_mode",
			captureID: currentCaptureTelemetryID,
			outcome: "manual_universal",
			detail:
				"input=selection_passthrough_global_monitor,permission=screen_recording,accessibility_required=false"
		)
		overlayController?.focusWindow(at: CGPoint(x: selection.midX, y: selection.midY))
		overlayController?.setScrollCaptureMousePassthroughActive(true)
		NativeHostTelemetry.captureEvent(
			"capture.scroll_input_ready",
			captureID: currentCaptureTelemetryID,
			detail:
				"input=selection_passthrough_global_monitor,overlay=focused,passthrough=window"
		)
		DispatchQueue.main.async { [weak self] in
			self?.refreshOverlay()
		}
		scheduleNativeScrollCaptureSample(
			extendingWindowBy: Self.scrollCaptureInitialSampleWindow
		)
	}

	private func prepareNativeScrollCaptureLiveStream(for selection: CGRect) {
		cancelPendingScreenCaptureStreamRelease(reason: "scroll_capture_start")
		let prewarmPoint = CGPoint(x: selection.midX, y: selection.midY)
		liveFrameStream.start(
			for: NSScreen.screens,
			prewarmPoint: prewarmPoint,
			captureID: currentCaptureTelemetryID
		)
		liveFrameStream.prime(at: prewarmPoint)
	}

	private func nativeScrollCaptureGeometry(
		for selection: CGRect
	) -> NativeScrollCaptureGeometry? {
		guard
			let displayFrame = chromeState.frozenDisplayFrame,
			let displayImage = chromeState.frozenDisplayImage,
			let pixelRect = try? RsnapExportEncoder.frozenDisplayCropRect(
				imageWidth: displayImage.width,
				imageHeight: displayImage.height,
				displayFrame: displayFrame,
				selection: selection
			),
			let baseImage = displayImage.cropping(to: pixelRect),
			let baseSnapshot = NativeHostImageBridge.rgbaSnapshot(from: baseImage)
		else {
			return nil
		}
		let samplingRect = Self.nativeScrollCaptureSamplingRect(
			pixelRect: pixelRect,
			displayFrame: displayFrame,
			displayImageSize: CGSize(
				width: CGFloat(displayImage.width),
				height: CGFloat(displayImage.height)
			)
		)

		return NativeScrollCaptureGeometry(
			baseImage: baseImage,
			baseSnapshot: baseSnapshot,
			pixelRect: pixelRect,
			samplingRect: samplingRect
		)
	}

	private static func nativeScrollCaptureSamplingRect(
		pixelRect: CGRect,
		displayFrame: CGRect,
		displayImageSize: CGSize
	) -> CGRect {
		let pointsPerPixelX = displayFrame.width / max(displayImageSize.width, 1)
		let pointsPerPixelY = displayFrame.height / max(displayImageSize.height, 1)
		let height = pixelRect.height * pointsPerPixelY
		let maxY = displayFrame.maxY - pixelRect.minY * pointsPerPixelY
		return CGRect(
			x: displayFrame.minX + pixelRect.minX * pointsPerPixelX,
			y: maxY - height,
			width: pixelRect.width * pointsPerPixelX,
			height: height
		)
	}

	private func configureNativeScrollCaptureChrome(
		baseImage: CGImage,
		baseSnapshot: RGBARegionSnapshot,
		selection: CGRect
	) {
		chromeState.frozenOverlay.reset()
		chromeState.frozenSelectionEditable = false
		chromeState.frozenSelectionInteraction = nil
		chromeState.frozenSelectionSnapshot = selection
		chromeState.captureFrameSource = .scrollCapture
		chromeState.captureFrameWindowID = nil
		chromeState.frozenDisplayFrame = nil
		chromeState.frozenDisplayImage = nil
		chromeState.frozenBaseImage = baseImage
		chromeState.scrollMinimapPreview = ScrollCaptureMinimapSnapshot(
			image: baseImage,
			exportSizePixels: CGSize(
				width: CGFloat(baseSnapshot.width),
				height: CGFloat(baseSnapshot.height)
			),
			viewportTopYPixels: 0,
			viewportHeightPixels: CGFloat(baseSnapshot.height)
		)
	}

	func scrollCaptureSelectionHasSufficientHeight(_ selection: CGRect) -> Bool {
		scrollCaptureSelectionHeightPixels(selection)
			>= Self.scrollCaptureMinimumSelectionHeightPixels
	}

	func scrollCaptureSelectionHeightPixels(_ selection: CGRect) -> Int {
		if chromeState.frozenSelectionSnapshot == selection,
			let frozenBaseImage = chromeState.frozenBaseImage
		{
			return frozenBaseImage.height
		}
		let point = CGPoint(x: selection.midX, y: selection.midY)
		let scale =
			screen(containing: point)?.backingScaleFactor
			?? NSScreen.main?.backingScaleFactor
			?? 1
		return Int((selection.height * scale).rounded())
	}
}
