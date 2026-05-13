import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	func screen(containing point: CGPoint) -> NSScreen? {
		NSScreen.screens.first(where: { $0.frame.contains(point) })
	}

	func activeMonitor(at point: CGPoint) -> MonitorSnapshot? {
		guard let screen = screen(containing: point) else {
			return nil
		}
		return MonitorSnapshot(
			id: Self.displayID(for: screen) ?? 0,
			frame: screen.frame,
			scaleFactorX1000: UInt32((screen.backingScaleFactor * 1_000).rounded())
		)
	}

	static func displayID(for screen: NSScreen) -> CGDirectDisplayID? {
		(screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
			.uint32Value
	}

	func highlightedWindow(at point: CGPoint) -> WindowSnapshot? {
		overlayController?.hoverWindow(at: point)
	}

	func currentLiveInputs(at point: CGPoint) -> (
		rgb: RGBSample?, activeMonitor: MonitorSnapshot?, highlightedWindow: WindowSnapshot?
	) {
		let chromeSample = overlayController?.liveChromeSnapshot(
			point: point,
			settings: currentSettings,
			includeLoupePatch: scene.loupeVisible
		)
		let rgbSample =
			chromeSample?.rgbSample
			?? frozenFrameAuthority.rgbSample(containing: point)
		let highlightedWindow = highlightedWindow(at: point)
		chromeState.rgbSample = rgbSample
		chromeState.loupePatch = scene.loupeVisible ? chromeSample?.loupePatch : nil
		return (
			rgb: rgbSample,
			activeMonitor: activeMonitor(at: point),
			highlightedWindow: highlightedWindow
		)
	}

	func sendHostStatusMessage(_ message: String) throws {
		guard let session else {
			return
		}
		try session.send(report: .statusMessage(message))
	}

	func setHostStatusMessage(_ message: String) throws {
		try sendHostStatusMessage(message)
		scene.statusMessage = message
	}
	func refreshOverlay() {
		overlayController?.update(
			scene: scene,
			chrome: chromeState,
			settings: settingsStore.settings
		)
		sceneDidChange?(scene)
	}

	func tearDownCapture() {
		let captureID = currentCaptureTelemetryID
		releaseScreenCaptureStreams()
		pendingFrozenCommit = nil
		frozenFrameLatchToken = nil
		frozenSnapshotGeneration &+= 1
		hostEffectJobGeneration &+= 1
		frozenPreparedExportStore.reset()
		completedHostEffect = nil
		removeNativeScrollCaptureMonitor()
		scrollCaptureState = nil
		chromeState = CaptureChromeState()
		overlayController?.close()
		overlayController = nil
		if let appController = NSApp.delegate as? NativeHostApplicationController {
			appController.window = nil
		}
		session = nil
		scene = SceneSnapshot(
			mode: .hidden,
			cursorIntent: .default,
			pointer: nil,
			activeMonitor: nil,
			highlightedWindow: nil,
			liveSelectionPreview: nil,
			frozenSelection: nil,
			rgb: nil,
			loupeVisible: false,
			toolbarItems: [],
			statusMessage: nil
		)
		sceneDidChange?(scene)
		captureStateDidChange?()
		if captureID != 0 {
			NativeHostTelemetry.captureEvent("capture.teardown", captureID: captureID)
		}
		activeCaptureTelemetryID = nil
	}

	func cancelPendingScreenCaptureStreamRelease(reason: String) {
		guard let pendingRelease = pendingLiveFrameStreamRelease else {
			return
		}
		pendingRelease.cancel()
		pendingLiveFrameStreamRelease = nil
		NativeHostTelemetry.captureEvent(
			"capture.stream_release_canceled",
			captureID: currentCaptureTelemetryID,
			detail: "reason=\(reason)"
		)
	}

	func releaseScreenCaptureStreams(immediate: Bool = false) {
		cancelPendingScreenCaptureStreamRelease(reason: "reschedule_release")
		let captureID = currentCaptureTelemetryID
		let scheduledAtUptime = ProcessInfo.processInfo.systemUptime
		let graceMilliseconds = immediate ? 0 : Int(Self.liveFrameStreamReleaseGrace * 1_000)
		NativeHostTelemetry.captureEvent(
			"capture.stream_release_scheduled",
			captureID: captureID,
			detail: "immediate=\(immediate) graceMs=\(graceMilliseconds)"
		)
		let releaseScreenCaptureStreams = { [weak self] in
			guard let self else {
				return
			}
			let elapsedMilliseconds = NativeHostTelemetry.milliseconds(since: scheduledAtUptime)
			NativeHostTelemetry.captureEvent(
				"capture.stream_release_requested",
				captureID: captureID,
				detail: "elapsedMs=\(String(format: "%.2f", elapsedMilliseconds))"
			)
			self.frozenFrameAuthority.stop()
			self.liveFrameStream.stop()
			self.pendingLiveFrameStreamRelease = nil
			NativeHostTelemetry.captureEvent(
				"capture.stream_release_completed",
				captureID: captureID,
				detail: "elapsedMs=\(String(format: "%.2f", elapsedMilliseconds))"
			)
		}
		if immediate {
			releaseScreenCaptureStreams()
			return
		}
		let workItem = DispatchWorkItem(block: releaseScreenCaptureStreams)
		pendingLiveFrameStreamRelease = workItem
		DispatchQueue.main.asyncAfter(
			deadline: .now() + Self.liveFrameStreamReleaseGrace,
			execute: workItem
		)
	}

	@objc
	func settingsDidChange() {
		invalidatePreparedFrozenExport()
		overlayController?.update(
			scene: scene,
			chrome: chromeState,
			settings: settingsStore.settings
		)
	}
}
