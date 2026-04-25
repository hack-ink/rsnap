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
		static let hudOpacity = "hudOpacity"
		static let hudBlur = "hudBlur"
		static let hudTint = "hudTint"
		static let hudTintHue = "hudTintHue"
		static let loupeSampleSize = "loupeSampleSize"
		static let migratedLegacyToml = "migratedLegacyToml"
	}

	private let defaults: UserDefaults
	private(set) var settings: NativeHostSettings

	init(defaults: UserDefaults = .standard) {
		self.defaults = defaults
		let baseSettings = NativeHostSettings.defaults
		var settings = NativeHostSettings(
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
			hudOpacity: defaults.object(forKey: DefaultsKey.hudOpacity) as? Double
				?? baseSettings.hudOpacity,
			hudBlur: defaults.object(forKey: DefaultsKey.hudBlur) as? Double
				?? baseSettings.hudBlur,
			hudTint: defaults.object(forKey: DefaultsKey.hudTint) as? Double
				?? baseSettings.hudTint,
			hudTintHue: defaults.object(forKey: DefaultsKey.hudTintHue) as? Double
				?? baseSettings.hudTintHue,
			loupeSampleSize: LoupeSampleSizePreference(
				rawValue: defaults.string(forKey: DefaultsKey.loupeSampleSize) ?? "")
				?? baseSettings.loupeSampleSize
		)
		if !defaults.bool(forKey: DefaultsKey.migratedLegacyToml),
			let migrated = Self.migrateLegacyToml(into: settings)
		{
			settings = migrated
			defaults.set(true, forKey: DefaultsKey.migratedLegacyToml)
			Self.persist(settings, into: defaults)
		}
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
		settings = next.sanitized()
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
		defaults.set(settings.hudOpacity, forKey: DefaultsKey.hudOpacity)
		defaults.set(settings.hudBlur, forKey: DefaultsKey.hudBlur)
		defaults.set(settings.hudTint, forKey: DefaultsKey.hudTint)
		defaults.set(settings.hudTintHue, forKey: DefaultsKey.hudTintHue)
		defaults.set(settings.loupeSampleSize.rawValue, forKey: DefaultsKey.loupeSampleSize)
	}

	private static func migrateLegacyToml(into settings: NativeHostSettings) -> NativeHostSettings?
	{
		let legacyPath = FileManager.default.homeDirectoryForCurrentUser
			.appendingPathComponent("Library", isDirectory: true)
			.appendingPathComponent("Application Support", isDirectory: true)
			.appendingPathComponent("ink.hack.rsnap", isDirectory: true)
			.appendingPathComponent("settings.toml", isDirectory: false)
		guard let contents = try? String(contentsOf: legacyPath) else {
			return nil
		}

		var migrated = settings
		let lines = contents.split(separator: "\n", omittingEmptySubsequences: false)
		for rawLine in lines {
			let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
			guard !line.isEmpty, !line.hasPrefix("#"), let separator = line.firstIndex(of: "=")
			else {
				continue
			}
			let key = line[..<separator].trimmingCharacters(in: .whitespacesAndNewlines)
			let value = line[line.index(after: separator)...].trimmingCharacters(
				in: .whitespacesAndNewlines)
			let unquoted = value.trimmingCharacters(in: CharacterSet(charactersIn: "\""))
			switch key {
			case "capture_hotkey":
				migrated.captureHotkey = unquoted
			case "output_dir":
				let url = URL(fileURLWithPath: unquoted, isDirectory: true)
				migrated.outputDirectory = url
			case "output_filename_prefix":
				migrated.outputFilenamePrefix = unquoted
			case "output_naming":
				if let naming = OutputNamingPreference(rawValue: unquoted) {
					migrated.outputNaming = naming
				}
			case "toolbar_placement":
				if let placement = ToolbarPlacementPreference(rawValue: unquoted) {
					migrated.toolbarPlacement = placement
				}
			case "frozen_resize_handle_orientation":
				if let orientation = FrozenResizeHandleOrientationPreference(rawValue: unquoted) {
					migrated.frozenResizeHandleOrientation = orientation
				}
			case "show_alt_hint_keycap":
				if let boolValue = parseTomlBool(unquoted) {
					migrated.showAltHintKeycap = boolValue
				}
			case "hud_glass_enabled":
				if let boolValue = parseTomlBool(unquoted) {
					migrated.hudGlassEnabled = boolValue
				}
			case "hud_opacity":
				if let value = parseTomlDouble(unquoted) {
					migrated.hudOpacity = value
				}
			case "hud_blur":
				if let value = parseTomlDouble(unquoted) {
					migrated.hudBlur = value
				}
			case "hud_tint":
				if let value = parseTomlDouble(unquoted) {
					migrated.hudTint = value
				}
			case "hud_tint_hue":
				if let value = parseTomlDouble(unquoted) {
					migrated.hudTintHue = value
				}
			case "loupe_sample_size":
				if let sampleSize = LoupeSampleSizePreference(rawValue: unquoted) {
					migrated.loupeSampleSize = sampleSize
				}
			default:
				continue
			}
		}

		return migrated
	}

	private static func parseTomlBool(_ raw: String) -> Bool? {
		switch raw.lowercased() {
		case "true":
			return true
		case "false":
			return false
		default:
			return nil
		}
	}

	private static func parseTomlDouble(_ raw: String) -> Double? {
		Double(raw)
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
	var hudOpacity: Double
	var hudBlur: Double
	var hudTint: Double
	var hudTintHue: Double
	var loupeSampleSize: LoupeSampleSizePreference

	static let defaults = NativeHostSettings(
		captureHotkey: "alt+KeyX",
		outputDirectory: FileManager.default.homeDirectoryForCurrentUser
			.appendingPathComponent("Desktop", isDirectory: true),
		outputFilenamePrefix: "rsnap",
		outputNaming: .timestamp,
		toolbarPlacement: .bottom,
		frozenResizeHandleOrientation: .inward,
		showAltHintKeycap: true,
		hudGlassEnabled: true,
		hudOpacity: 0.5,
		hudBlur: 0.5,
		hudTint: 0.5,
		hudTintHue: 215.0 / 360.0,
		loupeSampleSize: .medium
	)

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
		return trimmed.isEmpty ? defaults.captureHotkey : trimmed
	}
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

enum LoupeSampleSizePreference: String, CaseIterable {
	case small
	case medium
	case large

	var title: String {
		switch self {
		case .small:
			return "Small (15×15)"
		case .medium:
			return "Medium (21×21)"
		case .large:
			return "Large (31×31)"
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

extension Comparable {
	fileprivate func clamped(to range: ClosedRange<Self>) -> Self {
		min(max(self, range.lowerBound), range.upperBound)
	}
}
