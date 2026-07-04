import SwiftUI

enum SettingsControlLayout {
	static let controlColumnWidth: CGFloat = 178
	static let sliderValueWidth: CGFloat = 34
	static let sliderTrackWidth: CGFloat = 136
	static let compactSliderLabelWidth: CGFloat = 44
	static let compactSliderTrackWidth: CGFloat = 88
	static let framePresetSelectorHeight: CGFloat = 30
	static let framePresetSwatchWidth: CGFloat = 42
	static let framePresetSwatchHeight: CGFloat = 24
	static let framePresetSwatchSpacing: CGFloat = 6
}

struct SettingsPanel<Content: View>: View {
	let content: Content

	init(@ViewBuilder content: () -> Content) {
		self.content = content()
	}

	var body: some View {
		VStack(spacing: 0) {
			content
		}
		.frame(maxWidth: .infinity, alignment: .topLeading)
		.clipShape(RoundedRectangle(cornerRadius: 13, style: .continuous))
		.settingsGlassSurface(cornerRadius: 13, role: .panel)
	}
}

struct ModernSettingRow<Control: View>: View {
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
			ZStack {
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.fill(rowIconFill)
				Image(systemName: symbolName)
					.symbolRenderingMode(.hierarchical)
					.font(.system(size: 12.2, weight: .semibold))
					.foregroundStyle(rowIconForeground)
			}
			.frame(width: 25, height: 25)

			VStack(alignment: .leading, spacing: 3) {
				Text(title)
					.font(.system(size: 11.5, weight: .semibold))
				Text(subtitle)
					.font(.system(size: 9.5, weight: .medium))
					.foregroundStyle(.secondary)
					.lineLimit(1)
					.minimumScaleFactor(0.92)
			}
			.layoutPriority(1)

			Spacer(minLength: 12)

			control
		}
		.padding(.horizontal, 13)
		.padding(.vertical, 6)
		.frame(minHeight: 46)
		.background {
			if isHovered {
				Rectangle()
					.fill(
						colorScheme == .light
							? Color.black.opacity(0.020)
							: Color.white.opacity(0.030)
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
				.padding(.leading, 51)
		}
		.contentShape(Rectangle())
		.onHover { hovering in
			withAnimation(.easeOut(duration: 0.14)) {
				isHovered = hovering
			}
		}
	}

	private var rowIconFill: Color {
		colorScheme == .light ? Color.black.opacity(0.038) : Color.white.opacity(0.055)
	}

	private var rowIconForeground: Color {
		Color.secondary.opacity(colorScheme == .light ? 0.82 : 0.95)
	}
}

struct ModernSliderRow: View {
	let symbolName: String
	let title: String
	let subtitle: String
	@Binding var value: Double
	let isEnabled: Bool

	var body: some View {
		ModernSettingRow(symbolName: symbolName, title: title, subtitle: subtitle) {
			HStack(spacing: 10) {
				GlassSlider(value: $value, isEnabled: isEnabled)
					.frame(width: SettingsControlLayout.sliderTrackWidth, height: 24)
				Text("\(Int((value * 100).rounded()))")
					.font(.system(size: 11, weight: .semibold, design: .monospaced))
					.foregroundStyle(.secondary)
					.frame(width: SettingsControlLayout.sliderValueWidth, alignment: .trailing)
			}
			.frame(width: SettingsControlLayout.controlColumnWidth, alignment: .trailing)
			.disabled(!isEnabled)
		}
	}
}

