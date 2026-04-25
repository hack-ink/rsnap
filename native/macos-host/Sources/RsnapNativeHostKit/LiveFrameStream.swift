import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

final class LiveFrameStreamBroker {
	private struct SamplerMonitor: Equatable {
		let id: UInt32
		let appKitFrame: CGRect
		let quartzFrame: CGRect
		let scaleFactorX1000: UInt32
	}

	private let stateLock = NSLock()
	private var sampler: RsnapLiveSampler?
	private var monitors: [SamplerMonitor] = []
	private var mainDisplayHeight: CGFloat = 0
	private var streamGeneration: UInt64 = 0
	private var lastPrimedMonitorID: UInt32?
	private var lastPrimeGeneration: UInt64 = 0
	private var lastPrimeUptime: TimeInterval = 0

	init() {
		sampler = try? RsnapLiveSampler()
	}

	func start(for screens: [NSScreen], prewarmPoint: CGPoint? = nil) {
		stateLock.lock()
		if sampler == nil {
			sampler = try? RsnapLiveSampler()
		}
		let mainDisplayHeight = Self.mainDisplayHeight(for: screens)
		self.mainDisplayHeight = mainDisplayHeight
		let nextMonitors = screens.compactMap {
			Self.monitorSnapshot(for: $0, mainDisplayHeight: mainDisplayHeight)
		}
		let targetMonitor = prewarmPoint.flatMap { point in
			nextMonitors.first(where: { $0.appKitFrame.contains(point) })
		}
		let monitorsUnchanged = nextMonitors == monitors
		monitors = nextMonitors
		if monitorsUnchanged {
			stateLock.unlock()
			if let targetMonitor {
				prime(monitor: targetMonitor)
			}
			return
		}
		streamGeneration &+= 1
		stateLock.unlock()
		if let targetMonitor {
			prime(monitor: targetMonitor)
		}
	}

	func stop() {
		stateLock.lock()
		streamGeneration &+= 1
		monitors.removeAll()
		mainDisplayHeight = 0
		lastPrimedMonitorID = nil
		lastPrimeGeneration = 0
		lastPrimeUptime = 0
		let sampler = self.sampler
		stateLock.unlock()
		guard let sampler else {
			return
		}
		try? sampler.reset()
	}

	func sample(at point: CGPoint, sidePixels: Int) -> LiveChromeSample? {
		stateLock.lock()
		let sampler = self.sampler
		stateLock.unlock()
		guard let sampler, let monitor = monitor(containing: point) else {
			return nil
		}
		let samplerPoint = Self.appKitPointToQuartz(point, mainDisplayHeight: mainDisplayHeight)
		guard
			let sample = try? sampler.sampleCursor(
				monitor: samplerMonitorSnapshot(for: monitor),
				point: samplerPoint,
				patchSidePixels: sidePixels
			)
		else {
			return nil
		}

		return LiveChromeSample(
			rgbSample: sample.rgb,
			loupePatch: cgImage(from: sample)
		)
	}

	func patch(in rect: CGRect) -> CGImage? {
		let point = CGPoint(x: rect.midX, y: rect.midY)
		let sidePixels = max(Int(rect.width.rounded()), Int(rect.height.rounded()), 1)
		return sample(at: point, sidePixels: sidePixels)?.loupePatch
	}

	func region(in rect: CGRect) -> CGImage? {
		stateLock.lock()
		let sampler = self.sampler
		stateLock.unlock()
		guard
			let sampler,
			let monitor = monitor(containing: CGPoint(x: rect.midX, y: rect.midY)),
			let snapshot = try? sampler.peekRegion(
				monitor: samplerMonitorSnapshot(for: monitor),
				rect: rect
			)
		else {
			return nil
		}

		return cgImage(
			width: snapshot.width,
			height: snapshot.height,
			rgba: snapshot.rgba
		)
	}

	func latestMonitorImage(containing point: CGPoint) -> (frame: CGRect, image: CGImage)? {
		guard let monitor = monitor(containing: point) else {
			return nil
		}
		stateLock.lock()
		let sampler = self.sampler
		let encodedMonitor = samplerMonitorSnapshot(for: monitor)
		stateLock.unlock()
		guard let sampler else {
			return nil
		}
		for _ in 0..<3 {
			if let snapshot = try? sampler.peekLatestMonitorImage(monitor: encodedMonitor),
				let image = cgImage(
					width: snapshot.width, height: snapshot.height, rgba: snapshot.rgba)
			{
				return (frame: monitor.appKitFrame, image: image)
			}
			prime(monitor: monitor)
			Thread.sleep(forTimeInterval: 1.0 / 120.0)
		}
		return nil
	}

