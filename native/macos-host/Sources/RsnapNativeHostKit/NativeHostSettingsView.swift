import AppKit
import RsnapHostBridge
import SwiftUI

enum NativeHostSettingsWindowMetrics {
	static let width: CGFloat = 620
	static let minHeight: CGFloat = 304
	static let idealHeight: CGFloat = 304
	static let cornerRadius: CGFloat = 18
}

@MainActor
final class NativeHostSettingsViewModel: ObservableObject {
	@Published private(set) var settings: NativeHostSettings
	@Published private(set) var launchAtLoginState = LaunchAtLoginState.current()
	@Published private(set) var softwareUpdateSettings: NativeHostSoftwareUpdater.Snapshot

	private let settingsStore: NativeHostSettingsStore
	private let softwareUpdater: NativeHostSoftwareUpdater

	init(
		settingsStore: NativeHostSettingsStore,
		softwareUpdater: NativeHostSoftwareUpdater
	) {
		self.settingsStore = settingsStore
		self.softwareUpdater = softwareUpdater
		self.settings = settingsStore.settings
		self.softwareUpdateSettings = softwareUpdater.snapshot()
	}

	func refresh() {
		settings = settingsStore.settings
		launchAtLoginState = LaunchAtLoginState.current()
		softwareUpdateSettings = softwareUpdater.snapshot()
	}

	func update(_ mutate: (inout NativeHostSettings) -> Void) {
		settingsStore.update(mutate)
		settings = settingsStore.settings
	}

	func restoreDefaults() {
		update { $0 = NativeHostSettings.defaults }
	}

	func setLaunchAtLoginEnabled(_ isEnabled: Bool) {
		do {
			try LaunchAtLoginController.setEnabled(isEnabled)
			launchAtLoginState = LaunchAtLoginState.current()
		} catch {
			launchAtLoginState = LaunchAtLoginState.current(
				errorMessage: error.localizedDescription)
		}
	}

	func setSoftwareUpdateMode(_ mode: NativeHostSoftwareUpdater.Mode) {
		softwareUpdater.setMode(mode)
		refresh()
	}

	func checkForUpdates() {
		softwareUpdater.checkForUpdates(nil)
		refresh()
	}

	func chooseOutputDirectory() {
		let panel = NSOpenPanel()
		panel.canChooseDirectories = true
		panel.canChooseFiles = false
		panel.allowsMultipleSelection = false
		panel.directoryURL = settings.outputDirectory
		if panel.runModal() == .OK, let url = panel.url {
			update { $0.outputDirectory = url }
		}
	}
}

struct NativeHostSettingsView: View {
	@ObservedObject var model: NativeHostSettingsViewModel
	@State private var selectedSection: NativeHostSettingsSection = .appearance
	private let sidebarWidth: CGFloat = 142

	var body: some View {
		ZStack {
			SettingsAtmosphere(tintHue: model.settings.hudTintHue)

			HStack(alignment: .top, spacing: 10) {
				SettingsRail(selectedSection: $selectedSection)
					.frame(width: sidebarWidth)
					.padding(.top, 24)

				SettingsDashboard(
					model: model,
					section: selectedSection,
					restoreDefaults: model.restoreDefaults
				)
				.frame(maxWidth: .infinity, alignment: .topLeading)
			}
			.padding(.top, 12)
			.padding(.horizontal, 14)
			.padding(.bottom, 12)
		}
		.ignoresSafeArea(.container, edges: .top)
		.controlSize(.small)
		.frame(
			minWidth: NativeHostSettingsWindowMetrics.width,
			idealWidth: NativeHostSettingsWindowMetrics.width,
			minHeight: NativeHostSettingsWindowMetrics.minHeight,
			idealHeight: NativeHostSettingsWindowMetrics.idealHeight
		)
	}
}

private enum NativeHostSettingsSection: String, CaseIterable, Identifiable {
	case appearance
	case capture
	case output
	case permissions
	case about

	var id: Self { self }

	var title: String {
		switch self {
		case .appearance:
			return "Appearance"
		case .capture:
			return "Capture"
		case .output:
			return "Output"
		case .permissions:
			return "Permissions"
		case .about:
			return "About"
		}
	}

	var subtitle: String {
		switch self {
		case .appearance:
			return "HUD style"
		case .capture:
			return "Shortcut"
		case .output:
			return "Files"
		case .permissions:
			return "Access"
		case .about:
			return "Project"
		}
	}

	var symbolName: String {
		switch self {
		case .appearance:
			return "sparkles"
		case .capture:
			return "viewfinder"
		case .output:
			return "folder"
		case .permissions:
			return "lock.shield"
		case .about:
			return "info.circle"
		}
	}

	var allowsRestoreDefaults: Bool {
		switch self {
		case .appearance, .capture, .output:
			return true
		case .permissions, .about:
			return false
		}
	}
}

private enum NativeHostAboutLinks {
	static let source = "https://github.com/hack-ink/rsnap"
	static let creator = "https://x.com/YvetteCipher"
}

private enum SettingsControlLayout {
	static let controlColumnWidth: CGFloat = 178
	static let sliderValueWidth: CGFloat = 34
	static let sliderTrackWidth: CGFloat = 136
	static let compactSliderLabelWidth: CGFloat = 44
	static let compactSliderTrackWidth: CGFloat = 88
}

private struct SettingsRail: View {
	@Binding var selectedSection: NativeHostSettingsSection

	var body: some View {
		VStack(alignment: .leading, spacing: 14) {
			HStack(spacing: 8) {
				SettingsBrandIcon()
				Text(NativeHostBrand.displayName)
					.font(.system(size: 17, weight: .semibold, design: .rounded))
					.lineLimit(1)
			}
			.padding(.horizontal, 2)

			VStack(spacing: 5) {
				ForEach(NativeHostSettingsSection.allCases) { section in
					SettingsRailButton(
						section: section,
						isSelected: selectedSection == section
					) {
						selectedSection = section
					}
				}
			}
		}
		.padding(.top, 2)
	}
}

private struct SettingsBrandIcon: View {
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Image(nsImage: NSApp.applicationIconImage)
			.resizable()
			.interpolation(.high)
			.scaledToFit()
			.padding(1)
			.frame(width: 28, height: 28)
			.clipShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
			.overlay {
				RoundedRectangle(cornerRadius: 7, style: .continuous)
					.stroke(
						colorScheme == .light
							? Color.black.opacity(0.08)
							: Color.white.opacity(0.16),
						lineWidth: 1
					)
			}
	}
}

private struct SettingsRailButton: View {
	let section: NativeHostSettingsSection
	let isSelected: Bool
	let action: () -> Void
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false

	var body: some View {
		Button(action: action) {
			HStack(spacing: 8) {
				Image(systemName: section.symbolName)
					.symbolRenderingMode(.hierarchical)
					.font(.system(size: 12.5, weight: .semibold))
					.foregroundStyle(isSelected ? Color.accentColor : Color.secondary)
					.frame(width: 23, height: 23)

				VStack(alignment: .leading, spacing: 2) {
					Text(section.title)
						.font(.system(size: 12.5, weight: .semibold))
						.foregroundStyle(isSelected ? Color.primary : Color.primary.opacity(0.88))
						.lineLimit(1)
						.minimumScaleFactor(0.88)
					Text(section.subtitle)
						.font(.system(size: 10, weight: .medium))
						.foregroundStyle(.secondary)
						.lineLimit(1)
				}
				Spacer(minLength: 0)
			}
			.padding(.horizontal, 8)
			.padding(.vertical, 5)
			.frame(maxWidth: .infinity)
			.contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
			.background {
				if isSelected {
					RoundedRectangle(cornerRadius: 9, style: .continuous)
						.fill(
							colorScheme == .light
								? Color.black.opacity(0.040)
								: Color.white.opacity(0.058)
						)
					HStack {
						Capsule()
							.fill(Color.accentColor)
							.frame(width: 2, height: 16)
						Spacer()
					}
					.padding(.leading, 1)
				} else if isHovered {
					RoundedRectangle(cornerRadius: 9, style: .continuous)
						.fill(
							colorScheme == .light
								? Color.black.opacity(0.022)
								: Color.white.opacity(0.034)
						)
				}
			}
		}
		.buttonStyle(.plain)
		.animation(.easeOut(duration: 0.14), value: isSelected)
		.onHover { hovering in
			withAnimation(.easeOut(duration: 0.14)) {
				isHovered = hovering
			}
		}
	}
}

private struct SettingsDashboard: View {
	@ObservedObject var model: NativeHostSettingsViewModel
	let section: NativeHostSettingsSection
	let restoreDefaults: () -> Void

