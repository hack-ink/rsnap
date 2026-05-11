import AppKit
import Foundation

let waitMs = Int(ProcessInfo.processInfo.environment["PASTEBOARD_WAIT_MS"] ?? "1200") ?? 1_200
let deadline = Date().addingTimeInterval(TimeInterval(max(0, waitMs)) / 1_000)

while Date() <= deadline {
	if let image = NSImage(pasteboard: .general) {
		var proposedRect = CGRect(origin: .zero, size: image.size)
		if let cgImage = image.cgImage(forProposedRect: &proposedRect, context: nil, hints: nil) {
			print("width=\(cgImage.width) height=\(cgImage.height)")
			exit(0)
		}
	}
	usleep(50_000)
}

fputs("no image found on pasteboard\n", stderr)
exit(1)
