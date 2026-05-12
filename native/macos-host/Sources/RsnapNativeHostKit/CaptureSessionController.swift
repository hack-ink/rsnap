import AppKit
import CoreGraphics
import CoreImage
import CoreText
import Darwin
import Foundation
import QuartzCore
import RsnapHostBridge
import Vision

@MainActor
final class CaptureSessionController: NSObject {
	struct FrozenCaptureJobSource: Sendable {
		let referenceWindowID: CGWindowID
		let desktopFrame: CGRect
	}

	struct PendingFrozenCommit: Sendable {
		let id: UInt64
		let captureID: UInt64
		let generation: UInt64
		let selection: CGRect
		let editable: Bool
		let token: FrozenFrameLatchToken?
		let startedAtUptime: TimeInterval
		let snapshotStartedAtUptime: TimeInterval
		let hadLatchToken: Bool
	}

	static let autoCenterMaxIterations = 6
	static let displayFirstFrameWait: TimeInterval = 0.025
	static let coldSelfCaptureRecoveryWait: TimeInterval = 3.5
	static let scrollCaptureEnabled = true
	static let scrollCaptureMinimumSelectionHeightPixels = 120
	static let scrollCaptureForwardingPassthrough: TimeInterval = 0.012
	static let scrollCaptureControlledScrollSettleDelay: TimeInterval = 0.18
	static let scrollCaptureInputLiveFrameMaxAge: TimeInterval = 0.18
	static let scrollCaptureSampleInterval: TimeInterval = 1.0 / 30.0
	static let scrollCaptureMaxFramesPerSample = 3
	static let scrollCaptureInitialSampleWindow: TimeInterval = 0.35
	static let scrollCaptureInputSampleWindow: TimeInterval = 1.8
	static let scrollCaptureFallbackCaptureInterval: TimeInterval = 0.08
	static let scrollCapturePreviewRefreshInterval: TimeInterval = 0.18
	static let scrollCaptureToolbarBackdropRefreshInterval: TimeInterval = 1.0 / 120.0
	static let scrollCaptureWheelTelemetryInterval: TimeInterval = 0.25
	static let scrollCapturePassthroughWheelMotionHintMultiplier = 3.5
	static let liveFrameStreamReleaseGrace: TimeInterval = 3.0

	let settingsStore: NativeHostSettingsStore
	let liveFrameStream = LiveFrameStreamBroker()
	let frozenFrameAuthority = FrozenFrameAuthority()
	let frozenCommitQueue = DispatchQueue(
		label: "ink.hack.rsnap.frozen-commit",
		qos: .userInitiated
	)
	let scrollCaptureStitchQueue = DispatchQueue(
		label: "ink.hack.rsnap.scroll-capture-stitch",
		qos: .userInitiated
	)
	let frozenImageRenderQueue = DispatchQueue(
		label: "ink.hack.rsnap.frozen-image-render",
		qos: .userInitiated
	)
	let frozenPreparedExportStore = FrozenPreparedExportStore()
	let captureSuccessSound = CaptureSuccessSound.load()
	let ocrCompletionSound = OcrCompletionSound.load()
	var session: RsnapHostSession?
	var overlayController: CaptureOverlayController?
	var frozenFrameLatchToken: FrozenFrameLatchToken?
	var pendingFrozenCommit: PendingFrozenCommit?
	var nextPendingFrozenCommitID: UInt64 = 1
	var frozenSnapshotGeneration: UInt64 = 0
	var hostEffectJobGeneration: UInt64 = 0
	var completedHostEffect: HostEffectKind?
	var scrollCaptureState: NativeScrollCaptureState?
	var scrollCaptureGlobalMonitor: Any?
	var nextCaptureTelemetryID: UInt64 = 1
	var activeCaptureTelemetryID: UInt64?
	var pendingLiveFrameStreamRelease: DispatchWorkItem?
	var captureStateDidChange: (() -> Void)?
	var scene = SceneSnapshot(
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
	var chromeState = CaptureChromeState()
	var sceneDidChange: ((SceneSnapshot) -> Void)?

	init(settingsStore: NativeHostSettingsStore) {
		self.settingsStore = settingsStore
		super.init()
		NotificationCenter.default.addObserver(
			self,
			selector: #selector(settingsDidChange),
			name: NativeHostSettingsStore.didChangeNotification,
			object: settingsStore
		)
	}

	deinit {
		NotificationCenter.default.removeObserver(self)
	}

	var isCaptureActive: Bool {
		session != nil
	}

	var currentSceneMode: SceneKind {
		scene.mode
	}

	var currentSettings: NativeHostSettings {
		settingsStore.settings
	}

	var currentCaptureTelemetryID: UInt64 {
		activeCaptureTelemetryID ?? 0
	}

	var activeTelemetryCaptureID: UInt64 {
		currentCaptureTelemetryID
	}

	func pointTelemetryDetail(_ point: CGPoint) -> String {
		"x=\(Int(point.x.rounded())) y=\(Int(point.y.rounded()))"
	}

}
