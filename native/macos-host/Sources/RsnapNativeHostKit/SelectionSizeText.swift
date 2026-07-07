import CoreGraphics
import Foundation

package enum SelectionSizeText {
	package static func displayText(for rect: CGRect) -> String {
		let width = Int(round(rect.width))
		let height = Int(round(rect.height))
		return "\(width)x\(height)px"
	}
}
