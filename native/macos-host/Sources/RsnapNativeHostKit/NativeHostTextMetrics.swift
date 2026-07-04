import AppKit

extension String {
	func size(using font: NSFont) -> CGSize {
		(self as NSString).size(withAttributes: [.font: font])
	}
}