struct SettingsHeroControlTile<Control: View>: View {
	let symbolName: String
	let title: String
	let subtitle: String
	let control: Control

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
		HStack(spacing: 10) {
			SettingsTileIcon(symbolName: symbolName, size: 20)
			VStack(alignment: .leading, spacing: 2) {
				Text(title)
					.font(.system(size: 13, weight: .semibold))
					.lineLimit(1)
					.minimumScaleFactor(0.86)
				Text(subtitle)
					.font(.system(size: 10.8, weight: .medium))
					.foregroundStyle(.secondary)
					.lineLimit(1)
					.minimumScaleFactor(0.86)
			}
			.layoutPriority(1)
			Spacer(minLength: 8)
			control
				.frame(width: SettingsControlLayout.controlColumnWidth, alignment: .trailing)
		}
		.padding(.vertical, 6)
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

struct SettingsControlTile<Control: View>: View {
	let symbolName: String
	let title: String
	let subtitle: String
	let control: Control

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
		HStack(spacing: 10) {
			SettingsTileIcon(symbolName: symbolName, size: 19)
			VStack(alignment: .leading, spacing: 2) {
				Text(title)
					.font(.system(size: 13, weight: .semibold))
					.lineLimit(1)
				Text(subtitle)
					.font(.system(size: 10.5, weight: .medium))
					.foregroundStyle(.secondary)
					.lineLimit(1)
					.minimumScaleFactor(0.86)
			}
			.layoutPriority(1)
			Spacer(minLength: 10)
			control
				.frame(width: SettingsControlLayout.controlColumnWidth, alignment: .trailing)
		}
		.padding(.vertical, 5)
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

struct SettingsTileIcon: View {
	let symbolName: String
	let size: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Image(systemName: symbolName)
			.symbolRenderingMode(.hierarchical)
			.font(.system(size: size * 0.86, weight: .semibold))
			.foregroundStyle(Color.accentColor.opacity(colorScheme == .light ? 0.88 : 0.95))
			.frame(width: size + 8, height: size + 8)
			.contentShape(Rectangle())
	}
}

struct ModernSegmentButton: View {
	let title: String
	let isSelected: Bool
	let isEnabled: Bool
	let action: () -> Void
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false

	var body: some View {
		Button {
			withAnimation(.spring(response: 0.22, dampingFraction: 0.84)) {
				action()
			}
		} label: {
			VStack(spacing: 3) {
				Text(title)
					.font(.system(size: 10.2, weight: .semibold))
					.lineLimit(1)
					.minimumScaleFactor(0.9)
					.foregroundStyle(textColor)
					.padding(.horizontal, 6)

				Capsule()
					.fill(isSelected ? Color.accentColor : Color.clear)
					.frame(width: 14, height: 2)
			}
			.padding(.vertical, 2)
			.frame(maxWidth: .infinity, minHeight: 22)
			.background(hoverBackground, in: .rect(cornerRadius: 6, style: .continuous))
			.contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
		}
		.buttonStyle(.plain)
		.disabled(!isEnabled)
		.frame(maxWidth: .infinity)
		.animation(.spring(response: 0.22, dampingFraction: 0.84), value: isSelected)
		.animation(.easeOut(duration: 0.12), value: isHovered)
		.onHover { hovering in
			isHovered = hovering
		}
	}

	private var hoverBackground: Color {
		if isHovered && isSelected == false && isEnabled {
			return colorScheme == .light ? Color.black.opacity(0.035) : Color.white.opacity(0.050)
		}
		return .clear
	}

	private var textColor: Color {
		if isEnabled == false {
			return Color.secondary.opacity(0.54)
		}
		if isSelected {
			return Color.accentColor
		}
		return Color.primary.opacity(colorScheme == .light ? 0.88 : 0.92)
	}
}

struct GlassSlider: View {
	@Binding var value: Double
	let isEnabled: Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false

	var body: some View {
		GeometryReader { proxy in
			let width = max(proxy.size.width, 1)
			let clampedValue = value.clamped(to: 0...1)
			let progress = CGFloat(clampedValue)
			let knobSize: CGFloat = isHovered && isEnabled ? 14 : 13
			let knobOffset = min(max(0, width * progress - knobSize / 2), max(0, width - knobSize))

			ZStack(alignment: .leading) {
				Capsule()
					.fill(trackFill)
					.frame(height: 6)
				Capsule()
					.fill(fillGradient)
					.frame(width: min(width, max(8, width * progress)), height: 6)
				Circle()
					.fill(knobFill)
					.frame(width: knobSize, height: knobSize)
					.overlay {
						Circle()
							.stroke(knobBorder, lineWidth: 1)
					}
					.offset(x: knobOffset)
			}
			.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
			.contentShape(Rectangle())
			.animation(.spring(response: 0.18, dampingFraction: 0.82), value: isHovered)
			.gesture(
				DragGesture(minimumDistance: 0)
					.onChanged { gesture in
						guard isEnabled else {
							return
						}
						let nextValue = min(max(gesture.location.x / width, 0), 1)
						value = Double(nextValue)
					}
			)
		}
		.opacity(isEnabled ? 1 : 0.48)
		.animation(.easeOut(duration: 0.12), value: isEnabled)
		.onHover { hovering in
			isHovered = hovering
		}
	}

