import AppKit
import CoreGraphics
import CoreMedia
import CoreVideo
import Darwin
import Foundation
import ScreenCaptureKit

struct FrozenFrameDisplayTarget: Equatable, Sendable {
	let displayID: CGDirectDisplayID
	let frame: CGRect
	let widthPixels: Int
	let heightPixels: Int
	let framesPerSecond: Int
}

struct FrozenFramePreparedContentFilter: @unchecked Sendable {
	let filter: SCContentFilter
	let selfCaptureFilterComplete: Bool
	let expectedWindowCount: Int
	let matchedWindowCount: Int
}

final class FrozenFrameShareableContentCache: @unchecked Sendable {
	private let lock = NSLock()
	private var content: SCShareableContent?
	private var cachedAtUptime: TimeInterval = 0

	func store(_ content: SCShareableContent) {
		lock.lock()
		self.content = content
		cachedAtUptime = ProcessInfo.processInfo.systemUptime
		lock.unlock()
	}

	func fresh(
		maxAge: TimeInterval,
		covering displayIDs: Set<CGDirectDisplayID>? = nil
	) -> SCShareableContent? {
		let now = ProcessInfo.processInfo.systemUptime
		lock.lock()
		let content = now - cachedAtUptime <= maxAge ? self.content : nil
		lock.unlock()
		guard let content else {
			return nil
		}
		guard content.displays.isEmpty == false else {
			return nil
		}
		guard let displayIDs else {
			return content
		}
		let availableDisplayIDs = Set(content.displays.map(\.displayID))
		guard displayIDs.isSubset(of: availableDisplayIDs) else {
			return nil
		}
		return content
	}
}

enum FrozenFrameContentFilterPlanner {
	static func displayTarget(for screen: NSScreen) -> FrozenFrameDisplayTarget? {
		guard let displayID = screen.nativeDisplayID else {
			return nil
		}
		let scale = max(screen.backingScaleFactor, 1)
		return FrozenFrameDisplayTarget(
			displayID: displayID,
			frame: screen.frame,
			widthPixels: max(1, Int((screen.frame.width * scale).rounded())),
			heightPixels: max(1, Int((screen.frame.height * scale).rounded())),
			framesPerSecond: NativeHostDisplayRefresh.targetFramesPerSecond(for: screen)
		)
	}

	static func streamConfiguration(for target: FrozenFrameDisplayTarget) -> SCStreamConfiguration {
		let configuration = SCStreamConfiguration()
		configuration.width = target.widthPixels
		configuration.height = target.heightPixels
		configuration.pixelFormat = kCVPixelFormatType_32BGRA
		configuration.minimumFrameInterval = CMTime(
			value: 1, timescale: CMTimeScale(target.framesPerSecond))
		configuration.queueDepth = 3
		configuration.showsCursor = false
		configuration.scalesToFit = false
		if #available(macOS 14.0, *) {
			configuration.preservesAspectRatio = true
		}
		return configuration
	}

	static func filtersAreComplete(
		_ preparedFilters: [CGDirectDisplayID: FrozenFramePreparedContentFilter],
		for targets: [FrozenFrameDisplayTarget]
	) -> Bool {
		targets.allSatisfy { target in
			preparedFilters[target.displayID]?.selfCaptureFilterComplete == true
		}
	}

	static func contentFilters(
		for targets: [FrozenFrameDisplayTarget],
		in content: SCShareableContent,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>
	) -> [CGDirectDisplayID: FrozenFramePreparedContentFilter] {
		Dictionary(
			uniqueKeysWithValues: targets.compactMap { target in
				guard
					let filter = contentFilter(
						for: target,
						in: content,
						selfCaptureExceptionWindowIDs: selfCaptureExceptionWindowIDs,
						includedCurrentProcessWindowIDs: includedCurrentProcessWindowIDs
					)
				else {
					return nil
				}
				return (target.displayID, filter)
			}
		)
	}

	static func shareableContentHasDisplays(_ content: SCShareableContent) -> Bool {
		!content.displays.isEmpty
	}

	static func shareableContent(
		_ content: SCShareableContent,
		covers displayIDs: Set<CGDirectDisplayID>
	) -> Bool {
		guard shareableContentHasDisplays(content) else {
			return false
		}
		let availableDisplayIDs = Set(content.displays.map(\.displayID))
		return displayIDs.isSubset(of: availableDisplayIDs)
	}

	static func shareableContentDisplayDetail(
		_ content: SCShareableContent,
		requiredDisplayIDs: Set<CGDirectDisplayID>
	) -> String {
		let required = requiredDisplayIDs.sorted().map { String($0) }.joined(separator: ",")
		let available = content.displays.map(\.displayID).sorted().map { String($0) }.joined(
			separator: ",")
		return "requiredDisplayIDs=\(required) availableDisplayIDs=\(available)"
	}

	private static func contentFilter(
		for target: FrozenFrameDisplayTarget,
		in content: SCShareableContent,
		selfCaptureExceptionWindowIDs: Set<CGWindowID>,
		includedCurrentProcessWindowIDs: Set<CGWindowID>
	) -> FrozenFramePreparedContentFilter? {
		guard let display = content.displays.first(where: { $0.displayID == target.displayID })
		else {
			return nil
		}
		let currentPID = getpid()
		let excludedApplications = content.applications.filter { $0.processID == currentPID }
		if excludedApplications.isEmpty == false {
			let includedWindows = content.windows.filter {
				includedCurrentProcessWindowIDs.contains($0.windowID)
			}
			let matchedIncludedWindowIDs = Set(includedWindows.map(\.windowID))
			let missingIncludedWindowIDs =
				includedCurrentProcessWindowIDs.subtracting(matchedIncludedWindowIDs)
			return FrozenFramePreparedContentFilter(
				filter: SCContentFilter(
					display: display,
					excludingApplications: excludedApplications,
					exceptingWindows: includedWindows
				),
				selfCaptureFilterComplete: missingIncludedWindowIDs.isEmpty,
				expectedWindowCount: selfCaptureExceptionWindowIDs.count
					+ includedCurrentProcessWindowIDs.count,
				matchedWindowCount: selfCaptureExceptionWindowIDs.count
					+ matchedIncludedWindowIDs.count
			)
		}
		let excludedWindows = content.windows.filter {
			$0.owningApplication?.processID == currentPID
				&& !includedCurrentProcessWindowIDs.contains($0.windowID)
		}
		let matchedExcludedWindowIDs = Set(excludedWindows.map(\.windowID))
		let matchedIncludedWindowIDs = Set(
			content.windows.filter {
				$0.owningApplication?.processID == currentPID
					&& includedCurrentProcessWindowIDs.contains($0.windowID)
			}.map(\.windowID))
		let missingExcludedWindowIDs =
			selfCaptureExceptionWindowIDs.subtracting(matchedExcludedWindowIDs)
		let missingIncludedWindowIDs =
			includedCurrentProcessWindowIDs.subtracting(matchedIncludedWindowIDs)
		let hasCompleteWindowExclusion =
			missingExcludedWindowIDs.isEmpty && missingIncludedWindowIDs.isEmpty
		return FrozenFramePreparedContentFilter(
			filter: SCContentFilter(display: display, excludingWindows: excludedWindows),
			selfCaptureFilterComplete: hasCompleteWindowExclusion,
			expectedWindowCount: selfCaptureExceptionWindowIDs.count
				+ includedCurrentProcessWindowIDs.count,
			matchedWindowCount: selfCaptureExceptionWindowIDs.count
				- missingExcludedWindowIDs.count + matchedIncludedWindowIDs.count
		)
	}
}
