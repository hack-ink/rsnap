import AppKit
import CoreGraphics
import CoreText
import Darwin
import Foundation
import RsnapHostBridge

@MainActor
private enum LiveChromeTypography {
	static let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
	static let lineHeight = ceil("x=0".size(using: font).height)
	static let commaWidth = ",".size(using: font).width
	static let keycapTextSize = "Tab".size(using: font)
	static let keycapFrameSize = CGSize(width: keycapTextSize.width + 12, height: keycapTextSize.height + 4)
}

struct LivePositionDisplay: Equatable {
	let xValueText: String
	let yValueText: String
	let xSlotWidth: CGFloat
	let ySlotWidth: CGFloat
}

struct LiveColorDisplay: Equatable {
	let hexText: String
	let hexSlotWidth: CGFloat
}

struct LiveHudVisualSnapshot: Equatable {
	let sourceWindowNumber: Int
	let frame: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let positionDisplay: LivePositionDisplay
	let colorDisplay: LiveColorDisplay
	let rgbSample: RGBSample?
	let keycapVisible: Bool
}

struct LiveLoupeVisualSnapshot {
	let sourceWindowNumber: Int
	let frame: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let patch: CGImage
}

struct FrozenToolbarVisualItemSnapshot: Equatable {
	let kind: ToolbarItemKind
	let frame: CGRect
	let enabled: Bool
	let selected: Bool
}

struct FrozenToolbarVisualSnapshot: Equatable {
	let sourceWindowNumber: Int
	let frame: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let items: [FrozenToolbarVisualItemSnapshot]
}

struct LiveChromeVisualSnapshot {
	let sourceWindowNumber: Int?
	let hud: LiveHudVisualSnapshot?
	let loupe: LiveLoupeVisualSnapshot?
	let toolbar: FrozenToolbarVisualSnapshot?
}

@MainActor
enum PhosphorToolbarIcons {
	private final class BundleProbe {}

	private static let bundleCandidates = [
		"RsnapNativeHost_RsnapNativeHostKit.bundle",
		"RsnapNativeHostKit_RsnapNativeHostKit.bundle",
		"RsnapNativeHostKit.bundle",
	]
	struct CachedGlyph {
		let line: CTLine
		let bounds: CGRect
	}
	private struct GlyphKey: Hashable {
		let kind: ToolbarItemKind
		let selected: Bool
		let sizeX100: Int
	}
	private static var didAttemptRegisterFonts = false
	private static var resolvedFontBundle: Bundle?
	private static var glyphCache: [GlyphKey: CachedGlyph] = [:]

	static func icon(for kind: ToolbarItemKind) -> String {
		switch kind {
		case .pointer:
			return "\u{E1DC}"
		case .pen:
			return "\u{E3B4}"
		case .arrow:
			return "\u{E092}"
		case .text:
			return "\u{E48A}"
		case .mosaic:
			return "\u{E8C4}"
		case .spotlight:
			return "\u{E626}"
		case .undo:
			return "\u{E038}"
		case .redo:
			return "\u{E036}"
		case .autoCenter:
			return "\u{E09C}"
		case .scroll:
			return "\u{E098}"
		case .ocr:
			return "\u{E23A}"
		case .copy:
			return "\u{E1CA}"
		case .save:
			return "\u{E248}"
		}
	}

	static func font(selected: Bool, size: CGFloat) -> NSFont {
		ensureRegistered()
		let name = selected ? "Phosphor-Fill" : "Phosphor"
		return NSFont(name: name, size: size) ?? NSFont.systemFont(ofSize: size, weight: .regular)
	}

	static func cachedGlyph(for kind: ToolbarItemKind, selected: Bool, size: CGFloat) -> CachedGlyph {
		let key = GlyphKey(
			kind: kind,
			selected: selected,
			sizeX100: Int((size * 100).rounded())
		)
		if let cached = glyphCache[key] {
			return cached
		}
		let attributed = NSAttributedString(string: icon(for: kind), attributes: [
			.font: font(selected: selected, size: size),
			NSAttributedString.Key(rawValue: kCTForegroundColorFromContextAttributeName as String): true,
		])
		let line = CTLineCreateWithAttributedString(attributed)
		let glyph = CachedGlyph(
			line: line,
			bounds: CTLineGetBoundsWithOptions(line, [.useOpticalBounds, .excludeTypographicLeading])
		)
		glyphCache[key] = glyph
		return glyph
	}

