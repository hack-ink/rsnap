import AppKit
import RsnapHostBridge
import SwiftUI

enum CaptureFramePresetOption: Hashable, Identifiable {
	case off
	case background(CaptureFrameBackgroundPreference)

	var id: String {
		switch self {
		case .off:
			return "off"
		case .background(let background):
			return background.rawValue
		}
	}

	var title: String {
		switch self {
		case .off:
			return "Off"
		case .background(let background):
			return background.title
		}
	}

	static var allOptions: [CaptureFramePresetOption] {
		[.off] + CaptureFrameBackgroundPreference.allCases.map { .background($0) }
	}
}

struct CaptureFramePresetSelector: View {
	let selection: CaptureFramePresetOption
	let onSelect: (CaptureFramePresetOption) -> Void
	@State private var leadingIndex = 0

	var body: some View {
		let options = CaptureFramePresetOption.allOptions

		ZStack {
			HStack(spacing: SettingsControlLayout.framePresetSwatchSpacing) {
				ForEach(options) { option in
					CaptureFramePresetSwatchButton(option: option, isSelected: option == selection)
					{
						onSelect(option)
					}
				}
			}
			.padding(.horizontal, 1)
			.padding(.vertical, 2)
			.fixedSize(horizontal: true, vertical: false)
			.offset(x: -scrollOffset(for: options.count))
			.animation(.easeOut(duration: 0.16), value: leadingIndex)
			.frame(
				width: contentWidth(for: options.count),
				height: SettingsControlLayout.framePresetSelectorHeight,
				alignment: .leading
			)
			.frame(
				width: SettingsControlLayout.controlColumnWidth,
				height: SettingsControlLayout.framePresetSelectorHeight,
				alignment: .leading
			)
			.clipped()
			.mask {
				selectorMask(optionCount: options.count)
			}

			HStack {
				if canStepBackward {
					stepButton(systemName: "chevron.left", label: "Previous frame preset") {
						shiftLeadingIndex(by: -1, optionCount: options.count)
					}
				}
				Spacer(minLength: 0)
				if canStepForward(optionCount: options.count) {
					stepButton(systemName: "chevron.right", label: "Next frame preset") {
						shiftLeadingIndex(by: 1, optionCount: options.count)
					}
				}
			}
		}
		.frame(
			width: SettingsControlLayout.controlColumnWidth,
			height: SettingsControlLayout.framePresetSelectorHeight
		)
		.contentShape(Rectangle())
		.gesture(
			DragGesture(minimumDistance: 8).onEnded { value in
				if value.translation.width < -14 {
					shiftLeadingIndex(by: 1, optionCount: options.count)
				} else if value.translation.width > 14 {
					shiftLeadingIndex(by: -1, optionCount: options.count)
				}
			}
		)
		.onAppear {
			revealSelection(options: options)
		}
		.onChange(of: selection.id) { _, _ in
			revealSelection(options: options)
		}
		.accessibilityElement(children: .contain)
	}

	private var canStepBackward: Bool {
		leadingIndex > 0
	}

	private func canStepForward(optionCount: Int) -> Bool {
		leadingIndex < maxLeadingIndex(for: optionCount)
	}

	@ViewBuilder
	private func stepButton(systemName: String, label: String, action: @escaping () -> Void)
		-> some View
	{
		RepeatingFramePresetStepButton(
			systemName: systemName,
			label: label,
			action: action
		)
	}

	@ViewBuilder
	private func selectorMask(optionCount: Int) -> some View {
		HStack(spacing: 0) {
			if canStepBackward {
				LinearGradient(
					colors: [.clear, .black],
					startPoint: .leading,
					endPoint: .trailing
				)
				.frame(width: 18)
			}
			Rectangle().fill(.black)
			if canStepForward(optionCount: optionCount) {
				LinearGradient(
					colors: [.black, .clear],
					startPoint: .leading,
					endPoint: .trailing
				)
				.frame(width: 18)
			}
		}
	}

