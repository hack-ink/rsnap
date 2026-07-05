import AppKit
import CoreGraphics
import QuartzCore

final class SelectionFlowBandLayer: CALayer {
	private final class FlowPassLayers {
		let containerLayer = CALayer()
		let gradientLayer = CAGradientLayer()
		let maskLayer = CAShapeLayer()
		let alphaScale: CGFloat

		init(alphaScale: CGFloat) {
			self.alphaScale = alphaScale
		}
	}

	private static let pathOutset: CGFloat = 1.0
	private static let darkLineWidth: CGFloat = 1.8
	private static let lightLineWidth: CGFloat = 1.9
	private static let darkGlowLineWidth: CGFloat = 5.0
	private static let lightGlowLineWidth: CGFloat = 5.25
	private static let flowAnimationKey = "rsnap.selection-flow.rotation"
	private static let flowAnimationDuration: CFTimeInterval = 2.45
	private static let darkPalette: [(CGFloat, CGFloat, CGFloat, CGFloat)] = [
		(112.0 / 255.0, 215.0 / 255.0, 1.0, 0.98),
		(176.0 / 255.0, 154.0 / 255.0, 1.0, 0.94),
		(110.0 / 255.0, 245.0 / 255.0, 215.0 / 255.0, 0.90),
		(65.0 / 255.0, 150.0 / 255.0, 1.0, 0.96),
	]
	private static let lightPalette: [(CGFloat, CGFloat, CGFloat, CGFloat)] = [
		(0.0 / 255.0, 76.0 / 255.0, 196.0 / 255.0, 1.0),
		(83.0 / 255.0, 44.0 / 255.0, 194.0 / 255.0, 0.98),
		(0.0 / 255.0, 113.0 / 255.0, 98.0 / 255.0, 0.98),
		(196.0 / 255.0, 82.0 / 255.0, 0.0 / 255.0, 0.96),
	]

	private let glowPass = FlowPassLayers(alphaScale: 0.24)
	private let linePass = FlowPassLayers(alphaScale: 1.0)
	private let cornerAccentLayer = CAShapeLayer()
	private var focusRect: CGRect = .null
	private var theme: CaptureChromeTheme = .dark
	private var flowAnimating = false

	override init() {
		super.init()
		contentsScale = NSScreen.main?.backingScaleFactor ?? 2
		isOpaque = false
		allowsEdgeAntialiasing = true
		masksToBounds = false
		configureLayers()
	}