	private var trackFill: Color {
		colorScheme == .light ? Color.black.opacity(0.09) : Color.white.opacity(0.10)
	}

	private var fillGradient: LinearGradient {
		LinearGradient(
			colors: [
				Color.accentColor.opacity(colorScheme == .light ? 0.86 : 0.72),
				Color.accentColor.opacity(colorScheme == .light ? 0.86 : 0.72),
			],
			startPoint: .leading,
			endPoint: .trailing
		)
	}

	private var knobFill: LinearGradient {
		LinearGradient(
			colors: [
				Color.accentColor.opacity(colorScheme == .light ? 0.95 : 0.82),
				Color.accentColor.opacity(colorScheme == .light ? 0.95 : 0.82),
			],
			startPoint: .topLeading,
			endPoint: .bottomTrailing
		)
	}

	private var knobBorder: Color {
		Color.accentColor.opacity(colorScheme == .light ? 0.95 : 0.82)
	}
}

struct SettingsAtmosphere: View {
	let tintHue: Double
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		ZStack {
			Rectangle()
				.fill(Color(nsColor: .windowBackgroundColor))
			Rectangle()
				.fill(.ultraThinMaterial)
			LinearGradient(
				colors: [
					tintColor.opacity(colorScheme == .light ? 0.16 : 0.22),
					Color.clear,
					Color.accentColor.opacity(colorScheme == .light ? 0.08 : 0.14),
				],
				startPoint: .topLeading,
				endPoint: .bottomTrailing
			)
			if colorScheme == .light {
				Color.white.opacity(0.20)
			} else {
				Color.black.opacity(0.14)
			}
		}
		.clipShape(windowShape)
		.overlay {
			windowShape
				.stroke(windowBorderColor, lineWidth: 1)
				.allowsHitTesting(false)
		}
		.ignoresSafeArea()
	}

	private var windowShape: RoundedRectangle {
		RoundedRectangle(
			cornerRadius: NativeHostSettingsWindowMetrics.cornerRadius,
			style: .continuous
		)
	}

	private var tintColor: Color {
		Color(hue: tintHue, saturation: 0.58, brightness: 0.94)
	}

	private var windowBorderColor: Color {
		colorScheme == .light ? Color.white.opacity(0.58) : Color.white.opacity(0.10)
	}
}

extension View {
	@ViewBuilder
	func rsnapGlassButton(prominent: Bool) -> some View {
		self.buttonStyle(SettingsCommandButtonStyle(prominent: prominent))
	}

	func segmentedGlassBackground() -> some View {
		self.modifier(SegmentedGlassBackgroundModifier())
	}

	func settingsGlassSurface(cornerRadius: CGFloat, role _: SettingsGlassRole) -> some View {
		self.modifier(SettingsGlassSurfaceModifier(cornerRadius: cornerRadius))
	}
}

private struct SettingsCommandButtonStyle: ButtonStyle {
	let prominent: Bool
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.isEnabled) private var isEnabled

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.labelStyle(.titleAndIcon)
			.font(.system(size: 10.5, weight: .semibold))
			.foregroundStyle(foregroundColor)
			.symbolRenderingMode(.hierarchical)
			.padding(.horizontal, 9)
			.padding(.vertical, 4.5)
			.background(
				background(isPressed: configuration.isPressed),
				in: .rect(cornerRadius: 7, style: .continuous)
			)
			.opacity(isEnabled ? 1 : 0.55)
			.scaleEffect(configuration.isPressed ? 0.97 : 1)
			.animation(.easeOut(duration: 0.12), value: configuration.isPressed)
	}

	private var foregroundColor: Color {
		if prominent {
			return Color.accentColor
		}
		return Color.primary.opacity(colorScheme == .light ? 0.72 : 0.76)
	}

	private func background(isPressed: Bool) -> Color {
		if prominent {
			return Color.accentColor.opacity(isPressed ? 0.12 : 0.08)
		}

		return colorScheme == .light
			? Color.black.opacity(isPressed ? 0.045 : 0)
			: Color.white.opacity(isPressed ? 0.050 : 0)
	}
}

