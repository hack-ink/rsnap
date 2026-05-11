import AppKit
import Foundation
import Sparkle

@MainActor
final class NativeHostSoftwareUpdater {
	enum Mode: String, CaseIterable {
		case off
		case check
		case install

		var title: String {
			switch self {
			case .off:
				return "Off"
			case .check:
				return "Notify"
			case .install:
				return "Install"
			}
		}
	}

	struct Snapshot: Equatable {
		let isConfigured: Bool
		let canCheckForUpdates: Bool
		let allowsAutomaticUpdates: Bool
		let mode: Mode
		let currentVersion: String
		let lastCheckSummary: String

		var modeSubtitle: String {
			if isConfigured == false {
				return "Sparkle appcast not configured."
			}
			switch mode {
			case .off:
				return "Automatic update checks are off."
			case .check, .install:
				return lastCheckSummary
			}
		}

		var releaseVersionTitle: String {
			if isConfigured {
				return "Release Version"
			}
			return "GitHub Release"
		}

		var releaseVersionSubtitle: String {
			if isConfigured {
				return "Current \(currentVersion); Sparkle appcast."
			}
			return "Current \(currentVersion); opens latest release."
		}
	}

	static let releasePageURL = httpsURL(
		host: "github.com",
		path: "/hack-ink/rsnap/releases/latest")

	private let updaterController: SPUStandardUpdaterController?

	init() {
		if Self.hasSparkleConfiguration {
			let controller = SPUStandardUpdaterController(
				startingUpdater: true,
				updaterDelegate: nil,
				userDriverDelegate: nil)
			updaterController = controller
			NativeHostTelemetry.lifecycleEvent("native_host.sparkle_updater_started")
			requestImmediateLaunchUpdateCheckIfEnabled(using: controller.updater)
		} else {
			updaterController = nil
			NativeHostTelemetry.lifecycleWarning(
				"native_host.sparkle_updater_unconfigured",
				detail: "reason=missing_feed_or_public_key")
		}
	}

	func snapshot() -> Snapshot {
		guard let updater = updaterController?.updater else {
			return Snapshot(
				isConfigured: false,
				canCheckForUpdates: true,
				allowsAutomaticUpdates: false,
				mode: .off,
				currentVersion: Self.currentAppVersionLabel,
				lastCheckSummary: "Never checked."
			)
		}
		return Snapshot(
			isConfigured: true,
			canCheckForUpdates: updater.canCheckForUpdates,
			allowsAutomaticUpdates: updater.allowsAutomaticUpdates,
			mode: Self.mode(
				automaticallyChecksForUpdates: updater.automaticallyChecksForUpdates,
				automaticallyDownloadsUpdates: updater.automaticallyDownloadsUpdates),
			currentVersion: Self.currentAppVersionLabel,
			lastCheckSummary: Self.lastCheckSummary(for: updater.lastUpdateCheckDate)
		)
	}

	func setMode(_ mode: Mode) {
		guard let updater = updaterController?.updater else {
			return
		}
		switch mode {
		case .off:
			updater.automaticallyDownloadsUpdates = false
			updater.automaticallyChecksForUpdates = false
		case .check:
			updater.automaticallyChecksForUpdates = true
			updater.automaticallyDownloadsUpdates = false
		case .install:
			updater.automaticallyChecksForUpdates = true
			if updater.allowsAutomaticUpdates {
				updater.automaticallyDownloadsUpdates = true
			}
		}
		NativeHostTelemetry.lifecycleEvent(
			"native_host.sparkle_update_mode_changed",
			detail: "mode=\(mode.rawValue)")
	}

	func checkForUpdates(_ sender: Any?) {
		guard let updaterController else {
			NSWorkspace.shared.open(Self.releasePageURL)
			return
		}
		NSApp.setActivationPolicy(.regular)
		NSRunningApplication.current.activate(options: [.activateAllWindows])
		updaterController.checkForUpdates(sender)
	}

	private func requestImmediateLaunchUpdateCheckIfEnabled(using updater: SPUUpdater) {
		guard updater.automaticallyChecksForUpdates else {
			NativeHostTelemetry.lifecycleDebug(
				"native_host.sparkle_update_check_skipped",
				detail: "source=launch,reason=disabled")
			return
		}
		guard updater.sessionInProgress == false else {
			NativeHostTelemetry.lifecycleDebug(
				"native_host.sparkle_update_check_skipped",
				detail: "source=launch,reason=session_in_progress")
			return
		}
		updater.checkForUpdatesInBackground()
		NativeHostTelemetry.lifecycleEvent(
			"native_host.sparkle_update_check_scheduled",
			detail: "source=launch")
	}

	private static var hasSparkleConfiguration: Bool {
		nonEmptyInfoValue(forKey: "SUFeedURL") != nil
			&& nonEmptyInfoValue(forKey: "SUPublicEDKey") != nil
	}

	private static func mode(
		automaticallyChecksForUpdates: Bool,
		automaticallyDownloadsUpdates: Bool
	) -> Mode {
		if automaticallyDownloadsUpdates {
			return .install
		}
		if automaticallyChecksForUpdates {
			return .check
		}
		return .off
	}

	private static var currentAppVersionLabel: String {
		nonEmptyInfoValue(forKey: "CFBundleShortVersionString") ?? "Development Build"
	}

	private static func lastCheckSummary(for checkedAt: Date?) -> String {
		guard let checkedAt else {
			return "Never checked."
		}
		return "Checked \(checkedAt.formatted(date: .omitted, time: .shortened))."
	}

	private static func nonEmptyInfoValue(forKey key: String) -> String? {
		guard let value = Bundle.main.object(forInfoDictionaryKey: key) as? String else {
			return nil
		}
		let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
		return trimmed.isEmpty ? nil : trimmed
	}

	private static func httpsURL(host: String, path: String) -> URL {
		var components = URLComponents()
		components.scheme = "https"
		components.host = host
		components.path = path
		guard let url = components.url else {
			preconditionFailure("Invalid static Rsnap update URL: \(host)\(path)")
		}
		return url
	}
}
