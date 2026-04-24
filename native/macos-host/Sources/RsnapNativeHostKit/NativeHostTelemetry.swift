import Foundation
import OSLog

enum NativeHostTelemetry {
	static let subsystem = Bundle.main.bundleIdentifier ?? "ink.hack.rsnap"
	private static let liveChromeLogger = Logger(subsystem: subsystem, category: "LiveChromeTelemetry")

	static func distribution(
		_ name: String,
		category: String,
		unit: String = "ms",
		batchSize: Int = 120
	) -> DistributionMetric {
		DistributionMetric(
			name: name,
			category: category,
			unit: unit,
			batchSize: batchSize
		)
	}

	static func liveChromeRefreshTarget(displayHz: Int, targetHz: Int, frameBudgetMilliseconds: Double) {
		liveChromeLogger.info(
			"event=live_chrome.refresh_target displayHz=\(displayHz, privacy: .public) targetHz=\(targetHz, privacy: .public) frameBudgetMs=\(frameBudgetMilliseconds, format: .fixed(precision: 2), privacy: .public)"
		)
	}

	final class DistributionMetric {
		private let name: String
		private let unit: String
		private let batchSize: Int
		private let logger: Logger
		private let lock = NSLock()
		private var samples: [Double] = []

		fileprivate init(name: String, category: String, unit: String, batchSize: Int) {
			self.name = name
			self.unit = unit
			self.batchSize = max(1, batchSize)
			logger = Logger(subsystem: NativeHostTelemetry.subsystem, category: category)
		}

		func record(_ value: Double) {
			guard value.isFinite, value >= 0, value < 5_000 else {
				return
			}

			let batch: [Double]?
			lock.lock()
			samples.append(value)
			if samples.count >= batchSize {
				batch = samples
				samples.removeAll(keepingCapacity: true)
			} else {
				batch = nil
			}
			lock.unlock()

			if let batch {
				emit(batch)
			}
		}

		func recordMillisecondsSince(_ startUptime: TimeInterval) {
			record((ProcessInfo.processInfo.systemUptime - startUptime) * 1_000)
		}

		func recordLatencySince(_ inputUptime: TimeInterval?) {
			guard let inputUptime else {
				return
			}
			recordMillisecondsSince(inputUptime)
		}

		private func emit(_ batch: [Double]) {
			let sorted = batch.sorted()
			guard let maxValue = sorted.last else {
				return
			}
			let p50 = percentile(sorted, 0.50)
			let p95 = percentile(sorted, 0.95)
			logger.info(
				"metric=\(self.name, privacy: .public) unit=\(self.unit, privacy: .public) samples=\(sorted.count, privacy: .public) p50=\(p50, format: .fixed(precision: 2), privacy: .public) p95=\(p95, format: .fixed(precision: 2), privacy: .public) max=\(maxValue, format: .fixed(precision: 2), privacy: .public)"
			)
		}

		private func percentile(_ sorted: [Double], _ percentile: Double) -> Double {
			guard !sorted.isEmpty else {
				return 0
			}
			let fraction = min(max(percentile, 0), 1)
			let rawIndex = Int((Double(sorted.count - 1) * fraction).rounded(.up))
			let index = min(max(rawIndex, 0), sorted.count - 1)
			return sorted[index]
		}
	}
}
