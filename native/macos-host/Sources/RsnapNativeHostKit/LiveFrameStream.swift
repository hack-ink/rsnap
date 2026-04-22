import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

final class LiveFrameStreamBroker {
	private struct SamplerMonitor {
		let id: UInt32
		let appKitFrame: CGRect
		let quartzFrame: CGRect
		let scaleFactorX1000: UInt32
	}

	private let stateLock = NSLock()
	private let sampler: RsnapLiveSampler?
	private var monitors: [SamplerMonitor] = []
	private var mainDisplayHeight: CGFloat = 0

	init() {
		sampler = try? RsnapLiveSampler()
	}

	func start(for screens: [NSScreen]) {
		stateLock.lock()
		let mainDisplayHeight = Self.mainDisplayHeight(for: screens)
		self.mainDisplayHeight = mainDisplayHeight
		monitors = screens.compactMap { Self.monitorSnapshot(for: $0, mainDisplayHeight: mainDisplayHeight) }
		stateLock.unlock()
	}

	func stop() {
		stateLock.lock()
		monitors.removeAll()
		mainDisplayHeight = 0
		stateLock.unlock()
	}

	func sample(at point: CGPoint, sidePixels: Int) -> LiveChromeSample? {
		guard let sampler, let monitor = monitor(containing: point) else {
			return nil
		}
		let samplerPoint = Self.appKitPointToQuartz(point, mainDisplayHeight: mainDisplayHeight)
		guard let sample = try? sampler.sampleCursor(
			monitor: MonitorSnapshot(
				id: monitor.id,
				frame: monitor.quartzFrame,
				scaleFactorX1000: monitor.scaleFactorX1000
			),
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

	private func monitor(containing point: CGPoint) -> SamplerMonitor? {
		stateLock.lock()
		let monitors = self.monitors
		stateLock.unlock()
		return monitors.first(where: { $0.appKitFrame.contains(point) })
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

		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
		let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue)
		guard
			let provider = CGDataProvider(data: patchRGBA as CFData),
			let image = CGImage(
				width: sample.patchWidth,
				height: sample.patchHeight,
				bitsPerComponent: 8,
				bitsPerPixel: 32,
				bytesPerRow: sample.patchWidth * 4,
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
