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
			!scene.toolbarItems.contains(where: { $0.kind == .scroll }),
			scene.toolbarItems.contains(where: { $0.kind == .copy && $0.enabled }),
			scene.toolbarItems.contains(where: { $0.kind == .save && $0.enabled })
		else {
			fatalError("unexpected frozen scene: \(scene)")
		}
		try session.send(event: .toolbarItemInvoked(.scroll))
		guard try session.takeNextRequest() == nil else {
			fatalError("scroll toolbar invocation should stay disabled")
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

		let baseScrollFrame = makeScrollFrame(width: 16, height: 96, topRow: 0)
		let movedScrollFrame = makeScrollFrame(width: 16, height: 96, topRow: 24)
		let scrollSession = try RsnapScrollCaptureSession(
			baseImage: baseScrollFrame,
			previewWidthPixels: baseScrollFrame.width
		)
		let scrollResult = try scrollSession.observeDownwardFrame(movedScrollFrame)
		guard
			scrollResult.outcome == .committed,
			scrollResult.growthRows == 24,
			scrollResult.exportWidth == 16,
			scrollResult.exportHeight == 120,
			scrollResult.currentViewportTopY == 24
		else {
			fatalError("unexpected scroll observe result: \(scrollResult)")
		}
		guard let scrollExport = try scrollSession.exportImage(), scrollExport.height == 120 else {
			fatalError("unexpected scroll export image")
		}
		let png = try RsnapExportEncoder.pngData(from: scrollExport)
		guard let fullPNGDimensions = pngDimensions(png), fullPNGDimensions == (16, 120) else {
			fatalError("unexpected PNG export dimensions")
		}
		let croppedPNG = try RsnapExportEncoder.pngData(
			from: scrollExport,
			crop: CGRect(x: 1, y: 2, width: 4, height: 8)
		)
		guard let croppedPNGDimensions = pngDimensions(croppedPNG),
			croppedPNGDimensions == (4, 8)
		else {
			fatalError("unexpected cropped PNG export dimensions")
		}
		let frozenDisplayCrop = try RsnapExportEncoder.frozenDisplayCropRect(
			imageWidth: 2880,
			imageHeight: 1800,
			displayFrame: CGRect(x: 0, y: 0, width: 1440, height: 900),
			selection: CGRect(x: 100, y: 200, width: 300, height: 150)
		)
		guard frozenDisplayCrop == CGRect(x: 200, y: 1100, width: 600, height: 300) else {
			fatalError("unexpected frozen display crop rect")
		}
		let emptyFrozenDisplayCrop = try RsnapExportEncoder.frozenDisplayCropRect(
			imageWidth: 200,
			imageHeight: 200,
			displayFrame: CGRect(x: 0, y: 0, width: 100, height: 100),
			selection: CGRect(x: 120, y: 10, width: 10, height: 20)
		)
		guard emptyFrozenDisplayCrop == nil else {
			fatalError("unexpected out-of-bounds frozen display crop rect")
		}
		guard
			let mosaicPatch = try RsnapExportEncoder.frozenMosaicLightPrivacyPatch(
				imageWidth: 100,
				imageHeight: 80,
				sourceRect: CGRect(x: 4.2, y: 9.1, width: 28.4, height: 21.0)
			),
			mosaicPatch.width == 3,
			mosaicPatch.height == 3,
			Array(mosaicPatch.rgba.prefix(12)) == [
				211, 211, 211, 255, 205, 205, 205, 255, 202, 201, 199, 255,
			]
		else {
			fatalError("unexpected frozen mosaic privacy patch")
		}
		guard
			let framePlan = try RsnapCaptureFramePlanner.plan(
				imageWidth: 320,
				imageHeight: 180,
				screenScaleFactor: 2,
				source: .window
			),
			framePlan.canvasSize == CGSize(width: 416, height: 276),
			framePlan.imageRect == CGRect(x: 48, y: 48, width: 320, height: 180),
			framePlan.cornerRadius == 9.9,
			framePlan.shadows.count == 3,
			framePlan.shadows[0].blur == 80,
			framePlan.shadows[1].offset.height == -22
		else {
			fatalError("unexpected capture frame layout plan")
		}
		guard
			try RsnapCaptureFramePlanner.aspectFillCropRect(
				sourceWidth: 1600,
				sourceHeight: 900,
				destinationSize: CGSize(width: 1000, height: 1000)
			) == CGRect(x: 350, y: 0, width: 900, height: 900)
		else {
			fatalError("unexpected capture frame aspect-fill crop rect")
		}
		let backgroundPlan = try RsnapCaptureFramePlanner.backgroundPlan(for: .systemWallpaper)
		guard
			backgroundPlan.prefersWallpaper,
			backgroundPlan.wallpaperOverlayAlpha == 0.10,
			backgroundPlan.locations == [0, 0.54, 1],
			backgroundPlan.colorStops.count == 3,
			backgroundPlan.colorStops[0]
				== CaptureFrameColorStop(red: 0.10, green: 0.16, blue: 0.28, alpha: 1),
			backgroundPlan.colorStops[2]
				== CaptureFrameColorStop(red: 0.95, green: 0.61, blue: 0.43, alpha: 1)
		else {
			fatalError("unexpected capture frame background plan")
		}
		guard
			try RsnapCaptureFramePlanner.wallpaperRequestPlan(
				for: .systemWallpaper,
				destinationSize: CGSize(width: 1535.2, height: 996)
			) == CaptureFrameWallpaperRequest(targetPixelSize: 1536, overlayAlpha: 0.10),
			try RsnapCaptureFramePlanner.wallpaperRequestPlan(
				for: .aurora,
				destinationSize: CGSize(width: 1536, height: 996)
			) == nil
		else {
			fatalError("unexpected capture frame wallpaper request")
		}
		guard
			let minimapPlan = try RsnapScrollMinimapPlanner.plan(
				selection: CGRect(x: 100, y: 100, width: 100, height: 100),
				exportSize: CGSize(width: 100, height: 200),
				bounds: CGRect(x: 0, y: 0, width: 500, height: 500),
				preferredWidth: 96,
				minimumWidth: 44,
				gap: 10,
				margin: 10,
				imageInset: 3,
				viewportTopPixels: 20,
				viewportHeightPixels: 100
			),
			minimapPlan.frame == CGRect(x: 210, y: 54, width: 96, height: 192),
			minimapPlan.imageFrame == CGRect(x: 213, y: 57, width: 90, height: 186),
			minimapPlan.viewportFrame == CGRect(x: 213, y: 131.4, width: 90, height: 93)
		else {
			fatalError("unexpected scroll minimap layout plan")
		}
		let autoCenterFrame = makeAutoCenterFrame(
			width: 100,
			height: 80,
			content: CGRect(x: 30, y: 20, width: 24, height: 18)
		)
		guard
			try RsnapAutoCenterPlanner.contentBounds(in: autoCenterFrame)
				== CGRect(x: 30, y: 20, width: 24, height: 18),
			RsnapAutoCenterPlanner.marginBalanceShiftPoints(
				contentOriginPixels: 30,
				contentSizePixels: 24,
				cropSizePixels: 100,
				captureSizePoints: 50
			) == -4
		else {
			fatalError("unexpected auto-center plan")
		}

		print("rsnap-host-bridge probe ok")
	}

	private static func pngDimensions(_ data: Data) -> (Int, Int)? {
		let bytes = [UInt8](data)
		guard
			bytes.count >= 24,
			bytes[0..<8].elementsEqual([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
		else {
			return nil
		}
		let width =
			Int(UInt32(bytes[16]) << 24)
			| Int(UInt32(bytes[17]) << 16)
			| Int(UInt32(bytes[18]) << 8)
			| Int(UInt32(bytes[19]))
		let height =
			Int(UInt32(bytes[20]) << 24)
			| Int(UInt32(bytes[21]) << 16)
			| Int(UInt32(bytes[22]) << 8)
			| Int(UInt32(bytes[23]))

		return (width, height)
	}

	private static func makeScrollFrame(
		width: Int,
		height: Int,
		topRow: Int
	) -> RGBARegionSnapshot {
		var rgba = Data()
		rgba.reserveCapacity(width * height * 4)
		for y in 0..<height {
			let documentRow = topRow + y
			for x in 0..<width {
				rgba.append(UInt8((documentRow * 17 + x * 13) % 251))
				rgba.append(UInt8((documentRow * 29 + x * 7) % 251))
				rgba.append(UInt8((documentRow * 5 + x * 31) % 251))
				rgba.append(255)
			}
		}
		return RGBARegionSnapshot(width: width, height: height, rgba: rgba)
	}

	private static func makeAutoCenterFrame(
		width: Int,
		height: Int,
		content: CGRect
	) -> RGBARegionSnapshot {
		var rgba = Data(repeating: 180, count: width * height * 4)
		for index in stride(from: 3, to: rgba.count, by: 4) {
			rgba[index] = 255
		}
		let xRange = Int(content.minX)..<Int(content.maxX)
		let yRange = Int(content.minY)..<Int(content.maxY)
		for y in yRange {
			for x in xRange {
				let offset = (y * width + x) * 4
				rgba[offset] = 24
				rgba[offset + 1] = 32
				rgba[offset + 2] = 40
			}
		}

		return RGBARegionSnapshot(width: width, height: height, rgba: rgba)
	}
}
