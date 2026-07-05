import AppKit
import CoreGraphics
import Foundation
import QuartzCore
import RsnapHostBridge

@MainActor
final class LiveHudColorRollCoordinator {
	private let hudHexLayer: CATextLayer
	private let hudHexRollLayer: CALayer
	private let hudSwatchLayer: CALayer
	private let backingScaleProvider: () -> CGFloat

	private static let resolveAnimationKey = "rsnap.hud.color.resolve"
	private static let resolveBackgroundAnimationKey = "rsnap.hud.color.resolve.background"
	private static let rollAnimationKey = "rsnap.hud.color.roll"
	private static let pendingRollAnimationKey = "rsnap.hud.color.pending.roll"
	private static let rollDuration: TimeInterval = 0.40
	private static let rollDigitStagger: TimeInterval = 0.024
	private static let hexWheel = Array("0123456789ABCDEF")
	private static let pendingHexRollBaseSeed: UInt64 = 0x5EED_71A5_C01D

	private struct PendingRollColumnState {
		let digits: [Character]
		let scrollsUp: Bool
		let contentLayer: CALayer
	}

	private var lastColorPending: Bool?
	private var colorRevealArmed = true
	private var hasResolvedColor = false
	private var lastResolvedHexText: String?
	private var lastResolvedSwatchColor: CGColor?
	private var activeRollTarget: String?
	private var activeRollSwatchColor: CGColor?
	private var rollAnimationEndUptime: TimeInterval?
	private var pendingRollActive = false
	private var pendingRollColumns: [PendingRollColumnState] = []

	init(
		hudHexLayer: CATextLayer,
		hudHexRollLayer: CALayer,
		hudSwatchLayer: CALayer,
		backingScaleProvider: @escaping () -> CGFloat
	) {
		self.hudHexLayer = hudHexLayer
		self.hudHexRollLayer = hudHexRollLayer
		self.hudSwatchLayer = hudSwatchLayer
		self.backingScaleProvider = backingScaleProvider
	}

	func render(
		colorDisplay: LiveColorDisplay,
		rgbSample: RGBSample?,
		palette: CaptureChromePalette,
		swatchFrame: CGRect,
		hexFrame: CGRect,
		font: NSFont
	) {
		hudSwatchLayer.frame = swatchFrame
		hudSwatchLayer.cornerRadius = 0
		let pendingSwatchColor = palette.labelText.withAlphaComponent(0.16)
		let swatchColor =
			rgbSample.map {
				NSColor(
					calibratedRed: CGFloat($0.r) / 255, green: CGFloat($0.g) / 255,
					blue: CGFloat($0.b) / 255, alpha: 1)
			} ?? pendingSwatchColor
		hudSwatchLayer.backgroundColor = swatchColor.cgColor
		hudSwatchLayer.borderColor = palette.swatchStroke.cgColor
		hudSwatchLayer.borderWidth = 1

		let hexTextColor =
			colorDisplay.isPending
			? palette.labelText.withAlphaComponent(0.46) : palette.labelText
		applyText(
			hudHexLayer,
			text: colorDisplay.hexText,
			font: font,
			color: hexTextColor,
			frame: hexFrame,
			alignment: .left
		)
		update(
			isPending: colorDisplay.isPending,
			pendingSwatchColor: pendingSwatchColor,
			resolvedSwatchColor: swatchColor,
			resolvedHexText: colorDisplay.hexText,
			hexFrame: hexFrame,
			font: font,
			textColor: palette.labelText
		)
	}

