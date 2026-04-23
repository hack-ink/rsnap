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

	private struct CachedMonitorImage {
		let frame: CGRect
		let image: CGImage
		let generation: UInt64
	}

	private let stateLock = NSLock()
	private let samplerReleaseQueue = DispatchQueue(
		label: "ink.hack.rsnap.live-frame-stream.release",
		qos: .utility
	)
	private let monitorImageWarmQueue = DispatchQueue(
		label: "ink.hack.rsnap.live-frame-stream.monitor-image-warm",
		qos: .utility
	)
	private var sampler: RsnapLiveSampler?
	private var monitors: [SamplerMonitor] = []
	private var mainDisplayHeight: CGFloat = 0
	private var cachedMonitorImages: [UInt32: CachedMonitorImage] = [:]
	private var warmingMonitorGenerations: [UInt32: UInt64] = [:]
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
		let nextMonitors = screens.compactMap { Self.monitorSnapshot(for: $0, mainDisplayHeight: mainDisplayHeight) }
		let targetMonitor = prewarmPoint.flatMap { point in
			nextMonitors.first(where: { $0.appKitFrame.contains(point) })
		}
		let monitorsUnchanged = nextMonitors == monitors
		monitors = nextMonitors
		if monitorsUnchanged {
			let generation = streamGeneration
			stateLock.unlock()
			if let targetMonitor {
				prime(monitor: targetMonitor)
				warmMonitorImage(monitor: targetMonitor, generation: generation)
			}
			return
		}
		streamGeneration &+= 1
		let generation = streamGeneration
		let liveMonitors = Dictionary(uniqueKeysWithValues: monitors.map { ($0.id, $0) })
		cachedMonitorImages = cachedMonitorImages.filter { monitorID, cachedImage in
			guard let liveMonitor = liveMonitors[monitorID] else {
				return false
			}
			return liveMonitor.appKitFrame == cachedImage.frame
		}
		warmingMonitorGenerations.removeAll()
		stateLock.unlock()
		if let targetMonitor {
			prime(monitor: targetMonitor)
			warmMonitorImage(monitor: targetMonitor, generation: generation)
		}
	}

	func stop() {
		stateLock.lock()
		let retiringSampler = sampler
		streamGeneration &+= 1
		monitors.removeAll()
		mainDisplayHeight = 0
		warmingMonitorGenerations.removeAll()
		lastPrimedMonitorID = nil
		lastPrimeGeneration = 0
		lastPrimeUptime = 0
		sampler = nil
		stateLock.unlock()
		guard let retiringSampler else {
			return
		}
		samplerReleaseQueue.async {
			withExtendedLifetime(retiringSampler) {}
		}
	}

	func sample(at point: CGPoint, sidePixels: Int) -> LiveChromeSample? {
		stateLock.lock()
		let sampler = self.sampler
		stateLock.unlock()
		guard let sampler, let monitor = monitor(containing: point) else {
			return nil
		}
		let samplerPoint = Self.appKitPointToQuartz(point, mainDisplayHeight: mainDisplayHeight)
		guard let sample = try? sampler.sampleCursor(
			monitor: samplerMonitorSnapshot(for: monitor),
			point: samplerPoint,
			patchSidePixels: sidePixels
		) else {
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

	func monitorImage(containing point: CGPoint) -> (frame: CGRect, image: CGImage)? {
		guard let monitor = monitor(containing: point) else {
			return nil
		}
		stateLock.lock()
		let generation = streamGeneration
		let cachedImage = cachedMonitorImages[monitor.id]
		stateLock.unlock()
		if let cachedImage {
			if cachedImage.generation != generation {
				warmMonitorImage(monitor: monitor, generation: generation)
			}
			return (frame: cachedImage.frame, image: cachedImage.image)
		}
		warmMonitorImage(monitor: monitor)
		return nil
	}

	func prime(at point: CGPoint?) {
		guard let point, let monitor = monitor(containing: point) else {
			return
		}
		prime(monitor: monitor)
		warmMonitorImage(monitor: monitor)
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
		let cachedImage = cachedMonitorImages[monitor.id]
		let now = ProcessInfo.processInfo.systemUptime
		if cachedImage?.generation == generation {
			stateLock.unlock()
			return
		}
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

	private func warmMonitorImage(monitor: SamplerMonitor, generation requestedGeneration: UInt64? = nil) {
		stateLock.lock()
		let generation = requestedGeneration ?? streamGeneration
		if cachedMonitorImages[monitor.id]?.generation == generation ||
			warmingMonitorGenerations[monitor.id] == generation
		{
			stateLock.unlock()
			return
		}
		guard let sampler else {
			stateLock.unlock()
			return
		}
		warmingMonitorGenerations[monitor.id] = generation
		let encodedMonitor = samplerMonitorSnapshot(for: monitor)
		stateLock.unlock()

		monitorImageWarmQueue.async { [weak self] in
			guard let self else {
				return
			}
			var cachedImage: CachedMonitorImage?
			for _ in 0..<90 {
				if let snapshot = try? sampler.peekLatestMonitorImage(monitor: encodedMonitor),
					let image = self.cgImage(width: snapshot.width, height: snapshot.height, rgba: snapshot.rgba)
				{
					cachedImage = CachedMonitorImage(
						frame: monitor.appKitFrame,
						image: image,
						generation: generation
					)
					break
				}
				try? sampler.primeMonitor(encodedMonitor)
				Thread.sleep(forTimeInterval: 1.0 / 120.0)
			}

			self.stateLock.lock()
			if self.warmingMonitorGenerations[monitor.id] == generation {
				self.warmingMonitorGenerations.removeValue(forKey: monitor.id)
			}
			if let cachedImage,
				self.streamGeneration == generation,
				self.sampler != nil
			{
				self.cachedMonitorImages[monitor.id] = cachedImage
			}
			self.stateLock.unlock()
		}
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

	private static func appKitPointToQuartz(_ point: CGPoint, mainDisplayHeight: CGFloat) -> CGPoint {
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

private extension NSScreen {
	var displayID: CGDirectDisplayID? {
		(deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
	}
}
