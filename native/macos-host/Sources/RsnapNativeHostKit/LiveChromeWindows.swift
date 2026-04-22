import AppKit
import CoreGraphics
import Darwin
import Foundation
import RsnapHostBridge

struct LiveHudVisualSnapshot {
	let sourceWindowNumber: Int
	let frame: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let positionText: String
	let hexText: String
	let rgbText: String
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

struct LiveStatusVisualSnapshot {
	let sourceWindowNumber: Int
	let frame: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let message: String
}

struct LiveChromeVisualSnapshot {
	let hud: LiveHudVisualSnapshot?
	let loupe: LiveLoupeVisualSnapshot?
	let status: LiveStatusVisualSnapshot?

	var sourceWindowNumber: Int? {
		hud?.sourceWindowNumber ?? loupe?.sourceWindowNumber ?? status?.sourceWindowNumber
	}
}

private enum ChromeVisualKind {
	case hud
	case loupe
	case status
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
				setFrame(roundedFrame, display: false)
			} else if originChanged {
				setFrameOrigin(roundedFrame.origin)
			}
		} else {
			setFrame(roundedFrame, display: false)
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
	private let kind: ChromeVisualKind
	private var hudSnapshot: LiveHudVisualSnapshot?
	private var loupeSnapshot: LiveLoupeVisualSnapshot?
	private var statusSnapshot: LiveStatusVisualSnapshot?

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
		hudSnapshot = snapshot
		needsDisplay = true
	}

	func update(loupe snapshot: LiveLoupeVisualSnapshot?) {
		loupeSnapshot = snapshot
		needsDisplay = true
	}

	func update(status snapshot: LiveStatusVisualSnapshot?) {
		statusSnapshot = snapshot
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
		case .status:
			drawStatus(in: context)
		}
	}

	private func drawHud(in context: CGContext) {
		guard let snapshot = hudSnapshot else {
			return
		}
		let frame = bounds
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		drawPill(in: frame, context: context, palette: palette, settings: snapshot.settings, strongShadow: true)

		let positionSize = snapshot.positionText.size(using: font)
		let bulletSize = "•".size(using: font)
		let hexSize = snapshot.hexText.size(using: font)
		let rgbSize = snapshot.rgbText.size(using: font)
		let itemSpacing: CGFloat = 10
		var cursorX = CaptureChrome.hudInnerMarginX
		let baselineY = (frame.height - positionSize.height) / 2

		drawText(snapshot.positionText, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += positionSize.width + itemSpacing

		drawText("•", at: CGPoint(x: cursorX, y: baselineY), color: palette.secondaryText, font: font)
		cursorX += bulletSize.width + itemSpacing

		let swatchRect = CGRect(x: cursorX, y: frame.midY - 5, width: 10, height: 10)
		let swatchColor = snapshot.rgbSample.map {
			NSColor(calibratedRed: CGFloat($0.r) / 255, green: CGFloat($0.g) / 255, blue: CGFloat($0.b) / 255, alpha: 1)
		} ?? NSColor(calibratedWhite: 1, alpha: 0.12)
		context.setFillColor(swatchColor.cgColor)
		context.fillEllipse(in: swatchRect)
		context.setStrokeColor(palette.swatchStroke.cgColor)
		context.setLineWidth(1)
		context.strokeEllipse(in: swatchRect)
		cursorX += 10 + itemSpacing

		drawText(snapshot.hexText, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += hexSize.width + itemSpacing
		drawText(snapshot.rgbText, at: CGPoint(x: cursorX, y: baselineY), color: palette.secondaryText, font: font)
		cursorX += rgbSize.width + itemSpacing

		if snapshot.keycapVisible {
			let keycapText = "Tab"
			let keycapSize = keycapText.size(using: font)
			let keycapRect = CGRect(
				x: cursorX,
				y: frame.midY - (keycapSize.height + 4) / 2,
				width: keycapSize.width + 12,
				height: keycapSize.height + 4
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
					x: keycapRect.midX - keycapSize.width / 2,
					y: keycapRect.midY - keycapSize.height / 2
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

	private func drawStatus(in context: CGContext) {
		guard let snapshot = statusSnapshot else {
			return
		}
		let frame = bounds
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		let font = NSFont.systemFont(ofSize: 12, weight: .medium)
		drawPill(in: frame, context: context, palette: palette, settings: snapshot.settings, strongShadow: true)
		drawText(
			snapshot.message,
			at: CGPoint(x: CaptureChrome.hudInnerMarginX, y: CaptureChrome.hudInnerMarginY - 1),
			color: palette.labelText,
			font: font
		)
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
		context.setFillColor(palette.bodyFill.cgColor)
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
}

@MainActor
final class LiveChromeVisualWindowController {
	private let hudWindow = LiveChromeOverlayWindow(kind: .hud)
	private let loupeWindow = LiveChromeOverlayWindow(kind: .loupe)
	private let statusWindow = LiveChromeOverlayWindow(kind: .status)

	func update(snapshot: LiveChromeVisualSnapshot?, focusedWindowNumber: Int?) {
		guard let snapshot, snapshot.sourceWindowNumber == focusedWindowNumber else {
			hideAll()
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

		if let status = snapshot.status {
			statusWindow.renderView.update(status: status)
			statusWindow.update(frame: status.frame, settings: status.settings)
		} else {
			statusWindow.hide()
		}
	}

	func hideAll() {
		hudWindow.hide()
		loupeWindow.hide()
		statusWindow.hide()
	}
}