	private func update(
		isPending: Bool,
		pendingSwatchColor: NSColor,
		resolvedSwatchColor: NSColor,
		resolvedHexText: String,
		hexFrame: CGRect,
		font: NSFont,
		textColor: NSColor
	) {
		if isPending {
			hudSwatchLayer.removeAnimation(forKey: Self.resolveAnimationKey)
			hudSwatchLayer.removeAnimation(forKey: Self.resolveBackgroundAnimationKey)
			hudSwatchLayer.opacity = 1
			hudHexLayer.removeAnimation(forKey: Self.resolveAnimationKey)
			if hasResolvedColor {
				clearRollAnimation()
				if let lastResolvedSwatchColor {
					hudSwatchLayer.backgroundColor = lastResolvedSwatchColor
				}
				if let lastResolvedHexText {
					applyText(
						hudHexLayer,
						text: lastResolvedHexText,
						font: font,
						color: textColor,
						frame: hexFrame,
						alignment: .left
					)
				}
				hudHexLayer.isHidden = false
				lastColorPending = false
				colorRevealArmed = false
				return
			}
			beginOrUpdatePendingRollAnimation(
				frame: hexFrame,
				font: font,
				textColor: textColor
			)
			lastColorPending = true
			return
		}

		let wasPending = lastColorPending == true
		let shouldAnimateReveal = wasPending && colorRevealArmed && !hasResolvedColor
		let priorSwatchColor =
			wasPending ? hudSwatchLayer.presentation()?.backgroundColor : nil
		let priorSwatchOpacity =
			wasPending ? hudSwatchLayer.presentation()?.opacity : nil
		let priorHexOpacity = wasPending ? hudHexLayer.presentation()?.opacity : nil
		lastResolvedHexText = resolvedHexText
		lastResolvedSwatchColor = resolvedSwatchColor.cgColor
		hasResolvedColor = true
		lastColorPending = false
		colorRevealArmed = false

		guard shouldAnimateReveal else {
			updateRollVisibility(
				target: resolvedHexText,
				frame: hexFrame,
				font: font,
				textColor: textColor
			)
			return
		}

		addResolveAnimation(
			to: hudSwatchLayer,
			fromOpacity: priorSwatchOpacity.map(CGFloat.init) ?? 0.62
		)
		beginRollAnimation(
			target: resolvedHexText,
			frame: hexFrame,
			font: font,
			textColor: textColor,
			initialOpacity: priorHexOpacity.map(CGFloat.init) ?? 0.62,
			targetSwatchColor: resolvedSwatchColor
		)
		let colorAnimation = CABasicAnimation(keyPath: "backgroundColor")
		colorAnimation.fromValue = priorSwatchColor ?? pendingSwatchColor.cgColor
		colorAnimation.toValue = resolvedSwatchColor.cgColor
		colorAnimation.duration = 0.16
		colorAnimation.timingFunction = CAMediaTimingFunction(name: .easeOut)
		hudSwatchLayer.add(
			colorAnimation,
			forKey: Self.resolveBackgroundAnimationKey
		)
	}

	func reset() {
		lastColorPending = nil
		colorRevealArmed = true
		hasResolvedColor = false
		lastResolvedHexText = nil
		lastResolvedSwatchColor = nil
		activeRollTarget = nil
		activeRollSwatchColor = nil
		rollAnimationEndUptime = nil
		pendingRollActive = false
		pendingRollColumns.removeAll(keepingCapacity: true)
		hudSwatchLayer.removeAnimation(forKey: Self.resolveAnimationKey)
		hudSwatchLayer.removeAnimation(forKey: Self.resolveBackgroundAnimationKey)
		hudHexLayer.removeAnimation(forKey: Self.resolveAnimationKey)
		hudHexLayer.isHidden = false
		clearRollAnimation()
	}

	private func addResolveAnimation(to layer: CALayer, fromOpacity: CGFloat) {
		let animation = CABasicAnimation(keyPath: "opacity")
		animation.fromValue = fromOpacity
		animation.toValue = 1
		animation.duration = 0.16
		animation.timingFunction = CAMediaTimingFunction(name: .easeOut)
		layer.add(animation, forKey: Self.resolveAnimationKey)
	}

