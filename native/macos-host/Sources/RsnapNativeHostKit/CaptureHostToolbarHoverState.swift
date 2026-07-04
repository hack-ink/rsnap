import RsnapHostBridge

package struct CaptureHostToolbarHoverState: Equatable {
	package private(set) var pointerOverToolbar = false
	package private(set) var toolbarAction: ToolbarItemKind?
	package private(set) var annotationStyleAction: FrozenAnnotationStyleAction?

	package init() {}

	package var isActive: Bool {
		pointerOverToolbar || toolbarAction != nil || annotationStyleAction != nil
	}

	@discardableResult
	package mutating func update(to hitState: FrozenToolbarHitState) -> Bool {
		guard
			pointerOverToolbar != hitState.pointerOverToolbar
				|| toolbarAction != hitState.toolbarAction
				|| annotationStyleAction != hitState.annotationStyleAction
		else {
			return false
		}
		pointerOverToolbar = hitState.pointerOverToolbar
		toolbarAction = hitState.toolbarAction
		annotationStyleAction = hitState.annotationStyleAction
		return true
	}

	@discardableResult
	package mutating func clear() -> Bool {
		guard isActive else {
			return false
		}
		pointerOverToolbar = false
		toolbarAction = nil
		annotationStyleAction = nil
		return true
	}
}
