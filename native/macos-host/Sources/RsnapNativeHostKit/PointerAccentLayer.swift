import AppKit
import QuartzCore

final class PointerAccentLayer: CALayer {
	private static let diameter: CGFloat = 28

	private let contrastLayer = CALayer()
	private let glowLayer = CALayer()
	private let coreLayer = CALayer()

	override init() {
		super.init()
		masksToBounds = false
		allowsEdgeAntialiasing = true
		configureLayers()
		hide()
	}

	override init(layer: Any) {
		super.init(layer: layer)
		configureLayers()
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func update(pointer: CGPoint?, in bounds: CGRect, contentsScale scale: CGFloat) {
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		defer { CATransaction.commit() }

		guard let pointer, bounds.contains(pointer) else {
			hide()
			return
		}

		let diameter = Self.diameter
		let layerBounds = CGRect(origin: .zero, size: CGSize(width: diameter, height: diameter))
		let center = CGPoint(x: diameter / 2, y: diameter / 2)
		frame = layerBounds.offsetBy(dx: pointer.x - center.x, dy: pointer.y - center.y)
		updateLayerScales(scale)
		contrastLayer.isHidden = false
		glowLayer.isHidden = false
		coreLayer.isHidden = false
		isHidden = false
	}

	func hide() {
		isHidden = true
		contrastLayer.isHidden = true
		glowLayer.isHidden = true
		coreLayer.isHidden = true
	}

	private func configureLayers() {
		let contrastSize: CGFloat = 13
		contrastLayer.frame = centeredRect(size: contrastSize)
		contrastLayer.cornerRadius = contrastSize / 2
		contrastLayer.backgroundColor = NSColor.black.withAlphaComponent(0.2).cgColor
		contrastLayer.shadowColor = NSColor.black.cgColor
		contrastLayer.shadowOpacity = 0.36
		contrastLayer.shadowRadius = 3.5
		contrastLayer.shadowOffset = .zero
		contrastLayer.shadowPath = CGPath(ellipseIn: contrastLayer.bounds, transform: nil)
		addSublayer(contrastLayer)

		let glowSize: CGFloat = 10
		glowLayer.frame = centeredRect(size: glowSize)
		glowLayer.cornerRadius = glowSize / 2
		glowLayer.backgroundColor =
			NSColor(calibratedRed: 82 / 255, green: 226 / 255, blue: 1, alpha: 0.3).cgColor
		glowLayer.shadowColor =
			NSColor(calibratedRed: 82 / 255, green: 226 / 255, blue: 1, alpha: 1).cgColor
		glowLayer.shadowOpacity = 0.88
		glowLayer.shadowRadius = 7
		glowLayer.shadowOffset = .zero
		glowLayer.shadowPath = CGPath(ellipseIn: glowLayer.bounds, transform: nil)
		addSublayer(glowLayer)

		let coreSize: CGFloat = 3.2
		coreLayer.frame = centeredRect(size: coreSize)
		coreLayer.cornerRadius = coreSize / 2
		coreLayer.backgroundColor =
			NSColor(calibratedRed: 232 / 255, green: 253 / 255, blue: 1, alpha: 0.95).cgColor
		addSublayer(coreLayer)

		for layer in [contrastLayer, glowLayer, coreLayer] {
			layer.allowsEdgeAntialiasing = true
		}
	}

	private func updateLayerScales(_ scale: CGFloat) {
		for layer in [contrastLayer, glowLayer, coreLayer] {
			layer.contentsScale = scale
			layer.rasterizationScale = scale
		}
	}

	private func centeredRect(size: CGFloat) -> CGRect {
		CGRect(
			x: (Self.diameter - size) / 2,
			y: (Self.diameter - size) / 2,
			width: size,
			height: size
		)
	}
}