	private func updateRollVisibility(
		target: String,
		frame: CGRect,
		font _: NSFont,
		textColor _: NSColor
	) {
		guard let activeTarget = activeRollTarget else {
			clearRollAnimation()
			hudHexLayer.isHidden = false
			return
		}
		let now = ProcessInfo.processInfo.systemUptime
		if activeTarget != target {
			if let animationEnd = rollAnimationEndUptime {
				if now < animationEnd {
					if let activeRollSwatchColor {
						hudSwatchLayer.backgroundColor = activeRollSwatchColor
					}
					hudHexLayer.isHidden = true
					hudHexRollLayer.isHidden = false
					hudHexRollLayer.frame = frame
					return
				}
				finishRollAnimation()
			}
			clearRollAnimation()
			hudHexLayer.isHidden = false
			return
		}
		if let animationEnd = rollAnimationEndUptime,
			now >= animationEnd
		{
			finishRollAnimation()
		}

		hudHexLayer.isHidden = true
		hudHexRollLayer.isHidden = false
		hudHexRollLayer.frame = frame
	}

	private func finishRollAnimation() {
		rollAnimationEndUptime = nil
		activeRollSwatchColor = nil
		pendingRollActive = false
		pendingRollColumns.removeAll(keepingCapacity: true)
		removeRollLayerAnimations()
	}

	private func beginOrUpdatePendingRollAnimation(
		frame: CGRect,
		font: NSFont,
		textColor: NSColor
	) {
		hudHexLayer.isHidden = true
		hudHexRollLayer.isHidden = false
		hudHexRollLayer.frame = frame
		guard pendingRollActive == false else {
			return
		}

		clearRollAnimation()
		pendingRollActive = true
		pendingRollColumns.removeAll(keepingCapacity: true)
		hudHexLayer.isHidden = true
		hudHexRollLayer.isHidden = false
		hudHexRollLayer.frame = frame

		let lineHeight = ceil(LiveOverlayTypography.lineHeight)
		let characterFrames = hexCharacterFrames(
			for: "#FFFFFF",
			font: font,
			lineHeight: lineHeight
		)
		let hashLayer = makeRollTextLayer(
			text: "#",
			font: font,
			color: textColor.withAlphaComponent(0.72),
			frame: characterFrames.first ?? CGRect(x: 0, y: 0, width: 0, height: lineHeight)
		)
		hudHexRollLayer.addSublayer(hashLayer)

		for index in 0..<6 {
			let characterFrame =
				index + 1 < characterFrames.count
				? characterFrames[index + 1]
				: CGRect(x: 0, y: 0, width: 0, height: lineHeight)
			let columnLayer = CALayer()
			columnLayer.masksToBounds = true
			columnLayer.frame = characterFrame
			hudHexRollLayer.addSublayer(columnLayer)
			let columnState = addPendingRollColumn(
				to: columnLayer,
				index: index,
				font: font,
				textColor: textColor,
				lineHeight: lineHeight,
				digitWidth: characterFrame.width
			)
			pendingRollColumns.append(columnState)
		}
	}

	private static func pendingHexRollSeed(index: Int) -> UInt64 {
		let uptimeBucket = UInt64((ProcessInfo.processInfo.systemUptime * 1_000).rounded(.down))
		let mixedIndex = UInt64(index + 1) &* 0x9E37_79B9_7F4A_7C15
		return pendingHexRollBaseSeed ^ uptimeBucket ^ mixedIndex
	}

	private static func pendingHexRollSequence(index: Int) -> [Character] {
		var seed = pendingHexRollSeed(index: index)
		seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
		let visibleRows = 47 + Int((seed >> 57) & 0x1F) + index * 3
		var digits: [Character] = []
		digits.reserveCapacity(visibleRows + 1)
		var previous: Character?
		for offset in 0..<visibleRows {
			seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
			var wheelIndex = Int((seed >> 58) & 0xF)
			if let previousDigit = previous,
				Self.hexWheel[wheelIndex] == previousDigit
			{
				wheelIndex = (wheelIndex + offset + index + 1) % Self.hexWheel.count
			}
			let digit = Self.hexWheel[wheelIndex]
			digits.append(digit)
			previous = digit
		}
		if let first = digits.first {
			digits.append(first)
		}
		return digits
	}

