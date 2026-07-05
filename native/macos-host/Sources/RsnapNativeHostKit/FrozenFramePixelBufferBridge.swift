import CoreGraphics
import CoreVideo
import Foundation
import RsnapHostBridge

enum FrozenFramePixelBufferBridge {
	static func makeImage(from pixelBuffer: CVPixelBuffer) -> CGImage? {
		guard let backing = PixelBufferImageBacking(pixelBuffer) else {
			return nil
		}
		let width = CVPixelBufferGetWidth(pixelBuffer)
		let height = CVPixelBufferGetHeight(pixelBuffer)
		let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
		guard width > 0, height > 0, bytesPerRow >= width * 4 else {
			return nil
		}
		let retainedBacking = Unmanaged.passRetained(backing)
		guard
			let provider = CGDataProvider(
				dataInfo: retainedBacking.toOpaque(),
				data: backing.baseAddress,
				size: backing.byteCount,
				releaseData: { info, _, _ in
					guard let info else {
						return
					}
					Unmanaged<PixelBufferImageBacking>.fromOpaque(info).release()
				}
			)
		else {
			retainedBacking.release()
			return nil
		}
		let bitmapInfo = CGBitmapInfo.byteOrder32Little
			.union(CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue))
		return CGImage(
			width: width,
			height: height,
			bitsPerComponent: 8,
			bitsPerPixel: 32,
			bytesPerRow: bytesPerRow,
			space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
			bitmapInfo: bitmapInfo,
			provider: provider,
			decode: nil,
			shouldInterpolate: false,
			intent: .defaultIntent
		)
	}

	static func rgbSample(
		from pixelBuffer: CVPixelBuffer,
		point: CGPoint,
		displayFrame: CGRect
	) -> RGBSample? {
		let width = CVPixelBufferGetWidth(pixelBuffer)
		let height = CVPixelBufferGetHeight(pixelBuffer)
		let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
		guard width > 0, height > 0, bytesPerRow >= width * 4 else {
			return nil
		}
		guard CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly) == kCVReturnSuccess else {
			return nil
		}
		defer {
			CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly)
		}
		guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
			return nil
		}
		return try? RsnapBgraFrameSampler.rgbSample(
			width: width,
			height: height,
			bytesPerRow: bytesPerRow,
			baseAddress: baseAddress,
			byteCount: bytesPerRow * height,
			displayFrame: displayFrame,
			point: point
		)
	}

	static func loupePatch(
		from pixelBuffer: CVPixelBuffer,
		point: CGPoint,
		displayFrame: CGRect,
		sidePixels: Int
	) -> CGImage? {
		let width = CVPixelBufferGetWidth(pixelBuffer)
		let height = CVPixelBufferGetHeight(pixelBuffer)
		let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
		let side = max(sidePixels, 1)
		guard width > 0, height > 0, bytesPerRow >= width * 4 else {
			return nil
		}
		guard CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly) == kCVReturnSuccess else {
			return nil
		}
		defer {
			CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly)
		}
		guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
			return nil
		}
		guard
			let patch = try? RsnapBgraFrameSampler.loupePatch(
				width: width,
				height: height,
				bytesPerRow: bytesPerRow,
				baseAddress: baseAddress,
				byteCount: bytesPerRow * height,
				displayFrame: displayFrame,
				point: point,
				sidePixels: side
			)
		else {
			return nil
		}
		return NativeHostImageBridge.cgImage(
			width: patch.width,
			height: patch.height,
			rgba: patch.rgba
		)
	}

	private final class PixelBufferImageBacking {
		let pixelBuffer: CVPixelBuffer
		let baseAddress: UnsafeMutableRawPointer
		let byteCount: Int
		let unlockFlags = CVPixelBufferLockFlags.readOnly

		init?(_ pixelBuffer: CVPixelBuffer) {
			guard CVPixelBufferLockBaseAddress(pixelBuffer, unlockFlags) == kCVReturnSuccess else {
				return nil
			}
			guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
				CVPixelBufferUnlockBaseAddress(pixelBuffer, unlockFlags)
				return nil
			}
			let height = CVPixelBufferGetHeight(pixelBuffer)
			let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
			guard height > 0, bytesPerRow > 0 else {
				CVPixelBufferUnlockBaseAddress(pixelBuffer, unlockFlags)
				return nil
			}
			self.pixelBuffer = pixelBuffer
			self.baseAddress = baseAddress
			self.byteCount = bytesPerRow * height
		}

		deinit {
			CVPixelBufferUnlockBaseAddress(pixelBuffer, unlockFlags)
		}
	}
}
