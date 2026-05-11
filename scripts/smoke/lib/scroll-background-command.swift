import Foundation

func readInt(_ key: String, default value: Int) -> Int {
	if let raw = ProcessInfo.processInfo.environment[key], let parsed = Int(raw) {
		return parsed
	}
	return value
}

let count = readInt("SCROLL_COUNT", default: 28)
let deltaY = readInt("SCROLL_DELTA_Y", default: 120)
let intervalMs = readInt("SCROLL_INTERVAL_MS", default: 28)
let name = Notification.Name("ink.hack.rsnap.ScrollSmoke.ScrollBy")
let center = DistributedNotificationCenter.default()

for _ in 0..<max(1, count) {
	center.postNotificationName(
		name,
		object: nil,
		userInfo: ["deltaY": NSNumber(value: deltaY)],
		deliverImmediately: true
	)
	usleep(useconds_t(max(1, intervalMs)) * 1_000)
}