	private func revealSelection(options: [CaptureFramePresetOption]) {
		guard let selectedIndex = options.firstIndex(of: selection) else {
			return
		}
		let visibleCount = visibleOptionCount
		let upperVisibleIndex = leadingIndex + visibleCount - 1
		let updatedIndex =
			if selectedIndex < leadingIndex {
				selectedIndex
			} else if selectedIndex > upperVisibleIndex {
				selectedIndex - visibleCount + 1
			} else {
				leadingIndex
			}
		leadingIndex = clampedLeadingIndex(updatedIndex, optionCount: options.count)
	}

	private func shiftLeadingIndex(by delta: Int, optionCount: Int) {
		leadingIndex = clampedLeadingIndex(leadingIndex + delta, optionCount: optionCount)
	}

	private var visibleOptionCount: Int {
		let itemAdvance =
			SettingsControlLayout.framePresetSwatchWidth
			+ SettingsControlLayout.framePresetSwatchSpacing
		return max(
			Int(
				floor(
					(SettingsControlLayout.controlColumnWidth
						+ SettingsControlLayout.framePresetSwatchSpacing)
						/ itemAdvance)
			),
			1
		)
	}

	private func maxLeadingIndex(for optionCount: Int) -> Int {
		max(optionCount - visibleOptionCount, 0)
	}

	private func clampedLeadingIndex(_ index: Int, optionCount: Int) -> Int {
		min(max(index, 0), maxLeadingIndex(for: optionCount))
	}

	private func scrollOffset(for optionCount: Int) -> CGFloat {
		let itemAdvance =
			SettingsControlLayout.framePresetSwatchWidth
			+ SettingsControlLayout.framePresetSwatchSpacing
		let requestedOffset =
			CGFloat(clampedLeadingIndex(leadingIndex, optionCount: optionCount))
			* itemAdvance
		return min(requestedOffset, maxContentOffset(for: optionCount))
	}

	private func contentWidth(for optionCount: Int) -> CGFloat {
		guard optionCount > 0 else {
			return 0
		}
		return CGFloat(optionCount) * SettingsControlLayout.framePresetSwatchWidth
			+ CGFloat(optionCount - 1) * SettingsControlLayout.framePresetSwatchSpacing + 2
	}

	private func maxContentOffset(for optionCount: Int) -> CGFloat {
		max(contentWidth(for: optionCount) - SettingsControlLayout.controlColumnWidth, 0)
	}
}

private struct RepeatingFramePresetStepButton: View {
	let systemName: String
	let label: String
	let action: () -> Void
	@State private var isPressed = false
	@State private var repeatCount = 0
	@State private var repeatWorkItem: DispatchWorkItem?

	var body: some View {
		Image(systemName: systemName)
			.symbolRenderingMode(.hierarchical)
			.font(.system(size: 9, weight: .bold))
			.foregroundStyle(Color.primary.opacity(isPressed ? 0.90 : 0.74))
			.frame(width: 18, height: 22)
			.background(.thinMaterial, in: Capsule())
			.overlay {
				Capsule().stroke(Color.primary.opacity(isPressed ? 0.18 : 0.10), lineWidth: 1)
			}
			.scaleEffect(isPressed ? 0.96 : 1)
			.contentShape(Capsule())
			.gesture(
				DragGesture(minimumDistance: 0)
					.onChanged { _ in
						beginPress()
					}
					.onEnded { _ in
						endPress()
					}
			)
			.animation(.easeOut(duration: 0.10), value: isPressed)
			.help(label)
			.accessibilityLabel(label)
			.accessibilityAddTraits(.isButton)
			.accessibilityAction {
				action()
			}
			.onDisappear(perform: endPress)
	}

	private func beginPress() {
		guard isPressed == false else {
			return
		}

		isPressed = true
		repeatCount = 0
		action()
		scheduleRepeat(after: 0.34)
	}

	private func endPress() {
		isPressed = false
		repeatCount = 0
		repeatWorkItem?.cancel()
		repeatWorkItem = nil
	}

