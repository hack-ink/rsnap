import AppKit
import CoreGraphics
import CoreMedia
import CoreVideo
import Foundation
import RsnapHostBridge
@preconcurrency import ScreenCaptureKit

final class LiveFrameStreamBroker {
	private let stateLock = NSLock()
	private var samplers: [CGDirectDisplayID: LiveFrameStreamSampler] = [:]

	func start(for screens: [NSScreen]) {
		stop()
		var nextSamplers: [CGDirectDisplayID: LiveFrameStreamSampler] = [:]
		for screen in screens {
			guard let displayID = screen.displayID else {
				continue
			}
			let sampler = LiveFrameStreamSampler(screen: screen, displayID: displayID)
			nextSamplers[displayID] = sampler
			sampler.start()
		}
		stateLock.lock()
		samplers = nextSamplers
		stateLock.unlock()
	}

	func stop() {
		stateLock.lock()
		let currentSamplers = Array(samplers.values)
		samplers.removeAll()
		stateLock.unlock()
		for sampler in currentSamplers {
			sampler.stop()
		}
	}

	func sample(at point: CGPoint, sidePixels: Int) -> LiveChromeSample? {
		stateLock.lock()
		let currentSamplers = Array(samplers.values)
		stateLock.unlock()
		guard let sampler = currentSamplers.first(where: { $0.frame.contains(point) }) else {
			return nil
		}
		return sampler.sample(at: point, sidePixels: sidePixels)
	}

	func patch(in rect: CGRect) -> CGImage? {
		stateLock.lock()
		let currentSamplers = Array(samplers.values)
		stateLock.unlock()
		guard
			let sampler = currentSamplers.first(where: { $0.frame.intersects(rect) }),
			sampler.frame.contains(CGPoint(x: rect.midX, y: rect.midY))
		else {
			return nil
		}
		return sampler.patch(in: rect)
	}
}

private final class LiveFrameStreamSampler: NSObject, SCStreamOutput, SCStreamDelegate {
	private struct LatestFrame {
		let pixelBuffer: CVPixelBuffer
		let width: Int
		let height: Int
	}

	let frame: CGRect
	private let displayID: CGDirectDisplayID
	private let maximumFramesPerSecond: Int
	private let outputQueue: DispatchQueue
	private let stateLock = NSLock()
	private var latestFrame: LatestFrame?
	private var stream: SCStream?
	private var isStarting = false

	init(screen: NSScreen, displayID: CGDirectDisplayID) {
		self.frame = screen.frame
		self.displayID = displayID
		self.maximumFramesPerSecond = max(1, min(screen.maximumFramesPerSecond, 120))
		self.outputQueue = DispatchQueue(label: "ink.hack.rsnap.native-host.live-frame.\(displayID)")
		super.init()
	}

	func start() {
		guard !isStarting else {
			return
		}
		isStarting = true

		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: true) {
			[weak self] content, error in
			guard let self else {
				return
			}
			self.isStarting = false

			guard error == nil, let content else {
				return
			}
			guard let display = content.displays.first(where: { $0.displayID == self.displayID }) else {
				return
			}

			let currentPID = ProcessInfo.processInfo.processIdentifier
			let currentApplications = content.applications.filter { $0.processID == currentPID }
			let filter = SCContentFilter(
				display: display,
				excludingApplications: currentApplications,
				exceptingWindows: []
			)
			if #available(macOS 14.2, *) {
				filter.includeMenuBar = false
			}

