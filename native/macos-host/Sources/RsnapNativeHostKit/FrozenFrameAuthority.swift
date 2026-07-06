import AppKit
import CoreGraphics
import CoreVideo
import Foundation
import RsnapHostBridge
import ScreenCaptureKit

struct FrozenFrameLatchToken: Sendable {
	let displayID: CGDirectDisplayID
	let generation: UInt64
	let minSequence: UInt64
	let startedAtUptime: TimeInterval
}

struct FrozenFrameSnapshot: @unchecked Sendable {
	let displayID: CGDirectDisplayID
	let displayFrame: CGRect
	let image: CGImage
	let generation: UInt64
	let sequence: UInt64
	let capturedAtUptime: TimeInterval
	let source: String
	let selfCaptureSafe: Bool
	let selfCaptureFilterComplete: Bool

	func ageMilliseconds(now: TimeInterval = ProcessInfo.processInfo.systemUptime) -> Double {
		max(0, now - capturedAtUptime) * 1_000
	}
}

/// Owns the screenshot consistency protocol around ScreenCaptureKit's asynchronous frame stream.
///
/// The controller asks this type to prepare an overlay-safe filter, latch a commit point, and
/// resolve that latch into either a fresh frame, a pending self-capture-safe frame, or failure.
/// Keeping those states here prevents cached stream frames from being treated as authoritative
/// screenshots just because a pixel buffer happens to exist.
final class FrozenFrameAuthority: @unchecked Sendable {
	static let maximumSnapshotAgeMilliseconds = 150.0
	static let maximumLiveRgbAgeMilliseconds =
		LiveRgbSample.maximumDisplayAge * 1_000
	static let maximumLiveRegionAgeMilliseconds = maximumSnapshotAgeMilliseconds
	static let selfCaptureFilterRetryInterval: TimeInterval = 0.035
	static let selfCaptureFilterRetryWindow: TimeInterval = 2.5

	struct FrameRecord: @unchecked Sendable {
		let displayID: CGDirectDisplayID
		let displayFrame: CGRect
		let pixelBuffer: CVPixelBuffer
		let generation: UInt64
		let sequence: UInt64
		let capturedAtUptime: TimeInterval
		let selfCaptureFilterComplete: Bool

		func ageMilliseconds(now: TimeInterval = ProcessInfo.processInfo.systemUptime) -> Double {
			max(0, now - capturedAtUptime) * 1_000
		}
	}

	enum SnapshotResolution: Sendable {
		case resolved(FrozenFrameSnapshot)
		case pendingSelfCaptureFrame
		case noFreshFrame
	}

	final class DisplayStream: @unchecked Sendable {
		let stream: SCStream
		let output: FrozenFrameStreamOutput
		let selfCaptureFilterComplete: Bool

		init(
			stream: SCStream,
			output: FrozenFrameStreamOutput,
			selfCaptureFilterComplete: Bool
		) {
			self.stream = stream
			self.output = output
			self.selfCaptureFilterComplete = selfCaptureFilterComplete
		}

		func stop() {
			stream.stopCapture(completionHandler: nil)
		}
	}

	struct TelemetryContext {
		let captureID: UInt64
		let source: String
		let startedAtUptime: TimeInterval
	}