	private static func ensureRegistered() {
		guard !didAttemptRegisterFonts else {
			return
		}
		didAttemptRegisterFonts = true
		guard let bundle = resolvedFontBundle ?? locateFontBundle() else {
			return
		}
		resolvedFontBundle = bundle
		for resourceName in ["Phosphor", "Phosphor-Fill"] {
			guard
				let url = bundle.url(
					forResource: resourceName,
					withExtension: "ttf",
					subdirectory: nil
				)
			else {
				continue
			}
			CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
		}
	}

	private static func locateFontBundle() -> Bundle? {
		let fileManager = FileManager.default
		let searchRoots: [URL] = [
			Bundle.main.resourceURL,
			Bundle.main.bundleURL.deletingLastPathComponent(),
			Bundle(for: BundleProbe.self).resourceURL,
			Bundle(for: BundleProbe.self).bundleURL.deletingLastPathComponent(),
		].compactMap { $0 }

		for root in searchRoots {
			for candidate in bundleCandidates {
				let bundleURL = root.appendingPathComponent(candidate)
				if let bundle = Bundle(url: bundleURL),
					bundle.url(forResource: "Phosphor", withExtension: "ttf") != nil
				{
					return bundle
				}
			}

			guard let entries = try? fileManager.contentsOfDirectory(
				at: root,
				includingPropertiesForKeys: nil,
				options: [.skipsHiddenFiles]
			) else {
				continue
			}
			for entry in entries where entry.pathExtension == "bundle" {
				if let bundle = Bundle(url: entry),
					bundle.url(forResource: "Phosphor", withExtension: "ttf") != nil
				{
					return bundle
				}
			}
		}

		return nil
	}
}

private enum ChromeVisualKind {
	case hud
	case loupe
	case toolbar
}

@MainActor
private enum MacOSWindowBlurBridge {
	private typealias CGSMainConnectionIDFn = @convention(c) () -> UnsafeMutableRawPointer?
	private typealias CGSSetWindowBackgroundBlurRadiusFn = @convention(c) (UnsafeMutableRawPointer?, Int, Int64) -> Int32

	private static let handle: UnsafeMutableRawPointer? = dlopen(
		"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",
		RTLD_LAZY
	)
	private static let mainConnectionID: CGSMainConnectionIDFn? = {
		guard let handle, let symbol = dlsym(handle, "CGSMainConnectionID") else {
			return nil
		}
		return unsafeBitCast(symbol, to: CGSMainConnectionIDFn.self)
	}()
	private static let setBlurRadius: CGSSetWindowBackgroundBlurRadiusFn? = {
		guard let handle, let symbol = dlsym(handle, "CGSSetWindowBackgroundBlurRadius") else {
			return nil
		}
		return unsafeBitCast(symbol, to: CGSSetWindowBackgroundBlurRadiusFn.self)
	}()

	static func applyBlur(to window: NSWindow, amount: CGFloat) {
		guard
			let mainConnectionID,
			let setBlurRadius,
			let connection = mainConnectionID()
		else {
			return
		}
		let radius = Int64((amount.clamped(to: 0...1) * 12.0).rounded())
		_ = setBlurRadius(connection, window.windowNumber, radius)
	}
}

private final class LiveChromeOverlayWindow: NSWindow {
	let kind: ChromeVisualKind
	let renderView: LiveChromeRenderView
	private var lastPresentedFrame: CGRect?
	private var lastAppliedBlurAmount: CGFloat?
	private var isPresented = false

