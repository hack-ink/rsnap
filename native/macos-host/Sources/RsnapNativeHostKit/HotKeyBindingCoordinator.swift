import RsnapHostBridge

@MainActor
final class HotKeyBindingCoordinator {
	private struct BindingState: Equatable {
		let captureHotKey: String
		let sceneMode: SceneKind
		let plainFrozenShortcutsEnabled: Bool
	}

	var onCaptureRequested: (() -> Void)? {
		get { hotKeys.onCaptureRequested }
		set { hotKeys.onCaptureRequested = newValue }
	}

	var onCancelRequested: (() -> Void)? {
		get { hotKeys.onCancelRequested }
		set { hotKeys.onCancelRequested = newValue }
	}

	var onToggleLoupeRequested: (() -> Void)? {
		get { hotKeys.onToggleLoupeRequested }
		set { hotKeys.onToggleLoupeRequested = newValue }
	}

	var onCopyRequested: (() -> Void)? {
		get { hotKeys.onCopyRequested }
		set { hotKeys.onCopyRequested = newValue }
	}

	var onAutoCenterRequested: (() -> Void)? {
		get { hotKeys.onAutoCenterRequested }
		set { hotKeys.onAutoCenterRequested = newValue }
	}

	var onSaveRequested: (() -> Void)? {
		get { hotKeys.onSaveRequested }
		set { hotKeys.onSaveRequested = newValue }
	}

	private let hotKeys = GlobalHotKeyCenter()
	private var appliedState: BindingState?

	func update(
		captureHotKey: String,
		sceneMode: SceneKind,
		plainFrozenShortcutsEnabled: Bool
	) {
		let state = BindingState(
			captureHotKey: captureHotKey,
			sceneMode: sceneMode,
			plainFrozenShortcutsEnabled: plainFrozenShortcutsEnabled
		)
		guard state != appliedState else {
			return
		}
		let didApply = hotKeys.updateBindings(
			captureHotKey: state.captureHotKey,
			sceneMode: state.sceneMode,
			plainFrozenShortcutsEnabled: state.plainFrozenShortcutsEnabled
		)
		appliedState = didApply ? state : nil
	}

	func invalidate() {
		hotKeys.invalidate()
		appliedState = nil
	}
}
