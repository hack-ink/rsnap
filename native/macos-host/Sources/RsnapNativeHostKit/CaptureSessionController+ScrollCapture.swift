import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	var scrollCaptureToolbarEnabled: Bool {
		Self.scrollCaptureEnabled
			&& scene.mode == .frozen
			&& scrollCaptureState == nil
			&& currentFrozenSelection() != nil
	}

	func handleScrollCaptureWheel(_ event: NSEvent, at point: CGPoint) -> Bool {
		guard Self.scrollCaptureEnabled else {
			return false
		}
		guard var state = scrollCaptureState else {
			return false
		}
		guard state.viewportRect.contains(point) else {
			return false
		}

		let targetPoint = CGPoint(
			x: point.x.clamped(to: state.viewportRect.minX...state.viewportRect.maxX),
			y: point.y.clamped(to: state.viewportRect.minY...state.viewportRect.maxY)
		)
		let posted =
			overlayController?.withPrimaryMousePassthrough(
				duration: Self.scrollCaptureForwardingPassthrough
			) {
				Self.postScrollWheelEvent(matching: event, at: targetPoint)
			} ?? Self.postScrollWheelEvent(matching: event, at: targetPoint)

		guard posted else {
			try? setHostStatusMessage("Could not forward scroll input.")
			refreshOverlay()
			return true
		}

		state.sampleGeneration &+= 1
		let generation = state.sampleGeneration
		scrollCaptureState = state
		DispatchQueue.main.asyncAfter(deadline: .now() + Self.scrollCaptureSampleDelay) {
			[weak self] in
			self?.observeNativeScrollCaptureFrame(generation: generation)
		}

		return true
	}

	func installNativeScrollCaptureMonitor() {
		removeNativeScrollCaptureMonitor()
		scrollCaptureGlobalMonitor = NSEvent.addGlobalMonitorForEvents(matching: .scrollWheel) {
			[weak self] _ in
			DispatchQueue.main.async { [weak self] in
				self?.scheduleNativeScrollCaptureSampleIfPointerIsInViewport()
			}
		}
	}

	func removeNativeScrollCaptureMonitor() {
		if let monitor = scrollCaptureGlobalMonitor {
			NSEvent.removeMonitor(monitor)
			scrollCaptureGlobalMonitor = nil
		}
		overlayController?.setScrollCaptureMousePassthroughActive(false)
	}

	func scheduleNativeScrollCaptureSampleIfPointerIsInViewport() {
		guard let state = scrollCaptureState else {
			return
		}
		guard state.viewportRect.contains(NSEvent.mouseLocation) else {
			return
		}
		scheduleNativeScrollCaptureSample()
	}

	func scheduleNativeScrollCaptureSample() {
		guard var state = scrollCaptureState else {
			return
		}
		state.sampleGeneration &+= 1
		let generation = state.sampleGeneration
		scrollCaptureState = state
		DispatchQueue.main.asyncAfter(deadline: .now() + Self.scrollCaptureSampleDelay) {
			[weak self] in
			self?.observeNativeScrollCaptureFrame(generation: generation)
		}
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

		ensureFrozenBaseImageFromDisplayIfNeeded(for: selection)
		let baseImage = chromeState.frozenBaseImage ?? frozenBaseImageFromDisplay(for: selection)
		guard let baseImage, let baseSnapshot = NativeHostImageBridge.rgbaSnapshot(from: baseImage)
		else {
			try setHostStatusMessage("Scroll capture could not read the selected region.")
			refreshOverlay()
			return
		}

		let stitcher = try RsnapScrollCaptureSession(
			baseImage: baseSnapshot,
			previewWidthPixels: baseSnapshot.width
		)
		scrollCaptureState = NativeScrollCaptureState(
			stitcher: stitcher,
			viewportRect: selection
		)
		installNativeScrollCaptureMonitor()
		overlayController?.setScrollCaptureMousePassthroughActive(true)
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
		try setHostStatusMessage(
			"Scroll capture started. Scroll inside the selection, then copy or save.")
		refreshOverlay()
	}

	func observeNativeScrollCaptureFrame(generation: UInt64) {
		guard let state = scrollCaptureState, generation <= state.sampleGeneration else {
			return
		}
		guard
			let sampleImage = overlayController?.backgroundPatch(in: state.viewportRect),
			let sample = NativeHostImageBridge.rgbaSnapshot(from: sampleImage)
		else {
			try? setHostStatusMessage("Scroll capture could not sample the scrolled region.")
			refreshOverlay()
			return
		}

		do {
			let result = try state.stitcher.observeDownwardFrame(sample)
			try refreshNativeScrollCapturePreview(
				result: result,
				currentViewportSnapshot: sample
			)
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.scroll_observe_failed",
				captureID: currentCaptureTelemetryID,
				stage: "observe_frame",
				error: String(describing: error)
			)
			try? setHostStatusMessage("Scroll capture could not stitch that frame.")
			refreshOverlay()
		}
	}

	func refreshNativeScrollCapturePreview(
		result: ScrollObserveResult,
		currentViewportSnapshot: RGBARegionSnapshot
	) throws {
		guard let state = scrollCaptureState else {
			return
		}
		guard
			let export = try state.stitcher.exportImage(),
			let exportImage = NativeHostImageBridge.cgImage(from: export)
		else {
			try setHostStatusMessage("Scroll capture could not render the stitched image.")
			refreshOverlay()
			return
		}

		chromeState.frozenSelectionSnapshot = state.viewportRect
		chromeState.frozenSelectionEditable = false
		chromeState.frozenSelectionInteraction = nil
		chromeState.frozenDisplayFrame = nil
		chromeState.frozenDisplayImage = nil
		chromeState.scrollMinimapPreview = ScrollCaptureMinimapSnapshot(
			image: exportImage,
			exportSizePixels: CGSize(width: CGFloat(export.width), height: CGFloat(export.height)),
			viewportTopYPixels: CGFloat(result.currentViewportTopY),
			viewportHeightPixels: CGFloat(currentViewportSnapshot.height)
		)

		if result.outcome == .committed {
			try setHostStatusMessage(
				"Scroll capture appended \(result.growthRows) px. Copy or save exports the stitched image."
			)
		} else if result.outcome == .unsupportedDirection {
			try setHostStatusMessage("Scroll capture only appends downward motion.")
		}
		refreshOverlay()
	}
	static func postScrollWheelEvent(matching event: NSEvent, at point: CGPoint) -> Bool {
		let deltaX = Int32(event.scrollingDeltaX.rounded())
		let deltaY = Int32(event.scrollingDeltaY.rounded())
		guard deltaX != 0 || deltaY != 0 else {
			return false
		}

		let units: CGScrollEventUnit = event.hasPreciseScrollingDeltas ? .pixel : .line
		let wheelCount: UInt32 = deltaX == 0 ? 1 : 2
		guard
			let source = CGEventSource(stateID: .hidSystemState),
			let scrollEvent = CGEvent(
				scrollWheelEvent2Source: source,
				units: units,
				wheelCount: wheelCount,
				wheel1: deltaY,
				wheel2: deltaX,
				wheel3: 0
			)
		else {
			return false
		}

		scrollEvent.location = point
		scrollEvent.post(tap: .cghidEventTap)
		return true
	}
}