	init(kind: ChromeVisualKind) {
		self.kind = kind
		self.renderView = LiveChromeRenderView(kind: kind, frame: .zero)
		super.init(
			contentRect: CGRect(x: 0, y: 0, width: 1, height: 1),
			styleMask: [.borderless],
			backing: .buffered,
			defer: false
		)
		contentView = renderView
		backgroundColor = .clear
		collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
		hasShadow = false
		ignoresMouseEvents = true
		isMovable = false
		animationBehavior = .none
		isOpaque = false
		level = NSWindow.Level(rawValue: NSWindow.Level.screenSaver.rawValue + 1)
		sharingType = .none
		titleVisibility = .hidden
		titlebarAppearsTransparent = true
		orderOut(nil)
	}

	override var canBecomeKey: Bool { false }
	override var canBecomeMain: Bool { false }

	func update(frame: CGRect, settings: NativeHostSettings) {
		let roundedFrame = CGRect(
			x: frame.origin.x.rounded(),
			y: frame.origin.y.rounded(),
			width: ceil(frame.width),
			height: ceil(frame.height)
		)
		if let lastPresentedFrame {
			let sizeChanged =
				abs(lastPresentedFrame.width - roundedFrame.width) > 0.5 ||
				abs(lastPresentedFrame.height - roundedFrame.height) > 0.5
			let originChanged =
				abs(lastPresentedFrame.minX - roundedFrame.minX) > 0.5 ||
				abs(lastPresentedFrame.minY - roundedFrame.minY) > 0.5
			if sizeChanged {
				setFrame(roundedFrame, display: false, animate: false)
			} else if originChanged {
				setFrame(roundedFrame, display: false, animate: false)
			}
		} else {
			setFrame(roundedFrame, display: false, animate: false)
		}
		lastPresentedFrame = roundedFrame

		let blurAmount = settings.hudGlassEnabled ? settings.hudBlur : 0
		if lastAppliedBlurAmount == nil || abs((lastAppliedBlurAmount ?? 0) - blurAmount) > 0.01 {
			MacOSWindowBlurBridge.applyBlur(to: self, amount: blurAmount)
			lastAppliedBlurAmount = blurAmount
		}

		if !isPresented {
			orderFrontRegardless()
			isPresented = true
		}
	}

	func hide() {
		guard isPresented else {
			return
		}
		orderOut(nil)
		isPresented = false
		lastPresentedFrame = nil
	}
}

private final class LiveChromeRenderView: NSView {
	private struct LoupeSnapshotKey: Equatable {
		let frame: CGRect
		let theme: CaptureChromeTheme
		let settings: NativeHostSettings
		let patchIdentity: UInt
	}

	private let kind: ChromeVisualKind
	private var hudSnapshot: LiveHudVisualSnapshot?
	private var loupeSnapshot: LiveLoupeVisualSnapshot?
	private var toolbarSnapshot: FrozenToolbarVisualSnapshot?
	private var loupeSnapshotKey: LoupeSnapshotKey?

