import AppKit
import CoreGraphics
import CoreText
import Darwin
import Foundation
import RsnapHostBridge
import SwiftUI

struct LivePositionDisplay: Equatable {
	let xValueText: String
	let yValueText: String
	let xSlotWidth: CGFloat
	let ySlotWidth: CGFloat
}

struct LiveColorDisplay: Equatable {
	let hexText: String
	let hexSlotWidth: CGFloat
	let isPending: Bool
}

struct LiveChromeBackdropSnapshot {
	let sourceWindowNumber: Int?
	let hudFrame: CGRect?
	let loupeFrame: CGRect?
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
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

	static func cachedGlyph(for kind: ToolbarItemKind, selected: Bool, size: CGFloat) -> CachedGlyph
	{
		let key = GlyphKey(
			kind: kind,
			selected: selected,
			sizeX100: Int((size * 100).rounded())
		)
		if let cached = glyphCache[key] {
			return cached
		}
		let attributed = NSAttributedString(
			string: icon(for: kind),
			attributes: [
				.font: font(selected: selected, size: size),
				NSAttributedString.Key(
					rawValue: kCTForegroundColorFromContextAttributeName as String): true,
			])
		let line = CTLineCreateWithAttributedString(attributed)
		let glyph = CachedGlyph(
			line: line,
			bounds: CTLineGetBoundsWithOptions(
				line, [.useOpticalBounds, .excludeTypographicLeading])
		)
		glyphCache[key] = glyph
		return glyph
	}

