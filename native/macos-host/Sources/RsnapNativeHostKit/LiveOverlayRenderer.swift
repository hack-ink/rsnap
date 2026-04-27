import AppKit
import CoreGraphics
import Foundation
import QuartzCore
import RsnapHostBridge

enum LiveGlassSurfaceKind: Hashable {
	case hud
	case loupe
}

struct LivePreviewSnapshot {
	let bounds: CGRect
	let theme: CaptureChromeTheme
	let settings: NativeHostSettings
	let frozenPending: Bool
	let frozenDisplayFrame: CGRect?
	let frozenDisplayImage: CGImage?
	let pointerLocal: CGPoint?
	let dragSelectionLocal: CGRect?
	let hoverSelectionLocal: CGRect?
	let selectionSizeText: String?
	let hudFrame: CGRect?
	let loupeFrame: CGRect?
	let positionDisplay: LivePositionDisplay
	let colorDisplay: LiveColorDisplay
	let rgbSample: RGBSample?
	let keycapVisible: Bool
	let loupePatch: CGImage?
	let glassPatches: [LiveGlassSurfaceKind: CGImage]
}

@MainActor
private enum LiveOverlayTypography {
	static let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .medium)
	static let lineHeight = ceil("x=0".size(using: font).height)
	static let commaWidth = ",".size(using: font).width
	static let keycapTextSize = "Tab".size(using: font)
	static let keycapFrameSize = CGSize(
		width: keycapTextSize.width + 12, height: keycapTextSize.height + 4)
}

final class WindowSnapshotFeed {
	private static let ownPID = ProcessInfo.processInfo.processIdentifier
	private static let maxWindowLayerForTargeting = 3
	private let queue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.window-snapshot-feed", qos: .userInitiated)
	private let stateLock = NSLock()
	private var timer: DispatchSourceTimer?
	private var desktopFrame: CGRect = .null
	private var latestSnapshots: [WindowSnapshot] = []

	func start(desktopFrame: CGRect, initialSnapshots: [WindowSnapshot] = []) {
		stop()
		stateLock.lock()
		self.desktopFrame = desktopFrame
		latestSnapshots = initialSnapshots
		stateLock.unlock()
		let timer = DispatchSource.makeTimerSource(queue: queue)
		timer.schedule(
			deadline: .now(), repeating: LiveSamplingBudget.hoverWindowCacheRefreshInterval)
		timer.setEventHandler { [weak self] in
			self?.refresh()
		}
		self.timer = timer
		timer.resume()
	}

	func stop() {
		timer?.cancel()
		timer = nil
		stateLock.lock()
		latestSnapshots.removeAll()
		stateLock.unlock()
	}

	func window(at point: CGPoint) -> WindowSnapshot? {
		stateLock.lock()
		let snapshots = latestSnapshots
		stateLock.unlock()
		return snapshots.first(where: { $0.frame.contains(point) })
	}

