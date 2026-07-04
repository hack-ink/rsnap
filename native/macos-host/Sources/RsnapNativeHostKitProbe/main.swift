import CoreGraphics
import Foundation
import RsnapHostBridge
import RsnapNativeHostKit

@main
enum RsnapNativeHostKitProbe {
	static func main() {
		assertScrimRoundedExclusionKeepsCornersMasked()
		assertScrimOverlappingRoundedExclusionStaysClear()
		assertScrimExclusionPreservesExistingPixels()
		assertRoundedExclusionMaskKeepsCornersFilled()
		assertCaptureFrameEffectExpandsExportCanvas()
		assertCaptureOverlayLocalPointKeepsScreenEdgesVisible()
		assertScrollCaptureViewportPointAcceptsFlippedGlobalMouseCoordinates()
		assertScrollCaptureObservedInputAcceptsSourceWindowGutter()
		assertFrozenToolbarPlannerDisablesEditingDuringScroll()
		assertFrozenToolbarPlannerPlacesStyleRowAndHitsControls()
		assertSoftwareUpdateModeResolution()
		assertManualUpdateCheckRemainsAvailable()
		assertImmediateInstallGateWaitsForCaptureIdle()
		assertCaptureHostCursorMapping()
		assertCaptureHostPointerDispatchSupport()
		assertCaptureHostLivePrimaryInteractionState()
		assertCaptureHostFrozenFirstDisplayHandoffState()
		assertCaptureHostAnnotationStyleWheelGate()
		assertCaptureHostToolbarHoverState()
		assertCaptureHostLiveSampleCachePointMatching()
		let minimapExportSize = CGSize(width: 100, height: 200)
		guard
			let rightMinimap = scrollCaptureMinimapPlan(
				for: CGRect(x: 100, y: 100, width: 100, height: 100),
				exportSize: minimapExportSize,
				in: CGRect(x: 0, y: 0, width: 500, height: 500),
				preferredWidth: 96,
				minimumWidth: 44,
				gap: 10,
				margin: 10,
				imageInset: 3,
				viewportTopPixels: 20,
				viewportHeightPixels: 100
			)
		else {
			fatalError("expected right-side scroll minimap plan")
		}
		assertRectEqual(
			rightMinimap.frame,
			CGRect(x: 210, y: 54, width: 96, height: 192),
			"scroll minimap should prefer the right side when space is available"
		)
		assertRectEqual(
			rightMinimap.imageFrame,
			CGRect(x: 213, y: 57, width: 90, height: 186),
			"scroll minimap image frame should be planned by Rust"
		)
		assertRectEqual(
			rightMinimap.viewportFrame ?? .null,
			CGRect(x: 213, y: 131.4, width: 90, height: 93),
			"scroll minimap viewport frame should be planned by Rust"
		)
		guard
			let leftMinimap = scrollCaptureMinimapPlan(
				for: CGRect(x: 130, y: 100, width: 100, height: 100),
				exportSize: minimapExportSize,
				in: CGRect(x: 0, y: 0, width: 250, height: 500),
				preferredWidth: 96,
				minimumWidth: 44,
				gap: 10,
				margin: 10,
				imageInset: 3,
				viewportTopPixels: 20,
				viewportHeightPixels: 100
			)
		else {
			fatalError("expected left-side scroll minimap plan")
		}
		assertRectEqual(
			leftMinimap.frame,
			CGRect(x: 24, y: 54, width: 96, height: 192),
			"scroll minimap should fall back to the left when the right side is constrained"
		)
		assertLaunchAtLoginStateMapping()
	}

	private static func assertSoftwareUpdateModeResolution() {
		guard
			SoftwareUpdateModeResolution.modeRawValue(
				automaticallyChecksForUpdates: false,
				automaticallyDownloadsUpdates: false) == "off",
			SoftwareUpdateModeResolution.modeRawValue(
				automaticallyChecksForUpdates: false,
				automaticallyDownloadsUpdates: true) == "off",
			SoftwareUpdateModeResolution.modeRawValue(
				automaticallyChecksForUpdates: true,
				automaticallyDownloadsUpdates: false) == "check",
			SoftwareUpdateModeResolution.modeRawValue(
				automaticallyChecksForUpdates: true,
				automaticallyDownloadsUpdates: true) == "install"
		else {
			fatalError("software update mode should treat disabled checks as off")
		}
	}

	private static func assertManualUpdateCheckRemainsAvailable() {
		guard
			SoftwareUpdateManualCheckAvailability.isEnabled(sparkleCanCheckForUpdates: true),
			SoftwareUpdateManualCheckAvailability.isEnabled(sparkleCanCheckForUpdates: false)
		else {
			fatalError("manual update check should stay available across Sparkle session states")
		}
	}

	private static func assertImmediateInstallGateWaitsForCaptureIdle() {
		guard
			SoftwareUpdateImmediateInstallGate.canInstall(
				captureActive: false,
				quickScreenshotActive: false,
				userFacingWindowVisible: false),
			SoftwareUpdateImmediateInstallGate.canInstall(
				captureActive: true,
				quickScreenshotActive: false,
				userFacingWindowVisible: false) == false,
			SoftwareUpdateImmediateInstallGate.canInstall(
				captureActive: false,
				quickScreenshotActive: true,
				userFacingWindowVisible: false) == false,
			SoftwareUpdateImmediateInstallGate.canInstall(
				captureActive: true,
				quickScreenshotActive: true,
				userFacingWindowVisible: false) == false,
			SoftwareUpdateImmediateInstallGate.canInstall(
				captureActive: false,
				quickScreenshotActive: false,
				userFacingWindowVisible: true) == false
		else {
			fatalError("immediate update install should wait until Rsnap is idle")
		}
	}

