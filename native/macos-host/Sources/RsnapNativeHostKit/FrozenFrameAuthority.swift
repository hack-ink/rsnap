import AppKit
import CoreGraphics
import CoreMedia
import CoreVideo
import Foundation
import ScreenCaptureKit

struct FrozenFrameLatchToken {
	let displayID: CGDirectDisplayID
	let minSequence: UInt64
	let startedAtUptime: TimeInterval
}

struct FrozenFrameSnapshot {
	let displayID: CGDirectDisplayID
	let displayFrame: CGRect
	let image: CGImage
	let sequence: UInt64
	let capturedAtUptime: TimeInterval

	func ageMilliseconds(now: TimeInterval = ProcessInfo.processInfo.systemUptime) -> Double {
		max(0, now - capturedAtUptime) * 1_000
	}
}

final class FrozenFrameAuthority: @unchecked Sendable {
	private struct DisplayTarget: Equatable {
		let displayID: CGDirectDisplayID
		let frame: CGRect
		let widthPixels: Int
		let heightPixels: Int
		let framesPerSecond: Int
	}

	struct FrameRecord: @unchecked Sendable {
		let displayID: CGDirectDisplayID
		let displayFrame: CGRect
		let pixelBuffer: CVPixelBuffer
		let sequence: UInt64
		let capturedAtUptime: TimeInterval

		func ageMilliseconds(now: TimeInterval = ProcessInfo.processInfo.systemUptime) -> Double {
			max(0, now - capturedAtUptime) * 1_000
		}
	}

	private final class DisplayStream: @unchecked Sendable {
		let stream: SCStream
		let output: FrozenFrameStreamOutput

		init(stream: SCStream, output: FrozenFrameStreamOutput) {
			self.stream = stream
			self.output = output
		}

		func stop() {
			stream.stopCapture(completionHandler: nil)
		}
	}

	private final class PixelBufferImageBacking {
		let pixelBuffer: CVPixelBuffer
		let baseAddress: UnsafeMutableRawPointer
		let byteCount: Int
		let unlockFlags = CVPixelBufferLockFlags.readOnly

		init?(_ pixelBuffer: CVPixelBuffer) {
			guard CVPixelBufferLockBaseAddress(pixelBuffer, unlockFlags) == kCVReturnSuccess else {
				return nil
			}
			guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
				CVPixelBufferUnlockBaseAddress(pixelBuffer, unlockFlags)
				return nil
			}
			let height = CVPixelBufferGetHeight(pixelBuffer)
			let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
			guard height > 0, bytesPerRow > 0 else {
				CVPixelBufferUnlockBaseAddress(pixelBuffer, unlockFlags)
				return nil
			}
			self.pixelBuffer = pixelBuffer
			self.baseAddress = baseAddress
			self.byteCount = bytesPerRow * height
		}

