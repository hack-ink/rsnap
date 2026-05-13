import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge
import Vision

extension CaptureSessionController {
	struct RecognizeTextRun {
		let captureID: UInt64
		let startedAt: TimeInterval
		let recognitionLevel: String
		let usesLanguageCorrection: Bool
		let automaticallyDetectsLanguage: Bool
	}

	struct RecognizeTextResult {
		let observations: [VNRecognizedTextObservation]
		let recognizedLines: [String]
		let text: String
		let processingMilliseconds: Double
	}

	struct RecognizeTextPasteboardTiming {
		let clearMilliseconds: Double
		let writeMilliseconds: Double
	}

	func performRecognizeText() throws {
		guard let session else {
			return
		}
		let run = RecognizeTextRun(
			captureID: currentCaptureTelemetryID,
			startedAt: ProcessInfo.processInfo.systemUptime,
			recognitionLevel: "accurate",
			usesLanguageCorrection: true,
			automaticallyDetectsLanguage: true
		)
		let captureImageStartedAt = ProcessInfo.processInfo.systemUptime
		guard
			let captureImage = try recognizeTextCaptureImage(
				run: run,
				captureImageStartedAt: captureImageStartedAt
			)
		else {
			return
		}
		let cgImage = captureImage.image
		let captureImageMilliseconds = captureImage.captureImageMilliseconds
		let request = recognizeTextRequest(run: run)
		let visionRequestMilliseconds = try performRecognizeTextRequest(
			request,
			cgImage: cgImage,
			run: run,
			captureImageMilliseconds: captureImageMilliseconds,
			cacheHit: captureImage.cacheHit
		)
		let result = recognizeTextResult(from: request)
		guard
			let pasteboardTiming = try writeRecognizedTextIfNeeded(
				result.text,
				run: run,
				cgImage: cgImage,
				captureImageMilliseconds: captureImageMilliseconds,
				visionRequestMilliseconds: visionRequestMilliseconds,
				result: result,
				cacheHit: captureImage.cacheHit
			)
		else {
			return
		}

		recordRecognizeTextTiming(
			run: run,
			captureImageMilliseconds: captureImageMilliseconds,
			visionRequestMilliseconds: visionRequestMilliseconds,
			resultProcessingMilliseconds: result.processingMilliseconds,
			clearPasteboardMilliseconds: pasteboardTiming.clearMilliseconds,
			writePasteboardMilliseconds: pasteboardTiming.writeMilliseconds,
			success: true,
			outcome: result.text.isEmpty ? "no_text" : "text_ready",
			failureStage: "none",
			width: cgImage.width,
			height: cgImage.height,
			observationCount: result.observations.count,
			recognizedLines: result.recognizedLines.count,
			recognizedCharacters: result.text.count,
			cacheHit: captureImage.cacheHit
		)

		if result.text.isEmpty == false {
			ocrCompletionSound.play()
		}

		try session.send(report: .hostEffectCompleted(.recognizeText))
		let message =
			result.text.isEmpty
			? "No text was recognized."
			: "Recognized text copied to clipboard."
		try session.send(report: .statusMessage(message))
		completedHostEffect = .recognizeText
	}

	func recognizeTextCaptureImage(
		run: RecognizeTextRun,
		captureImageStartedAt: TimeInterval
	) throws -> PreparedRecognizeTextCaptureImage? {
		guard
			let captureImage = try preparedRecognizeTextCaptureImage(
				captureImageStartedAt: captureImageStartedAt
			)
		else {
			recordRecognizeTextTiming(
				run: run,
				captureImageMilliseconds: NativeHostTelemetry.milliseconds(
					since: captureImageStartedAt),
				visionRequestMilliseconds: 0,
				resultProcessingMilliseconds: 0,
				clearPasteboardMilliseconds: 0,
				writePasteboardMilliseconds: 0,
				success: false,
				outcome: "recognize_error",
				failureStage: "capture_image",
				width: 0,
				height: 0,
				observationCount: 0,
				recognizedLines: 0,
				recognizedCharacters: 0,
				cacheHit: false
			)
			try sendHostStatusMessage("Could not capture the frozen selection.")
			return nil
		}
		return captureImage
	}

	func recognizeTextRequest(run: RecognizeTextRun) -> VNRecognizeTextRequest {
		let request = VNRecognizeTextRequest()
		request.recognitionLevel = .accurate
		request.usesLanguageCorrection = run.usesLanguageCorrection
		request.automaticallyDetectsLanguage = run.automaticallyDetectsLanguage
		return request
	}

