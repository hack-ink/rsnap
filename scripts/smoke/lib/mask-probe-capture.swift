import AppKit
import CoreGraphics
import CoreImage
import CoreMedia
import CoreVideo
import Foundation
import ScreenCaptureKit

func readInt(_ key: String, default value: Int) -> Int {
	if let raw = ProcessInfo.processInfo.environment[key], let parsed = Int(raw) {
		return parsed
	}
	return value
}

func readRequiredString(_ key: String) -> String {
	guard let value = ProcessInfo.processInfo.environment[key], !value.isEmpty else {
		fputs("missing env \(key)\n", stderr)
		exit(2)
	}
	return value
}

func readOptionalString(_ key: String) -> String? {
	guard let value = ProcessInfo.processInfo.environment[key], !value.isEmpty else {
		return nil
	}
	return value
}

func readRequiredPoint(_ key: String) -> CGPoint {
	let raw = readRequiredString(key)
	let parts = raw.split(separator: ",")
	guard parts.count == 2, let x = Double(parts[0]), let y = Double(parts[1]) else {
		fputs("invalid point env for \(key): \(raw)\n", stderr)
		exit(2)
	}
	return CGPoint(x: x, y: y)
}

@available(macOS 12.3, *)
final class MaskProbeCapture: NSObject, SCStreamOutput {
	private struct Sample {
		let phase: String
		let uptime: TimeInterval
		let luminance: Double
	}

	private let outputPath: String
	private let phasePath: String
	private let readyPath: String?
	private let screenshotPath: String?
	private let releasedScreenshotPath: String?
	private let point: CGPoint
	private let displayFrame: CGRect
	private let lock = NSLock()
	private var samples: [Sample] = []
	private var wroteReady = false
	private var wroteScreenshot = false
	private var wroteReleasedScreenshot = false
	private var completedReleasedScreenshot = false

	init(
		outputPath: String,
		phasePath: String,
		readyPath: String?,
		screenshotPath: String?,
		releasedScreenshotPath: String?,
		point: CGPoint,
		displayFrame: CGRect
	) {
		self.outputPath = outputPath
		self.phasePath = phasePath
		self.readyPath = readyPath
		self.screenshotPath = screenshotPath
		self.releasedScreenshotPath = releasedScreenshotPath
		self.point = point
		self.displayFrame = displayFrame
	}

	func stream(
		_ stream: SCStream,
		didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
		of type: SCStreamOutputType
	) {
		guard type == .screen,
			sampleBuffer.isValid,
			let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer),
			let luminance = sampleLuminance(pixelBuffer)
		else {
			return
		}
		let phase = currentPhase()
		let screenshotToWrite: (path: String, phase: String)?
		lock.lock()
		if phase == "holding", let screenshotPath, !wroteScreenshot {
			wroteScreenshot = true
			screenshotToWrite = (screenshotPath, phase)
		} else if phase == "released", let releasedScreenshotPath, !wroteReleasedScreenshot {
			wroteReleasedScreenshot = true
			screenshotToWrite = (releasedScreenshotPath, phase)
		} else {
			screenshotToWrite = nil
		}
		samples.append(
			Sample(
				phase: phase,
				uptime: ProcessInfo.processInfo.systemUptime,
				luminance: luminance
			)
		)
		writeReadyIfNeeded()
		lock.unlock()
		if let screenshotToWrite {
			let wroteScreenshot = writeScreenshot(pixelBuffer, to: screenshotToWrite.path)
			if screenshotToWrite.phase == "released", wroteScreenshot {
				lock.lock()
				completedReleasedScreenshot = true
				lock.unlock()
			}
		}
	}

	func writeSamples() {
		lock.lock()
		let samples = self.samples
		lock.unlock()

		var output = "phase,uptime,luminance\n"
		for sample in samples {
			output +=
				"\(sample.phase),\(String(format: "%.6f", sample.uptime)),\(String(format: "%.6f", sample.luminance))\n"
		}
		do {
			try output.write(toFile: outputPath, atomically: true, encoding: .utf8)
		} catch {
			fputs("failed to write mask probe samples: \(error)\n", stderr)
			exit(1)
		}
	}

	func hasRequiredSamples(minDraggingSamples: Int, minReleasedSamples: Int) -> Bool {
		lock.lock()
		let samples = self.samples
		let releasedScreenshotComplete =
			releasedScreenshotPath == nil || completedReleasedScreenshot
		lock.unlock()

		var draggingSamples = 0
		var releasedSamples = 0
		for sample in samples {
			switch sample.phase {
			case "dragging", "holding":
				draggingSamples += 1
			case "released":
				releasedSamples += 1
			default:
				continue
			}
		}
		return draggingSamples >= minDraggingSamples
			&& releasedSamples >= minReleasedSamples
			&& releasedScreenshotComplete
	}

	private func currentPhase() -> String {
		guard
			let phase = try? String(contentsOfFile: phasePath, encoding: .utf8)
				.trimmingCharacters(in: .whitespacesAndNewlines),
			!phase.isEmpty
		else {
			return "pre"
		}
		return phase
	}

	private func writeReadyIfNeeded() {
		guard !wroteReady, let readyPath else {
			return
		}
		wroteReady = true
		do {
			try "ready\n".write(toFile: readyPath, atomically: true, encoding: .utf8)
		} catch {
			fputs("failed to write mask probe ready marker: \(error)\n", stderr)
		}
	}

	private func writeScreenshot(_ pixelBuffer: CVPixelBuffer, to path: String) -> Bool {
		let width = CVPixelBufferGetWidth(pixelBuffer)
		let height = CVPixelBufferGetHeight(pixelBuffer)
		let ciImage = CIImage(cvPixelBuffer: pixelBuffer)
		let context = CIContext(options: nil)
		guard
			let cgImage = context.createCGImage(
				ciImage,
				from: CGRect(x: 0, y: 0, width: width, height: height)
			)
		else {
			fputs("failed to create drag screenshot image\n", stderr)
			return false
		}
		let bitmap = NSBitmapImageRep(cgImage: cgImage)
		guard let data = bitmap.representation(using: .png, properties: [:]) else {
			fputs("failed to encode drag screenshot PNG\n", stderr)
			return false
		}
		do {
			try data.write(to: URL(fileURLWithPath: path), options: .atomic)
			return true
		} catch {
			fputs("failed to write drag screenshot: \(error)\n", stderr)
			return false
		}
	}

	private func sampleLuminance(_ pixelBuffer: CVPixelBuffer) -> Double? {
		CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
		defer {
			CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly)
		}
		guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
			return nil
		}
		let width = CVPixelBufferGetWidth(pixelBuffer)
		let height = CVPixelBufferGetHeight(pixelBuffer)
		let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
		guard width > 0, height > 0, displayFrame.width > 0, displayFrame.height > 0 else {
			return nil
		}

		let normalizedX = (point.x - displayFrame.minX) / displayFrame.width
		let normalizedY = (point.y - displayFrame.minY) / displayFrame.height
		let centerX = Int((normalizedX * CGFloat(width)).rounded())
		let centerY = Int((normalizedY * CGFloat(height)).rounded())
		let radius = 2
		let minX = max(0, centerX - radius)
		let maxX = min(width - 1, centerX + radius)
		let minY = max(0, centerY - radius)
		let maxY = min(height - 1, centerY + radius)

		let pointer = baseAddress.assumingMemoryBound(to: UInt8.self)
		var total = 0.0
		var count = 0
		for y in minY...maxY {
			for x in minX...maxX {
				let index = y * bytesPerRow + x * 4
				let blue = Double(pointer[index]) / 255.0
				let green = Double(pointer[index + 1]) / 255.0
				let red = Double(pointer[index + 2]) / 255.0
				total += 0.2126 * red + 0.7152 * green + 0.0722 * blue
				count += 1
			}
		}
		return count > 0 ? total / Double(count) : nil
	}
}