		deinit {
			CVPixelBufferUnlockBaseAddress(pixelBuffer, unlockFlags)
		}
	}

	private let stateLock = NSCondition()
	private let outputQueue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.frozen-frame-authority-output",
		qos: .userInteractive
	)
	private var generation: UInt64 = 0
	private var setupDisplayIDs: Set<CGDirectDisplayID>?
	private var activeDisplayIDs: Set<CGDirectDisplayID> = []
	private var displayTargets: [CGDirectDisplayID: DisplayTarget] = [:]
	private var streams: [CGDirectDisplayID: DisplayStream] = [:]
	private var latestFrames: [CGDirectDisplayID: FrameRecord] = [:]

	func start(for screens: [NSScreen]) {
		let targets = screens.compactMap(Self.displayTarget(for:))
		guard !targets.isEmpty else {
			stop()
			return
		}
		let targetIDs = Set(targets.map(\.displayID))
		let nextTargets = Dictionary(uniqueKeysWithValues: targets.map { ($0.displayID, $0) })

		stateLock.lock()
		let unchanged = activeDisplayIDs == targetIDs && displayTargets == nextTargets
		displayTargets = nextTargets
		if unchanged, !streams.isEmpty || setupDisplayIDs == targetIDs {
			stateLock.unlock()
			return
		}
		generation &+= 1
		let requestGeneration = generation
		activeDisplayIDs = targetIDs
		setupDisplayIDs = targetIDs
		latestFrames = latestFrames.filter { targetIDs.contains($0.key) }
		let staleStreams = streams.values
		streams.removeAll()
		stateLock.unlock()

		staleStreams.forEach { $0.stop() }

		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: true) { [weak self] content, error in
			guard let self else {
				return
			}
			guard let content else {
				NSLog("Frozen frame authority content lookup failed: \(String(describing: error))")
				self.finishSetup(generation: requestGeneration)
				return
			}
			self.configureStreams(content: content, targets: targets, generation: requestGeneration)
		}
	}

	func stop() {
		stateLock.lock()
		generation &+= 1
		activeDisplayIDs.removeAll()
		setupDisplayIDs = nil
		displayTargets.removeAll()
		latestFrames.removeAll()
		let staleStreams = streams.values
		streams.removeAll()
		stateLock.unlock()

		staleStreams.forEach { $0.stop() }
	}

	func latchToken(containing point: CGPoint) -> FrozenFrameLatchToken? {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard let displayID = displayTargets.first(where: { $0.value.frame.contains(point) })?.key else {
			return nil
		}
		return FrozenFrameLatchToken(
			displayID: displayID,
			minSequence: latestFrames[displayID]?.sequence ?? 0,
			startedAtUptime: ProcessInfo.processInfo.systemUptime
		)
	}

	func snapshot(
		containing point: CGPoint,
		after token: FrozenFrameLatchToken?,
		maxWait: TimeInterval
	) -> FrozenFrameSnapshot? {
		let deadline = Date(timeIntervalSinceNow: max(0, maxWait))
		stateLock.lock()
		let displayID = token?.displayID ?? displayTargets.first(where: { $0.value.frame.contains(point) })?.key
		guard let displayID else {
			stateLock.unlock()
			return nil
		}
		let minimumSequence = token?.minSequence ?? 0
		var record = freshRecordLocked(displayID: displayID, minimumSequence: minimumSequence)
		while record == nil, Date() < deadline {
			stateLock.wait(until: deadline)
			record = freshRecordLocked(displayID: displayID, minimumSequence: minimumSequence)
		}
		stateLock.unlock()

		guard let record, let image = Self.makeImage(from: record.pixelBuffer) else {
			return nil
		}
		return FrozenFrameSnapshot(
			displayID: record.displayID,
			displayFrame: record.displayFrame,
			image: image,
			sequence: record.sequence,
			capturedAtUptime: record.capturedAtUptime
		)
	}

	private func freshRecordLocked(displayID: CGDirectDisplayID, minimumSequence: UInt64) -> FrameRecord? {
		guard let record = latestFrames[displayID] else {
			return nil
		}
		if record.sequence > minimumSequence {
			return record
		}
		if minimumSequence == 0, record.ageMilliseconds() <= 150 {
			return record
		}
		return nil
	}

	private func configureStreams(
		content: SCShareableContent,
		targets: [DisplayTarget],
		generation requestGeneration: UInt64
	) {
		let currentPID = getpid()
		let excludedApplications = content.applications.filter { $0.processID == currentPID }
		let excludedWindows = content.windows.filter { $0.owningApplication?.processID == currentPID }

		for target in targets {
			guard let display = content.displays.first(where: { $0.displayID == target.displayID }) else {
				continue
			}
			let filter: SCContentFilter
			if !excludedApplications.isEmpty {
				filter = SCContentFilter(
					display: display,
					excludingApplications: excludedApplications,
					exceptingWindows: []
				)
			} else {
				filter = SCContentFilter(display: display, excludingWindows: excludedWindows)
			}

			let output = FrozenFrameStreamOutput(displayID: target.displayID, displayFrame: target.frame) { [weak self] frame in
				self?.store(frame: frame, generation: requestGeneration)
			}
			let stream = SCStream(filter: filter, configuration: Self.streamConfiguration(for: target), delegate: output)
			do {
				try stream.addStreamOutput(output, type: SCStreamOutputType.screen, sampleHandlerQueue: outputQueue)
			} catch {
				NSLog("Frozen frame authority output install failed display=\(target.displayID): \(error)")
				continue
			}

			stateLock.lock()
			let shouldStart = generation == requestGeneration
			if shouldStart {
				streams[target.displayID] = DisplayStream(stream: stream, output: output)
			}
			stateLock.unlock()
			guard shouldStart else {
				continue
			}

			stream.startCapture { error in
				if let error {
					NSLog("Frozen frame authority stream start failed display=\(target.displayID): \(error)")
				}
			}
		}
		finishSetup(generation: requestGeneration)
	}

	private func store(frame: FrameRecord, generation requestGeneration: UInt64) {
		stateLock.lock()
		if generation == requestGeneration, activeDisplayIDs.contains(frame.displayID) {
			latestFrames[frame.displayID] = frame
			stateLock.broadcast()
		}
		stateLock.unlock()
	}

	private func finishSetup(generation requestGeneration: UInt64) {
		stateLock.lock()
		if generation == requestGeneration {
			setupDisplayIDs = nil
			stateLock.broadcast()
		}
		stateLock.unlock()
	}

	private static func streamConfiguration(for target: DisplayTarget) -> SCStreamConfiguration {
		let configuration = SCStreamConfiguration()
		configuration.width = target.widthPixels
		configuration.height = target.heightPixels
		configuration.pixelFormat = kCVPixelFormatType_32BGRA
		configuration.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(target.framesPerSecond))
		configuration.queueDepth = 3
		configuration.showsCursor = false
		configuration.scalesToFit = false
		if #available(macOS 14.0, *) {
			configuration.preservesAspectRatio = true
		}
		return configuration
	}

	private static func displayTarget(for screen: NSScreen) -> DisplayTarget? {
		guard let displayID = screen.displayID else {
			return nil
		}
		let scale = max(screen.backingScaleFactor, 1)
		return DisplayTarget(
			displayID: displayID,
			frame: screen.frame,
			widthPixels: max(1, Int((screen.frame.width * scale).rounded())),
			heightPixels: max(1, Int((screen.frame.height * scale).rounded())),
			framesPerSecond: NativeHostDisplayRefresh.effectiveFramesPerSecond(for: screen)
		)
	}

	private static func makeImage(from pixelBuffer: CVPixelBuffer) -> CGImage? {
		guard let backing = PixelBufferImageBacking(pixelBuffer) else {
			return nil
		}
		let width = CVPixelBufferGetWidth(pixelBuffer)
		let height = CVPixelBufferGetHeight(pixelBuffer)
		let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
		guard width > 0, height > 0, bytesPerRow >= width * 4 else {
			return nil
		}
		let retainedBacking = Unmanaged.passRetained(backing)
		guard let provider = CGDataProvider(
			dataInfo: retainedBacking.toOpaque(),
			data: backing.baseAddress,
			size: backing.byteCount,
			releaseData: { info, _, _ in
				guard let info else {
					return
				}
				Unmanaged<PixelBufferImageBacking>.fromOpaque(info).release()
			}
		) else {
			retainedBacking.release()
			return nil
		}
		let bitmapInfo = CGBitmapInfo.byteOrder32Little
			.union(CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue))
		return CGImage(
			width: width,
			height: height,
			bitsPerComponent: 8,
			bitsPerPixel: 32,
			bytesPerRow: bytesPerRow,
			space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
			bitmapInfo: bitmapInfo,
			provider: provider,
			decode: nil,
			shouldInterpolate: false,
			intent: .defaultIntent
		)
	}
}