			let config = SCStreamConfiguration()
			let pointScale = max(1, CGFloat(filter.pointPixelScale))
			config.width = max(1, Int((filter.contentRect.width * pointScale).rounded()))
			config.height = max(1, Int((filter.contentRect.height * pointScale).rounded()))
			config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(self.maximumFramesPerSecond))
			config.pixelFormat = UInt32(bigEndian: 0x42475241) // BGRA
			config.queueDepth = 3
			config.showsCursor = false
			if #available(macOS 15.0, *) {
				config.showMouseClicks = false
			}

			let stream = SCStream(filter: filter, configuration: config, delegate: self)
			do {
				try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: self.outputQueue)
			} catch {
				return
			}

			stream.startCapture(completionHandler: { [weak self] error in
				guard let self else {
					return
				}
				if error == nil {
					self.stream = stream
				} else {
					self.stateLock.lock()
					self.latestFrame = nil
					self.stateLock.unlock()
				}
			})
		}
	}

	func stop() {
		stream?.stopCapture(completionHandler: { _ in })
		stream = nil
		stateLock.lock()
		latestFrame = nil
		stateLock.unlock()
	}

	func sample(at point: CGPoint, sidePixels: Int) -> LiveChromeSample? {
		stateLock.lock()
		let latestFrame = latestFrame
		stateLock.unlock()

		guard let latestFrame else {
			return nil
		}

		let width = latestFrame.width
		let height = latestFrame.height
		guard width > 0, height > 0 else {
			return nil
		}

		let pixelsPerPointX = CGFloat(width) / max(frame.width, .leastNonzeroMagnitude)
		let pixelsPerPointY = CGFloat(height) / max(frame.height, .leastNonzeroMagnitude)
		let localX = point.x - frame.minX
		let localYFromTop = frame.maxY - point.y
		guard localX >= 0, localX < frame.width, localYFromTop >= 0, localYFromTop < frame.height else {
			return nil
		}

		let centerX = Int((localX * pixelsPerPointX).rounded(.down)).clamped(to: 0...(width - 1))
		let centerY = Int((localYFromTop * pixelsPerPointY).rounded(.down)).clamped(to: 0...(height - 1))

		let pixelBuffer = latestFrame.pixelBuffer
		CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
		defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

		guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
			return nil
		}
		let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
		let pixelBytes = baseAddress.assumingMemoryBound(to: UInt8.self)
		let centerOffset = centerY * bytesPerRow + centerX * 4
		let b = pixelBytes[centerOffset]
		let g = pixelBytes[centerOffset + 1]
		let r = pixelBytes[centerOffset + 2]
		let rgbSample = RGBSample(r: r, g: g, b: b)

		guard sidePixels > 1 else {
			return LiveChromeSample(rgbSample: rgbSample, loupePatch: nil)
		}

		let patchWidth = min(sidePixels, width)
		let patchHeight = min(sidePixels, height)
		let startX = min(max(0, centerX - patchWidth / 2), width - patchWidth)
		let startY = min(max(0, centerY - patchHeight / 2), height - patchHeight)

		var patchBytes = Data(count: patchWidth * patchHeight * 4)
		patchBytes.withUnsafeMutableBytes { rawBuffer in
			guard let destination = rawBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
				return
			}
			for row in 0..<patchHeight {
				let sourceOffset = (startY + row) * bytesPerRow + startX * 4
				let destinationOffset = row * patchWidth * 4
				destination.advanced(by: destinationOffset)
					.update(from: pixelBytes.advanced(by: sourceOffset), count: patchWidth * 4)
			}
		}

		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
		let bitmapInfo = CGBitmapInfo.byteOrder32Little.union(
			CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue)
		)
		guard
			let provider = CGDataProvider(data: patchBytes as CFData),
			let loupePatch = CGImage(
				width: patchWidth,
				height: patchHeight,
				bitsPerComponent: 8,
				bitsPerPixel: 32,
				bytesPerRow: patchWidth * 4,
				space: colorSpace,
				bitmapInfo: bitmapInfo,
				provider: provider,
				decode: nil,
				shouldInterpolate: false,
				intent: .defaultIntent
			)
		else {
			return LiveChromeSample(rgbSample: rgbSample, loupePatch: nil)
		}

		return LiveChromeSample(rgbSample: rgbSample, loupePatch: loupePatch)
	}

	func patch(in rect: CGRect) -> CGImage? {
		stateLock.lock()
		let latestFrame = latestFrame
		stateLock.unlock()

		guard let latestFrame else {
			return nil
		}

		let width = latestFrame.width
		let height = latestFrame.height
		guard width > 0, height > 0 else {
			return nil
		}

		let clippedRect = rect.intersection(frame)
		guard !clippedRect.isNull, clippedRect.width >= 1, clippedRect.height >= 1 else {
			return nil
		}

		let pixelsPerPointX = CGFloat(width) / max(frame.width, .leastNonzeroMagnitude)
		let pixelsPerPointY = CGFloat(height) / max(frame.height, .leastNonzeroMagnitude)
		let localMinX = clippedRect.minX - frame.minX
		let localMaxX = clippedRect.maxX - frame.minX
		let localMinYFromTop = frame.maxY - clippedRect.maxY
		let localMaxYFromTop = frame.maxY - clippedRect.minY

		let startX = Int((localMinX * pixelsPerPointX).rounded(.down)).clamped(to: 0...(width - 1))
		let endX = Int((localMaxX * pixelsPerPointX).rounded(.up)).clamped(to: (startX + 1)...width)
		let startY = Int((localMinYFromTop * pixelsPerPointY).rounded(.down)).clamped(to: 0...(height - 1))
		let endY = Int((localMaxYFromTop * pixelsPerPointY).rounded(.up)).clamped(to: (startY + 1)...height)
		let patchWidth = max(1, endX - startX)
		let patchHeight = max(1, endY - startY)

		let pixelBuffer = latestFrame.pixelBuffer
		CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
		defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

		guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
			return nil
		}

		let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
		let pixelBytes = baseAddress.assumingMemoryBound(to: UInt8.self)
		var patchBytes = Data(count: patchWidth * patchHeight * 4)
		patchBytes.withUnsafeMutableBytes { rawBuffer in
			guard let destination = rawBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self) else {
				return
			}
			for row in 0..<patchHeight {
				let sourceOffset = (startY + row) * bytesPerRow + startX * 4
				let destinationOffset = row * patchWidth * 4
				destination.advanced(by: destinationOffset)
					.update(from: pixelBytes.advanced(by: sourceOffset), count: patchWidth * 4)
			}
		}

		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
		let bitmapInfo = CGBitmapInfo.byteOrder32Little.union(
			CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue)
		)
		guard
			let provider = CGDataProvider(data: patchBytes as CFData),
			let patch = CGImage(
				width: patchWidth,
				height: patchHeight,
				bitsPerComponent: 8,
				bitsPerPixel: 32,
				bytesPerRow: patchWidth * 4,
				space: colorSpace,
				bitmapInfo: bitmapInfo,
				provider: provider,
				decode: nil,
				shouldInterpolate: true,
				intent: .defaultIntent
			)
		else {
			return nil
		}

		return patch
	}

	func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of outputType: SCStreamOutputType) {
		guard outputType == .screen, let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else {
			return
		}
		let latestFrame = LatestFrame(
			pixelBuffer: pixelBuffer,
			width: CVPixelBufferGetWidth(pixelBuffer),
			height: CVPixelBufferGetHeight(pixelBuffer)
		)
		stateLock.lock()
		self.latestFrame = latestFrame
		stateLock.unlock()
	}

	func stream(_ stream: SCStream, didStopWithError error: Error) {
		stateLock.lock()
		latestFrame = nil
		stateLock.unlock()
		self.stream = nil
	}
}

private extension NSScreen {
	var displayID: CGDirectDisplayID? {
		(deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
	}
}

private extension Int {
	func clamped(to range: ClosedRange<Int>) -> Int {
		Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
	}
}
