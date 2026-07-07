import AppKit
import CoreGraphics
import Foundation
import RsnapHostBridge

@MainActor
final class LiveChromePipeline {
	typealias FrameRegionSampler = @Sendable (CGRect) -> CGImage?

	private let liveFrameStream: LiveFrameStreamBroker
	private let chromeSampleFeed: ChromeSampleFeed
	private let liveChromeBackdrops = LiveChromeBackdropWindowController()
	private let frameRgbSampler: ChromeSampleFeed.FrameRgbSampler
	private let framePatchSampler: ChromeSampleFeed.FramePatchSampler
	private let frameRegionSampler: FrameRegionSampler

	init(
		liveFrameStream: LiveFrameStreamBroker,
		frameRgbSampler: @escaping ChromeSampleFeed.FrameRgbSampler,
		framePatchSampler: @escaping ChromeSampleFeed.FramePatchSampler,
		frameRegionSampler: @escaping FrameRegionSampler,
		sampleUpdated: @escaping () -> Void
	) {
		self.liveFrameStream = liveFrameStream
		self.frameRgbSampler = frameRgbSampler
		self.framePatchSampler = framePatchSampler
		self.frameRegionSampler = frameRegionSampler
		self.chromeSampleFeed = ChromeSampleFeed(
			frameRgbSampler: frameRgbSampler,
			framePatchSampler: framePatchSampler,
			backgroundSampler: OverlayImageSampler.chromeSampleAtDisplayPoint,
			sampleUpdated: {
				DispatchQueue.main.async {
					sampleUpdated()
				}
			}
		)
	}

	func startFrameStream(
		for screens: [NSScreen],
		focusPoint: CGPoint,
		captureID: UInt64
	) {
		liveFrameStream.start(
			for: screens,
			prewarmPoint: focusPoint,
			captureID: captureID
		)
	}

	func startSampling(
		focusPoint: CGPoint,
		captureID: UInt64,
		source: LiveColorSampleSource?
	) {
		chromeSampleFeed.start(
			targetFramesPerSecond: NativeHostDisplayRefresh.samplingFramesPerSecond(),
			captureID: captureID)
		chromeSampleFeed.updateDemand(
			point: focusPoint,
			sidePixels: 1,
			includeLoupePatch: false,
			source: source
		)
	}

	func stop() {
		chromeSampleFeed.stop()
		liveChromeBackdrops.hideAll()
	}

	func hideBackdrops() {
		liveChromeBackdrops.hideAll()
	}

	func backgroundPatch(
		in rect: CGRect,
		captureBelowOverlay: () -> CGImage?
	) -> CGImage? {
		frameRegionSampler(rect)
			?? captureBelowOverlay()
	}

	func streamPatch(in rect: CGRect) -> CGImage? {
		frameRegionSampler(rect)
	}

	func cachedRegionImage(in rect: CGRect) -> CGImage? {
		frameRegionSampler(rect)
	}

	func nextRegionFrame(
		in rect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		liveFrameStream.nextRegionFrame(
			in: rect,
			afterFrameSequence: afterFrameSequence,
			waitForFresh: waitForFresh
		)
	}

	func nextRegionFrame(
		in rect: CGRect,
		pixelRect: CGRect,
		afterFrameSequence: UInt64,
		waitForFresh: Bool
	) -> RGBARegionFrameSnapshot? {
		liveFrameStream.nextRegionFrame(
			in: rect,
			pixelRect: pixelRect,
			afterFrameSequence: afterFrameSequence,
			waitForFresh: waitForFresh
		)
	}

	func updateLivePreviewDemand(
		point: CGPoint?,
		settings: NativeHostSettings,
		includeLoupePatch: Bool,
		source: LiveColorSampleSource?
	) {
		let samplePixels = includeLoupePatch ? settings.loupeSampleSize.sidePixels : 1
		chromeSampleFeed.updateDemand(
			point: point,
			sidePixels: samplePixels,
			includeLoupePatch: includeLoupePatch,
			source: source
		)
	}

	func liveChromeSnapshot(
		point: CGPoint?,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		let latestSample = chromeSampleFeed.snapshot(for: point)
		let wantsLoupePatch = includeLoupePatch
		let wantsLoupePatchSide = settings.loupeSampleSize.sidePixels
		let latestLoupePatchSatisfiesDemand =
			latestSample?.loupePatch.map {
				$0.width == wantsLoupePatchSide && $0.height == wantsLoupePatchSide
			}
			?? false
		let latestSampleSatisfiesDemand =
			latestSample?.rgbSample != nil
			&& (!wantsLoupePatch || latestLoupePatchSatisfiesDemand)
		if latestSampleSatisfiesDemand {
			return latestSample
		}
		if wantsLoupePatch, latestLoupePatchSatisfiesDemand {
			return latestSample
		}

		let _ = point
		if wantsLoupePatch, let latestSample {
			return LiveChromeSample(rgb: latestSample.rgb, loupePatch: nil)
		}
		return latestSample
	}

	func immediateLiveChromeSample(
		point: CGPoint,
		settings: NativeHostSettings,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		let samplePixels = includeLoupePatch ? settings.loupeSampleSize.sidePixels : 1
		return nativeChromeSample(
			point: point,
			sidePixels: samplePixels,
			includeLoupePatch: includeLoupePatch
		)
			?? chromeSampleFeed.snapshot(for: point)
	}

	private func nativeChromeSample(
		point: CGPoint,
		sidePixels: Int,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		let rgbSample = frameRgbSampler(point)
		let loupePatch = includeLoupePatch ? framePatchSampler(point, sidePixels) : nil
		guard rgbSample != nil || loupePatch != nil else {
			return nil
		}
		return LiveChromeSample(rgb: rgbSample, loupePatch: loupePatch)
	}

	func updateLiveChromeBackdrops(
		_ snapshot: LiveChromeBackdropSnapshot?,
		focusedWindowNumber: Int?
	) {
		liveChromeBackdrops.update(snapshot: snapshot, focusedWindowNumber: focusedWindowNumber)
	}
}
