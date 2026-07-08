import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge
import SwiftUI

private final class SettingsWindow: NSWindow {
	weak var shortcutRecorder: SettingsShortcutRecorder?

	override var canBecomeKey: Bool {
		true
	}

	override var canBecomeMain: Bool {
		true
	}

	override func performKeyEquivalent(with event: NSEvent) -> Bool {
		if shortcutRecorder?.handleKeyEvent(event) == true {
			return true
		}
		if handleCommandShortcut(event) {
			return true
		}
		return super.performKeyEquivalent(with: event)
	}

	override func keyDown(with event: NSEvent) {
		if shortcutRecorder?.handleKeyEvent(event) == true {
			return
		}
		if handleCommandShortcut(event) {
			return
		}
		super.keyDown(with: event)
	}

	override func sendEvent(_ event: NSEvent) {
		if shortcutRecorder?.isRecording == true {
			switch event.type {
			case .leftMouseDown, .rightMouseDown, .otherMouseDown:
				shortcutRecorder?.cancel()
				return
			default:
				break
			}
		}
		super.sendEvent(event)
	}

	override func cancelOperation(_ sender: Any?) {
		if shortcutRecorder?.isRecording == true {
			shortcutRecorder?.cancel()
			return
		}
		super.cancelOperation(sender)
	}

	private func handleCommandShortcut(_ event: NSEvent) -> Bool {
		let commandModifiers = event.modifierFlags.intersection([
			.command, .option, .control, .shift,
		])
		guard commandModifiers == .command,
			let character = event.charactersIgnoringModifiers?.lowercased()
		else {
			return false
		}

		switch character {
		case "w":
			performClose(nil)
			return true
		case "q":
			NSApp.terminate(nil)
			return true
		default:
			return false
		}
	}
}

@MainActor
final class SettingsWindowController: NSWindowController, NSWindowDelegate {
	private let viewModel: NativeHostSettingsViewModel
	let shortcutRecorder = SettingsShortcutRecorder()
	private let onClose: () -> Void
	private let onShortcutRecordingChanged: (Bool) -> Void

	init(
		settingsStore: NativeHostSettingsStore,
		softwareUpdater: SoftwareUpdater,
		onShortcutRecordingChanged: @escaping (Bool) -> Void = { _ in },
		onClose: @escaping () -> Void = {}
	) {
		self.viewModel = NativeHostSettingsViewModel(
			settingsStore: settingsStore,
			softwareUpdater: softwareUpdater)
		self.onShortcutRecordingChanged = onShortcutRecordingChanged
		self.onClose = onClose

		let contentRect = NSRect(
			x: 0,
			y: 0,
			width: NativeHostSettingsWindowMetrics.width,
			height: NativeHostSettingsWindowMetrics.idealHeight
		)
		let window = SettingsWindow(
			contentRect: contentRect,
			styleMask: [.titled, .closable, .miniaturizable, .fullSizeContentView],
			backing: .buffered,
			defer: false
		)
		window.title = "Settings"
		window.shortcutRecorder = shortcutRecorder
		window.titleVisibility = .hidden
		window.titlebarAppearsTransparent = true
		window.isMovableByWindowBackground = false
		window.backgroundColor = .clear
		window.isOpaque = false
		if #available(macOS 11.0, *) {
			window.titlebarSeparatorStyle = .none
		}
		window.isReleasedWhenClosed = false
		window.contentMinSize = NSSize(
			width: NativeHostSettingsWindowMetrics.width,
			height: NativeHostSettingsWindowMetrics.minHeight
		)
		window.collectionBehavior.insert(.moveToActiveSpace)
		super.init(window: window)

		window.delegate = self
		let hostingController = NSHostingController(
			rootView: NativeHostSettingsView(
				model: viewModel,
				shortcutRecorder: shortcutRecorder))
		hostingController.view.wantsLayer = true
		hostingController.view.layer?.backgroundColor = NSColor.clear.cgColor
		window.contentViewController = hostingController
		window.center()
		shortcutRecorder.onRecordingChanged = onShortcutRecordingChanged
		viewModel.refresh()
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func present() {
		NSApp.setActivationPolicy(.regular)
		viewModel.refresh()
		showWindow(nil)
		NSRunningApplication.current.activate(options: [.activateAllWindows])
		window?.makeKeyAndOrderFront(nil)
		window?.invalidateShadow()
		NSApp.activate(ignoringOtherApps: true)
	}

	var captureExceptionWindowIDs: Set<CGWindowID> {
		guard window?.isVisible == true, let windowNumber = window?.windowNumber,
			windowNumber > 0
		else {
			return []
		}
		return [CGWindowID(windowNumber)]
	}

	func windowWillClose(_: Notification) {
		shortcutRecorder.cancel()
		onClose()
	}

	func windowDidResignKey(_: Notification) {
		shortcutRecorder.cancel()
	}
}

@MainActor
enum NativePermissions {
	static var screenRecordingGranted: Bool {
		CGPreflightScreenCaptureAccess()
	}

	static func requestScreenRecording() -> Bool {
		let granted = screenRecordingGranted || CGRequestScreenCaptureAccess()
		if granted == false {
			openScreenRecordingSettings()
		}
		return granted
	}

	@discardableResult
	static func openScreenRecordingSettings() -> Bool {
		let privacyQuery = "Privacy_ScreenCapture"
		let modernURLString =
			"x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?\(privacyQuery)"
		if let modernURL = URL(string: modernURLString), NSWorkspace.shared.open(modernURL) {
			return true
		}

		let fallbackURLString =
			"x-apple.systempreferences:com.apple.preference.security?\(privacyQuery)"
		guard let fallbackURL = URL(string: fallbackURLString) else {
			return false
		}
		return NSWorkspace.shared.open(fallbackURL)
	}
}
