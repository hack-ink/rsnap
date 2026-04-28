import AppKit
import ApplicationServices
import CoreGraphics
import Foundation
import RsnapHostBridge

@MainActor
final class SettingsWindowController: NSWindowController, NSWindowDelegate {
	private let settingsStore: NativeHostSettingsStore
	private let outputDirectoryValueLabel = NSTextField(labelWithString: "")
	private let prefixField = NSTextField(string: "")
	private let namingControl = NSSegmentedControl(
		labels: OutputNamingPreference.allCases.map(\.title), trackingMode: .selectOne, target: nil,
		action: nil)
	private let toolbarPlacementControl = NSSegmentedControl(
		labels: ToolbarPlacementPreference.allCases.map(\.title), trackingMode: .selectOne,
		target: nil, action: nil)
	private let frozenResizeHandleOrientationControl = NSSegmentedControl(
		labels: FrozenResizeHandleOrientationPreference.allCases.map(\.title),
		trackingMode: .selectOne, target: nil, action: nil)
	private let showAltHintKeycapButton = NSButton(
		checkboxWithTitle: "Show Tab hint in HUD", target: nil, action: nil)
	private let hudGlassEnabledButton = NSButton(
		checkboxWithTitle: "Enable glass", target: nil, action: nil)
	private let hudGlassModeControl = NSSegmentedControl(
		labels: HudGlassModePreference.allCases.map(\.title), trackingMode: .selectOne,
		target: nil, action: nil)
	private let liquidGlassStyleControl = NSSegmentedControl(
		labels: LiquidGlassStylePreference.allCases.map(\.title), trackingMode: .selectOne,
		target: nil, action: nil)
	private let loupeSampleSizeControl = NSSegmentedControl(
		labels: LoupeSampleSizePreference.allCases.map(\.title), trackingMode: .selectOne,
		target: nil, action: nil)
	private let hudOpacitySlider = NSSlider(
		value: 50, minValue: 0, maxValue: 100, target: nil, action: nil)
	private let hudBlurSlider = NSSlider(
		value: 50, minValue: 0, maxValue: 100, target: nil, action: nil)
	private let hudTintSlider = NSSlider(
		value: 50, minValue: 0, maxValue: 100, target: nil, action: nil)
	private let hudTintColorWell = NSColorWell(frame: NSRect(x: 0, y: 0, width: 44, height: 24))
	private let hudOpacityValueLabel = NSTextField(labelWithString: "")
	private let hudBlurValueLabel = NSTextField(labelWithString: "")
	private let hudTintValueLabel = NSTextField(labelWithString: "")
	private var glassTintOptionViews: [NSView] = []
	private var classicGlassOptionViews: [NSView] = []
	private var liquidGlassOptionViews: [NSView] = []

