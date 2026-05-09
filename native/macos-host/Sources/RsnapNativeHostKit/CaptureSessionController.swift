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
	static let scrollCaptureEnabled = false
	static let scrollCaptureForwardingPassthrough: TimeInterval = 0.055
	static let scrollCaptureSampleDelay: TimeInterval = 0.04
	static let liveFrameStreamReleaseGrace: TimeInterval = 1.5

	let settingsStore: NativeHostSettingsStore
	let liveFrameStream = LiveFrameStreamBroker()
	let frozenFrameAuthority = FrozenFrameAuthority()
	let frozenCommitQueue = DispatchQueue(
		label: "ink.hack.rsnap.frozen-commit",
		qos: .userInitiated
	)
	let captureSuccessSound = CaptureSuccessSound.load()
	let ocrCompletionSound = OcrCompletionSound.load()
	var session: RsnapHostSession?
	var overlayController: CaptureOverlayController?
	var frozenFrameLatchToken: FrozenFrameLatchToken?
	var pendingFrozenCommit: PendingFrozenCommit?
	var nextPendingFrozenCommitID: UInt64 = 1
	var frozenSnapshotGeneration: UInt64 = 0
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