	var body: some View {
		VStack(alignment: .leading, spacing: 6) {
			SettingsContentHeader(
				section: section,
				restoreDefaults: restoreDefaults
			)

			ScrollView {
				activePanel
					.id(section)
					.transition(
						.asymmetric(
							insertion: .opacity.combined(with: .move(edge: .bottom)),
							removal: .opacity.combined(with: .move(edge: .top))
						)
					)
					.padding(.trailing, 8)
					.padding(.bottom, 6)
			}
			.scrollIndicators(.hidden)
			.frame(maxWidth: .infinity, alignment: .topLeading)
			.animation(.spring(response: 0.34, dampingFraction: 0.86), value: section)
		}
		.padding(.horizontal, 13)
		.padding(.vertical, 9)
		.settingsGlassSurface(cornerRadius: 18, role: .panel)
	}

	@ViewBuilder
	private var activePanel: some View {
		switch section {
		case .appearance:
			AppearanceSettingsPanel(model: model)
		case .capture:
			CaptureSettingsPanel(model: model)
		case .output:
			OutputSettingsPanel(model: model)
		case .permissions:
			PermissionsSettingsPanel(model: model)
		case .about:
			AboutSettingsPanel(model: model)
		}
	}
}

private struct SettingsContentHeader: View {
	let section: NativeHostSettingsSection
	let restoreDefaults: () -> Void

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 10) {
			VStack(alignment: .leading, spacing: 2) {
				Text(section.title)
					.font(.system(size: 18, weight: .semibold))
				Text(section.subtitle)
					.font(.system(size: 11, weight: .medium))
					.foregroundStyle(.secondary)
			}
			.frame(maxWidth: .infinity, alignment: .leading)

			if section.allowsRestoreDefaults {
				Button(action: restoreDefaults) {
					Label("Restore Defaults", systemImage: "arrow.counterclockwise")
						.labelStyle(.titleAndIcon)
				}
				.rsnapGlassButton(prominent: false)
				.controlSize(.small)
			}
		}
		.padding(.bottom, 2)
	}
}

private struct SettingsSectionInspector: View {
	@ObservedObject var model: NativeHostSettingsViewModel
	let section: NativeHostSettingsSection

	var body: some View {
		VStack(alignment: .leading, spacing: 12) {
			switch section {
			case .appearance:
				AppearanceInspector(settings: model.settings)
			case .capture:
				CaptureInspector(settings: model.settings)
			case .output:
				OutputInspector(settings: model.settings)
			case .permissions:
				PermissionsInspector()
			case .about:
				AboutInspector()
			}
		}
		.padding(14)
		.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
		.settingsGlassSurface(cornerRadius: 18, role: .panel)
	}
}

private func abbreviatedPath(_ url: URL) -> String {
	let path = url.path
	let home = FileManager.default.homeDirectoryForCurrentUser.path
	if path == home {
		return "~"
	}
	if path.hasPrefix(home + "/") {
		return "~" + path.dropFirst(home.count)
	}
	return path
}

private struct AppearanceInspector: View {
	let settings: NativeHostSettings

	var body: some View {
		VStack(alignment: .leading, spacing: 12) {
			MiniHudPreview(settings: settings)

			VStack(spacing: 8) {
				InspectorMetric(
					title: "Material",
					value: settings.resolvedHudGlassMode.title,
					symbolName: "square.stack.3d.down.right"
				)
				InspectorMetric(
					title: "Tint",
					value: "\(Int((settings.hudTint * 100).rounded()))%",
					symbolName: "eyedropper"
				)
				InspectorMetric(
					title: "Color",
					value: tintHex,
					symbolName: "paintpalette"
				)
			}
		}
	}

	private var tintHex: String {
		let color = NSColor(
			hue: CGFloat(settings.hudTintHue),
			saturation: CGFloat(settings.hudTintSaturation),
			brightness: CGFloat(settings.hudTintBrightness),
			alpha: 1
		)
		let converted = color.usingColorSpace(.deviceRGB) ?? color
		return String(
			format: "#%02X%02X%02X",
			Int((converted.redComponent * 255).rounded()),
			Int((converted.greenComponent * 255).rounded()),
			Int((converted.blueComponent * 255).rounded())
		)
	}
}

private struct CaptureInspector: View {
	let settings: NativeHostSettings

	var body: some View {
		VStack(alignment: .leading, spacing: 12) {
			ShortcutHero(
				title: NativeHostSettings.captureHotKeyPresentation(for: settings.captureHotkey)
					.displayTitle
			)
			InspectorMetric(
				title: "Toolbar",
				value: settings.toolbarPlacement.title,
				symbolName: "rectangle.bottomthird.inset.filled"
			)
			InspectorMetric(
				title: "Loupe",
				value: settings.loupeSampleSize.title,
				symbolName: "plus.magnifyingglass"
			)
			InspectorMetric(
				title: "Handles",
				value: settings.frozenResizeHandleOrientation.title,
				symbolName: "crop"
			)
		}
	}
}

private struct OutputInspector: View {
	let settings: NativeHostSettings

	var body: some View {
		VStack(alignment: .leading, spacing: 12) {
			OutputFilePreview(settings: settings)
			InspectorMetric(
				title: "Folder",
				value: abbreviatedPath(settings.outputDirectory),
				symbolName: "folder"
			)
			InspectorMetric(
				title: "Naming",
				value: settings.outputNaming.title,
				symbolName: "number"
			)
			InspectorMetric(
				title: "Frame",
				value: settings.captureFrameEffectEnabled
					? settings.captureFrameBackground.title : CaptureFramePresetOption.off.title,
				symbolName: "photo.on.rectangle.angled"
			)
		}
	}
}

private struct PermissionsInspector: View {
	var body: some View {
		let granted = NativePermissions.screenRecordingGranted ? 1 : 0

		VStack(alignment: .leading, spacing: 12) {
			PermissionProgressBadge(granted: granted, total: 1)
			InspectorMetric(
				title: "Required",
				value: "1",
				symbolName: "lock.shield"
			)
			InspectorMetric(
				title: "Granted",
				value: "\(granted)",
				symbolName: "checkmark.seal"
			)
		}
	}
}

private struct AboutInspector: View {
	var body: some View {
		VStack(alignment: .leading, spacing: 12) {
			SettingsBrandIcon()
				.frame(width: 42, height: 42)
			InspectorMetric(
				title: "Source",
				value: "Open",
				symbolName: "curlybraces.square"
			)
		}
	}
}

private struct MiniHudPreview: View {
	let settings: NativeHostSettings
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		ZStack(alignment: .bottomLeading) {
			RoundedRectangle(cornerRadius: 16, style: .continuous)
				.fill(previewFill)
			LinearGradient(
				colors: [
					tintColor.opacity(settings.hudTint.clamped(to: 0...1) * 0.42 + 0.10),
					Color.clear,
				],
				startPoint: .topLeading,
				endPoint: .bottomTrailing
			)
			.clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))

			HStack(spacing: 7) {
				PreviewDot()
				Capsule()
					.fill(Color.primary.opacity(0.62))
					.frame(width: 44, height: 5)
				Capsule()
					.fill(Color.primary.opacity(0.24))
					.frame(width: 22, height: 5)
			}
			.padding(14)
		}
		.frame(height: 112)
		.overlay(alignment: .topLeading) {
			Text("HUD Preview")
				.font(.system(size: 10.5, weight: .semibold))
				.foregroundStyle(.secondary)
				.padding(14)
		}
		.overlay {
			RoundedRectangle(cornerRadius: 16, style: .continuous)
				.stroke(Color.white.opacity(colorScheme == .light ? 0.52 : 0.10), lineWidth: 1)
		}
	}

	private var tintColor: Color {
		Color(
			hue: settings.hudTintHue,
			saturation: settings.hudTintSaturation,
			brightness: settings.hudTintBrightness
		)
	}

	private var previewFill: Color {
		colorScheme == .light ? Color.white.opacity(0.46) : Color.black.opacity(0.18)
	}
}

private struct PreviewDot: View {
	var body: some View {
		ZStack {
			Circle()
				.fill(Color.accentColor.opacity(0.22))
			Image(systemName: "sparkles")
				.font(.system(size: 11, weight: .semibold))
				.foregroundStyle(Color.accentColor)
		}
		.frame(width: 26, height: 26)
	}
}

private struct ShortcutHero: View {
	let title: String

