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
}