	private static func pendingHexRollColumnDuration(index: Int) -> TimeInterval {
		let seed =
			pendingHexRollSeed(index: index)
			&* 2_862_933_555_777_941_757
			&+ 3_037_000_493
		return 1.58 + Double((seed >> 56) & 0x1F) * 0.031
	}

	private static func pendingHexRollColumnPhase(index: Int, duration: TimeInterval)
		-> TimeInterval
	{
		let seed =
			pendingHexRollSeed(index: index)
			&* 11_400_714_819_323_198_485
			&+ 12_829_314
		let ratio = Double((seed >> 40) & 0xFFFF) / 65_535.0
		return duration * ratio
	}

	private static func pendingHexRollColumnScrollsUp(index: Int) -> Bool {
		let uptimeBucket = UInt64((ProcessInfo.processInfo.systemUptime * 1_000).rounded(.down))
		let startsUp = ((pendingHexRollBaseSeed ^ uptimeBucket) & 1) == 0
		if index <= 1 {
			return index == 0 ? startsUp : !startsUp
		}
		let seed =
			pendingHexRollSeed(index: index)
			&* 3_202_034_522_624_059_733
			&+ 1_029
		return ((seed >> 63) & 1) == 0
	}

	private static func resolveHexRollColumnScrollsUp(
		index: Int,
		startDigit: Character,
		targetDigit: Character
	) -> Bool {
		let startValue = UInt64(startDigit.unicodeScalars.first?.value ?? 0)
		let targetValue = UInt64(targetDigit.unicodeScalars.first?.value ?? 0)
		let seed =
			pendingHexRollSeed(index: index)
			^ (startValue &* 1_099_511_628_211)
			^ (targetValue &* 2_862_933_555_777_941_757)
		let startsUp = ((seed >> 63) & 1) == 0
		if index <= 1 {
			return index == 0 ? startsUp : !startsUp
		}
		return ((seed >> 59) & 1) == 0
	}

	private static func resolveHexRollExtraLoops(index: Int, targetDigit: Character) -> Int {
		let targetValue = UInt64(targetDigit.unicodeScalars.first?.value ?? 0)
		let seed =
			pendingHexRollSeed(index: index)
			^ (targetValue &* 11_400_714_819_323_198_485)
		return 1 + Int((seed >> 60) & 1)
	}

	private static func resolveHexRollSequence(
		from startDigit: Character,
		to targetDigit: Character,
		index: Int,
		scrollsUp: Bool
	) -> [Character] {
		let wheelCount = max(hexWheel.count, 1)
		let startIndex = hexWheel.firstIndex(of: startDigit) ?? 0
		let targetIndex = hexWheel.firstIndex(of: targetDigit) ?? startIndex
		let directedDistance =
			scrollsUp
			? (targetIndex - startIndex + wheelCount) % wheelCount
			: (startIndex - targetIndex + wheelCount) % wheelCount
		let extraSteps =
			resolveHexRollExtraLoops(index: index, targetDigit: targetDigit)
			* wheelCount
		let totalSteps =
			directedDistance + extraSteps
		return (0...totalSteps).map { offset in
			let wheelIndex =
				scrollsUp
				? (startIndex + offset) % wheelCount
				: (startIndex - offset + (totalSteps + wheelCount) * wheelCount) % wheelCount
			return hexWheel[wheelIndex]
		}
	}