	var body: some View {
		VStack(alignment: .leading, spacing: 10) {
			Text("Global Shortcut")
				.font(.system(size: 10.5, weight: .semibold))
				.foregroundStyle(.secondary)
			HStack(spacing: 6) {
				ForEach(title.split(separator: "-").map(String.init), id: \.self) { token in
					Text(token)
						.font(.system(size: 12, weight: .bold, design: .rounded))
						.padding(.horizontal, 9)
						.padding(.vertical, 7)
						.background(Color.primary.opacity(0.10), in: .rect(cornerRadius: 8))
				}
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(12)
		.background(Color.primary.opacity(0.050), in: .rect(cornerRadius: 14))
	}
}

private struct OutputFilePreview: View {
	let settings: NativeHostSettings

	var body: some View {
		VStack(alignment: .leading, spacing: 9) {
			HStack(spacing: 8) {
				Image(systemName: "doc.richtext")
					.font(.system(size: 17, weight: .semibold))
					.foregroundStyle(Color.accentColor)
				Text(sampleName)
					.font(.system(size: 11.5, weight: .semibold, design: .monospaced))
					.lineLimit(1)
			}
			Text(abbreviatedPath(settings.outputDirectory))
				.font(.system(size: 10, weight: .medium))
				.foregroundStyle(.secondary)
				.lineLimit(2)
		}
		.padding(12)
		.frame(maxWidth: .infinity, alignment: .leading)
		.background(Color.primary.opacity(0.050), in: .rect(cornerRadius: 14))
	}

	private var sampleName: String {
		switch settings.outputNaming {
		case .timestamp:
			return "\(settings.outputFilenamePrefix)-20260503.png"
		case .sequence:
			return "\(settings.outputFilenamePrefix)-0038.png"
		}
	}
}

private struct PermissionProgressBadge: View {
	let granted: Int
	let total: Int

	var body: some View {
		VStack(alignment: .leading, spacing: 9) {
			ZStack {
				Circle()
					.stroke(Color.primary.opacity(0.10), lineWidth: 8)
				Circle()
					.trim(from: 0, to: progress)
					.stroke(Color.accentColor, style: StrokeStyle(lineWidth: 8, lineCap: .round))
					.rotationEffect(.degrees(-90))
				Text("\(granted)/\(max(total, 1))")
					.font(.system(size: 15, weight: .bold, design: .rounded))
			}
			.frame(width: 74, height: 74)

			Text(total == granted ? "Capture access ready" : "Permission setup needed")
				.font(.system(size: 11.5, weight: .semibold))
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(12)
		.background(Color.primary.opacity(0.050), in: .rect(cornerRadius: 14))
	}

	private var progress: CGFloat {
		guard total > 0 else {
			return 1
		}
		return CGFloat(granted) / CGFloat(total)
	}
}

private struct InspectorMetric: View {
	let title: String
	let value: String
	let symbolName: String

	var body: some View {
		HStack(spacing: 9) {
			Image(systemName: symbolName)
				.symbolRenderingMode(.hierarchical)
				.font(.system(size: 11.5, weight: .semibold))
				.foregroundStyle(.secondary)
				.frame(width: 18, height: 18)
			VStack(alignment: .leading, spacing: 2) {
				Text(title)
					.font(.system(size: 9.5, weight: .medium))
					.foregroundStyle(.secondary)
				Text(value)
					.font(.system(size: 11, weight: .semibold))
					.lineLimit(1)
					.minimumScaleFactor(0.76)
			}
			Spacer(minLength: 0)
		}
		.padding(.horizontal, 10)
		.padding(.vertical, 8)
		.background(Color.primary.opacity(0.045), in: .rect(cornerRadius: 12))
	}
}

private struct HudGlassModePicker: View {
	let selection: HudGlassModePreference
	let isEnabled: Bool
	let onSelect: (HudGlassModePreference) -> Void

	var body: some View {
		HStack(spacing: 8) {
			ForEach(HudGlassModePreference.allCases, id: \.rawValue) { mode in
				let available =
					mode != .liquidGlass || LiveChromeGlassMaterialSupport.isLiquidGlassAvailable
				ModernSegmentButton(
					title: mode.title,
					isSelected: selection == mode,
					isEnabled: isEnabled && available
				) {
					onSelect(mode)
				}
				.help(available ? mode.title : LiveChromeGlassMaterialSupport.unavailableHelpText)
			}
		}
		.padding(.horizontal, 1)
		.frame(width: SettingsControlLayout.controlColumnWidth)
		.segmentedGlassBackground()
	}
}

private struct LiquidGlassStylePicker: View {
	let selection: LiquidGlassStylePreference
	let isEnabled: Bool
	let onSelect: (LiquidGlassStylePreference) -> Void

	var body: some View {
		HStack(spacing: 8) {
			ForEach(LiquidGlassStylePreference.allCases, id: \.rawValue) { style in
				ModernSegmentButton(
					title: style.title,
					isSelected: selection == style,
					isEnabled: isEnabled
				) {
					onSelect(style)
				}
			}
		}
		.padding(.horizontal, 1)
		.frame(width: SettingsControlLayout.controlColumnWidth)
		.segmentedGlassBackground()
	}
}

private struct ToolbarPlacementPicker: View {
	let selection: ToolbarPlacementPreference
	let onSelect: (ToolbarPlacementPreference) -> Void

	var body: some View {
		HStack(spacing: 8) {
			ForEach(ToolbarPlacementPreference.allCases, id: \.rawValue) { placement in
				ModernSegmentButton(
					title: placement.title,
					isSelected: selection == placement,
					isEnabled: true
				) {
					onSelect(placement)
				}
			}
		}
		.padding(.horizontal, 1)
		.frame(width: SettingsControlLayout.controlColumnWidth)
		.segmentedGlassBackground()
	}
}

private struct FrozenResizeHandleOrientationPicker: View {
	let selection: FrozenResizeHandleOrientationPreference
	let onSelect: (FrozenResizeHandleOrientationPreference) -> Void

	var body: some View {
		HStack(spacing: 8) {
			ForEach(FrozenResizeHandleOrientationPreference.allCases, id: \.rawValue) {
				orientation in
				ModernSegmentButton(
					title: orientation.title,
					isSelected: selection == orientation,
					isEnabled: true
				) {
					onSelect(orientation)
				}
			}
		}
		.padding(.horizontal, 1)
		.frame(width: SettingsControlLayout.controlColumnWidth)
		.segmentedGlassBackground()
	}
}

private struct LoupeSampleSizePicker: View {
	let selection: LoupeSampleSizePreference
	let onSelect: (LoupeSampleSizePreference) -> Void

	var body: some View {
		HStack(spacing: 8) {
			ForEach(LoupeSampleSizePreference.allCases, id: \.rawValue) { size in
				ModernSegmentButton(
					title: size.title,
					isSelected: selection == size,
					isEnabled: true
				) {
					onSelect(size)
				}
			}
		}
		.padding(.horizontal, 1)
		.frame(width: SettingsControlLayout.controlColumnWidth)
		.segmentedGlassBackground()
	}
}

private struct SoftwareUpdateModePicker: View {
	let snapshot: NativeHostSoftwareUpdater.Snapshot
	let onSelect: (NativeHostSoftwareUpdater.Mode) -> Void

	var body: some View {
		HStack(spacing: 8) {
			ForEach(NativeHostSoftwareUpdater.Mode.allCases, id: \.rawValue) { mode in
				let enabled = isEnabled(mode)
				ModernSegmentButton(
					title: mode.title,
					isSelected: snapshot.mode == mode,
					isEnabled: enabled
				) {
					onSelect(mode)
				}
				.help(helpText(for: mode, isEnabled: enabled))
			}
		}
		.padding(.horizontal, 1)
		.frame(width: SettingsControlLayout.controlColumnWidth)
		.segmentedGlassBackground()
	}

	private func isEnabled(_ mode: NativeHostSoftwareUpdater.Mode) -> Bool {
		guard snapshot.isConfigured else {
			return false
		}
		if mode == .install {
			return snapshot.allowsAutomaticUpdates
		}
		return true
	}

	private func helpText(
		for mode: NativeHostSoftwareUpdater.Mode,
		isEnabled: Bool
	) -> String {
		if !snapshot.isConfigured {
			return "Sparkle appcast not configured."
		}
		if mode == .install, !isEnabled {
			return "Automatic install is unavailable."
		}
		switch mode {
		case .off:
			return "Turn off automatic update checks."
		case .check:
			return "Check automatically and notify when an update is available."
		case .install:
			return "Download updates automatically and install after confirmation."
		}
	}
}

private struct OutputNamingPicker: View {
	let selection: OutputNamingPreference
	let onSelect: (OutputNamingPreference) -> Void

	var body: some View {
		HStack(spacing: 8) {
			ForEach(OutputNamingPreference.allCases, id: \.rawValue) { naming in
				ModernSegmentButton(
					title: naming.title,
					isSelected: selection == naming,
					isEnabled: true
				) {
					onSelect(naming)
				}
			}
		}
		.padding(.horizontal, 1)
		.frame(width: SettingsControlLayout.controlColumnWidth)
		.segmentedGlassBackground()
	}
}

private enum CaptureFramePresetOption: Hashable, Identifiable {
	case off
	case background(CaptureFrameBackgroundPreference)

	var id: String {
		switch self {
		case .off:
			return "off"
		case .background(let background):
			return background.rawValue
		}
	}

	var title: String {
		switch self {
		case .off:
			return "Off"
		case .background(let background):
			return background.title
		}
	}
}

private struct CaptureFramePresetPicker: View {
	let selection: CaptureFramePresetOption
	let onSelect: (CaptureFramePresetOption) -> Void

	var body: some View {
		Picker(
			"",
			selection: Binding(
				get: { selection },
				set: { value in onSelect(value) }
			)
		) {
			Text(CaptureFramePresetOption.off.title).tag(CaptureFramePresetOption.off)
			ForEach(CaptureFrameBackgroundPreference.allCases, id: \.rawValue) { background in
				let option = CaptureFramePresetOption.background(background)
				Text(option.title).tag(option)
			}
		}
		.labelsHidden()
		.pickerStyle(.menu)
		.frame(width: SettingsControlLayout.controlColumnWidth)
	}
}

private struct CaptureFrameApplicabilityPicker: View {
	let selection: CaptureFrameApplicabilityPreference
	let isEnabled: Bool
	let onSelect: (CaptureFrameApplicabilityPreference) -> Void

	var body: some View {
		HStack(spacing: 8) {
			ForEach(CaptureFrameApplicabilityPreference.allCases, id: \.rawValue) { target in
				ModernSegmentButton(
					title: target.title,
					isSelected: selection == target,
					isEnabled: isEnabled
				) {
					onSelect(target)
				}
			}
		}
		.padding(.horizontal, 1)
		.frame(width: SettingsControlLayout.controlColumnWidth)
		.segmentedGlassBackground()
		.disabled(!isEnabled)
	}
}

private struct ModernSegmentButton: View {
	let title: String
	let isSelected: Bool
	let isEnabled: Bool
	let action: () -> Void
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false

	var body: some View {
		Button {
			withAnimation(.spring(response: 0.22, dampingFraction: 0.84)) {
				action()
			}
		} label: {
			VStack(spacing: 3) {
				Text(title)
					.font(.system(size: 10.2, weight: .semibold))
					.lineLimit(1)
					.minimumScaleFactor(0.9)
					.foregroundStyle(textColor)
					.padding(.horizontal, 6)

				Capsule()
					.fill(isSelected ? Color.accentColor : Color.clear)
					.frame(width: 14, height: 2)
			}
			.padding(.vertical, 2)
			.frame(maxWidth: .infinity, minHeight: 22)
			.background(hoverBackground, in: .rect(cornerRadius: 6, style: .continuous))
			.contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
		}
		.buttonStyle(.plain)
		.disabled(!isEnabled)
		.frame(maxWidth: .infinity)
		.animation(.spring(response: 0.22, dampingFraction: 0.84), value: isSelected)
		.animation(.easeOut(duration: 0.12), value: isHovered)
		.onHover { hovering in
			isHovered = hovering
		}
	}

	private var hoverBackground: Color {
		if isHovered && !isSelected && isEnabled {
			return colorScheme == .light ? Color.black.opacity(0.035) : Color.white.opacity(0.050)
		}
		return .clear
	}

	private var textColor: Color {
		if !isEnabled {
			return Color.secondary.opacity(0.54)
		}
		if isSelected {
			return Color.accentColor
		}
		return Color.primary.opacity(colorScheme == .light ? 0.88 : 0.92)
	}
}

private struct LaunchAtLoginToggle: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		Toggle(
			"",
			isOn: Binding(
				get: { model.launchAtLoginState.isOn },
				set: { value in model.setLaunchAtLoginEnabled(value) }
			)
		)
		.labelsHidden()
		.toggleStyle(SettingsToggleStyle())
		.disabled(!model.launchAtLoginState.isControlEnabled)
		.help(model.launchAtLoginState.helpText)
	}
}

private struct AppearanceSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		VStack(spacing: 8) {
			SettingsHeroControlTile(
				symbolName: "sparkles",
				title: "Glass HUD",
				subtitle: "Translucent capture chrome."
			) {
				Toggle(
					"",
					isOn: Binding(
						get: { model.settings.hudGlassEnabled },
						set: { value in model.update { $0.hudGlassEnabled = value } }
					)
				)
				.labelsHidden()
				.toggleStyle(SettingsToggleStyle())
			}

