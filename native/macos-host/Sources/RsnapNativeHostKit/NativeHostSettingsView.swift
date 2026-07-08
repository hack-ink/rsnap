import AppKit
import SwiftUI

enum NativeHostSettingsWindowMetrics {
	static let width: CGFloat = 620
	static let minHeight: CGFloat = 340
	static let idealHeight: CGFloat = 340
	static let cornerRadius: CGFloat = 18
}

@MainActor
final class NativeHostSettingsViewModel: ObservableObject {
	@Published private(set) var settings: NativeHostSettings
	@Published private(set) var launchAtLoginState = LaunchAtLoginState.current()
	@Published private(set) var softwareUpdateSettings: SoftwareUpdater.Snapshot

	private let settingsStore: NativeHostSettingsStore
	private let softwareUpdater: SoftwareUpdater

	init(
		settingsStore: NativeHostSettingsStore,
		softwareUpdater: SoftwareUpdater
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

	func setSoftwareUpdateMode(_ mode: SoftwareUpdater.Mode) {
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
	@ObservedObject var shortcutRecorder: SettingsShortcutRecorder
	@State private var selectedSection: NativeHostSettingsSection = .appearance
	private let sidebarWidth: CGFloat = 142

	var body: some View {
		ZStack {
			SettingsAtmosphere(tintHue: model.settings.hudTintHue)

			HStack(alignment: .top, spacing: SettingsControlLayout.margin) {
				SettingsRail(selectedSection: $selectedSection)
					.frame(width: sidebarWidth)
					.padding(.top, SettingsControlLayout.sidebarTitlebarOffset)

				SettingsDashboard(
					model: model,
					shortcutRecorder: shortcutRecorder,
					section: selectedSection,
					restoreDefaults: model.restoreDefaults
				)
				.frame(maxWidth: .infinity, alignment: .topLeading)
			}
			.padding(SettingsControlLayout.margin)
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
		.settingsGlassSurface(cornerRadius: SettingsControlLayout.panelCornerRadius, role: .panel)
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
