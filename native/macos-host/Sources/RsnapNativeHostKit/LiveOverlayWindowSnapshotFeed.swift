import CoreGraphics
import Foundation
import RsnapHostBridge

final class WindowSnapshotFeed {
	struct SnapshotReport {
		let snapshots: [WindowSnapshot]
		let candidateWindowCount: Int
		let ownWindowCount: Int
		let ownTargetableWindowCount: Int
		let highLayerWindowCount: Int
		let tinyWindowCount: Int
		let transparentWindowCount: Int
	}

	private static let ownPID = ProcessInfo.processInfo.processIdentifier
	private static let maxWindowLayerForTargeting = 3
	private static let slowSnapshotRefreshThresholdMilliseconds = 8.0
	private static let telemetrySummaryInterval: TimeInterval = 1.0
	private let queue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.window-snapshot-feed", qos: .userInitiated)
	private let stateLock = NSLock()
	private let snapshotRefreshDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.window_snapshot_refresh_duration",
		category: "LiveChromeTelemetry",
		batchSize: 30
	)
	private let snapshotCandidateWindowCountMetric = NativeHostTelemetry.distribution(
		"live_chrome.window_snapshot_candidate_count",
		category: "LiveChromeTelemetry",
		unit: "windows",
		batchSize: 30
	)
	private let snapshotTargetableWindowCountMetric = NativeHostTelemetry.distribution(
		"live_chrome.window_snapshot_targetable_count",
		category: "LiveChromeTelemetry",
		unit: "windows",
		batchSize: 30
	)
	private var timer: DispatchSourceTimer?
	private var desktopFrame: CGRect = .null
	private var latestSnapshots: [WindowSnapshot] = []
	private var captureID: UInt64 = 0
	private var lastTelemetrySummaryUptime: TimeInterval = 0

	func start(
		desktopFrame: CGRect,
		initialSnapshots: [WindowSnapshot] = [],
		captureID: UInt64 = 0
	) {
		stop()
		stateLock.lock()
		self.desktopFrame = desktopFrame
		latestSnapshots = initialSnapshots
		self.captureID = captureID
		lastTelemetrySummaryUptime = 0
		stateLock.unlock()
		let timer = DispatchSource.makeTimerSource(queue: queue)
		timer.schedule(
			deadline: .now(), repeating: LiveSamplingBudget.hoverWindowCacheRefreshInterval)
		timer.setEventHandler { [weak self] in
			self?.refresh()
		}
		self.timer = timer
		timer.resume()
	}

	func stop() {
		timer?.cancel()
		timer = nil
		stateLock.lock()
		latestSnapshots.removeAll()
		captureID = 0
		lastTelemetrySummaryUptime = 0
		stateLock.unlock()
	}

	func window(at point: CGPoint) -> WindowSnapshot? {
		stateLock.lock()
		let snapshots = latestSnapshots
		stateLock.unlock()
		return snapshots.first(where: { $0.frame.inclusivelyContains(point) })
	}

	static func snapshots(desktopFrame: CGRect) -> [WindowSnapshot] {
		snapshotReport(desktopFrame: desktopFrame).snapshots
	}

	static func snapshotReport(desktopFrame: CGRect) -> SnapshotReport {
		let candidateWindows =
			(CGWindowListCopyWindowInfo(
				[.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
				as? [[String: Any]])
			?? []
		var snapshots: [WindowSnapshot] = []
		var ownWindowCount = 0
		var ownTargetableWindowCount = 0
		var highLayerWindowCount = 0
		var tinyWindowCount = 0
		var transparentWindowCount = 0
		for info in candidateWindows {
			let isOnScreen = (info[kCGWindowIsOnscreen as String] as? NSNumber)?.boolValue ?? false
			let ownerPID = (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value ?? -1
			if ownerPID == ownPID {
				ownWindowCount += 1
			}
			if isOnScreen == false {
				continue
			}
			let alpha = (info[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 1
			if alpha < 0.05 {
				transparentWindowCount += 1
				continue
			}
			let layer = (info[kCGWindowLayer as String] as? NSNumber)?.intValue ?? 0
			if layer < 0 || layer > maxWindowLayerForTargeting {
				highLayerWindowCount += 1
				continue
			}
			if ownerPID == ownPID && !Self.isTargetableOwnWindow(info, layer: layer) {
				continue
			}
			guard let boundsDictionary = info[kCGWindowBounds as String] as? NSDictionary else {
				continue
			}
			var quartzBounds = CGRect.null
			guard CGRectMakeWithDictionaryRepresentation(boundsDictionary, &quartzBounds) else {
				continue
			}
			let appKitBounds = CGRect(
				x: quartzBounds.minX,
				y: desktopFrame.maxY - quartzBounds.maxY,
				width: quartzBounds.width,
				height: quartzBounds.height
			)
			if appKitBounds.width < 40 || appKitBounds.height < 40 {
				tinyWindowCount += 1
				continue
			}
			let windowID = (info[kCGWindowNumber as String] as? NSNumber)?.uint32Value
			if ownerPID == ownPID {
				ownTargetableWindowCount += 1
			}
			snapshots.append(WindowSnapshot(windowID: windowID, frame: appKitBounds))
		}
		return SnapshotReport(
			snapshots: snapshots,
			candidateWindowCount: candidateWindows.count,
			ownWindowCount: ownWindowCount,
			ownTargetableWindowCount: ownTargetableWindowCount,
			highLayerWindowCount: highLayerWindowCount,
			tinyWindowCount: tinyWindowCount,
			transparentWindowCount: transparentWindowCount
		)
	}

	private static func isTargetableOwnWindow(_ info: [String: Any], layer: Int) -> Bool {
		guard layer == 0 else {
			return false
		}
		let name = (info[kCGWindowName as String] as? String) ?? ""
		return name == "Settings"
	}

	static func window(at point: CGPoint, in snapshots: [WindowSnapshot]) -> WindowSnapshot? {
		snapshots.first(where: { $0.frame.inclusivelyContains(point) })
	}

	private func refresh() {
		let startedAt = ProcessInfo.processInfo.systemUptime
		stateLock.lock()
		let desktopFrame = self.desktopFrame
		let captureID = self.captureID
		stateLock.unlock()
		let report = Self.snapshotReport(desktopFrame: desktopFrame)
		let totalMilliseconds = NativeHostTelemetry.milliseconds(since: startedAt)
		stateLock.lock()
		latestSnapshots = report.snapshots
		let summaryDue =
			startedAt - lastTelemetrySummaryUptime >= Self.telemetrySummaryInterval
		if summaryDue {
			lastTelemetrySummaryUptime = startedAt
		}
		stateLock.unlock()
		snapshotRefreshDurationMetric.record(totalMilliseconds)
		snapshotCandidateWindowCountMetric.record(Double(report.candidateWindowCount))
		snapshotTargetableWindowCountMetric.record(Double(report.snapshots.count))
		if summaryDue || totalMilliseconds >= Self.slowSnapshotRefreshThresholdMilliseconds {
			let telemetrySource =
				totalMilliseconds >= Self.slowSnapshotRefreshThresholdMilliseconds
				? "periodic_slow" : "periodic_summary"
			NativeHostTelemetry.liveChromeWindowSnapshotRefresh(
				captureID: captureID,
				source: telemetrySource,
				totalMilliseconds: totalMilliseconds,
				candidateWindowCount: report.candidateWindowCount,
				targetableWindowCount: report.snapshots.count,
				ownWindowCount: report.ownWindowCount,
				ownTargetableWindowCount: report.ownTargetableWindowCount,
				highLayerWindowCount: report.highLayerWindowCount,
				tinyWindowCount: report.tinyWindowCount,
				transparentWindowCount: report.transparentWindowCount
			)
		}
	}
}