			VStack(spacing: 0) {
				SettingsControlTile(
					symbolName: "rectangle.3.group.bubble",
					title: "Material",
					subtitle: materialSubtitle
				) {
					HudGlassModePicker(
						selection: model.settings.resolvedHudGlassMode,
						isEnabled: model.settings.hudGlassEnabled,
						onSelect: updateGlassMode
					)
					.disabled(!model.settings.hudGlassEnabled)
				}

				if model.settings.resolvedHudGlassMode == .liquidGlass {
					SettingsControlTile(
						symbolName: "circle.hexagongrid",
						title: "Liquid Style",
						subtitle: "Material profile."
					) {
						LiquidGlassStylePicker(
							selection: model.settings.liquidGlassStyle,
							isEnabled: model.settings.hudGlassEnabled
						) { value in
							model.update { $0.liquidGlassStyle = value }
						}
						.disabled(!model.settings.hudGlassEnabled)
					}
					.transition(.opacity)
				}
			}

			if model.settings.resolvedHudGlassMode == .classicGlass {
				SettingsControlTile(
					symbolName: "slider.horizontal.3",
					title: "Classic Tuning",
					subtitle: "Opacity and blur."
				) {
					SettingsCompactSliderStack(
						primaryValue: Binding(
							get: { model.settings.hudOpacity },
							set: { value in model.update { $0.hudOpacity = value } }
						),
						primaryLabel: "Opacity",
						secondaryValue: Binding(
							get: { model.settings.hudBlur },
							set: { value in model.update { $0.hudBlur = value } }
						),
						secondaryLabel: "Blur",
						isEnabled: model.settings.hudGlassEnabled
					)
				}
				.transition(.opacity)
			}

			VStack(spacing: 0) {
				SettingsControlTile(
					symbolName: "eyedropper.halffull",
					title: "Tint Strength",
					subtitle: "Accent weight."
				) {
					SettingsTileSlider(
						value: Binding(
							get: { model.settings.hudTint },
							set: { value in model.update { $0.hudTint = value } }
						),
						isEnabled: model.settings.hudGlassEnabled
					)
				}

				SettingsControlTile(
					symbolName: "paintpalette",
					title: "Tint Color",
					subtitle: "HUD accent."
				) {
					FlatColorSwatch(
						selection: tintColorBinding,
						isEnabled: model.settings.hudGlassEnabled
					)
				}
			}
		}
		.animation(
			.spring(response: 0.26, dampingFraction: 0.86),
			value: model.settings.resolvedHudGlassMode
		)
	}

	private var materialSubtitle: String {
		LiveChromeGlassMaterialSupport.settingsSubtitle
	}

	private var tintColorBinding: Binding<Color> {
		Binding(
			get: {
				Color(
					hue: model.settings.hudTintHue,
					saturation: model.settings.hudTintSaturation,
					brightness: model.settings.hudTintBrightness
				)
			},
			set: { color in
				let nsColor = NSColor(color)
				let converted = nsColor.usingColorSpace(.deviceRGB) ?? nsColor
				var hue: CGFloat = 0
				var saturation: CGFloat = 0
				var brightness: CGFloat = 0
				converted.getHue(&hue, saturation: &saturation, brightness: &brightness, alpha: nil)
				model.update {
					$0.hudTintHue = Double(hue)
					$0.hudTintSaturation = Double(saturation)
					$0.hudTintBrightness = Double(brightness)
				}
			}
		)
	}

	private func updateGlassMode(_ mode: HudGlassModePreference) {
		if mode == .liquidGlass, !LiveChromeGlassMaterialSupport.isLiquidGlassAvailable {
			model.refresh()
			return
		}
		model.update { $0.hudGlassMode = mode }
	}
}

private struct SettingsHeroControlTile<Control: View>: View {
	let symbolName: String
	let title: String
	let subtitle: String
	let control: Control

	init(
		symbolName: String,
		title: String,
		subtitle: String,
		@ViewBuilder control: () -> Control
	) {
		self.symbolName = symbolName
		self.title = title
		self.subtitle = subtitle
		self.control = control()
	}

