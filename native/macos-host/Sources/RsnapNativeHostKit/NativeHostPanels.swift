import AppKit
import ApplicationServices
import CoreGraphics
import Foundation
import RsnapHostBridge
import SwiftUI

private final class SettingsWindow: NSWindow {
	override var canBecomeKey: Bool {
		true
	}

	override var canBecomeMain: Bool {
		true
	}

	override func performKeyEquivalent(with event: NSEvent) -> Bool {
		if handleCommandShortcut(event) {
			return true
		}
		return super.performKeyEquivalent(with: event)
	}

	override func keyDown(with event: NSEvent) {
		if handleCommandShortcut(event) {
			return
		}
		super.keyDown(with: event)
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
	private let onClose: () -> Void

	init(settingsStore: NativeHostSettingsStore, onClose: @escaping () -> Void = {}) {
		self.viewModel = NativeHostSettingsViewModel(settingsStore: settingsStore)
		self.onClose = onClose

		let contentRect = NSRect(x: 0, y: 0, width: 620, height: 320)
		let window = SettingsWindow(
			contentRect: contentRect,
			styleMask: [.titled, .closable, .miniaturizable, .fullSizeContentView],
			backing: .buffered,
			defer: false
		)
		window.title = "Settings"
		window.titleVisibility = .hidden
		window.titlebarAppearsTransparent = true
		window.isMovableByWindowBackground = false
		window.backgroundColor = .clear
		window.isOpaque = false
		if #available(macOS 11.0, *) {
			window.titlebarSeparatorStyle = .none
		}
		window.isReleasedWhenClosed = false
		window.contentMinSize = NSSize(width: 620, height: 300)
		window.collectionBehavior.insert(.moveToActiveSpace)
		super.init(window: window)

		window.delegate = self
		let hostingController = NSHostingController(
			rootView: NativeHostSettingsView(model: viewModel))
		hostingController.view.wantsLayer = true
		hostingController.view.layer?.backgroundColor = NSColor.clear.cgColor
		window.contentViewController = hostingController
		window.center()
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
		NSApp.activate(ignoringOtherApps: true)
	}

	func windowWillClose(_: Notification) {
		onClose()
	}
}

@MainActor
enum NativePermissions {
	static func requiredForCurrentNativeHost(_ kind: PermissionKind) -> Bool {
		switch kind {
		case .screenRecording:
			return true
		case .accessibility, .inputMonitoring:
			return false
		}
	}

	static func status(for kind: PermissionKind) -> Bool {
		switch kind {
		case .screenRecording:
			return CGPreflightScreenCaptureAccess()
		case .accessibility:
			return AXIsProcessTrusted()
		case .inputMonitoring:
			return CGPreflightListenEventAccess()
		}
	}

	static func request(_ kind: PermissionKind) -> Bool {
		let granted: Bool
		switch kind {
		case .screenRecording:
			granted = CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess()
		case .accessibility:
			let promptKey = "AXTrustedCheckOptionPrompt"
			let options = [promptKey: true] as CFDictionary
			granted = AXIsProcessTrustedWithOptions(options)
		case .inputMonitoring:
			granted = CGPreflightListenEventAccess() || CGRequestListenEventAccess()
		}
		if !granted {
			openSystemSettings(for: kind)
		}
		return granted
	}

	@discardableResult
	static func openSystemSettings(for kind: PermissionKind) -> Bool {
		let privacyQuery: String
		switch kind {
		case .screenRecording:
			privacyQuery = "Privacy_ScreenCapture"
		case .accessibility:
			privacyQuery = "Privacy_Accessibility"
		case .inputMonitoring:
			privacyQuery = "Privacy_ListenEvent"
		}

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
