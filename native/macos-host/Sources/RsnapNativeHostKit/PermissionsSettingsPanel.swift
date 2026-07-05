import AppKit
import SwiftUI

struct PermissionsSettingsPanel: View {
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
					},
					isGrantedProvider: {
						NativePermissions.screenRecordingGranted
					},
					titleWhenGranted: "Screen Recording ready",
					titleWhenMissing: "Screen Recording access needed",
					subtitleWhenGranted: "The native capture host can see the screen.",
					subtitleWhenMissing:
						"Open System Settings, then drag Rsnap.app into the Screen Recording app list.",
					missingBadgeTitle: "Required",
					openSettingsHelp: "Open Screen Recording settings"
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

private struct PermissionGrantCard: View {
	let refreshID: Int
	let bundleURL: URL
	let appIcon: NSImage
	let openSettings: () -> Void
	let refresh: () -> Void
	let isGrantedProvider: () -> Bool
	let titleWhenGranted: String
	let titleWhenMissing: String
	let subtitleWhenGranted: String
	let subtitleWhenMissing: String
	let missingBadgeTitle: String
	let openSettingsHelp: String
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
							title: isGranted ? "Granted" : missingBadgeTitle,
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
				.help(openSettingsHelp)

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
		return isGrantedProvider()
	}

	private var title: String {
		isGranted ? titleWhenGranted : titleWhenMissing
	}

	private var subtitle: String {
		if isGranted {
			return subtitleWhenGranted
		}
		return subtitleWhenMissing
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