	private static func ensureRegistered() {
		guard didAttemptRegisterFonts == false else {
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

			guard
				let entries = try? fileManager.contentsOfDirectory(
					at: root,
					includingPropertiesForKeys: nil,
					options: [.skipsHiddenFiles]
				)
			else {
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

@MainActor
private enum MacOSWindowBlurBridge {
	private typealias CGSMainConnectionIDFn = @convention(c) () -> UnsafeMutableRawPointer?
	private typealias CGSSetWindowBackgroundBlurRadiusFn =
		@convention(c) (UnsafeMutableRawPointer?, Int, Int64) -> Int32

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

@MainActor
enum LiveChromeLiquidGlassBridge {
	static func makeGlassView() -> NSView? {
		guard LiveChromeGlassMaterialSupport.isLiquidGlassAvailable else {
			return nil
		}
		let glassView = LiveChromeLiquidGlassView(frame: .zero)
		glassView.autoresizingMask = [.width, .height]
		return glassView
	}

	static func update(_ glassView: NSView, settings: NativeHostSettings) {
		guard let glassView = glassView as? LiveChromeLiquidGlassView else {
			return
		}
		glassView.update(settings: settings)
	}
}

@MainActor
final class LiveChromeLiquidGlassView: NSView {
	private let glassHostView: NSHostingView<AnyView>
	private var currentSettings: NativeHostSettings?

	override var isOpaque: Bool { false }

	override func hitTest(_ point: NSPoint) -> NSView? {
		nil
	}

	override init(frame frameRect: NSRect) {
		self.glassHostView = NSHostingView(
			rootView: Self.makeGlassRoot(settings: .defaults))
		super.init(frame: frameRect)

		wantsLayer = true
		layer?.backgroundColor = NSColor.clear.cgColor
		layer?.isOpaque = false

		glassHostView.frame = bounds
		glassHostView.autoresizingMask = [.width, .height]
		glassHostView.wantsLayer = true
		glassHostView.layer?.backgroundColor = NSColor.clear.cgColor
		glassHostView.layer?.isOpaque = false
		addSubview(glassHostView)
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func update(settings: NativeHostSettings) {
		guard currentSettings != settings else {
			return
		}
		currentSettings = settings
		glassHostView.rootView = Self.makeGlassRoot(settings: settings)
	}

	private static func makeGlassRoot(settings: NativeHostSettings) -> AnyView {
		#if compiler(>=6.2)
			if #available(macOS 26.0, *) {
				return makeAvailableGlassRoot(settings: settings)
			}
		#endif
		return AnyView(Color.clear)
	}

	#if compiler(>=6.2)
		@available(macOS 26.0, *)
		private static func makeAvailableGlassRoot(settings: NativeHostSettings) -> AnyView {
			var glass =
				switch settings.liquidGlassStyle {
				case .regular:
					Glass.regular
				case .clear:
					Glass.clear
				}
			glass = glass.tint(liquidGlassTint(settings: settings)).interactive(false)
			return AnyView(
				GlassEffectContainer(spacing: 0) {
					ZStack {
						Color.clear
							.frame(maxWidth: .infinity, maxHeight: .infinity)
							.glassEffect(
								glass, in: .rect(cornerRadius: CaptureChrome.hudCornerRadius))
						if let fill = liquidGlassTintFill(settings: settings) {
							RoundedRectangle(
								cornerRadius: CaptureChrome.hudCornerRadius,
								style: .continuous
							)
							.fill(fill)
							.allowsHitTesting(false)
						}
					}
					.frame(maxWidth: .infinity, maxHeight: .infinity)
				}
				.allowsHitTesting(false)
			)
		}

		@available(macOS 26.0, *)
		private static func liquidGlassTint(settings: NativeHostSettings) -> Color? {
			let strength = settings.hudTint.clamped(to: 0...1)
			guard strength > 0 else {
				return nil
			}
			let maximumOpacity =
				switch settings.liquidGlassStyle {
				case .regular:
					0.12
				case .clear:
					0.38
				}
			return Color(
				hue: settings.hudTintHue.clamped(to: 0...1),
				saturation: settings.hudTintSaturation.clamped(to: 0...1),
				brightness: settings.hudTintBrightness.clamped(to: 0...1),
				opacity: strength * maximumOpacity
			)
		}

		@available(macOS 26.0, *)
		private static func liquidGlassTintFill(settings: NativeHostSettings) -> Color? {
			guard settings.liquidGlassStyle == .clear else {
				return nil
			}
			let strength = settings.hudTint.clamped(to: 0...1)
			guard strength > 0 else {
				return nil
			}
			return Color(
				hue: settings.hudTintHue.clamped(to: 0...1),
				saturation: settings.hudTintSaturation.clamped(to: 0...1),
				brightness: settings.hudTintBrightness.clamped(to: 0...1),
				opacity: strength * 0.22
			)
		}
	#endif
}

private final class LiveChromeBackdropWindow: NSWindow {
	let renderView: LiveChromeBackdropView
	private var lastPresentedFrame: CGRect?
	private var lastAppliedBlurAmount: CGFloat?
	private var isPresented = false

	init() {
		self.renderView = LiveChromeBackdropView(frame: .zero)
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
		level = NSWindow.Level(rawValue: NSWindow.Level.screenSaver.rawValue - 1)
		sharingType = .none
		titleVisibility = .hidden
		titlebarAppearsTransparent = true
		orderOut(nil)
	}

	override var canBecomeKey: Bool { false }
	override var canBecomeMain: Bool { false }

	private var presentationScale: CGFloat {
		max(
			backingScaleFactor,
			screen?.backingScaleFactor ?? 0,
			NSScreen.main?.backingScaleFactor ?? 1,
			1
		)
	}

	private func presentationFrame(from frame: CGRect) -> CGRect {
		let scale = presentationScale
		return CGRect(
			x: (frame.origin.x * scale).rounded() / scale,
			y: (frame.origin.y * scale).rounded() / scale,
			width: (frame.width * scale).rounded(.up) / scale,
			height: (frame.height * scale).rounded(.up) / scale
		)
	}

	func update(frame: CGRect, theme: CaptureChromeTheme, settings: NativeHostSettings) {
		let roundedFrame = presentationFrame(from: frame)
		let tolerance: CGFloat = 0.001
		if let lastPresentedFrame {
			let sizeChanged =
				abs(lastPresentedFrame.width - roundedFrame.width) > tolerance
				|| abs(lastPresentedFrame.height - roundedFrame.height) > tolerance
			let originChanged =
				abs(lastPresentedFrame.minX - roundedFrame.minX) > tolerance
				|| abs(lastPresentedFrame.minY - roundedFrame.minY) > tolerance
			if sizeChanged {
				setFrame(roundedFrame, display: false, animate: false)
			} else if originChanged {
				setFrameOrigin(roundedFrame.origin)
			}
		} else {
			setFrame(roundedFrame, display: false, animate: false)
		}
		lastPresentedFrame = roundedFrame

		let blurAmount = settings.usesClassicHudGlass ? settings.hudBlur : 0
		if lastAppliedBlurAmount == nil || abs((lastAppliedBlurAmount ?? 0) - blurAmount) > 0.01 {
			MacOSWindowBlurBridge.applyBlur(to: self, amount: blurAmount)
			lastAppliedBlurAmount = blurAmount
		}

		renderView.update(theme: theme, settings: settings)
		if isPresented == false {
			orderFrontRegardless()
			isPresented = true
		}
		if alphaValue != 1 {
			alphaValue = 1
		}
	}

	func hide() {
		guard isPresented else {
			return
		}
		orderOut(nil)
		isPresented = false
		lastPresentedFrame = nil
		alphaValue = 1
	}
}

private final class LiveChromeBackdropView: NSView {
	private var theme: CaptureChromeTheme = .dark
	private var settings = NativeHostSettings.defaults

	override var isOpaque: Bool { false }

	func update(theme: CaptureChromeTheme, settings: NativeHostSettings) {
		let changed = self.theme != theme || self.settings != settings
		self.theme = theme
		self.settings = settings
		if changed {
			needsDisplay = true
		}
	}

	override func draw(_ dirtyRect: NSRect) {
		super.draw(dirtyRect)
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let pillPath = NSBezierPath(
			roundedRect: bounds,
			xRadius: CaptureChrome.hudCornerRadius,
			yRadius: CaptureChrome.hudCornerRadius
		)
		guard let context = NSGraphicsContext.current?.cgContext else {
			return
		}
		context.saveGState()
		context.setShadow(offset: .zero, blur: 10, color: palette.shadow.cgColor)
		context.setFillColor(
			CaptureChrome.effectiveBodyFill(
				palette: palette,
				settings: settings,
				hasGlass: true
			).cgColor
		)
		pillPath.fill()
		context.restoreGState()
	}
}

@MainActor
final class LiveChromeBackdropWindowController {
	private let hudWindow = LiveChromeBackdropWindow()
	private let loupeWindow = LiveChromeBackdropWindow()

	func update(snapshot: LiveChromeBackdropSnapshot?, focusedWindowNumber: Int?) {
		guard let snapshot else {
			hideAll()
			return
		}
		guard snapshot.sourceWindowNumber == focusedWindowNumber else {
			return
		}
		guard snapshot.settings.usesClassicHudGlass else {
			hideAll()
			return
		}

		if let hudFrame = snapshot.hudFrame {
			hudWindow.update(frame: hudFrame, theme: snapshot.theme, settings: snapshot.settings)
		} else {
			hudWindow.hide()
		}

		if let loupeFrame = snapshot.loupeFrame {
			loupeWindow.update(
				frame: loupeFrame, theme: snapshot.theme, settings: snapshot.settings)
		} else {
			loupeWindow.hide()
		}
	}

	func hideAll() {
		hudWindow.hide()
		loupeWindow.hide()
	}
}