	private static func assertCaptureHostCursorMapping() {
		guard
			CaptureHostCursorSupport.presentation(for: .default) == .arrow,
			CaptureHostCursorSupport.presentation(for: .crosshair) == .crosshair,
			CaptureHostCursorSupport.presentation(for: .grab) == .openHand,
			CaptureHostCursorSupport.presentation(for: .grabbing) == .closedHand,
			CaptureHostCursorSupport.presentation(for: .resizeNorth) == .resizeUpDown,
			CaptureHostCursorSupport.presentation(for: .resizeSouth) == .resizeUpDown,
			CaptureHostCursorSupport.presentation(for: .resizeEast) == .resizeLeftRight,
			CaptureHostCursorSupport.presentation(for: .resizeWest) == .resizeLeftRight,
			CaptureHostCursorSupport.presentation(for: .resizeNorthEast) == .resizeTopRight,
			CaptureHostCursorSupport.presentation(for: .resizeNorthWest) == .resizeTopLeft,
			CaptureHostCursorSupport.presentation(for: .resizeSouthEast) == .resizeBottomRight,
			CaptureHostCursorSupport.presentation(for: .resizeSouthWest) == .resizeBottomLeft,
			CaptureHostCursorSupport.presentation(for: .text) == .iBeam,
			CaptureHostCursorSupport.cursorIntent(for: .move, active: false) == .grab,
			CaptureHostCursorSupport.cursorIntent(for: .move, active: true) == .grabbing,
			CaptureHostCursorSupport.cursorIntent(for: .resizeLeft, active: false)
				== .resizeWest,
			CaptureHostCursorSupport.cursorIntent(for: .resizeRight, active: false)
				== .resizeEast,
			CaptureHostCursorSupport.cursorIntent(for: .resizeTop, active: false)
				== .resizeNorth,
			CaptureHostCursorSupport.cursorIntent(for: .resizeBottom, active: false)
				== .resizeSouth,
			CaptureHostCursorSupport.cursorIntent(for: .resizeTopLeft, active: false)
				== .resizeNorthWest,
			CaptureHostCursorSupport.cursorIntent(for: .resizeTopRight, active: false)
				== .resizeNorthEast,
			CaptureHostCursorSupport.cursorIntent(for: .resizeBottomLeft, active: false)
				== .resizeSouthWest,
			CaptureHostCursorSupport.cursorIntent(for: .resizeBottomRight, active: true)
				== .resizeSouthEast
		else {
			fatalError("capture host cursor support should preserve cursor mappings")
		}
	}

	private static func assertCaptureHostPointerDispatchSupport() {
		guard
			CaptureHostPointerDispatchEvent.moved(.zero).track == .hover,
			CaptureHostPointerDispatchEvent.liveDragged(.zero).track == .drag,
			approximatelyEqual(
				CaptureHostPointerDispatchTiming.delay(
					now: 10,
					targetInterval: 0.25,
					lastDispatchUptime: 9.90),
				0.15),
			CaptureHostPointerDispatchTiming.delay(
				now: 10,
				targetInterval: 0.25,
				lastDispatchUptime: 9.50) == 0
		else {
			fatalError("capture host pointer dispatch support should preserve throttling")
		}
	}

	private static func assertCaptureHostLivePrimaryInteractionState() {
		var state = CaptureHostLivePrimaryInteractionState()
		guard state.hasInteraction == false, state.dragDistance(from: .zero) == 0 else {
			fatalError("live primary state should start idle")
		}

		guard state.suppressHoverChrome(), state.hoverChromeSuppressed else {
			fatalError("live primary state should record hover suppression")
		}
		state.begin(at: CGPoint(x: 10, y: 20))
		guard
			state.hasInteraction,
			state.hoverChromeSuppressed,
			state.completionPoint(for: CGPoint(x: 40, y: 50)) == CGPoint(x: 10, y: 20),
			state.updateDragThreshold(from: CGPoint(x: 12, y: 22), threshold: 3) == false,
			state.updateDragThreshold(from: CGPoint(x: 14, y: 20), threshold: 3),
			state.dragExceededThreshold,
			state.completionPoint(for: CGPoint(x: 4, y: 30)) == CGPoint(x: 4, y: 30),
			state.immediateDragSelectionGlobal(
				current: CGPoint(x: 4, y: 30),
				in: CGRect(x: 0, y: 0, width: 100, height: 100)
			) == CGRect(x: 4, y: 20, width: 6, height: 10)
		else {
			fatalError("live primary state should preserve drag threshold behavior")
		}

		let completionPoint = state.markReleased(at: CGPoint(x: 4, y: 30))
		guard
			completionPoint == CGPoint(x: 4, y: 30),
			state.completionInFlight,
			state.hoverChromeSuppressed == false,
			state.immediateDragSelectionGlobal(
				current: nil,
				in: CGRect(x: 0, y: 0, width: 100, height: 100)
			) == CGRect(x: 4, y: 20, width: 6, height: 10)
		else {
			fatalError("live primary state should preserve release behavior")
		}

		state.reset()
		guard state == CaptureHostLivePrimaryInteractionState() else {
			fatalError("live primary state should reset to idle")
		}
	}

