import AppKit
import Foundation

final class ScrollDocumentView: NSView {
	private let rowHeight: CGFloat = 72
	private let rows = 80
	private let textAttributes: [NSAttributedString.Key: Any] = [
		.font: NSFont.monospacedSystemFont(ofSize: 19, weight: .medium),
		.foregroundColor: NSColor(calibratedWhite: 0.10, alpha: 1),
	]
	private let smallTextAttributes: [NSAttributedString.Key: Any] = [
		.font: NSFont.monospacedSystemFont(ofSize: 14, weight: .semibold),
		.foregroundColor: NSColor(calibratedWhite: 0.12, alpha: 0.82),
	]

	override var isFlipped: Bool { true }

	override init(frame frameRect: NSRect) {
		super.init(frame: frameRect)
		wantsLayer = true
		layer?.backgroundColor = NSColor.white.cgColor
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	override func draw(_ dirtyRect: NSRect) {
		NSColor.white.setFill()
		dirtyRect.fill()

		for row in 0..<rows {
			let y = CGFloat(row) * rowHeight
			let rect = CGRect(x: 0, y: y, width: bounds.width, height: rowHeight)
			guard rect.intersects(dirtyRect) else {
				continue
			}

			let hue = CGFloat((row * 37) % 360) / 360
			NSColor(calibratedHue: hue, saturation: 0.24, brightness: 0.97, alpha: 1).setFill()
			rect.fill()
			NSColor(calibratedWhite: 0.18, alpha: 0.18).setStroke()
			NSBezierPath(rect: CGRect(x: 0, y: y, width: bounds.width, height: 1)).stroke()

			let markerColor = NSColor(
				calibratedHue: hue, saturation: 0.64, brightness: 0.78, alpha: 1)
			markerColor.setFill()
			for markerX in markerColumns(width: bounds.width) {
				let markerRect = CGRect(x: markerX, y: y + 14, width: 34, height: 34)
				NSBezierPath(roundedRect: markerRect, xRadius: 6, yRadius: 6).fill()
			}

			let text =
				"Rsnap scroll smoke row \(String(format: "%02d", row))  --  stable marker \(row * 7919)"
			for textX in textColumns(width: bounds.width) {
				text.draw(at: CGPoint(x: textX, y: y + 18), withAttributes: textAttributes)
			}

			let centerLabel = "center proof \(String(format: "%02d", row))"
			centerLabel.draw(
				at: CGPoint(x: max(24, bounds.midX - 92), y: y + 48),
				withAttributes: smallTextAttributes
			)
		}
	}

	private func markerColumns(width: CGFloat) -> [CGFloat] {
		[
			28,
			max(92, width * 0.36),
			max(126, width * 0.64),
		]
	}

	private func textColumns(width: CGFloat) -> [CGFloat] {
		[
			82,
			max(150, width * 0.36 + 54),
			max(220, width * 0.64 + 54),
		].filter { $0 < width - 360 }
	}
}

final class ScrollBackgroundDelegate: NSObject, NSApplicationDelegate {
	private var window: NSWindow?
	private var scrollView: NSScrollView?
	private var boundsObserver: NSObjectProtocol?
	private let scrollCommandName = Notification.Name("ink.hack.rsnap.ScrollSmoke.ScrollBy")

	func applicationDidFinishLaunching(_: Notification) {
		let frame = NSScreen.main?.frame ?? CGRect(x: 0, y: 0, width: 1_280, height: 720)
		let window = NSWindow(
			contentRect: frame,
			styleMask: [.borderless],
			backing: .buffered,
			defer: false
		)
		window.backgroundColor = NSColor.white
		window.isOpaque = true
		window.level = .floating
		window.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]

		let scrollView = NSScrollView(frame: CGRect(origin: .zero, size: frame.size))
		scrollView.autoresizingMask = [.width, .height]
		scrollView.hasVerticalScroller = true
		scrollView.hasHorizontalScroller = false
		scrollView.drawsBackground = true
		scrollView.backgroundColor = .white
		scrollView.borderType = .noBorder
		scrollView.scrollerStyle = .overlay
		scrollView.contentView.postsBoundsChangedNotifications = true

		let documentHeight = max(frame.height * 5, 5_760)
		let documentView = ScrollDocumentView(
			frame: CGRect(x: 0, y: 0, width: frame.width, height: documentHeight)
		)
		scrollView.documentView = documentView
		window.contentView = scrollView

		DistributedNotificationCenter.default().addObserver(
			self,
			selector: #selector(handleScrollCommand(_:)),
			name: scrollCommandName,
			object: nil
		)
		boundsObserver = NotificationCenter.default.addObserver(
			forName: NSView.boundsDidChangeNotification,
			object: scrollView.contentView,
			queue: .main
		) { [weak self] _ in
			self?.logCurrentOffset()
		}
		window.orderFrontRegardless()
		window.makeKey()
		NSApp.activate(ignoringOtherApps: true)
		self.window = window
		self.scrollView = scrollView
		fputs("ready\n", stdout)
		fflush(stdout)
	}

	@objc
	private func handleScrollCommand(_ notification: Notification) {
		guard let scrollView else {
			return
		}
		let rawDelta = notification.userInfo?["deltaY"] as? NSNumber
		let deltaY = CGFloat(rawDelta?.doubleValue ?? 0)
		guard deltaY != 0 else {
			return
		}

		let clipView = scrollView.contentView
		let documentHeight = scrollView.documentView?.bounds.height ?? clipView.bounds.height
		let maxY = max(0, documentHeight - clipView.bounds.height)
		let nextY = min(max(clipView.bounds.origin.y + deltaY, 0), maxY)
		clipView.scroll(to: CGPoint(x: clipView.bounds.origin.x, y: nextY))
		scrollView.reflectScrolledClipView(clipView)
		logCurrentOffset()
	}

	private func logCurrentOffset() {
		guard let scrollView else {
			return
		}
		let offsetY = scrollView.contentView.bounds.origin.y
		fputs("offsetY=\(String(format: "%.2f", offsetY))\n", stdout)
		fflush(stdout)
	}

	func applicationWillTerminate(_: Notification) {
		DistributedNotificationCenter.default().removeObserver(self)
		if let boundsObserver {
			NotificationCenter.default.removeObserver(boundsObserver)
		}
	}
}

let app = NSApplication.shared
let delegate = ScrollBackgroundDelegate()
app.setActivationPolicy(.accessory)
app.delegate = delegate
app.run()
