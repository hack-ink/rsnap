import AppKit
import RsnapHostBridge
import SwiftUI

@MainActor
final class NativeHostSettingsViewModel: ObservableObject {
	@Published private(set) var settings: NativeHostSettings

	private let settingsStore: NativeHostSettingsStore

	init(settingsStore: NativeHostSettingsStore) {
		self.settingsStore = settingsStore
		self.settings = settingsStore.settings
	}

	func refresh() {
		settings = settingsStore.settings
	}

	func update(_ mutate: (inout NativeHostSettings) -> Void) {
		settingsStore.update(mutate)
		settings = settingsStore.settings
	}

	func restoreDefaults() {
		update { $0 = NativeHostSettings.defaults }
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
	@Environment(\.colorScheme) private var colorScheme
	@State private var selectedSection: NativeHostSettingsSection = .appearance

	var body: some View {
		ZStack {
			SettingsAtmosphere()

			HStack(spacing: 0) {
				SettingsSidebarBackdrop()
					.frame(width: 164)
					.overlay(alignment: .trailing) {
						Rectangle()
							.fill(sidebarDividerColor)
							.frame(width: 1)
					}
				Color.clear
			}
			.ignoresSafeArea(.container, edges: .top)

			HStack(spacing: 0) {
				ZStack {
					SettingsSidebarBackdrop()
					SettingsRail(selectedSection: $selectedSection)
				}
				.frame(width: 164)
				.overlay(alignment: .trailing) {
					Rectangle()
						.fill(sidebarDividerColor)
						.frame(width: 1)
				}

				VStack(alignment: .leading, spacing: 9) {
					SettingsContentHeader(
						section: selectedSection,
						restoreDefaults: model.restoreDefaults
					)

					SettingsSectionPreview(model: model, section: selectedSection)
						.id("preview-\(selectedSection.rawValue)")
						.transition(.opacity)

					ScrollView {
						Group {
							switch selectedSection {
							case .appearance:
								AppearanceSettingsPanel(model: model)
							case .capture:
								CaptureSettingsPanel(model: model)
							case .output:
								OutputSettingsPanel(model: model)
							case .permissions:
								PermissionsSettingsPanel()
							}
						}
						.id(selectedSection)
						.transition(
							.asymmetric(
								insertion: .opacity.combined(with: .move(edge: .bottom)),
								removal: .opacity.combined(with: .move(edge: .top))
							)
						)
						.padding(.bottom, 10)
					}
					.scrollIndicators(.automatic)
				}
				.padding(.top, 9)
				.padding(.horizontal, 18)
				.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
			}
		}
		.controlSize(.small)
		.animation(.spring(response: 0.34, dampingFraction: 0.86), value: selectedSection)
		.frame(minWidth: 690, idealWidth: 690, minHeight: 420, idealHeight: 430)
	}

	private var sidebarDividerColor: Color {
		colorScheme == .light ? Color.black.opacity(0.055) : Color.white.opacity(0.07)
	}
}

private enum NativeHostSettingsSection: String, CaseIterable, Identifiable {
	case appearance
	case capture
	case output
	case permissions

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
		}
	}
}

private struct SettingsSidebarBackdrop: View {
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		ZStack {
			Rectangle()
				.fill(Color(nsColor: .controlBackgroundColor))
			if colorScheme == .light {
				Color.white.opacity(0.18)
			} else {
				Color.black.opacity(0.08)
			}
		}
	}
}

private struct SettingsRail: View {
	@Binding var selectedSection: NativeHostSettingsSection

