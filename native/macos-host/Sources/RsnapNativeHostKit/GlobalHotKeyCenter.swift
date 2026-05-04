import AppKit
import Carbon
import Foundation
import RsnapHostBridge

@MainActor
final class GlobalHotKeyCenter {
	private enum Binding: UInt32, CaseIterable {
		case capture = 1
		case cancel = 2
		case loupe = 3
	}

	private struct HotKeyDefinition {
		let keyCode: UInt32
		let modifiers: UInt32
	}

	private static let signature = OSType(0x5253_4E50)  // RSNP

	var onCaptureRequested: (() -> Void)?
	var onCancelRequested: (() -> Void)?
	var onToggleLoupeRequested: (() -> Void)?

	private var handlerRef: EventHandlerRef?
	private var hotKeyRefs: [Binding: EventHotKeyRef?] = [:]
	private var registeredBindings: Set<Binding> = []

	init() {
		var eventType = EventTypeSpec(
			eventClass: OSType(kEventClassKeyboard),
			eventKind: UInt32(kEventHotKeyPressed)
		)
		InstallEventHandler(
			GetEventDispatcherTarget(),
			{ _, eventRef, userData in
				guard let eventRef, let userData else {
					return OSStatus(eventNotHandledErr)
				}
				let center = Unmanaged<GlobalHotKeyCenter>.fromOpaque(userData)
					.takeUnretainedValue()
				return center.handleHotKey(eventRef)
			},
			1,
			&eventType,
			Unmanaged.passUnretained(self).toOpaque(),
			&handlerRef
		)
	}

	func updateBindings(captureHotKey: String, sceneMode: SceneKind) {
		let captureDefinition = Self.parseCaptureHotKey(captureHotKey) ?? Self.defaultCaptureHotKey
		register(.capture, definition: captureDefinition)

		let wantsCancel = sceneMode != .hidden
		let wantsLoupe = sceneMode == .live

		if wantsCancel {
			register(.cancel, definition: HotKeyDefinition(keyCode: 53, modifiers: 0))
		} else {
			unregister(.cancel)
		}

		if wantsLoupe {
			register(.loupe, definition: HotKeyDefinition(keyCode: 48, modifiers: 0))
		} else {
			unregister(.loupe)
		}
	}

	private func register(_ binding: Binding, definition: HotKeyDefinition) {
		if registeredBindings.contains(binding) {
			unregister(binding)
		}

		var hotKeyRef: EventHotKeyRef?
		let hotKeyID = EventHotKeyID(signature: Self.signature, id: binding.rawValue)
		let status = RegisterEventHotKey(
			definition.keyCode,
			definition.modifiers,
			hotKeyID,
			GetEventDispatcherTarget(),
			0,
			&hotKeyRef
		)
		guard status == noErr else {
			NativeHostTelemetry.lifecycleWarning(
				"native_host.hotkey_register_failed",
				detail:
					"binding=\(binding.rawValue),keyCode=\(definition.keyCode),modifiers=\(definition.modifiers),status=\(status)"
			)
			return
		}
		hotKeyRefs[binding] = hotKeyRef
		registeredBindings.insert(binding)
		NativeHostTelemetry.lifecycleEvent(
			"native_host.hotkey_registered",
			detail:
				"binding=\(binding.rawValue),keyCode=\(definition.keyCode),modifiers=\(definition.modifiers)"
		)
	}

	private func unregister(_ binding: Binding) {
		if let hotKeyRef = hotKeyRefs[binding] {
			UnregisterEventHotKey(hotKeyRef)
		}
		hotKeyRefs[binding] = nil
		registeredBindings.remove(binding)
	}

	private func handleHotKey(_ eventRef: EventRef) -> OSStatus {
		var hotKeyID = EventHotKeyID()
		let status = GetEventParameter(
			eventRef,
			EventParamName(kEventParamDirectObject),
			EventParamType(typeEventHotKeyID),
			nil,
			MemoryLayout<EventHotKeyID>.size,
			nil,
			&hotKeyID
		)
		guard status == noErr, hotKeyID.signature == Self.signature,
			let binding = Binding(rawValue: hotKeyID.id)
		else {
			return OSStatus(eventNotHandledErr)
		}

		switch binding {
		case .capture:
			onCaptureRequested?()
		case .cancel:
			onCancelRequested?()
		case .loupe:
			onToggleLoupeRequested?()
		}
		return noErr
	}

	private static let defaultCaptureHotKey = HotKeyDefinition(
		keyCode: 7,
		modifiers: UInt32(optionKey)
	)

	private static func parseCaptureHotKey(_ raw: String) -> HotKeyDefinition? {
		let tokens = NativeHostSettings.captureHotKeyTokens(from: raw)
		guard !tokens.isEmpty else {
			return nil
		}

		var modifiers: UInt32 = 0
		var resolvedKeyCode: UInt32?
		for token in tokens {
			switch token.lowercased() {
			case "alt", "option":
				modifiers |= UInt32(optionKey)
			case "ctrl", "control":
				modifiers |= UInt32(controlKey)
			case "shift":
				modifiers |= UInt32(shiftKey)
			case "cmd", "command", "super", "meta", "win":
				modifiers |= UInt32(cmdKey)
			default:
				resolvedKeyCode = Self.keyCode(for: token)
			}
		}

		guard let keyCode = resolvedKeyCode else {
			return nil
		}
		return HotKeyDefinition(keyCode: keyCode, modifiers: modifiers)
	}

	private static func keyCode(for token: String) -> UInt32? {
		let normalized = token.lowercased()
		let key = normalized.hasPrefix("key") ? String(normalized.dropFirst(3)) : normalized
		let letterKeyCodes: [String: UInt32] = [
			"a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7,
			"c": 8, "v": 9, "b": 11, "q": 12, "w": 13, "e": 14, "r": 15, "y": 16,
			"t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23, "=": 24,
			"9": 25, "7": 26, "-": 27, "8": 28, "0": 29, "]": 30, "o": 31, "u": 32,
			"[": 33, "i": 34, "p": 35, "l": 37, "j": 38, "'": 39, "k": 40, ";": 41,
			"\\": 42, ",": 43, "/": 44, "n": 45, "m": 46, ".": 47,
		]
		return letterKeyCodes[key]
	}
}
