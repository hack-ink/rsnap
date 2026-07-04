import AppKit

extension NSCursor {
	private static func frozenDiagonalCursor(
		from baseCursor: NSCursor
	) -> NSCursor {
		NSCursor(image: baseCursor.image, hotSpot: baseCursor.hotSpot)
	}

	private static var _diagonalTopLeftBottomRight: NSCursor {
		if #available(macOS 15.0, *) {
			return frozenDiagonalCursor(
				from: .frameResize(position: .topLeft, directions: [.inward, .outward])
			)
		}
		return .crosshair
	}

	private static var _diagonalTopRightBottomLeft: NSCursor {
		if #available(macOS 15.0, *) {
			return frozenDiagonalCursor(
				from: .frameResize(position: .topRight, directions: [.inward, .outward])
			)
		}
		return .crosshair
	}

	static var _windowResizeTopRight: NSCursor {
		_diagonalTopRightBottomLeft
	}

	static var _windowResizeTopLeft: NSCursor {
		_diagonalTopLeftBottomRight
	}

	static var _windowResizeBottomLeft: NSCursor {
		_diagonalTopRightBottomLeft
	}

	static var _windowResizeBottomRight: NSCursor {
		_diagonalTopLeftBottomRight
	}
}
