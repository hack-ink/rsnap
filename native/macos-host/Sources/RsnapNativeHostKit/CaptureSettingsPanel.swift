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