struct SettingsToggleStyle: ToggleStyle {
	@Environment(\.colorScheme) private var colorScheme

	func makeBody(configuration: Configuration) -> some View {
		Button {
			withAnimation(.spring(response: 0.22, dampingFraction: 0.82)) {
				configuration.isOn.toggle()
			}
		} label: {
			ZStack(alignment: configuration.isOn ? .trailing : .leading) {
				Capsule()
					.fill(trackFill(isOn: configuration.isOn))
					.overlay {
						Capsule()
							.stroke(trackBorder(isOn: configuration.isOn), lineWidth: 1)
					}
				Circle()
					.fill(knobFill(isOn: configuration.isOn))
					.frame(width: 16, height: 16)
					.overlay {
						Circle()
							.stroke(knobBorder(isOn: configuration.isOn), lineWidth: 1)
					}
					.padding(2)
			}
			.frame(width: 38, height: 20)
			.animation(
				.spring(response: 0.22, dampingFraction: 0.82),
				value: configuration.isOn
			)
		}
		.buttonStyle(.plain)
	}

	private func trackFill(isOn: Bool) -> LinearGradient {
		let colors =
			isOn
			? [Color.accentColor.opacity(0.24), Color.accentColor.opacity(0.24)]
			: [
				colorScheme == .light ? Color.black.opacity(0.13) : Color.white.opacity(0.12),
				colorScheme == .light ? Color.black.opacity(0.13) : Color.white.opacity(0.12),
			]
		return LinearGradient(colors: colors, startPoint: .leading, endPoint: .trailing)
	}

	private func trackBorder(isOn: Bool) -> Color {
		isOn ? Color.accentColor.opacity(0.18) : Color.primary.opacity(0.08)
	}

	private func knobFill(isOn: Bool) -> LinearGradient {
		LinearGradient(
			colors: [
				isOn
					? Color.accentColor.opacity(colorScheme == .light ? 0.90 : 0.78)
					: (colorScheme == .light ? Color.white : Color.white.opacity(0.92)),
				isOn
					? Color.accentColor.opacity(colorScheme == .light ? 0.90 : 0.78)
					: (colorScheme == .light ? Color.white : Color.white.opacity(0.92)),
			],
			startPoint: .topLeading,
			endPoint: .bottomTrailing
		)
	}

	private func knobBorder(isOn: Bool) -> Color {
		isOn
			? Color.accentColor.opacity(0.90)
			: (colorScheme == .light ? Color.black.opacity(0.10) : Color.white.opacity(0.20))
	}
}

private struct SegmentedGlassBackgroundModifier: ViewModifier {
	func body(content: Content) -> some View {
		content
	}
}

enum SettingsGlassRole {
	case panel
}

private struct SettingsGlassSurfaceModifier: ViewModifier {
	let cornerRadius: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	@ViewBuilder
	func body(content: Content) -> some View {
		content
			.background(.thinMaterial, in: shape)
			.background(panelFill, in: shape)
			.overlay {
				RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
					.stroke(panelBorderColor, lineWidth: 1)
					.allowsHitTesting(false)
			}
	}

	private var shape: RoundedRectangle {
		RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
	}

	private var panelFill: Color {
		colorScheme == .light
			? Color.white.opacity(0.54)
			: Color.white.opacity(0.050)
	}

	private var panelBorderColor: Color {
		colorScheme == .light ? Color.white.opacity(0.62) : Color.white.opacity(0.090)
	}
}
