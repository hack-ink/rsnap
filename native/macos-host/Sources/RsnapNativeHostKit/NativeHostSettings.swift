import AppKit
import Foundation
import RsnapHostBridge

@MainActor
final class NativeHostSettingsStore {
	static let didChangeNotification = Notification.Name("RsnapNativeHostSettingsDidChange")

	private enum DefaultsKey {
		static let captureHotkey = "captureHotkey"
		static let outputDirectory = "outputDirectory"
		static let outputFilenamePrefix = "outputFilenamePrefix"
		static let outputNaming = "outputNaming"
		static let toolbarPlacement = "toolbarPlacement"
		static let frozenResizeHandleOrientation = "frozenResizeHandleOrientation"
		static let showAltHintKeycap = "showAltHintKeycap"
		static let hudGlassEnabled = "hudGlassEnabled"
		static let hudGlassMode = "hudGlassMode"
		static let hudOpacity = "hudOpacity"
		static let hudBlur = "hudBlur"
		static let hudTint = "hudTint"
		static let hudTintHue = "hudTintHue"
		static let liquidGlassStyle = "liquidGlassStyle"
		static let loupeSampleSize = "loupeSampleSize"
	}

	private let defaults: UserDefaults
	private(set) var settings: NativeHostSettings

	init(defaults: UserDefaults = .standard) {
		self.defaults = defaults
		let baseSettings = NativeHostSettings.defaults
		let persistedHudGlassMode = HudGlassModePreference(
			rawValue: defaults.string(forKey: DefaultsKey.hudGlassMode) ?? "")
		let hudGlassMode = persistedHudGlassMode ?? baseSettings.hudGlassMode
		let settings = NativeHostSettings(
			captureHotkey: defaults.string(forKey: DefaultsKey.captureHotkey)
				?? baseSettings.captureHotkey,
			outputDirectory: defaults.url(forKey: DefaultsKey.outputDirectory)
				?? baseSettings.outputDirectory,
			outputFilenamePrefix: defaults.string(forKey: DefaultsKey.outputFilenamePrefix)
				?? baseSettings.outputFilenamePrefix,
			outputNaming: OutputNamingPreference(
				rawValue: defaults.string(forKey: DefaultsKey.outputNaming) ?? "")
				?? baseSettings.outputNaming,
			toolbarPlacement: ToolbarPlacementPreference(
				rawValue: defaults.string(forKey: DefaultsKey.toolbarPlacement) ?? "")
				?? baseSettings.toolbarPlacement,
			frozenResizeHandleOrientation: FrozenResizeHandleOrientationPreference(
				rawValue: defaults.string(forKey: DefaultsKey.frozenResizeHandleOrientation) ?? "")
				?? baseSettings.frozenResizeHandleOrientation,
			showAltHintKeycap: defaults.object(forKey: DefaultsKey.showAltHintKeycap) as? Bool
				?? baseSettings.showAltHintKeycap,
			hudGlassEnabled: defaults.object(forKey: DefaultsKey.hudGlassEnabled) as? Bool
				?? baseSettings.hudGlassEnabled,
			hudGlassMode: hudGlassMode,
			hudOpacity: defaults.object(forKey: DefaultsKey.hudOpacity) as? Double
				?? baseSettings.hudOpacity,
			hudBlur: defaults.object(forKey: DefaultsKey.hudBlur) as? Double
				?? baseSettings.hudBlur,
			hudTint: defaults.object(forKey: DefaultsKey.hudTint) as? Double
				?? baseSettings.hudTint,
			hudTintHue: defaults.object(forKey: DefaultsKey.hudTintHue) as? Double
				?? baseSettings.hudTintHue,
			liquidGlassStyle: LiquidGlassStylePreference(
				rawValue: defaults.string(forKey: DefaultsKey.liquidGlassStyle) ?? "")
				?? baseSettings.liquidGlassStyle,
			loupeSampleSize: LoupeSampleSizePreference(
				rawValue: defaults.string(forKey: DefaultsKey.loupeSampleSize) ?? "")
				?? baseSettings.loupeSampleSize
		)
		self.settings = settings.sanitized()
		Self.persist(self.settings, into: defaults)
	}

	var sessionConfiguration: SessionConfiguration {
		SessionConfiguration(
			allowTextInput: true,
			prefersToolbarAboveSelection: settings.toolbarPlacement == .top
		)
	}

	func update(_ mutate: (inout NativeHostSettings) -> Void) {
		var next = settings
		mutate(&next)
		let sanitized = next.sanitized()
		settings = sanitized
		Self.persist(settings, into: defaults)
		NotificationCenter.default.post(name: Self.didChangeNotification, object: self)
	}