	static func snapshots(desktopFrame: CGRect) -> [WindowSnapshot] {
		let candidateWindows =
			(CGWindowListCopyWindowInfo(
				[.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
				as? [[String: Any]])
			?? []
		var snapshots: [WindowSnapshot] = []
		for info in candidateWindows {
			let isOnScreen = (info[kCGWindowIsOnscreen as String] as? NSNumber)?.boolValue ?? false
			let ownerPID = (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value ?? -1
			if !isOnScreen || ownerPID == ownPID {
				continue
			}
			let alpha = (info[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 1
			if alpha < 0.05 {
				continue
			}
			let layer = (info[kCGWindowLayer as String] as? NSNumber)?.intValue ?? 0
			if layer < 0 || layer > maxWindowLayerForTargeting {
				continue
			}
			guard let boundsDictionary = info[kCGWindowBounds as String] as? NSDictionary else {
				continue
			}
			var quartzBounds = CGRect.null
			guard CGRectMakeWithDictionaryRepresentation(boundsDictionary, &quartzBounds) else {
				continue
			}
			let appKitBounds = CGRect(
				x: quartzBounds.minX,
				y: desktopFrame.maxY - quartzBounds.maxY,
				width: quartzBounds.width,
				height: quartzBounds.height
			)
			if appKitBounds.width < 40 || appKitBounds.height < 40 {
				continue
			}
			let windowID = (info[kCGWindowNumber as String] as? NSNumber)?.uint32Value
			snapshots.append(WindowSnapshot(windowID: windowID, frame: appKitBounds))
		}
		return snapshots
	}

	static func window(at point: CGPoint, in snapshots: [WindowSnapshot]) -> WindowSnapshot? {
		snapshots.first(where: { $0.frame.contains(point) })
	}

	private func refresh() {
		stateLock.lock()
		let desktopFrame = self.desktopFrame
		stateLock.unlock()
		let snapshots = Self.snapshots(desktopFrame: desktopFrame)
		stateLock.lock()
		latestSnapshots = snapshots
		stateLock.unlock()
	}
}

final class ChromeSampleFeed: @unchecked Sendable {
	typealias BackgroundSampler = @Sendable (CGPoint, LiveColorSampleSource) -> RGBSample?

	private let broker: LiveFrameStreamBroker
	private let backgroundSampler: BackgroundSampler
	private let queue = DispatchQueue(
		label: "ink.hack.rsnap.native-host.chrome-sample-feed", qos: .userInteractive)
	private let stateLock = NSLock()
	private var timer: DispatchSourceTimer?
	private var desiredPoint: CGPoint?
	private var desiredSidePixels: Int = 1
	private var desiredIncludesLoupePatch = false
	private var desiredSource: LiveColorSampleSource?
	private var latestSample: LiveChromeSample?
	private var lastPointChangeUptime = ProcessInfo.processInfo.systemUptime
	private var lastColorSampleUptime: TimeInterval = 0

	init(
		broker: LiveFrameStreamBroker,
		backgroundSampler: @escaping BackgroundSampler
	) {
		self.broker = broker
		self.backgroundSampler = backgroundSampler
	}

	func start() {
		stop()
		let timer = DispatchSource.makeTimerSource(queue: queue)
		let intervalNanoseconds = max(
			1,
			Int((NativeHostDisplayRefresh.frameInterval * 1_000_000_000.0).rounded())
		)
		timer.schedule(
			deadline: .now(),
			repeating: .nanoseconds(intervalNanoseconds),
			leeway: .nanoseconds(0)
		)
		timer.setEventHandler { [weak self] in
			self?.refresh()
		}
		self.timer = timer
		timer.resume()
	}

	func stop() {
		timer?.cancel()
		timer = nil
		stateLock.lock()
		desiredPoint = nil
		desiredIncludesLoupePatch = false
		desiredSource = nil
		latestSample = nil
		stateLock.unlock()
	}

	func updateDemand(
		point: CGPoint?,
		sidePixels: Int,
		includeLoupePatch: Bool,
		source: LiveColorSampleSource?
	) {
		stateLock.lock()
		let nextSidePixels = max(1, sidePixels)
		let sidePixelsChanged = nextSidePixels != desiredSidePixels
		let patchDemandChanged = includeLoupePatch != desiredIncludesLoupePatch
		let sourceChanged = desiredSource != source
		let activating = desiredPoint == nil && point != nil
		let pointChanged =
			desiredPoint.map { current in
				guard let point else {
					return true
				}
				return abs(current.x - point.x) > 0.5 || abs(current.y - point.y) > 0.5
			} ?? (point != nil)
		if sidePixelsChanged || patchDemandChanged {
			latestSample = latestSample.map {
				LiveChromeSample(rgbSample: $0.rgbSample, loupePatch: nil)
			}
		}
		if pointChanged || sourceChanged {
			lastPointChangeUptime = ProcessInfo.processInfo.systemUptime
		}
		if activating || sourceChanged || sidePixelsChanged || patchDemandChanged {
			lastColorSampleUptime = 0
		}
		desiredPoint = point
		desiredSidePixels = nextSidePixels
		desiredIncludesLoupePatch = includeLoupePatch
		desiredSource = source
		stateLock.unlock()
		if activating || sidePixelsChanged || patchDemandChanged || sourceChanged {
			queue.async { [weak self] in
				self?.refresh()
			}
		}
	}

	func prime(
		point: CGPoint?,
		sidePixels: Int,
		includeLoupePatch: Bool,
		source: LiveColorSampleSource?
	) {
		updateDemand(
			point: point,
			sidePixels: sidePixels,
			includeLoupePatch: includeLoupePatch,
			source: source
		)
		guard let point else {
			return
		}
		let streamSample =
			includeLoupePatch ? broker.sample(at: point, sidePixels: max(1, sidePixels)) : nil
		let rgbSample =
			source.flatMap { backgroundSampler(point, $0) }
			?? streamSample?.rgbSample
			?? broker.rgbSample(at: point)
		let sample = Self.sampleWithUpdatedPatch(
			rgbSample: rgbSample,
			patchSample: streamSample
		)
		stateLock.lock()
		if let sample {
			latestSample = sample
			lastColorSampleUptime = ProcessInfo.processInfo.systemUptime
		}
		stateLock.unlock()
	}

	func snapshot() -> LiveChromeSample? {
		stateLock.lock()
		let latestSample = self.latestSample
		stateLock.unlock()
		return latestSample
	}

	private func refresh() {
		let now = ProcessInfo.processInfo.systemUptime
		stateLock.lock()
		let point = desiredPoint
		let sidePixels = desiredSidePixels
		let includeLoupePatch = desiredIncludesLoupePatch
		let source = desiredSource
		let previousSample = latestSample
		let pointIdleDuration = now - lastPointChangeUptime
		let colorSampleInterval =
			pointIdleDuration >= LiveSamplingBudget.stationaryColorSampleIdleDelay
			? LiveSamplingBudget.stationaryColorSampleInterval
			: LiveSamplingBudget.movingColorSampleInterval
		let colorSampleDue = now - lastColorSampleUptime >= colorSampleInterval
		stateLock.unlock()
		guard let point else {
			stateLock.lock()
			latestSample = nil
			stateLock.unlock()
			return
		}
		let streamSample =
			includeLoupePatch ? broker.sample(at: point, sidePixels: sidePixels) : nil
		var sample =
			Self.sampleWithUpdatedPatch(
				rgbSample: previousSample?.rgbSample,
				patchSample: streamSample ?? previousSample
			) ?? streamSample
		if colorSampleDue, let source {
			stateLock.lock()
			lastColorSampleUptime = now
			stateLock.unlock()
			if let rgbSample = currentRgbSample(
				point: point,
				source: source,
				streamSample: streamSample,
				prefersCachedStream: pointIdleDuration
					< LiveSamplingBudget.stationaryColorSampleIdleDelay
			) {
				sample = Self.sampleWithUpdatedPatch(
					rgbSample: rgbSample,
					patchSample: streamSample ?? previousSample
				)
			}
		}
		stateLock.lock()
		latestSample = sample
		stateLock.unlock()
	}

	private static func sampleWithUpdatedPatch(
		rgbSample: RGBSample?,
		patchSample: LiveChromeSample?
	) -> LiveChromeSample? {
		guard rgbSample != nil || patchSample != nil else {
			return nil
		}
		return LiveChromeSample(
			rgbSample: rgbSample,
			loupePatch: patchSample?.loupePatch
		)
	}

	private func currentRgbSample(
		point: CGPoint,
		source: LiveColorSampleSource,
		streamSample: LiveChromeSample?,
		prefersCachedStream: Bool
	) -> RGBSample? {
		if prefersCachedStream,
			let rgbSample = streamSample?.rgbSample ?? broker.rgbSample(at: point)
		{
			return rgbSample
		}
		if let rgbSample = backgroundSampler(point, source) {
			return rgbSample
		}
		if !prefersCachedStream,
			let rgbSample = streamSample?.rgbSample ?? broker.rgbSample(at: point)
		{
			return rgbSample
		}
		return nil
	}
}

final class LiveFrameClockDriver: @unchecked Sendable {
	var onTick: (() -> Void)?
	private let stateLock = NSLock()
	private let tickGapMetric = NativeHostTelemetry.distribution(
		"live_chrome.frame_tick_gap",
		category: "LiveChromeTelemetry"
	)
	private var timer: DispatchSourceTimer?
	private var currentTargetFramesPerSecond: Int?
	private var lastTickUptime: TimeInterval?

	func start(targetFramesPerSecond: Int) {
		let sanitizedTarget = max(1, targetFramesPerSecond)
		stateLock.lock()
		let alreadyRunning = timer != nil && currentTargetFramesPerSecond == sanitizedTarget
		stateLock.unlock()
		guard !alreadyRunning else {
			return
		}

		stop()
		let timer = DispatchSource.makeTimerSource(queue: .main)
		let intervalNanoseconds = max(
			1,
			Int((1_000_000_000.0 / Double(sanitizedTarget)).rounded())
		)
		timer.schedule(
			deadline: .now(),
			repeating: .nanoseconds(intervalNanoseconds),
			leeway: .nanoseconds(0)
		)
		timer.setEventHandler { [weak self] in
			self?.tick()
		}
		stateLock.lock()
		self.timer = timer
		currentTargetFramesPerSecond = sanitizedTarget
		lastTickUptime = nil
		stateLock.unlock()
		timer.resume()
	}

	private func tick() {
		let now = ProcessInfo.processInfo.systemUptime
		if let lastTickUptime {
			let gapMilliseconds = (now - lastTickUptime) * 1_000
			if gapMilliseconds >= 0, gapMilliseconds < 250 {
				tickGapMetric.record(gapMilliseconds)
			}
		}
		lastTickUptime = now
		onTick?()
	}

	func stop() {
		stateLock.lock()
		guard let timer else {
			currentTargetFramesPerSecond = nil
			lastTickUptime = nil
			stateLock.unlock()
			return
		}
		self.timer = nil
		currentTargetFramesPerSecond = nil
		lastTickUptime = nil
		stateLock.unlock()
		timer.cancel()
	}

	deinit {
		stop()
	}
}

private final class SelectionFlowBandLayer: CALayer {
	private struct SamplePoint {
		let point: CGPoint
		let progress: CGFloat
	}

	private static let cornerRadius: CGFloat = 9.0
	private static let minSegments = 160
	private static let maxSegments = 1_536
	private static let sampleStep: CGFloat = 3.2
	private static let speed: CGFloat = 0.24
	private static let bandWidth: CGFloat = 0.06
	private static let flowBoost: CGFloat = 2.8
	private static let phaseMultiplier: CGFloat = 1.28
	private static let phaseOffset: CGFloat = 0.72
	private static let darkPalette: [(CGFloat, CGFloat, CGFloat)] = [
		(196.0 / 255.0, 226.0 / 255.0, 1.0),
		(228.0 / 255.0, 198.0 / 255.0, 1.0),
		(176.0 / 255.0, 244.0 / 255.0, 224.0 / 255.0),
	]
	private static let lightPalette = darkPalette
	private static let passes: [(width: CGFloat, alphaScale: CGFloat)] = [
		(2.4, 0.52)
	]

	private var focusRect: CGRect = .null
	private var theme: CaptureChromeTheme = .dark
	private var phase: CGFloat = 0

	override init() {
		super.init()
		contentsScale = NSScreen.main?.backingScaleFactor ?? 2
		isOpaque = false
		needsDisplayOnBoundsChange = true
	}

	override init(layer: Any) {
		super.init(layer: layer)
		if let layer = layer as? SelectionFlowBandLayer {
			focusRect = layer.focusRect
			theme = layer.theme
			phase = layer.phase
		}
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	func hide() {
		guard !isHidden || !focusRect.isNull else {
			return
		}
		isHidden = true
		focusRect = .null
	}

	func update(
		frame: CGRect,
		focusRect: CGRect,
		theme: CaptureChromeTheme,
		timestamp: CFTimeInterval,
		contentsScale: CGFloat
	) {
		self.frame = frame
		self.contentsScale = contentsScale
		self.focusRect = focusRect
		self.theme = theme
		phase = CGFloat(timestamp) * Self.speed * Self.phaseMultiplier + Self.phaseOffset
		isHidden = false
		setNeedsDisplay()
	}

	override func draw(in ctx: CGContext) {
		guard !focusRect.isNull else {
			return
		}
		let cornerRadius = selectionFlowCornerRadius(for: focusRect)
		let perimeter = selectionFlowPerimeter(for: focusRect, cornerRadius: cornerRadius)
		let sampleCount = selectionFlowSampleCount(for: perimeter)
		let seamOffset: CGFloat
		if focusRect.width > cornerRadius * 2 {
			seamOffset = (focusRect.width - cornerRadius * 2) * 0.5
		} else {
			seamOffset = 0
		}
		let samples = selectionFlowSamples(
			for: focusRect,
			cornerRadius: cornerRadius,
			sampleCount: sampleCount,
			startOffset: seamOffset
		)
		let normals = selectionFlowNormals(for: samples)
		guard samples.count > 1, normals.count == samples.count else {
			return
		}
		ctx.setAllowsAntialiasing(true)
		ctx.setShouldAntialias(true)
		for pass in Self.passes {
			let half = max(pass.width * 0.5, 0.1)
			for index in samples.indices {
				let current = samples[index]
				let next = samples[(index + 1) % samples.count]
				let currentMovement = selectionFlowBand(
					progress: current.progress,
					phase: phase,
					bandWidth: Self.bandWidth
				)
				let nextMovement = selectionFlowBand(
					progress: next.progress,
					phase: phase,
					bandWidth: Self.bandWidth
				)
				let currentIntensity = Self.flowBoost * currentMovement
				let nextIntensity = Self.flowBoost * nextMovement
				let currentColor = selectionFlowColor(
					progress: current.progress + phase,
					alphaScale: pass.alphaScale,
					intensity: currentIntensity
				)
				let nextColor = selectionFlowColor(
					progress: next.progress + phase,
					alphaScale: pass.alphaScale,
					intensity: nextIntensity
				)
				let color = averagedColor(currentColor, nextColor)
				if color.alphaComponent <= 0.002 {
					continue
				}
				let currentNormal = scaledVector(normals[index], by: half)
				let nextNormal = scaledVector(normals[(index + 1) % normals.count], by: half)
				let quad = CGMutablePath()
				quad.move(to: offset(current.point, by: currentNormal))
				quad.addLine(to: offset(next.point, by: nextNormal))
				quad.addLine(to: offset(next.point, by: scaledVector(nextNormal, by: -1)))
				quad.addLine(to: offset(current.point, by: scaledVector(currentNormal, by: -1)))
				quad.closeSubpath()
				ctx.setFillColor(color.cgColor)
				ctx.addPath(quad)
				ctx.fillPath()
			}
		}
	}

	private func selectionFlowCornerRadius(for rect: CGRect) -> CGFloat {
		max(
			0,
			min(
				Self.cornerRadius,
				min(rect.width / 2 - 0.25, rect.height / 2 - 0.25)
			)
		)
	}

	private func selectionFlowSampleCount(for perimeter: CGFloat) -> Int {
		guard perimeter > 0, perimeter.isFinite else {
			return Self.minSegments
		}
		let byStep = Int(ceil(perimeter / Self.sampleStep))
		return min(max(byStep, Self.minSegments), Self.maxSegments)
	}

	private func selectionFlowBand(
		progress: CGFloat,
		phase: CGFloat,
		bandWidth: CGFloat
	) -> CGFloat {
		let width = min(max(bandWidth, 0.001), 0.5)
		let wrapped = (progress - phase).truncatingRemainder(dividingBy: 1)
		let distance = wrapped >= 0 ? wrapped : wrapped + 1
		let shortest = min(distance, 1 - distance)
		let normalized = min(shortest / width, 1)
		return pow(1 - normalized, 2)
	}

	private func selectionFlowColor(
		progress: CGFloat,
		alphaScale: CGFloat,
		intensity: CGFloat
	) -> NSColor {
		let palette = theme == .dark ? Self.darkPalette : Self.lightPalette
		let normalized =
			progress.truncatingRemainder(dividingBy: 1) >= 0
			? progress.truncatingRemainder(dividingBy: 1)
			: progress.truncatingRemainder(dividingBy: 1) + 1
		let bandPosition = normalized * CGFloat(palette.count)
		let bandIndex = Int(floor(bandPosition)) % palette.count
		let local = bandPosition - CGFloat(bandIndex)
		let current = palette[bandIndex]
		let next = palette[(bandIndex + 1) % palette.count]
		let blend = { (lhs: CGFloat, rhs: CGFloat) in lhs + (rhs - lhs) * local }
		let alpha = min(max(alphaScale * intensity, 0), 1)
		return NSColor(
			red: blend(current.0, next.0),
			green: blend(current.1, next.1),
			blue: blend(current.2, next.2),
			alpha: alpha
		)
	}

	private func selectionFlowSamples(
		for rect: CGRect,
		cornerRadius: CGFloat,
		sampleCount: Int,
		startOffset: CGFloat
	) -> [SamplePoint] {
		let perimeter = selectionFlowPerimeter(for: rect, cornerRadius: cornerRadius)
		guard perimeter > 0 else {
			return []
		}
		let start = (startOffset / perimeter).truncatingRemainder(dividingBy: 1)
		return (0..<sampleCount).map { index in
			let t = (CGFloat(index) + 0.5) / CGFloat(sampleCount)
			let progress = (t + start).truncatingRemainder(dividingBy: 1)
			return SamplePoint(
				point: selectionFlowPoint(
					for: rect,
					cornerRadius: cornerRadius,
					distance: perimeter * progress
				),
				progress: t
			)
		}
	}

	private func selectionFlowNormals(for samples: [SamplePoint]) -> [CGVector] {
		let count = samples.count
		guard count > 0 else {
			return []
		}
		var normals = Array(repeating: CGVector(dx: 0, dy: 0), count: count)
		var firstNonZero: Int?
		for index in 0..<count {
			let current = samples[index].point
			let previous = samples[(index + count - 1) % count].point
			let next = samples[(index + 1) % count].point
			let previousTangent = CGVector(dx: current.x - previous.x, dy: current.y - previous.y)
			let nextTangent = CGVector(dx: next.x - current.x, dy: next.y - current.y)
			var normal = CGVector(dx: 0, dy: 0)
			if lengthSquared(previousTangent) > CGFloat.ulpOfOne {
				let previousLength = length(previousTangent)
				normal.dx += -previousTangent.dy / previousLength
				normal.dy += previousTangent.dx / previousLength
			}
			if lengthSquared(nextTangent) > CGFloat.ulpOfOne {
				let nextLength = length(nextTangent)
				normal.dx += -nextTangent.dy / nextLength
				normal.dy += nextTangent.dx / nextLength
			}
			if lengthSquared(normal) <= CGFloat.ulpOfOne {
				if lengthSquared(nextTangent) > CGFloat.ulpOfOne {
					let nextLength = length(nextTangent)
					normal = CGVector(
						dx: -nextTangent.dy / nextLength, dy: nextTangent.dx / nextLength)
				} else if lengthSquared(previousTangent) > CGFloat.ulpOfOne {
					let previousLength = length(previousTangent)
					normal = CGVector(
						dx: -previousTangent.dy / previousLength,
						dy: previousTangent.dx / previousLength)
				}
			}
			if lengthSquared(normal) > CGFloat.ulpOfOne {
				let normalized = scaledVector(normal, by: 1 / length(normal))
				if firstNonZero == nil {
					firstNonZero = index
				}
				normals[index] = normalized
			}
		}
		if let firstNonZero {
			var previous = normals[firstNonZero]
			if lengthSquared(previous) > CGFloat.ulpOfOne {
				for index in (firstNonZero + 1)..<count {
					if lengthSquared(normals[index]) > CGFloat.ulpOfOne,
						dot(normals[index], previous) < 0
					{
						normals[index] = scaledVector(normals[index], by: -1)
					}
					if lengthSquared(normals[index]) > CGFloat.ulpOfOne {
						previous = normals[index]
					}
				}
				if firstNonZero > 0 {
					for index in stride(from: firstNonZero - 1, through: 0, by: -1) {
						if lengthSquared(normals[index]) > CGFloat.ulpOfOne,
							dot(normals[index], previous) < 0
						{
							normals[index] = scaledVector(normals[index], by: -1)
						}
						if lengthSquared(normals[index]) > CGFloat.ulpOfOne {
							previous = normals[index]
						}
					}
				}
			}
		}
		return normals
	}

	private func selectionFlowPerimeter(for rect: CGRect, cornerRadius: CGFloat) -> CGFloat {
		let edgeTop = max(rect.width - cornerRadius * 2, 0)
		let edgeRight = max(rect.height - cornerRadius * 2, 0)
		let cornerLength = (.pi / 2) * cornerRadius
		return 2 * (edgeTop + edgeRight) + 4 * cornerLength
	}

	private func selectionFlowPoint(
		for rect: CGRect,
		cornerRadius: CGFloat,
		distance: CGFloat
	) -> CGPoint {
		if cornerRadius <= .ulpOfOne {
			let perimeter = selectionFlowPerimeter(for: rect, cornerRadius: 0)
			let keep = distance.truncatingRemainder(dividingBy: perimeter)
			let edgeTop = rect.width
			let edgeRight = rect.height
			if keep < edgeTop {
				return CGPoint(x: rect.minX + keep, y: rect.minY)
			}
			if keep < edgeTop + edgeRight {
				return CGPoint(x: rect.maxX, y: rect.minY + (keep - edgeTop))
			}
			if keep < edgeTop * 2 + edgeRight {
				return CGPoint(x: rect.maxX - (keep - edgeTop - edgeRight), y: rect.maxY)
			}
			return CGPoint(
				x: rect.minX,
				y: rect.maxY - (keep - edgeTop * 2 - edgeRight)
			)
		}

		let x0 = rect.minX
		let x1 = rect.maxX
		let y0 = rect.minY
		let y1 = rect.maxY
		let perimeter = selectionFlowPerimeter(for: rect, cornerRadius: cornerRadius)
		var remain = distance.truncatingRemainder(dividingBy: perimeter)
		if remain < 0 {
			remain += perimeter
		}
		let edgeTop = max(rect.width - cornerRadius * 2, 0)
		let edgeRight = max(rect.height - cornerRadius * 2, 0)
		let cornerLength = (.pi / 2) * cornerRadius

		if remain < edgeTop {
			return CGPoint(x: x0 + cornerRadius + remain, y: y0)
		}
		remain -= edgeTop
		if remain < cornerLength {
			let angle = -(.pi / 2) + remain / cornerRadius
			return CGPoint(
				x: x1 - cornerRadius + cornerRadius * cos(angle),
				y: y0 + cornerRadius + cornerRadius * sin(angle)
			)
		}
		remain -= cornerLength
		if remain < edgeRight {
			return CGPoint(x: x1, y: y0 + cornerRadius + remain)
		}
		remain -= edgeRight
		if remain < cornerLength {
			let angle = remain / cornerRadius
			return CGPoint(
				x: x1 - cornerRadius + cornerRadius * cos(angle),
				y: y1 - cornerRadius + cornerRadius * sin(angle)
			)
		}
		remain -= cornerLength
		if remain < edgeTop {
			return CGPoint(x: x1 - cornerRadius - remain, y: y1)
		}
		remain -= edgeTop
		if remain < cornerLength {
			let angle = (.pi / 2) + remain / cornerRadius
			return CGPoint(
				x: x0 + cornerRadius + cornerRadius * cos(angle),
				y: y1 - cornerRadius + cornerRadius * sin(angle)
			)
		}
		remain -= cornerLength
		if remain < edgeRight {
			return CGPoint(x: x0, y: y1 - cornerRadius - remain)
		}
		remain -= edgeRight
		if remain < cornerLength {
			let angle = .pi + remain / cornerRadius
			return CGPoint(
				x: x0 + cornerRadius + cornerRadius * cos(angle),
				y: y0 + cornerRadius + cornerRadius * sin(angle)
			)
		}
		return CGPoint(x: x0 + cornerRadius, y: y0)
	}

	private func averagedColor(_ lhs: NSColor, _ rhs: NSColor) -> NSColor {
		let left = lhs.usingColorSpace(.deviceRGB) ?? lhs
		let right = rhs.usingColorSpace(.deviceRGB) ?? rhs
		return NSColor(
			red: (left.redComponent + right.redComponent) * 0.5,
			green: (left.greenComponent + right.greenComponent) * 0.5,
			blue: (left.blueComponent + right.blueComponent) * 0.5,
			alpha: (left.alphaComponent + right.alphaComponent) * 0.5
		)
	}

	private func offset(_ point: CGPoint, by vector: CGVector) -> CGPoint {
		CGPoint(x: point.x + vector.dx, y: point.y + vector.dy)
	}

	private func scaledVector(_ vector: CGVector, by scalar: CGFloat) -> CGVector {
		CGVector(dx: vector.dx * scalar, dy: vector.dy * scalar)
	}

	private func dot(_ lhs: CGVector, _ rhs: CGVector) -> CGFloat {
		lhs.dx * rhs.dx + lhs.dy * rhs.dy
	}

	private func lengthSquared(_ vector: CGVector) -> CGFloat {
		vector.dx * vector.dx + vector.dy * vector.dy
	}

	private func length(_ vector: CGVector) -> CGFloat {
		sqrt(lengthSquared(vector))
	}
}

@MainActor
final class LiveOverlayRenderer {
	private weak var hostView: NSView?
	var onTick: (() -> Void)?
	private let rootLayer = CALayer()
	private let frozenDisplayLayer = CALayer()
	private let topScrimLayer = CALayer()
	private let leftScrimLayer = CALayer()
	private let rightScrimLayer = CALayer()
	private let bottomScrimLayer = CALayer()
	private let hoverGlowLayer = CAShapeLayer()
	private let hoverFlowLayer = SelectionFlowBandLayer()
	private let dragBorderOutlineLayer = CAShapeLayer()
	private let dragBorderLayer = CAShapeLayer()
	private let selectionSizeLayer = CATextLayer()
	private let hudLayer = CALayer()
	private let hudGlassLayer = CALayer()
	private let hudFillLayer = CALayer()
	private let hudStrokeLayer = CAShapeLayer()
	private let hudPositionLayer = CATextLayer()
	private let hudHexLayer = CATextLayer()
	private let hudSwatchLayer = CALayer()
	private let hudKeycapLayer = CALayer()
	private let hudKeycapTextLayer = CATextLayer()
	private let loupeLayer = CALayer()
	private let loupeGlassLayer = CALayer()
	private let loupeFillLayer = CALayer()
	private let loupeStrokeLayer = CAShapeLayer()
	private let loupePatchLayer = CALayer()
	private let loupeCenterLayer = CAShapeLayer()
	private let frameClock = LiveFrameClockDriver()
	private let layerRenderDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.layer_render_duration",
		category: "LiveChromeTelemetry"
	)
	private let layerChromeRenderDurationMetric = NativeHostTelemetry.distribution(
		"live_chrome.layer_chrome_render_duration",
		category: "LiveChromeTelemetry"
	)
	private var snapshotProvider: (() -> LivePreviewSnapshot?)?

	init(hostView: NSView) {
		self.hostView = hostView
		configureLayers()
		frameClock.onTick = { [weak self] in
			self?.renderCurrentSnapshot()
			self?.onTick?()
		}
	}

	func install(snapshotProvider: @escaping () -> LivePreviewSnapshot?) {
		self.snapshotProvider = snapshotProvider
		guard let hostView else {
			return
		}
		if hostView.layer == nil {
			hostView.wantsLayer = true
		}
		hostView.layer?.addSublayer(rootLayer)
		rootLayer.isHidden = true
	}

	func updateDisplayID(_ displayID: CGDirectDisplayID?, targetFramesPerSecond: Int) {
		guard displayID != nil else {
			stop()
			return
		}
		frameClock.start(targetFramesPerSecond: targetFramesPerSecond)
	}

	func stopFrameClock() {
		frameClock.stop()
	}

	func stop() {
		frameClock.stop()
		rootLayer.isHidden = true
	}

	func suspend() {
		rootLayer.isHidden = true
	}

	func renderNow() {
		renderCurrentSnapshot()
		onTick?()
	}

	func renderLiveChromeNow() {
		renderChromeSnapshot()
		onTick?()
	}

	private func configureLayers() {
		rootLayer.zPosition = 100
		rootLayer.masksToBounds = false
		frozenDisplayLayer.isHidden = true
		rootLayer.addSublayer(frozenDisplayLayer)
		for scrimLayer in [topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer] {
			rootLayer.addSublayer(scrimLayer)
			scrimLayer.isHidden = true
		}
		hoverGlowLayer.fillColor = NSColor.clear.cgColor
		hoverGlowLayer.lineWidth = 2.25
		hoverGlowLayer.shadowOffset = .zero
		hoverGlowLayer.shadowRadius = 12
		rootLayer.addSublayer(hoverGlowLayer)

		rootLayer.addSublayer(hoverFlowLayer)

		dragBorderOutlineLayer.fillColor = NSColor.clear.cgColor
		rootLayer.addSublayer(dragBorderOutlineLayer)

		dragBorderLayer.fillColor = NSColor.clear.cgColor
		rootLayer.addSublayer(dragBorderLayer)

		selectionSizeLayer.contentsScale = 2
		rootLayer.addSublayer(selectionSizeLayer)

		for chromeLayer in [hudLayer, loupeLayer] {
			chromeLayer.masksToBounds = false
			rootLayer.addSublayer(chromeLayer)
		}
		for hudSublayer in [
			hudGlassLayer, hudFillLayer, hudStrokeLayer, hudSwatchLayer, hudPositionLayer,
			hudHexLayer, hudKeycapLayer, hudKeycapTextLayer,
		] {
			hudLayer.addSublayer(hudSublayer)
		}
		for loupeSublayer in [
			loupeGlassLayer, loupeFillLayer, loupeStrokeLayer, loupePatchLayer, loupeCenterLayer,
		] {
			loupeLayer.addSublayer(loupeSublayer)
		}
		for chromeLayer in [hudLayer, loupeLayer] {
			chromeLayer.isHidden = true
		}
	}

	private func renderCurrentSnapshot() {
		guard let snapshot = snapshotProvider?() else {
			rootLayer.isHidden = true
			return
		}
		let renderStart = ProcessInfo.processInfo.systemUptime
		defer {
			layerRenderDurationMetric.recordMillisecondsSince(renderStart)
		}
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		rootLayer.isHidden = false
		rootLayer.frame = snapshot.bounds
		renderFrozenDisplay(snapshot)
		renderFocus(snapshot)
		renderHud(snapshot)
		renderLoupe(snapshot)
		CATransaction.commit()
	}

	private func renderChromeSnapshot() {
		guard let snapshot = snapshotProvider?() else {
			rootLayer.isHidden = true
			return
		}
		let renderStart = ProcessInfo.processInfo.systemUptime
		defer {
			layerChromeRenderDurationMetric.recordMillisecondsSince(renderStart)
		}
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		rootLayer.isHidden = false
		rootLayer.frame = snapshot.bounds
		renderHud(snapshot)
		renderLoupe(snapshot)
		CATransaction.commit()
	}

	private func renderFrozenDisplay(_ snapshot: LivePreviewSnapshot) {
		guard let image = snapshot.frozenDisplayImage, let frame = snapshot.frozenDisplayFrame
		else {
			frozenDisplayLayer.isHidden = true
			frozenDisplayLayer.contents = nil
			return
		}
		frozenDisplayLayer.contentsGravity = .resize
		frozenDisplayLayer.contentsScale = hostView?.window?.screen?.backingScaleFactor ?? 2
		frozenDisplayLayer.frame = frame
		frozenDisplayLayer.contents = image
		frozenDisplayLayer.isHidden = false
	}

	private func renderFocus(_ snapshot: LivePreviewSnapshot) {
		let focusRect = snapshot.dragSelectionLocal ?? snapshot.hoverSelectionLocal
		guard let focusRect else {
			for scrimLayer in [topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer] {
				scrimLayer.isHidden = true
			}
			hoverGlowLayer.isHidden = true
			hoverFlowLayer.hide()
			dragBorderOutlineLayer.isHidden = true
			dragBorderLayer.isHidden = true
			selectionSizeLayer.isHidden = true
			return
		}

		let scrimAlpha = CGFloat(CaptureChrome.liveScrimAlpha)
		let scrimColor = NSColor(calibratedWhite: 0, alpha: scrimAlpha).cgColor
		let bounds = snapshot.bounds
		let rects = [
			CGRect(
				x: bounds.minX, y: bounds.minY, width: bounds.width,
				height: max(0, focusRect.minY - bounds.minY)),
			CGRect(
				x: bounds.minX, y: focusRect.minY, width: max(0, focusRect.minX - bounds.minX),
				height: focusRect.height),
			CGRect(
				x: focusRect.maxX, y: focusRect.minY, width: max(0, bounds.maxX - focusRect.maxX),
				height: focusRect.height),
			CGRect(
				x: bounds.minX, y: focusRect.maxY, width: bounds.width,
				height: max(0, bounds.maxY - focusRect.maxY)),
		]
		for (layer, rect) in zip(
			[topScrimLayer, leftScrimLayer, rightScrimLayer, bottomScrimLayer], rects)
		{
			layer.backgroundColor = scrimColor
			layer.frame = rect
			layer.isHidden = rect.width <= 0 || rect.height <= 0
		}

		if snapshot.frozenPending {
			hoverGlowLayer.isHidden = true
			hoverFlowLayer.hide()
			dragBorderOutlineLayer.isHidden = false
			dragBorderLayer.isHidden = false
			selectionSizeLayer.isHidden = true
			let pixelsPerPoint = hostView?.window?.screen?.backingScaleFactor ?? 1
			let borderOutset = CaptureChrome.dashedBorderOutset(
				strokeWidth: CaptureChrome.frozenDashedBorderWidth,
				pixelsPerPoint: pixelsPerPoint
			)
			let borderRect = focusRect.insetBy(dx: -borderOutset, dy: -borderOutset)
			let frozenPath = CaptureChrome.dashedBorderPath(for: borderRect)
			dragBorderOutlineLayer.path = frozenPath
			dragBorderOutlineLayer.strokeColor =
				NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
				.cgColor
			dragBorderOutlineLayer.lineWidth = CaptureChrome.frozenDashedBorderWidth + 0.75
			dragBorderOutlineLayer.lineCap = .butt
			dragBorderOutlineLayer.lineJoin = .miter
			dragBorderLayer.path = frozenPath
			dragBorderLayer.strokeColor =
				NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 248 / 255)
				.cgColor
			dragBorderLayer.lineWidth = CaptureChrome.frozenDashedBorderWidth
			dragBorderLayer.lineCap = .butt
			dragBorderLayer.lineJoin = .miter
			return
		}

		if let dragSelection = snapshot.dragSelectionLocal {
			hoverGlowLayer.isHidden = true
			hoverFlowLayer.hide()
			dragBorderOutlineLayer.isHidden = false
			dragBorderLayer.isHidden = false
			let pixelsPerPoint = hostView?.window?.screen?.backingScaleFactor ?? 1
			let borderOutset = CaptureChrome.dashedBorderOutset(
				strokeWidth: CaptureChrome.liveDashedBorderWidth,
				pixelsPerPoint: pixelsPerPoint
			)
			let borderRect = dragSelection.insetBy(dx: -borderOutset, dy: -borderOutset)
			let dragPath = CaptureChrome.dashedBorderPath(for: borderRect)
			dragBorderOutlineLayer.path = dragPath
			dragBorderOutlineLayer.strokeColor =
				NSColor(calibratedRed: 229 / 255, green: 247 / 255, blue: 1, alpha: 116 / 255)
				.cgColor
			dragBorderOutlineLayer.lineWidth = CaptureChrome.liveDashedBorderWidth + 0.75
			dragBorderOutlineLayer.lineCap = .butt
			dragBorderOutlineLayer.lineJoin = .miter
			dragBorderLayer.path = dragPath
			dragBorderLayer.strokeColor =
				NSColor(calibratedRed: 167 / 255, green: 223 / 255, blue: 1, alpha: 0.96).cgColor
			dragBorderLayer.lineWidth = CaptureChrome.liveDashedBorderWidth
			dragBorderLayer.lineCap = .butt
			dragBorderLayer.lineJoin = .miter
			if let selectionSizeText = snapshot.selectionSizeText {
				let font = LiveOverlayTypography.font
				let textSize = selectionSizeText.size(using: font)
				let frame = CaptureChrome.selectionSizeBadgeFrame(
					for: dragSelection,
					textSize: textSize,
					in: bounds
				)
				applyText(
					selectionSizeLayer,
					text: selectionSizeText,
					font: font,
					color: NSColor.white.withAlphaComponent(0.98),
					frame: frame,
					alignment: .left
				)
				selectionSizeLayer.isHidden = false
			} else {
				selectionSizeLayer.isHidden = true
			}
			return
		}

		dragBorderOutlineLayer.isHidden = true
		dragBorderLayer.isHidden = true
		selectionSizeLayer.isHidden = true
		let hoverPath = NSBezierPath(
			roundedRect: focusRect,
			xRadius: CaptureChrome.liveSelectionCornerRadius,
			yRadius: CaptureChrome.liveSelectionCornerRadius
		).cgPath
		hoverGlowLayer.path = hoverPath
		hoverGlowLayer.isHidden = true
		hoverFlowLayer.update(
			frame: snapshot.bounds,
			focusRect: focusRect,
			theme: snapshot.theme,
			timestamp: CACurrentMediaTime(),
			contentsScale: hostView?.window?.screen?.backingScaleFactor ?? 2
		)
	}

	private func renderHud(_ snapshot: LivePreviewSnapshot) {
		guard let hudFrame = snapshot.hudFrame else {
			hudLayer.isHidden = true
			return
		}
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		hudLayer.isHidden = false
		hudLayer.frame = hudFrame
		applySurfaceStyle(
			container: hudLayer,
			glassLayer: hudGlassLayer,
			fillLayer: hudFillLayer,
			strokeLayer: hudStrokeLayer,
			frame: hudLayer.bounds,
			palette: palette,
			settings: snapshot.settings,
			glassImage: snapshot.glassPatches[.hud]
		)

		let font = LiveOverlayTypography.font
		let positionText =
			"x=\(snapshot.positionDisplay.xValueText), y=\(snapshot.positionDisplay.yValueText)"
		let positionSize = CGSize(
			width: snapshot.positionDisplay.xSlotWidth
				+ LiveOverlayTypography.commaWidth
				+ snapshot.positionDisplay.ySlotWidth,
			height: LiveOverlayTypography.lineHeight
		)
		var cursorX = CaptureChrome.hudInnerMarginX
		let baselineY = (hudLayer.bounds.height - positionSize.height) / 2
		applyText(
			hudPositionLayer,
			text: positionText,
			font: font,
			color: palette.labelText,
			frame: CGRect(
				x: cursorX, y: baselineY, width: ceil(positionSize.width),
				height: ceil(positionSize.height)),
			alignment: .left
		)
		cursorX += positionSize.width + 10

		hudSwatchLayer.frame = CGRect(
			x: cursorX, y: hudLayer.bounds.midY - 5, width: 10, height: 10)
		hudSwatchLayer.cornerRadius = 0
		let swatchColor =
			snapshot.rgbSample.map {
				NSColor(
					calibratedRed: CGFloat($0.r) / 255, green: CGFloat($0.g) / 255,
					blue: CGFloat($0.b) / 255, alpha: 1)
			} ?? NSColor(calibratedWhite: 1, alpha: 0.12)
		hudSwatchLayer.backgroundColor = swatchColor.cgColor
		hudSwatchLayer.borderColor = palette.swatchStroke.cgColor
		hudSwatchLayer.borderWidth = 1
		cursorX += 20

		applyText(
			hudHexLayer, text: snapshot.colorDisplay.hexText, font: font, color: palette.labelText,
			frame: CGRect(
				x: cursorX, y: baselineY, width: ceil(snapshot.colorDisplay.hexSlotWidth),
				height: ceil(LiveOverlayTypography.lineHeight)), alignment: .left)
		cursorX += snapshot.colorDisplay.hexSlotWidth + 10

		if snapshot.keycapVisible {
			let keycapText = "Tab"
			let keycapFont = font
			let keycapFrame = CGRect(
				x: cursorX,
				y: hudLayer.bounds.midY - LiveOverlayTypography.keycapFrameSize.height / 2,
				width: LiveOverlayTypography.keycapFrameSize.width,
				height: LiveOverlayTypography.keycapFrameSize.height
			)
			hudKeycapLayer.isHidden = false
			hudKeycapTextLayer.isHidden = false
			hudKeycapLayer.frame = keycapFrame
			hudKeycapLayer.cornerRadius = 6
			hudKeycapLayer.backgroundColor = palette.keycapFill.cgColor
			hudKeycapLayer.borderColor = palette.keycapStroke.cgColor
			hudKeycapLayer.borderWidth = 1
			applyText(
				hudKeycapTextLayer, text: keycapText, font: keycapFont, color: palette.keycapText,
				frame: keycapFrame, alignment: .center)
		} else {
			hudKeycapLayer.isHidden = true
			hudKeycapTextLayer.isHidden = true
		}
	}

	private func renderLoupe(_ snapshot: LivePreviewSnapshot) {
		guard let loupeFrame = snapshot.loupeFrame, let loupePatch = snapshot.loupePatch else {
			loupeLayer.isHidden = true
			return
		}
		let palette = CaptureChrome.palette(for: snapshot.theme, settings: snapshot.settings)
		loupeLayer.isHidden = false
		loupeLayer.frame = loupeFrame
		applySurfaceStyle(
			container: loupeLayer,
			glassLayer: loupeGlassLayer,
			fillLayer: loupeFillLayer,
			strokeLayer: loupeStrokeLayer,
			frame: loupeLayer.bounds,
			palette: palette,
			settings: snapshot.settings,
			glassImage: snapshot.glassPatches[.loupe]
		)
		loupePatchLayer.frame = loupeLayer.bounds.insetBy(dx: 10, dy: 10)
		loupePatchLayer.contentsGravity = .resizeAspectFill
		loupePatchLayer.minificationFilter = .nearest
		loupePatchLayer.magnificationFilter = .nearest
		loupePatchLayer.contents = loupePatch
		let centerRect = CGRect(
			x: loupePatchLayer.frame.midX - CaptureChrome.loupeCellSize / 2,
			y: loupePatchLayer.frame.midY - CaptureChrome.loupeCellSize / 2,
			width: CaptureChrome.loupeCellSize,
			height: CaptureChrome.loupeCellSize
		).insetBy(dx: 1, dy: 1)
		loupeCenterLayer.path = CGPath(rect: centerRect, transform: nil)
		loupeCenterLayer.fillColor = NSColor.clear.cgColor
		loupeCenterLayer.strokeColor = NSColor.white.withAlphaComponent(0.9).cgColor
		loupeCenterLayer.lineWidth = 2
	}

	private func applySurfaceStyle(
		container: CALayer,
		glassLayer: CALayer,
		fillLayer: CALayer,
		strokeLayer: CAShapeLayer,
		frame: CGRect,
		palette: CaptureChromePalette,
		settings: NativeHostSettings,
		glassImage: CGImage?
	) {
		let cornerRadius = CaptureChrome.hudCornerRadius
		let boundsPath = CGPath(
			roundedRect: frame,
			cornerWidth: cornerRadius,
			cornerHeight: cornerRadius,
			transform: nil
		)
		let glassEnabled = settings.usesClassicHudGlass
		let opacity = CaptureChrome.effectiveHudOpacity(settings: settings)
		let hasInlineGlass = glassEnabled && glassImage != nil
		let usesExternalGlass = glassEnabled && glassImage == nil
		let hasGlass = hasInlineGlass || usesExternalGlass

		container.cornerRadius = cornerRadius
		container.shadowColor = palette.shadow.cgColor
		container.shadowOffset = .zero
		container.shadowRadius = 10
		container.shadowOpacity = usesExternalGlass ? 0 : Float(max(0.12, opacity * 0.75))

		glassLayer.frame = frame
		glassLayer.cornerRadius = cornerRadius
		glassLayer.masksToBounds = true
		glassLayer.contentsGravity = .resizeAspectFill
		glassLayer.contents = glassImage
		glassLayer.opacity = hasInlineGlass ? CaptureChrome.glassOpacity(settings: settings) : 0
		glassLayer.isHidden = !hasInlineGlass

		fillLayer.frame = frame
		fillLayer.cornerRadius = cornerRadius
		fillLayer.backgroundColor =
			usesExternalGlass
			? NSColor.clear.cgColor
			: CaptureChrome.effectiveBodyFill(
				palette: palette,
				settings: settings,
				hasGlass: hasGlass
			).cgColor

		strokeLayer.frame = frame
		strokeLayer.path = boundsPath
		strokeLayer.fillColor = NSColor.clear.cgColor
		strokeLayer.strokeColor = palette.outerStroke.cgColor
		strokeLayer.lineWidth = 1
	}

	private func applyText(
		_ layer: CATextLayer,
		text: String,
		font: NSFont,
		color: NSColor,
		frame: CGRect,
		alignment: CATextLayerAlignmentMode
	) {
		layer.contentsScale = hostView?.window?.backingScaleFactor ?? 2
		layer.string = text
		layer.font = font
		layer.fontSize = font.pointSize
		layer.foregroundColor = color.cgColor
		layer.alignmentMode = alignment
		layer.frame = frame
		layer.isWrapped = false
	}
}
