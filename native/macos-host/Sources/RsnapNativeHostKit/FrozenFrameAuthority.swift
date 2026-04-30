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
	let source: String

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

	private struct TelemetryContext {
		let captureID: UInt64
		let source: String
		let startedAtUptime: TimeInterval
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
	private var firstFrameStartUptimes: [CGDirectDisplayID: TimeInterval] = [:]
	private var firstFrameLoggedDisplayIDs: Set<CGDirectDisplayID> = []
	private var telemetryContext = TelemetryContext(
		captureID: 0, source: "capture", startedAtUptime: 0)

	func start(
		for screens: [NSScreen],
		captureID: UInt64 = 0,
		source: String = "capture",
		refreshContentFilter: Bool = false
	) {
		let setupStartedAt = ProcessInfo.processInfo.systemUptime
		let targets = screens.compactMap(Self.displayTarget(for:))
		guard !targets.isEmpty else {
			stop()
			return
		}
		let targetIDs = Set(targets.map(\.displayID))
		let nextTargets = Dictionary(uniqueKeysWithValues: targets.map { ($0.displayID, $0) })

		stateLock.lock()
		let unchanged = activeDisplayIDs == targetIDs && displayTargets == nextTargets
		let streamsCoverTargets = Set(streams.keys) == targetIDs
		let setupInProgressForTargets = setupDisplayIDs == targetIDs
		displayTargets = nextTargets
		if unchanged, streamsCoverTargets || setupInProgressForTargets {
			updateTelemetryContextLocked(
				captureID: captureID,
				source: source,
				startedAtUptime: setupStartedAt,
				targetIDs: targetIDs
			)
			let requestGeneration = generation
			stateLock.unlock()
			if refreshContentFilter, streamsCoverTargets {
				refreshContentFilters(
					for: targets,
					generation: requestGeneration,
					captureID: captureID,
					source: source,
					startedAtUptime: setupStartedAt
				)
			}
			return
		}
		generation &+= 1
		let requestGeneration = generation
		activeDisplayIDs = targetIDs
		setupDisplayIDs = targetIDs
		latestFrames = latestFrames.filter { targetIDs.contains($0.key) }
		updateTelemetryContextLocked(
			captureID: captureID,
			source: source,
			startedAtUptime: setupStartedAt,
			targetIDs: targetIDs
		)
		let staleStreams = streams.values
		streams.removeAll()
		stateLock.unlock()

		for staleStream in staleStreams {
			staleStream.stop()
		}

		configureStreamsFromShareableContent(
			targets: targets,
			generation: requestGeneration,
			captureID: captureID,
			source: source,
			startedAtUptime: setupStartedAt
		)
	}

	private func configureStreamsFromShareableContent(
		targets: [DisplayTarget],
		generation requestGeneration: UInt64,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval
	) {
		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: true) {
			[weak self] content, error in
			guard let self else {
				return
			}
			guard self.isCurrentGeneration(requestGeneration) else {
				return
			}
			guard let content else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.content_lookup_failed",
					captureID: captureID,
					source: source,
					displayID: 0,
					error: String(describing: error)
				)
				NativeHostTelemetry.frozenAuthorityContentLookupTiming(
					captureID: captureID,
					source: source,
					totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
					success: false,
					displayCount: targets.count,
					windowCount: 0
				)
				self.finishSetup(generation: requestGeneration)
				return
			}
			NativeHostTelemetry.frozenAuthorityContentLookupTiming(
				captureID: captureID,
				source: source,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
				success: true,
				displayCount: content.displays.count,
				windowCount: content.windows.count
			)
			self.configureStreams(
				content: content,
				targets: targets,
				generation: requestGeneration,
				captureID: captureID,
				source: source
			)
		}
	}

	private func refreshContentFilters(
		for targets: [DisplayTarget],
		generation requestGeneration: UInt64,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval
	) {
		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: true) {
			[weak self] content, error in
			guard let self else {
				return
			}
			guard self.isCurrentGeneration(requestGeneration) else {
				return
			}
			guard let content else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.content_lookup_failed",
					captureID: captureID,
					source: source,
					displayID: 0,
					error: String(describing: error)
				)
				NativeHostTelemetry.frozenAuthorityContentLookupTiming(
					captureID: captureID,
					source: source,
					totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
					success: false,
					displayCount: targets.count,
					windowCount: 0
				)
				return
			}
			NativeHostTelemetry.frozenAuthorityContentLookupTiming(
				captureID: captureID,
				source: source,
				totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
				success: true,
				displayCount: content.displays.count,
				windowCount: content.windows.count
			)
			for target in targets {
				guard let filter = Self.contentFilter(for: target, in: content) else {
					continue
				}
				self.updateContentFilter(
					filter,
					displayID: target.displayID,
					generation: requestGeneration,
					captureID: captureID,
					source: source
				)
			}
		}
	}

	private func updateContentFilter(
		_ filter: SCContentFilter,
		displayID: CGDirectDisplayID,
		generation requestGeneration: UInt64,
		captureID: UInt64,
		source: String
	) {
		stateLock.lock()
		let stream = generation == requestGeneration ? streams[displayID]?.stream : nil
		stateLock.unlock()
		guard let stream else {
			return
		}
		stream.updateContentFilter(filter) { error in
			guard let error else {
				return
			}
			NativeHostTelemetry.frozenAuthorityWarning(
				"frozen_authority.content_filter_update_failed",
				captureID: captureID,
				source: source,
				displayID: displayID,
				error: String(describing: error)
			)
		}
	}

	func stop() {
		stateLock.lock()
		generation &+= 1
		activeDisplayIDs.removeAll()
		setupDisplayIDs = nil
		displayTargets.removeAll()
		latestFrames.removeAll()
		firstFrameStartUptimes.removeAll()
		firstFrameLoggedDisplayIDs.removeAll()
		telemetryContext = TelemetryContext(
			captureID: 0, source: "capture", startedAtUptime: 0)
		let staleStreams = streams.values
		streams.removeAll()
		stateLock.unlock()

		for staleStream in staleStreams {
			staleStream.stop()
		}
	}

	func latchToken(containing point: CGPoint) -> FrozenFrameLatchToken? {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard let displayID = displayTargets.first(where: { $0.value.frame.contains(point) })?.key
		else {
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
		let displayID =
			token?.displayID ?? displayTargets.first(where: { $0.value.frame.contains(point) })?.key
		guard let displayID else {
			stateLock.unlock()
			return nil
		}
		let minimumSequence = token?.minSequence ?? 0
		var source = "post_token"
		var record = freshRecordLocked(displayID: displayID, minimumSequence: minimumSequence)
		while record == nil, Date() < deadline {
			stateLock.wait(until: deadline)
			record = freshRecordLocked(displayID: displayID, minimumSequence: minimumSequence)
		}
		if record == nil,
			let fallbackRecord = unchangedRecordLocked(
				displayID: displayID,
				minimumSequence: minimumSequence
			)
		{
			record = fallbackRecord
			source = "latest_unchanged"
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
			capturedAtUptime: record.capturedAtUptime,
			source: source
		)
	}

	func latestSnapshot(containing point: CGPoint) -> FrozenFrameSnapshot? {
		stateLock.lock()
		let displayID = displayTargets.first(where: { $0.value.frame.contains(point) })?.key
		let record = displayID.flatMap {
			freshRecordLocked(displayID: $0, minimumSequence: 0)
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
			capturedAtUptime: record.capturedAtUptime,
			source: "authority_latest"
		)
	}

	private func freshRecordLocked(displayID: CGDirectDisplayID, minimumSequence: UInt64)
		-> FrameRecord?
	{
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

	private func unchangedRecordLocked(displayID: CGDirectDisplayID, minimumSequence: UInt64)
		-> FrameRecord?
	{
		guard minimumSequence > 0, let record = latestFrames[displayID],
			record.sequence == minimumSequence
		else {
			return nil
		}
		return record
	}

	private func configureStreams(
		content: SCShareableContent,
		targets: [DisplayTarget],
		generation requestGeneration: UInt64,
		captureID: UInt64,
		source: String
	) {
		for target in targets {
			guard let filter = Self.contentFilter(for: target, in: content) else {
				continue
			}

			let output = FrozenFrameStreamOutput(
				displayID: target.displayID,
				displayFrame: target.frame
			) { [weak self] frame in
				self?.store(frame: frame, generation: requestGeneration)
			} telemetrySnapshot: { [weak self] in
				self?.currentTelemetrySnapshot() ?? (captureID: captureID, source: source)
			}
			let stream = SCStream(
				filter: filter, configuration: Self.streamConfiguration(for: target),
				delegate: output)
			do {
				try stream.addStreamOutput(
					output, type: SCStreamOutputType.screen, sampleHandlerQueue: outputQueue)
			} catch {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.output_install_failed",
					captureID: captureID,
					source: source,
					displayID: target.displayID,
					error: String(describing: error)
				)
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
					NativeHostTelemetry.frozenAuthorityWarning(
						"frozen_authority.stream_start_failed",
						captureID: captureID,
						source: source,
						displayID: target.displayID,
						error: String(describing: error)
					)
				}
			}
		}
		finishSetup(generation: requestGeneration)
	}

	private static func contentFilter(
		for target: DisplayTarget,
		in content: SCShareableContent
	) -> SCContentFilter? {
		guard let display = content.displays.first(where: { $0.displayID == target.displayID })
		else {
			return nil
		}
		let currentPID = getpid()
		let excludedApplications = content.applications.filter { $0.processID == currentPID }
		if !excludedApplications.isEmpty {
			return SCContentFilter(
				display: display,
				excludingApplications: excludedApplications,
				exceptingWindows: []
			)
		}
		let excludedWindows = content.windows.filter {
			$0.owningApplication?.processID == currentPID
		}
		return SCContentFilter(display: display, excludingWindows: excludedWindows)
	}

	private func store(
		frame: FrameRecord,
		generation requestGeneration: UInt64
	) {
		var firstFrameTelemetry: TelemetryContext?
		stateLock.lock()
		if generation == requestGeneration, activeDisplayIDs.contains(frame.displayID) {
			if !firstFrameLoggedDisplayIDs.contains(frame.displayID) {
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
				sequence: frame.sequence
			)
		}
	}

	private func finishSetup(generation requestGeneration: UInt64) {
		stateLock.lock()
		if generation == requestGeneration {
			setupDisplayIDs = nil
			stateLock.broadcast()
		}
		stateLock.unlock()
	}

	private func isCurrentGeneration(_ requestGeneration: UInt64) -> Bool {
		stateLock.lock()
		let isCurrent = generation == requestGeneration
		stateLock.unlock()
		return isCurrent
	}

	private func updateTelemetryContextLocked(
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

	private func currentTelemetrySnapshot() -> (captureID: UInt64, source: String) {
		stateLock.lock()
		let snapshot = (captureID: telemetryContext.captureID, source: telemetryContext.source)
		stateLock.unlock()
		return snapshot
	}

	private static func streamConfiguration(for target: DisplayTarget) -> SCStreamConfiguration {
		let configuration = SCStreamConfiguration()
		configuration.width = target.widthPixels
		configuration.height = target.heightPixels
		configuration.pixelFormat = kCVPixelFormatType_32BGRA
		configuration.minimumFrameInterval = CMTime(
			value: 1, timescale: CMTimeScale(target.framesPerSecond))
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
			framesPerSecond: NativeHostDisplayRefresh.targetFramesPerSecond(for: screen)
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
		guard
			let provider = CGDataProvider(
				dataInfo: retainedBacking.toOpaque(),
				data: backing.baseAddress,
				size: backing.byteCount,
				releaseData: { info, _, _ in
					guard let info else {
						return
					}
					Unmanaged<PixelBufferImageBacking>.fromOpaque(info).release()
				}
			)
		else {
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

private final class FrozenFrameStreamOutput: NSObject, SCStreamOutput, SCStreamDelegate,
	@unchecked Sendable
{
	private let displayID: CGDirectDisplayID
	private let displayFrame: CGRect
	private let onFrame: (FrozenFrameAuthority.FrameRecord) -> Void
	private let telemetrySnapshot: () -> (captureID: UInt64, source: String)
	private var sequence: UInt64 = 0

	init(
		displayID: CGDirectDisplayID,
		displayFrame: CGRect,
		onFrame: @escaping (FrozenFrameAuthority.FrameRecord) -> Void,
		telemetrySnapshot: @escaping () -> (captureID: UInt64, source: String)
	) {
		self.displayID = displayID
		self.displayFrame = displayFrame
		self.onFrame = onFrame
		self.telemetrySnapshot = telemetrySnapshot
	}

	func stream(
		_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
		of type: SCStreamOutputType
	) {
		guard type == .screen, Self.isUsableFrame(sampleBuffer),
			let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer)
		else {
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
		let snapshot = telemetrySnapshot()
		NativeHostTelemetry.frozenAuthorityWarning(
			"frozen_authority.stream_stopped",
			captureID: snapshot.captureID,
			source: snapshot.source,
			displayID: displayID,
			error: String(describing: error)
		)
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

extension NSScreen {
	fileprivate var displayID: CGDirectDisplayID? {
		(deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
	}
}