	init(settingsStore: NativeHostSettingsStore) {
		self.settingsStore = settingsStore

		let contentRect = NSRect(x: 0, y: 0, width: 560, height: 640)
		let window = NSWindow(
			contentRect: contentRect,
			styleMask: [.titled, .closable, .miniaturizable],
			backing: .buffered,
			defer: false
		)
		window.title = "Settings"
		window.isReleasedWhenClosed = false
		super.init(window: window)

		window.delegate = self
		window.contentView = buildContentView()
		window.center()
		refreshFromSettings()
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func present() {
		showWindow(nil)
		window?.makeKeyAndOrderFront(nil)
		NSApp.activate(ignoringOtherApps: true)
	}

	private func buildContentView() -> NSView {
		let root = NSView(frame: .zero)
		root.translatesAutoresizingMaskIntoConstraints = false

		let stack = NSStackView()
		stack.orientation = .vertical
		stack.alignment = .leading
		stack.spacing = 18
		stack.edgeInsets = NSEdgeInsets(top: 20, left: 20, bottom: 20, right: 20)
		stack.translatesAutoresizingMaskIntoConstraints = false

		stack.addArrangedSubview(makeSectionTitle("Output"))
		stack.addArrangedSubview(makeOutputDirectoryRow())
		stack.addArrangedSubview(makePrefixRow())
		stack.addArrangedSubview(makeNamingRow())
		stack.addArrangedSubview(makeSectionTitle("Overlay"))
		stack.addArrangedSubview(makeOverlayRow())
		stack.addArrangedSubview(makeHudGlassModeRow())
		stack.addArrangedSubview(makeLoupeSampleSizeRow())
		stack.addArrangedSubview(makeToolbarPlacementRow())
		stack.addArrangedSubview(makeFrozenResizeHandleOrientationRow())

		root.addSubview(stack)
		NSLayoutConstraint.activate([
			stack.leadingAnchor.constraint(equalTo: root.leadingAnchor),
			stack.trailingAnchor.constraint(equalTo: root.trailingAnchor),
			stack.topAnchor.constraint(equalTo: root.topAnchor),
			stack.bottomAnchor.constraint(lessThanOrEqualTo: root.bottomAnchor),
		])
		return root
	}

	private func makeSectionTitle(_ title: String) -> NSTextField {
		let label = NSTextField(labelWithString: title)
		label.font = .systemFont(ofSize: 15, weight: .semibold)
		return label
	}

	private func makeOutputDirectoryRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let title = NSTextField(labelWithString: "Output directory")
		title.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(title)

		outputDirectoryValueLabel.lineBreakMode = .byTruncatingMiddle
		outputDirectoryValueLabel.font = .systemFont(ofSize: 12)
		outputDirectoryValueLabel.textColor = .secondaryLabelColor
		outputDirectoryValueLabel.setContentCompressionResistancePriority(
			.defaultLow, for: .horizontal)

		let chooseButton = NSButton(
			title: "Choose…", target: self, action: #selector(chooseOutputDirectory))
		let row = NSStackView(views: [outputDirectoryValueLabel, chooseButton])
		row.orientation = .horizontal
		row.alignment = .centerY
		row.spacing = 12
		container.addArrangedSubview(row)
		return container
	}

	private func makePrefixRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let title = NSTextField(labelWithString: "Filename prefix")
		title.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(title)

		prefixField.target = self
		prefixField.action = #selector(prefixChanged)
		prefixField.placeholderString = "rsnap"
		prefixField.translatesAutoresizingMaskIntoConstraints = false
		prefixField.widthAnchor.constraint(equalToConstant: 240).isActive = true
		container.addArrangedSubview(prefixField)
		return container
	}

	private func makeNamingRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let title = NSTextField(labelWithString: "Output naming")
		title.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(title)

