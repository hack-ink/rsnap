import AppKit
import CoreGraphics
import RsnapHostBridge

package enum CaptureHostCursorPresentation: Equatable {
	case arrow
	case crosshair
	case openHand
	case closedHand
	case resizeUpDown
	case resizeLeftRight
	case resizeTopLeft
	case resizeTopRight
	case resizeBottomLeft
	case resizeBottomRight
	case iBeam
}

package enum CaptureHostCursorSupport {
	package static func presentation(for intent: CursorIntent) -> CaptureHostCursorPresentation {
		switch intent {
		case .default:
			return .arrow
		case .crosshair:
			return .crosshair
		case .grab:
			return .openHand
		case .grabbing:
			return .closedHand
		case .resizeNorth, .resizeSouth:
			return .resizeUpDown
		case .resizeEast, .resizeWest:
			return .resizeLeftRight
		case .resizeNorthEast:
			return .resizeTopRight
		case .resizeNorthWest:
			return .resizeTopLeft
		case .resizeSouthEast:
			return .resizeBottomRight
		case .resizeSouthWest:
			return .resizeBottomLeft
		case .text:
			return .iBeam
		}
	}

	package static func cursorIntent(
		for interactionKind: FrozenSelectionTransformKind,
		active: Bool
	) -> CursorIntent {
		switch interactionKind {
		case .move:
			return active ? .grabbing : .grab
		case .resizeLeft:
			return .resizeWest
		case .resizeRight:
			return .resizeEast
		case .resizeTop:
			return .resizeNorth
		case .resizeBottom:
			return .resizeSouth
		case .resizeTopLeft:
			return .resizeNorthWest
		case .resizeTopRight:
			return .resizeNorthEast
		case .resizeBottomLeft:
			return .resizeSouthWest
		case .resizeBottomRight:
			return .resizeSouthEast
		}
	}

	package static func editableFrozenCursorIntent(
		at point: CGPoint,
		selection: CGRect
	) -> CursorIntent? {
		guard
			let kind = try? RsnapFrozenSelectionTransformPlanner.hitTest(
				point: point,
				selection: selection,
				handleRadius: 12,
				edgeTolerance: 4
			)
		else {
			return nil
		}
		return cursorIntent(for: kind, active: false)
	}

	static func cursor(for presentation: CaptureHostCursorPresentation) -> NSCursor {
		switch presentation {
		case .arrow:
			return .arrow
		case .crosshair:
			return .crosshair
		case .openHand:
			return .openHand
		case .closedHand:
			return .closedHand
		case .resizeUpDown:
			return .resizeUpDown
		case .resizeLeftRight:
			return .resizeLeftRight
		case .resizeTopLeft:
			return ._windowResizeTopLeft
		case .resizeTopRight:
			return ._windowResizeTopRight
		case .resizeBottomLeft:
			return ._windowResizeBottomLeft
		case .resizeBottomRight:
			return ._windowResizeBottomRight
		case .iBeam:
			return .iBeam
		}
	}
}

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