	func performRecognizeTextRequest(
		_ request: VNRecognizeTextRequest,
		cgImage: CGImage,
		run: RecognizeTextRun,
		captureImageMilliseconds: Double,
		cacheHit: Bool
	) throws -> Double {
		let handler = VNImageRequestHandler(cgImage: cgImage)
		let visionStartedAt = ProcessInfo.processInfo.systemUptime
		do {
			try handler.perform([request])
		} catch {
			let visionRequestMilliseconds = NativeHostTelemetry.milliseconds(since: visionStartedAt)
			recordRecognizeTextTiming(
				run: run,
				captureImageMilliseconds: captureImageMilliseconds,
				visionRequestMilliseconds: visionRequestMilliseconds,
				resultProcessingMilliseconds: 0,
				clearPasteboardMilliseconds: 0,
				writePasteboardMilliseconds: 0,
				success: false,
				outcome: "recognize_error",
				failureStage: "vision_request",
				width: cgImage.width,
				height: cgImage.height,
				observationCount: 0,
				recognizedLines: 0,
				recognizedCharacters: 0,
				cacheHit: cacheHit
			)
			throw error
		}
		return NativeHostTelemetry.milliseconds(since: visionStartedAt)
	}

	func recognizeTextResult(from request: VNRecognizeTextRequest) -> RecognizeTextResult {
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
		return RecognizeTextResult(
			observations: observations,
			recognizedLines: recognizedLines,
			text: recognizedLines.joined(separator: "\n"),
			processingMilliseconds: NativeHostTelemetry.milliseconds(
				since: resultProcessingStartedAt)
		)
	}

	func writeRecognizedTextIfNeeded(
		_ text: String,
		run: RecognizeTextRun,
		cgImage: CGImage,
		captureImageMilliseconds: Double,
		visionRequestMilliseconds: Double,
		result: RecognizeTextResult,
		cacheHit: Bool
	) throws -> RecognizeTextPasteboardTiming? {
		guard text.isEmpty == false else {
			return RecognizeTextPasteboardTiming(clearMilliseconds: 0, writeMilliseconds: 0)
		}
		let pasteboard = NSPasteboard.general
		let clearPasteboardStartedAt = ProcessInfo.processInfo.systemUptime
		pasteboard.clearContents()
		let clearPasteboardMilliseconds =
			NativeHostTelemetry.milliseconds(since: clearPasteboardStartedAt)
		let writePasteboardStartedAt = ProcessInfo.processInfo.systemUptime
		guard pasteboard.setString(text, forType: .string) else {
			let writePasteboardMilliseconds =
				NativeHostTelemetry.milliseconds(since: writePasteboardStartedAt)
			recordRecognizeTextTiming(
				run: run,
				captureImageMilliseconds: captureImageMilliseconds,
				visionRequestMilliseconds: visionRequestMilliseconds,
				resultProcessingMilliseconds: result.processingMilliseconds,
				clearPasteboardMilliseconds: clearPasteboardMilliseconds,
				writePasteboardMilliseconds: writePasteboardMilliseconds,
				success: false,
				outcome: "recognize_error",
				failureStage: "pasteboard_write",
				width: cgImage.width,
				height: cgImage.height,
				observationCount: result.observations.count,
				recognizedLines: result.recognizedLines.count,
				recognizedCharacters: text.count,
				cacheHit: cacheHit
			)
			try sendHostStatusMessage("Could not copy recognized text.")
			return nil
		}
		return RecognizeTextPasteboardTiming(
			clearMilliseconds: clearPasteboardMilliseconds,
			writeMilliseconds: NativeHostTelemetry.milliseconds(since: writePasteboardStartedAt)
		)
	}

	func recordRecognizeTextTiming(
		run: RecognizeTextRun,
		captureImageMilliseconds: Double,
		visionRequestMilliseconds: Double,
		resultProcessingMilliseconds: Double,
		clearPasteboardMilliseconds: Double,
		writePasteboardMilliseconds: Double,
		success: Bool,
		outcome: String,
		failureStage: String,
		width: Int,
		height: Int,
		observationCount: Int,
		recognizedLines: Int,
		recognizedCharacters: Int,
		cacheHit: Bool
	) {
		NativeHostTelemetry.recognizeTextTiming(
			captureID: run.captureID,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: run.startedAt),
			captureImageMilliseconds: captureImageMilliseconds,
			visionRequestMilliseconds: visionRequestMilliseconds,
			resultProcessingMilliseconds: resultProcessingMilliseconds,
			clearPasteboardMilliseconds: clearPasteboardMilliseconds,
			writePasteboardMilliseconds: writePasteboardMilliseconds,
			success: success,
			outcome: outcome,
			failureStage: failureStage,
			width: width,
			height: height,
			observationCount: observationCount,
			recognizedLines: recognizedLines,
			recognizedCharacters: recognizedCharacters,
			recognitionLevel: run.recognitionLevel,
			languageCorrection: run.usesLanguageCorrection,
			automaticLanguageDetection: run.automaticallyDetectsLanguage,
			cacheHit: cacheHit
		)
	}
}
