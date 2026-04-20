import Cocoa
import ApplicationServices

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
              let y = Double(parts[1]) else {
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

func sleepMs(_ ms: useconds_t) {
    usleep(ms * 1000)
}

func mouseEvent(_ type: CGEventType, at point: CGPoint) {
    let source = CGEventSource(stateID: .hidSystemState)
    let event = CGEvent(
        mouseEventSource: source,
        mouseType: type,
        mouseCursorPosition: point,
        mouseButton: .left
    )
    event?.post(tap: .cghidEventTap)
}

func moveAlong(points: [CGPoint], stepsPerSegment: Int, delayMs: useconds_t, cycles: Int) {
    mouseEvent(.mouseMoved, at: points[0])
    sleepMs(120)

    for _ in 0..<max(1, cycles) {
        for (start, end) in zip(points, points.dropFirst()) {
            for step in 1...max(1, stepsPerSegment) {
                let t = CGFloat(step) / CGFloat(max(1, stepsPerSegment))
                let point = CGPoint(
                    x: start.x + (end.x - start.x) * t,
                    y: start.y + (end.y - start.y) * t
                )
                mouseEvent(.mouseMoved, at: point)
                sleepMs(delayMs)
            }
        }
    }
}

let points = readPoints("PATH_POINTS")
moveAlong(
    points: points,
    stepsPerSegment: readInt("PATH_SEGMENT_STEPS", default: 18),
    delayMs: useconds_t(readInt("PATH_STEP_DELAY_MS", default: 10)),
    cycles: readInt("PATH_CYCLES", default: 2)
)