	var body: some View {
		HStack(spacing: 10) {
			SettingsTileIcon(symbolName: symbolName, size: 20)
			VStack(alignment: .leading, spacing: 2) {
				Text(title)
					.font(.system(size: 13, weight: .semibold))
				Text(subtitle)
					.font(.system(size: 10.8, weight: .medium))
					.foregroundStyle(.secondary)
					.lineLimit(1)
					.minimumScaleFactor(0.86)
			}
			.layoutPriority(1)
			Spacer(minLength: 8)
			control
				.frame(width: SettingsControlLayout.controlColumnWidth, alignment: .trailing)
		}
		.padding(.vertical, 6)
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

private struct SettingsControlTile<Control: View>: View {
	let symbolName: String
	let title: String
	let subtitle: String
	let control: Control

	init(
		symbolName: String,
		title: String,
		subtitle: String,
		@ViewBuilder control: () -> Control
	) {
		self.symbolName = symbolName
		self.title = title
		self.subtitle = subtitle
		self.control = control()
	}

	var body: some View {
		HStack(spacing: 10) {
			SettingsTileIcon(symbolName: symbolName, size: 19)
			VStack(alignment: .leading, spacing: 2) {
				Text(title)
					.font(.system(size: 13, weight: .semibold))
					.lineLimit(1)
				Text(subtitle)
					.font(.system(size: 10.5, weight: .medium))
					.foregroundStyle(.secondary)
					.lineLimit(1)
					.minimumScaleFactor(0.86)
			}
			.layoutPriority(1)
			Spacer(minLength: 10)
			control
				.frame(width: SettingsControlLayout.controlColumnWidth, alignment: .trailing)
		}
		.padding(.vertical, 5)
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

private struct SettingsTileIcon: View {
	let symbolName: String
	let size: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Image(systemName: symbolName)
			.symbolRenderingMode(.hierarchical)
			.font(.system(size: size * 0.86, weight: .semibold))
			.foregroundStyle(Color.accentColor.opacity(colorScheme == .light ? 0.88 : 0.95))
			.frame(width: size + 8, height: size + 8)
			.contentShape(Rectangle())
	}
}

private struct SettingsTileSlider: View {
	@Binding var value: Double
	let isEnabled: Bool

	var body: some View {
		HStack(spacing: 8) {
			GlassSlider(value: $value, isEnabled: isEnabled)
				.frame(width: SettingsControlLayout.sliderTrackWidth, height: 24)
			Text("\(Int((value * 100).rounded()))%")
				.font(.system(size: 10.6, weight: .semibold, design: .monospaced))
				.foregroundStyle(.secondary)
				.frame(width: SettingsControlLayout.sliderValueWidth, alignment: .trailing)
		}
		.frame(width: SettingsControlLayout.controlColumnWidth, alignment: .trailing)
	}
}

private struct SettingsCompactSliderStack: View {
	@Binding var primaryValue: Double
	let primaryLabel: String
	@Binding var secondaryValue: Double
	let secondaryLabel: String
	let isEnabled: Bool

	var body: some View {
		VStack(spacing: 2) {
			SettingsCompactSliderLine(
				label: primaryLabel,
				value: $primaryValue,
				isEnabled: isEnabled
			)
			SettingsCompactSliderLine(
				label: secondaryLabel,
				value: $secondaryValue,
				isEnabled: isEnabled
			)
		}
		.frame(width: SettingsControlLayout.controlColumnWidth)
		.opacity(isEnabled ? 1 : 0.46)
		.animation(.easeOut(duration: 0.14), value: isEnabled)
	}
}

private struct SettingsCompactSliderLine: View {
	let label: String
	@Binding var value: Double
	let isEnabled: Bool

	var body: some View {
		HStack(spacing: 6) {
			Text(label)
				.font(.system(size: 10.3, weight: .medium))
				.foregroundStyle(.secondary)
				.lineLimit(1)
				.frame(width: SettingsControlLayout.compactSliderLabelWidth, alignment: .leading)
			GlassSlider(value: $value, isEnabled: isEnabled)
				.frame(width: SettingsControlLayout.compactSliderTrackWidth, height: 16)
			Text("\(Int((value * 100).rounded()))%")
				.font(.system(size: 10.3, weight: .semibold, design: .monospaced))
				.foregroundStyle(.secondary)
				.frame(width: SettingsControlLayout.sliderValueWidth, alignment: .trailing)
		}
	}
}

private struct FlatColorSwatch: View {
	@Binding var selection: Color
	let isEnabled: Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false

	var body: some View {
		ZStack {
			RoundedRectangle(cornerRadius: 6, style: .continuous)
				.fill(selection)
			SettingsColorWell(selection: $selection, isEnabled: isEnabled)
		}
		.overlay {
			RoundedRectangle(cornerRadius: 6, style: .continuous)
				.stroke(borderColor, lineWidth: 1)
				.allowsHitTesting(false)
		}
		.frame(width: 30, height: 18)
		.opacity(isEnabled ? 1 : 0.45)
		.scaleEffect(isHovered && isEnabled ? 1.04 : 1)
		.animation(.easeOut(duration: 0.12), value: isHovered)
		.onHover { hovering in
			isHovered = hovering
		}
		.allowsHitTesting(isEnabled)
	}

	private var borderColor: Color {
		colorScheme == .light ? Color.black.opacity(0.12) : Color.white.opacity(0.18)
	}
}

private struct SettingsColorWell: NSViewRepresentable {
	@Binding var selection: Color
	let isEnabled: Bool

	func makeNSView(context: Context) -> ColorPanelTriggerView {
		let view = ColorPanelTriggerView()
		view.coordinator = context.coordinator
		return view
	}

	func updateNSView(_ view: ColorPanelTriggerView, context: Context) {
		context.coordinator.selection = $selection
		view.allowsColorPanel = isEnabled
		view.color = NSColor(selection).usingColorSpace(.deviceRGB) ?? NSColor(selection)
		view.coordinator = context.coordinator
	}

	func makeCoordinator() -> Coordinator {
		Coordinator(selection: $selection)
	}

	final class ColorPanelTriggerView: NSView {
		var allowsColorPanel = true
		var color = NSColor.systemBlue
		weak var coordinator: Coordinator?

		override var acceptsFirstResponder: Bool {
			false
		}

		override func mouseDown(with event: NSEvent) {
			guard allowsColorPanel else {
				return
			}
			NSApp.activate(ignoringOtherApps: true)
			let panel = NSColorPanel.shared
			panel.showsAlpha = false
			panel.isContinuous = true
			panel.color = color
			panel.setTarget(coordinator)
			panel.setAction(#selector(Coordinator.colorChanged(_:)))
			panel.orderFront(self)
		}
	}

	final class Coordinator: NSObject {
		var selection: Binding<Color>

		init(selection: Binding<Color>) {
			self.selection = selection
		}

		@MainActor
		@objc
		func colorChanged(_ sender: NSColorPanel) {
			NSColorPanel.shared.showsAlpha = false
			selection.wrappedValue = Color(nsColor: sender.color)
		}
	}
}

private struct CaptureSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		VStack(spacing: 8) {
			SettingsHeroControlTile(
				symbolName: "keyboard",
				title: "New Capture Shortcut",
				subtitle: "Current: \(shortcutPresentation.displayTitle)."
			) {
				CaptureHotKeyField(model: model)
			}

			VStack(spacing: 0) {
				SettingsControlTile(
					symbolName: "rectangle.bottomthird.inset.filled",
					title: "Frozen Toolbar",
					subtitle: "Command bar."
				) {
					ToolbarPlacementPicker(selection: model.settings.toolbarPlacement) { value in
						model.update { $0.toolbarPlacement = value }
					}
				}

				SettingsControlTile(
					symbolName: "crop",
					title: "Corner Handles",
					subtitle: "Resize direction."
				) {
					FrozenResizeHandleOrientationPicker(
						selection: model.settings.frozenResizeHandleOrientation
					) { value in
						model.update { $0.frozenResizeHandleOrientation = value }
					}
				}
			}

			VStack(spacing: 0) {
				SettingsControlTile(
					symbolName: "plus.magnifyingglass",
					title: "Loupe Sample",
					subtitle: "Patch size."
				) {
					LoupeSampleSizePicker(selection: model.settings.loupeSampleSize) { value in
						model.update { $0.loupeSampleSize = value }
					}
				}

				SettingsControlTile(
					symbolName: "lightbulb",
					title: "HUD Hint",
					subtitle: "Tab keycap."
				) {
					Toggle(
						"",
						isOn: Binding(
							get: { model.settings.showAltHintKeycap },
							set: { value in model.update { $0.showAltHintKeycap = value } }
						)
					)
					.labelsHidden()
					.toggleStyle(SettingsToggleStyle())
				}
			}
		}
	}

	private var shortcutPresentation: CaptureHotKeyPresentation {
		NativeHostSettings.captureHotKeyPresentation(for: model.settings.captureHotkey)
	}
}

private struct CaptureHotKeyField: View {
	@ObservedObject var model: NativeHostSettingsViewModel
	@FocusState private var isFocused: Bool
	@State private var draft = ""

	var body: some View {
		TextField("Option-X", text: $draft)
			.font(.system(size: 10.5, weight: .semibold, design: .monospaced))
			.textFieldStyle(.plain)
			.padding(.horizontal, 10)
			.padding(.vertical, 6)
			.background(Color.primary.opacity(0.070), in: .rect(cornerRadius: 9))
			.overlay {
				RoundedRectangle(cornerRadius: 9, style: .continuous)
					.stroke(Color.primary.opacity(0.075), lineWidth: 1)
			}
			.frame(width: SettingsControlLayout.controlColumnWidth)
			.focused($isFocused)
			.onAppear(perform: syncDraft)
			.onSubmit(commitDraft)
			.onChange(of: isFocused) { _, focused in
				if focused {
					syncDraft()
				} else {
					commitDraft()
				}
			}
			.onChange(of: model.settings.captureHotkey) { _, _ in
				if !isFocused {
					syncDraft()
				}
			}
	}

	private func syncDraft() {
		draft =
			NativeHostSettings.captureHotKeyPresentation(
				for: model.settings.captureHotkey
			).displayTitle
	}

	private func commitDraft() {
		let committed = NativeHostSettings.captureHotKeyPresentation(for: draft).displayTitle
		if committed != model.settings.captureHotkey {
			model.update { $0.captureHotkey = committed }
		}
		draft = committed
	}
}

private struct PermissionsSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel
	@State private var refreshID = 0

	var body: some View {
		VStack(spacing: 8) {
			VStack(spacing: 0) {
				PermissionGrantCard(
					refreshID: refreshID,
					bundleURL: Self.appBundleURL,
					appIcon: Self.appIcon,
					openSettings: {
						NativePermissions.openScreenRecordingSettings()
					},
					refresh: {
						refreshID += 1
						model.refresh()
					}
				)
			}

			SettingsHeroControlTile(
				symbolName: "power",
				title: "Open At Login",
				subtitle: model.launchAtLoginState.subtitle
			) {
				LaunchAtLoginToggle(model: model)
			}
		}
	}

	private static var appBundleURL: URL {
		Bundle.main.bundleURL
	}

	private static var appIcon: NSImage {
		NSWorkspace.shared.icon(forFile: appBundleURL.path)
	}
}

private struct PermissionGrantCard: View {
	let refreshID: Int
	let bundleURL: URL
	let appIcon: NSImage
	let openSettings: () -> Void
	let refresh: () -> Void
	@Environment(\.colorScheme) private var colorScheme
	@State private var didRefresh = false

