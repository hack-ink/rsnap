import AppKit
import Carbon
@preconcurrency import CoreGraphics
import Foundation
import RsnapHostBridge

@MainActor
final class GlobalHotKeyCenter {
	private enum Binding: UInt32, CaseIterable {
		case capture = 1
		case cancel = 2
		case loupe = 3
		case save = 4
		case quickScreenshot = 5
	}

	private struct HotKeyDefinition: Equatable {
		let keyCode: UInt32
		let modifiers: UInt32

		var eventFlags: CGEventFlags {
			var flags = CGEventFlags()
			if modifiers & UInt32(optionKey) != 0 {
				flags.insert(.maskAlternate)
			}
			if modifiers & UInt32(cmdKey) != 0 {
				flags.insert(.maskCommand)
			}
			if modifiers & UInt32(controlKey) != 0 {
				flags.insert(.maskControl)
			}
			if modifiers & UInt32(shiftKey) != 0 {
				flags.insert(.maskShift)
			}
			return flags
		}
	}

	private static let signature = OSType(0x5253_4E50)  // RSNP
	private static let trackedEventFlags: CGEventFlags = [
		.maskAlternate, .maskCommand, .maskControl, .maskShift,
	]

	var onCaptureRequested: (() -> Void)?
	var onQuickScreenshotRequested: (() -> Void)?
	var onCancelRequested: (() -> Void)?
	var onToggleLoupeRequested: (() -> Void)?
	var onSaveRequested: (() -> Void)?

	private var handlerRef: EventHandlerRef?
	private var hotKeyRefs: [Binding: EventHotKeyRef?] = [:]
	private var registeredDefinitions: [Binding: HotKeyDefinition] = [:]
	private var quickScreenshotEventTap: CFMachPort?
	private var quickScreenshotEventTapSource: CFRunLoopSource?
	private var quickScreenshotEventTapDefinition: HotKeyDefinition?

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

	func invalidate() {
		unregisterQuickScreenshotEventTap()
		for binding in Binding.allCases {
			unregister(binding)
		}
		if let handlerRef {
			RemoveEventHandler(handlerRef)
			self.handlerRef = nil
		}
	}

	func updateBindings(
		captureHotKey: String,
		quickScreenshotHotKey: String,
		sceneMode: SceneKind
	) -> Bool {
		var allRequestedBindingsRegistered = true
		let captureDefinition = Self.parseCaptureHotKey(captureHotKey) ?? Self.defaultCaptureHotKey
		allRequestedBindingsRegistered =
			register(.capture, definition: captureDefinition) && allRequestedBindingsRegistered

		let quickScreenshotDefinition =
			Self.parseCaptureHotKey(quickScreenshotHotKey) ?? Self.defaultQuickScreenshotHotKey
		if registerQuickScreenshotEventTap(definition: quickScreenshotDefinition) {
			unregister(.quickScreenshot)
		} else {
			allRequestedBindingsRegistered =
				register(.quickScreenshot, definition: quickScreenshotDefinition)
				&& allRequestedBindingsRegistered
		}

		let wantsCancel = sceneMode != .hidden
		let wantsLoupe = sceneMode == .live
		let wantsFrozen = sceneMode == .frozen

		if wantsCancel {
			allRequestedBindingsRegistered =
				register(.cancel, definition: HotKeyDefinition(keyCode: 53, modifiers: 0))
				&& allRequestedBindingsRegistered
		} else {
			unregister(.cancel)
		}

		if wantsLoupe {
			allRequestedBindingsRegistered =
				register(.loupe, definition: HotKeyDefinition(keyCode: 48, modifiers: 0))
				&& allRequestedBindingsRegistered
		} else {
			unregister(.loupe)
		}

		if wantsFrozen {
			allRequestedBindingsRegistered =
				register(.save, definition: HotKeyDefinition(keyCode: 1, modifiers: UInt32(cmdKey)))
				&& allRequestedBindingsRegistered
		} else {
			unregister(.save)
		}

		return allRequestedBindingsRegistered
	}

	private func register(_ binding: Binding, definition: HotKeyDefinition) -> Bool {
		if registeredDefinitions[binding] == definition {
			return true
		}
		if registeredDefinitions[binding] != nil {
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
			return false
		}
		hotKeyRefs[binding] = hotKeyRef
		registeredDefinitions[binding] = definition
		NativeHostTelemetry.lifecycleEvent(
			"native_host.hotkey_registered",
			detail:
				"binding=\(binding.rawValue),keyCode=\(definition.keyCode),modifiers=\(definition.modifiers)"
		)
		return true
	}