	override init(layer: Any) {
		super.init(layer: layer)
		if let layer = layer as? SelectionFlowBandLayer {
			focusRect = layer.focusRect
			theme = layer.theme
			flowAnimating = layer.flowAnimating
		}
		configureLayers()
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func hide() {
		guard isHidden == false || focusRect.isNull == false else {
			return
		}
		isHidden = true
		focusRect = .null
		flowAnimating = false
		removeFlowAnimation()
	}

	func update(
		frame: CGRect,
		focusRect: CGRect,
		theme: CaptureChromeTheme,
		timestamp _: CFTimeInterval,
		contentsScale: CGFloat,
		animates: Bool,
		roundedExclusions _: [OverlayMaskGeometry.RoundedExclusion]
	) {
		let focusChanged = self.focusRect != focusRect
		let themeChanged = self.theme != theme
		let frameChanged = self.frame != frame
		let scaleChanged = self.contentsScale != contentsScale
		let animationChanged = flowAnimating != animates
		let wasHidden = isHidden
		self.frame = frame
		self.contentsScale = contentsScale
		self.focusRect = focusRect
		self.theme = theme
		flowAnimating = animates
		if wasHidden || focusChanged || themeChanged || frameChanged || scaleChanged {
			updateAppearance()
		}
		if animates {
			isHidden = false
			installFlowAnimation(restartsAnimation: wasHidden || animationChanged)
		} else {
			isHidden = true
			removeFlowAnimation()
		}
	}

	func updateRoundedExclusions(_: [OverlayMaskGeometry.RoundedExclusion]) {}

	private func configureLayers() {
		for pass in [glowPass, linePass] {
			pass.containerLayer.masksToBounds = false
			pass.containerLayer.allowsEdgeAntialiasing = true
			pass.containerLayer.addSublayer(pass.gradientLayer)
			pass.containerLayer.mask = pass.maskLayer
			addSublayer(pass.containerLayer)

			pass.gradientLayer.type = .conic
			pass.gradientLayer.startPoint = CGPoint(x: 0.5, y: 0.5)
			pass.gradientLayer.endPoint = CGPoint(x: 1.0, y: 0.5)
			pass.gradientLayer.allowsEdgeAntialiasing = true

			pass.maskLayer.fillColor = NSColor.clear.cgColor
			pass.maskLayer.strokeColor = NSColor.white.cgColor
			pass.maskLayer.lineCap = .butt
			pass.maskLayer.lineJoin = .miter
			pass.maskLayer.allowsEdgeAntialiasing = true
		}
		glowPass.containerLayer.opacity = selectionFlowGlowOpacity()

		cornerAccentLayer.fillColor = NSColor.clear.cgColor
		cornerAccentLayer.lineCap = .butt
		cornerAccentLayer.lineJoin = .miter
		cornerAccentLayer.allowsEdgeAntialiasing = true
		addSublayer(cornerAccentLayer)
	}

	private func updateAppearance() {
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		let strokeRect = focusRect.insetBy(dx: -Self.pathOutset, dy: -Self.pathOutset)
		update(glowPass, strokeRect: strokeRect, lineWidth: selectionFlowGlowLineWidth())
		update(linePass, strokeRect: strokeRect, lineWidth: selectionFlowLineWidth())
		updateCornerAccent(strokeRect: strokeRect)
		CATransaction.commit()
	}

	private func installFlowAnimation(restartsAnimation: Bool) {
		let hasAnimations = linePass.gradientLayer.animation(forKey: Self.flowAnimationKey) != nil
		if restartsAnimation == false, hasAnimations {
			return
		}
		removeFlowAnimation()
		installFlowAnimation(on: linePass.gradientLayer)
	}

	private func installFlowAnimation(on layer: CALayer) {
		let keyPath = "transform.rotation.z"
		let currentRotation =
			(layer.presentation()?.value(forKeyPath: keyPath) as? CGFloat) ?? 0
		let animation = CABasicAnimation(keyPath: keyPath)
		animation.fromValue = currentRotation
		animation.toValue = currentRotation + CGFloat.pi * 2
		animation.duration = Self.flowAnimationDuration
		animation.repeatCount = .infinity
		animation.timingFunction = CAMediaTimingFunction(name: .linear)
		layer.add(animation, forKey: Self.flowAnimationKey)
	}

	private func removeFlowAnimation() {
		for pass in [glowPass, linePass] {
			pass.gradientLayer.removeAnimation(forKey: Self.flowAnimationKey)
		}
	}

	private func update(_ pass: FlowPassLayers, strokeRect: CGRect, lineWidth: CGFloat) {
		let layerBounds = bounds
		pass.containerLayer.frame = layerBounds
		pass.containerLayer.isHidden = layerBounds.width <= 0 || layerBounds.height <= 0
		pass.containerLayer.opacity = pass === glowPass ? selectionFlowGlowOpacity() : 1.0
		pass.gradientLayer.frame = pixelAligned(conicGradientFrame(in: layerBounds))
		pass.gradientLayer.colors = gradientColors(alphaScale: pass.alphaScale)
		pass.gradientLayer.locations = gradientLocations()

		pass.maskLayer.frame = layerBounds
		pass.maskLayer.contentsScale = contentsScale
		pass.maskLayer.lineWidth = lineWidth
		pass.maskLayer.path = NSBezierPath(rect: strokeRect).cgPath
	}

	private func conicGradientFrame(in layerBounds: CGRect) -> CGRect {
		let side = max(hypot(layerBounds.width, layerBounds.height), 1)
		return CGRect(
			x: layerBounds.midX - side / 2,
			y: layerBounds.midY - side / 2,
			width: side,
			height: side
		)
	}

	private func updateCornerAccent(strokeRect: CGRect) {
		cornerAccentLayer.frame = bounds
		cornerAccentLayer.contentsScale = contentsScale
		cornerAccentLayer.lineWidth = selectionFlowLineWidth()
		cornerAccentLayer.opacity = theme == .dark ? 0.86 : 0.72
		cornerAccentLayer.strokeColor = cgColor(
			from: (theme == .dark ? Self.darkPalette[0] : Self.lightPalette[0]),
			alphaScale: 0.90
		)
		cornerAccentLayer.path = selectionFlowCornerAccentPath(for: strokeRect)
	}

	private func selectionFlowCornerAccentPath(for rect: CGRect) -> CGPath {
		let overhang = selectionFlowCornerOverhang()
		let inset = overhang * 1.4
		let path = CGMutablePath()
		path.move(to: CGPoint(x: rect.minX - overhang, y: rect.minY))
		path.addLine(to: CGPoint(x: rect.minX + inset, y: rect.minY))
		path.move(to: CGPoint(x: rect.maxX, y: rect.minY - overhang))
		path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY + inset))
		path.move(to: CGPoint(x: rect.maxX + overhang, y: rect.maxY))
		path.addLine(to: CGPoint(x: rect.maxX - inset, y: rect.maxY))
		path.move(to: CGPoint(x: rect.minX, y: rect.maxY + overhang))
		path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY - inset))
		path.move(to: CGPoint(x: rect.maxX + overhang, y: rect.minY))
		path.addLine(to: CGPoint(x: rect.maxX - inset, y: rect.minY))
		path.move(to: CGPoint(x: rect.maxX, y: rect.maxY + overhang))
		path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY - inset))
		path.move(to: CGPoint(x: rect.minX - overhang, y: rect.maxY))
		path.addLine(to: CGPoint(x: rect.minX + inset, y: rect.maxY))
		path.move(to: CGPoint(x: rect.minX, y: rect.minY - overhang))
		path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + inset))
		return path
	}

	private func gradientColors(alphaScale: CGFloat) -> [CGColor] {
		let palette = theme == .dark ? Self.darkPalette : Self.lightPalette
		var colors = palette.map { cgColor(from: $0, alphaScale: alphaScale) }
		if let first = palette.first {
			colors.append(cgColor(from: first, alphaScale: alphaScale))
		}
		return colors
	}

	private func gradientLocations() -> [NSNumber] {
		let paletteCount = max((theme == .dark ? Self.darkPalette : Self.lightPalette).count, 1)
		return (0...paletteCount).map { index in
			NSNumber(value: Double(index) / Double(paletteCount))
		}
	}

	private func cgColor(
		from color: (CGFloat, CGFloat, CGFloat, CGFloat),
		alphaScale: CGFloat
	) -> CGColor {
		NSColor(
			calibratedRed: color.0,
			green: color.1,
			blue: color.2,
			alpha: min(max(color.3 * alphaScale, 0), 1)
		).cgColor
	}

	private func pixelAligned(_ rect: CGRect) -> CGRect {
		let scale = max(contentsScale, 1)
		return CGRect(
			x: floor(rect.minX * scale) / scale,
			y: floor(rect.minY * scale) / scale,
			width: ceil(rect.width * scale) / scale,
			height: ceil(rect.height * scale) / scale
		)
	}

	private func selectionFlowLineWidth() -> CGFloat {
		theme == .dark ? Self.darkLineWidth : Self.lightLineWidth
	}

	private func selectionFlowGlowLineWidth() -> CGFloat {
		theme == .dark ? Self.darkGlowLineWidth : Self.lightGlowLineWidth
	}

	private func selectionFlowGlowOpacity() -> Float {
		theme == .dark ? 0.30 : 0.34
	}

	private func selectionFlowCornerOverhang() -> CGFloat {
		max(selectionFlowGlowLineWidth() / 2, 3)
	}
}

