import AppKit
import CoreGraphics
import SwiftUI

@MainActor
final class PermissionRecoveryGuideWindowController: NSWindowController {
	private enum GuideDirection: Equatable {
		case left
		case right

		var symbolName: String {
			switch self {
			case .left:
				return "arrow.left"
			case .right:
				return "arrow.right"
			}
		}
	}

	private static let windowSize = NSSize(width: 318, height: 50)
	private static let cornerRadius: CGFloat = 17
	private static let windowGap: CGFloat = 14
	private var positionWorkItem: DispatchWorkItem?
	private var statusPollWorkItem: DispatchWorkItem?
	private var guideDirection: GuideDirection = .left
	private let materialView = NSVisualEffectView()
	private var hostingController: NSHostingController<PermissionRecoveryGuideView>?

	init() {
		let panel = NSPanel(
			contentRect: NSRect(origin: .zero, size: Self.windowSize),
			styleMask: [.borderless, .nonactivatingPanel, .fullSizeContentView],
			backing: .buffered,
			defer: false
		)
		panel.backgroundColor = .clear
		panel.collectionBehavior = [.fullScreenAuxiliary, .moveToActiveSpace]
		panel.hasShadow = true
		panel.hidesOnDeactivate = false
		panel.isFloatingPanel = true
		panel.isMovable = false
		panel.isOpaque = false
		panel.isReleasedWhenClosed = false
		panel.level = .floating
		super.init(window: panel)
		configureMaterialView()
		updateRootView()
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func present() {
		NativePermissions.openScreenRecordingSettings()
		updateRootView()
		window?.orderOut(nil)
		scheduleSystemSettingsPositioning()
		schedulePermissionStatusPolling()
	}

	override func close() {
		positionWorkItem?.cancel()
		statusPollWorkItem?.cancel()
		positionWorkItem = nil
		statusPollWorkItem = nil
		super.close()
	}

	private func updateRootView() {
		let bundleURL = Bundle.main.bundleURL
		let appIcon = NSWorkspace.shared.icon(forFile: bundleURL.path)
		let rootView = PermissionRecoveryGuideView(
			directionSymbolName: guideDirection.symbolName,
			bundleURL: bundleURL,
			appIcon: appIcon,
			openSettings: { [weak self] in
				guard let self else {
					return
				}
				NativePermissions.openScreenRecordingSettings()
				self.scheduleSystemSettingsPositioning()
			}
		)
		if let hostingController {
			hostingController.rootView = rootView
			return
		}

		let hostingController = NSHostingController(rootView: rootView)
		hostingController.view.translatesAutoresizingMaskIntoConstraints = false
		hostingController.view.wantsLayer = true
		hostingController.view.layer?.backgroundColor = NSColor.clear.cgColor
		materialView.addSubview(hostingController.view)
		NSLayoutConstraint.activate([
			hostingController.view.leadingAnchor.constraint(equalTo: materialView.leadingAnchor),
			hostingController.view.trailingAnchor.constraint(equalTo: materialView.trailingAnchor),
			hostingController.view.topAnchor.constraint(equalTo: materialView.topAnchor),
			hostingController.view.bottomAnchor.constraint(equalTo: materialView.bottomAnchor),
		])
		self.hostingController = hostingController
	}

	private func configureMaterialView() {
		materialView.frame = NSRect(origin: .zero, size: Self.windowSize)
		materialView.autoresizingMask = [.width, .height]
		materialView.blendingMode = .withinWindow
		materialView.material = .popover
		materialView.state = .active
		materialView.wantsLayer = true
		materialView.layer?.cornerRadius = Self.cornerRadius
		if #available(macOS 10.15, *) {
			materialView.layer?.cornerCurve = .continuous
		}
		materialView.layer?.masksToBounds = true
		materialView.maskImage = Self.roundedMaskImage(
			size: Self.windowSize,
			cornerRadius: Self.cornerRadius
		)
		window?.contentView = materialView
	}

	private static func roundedMaskImage(size: NSSize, cornerRadius: CGFloat) -> NSImage {
		let image = NSImage(size: size)
		image.lockFocus()
		NSColor.black.setFill()
		NSBezierPath(
			roundedRect: NSRect(origin: .zero, size: size),
			xRadius: cornerRadius,
			yRadius: cornerRadius
		)
		.fill()
		image.unlockFocus()
		return image
	}

	private func scheduleSystemSettingsPositioning() {
		positionWorkItem?.cancel()
		positionNearSystemSettings(remainingAttempts: 8)
	}

