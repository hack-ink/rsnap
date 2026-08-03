import AppKit
import RsnapNativeHostKit

@MainActor
@main
enum RsnapNativeHostMain {
	private static let controller = NativeHostApplicationController()

	static func main() {
		if TextRecognitionHelper.runIfRequested() {
			return
		}
		let application = NSApplication.shared
		application.delegate = controller
		controller.finishLaunching()
		application.run()
	}
}
