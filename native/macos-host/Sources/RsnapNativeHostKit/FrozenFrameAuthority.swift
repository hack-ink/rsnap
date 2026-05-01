import AppKit
import CoreGraphics
import CoreMedia
import CoreVideo
import Foundation
import ScreenCaptureKit

struct FrozenFrameLatchToken {
	let displayID: CGDirectDisplayID
	let generation: UInt64
	let minSequence: UInt64
	let startedAtUptime: TimeInterval
}

struct FrozenFrameSnapshot {
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

final class FrozenFrameAuthority: @unchecked Sendable {
	private static let maximumSnapshotAgeMilliseconds = 150.0
	private static let selfCaptureFilterRetryInterval: TimeInterval = 0.035
	private static let selfCaptureFilterRetryWindow: TimeInterval = 2.5

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
		let generation: UInt64
		let sequence: UInt64
		let capturedAtUptime: TimeInterval
		let selfCaptureFilterComplete: Bool

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

	private struct PreparedContentFilter {
		let filter: SCContentFilter
		let selfCaptureFilterComplete: Bool
		let expectedWindowCount: Int
		let matchedWindowCount: Int
	}

	private let stateLock = NSCondition()
	private let outputQueue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.frozen-frame-authority-output",
		qos: .userInteractive
	)
	private var generation: UInt64 = 0
	private var setupRequestID: UInt64 = 0
	private var setupDisplayIDs: Set<CGDirectDisplayID>?
	private var selfCaptureFilterRequired = false
	private var selfCaptureUnsafeAfterUptime: TimeInterval?
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
		rebuildContentFilter: Bool = false,
		selfCaptureExceptionWindowIDs: Set<CGWindowID> = []
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
		if rebuildContentFilter {
			selfCaptureFilterRequired = true
			selfCaptureUnsafeAfterUptime = setupStartedAt
		}
		// When overlay windows become visible, discard pre-overlay streams instead of updating
		// filters in place. Keep the old stream running until the replacement is configured so a
		// fast click can still freeze a fresh frame, but only if that frame came from a complete
		// self-capture-excluding filter.
		if rebuildContentFilter, unchanged, streamsCoverTargets {
			setupRequestID &+= 1
			let requestID = setupRequestID
			setupDisplayIDs = targetIDs
			updateTelemetryContextLocked(
				captureID: captureID,
				source: source,
				startedAtUptime: setupStartedAt,
				targetIDs: targetIDs
			)
			stateLock.unlock()
			rebuildStreamsFromShareableContent(
				targets: targets,
				targetIDs: targetIDs,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				captureID: captureID,
				source: source,
				startedAtUptime: setupStartedAt,
				retryUntilUptime: setupStartedAt + Self.selfCaptureFilterRetryWindow,
				requestID: requestID
			)
			return
		}
		if unchanged, streamsCoverTargets || setupInProgressForTargets, !rebuildContentFilter {
			updateTelemetryContextLocked(
				captureID: captureID,
				source: source,
				startedAtUptime: setupStartedAt,
				targetIDs: targetIDs
			)
			stateLock.unlock()
			return
		}
		generation &+= 1
		setupRequestID &+= 1
		let requestGeneration = generation
		activeDisplayIDs = targetIDs
		setupDisplayIDs = targetIDs
		if !rebuildContentFilter {
			selfCaptureFilterRequired = false
			selfCaptureUnsafeAfterUptime = nil
		}
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
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			generation: requestGeneration,
			captureID: captureID,
			source: source,
			startedAtUptime: setupStartedAt
		)
	}

	private func rebuildStreamsFromShareableContent(
		targets: [DisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		requestID: UInt64
	) {
		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) {
			[weak self] content, error in
			guard let self else {
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
				self.finishSetup(targetIDs: targetIDs)
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
			guard self.isCurrentSetupRequest(requestID, targetIDs: targetIDs) else {
				return
			}
			let preparedFilters = Self.contentFilters(
				for: targets,
				in: content,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs
			)
			guard Self.filtersAreComplete(preparedFilters, for: targets) else {
				if ProcessInfo.processInfo.systemUptime < retryUntilUptime {
					DispatchQueue.global(qos: .userInteractive).asyncAfter(
						deadline: .now() + Self.selfCaptureFilterRetryInterval
					) { [weak self] in
						self?.rebuildStreamsFromShareableContent(
							targets: targets,
							targetIDs: targetIDs,
							selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
							captureID: captureID,
							source: source,
							startedAtUptime: startedAtUptime,
							retryUntilUptime: retryUntilUptime,
							requestID: requestID
						)
					}
					return
				}
				self.logIncompleteFilters(
					preparedFilters,
					targets: targets,
					captureID: captureID,
					source: source
				)
				self.finishSetup(targetIDs: targetIDs)
				return
			}

			self.stateLock.lock()
			guard self.setupRequestID == requestID, self.activeDisplayIDs == targetIDs else {
				self.stateLock.unlock()
				return
			}
			self.generation &+= 1
			let requestGeneration = self.generation
			self.setupDisplayIDs = targetIDs
			self.latestFrames = self.latestFrames.filter { targetIDs.contains($0.key) }
			self.updateTelemetryContextLocked(
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				targetIDs: targetIDs
			)
			let staleStreams = self.streams.values
			self.streams.removeAll()
			self.stateLock.unlock()

			for staleStream in staleStreams {
				staleStream.stop()
			}

			self.configureStreams(
				targets: targets,
				preparedFilters: preparedFilters,
				generation: requestGeneration,
				captureID: captureID,
				source: source
			)
		}
	}

	private func configureStreamsFromShareableContent(
		targets: [DisplayTarget],
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		generation requestGeneration: UInt64,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval
	) {
		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) {
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
			let preparedFilters = Self.contentFilters(
				for: targets,
				in: content,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs
			)
			self.configureStreams(
				targets: targets,
				preparedFilters: preparedFilters,
				generation: requestGeneration,
				captureID: captureID,
				source: source
			)
		}
	}

	func stop() {
		stateLock.lock()
		generation &+= 1
		setupRequestID &+= 1
		activeDisplayIDs.removeAll()
		setupDisplayIDs = nil
		selfCaptureFilterRequired = false
		selfCaptureUnsafeAfterUptime = nil
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
		let latestRecord = latestFrames[displayID]
		let tokenRecord =
			latestRecord.flatMap { snapshotEligibleRecordLocked($0) }
		return FrozenFrameLatchToken(
			displayID: displayID,
			generation: tokenRecord?.generation ?? 0,
			minSequence: tokenRecord?.sequence ?? 0,
			startedAtUptime: ProcessInfo.processInfo.systemUptime
		)
	}

	func needsSelfCaptureCompleteFrame(containing point: CGPoint) -> Bool {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard selfCaptureFilterRequired else {
			return false
		}
		guard let displayID = displayTargets.first(where: { $0.value.frame.contains(point) })?.key
		else {
			return false
		}
		guard let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record)
		else {
			return true
		}
		return !eligibleRecord.selfCaptureFilterComplete
	}

	func hasSelfCaptureCompleteFrame(containing point: CGPoint) -> Bool {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard let displayID = displayTargets.first(where: { $0.value.frame.contains(point) })?.key,
			let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record)
		else {
			return false
		}
		return eligibleRecord.selfCaptureFilterComplete
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
		var source = "post_token"
		var record = freshRecordLocked(displayID: displayID, token: token)
		if record == nil,
			let fallbackRecord = unchangedRecordLocked(
				displayID: displayID,
				token: token
			)
		{
			record = fallbackRecord
			source = "latest_unchanged"
		}
		while record == nil, Date() < deadline {
			stateLock.wait(until: deadline)
			record = freshRecordLocked(displayID: displayID, token: token)
			if record == nil,
				let fallbackRecord = unchangedRecordLocked(
					displayID: displayID,
					token: token
				)
			{
				record = fallbackRecord
				source = "latest_unchanged"
			}
		}
		stateLock.unlock()

		guard let record, let image = Self.makeImage(from: record.pixelBuffer) else {
			return nil
		}
		return FrozenFrameSnapshot(
			displayID: record.displayID,
			displayFrame: record.displayFrame,
			image: image,
			generation: record.generation,
			sequence: record.sequence,
			capturedAtUptime: record.capturedAtUptime,
			source: source,
			selfCaptureSafe: true,
			selfCaptureFilterComplete: record.selfCaptureFilterComplete
		)
	}

	private func freshRecordLocked(displayID: CGDirectDisplayID, token: FrozenFrameLatchToken?)
		-> FrameRecord?
	{
		guard let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record)
		else {
			return nil
		}
		guard eligibleRecord.selfCaptureFilterComplete || Self.isFreshForSnapshot(record) else {
			return nil
		}
		guard let token else {
			return eligibleRecord
		}
		if eligibleRecord.capturedAtUptime >= token.startedAtUptime {
			return eligibleRecord
		}
		if eligibleRecord.generation == token.generation,
			eligibleRecord.sequence > token.minSequence
		{
			return eligibleRecord
		}
		if token.minSequence == 0 {
			return eligibleRecord
		}
		return nil
	}

	private func unchangedRecordLocked(displayID: CGDirectDisplayID, token: FrozenFrameLatchToken?)
		-> FrameRecord?
	{
		guard let token, token.minSequence > 0, let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record),
			eligibleRecord.generation == token.generation,
			eligibleRecord.sequence == token.minSequence
		else {
			return nil
		}
		if !eligibleRecord.selfCaptureFilterComplete, !Self.isFreshForSnapshot(eligibleRecord) {
			return nil
		}
		// ScreenCaptureKit display streams may not emit another frame while the display is
		// visually unchanged. A complete self-capture-excluding stream may stand in for a
		// post-token frame on a static desktop; pre-overlay frames may do that only while still
		// inside the freshness budget, because older pre-overlay pixels are indistinguishable from
		// a stale video frame.
		return eligibleRecord
	}

	private func snapshotEligibleRecordLocked(_ record: FrameRecord) -> FrameRecord? {
		if !isSelfCaptureSafeLocked(record) {
			return nil
		}
		return record
	}

	private func isSelfCaptureSafeLocked(_ record: FrameRecord) -> Bool {
		if record.selfCaptureFilterComplete {
			return true
		}
		guard selfCaptureFilterRequired else {
			return true
		}
		guard let unsafeAfterUptime = selfCaptureUnsafeAfterUptime else {
			return false
		}
		return record.capturedAtUptime < unsafeAfterUptime
	}

	private static func isFreshForSnapshot(_ record: FrameRecord) -> Bool {
		record.ageMilliseconds() <= maximumSnapshotAgeMilliseconds
	}

	private func configureStreams(
		targets: [DisplayTarget],
		preparedFilters: [CGDirectDisplayID: PreparedContentFilter],
		generation requestGeneration: UInt64,
		captureID: UInt64,
		source: String
	) {
		for target in targets {
			guard let preparedFilter = preparedFilters[target.displayID] else {
				continue
			}

			let output = FrozenFrameStreamOutput(
				displayID: target.displayID,
				displayFrame: target.frame,
				generation: requestGeneration,
				selfCaptureFilterComplete: preparedFilter.selfCaptureFilterComplete
			) { [weak self] frame in
				self?.store(frame: frame, generation: requestGeneration)
			} telemetrySnapshot: { [weak self] in
				self?.currentTelemetrySnapshot() ?? (captureID: captureID, source: source)
			}
			let stream = SCStream(
				filter: preparedFilter.filter, configuration: Self.streamConfiguration(for: target),
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
			let shouldStart =
				generation == requestGeneration
				&& (!selfCaptureFilterRequired || preparedFilter.selfCaptureFilterComplete)
			if shouldStart {
				streams[target.displayID] = DisplayStream(stream: stream, output: output)
			}
			stateLock.unlock()
			if selfCaptureFilterRequired, !preparedFilter.selfCaptureFilterComplete {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.self_capture_filter_incomplete",
					captureID: captureID,
					source: source,
					displayID: target.displayID,
					error:
						"expectedWindowCount=\(preparedFilter.expectedWindowCount) matchedWindowCount=\(preparedFilter.matchedWindowCount)"
				)
			}
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

	private func logIncompleteFilters(
		_ preparedFilters: [CGDirectDisplayID: PreparedContentFilter],
		targets: [DisplayTarget],
		captureID: UInt64,
		source: String
	) {
		for target in targets {
			guard let preparedFilter = preparedFilters[target.displayID] else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.self_capture_filter_incomplete",
					captureID: captureID,
					source: source,
					displayID: target.displayID,
					error: "missingFilter"
				)
				continue
			}
			guard !preparedFilter.selfCaptureFilterComplete else {
				continue
			}
			NativeHostTelemetry.frozenAuthorityWarning(
				"frozen_authority.self_capture_filter_incomplete",
				captureID: captureID,
				source: source,
				displayID: target.displayID,
				error:
					"expectedWindowCount=\(preparedFilter.expectedWindowCount) matchedWindowCount=\(preparedFilter.matchedWindowCount)"
			)
		}
	}

	private static func filtersAreComplete(
		_ preparedFilters: [CGDirectDisplayID: PreparedContentFilter],
		for targets: [DisplayTarget]
	) -> Bool {
		targets.allSatisfy { target in
			preparedFilters[target.displayID]?.selfCaptureFilterComplete == true
		}
	}

	private static func contentFilters(
		for targets: [DisplayTarget],
		in content: SCShareableContent,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>
	) -> [CGDirectDisplayID: PreparedContentFilter] {
		Dictionary(
			uniqueKeysWithValues: targets.compactMap { target in
				guard
					let filter = contentFilter(
						for: target,
						in: content,
						selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs
					)
				else {
					return nil
				}
				return (target.displayID, filter)
			}
		)
	}

	private static func contentFilter(
		for target: DisplayTarget,
		in content: SCShareableContent,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>
	) -> PreparedContentFilter? {
		guard let display = content.displays.first(where: { $0.displayID == target.displayID })
		else {
			return nil
		}
		let currentPID = getpid()
		let excludedApplications = content.applications.filter { $0.processID == currentPID }
		if !excludedApplications.isEmpty {
			return PreparedContentFilter(
				filter: SCContentFilter(
					display: display,
					excludingApplications: excludedApplications,
					exceptingWindows: []
				),
				selfCaptureFilterComplete: true,
				expectedWindowCount: selfCaptureExceptionWindowIDs.count,
				matchedWindowCount: selfCaptureExceptionWindowIDs.count
			)
		}
		let excludedWindows = content.windows.filter {
			$0.owningApplication?.processID == currentPID
		}
		let matchedWindowIDs = Set(excludedWindows.map(\.windowID))
		let missingWindowIDs = selfCaptureExceptionWindowIDs.subtracting(matchedWindowIDs)
		let hasCompleteWindowExclusion =
			!selfCaptureExceptionWindowIDs.isEmpty && missingWindowIDs.isEmpty
		return PreparedContentFilter(
			filter: SCContentFilter(display: display, excludingWindows: excludedWindows),
			selfCaptureFilterComplete: hasCompleteWindowExclusion,
			expectedWindowCount: selfCaptureExceptionWindowIDs.count,
			matchedWindowCount: selfCaptureExceptionWindowIDs.count - missingWindowIDs.count
		)
	}

	private func store(
		frame: FrameRecord,
		generation requestGeneration: UInt64
	) {
		var firstFrameTelemetry: TelemetryContext?
		stateLock.lock()
		if generation == requestGeneration, activeDisplayIDs.contains(frame.displayID),
			isSelfCaptureSafeLocked(frame)
		{
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
				sequence: frame.sequence,
				generation: frame.generation,
				selfCaptureSafe: true,
				selfCaptureFilterComplete: frame.selfCaptureFilterComplete
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

	private func finishSetup(targetIDs: Set<CGDirectDisplayID>) {
		stateLock.lock()
		if setupDisplayIDs == targetIDs {
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

	private func isCurrentSetupRequest(_ requestID: UInt64, targetIDs: Set<CGDirectDisplayID>)
		-> Bool
	{
		stateLock.lock()
		let isCurrent = setupRequestID == requestID && activeDisplayIDs == targetIDs
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
	private let generation: UInt64
	private let selfCaptureFilterComplete: Bool
	private let onFrame: (FrozenFrameAuthority.FrameRecord) -> Void
	private let telemetrySnapshot: () -> (captureID: UInt64, source: String)
	private var sequence: UInt64 = 0

	init(
		displayID: CGDirectDisplayID,
		displayFrame: CGRect,
		generation: UInt64,
		selfCaptureFilterComplete: Bool,
		onFrame: @escaping (FrozenFrameAuthority.FrameRecord) -> Void,
		telemetrySnapshot: @escaping () -> (captureID: UInt64, source: String)
	) {
		self.displayID = displayID
		self.displayFrame = displayFrame
		self.generation = generation
		self.selfCaptureFilterComplete = selfCaptureFilterComplete
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
				generation: generation,
				sequence: sequence,
				capturedAtUptime: ProcessInfo.processInfo.systemUptime,
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