		namingControl.target = self
		namingControl.action = #selector(namingChanged)
		container.addArrangedSubview(namingControl)
		return container
	}

	private func makeToolbarPlacementRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let title = NSTextField(labelWithString: "Frozen toolbar placement")
		title.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(title)

		let subtitle = NSTextField(labelWithString: "Applies on the next capture session.")
		subtitle.font = .systemFont(ofSize: 12)
		subtitle.textColor = .secondaryLabelColor
		container.addArrangedSubview(subtitle)

		toolbarPlacementControl.target = self
		toolbarPlacementControl.action = #selector(toolbarPlacementChanged)
		container.addArrangedSubview(toolbarPlacementControl)
		return container
	}

	private func makeFrozenResizeHandleOrientationRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let title = NSTextField(labelWithString: "Frozen corner handle direction")
		title.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(title)

		let subtitle = NSTextField(
			labelWithString: "Controls whether the corner brackets open outward or inward.")
		subtitle.font = .systemFont(ofSize: 12)
		subtitle.textColor = .secondaryLabelColor
		container.addArrangedSubview(subtitle)

		frozenResizeHandleOrientationControl.target = self
		frozenResizeHandleOrientationControl.action = #selector(
			frozenResizeHandleOrientationChanged)
		container.addArrangedSubview(frozenResizeHandleOrientationControl)
		return container
	}

	private func makeOverlayRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		showAltHintKeycapButton.target = self
		showAltHintKeycapButton.action = #selector(showAltHintKeycapChanged)

		container.addArrangedSubview(showAltHintKeycapButton)
		return container
	}

	private func makeHudGlassModeRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let title = NSTextField(labelWithString: "Glass style")
		title.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(title)

		hudGlassEnabledButton.target = self
		hudGlassEnabledButton.action = #selector(hudGlassEnabledChanged)
		container.addArrangedSubview(hudGlassEnabledButton)

		hudGlassModeControl.target = self
		hudGlassModeControl.action = #selector(hudGlassModeChanged)
		hudGlassModeControl.segmentStyle = .rounded
		hudGlassModeControl.widthAnchor.constraint(equalToConstant: 260).isActive = true
		if let liquidGlassSegment = HudGlassModePreference.allCases.firstIndex(of: .liquidGlass) {
			hudGlassModeControl.setToolTip(
				"Requires Liquid Glass support.", forSegment: liquidGlassSegment)
		}
		container.addArrangedSubview(hudGlassModeControl)

		let tintRows = [
			makeGlassSubsectionTitle("Glass tint"),
			makeSliderRow(
				title: "Tint", slider: hudTintSlider, valueLabel: hudTintValueLabel,
				action: #selector(hudTintChanged)),
			makeTintColorRow(),
		]
		glassTintOptionViews = tintRows
		for row in tintRows {
			container.addArrangedSubview(row)
		}

		let classicRows = [
			makeGlassSubsectionTitle("Classic Glass options"),
			makeSliderRow(
				title: "Opacity", slider: hudOpacitySlider, valueLabel: hudOpacityValueLabel,
				action: #selector(hudOpacityChanged)),
			makeSliderRow(
				title: "Blur", slider: hudBlurSlider, valueLabel: hudBlurValueLabel,
				action: #selector(hudBlurChanged)),
		]
		classicGlassOptionViews = classicRows
		for row in classicRows {
			container.addArrangedSubview(row)
		}

		let liquidRows = [
			makeGlassSubsectionTitle("Liquid Glass options"),
			makeLiquidGlassStyleRow(),
		]
		liquidGlassOptionViews = liquidRows
		for row in liquidRows {
			container.addArrangedSubview(row)
		}
		return container
	}

	private func makeGlassSubsectionTitle(_ title: String) -> NSTextField {
		let label = NSTextField(labelWithString: title)
		label.font = .systemFont(ofSize: 12, weight: .medium)
		label.textColor = .secondaryLabelColor
		return label
	}

	private func makeLiquidGlassStyleRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let title = NSTextField(labelWithString: "Style")
		title.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(title)

		liquidGlassStyleControl.target = self
		liquidGlassStyleControl.action = #selector(liquidGlassStyleChanged)
		container.addArrangedSubview(liquidGlassStyleControl)
		return container
	}

	private func makeLoupeSampleSizeRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let title = NSTextField(labelWithString: "Loupe sample size")
		title.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(title)

		loupeSampleSizeControl.target = self
		loupeSampleSizeControl.action = #selector(loupeSampleSizeChanged)
		container.addArrangedSubview(loupeSampleSizeControl)
		return container
	}

	private func makeSliderRow(
		title: String, slider: NSSlider, valueLabel: NSTextField, action: Selector
	) -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let titleLabel = NSTextField(labelWithString: title)
		titleLabel.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(titleLabel)

		slider.target = self
		slider.action = action

		valueLabel.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
		valueLabel.textColor = .secondaryLabelColor
		valueLabel.alignment = .right
		valueLabel.widthAnchor.constraint(equalToConstant: 52).isActive = true

		let row = NSStackView(views: [slider, valueLabel])
		row.orientation = .horizontal
		row.alignment = .centerY
		row.spacing = 12
		slider.widthAnchor.constraint(equalToConstant: 280).isActive = true
		container.addArrangedSubview(row)
		return container
	}

	private func makeTintColorRow() -> NSView {
		let container = NSStackView()
		container.orientation = .vertical
		container.alignment = .leading
		container.spacing = 6

		let titleLabel = NSTextField(labelWithString: "Tint color")
		titleLabel.font = .systemFont(ofSize: 13, weight: .medium)
		container.addArrangedSubview(titleLabel)

		hudTintColorWell.target = self
		hudTintColorWell.action = #selector(hudTintColorChanged)

		let hint = NSTextField(
			labelWithString: "Tint strength applies to Classic and Liquid Glass.")
		hint.font = .systemFont(ofSize: 12)
		hint.textColor = .secondaryLabelColor

		let row = NSStackView(views: [hudTintColorWell, hint])
		row.orientation = .horizontal
		row.alignment = .centerY
		row.spacing = 12
		container.addArrangedSubview(row)
		return container
	}

	private func refreshFromSettings() {
		let settings = settingsStore.settings
		outputDirectoryValueLabel.stringValue = settings.outputDirectory.path
		prefixField.stringValue = settings.outputFilenamePrefix
		namingControl.selectedSegment =
			OutputNamingPreference.allCases.firstIndex(of: settings.outputNaming) ?? 0
		toolbarPlacementControl.selectedSegment =
			ToolbarPlacementPreference.allCases.firstIndex(of: settings.toolbarPlacement) ?? 0
		frozenResizeHandleOrientationControl.selectedSegment =
			FrozenResizeHandleOrientationPreference.allCases.firstIndex(
				of: settings.frozenResizeHandleOrientation) ?? 0
		showAltHintKeycapButton.state = settings.showAltHintKeycap ? .on : .off
		hudGlassEnabledButton.state = settings.hudGlassEnabled ? .on : .off
		hudGlassModeControl.selectedSegment =
			HudGlassModePreference.allCases.firstIndex(of: settings.resolvedHudGlassMode) ?? 0
		for (index, mode) in HudGlassModePreference.allCases.enumerated() {
			let supported =
				mode != .liquidGlass || LiveChromeGlassMaterialSupport.isLiquidGlassAvailable
			hudGlassModeControl.setEnabled(supported, forSegment: index)
		}
		liquidGlassStyleControl.selectedSegment =
			LiquidGlassStylePreference.allCases.firstIndex(of: settings.liquidGlassStyle) ?? 0
		loupeSampleSizeControl.selectedSegment =
			LoupeSampleSizePreference.allCases.firstIndex(of: settings.loupeSampleSize) ?? 0
		hudOpacitySlider.doubleValue = settings.hudOpacity * 100
		hudBlurSlider.doubleValue = settings.hudBlur * 100
		hudTintSlider.doubleValue = settings.hudTint * 100
		hudTintColorWell.color = NSColor(
			calibratedHue: CGFloat(settings.hudTintHue),
			saturation: 0.85,
			brightness: 1,
			alpha: 1
		)
		hudOpacityValueLabel.stringValue = "\(Int(settings.hudOpacity * 100))"
		hudBlurValueLabel.stringValue = "\(Int(settings.hudBlur * 100))"
		hudTintValueLabel.stringValue = "\(Int(settings.hudTint * 100))"
		let glassEnabled = settings.hudGlassEnabled
		let glassMode = settings.resolvedHudGlassMode
		let classicGlassSelected = glassMode == .classicGlass
		let liquidGlassSelected = glassMode == .liquidGlass
		hudGlassModeControl.isEnabled = glassEnabled
		for view in glassTintOptionViews {
			view.isHidden = !glassEnabled
		}
		for view in classicGlassOptionViews {
			view.isHidden = !glassEnabled || !classicGlassSelected
		}
		for view in liquidGlassOptionViews {
			view.isHidden = !glassEnabled || !liquidGlassSelected
		}
		hudOpacitySlider.isEnabled = glassEnabled && classicGlassSelected
		hudBlurSlider.isEnabled = glassEnabled && classicGlassSelected
		hudTintSlider.isEnabled = glassEnabled
		hudTintColorWell.isEnabled = glassEnabled
		liquidGlassStyleControl.isEnabled = glassEnabled && liquidGlassSelected
	}

	@objc
	private func chooseOutputDirectory() {
		let panel = NSOpenPanel()
		panel.canChooseDirectories = true
		panel.canChooseFiles = false
		panel.allowsMultipleSelection = false
		panel.directoryURL = settingsStore.settings.outputDirectory
		if panel.runModal() == .OK, let url = panel.url {
			settingsStore.update { $0.outputDirectory = url }
			refreshFromSettings()
		}
	}

	@objc
	private func prefixChanged() {
		let prefix = prefixField.stringValue
		settingsStore.update { $0.outputFilenamePrefix = prefix }
		refreshFromSettings()
	}

	@objc
	private func namingChanged() {
		let index = namingControl.selectedSegment
		guard OutputNamingPreference.allCases.indices.contains(index) else {
			return
		}
		settingsStore.update { $0.outputNaming = OutputNamingPreference.allCases[index] }
		refreshFromSettings()
	}

	@objc
	private func toolbarPlacementChanged() {
		let index = toolbarPlacementControl.selectedSegment
		guard ToolbarPlacementPreference.allCases.indices.contains(index) else {
			return
		}
		settingsStore.update { $0.toolbarPlacement = ToolbarPlacementPreference.allCases[index] }
		refreshFromSettings()
	}

	@objc
	private func frozenResizeHandleOrientationChanged() {
		let index = frozenResizeHandleOrientationControl.selectedSegment
		guard FrozenResizeHandleOrientationPreference.allCases.indices.contains(index) else {
			return
		}
		settingsStore.update {
			$0.frozenResizeHandleOrientation =
				FrozenResizeHandleOrientationPreference.allCases[index]
		}
		refreshFromSettings()
	}

	@objc
	private func showAltHintKeycapChanged() {
		settingsStore.update { $0.showAltHintKeycap = showAltHintKeycapButton.state == .on }
		refreshFromSettings()
	}

	@objc
	private func hudGlassEnabledChanged() {
		settingsStore.update { $0.hudGlassEnabled = hudGlassEnabledButton.state == .on }
		refreshFromSettings()
	}

	@objc
	private func hudGlassModeChanged() {
		let index = hudGlassModeControl.selectedSegment
		guard HudGlassModePreference.allCases.indices.contains(index) else {
			refreshFromSettings()
			return
		}
		let mode = HudGlassModePreference.allCases[index]
		if mode == .liquidGlass, !LiveChromeGlassMaterialSupport.isLiquidGlassAvailable {
			refreshFromSettings()
			return
		}
		settingsStore.update { $0.hudGlassMode = mode }
		refreshFromSettings()
	}

	@objc
	private func liquidGlassStyleChanged() {
		let index = liquidGlassStyleControl.selectedSegment
		guard LiquidGlassStylePreference.allCases.indices.contains(index) else {
			return
		}
		settingsStore.update {
			$0.liquidGlassStyle = LiquidGlassStylePreference.allCases[index]
		}
		refreshFromSettings()
	}

	@objc
	private func loupeSampleSizeChanged() {
		let index = loupeSampleSizeControl.selectedSegment
		guard LoupeSampleSizePreference.allCases.indices.contains(index) else {
			return
		}
		settingsStore.update { $0.loupeSampleSize = LoupeSampleSizePreference.allCases[index] }
		refreshFromSettings()
	}

	@objc
	private func hudOpacityChanged() {
		settingsStore.update { $0.hudOpacity = hudOpacitySlider.doubleValue / 100.0 }
		refreshFromSettings()
	}

	@objc
	private func hudBlurChanged() {
		settingsStore.update { $0.hudBlur = hudBlurSlider.doubleValue / 100.0 }
		refreshFromSettings()
	}

	@objc
	private func hudTintChanged() {
		settingsStore.update { $0.hudTint = hudTintSlider.doubleValue / 100.0 }
		refreshFromSettings()
	}

	@objc
	private func hudTintColorChanged() {
		let converted = hudTintColorWell.color.usingColorSpace(.deviceRGB) ?? hudTintColorWell.color
		let hue = converted.hueComponent
		settingsStore.update {
			$0.hudTintHue = Double(hue)
		}
		refreshFromSettings()
	}

}

