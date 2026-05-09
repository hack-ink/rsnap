import AppKit
import Foundation

struct NativeHostFeedbackSound {
	let sound: NSSound?
	let playFailedEvent: String

	static func load(
		candidatePaths: [String],
		loadedEvent: String,
		loadFailedEvent: String,
		playFailedEvent: String
	) -> Self {
		for path in candidatePaths {
			if let sound = NSSound(contentsOfFile: path, byReference: true) {
				NativeHostTelemetry.lifecycleEvent(
					loadedEvent,
					detail: "path=\(path)"
				)
				return Self(sound: sound, playFailedEvent: playFailedEvent)
			}
		}

		let candidates = candidatePaths.joined(separator: ",")
		NativeHostTelemetry.lifecycleWarning(
			loadFailedEvent,
			detail: "candidates=\(candidates)"
		)
		return Self(sound: nil, playFailedEvent: playFailedEvent)
	}

	func play() {
		guard let sound else {
			return
		}
		sound.stop()
		sound.currentTime = 0
		if sound.play() == false {
			NativeHostTelemetry.lifecycleWarning(playFailedEvent)
		}
	}
}

enum CaptureSuccessSound {
	private static let candidatePaths = [
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Screen Capture.aif",
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Shutter.aif",
	]

	static func load() -> NativeHostFeedbackSound {
		NativeHostFeedbackSound.load(
			candidatePaths: candidatePaths,
			loadedEvent: "native_host.capture_success_sound_loaded",
			loadFailedEvent: "native_host.capture_success_sound_load_failed",
			playFailedEvent: "native_host.capture_success_sound_play_failed"
		)
	}
}

enum OcrCompletionSound {
	private static let candidatePaths = [
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/accessibility/Sticky Keys ON.aif",
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/siri/jbl_confirm.caf",
		"/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Volume Mount.aif",
		"/System/Library/Sounds/Glass.aiff",
	]

	static func load() -> NativeHostFeedbackSound {
		NativeHostFeedbackSound.load(
			candidatePaths: candidatePaths,
			loadedEvent: "native_host.ocr_completion_sound_loaded",
			loadFailedEvent: "native_host.ocr_completion_sound_load_failed",
			playFailedEvent: "native_host.ocr_completion_sound_play_failed"
		)
	}
}