	func prime(at point: CGPoint?) {
		guard let point, let monitor = monitor(containing: point) else {
			return
		}
		prime(monitor: monitor)
	}

	func seedSample(
		at point: CGPoint,
		sidePixels: Int
	) -> LiveChromeSample? {
		return sample(at: point, sidePixels: sidePixels)
	}

	private func monitor(containing point: CGPoint) -> SamplerMonitor? {
		stateLock.lock()
		let monitors = self.monitors
		stateLock.unlock()
		return monitors.first(where: { $0.appKitFrame.contains(point) })
	}

	private func prime(monitor: SamplerMonitor) {
		stateLock.lock()
		let sampler = self.sampler
		let generation = streamGeneration
		let now = ProcessInfo.processInfo.systemUptime
		if lastPrimedMonitorID == monitor.id,
			lastPrimeGeneration == generation,
			now - lastPrimeUptime < (1.0 / 30.0)
		{
			stateLock.unlock()
			return
		}
		lastPrimedMonitorID = monitor.id
		lastPrimeGeneration = generation
		lastPrimeUptime = now
		stateLock.unlock()
		guard let sampler else {
			return
		}
		try? sampler.primeMonitor(samplerMonitorSnapshot(for: monitor))
	}

	private func samplerMonitorSnapshot(for monitor: SamplerMonitor) -> MonitorSnapshot {
		MonitorSnapshot(
			id: monitor.id,
			frame: monitor.quartzFrame,
			scaleFactorX1000: monitor.scaleFactorX1000
		)
	}

	private static func monitorSnapshot(
		for screen: NSScreen,
		mainDisplayHeight: CGFloat
	) -> SamplerMonitor? {
		guard let displayID = screen.displayID else {
			return nil
		}
		let appKitFrame = screen.frame
		return SamplerMonitor(
			id: displayID,
			appKitFrame: appKitFrame,
			quartzFrame: appKitRectToQuartz(appKitFrame, mainDisplayHeight: mainDisplayHeight),
			scaleFactorX1000: UInt32(max((screen.backingScaleFactor * 1000).rounded(), 1000))
		)
	}

	private static func mainDisplayHeight(for screens: [NSScreen]) -> CGFloat {
		screens
			.first(where: { $0.frame.origin.x.rounded() == 0 && $0.frame.origin.y.rounded() == 0 })?
			.frame.height
			.rounded()
			?? screens.first?.frame.height.rounded()
			?? 0
	}

	private static func appKitRectToQuartz(_ rect: CGRect, mainDisplayHeight: CGFloat) -> CGRect {
		CGRect(
			x: rect.minX,
			y: mainDisplayHeight - rect.maxY,
			width: rect.width,
			height: rect.height
		)
	}

	private static func appKitPointToQuartz(_ point: CGPoint, mainDisplayHeight: CGFloat) -> CGPoint
	{
		CGPoint(x: point.x, y: mainDisplayHeight - point.y - 1)
	}

	private func cgImage(from sample: LiveSampleSnapshot?) -> CGImage? {
		guard
			let sample,
			let patchRGBA = sample.patchRGBA,
			sample.patchWidth > 0,
			sample.patchHeight > 0
		else {
			return nil
		}

		return cgImage(
			width: sample.patchWidth,
			height: sample.patchHeight,
			rgba: patchRGBA
		)
	}

	private func cgImage(width: Int, height: Int, rgba: Data) -> CGImage? {
		guard width > 0, height > 0 else {
			return nil
		}
		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
		let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue)
		guard
			let provider = CGDataProvider(data: rgba as CFData),
			let image = CGImage(
				width: width,
				height: height,
				bitsPerComponent: 8,
				bitsPerPixel: 32,
				bytesPerRow: width * 4,
				space: colorSpace,
				bitmapInfo: bitmapInfo,
				provider: provider,
				decode: nil,
				shouldInterpolate: false,
				intent: .defaultIntent
			)
		else {
			return nil
		}

		return image
	}
}

extension NSScreen {
	fileprivate var displayID: CGDirectDisplayID? {
		(deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
	}
}
