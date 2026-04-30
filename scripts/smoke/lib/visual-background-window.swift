import AppKit
import Foundation

final class VisualBackgroundDelegate: NSObject, NSApplicationDelegate {
	private var window: NSWindow?

	func applicationDidFinishLaunching(_: Notification) {
		let frame = NSScreen.main?.frame ?? CGRect(x: 0, y: 0, width: 1280, height: 720)
		let window = NSWindow(
			contentRect: frame,
			styleMask: [.borderless],
			backing: .buffered,
			defer: false
		)
		window.backgroundColor = NSColor(calibratedWhite: 0.88, alpha: 1)
		window.isOpaque = true
		window.ignoresMouseEvents = true
		window.level = .normal
		window.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]
		window.orderFrontRegardless()
		self.window = window
		fputs("ready\n", stdout)
		fflush(stdout)
	}
}

let app = NSApplication.shared
let delegate = VisualBackgroundDelegate()
app.setActivationPolicy(.accessory)
app.delegate = delegate
app.run()
