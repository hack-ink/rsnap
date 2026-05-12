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
		static let hudTintSaturation = "hudTintSaturation"
		static let hudTintBrightness = "hudTintBrightness"
		static let liquidGlassStyle = "liquidGlassStyle"
		static let loupeSampleSize = "loupeSampleSize"
		static let captureFrameEffectEnabled = "captureFrameEffectEnabled"
		static let captureFrameBackground = "captureFrameBackground"
		static let captureFrameApplicability = "captureFrameApplicability"
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
			hudTintSaturation: defaults.object(forKey: DefaultsKey.hudTintSaturation) as? Double
				?? baseSettings.hudTintSaturation,
			hudTintBrightness: defaults.object(forKey: DefaultsKey.hudTintBrightness) as? Double
				?? baseSettings.hudTintBrightness,
			liquidGlassStyle: LiquidGlassStylePreference(
				rawValue: defaults.string(forKey: DefaultsKey.liquidGlassStyle) ?? "")
				?? baseSettings.liquidGlassStyle,
			loupeSampleSize: LoupeSampleSizePreference(
				rawValue: defaults.string(forKey: DefaultsKey.loupeSampleSize) ?? "")
				?? baseSettings.loupeSampleSize,
			captureFrameEffectEnabled: defaults.object(
				forKey: DefaultsKey.captureFrameEffectEnabled) as? Bool
				?? baseSettings.captureFrameEffectEnabled,
			captureFrameBackground: CaptureFrameBackgroundPreference(
				rawValue: defaults.string(forKey: DefaultsKey.captureFrameBackground) ?? "")
				?? baseSettings.captureFrameBackground,
			captureFrameApplicability: CaptureFrameApplicabilityPreference(
				rawValue: defaults.string(forKey: DefaultsKey.captureFrameApplicability) ?? "")
				?? baseSettings.captureFrameApplicability
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
		defaults.set(settings.hudTintSaturation, forKey: DefaultsKey.hudTintSaturation)
		defaults.set(settings.hudTintBrightness, forKey: DefaultsKey.hudTintBrightness)
		defaults.set(settings.liquidGlassStyle.rawValue, forKey: DefaultsKey.liquidGlassStyle)
		defaults.set(settings.loupeSampleSize.rawValue, forKey: DefaultsKey.loupeSampleSize)
		defaults.set(
			settings.captureFrameEffectEnabled,
			forKey: DefaultsKey.captureFrameEffectEnabled)
		defaults.set(
			settings.captureFrameBackground.rawValue,
			forKey: DefaultsKey.captureFrameBackground)
		defaults.set(
			settings.captureFrameApplicability.rawValue,
			forKey: DefaultsKey.captureFrameApplicability)
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
	var hudTintSaturation: Double
	var hudTintBrightness: Double
	var liquidGlassStyle: LiquidGlassStylePreference
	var loupeSampleSize: LoupeSampleSizePreference
	var captureFrameEffectEnabled: Bool
	var captureFrameBackground: CaptureFrameBackgroundPreference
	var captureFrameApplicability: CaptureFrameApplicabilityPreference

	static var defaults: NativeHostSettings {
		NativeHostSettings(
			captureHotkey: "Option-X",
			outputDirectory: FileManager.default.homeDirectoryForCurrentUser
				.appendingPathComponent("Desktop", isDirectory: true),
			outputFilenamePrefix: NativeHostBrand.defaultFilenamePrefix,
			outputNaming: .timestamp,
			toolbarPlacement: .bottom,
			frozenResizeHandleOrientation: .outward,
			showAltHintKeycap: true,
			hudGlassEnabled: true,
			hudGlassMode: .liquidGlass,
			hudOpacity: 0.499_974_769_319_492_5,
			hudBlur: 0.503_262_867_647_058_9,
			hudTint: 0.303_452_435_661_764_7,
			hudTintHue: 0.639_998_499_393_907_3,
			hudTintSaturation: 0.991_547_981_230_003_2,
			hudTintBrightness: 0.574_992_895_1,
			liquidGlassStyle: .clear,
			loupeSampleSize: .small,
			captureFrameEffectEnabled: false,
			captureFrameBackground: .systemWallpaper,
			captureFrameApplicability: .window
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
		copy.hudTintSaturation = copy.hudTintSaturation.clamped(to: 0...1)
		copy.hudTintBrightness = copy.hudTintBrightness.clamped(to: 0...1)
		copy.captureFrameApplicability = copy.captureFrameApplicability.normalizedForStorage
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
		return collapsed.isEmpty ? NativeHostBrand.defaultFilenamePrefix : collapsed
	}

	private static func sanitizeCaptureHotkey(_ raw: String) -> String {
		let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
		guard trimmed.isEmpty == false else {
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
		guard tokens.isEmpty == false else {
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

package enum CaptureFrameBackgroundPreference: String, CaseIterable, Sendable {
	case systemWallpaper = "system_wallpaper"
	case aurora
	case graphite
	case linen

	var title: String {
		switch self {
		case .systemWallpaper:
			return "Wallpaper"
		case .aurora:
			return "Aurora"
		case .graphite:
			return "Graphite"
		case .linen:
			return "Linen"
		}
	}
}

enum CaptureFrameApplicabilityPreference: String, CaseIterable, Sendable {
	case dragRegion = "drag_region"
	case window
	case scrollCapture = "scroll_capture"
	case all
	case both

	static let allCases: [Self] = [.dragRegion, .window, .all]

	var title: String {
		switch self {
		case .dragRegion:
			return "Drag"
		case .window:
			return "Window"
		case .scrollCapture:
			return "Scroll"
		case .all, .both:
			return "Both"
		}
	}

	var normalizedForStorage: Self {
		switch self {
		case .scrollCapture:
			return .dragRegion
		case .both:
			return .all
		case .dragRegion, .window, .all:
			return self
		}
	}

	func includes(_ source: CaptureFrameSource) -> Bool {
		switch (self, source) {
		case (.dragRegion, .dragRegion), (.dragRegion, .scrollCapture),
			(.window, .window), (.scrollCapture, .scrollCapture), (.all, .dragRegion),
			(.all, .window), (.all, .scrollCapture), (.both, .dragRegion),
			(.both, .window), (.both, .scrollCapture):
			return true
		case (.dragRegion, .window), (.window, .dragRegion), (.window, .scrollCapture),
			(.scrollCapture, .dragRegion), (.scrollCapture, .window), (_, .fullScreen),
			(_, .unknown):
			return false
		}
	}
}

enum LiveChromeGlassMaterialSupport {
	static let isLiquidGlassBuildSupported: Bool = {
		#if compiler(>=6.2)
			return true
		#else
			return false
		#endif
	}()

	static let isMacOSRuntimeSupported: Bool = {
		let version = ProcessInfo.processInfo.operatingSystemVersion
		return version.majorVersion >= 26
	}()

	static var isLiquidGlassAvailable: Bool {
		isLiquidGlassBuildSupported && isMacOSRuntimeSupported
	}

	static var unavailableHelpText: String {
		if isMacOSRuntimeSupported {
			return "This build was made without Liquid Glass support."
		}
		return "Requires macOS 26."
	}

	static var settingsSubtitle: String {
		if isLiquidGlassAvailable {
			return "Liquid or blur."
		}
		if isMacOSRuntimeSupported {
			return "Classic fallback in this build."
		}
		return "Classic fallback before macOS 26."
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