	var body: some View {
		VStack(alignment: .leading, spacing: 11) {
			VStack(alignment: .leading, spacing: 3) {
				Text("rsnap")
					.font(.system(size: 18, weight: .semibold, design: .rounded))
				Text("Settings")
					.font(.system(size: 10, weight: .medium))
					.foregroundStyle(.secondary)
			}
			.padding(.top, 17)

			VStack(spacing: 4) {
				ForEach(NativeHostSettingsSection.allCases) { section in
					SettingsRailButton(
						section: section,
						isSelected: selectedSection == section
					) {
						withAnimation(.easeOut(duration: 0.16)) {
							selectedSection = section
						}
					}
				}
			}

			Spacer(minLength: 12)
		}
		.padding(.horizontal, 10)
		.padding(.bottom, 12)
		.frame(maxHeight: .infinity, alignment: .topLeading)
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
					.font(.system(size: 12, weight: .semibold))
					.foregroundStyle(isSelected ? Color.accentColor : Color.secondary)
					.frame(width: 17, height: 17)
				VStack(alignment: .leading, spacing: 2) {
					Text(section.title)
						.font(.system(size: 11.6, weight: .semibold))
						.foregroundStyle(isSelected ? Color.primary : Color.primary.opacity(0.88))
					Text(section.subtitle)
						.font(.system(size: 9.2, weight: .medium))
						.foregroundStyle(.secondary)
				}
				Spacer(minLength: 0)
			}
			.padding(.horizontal, 9)
			.padding(.vertical, 5)
			.frame(maxWidth: .infinity)
			.contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
			.background {
				if isSelected {
					RoundedRectangle(cornerRadius: 9, style: .continuous)
						.fill(
							colorScheme == .light
								? Color.black.opacity(0.055)
								: Color.white.opacity(0.070)
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
								? Color.black.opacity(0.020)
								: Color.white.opacity(0.030)
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

private struct SettingsContentHeader: View {
	let section: NativeHostSettingsSection
	let restoreDefaults: () -> Void

	var body: some View {
		HStack(alignment: .center, spacing: 18) {
			VStack(alignment: .leading, spacing: 5) {
				Text(section.title)
					.font(.system(size: 20, weight: .semibold))
				Text(section.subtitle)
					.font(.system(size: 10.5, weight: .medium))
					.foregroundStyle(.secondary)
			}
			.frame(maxWidth: .infinity, alignment: .leading)

			if section != .permissions {
				Button("Restore Defaults", action: restoreDefaults)
					.rsnapGlassButton(prominent: false)
					.controlSize(.small)
			}
		}
		.frame(height: 36)
	}
}

private struct SettingsSectionPreview: View {
	@ObservedObject var model: NativeHostSettingsViewModel
	let section: NativeHostSettingsSection
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		ZStack {
			RoundedRectangle(cornerRadius: 16, style: .continuous)
				.fill(Color.clear)
			RoundedRectangle(cornerRadius: 16, style: .continuous)
				.fill(previewOverlay)
			LinearGradient(
				colors: [
					tintColor.opacity(colorScheme == .light ? 0.045 : 0.060),
					Color.clear,
				],
				startPoint: .topLeading,
				endPoint: .bottomTrailing
			)

			HStack(spacing: 12) {
				previewContent
			}
			.padding(.horizontal, 16)
			.padding(.vertical, 9)
		}
		.frame(height: 58)
		.settingsGlassSurface(cornerRadius: 13, role: .preview)
	}

	private var previewOverlay: Color {
		colorScheme == .light ? Color.white.opacity(0.30) : Color.black.opacity(0.05)
	}

	@ViewBuilder
	private var previewContent: some View {
		switch section {
		case .appearance:
			RoundedRectangle(cornerRadius: 11, style: .continuous)
				.fill(tintColor.gradient)
				.frame(width: 30, height: 30)
				.overlay {
					RoundedRectangle(cornerRadius: 11, style: .continuous)
						.stroke(Color.white.opacity(0.55), lineWidth: 1)
				}
			VStack(alignment: .leading, spacing: 2) {
				Text("Live HUD")
					.font(.system(size: 12.5, weight: .semibold))
				Text("\(tintHex) · \(Int((model.settings.hudTint * 100).rounded()))% tint")
					.font(.system(size: 9.5, weight: .medium, design: .monospaced))
					.foregroundStyle(.secondary)
			}
			Spacer(minLength: 8)
			SettingsPreviewPill(model.settings.resolvedHudGlassMode.title)
			SettingsPreviewPill(model.settings.liquidGlassStyle.title)

		case .capture:
			Image(systemName: "viewfinder")
				.font(.system(size: 21, weight: .semibold))
				.foregroundStyle(Color.accentColor)
				.frame(width: 30, height: 30)
			VStack(alignment: .leading, spacing: 2) {
				Text(shortcutTitle)
					.font(.system(size: 12.5, weight: .semibold, design: .rounded))
				Text(
					"\(model.settings.toolbarPlacement.title) toolbar · \(model.settings.loupeSampleSize.title)"
				)
				.font(.system(size: 9.5, weight: .medium))
				.foregroundStyle(.secondary)
				.lineLimit(1)
			}
			Spacer(minLength: 8)
			SettingsPreviewPill("Right-click exits")

		case .output:
			Image(systemName: "folder")
				.font(.system(size: 21, weight: .semibold))
				.foregroundStyle(Color.accentColor)
				.frame(width: 30, height: 30)
			VStack(alignment: .leading, spacing: 2) {
				Text(abbreviatedPath(model.settings.outputDirectory))
					.font(.system(size: 12, weight: .semibold))
					.lineLimit(1)
				Text(
					"\(model.settings.outputFilenamePrefix) · \(model.settings.outputNaming.title)"
				)
				.font(.system(size: 9.5, weight: .medium))
				.foregroundStyle(.secondary)
				.lineLimit(1)
			}
			Spacer(minLength: 8)
			SettingsPreviewPill("Ready")

		case .permissions:
			Image(systemName: "lock.shield")
				.font(.system(size: 21, weight: .semibold))
				.foregroundStyle(Color.accentColor)
				.frame(width: 30, height: 30)
			VStack(alignment: .leading, spacing: 2) {
				Text("Native access")
					.font(.system(size: 12, weight: .semibold))
				Text("Required permissions for capture.")
					.font(.system(size: 9.5, weight: .medium))
					.foregroundStyle(.secondary)
			}
			Spacer(minLength: 8)
			SettingsPreviewPill(permissionSummary)
		}
	}

	private var tintColor: Color {
		Color(hue: model.settings.hudTintHue, saturation: 0.72, brightness: 0.95)
	}

	private var tintHex: String {
		let color = NSColor(
			hue: CGFloat(model.settings.hudTintHue),
			saturation: 0.72,
			brightness: 0.95,
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

	private var shortcutTitle: String {
		NativeHostSettings.captureHotKeyPresentation(for: model.settings.captureHotkey)
			.displayTitle
	}

	private var permissionSummary: String {
		let required = [
			PermissionKind.screenRecording,
			.accessibility,
			.inputMonitoring,
		].filter { NativePermissions.requiredForCurrentNativeHost($0) }
		let granted = required.filter { NativePermissions.status(for: $0) }.count
		return "\(granted)/\(required.count) granted"
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

private struct SettingsPreviewPill: View {
	let title: String
	@Environment(\.colorScheme) private var colorScheme

	init(_ title: String) {
		self.title = title
	}

	var body: some View {
		Text(title)
			.font(.system(size: 9.5, weight: .semibold))
			.lineLimit(1)
			.padding(.horizontal, 7)
			.padding(.vertical, 4)
			.background(
				colorScheme == .light ? Color.white.opacity(0.72) : Color.white.opacity(0.10),
				in: Capsule()
			)
			.overlay {
				Capsule()
					.stroke(
						colorScheme == .light
							? Color.black.opacity(0.06)
							: Color.white.opacity(0.10),
						lineWidth: 1
					)
			}
	}
}

private struct HudGlassModePicker: View {
	let selection: HudGlassModePreference
	let isEnabled: Bool
	let onSelect: (HudGlassModePreference) -> Void

	var body: some View {
		HStack(spacing: 4) {
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
				.help(available ? mode.title : "Requires Liquid Glass support.")
			}
		}
		.padding(2)
		.frame(width: 164)
		.segmentedGlassBackground()
	}
}

private struct LiquidGlassStylePicker: View {
	let selection: LiquidGlassStylePreference
	let isEnabled: Bool
	let onSelect: (LiquidGlassStylePreference) -> Void

	var body: some View {
		HStack(spacing: 4) {
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
		.padding(2)
		.frame(width: 124)
		.segmentedGlassBackground()
	}
}

private struct ToolbarPlacementPicker: View {
	let selection: ToolbarPlacementPreference
	let onSelect: (ToolbarPlacementPreference) -> Void

	var body: some View {
		HStack(spacing: 4) {
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
		.padding(2)
		.frame(width: 124)
		.segmentedGlassBackground()
	}
}

private struct FrozenResizeHandleOrientationPicker: View {
	let selection: FrozenResizeHandleOrientationPreference
	let onSelect: (FrozenResizeHandleOrientationPreference) -> Void

	var body: some View {
		HStack(spacing: 4) {
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
		.padding(2)
		.frame(width: 186)
		.segmentedGlassBackground()
	}
}

private struct LoupeSampleSizePicker: View {
	let selection: LoupeSampleSizePreference
	let onSelect: (LoupeSampleSizePreference) -> Void

	var body: some View {
		HStack(spacing: 4) {
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
		.padding(2)
		.frame(width: 160)
		.segmentedGlassBackground()
	}
}

private struct OutputNamingPicker: View {
	let selection: OutputNamingPreference
	let onSelect: (OutputNamingPreference) -> Void

	var body: some View {
		HStack(spacing: 4) {
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
		.padding(2)
		.frame(width: 140)
		.segmentedGlassBackground()
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
		Button(action: action) {
			ZStack {
				Color.clear

				if isHovered && !isSelected && isEnabled {
					RoundedRectangle(cornerRadius: 6, style: .continuous)
						.fill(
							colorScheme == .light
								? Color.black.opacity(0.035)
								: Color.white.opacity(0.045)
						)
				}

				if isSelected {
					RoundedRectangle(cornerRadius: 6, style: .continuous)
						.fill(
							colorScheme == .light
								? Color(nsColor: .controlBackgroundColor)
								: Color.white.opacity(0.105)
						)
						.overlay {
							RoundedRectangle(cornerRadius: 6, style: .continuous)
								.stroke(
									colorScheme == .light
										? Color.black.opacity(0.055)
										: Color.white.opacity(0.082),
									lineWidth: 1
								)
						}
				}

				Text(title)
					.font(.system(size: 9.3, weight: .semibold))
					.lineLimit(1)
					.minimumScaleFactor(0.9)
					.foregroundStyle(isEnabled ? Color.primary : Color.secondary.opacity(0.54))
					.padding(.horizontal, 7)
			}
			.frame(maxWidth: .infinity, minHeight: 20)
			.contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
		}
		.buttonStyle(.plain)
		.disabled(!isEnabled)
		.frame(maxWidth: .infinity)
		.animation(.spring(response: 0.22, dampingFraction: 0.78), value: isSelected)
		.animation(.easeOut(duration: 0.12), value: isHovered)
		.onHover { hovering in
			isHovered = hovering
		}
	}
}

private struct AppearanceSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		SettingsPanel {
			ModernSettingRow(
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
				.toggleStyle(.switch)
			}

			ModernSettingRow(
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
				ModernSettingRow(
					symbolName: "circle.hexagongrid",
					title: "Liquid Glass style",
					subtitle: "Clear or Regular."
				) {
					LiquidGlassStylePicker(
						selection: model.settings.liquidGlassStyle,
						isEnabled: model.settings.hudGlassEnabled
					) { value in
						model.update { $0.liquidGlassStyle = value }
					}
					.disabled(!model.settings.hudGlassEnabled)
				}
			}

			if model.settings.resolvedHudGlassMode == .classicGlass {
				ModernSliderRow(
					symbolName: "circle.lefthalf.filled",
					title: "Opacity",
					subtitle: "Background weight.",
					value: Binding(
						get: { model.settings.hudOpacity },
						set: { value in model.update { $0.hudOpacity = value } }
					),
					isEnabled: model.settings.hudGlassEnabled
				)

				ModernSliderRow(
					symbolName: "camera.filters",
					title: "Blur",
					subtitle: "Background separation.",
					value: Binding(
						get: { model.settings.hudBlur },
						set: { value in model.update { $0.hudBlur = value } }
					),
					isEnabled: model.settings.hudGlassEnabled
				)
			}

			ModernSliderRow(
				symbolName: "eyedropper.halffull",
				title: "Tint strength",
				subtitle: "HUD accent weight.",
				value: Binding(
					get: { model.settings.hudTint },
					set: { value in model.update { $0.hudTint = value } }
				),
				isEnabled: model.settings.hudGlassEnabled
			)

			ModernSettingRow(
				symbolName: "paintpalette",
				title: "Tint color",
				subtitle: "HUD accent."
			) {
				ColorPicker(
					"",
					selection: tintColorBinding,
					supportsOpacity: false
				)
				.labelsHidden()
				.frame(width: 52)
				.disabled(!model.settings.hudGlassEnabled)
			}
		}
	}

	private var materialSubtitle: String {
		LiveChromeGlassMaterialSupport.isLiquidGlassAvailable
			? "Liquid Glass or blur."
			: "Classic Glass fallback."
	}

	private var tintColorBinding: Binding<Color> {
		Binding(
			get: {
				Color(
					hue: model.settings.hudTintHue,
					saturation: 0.72,
					brightness: 0.95
				)
			},
			set: { color in
				let nsColor = NSColor(color)
				let converted = nsColor.usingColorSpace(.deviceRGB) ?? nsColor
				model.update { $0.hudTintHue = Double(converted.hueComponent) }
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

private struct CaptureSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		SettingsPanel {
			ModernSettingRow(
				symbolName: "keyboard",
				title: "New capture shortcut",
				subtitle: "Current: \(shortcutPresentation.displayTitle)."
			) {
				CaptureHotKeyField(model: model)
			}

			ModernSettingRow(
				symbolName: "rectangle.bottomthird.inset.filled",
				title: "Frozen toolbar",
				subtitle: "Command bar position."
			) {
				ToolbarPlacementPicker(selection: model.settings.toolbarPlacement) { value in
					model.update { $0.toolbarPlacement = value }
				}
			}

			ModernSettingRow(
				symbolName: "crop",
				title: "Corner handles",
				subtitle: "Resize bracket direction."
			) {
				FrozenResizeHandleOrientationPicker(
					selection: model.settings.frozenResizeHandleOrientation
				) { value in
					model.update { $0.frozenResizeHandleOrientation = value }
				}
			}

			ModernSettingRow(
				symbolName: "plus.magnifyingglass",
				title: "Loupe sample",
				subtitle: "Color patch size."
			) {
				LoupeSampleSizePicker(selection: model.settings.loupeSampleSize) { value in
					model.update { $0.loupeSampleSize = value }
				}
			}

			ModernSettingRow(
				symbolName: "lightbulb",
				title: "HUD hint",
				subtitle: "Show Tab keycap."
			) {
				Toggle(
					"",
					isOn: Binding(
						get: { model.settings.showAltHintKeycap },
						set: { value in model.update { $0.showAltHintKeycap = value } }
					)
				)
				.labelsHidden()
				.toggleStyle(.switch)
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
			.textFieldStyle(.roundedBorder)
			.frame(width: 116)
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
	@State private var refreshID = 0
	private let primaryKind = PermissionKind.screenRecording

	var body: some View {
		VStack(spacing: 10) {
			PermissionGrantCard(
				kind: primaryKind,
				refreshID: refreshID,
				bundleURL: Self.appBundleURL,
				appIcon: Self.appIcon,
				openSettings: {
					NativePermissions.openSystemSettings(for: primaryKind)
				},
				refresh: {
					refreshID += 1
				}
			)

			SettingsPanel {
				ForEach(Self.rows) { row in
					PermissionStatusRow(
						row: row,
						refreshID: refreshID,
						openSettings: { kind in
							NativePermissions.openSystemSettings(for: kind)
							refreshID += 1
						}
					)
				}
			}
		}
	}

	private static var appBundleURL: URL {
		Bundle.main.bundleURL
	}

	private static var appIcon: NSImage {
		NSWorkspace.shared.icon(forFile: appBundleURL.path)
	}

	private static let rows: [PermissionSettingsRow] = [
		PermissionSettingsRow(
			kind: .screenRecording,
			title: "Screen Recording",
			symbolName: "rectangle.on.rectangle"
		),
		PermissionSettingsRow(
			kind: .accessibility,
			title: "Accessibility",
			symbolName: "accessibility"
		),
		PermissionSettingsRow(
			kind: .inputMonitoring,
			title: "Input Monitoring",
			symbolName: "keyboard"
		),
	]
}

private struct PermissionSettingsRow: Identifiable {
	let kind: PermissionKind
	let title: String
	let symbolName: String

	var id: PermissionKind { kind }
}

private struct PermissionGrantCard: View {
	let kind: PermissionKind
	let refreshID: Int
	let bundleURL: URL
	let appIcon: NSImage
	let openSettings: () -> Void
	let refresh: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .center, spacing: 12) {
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
				HStack(spacing: 7) {
					Text(title)
						.font(.system(size: 12.5, weight: .semibold))
						.lineLimit(2)
					PermissionStateBadge(
						title: isGranted ? "Granted" : "Required",
						style: isGranted ? .granted : .required
					)
				}
				Text(subtitle)
					.font(.system(size: 10, weight: .medium))
					.foregroundStyle(.secondary)
					.lineLimit(2)
					.fixedSize(horizontal: false, vertical: true)
			}
			.layoutPriority(1)

			Spacer(minLength: 8)

			VStack(alignment: .trailing, spacing: 7) {
				PermissionAppDragSource(bundleURL: bundleURL, icon: appIcon, label: "rsnap")
					.frame(width: 112, height: 34)
					.opacity(isGranted ? 0.76 : 1)

				HStack(spacing: 6) {
					Button {
						openSettings()
					} label: {
						Label("Open", systemImage: "gearshape")
					}
					.rsnapGlassButton(prominent: false)
					.controlSize(.small)

					Button(action: refresh) {
						Image(systemName: "arrow.clockwise")
							.frame(width: 13, height: 13)
					}
					.rsnapGlassButton(prominent: false)
					.controlSize(.small)
					.help("Refresh status")
				}
			}
		}
		.padding(.horizontal, 14)
		.padding(.vertical, 12)
		.frame(maxWidth: .infinity, minHeight: 104, alignment: .leading)
		.settingsGlassSurface(cornerRadius: 13, role: .panel)
	}

	private var isGranted: Bool {
		_ = refreshID
		return NativePermissions.status(for: kind)
	}

	private var title: String {
		isGranted ? "Screen Recording ready" : "Drag rsnap into Screen Recording"
	}

	private var subtitle: String {
		if isGranted {
			return "The native capture host can see the screen."
		}
		return "Open System Settings, then drop the app chip into the allowed apps list."
	}

	private var iconBackgroundColor: Color {
		if isGranted {
			return Color.green.opacity(colorScheme == .light ? 0.12 : 0.18)
		}
		return Color.accentColor.opacity(colorScheme == .light ? 0.12 : 0.20)
	}

}

private struct PermissionStatusRow: View {
	let row: PermissionSettingsRow
	let refreshID: Int
	let openSettings: (PermissionKind) -> Void

	var body: some View {
		ModernSettingRow(
			symbolName: row.symbolName,
			title: row.title,
			subtitle: subtitle
		) {
			HStack(spacing: 7) {
				PermissionStateBadge(title: badgeTitle, style: badgeStyle)
				if canOpen {
					Button {
						openSettings(row.kind)
					} label: {
						Image(systemName: "arrow.up.forward.app")
							.frame(width: 13, height: 13)
					}
					.rsnapGlassButton(prominent: false)
					.controlSize(.small)
					.help("Open \(row.title)")
				}
			}
		}
	}

	private var isGranted: Bool {
		_ = refreshID
		return NativePermissions.status(for: row.kind)
	}

	private var isRequired: Bool {
		NativePermissions.requiredForCurrentNativeHost(row.kind)
	}

	private var subtitle: String {
		if isGranted {
			return "Granted."
		}
		return isRequired ? "Required for native capture." : "Not used by current host."
	}

	private var badgeTitle: String {
		if isGranted {
			return "Granted"
		}
		return isRequired ? "Required" : "Not Used"
	}

	private var badgeStyle: PermissionStateBadge.Style {
		if isGranted {
			return .granted
		}
		return isRequired ? .required : .muted
	}

	private var canOpen: Bool {
		isRequired && !isGranted
	}
}

private struct PermissionStateBadge: View {
	enum Style {
		case granted
		case required
		case muted
	}

	let title: String
	let style: Style
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Text(title)
			.font(.system(size: 9.2, weight: .semibold))
			.lineLimit(1)
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
		case .muted:
			return Color.secondary
		}
	}

	private var backgroundColor: Color {
		switch style {
		case .granted:
			return Color.green.opacity(colorScheme == .light ? 0.10 : 0.16)
		case .required:
			return Color.accentColor.opacity(colorScheme == .light ? 0.10 : 0.18)
		case .muted:
			return Color.secondary.opacity(colorScheme == .light ? 0.08 : 0.12)
		}
	}

	private var borderColor: Color {
		switch style {
		case .granted:
			return Color.green.opacity(colorScheme == .light ? 0.20 : 0.26)
		case .required:
			return Color.accentColor.opacity(colorScheme == .light ? 0.20 : 0.28)
		case .muted:
			return Color.secondary.opacity(colorScheme == .light ? 0.14 : 0.20)
		}
	}
}

private struct OutputSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel

	var body: some View {
		SettingsPanel {
			ModernSettingRow(
				symbolName: "folder",
				title: "Save location",
				subtitle: abbreviatedPath(model.settings.outputDirectory)
			) {
				Button("Choose", action: model.chooseOutputDirectory)
					.rsnapGlassButton(prominent: false)
					.controlSize(.small)
			}

			ModernSettingRow(
				symbolName: "textformat.abc",
				title: "Filename prefix",
				subtitle: "Safe filename text."
			) {
				TextField(
					"rsnap",
					text: Binding(
						get: { model.settings.outputFilenamePrefix },
						set: { value in model.update { $0.outputFilenamePrefix = value } }
					)
				)
				.textFieldStyle(.roundedBorder)
				.frame(width: 172)
			}

			ModernSettingRow(
				symbolName: "number",
				title: "Naming",
				subtitle: "Filename style."
			) {
				OutputNamingPicker(selection: model.settings.outputNaming) { value in
					model.update { $0.outputNaming = value }
				}
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
		.settingsGlassSurface(cornerRadius: 10, role: .panel)
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
			Image(systemName: symbolName)
				.symbolRenderingMode(.hierarchical)
				.font(.system(size: 12.5, weight: .semibold))
				.foregroundStyle(Color.secondary.opacity(colorScheme == .light ? 0.78 : 0.90))
				.frame(width: 20, height: 20)

			VStack(alignment: .leading, spacing: 3) {
				Text(title)
					.font(.system(size: 11, weight: .semibold))
				Text(subtitle)
					.font(.system(size: 9.2, weight: .medium))
					.foregroundStyle(.secondary)
					.lineLimit(1)
					.minimumScaleFactor(0.92)
			}
			.layoutPriority(1)

			Spacer(minLength: 12)

			control
		}
		.padding(.horizontal, 12)
		.padding(.vertical, 5)
		.frame(minHeight: 42)
		.background {
			if isHovered {
				Rectangle()
					.fill(
						colorScheme == .light
							? Color.black.opacity(0.018)
							: Color.white.opacity(0.026)
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
				.padding(.leading, 44)
		}
		.contentShape(Rectangle())
		.onHover { hovering in
			withAnimation(.easeOut(duration: 0.14)) {
				isHovered = hovering
			}
		}
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
			HStack(spacing: 12) {
				Slider(value: $value, in: 0...1)
					.frame(width: 116)
				Text("\(Int((value * 100).rounded()))")
					.font(.system(size: 11, weight: .semibold, design: .monospaced))
					.foregroundStyle(.secondary)
					.frame(width: 30, alignment: .trailing)
			}
			.disabled(!isEnabled)
		}
	}
}

private struct SettingsAtmosphere: View {
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		ZStack {
			Rectangle()
				.fill(Color(nsColor: .windowBackgroundColor))
			if colorScheme == .light {
				Color.white.opacity(0.16)
			} else {
				Color.black.opacity(0.08)
			}
		}
		.ignoresSafeArea()
	}
}

extension View {
	@ViewBuilder
	fileprivate func rsnapGlassButton(prominent: Bool) -> some View {
		if prominent {
			self.buttonStyle(.borderedProminent)
		} else {
			self.buttonStyle(.bordered)
		}
	}

	fileprivate func segmentedGlassBackground() -> some View {
		self.modifier(SegmentedGlassBackgroundModifier())
	}

	fileprivate func settingsGlassSurface(cornerRadius: CGFloat, role: SettingsGlassRole)
		-> some View
	{
		self.modifier(SettingsGlassSurfaceModifier(cornerRadius: cornerRadius, role: role))
	}
}

private struct SegmentedGlassBackgroundModifier: ViewModifier {
	@Environment(\.colorScheme) private var colorScheme

	func body(content: Content) -> some View {
		content
			.background(
				colorScheme == .light ? Color.black.opacity(0.040) : Color.white.opacity(0.052),
				in: .rect(cornerRadius: 8, style: .continuous)
			)
			.overlay {
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.stroke(
						colorScheme == .light
							? Color.black.opacity(0.048)
							: Color.white.opacity(0.074),
						lineWidth: 1
					)
			}
	}
}

private enum SettingsGlassRole {
	case panel
	case preview
}

private struct SettingsGlassSurfaceModifier: ViewModifier {
	let cornerRadius: CGFloat
	let role: SettingsGlassRole
	@Environment(\.colorScheme) private var colorScheme

	@ViewBuilder
	func body(content: Content) -> some View {
		switch role {
		case .panel:
			content
				.background(panelFill, in: shape)
				.overlay {
					RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
						.stroke(panelBorderColor, lineWidth: 1)
						.allowsHitTesting(false)
				}
		case .preview:
			content
				.background(.regularMaterial, in: shape)
				.background(baseTint, in: shape)
				.overlay(alignment: .top) {
					LinearGradient(
						colors: [
							Color.white.opacity(colorScheme == .light ? 0.54 : 0.12),
							Color.white.opacity(0),
						],
						startPoint: .top,
						endPoint: .bottom
					)
					.frame(height: 34)
					.clipShape(shape)
					.allowsHitTesting(false)
				}
				.overlay {
					RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
						.stroke(borderGradient, lineWidth: 1)
						.allowsHitTesting(false)
				}
				.shadow(color: shadowColor, radius: 7, y: 2)
		}
	}

	private var shape: RoundedRectangle {
		RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
	}

	private var baseTint: Color {
		switch role {
		case .panel:
			return panelFill
		case .preview:
			return colorScheme == .light ? Color.white.opacity(0.28) : Color.black.opacity(0.035)
		}
	}

	private var panelFill: Color {
		colorScheme == .light
			? Color(nsColor: .controlBackgroundColor)
			: Color.white.opacity(0.032)
	}

	private var panelBorderColor: Color {
		colorScheme == .light ? Color.black.opacity(0.062) : Color.white.opacity(0.058)
	}

	private var borderGradient: LinearGradient {
		LinearGradient(
			colors: [
				colorScheme == .light ? Color.white.opacity(0.72) : Color.white.opacity(0.12),
				colorScheme == .light ? Color.black.opacity(0.050) : Color.white.opacity(0.065),
			],
			startPoint: .topLeading,
			endPoint: .bottomTrailing
		)
	}

	private var shadowColor: Color {
		colorScheme == .light ? Color.black.opacity(0.045) : Color.black.opacity(0.14)
	}
}
