import AppKit
import CoreGraphics
import RsnapHostBridge
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

	private static let windowSize = NSSize(width: 314, height: 118)
	private static let windowGap: CGFloat = 14
	private var kind: PermissionKind = .screenRecording
	private var positionWorkItem: DispatchWorkItem?
	private var statusPollWorkItem: DispatchWorkItem?
	private var guideDirection: GuideDirection = .left

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
		updateRootView()
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func present(kind: PermissionKind) {
		self.kind = kind
		NativePermissions.openSystemSettings(for: kind)
		updateRootView()
		positionAtFallbackLocation()
		showWindow(nil)
		window?.orderFrontRegardless()
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
		window?.contentViewController = NSHostingController(
			rootView: PermissionRecoveryGuideView(
				directionSymbolName: guideDirection.symbolName,
				bundleURL: bundleURL,
				appIcon: appIcon,
				openSettings: { [weak self] in
					guard let self else {
						return
					}
					NativePermissions.openSystemSettings(for: self.kind)
					self.scheduleSystemSettingsPositioning()
				},
				close: { [weak self] in
					self?.close()
				}
			)
		)
		window?.contentViewController?.view.wantsLayer = true
		window?.contentViewController?.view.layer?.backgroundColor = NSColor.clear.cgColor
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
			window?.orderFrontRegardless()
			return
		}

		guard remainingAttempts > 0 else {
			window?.orderFrontRegardless()
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
		guard window?.isVisible == true else {
			return
		}
		if NativePermissions.status(for: kind) {
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
	let close: () -> Void
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme
	@State private var pulse = false

	var body: some View {
		HStack(spacing: 12) {
			PermissionGuideArrow(symbolName: directionSymbolName, pulse: pulse)
				.frame(width: 38, height: 52)

			VStack(alignment: .leading, spacing: 8) {
				HStack(alignment: .center, spacing: 8) {
					Text("Drag rsnap here")
						.font(.system(size: 12.5, weight: .semibold))
						.lineLimit(1)
					Spacer(minLength: 6)
					Button(action: close) {
						Image(systemName: "xmark")
							.font(.system(size: 9.5, weight: .bold))
							.frame(width: 18, height: 18)
					}
					.buttonStyle(.plain)
					.foregroundStyle(.secondary)
					.help("Close")
				}

				HStack(spacing: 9) {
					PermissionAppDragSource(bundleURL: bundleURL, icon: appIcon, label: "rsnap")
						.frame(width: 116, height: 36)
						.overlay {
							Capsule()
								.stroke(
									Color.accentColor.opacity(pulse ? 0.58 : 0.20), lineWidth: 1.2
								)
								.scaleEffect(reduceMotion ? 1 : (pulse ? 1.06 : 1))
								.allowsHitTesting(false)
						}
					Button(action: openSettings) {
						Image(systemName: "gearshape")
							.font(.system(size: 12.5, weight: .semibold))
							.frame(width: 30, height: 30)
					}
					.buttonStyle(.plain)
					.foregroundStyle(Color.accentColor)
					.background(
						Color.accentColor.opacity(colorScheme == .light ? 0.075 : 0.13),
						in: RoundedRectangle(cornerRadius: 8, style: .continuous)
					)
					.help("Open Screen Recording settings")
				}

				Text("Drop it into Screen Recording, then turn it on.")
					.font(.system(size: 10, weight: .medium))
					.foregroundStyle(.secondary)
					.lineLimit(1)
					.minimumScaleFactor(0.9)
			}
		}
		.padding(.horizontal, 13)
		.padding(.vertical, 11)
		.frame(width: 314, height: 118)
		.background(.thinMaterial, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
		.background(panelFill, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
		.overlay {
			RoundedRectangle(cornerRadius: 18, style: .continuous)
				.stroke(panelBorder, lineWidth: 1)
		}
		.shadow(color: .black.opacity(colorScheme == .light ? 0.16 : 0.34), radius: 18, y: 8)
		.onAppear {
			guard !reduceMotion else {
				return
			}
			withAnimation(.easeInOut(duration: 0.78).repeatForever(autoreverses: true)) {
				pulse = true
			}
		}
	}

	private var panelFill: Color {
		colorScheme == .light ? Color.white.opacity(0.42) : Color.black.opacity(0.20)
	}

	private var panelBorder: Color {
		colorScheme == .light ? Color.black.opacity(0.10) : Color.white.opacity(0.16)
	}
}

private struct PermissionGuideArrow: View {
	let symbolName: String
	let pulse: Bool
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	var body: some View {
		VStack(spacing: 4) {
			ForEach(0..<3) { index in
				Circle()
					.fill(Color.accentColor.opacity(dotOpacity(index: index)))
					.frame(width: 4.5, height: 4.5)
			}
			Image(systemName: symbolName)
				.font(.system(size: 22, weight: .bold))
				.foregroundStyle(Color.accentColor)
				.offset(x: arrowOffset)
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
}