@MainActor
final class PermissionsWindowController: NSWindowController {
	private struct Row {
		let title: String
		let kind: PermissionKind
		let statusLabel: NSTextField
		let actionButton: NSButton
	}

	private let rows: [Row]

	init() {
		let contentRect = NSRect(x: 0, y: 0, width: 520, height: 240)
		let window = NSWindow(
			contentRect: contentRect,
			styleMask: [.titled, .closable, .miniaturizable],
			backing: .buffered,
			defer: false
		)
		window.title = "Permissions"
		window.isReleasedWhenClosed = false

		let screenRecordingStatus = NSTextField(labelWithString: "")
		let accessibilityStatus = NSTextField(labelWithString: "")
		let inputMonitoringStatus = NSTextField(labelWithString: "")

		let screenRecordingButton = NSButton(
			title: "Request / Open Settings", target: nil, action: nil)
		let accessibilityButton = NSButton(
			title: "Request / Open Settings", target: nil, action: nil)
		let inputMonitoringButton = NSButton(
			title: "Request / Open Settings", target: nil, action: nil)

		rows = [
			Row(
				title: "Screen Recording", kind: .screenRecording,
				statusLabel: screenRecordingStatus, actionButton: screenRecordingButton),
			Row(
				title: "Accessibility", kind: .accessibility, statusLabel: accessibilityStatus,
				actionButton: accessibilityButton),
			Row(
				title: "Input Monitoring", kind: .inputMonitoring,
				statusLabel: inputMonitoringStatus, actionButton: inputMonitoringButton),
		]

		super.init(window: window)

		for row in rows {
			row.actionButton.target = self
			row.actionButton.action = #selector(requestPermission(_:))
			row.actionButton.identifier = NSUserInterfaceItemIdentifier(
				rawValue: String(row.kind.rawValue))
		}

		window.contentView = buildContentView()
		window.center()
		refreshStatuses()
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func present() {
		showWindow(nil)
		window?.makeKeyAndOrderFront(nil)
		NSApp.activate(ignoringOtherApps: true)
		refreshStatuses()
	}

	func refreshStatuses() {
		for row in rows {
			let granted = NativePermissions.status(for: row.kind)
			let required = NativePermissions.requiredForCurrentNativeHost(row.kind)
			row.statusLabel.stringValue =
				granted ? "Granted" : (required ? "Required" : "Not needed")
			row.statusLabel.textColor =
				granted
				? NSColor.systemGreen
				: (required ? NSColor.secondaryLabelColor : NSColor.tertiaryLabelColor)
			row.actionButton.isEnabled = !granted && required
			row.actionButton.title = required ? "Request / Open Settings" : "Not used"
		}
	}

	private func buildContentView() -> NSView {
		let root = NSView(frame: .zero)
		root.translatesAutoresizingMaskIntoConstraints = false

		let stack = NSStackView()
		stack.orientation = .vertical
		stack.alignment = .leading
		stack.spacing = 14
		stack.edgeInsets = NSEdgeInsets(top: 20, left: 20, bottom: 20, right: 20)
		stack.translatesAutoresizingMaskIntoConstraints = false

		let intro = NSTextField(
			wrappingLabelWithString:
				"Normal native capture only needs Screen Recording. Accessibility and Input Monitoring are reserved for scroll automation."
		)
		intro.maximumNumberOfLines = 0
		intro.textColor = .secondaryLabelColor
		stack.addArrangedSubview(intro)

		for row in rows {
			let title = NSTextField(labelWithString: row.title)
			title.font = .systemFont(ofSize: 13, weight: .medium)
			let line = NSStackView(views: [title, row.statusLabel, row.actionButton])
			line.orientation = .horizontal
			line.alignment = .centerY
			line.spacing = 12
			stack.addArrangedSubview(line)
		}

		root.addSubview(stack)
		NSLayoutConstraint.activate([
			stack.leadingAnchor.constraint(equalTo: root.leadingAnchor),
			stack.trailingAnchor.constraint(equalTo: root.trailingAnchor),
			stack.topAnchor.constraint(equalTo: root.topAnchor),
			stack.bottomAnchor.constraint(lessThanOrEqualTo: root.bottomAnchor),
		])
		return root
	}

	@objc
	private func requestPermission(_ sender: NSButton) {
		guard
			let identifier = sender.identifier?.rawValue,
			let rawValue = UInt32(identifier),
			let kind = PermissionKind(rawValue: rawValue)
		else {
			return
		}
		guard NativePermissions.requiredForCurrentNativeHost(kind) else {
			refreshStatuses()
			return
		}
		_ = NativePermissions.request(kind)
		refreshStatuses()
	}
}

@MainActor
enum NativePermissions {
	static func requiredForCurrentNativeHost(_ kind: PermissionKind) -> Bool {
		switch kind {
		case .screenRecording:
			return true
		case .accessibility, .inputMonitoring:
			return false
		}
	}

	static func status(for kind: PermissionKind) -> Bool {
		switch kind {
		case .screenRecording:
			return CGPreflightScreenCaptureAccess()
		case .accessibility:
			return AXIsProcessTrusted()
		case .inputMonitoring:
			return CGPreflightListenEventAccess()
		}
	}

	static func request(_ kind: PermissionKind) -> Bool {
		let granted: Bool
		switch kind {
		case .screenRecording:
			granted = CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess()
		case .accessibility:
			let promptKey = "AXTrustedCheckOptionPrompt"
			let options = [promptKey: true] as CFDictionary
			granted = AXIsProcessTrustedWithOptions(options)
		case .inputMonitoring:
			granted = CGPreflightListenEventAccess() || CGRequestListenEventAccess()
		}
		if !granted {
			openSystemSettings(for: kind)
		}
		return granted
	}

	static func openSystemSettings(for kind: PermissionKind) {
		let urlString: String
		switch kind {
		case .screenRecording:
			urlString =
				"x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
		case .accessibility:
			urlString =
				"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
		case .inputMonitoring:
			urlString =
				"x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
		}

		guard let url = URL(string: urlString) else {
			return
		}
		NSWorkspace.shared.open(url)
	}
}
