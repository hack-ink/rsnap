import AppKit
import SwiftUI

private enum NativeHostAboutLinks {
	static let source = "https://github.com/acg-box/rsnap"
	static let creator = "https://x.com/hackink"
}

private struct SoftwareUpdateModePicker: View {
	let snapshot: SoftwareUpdater.Snapshot
	let onSelect: (SoftwareUpdater.Mode) -> Void

	var body: some View {
		HStack(spacing: 8) {
			ForEach(SoftwareUpdater.Mode.allCases, id: \.rawValue) { mode in
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

	private func isEnabled(_ mode: SoftwareUpdater.Mode) -> Bool {
		guard snapshot.isConfigured else {
			return false
		}
		if mode == .install {
			return snapshot.allowsAutomaticUpdates
		}
		return true
	}

	private func helpText(
		for mode: SoftwareUpdater.Mode,
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
