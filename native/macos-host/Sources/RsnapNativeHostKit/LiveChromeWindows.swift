import AppKit
import CoreGraphics
import Darwin
import Foundation
import RsnapHostBridge

struct LivePositionDisplay {
	let xValueText: String
	let yValueText: String
	let xSlotWidth: CGFloat
	let ySlotWidth: CGFloat
}

enum LiveRGBValueDisplay {
	case sample(rText: String, gText: String, bText: String, componentSlotWidth: CGFloat)
	case placeholder(text: String)
}

struct LiveColorDisplay {
	let hexText: String
	let hexSlotWidth: CGFloat
	let rgbValueDisplay: LiveRGBValueDisplay
}

struct LiveHudVisualSnapshot {
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

struct LiveStatusVisualSnapshot {
	let sourceWindowNumber: Int
	let frame: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let message: String
}

struct FrozenToolbarVisualItemSnapshot {
	let kind: ToolbarItemKind
	let frame: CGRect
	let enabled: Bool
	let selected: Bool
}

struct FrozenToolbarVisualSnapshot {
	let sourceWindowNumber: Int
	let frame: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let items: [FrozenToolbarVisualItemSnapshot]
}

struct LiveChromeVisualSnapshot {
	let hud: LiveHudVisualSnapshot?
	let loupe: LiveLoupeVisualSnapshot?
	let status: LiveStatusVisualSnapshot?
	let toolbar: FrozenToolbarVisualSnapshot?

	var sourceWindowNumber: Int? {
		hud?.sourceWindowNumber
			?? loupe?.sourceWindowNumber
			?? status?.sourceWindowNumber
			?? toolbar?.sourceWindowNumber
	}
}

private enum ChromeVisualKind {
	case hud
	case loupe
	case status
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
	private let kind: ChromeVisualKind
	private var hudSnapshot: LiveHudVisualSnapshot?
	private var loupeSnapshot: LiveLoupeVisualSnapshot?
	private var statusSnapshot: LiveStatusVisualSnapshot?
	private var toolbarSnapshot: FrozenToolbarVisualSnapshot?

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