	var body: some View {
		VStack(alignment: .leading, spacing: 7) {
			HStack(alignment: .top, spacing: 11) {
				ZStack {
					Circle()
						.fill(iconBackgroundColor)
					Image(systemName: isGranted ? "checkmark.seal.fill" : "hand.draw.fill")
						.symbolRenderingMode(.hierarchical)
						.font(.system(size: 17, weight: .semibold))
						.foregroundStyle(isGranted ? Color.green : Color.accentColor)
				}
				.frame(width: 34, height: 34)

				VStack(alignment: .leading, spacing: 5) {
					HStack(alignment: .firstTextBaseline, spacing: 7) {
						Text(title)
							.font(.system(size: 12.5, weight: .semibold))
							.lineLimit(2)
							.fixedSize(horizontal: false, vertical: true)
							.layoutPriority(1)
						PermissionStateBadge(
							title: isGranted ? "Granted" : "Required",
							style: isGranted ? .granted : .required
						)
					}
					Text(subtitle)
						.font(.system(size: 10.5, weight: .medium))
						.foregroundStyle(.secondary)
						.lineLimit(2)
						.fixedSize(horizontal: false, vertical: true)
				}
			}

			HStack(alignment: .center, spacing: 8) {
				PermissionAppDragSource(
					bundleURL: bundleURL,
					icon: appIcon,
					label: NativeHostBrand.appBundleName
				)
				.frame(width: 108, height: 31)
				.opacity(isGranted ? 0.76 : 1)

				Spacer(minLength: 6)

				Button {
					openSettings()
				} label: {
					Label("Settings", systemImage: "gearshape")
						.labelStyle(.titleAndIcon)
				}
				.rsnapGlassButton(prominent: false)
				.controlSize(.small)
				.help("Open Screen Recording settings")

				Button(action: refreshStatus) {
					Label(
						didRefresh ? "Updated" : "Refresh",
						systemImage: didRefresh ? "checkmark" : "arrow.clockwise"
					)
					.labelStyle(.titleAndIcon)
				}
				.rsnapGlassButton(prominent: false)
				.controlSize(.small)
				.help("Refresh permission status")
			}
		}
		.padding(.vertical, 7)
		.frame(maxWidth: .infinity, minHeight: 98, alignment: .leading)
	}

	private var isGranted: Bool {
		_ = refreshID
		return NativePermissions.screenRecordingGranted
	}

	private var title: String {
		isGranted ? "Screen Recording ready" : "Screen Recording access needed"
	}

	private var subtitle: String {
		if isGranted {
			return "The native capture host can see the screen."
		}
		return "Open System Settings, then drag Rsnap.app into the Screen Recording app list."
	}

	private var iconBackgroundColor: Color {
		if isGranted {
			return Color.green.opacity(colorScheme == .light ? 0.12 : 0.18)
		}
		return Color.accentColor.opacity(colorScheme == .light ? 0.12 : 0.20)
	}

	private func refreshStatus() {
		refresh()
		didRefresh = true
		DispatchQueue.main.asyncAfter(deadline: .now() + 1.1) {
			didRefresh = false
		}
	}

}

private struct PermissionStateBadge: View {
	enum Style {
		case granted
		case required
	}

	let title: String
	let style: Style
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Text(title)
			.font(.system(size: 9.2, weight: .semibold))
			.lineLimit(1)
			.fixedSize(horizontal: true, vertical: false)
			.padding(.horizontal, 7)
			.padding(.vertical, 4)
			.foregroundStyle(foregroundColor)
			.background(backgroundColor, in: Capsule())
			.overlay {
				Capsule()
					.stroke(borderColor, lineWidth: 1)
			}
	}

	private var foregroundColor: Color {
		switch style {
		case .granted:
			return Color.green
		case .required:
			return Color.accentColor
		}
	}

	private var backgroundColor: Color {
		switch style {
		case .granted:
			return Color.green.opacity(colorScheme == .light ? 0.10 : 0.16)
		case .required:
			return Color.accentColor.opacity(colorScheme == .light ? 0.10 : 0.18)
		}
	}

	private var borderColor: Color {
		switch style {
		case .granted:
			return Color.green.opacity(colorScheme == .light ? 0.20 : 0.26)
		case .required:
			return Color.accentColor.opacity(colorScheme == .light ? 0.20 : 0.28)
		}
	}
}

private struct AboutSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		VStack(alignment: .leading, spacing: 8) {
			AboutIntroBlock()

			VStack(spacing: 0) {
				AboutLinkTile(
					symbolName: "curlybraces.square",
					title: "Open Source",
					buttonTitle: "GitHub",
					urlString: NativeHostAboutLinks.source
				)
			}

			VStack(spacing: 0) {
				SettingsControlTile(
					symbolName: "arrow.triangle.2.circlepath",
					title: "Auto Update",
					subtitle: model.softwareUpdateSettings.modeSubtitle
				) {
					SoftwareUpdateModePicker(snapshot: model.softwareUpdateSettings) { mode in
						model.setSoftwareUpdateMode(mode)
					}
				}

				SettingsControlTile(
					symbolName: "tag",
					title: model.softwareUpdateSettings.releaseVersionTitle,
					subtitle: model.softwareUpdateSettings.releaseVersionSubtitle
				) {
					UpdateCheckButtonGroup(model: model)
				}
			}
		}
	}
}

private struct UpdateCheckButtonGroup: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		HStack(spacing: 6) {
			Button {
				model.checkForUpdates()
			} label: {
				Label("Check", systemImage: "arrow.clockwise")
					.labelStyle(.titleAndIcon)
			}
			.rsnapGlassButton(prominent: false)
			.controlSize(.small)
			.disabled(
				model.softwareUpdateSettings.isConfigured
					&& !model.softwareUpdateSettings.canCheckForUpdates)
		}
	}
}

private struct AboutIntroBlock: View {
	var body: some View {
		HStack(alignment: .top, spacing: 10) {
			SettingsTileIcon(symbolName: "sparkles", size: 20)
			VStack(alignment: .leading, spacing: 5) {
				HStack(alignment: .firstTextBaseline, spacing: 8) {
					Text("Built by Yvette Cipher")
						.font(.system(size: 13, weight: .semibold))
					Spacer(minLength: 8)
					Button(action: openCreator) {
						Label("Follow on X", systemImage: "arrow.up.forward")
							.labelStyle(.titleAndIcon)
					}
					.rsnapGlassButton(prominent: false)
					.controlSize(.small)
					.help(NativeHostAboutLinks.creator)
				}
				Text(
					"Rsnap is an open-source macOS capture tool. I keep sharing progress, design notes, and release updates on X; following helps support future work through X creator rewards."
				)
				.font(.system(size: 10.8, weight: .medium))
				.foregroundStyle(.secondary)
				.lineLimit(4)
				.fixedSize(horizontal: false, vertical: true)
			}
			.layoutPriority(1)
		}
		.padding(.vertical, 6)
		.frame(maxWidth: .infinity, alignment: .leading)
	}

