import RsnapHostBridge

@MainActor
final class HotKeyBindingCoordinator {
	private struct BindingState: Equatable {
		let captureHotKey: String
		let quickScreenshotHotKey: String
		let sceneMode: SceneKind
	}

	var onCaptureRequested: (() -> Void)? {
		get { hotKeys.onCaptureRequested }
		set { hotKeys.onCaptureRequested = newValue }
	}

	var onQuickScreenshotRequested: (() -> Void)? {
		get { hotKeys.onQuickScreenshotRequested }
		set { hotKeys.onQuickScreenshotRequested = newValue }
	}

	var onCancelRequested: (() -> Void)? {
		get { hotKeys.onCancelRequested }
		set { hotKeys.onCancelRequested = newValue }
	}

	var onToggleLoupeRequested: (() -> Void)? {
		get { hotKeys.onToggleLoupeRequested }
		set { hotKeys.onToggleLoupeRequested = newValue }
	}

	var onSaveRequested: (() -> Void)? {
		get { hotKeys.onSaveRequested }
		set { hotKeys.onSaveRequested = newValue }
	}

	private let hotKeys = GlobalHotKeyCenter()
	private var appliedState: BindingState?

	func update(
		captureHotKey: String,
		quickScreenshotHotKey: String,
		sceneMode: SceneKind
	) {
		let state = BindingState(
			captureHotKey: captureHotKey,
			quickScreenshotHotKey: quickScreenshotHotKey,
			sceneMode: sceneMode
		)
		guard state != appliedState else {
			return
		}
		let didApply = hotKeys.updateBindings(
			captureHotKey: state.captureHotKey,
			quickScreenshotHotKey: state.quickScreenshotHotKey,
			sceneMode: state.sceneMode
		)
		appliedState = didApply ? state : nil
	}

	func invalidate() {
		hotKeys.invalidate()
		appliedState = nil
	}

	func suspendBindings() {
		hotKeys.suspendRegisteredHotKeys()
		appliedState = nil
	}
}
