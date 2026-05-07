import Foundation
import ServiceManagement

package struct LaunchAtLoginState: Equatable {
	package let isOn: Bool
	package let isControlEnabled: Bool
	package let subtitle: String
	package let helpText: String

	@MainActor
	static func current(errorMessage: String? = nil) -> Self {
		LaunchAtLoginController.currentState(errorMessage: errorMessage)
	}
}

package enum LaunchAtLoginStatusSnapshot: Equatable {
	case notRegistered
	case enabled
	case requiresApproval
	case notFound
	case unknown
}

package enum LaunchAtLoginController {
	@MainActor
	package static var currentStatus: LaunchAtLoginStatusSnapshot {
		snapshot(for: SMAppService.mainApp.status)
	}

	@MainActor
	static func setEnabled(_ isEnabled: Bool) throws {
		let service = SMAppService.mainApp
		let status = snapshot(for: service.status)

		if isEnabled {
			switch status {
			case .enabled, .requiresApproval:
				return
			case .notRegistered, .notFound, .unknown:
				try service.register()
			}
			return
		}

		switch status {
		case .enabled, .requiresApproval, .unknown:
			try service.unregister()
		case .notRegistered, .notFound:
			return
		}
	}

	@MainActor
	package static func currentState(errorMessage: String? = nil) -> LaunchAtLoginState {
		state(for: currentStatus, errorMessage: errorMessage)
	}

	package static func state(
		for status: LaunchAtLoginStatusSnapshot,
		errorMessage: String? = nil
	) -> LaunchAtLoginState {
		let base = baseState(for: status)
		guard let errorMessage, !errorMessage.isEmpty else {
			return base
		}
		return LaunchAtLoginState(
			isOn: base.isOn,
			isControlEnabled: base.isControlEnabled,
			subtitle: "Update failed.",
			helpText: errorMessage
		)
	}

	private static func baseState(for status: LaunchAtLoginStatusSnapshot) -> LaunchAtLoginState {
		switch status {
		case .enabled:
			return LaunchAtLoginState(
				isOn: true,
				isControlEnabled: true,
				subtitle: "Starts at sign-in.",
				helpText: "Rsnap is registered in macOS Login Items."
			)
		case .requiresApproval:
			return LaunchAtLoginState(
				isOn: true,
				isControlEnabled: true,
				subtitle: "Needs approval.",
				helpText: "Approve Rsnap in System Settings > General > Login Items."
			)
		case .notRegistered:
			return LaunchAtLoginState(
				isOn: false,
				isControlEnabled: true,
				subtitle: "Manual startup.",
				helpText: "Register Rsnap with macOS Login Items."
			)
		case .notFound:
			return LaunchAtLoginState(
				isOn: false,
				isControlEnabled: true,
				subtitle: "Try enabling.",
				helpText: "Register Rsnap with macOS Login Items."
			)
		case .unknown:
			return LaunchAtLoginState(
				isOn: false,
				isControlEnabled: true,
				subtitle: "Status unknown.",
				helpText: "macOS returned an unknown Login Items status."
			)
		}
	}

	private static func snapshot(for status: SMAppService.Status) -> LaunchAtLoginStatusSnapshot {
		switch status {
		case .notRegistered:
			return .notRegistered
		case .enabled:
			return .enabled
		case .requiresApproval:
			return .requiresApproval
		case .notFound:
			return .notFound
		@unknown default:
			return .unknown
		}
	}
}