	private static func persist(_ settings: NativeHostSettings, into defaults: UserDefaults) {
		defaults.set(settings.outputDirectory, forKey: DefaultsKey.outputDirectory)
		defaults.set(settings.captureHotkey, forKey: DefaultsKey.captureHotkey)
		defaults.set(settings.outputFilenamePrefix, forKey: DefaultsKey.outputFilenamePrefix)
		defaults.set(settings.outputNaming.rawValue, forKey: DefaultsKey.outputNaming)
		defaults.set(settings.toolbarPlacement.rawValue, forKey: DefaultsKey.toolbarPlacement)
		defaults.set(
			settings.frozenResizeHandleOrientation.rawValue,
			forKey: DefaultsKey.frozenResizeHandleOrientation)
		defaults.set(settings.showAltHintKeycap, forKey: DefaultsKey.showAltHintKeycap)
		defaults.set(settings.hudGlassEnabled, forKey: DefaultsKey.hudGlassEnabled)
		defaults.set(settings.hudGlassMode.rawValue, forKey: DefaultsKey.hudGlassMode)
		defaults.set(settings.hudOpacity, forKey: DefaultsKey.hudOpacity)
		defaults.set(settings.hudBlur, forKey: DefaultsKey.hudBlur)
		defaults.set(settings.hudTint, forKey: DefaultsKey.hudTint)
		defaults.set(settings.hudTintHue, forKey: DefaultsKey.hudTintHue)
		defaults.set(settings.liquidGlassStyle.rawValue, forKey: DefaultsKey.liquidGlassStyle)
		defaults.set(settings.loupeSampleSize.rawValue, forKey: DefaultsKey.loupeSampleSize)
	}
}

struct NativeHostSettings: Equatable {
	var captureHotkey: String
	var outputDirectory: URL
	var outputFilenamePrefix: String
	var outputNaming: OutputNamingPreference
	var toolbarPlacement: ToolbarPlacementPreference
	var frozenResizeHandleOrientation: FrozenResizeHandleOrientationPreference
	var showAltHintKeycap: Bool
	var hudGlassEnabled: Bool
	var hudGlassMode: HudGlassModePreference
	var hudOpacity: Double
	var hudBlur: Double
	var hudTint: Double
	var hudTintHue: Double
	var liquidGlassStyle: LiquidGlassStylePreference
	var loupeSampleSize: LoupeSampleSizePreference

	static var defaults: NativeHostSettings {
		NativeHostSettings(
			captureHotkey: "Option-X",
			outputDirectory: FileManager.default.homeDirectoryForCurrentUser
				.appendingPathComponent("Desktop", isDirectory: true),
			outputFilenamePrefix: "rsnap",
			outputNaming: .timestamp,
			toolbarPlacement: .bottom,
			frozenResizeHandleOrientation: .outward,
			showAltHintKeycap: true,
			hudGlassEnabled: true,
			hudGlassMode: .liquidGlass,
			hudOpacity: 0.4999747693194925,
			hudBlur: 0.5032628676470589,
			hudTint: 0.4990234375,
			hudTintHue: 0.6074879184861536,
			liquidGlassStyle: .clear,
			loupeSampleSize: .small
		)
	}

	func sanitized() -> Self {
		var copy = self
		if copy.outputDirectory.path.isEmpty {
			copy.outputDirectory = Self.defaults.outputDirectory
		}
		copy.captureHotkey = Self.sanitizeCaptureHotkey(copy.captureHotkey)
		copy.outputFilenamePrefix = Self.sanitizeFilenamePrefix(copy.outputFilenamePrefix)
		copy.hudOpacity = copy.hudOpacity.clamped(to: 0...1)
		copy.hudBlur = copy.hudBlur.clamped(to: 0...1)
		copy.hudTint = copy.hudTint.clamped(to: 0...1)
		copy.hudTintHue = copy.hudTintHue.clamped(to: 0...1)
		return copy
	}

	private static func sanitizeFilenamePrefix(_ raw: String) -> String {
		let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
		let sanitized = trimmed.map { character -> Character in
			if character.isASCII
				&& (character.isLetter || character.isNumber || character == "-"
					|| character == "_")
			{
				return character
			}
			return "_"
		}
		let collapsed = String(sanitized).trimmingCharacters(in: CharacterSet(charactersIn: "_"))
		return collapsed.isEmpty ? "rsnap" : collapsed
	}

	private static func sanitizeCaptureHotkey(_ raw: String) -> String {
		let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
		guard !trimmed.isEmpty else {
			return defaults.captureHotkey
		}
		return captureHotKeyPresentation(for: trimmed).displayTitle
	}

	static func captureHotKeyPresentation(for raw: String) -> CaptureHotKeyPresentation {
		parseCaptureHotKeyPresentation(raw)
			?? parseCaptureHotKeyPresentation(defaults.captureHotkey)
			?? CaptureHotKeyPresentation(
				displayTitle: "Option-X",
				keyEquivalent: "x",
				modifierMask: [.option])
	}

