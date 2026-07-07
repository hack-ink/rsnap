import CoreGraphics
import Foundation
import ScreenCaptureKit

enum ShareableContentLookup {
	enum Outcome {
		case prepared([CGDirectDisplayID: PreparedContentFilter])
		case unavailable
	}

	private static let cacheMaxAge: TimeInterval = 3_600
	private static let cache = ShareableContentCache()

	static func refreshCache(captureID: UInt64 = 0, source: String = "cache") {
		let startedAtUptime = ProcessInfo.processInfo.systemUptime
		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) {
			content, error in
			guard let content else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.content_cache_refresh_failed",
					captureID: captureID,
					source: source,
					displayID: 0,
					error: String(describing: error)
				)
				return
			}
			guard ContentFilterPlanner.shareableContentHasDisplays(content) else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.content_cache_refresh_invalid",
					captureID: captureID,
					source: source,
					displayID: 0,
					error: ContentFilterPlanner.shareableContentDisplayDetail(
						content,
						requiredDisplayIDs: []
					)
				)
				recordLookupTiming(
					content,
					captureID: captureID,
					source: source,
					startedAtUptime: startedAtUptime,
					success: false
				)
				return
			}
			cache.store(content)
			recordLookupTiming(
				content,
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				success: true
			)
		}
	}

	static func hasFreshCache() -> Bool {
		cachedContent() != nil
	}

	static func resolveFilters(
		targets: [FrozenDisplayTarget],
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		shouldContinue: @escaping @Sendable () -> Bool,
		completion: @escaping @Sendable (Outcome) -> Void
	) {
		let targetIDs = Set(targets.map(\.displayID))
		if let content = cachedContent(covering: targetIDs) {
			recordLookupTiming(
				content,
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				success: true
			)
			completion(
				.prepared(
					preparedFilters(
						targets: targets,
						content: content,
						selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
						includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
					)
				)
			)
			return
		}

		lookupContent { content, error in
			guard shouldContinue() else {
				return
			}
			guard let content else {
				recordLookupFailure(
					error,
					targets: targets,
					captureID: captureID,
					source: source,
					startedAtUptime: startedAtUptime
				)
				completion(.unavailable)
				return
			}
			let contentCoversTargets = contentCovers(content, targetIDs: targetIDs)
			recordLookupTiming(
				content,
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				success: contentCoversTargets
			)
			guard contentCoversTargets else {
				recordInvalidContent(
					content, targetIDs: targetIDs, captureID: captureID, source: source)
				if ProcessInfo.processInfo.systemUptime < retryUntilUptime {
					retryResolveFilters(
						targets: targets,
						selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
						includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
						captureID: captureID,
						source: source,
						startedAtUptime: startedAtUptime,
						retryUntilUptime: retryUntilUptime,
						shouldContinue: shouldContinue,
						completion: completion
					)
					return
				}
				completion(.unavailable)
				return
			}
			cache.store(content)
			completion(
				.prepared(
					preparedFilters(
						targets: targets,
						content: content,
						selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
						includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
					)
				)
			)
		}
	}

	static func resolveCompleteFilters(
		targets: [FrozenDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		shouldContinue: @escaping @Sendable () -> Bool,
		completion: @escaping @Sendable (Outcome) -> Void
	) {
		if let content = cachedContent(covering: targetIDs) {
			let preparedFilters = preparedFilters(
				targets: targets,
				content: content,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
			)
			guard filtersAreComplete(preparedFilters, targets: targets) else {
				return lookupCompleteFilters(
					targets: targets,
					targetIDs: targetIDs,
					selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
					includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
					captureID: captureID,
					source: source,
					startedAtUptime: startedAtUptime,
					retryUntilUptime: retryUntilUptime,
					shouldContinue: shouldContinue,
					completion: completion
				)
			}
			recordLookupTiming(
				content,
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				success: true
			)
			completion(.prepared(preparedFilters))
			return
		}

		lookupCompleteFilters(
			targets: targets,
			targetIDs: targetIDs,
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
			captureID: captureID,
			source: source,
			startedAtUptime: startedAtUptime,
			retryUntilUptime: retryUntilUptime,
			shouldContinue: shouldContinue,
			completion: completion
		)
	}

	private static func lookupCompleteFilters(
		targets: [FrozenDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		shouldContinue: @escaping @Sendable () -> Bool,
		completion: @escaping @Sendable (Outcome) -> Void
	) {
		lookupContent { content, error in
			guard let content else {
				recordLookupFailure(
					error,
					targets: targets,
					captureID: captureID,
					source: source,
					startedAtUptime: startedAtUptime
				)
				if shouldContinue() {
					completion(.unavailable)
				}
				return
			}

			let contentCoversTargets = contentCovers(content, targetIDs: targetIDs)
			recordLookupTiming(
				content,
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				success: contentCoversTargets
			)
			guard shouldContinue() else {
				return
			}
			guard contentCoversTargets else {
				recordInvalidContent(
					content, targetIDs: targetIDs, captureID: captureID, source: source)
				if ProcessInfo.processInfo.systemUptime < retryUntilUptime {
					retryResolveCompleteFilters(
						targets: targets,
						targetIDs: targetIDs,
						selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
						includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
						captureID: captureID,
						source: source,
						startedAtUptime: startedAtUptime,
						retryUntilUptime: retryUntilUptime,
						shouldContinue: shouldContinue,
						completion: completion
					)
					return
				}
				completion(.unavailable)
				return
			}
			cache.store(content)
			let preparedFilters = preparedFilters(
				targets: targets,
				content: content,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
			)
			guard filtersAreComplete(preparedFilters, targets: targets) else {
				if ProcessInfo.processInfo.systemUptime < retryUntilUptime {
					retryResolveCompleteFilters(
						targets: targets,
						targetIDs: targetIDs,
						selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
						includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
						captureID: captureID,
						source: source,
						startedAtUptime: startedAtUptime,
						retryUntilUptime: retryUntilUptime,
						shouldContinue: shouldContinue,
						completion: completion
					)
					return
				}
				recordIncompleteFilters(
					preparedFilters, targets: targets, captureID: captureID, source: source)
				completion(.unavailable)
				return
			}
			completion(.prepared(preparedFilters))
		}
	}

	private static func retryResolveFilters(
		targets: [FrozenDisplayTarget],
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		shouldContinue: @escaping @Sendable () -> Bool,
		completion: @escaping @Sendable (Outcome) -> Void
	) {
		DispatchQueue.global(qos: .userInteractive).asyncAfter(
			deadline: .now() + FrozenFrameAuthority.selfCaptureFilterRetryInterval
		) {
			resolveFilters(
				targets: targets,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				retryUntilUptime: retryUntilUptime,
				shouldContinue: shouldContinue,
				completion: completion
			)
		}
	}

	private static func retryResolveCompleteFilters(
		targets: [FrozenDisplayTarget],
		targetIDs: Set<CGDirectDisplayID>,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		retryUntilUptime: TimeInterval,
		shouldContinue: @escaping @Sendable () -> Bool,
		completion: @escaping @Sendable (Outcome) -> Void
	) {
		DispatchQueue.global(qos: .userInteractive).asyncAfter(
			deadline: .now() + FrozenFrameAuthority.selfCaptureFilterRetryInterval
		) {
			resolveCompleteFilters(
				targets: targets,
				targetIDs: targetIDs,
				selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
				includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs,
				captureID: captureID,
				source: source,
				startedAtUptime: startedAtUptime,
				retryUntilUptime: retryUntilUptime,
				shouldContinue: shouldContinue,
				completion: completion
			)
		}
	}

	private static func lookupContent(
		completion: @escaping @Sendable (SCShareableContent?, Error?) -> Void
	) {
		SCShareableContent.getExcludingDesktopWindows(false, onScreenWindowsOnly: false) {
			content, error in
			completion(content, error)
		}
	}

	private static func cachedContent(
		covering displayIDs: Set<CGDirectDisplayID>? = nil
	) -> SCShareableContent? {
		cache.fresh(maxAge: cacheMaxAge, covering: displayIDs)
	}

	private static func preparedFilters(
		targets: [FrozenDisplayTarget],
		content: SCShareableContent,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>
	) -> [CGDirectDisplayID: PreparedContentFilter] {
		ContentFilterPlanner.contentFilters(
			for: targets,
			in: content,
			selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
			includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
		)
	}

	private static func filtersAreComplete(
		_ preparedFilters: [CGDirectDisplayID: PreparedContentFilter],
		targets: [FrozenDisplayTarget]
	) -> Bool {
		ContentFilterPlanner.filtersAreComplete(preparedFilters, for: targets)
	}

	private static func contentCovers(
		_ content: SCShareableContent,
		targetIDs: Set<CGDirectDisplayID>
	) -> Bool {
		ContentFilterPlanner.shareableContent(content, covers: targetIDs)
	}

	private static func recordLookupFailure(
		_ error: Error?,
		targets: [FrozenDisplayTarget],
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval
	) {
		NativeHostTelemetry.frozenAuthorityWarning(
			"frozen_authority.content_lookup_failed",
			captureID: captureID,
			source: source,
			displayID: 0,
			error: String(describing: error)
		)
		NativeHostTelemetry.frozenAuthorityContentLookupTiming(
			captureID: captureID,
			source: source,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
			success: false,
			displayCount: targets.count,
			windowCount: 0
		)
	}

	private static func recordInvalidContent(
		_ content: SCShareableContent,
		targetIDs: Set<CGDirectDisplayID>,
		captureID: UInt64,
		source: String
	) {
		NativeHostTelemetry.frozenAuthorityWarning(
			"frozen_authority.content_lookup_invalid",
			captureID: captureID,
			source: source,
			displayID: 0,
			error: ContentFilterPlanner.shareableContentDisplayDetail(
				content, requiredDisplayIDs: targetIDs)
		)
	}

	private static func recordLookupTiming(
		_ content: SCShareableContent,
		captureID: UInt64,
		source: String,
		startedAtUptime: TimeInterval,
		success: Bool
	) {
		NativeHostTelemetry.frozenAuthorityContentLookupTiming(
			captureID: captureID,
			source: source,
			totalMilliseconds: NativeHostTelemetry.milliseconds(since: startedAtUptime),
			success: success,
			displayCount: content.displays.count,
			windowCount: content.windows.count
		)
	}

	private static func recordIncompleteFilters(
		_ preparedFilters: [CGDirectDisplayID: PreparedContentFilter],
		targets: [FrozenDisplayTarget],
		captureID: UInt64,
		source: String
	) {
		for target in targets {
			guard let preparedFilter = preparedFilters[target.displayID] else {
				NativeHostTelemetry.frozenAuthorityWarning(
					"frozen_authority.self_capture_filter_incomplete",
					captureID: captureID,
					source: source,
					displayID: target.displayID,
					error: "missingFilter"
				)
				continue
			}
			guard preparedFilter.selfCaptureFilterComplete == false else {
				continue
			}
			NativeHostTelemetry.frozenAuthorityWarning(
				"frozen_authority.self_capture_filter_incomplete",
				captureID: captureID,
				source: source,
				displayID: target.displayID,
				error:
					"expectedWindowCount=\(preparedFilter.expectedWindowCount) matchedWindowCount=\(preparedFilter.matchedWindowCount)"
			)
		}
	}
}
