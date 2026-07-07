import CoreGraphics
import Foundation
import RsnapHostBridge

package struct ToolbarBackdropCaptureResult {
	package let patch: CGImage?
	package let frameSequence: UInt64?
	package let signature: UInt64?

	package init(patch: CGImage?, frameSequence: UInt64?, signature: UInt64?) {
		self.patch = patch
		self.frameSequence = frameSequence
		self.signature = signature
	}
}

package enum ToolbarBackdropWorker {
	nonisolated static func captureResult(
		in globalFrame: CGRect,
		liveFrameStream: LiveFrameStreamBroker,
		fallbackSource: CaptureSessionController.FrozenCaptureJobSource,
		maximumLiveFrameAgeMicroseconds: UInt64,
		capture: ToolbarBackdropCaptureStart
	) -> ToolbarBackdropCaptureResult {
		let rawFrame = liveFrameStream.nextRegionFrame(
			in: globalFrame,
			afterFrameSequence: capture.afterFrameSequence,
			waitForFresh: false
		)
		return captureResult(
			rawFrame: rawFrame,
			maximumLiveFrameAgeMicroseconds: maximumLiveFrameAgeMicroseconds,
			capture: capture,
			fallbackPatch: {
				OverlayImageSampler.captureBelowOverlay(
					in: globalFrame,
					source: fallbackSource
				)
			},
			fallbackSnapshot: {
				NativeHostImageBridge.rgbaSnapshot(from: $0)
			}
		)
	}

	nonisolated package static func captureResult(
		rawFrame: RGBARegionFrameSnapshot?,
		maximumLiveFrameAgeMicroseconds: UInt64,
		capture: ToolbarBackdropCaptureStart,
		fallbackPatch: () -> CGImage?,
		fallbackSnapshot: (CGImage) -> RGBARegionSnapshot?
	) -> ToolbarBackdropCaptureResult {
		let freshFrame: RGBARegionFrameSnapshot? =
			if let rawFrame,
				frameIsFresh(
					rawFrame,
					maximumAgeMicroseconds: maximumLiveFrameAgeMicroseconds
				)
			{
				rawFrame
			} else {
				nil
			}
		let frameSequence = max(
			rawFrame?.frameSequence ?? 0,
			freshFrame?.frameSequence ?? 0
		)
		let livePatch = freshFrame.flatMap {
			NativeHostImageBridge.cgImage(from: $0.region)
		}
		let liveSignature = freshFrame.map {
			signature($0.region)
		}
		let liveWouldRemainStatic =
			liveSignature == nil
			|| (capture.previousSignature != nil
				&& liveSignature == capture.previousSignature)
		let fallbackPatch =
			liveWouldRemainStatic && capture.fallbackPermitted
			? fallbackPatch() : nil
		let fallbackSnapshot = fallbackPatch.flatMap {
			fallbackSnapshot($0)
		}
		let fallbackSignature = fallbackSnapshot.map {
			signature($0)
		}
		let shouldUseFallback =
			fallbackPatch != nil
			&& (capture.previousSignature == nil
				|| fallbackSignature != capture.previousSignature)
		let patch = shouldUseFallback ? fallbackPatch : (livePatch ?? fallbackPatch)
		let resultSignature =
			shouldUseFallback ? fallbackSignature : (liveSignature ?? fallbackSignature)
		return ToolbarBackdropCaptureResult(
			patch: patch,
			frameSequence: frameSequence > 0 ? frameSequence : nil,
			signature: resultSignature
		)
	}

	nonisolated package static func frameIsFresh(
		_ frame: RGBARegionFrameSnapshot,
		maximumAgeMicroseconds: UInt64
	) -> Bool {
		frame.frameAgeMicroseconds <= maximumAgeMicroseconds
	}

	nonisolated package static func signature(_ region: RGBARegionSnapshot) -> UInt64 {
		var hash: UInt64 = 14_695_981_039_346_656_037
		let stride = max(region.rgba.count / 256, 4)
		region.rgba.withUnsafeBytes { rawBuffer in
			guard let bytes = rawBuffer.bindMemory(to: UInt8.self).baseAddress else {
				return
			}
			var index = 0
			while index < region.rgba.count {
				hash ^= UInt64(bytes[index])
				hash &*= 1_099_511_628_211
				index += stride
			}
		}
		hash ^= UInt64(max(region.width, 0))
		hash &*= 1_099_511_628_211
		hash ^= UInt64(max(region.height, 0))
		return hash
	}
}