	private static func assertCaptureHostFrozenFirstDisplayHandoffState() {
		var state = CaptureHostFrozenFirstDisplayHandoffState()
		guard
			state.pending == false,
			state.completionQueued == false,
			state.startedAt == nil,
			state.pendingFrameDisplayed == false,
			state.allowsClassicToolbarGlass
		else {
			fatalError("frozen first-display handoff should start idle")
		}

		state.beginTransitionToFrozen(now: 4.25)
		guard
			state.pending,
			state.startedAt == 4.25,
			state.pendingFrameDisplayed == false,
			state.queueCompletionIfNeeded(),
			state.queueCompletionIfNeeded() == false
		else {
			fatalError("frozen first-display transition should queue completion once")
		}
		state.markPendingFrameDisplayed()
		guard
			state.finish()
				== CaptureHostFrozenFirstDisplayHandoffCompletion(
					startedAt: 4.25,
					pendingFrameDisplayed: true,
					deferredClassicToolbarGlass: false),
			state.pending == false,
			state.completionQueued == false,
			state.startedAt == nil,
			state.pendingFrameDisplayed == false,
			state.allowsClassicToolbarGlass
		else {
			fatalError("frozen first-display transition should finish with display evidence")
		}

		state.beginFrozenFirstFrameInstall(
			pending: true,
			defersClassicToolbarGlass: true,
			now: 8.5
		)
		guard
			state.pending,
			state.startedAt == 8.5,
			state.allowsClassicToolbarGlass == false,
			state.finish()
				== CaptureHostFrozenFirstDisplayHandoffCompletion(
					startedAt: 8.5,
					pendingFrameDisplayed: false,
					deferredClassicToolbarGlass: true),
			state.allowsClassicToolbarGlass == false
		else {
			fatalError("frozen first-frame install should preserve deferred toolbar glass")
		}
		state.clearDeferredClassicToolbarGlass()
		guard state.allowsClassicToolbarGlass else {
			fatalError("frozen first-frame install should clear deferred toolbar glass later")
		}

		state.beginFrozenFirstFrameInstall(
			pending: false,
			defersClassicToolbarGlass: true,
			now: 10
		)
		guard
			state.pending == false,
			state.startedAt == nil,
			state.queueCompletionIfNeeded() == false,
			state.finish() == nil
		else {
			fatalError("frozen first-frame install should stay idle when no pending frame exists")
		}

		state.reset()
		guard state == CaptureHostFrozenFirstDisplayHandoffState() else {
			fatalError("frozen first-display handoff should reset to idle")
		}
	}

	private static func assertCaptureHostAnnotationStyleWheelGate() {
		var gate = CaptureHostAnnotationStyleWheelGate()
		guard
			gate.steps(
				timestamp: 1,
				deltaY: 0.01,
				hasPreciseScrollingDeltas: false,
				phaseActive: false,
				phaseEndedOrCancelled: false,
				momentumActive: false
			) == 0,
			gate.steps(
				timestamp: 1,
				deltaY: 1,
				hasPreciseScrollingDeltas: false,
				phaseActive: false,
				phaseEndedOrCancelled: false,
				momentumActive: false
			) == 1,
			gate.steps(
				timestamp: 1.02,
				deltaY: -1,
				hasPreciseScrollingDeltas: false,
				phaseActive: false,
				phaseEndedOrCancelled: false,
				momentumActive: false
			) == 0,
			gate.steps(
				timestamp: 1.05,
				deltaY: -1,
				hasPreciseScrollingDeltas: false,
				phaseActive: false,
				phaseEndedOrCancelled: false,
				momentumActive: false
			) == -1
		else {
			fatalError("annotation style wheel gate should throttle discrete wheel steps")
		}

		gate.reset()
		guard
			gate.steps(
				timestamp: 2,
				deltaY: 1,
				hasPreciseScrollingDeltas: true,
				phaseActive: true,
				phaseEndedOrCancelled: false,
				momentumActive: false
			) == 1,
			gate.steps(
				timestamp: 2.10,
				deltaY: 1,
				hasPreciseScrollingDeltas: true,
				phaseActive: true,
				phaseEndedOrCancelled: false,
				momentumActive: false
			) == 0,
			gate.steps(
				timestamp: 2.19,
				deltaY: 1,
				hasPreciseScrollingDeltas: true,
				phaseActive: true,
				phaseEndedOrCancelled: false,
				momentumActive: false
			) == 1,
			gate.steps(
				timestamp: 2.20,
				deltaY: 1,
				hasPreciseScrollingDeltas: false,
				phaseActive: false,
				phaseEndedOrCancelled: false,
				momentumActive: true
			) == 0
		else {
			fatalError("annotation style wheel gate should throttle precise wheel steps")
		}

		guard
			gate.steps(
				timestamp: 2.21,
				deltaY: 1,
				hasPreciseScrollingDeltas: false,
				phaseActive: false,
				phaseEndedOrCancelled: true,
				momentumActive: false
			) == 0,
			gate.steps(
				timestamp: 2.22,
				deltaY: -1,
				hasPreciseScrollingDeltas: false,
				phaseActive: false,
				phaseEndedOrCancelled: false,
				momentumActive: false
			) == -1
		else {
			fatalError("annotation style wheel gate should reset on ended phases")
		}
	}

