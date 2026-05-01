import CoreGraphics
import Foundation
import RsnapHostBridge

@main
enum RsnapHostBridgeProbe {
	static func main() throws {
		let session = try RsnapHostSession()
		try session.enterLive()

		let liveRequests = try session.drainRequests()
		guard liveRequests == [.startLiveCapture] else {
			fatalError("unexpected live requests: \(liveRequests)")
		}

		try session.send(
			event: .pointerMoved(
				point: CGPoint(x: 120, y: 180),
				rgb: RGBSample(r: 1, g: 2, b: 3),
				activeMonitor: MonitorSnapshot(
					id: 9,
					frame: CGRect(x: 0, y: 0, width: 1440, height: 900),
					scaleFactorX1000: 2_000
				),
				highlightedWindow: WindowSnapshot(
					windowID: 42,
					frame: CGRect(x: 110, y: 150, width: 300, height: 240)
				)
			)
		)
		try session.send(
			event: .primaryInteractionStarted(
				point: CGPoint(x: 120, y: 180),
				activeMonitor: MonitorSnapshot(
					id: 9,
					frame: CGRect(x: 0, y: 0, width: 1440, height: 900),
					scaleFactorX1000: 2_000
				),
				highlightedWindow: WindowSnapshot(
					windowID: 42,
					frame: CGRect(x: 110, y: 150, width: 300, height: 240)
				)
			)
		)
		try session.send(
			event: .primaryInteractionUpdated(
				point: CGPoint(x: 260, y: 320),
				activeMonitor: MonitorSnapshot(
					id: 9,
					frame: CGRect(x: 0, y: 0, width: 1440, height: 900),
					scaleFactorX1000: 2_000
				),
				highlightedWindow: WindowSnapshot(
					windowID: 42,
					frame: CGRect(x: 110, y: 150, width: 300, height: 240)
				)
			)
		)
		var scene = try session.currentScene()
		guard scene.liveSelectionPreview == CGRect(x: 120, y: 180, width: 140, height: 140) else {
			fatalError("unexpected live preview scene: \(scene)")
		}
		try session.send(
			event: .primaryInteractionCompleted(
				point: CGPoint(x: 260, y: 320),
				activeMonitor: MonitorSnapshot(
					id: 9,
					frame: CGRect(x: 0, y: 0, width: 1440, height: 900),
					scaleFactorX1000: 2_000
				),
				highlightedWindow: WindowSnapshot(
					windowID: 42,
					frame: CGRect(x: 110, y: 150, width: 300, height: 240)
				)
			)
		)
		scene = try session.currentScene()
		guard
			scene.mode == .live,
			scene.liveSelectionPreview == CGRect(x: 120, y: 180, width: 140, height: 140)
		else {
			fatalError("unexpected post-complete live scene: \(scene)")
		}
		guard
			try session.takeNextRequest()
				== .requestFreezeSnapshot(
					selection: CGRect(x: 120, y: 180, width: 140, height: 140),
					selectionEditable: true)
		else {
			fatalError("expected a freeze snapshot request")
		}

		try session.send(
			report: .freezeSnapshotCommitted(
				selection: CGRect(x: 120, y: 180, width: 140, height: 140)))
		scene = try session.currentScene()
		guard
			scene.mode == .frozen,
			scene.cursorIntent == .grab,
			scene.activeMonitor == nil,
			scene.highlightedWindow == nil,
			scene.liveSelectionPreview == nil,
			scene.frozenSelection == CGRect(x: 120, y: 180, width: 140, height: 140),
			scene.statusMessage == nil,
			scene.toolbarItems.contains(where: { $0.kind == .pointer && $0.selected }),
			scene.toolbarItems.contains(where: { $0.kind == .ocr && $0.enabled }),
			scene.toolbarItems.contains(where: { $0.kind == .copy && $0.enabled }),
			scene.toolbarItems.contains(where: { $0.kind == .save && $0.enabled })
		else {
			fatalError("unexpected frozen scene: \(scene)")
		}

		try session.send(
			event: .pointerMoved(
				point: CGPoint(x: 260, y: 250),
				rgb: nil,
				activeMonitor: nil,
				highlightedWindow: nil
			)
		)
		scene = try session.currentScene()
		guard scene.cursorIntent == .resizeEast else {
			fatalError("unexpected frozen resize cursor: \(scene)")
		}

		try session.send(event: .toolbarItemInvoked(.text))
		try session.send(
			event: .pointerMoved(
				point: CGPoint(x: 180, y: 200),
				rgb: nil,
				activeMonitor: nil,
				highlightedWindow: nil
			)
		)
		scene = try session.currentScene()
		guard
			scene.cursorIntent == .text,
			scene.toolbarItems.contains(where: { $0.kind == .text && $0.selected }),
			!scene.toolbarItems.contains(where: { $0.kind == .pointer && $0.selected })
		else {
			fatalError("unexpected text-tool scene: \(scene)")
		}

		try session.send(event: .toolbarItemInvoked(.ocr))
		guard try session.takeNextRequest() == .recognizeText else {
			fatalError("expected a recognize-text host request")
		}
		try session.send(report: .hostEffectCompleted(.recognizeText))
		scene = try session.currentScene()
		guard scene.statusMessage == "Recognized text." else {
			fatalError(
				"unexpected status message after OCR: \(String(describing: scene.statusMessage))")
		}

		try session.send(event: .toolbarItemInvoked(.copy))
		guard try session.takeNextRequest() == .copyCapture else {
			fatalError("expected a copy host request")
		}

		try session.send(report: .hostEffectCompleted(.copyCapture))
		scene = try session.currentScene()
		guard scene.statusMessage == "Copied capture." else {
			fatalError(
				"unexpected status message after copy: \(String(describing: scene.statusMessage))")
		}

		try session.send(report: .statusMessage("Host-only status"))
		scene = try session.currentScene()
		guard scene.statusMessage == "Host-only status" else {
			fatalError("unexpected host status message: \(String(describing: scene.statusMessage))")
		}

		try session.enterLive()
		_ = try session.takeNextRequest()
		let clickSelection = CGRect(x: 300, y: 220, width: 360, height: 260)
		let clickWindow = WindowSnapshot(windowID: 88, frame: clickSelection)
		try session.send(
			event: .primaryInteractionStarted(
				point: CGPoint(x: 420, y: 340),
				activeMonitor: MonitorSnapshot(
					id: 9,
					frame: CGRect(x: 0, y: 0, width: 1440, height: 900),
					scaleFactorX1000: 2_000
				),
				highlightedWindow: clickWindow
			)
		)
		try session.send(
			event: .primaryInteractionCompleted(
				point: CGPoint(x: 420, y: 340),
				activeMonitor: MonitorSnapshot(
					id: 9,
					frame: CGRect(x: 0, y: 0, width: 1440, height: 900),
					scaleFactorX1000: 2_000
				),
				highlightedWindow: clickWindow
			)
		)
		guard
			try session.takeNextRequest()
				== .requestFreezeSnapshot(
					selection: clickSelection,
					selectionEditable: false)
		else {
			fatalError("expected a fixed click-window freeze request")
		}
		try session.send(report: .freezeSnapshotCommitted(selection: clickSelection))
		try session.send(
			event: .pointerMoved(
				point: CGPoint(x: 420, y: 340),
				rgb: nil,
				activeMonitor: nil,
				highlightedWindow: nil
			)
		)
		scene = try session.currentScene()
		guard scene.mode == .frozen, scene.cursorIntent == .default else {
			fatalError("unexpected click-window frozen cursor: \(scene)")
		}

		try session.enterLive()
		_ = try session.takeNextRequest()
		let fullscreenMonitor = MonitorSnapshot(
			id: 10,
			frame: CGRect(x: 0, y: 0, width: 1440, height: 900),
			scaleFactorX1000: 2_000
		)
		try session.send(
			event: .primaryInteractionStarted(
				point: CGPoint(x: 700, y: 500),
				activeMonitor: fullscreenMonitor,
				highlightedWindow: nil
			)
		)
		try session.send(
			event: .primaryInteractionCompleted(
				point: CGPoint(x: 700, y: 500),
				activeMonitor: fullscreenMonitor,
				highlightedWindow: nil
			)
		)
		let fullscreenSelection = fullscreenMonitor.frame
		guard
			try session.takeNextRequest()
				== .requestFreezeSnapshot(
					selection: fullscreenSelection,
					selectionEditable: false)
		else {
			fatalError("expected a fixed fullscreen fallback freeze request")
		}
		try session.send(report: .freezeSnapshotCommitted(selection: fullscreenSelection))
		try session.send(
			event: .pointerMoved(
				point: CGPoint(x: 700, y: 500),
				rgb: nil,
				activeMonitor: nil,
				highlightedWindow: nil
			)
		)
		scene = try session.currentScene()
		guard scene.mode == .frozen, scene.cursorIntent == .default else {
			fatalError("unexpected fullscreen frozen cursor: \(scene)")
		}

		try session.enterLive()
		_ = try session.takeNextRequest()
		try session.send(
			event: .pointerMoved(
				point: CGPoint(x: 200, y: 260),
				rgb: nil,
				activeMonitor: MonitorSnapshot(
					id: 11,
					frame: CGRect(x: 1440, y: 0, width: 1728, height: 1117),
					scaleFactorX1000: 2_000
				),
				highlightedWindow: WindowSnapshot(
					windowID: 77,
					frame: CGRect(x: 1500, y: 100, width: 500, height: 400)
				)
			)
		)
		scene = try session.currentScene()
		guard
			scene.mode == .live,
			scene.activeMonitor?.id == 11,
			scene.activeMonitor?.frame == CGRect(x: 1440, y: 0, width: 1728, height: 1117),
			scene.highlightedWindow?.windowID == 77,
			scene.highlightedWindow?.frame == CGRect(x: 1500, y: 100, width: 500, height: 400)
		else {
			fatalError("unexpected live monitor/window scene: \(scene)")
		}
		try session.send(
			event: .pointerMoved(
				point: CGPoint(x: 2600, y: 800),
				rgb: nil,
				activeMonitor: MonitorSnapshot(
					id: 11,
					frame: CGRect(x: 1440, y: 0, width: 1728, height: 1117),
					scaleFactorX1000: 2_000
				),
				highlightedWindow: nil
			)
		)
		scene = try session.currentScene()
		guard scene.highlightedWindow == nil else {
			fatalError("stale live highlighted window was not cleared: \(scene)")
		}

		print("rsnap-host-bridge probe ok")
	}
}
