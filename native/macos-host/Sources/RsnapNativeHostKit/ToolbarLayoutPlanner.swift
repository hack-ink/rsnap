import CoreGraphics
import RsnapHostBridge

package struct FrozenToolbarAvailability {
	package let scrollCaptureActive: Bool
	package let canUndo: Bool
	package let canRedo: Bool
	package let frozenSelectionAvailable: Bool
	package let keepsFrozenSelectionFixed: Bool
	package let scrollToolbarEnabled: Bool
	package let hasRecognizeTextBlockingEdits: Bool

	package init(
		scrollCaptureActive: Bool,
		canUndo: Bool,
		canRedo: Bool,
		frozenSelectionAvailable: Bool,
		keepsFrozenSelectionFixed: Bool,
		scrollToolbarEnabled: Bool,
		hasRecognizeTextBlockingEdits: Bool
	) {
		self.scrollCaptureActive = scrollCaptureActive
		self.canUndo = canUndo
		self.canRedo = canRedo
		self.frozenSelectionAvailable = frozenSelectionAvailable
		self.keepsFrozenSelectionFixed = keepsFrozenSelectionFixed
		self.scrollToolbarEnabled = scrollToolbarEnabled
		self.hasRecognizeTextBlockingEdits = hasRecognizeTextBlockingEdits
	}
}

package struct FrozenToolbarHitState: Equatable {
	package let pointerOverToolbar: Bool
	package let toolbarAction: ToolbarItemKind?
	package let annotationStyleAction: FrozenAnnotationStyleAction?

	package init(
		pointerOverToolbar: Bool,
		toolbarAction: ToolbarItemKind?,
		annotationStyleAction: FrozenAnnotationStyleAction?
	) {
		self.pointerOverToolbar = pointerOverToolbar
		self.toolbarAction = toolbarAction
		self.annotationStyleAction = annotationStyleAction
	}
}

