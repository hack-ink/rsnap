import CoreGraphics
import Foundation
import RsnapHostBridge

extension FrozenFrameAuthority {
	func latchToken(containing point: CGPoint) -> FrozenFrameLatchToken? {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard
			let displayID = displayTargets.first(where: {
				$0.value.frame.inclusivelyContains(point)
			})?.key
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
		guard
			let displayID = displayTargets.first(where: {
				$0.value.frame.inclusivelyContains(point)
			})?.key
		else {
			return false
		}
		guard let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record)
		else {
			return true
		}
		return eligibleRecord.selfCaptureFilterComplete == false
	}

	func hasSelfCaptureCompleteFrame(containing point: CGPoint) -> Bool {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard
			let displayID = displayTargets.first(where: {
				$0.value.frame.inclusivelyContains(point)
			})?.key,
			let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record)
		else {
			return false
		}
		return eligibleRecord.selfCaptureFilterComplete
	}

	func hasSelfCaptureCompleteStream(containing point: CGPoint) -> Bool {
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard selfCaptureFilterRequired else {
			return false
		}
		guard
			let displayID = displayTargets.first(where: {
				$0.value.frame.inclusivelyContains(point)
			})?.key,
			let stream = streams[displayID]
		else {
			return false
		}
		return stream.selfCaptureFilterComplete
	}

	func rgbSample(containing point: CGPoint) -> RGBSample? {
		liveRgbSample(containing: point)?.rgb
	}

	func liveRgbSample(containing point: CGPoint) -> LiveRgbSample? {
		stateLock.lock()
		let displayID = displayTargets.first(where: { $0.value.frame.inclusivelyContains(point) })?
			.key
		let record = displayID.flatMap { latestFrames[$0] }.flatMap(snapshotEligibleRecordLocked)
		stateLock.unlock()
		guard let record else {
			return nil
		}
		guard record.ageMilliseconds() <= Self.maximumLiveRgbAgeMilliseconds else {
			return nil
		}
		guard
			let rgb = FrozenFramePixelBufferBridge.rgbSample(
				from: record.pixelBuffer,
				point: point,
				displayFrame: record.displayFrame
			)
		else {
			return nil
		}
		return LiveRgbSample(
			rgb: rgb,
			capturedAtUptime: record.capturedAtUptime,
			source: "frame_authority"
		)
	}

	func loupePatch(containing point: CGPoint, sidePixels: Int) -> CGImage? {
		stateLock.lock()
		let displayID = displayTargets.first(where: { $0.value.frame.inclusivelyContains(point) })?
			.key
		let record = displayID.flatMap { latestFrames[$0] }.flatMap(snapshotEligibleRecordLocked)
		stateLock.unlock()
		guard let record else {
			return nil
		}
		guard record.ageMilliseconds() <= Self.maximumLiveRgbAgeMilliseconds else {
			return nil
		}
		return FrozenFramePixelBufferBridge.loupePatch(
			from: record.pixelBuffer,
			point: point,
			displayFrame: record.displayFrame,
			sidePixels: sidePixels
		)
	}

	func regionImage(in rect: CGRect) -> CGImage? {
		stateLock.lock()
		let center = CGPoint(x: rect.midX, y: rect.midY)
		let displayID =
			displayTargets.first(where: { $0.value.frame.inclusivelyContains(center) })?.key
			?? displayTargets.first(where: { $0.value.frame.intersects(rect) })?.key
		let record = displayID.flatMap { latestFrames[$0] }.flatMap(snapshotEligibleRecordLocked)
		stateLock.unlock()
		guard let record else {
			return nil
		}
		guard record.ageMilliseconds() <= Self.maximumLiveRegionAgeMilliseconds else {
			return nil
		}
		return FrozenFramePixelBufferBridge.regionImage(
			from: record.pixelBuffer,
			rect: rect,
			displayFrame: record.displayFrame
		)
	}

	func nextRegionFrame(
		in rect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		guard
			let record = nextOrderedRegionRecord(
				intersecting: rect,
				afterFrameSequence: afterFrameSequence,
				waitForFresh: waitForFresh
			),
			let region = FrozenFramePixelBufferBridge.regionSnapshot(
				from: record.pixelBuffer,
				rect: rect,
				displayFrame: record.displayFrame
			)
		else {
			return nil
		}
		return RGBARegionFrameSnapshot(
			frameSequence: record.sequence,
			frameAgeMicroseconds: Self.frameAgeMicroseconds(record),
			region: region
		)
	}

	func nextRegionFrame(
		in rect: CGRect,
		pixelRect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		guard
			let record = nextOrderedRegionRecord(
				intersecting: rect,
				afterFrameSequence: afterFrameSequence,
				waitForFresh: waitForFresh
			),
			let region = FrozenFramePixelBufferBridge.regionSnapshot(
				from: record.pixelBuffer,
				pixelRect: pixelRect
			)
		else {
			return nil
		}
		return RGBARegionFrameSnapshot(
			frameSequence: record.sequence,
			frameAgeMicroseconds: Self.frameAgeMicroseconds(record),
			region: region
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
			token?.displayID
			?? displayTargets.first(where: { $0.value.frame.inclusivelyContains(point) })?.key
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

		guard let record,
			let image = FrozenFramePixelBufferBridge.makeImage(from: record.pixelBuffer)
		else {
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

	func resolveSnapshot(
		containing point: CGPoint,
		after token: FrozenFrameLatchToken?,
		maxWait: TimeInterval
	) -> SnapshotResolution {
		if let snapshot = snapshot(containing: point, after: token, maxWait: maxWait) {
			return .resolved(snapshot)
		}
		if needsSelfCaptureCompleteFrame(containing: point) {
			return .pendingSelfCaptureFrame
		}
		return .noFreshFrame
	}

	private func freshRecordLocked(displayID: CGDirectDisplayID, token: FrozenFrameLatchToken?)
		-> FrameRecord?
	{
		guard let record = latestFrames[displayID],
			let eligibleRecord = snapshotEligibleRecordLocked(record)
		else {
			return nil
		}
		guard Self.isFreshForSnapshot(eligibleRecord) else {
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
		guard Self.isFreshForSnapshot(eligibleRecord) else {
			return nil
		}
		// ScreenCaptureKit display streams may not emit another frame while the display is
		// visually unchanged. Even then, same-sequence frames must stay inside the freshness
		// budget; a complete self-capture-excluding filter proves visibility safety, not age.
		return eligibleRecord
	}

	func snapshotEligibleRecordLocked(_ record: FrameRecord) -> FrameRecord? {
		if isSelfCaptureSafeLocked(record) == false {
			return nil
		}
		return record
	}

	func isSelfCaptureSafeLocked(_ record: FrameRecord) -> Bool {
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

	private func nextOrderedRegionRecord(
		intersecting rect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> FrameRecord? {
		let deadline =
			waitForFresh
			? Date(timeIntervalSinceNow: Self.orderedRegionAheadWaitTimeout)
			: Date()
		stateLock.lock()
		defer {
			stateLock.unlock()
		}
		guard let displayID = displayIDLocked(intersecting: rect) else {
			return nil
		}
		var record = nextOrderedRegionRecordLocked(
			displayID: displayID,
			afterFrameSequence: afterFrameSequence
		)
		while record == nil, waitForFresh, Date() < deadline {
			stateLock.wait(until: deadline)
			record = nextOrderedRegionRecordLocked(
				displayID: displayID,
				afterFrameSequence: afterFrameSequence
			)
		}
		return record
	}

	private func displayIDLocked(intersecting rect: CGRect) -> CGDirectDisplayID? {
		let center = CGPoint(x: rect.midX, y: rect.midY)
		return displayTargets.first(where: { $0.value.frame.inclusivelyContains(center) })?.key
			?? displayTargets.first(where: { $0.value.frame.intersects(rect) })?.key
	}

	private func nextOrderedRegionRecordLocked(
		displayID: CGDirectDisplayID,
		afterFrameSequence: UInt64
	) -> FrameRecord? {
		let now = ProcessInfo.processInfo.systemUptime
		guard let frames = orderedFrameHistory[displayID] else {
			return nil
		}
		return frames.first { frame in
			frame.sequence > afterFrameSequence
				&& snapshotEligibleRecordLocked(frame) != nil
				&& Self.frameAgeMicroseconds(frame, now: now)
					<= Self.maximumOrderedRegionAgeMicroseconds
		}
	}

	private static func frameAgeMicroseconds(
		_ record: FrameRecord,
		now: TimeInterval = ProcessInfo.processInfo.systemUptime
	) -> UInt64 {
		let microseconds = max(0, now - record.capturedAtUptime) * 1_000_000
		return UInt64(min(microseconds, Double(UInt64.max)))
	}
}