	private func scheduleRepeat(after delay: TimeInterval) {
		repeatWorkItem?.cancel()
		let workItem = DispatchWorkItem {
			guard isPressed else {
				return
			}

			action()
			repeatCount += 1
			scheduleRepeat(after: repeatInterval)
		}
		repeatWorkItem = workItem
		DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: workItem)
	}

	private var repeatInterval: TimeInterval {
		switch repeatCount {
		case 0..<4:
			return 0.15
		case 4..<10:
			return 0.10
		default:
			return 0.065
		}
	}
}

private struct CaptureFramePresetSwatchButton: View {
	let option: CaptureFramePresetOption
	let isSelected: Bool
	let onSelect: () -> Void
	@State private var isHovered = false

	var body: some View {
		Button(action: onSelect) {
			CaptureFramePresetSwatch(option: option, isSelected: isSelected)
				.scaleEffect(isHovered ? 1.04 : 1)
		}
		.buttonStyle(.plain)
		.help(option.title)
		.accessibilityLabel(option.title)
		.accessibilityValue(isSelected ? "Selected" : "")
		.animation(.easeOut(duration: 0.12), value: isHovered)
		.onHover { hovering in
			isHovered = hovering
		}
	}
}

private struct CaptureFramePresetSwatch: View {
	let option: CaptureFramePresetOption
	let isSelected: Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var wallpaperImage: NSImage?
	@State private var wallpaperThumbnailRequestID: String?

	var body: some View {
		ZStack {
			swatchFill
			offOverlay
			wallpaperFallbackOverlay
			selectionBadge
		}
		.frame(
			width: SettingsControlLayout.framePresetSwatchWidth,
			height: SettingsControlLayout.framePresetSwatchHeight
		)
		.clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
		.overlay {
			RoundedRectangle(cornerRadius: 6, style: .continuous)
				.stroke(borderColor, lineWidth: isSelected ? 1.6 : 1)
		}
		.shadow(color: shadowColor, radius: isSelected ? 3 : 0, x: 0, y: 1)
		.onAppear(perform: refreshWallpaperThumbnail)
	}

	@ViewBuilder
	private var swatchFill: some View {
		switch option {
		case .off:
			LinearGradient(
				colors: [
					Color.primary.opacity(colorScheme == .light ? 0.035 : 0.080),
					Color.primary.opacity(colorScheme == .light ? 0.070 : 0.135),
				],
				startPoint: .topLeading,
				endPoint: .bottomTrailing
			)
		case .background(let background):
			if background == .systemWallpaper, let wallpaperImage {
				Image(nsImage: wallpaperImage)
					.resizable()
					.interpolation(.high)
					.scaledToFill()
			} else {
				backgroundGradient(for: background)
			}
		}
	}

	@ViewBuilder
	private var offOverlay: some View {
		if case .off = option {
			Image(systemName: "slash")
				.symbolRenderingMode(.hierarchical)
				.font(.system(size: 13, weight: .bold))
				.foregroundStyle(Color.secondary.opacity(colorScheme == .light ? 0.64 : 0.78))
		}
	}

	@ViewBuilder
	private var wallpaperFallbackOverlay: some View {
		if case .background(.systemWallpaper) = option, wallpaperImage == nil {
			Image(systemName: "photo")
				.symbolRenderingMode(.hierarchical)
				.font(.system(size: 11, weight: .semibold))
				.foregroundStyle(Color.white.opacity(0.72))
				.shadow(color: Color.black.opacity(0.22), radius: 2, x: 0, y: 1)
		}
	}

	@ViewBuilder
	private var selectionBadge: some View {
		if isSelected {
			VStack {
				Spacer()
				HStack {
					Spacer()
					Circle()
						.fill(Color.accentColor)
						.frame(width: 11, height: 11)
						.overlay {
							Image(systemName: "checkmark")
								.font(.system(size: 6.5, weight: .black))
								.foregroundStyle(Color.white)
						}
						.padding(3)
				}
			}
		}
	}

