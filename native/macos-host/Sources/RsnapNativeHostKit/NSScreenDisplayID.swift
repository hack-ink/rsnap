import AppKit
import CoreGraphics

extension NSScreen {
	var nativeDisplayID: CGDirectDisplayID? {
		(deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
	}
}
