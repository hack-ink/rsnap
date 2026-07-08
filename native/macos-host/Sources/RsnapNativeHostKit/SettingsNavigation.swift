import AppKit
import SwiftUI

enum NativeHostSettingsSection: String, CaseIterable, Identifiable {
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

struct SettingsRail: View {
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
	}
}

struct SettingsBrandIcon: View {
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

struct SettingsDashboard: View {
	@ObservedObject var model: NativeHostSettingsViewModel
	@ObservedObject var shortcutRecorder: SettingsShortcutRecorder
	let section: NativeHostSettingsSection
	let restoreDefaults: () -> Void

	var body: some View {
		VStack(alignment: .leading, spacing: SettingsControlLayout.panelContentSpacing) {
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
			}
			.scrollIndicators(.hidden)
			.frame(maxWidth: .infinity, alignment: .topLeading)
			.animation(.spring(response: 0.34, dampingFraction: 0.86), value: section)
		}
		.padding(SettingsControlLayout.margin)
		.settingsGlassSurface(cornerRadius: SettingsControlLayout.panelCornerRadius, role: .panel)
	}

	@ViewBuilder
	private var activePanel: some View {
		switch section {
		case .appearance:
			AppearanceSettingsPanel(model: model)
		case .capture:
			CaptureSettingsPanel(model: model, shortcutRecorder: shortcutRecorder)
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
	}
}
