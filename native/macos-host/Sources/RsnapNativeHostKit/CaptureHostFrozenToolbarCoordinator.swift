import AppKit
import CoreGraphics
import RsnapHostBridge

@MainActor
final class CaptureHostFrozenToolbarCoordinator {
	private unowned let hostView: CaptureHostView
	private(set) var hoverState = CaptureHostToolbarHoverState()

	init(hostView: CaptureHostView) {
		self.hostView = hostView
	}

	private var scene: SceneSnapshot { hostView.scene }
	private var chrome: CaptureChromeState { hostView.chrome }
	private var settings: NativeHostSettings { hostView.settings }
	private var controller: CaptureSessionController? { hostView.controller }

	func layout(for selection: CGRect) -> FrozenToolbarLayout? {
		FrozenToolbarLayoutPlanner.layout(
			selection: selection,
			bounds: hostView.bounds,
			prefersTopPlacement: settings.toolbarPlacement == .top,
			items: visibleItems(),
			annotationStyle: chrome.annotationStyle
		)
	}

	func visibleItems() -> [ToolbarItem] {
		FrozenToolbarLayoutPlanner.visibleItems(
			from: scene.toolbarItems,
			availability: FrozenToolbarAvailability(
				scrollCaptureActive: chrome.scrollMinimapPreview != nil,
				canUndo: chrome.frozenOverlay.canUndo,
				canRedo: chrome.frozenOverlay.canRedo,
				frozenSelectionAvailable: scene.frozenSelection != nil,
				keepsFrozenSelectionFixed: chrome.frozenOverlay.keepsFrozenSelectionFixed,
				scrollToolbarEnabled: controller?.scrollCaptureToolbarEnabled ?? false,
				hasRecognizeTextBlockingEdits: chrome.frozenOverlay.hasRecognizeTextBlockingEdits
			)
		)
	}

	func item(_ kind: ToolbarItemKind) -> ToolbarItem? {
		visibleItems().first(where: { $0.kind == kind })
	}

	func toolbarAction(at point: CGPoint) -> ToolbarItemKind? {
		hitState(at: point).toolbarAction
	}

	func annotationStyleAction(at point: CGPoint) -> FrozenAnnotationStyleAction? {
		hitState(at: point).annotationStyleAction
	}

	func annotationStyleSizeControlContains(_ point: CGPoint) -> Bool {
		guard scene.mode == .frozen, let selection = hostView.localFrozenSelectionRect(),
			let styleLayout = layout(for: selection)?.annotationStyle
		else {
			return false
		}
		return styleLayout.sizeControlFrame.contains(point)
	}

	func frameContains(_ point: CGPoint) -> Bool {
		hitState(at: point).pointerOverToolbar
	}

	func performToolbarAction(_ action: ToolbarItemKind) {
		switch action {
		case .undo:
			controller?.performFrozenUndo()
		case .redo:
			controller?.performFrozenRedo()
		case .autoCenter:
			controller?.performFrozenAutoCenter()
		default:
			controller?.invokeToolbarItem(action)
		}
	}

	func performAnnotationStyleAction(_ action: FrozenAnnotationStyleAction) {
		controller?.performFrozenAnnotationStyleAction(action)
	}

	func clearHoveredAction() {
		guard hoverState.clear() else {
			return
		}
	}

	func refreshHoveredAction(for localPoint: CGPoint? = nil) {
		let probePoint =
			scene.mode == .frozen ? (localPoint ?? hostView.currentLocalMousePoint()) : nil
		let nextHitState: FrozenToolbarHitState
		if let probePoint {
			nextHitState = hitState(at: probePoint)
		} else {
			nextHitState = FrozenToolbarHitState(
				pointerOverToolbar: false,
				toolbarAction: nil,
				annotationStyleAction: nil
			)
		}
		if hoverState.update(to: nextHitState) {
			hostView.syncVisibleCursor()
			hostView.updateChromeMaterialViews()
			hostView.needsDisplay = true
		}
	}

	func hitState(at point: CGPoint) -> FrozenToolbarHitState {
		guard scene.mode == .frozen, let selection = hostView.localFrozenSelectionRect() else {
			return FrozenToolbarHitState(
				pointerOverToolbar: false,
				toolbarAction: nil,
				annotationStyleAction: nil
			)
		}
		return FrozenToolbarLayoutPlanner.hitState(at: point, in: layout(for: selection))
	}
}