	private func positionNearSystemSettings(remainingAttempts: Int) {
		if let settingsFrame = Self.systemSettingsWindowFrame() {
			let direction = positionBesideSystemSettings(frame: settingsFrame)
			if guideDirection != direction {
				guideDirection = direction
				updateRootView()
			}
			revealGuideWindow()
			return
		}

		guard remainingAttempts > 0 else {
			positionAtFallbackLocation()
			if guideDirection != .left {
				guideDirection = .left
				updateRootView()
			}
			revealGuideWindow()
			return
		}
		let workItem = DispatchWorkItem { [weak self] in
			self?.positionNearSystemSettings(remainingAttempts: remainingAttempts - 1)
		}
		positionWorkItem = workItem
		DispatchQueue.main.asyncAfter(deadline: .now() + 0.45, execute: workItem)
	}

	private func schedulePermissionStatusPolling() {
		statusPollWorkItem?.cancel()
		pollPermissionStatus(remainingAttempts: 90)
	}

	private func pollPermissionStatus(remainingAttempts: Int) {
		if NativePermissions.screenRecordingGranted {
			close()
			return
		}
		guard remainingAttempts > 0 else {
			return
		}
		let workItem = DispatchWorkItem { [weak self] in
			self?.pollPermissionStatus(remainingAttempts: remainingAttempts - 1)
		}
		statusPollWorkItem = workItem
		DispatchQueue.main.asyncAfter(deadline: .now() + 0.75, execute: workItem)
	}

	private func revealGuideWindow() {
		showWindow(nil)
		window?.orderFrontRegardless()
	}

	private func positionAtFallbackLocation() {
		guard let screen = NSScreen.main ?? NSScreen.screens.first else {
			return
		}
		let visibleFrame = screen.visibleFrame
		let origin = NSPoint(
			x: visibleFrame.maxX - Self.windowSize.width - 30,
			y: visibleFrame.maxY - Self.windowSize.height - 86
		)
		window?.setFrame(NSRect(origin: origin, size: Self.windowSize), display: true)
		guideDirection = .left
	}

	@discardableResult
	private func positionBesideSystemSettings(frame settingsFrame: CGRect) -> GuideDirection {
		guard let screen = Self.screen(containing: settingsFrame) ?? NSScreen.main else {
			return .left
		}
		let visibleFrame = screen.visibleFrame
		let y = min(
			max(settingsFrame.midY - Self.windowSize.height / 2, visibleFrame.minY + 12),
			visibleFrame.maxY - Self.windowSize.height - 12
		)
		let rightOrigin = NSPoint(
			x: settingsFrame.maxX + Self.windowGap,
			y: y
		)
		if rightOrigin.x + Self.windowSize.width <= visibleFrame.maxX - 8 {
			window?.setFrame(NSRect(origin: rightOrigin, size: Self.windowSize), display: true)
			return .left
		}

		let leftOrigin = NSPoint(
			x: settingsFrame.minX - Self.windowGap - Self.windowSize.width,
			y: y
		)
		if leftOrigin.x >= visibleFrame.minX + 8 {
			window?.setFrame(NSRect(origin: leftOrigin, size: Self.windowSize), display: true)
			return .right
		}

		let fallbackOrigin = NSPoint(
			x: min(
				max(settingsFrame.maxX - Self.windowSize.width - 18, visibleFrame.minX + 8),
				visibleFrame.maxX - Self.windowSize.width - 8
			),
			y: min(settingsFrame.maxY + 12, visibleFrame.maxY - Self.windowSize.height - 8)
		)
		window?.setFrame(NSRect(origin: fallbackOrigin, size: Self.windowSize), display: true)
		return .left
	}

	private static func systemSettingsWindowFrame() -> CGRect? {
		guard
			let windowInfos = CGWindowListCopyWindowInfo(
				[.optionOnScreenOnly, .excludeDesktopElements],
				kCGNullWindowID
			) as? [[String: Any]]
		else {
			return nil
		}

		return windowInfos.compactMap { info -> CGRect? in
			guard
				let ownerName = info[kCGWindowOwnerName as String] as? String,
				ownerName == "System Settings" || ownerName == "System Preferences",
				(info[kCGWindowLayer as String] as? Int) == 0,
				let bounds = info[kCGWindowBounds as String] as? [String: Any],
				let frame = CGRect(dictionaryRepresentation: bounds as CFDictionary)
			else {
				return nil
			}
			return convertCGWindowFrameToAppKit(frame)
		}
		.max { lhs, rhs in
			lhs.width * lhs.height < rhs.width * rhs.height
		}
	}

	private static func convertCGWindowFrameToAppKit(_ cgFrame: CGRect) -> CGRect {
		for screen in NSScreen.screens {
			let candidate = CGRect(
				x: cgFrame.minX,
				y: screen.frame.maxY - cgFrame.maxY,
				width: cgFrame.width,
				height: cgFrame.height
			)
			if candidate.intersects(screen.frame.insetBy(dx: -40, dy: -40)) {
				return candidate
			}
		}
		guard let mainScreen = NSScreen.main else {
			return cgFrame
		}
		return CGRect(
			x: cgFrame.minX,
			y: mainScreen.frame.maxY - cgFrame.maxY,
			width: cgFrame.width,
			height: cgFrame.height
		)
	}

