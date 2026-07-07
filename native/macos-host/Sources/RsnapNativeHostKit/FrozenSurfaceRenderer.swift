import AppKit
import CoreGraphics

@MainActor
struct FrozenSurfaceMaterialState {
	let toolbarLiquidGlassVisible: Bool
	let toolbarLiquidGlassContentDrawn: Bool
	let allowsClassicToolbarGlass: Bool
}

@MainActor
struct FrozenSurfaceRenderer {
	static func render(
		selection: CGRect,
		bounds: CGRect,
		backingScaleFactor: CGFloat,
		theme: CaptureChromeTheme,
		settings: NativeHostSettings,
		chrome: CaptureChromeState,
		toolbarLayout: FrozenToolbarLayout?,
		toolbarHoverState: ToolbarHoverState,
		materialState: FrozenSurfaceMaterialState,
		frozenDisplayFrame: CGRect?,
		frozenDisplayImage: CGImage?,
		windowFrame: CGRect?,
		selectionSizeText: String,
		glassPatch: (GlassSurfaceKind, CGRect) -> CGImage?,
		in context: CGContext
	) {
		drawFrozenDisplaySurface(
			frame: frozenDisplayFrame,
			image: frozenDisplayImage,
			bounds: bounds,
			in: context
		)
		let toolbarScrimExclusionPath = frozenToolbarScrimExclusionPath(
			for: selection,
			bounds: bounds,
			settings: settings,
			chrome: chrome,
			toolbarLayout: toolbarLayout,
			materialState: materialState
		)
		SelectionChromeRenderer.drawSelectionScrim(
			for: selection,
			bounds: bounds,
			in: context,
			alpha: CaptureChrome.frozenScrimAlpha,
			excluding: toolbarScrimExclusionPath
		)
		SelectionChromeRenderer.drawDashedSelectionBorder(
			around: selection,
			in: context,
			lineWidth: CaptureChrome.frozenDashedBorderWidth,
			pixelsPerPoint: backingScaleFactor
		)
		if chrome.frozenSelectionTransformAllowed {
			SelectionChromeRenderer.drawFrozenResizeHandles(
				for: selection,
				orientation: settings.frozenResizeHandleOrientation,
				in: context
			)
		}
		drawFrozenOverlays(
			for: selection,
			chrome: chrome,
			windowFrame: windowFrame,
			bounds: bounds,
			in: context
		)
		drawScrollCaptureMinimap(
			for: selection,
			bounds: bounds,
			theme: theme,
			settings: settings,
			chrome: chrome,
			in: context
		)
		SelectionChromeRenderer.drawSelectionSizeBadge(
			for: selection,
			text: selectionSizeText,
			bounds: bounds,
			avoiding: toolbarLayout?.frame,
			in: context
		)
		drawFrozenToolbar(
			layout: toolbarLayout,
			theme: theme,
			settings: settings,
			chrome: chrome,
			toolbarHoverState: toolbarHoverState,
			materialState: materialState,
			glassPatch: glassPatch,
			in: context
		)
	}

	static func pixelAlignedSelectionRect(_ rect: CGRect, backingScaleFactor: CGFloat) -> CGRect {
		let scale = max(backingScaleFactor, 1)
		let minX = floor(rect.minX * scale) / scale
		let minY = floor(rect.minY * scale) / scale
		let maxX = ceil(rect.maxX * scale) / scale
		let maxY = ceil(rect.maxY * scale) / scale
		return CGRect(
			x: minX,
			y: minY,
			width: max(0, maxX - minX),
			height: max(0, maxY - minY)
		)
	}

	private static func drawFrozenDisplaySurface(
		frame: CGRect?,
		image: CGImage?,
		bounds: CGRect,
		in context: CGContext
	) {
		guard let frame, let image else {
			return
		}

		context.saveGState()
		context.interpolationQuality = .high
		context.clip(to: bounds)
		context.draw(image, in: frame)
		context.restoreGState()
	}

	private static func drawScrollCaptureMinimap(
		for selection: CGRect,
		bounds: CGRect,
		theme: CaptureChromeTheme,
		settings: NativeHostSettings,
		chrome: CaptureChromeState,
		in context: CGContext
	) {
		guard let preview = chrome.scrollMinimapPreview else {
			return
		}
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		ScrollMinimapRenderer.render(
			preview: preview,
			selection: selection,
			bounds: bounds,
			palette: palette,
			in: context
		)
	}

