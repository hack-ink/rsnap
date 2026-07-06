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

	static func regionImage(
		from pixelBuffer: CVPixelBuffer,
		rect: CGRect,
		displayFrame: CGRect
	) -> CGImage? {
		guard
			let snapshot = regionSnapshot(from: pixelBuffer, rect: rect, displayFrame: displayFrame)
		else {
			return nil
		}
		return NativeHostImageBridge.cgImage(from: snapshot)
	}

	static func regionSnapshot(
		from pixelBuffer: CVPixelBuffer,
		rect: CGRect,
		displayFrame: CGRect
	) -> RGBARegionSnapshot? {
		let frameWidth = CVPixelBufferGetWidth(pixelBuffer)
		let frameHeight = CVPixelBufferGetHeight(pixelBuffer)
		let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
		guard frameWidth > 0, frameHeight > 0, bytesPerRow >= frameWidth * 4,
			displayFrame.width > 0, displayFrame.height > 0, rect.width > 0, rect.height > 0
		else {
			return nil
		}
		guard
			let pixelRect = pixelRect(
				for: rect, displayFrame: displayFrame, frameWidth: frameWidth,
				frameHeight: frameHeight)
		else {
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

		let outputBytesPerRow = pixelRect.width * 4
		var rgba = Data(count: outputBytesPerRow * pixelRect.height)
		rgba.withUnsafeMutableBytes { outputBuffer in
			guard let output = outputBuffer.bindMemory(to: UInt8.self).baseAddress else {
				return
			}
			let source = baseAddress.assumingMemoryBound(to: UInt8.self)
			for row in 0..<pixelRect.height {
				let sourceRow = source.advanced(
					by: (pixelRect.y + row) * bytesPerRow + pixelRect.x * 4)
				let outputRow = output.advanced(by: row * outputBytesPerRow)
				for column in 0..<pixelRect.width {
					let sourcePixel = sourceRow.advanced(by: column * 4)
					let outputPixel = outputRow.advanced(by: column * 4)
					outputPixel[0] = sourcePixel[2]
					outputPixel[1] = sourcePixel[1]
					outputPixel[2] = sourcePixel[0]
					outputPixel[3] = sourcePixel[3]
				}
			}
		}

		return RGBARegionSnapshot(width: pixelRect.width, height: pixelRect.height, rgba: rgba)
	}

	private static func pixelRect(
		for rect: CGRect,
		displayFrame: CGRect,
		frameWidth: Int,
		frameHeight: Int
	) -> (x: Int, y: Int, width: Int, height: Int)? {
		let clipped = rect.intersection(displayFrame)
		guard clipped.isNull == false, clipped.width > 0, clipped.height > 0 else {
			return nil
		}
		let displayMaxY = displayFrame.maxY
		let x0 = floor(
			(clipped.minX - displayFrame.minX) / displayFrame.width * CGFloat(frameWidth))
		let x1 = ceil((clipped.maxX - displayFrame.minX) / displayFrame.width * CGFloat(frameWidth))
		let y0 = floor((displayMaxY - clipped.maxY) / displayFrame.height * CGFloat(frameHeight))
		let y1 = ceil((displayMaxY - clipped.minY) / displayFrame.height * CGFloat(frameHeight))
		let minX = max(0, min(frameWidth, Int(x0)))
		let maxX = max(0, min(frameWidth, Int(x1)))
		let minY = max(0, min(frameHeight, Int(y0)))
		let maxY = max(0, min(frameHeight, Int(y1)))
		guard maxX > minX, maxY > minY else {
			return nil
		}
		return (x: minX, y: minY, width: maxX - minX, height: maxY - minY)
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

package enum FrozenFramePixelBufferRegionProbe {
	package static func verifyRegionSnapshotMapping() {
		let pixelBuffer = makePixelBuffer(width: 8, height: 6)
		fillBgraFixture(pixelBuffer)
		let displayFrame = CGRect(x: 100, y: 50, width: 4, height: 3)

		guard
			let scaledRegion = FrozenFramePixelBufferBridge.regionSnapshot(
				from: pixelBuffer,
				rect: CGRect(x: 101, y: 51, width: 1, height: 1),
				displayFrame: displayFrame
			)
		else {
			fatalError("expected scaled nonzero-origin region")
		}
		assertRegion(
			scaledRegion,
			width: 2,
			height: 2,
			rgba: [
				22, 12, 4, 204, 23, 12, 5, 205,
				22, 13, 5, 205, 23, 13, 6, 206,
			],
			message: "scaled region should map display points to BGRA pixels and emit RGBA"
		)

		guard
			let clippedRegion = FrozenFramePixelBufferBridge.regionSnapshot(
				from: pixelBuffer,
				rect: CGRect(x: 103.5, y: 52.5, width: 2, height: 2),
				displayFrame: displayFrame
			)
		else {
			fatalError("expected clipped edge region")
		}
		assertRegion(
			clippedRegion,
			width: 1,
			height: 1,
			rgba: [27, 10, 7, 207],
			message: "edge region should clip to the display frame before sampling"
		)

		if FrozenFramePixelBufferBridge.regionSnapshot(
			from: pixelBuffer,
			rect: CGRect(x: 120, y: 80, width: 1, height: 1),
			displayFrame: displayFrame
		) != nil {
			fatalError("outside region should not sample")
		}
	}

	private static func makePixelBuffer(width: Int, height: Int) -> CVPixelBuffer {
		var pixelBuffer: CVPixelBuffer?
		let status = CVPixelBufferCreate(
			kCFAllocatorDefault,
			width,
			height,
			kCVPixelFormatType_32BGRA,
			nil,
			&pixelBuffer
		)
		guard status == kCVReturnSuccess, let pixelBuffer else {
			fatalError("failed to create pixel buffer: \(status)")
		}
		return pixelBuffer
	}

	private static func fillBgraFixture(_ pixelBuffer: CVPixelBuffer) {
		guard CVPixelBufferLockBaseAddress(pixelBuffer, []) == kCVReturnSuccess else {
			fatalError("failed to lock pixel buffer")
		}
		defer {
			CVPixelBufferUnlockBaseAddress(pixelBuffer, [])
		}
		guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
			fatalError("missing pixel buffer base address")
		}
		let width = CVPixelBufferGetWidth(pixelBuffer)
		let height = CVPixelBufferGetHeight(pixelBuffer)
		let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
		let bytes = baseAddress.assumingMemoryBound(to: UInt8.self)

		for y in 0..<height {
			for x in 0..<width {
				let pixel = bytes.advanced(by: y * bytesPerRow + x * 4)
				pixel[0] = UInt8(x + y)
				pixel[1] = UInt8(y + 10)
				pixel[2] = UInt8(x + 20)
				pixel[3] = UInt8(200 + x + y)
			}
		}
	}

	private static func assertRegion(
		_ region: RGBARegionSnapshot,
		width: Int,
		height: Int,
		rgba: [UInt8],
		message: String
	) {
		guard region.width == width, region.height == height, Array(region.rgba) == rgba else {
			fatalError("\(message): \(region)")
		}
	}
}