	private static func parseCaptureHotKeyPresentation(_ raw: String) -> CaptureHotKeyPresentation?
	{
		let tokens = captureHotKeyTokens(from: raw)
		guard !tokens.isEmpty else {
			return nil
		}

		var modifiers = NSEvent.ModifierFlags()
		var keyEquivalent: String?
		for token in tokens {
			switch token.lowercased() {
			case "alt", "option":
				modifiers.insert(.option)
			case "ctrl", "control":
				modifiers.insert(.control)
			case "shift":
				modifiers.insert(.shift)
			case "cmd", "command", "super", "meta", "win":
				modifiers.insert(.command)
			default:
				keyEquivalent = normalizedMenuKeyEquivalent(for: token)
			}
		}

		guard let keyEquivalent else {
			return nil
		}

		var titleParts: [String] = []
		if modifiers.contains(.control) {
			titleParts.append("Control")
		}
		if modifiers.contains(.option) {
			titleParts.append("Option")
		}
		if modifiers.contains(.shift) {
			titleParts.append("Shift")
		}
		if modifiers.contains(.command) {
			titleParts.append("Command")
		}
		titleParts.append(keyEquivalent.uppercased())
		return CaptureHotKeyPresentation(
			displayTitle: titleParts.joined(separator: "-"),
			keyEquivalent: keyEquivalent,
			modifierMask: modifiers
		)
	}

	private static func normalizedMenuKeyEquivalent(for token: String) -> String? {
		let normalized = token.lowercased()
		let key = normalized.hasPrefix("key") ? String(normalized.dropFirst(3)) : normalized
		guard key.count == 1, key.unicodeScalars.allSatisfy(\.isASCII) else {
			return nil
		}
		return key
	}

	static func captureHotKeyTokens(from raw: String) -> [String] {
		raw
			.split { character in
				character == "+" || character == "-"
			}
			.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
			.filter { !$0.isEmpty }
	}
}

struct CaptureHotKeyPresentation: Equatable {
	let displayTitle: String
	let keyEquivalent: String
	let modifierMask: NSEvent.ModifierFlags
}

enum OutputNamingPreference: String, CaseIterable {
	case timestamp
	case sequence

	var title: String {
		switch self {
		case .timestamp:
			return "Timestamp"
		case .sequence:
			return "Sequence"
		}
	}
}

enum ToolbarPlacementPreference: String, CaseIterable {
	case bottom
	case top

	var title: String {
		switch self {
		case .bottom:
			return "Bottom"
		case .top:
			return "Top"
		}
	}
}

enum FrozenResizeHandleOrientationPreference: String, CaseIterable {
	case outward
	case inward

	var title: String {
		switch self {
		case .outward:
			return "Open Outward"
		case .inward:
			return "Open Inward"
		}
	}
}

enum HudGlassModePreference: String, CaseIterable {
	case classicGlass = "classic_glass"
	case liquidGlass = "liquid_glass"

	var title: String {
		switch self {
		case .liquidGlass:
			return "Liquid Glass"
		case .classicGlass:
			return "Classic Glass"
		}
	}
}

enum LiquidGlassStylePreference: String, CaseIterable {
	case regular
	case clear

	var title: String {
		switch self {
		case .regular:
			return "Regular"
		case .clear:
			return "Clear"
		}
	}

}

enum LoupeSampleSizePreference: String, CaseIterable {
	case small
	case medium
	case large

	var title: String {
		switch self {
		case .small:
			return "15×15"
		case .medium:
			return "21×21"
		case .large:
			return "31×31"
		}
	}

	var sidePixels: Int {
		switch self {
		case .small:
			return 15
		case .medium:
			return 21
		case .large:
			return 31
		}
	}
}

enum LiveChromeGlassMaterialSupport {
	static var isLiquidGlassAvailable: Bool {
		#if compiler(>=6.2)
			if #available(macOS 26.0, *) {
				return true
			}
		#endif
		return false
	}
}

extension NativeHostSettings {
	var resolvedHudGlassMode: HudGlassModePreference {
		if hudGlassMode == .liquidGlass,
			!LiveChromeGlassMaterialSupport.isLiquidGlassAvailable
		{
			return .classicGlass
		}
		return hudGlassMode
	}

	var usesClassicHudGlass: Bool {
		hudGlassEnabled && resolvedHudGlassMode == .classicGlass && hudBlur > 0.01
	}

	var usesLiquidHudGlass: Bool {
		hudGlassEnabled && resolvedHudGlassMode == .liquidGlass
	}
}

extension Comparable {
	fileprivate func clamped(to range: ClosedRange<Self>) -> Self {
		min(max(self, range.lowerBound), range.upperBound)
	}
}
