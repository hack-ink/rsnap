import ApplicationServices
import Cocoa
import Darwin

func readInt(_ key: String, default value: Int? = nil) -> Int {
	if let raw = ProcessInfo.processInfo.environment[key], let parsed = Int(raw) {
		return parsed
	}
	if let value {
		return value
	}
	fputs("invalid int env for \(key)\n", stderr)
	exit(2)
}

func readPoints(_ key: String) -> [CGPoint] {
	let raw = ProcessInfo.processInfo.environment[key] ?? ""
	let points = raw.split(separator: ";").compactMap { item -> CGPoint? in
		let parts = item.split(separator: ",")
		guard parts.count == 2,
			let x = Double(parts[0]),
			let y = Double(parts[1])
		else {
			return nil
		}
		return CGPoint(x: x, y: y)
	}
	guard points.count >= 2 else {
		fputs("invalid points env for \(key): \(raw)\n", stderr)
		exit(2)
	}
	return points
}

func readString(_ key: String, default value: String) -> String {
	ProcessInfo.processInfo.environment[key] ?? value
}

func sleepMs(_ ms: useconds_t) {
	usleep(ms * 1000)
}

final class MousePathDriver {
	private let source = CGEventSource(stateID: .hidSystemState)
	private let mode = readString("PATH_DRIVER", default: "event")

	func mouseEvent(_ type: CGEventType, at point: CGPoint) {
		if mode == "warp" {
			_ = CGWarpMouseCursorPosition(point)
			return
		}
		let event = CGEvent(
			mouseEventSource: source,
			mouseType: type,
			mouseCursorPosition: point,
			mouseButton: .left
		)
		event?.post(tap: .cghidEventTap)
	}
}

func moveAlong(points: [CGPoint], stepsPerSegment: Int, delayMs: useconds_t, cycles: Int) {
	let driver = MousePathDriver()
	driver.mouseEvent(.mouseMoved, at: points[0])
	sleepMs(120)

	for _ in 0..<max(1, cycles) {
		for (start, end) in zip(points, points.dropFirst()) {
			for step in 1...max(1, stepsPerSegment) {
				let t = CGFloat(step) / CGFloat(max(1, stepsPerSegment))
				let point = CGPoint(
					x: start.x + (end.x - start.x) * t,
					y: start.y + (end.y - start.y) * t
				)
				driver.mouseEvent(.mouseMoved, at: point)
				sleepMs(delayMs)
			}
		}
	}
}

let machTimebase: mach_timebase_info_data_t = {
	var info = mach_timebase_info_data_t()
	mach_timebase_info(&info)
	return info
}()

func machTicks(forNanoseconds nanoseconds: UInt64) -> UInt64 {
	nanoseconds * UInt64(machTimebase.denom) / UInt64(machTimebase.numer)
}

func sleepUntil(_ deadline: UInt64) {
	_ = mach_wait_until(deadline)
}

func writeMaskProbePhase(_ phase: String) {
	guard let path = ProcessInfo.processInfo.environment["MASK_PROBE_PHASE_PATH"], !path.isEmpty
	else {
		return
	}
	do {
		try phase.write(toFile: path, atomically: true, encoding: .utf8)
	} catch {
		fputs("failed to write mask probe phase: \(error)\n", stderr)
		exit(1)
	}
}

func releasePrimaryButton(with driver: MousePathDriver, at point: CGPoint) {
	driver.mouseEvent(.leftMouseUp, at: point)
	sleepMs(20)
	driver.mouseEvent(.leftMouseUp, at: point)
	sleepMs(20)
	driver.mouseEvent(.mouseMoved, at: point)
}

