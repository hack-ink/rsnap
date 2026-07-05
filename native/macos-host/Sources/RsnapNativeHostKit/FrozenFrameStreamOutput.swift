import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit

final class FrozenFrameStreamOutput: NSObject, SCStreamOutput, SCStreamDelegate,
	@unchecked Sendable
{
	private static let machTimebaseInfo: mach_timebase_info_data_t = {
		var info = mach_timebase_info_data_t()
		mach_timebase_info(&info)
		if info.denom == 0 {
			info.denom = 1
		}
		return info
	}()

	private let displayID: CGDirectDisplayID
	private let displayFrame: CGRect
	private let generation: UInt64
	private let selfCaptureFilterComplete: Bool
	private let onFrame: (FrozenFrameAuthority.FrameRecord) -> Void
	private let onStop: (CGDirectDisplayID, UInt64) -> Void
	private let telemetrySnapshot: () -> (captureID: UInt64, source: String)
	private var sequence: UInt64 = 0

	init(
		displayID: CGDirectDisplayID,
		displayFrame: CGRect,
		generation: UInt64,
		selfCaptureFilterComplete: Bool,
		onFrame: @escaping (FrozenFrameAuthority.FrameRecord) -> Void,
		onStop: @escaping (CGDirectDisplayID, UInt64) -> Void,
		telemetrySnapshot: @escaping () -> (captureID: UInt64, source: String)
	) {
		self.displayID = displayID
		self.displayFrame = displayFrame
		self.generation = generation
		self.selfCaptureFilterComplete = selfCaptureFilterComplete
		self.onFrame = onFrame
		self.onStop = onStop
		self.telemetrySnapshot = telemetrySnapshot
	}

	func stream(
		_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
		of type: SCStreamOutputType
	) {
		let frameInfo = Self.frameInfo(from: sampleBuffer)
		guard type == .screen, Self.isUsableFrame(sampleBuffer, frameInfo: frameInfo),
			let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer),
			let capturedAtUptime = Self.capturedAtUptime(frameInfo: frameInfo)
		else {
			return
		}
		sequence &+= 1
		onFrame(
			FrozenFrameAuthority.FrameRecord(
				displayID: displayID,
				displayFrame: displayFrame,
				pixelBuffer: pixelBuffer,
				generation: generation,
				sequence: sequence,
				capturedAtUptime: capturedAtUptime,
				selfCaptureFilterComplete: selfCaptureFilterComplete
			)
		)
	}

	func stream(_ stream: SCStream, didStopWithError error: Error) {
		let snapshot = telemetrySnapshot()
		NativeHostTelemetry.frozenAuthorityWarning(
			"frozen_authority.stream_stopped",
			captureID: snapshot.captureID,
			source: snapshot.source,
			displayID: displayID,
			error: String(describing: error)
		)
		onStop(displayID, generation)
	}

	private static func isUsableFrame(
		_ sampleBuffer: CMSampleBuffer,
		frameInfo: [SCStreamFrameInfo: Any]?
	) -> Bool {
		guard CMSampleBufferDataIsReady(sampleBuffer) else {
			return false
		}
		guard let rawStatus = frameInfo?[.status], let status = frameStatus(from: rawStatus) else {
			return true
		}
		return status == .complete
	}

	private static func frameInfo(from sampleBuffer: CMSampleBuffer) -> [SCStreamFrameInfo: Any]? {
		guard
			let attachments = CMSampleBufferGetSampleAttachmentsArray(
				sampleBuffer,
				createIfNecessary: false
			) as? [[SCStreamFrameInfo: Any]]
		else {
			return nil
		}
		return attachments.first
	}

	private static func frameStatus(from value: Any) -> SCFrameStatus? {
		if let status = value as? Int {
			return SCFrameStatus(rawValue: status)
		}
		if let status = value as? NSNumber {
			return SCFrameStatus(rawValue: status.intValue)
		}
		return nil
	}

	private static func capturedAtUptime(frameInfo: [SCStreamFrameInfo: Any]?) -> TimeInterval? {
		guard let displayTime = machAbsoluteDisplayTime(from: frameInfo) else {
			return nil
		}
		return uptimeSeconds(fromMachAbsoluteTime: displayTime)
	}

	private static func machAbsoluteDisplayTime(
		from frameInfo: [SCStreamFrameInfo: Any]?
	) -> UInt64? {
		guard let displayTime = frameInfo?[.displayTime] else {
			return nil
		}
		if let value = displayTime as? UInt64, value > 0 {
			return value
		}
		if let value = displayTime as? Int, value > 0 {
			return UInt64(value)
		}
		if let value = displayTime as? Int64, value > 0 {
			return UInt64(value)
		}
		if let value = displayTime as? NSNumber {
			let machTime = value.uint64Value
			return machTime > 0 ? machTime : nil
		}
		return nil
	}

	private static func uptimeSeconds(fromMachAbsoluteTime machTime: UInt64) -> TimeInterval {
		let timebase = machTimebaseInfo
		let nanoseconds =
			Double(machTime) * Double(timebase.numer) / Double(timebase.denom)
		return nanoseconds / 1_000_000_000
	}
}
