import AppKit
import Foundation

@MainActor
final class SettingsShortcutRecorder: ObservableObject {
	enum Target {
		case capture
		case quickScreenshot
	}

	@Published private(set) var target: Target?

	var isRecording: Bool {
		target != nil
	}

	var onRecordingChanged: ((Bool) -> Void)?

	private var commitHandler: ((String) -> Void)?

	func toggle(_ target: Target, onCommit: @escaping (String) -> Void) {
		if self.target == target {
			cancel()
			return
		}
		begin(target, onCommit: onCommit)
	}

	func cancel() {
		guard target != nil else {
			return
		}
		target = nil
		commitHandler = nil
		onRecordingChanged?(false)
	}

	func handleKeyEvent(_ event: NSEvent) -> Bool {
		guard target != nil else {
			return false
		}
		if event.keyCode == 53 {
			cancel()
			return true
		}
		guard
			let title = NativeHostSettings.hotKeyDisplayTitle(
				for: event.modifierFlags,
				keyCode: UInt32(event.keyCode))
		else {
			NSSound.beep()
			return true
		}

		let handler = commitHandler
		target = nil
		commitHandler = nil
		onRecordingChanged?(false)
		DispatchQueue.main.async {
			handler?(title)
		}
		return true
	}

	private func begin(_ target: Target, onCommit: @escaping (String) -> Void) {
		let wasRecording = self.target != nil
		self.target = target
		self.commitHandler = onCommit
		if wasRecording == false {
			onRecordingChanged?(true)
		}
	}
}
