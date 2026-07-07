import CoreGraphics
import Foundation
import Vision

struct NativeTextRecognitionConfiguration: Sendable {
	let recognitionLevel: String
	let usesLanguageCorrection: Bool
	let automaticallyDetectsLanguage: Bool
}

struct NativeTextRecognitionResult: @unchecked Sendable {
	let text: String
	let observationCount: Int
	let recognizedLines: Int
	let recognizedCharacters: Int
	let visionRequestMilliseconds: Double
	let processingMilliseconds: Double
	let failureDescription: String?
}

final class NativeTextRecognitionEngine: @unchecked Sendable {
	private let queue = DispatchQueue(
		label: "ink.hack.rsnap.recognize-text",
		qos: .userInitiated
	)
	private let lock = NSLock()
	private var prewarmStarted = false

	func prewarm(reason: String, captureID: UInt64 = 0) {
		lock.lock()
		guard prewarmStarted == false else {
			lock.unlock()
			return
		}
		prewarmStarted = true
		lock.unlock()

		queue.async {
			let startedAt = ProcessInfo.processInfo.systemUptime
			let result = Self.perform(
				cgImage: Self.prewarmImage(),
				configuration: Self.defaultConfiguration
			)
			NativeHostTelemetry.captureEvent(
				"capture.recognize_text_prewarm",
				captureID: captureID,
				outcome: result.failureDescription == nil ? "success" : "failed",
				detail:
					"reason=\(reason) totalMs=\(String(format: "%.2f", NativeHostTelemetry.milliseconds(since: startedAt))) visionRequestMs=\(String(format: "%.2f", result.visionRequestMilliseconds))"
			)
		}
	}

	func recognize(
		cgImage: CGImage,
		configuration: NativeTextRecognitionConfiguration,
		completion: @escaping @MainActor @Sendable (NativeTextRecognitionResult) -> Void
	) {
		queue.async {
			let result = Self.perform(
				cgImage: cgImage,
				configuration: configuration
			)
			Task { @MainActor in
				completion(result)
			}
		}
	}

	private static let defaultConfiguration = NativeTextRecognitionConfiguration(
		recognitionLevel: "accurate",
		usesLanguageCorrection: true,
		automaticallyDetectsLanguage: true
	)

	private static func perform(
		cgImage: CGImage,
		configuration: NativeTextRecognitionConfiguration
	) -> NativeTextRecognitionResult {
		let request = VNRecognizeTextRequest()
		request.recognitionLevel = .accurate
		request.usesLanguageCorrection = configuration.usesLanguageCorrection
		request.automaticallyDetectsLanguage = configuration.automaticallyDetectsLanguage

		let handler = VNImageRequestHandler(cgImage: cgImage)
		let visionStartedAt = ProcessInfo.processInfo.systemUptime
		do {
			try handler.perform([request])
		} catch {
			return NativeTextRecognitionResult(
				text: "",
				observationCount: 0,
				recognizedLines: 0,
				recognizedCharacters: 0,
				visionRequestMilliseconds: NativeHostTelemetry.milliseconds(
					since: visionStartedAt),
				processingMilliseconds: 0,
				failureDescription: String(describing: error)
			)
		}

		let visionRequestMilliseconds = NativeHostTelemetry.milliseconds(since: visionStartedAt)
		let resultProcessingStartedAt = ProcessInfo.processInfo.systemUptime
		let observations = request.results ?? []
		let recognizedLines = observations.compactMap { observation -> String? in
			guard let line = observation.topCandidates(1).first?.string,
				line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
			else {
				return nil
			}
			return line
		}
		let text = recognizedLines.joined(separator: "\n")
		return NativeTextRecognitionResult(
			text: text,
			observationCount: observations.count,
			recognizedLines: recognizedLines.count,
			recognizedCharacters: text.count,
			visionRequestMilliseconds: visionRequestMilliseconds,
			processingMilliseconds: NativeHostTelemetry.milliseconds(
				since: resultProcessingStartedAt),
			failureDescription: nil
		)
	}

	private static func prewarmImage() -> CGImage {
		let width = 8
		let height = 8
		let colorSpace = CGColorSpaceCreateDeviceGray()
		guard
			let context = CGContext(
				data: nil,
				width: width,
				height: height,
				bitsPerComponent: 8,
				bytesPerRow: width,
				space: colorSpace,
				bitmapInfo: CGImageAlphaInfo.none.rawValue
			)
		else {
			fatalError("Failed to create OCR prewarm bitmap context.")
		}
		context.setFillColor(gray: 1, alpha: 1)
		context.fill(CGRect(x: 0, y: 0, width: width, height: height))
		guard let image = context.makeImage() else {
			fatalError("Failed to create OCR prewarm image.")
		}
		return image
	}
}