	init(kind: ChromeVisualKind, frame: CGRect) {
		self.kind = kind
		super.init(frame: frame)
		wantsLayer = true
		layerContentsRedrawPolicy = .duringViewResize
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	override var isOpaque: Bool { false }

	func update(hud snapshot: LiveHudVisualSnapshot?) {
		guard hudSnapshot != snapshot else {
			return
		}
		hudSnapshot = snapshot
		needsDisplay = true
	}

	func update(loupe snapshot: LiveLoupeVisualSnapshot?) {
		let nextKey = snapshot.map {
			LoupeSnapshotKey(
				frame: $0.frame,
				theme: $0.theme,
				settings: $0.settings,
				patchIdentity: UInt(bitPattern: Unmanaged.passUnretained($0.patch).toOpaque())
			)
		}
		guard loupeSnapshotKey != nextKey else {
			return
		}
		loupeSnapshotKey = nextKey
		loupeSnapshot = snapshot
		needsDisplay = true
	}

	func update(toolbar snapshot: FrozenToolbarVisualSnapshot?) {
		guard toolbarSnapshot != snapshot else {
			return
		}
		toolbarSnapshot = snapshot
		needsDisplay = true
	}

	override func draw(_ dirtyRect: NSRect) {
		super.draw(dirtyRect)
		guard let context = NSGraphicsContext.current?.cgContext else {
			return
		}
		switch kind {
		case .hud:
			drawHud(in: context)
		case .loupe:
			drawLoupe(in: context)
		case .toolbar:
			drawToolbar(in: context)
		}
	}

	private func drawHud(in context: CGContext) {
		guard let snapshot = hudSnapshot else {
			return
		}
		let frame = bounds
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		let font = LiveChromeTypography.font
		drawPill(in: frame, context: context, palette: palette, settings: snapshot.settings, strongShadow: true)

		let commaSeparator = ","
		let xGroupText = "x=\(snapshot.positionDisplay.xValueText)"
		let yGroupText = "y=\(snapshot.positionDisplay.yValueText)"
		let positionHeight = LiveChromeTypography.lineHeight
		let itemSpacing: CGFloat = 8
		var cursorX = CaptureChrome.hudInnerMarginX
		let baselineY = (frame.height - positionHeight) / 2

		drawText(xGroupText, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += snapshot.positionDisplay.xSlotWidth
		drawText(commaSeparator, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += LiveChromeTypography.commaWidth
		drawText(yGroupText, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += snapshot.positionDisplay.ySlotWidth + itemSpacing

		let swatchRect = CGRect(x: cursorX, y: frame.midY - 5, width: 10, height: 10)
		let swatchColor = snapshot.rgbSample.map {
			NSColor(calibratedRed: CGFloat($0.r) / 255, green: CGFloat($0.g) / 255, blue: CGFloat($0.b) / 255, alpha: 1)
		} ?? NSColor(calibratedWhite: 1, alpha: 0.12)
		context.setFillColor(swatchColor.cgColor)
		context.fill(swatchRect)
		context.setStrokeColor(palette.swatchStroke.cgColor)
		context.setLineWidth(1)
		context.stroke(swatchRect)
		cursorX += 10 + itemSpacing

		drawText(
			snapshot.colorDisplay.hexText,
			at: CGPoint(x: cursorX, y: baselineY),
			color: palette.labelText,
			font: font
		)
		cursorX += snapshot.colorDisplay.hexSlotWidth + itemSpacing

		if snapshot.keycapVisible {
			let keycapText = "Tab"
			let keycapRect = CGRect(
				x: cursorX,
				y: frame.midY - LiveChromeTypography.keycapFrameSize.height / 2,
				width: LiveChromeTypography.keycapFrameSize.width,
				height: LiveChromeTypography.keycapFrameSize.height
			)
			context.setFillColor(palette.keycapFill.cgColor)
			let keycapPath = NSBezierPath(roundedRect: keycapRect, xRadius: 6, yRadius: 6)
			keycapPath.fill()
			context.setStrokeColor(palette.keycapStroke.cgColor)
			context.setLineWidth(1)
			keycapPath.stroke()
			drawText(
				keycapText,
				at: CGPoint(
					x: keycapRect.midX - LiveChromeTypography.keycapTextSize.width / 2,
					y: keycapRect.midY - LiveChromeTypography.keycapTextSize.height / 2
				),
				color: palette.keycapText,
				font: font
			)
		}
	}

	private func drawLoupe(in context: CGContext) {
		guard let snapshot = loupeSnapshot else {
			return
		}
		let frame = bounds
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		drawPill(in: frame, context: context, palette: palette, settings: snapshot.settings, strongShadow: true)

		let imageRect = frame.insetBy(dx: 10, dy: 10)
		context.saveGState()
		context.interpolationQuality = .none
		context.draw(snapshot.patch, in: imageRect)
		context.restoreGState()

		let centerX = imageRect.minX + floor(CGFloat(snapshot.patch.width) / 2) * CaptureChrome.loupeCellSize
		let centerY = imageRect.minY + floor(CGFloat(snapshot.patch.height) / 2) * CaptureChrome.loupeCellSize
		let centerRect = CGRect(
			x: centerX,
			y: centerY,
			width: CaptureChrome.loupeCellSize,
			height: CaptureChrome.loupeCellSize
		).insetBy(dx: 1, dy: 1)
		context.setStrokeColor(NSColor.white.withAlphaComponent(0.9).cgColor)
		context.setLineWidth(2)
		context.stroke(centerRect)
	}

	private func drawToolbar(in context: CGContext) {
		guard let snapshot = toolbarSnapshot else {
			return
		}
		let frame = bounds
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		drawPill(in: frame, context: context, palette: palette, settings: snapshot.settings, strongShadow: false)

		for item in snapshot.items {
			if item.selected {
				context.setFillColor(palette.toolbarSelectedBackground.cgColor)
				let hoverPath = NSBezierPath(roundedRect: item.frame, xRadius: 8, yRadius: 8)
				hoverPath.fill()
			}

			let symbolColor = item.enabled
				? (item.selected ? palette.toolbarSelectedIcon : palette.toolbarIcon)
				: palette.toolbarDisabledIcon
			drawToolbarGlyph(
				item.kind,
				selected: item.selected,
				in: item.frame,
				color: symbolColor,
				context: context
			)
		}
	}

	private func drawPill(
		in frame: CGRect,
		context: CGContext,
		palette: CaptureChromePalette,
		settings: NativeHostSettings,
		strongShadow: Bool
	) {
		let pillPath = NSBezierPath(
			roundedRect: frame,
			xRadius: CaptureChrome.hudCornerRadius,
			yRadius: CaptureChrome.hudCornerRadius
		)
		context.saveGState()
		if strongShadow {
			context.setShadow(offset: .zero, blur: 10, color: palette.shadow.cgColor)
		}
		context.setFillColor(
			CaptureChrome.effectiveBodyFill(
				palette: palette,
				settings: settings,
				hasGlass: settings.hudGlassEnabled && settings.hudBlur > 0.01
			).cgColor
		)
		pillPath.fill()
		context.restoreGState()

		context.setStrokeColor(palette.outerStroke.cgColor)
		context.setLineWidth(1)
		pillPath.stroke()
	}

	private func drawText(_ text: String, at point: CGPoint, color: NSColor, font: NSFont) {
		(text as NSString).draw(at: point, withAttributes: [
			.font: font,
			.foregroundColor: color,
		])
	}

	private func drawToolbarGlyph(
		_ kind: ToolbarItemKind,
		selected: Bool,
		in rect: CGRect,
		color: NSColor,
		context: CGContext
	) {
		let glyph = PhosphorToolbarIcons.cachedGlyph(for: kind, selected: selected, size: 18)
		let origin = CGPoint(
			x: rect.midX - glyph.bounds.width * 0.5 - glyph.bounds.origin.x,
			y: rect.midY - glyph.bounds.height * 0.5 - glyph.bounds.origin.y
		)
		context.saveGState()
		context.setFillColor(color.cgColor)
		context.textMatrix = .identity
		context.textPosition = origin
		CTLineDraw(glyph.line, context)
		context.restoreGState()
	}
}

@MainActor
final class LiveChromeVisualWindowController {
	private let hudWindow = LiveChromeOverlayWindow(kind: .hud)
	private let loupeWindow = LiveChromeOverlayWindow(kind: .loupe)
	private let toolbarWindow = LiveChromeOverlayWindow(kind: .toolbar)

	func update(snapshot: LiveChromeVisualSnapshot?, focusedWindowNumber: Int?) {
		guard let snapshot else {
			hideAll()
			return
		}
		guard snapshot.sourceWindowNumber == focusedWindowNumber else {
			return
		}

		if let hud = snapshot.hud {
			hudWindow.renderView.update(hud: hud)
			hudWindow.update(frame: hud.frame, settings: hud.settings)
		} else {
			hudWindow.hide()
		}

		if let loupe = snapshot.loupe {
			loupeWindow.renderView.update(loupe: loupe)
			loupeWindow.update(frame: loupe.frame, settings: loupe.settings)
		} else {
			loupeWindow.hide()
		}

		if let toolbar = snapshot.toolbar {
			toolbarWindow.renderView.update(toolbar: toolbar)
			toolbarWindow.update(frame: toolbar.frame, settings: toolbar.settings)
		} else {
			toolbarWindow.hide()
		}
	}

	func hideAll() {
		hudWindow.hide()
		loupeWindow.hide()
		toolbarWindow.hide()
	}

	func hideLiveWindows() {
		hudWindow.hide()
		loupeWindow.hide()
	}
}