package enum FrozenToolbarLayoutPlanner {
	package static func visibleItems(
		from sourceItems: [ToolbarItem],
		availability: FrozenToolbarAvailability
	) -> [ToolbarItem] {
		sourceItems.map { originalItem in
			var item = originalItem
			switch item.kind {
			case .pointer, .pen, .arrow, .mosaic, .spotlight, .text:
				item.enabled = originalItem.enabled && !availability.scrollCaptureActive
			case .undo:
				item.enabled = availability.canUndo && !availability.scrollCaptureActive
			case .redo:
				item.enabled = availability.canRedo && !availability.scrollCaptureActive
			case .autoCenter:
				item.enabled =
					availability.frozenSelectionAvailable
					&& !availability.keepsFrozenSelectionFixed
					&& !availability.scrollCaptureActive
			case .scroll:
				item.enabled = availability.scrollToolbarEnabled
			case .ocr:
				item.enabled =
					originalItem.enabled && !availability.hasRecognizeTextBlockingEdits
			case .copy, .save:
				item.enabled = originalItem.enabled
			}
			return item
		}
	}

	package static func layout(
		selection: CGRect,
		bounds: CGRect,
		prefersTopPlacement: Bool,
		items: [ToolbarItem],
		annotationStyle: FrozenAnnotationStyleState
	) -> FrozenToolbarLayout? {
		guard items.isEmpty == false else {
			return nil
		}

		let styleKind = selectedAnnotationStyleKind(in: items)
		let metrics = CaptureChrome.toolbarMetrics()
		let itemCount = CGFloat(items.count)
		let primaryContentWidth =
			itemCount * metrics.buttonSize
			+ max(0, itemCount - 1) * metrics.itemSpacing
		let styleContentWidth =
			styleKind.map { annotationStyleContentWidth(for: $0, metrics: metrics) } ?? 0
		let contentWidth = max(primaryContentWidth, styleContentWidth)
		let width = contentWidth + metrics.horizontalPadding * 2
		let primaryRowHeight = metrics.verticalPadding * 2 + metrics.buttonSize
		let height = styleKind == nil ? primaryRowHeight : primaryRowHeight * 2
		let frame = toolbarFrame(
			selection: selection,
			bounds: bounds,
			width: width,
			height: height,
			gap: metrics.gap,
			prefersTopPlacement: prefersTopPlacement
		)
		let toolbarAboveSelection = frame.midY >= selection.midY
		let primaryY =
			if styleKind == nil {
				frame.midY - metrics.buttonSize / 2
			} else if toolbarAboveSelection {
				frame.minY + metrics.verticalPadding
			} else {
				frame.maxY - metrics.verticalPadding - metrics.buttonSize
			}
		let itemFrames = primaryItemLayouts(
			for: items,
			frame: frame,
			primaryContentWidth: primaryContentWidth,
			primaryY: primaryY,
			metrics: metrics
		)
		let styleLayout: FrozenAnnotationStyleLayout?
		if let styleKind {
			styleLayout = annotationStyleLayout(
				for: styleKind,
				in: frame,
				contentWidth: styleContentWidth,
				metrics: metrics,
				toolbarAboveSelection: toolbarAboveSelection,
				annotationStyle: annotationStyle
			)
		} else {
			styleLayout = nil
		}

		return FrozenToolbarLayout(
			scale: metrics.scale,
			frame: frame,
			items: itemFrames,
			annotationStyle: styleLayout
		)
	}

	package static func hitState(at point: CGPoint, in layout: FrozenToolbarLayout?)
		-> FrozenToolbarHitState
	{
		guard let layout else {
			return FrozenToolbarHitState(
				pointerOverToolbar: false,
				toolbarAction: nil,
				annotationStyleAction: nil
			)
		}

		let hoveredAction = layout.items.first { item in
			item.enabled && item.frame.contains(point)
		}?.kind
		let hoveredStyleAction = annotationStyleAction(at: point, in: layout.annotationStyle)
		return FrozenToolbarHitState(
			pointerOverToolbar: layout.frame.contains(point),
			toolbarAction: hoveredAction,
			annotationStyleAction: hoveredStyleAction
		)
	}

	package static func localAnnotationStyleLayout(
		_ layout: FrozenAnnotationStyleLayout,
		relativeTo toolbarFrame: CGRect
	) -> FrozenAnnotationStyleLayout {
		FrozenAnnotationStyleLayout(
			kind: layout.kind,
			scale: layout.scale,
			frame: layout.frame.offsetBy(dx: -toolbarFrame.minX, dy: -toolbarFrame.minY),
			sizeControlFrame: layout.sizeControlFrame.offsetBy(
				dx: -toolbarFrame.minX,
				dy: -toolbarFrame.minY
			),
			decreaseFrame: layout.decreaseFrame.offsetBy(
				dx: -toolbarFrame.minX,
				dy: -toolbarFrame.minY
			),
			increaseFrame: layout.increaseFrame.offsetBy(
				dx: -toolbarFrame.minX,
				dy: -toolbarFrame.minY
			),
			displayFrame: layout.displayFrame.offsetBy(
				dx: -toolbarFrame.minX,
				dy: -toolbarFrame.minY
			),
			swatches: layout.swatches.map { swatch in
				FrozenAnnotationColorSwatchLayout(
					color: swatch.color,
					frame: swatch.frame.offsetBy(
						dx: -toolbarFrame.minX,
						dy: -toolbarFrame.minY
					),
					selected: swatch.selected
				)
			}
		)
	}

	private static func selectedAnnotationStyleKind(in items: [ToolbarItem])
		-> FrozenAnnotationStyleToolbarKind?
	{
		for item in items where item.enabled && item.selected {
			if let kind = FrozenAnnotationStyleToolbarKind(selectedTool: item.kind) {
				return kind
			}
		}
		return nil
	}

	private static func toolbarFrame(
		selection: CGRect,
		bounds: CGRect,
		width: CGFloat,
		height: CGFloat,
		gap: CGFloat,
		prefersTopPlacement: Bool
	) -> CGRect {
		let desiredY = selection.maxY + gap
		let placedAbove =
			prefersTopPlacement
			|| desiredY + height > bounds.maxY - CaptureChrome.toolbarScreenMargin
		let y =
			placedAbove
			? max(
				bounds.minY + CaptureChrome.toolbarScreenMargin,
				selection.minY - gap - height)
			: min(bounds.maxY - CaptureChrome.toolbarScreenMargin - height, desiredY)
		let minX = bounds.minX + CaptureChrome.toolbarScreenMargin
		let maxX = max(minX, bounds.maxX - CaptureChrome.toolbarScreenMargin - width)
		let x = (selection.midX - width / 2).clamped(to: minX...maxX)
		return CGRect(x: x, y: y, width: width, height: height)
	}

	private static func primaryItemLayouts(
		for items: [ToolbarItem],
		frame: CGRect,
		primaryContentWidth: CGFloat,
		primaryY: CGFloat,
		metrics: CaptureChrome.ToolbarMetrics
	) -> [FrozenToolbarItemLayout] {
		var itemFrames: [FrozenToolbarItemLayout] = []
		var cursorX = frame.midX - primaryContentWidth / 2
		for item in items {
			let itemFrame = CGRect(
				x: cursorX,
				y: primaryY,
				width: metrics.buttonSize,
				height: metrics.buttonSize
			)
			itemFrames.append(
				FrozenToolbarItemLayout(
					kind: item.kind,
					frame: itemFrame,
					enabled: item.enabled,
					selected: item.selected
				)
			)
			cursorX += metrics.buttonSize + metrics.itemSpacing
		}
		return itemFrames
	}

	private static func annotationStyleContentWidth(
		for kind: FrozenAnnotationStyleToolbarKind,
		metrics: CaptureChrome.ToolbarMetrics
	) -> CGFloat {
		let swatchCount = CGFloat(FrozenAnnotationColor.allCases.count)
		let swatchesWidth =
			swatchCount * metrics.annotationSwatchSize
			+ max(0, swatchCount - 1) * metrics.annotationSwatchGap
		return kind.sizeControlWidth(scale: metrics.scale)
			+ metrics.annotationStyleControlGap
			+ swatchesWidth
	}

	private static func annotationStyleLayout(
		for kind: FrozenAnnotationStyleToolbarKind,
		in frame: CGRect,
		contentWidth: CGFloat,
		metrics: CaptureChrome.ToolbarMetrics,
		toolbarAboveSelection: Bool,
		annotationStyle: FrozenAnnotationStyleState
	) -> FrozenAnnotationStyleLayout {
		let rowY =
			toolbarAboveSelection
			? frame.maxY - metrics.verticalPadding - metrics.annotationStyleRowHeight
			: frame.minY + metrics.verticalPadding
		let rowFrame = CGRect(
			x: frame.midX - contentWidth / 2,
			y: rowY,
			width: contentWidth,
			height: metrics.annotationStyleRowHeight
		)
		let sizeControlFrame = CGRect(
			x: rowFrame.minX,
			y: rowFrame.minY,
			width: kind.sizeControlWidth(scale: metrics.scale),
			height: rowFrame.height
		)
		let decreaseFrame = CGRect(
			x: sizeControlFrame.minX,
			y: sizeControlFrame.minY,
			width: metrics.annotationSizeButtonWidth,
			height: sizeControlFrame.height
		)
		let increaseFrame = CGRect(
			x: sizeControlFrame.maxX - metrics.annotationSizeButtonWidth,
			y: sizeControlFrame.minY,
			width: metrics.annotationSizeButtonWidth,
			height: sizeControlFrame.height
		)
		let displayFrame = CGRect(
			x: decreaseFrame.maxX,
			y: sizeControlFrame.minY,
			width: max(0, increaseFrame.minX - decreaseFrame.maxX),
			height: sizeControlFrame.height
		)
		var swatches: [FrozenAnnotationColorSwatchLayout] = []
		var swatchX = sizeControlFrame.maxX + metrics.annotationStyleControlGap
		for color in FrozenAnnotationColor.allCases {
			let swatchFrame = CGRect(
				x: swatchX,
				y: rowFrame.midY - metrics.annotationSwatchSize / 2,
				width: metrics.annotationSwatchSize,
				height: metrics.annotationSwatchSize
			)
			swatches.append(
				FrozenAnnotationColorSwatchLayout(
					color: color,
					frame: swatchFrame,
					selected: kind.selectedColor(in: annotationStyle) == color
				))
			swatchX += metrics.annotationSwatchSize + metrics.annotationSwatchGap
		}
		return FrozenAnnotationStyleLayout(
			kind: kind,
			scale: metrics.scale,
			frame: rowFrame,
			sizeControlFrame: sizeControlFrame,
			decreaseFrame: decreaseFrame,
			increaseFrame: increaseFrame,
			displayFrame: displayFrame,
			swatches: swatches
		)
	}

	private static func annotationStyleAction(
		at point: CGPoint,
		in layout: FrozenAnnotationStyleLayout?
	) -> FrozenAnnotationStyleAction? {
		guard let layout else {
			return nil
		}
		if layout.decreaseFrame.contains(point) {
			return .decreaseSize
		}
		if layout.increaseFrame.contains(point) {
			return .increaseSize
		}
		for swatch in layout.swatches where swatch.frame.contains(point) {
			return .color(swatch.color)
		}
		return nil
	}
}