	let stateLock = NSCondition()
	let outputQueue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.frozen-frame-authority-output",
		qos: .userInteractive
	)
	var generation: UInt64 = 0
	var setupRequestID: UInt64 = 0
	var setupDisplayIDs: Set<CGDirectDisplayID>?
	var selfCaptureFilterRequired = false
	var selfCaptureUnsafeAfterUptime: TimeInterval?
	var activeDisplayIDs: Set<CGDirectDisplayID> = []
	var displayTargets: [CGDirectDisplayID: FrozenFrameDisplayTarget] = [:]
	var streams: [CGDirectDisplayID: DisplayStream] = [:]
	var latestFrames: [CGDirectDisplayID: FrameRecord] = [:]
	var firstFrameStartUptimes: [CGDirectDisplayID: TimeInterval] = [:]
	var firstFrameLoggedDisplayIDs: Set<CGDirectDisplayID> = []
	var telemetryContext = TelemetryContext(
		captureID: 0, source: "capture", startedAtUptime: 0)

	func store(
		frame: FrameRecord,
		generation requestGeneration: UInt64
	) {
		var firstFrameTelemetry: TelemetryContext?
		stateLock.lock()
		if generation == requestGeneration, activeDisplayIDs.contains(frame.displayID),
			isSelfCaptureSafeLocked(frame)
		{
			if firstFrameLoggedDisplayIDs.contains(frame.displayID) == false {
				firstFrameLoggedDisplayIDs.insert(frame.displayID)
				let startedAt =
					firstFrameStartUptimes[frame.displayID] ?? telemetryContext.startedAtUptime
				firstFrameTelemetry = TelemetryContext(
					captureID: telemetryContext.captureID,
					source: telemetryContext.source,
					startedAtUptime: startedAt
				)
			}
			latestFrames[frame.displayID] = frame
			stateLock.broadcast()
		}
		stateLock.unlock()
		if let firstFrameTelemetry {
			NativeHostTelemetry.frozenAuthorityFirstFrameTiming(
				captureID: firstFrameTelemetry.captureID,
				source: firstFrameTelemetry.source,
				displayID: frame.displayID,
				totalMilliseconds: NativeHostTelemetry.milliseconds(
					since: firstFrameTelemetry.startedAtUptime),
				frameAgeMilliseconds: frame.ageMilliseconds(),
				sequence: frame.sequence,
				generation: frame.generation,
				selfCaptureSafe: true,
				selfCaptureFilterComplete: frame.selfCaptureFilterComplete
			)
		}
	}

	func handleStreamStopped(
		displayID: CGDirectDisplayID,
		generation stoppedGeneration: UInt64
	) {
		stateLock.lock()
		if generation == stoppedGeneration {
			streams.removeValue(forKey: displayID)
			latestFrames.removeValue(forKey: displayID)
			firstFrameLoggedDisplayIDs.remove(displayID)
			stateLock.broadcast()
		}
		stateLock.unlock()
	}

	func finishSetup(generation requestGeneration: UInt64) {
		stateLock.lock()
		if generation == requestGeneration {
			setupDisplayIDs = nil
			stateLock.broadcast()
		}
		stateLock.unlock()
	}

	func finishSetup(targetIDs: Set<CGDirectDisplayID>) {
		stateLock.lock()
		if setupDisplayIDs == targetIDs {
			setupDisplayIDs = nil
			stateLock.broadcast()
		}
		stateLock.unlock()
	}

	func isCurrentGeneration(_ requestGeneration: UInt64) -> Bool {
		stateLock.lock()
		let isCurrent = generation == requestGeneration
		stateLock.unlock()
		return isCurrent
	}

	func isCurrentSetupRequest(_ requestID: UInt64, targetIDs: Set<CGDirectDisplayID>)
		-> Bool
	{
		stateLock.lock()
		let isCurrent = setupRequestID == requestID && activeDisplayIDs == targetIDs
		stateLock.unlock()
		return isCurrent
	}

	func updateTelemetryContextLocked(
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		targetIDs: Set<CGDirectDisplayID>
	) {
		telemetryContext = TelemetryContext(
			captureID: captureID,
			source: source,
			startedAtUptime: startedAtUptime
		)
		firstFrameStartUptimes = firstFrameStartUptimes.filter { targetIDs.contains($0.key) }
		firstFrameLoggedDisplayIDs.removeAll(keepingCapacity: true)
		for targetID in targetIDs {
			firstFrameStartUptimes[targetID] = startedAtUptime
		}
	}

	func currentTelemetrySnapshot() -> (captureID: UInt64, source: String) {
		stateLock.lock()
		let snapshot = (captureID: telemetryContext.captureID, source: telemetryContext.source)
		stateLock.unlock()
		return snapshot
	}

}
