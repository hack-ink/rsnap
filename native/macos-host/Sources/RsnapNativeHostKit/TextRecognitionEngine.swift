import CoreGraphics
import CoreText
import Foundation

struct TextRecognitionConfiguration: Sendable {
	let recognitionLevel: String
	let usesLanguageCorrection: Bool
	let automaticallyDetectsLanguage: Bool
}

struct TextRecognitionResult: @unchecked Sendable {
	let text: String
	let observationCount: Int
	let recognizedLines: Int
	let recognizedCharacters: Int
	let visionRequestMilliseconds: Double
	let processingMilliseconds: Double
	let failureDescription: String?
	let computePath: String
	let workerAttempts: Int
}

package final class TextRecognitionEngine: @unchecked Sendable {
	private let queue = DispatchQueue(
		label: "ink.hack.rsnap.recognize-text",
		qos: .userInitiated
	)
	private let lock = NSLock()
	private let worker: TextRecognitionWorker?
	private var prewarmStarted = false

	init(executableURL: URL? = Bundle.main.executableURL) {
		worker = executableURL.map { TextRecognitionWorker(executableURL: $0) }
	}

	func prewarm(reason: String, captureID: UInt64 = 0) {
		lock.lock()
		guard prewarmStarted == false else {
			lock.unlock()
			return
		}
		prewarmStarted = true
		lock.unlock()

		queue.async { [self] in
			let startedAt = ProcessInfo.processInfo.systemUptime
			let result = performWithWorker(
				cgImage: Self.prewarmImage(),
				configuration: Self.defaultConfiguration
			)
			NativeHostTelemetry.captureEvent(
				"capture.recognize_text_prewarm",
				captureID: captureID,
				outcome: result.failureDescription == nil ? "success" : "failed",
				detail:
					"reason=\(reason) totalMs=\(String(format: "%.2f", NativeHostTelemetry.milliseconds(since: startedAt))) visionRequestMs=\(String(format: "%.2f", result.visionRequestMilliseconds)) computePath=\(result.computePath) workerAttempts=\(result.workerAttempts)"
			)
		}
	}

	func recognize(
		cgImage: CGImage,
		configuration: TextRecognitionConfiguration,
		completion: @escaping @MainActor @Sendable (TextRecognitionResult) -> Void
	) {
		queue.async { [self] in
			let result = performWithWorker(
				cgImage: cgImage,
				configuration: configuration
			)
			Task { @MainActor in
				completion(result)
			}
		}
	}

	private static let defaultConfiguration = TextRecognitionConfiguration(
		recognitionLevel: "accurate",
		usesLanguageCorrection: true,
		automaticallyDetectsLanguage: true
	)

	private func performWithWorker(
		cgImage: CGImage,
		configuration: TextRecognitionConfiguration
	) -> TextRecognitionResult {
		// Vision can retain an E5 program after E5RT reports that it must be recompiled.
		// Vision has no public reset API. The worker retains the warm model during healthy
		// requests and gives it a new process lifecycle when E5RT requires recompilation.
		guard let worker else {
			return Self.failureResult(
				description: "Rsnap could not resolve its OCR worker executable.",
				workerAttempts: 0
			)
		}
		guard let snapshot = NativeHostImageBridge.rgbaSnapshot(from: cgImage) else {
			return Self.failureResult(
				description: "Rsnap could not serialize the OCR image.",
				workerAttempts: 0
			)
		}

		let input = TextRecognitionHelper.Input(
			width: snapshot.width,
			height: snapshot.height,
			rgba: snapshot.rgba,
			usesLanguageCorrection: configuration.usesLanguageCorrection,
			automaticallyDetectsLanguage: configuration.automaticallyDetectsLanguage
		)
		let encodedInput: Data
		do {
			encodedInput = try TextRecognitionHelper.encode(input)
		} catch {
			return Self.failureResult(
				description: "Rsnap could not encode the OCR helper request: \(error)",
				workerAttempts: 0
			)
		}

		var firstFailure: String?
		var visionRequestMilliseconds = 0.0
		for attempt in 1...TextRecognitionHelper.maximumWorkerAttempts {
			let output: TextRecognitionHelper.Output
			do {
				output = try worker.perform(encodedInput: encodedInput)
			} catch {
				let failure = "Rsnap OCR worker failed: \(error)"
				if attempt < TextRecognitionHelper.maximumWorkerAttempts {
					firstFailure = failure
					worker.restart()
					continue
				}
				return Self.failureResult(
					description: Self.combinedFailure(firstFailure, finalFailure: failure),
					visionRequestMilliseconds: visionRequestMilliseconds,
					workerAttempts: attempt
				)
			}
			visionRequestMilliseconds += output.visionRequestMilliseconds

			if let failure = output.failureDescription {
				if attempt < TextRecognitionHelper.maximumWorkerAttempts,
					TextRecognitionHelper.isE5RecompileRequired(failure)
				{
					firstFailure = failure
					worker.restart()
					continue
				}
				return Self.failureResult(
					description: Self.combinedFailure(firstFailure, finalFailure: failure),
					visionRequestMilliseconds: visionRequestMilliseconds,
					workerAttempts: attempt
				)
			}

			return TextRecognitionResult(
				text: output.text,
				observationCount: output.observationCount,
				recognizedLines: output.recognizedLines,
				recognizedCharacters: output.recognizedCharacters,
				visionRequestMilliseconds: visionRequestMilliseconds,
				processingMilliseconds: output.processingMilliseconds,
				failureDescription: nil,
				computePath: "restartable_neural_engine_worker",
				workerAttempts: attempt
			)
		}

		return Self.failureResult(
			description: "Rsnap OCR worker exhausted its process attempts.",
			workerAttempts: TextRecognitionHelper.maximumWorkerAttempts
		)
	}

	private static func combinedFailure(
		_ firstFailure: String?,
		finalFailure: String
	) -> String {
		firstFailure.map {
			"firstAttempt=\($0); finalAttempt=\(finalFailure)"
		} ?? finalFailure
	}

	private static func failureResult(
		description: String,
		visionRequestMilliseconds: Double = 0,
		workerAttempts: Int
	) -> TextRecognitionResult {
		TextRecognitionResult(
			text: "",
			observationCount: 0,
			recognizedLines: 0,
			recognizedCharacters: 0,
			visionRequestMilliseconds: visionRequestMilliseconds,
			processingMilliseconds: 0,
			failureDescription: description,
			computePath: "restartable_neural_engine_worker",
			workerAttempts: workerAttempts
		)
	}

	private static func prewarmImage() -> CGImage {
		let width = 512
		let height = 128
		let colorSpace = CGColorSpaceCreateDeviceRGB()
		guard
			let context = CGContext(
				data: nil,
				width: width,
				height: height,
				bitsPerComponent: 8,
				bytesPerRow: width * 4,
				space: colorSpace,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			fatalError("Failed to create OCR prewarm bitmap context.")
		}
		context.setFillColor(CGColor(gray: 1, alpha: 1))
		context.fill(CGRect(x: 0, y: 0, width: width, height: height))
		let font = CTFontCreateWithName("Helvetica" as CFString, 52, nil)
		let line = CTLineCreateWithAttributedString(
			NSAttributedString(
				string: "Rsnap OCR",
				attributes: [
					kCTFontAttributeName as NSAttributedString.Key: font,
					kCTForegroundColorAttributeName as NSAttributedString.Key: CGColor(
						gray: 0,
						alpha: 1
					),
				]
			)
		)
		context.textPosition = CGPoint(x: 24, y: 38)
		CTLineDraw(line, context)
		guard let image = context.makeImage() else {
			fatalError("Failed to create OCR prewarm image.")
		}
		return image
	}
}