	private static func assertCaptureHostToolbarHoverState() {
		var state = CaptureHostToolbarHoverState()
		guard state.isActive == false, state.clear() == false else {
			fatalError("toolbar hover state should start idle")
		}

		let toolbarHit = FrozenToolbarHitState(
			pointerOverToolbar: true,
			toolbarAction: .copy,
			annotationStyleAction: nil
		)
		guard
			state.update(to: toolbarHit),
			state.isActive,
			state.pointerOverToolbar,
			state.toolbarAction == .copy,
			state.annotationStyleAction == nil,
			state.update(to: toolbarHit) == false
		else {
			fatalError("toolbar hover state should update only when toolbar hit state changes")
		}

		let styleHit = FrozenToolbarHitState(
			pointerOverToolbar: true,
			toolbarAction: nil,
			annotationStyleAction: .decreaseSize
		)
		guard
			state.update(to: styleHit),
			state.pointerOverToolbar,
			state.toolbarAction == nil,
			state.annotationStyleAction == .decreaseSize,
			state.clear(),
			state == CaptureHostToolbarHoverState()
		else {
			fatalError("toolbar hover state should switch and clear hover targets")
		}
	}

	private static func assertCaptureHostLiveSampleCachePointMatching() {
		let currentRgb = LiveRgbSample(
			rgb: RGBSample(r: 1, g: 2, b: 3),
			capturedAtUptime: ProcessInfo.processInfo.systemUptime,
			source: "probe"
		)
		let staleRgb = LiveRgbSample(
			rgb: RGBSample(r: 4, g: 5, b: 6),
			capturedAtUptime:
				ProcessInfo.processInfo.systemUptime - LiveRgbSample.maximumDisplayAge - 1,
			source: "probe-stale"
		)
		let loupePatch = makeProbeImage()
		var cache = CaptureHostLiveSampleCache()
		cache.seedChrome(LiveChromeSample(rgb: currentRgb, loupePatch: nil), point: .zero)
		cache.seedRgb(currentRgb, point: .zero)
		guard
			cache.chromeSample(matching: CGPoint(x: 0.5, y: -0.5))?.rgb?.rgb
				== RGBSample(r: 1, g: 2, b: 3),
			cache.rgbSample(matching: CGPoint(x: 0.5, y: -0.5))?.rgb
				== RGBSample(r: 1, g: 2, b: 3),
			cache.chromeSample(matching: CGPoint(x: 0.51, y: 0)) == nil,
			cache.rgbSample(matching: CGPoint(x: 0.51, y: 0)) == nil
		else {
			fatalError("live sample cache should reuse fresh samples only at matching points")
		}

		cache.seedChrome(LiveChromeSample(rgb: staleRgb, loupePatch: loupePatch), point: .zero)
		cache.seedRgb(staleRgb, point: .zero)
		guard
			cache.chromeSample(matching: .zero)?.rgb == nil,
			cache.chromeSample(matching: .zero)?.loupePatch === loupePatch,
			cache.rgbSample(matching: .zero) == nil
		else {
			fatalError(
				"live sample cache should preserve stale chrome patch while stripping stale RGB")
		}
		cache.reset()
		guard cache.latestChrome == nil, cache.latestRgb == nil else {
			fatalError("live sample cache should reset cached samples")
		}

		guard
			CaptureHostLiveSampleCache.pointsMatch(nil, nil),
			CaptureHostLiveSampleCache.pointsMatch(CGPoint(x: 10, y: 20), nil) == false,
			CaptureHostLiveSampleCache.pointsMatch(nil, CGPoint(x: 10, y: 20)) == false,
			CaptureHostLiveSampleCache.pointsMatch(
				CGPoint(x: 10, y: 20),
				CGPoint(x: 10.5, y: 19.5)
			),
			CaptureHostLiveSampleCache.pointsMatch(
				CGPoint(x: 10, y: 20),
				CGPoint(x: 10.51, y: 20)
			) == false,
			CaptureHostLiveSampleCache.pointsMatch(
				CGPoint(x: 10, y: 20),
				CGPoint(x: 10, y: 19.49)
			) == false
		else {
			fatalError("live sample cache should preserve point matching tolerance")
		}
	}

	private static func makeProbeImage() -> CGImage {
		let data = Data([255, 0, 0, 255])
		guard
			let provider = CGDataProvider(data: data as CFData),
			let image = CGImage(
				width: 1,
				height: 1,
				bitsPerComponent: 8,
				bitsPerPixel: 32,
				bytesPerRow: 4,
				space: CGColorSpaceCreateDeviceRGB(),
				bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
				provider: provider,
				decode: nil,
				shouldInterpolate: false,
				intent: .defaultIntent
			)
		else {
			fatalError("expected probe image")
		}
		return image
	}

	private static func approximatelyEqual(
		_ lhs: TimeInterval,
		_ rhs: TimeInterval,
		tolerance: TimeInterval = 0.000_001
	) -> Bool {
		abs(lhs - rhs) <= tolerance
	}