	private func openCreator() {
		guard let url = URL(string: NativeHostAboutLinks.creator) else {
			return
		}
		NSWorkspace.shared.open(url)
	}
}

private struct AboutLinkTile: View {
	let symbolName: String
	let title: String
	let subtitle: String?
	let buttonTitle: String
	let urlString: String

	init(
		symbolName: String,
		title: String,
		subtitle: String? = nil,
		buttonTitle: String,
		urlString: String
	) {
		self.symbolName = symbolName
		self.title = title
		self.subtitle = subtitle
		self.buttonTitle = buttonTitle
		self.urlString = urlString
	}

	var body: some View {
		HStack(spacing: 10) {
			SettingsTileIcon(symbolName: symbolName, size: 19)
			VStack(alignment: .leading, spacing: hasSubtitle ? 2 : 0) {
				Text(title)
					.font(.system(size: 13, weight: .semibold))
					.lineLimit(1)
				if let subtitle, !subtitle.isEmpty {
					Text(subtitle)
						.font(.system(size: 10.5, weight: .medium))
						.foregroundStyle(.secondary)
						.lineLimit(2)
						.fixedSize(horizontal: false, vertical: true)
				}
			}
			.layoutPriority(1)
			Spacer(minLength: 10)
			HStack {
				Spacer(minLength: 0)
				Button(action: openURL) {
					Label(buttonTitle, systemImage: "arrow.up.forward")
						.labelStyle(.titleAndIcon)
				}
				.rsnapGlassButton(prominent: false)
				.controlSize(.small)
				.help(urlString)
			}
			.frame(width: SettingsControlLayout.controlColumnWidth, alignment: .trailing)
		}
		.padding(.vertical, 5)
		.frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
	}

	private func openURL() {
		guard let url = URL(string: urlString) else {
			return
		}
		NSWorkspace.shared.open(url)
	}

	private var hasSubtitle: Bool {
		guard let subtitle else {
			return false
		}
		return !subtitle.isEmpty
	}
}

private struct OutputSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		VStack(spacing: 8) {
			SettingsHeroControlTile(
				symbolName: "folder",
				title: "Save Location",
				subtitle: abbreviatedPath(model.settings.outputDirectory)
			) {
				Button(action: model.chooseOutputDirectory) {
					Label("Choose", systemImage: "folder.badge.plus")
						.labelStyle(.titleAndIcon)
				}
				.rsnapGlassButton(prominent: false)
				.controlSize(.small)
			}

			VStack(spacing: 0) {
				SettingsControlTile(
					symbolName: "textformat.abc",
					title: "Filename Prefix",
					subtitle: "Safe text."
				) {
					TextField(
						NativeHostBrand.defaultFilenamePrefix,
						text: Binding(
							get: { model.settings.outputFilenamePrefix },
							set: { value in model.update { $0.outputFilenamePrefix = value } }
						)
					)
					.textFieldStyle(.plain)
					.font(.system(size: 10.8, weight: .semibold, design: .monospaced))
					.padding(.horizontal, 10)
					.padding(.vertical, 7)
					.background(Color.primary.opacity(0.070), in: .rect(cornerRadius: 10))
					.overlay {
						RoundedRectangle(cornerRadius: 10, style: .continuous)
							.stroke(Color.primary.opacity(0.075), lineWidth: 1)
					}
					.frame(width: SettingsControlLayout.controlColumnWidth)
				}

				SettingsControlTile(
					symbolName: "number",
					title: "Naming",
					subtitle: "Filename style."
				) {
					OutputNamingPicker(selection: model.settings.outputNaming) { value in
						model.update { $0.outputNaming = value }
					}
				}
			}

			VStack(spacing: 0) {
				SettingsControlTile(
					symbolName: "rectangle.on.rectangle",
					title: "Frame Preset",
					subtitle: "Background style."
				) {
					CaptureFramePresetPicker(selection: captureFramePresetSelection) {
						option in
						updateCaptureFramePreset(option)
					}
				}

				SettingsControlTile(
					symbolName: "viewfinder",
					title: "Apply To",
					subtitle: "Fullscreen excluded."
				) {
					CaptureFrameApplicabilityPicker(
						selection: model.settings.captureFrameApplicability,
						isEnabled: captureFrameApplyToEnabled
					) { value in
						model.update { $0.captureFrameApplicability = value }
					}
				}
			}
		}
	}

	private var captureFramePresetSelection: CaptureFramePresetOption {
		model.settings.captureFrameEffectEnabled
			? .background(model.settings.captureFrameBackground) : .off
	}

	private var captureFrameApplyToEnabled: Bool {
		model.settings.captureFrameEffectEnabled
	}

	private func updateCaptureFramePreset(_ option: CaptureFramePresetOption) {
		model.update { settings in
			switch option {
			case .off:
				settings.captureFrameEffectEnabled = false
			case .background(let background):
				settings.captureFrameEffectEnabled = true
				settings.captureFrameBackground = background
			}
		}
	}

	private func abbreviatedPath(_ url: URL) -> String {
		let path = url.path
		let home = FileManager.default.homeDirectoryForCurrentUser.path
		if path == home {
			return "~"
		}
		if path.hasPrefix(home + "/") {
			return "~" + path.dropFirst(home.count)
		}
		return path
	}
}

private struct SettingsPanel<Content: View>: View {
	let content: Content

	init(@ViewBuilder content: () -> Content) {
		self.content = content()
	}

	var body: some View {
		VStack(spacing: 0) {
			content
		}
		.frame(maxWidth: .infinity, alignment: .topLeading)
		.clipShape(RoundedRectangle(cornerRadius: 13, style: .continuous))
		.settingsGlassSurface(cornerRadius: 13, role: .panel)
	}
}

private struct ModernSettingRow<Control: View>: View {
	let symbolName: String
	let title: String
	let subtitle: String
	let control: Control
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false

	init(
		symbolName: String,
		title: String,
		subtitle: String,
		@ViewBuilder control: () -> Control
	) {
		self.symbolName = symbolName
		self.title = title
		self.subtitle = subtitle
		self.control = control()
	}

	var body: some View {
		HStack(alignment: .center, spacing: 12) {
			ZStack {
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.fill(rowIconFill)
				Image(systemName: symbolName)
					.symbolRenderingMode(.hierarchical)
					.font(.system(size: 12.2, weight: .semibold))
					.foregroundStyle(rowIconForeground)
			}
			.frame(width: 25, height: 25)

			VStack(alignment: .leading, spacing: 3) {
				Text(title)
					.font(.system(size: 11.5, weight: .semibold))
				Text(subtitle)
					.font(.system(size: 9.5, weight: .medium))
					.foregroundStyle(.secondary)
					.lineLimit(1)
					.minimumScaleFactor(0.92)
			}
			.layoutPriority(1)

			Spacer(minLength: 12)

			control
		}
		.padding(.horizontal, 13)
		.padding(.vertical, 6)
		.frame(minHeight: 46)
		.background {
			if isHovered {
				Rectangle()
					.fill(
						colorScheme == .light
							? Color.black.opacity(0.020)
							: Color.white.opacity(0.030)
					)
			}
		}
		.overlay(alignment: .bottom) {
			Rectangle()
				.fill(
					colorScheme == .light
						? Color.black.opacity(0.035)
						: Color.white.opacity(0.052)
				)
				.frame(height: 1)
				.padding(.leading, 51)
		}
		.contentShape(Rectangle())
		.onHover { hovering in
			withAnimation(.easeOut(duration: 0.14)) {
				isHovered = hovering
			}
		}
	}

	private var rowIconFill: Color {
		colorScheme == .light ? Color.black.opacity(0.038) : Color.white.opacity(0.055)
	}

	private var rowIconForeground: Color {
		Color.secondary.opacity(colorScheme == .light ? 0.82 : 0.95)
	}
}

private struct ModernSliderRow: View {
	let symbolName: String
	let title: String
	let subtitle: String
	@Binding var value: Double
	let isEnabled: Bool

	var body: some View {
		ModernSettingRow(symbolName: symbolName, title: title, subtitle: subtitle) {
			HStack(spacing: 10) {
				GlassSlider(value: $value, isEnabled: isEnabled)
					.frame(width: SettingsControlLayout.sliderTrackWidth, height: 24)
				Text("\(Int((value * 100).rounded()))")
					.font(.system(size: 11, weight: .semibold, design: .monospaced))
					.foregroundStyle(.secondary)
					.frame(width: SettingsControlLayout.sliderValueWidth, alignment: .trailing)
			}
			.frame(width: SettingsControlLayout.controlColumnWidth, alignment: .trailing)
			.disabled(!isEnabled)
		}
	}
}

private struct GlassSlider: View {
	@Binding var value: Double
	let isEnabled: Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false

