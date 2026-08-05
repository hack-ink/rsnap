import CoreGraphics
import Foundation

package enum SelectionSizeText {
	package static func displayText(for rect: CGRect, scale: CGFloat) -> String {
		let resolvedScale = scale.isFinite && scale > 0 ? scale : 1
		let width = Int(round(rect.width * resolvedScale))
		let height = Int(round(rect.height * resolvedScale))
		let sizeText = "\(width)x\(height)px"

		if abs(resolvedScale - 1) <= 0.005 {
			return sizeText
		}

		return "\(sizeText) @\(String(format: "%g", Double(resolvedScale)))x"
	}
}
