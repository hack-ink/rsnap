import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

extension CaptureSessionController {
	struct RecognizeTextRun {
		let captureID: UInt64
		let startedAt: TimeInterval
		let recognitionLevel: String
		let usesLanguageCorrection: Bool
		let automaticallyDetectsLanguage: Bool
	}

	struct RecognizeTextPasteboardTiming {
		let clearMilliseconds: Double
		let writeMilliseconds: Double
	}

	func performRecognizeText() throws {
		guard session != nil else {
			return
		}
		guard recognizeTextActionEnabled else {
			try setHostStatusMessage(recognizeTextBlockedMessage())
			refreshOverlay()
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
		let cacheHit = captureImage.cacheHit
		hostEffectJobGeneration &+= 1
		let jobGeneration = hostEffectJobGeneration
		try setHostStatusMessage("Recognizing text...")
		refreshOverlay()
		NativeHostTelemetry.captureEvent(
			"capture.recognize_text_queued",
			captureID: run.captureID,
			detail:
				"width=\(cgImage.width) height=\(cgImage.height) cacheHit=\(cacheHit) jobGeneration=\(jobGeneration)"
		)

		textRecognitionEngine.recognize(
			cgImage: cgImage,
			configuration: NativeTextRecognitionConfiguration(
				recognitionLevel: run.recognitionLevel,
				usesLanguageCorrection: run.usesLanguageCorrection,
				automaticallyDetectsLanguage: run.automaticallyDetectsLanguage
			)
		) { [weak self] result in
			self?.finishRecognizeTextJob(
				result,
				run: run,
				cgImage: cgImage,
				captureImageMilliseconds: captureImageMilliseconds,
				cacheHit: cacheHit,
				jobGeneration: jobGeneration
			)
		}
	}

	func finishRecognizeTextJob(
		_ result: NativeTextRecognitionResult,
		run: RecognizeTextRun,
		cgImage: CGImage,
		captureImageMilliseconds: Double,
		cacheHit: Bool,
		jobGeneration: UInt64
	) {
		guard hostEffectJobGeneration == jobGeneration, session != nil else {
			NativeHostTelemetry.captureEvent(
				"capture.recognize_text_discarded",
				captureID: run.captureID,
				outcome: "stale_job",
				detail:
					"jobGeneration=\(jobGeneration) currentGeneration=\(hostEffectJobGeneration)"
			)
			return
		}
		if let failureDescription = result.failureDescription {
			recordRecognizeTextTiming(
				run: run,
				captureImageMilliseconds: captureImageMilliseconds,
				visionRequestMilliseconds: result.visionRequestMilliseconds,
				resultProcessingMilliseconds: result.processingMilliseconds,
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
			NativeHostTelemetry.captureWarning(
				"capture.recognize_text_failed",
				captureID: run.captureID,
				stage: "vision_request",
				error: failureDescription
			)
			try? setHostStatusMessage("Could not recognize text.")
			refreshOverlay()
			return
		}

		guard
			let pasteboardTiming = try? writeRecognizedTextIfNeeded(
				result.text,
				run: run,
				cgImage: cgImage,
				captureImageMilliseconds: captureImageMilliseconds,
				visionRequestMilliseconds: result.visionRequestMilliseconds,
				result: result,
				cacheHit: cacheHit
			)
		else {
			return
		}

		recordRecognizeTextTiming(
			run: run,
			captureImageMilliseconds: captureImageMilliseconds,
			visionRequestMilliseconds: result.visionRequestMilliseconds,
			resultProcessingMilliseconds: result.processingMilliseconds,
			clearPasteboardMilliseconds: pasteboardTiming.clearMilliseconds,
			writePasteboardMilliseconds: pasteboardTiming.writeMilliseconds,
			success: true,
			outcome: result.text.isEmpty ? "no_text" : "text_ready",
			failureStage: "none",
			width: cgImage.width,
			height: cgImage.height,
			observationCount: result.observationCount,
			recognizedLines: result.recognizedLines,
			recognizedCharacters: result.recognizedCharacters,
			cacheHit: cacheHit
		)

		if result.text.isEmpty == false {
			ocrCompletionSound.play()
		}

		let message =
			result.text.isEmpty
			? "No text was recognized."
			: "Recognized text copied to clipboard."
		do {
			try session?.send(report: .hostEffectCompleted(.recognizeText))
			try session?.send(report: .statusMessage(message))
			completedHostEffect = .recognizeText
			tearDownCapture()
		} catch {
			NativeHostTelemetry.captureWarning(
				"capture.host_effect_complete_failed",
				captureID: run.captureID,
				stage: String(describing: HostEffectKind.recognizeText),
				error: String(describing: error)
			)
			try? setHostStatusMessage(message)
			refreshOverlay()
		}
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

	func writeRecognizedTextIfNeeded(
		_ text: String,
		run: RecognizeTextRun,
		cgImage: CGImage,
		captureImageMilliseconds: Double,
		visionRequestMilliseconds: Double,
		result: NativeTextRecognitionResult,
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
				observationCount: result.observationCount,
				recognizedLines: result.recognizedLines,
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