@available(macOS 12.3, *)
func runMaskProbe() async throws {
	let outputPath = readRequiredString("MASK_PROBE_OUTPUT")
	let phasePath = readRequiredString("MASK_PROBE_PHASE_PATH")
	let readyPath = readOptionalString("MASK_PROBE_READY_PATH")
	let screenshotPath = readOptionalString("MASK_PROBE_SCREENSHOT_PATH")
	let releasedScreenshotPath = readOptionalString("MASK_PROBE_RELEASED_SCREENSHOT_PATH")
	let point = readRequiredPoint("MASK_PROBE_POINT")
	let durationMs = readInt("MASK_PROBE_DURATION_MS", default: 1_400)
	let rateHz = readInt("MASK_PROBE_RATE_HZ", default: 60)
	let minPhaseSamples = readInt("MASK_PROBE_MIN_PHASE_SAMPLES", default: 3)
	let pollMs = readInt("MASK_PROBE_POLL_MS", default: 20)
	let stopCapture = readInt("MASK_PROBE_STOP_CAPTURE", default: 0) == 1

	let content = try await SCShareableContent.current
	guard
		let display = content.displays.first(where: { $0.displayID == CGMainDisplayID() })
			?? content.displays.first
	else {
		fputs("missing shareable display for mask probe\n", stderr)
		exit(1)
	}

	let configuration = SCStreamConfiguration()
	configuration.width = display.width
	configuration.height = display.height
	configuration.pixelFormat = kCVPixelFormatType_32BGRA
	configuration.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(max(1, rateHz)))
	configuration.queueDepth = 4

	let filter = SCContentFilter(display: display, excludingWindows: [])
	let capture = MaskProbeCapture(
		outputPath: outputPath,
		phasePath: phasePath,
		readyPath: readyPath,
		screenshotPath: screenshotPath,
		releasedScreenshotPath: releasedScreenshotPath,
		point: point,
		displayFrame: display.frame
	)
	let stream = SCStream(filter: filter, configuration: configuration, delegate: nil)
	try stream.addStreamOutput(
		capture,
		type: .screen,
		sampleHandlerQueue: DispatchQueue(label: "ink.hack.rsnap.mask-probe", qos: .userInteractive)
	)
	try await stream.startCapture()
	let started = ProcessInfo.processInfo.systemUptime
	while (ProcessInfo.processInfo.systemUptime - started) * 1_000 < Double(max(1, durationMs)) {
		if capture.hasRequiredSamples(
			minDraggingSamples: minPhaseSamples,
			minReleasedSamples: minPhaseSamples
		) {
			break
		}
		try await Task.sleep(nanoseconds: UInt64(max(1, pollMs)) * 1_000_000)
	}
	if stopCapture {
		try await stream.stopCapture()
	}
	capture.writeSamples()
}

if #available(macOS 12.3, *) {
	let semaphore = DispatchSemaphore(value: 0)
	Task {
		do {
			try await runMaskProbe()
		} catch {
			fputs("mask probe failed: \(error)\n", stderr)
			exit(1)
		}
		semaphore.signal()
	}
	semaphore.wait()
} else {
	fputs("mask probe requires macOS 12.3 or newer\n", stderr)
	exit(1)
}