	private static func drawFrozenOverlays(
		for selection: CGRect,
		chrome: CaptureChromeState,
		windowFrame: CGRect?,
		bounds: CGRect,
		in context: CGContext
	) {
		guard let windowFrame else {
			return
		}
		FrozenOverlayRenderer.render(
			selection: selection,
			chrome: chrome,
			windowFrame: windowFrame,
			bounds: bounds,
			in: context
		)
	}

	private static func frozenToolbarScrimExclusionPath(
		for selection: CGRect,
		bounds: CGRect,
		settings: NativeHostSettings,
		chrome: CaptureChromeState,
		toolbarLayout: FrozenToolbarLayout?,
		materialState: FrozenSurfaceMaterialState
	) -> CGPath? {
		guard settings.usesLiquidHudGlass,
			let toolbarFrame = toolbarLayout?.frame
		else {
			return nil
		}
		guard
			chrome.scrollMinimapPreview != nil
				|| (materialState.toolbarLiquidGlassVisible
					&& materialState.toolbarLiquidGlassContentDrawn)
		else {
			return nil
		}
		let visibleSelection = selection.intersection(bounds)
		if visibleSelection.isNull == false, toolbarFrame.intersects(visibleSelection) {
			return nil
		}
		return CGPath(
			roundedRect: toolbarFrame,
			cornerWidth: CaptureChrome.hudCornerRadius,
			cornerHeight: CaptureChrome.hudCornerRadius,
			transform: nil
		)
	}

	private static func drawFrozenToolbar(
		layout: FrozenToolbarLayout?,
		theme: CaptureChromeTheme,
		settings: NativeHostSettings,
		chrome: CaptureChromeState,
		toolbarHoverState: ToolbarHoverState,
		materialState: FrozenSurfaceMaterialState,
		glassPatch: (GlassSurfaceKind, CGRect) -> CGImage?,
		in context: CGContext
	) {
		guard
			!settings.usesLiquidHudGlass || !materialState.toolbarLiquidGlassVisible
				|| !materialState.toolbarLiquidGlassContentDrawn
		else {
			return
		}
		guard let layout else {
			return
		}
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		drawPill(
			in: layout.frame,
			context: context,
			theme: theme,
			settings: settings,
			strongShadow: false,
			surfaceKind: .toolbar,
			allowsClassicGlass: materialState.allowsClassicToolbarGlass,
			glassPatch: glassPatch
		)
		FrozenToolbarDrawing.drawToolbarContent(
			items: layout.items,
			hoveredToolbarAction: toolbarHoverState.toolbarAction,
			toolbarScale: layout.scale,
			annotationStyleState: chrome.annotationStyle,
			annotationStyleLayout: layout.annotationStyle,
			hoveredAnnotationStyleAction: toolbarHoverState.annotationStyleAction,
			palette: palette,
			in: context
		)
	}

	private static func drawPill(
		in frame: CGRect,
		context: CGContext,
		theme: CaptureChromeTheme,
		settings: NativeHostSettings,
		strongShadow: Bool,
		surfaceKind: GlassSurfaceKind,
		allowsLiquidGlassClearFill: Bool = true,
		allowsClassicGlass: Bool = true,
		glassPatch: (GlassSurfaceKind, CGRect) -> CGImage?
	) {
		let palette = CaptureChrome.palette(for: theme, settings: settings)
		let pillPath = NSBezierPath(
			roundedRect: frame,
			xRadius: CaptureChrome.hudCornerRadius,
			yRadius: CaptureChrome.hudCornerRadius
		)
		let glassImage =
			settings.usesClassicHudGlass && allowsClassicGlass
			? glassPatch(surfaceKind, frame) : nil
		let hasGlass = glassImage != nil
		context.saveGState()
		if strongShadow {
			context.setShadow(offset: .zero, blur: 10, color: palette.shadow.cgColor)
		}
		if hasGlass,
			let clipPath = pillPath.copy() as? NSBezierPath,
			let glassImage
		{
			clipPath.addClip()
			context.saveGState()
			context.setAlpha(CGFloat(CaptureChrome.glassOpacity(settings: settings)))
			context.draw(glassImage, in: frame)
			context.restoreGState()
		}
		let usesLiquidGlass = allowsLiquidGlassClearFill && settings.usesLiquidHudGlass
		let fillColor =
			usesLiquidGlass
			? NSColor.clear
			: CaptureChrome.effectiveBodyFill(
				palette: palette,
				settings: settings,
				hasGlass: hasGlass
			)
		context.setFillColor(fillColor.cgColor)
		pillPath.fill()
		context.restoreGState()

		context.setStrokeColor(palette.outerStroke.cgColor)
		context.setLineWidth(1)
		pillPath.stroke()
	}
}
