import CoreGraphics

extension CGRect {
	package func inclusivelyContains(_ point: CGPoint) -> Bool {
		!isNull && !isInfinite
			&& point.x >= minX && point.x <= maxX
			&& point.y >= minY && point.y <= maxY
	}

	package func clampedInclusivePoint(_ point: CGPoint) -> CGPoint? {
		guard inclusivelyContains(point) else {
			return nil
		}
		return CGPoint(
			x: point.x.clamped(to: minX...maxX),
			y: point.y.clamped(to: minY...maxY)
		)
	}

	private func clampedPoint(_ point: CGPoint) -> CGPoint {
		CGPoint(
			x: point.x.clamped(to: minX...maxX),
			y: point.y.clamped(to: minY...maxY)
		)
	}

	package func normalizedRect(anchor: CGPoint, current: CGPoint) -> CGRect {
		let clampedAnchor = clampedPoint(anchor)
		let clampedCurrent = clampedPoint(current)
		return CGRect(
			x: min(clampedAnchor.x, clampedCurrent.x),
			y: min(clampedAnchor.y, clampedCurrent.y),
			width: abs(clampedCurrent.x - clampedAnchor.x),
			height: abs(clampedCurrent.y - clampedAnchor.y)
		)
	}
}

package func captureOverlayLocalPoint(
	from globalPoint: CGPoint,
	windowFrame: CGRect,
	bounds: CGRect
) -> CGPoint? {
	let local = CGPoint(
		x: globalPoint.x - windowFrame.minX,
		y: globalPoint.y - windowFrame.minY
	)
	return bounds.clampedInclusivePoint(local)
}