	private static func assertFrozenToolbarPlannerDisablesEditingDuringScroll() {
		let items = FrozenToolbarLayoutPlanner.visibleItems(
			from: [
				ToolbarItem(kind: .pen, enabled: true, selected: true),
				ToolbarItem(kind: .undo, enabled: true, selected: false),
				ToolbarItem(kind: .redo, enabled: true, selected: false),
				ToolbarItem(kind: .autoCenter, enabled: true, selected: false),
				ToolbarItem(kind: .scroll, enabled: false, selected: false),
				ToolbarItem(kind: .copy, enabled: true, selected: false),
			],
			availability: FrozenToolbarAvailability(
				scrollCaptureActive: true,
				canUndo: true,
				canRedo: true,
				frozenSelectionAvailable: true,
				keepsFrozenSelectionFixed: false,
				scrollToolbarEnabled: true,
				hasRecognizeTextBlockingEdits: false
			)
		)
		guard
			items.first(where: { $0.kind == .pen })?.enabled == false,
			items.first(where: { $0.kind == .undo })?.enabled == false,
			items.first(where: { $0.kind == .redo })?.enabled == false,
			items.first(where: { $0.kind == .autoCenter })?.enabled == false,
			items.first(where: { $0.kind == .scroll })?.enabled == true,
			items.first(where: { $0.kind == .copy })?.enabled == true
		else {
			fatalError("frozen toolbar planner should isolate scroll capture edit availability")
		}
	}

	private static func assertFrozenToolbarPlannerPlacesStyleRowAndHitsControls() {
		let items = FrozenToolbarLayoutPlanner.visibleItems(
			from: [
				ToolbarItem(kind: .pen, enabled: true, selected: true),
				ToolbarItem(kind: .copy, enabled: true, selected: false),
				ToolbarItem(kind: .save, enabled: true, selected: false),
			],
			availability: FrozenToolbarAvailability(
				scrollCaptureActive: false,
				canUndo: false,
				canRedo: false,
				frozenSelectionAvailable: true,
				keepsFrozenSelectionFixed: false,
				scrollToolbarEnabled: false,
				hasRecognizeTextBlockingEdits: false
			)
		)
		let selection = CGRect(x: 100, y: 100, width: 80, height: 50)
		guard
			let layout = FrozenToolbarLayoutPlanner.layout(
				selection: selection,
				bounds: CGRect(x: 0, y: 0, width: 320, height: 320),
				prefersTopPlacement: false,
				items: items,
				annotationStyle: FrozenAnnotationStyleState()
			),
			let annotationStyle = layout.annotationStyle
		else {
			fatalError("frozen toolbar planner should include brush style controls")
		}

		guard layout.frame.minY > selection.maxY else {
			fatalError("frozen toolbar planner should place room-available bottom toolbar")
		}
		let firstItem = layout.items[0]
		let itemHit = FrozenToolbarLayoutPlanner.hitState(
			at: CGPoint(x: firstItem.frame.midX, y: firstItem.frame.midY),
			in: layout
		)
		guard itemHit.pointerOverToolbar, itemHit.toolbarAction == .pen else {
			fatalError("frozen toolbar planner should hit enabled primary toolbar items")
		}

		let decreaseHit = FrozenToolbarLayoutPlanner.hitState(
			at: CGPoint(
				x: annotationStyle.decreaseFrame.midX, y: annotationStyle.decreaseFrame.midY),
			in: layout
		)
		guard decreaseHit.annotationStyleAction == .decreaseSize else {
			fatalError("frozen toolbar planner should hit annotation size controls")
		}

		let localStyle = FrozenToolbarLayoutPlanner.localAnnotationStyleLayout(
			annotationStyle,
			relativeTo: layout.frame
		)
		guard localStyle.frame.minX >= 0, localStyle.frame.maxX <= layout.frame.width else {
			fatalError(
				"frozen toolbar planner should translate style controls into toolbar-local space")
		}
	}

	private static func assertCaptureOverlayLocalPointKeepsScreenEdgesVisible() {
		let windowFrame = CGRect(x: 0, y: 0, width: 1_440, height: 900)
		let bounds = CGRect(x: 0, y: 0, width: 1_440, height: 900)

		guard
			captureOverlayLocalPoint(
				from: CGPoint(x: windowFrame.maxX, y: windowFrame.maxY),
				windowFrame: windowFrame,
				bounds: bounds
			) == CGPoint(x: bounds.maxX, y: bounds.maxY)
		else {
			fatalError("capture overlay should keep HUD placement alive at screen max edges")
		}

		guard
			captureOverlayLocalPoint(
				from: CGPoint(x: windowFrame.minX, y: windowFrame.minY),
				windowFrame: windowFrame,
				bounds: bounds
			) == CGPoint(x: bounds.minX, y: bounds.minY)
		else {
			fatalError("capture overlay should keep HUD placement alive at screen min edges")
		}

		guard
			captureOverlayLocalPoint(
				from: CGPoint(x: windowFrame.maxX + 1, y: windowFrame.midY),
				windowFrame: windowFrame,
				bounds: bounds
			) == nil
		else {
			fatalError("capture overlay should still reject points outside the screen edge")
		}
	}

