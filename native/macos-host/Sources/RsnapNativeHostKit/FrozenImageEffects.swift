import CoreGraphics
import RsnapHostBridge

func makeFrozenMosaicPatch(from image: CGImage, sourceRect: CGRect) -> CGImage? {
	guard
		let patch = try? RsnapExportEncoder.frozenMosaicLightPrivacyPatch(
			imageWidth: image.width,
			imageHeight: image.height,
			sourceRect: sourceRect
		)
	else {
		return nil
	}

	let bitmapInfo =
		CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue
	return NativeHostImageBridge.cgImage(
		width: patch.width,
		height: patch.height,
		rgba: patch.rgba,
		bitmapInfo: CGBitmapInfo(rawValue: bitmapInfo)
	)
}
