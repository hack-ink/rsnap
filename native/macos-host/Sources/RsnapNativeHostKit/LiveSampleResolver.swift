import CoreGraphics
import RsnapHostBridge

enum LiveSampleResolver {
	static func currentLiveChromeSample(
		at point: CGPoint?,
		scenePointer: CGPoint?,
		loupeVisible: Bool,
		hoverChromeSuppressed: Bool,
		settings: NativeHostSettings,
		chrome: CaptureChromeState,
		cache: inout LiveSampleCache,
		sampleProvider: (_ wantsLoupePatch: Bool) -> LiveChromeSample?
	) -> LiveChromeSample? {
		let wantsLoupePatch = loupeVisible && !hoverChromeSuppressed
		let sample = sampleProvider(wantsLoupePatch)
		if let sample {
			let resolvedSample = sampleWithCachedLoupePatch(
				sample,
				point: point,
				scenePointer: scenePointer,
				wantsLoupePatch: wantsLoupePatch,
				settings: settings,
				chrome: chrome,
				cache: cache
			)
			cache.seedChrome(resolvedSample, point: point)
			if let rgbSample = resolvedSample.rgb {
				cache.seedRgb(rgbSample, point: point)
			}
			return resolvedSample
		}
		if let cachedSample = cache.chromeSample(matching: point) {
			return cachedSample
		}
		if chrome.loupePatch != nil,
			LiveSampleCache.pointsMatch(scenePointer, point)
		{
			seedChromeSample(from: chrome, point: scenePointer, cache: &cache)
			return cache.chromeSample(matching: point)
		}
		if wantsLoupePatch,
			let cachedPatch = reusableLiveLoupePatch(
				cache: cache,
				chrome: chrome,
				settings: settings
			)
		{
			return LiveChromeSample(rgb: nil, loupePatch: cachedPatch)
		}
		return nil
	}

	static func reusableLiveLoupePatch(
		cache: LiveSampleCache,
		chrome: CaptureChromeState,
		settings: NativeHostSettings
	) -> CGImage? {
		if let patch = cache.latestChrome?.loupePatch,
			liveLoupePatchMatchesCurrentSize(patch, settings: settings)
		{
			return patch
		}
		if let patch = chrome.loupePatch,
			liveLoupePatchMatchesCurrentSize(patch, settings: settings)
		{
			return patch
		}
		return nil
	}

	static func liveRgbSample(
		from sample: LiveChromeSample?,
		at point: CGPoint?,
		cache: inout LiveSampleCache
	) -> RGBSample? {
		if let rgbSample = sample?.rgb,
			rgbSample.isFresh()
		{
			cache.seedRgb(rgbSample, point: point)
			return rgbSample.rgb
		}
		return cache.rgbSample(matching: point)?.rgb
	}

	static func seedChromeSample(
		from chrome: CaptureChromeState,
		point: CGPoint?,
		cache: inout LiveSampleCache
	) {
		guard chrome.loupePatch != nil else {
			return
		}
		cache.seedChrome(
			LiveChromeSample(
				rgb: nil,
				loupePatch: chrome.loupePatch
			),
			point: point
		)
	}

	private static func sampleWithCachedLoupePatch(
		_ sample: LiveChromeSample,
		point: CGPoint?,
		scenePointer: CGPoint?,
		wantsLoupePatch: Bool,
		settings: NativeHostSettings,
		chrome: CaptureChromeState,
		cache: LiveSampleCache
	) -> LiveChromeSample {
		guard wantsLoupePatch, sample.loupePatch == nil else {
			return sample
		}
		if let cachedSample = cache.chromeSample(matching: point),
			let cachedPatch = cachedSample.loupePatch
		{
			return LiveChromeSample(
				rgb: sample.rgb,
				loupePatch: cachedPatch
			)
		}
		if let cachedPatch = reusableLiveLoupePatch(
			cache: cache,
			chrome: chrome,
			settings: settings
		) {
			return LiveChromeSample(
				rgb: sample.rgb,
				loupePatch: cachedPatch
			)
		}
		if LiveSampleCache.pointsMatch(scenePointer, point),
			let chromePatch = chrome.loupePatch,
			liveLoupePatchMatchesCurrentSize(chromePatch, settings: settings)
		{
			return LiveChromeSample(
				rgb: sample.rgb,
				loupePatch: chromePatch
			)
		}
		return sample
	}

	private static func liveLoupePatchMatchesCurrentSize(
		_ patch: CGImage,
		settings: NativeHostSettings
	) -> Bool {
		let sidePixels = settings.loupeSampleSize.sidePixels
		return patch.width == sidePixels && patch.height == sidePixels
	}
}
