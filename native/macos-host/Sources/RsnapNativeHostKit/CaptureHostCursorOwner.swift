import AppKit

@MainActor
final class CaptureHostCursorOwner {
	private var appliedPresentation: CaptureHostCursorPresentation?

	isolated deinit {
		clear()
	}

	func set(_ presentation: CaptureHostCursorPresentation) {
		CaptureHostCursorSupport.cursor(for: presentation).set()
		appliedPresentation = presentation
	}

	func clear() {
		guard appliedPresentation != nil else {
			return
		}
		NSCursor.arrow.set()
		appliedPresentation = nil
	}
}
