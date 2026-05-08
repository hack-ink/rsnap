import CoreGraphics
import RsnapNativeHostKit

@main
enum RsnapNativeHostKitProbe {
	static func main() {
		let selection = CGRect(x: 100, y: 200, width: 50, height: 100)
		let imageSize = CGSize(width: 100, height: 200)
		let rectNearTop = CGRect(x: 110, y: 270, width: 20, height: 20)

		assertRectEqual(
			frozenExportOverlayRect(
				rectNearTop,
				selection: selection,
				imageSize: imageSize
			),
			CGRect(x: 20, y: 140, width: 40, height: 40),
			"rect overlay export must stay in bottom-left drawing coordinates"
		)
		assertRectEqual(
			frozenExportSourceImageRect(
				rectNearTop,
				selection: selection,
				imageSize: imageSize
			),
			CGRect(x: 20, y: 20, width: 40, height: 40),
			"source image rect must stay in top-down CGImage coordinates"
		)
		assertPointEqual(
			frozenExportOverlayPoint(
				CGPoint(x: 125, y: 280),
				selection: selection,
				imageSize: imageSize
			),
			CGPoint(x: 50, y: 160),
			"point annotation export must match bottom-left drawing coordinates"
		)
		assertRectOverlayDrawsAtVisualTop(
			rectNearTop,
			selection: selection,
			imageSize: imageSize
		)
		assertScrimRoundedExclusionKeepsCornersMasked()
		assertScrimOverlappingRoundedExclusionStaysClear()
		assertScrimExclusionPreservesExistingPixels()
		assertRoundedExclusionMaskKeepsCornersFilled()
		assertCaptureFrameEffectExpandsExportCanvas()
		let minimapExportSize = CGSize(width: 100, height: 200)
		guard
			let rightMinimap = scrollCaptureMinimapFrame(
				for: CGRect(x: 100, y: 100, width: 100, height: 100),
				exportSize: minimapExportSize,
				in: CGRect(x: 0, y: 0, width: 500, height: 500),
				preferredWidth: 96,
				minimumWidth: 44,
				gap: 10,
				margin: 10
			)
		else {
			fatalError("expected right-side scroll minimap frame")
		}
		assertRectEqual(
			rightMinimap,
			CGRect(x: 210, y: 54, width: 96, height: 192),
			"scroll minimap should prefer the right side when space is available"
		)
		guard
			let leftMinimap = scrollCaptureMinimapFrame(
				for: CGRect(x: 130, y: 100, width: 100, height: 100),
				exportSize: minimapExportSize,
				in: CGRect(x: 0, y: 0, width: 250, height: 500),
				preferredWidth: 96,
				minimumWidth: 44,
				gap: 10,
				margin: 10
			)
		else {
			fatalError("expected left-side scroll minimap frame")
		}
		assertRectEqual(
			leftMinimap,
			CGRect(x: 24, y: 54, width: 96, height: 192),
			"scroll minimap should fall back to the left when the right side is constrained"
		)
		assertLaunchAtLoginStateMapping()
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

	private static func assertPointEqual(_ actual: CGPoint, _ expected: CGPoint, _ message: String)
	{
		guard nearlyEqual(actual.x, expected.x), nearlyEqual(actual.y, expected.y) else {
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
		guard !missingBundle.isOn, missingBundle.isControlEnabled else {
			fatalError("missing app bundle should keep the login item toggle clickable")
		}

		let failed = LaunchAtLoginController.state(
			for: .notRegistered,
			errorMessage: "registration failed")
		guard !failed.isOn, failed.subtitle.contains("failed") else {
			fatalError("failed login item update should keep current state and surface failure")
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

	private static func assertRectOverlayDrawsAtVisualTop(
		_ rect: CGRect,
		selection: CGRect,
		imageSize: CGSize
	) {
		let width = Int(imageSize.width)
		let height = Int(imageSize.height)
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
			fatalError("could not create geometry probe context")
		}

		context.setFillColor(CGColor(red: 1, green: 0, blue: 0, alpha: 1))
		context.fill(
			frozenExportOverlayRect(
				rect,
				selection: selection,
				imageSize: imageSize
			))

		guard redPixel(in: data, width: width, x: 25, yFromTop: 30) else {
			fatalError("rect overlay export did not mark the visual top rows")
		}
		guard !redPixel(in: data, width: width, x: 25, yFromTop: 170) else {
			fatalError("rect overlay export marked the mirrored bottom rows")
		}
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

	private static func redPixel(
		in data: UnsafePointer<UInt8>,
		width: Int,
		x: Int,
		yFromTop: Int
	) -> Bool {
		let offset = (yFromTop * width + x) * 4
		return data[offset] > 200
			&& data[offset + 1] < 80
			&& data[offset + 2] < 20
			&& data[offset + 3] > 200
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