final class LiveScrimLayer: CAShapeLayer {
	private let exclusionMaskLayer = CAShapeLayer()
	private var renderedBounds = CGRect.null
	private var focusRect = CGRect.null
	private var roundedExclusions: [OverlayMaskGeometry.RoundedExclusion] = []
	var scrimColor: CGColor =
		NSColor(calibratedWhite: 0, alpha: CGFloat(CaptureChrome.liveScrimAlpha)).cgColor

	override init() {
		super.init()
		configureShape()
	}

	override init(layer: Any) {
		if let layer = layer as? LiveScrimLayer {
			renderedBounds = layer.renderedBounds
			focusRect = layer.focusRect
			roundedExclusions = layer.roundedExclusions
			scrimColor = layer.scrimColor
		}
		super.init(layer: layer)
		configureShape()
	}

	private func configureShape() {
		isOpaque = false
		fillRule = .evenOdd
		fillColor = scrimColor
		strokeColor = nil
		needsDisplayOnBoundsChange = false
		exclusionMaskLayer.fillRule = .evenOdd
		exclusionMaskLayer.fillColor = NSColor.black.cgColor
		exclusionMaskLayer.strokeColor = nil
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func update(
		focusRect: CGRect,
		color: CGColor,
		roundedExclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		let currentBounds = bounds
		guard
			renderedBounds != currentBounds
				|| self.focusRect != focusRect
				|| !CFEqual(scrimColor, color)
				|| self.roundedExclusions != roundedExclusions
		else {
			return
		}
		renderedBounds = currentBounds
		self.focusRect = focusRect
		self.scrimColor = color
		self.roundedExclusions = roundedExclusions
		fillColor = color
		path = OverlayMaskGeometry.scrimPath(
			bounds: currentBounds,
			focusRect: focusRect
		)
		updateExclusionMask(bounds: currentBounds, roundedExclusions: roundedExclusions)
	}

	private func updateExclusionMask(
		bounds: CGRect,
		roundedExclusions: [OverlayMaskGeometry.RoundedExclusion]
	) {
		guard roundedExclusions.isEmpty == false else {
			mask = nil
			return
		}
		exclusionMaskLayer.frame = bounds
		exclusionMaskLayer.contentsScale = contentsScale
		exclusionMaskLayer.path = OverlayMaskGeometry.evenOddMaskPath(
			bounds: bounds,
			roundedExclusions: roundedExclusions
		)
		mask = exclusionMaskLayer
	}
}