	private static func assertRectEqual(_ actual: CGRect, _ expected: CGRect, _ message: String) {
		guard nearlyEqual(actual.origin.x, expected.origin.x),
			nearlyEqual(actual.origin.y, expected.origin.y),
			nearlyEqual(actual.width, expected.width),
			nearlyEqual(actual.height, expected.height)
		else {
			fatalError("\(message): expected \(expected), got \(actual)")
		}
	}

	private static func nearlyEqual(_ actual: CGFloat, _ expected: CGFloat) -> Bool {
		abs(actual - expected) <= 0.000_1
	}

	private static func assertLaunchAtLoginStateMapping() {
		let enabled = LaunchAtLoginController.state(for: .enabled)
		guard enabled.isOn, enabled.isControlEnabled else {
			fatalError("enabled login item state should keep the toggle on")
		}

		let pending = LaunchAtLoginController.state(for: .requiresApproval)
		guard pending.isOn, pending.subtitle.contains("approval") else {
			fatalError("pending login item state should explain approval")
		}

		let missingBundle = LaunchAtLoginController.state(for: .notFound)
		guard missingBundle.isOn == false, missingBundle.isControlEnabled else {
			fatalError("missing app bundle should keep the login item toggle clickable")
		}

		let failed = LaunchAtLoginController.state(
			for: .notRegistered,
			errorMessage: "registration failed")
		guard failed.isOn == false, failed.subtitle.contains("failed") else {
			fatalError("failed login item update should keep current state and surface failure")
		}
	}

	private static func assertScrollCaptureViewportPointAcceptsFlippedGlobalMouseCoordinates() {
		let viewport = CGRect(x: 327, y: 941, width: 808, height: 295)
		let desktop = CGRect(x: 0, y: 0, width: 3_008, height: 1_692)
		let rawPoint = CGPoint(x: 1_006, y: 676)
		guard
			let viewportPoint = scrollCaptureViewportPoint(
				for: rawPoint,
				in: viewport,
				desktopFrame: desktop
			),
			viewportPoint == CGPoint(x: 1_006, y: 1_016)
		else {
			fatalError("scroll capture should accept top-origin global wheel coordinates")
		}

		let nativePoint = CGPoint(x: 1_006, y: 1_016)
		guard
			scrollCaptureViewportPoint(
				for: nativePoint,
				in: viewport,
				desktopFrame: desktop
			) == nativePoint
		else {
			fatalError("scroll capture should preserve native bottom-origin wheel coordinates")
		}
	}

	private static func assertScrollCaptureObservedInputAcceptsSourceWindowGutter() {
		let viewport = CGRect(x: 333, y: 1_119, width: 821, height: 461)
		let sourceFrame = CGRect(x: 200, y: 900, width: 1_500, height: 720)
		let desktop = CGRect(x: 0, y: 0, width: 3_008, height: 1_692)
		let gutterPoint = CGPoint(x: 1_319, y: 1_090)
		guard
			let inputPoint = scrollCaptureObservedInputPoint(
				for: gutterPoint,
				viewportRect: viewport,
				sourceFrame: sourceFrame,
				desktopFrame: desktop,
				padding: 260
			),
			inputPoint.inputSource == "near_viewport",
			inputPoint.insideViewport == false,
			inputPoint.viewportPoint == CGPoint(x: 1_154, y: 1_119)
		else {
			fatalError("scroll capture should accept wheel input in the source window gutter")
		}

		let smokeViewport = CGRect(x: 902, y: 508, width: 1_203, height: 677)
		let smokeRightOutsidePoint = CGPoint(x: 2_325, y: 847)
		guard
			let smokeInputPoint = scrollCaptureObservedInputPoint(
				for: smokeRightOutsidePoint,
				viewportRect: smokeViewport,
				sourceFrame: .null,
				desktopFrame: desktop,
				padding: 360
			),
			smokeInputPoint.inputSource == "near_viewport",
			smokeInputPoint.insideViewport == false,
			smokeInputPoint.viewportPoint == CGPoint(x: 2_105, y: 847)
		else {
			fatalError("scroll capture should accept right-side smoke input near the viewport")
		}

		let unrelatedPoint = CGPoint(x: 40, y: 40)
		guard
			scrollCaptureObservedInputPoint(
				for: unrelatedPoint,
				viewportRect: viewport,
				sourceFrame: sourceFrame,
				desktopFrame: desktop,
				padding: 260
			) == nil
		else {
			fatalError("scroll capture should ignore wheel input far outside the source window")
		}
	}