	private func beginRollAnimation(
		target: String,
		frame: CGRect,
		font: NSFont,
		textColor: NSColor,
		initialOpacity: CGFloat,
		targetSwatchColor: NSColor? = nil
	) {
		let lineHeight = ceil(LiveOverlayTypography.lineHeight)
		let startDigits = currentPendingDigits(lineHeight: lineHeight)
		let pendingDirections = pendingRollColumns.map(\.scrollsUp)
		clearRollAnimation()
		activeRollTarget = target
		activeRollSwatchColor = targetSwatchColor?.cgColor
		let now = ProcessInfo.processInfo.systemUptime
		let targetDigits = Array(target.dropFirst())
		var rollEndOffset: TimeInterval = 0
		hudHexLayer.isHidden = true
		hudHexRollLayer.isHidden = false
		hudHexRollLayer.frame = frame

		let characterFrames = hexCharacterFrames(
			for: target,
			font: font,
			lineHeight: lineHeight
		)
		let hashLayer = makeRollTextLayer(
			text: "#",
			font: font,
			color: textColor.withAlphaComponent(0.72),
			frame: characterFrames.first ?? CGRect(x: 0, y: 0, width: 0, height: lineHeight)
		)
		hudHexRollLayer.addSublayer(hashLayer)

		for (index, targetDigit) in targetDigits.enumerated() {
			let characterFrame =
				index + 1 < characterFrames.count
				? characterFrames[index + 1]
				: CGRect(x: 0, y: 0, width: 0, height: lineHeight)
			let columnLayer = CALayer()
			columnLayer.masksToBounds = true
			columnLayer.frame = characterFrame
			hudHexRollLayer.addSublayer(columnLayer)
			let startDigit =
				index < startDigits.count
				? startDigits[index]
				: nil
			let resolvedStartDigit = startDigit ?? Self.hexWheel.first ?? targetDigit
			let scrollsUp =
				index < pendingDirections.count
				? pendingDirections[index]
				: Self.resolveHexRollColumnScrollsUp(
					index: index,
					startDigit: resolvedStartDigit,
					targetDigit: targetDigit
				)
			let columnEndOffset = addRollDigit(
				to: columnLayer,
				startDigit: resolvedStartDigit,
				targetDigit: targetDigit,
				index: index,
				font: font,
				textColor: textColor,
				initialOpacity: initialOpacity,
				lineHeight: lineHeight,
				digitWidth: characterFrame.width,
				scrollsUp: scrollsUp
			)
			pendingRollColumns.append(columnEndOffset.state)
			rollEndOffset = max(rollEndOffset, columnEndOffset.endOffset)
		}
		rollAnimationEndUptime = now + rollEndOffset + 0.03
	}

	private func hexCharacterFrames(
		for text: String,
		font: NSFont,
		lineHeight: CGFloat
	) -> [CGRect] {
		let characters = Array(text)
		return characters.indices.map { index in
			let prefixStart = String(characters.prefix(index)).size(using: font).width
			let prefixEnd = String(characters.prefix(index + 1)).size(using: font).width
			return CGRect(
				x: prefixStart,
				y: 0,
				width: max(prefixEnd - prefixStart, 1),
				height: lineHeight
			)
		}
	}

