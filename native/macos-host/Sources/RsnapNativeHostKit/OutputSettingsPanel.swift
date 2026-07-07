import SwiftUI

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