	func update(toolbar snapshot: FrozenToolbarVisualSnapshot?) {
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
		case .status:
			drawStatus(in: context)
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
		let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
		drawPill(in: frame, context: context, palette: palette, settings: snapshot.settings, strongShadow: true)

		let commaSeparator = ","
		let xGroupText = "x=\(snapshot.positionDisplay.xValueText)"
		let yGroupText = "y=\(snapshot.positionDisplay.yValueText)"
		let positionHeight = max(
			xGroupText.size(using: font).height,
			yGroupText.size(using: font).height
		)
		let itemSpacing: CGFloat = 8
		var cursorX = CaptureChrome.hudInnerMarginX
		let baselineY = (frame.height - positionHeight) / 2

		drawText(xGroupText, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += snapshot.positionDisplay.xSlotWidth
		drawText(commaSeparator, at: CGPoint(x: cursorX, y: baselineY), color: palette.labelText, font: font)
		cursorX += commaSeparator.size(using: font).width
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
			drawToolbarGlyph(item.kind, in: item.frame, color: symbolColor, context: context)
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

	private func drawToolbarGlyph(
		_ kind: ToolbarItemKind,
		in rect: CGRect,
		color: NSColor,
		context: CGContext
	) {
		context.saveGState()
		context.setStrokeColor(color.cgColor)
		context.setFillColor(color.cgColor)
		context.setLineWidth(1.7)
		context.setLineCap(.round)
		context.setLineJoin(.round)

		let insetRect = rect.insetBy(dx: 5.5, dy: 5.5)
		switch kind {
		case .pointer:
			let path = NSBezierPath()
			path.move(to: CGPoint(x: insetRect.minX, y: insetRect.minY))
			path.line(to: CGPoint(x: insetRect.maxX - 2, y: insetRect.midY - 1))
			path.line(to: CGPoint(x: insetRect.midX + 0.5, y: insetRect.midY + 0.5))
			path.line(to: CGPoint(x: insetRect.maxX, y: insetRect.maxY))
			path.lineWidth = 1.6
			path.stroke()
		case .pen:
			context.move(to: CGPoint(x: insetRect.minX + 1, y: insetRect.minY + 1))
			context.addLine(to: CGPoint(x: insetRect.maxX - 2, y: insetRect.maxY - 2))
			context.strokePath()
			context.fillEllipse(in: CGRect(x: insetRect.maxX - 3.5, y: insetRect.maxY - 3.5, width: 3, height: 3))
		case .arrow:
			drawArrow(
				from: CGPoint(x: insetRect.minX, y: insetRect.minY + 1),
				to: CGPoint(x: insetRect.maxX, y: insetRect.maxY),
				in: context
			)
		case .text:
			let font = NSFont.systemFont(ofSize: 13, weight: .semibold)
			drawText("T", at: CGPoint(x: rect.midX - 4, y: rect.midY - 7), color: color, font: font)
		case .mosaic:
			let size = insetRect.width / 3
			for row in 0..<3 {
				for column in 0..<3 {
					if (row + column).isMultiple(of: 2) {
						let cell = CGRect(
							x: insetRect.minX + CGFloat(column) * size,
							y: insetRect.minY + CGFloat(row) * size,
							width: size - 1,
							height: size - 1
						)
						context.fill(cell)
					}
				}
			}
		case .spotlight:
			context.strokeEllipse(in: insetRect)
			context.move(to: CGPoint(x: insetRect.maxX - 1, y: insetRect.minY + 2))
			context.addLine(to: CGPoint(x: insetRect.maxX + 3, y: insetRect.minY - 2))
			context.strokePath()
		case .undo:
			context.move(to: CGPoint(x: insetRect.maxX, y: insetRect.midY))
			context.addQuadCurve(to: CGPoint(x: insetRect.minX + 3, y: insetRect.maxY - 1), control: CGPoint(x: insetRect.midX, y: insetRect.maxY + 2))
			context.strokePath()
			context.move(to: CGPoint(x: insetRect.minX + 3, y: insetRect.maxY - 1))
			context.addLine(to: CGPoint(x: insetRect.minX + 2, y: insetRect.maxY - 5))
			context.addLine(to: CGPoint(x: insetRect.minX + 6, y: insetRect.maxY - 3))
			context.strokePath()
		case .redo:
			context.move(to: CGPoint(x: insetRect.minX, y: insetRect.midY))
			context.addQuadCurve(to: CGPoint(x: insetRect.maxX - 3, y: insetRect.maxY - 1), control: CGPoint(x: insetRect.midX, y: insetRect.maxY + 2))
			context.strokePath()
			context.move(to: CGPoint(x: insetRect.maxX - 3, y: insetRect.maxY - 1))
			context.addLine(to: CGPoint(x: insetRect.maxX - 6, y: insetRect.maxY - 3))
			context.addLine(to: CGPoint(x: insetRect.maxX - 2, y: insetRect.maxY - 5))
			context.strokePath()
		case .autoCenter:
			context.stroke(CGRect(x: insetRect.minX + 1, y: insetRect.minY + 1, width: insetRect.width - 2, height: insetRect.height - 2))
			context.fillEllipse(in: CGRect(x: insetRect.midX - 1.6, y: insetRect.midY - 1.6, width: 3.2, height: 3.2))
		case .scroll:
			context.move(to: CGPoint(x: insetRect.midX, y: insetRect.maxY))
			context.addLine(to: CGPoint(x: insetRect.midX, y: insetRect.minY + 2))
			context.strokePath()
			context.move(to: CGPoint(x: insetRect.midX - 3, y: insetRect.maxY - 3))
			context.addLine(to: CGPoint(x: insetRect.midX, y: insetRect.maxY))
			context.addLine(to: CGPoint(x: insetRect.midX + 3, y: insetRect.maxY - 3))
			context.strokePath()
			context.move(to: CGPoint(x: insetRect.midX - 3, y: insetRect.minY + 5))
			context.addLine(to: CGPoint(x: insetRect.midX, y: insetRect.minY + 2))
			context.addLine(to: CGPoint(x: insetRect.midX + 3, y: insetRect.minY + 5))
			context.strokePath()
		case .ocr:
			let font = NSFont.monospacedSystemFont(ofSize: 11, weight: .semibold)
			drawText("OCR", at: CGPoint(x: rect.midX - 9, y: rect.midY - 6), color: color, font: font)
		case .copy:
			context.stroke(CGRect(x: insetRect.minX + 2, y: insetRect.minY + 1, width: insetRect.width - 4, height: insetRect.height - 4))
			context.stroke(CGRect(x: insetRect.minX + 5, y: insetRect.minY + 4, width: insetRect.width - 4, height: insetRect.height - 4))
		case .save:
			context.stroke(CGRect(x: insetRect.minX + 1, y: insetRect.minY + 1, width: insetRect.width - 2, height: insetRect.height - 2))
			context.fill(CGRect(x: insetRect.minX + 3, y: insetRect.maxY - 5, width: insetRect.width - 6, height: 3))
			context.stroke(CGRect(x: insetRect.midX - 3, y: insetRect.minY + 3, width: 6, height: 4))
		}

		context.restoreGState()
	}

	private func drawArrow(from start: CGPoint, to end: CGPoint, in context: CGContext) {
		context.beginPath()
		context.move(to: start)
		context.addLine(to: end)
		context.strokePath()

		let angle = atan2(end.y - start.y, end.x - start.x)
		let headLength: CGFloat = 10
		let headSpread: CGFloat = .pi / 7
		let left = CGPoint(
			x: end.x - cos(angle - headSpread) * headLength,
			y: end.y - sin(angle - headSpread) * headLength
		)
		let right = CGPoint(
			x: end.x - cos(angle + headSpread) * headLength,
			y: end.y - sin(angle + headSpread) * headLength
		)
		context.beginPath()
		context.move(to: end)
		context.addLine(to: left)
		context.move(to: end)
		context.addLine(to: right)
		context.strokePath()
	}
}

@MainActor
final class LiveChromeVisualWindowController {
	private let hudWindow = LiveChromeOverlayWindow(kind: .hud)
	private let loupeWindow = LiveChromeOverlayWindow(kind: .loupe)
	private let statusWindow = LiveChromeOverlayWindow(kind: .status)
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

		if let status = snapshot.status {
			statusWindow.renderView.update(status: status)
			statusWindow.update(frame: status.frame, settings: status.settings)
		} else {
			statusWindow.hide()
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
		statusWindow.hide()
		toolbarWindow.hide()
	}

	func hideLiveWindows() {
		hudWindow.hide()
		loupeWindow.hide()
		statusWindow.hide()
	}
}
