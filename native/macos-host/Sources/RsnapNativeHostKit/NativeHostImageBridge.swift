import CoreGraphics
import Foundation
import RsnapHostBridge

enum NativeHostImageBridge {
	static func rgbaSnapshot(
		from image: CGImage,
		interpolationQuality: CGInterpolationQuality = .none,
		bitmapInfo: UInt32 =
			CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
	) -> RGBARegionSnapshot? {
		let width = image.width
		let height = image.height
		guard width > 0, height > 0 else {
			return nil
		}

		let bytesPerRow = width * 4
		var rgba = Data(count: bytesPerRow * height)
		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
		let rendered = rgba.withUnsafeMutableBytes { buffer -> Bool in
			guard
				let baseAddress = buffer.baseAddress,
				let context = CGContext(
					data: baseAddress,
					width: width,
					height: height,
					bitsPerComponent: 8,
					bytesPerRow: bytesPerRow,
					space: colorSpace,
					bitmapInfo: bitmapInfo
				)
			else {
				return false
			}

			context.interpolationQuality = interpolationQuality
			context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
			return true
		}
		guard rendered else {
			return nil
		}

		return RGBARegionSnapshot(width: width, height: height, rgba: rgba)
	}

	static func cgImage(
		from snapshot: RGBARegionSnapshot,
		shouldInterpolate: Bool = false
	) -> CGImage? {
		cgImage(
			width: snapshot.width,
			height: snapshot.height,
			rgba: snapshot.rgba,
			shouldInterpolate: shouldInterpolate
		)
	}

	static func cgImage(
		width: Int,
		height: Int,
		rgba: Data,
		bitmapInfo: CGBitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
		shouldInterpolate: Bool = false
	) -> CGImage? {
		guard width > 0, height > 0 else {
			return nil
		}
		let bytesPerRow = width * 4
		guard rgba.count == bytesPerRow * height else {
			return nil
		}

		let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
		guard
			let provider = CGDataProvider(data: rgba as CFData),
			let image = CGImage(
				width: width,
				height: height,
				bitsPerComponent: 8,
				bitsPerPixel: 32,
				bytesPerRow: bytesPerRow,
				space: colorSpace,
				bitmapInfo: bitmapInfo,
				provider: provider,
				decode: nil,
				shouldInterpolate: shouldInterpolate,
				intent: .defaultIntent
			)
		else {
			return nil
		}

		return image
	}
}