	var body: some View {
		GeometryReader { proxy in
			let width = max(proxy.size.width, 1)
			let clampedValue = value.clamped(to: 0...1)
			let progress = CGFloat(clampedValue)
			let knobSize: CGFloat = isHovered && isEnabled ? 14 : 13
			let knobOffset = min(max(0, width * progress - knobSize / 2), max(0, width - knobSize))

			ZStack(alignment: .leading) {
				Capsule()
					.fill(trackFill)
					.frame(height: 6)
				Capsule()
					.fill(fillGradient)
					.frame(width: min(width, max(8, width * progress)), height: 6)
				Circle()
					.fill(knobFill)
					.frame(width: knobSize, height: knobSize)
					.overlay {
						Circle()
							.stroke(knobBorder, lineWidth: 1)
					}
					.offset(x: knobOffset)
			}
			.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
			.contentShape(Rectangle())
			.animation(.spring(response: 0.18, dampingFraction: 0.82), value: isHovered)
			.gesture(
				DragGesture(minimumDistance: 0)
					.onChanged { gesture in
						guard isEnabled else {
							return
						}
						let nextValue = min(max(gesture.location.x / width, 0), 1)
						value = Double(nextValue)
					}
			)
		}
		.opacity(isEnabled ? 1 : 0.48)
		.animation(.easeOut(duration: 0.12), value: isEnabled)
		.onHover { hovering in
			isHovered = hovering
		}
	}

	private var trackFill: Color {
		colorScheme == .light ? Color.black.opacity(0.09) : Color.white.opacity(0.10)
	}

	private var fillGradient: LinearGradient {
		LinearGradient(
			colors: [
				Color.accentColor.opacity(colorScheme == .light ? 0.86 : 0.72),
				Color.accentColor.opacity(colorScheme == .light ? 0.86 : 0.72),
			],
			startPoint: .leading,
			endPoint: .trailing
		)
	}

	private var knobFill: LinearGradient {
		LinearGradient(
			colors: [
				Color.accentColor.opacity(colorScheme == .light ? 0.95 : 0.82),
				Color.accentColor.opacity(colorScheme == .light ? 0.95 : 0.82),
			],
			startPoint: .topLeading,
			endPoint: .bottomTrailing
		)
	}

	private var knobBorder: Color {
		Color.accentColor.opacity(colorScheme == .light ? 0.95 : 0.82)
	}
}

private struct SettingsAtmosphere: View {
	let tintHue: Double
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		ZStack {
			Rectangle()
				.fill(Color(nsColor: .windowBackgroundColor))
			Rectangle()
				.fill(.ultraThinMaterial)
			LinearGradient(
				colors: [
					tintColor.opacity(colorScheme == .light ? 0.16 : 0.22),
					Color.clear,
					Color.accentColor.opacity(colorScheme == .light ? 0.08 : 0.14),
				],
				startPoint: .topLeading,
				endPoint: .bottomTrailing
			)
			if colorScheme == .light {
				Color.white.opacity(0.20)
			} else {
				Color.black.opacity(0.14)
			}
		}
		.clipShape(windowShape)
		.overlay {
			windowShape
				.stroke(windowBorderColor, lineWidth: 1)
				.allowsHitTesting(false)
		}
		.ignoresSafeArea()
	}

	private var windowShape: RoundedRectangle {
		RoundedRectangle(
			cornerRadius: NativeHostSettingsWindowMetrics.cornerRadius,
			style: .continuous
		)
	}

	private var tintColor: Color {
		Color(hue: tintHue, saturation: 0.58, brightness: 0.94)
	}

	private var windowBorderColor: Color {
		colorScheme == .light ? Color.white.opacity(0.58) : Color.white.opacity(0.10)
	}
}

extension View {
	@ViewBuilder
	fileprivate func rsnapGlassButton(prominent: Bool) -> some View {
		self.buttonStyle(SettingsCommandButtonStyle(prominent: prominent))
	}

	fileprivate func segmentedGlassBackground() -> some View {
		self.modifier(SegmentedGlassBackgroundModifier())
	}

	fileprivate func settingsGlassSurface(cornerRadius: CGFloat, role _: SettingsGlassRole)
		-> some View
	{
		self.modifier(SettingsGlassSurfaceModifier(cornerRadius: cornerRadius))
	}
}

private struct SettingsCommandButtonStyle: ButtonStyle {
	let prominent: Bool
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.isEnabled) private var isEnabled

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.labelStyle(.titleAndIcon)
			.font(.system(size: 10.5, weight: .semibold))
			.foregroundStyle(foregroundColor)
			.symbolRenderingMode(.hierarchical)
			.padding(.horizontal, 9)
			.padding(.vertical, 4.5)
			.background(
				background(isPressed: configuration.isPressed),
				in: .rect(cornerRadius: 7, style: .continuous)
			)
			.opacity(isEnabled ? 1 : 0.55)
			.scaleEffect(configuration.isPressed ? 0.97 : 1)
			.animation(.easeOut(duration: 0.12), value: configuration.isPressed)
	}

	private var foregroundColor: Color {
		if prominent {
			return Color.accentColor
		}
		return Color.primary.opacity(colorScheme == .light ? 0.72 : 0.76)
	}

	private func background(isPressed: Bool) -> Color {
		if prominent {
			return Color.accentColor.opacity(isPressed ? 0.12 : 0.08)
		}

		return colorScheme == .light
			? Color.black.opacity(isPressed ? 0.045 : 0)
			: Color.white.opacity(isPressed ? 0.050 : 0)
	}
}

private struct SettingsToggleStyle: ToggleStyle {
	@Environment(\.colorScheme) private var colorScheme

	func makeBody(configuration: Configuration) -> some View {
		Button {
			withAnimation(.spring(response: 0.22, dampingFraction: 0.82)) {
				configuration.isOn.toggle()
			}
		} label: {
			ZStack(alignment: configuration.isOn ? .trailing : .leading) {
				Capsule()
					.fill(trackFill(isOn: configuration.isOn))
					.overlay {
						Capsule()
							.stroke(trackBorder(isOn: configuration.isOn), lineWidth: 1)
					}
				Circle()
					.fill(knobFill(isOn: configuration.isOn))
					.frame(width: 16, height: 16)
					.overlay {
						Circle()
							.stroke(knobBorder(isOn: configuration.isOn), lineWidth: 1)
					}
					.padding(2)
			}
			.frame(width: 38, height: 20)
			.animation(
				.spring(response: 0.22, dampingFraction: 0.82),
				value: configuration.isOn
			)
		}
		.buttonStyle(.plain)
	}

	private func trackFill(isOn: Bool) -> LinearGradient {
		let colors =
			isOn
			? [Color.accentColor.opacity(0.24), Color.accentColor.opacity(0.24)]
			: [
				colorScheme == .light ? Color.black.opacity(0.13) : Color.white.opacity(0.12),
				colorScheme == .light ? Color.black.opacity(0.13) : Color.white.opacity(0.12),
			]
		return LinearGradient(colors: colors, startPoint: .leading, endPoint: .trailing)
	}

	private func trackBorder(isOn: Bool) -> Color {
		isOn ? Color.accentColor.opacity(0.18) : Color.primary.opacity(0.08)
	}

	private func knobFill(isOn: Bool) -> LinearGradient {
		LinearGradient(
			colors: [
				isOn
					? Color.accentColor.opacity(colorScheme == .light ? 0.90 : 0.78)
					: (colorScheme == .light ? Color.white : Color.white.opacity(0.92)),
				isOn
					? Color.accentColor.opacity(colorScheme == .light ? 0.90 : 0.78)
					: (colorScheme == .light ? Color.white : Color.white.opacity(0.92)),
			],
			startPoint: .topLeading,
			endPoint: .bottomTrailing
		)
	}

	private func knobBorder(isOn: Bool) -> Color {
		isOn
			? Color.accentColor.opacity(0.90)
			: (colorScheme == .light ? Color.black.opacity(0.10) : Color.white.opacity(0.20))
	}
}

private struct SegmentedGlassBackgroundModifier: ViewModifier {
	func body(content: Content) -> some View {
		content
	}
}

private enum SettingsGlassRole {
	case panel
}

private struct SettingsGlassSurfaceModifier: ViewModifier {
	let cornerRadius: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	@ViewBuilder
	func body(content: Content) -> some View {
		content
			.background(.thinMaterial, in: shape)
			.background(panelFill, in: shape)
			.overlay {
				RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
					.stroke(panelBorderColor, lineWidth: 1)
					.allowsHitTesting(false)
			}
	}

	private var shape: RoundedRectangle {
		RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
	}

	private var panelFill: Color {
		colorScheme == .light
			? Color.white.opacity(0.54)
			: Color.white.opacity(0.050)
	}

	private var panelBorderColor: Color {
		colorScheme == .light ? Color.white.opacity(0.62) : Color.white.opacity(0.090)
	}
}
