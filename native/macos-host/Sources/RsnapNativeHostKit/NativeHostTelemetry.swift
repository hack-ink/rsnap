import Foundation
import OSLog

enum NativeHostTelemetry {
	static let subsystem = Bundle.main.bundleIdentifier ?? "ink.hack.rsnap"
	static let schema = "rsnap.native_host.telemetry/1"
	static let runID = UUID().uuidString
	private static let distributionEmitQueue = DispatchQueue(
		label: "ink.hack.rsnap.telemetry.distribution",
		qos: .utility
	)
	private static let lifecycleLogger = Logger(
		subsystem: subsystem, category: "Lifecycle")
	private static let captureLogger = Logger(
		subsystem: subsystem, category: "Capture")
	private static let captureTimingLogger = Logger(
		subsystem: subsystem, category: "CaptureTiming")
	private static let frozenAuthorityLogger = Logger(
		subsystem: subsystem, category: "FrozenFrameAuthority")
	private static let liveChromeLogger = Logger(
		subsystem: subsystem, category: "LiveChromeTelemetry")

	static func milliseconds(since startUptime: TimeInterval) -> Double {
		max(0, ProcessInfo.processInfo.systemUptime - startUptime) * 1_000
	}

	static func lifecycleEvent(
		_ event: String,
		outcome: String = "success",
		detail: String = "none"
	) {
		lifecycleLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) event=\(event, privacy: .public) outcome=\(outcome, privacy: .public) detail=\(detail, privacy: .public)"
		)
	}

	static func lifecycleWarning(
		_ event: String,
		outcome: String = "failure",
		detail: String = "none"
	) {
		lifecycleLogger.warning(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) event=\(event, privacy: .public) outcome=\(outcome, privacy: .public) detail=\(detail, privacy: .public)"
		)
	}

	static func lifecycleDebug(
		_ event: String,
		outcome: String = "success",
		detail: String = "none"
	) {
		lifecycleLogger.debug(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) event=\(event, privacy: .public) outcome=\(outcome, privacy: .public) detail=\(detail, privacy: .public)"
		)
	}

	static func captureEvent(
		_ event: String,
		captureID: UInt64,
		outcome: String = "success",
		detail: String = "none"
	) {
		captureLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=\(event, privacy: .public) outcome=\(outcome, privacy: .public) detail=\(detail, privacy: .public)"
		)
	}

	static func captureWarning(
		_ event: String,
		captureID: UInt64,
		stage: String,
		error: String
	) {
		captureLogger.warning(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=\(event, privacy: .public) stage=\(stage, privacy: .public) error=\(error, privacy: .public)"
		)
	}

	static func frozenAuthorityWarning(
		_ event: String,
		captureID: UInt64,
		source: String,
		displayID: UInt32,
		error: String
	) {
		frozenAuthorityLogger.warning(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) source=\(source, privacy: .public) event=\(event, privacy: .public) displayID=\(displayID, privacy: .public) error=\(error, privacy: .public)"
		)
	}

	static func distribution(
		_ name: String,
		category: String,
		unit: String = "ms",
		batchSize: Int = 120
	) -> DistributionMetric {
		DistributionMetric(
			name: name,
			category: category,
			unit: unit,
			batchSize: batchSize
		)
	}

	static func liveChromeRefreshTarget(
		captureID: UInt64,
		targetHz: Int,
		frameBudgetMilliseconds: Double,
		hudGlassEnabled: Bool,
		hudGlassMode: String,
		liquidGlassStyle: String,
		liquidGlassAvailable: Bool
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=live_chrome.refresh_target targetHz=\(targetHz, privacy: .public) frameBudgetMs=\(frameBudgetMilliseconds, format: .fixed(precision: 2), privacy: .public) hudGlassEnabled=\(hudGlassEnabled, privacy: .public) hudGlassMode=\(hudGlassMode, privacy: .public) liquidGlassStyle=\(liquidGlassStyle, privacy: .public) liquidGlassAvailable=\(liquidGlassAvailable, privacy: .public)"
		)
	}

	static func liveChromeFirstRgbSample(
		captureID: UInt64,
		totalMilliseconds: Double,
		refreshCount: UInt64,
		source: String,
		hasPatch: Bool,
		includeLoupePatch: Bool
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=live_chrome.first_rgb_sample totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) refreshCount=\(refreshCount, privacy: .public) source=\(source, privacy: .public) hasPatch=\(hasPatch, privacy: .public) includeLoupePatch=\(includeLoupePatch, privacy: .public)"
		)
	}

	static func liveChromeSampleFeedStarted(
		captureID: UInt64,
		targetHz: Int
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=live_chrome.sample_feed_started targetHz=\(targetHz, privacy: .public)"
		)
	}

	static func liveStreamFirstRgbSample(
		captureID: UInt64,
		totalMilliseconds: Double,
		hasPatch: Bool
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=live_chrome.live_stream_first_rgb_sample totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) hasPatch=\(hasPatch, privacy: .public)"
		)
	}

	static func liveStreamAuthorityPrepared(
		totalMilliseconds: Double,
		reason: String
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) event=live_chrome.live_stream_authority_prepared totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) reason=\(reason, privacy: .public)"
		)
	}

	static func liveStreamSample(
		captureID: UInt64,
		totalMilliseconds: Double,
		outcome: String,
		frameAgeMilliseconds: Double,
		hasPatch: Bool
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=live_chrome.live_stream_sample totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) outcome=\(outcome, privacy: .public) frameAgeMs=\(frameAgeMilliseconds, format: .fixed(precision: 2), privacy: .public) hasPatch=\(hasPatch, privacy: .public)"
		)
	}

	static func liveChromeBackgroundSample(
		captureID: UInt64,
		totalMilliseconds: Double,
		outcome: String,
		source: String,
		includeLoupePatch: Bool,
		immediate: Bool
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=live_chrome.background_sample totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) outcome=\(outcome, privacy: .public) source=\(source, privacy: .public) includeLoupePatch=\(includeLoupePatch, privacy: .public) immediate=\(immediate, privacy: .public)"
		)
	}

	static func liveChromeWindowSnapshotRefresh(
		captureID: UInt64,
		source: String,
		totalMilliseconds: Double,
		candidateWindowCount: Int,
		targetableWindowCount: Int,
		ownWindowCount: Int,
		ownTargetableWindowCount: Int,
		highLayerWindowCount: Int,
		tinyWindowCount: Int,
		transparentWindowCount: Int
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=live_chrome.window_snapshot_refresh source=\(source, privacy: .public) totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) candidateWindowCount=\(candidateWindowCount, privacy: .public) targetableWindowCount=\(targetableWindowCount, privacy: .public) ownWindowCount=\(ownWindowCount, privacy: .public) ownTargetableWindowCount=\(ownTargetableWindowCount, privacy: .public) highLayerWindowCount=\(highLayerWindowCount, privacy: .public) tinyWindowCount=\(tinyWindowCount, privacy: .public) transparentWindowCount=\(transparentWindowCount, privacy: .public)"
		)
	}

	static func liveStreamSelfCaptureExceptionUpdate(
		captureID: UInt64,
		previousWindowCount: Int,
		nextWindowCount: Int,
		samplerRebuilt: Bool
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=live_chrome.self_capture_exception_update previousWindowCount=\(previousWindowCount, privacy: .public) nextWindowCount=\(nextWindowCount, privacy: .public) samplerRebuilt=\(samplerRebuilt, privacy: .public)"
		)
	}

	static func liveChromeInputSummary(
		captureID: UInt64,
		reason: String,
		mouseEvents: Int,
		followTicks: Int,
		fastMoveAttempts: Int,
		fastMoveSuccesses: Int,
		loupeFastMoveAttempts: Int,
		loupeFastMoveSuccesses: Int,
		predictedMoves: Int,
		fallbackRefreshes: Int,
		immediateRefreshes: Int
	) {
		liveChromeLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=live_chrome.input_summary reason=\(reason, privacy: .public) mouseEvents=\(mouseEvents, privacy: .public) followTicks=\(followTicks, privacy: .public) fastMoveAttempts=\(fastMoveAttempts, privacy: .public) fastMoveSuccesses=\(fastMoveSuccesses, privacy: .public) loupeFastMoveAttempts=\(loupeFastMoveAttempts, privacy: .public) loupeFastMoveSuccesses=\(loupeFastMoveSuccesses, privacy: .public) predictedMoves=\(predictedMoves, privacy: .public) fallbackRefreshes=\(fallbackRefreshes, privacy: .public) immediateRefreshes=\(immediateRefreshes, privacy: .public)"
		)
	}

	static func liveSamplingWarmTiming(
		captureID: UInt64,
		source: String,
		totalMilliseconds: Double,
		frozenAuthorityStartMilliseconds: Double,
		liveStreamStartMilliseconds: Double,
		seedSampleMilliseconds: Double,
		sampleReady: Bool,
		screenCount: Int
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.live_sampling_warm source=\(source, privacy: .public) totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) frozenAuthorityStartMs=\(frozenAuthorityStartMilliseconds, format: .fixed(precision: 2), privacy: .public) liveStreamStartMs=\(liveStreamStartMilliseconds, format: .fixed(precision: 2), privacy: .public) seedSampleMs=\(seedSampleMilliseconds, format: .fixed(precision: 2), privacy: .public) sampleReady=\(sampleReady, privacy: .public) screenCount=\(screenCount, privacy: .public)"
		)
	}

	static func captureStartTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		warmMilliseconds: Double,
		windowSnapshotMilliseconds: Double,
		sessionSetupMilliseconds: Double,
		overlayShowMilliseconds: Double,
		initialSampleReady: Bool,
		screenCount: Int,
		windowCount: Int
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.start_capture totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) warmMs=\(warmMilliseconds, format: .fixed(precision: 2), privacy: .public) windowSnapshotMs=\(windowSnapshotMilliseconds, format: .fixed(precision: 2), privacy: .public) sessionSetupMs=\(sessionSetupMilliseconds, format: .fixed(precision: 2), privacy: .public) overlayShowMs=\(overlayShowMilliseconds, format: .fixed(precision: 2), privacy: .public) initialSampleReady=\(initialSampleReady, privacy: .public) screenCount=\(screenCount, privacy: .public) windowCount=\(windowCount, privacy: .public)"
		)
	}

	static func captureStartFailureTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		failureStage: String
	) {
		captureTimingLogger.warning(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.start_capture_failed totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) failureStage=\(failureStage, privacy: .public)"
		)
	}

	static func frozenAuthorityContentLookupTiming(
		captureID: UInt64,
		source: String,
		totalMilliseconds: Double,
		success: Bool,
		displayCount: Int,
		windowCount: Int
	) {
		frozenAuthorityLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) source=\(source, privacy: .public) event=capture_timing.frozen_authority_content_lookup totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) success=\(success, privacy: .public) displayCount=\(displayCount, privacy: .public) windowCount=\(windowCount, privacy: .public)"
		)
	}

	static func frozenAuthorityFirstFrameTiming(
		captureID: UInt64,
		source: String,
		displayID: UInt32,
		totalMilliseconds: Double,
		frameAgeMilliseconds: Double,
		sequence: UInt64,
		generation: UInt64,
		selfCaptureSafe: Bool,
		selfCaptureFilterComplete: Bool
	) {
		frozenAuthorityLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) source=\(source, privacy: .public) event=capture_timing.frozen_authority_first_frame displayID=\(displayID, privacy: .public) totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) frameAgeMs=\(frameAgeMilliseconds, format: .fixed(precision: 2), privacy: .public) generation=\(generation, privacy: .public) sequence=\(sequence, privacy: .public) selfCaptureSafe=\(selfCaptureSafe, privacy: .public) selfCaptureFilterComplete=\(selfCaptureFilterComplete, privacy: .public)"
		)
	}

	static func freezeCommitTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		snapshotWaitMilliseconds: Double,
		baseImageMilliseconds: Double,
		presentMilliseconds: Double,
		frameAgeMilliseconds: Double,
		displayID: UInt32,
		sequence: UInt64,
		snapshotSource: String,
		snapshotGeneration: UInt64,
		selfCaptureSafe: Bool,
		selfCaptureFilterComplete: Bool,
		hadLatchToken: Bool,
		baseReady: Bool
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.freeze_commit totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) snapshotWaitMs=\(snapshotWaitMilliseconds, format: .fixed(precision: 2), privacy: .public) baseImageMs=\(baseImageMilliseconds, format: .fixed(precision: 2), privacy: .public) presentMs=\(presentMilliseconds, format: .fixed(precision: 2), privacy: .public) frameAgeMs=\(frameAgeMilliseconds, format: .fixed(precision: 2), privacy: .public) displayID=\(displayID, privacy: .public) generation=\(snapshotGeneration, privacy: .public) sequence=\(sequence, privacy: .public) snapshotSource=\(snapshotSource, privacy: .public) selfCaptureSafe=\(selfCaptureSafe, privacy: .public) selfCaptureFilterComplete=\(selfCaptureFilterComplete, privacy: .public) hadLatchToken=\(hadLatchToken, privacy: .public) baseReady=\(baseReady, privacy: .public)"
		)
	}

	static func freezeCommitFailureTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		snapshotWaitMilliseconds: Double,
		hadLatchToken: Bool
	) {
		captureTimingLogger.warning(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.freeze_commit_failed totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) snapshotWaitMs=\(snapshotWaitMilliseconds, format: .fixed(precision: 2), privacy: .public) hadLatchToken=\(hadLatchToken, privacy: .public)"
		)
	}

	static func frozenFirstDisplayHandoffTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		materialMilliseconds: Double,
		liveRendererStopMilliseconds: Double,
		displayMilliseconds: Double,
		toolbarVisible: Bool,
		toolbarItemCount: Int,
		usesLiquidHudGlass: Bool,
		usesClassicHudGlass: Bool,
		liquidGlassAvailable: Bool,
		frozenToolbarLiquidGlassVisible: Bool,
		frozenToolbarLiquidGlassContentDrawn: Bool,
		frozenSelectionEditable: Bool,
		pendingFrameDisplayed: Bool
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.frozen_first_display_handoff totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) materialMs=\(materialMilliseconds, format: .fixed(precision: 2), privacy: .public) liveRendererStopMs=\(liveRendererStopMilliseconds, format: .fixed(precision: 2), privacy: .public) displayMs=\(displayMilliseconds, format: .fixed(precision: 2), privacy: .public) toolbarVisible=\(toolbarVisible, privacy: .public) toolbarItemCount=\(toolbarItemCount, privacy: .public) usesLiquidHudGlass=\(usesLiquidHudGlass, privacy: .public) usesClassicHudGlass=\(usesClassicHudGlass, privacy: .public) liquidGlassAvailable=\(liquidGlassAvailable, privacy: .public) frozenToolbarLiquidGlassVisible=\(frozenToolbarLiquidGlassVisible, privacy: .public) frozenToolbarLiquidGlassContentDrawn=\(frozenToolbarLiquidGlassContentDrawn, privacy: .public) frozenSelectionEditable=\(frozenSelectionEditable, privacy: .public) pendingFrameDisplayed=\(pendingFrameDisplayed, privacy: .public)"
		)
	}

	static func frozenSelectionImageTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		ensureMilliseconds: Double,
		refreshMilliseconds: Double,
		compositeMilliseconds: Double,
		source: String,
		success: Bool,
		width: Int,
		height: Int,
		hasOverlayEdits: Bool
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.frozen_selection_image totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) ensureMs=\(ensureMilliseconds, format: .fixed(precision: 2), privacy: .public) refreshMs=\(refreshMilliseconds, format: .fixed(precision: 2), privacy: .public) compositeMs=\(compositeMilliseconds, format: .fixed(precision: 2), privacy: .public) source=\(source, privacy: .public) success=\(success, privacy: .public) width=\(width, privacy: .public) height=\(height, privacy: .public) hasOverlayEdits=\(hasOverlayEdits, privacy: .public)"
		)
	}

	static func copyCaptureTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		captureImageMilliseconds: Double,
		clearPasteboardMilliseconds: Double,
		makeImageMilliseconds: Double,
		writePasteboardMilliseconds: Double,
		success: Bool,
		failureStage: String,
		width: Int,
		height: Int,
		cacheHit: Bool = false
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.copy_capture totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) captureImageMs=\(captureImageMilliseconds, format: .fixed(precision: 2), privacy: .public) clearPasteboardMs=\(clearPasteboardMilliseconds, format: .fixed(precision: 2), privacy: .public) makeImageMs=\(makeImageMilliseconds, format: .fixed(precision: 2), privacy: .public) writePasteboardMs=\(writePasteboardMilliseconds, format: .fixed(precision: 2), privacy: .public) success=\(success, privacy: .public) failureStage=\(failureStage, privacy: .public) width=\(width, privacy: .public) height=\(height, privacy: .public) cacheHit=\(cacheHit, privacy: .public)"
		)
	}

	static func preparedFrozenExportTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		captureImageMilliseconds: Double,
		makeImageMilliseconds: Double,
		success: Bool,
		reason: String,
		width: Int,
		height: Int
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.prepared_frozen_export totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) captureImageMs=\(captureImageMilliseconds, format: .fixed(precision: 2), privacy: .public) makeImageMs=\(makeImageMilliseconds, format: .fixed(precision: 2), privacy: .public) success=\(success, privacy: .public) reason=\(reason, privacy: .public) width=\(width, privacy: .public) height=\(height, privacy: .public)"
		)
	}

	static func saveCaptureTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		captureImageMilliseconds: Double,
		makeImageMilliseconds: Double,
		writeFileMilliseconds: Double,
		success: Bool,
		failureStage: String,
		width: Int,
		height: Int,
		cacheHit: Bool
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.save_capture totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) captureImageMs=\(captureImageMilliseconds, format: .fixed(precision: 2), privacy: .public) makeImageMs=\(makeImageMilliseconds, format: .fixed(precision: 2), privacy: .public) writeFileMs=\(writeFileMilliseconds, format: .fixed(precision: 2), privacy: .public) success=\(success, privacy: .public) failureStage=\(failureStage, privacy: .public) width=\(width, privacy: .public) height=\(height, privacy: .public) cacheHit=\(cacheHit, privacy: .public)"
		)
	}

	static func preparedRecognizeTextImageTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
		captureImageMilliseconds: Double,
		success: Bool,
		reason: String,
		width: Int,
		height: Int
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.prepared_recognize_text_image totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) captureImageMs=\(captureImageMilliseconds, format: .fixed(precision: 2), privacy: .public) success=\(success, privacy: .public) reason=\(reason, privacy: .public) width=\(width, privacy: .public) height=\(height, privacy: .public)"
		)
	}

	static func recognizeTextTiming(
		captureID: UInt64,
		totalMilliseconds: Double,
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
		recognitionLevel: String,
		languageCorrection: Bool,
		automaticLanguageDetection: Bool,
		cacheHit: Bool = false
	) {
		captureTimingLogger.info(
			"schema=\(schema, privacy: .public) runID=\(runID, privacy: .public) captureID=\(captureID, privacy: .public) event=capture_timing.recognize_text totalMs=\(totalMilliseconds, format: .fixed(precision: 2), privacy: .public) captureImageMs=\(captureImageMilliseconds, format: .fixed(precision: 2), privacy: .public) visionRequestMs=\(visionRequestMilliseconds, format: .fixed(precision: 2), privacy: .public) resultProcessingMs=\(resultProcessingMilliseconds, format: .fixed(precision: 2), privacy: .public) clearPasteboardMs=\(clearPasteboardMilliseconds, format: .fixed(precision: 2), privacy: .public) writePasteboardMs=\(writePasteboardMilliseconds, format: .fixed(precision: 2), privacy: .public) success=\(success, privacy: .public) outcome=\(outcome, privacy: .public) failureStage=\(failureStage, privacy: .public) width=\(width, privacy: .public) height=\(height, privacy: .public) observationCount=\(observationCount, privacy: .public) recognizedLines=\(recognizedLines, privacy: .public) recognizedCharacters=\(recognizedCharacters, privacy: .public) recognitionLevel=\(recognitionLevel, privacy: .public) languageCorrection=\(languageCorrection, privacy: .public) automaticLanguageDetection=\(automaticLanguageDetection, privacy: .public) cacheHit=\(cacheHit, privacy: .public)"
		)
	}

	final class DistributionMetric: @unchecked Sendable {
		private let name: String
		private let unit: String
		private let batchSize: Int
		private let logger: Logger
		private let lock = NSLock()
		private var samples: [Double] = []

		fileprivate init(name: String, category: String, unit: String, batchSize: Int) {
			self.name = name
			self.unit = unit
			self.batchSize = max(1, batchSize)
			logger = Logger(subsystem: NativeHostTelemetry.subsystem, category: category)
		}

		func record(_ value: Double) {
			guard value.isFinite, value >= 0, value < 5_000 else {
				return
			}

			let batch: [Double]?
			lock.lock()
			samples.append(value)
			if samples.count >= batchSize {
				batch = samples
				samples.removeAll(keepingCapacity: true)
			} else {
				batch = nil
			}
			lock.unlock()

			if let batch {
				NativeHostTelemetry.distributionEmitQueue.async { [weak self] in
					self?.emit(batch)
				}
			}
		}

		func recordMillisecondsSince(_ startUptime: TimeInterval) {
			record((ProcessInfo.processInfo.systemUptime - startUptime) * 1_000)
		}

		func recordLatencySince(_ inputUptime: TimeInterval?) {
			guard let inputUptime else {
				return
			}
			recordMillisecondsSince(inputUptime)
		}

		private func emit(_ batch: [Double]) {
			let sorted = batch.sorted()
			guard let maxValue = sorted.last else {
				return
			}
			let p50 = percentile(sorted, 0.50)
			let p95 = percentile(sorted, 0.95)
			logger.info(
				"schema=\(NativeHostTelemetry.schema, privacy: .public) runID=\(NativeHostTelemetry.runID, privacy: .public) metric=\(self.name, privacy: .public) unit=\(self.unit, privacy: .public) samples=\(sorted.count, privacy: .public) p50=\(p50, format: .fixed(precision: 2), privacy: .public) p95=\(p95, format: .fixed(precision: 2), privacy: .public) max=\(maxValue, format: .fixed(precision: 2), privacy: .public)"
			)
		}

		private func percentile(_ sorted: [Double], _ percentile: Double) -> Double {
			guard sorted.isEmpty == false else {
				return 0
			}
			let fraction = min(max(percentile, 0), 1)
			let rawIndex = Int((Double(sorted.count - 1) * fraction).rounded(.up))
			let index = min(max(rawIndex, 0), sorted.count - 1)
			return sorted[index]
		}
	}
}
