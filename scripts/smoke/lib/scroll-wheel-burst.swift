import ApplicationServices
import Foundation

func readInt(_ key: String, default value: Int) -> Int {
	if let raw = ProcessInfo.processInfo.environment[key], let parsed = Int(raw) {
		return parsed
	}
	return value
}

func readPoint(_ key: String) -> CGPoint {
	let raw = ProcessInfo.processInfo.environment[key] ?? ""
	let parts = raw.split(separator: ",")
	guard parts.count == 2,
		let x = Double(parts[0]),
		let y = Double(parts[1])
	else {
		fputs("invalid point env for \(key): \(raw)\n", stderr)
		exit(2)
	}
	return CGPoint(x: x, y: y)
}

let point = readPoint("SCROLL_POINT")
let count = readInt("SCROLL_COUNT", default: 28)
let deltaY = readInt("SCROLL_DELTA_Y", default: -120)
let intervalMs = readInt("SCROLL_INTERVAL_MS", default: 28)
guard let source = CGEventSource(stateID: .hidSystemState) else {
	fputs("failed to create CGEventSource\n", stderr)
	exit(1)
}

_ = CGWarpMouseCursorPosition(point)
CGEvent(
	mouseEventSource: source,
	mouseType: .mouseMoved,
	mouseCursorPosition: point,
	mouseButton: .left
)?.post(tap: .cghidEventTap)
usleep(120_000)

for _ in 0..<max(1, count) {
	guard
		let event = CGEvent(
			scrollWheelEvent2Source: source,
			units: .pixel,
			wheelCount: 1,
			wheel1: Int32(deltaY),
			wheel2: 0,
			wheel3: 0
		)
	else {
		fputs("failed to create scroll event\n", stderr)
		exit(1)
	}
	event.location = point
	event.post(tap: .cghidEventTap)
	usleep(useconds_t(max(1, intervalMs)) * 1_000)
}
