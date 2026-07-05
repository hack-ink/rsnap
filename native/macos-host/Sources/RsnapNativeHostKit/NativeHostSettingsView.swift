import AppKit
import SwiftUI

enum NativeHostSettingsWindowMetrics {
	static let width: CGFloat = 620
	static let minHeight: CGFloat = 332
	static let idealHeight: CGFloat = 332
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

private enum NativeHostAboutLinks {
	static let source = "https://github.com/hack-ink/rsnap"
	static let creator = "https://x.com/hackink"
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
				title: "Quick",
				value: NativeHostSettings.quickScreenshotHotKeyPresentation(
					for: settings.quickScreenshotHotkey
				).displayTitle,
				symbolName: "bolt.fill"
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
		let requiredGranted = NativePermissions.screenRecordingGranted ? 1 : 0

		VStack(alignment: .leading, spacing: 12) {
			PermissionProgressBadge(granted: requiredGranted, total: 1)
			InspectorMetric(
				title: "Required",
				value: "1",
				symbolName: "lock.shield"
			)
			InspectorMetric(
				title: "Granted",
				value: "\(requiredGranted)",
				symbolName: "checkmark.seal"
			)
			InspectorMetric(
				title: "Scroll Capture",
				value: NativePermissions.screenRecordingGranted ? "Ready" : "Waiting",
				symbolName: "arrow.down.to.line.compact"
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
		if snapshot.isConfigured == false {
			return "Sparkle appcast not configured."
		}
		if mode == .install, isEnabled == false {
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

struct CaptureSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		VStack(spacing: 8) {
			SettingsHeroControlTile(
				symbolName: "keyboard",
				title: "New Screenshot Shortcut",
				subtitle: "Current: \(shortcutPresentation.displayTitle)."
			) {
				CaptureHotKeyField(model: model)
			}

			SettingsHeroControlTile(
				symbolName: "bolt.fill",
				title: "Quick Screenshot Shortcut",
				subtitle: "Current: \(quickScreenshotShortcutPresentation.displayTitle)."
			) {
				QuickScreenshotHotKeyField(model: model)
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

	private var quickScreenshotShortcutPresentation: CaptureHotKeyPresentation {
		NativeHostSettings.quickScreenshotHotKeyPresentation(
			for: model.settings.quickScreenshotHotkey)
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
				if isFocused == false {
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

private struct QuickScreenshotHotKeyField: View {
	@ObservedObject var model: NativeHostSettingsViewModel
	@FocusState private var isFocused: Bool
	@State private var draft = ""

	var body: some View {
		TextField("Option-Shift-X", text: $draft)
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
			.onChange(of: model.settings.quickScreenshotHotkey) { _, _ in
				if isFocused == false {
					syncDraft()
				}
			}
	}

	private func syncDraft() {
		draft =
			NativeHostSettings.quickScreenshotHotKeyPresentation(
				for: model.settings.quickScreenshotHotkey
			).displayTitle
	}

	private func commitDraft() {
		let committed = NativeHostSettings.quickScreenshotHotKeyPresentation(for: draft)
			.displayTitle
		if committed != model.settings.quickScreenshotHotkey {
			model.update { $0.quickScreenshotHotkey = committed }
		}
		draft = committed
	}
}

struct AboutSettingsPanel: View {
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
				!SoftwareUpdateManualCheckAvailability.isEnabled(
					sparkleCanCheckForUpdates: model.softwareUpdateSettings.canCheckForUpdates))
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
					"Rsnap is an open-source macOS capture tool. Follow @hackink on X for progress, design notes, and release updates; attention there helps support future work."
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
		return subtitle.isEmpty == false
	}
}

struct OutputSettingsPanel: View {
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
					CaptureFramePresetSelector(selection: captureFramePresetSelection) {
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