	private func unregister(_ binding: Binding) {
		guard registeredDefinitions[binding] != nil || hotKeyRefs[binding] != nil else {
			return
		}
		if let hotKeyRef = hotKeyRefs[binding] {
			UnregisterEventHotKey(hotKeyRef)
		}
		hotKeyRefs[binding] = nil
		registeredDefinitions.removeValue(forKey: binding)
	}

	private func registerQuickScreenshotEventTap(definition: HotKeyDefinition) -> Bool {
		if quickScreenshotEventTapDefinition == definition, quickScreenshotEventTap != nil {
			return true
		}
		unregisterQuickScreenshotEventTap()

		let mask = CGEventMask(1) << CGEventType.keyDown.rawValue
		let callback: CGEventTapCallBack = { _, type, event, userInfo in
			guard let userInfo else {
				return Unmanaged.passUnretained(event)
			}
			return MainActor.assumeIsolated {
				let center = Unmanaged<GlobalHotKeyCenter>
					.fromOpaque(userInfo)
					.takeUnretainedValue()
				return center.handleQuickScreenshotEventTap(type: type, event: event)
			}
		}
		guard
			let eventTap = CGEvent.tapCreate(
				tap: .cgSessionEventTap,
				place: .headInsertEventTap,
				options: .defaultTap,
				eventsOfInterest: mask,
				callback: callback,
				userInfo: Unmanaged.passUnretained(self).toOpaque()
			)
		else {
			NativeHostTelemetry.lifecycleWarning(
				"native_host.quick_screenshot_event_tap_failed",
				detail:
					"keyCode=\(definition.keyCode),modifiers=\(definition.modifiers)"
			)
			return false
		}

		let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, eventTap, 0)
		CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
		CGEvent.tapEnable(tap: eventTap, enable: true)
		quickScreenshotEventTap = eventTap
		quickScreenshotEventTapSource = source
		quickScreenshotEventTapDefinition = definition
		NativeHostTelemetry.lifecycleEvent(
			"native_host.quick_screenshot_event_tap_registered",
			detail:
				"keyCode=\(definition.keyCode),modifiers=\(definition.modifiers)"
		)
		return true
	}

	private func unregisterQuickScreenshotEventTap() {
		if let quickScreenshotEventTap {
			CGEvent.tapEnable(tap: quickScreenshotEventTap, enable: false)
		}
		if let quickScreenshotEventTapSource {
			CFRunLoopRemoveSource(CFRunLoopGetMain(), quickScreenshotEventTapSource, .commonModes)
		}
		quickScreenshotEventTap = nil
		quickScreenshotEventTapSource = nil
		quickScreenshotEventTapDefinition = nil
	}

	private func handleQuickScreenshotEventTap(type: CGEventType, event: CGEvent)
		-> Unmanaged<CGEvent>?
	{
		switch type {
		case .tapDisabledByTimeout, .tapDisabledByUserInput:
			if let quickScreenshotEventTap {
				CGEvent.tapEnable(tap: quickScreenshotEventTap, enable: true)
			}
			return Unmanaged.passUnretained(event)
		case .keyDown:
			guard matchesQuickScreenshotEvent(event) else {
				return Unmanaged.passUnretained(event)
			}
			if event.getIntegerValueField(.keyboardEventAutorepeat) == 0 {
				DispatchQueue.main.async { [weak self] in
					self?.onQuickScreenshotRequested?()
				}
			}
			return nil
		default:
			return Unmanaged.passUnretained(event)
		}
	}

	private func matchesQuickScreenshotEvent(_ event: CGEvent) -> Bool {
		guard let definition = quickScreenshotEventTapDefinition else {
			return false
		}
		let keyCode = UInt32(event.getIntegerValueField(.keyboardEventKeycode))
		guard keyCode == definition.keyCode else {
			return false
		}
		return event.flags.intersection(Self.trackedEventFlags)
			== definition.eventFlags.intersection(Self.trackedEventFlags)
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
		case .quickScreenshot:
			onQuickScreenshotRequested?()
		case .cancel:
			onCancelRequested?()
		case .loupe:
			onToggleLoupeRequested?()
		case .save:
			onSaveRequested?()
		}
		return noErr
	}

	private static let defaultCaptureHotKey = HotKeyDefinition(
		keyCode: 7,
		modifiers: UInt32(optionKey)
	)
	private static let defaultQuickScreenshotHotKey = HotKeyDefinition(
		keyCode: 7,
		modifiers: UInt32(optionKey | shiftKey)
	)

	private static func parseCaptureHotKey(_ raw: String) -> HotKeyDefinition? {
		let tokens = NativeHostSettings.captureHotKeyTokens(from: raw)
		guard tokens.isEmpty == false else {
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