	private func addPendingRollColumn(
		to columnLayer: CALayer,
		index: Int,
		font: NSFont,
		textColor: NSColor,
		lineHeight: CGFloat,
		digitWidth: CGFloat
	) -> PendingRollColumnState {
		var digits = Self.pendingHexRollSequence(index: index)
		let scrollsUp = Self.pendingHexRollColumnScrollsUp(index: index)
		if scrollsUp == false {
			digits.reverse()
		}
		let contentText = digits.map(String.init).joined(separator: "\n")
		let contentLayer = CALayer()
		contentLayer.frame = CGRect(
			x: 0,
			y: 0,
			width: digitWidth,
			height: lineHeight * CGFloat(digits.count)
		)
		columnLayer.addSublayer(contentLayer)

		let digitLayer = makeRollMultilineTextLayer(
			text: contentText,
			font: font,
			color: textColor.withAlphaComponent(0.72),
			lineHeight: lineHeight,
			frame: contentLayer.bounds
		)
		contentLayer.addSublayer(digitLayer)

		let animation = CABasicAnimation(keyPath: "transform.translation.y")
		let travel = lineHeight * CGFloat(max(digits.count - 1, 1))
		animation.fromValue = scrollsUp ? 0 : -travel
		animation.toValue = scrollsUp ? -travel : 0
		let duration = Self.pendingHexRollColumnDuration(index: index)
		animation.duration = duration
		animation.beginTime =
			CACurrentMediaTime()
			- Self.pendingHexRollColumnPhase(index: index, duration: duration)
		animation.repeatCount = .infinity
		animation.timingFunction = CAMediaTimingFunction(name: .linear)
		animation.isRemovedOnCompletion = false
		contentLayer.add(animation, forKey: Self.pendingRollAnimationKey)
		return PendingRollColumnState(
			digits: digits,
			scrollsUp: scrollsUp,
			contentLayer: contentLayer
		)
	}

	private func currentPendingDigits(lineHeight: CGFloat) -> [Character?] {
		pendingRollColumns.map { column in
			guard column.digits.isEmpty == false else {
				return nil
			}
			let presentationLayer = column.contentLayer.presentation() ?? column.contentLayer
			let translationY = presentationLayer.transform.m42
			let rawIndex = Int((-translationY / lineHeight).rounded())
			let visibleIndex = min(max(rawIndex, 0), column.digits.count - 1)
			return column.digits[visibleIndex]
		}
	}

	private func addRollDigit(
		to columnLayer: CALayer,
		startDigit: Character,
		targetDigit: Character,
		index: Int,
		font: NSFont,
		textColor: NSColor,
		initialOpacity: CGFloat,
		lineHeight: CGFloat,
		digitWidth: CGFloat,
		scrollsUp: Bool
	) -> (state: PendingRollColumnState, endOffset: TimeInterval) {
		let rollDigits = Self.resolveHexRollSequence(
			from: startDigit,
			to: targetDigit,
			index: index,
			scrollsUp: scrollsUp
		)
		let terminalPaddingRows = 2
		let contentDigits: [Character]
		let startRowIndex: Int
		let targetRowIndex: Int
		if scrollsUp {
			contentDigits =
				rollDigits
				+ Array(repeating: targetDigit, count: terminalPaddingRows)
			startRowIndex = 0
			targetRowIndex = max(rollDigits.count - 1, 0)
		} else {
			contentDigits =
				Array(repeating: targetDigit, count: terminalPaddingRows)
				+ Array(rollDigits.reversed())
			startRowIndex = max(contentDigits.count - 1, 0)
			targetRowIndex = terminalPaddingRows
		}
		let contentLayer = CALayer()
		contentLayer.opacity = Float(max(initialOpacity, 0.72))
		contentLayer.frame = CGRect(
			x: 0,
			y: 0,
			width: digitWidth,
			height: lineHeight * CGFloat(contentDigits.count)
		)
		columnLayer.addSublayer(contentLayer)

		addRollDigitStack(
			to: contentLayer,
			digits: contentDigits,
			font: font,
			color: textColor,
			lineHeight: lineHeight,
			digitWidth: digitWidth
		)

		let fromY = -lineHeight * CGFloat(startRowIndex)
		let toY = -lineHeight * CGFloat(targetRowIndex)
		contentLayer.transform = CATransform3DMakeTranslation(0, toY, 0)

		let stagger = Double(index) * Self.rollDigitStagger
		let duration =
			Self.rollDuration
			+ Double(Self.resolveHexRollExtraLoops(index: index, targetDigit: targetDigit)) * 0.035
		let animation = CABasicAnimation(keyPath: "transform.translation.y")
		animation.fromValue = fromY
		animation.toValue = toY
		animation.beginTime = CACurrentMediaTime() + stagger
		animation.duration = duration
		animation.timingFunction = CAMediaTimingFunction(name: .easeOut)
		animation.fillMode = .both
		animation.isRemovedOnCompletion = false
		contentLayer.add(animation, forKey: Self.rollAnimationKey)
		let columnState = PendingRollColumnState(
			digits: contentDigits,
			scrollsUp: scrollsUp,
			contentLayer: contentLayer
		)
		return (columnState, stagger + duration)
	}

