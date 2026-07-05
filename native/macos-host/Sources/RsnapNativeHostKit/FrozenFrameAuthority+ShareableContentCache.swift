import CoreGraphics
import Foundation
import ScreenCaptureKit

extension FrozenFrameAuthority {
	func refreshShareableContentCache(captureID: UInt64 = 0, source: String = "cache") {
		FrozenFrameShareableContentLookup.refreshCache(captureID: captureID, source: source)
	}

	func hasFreshShareableContentCache() -> Bool {
		FrozenFrameShareableContentLookup.hasFreshCache()
	}
}