	private static func assertCaptureFrameEffectExpandsExportCanvas() {
		let imageSize = CGSize(width: 320, height: 180)
		let canvasSize = CaptureFrameEffectRenderer.canvasSize(for: imageSize)
		guard canvasSize.width > imageSize.width, canvasSize.height > imageSize.height else {
			fatalError("capture frame canvas should add room for wallpaper and shadow")
		}
		let imageRect = CaptureFrameEffectRenderer.imageRect(for: imageSize)
		guard imageRect.minX > 0, imageRect.minY > 0 else {
			fatalError("capture frame image rect should be inset from the canvas edge")
		}
		guard let source = solidImage(width: 320, height: 180) else {
			fatalError("could not build capture frame probe image")
		}
		guard
			let rendered = CaptureFrameEffectRenderer.render(
				image: source,
				background: .aurora,
				screen: nil,
				source: .window
			)
		else {
			fatalError("capture frame renderer should produce an image for gradient presets")
		}
		guard rendered.width == Int(canvasSize.width), rendered.height == Int(canvasSize.height)
		else {
			fatalError("capture frame renderer size should match layout geometry")
		}
		guard
			let renderedWindowSnapshot = CaptureFrameEffectRenderer.renderWindowSnapshot(
				image: source,
				background: .aurora,
				screen: nil
			),
			renderedWindowSnapshot.width == Int(canvasSize.width),
			renderedWindowSnapshot.height == Int(canvasSize.height)
		else {
			fatalError("window snapshot frame renderer should preserve layout geometry")
		}
		guard let pixels = rgbaPixels(from: rendered) else {
			fatalError("could not read capture frame rendered pixels")
		}
		let center = pixel(
			in: pixels,
			width: rendered.width,
			height: rendered.height,
			x: Int(imageRect.midX),
			yFromBottom: Int(imageRect.midY)
		)
		guard center.0 > 20, center.1 > 35, center.2 > 60, center.3 > 240 else {
			fatalError("capture frame should draw the source image inside the framed rect")
		}
		let background = pixel(
			in: pixels,
			width: rendered.width,
			height: rendered.height,
			x: 8,
			yFromBottom: 8
		)
		guard background.3 > 240 else {
			fatalError("capture frame background should be opaque")
		}
	}