	private func addRollDigitStack(
		to contentLayer: CALayer,
		digits: [Character],
		font: NSFont,
		color: NSColor,
		lineHeight: CGFloat,
		digitWidth: CGFloat
	) {
		for (row, digit) in digits.enumerated() {
			let digitLayer = makeRollTextLayer(
				text: String(digit),
				font: font,
				color: color,
				frame: CGRect(
					x: 0,
					y: CGFloat(row) * lineHeight,
					width: digitWidth,
					height: lineHeight
				)
			)
			contentLayer.addSublayer(digitLayer)
		}
	}

	private func removeRollLayerAnimations() {
		hudHexRollLayer.removeAllAnimations()
		for sublayer in hudHexRollLayer.sublayers ?? [] {
			removeAnimationsRecursively(from: sublayer)
		}
	}

	private func removeAnimationsRecursively(from layer: CALayer) {
		layer.removeAllAnimations()
		for sublayer in layer.sublayers ?? [] {
			removeAnimationsRecursively(from: sublayer)
		}
	}

	private func makeRollTextLayer(
		text: String,
		font: NSFont,
		color: NSColor,
		frame: CGRect
	) -> CATextLayer {
		let layer = CATextLayer()
		layer.contentsScale = backingScaleProvider()
		layer.string = text
		layer.font = font
		layer.fontSize = font.pointSize
		layer.foregroundColor = color.cgColor
		layer.alignmentMode = .left
		layer.frame = frame
		layer.isWrapped = false
		return layer
	}

	private func makeRollMultilineTextLayer(
		text: String,
		font: NSFont,
		color: NSColor,
		lineHeight: CGFloat,
		frame: CGRect
	) -> CATextLayer {
		let paragraphStyle = NSMutableParagraphStyle()
		paragraphStyle.alignment = .left
		paragraphStyle.lineBreakMode = .byClipping
		paragraphStyle.minimumLineHeight = lineHeight
		paragraphStyle.maximumLineHeight = lineHeight
		let attributedString = NSAttributedString(
			string: text,
			attributes: [
				.font: font,
				.foregroundColor: color,
				.paragraphStyle: paragraphStyle,
			]
		)
		let layer = CATextLayer()
		layer.contentsScale = backingScaleProvider()
		layer.string = attributedString
		layer.alignmentMode = .left
		layer.frame = frame
		layer.isWrapped = true
		layer.truncationMode = .none
		return layer
	}

	private func clearRollAnimation() {
		activeRollTarget = nil
		activeRollSwatchColor = nil
		rollAnimationEndUptime = nil
		pendingRollActive = false
		pendingRollColumns.removeAll(keepingCapacity: true)
		removeRollLayerAnimations()
		for sublayer in hudHexRollLayer.sublayers ?? [] {
			sublayer.removeFromSuperlayer()
		}
		hudHexRollLayer.isHidden = true
	}

	private func applyText(
		_ layer: CATextLayer,
		text: String,
		font: NSFont,
		color: NSColor,
		frame: CGRect,
		alignment: CATextLayerAlignmentMode
	) {
		layer.contentsScale = backingScaleProvider()
		layer.string = text
		layer.font = font
		layer.fontSize = font.pointSize
		layer.foregroundColor = color.cgColor
		layer.alignmentMode = alignment
		layer.frame = frame
		layer.isWrapped = false
	}
}
