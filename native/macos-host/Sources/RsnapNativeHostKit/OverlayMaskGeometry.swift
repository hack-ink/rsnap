import CoreGraphics

package enum OverlayMaskGeometry {
	package struct RoundedExclusion: Equatable {
		package let rect: CGRect
		package let cornerRadius: CGFloat

		package init(rect: CGRect, cornerRadius: CGFloat) {
			self.rect = rect
			self.cornerRadius = cornerRadius
		}

		package func offsetBy(dx: CGFloat, dy: CGFloat) -> Self {
			Self(rect: rect.offsetBy(dx: dx, dy: dy), cornerRadius: cornerRadius)
		}
	}

	package static func drawScrim(
		in context: CGContext,
		bounds: CGRect,
		focusRect: CGRect,
		color: CGColor,
		roundedExclusions: [RoundedExclusion] = [],
		pathExclusions: [CGPath] = []
	) {
		context.saveGState()
		context.setFillColor(color)
		context.clip(to: bounds)
		context.addPath(
			scrimPath(
				bounds: bounds,
				focusRect: focusRect
			)
		)
		context.fillPath(using: .evenOdd)
		context.setBlendMode(.clear)
		for exclusion in roundedExclusions {
			clearRoundedRect(exclusion, in: context)
		}
		for path in pathExclusions {
			context.addPath(path)
			context.fillPath()
		}
		context.restoreGState()
	}

	package static func scrimPath(
		bounds: CGRect,
		focusRect: CGRect,
		roundedExclusions: [RoundedExclusion] = [],
		pathExclusions: [CGPath] = []
	) -> CGPath {
		let path = CGMutablePath()
		guard bounds.isRenderableMaskRect else {
			return path
		}
		path.addRect(bounds)
		if focusRect.isRenderableMaskRect {
			path.addRect(focusRect)
		}
		for exclusion in roundedExclusions {
			if let exclusionPath = roundedPath(for: exclusion) {
				path.addPath(exclusionPath)
			}
		}
		for exclusionPath in pathExclusions {
			path.addPath(exclusionPath)
		}
		return path
	}

	package static func evenOddMaskPath(
		bounds: CGRect,
		roundedExclusions: [RoundedExclusion]
	) -> CGPath {
		scrimPath(
			bounds: bounds,
			focusRect: .null,
			roundedExclusions: roundedExclusions
		)
	}

	private static func roundedPath(for exclusion: RoundedExclusion) -> CGPath? {
		guard exclusion.rect.isRenderableMaskRect else {
			return nil
		}
		let radius = min(
			max(0, exclusion.cornerRadius),
			exclusion.rect.width / 2,
			exclusion.rect.height / 2
		)
		return CGPath(
			roundedRect: exclusion.rect,
			cornerWidth: radius,
			cornerHeight: radius,
			transform: nil
		)
	}

	private static func clearRoundedRect(
		_ exclusion: RoundedExclusion,
		in context: CGContext
	) {
		guard let path = roundedPath(for: exclusion) else {
			return
		}
		context.addPath(path)
		context.fillPath()
	}
}

extension CGRect {
	fileprivate var isRenderableMaskRect: Bool {
		!isNull && !isInfinite && width > 0 && height > 0
	}
}