	private static func solidImage(width: Int, height: Int) -> CGImage? {
		guard
			let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
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
			return nil
		}
		context.setFillColor(CGColor(red: 0.14, green: 0.22, blue: 0.32, alpha: 1))
		context.fill(CGRect(x: 0, y: 0, width: width, height: height))
		return context.makeImage()
	}

	private static func rgbaPixels(from image: CGImage) -> [UInt8]? {
		let width = image.width
		let height = image.height
		var pixels = [UInt8](repeating: 0, count: width * height * 4)
		let rendered = pixels.withUnsafeMutableBytes { buffer -> Bool in
			guard
				let data = buffer.baseAddress,
				let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
				let context = CGContext(
					data: data,
					width: width,
					height: height,
					bitsPerComponent: 8,
					bytesPerRow: width * 4,
					space: colorSpace,
					bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
				)
			else {
				return false
			}
			context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
			return true
		}
		return rendered ? pixels : nil
	}

	private static func pixel(
		in data: [UInt8],
		width: Int,
		height: Int,
		x: Int,
		yFromBottom: Int
	) -> (UInt8, UInt8, UInt8, UInt8) {
		let clampedX = max(0, min(width - 1, x))
		let y = rowIndex(fromBottom: yFromBottom, height: height)
		let offset = (y * width + clampedX) * 4
		return (data[offset], data[offset + 1], data[offset + 2], data[offset + 3])
	}

	private static func assertScrimRoundedExclusionKeepsCornersMasked() {
		let width = 80
		let height = 80
		let byteCount = width * height * 4
		let data = UnsafeMutablePointer<UInt8>.allocate(capacity: byteCount)
		data.initialize(repeating: 0, count: byteCount)
		defer {
			data.deinitialize(count: byteCount)
			data.deallocate()
		}
		guard
			let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
			let context = CGContext(
				data: data,
				width: width,
				height: height,
				bitsPerComponent: 8,
				bytesPerRow: width * 4,
				space: colorSpace,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			fatalError("could not create scrim geometry probe context")
		}

		OverlayMaskGeometry.drawScrim(
			in: context,
			bounds: CGRect(x: 0, y: 0, width: width, height: height),
			focusRect: CGRect(x: 48, y: 48, width: 16, height: 16),
			color: CGColor(red: 0, green: 0, blue: 0, alpha: 1),
			roundedExclusions: [
				OverlayMaskGeometry.RoundedExclusion(
					rect: CGRect(x: 10, y: 10, width: 40, height: 24),
					cornerRadius: 12
				)
			]
		)

		guard clearPixel(in: data, width: width, height: height, x: 30, yFromBottom: 22)
		else {
			fatalError("rounded scrim exclusion did not clear the HUD body")
		}
		guard opaquePixel(in: data, width: width, height: height, x: 10, yFromBottom: 10)
		else {
			fatalError("rounded scrim exclusion cleared a square corner")
		}
		guard clearPixel(in: data, width: width, height: height, x: 56, yFromBottom: 56)
		else {
			fatalError("selection focus rect was not cleared")
		}
	}

	private static func assertScrimOverlappingRoundedExclusionStaysClear() {
		let width = 96
		let height = 80
		let byteCount = width * height * 4
		let data = UnsafeMutablePointer<UInt8>.allocate(capacity: byteCount)
		data.initialize(repeating: 0, count: byteCount)
		defer {
			data.deinitialize(count: byteCount)
			data.deallocate()
		}
		guard
			let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
			let context = CGContext(
				data: data,
				width: width,
				height: height,
				bitsPerComponent: 8,
				bytesPerRow: width * 4,
				space: colorSpace,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			fatalError("could not create overlapping scrim geometry probe context")
		}

		OverlayMaskGeometry.drawScrim(
			in: context,
			bounds: CGRect(x: 0, y: 0, width: width, height: height),
			focusRect: CGRect(x: 42, y: 18, width: 28, height: 28),
			color: CGColor(red: 0, green: 0, blue: 0, alpha: 1),
			roundedExclusions: [
				OverlayMaskGeometry.RoundedExclusion(
					rect: CGRect(x: 24, y: 18, width: 40, height: 24),
					cornerRadius: 12
				)
			]
		)

		guard clearPixel(in: data, width: width, height: height, x: 36, yFromBottom: 30)
		else {
			fatalError("rounded scrim exclusion did not clear the HUD body outside focus")
		}
		guard clearPixel(in: data, width: width, height: height, x: 52, yFromBottom: 30)
		else {
			fatalError("overlapping focus and HUD exclusions refilled the scrim")
		}
		guard opaquePixel(in: data, width: width, height: height, x: 12, yFromBottom: 12)
		else {
			fatalError("overlapping scrim probe did not leave ordinary scrim opaque")
		}
	}

	private static func assertScrimExclusionPreservesExistingPixels() {
		let width = 80
		let height = 80
		let byteCount = width * height * 4
		let data = UnsafeMutablePointer<UInt8>.allocate(capacity: byteCount)
		for index in stride(from: 0, to: byteCount, by: 4) {
			data[index] = 24
			data[index + 1] = 96
			data[index + 2] = 180
			data[index + 3] = 255
		}
		defer {
			data.deinitialize(count: byteCount)
			data.deallocate()
		}
		guard
			let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
			let context = CGContext(
				data: data,
				width: width,
				height: height,
				bitsPerComponent: 8,
				bytesPerRow: width * 4,
				space: colorSpace,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			fatalError("could not create scrim preservation probe context")
		}

		let exclusionPath = CGPath(
			roundedRect: CGRect(x: 10, y: 10, width: 40, height: 24),
			cornerWidth: 12,
			cornerHeight: 12,
			transform: nil
		)
		OverlayMaskGeometry.drawScrim(
			in: context,
			bounds: CGRect(x: 0, y: 0, width: width, height: height),
			focusRect: CGRect(x: 50, y: 50, width: 16, height: 16),
			color: CGColor(red: 0, green: 0, blue: 0, alpha: 1),
			pathExclusions: [exclusionPath]
		)

		guard
			rgbaPixel(in: data, width: width, height: height, x: 30, yFromBottom: 22)
				== (24, 96, 180, 255)
		else {
			fatalError("scrim exclusion cleared existing toolbar backing pixels")
		}
		guard opaquePixel(in: data, width: width, height: height, x: 4, yFromBottom: 4)
		else {
			fatalError("ordinary scrim pixel became transparent")
		}
	}

	private static func assertRoundedExclusionMaskKeepsCornersFilled() {
		let width = 80
		let height = 80
		let byteCount = width * height * 4
		let data = UnsafeMutablePointer<UInt8>.allocate(capacity: byteCount)
		data.initialize(repeating: 0, count: byteCount)
		defer {
			data.deinitialize(count: byteCount)
			data.deallocate()
		}
		guard
			let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
			let context = CGContext(
				data: data,
				width: width,
				height: height,
				bitsPerComponent: 8,
				bytesPerRow: width * 4,
				space: colorSpace,
				bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
			)
		else {
			fatalError("could not create rounded mask probe context")
		}

		context.setFillColor(CGColor(red: 0, green: 0, blue: 0, alpha: 1))
		context.addPath(
			OverlayMaskGeometry.evenOddMaskPath(
				bounds: CGRect(x: 0, y: 0, width: width, height: height),
				roundedExclusions: [
					OverlayMaskGeometry.RoundedExclusion(
						rect: CGRect(x: 10, y: 10, width: 40, height: 24),
						cornerRadius: 12
					)
				]
			)
		)
		context.drawPath(using: .eoFill)

		guard clearPixel(in: data, width: width, height: height, x: 30, yFromBottom: 22)
		else {
			fatalError("rounded mask did not exclude the HUD body")
		}
		guard opaquePixel(in: data, width: width, height: height, x: 11, yFromBottom: 11)
		else {
			fatalError("rounded mask excluded a square corner")
		}
	}

	private static func opaquePixel(
		in data: UnsafePointer<UInt8>,
		width: Int,
		height: Int,
		x: Int,
		yFromBottom: Int
	) -> Bool {
		let offset = (rowIndex(fromBottom: yFromBottom, height: height) * width + x) * 4
		return data[offset + 3] > 200
	}

	private static func clearPixel(
		in data: UnsafePointer<UInt8>,
		width: Int,
		height: Int,
		x: Int,
		yFromBottom: Int
	) -> Bool {
		let offset = (rowIndex(fromBottom: yFromBottom, height: height) * width + x) * 4
		return data[offset + 3] < 20
	}

	private static func rgbaPixel(
		in data: UnsafePointer<UInt8>,
		width: Int,
		height: Int,
		x: Int,
		yFromBottom: Int
	) -> (UInt8, UInt8, UInt8, UInt8) {
		let offset = (rowIndex(fromBottom: yFromBottom, height: height) * width + x) * 4
		return (data[offset], data[offset + 1], data[offset + 2], data[offset + 3])
	}

	private static func rowIndex(fromBottom y: Int, height: Int) -> Int {
		max(0, min(height - 1, height - y - 1))
	}
}
