import SwiftUI

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

struct CaptureSettingsPanel: View {
	@ObservedObject var model: NativeHostSettingsViewModel
	@ObservedObject var shortcutRecorder: SettingsShortcutRecorder

	var body: some View {
		VStack(spacing: 8) {
			SettingsHeroControlTile(
				symbolName: "keyboard",
				title: "New Screenshot Shortcut",
				subtitle: "Current: \(shortcutPresentation.displayTitle)."
			) {
				captureHotKeyField
			}

			SettingsHeroControlTile(
				symbolName: "bolt.fill",
				title: "Quick Screenshot Shortcut",
				subtitle: "Current: \(quickScreenshotShortcutPresentation.displayTitle)."
			) {
				quickScreenshotHotKeyField
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

	private var captureHotKeyField: some View {
		ShortcutRecorderField(
			value: NativeHostSettings.captureHotKeyPresentation(
				for: model.settings.captureHotkey
			).displayTitle,
			isListening: shortcutRecorder.target == .capture
		) {
			shortcutRecorder.toggle(.capture) { value in
				model.update { $0.captureHotkey = value }
			}
		}
		.frame(width: SettingsControlLayout.controlColumnWidth, height: 26)
	}

	private var quickScreenshotHotKeyField: some View {
		ShortcutRecorderField(
			value: NativeHostSettings.quickScreenshotHotKeyPresentation(
				for: model.settings.quickScreenshotHotkey
			).displayTitle,
			isListening: shortcutRecorder.target == .quickScreenshot
		) {
			shortcutRecorder.toggle(.quickScreenshot) { value in
				model.update { $0.quickScreenshotHotkey = value }
			}
		}
		.frame(width: SettingsControlLayout.controlColumnWidth, height: 26)
	}
}

private struct ShortcutRecorderField: View {
	let value: String
	let isListening: Bool
	let onClick: () -> Void

	var body: some View {
		Button(action: onClick) {
			Text(isListening ? "Listening..." : value)
				.font(.system(size: 10.5, weight: .semibold, design: .monospaced))
				.foregroundStyle(isListening ? Color.accentColor : Color.primary)
				.lineLimit(1)
				.minimumScaleFactor(0.75)
				.frame(
					width: SettingsControlLayout.controlColumnWidth,
					height: 26,
					alignment: .center
				)
				.background(
					Color.primary.opacity(isListening ? 0.095 : 0.070),
					in: .rect(cornerRadius: 9)
				)
				.overlay {
					RoundedRectangle(cornerRadius: 9, style: .continuous)
						.stroke(
							isListening
								? Color.accentColor.opacity(0.45)
								: Color.primary.opacity(0.075),
							lineWidth: 1)
				}
				.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.frame(width: SettingsControlLayout.controlColumnWidth, height: 26)
		.accessibilityValue(isListening ? "Listening" : value)
	}
}