	private var borderColor: Color {
		if isSelected {
			return Color.accentColor.opacity(colorScheme == .light ? 0.90 : 0.95)
		}
		return colorScheme == .light ? Color.black.opacity(0.12) : Color.white.opacity(0.18)
	}

	private var shadowColor: Color {
		Color.accentColor.opacity(colorScheme == .light ? 0.15 : 0.20)
	}

	private func backgroundGradient(
		for background: CaptureFrameBackgroundPreference
	) -> LinearGradient {
		let plan = CaptureFrameEffectRenderer.backgroundPlan(for: background)
		let colorStops: [CaptureFrameColorStop] = plan?.colorStops ?? fallbackColorStops
		let locations: [CGFloat] = plan?.locations ?? fallbackLocations
		var gradientStops: [Gradient.Stop] = []

		for index in colorStops.indices {
			let colorStop = colorStops[index]
			let location = locations.indices.contains(index) ? locations[index] : CGFloat(index)
			let color = Color(
				red: Double(colorStop.red),
				green: Double(colorStop.green),
				blue: Double(colorStop.blue),
				opacity: Double(colorStop.alpha)
			)
			gradientStops.append(
				Gradient.Stop(color: color, location: location.clamped(to: 0...1)))
		}

		return LinearGradient(
			gradient: Gradient(stops: gradientStops),
			startPoint: .topLeading,
			endPoint: .bottomTrailing
		)
	}

	private var fallbackColorStops: [CaptureFrameColorStop] {
		[
			CaptureFrameColorStop(red: 0.10, green: 0.16, blue: 0.28, alpha: 1),
			CaptureFrameColorStop(red: 0.30, green: 0.47, blue: 0.71, alpha: 1),
			CaptureFrameColorStop(red: 0.95, green: 0.61, blue: 0.43, alpha: 1),
		]
	}

	private var fallbackLocations: [CGFloat] {
		[0, 0.54, 1]
	}

	private func refreshWallpaperThumbnail() {
		guard case .background(.systemWallpaper) = option else {
			wallpaperImage = nil
			wallpaperThumbnailRequestID = nil
			return
		}
		guard
			let wallpaperPath = CaptureFrameEffectRenderer.systemWallpaperPath(
				screen: NSScreen.main)
		else {
			wallpaperImage = nil
			wallpaperThumbnailRequestID = nil
			return
		}

		let targetPixelSize = max(
			Int(
				(SettingsControlLayout.framePresetSwatchWidth
					* (NSScreen.main?.backingScaleFactor ?? 2))
					.rounded(.up)
			),
			1
		)
		let requestID = "\(wallpaperPath)#\(targetPixelSize)"
		guard wallpaperThumbnailRequestID != requestID else {
			return
		}

		wallpaperThumbnailRequestID = requestID
		wallpaperImage = nil
		DispatchQueue.global(qos: .utility).async {
			let snapshot = try? RsnapWallpaperThumbnailDecoder.pngThumbnail(
				path: wallpaperPath,
				targetPixelSize: targetPixelSize
			)
			DispatchQueue.main.async {
				guard wallpaperThumbnailRequestID == requestID else {
					return
				}
				wallpaperImage = snapshot.flatMap(Self.image)
			}
		}
	}

	private static func image(from snapshot: RGBARegionSnapshot) -> NSImage? {
		let expectedByteCount = snapshot.width * snapshot.height * 4
		guard
			snapshot.width > 0,
			snapshot.height > 0,
			snapshot.rgba.count == expectedByteCount,
			let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
			let provider = CGDataProvider(data: snapshot.rgba as CFData)
		else {
			return nil
		}

		guard
			let cgImage = CGImage(
				width: snapshot.width,
				height: snapshot.height,
				bitsPerComponent: 8,
				bitsPerPixel: 32,
				bytesPerRow: snapshot.width * 4,
				space: colorSpace,
				bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
				provider: provider,
				decode: nil,
				shouldInterpolate: true,
				intent: .defaultIntent
			)
		else {
			return nil
		}

		return NSImage(
			cgImage: cgImage,
			size: NSSize(width: snapshot.width, height: snapshot.height)
		)
	}
}
