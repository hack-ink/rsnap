import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

final class LiveFrameStreamBroker {
	private let stateLock = NSLock()
	private let sampler: RsnapLiveSampler?
	private var monitors: [MonitorSnapshot] = []

	init() {
		sampler = try? RsnapLiveSampler()
	}

	func start(for screens: [NSScreen]) {
		stateLock.lock()
		monitors = screens.compactMap(Self.monitorSnapshot(for:))
		stateLock.unlock()
	}

	func stop() {
		stateLock.lock()
		monitors.removeAll()
		stateLock.unlock()
	}

	func sample(at point: CGPoint, sidePixels: Int) -> LiveChromeSample? {
		guard let sampler, let monitor = monitor(containing: point) else {
			return nil
		}
		guard let sample = try? sampler.sampleCursor(
			monitor: monitor,
			point: point,
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

	private func monitor(containing point: CGPoint) -> MonitorSnapshot? {
		stateLock.lock()
		let monitors = self.monitors
		stateLock.unlock()
		return monitors.first(where: { $0.frame.contains(point) })
	}

	private static func monitorSnapshot(for screen: NSScreen) -> MonitorSnapshot? {
		guard let displayID = screen.displayID else {
			return nil
		}
		return MonitorSnapshot(
			id: displayID,
			frame: screen.frame,
			scaleFactorX1000: UInt32(max((screen.backingScaleFactor * 1000).rounded(), 1000))
		)
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