func moveSmooth(points: [CGPoint], durationMs: Int, rateHz: Int, cycles: Int) {
	let driver = MousePathDriver()
	let minX = points.map(\.x).min() ?? 0
	let maxX = points.map(\.x).max() ?? minX
	let minY = points.map(\.y).min() ?? 0
	let maxY = points.map(\.y).max() ?? minY
	let center = CGPoint(x: (minX + maxX) * 0.5, y: (minY + maxY) * 0.5)
	let radiusX = max((maxX - minX) * 0.48, 1)
	let radiusY = max((maxY - minY) * 0.48, 1)
	let sampleCount = max(2, durationMs * max(rateHz, 1) / 1_000)
	let stepTicks = machTicks(forNanoseconds: UInt64(1_000_000_000 / max(rateHz, 1)))
	let start = mach_absolute_time()
	let cycleCount = max(cycles, 1)

	for index in 0...sampleCount {
		let progress = Double(index) / Double(sampleCount)
		let phase = progress * Double(cycleCount) * 2 * Double.pi
		let point = CGPoint(
			x: center.x + radiusX * CGFloat(sin(phase * 2)),
			y: center.y + radiusY * CGFloat(sin(phase * 3 + Double.pi / 2))
		)
		driver.mouseEvent(.mouseMoved, at: point)
		sleepUntil(start + UInt64(index + 1) * stepTicks)
	}
}

func dragRegion(points: [CGPoint], durationMs: Int, rateHz: Int) {
	let driver = MousePathDriver()
	let start = points[0]
	let end = points[1]
	let sampleCount = max(2, durationMs * max(rateHz, 1) / 1_000)
	let stepTicks = machTicks(forNanoseconds: UInt64(1_000_000_000 / max(rateHz, 1)))
	writeMaskProbePhase("pre")
	driver.mouseEvent(.mouseMoved, at: start)
	sleepMs(120)
	driver.mouseEvent(.leftMouseDown, at: start)
	writeMaskProbePhase("dragging")
	sleepMs(16)
	let dragStart = mach_absolute_time()

	for index in 1...sampleCount {
		let progress = CGFloat(index) / CGFloat(sampleCount)
		let point = CGPoint(
			x: start.x + (end.x - start.x) * progress,
			y: start.y + (end.y - start.y) * progress
		)
		driver.mouseEvent(.leftMouseDragged, at: point)
		sleepUntil(dragStart + UInt64(index) * stepTicks)
	}
	let holdBeforeReleaseMs = readInt("PATH_HOLD_BEFORE_RELEASE_MS", default: 0)
	if holdBeforeReleaseMs > 0 {
		writeMaskProbePhase("holding")
		sleepMs(useconds_t(holdBeforeReleaseMs))
	}
	releasePrimaryButton(with: driver, at: end)
	writeMaskProbePhase("released")
	if ProcessInfo.processInfo.environment["MASK_PROBE_PHASE_PATH"] != nil {
		sleepMs(useconds_t(readInt("MASK_PROBE_POST_RELEASE_MS", default: 360)))
	}
}

func clickPoint(points: [CGPoint]) {
	let driver = MousePathDriver()
	let point = points[0]
	driver.mouseEvent(.mouseMoved, at: point)
	sleepMs(120)
	driver.mouseEvent(.leftMouseDown, at: point)
	sleepMs(24)
	releasePrimaryButton(with: driver, at: point)
}

func releasePrimaryButton(points: [CGPoint]) {
	let driver = MousePathDriver()
	releasePrimaryButton(with: driver, at: points[0])
}

let points = readPoints("PATH_POINTS")
switch readString("PATH_MODE", default: "smooth") {
case "click-point":
	clickPoint(points: points)
case "drag-region":
	dragRegion(
		points: points,
		durationMs: readInt("PATH_DURATION_MS", default: 260),
		rateHz: readInt("PATH_RATE_HZ", default: 120)
	)
case "waypoints":
	moveAlong(
		points: points,
		stepsPerSegment: readInt("PATH_SEGMENT_STEPS", default: 18),
		delayMs: useconds_t(readInt("PATH_STEP_DELAY_MS", default: 10)),
		cycles: readInt("PATH_CYCLES", default: 2)
	)
case "release-primary":
	releasePrimaryButton(points: points)
default:
	moveSmooth(
		points: points,
		durationMs: readInt("PATH_DURATION_MS", default: 2_500),
		rateHz: readInt("PATH_RATE_HZ", default: 120),
		cycles: readInt("PATH_CYCLES", default: 3)
	)
}
