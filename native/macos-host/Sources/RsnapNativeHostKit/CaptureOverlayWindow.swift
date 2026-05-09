import AppKit
import Foundation
import RsnapHostBridge

@MainActor
final class CaptureOverlayWindow: NSPanel {
	let hostView: CaptureHostView

	override var canBecomeKey: Bool { true }
	override var canBecomeMain: Bool { false }

	init(
		screen: NSScreen,
		controller: CaptureSessionController?,
		initialScene: SceneSnapshot,
		initialChrome: CaptureChromeState,
		initialSettings: NativeHostSettings
	) {
		hostView = CaptureHostView(frame: screen.frame)
		super.init(
			contentRect: screen.frame,
			styleMask: [.borderless, .nonactivatingPanel],
			backing: .buffered,
			defer: false
		)

		setFrame(screen.frame, display: false)
		hostView.controller = controller
		hostView.seedInitialState(
			scene: initialScene,
			chrome: initialChrome,
			settings: initialSettings
		)
		contentView = hostView
		acceptsMouseMovedEvents = true
		animationBehavior = .none
		backgroundColor = .clear
		collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
		hasShadow = false
		hidesOnDeactivate = false
		ignoresMouseEvents = false
		isFloatingPanel = true
		isMovable = false
		isOpaque = false
		level = .screenSaver
		sharingType = .readOnly
		titleVisibility = .hidden
		titlebarAppearsTransparent = true
	}
}