	private static func screen(containing frame: CGRect) -> NSScreen? {
		NSScreen.screens.max { lhs, rhs in
			intersectionArea(lhs.frame, frame) < intersectionArea(rhs.frame, frame)
		}
	}

	private static func intersectionArea(_ lhs: CGRect, _ rhs: CGRect) -> CGFloat {
		let intersection = lhs.intersection(rhs)
		guard !intersection.isNull, !intersection.isInfinite else {
			return 0
		}
		return max(0, intersection.width) * max(0, intersection.height)
	}
}

private struct PermissionRecoveryGuideView: View {
	let directionSymbolName: String
	let bundleURL: URL
	let appIcon: NSImage
	let openSettings: () -> Void
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme
	@State private var pulse = false

	var body: some View {
		HStack(alignment: .center, spacing: 8) {
			if pointsLeft {
				arrowGuide
				appDragChip
				instructionText
				openSettingsButton
			} else {
				openSettingsButton
				instructionText
				appDragChip
				arrowGuide
			}
		}
		.padding(.horizontal, 11)
		.frame(width: 318, height: 50)
		.onAppear {
			guard !reduceMotion else {
				return
			}
			withAnimation(.easeInOut(duration: 0.78).repeatForever(autoreverses: true)) {
				pulse = true
			}
		}
	}

	private var pointsLeft: Bool {
		directionSymbolName == "arrow.left"
	}

	private var guideTextFont: Font {
		.system(size: 11.2, weight: .semibold)
	}

	private var guideTextColor: Color {
		Color.primary.opacity(colorScheme == .light ? 0.78 : 0.86)
	}

	private var arrowGuide: some View {
		PermissionGuideArrow(symbolName: directionSymbolName, pulse: pulse)
			.frame(width: 40, height: 31)
	}

	private var appDragChip: some View {
		PermissionAppDragSource(
			bundleURL: bundleURL, icon: appIcon, label: NativeHostBrand.appBundleName
		)
		.frame(width: 114, height: 31)
		.overlay {
			Capsule()
				.stroke(Color.accentColor.opacity(pulse ? 0.58 : 0.20), lineWidth: 1.2)
				.scaleEffect(reduceMotion ? 1 : (pulse ? 1.055 : 1))
				.allowsHitTesting(false)
		}
	}

	private var instructionText: some View {
		Text("Drop in, turn on")
			.font(guideTextFont)
			.foregroundStyle(guideTextColor)
			.lineLimit(1)
			.minimumScaleFactor(0.88)
			.frame(maxWidth: .infinity, alignment: .leading)
			.layoutPriority(1)
	}

	private var openSettingsButton: some View {
		Button(action: openSettings) {
			Image(systemName: "arrow.up.forward.app")
				.font(.system(size: 11.4, weight: .semibold))
				.frame(width: 24, height: 24)
		}
		.buttonStyle(.plain)
		.foregroundStyle(Color.accentColor)
		.background(
			Color.accentColor.opacity(colorScheme == .light ? 0.075 : 0.13),
			in: RoundedRectangle(cornerRadius: 7, style: .continuous)
		)
		.frame(width: 28, height: 31)
		.help("Open Screen Recording settings")
	}
}

private struct PermissionGuideArrow: View {
	let symbolName: String
	let pulse: Bool
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	var body: some View {
		HStack(spacing: 3) {
			if pointsLeft {
				arrow
				dots
			} else {
				dots
				arrow
			}
		}
	}

	private var arrow: some View {
		Image(systemName: symbolName)
			.font(.system(size: 19, weight: .bold))
			.foregroundStyle(Color.accentColor)
			.offset(x: arrowOffset)
	}

	private var dots: some View {
		HStack(spacing: 3) {
			ForEach(0..<3) { index in
				Circle()
					.fill(Color.accentColor.opacity(dotOpacity(index: index)))
					.frame(width: 3.8, height: 3.8)
			}
		}
	}

	private func dotOpacity(index: Int) -> Double {
		guard !reduceMotion else {
			return 0.34
		}
		let activeIndex = pulse ? 2 : 0
		return index == activeIndex ? 0.80 : 0.26
	}

	private var arrowOffset: CGFloat {
		guard !reduceMotion else {
			return 0
		}
		switch symbolName {
		case "arrow.left":
			return pulse ? -3 : 1
		case "arrow.right":
			return pulse ? 3 : -1
		default:
			return 0
		}
	}

	private var pointsLeft: Bool {
		symbolName == "arrow.left"
	}
}
