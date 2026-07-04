import AppKit
import SwiftUI

struct AppearanceSettingsPanel: View {
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