private final class FrozenFrameStreamOutput: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
	private let displayID: CGDirectDisplayID
	private let displayFrame: CGRect
	private let onFrame: (FrozenFrameAuthority.FrameRecord) -> Void
	private var sequence: UInt64 = 0

	init(
		displayID: CGDirectDisplayID,
		displayFrame: CGRect,
		onFrame: @escaping (FrozenFrameAuthority.FrameRecord) -> Void
	) {
		self.displayID = displayID
		self.displayFrame = displayFrame
		self.onFrame = onFrame
	}

	func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
		guard type == .screen, Self.isUsableFrame(sampleBuffer), let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else {
			return
		}
		sequence &+= 1
		onFrame(
			FrozenFrameAuthority.FrameRecord(
				displayID: displayID,
				displayFrame: displayFrame,
				pixelBuffer: pixelBuffer,
				sequence: sequence,
				capturedAtUptime: ProcessInfo.processInfo.systemUptime
			)
		)
	}

	func stream(_ stream: SCStream, didStopWithError error: Error) {
		NSLog("Frozen frame authority stream stopped display=\(displayID): \(error)")
	}

	private static func isUsableFrame(_ sampleBuffer: CMSampleBuffer) -> Bool {
		guard CMSampleBufferDataIsReady(sampleBuffer) else {
			return false
		}
		guard
			let attachments = CMSampleBufferGetSampleAttachmentsArray(
				sampleBuffer,
				createIfNecessary: false
			) as? [[SCStreamFrameInfo: Any]],
			let rawStatus = attachments.first?[.status] as? Int,
			let status = SCFrameStatus(rawValue: rawStatus)
		else {
			return true
		}
		return status == .complete
	}
}

private extension NSScreen {
	var displayID: CGDirectDisplayID? {
		(deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
	}
}
