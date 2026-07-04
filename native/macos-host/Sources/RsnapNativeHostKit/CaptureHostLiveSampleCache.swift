import CoreGraphics

package struct CaptureHostLiveSampleCache {
	private var latestChromeSample: LiveChromeSample?
	private var latestChromeSamplePoint: CGPoint?
	private var latestRgbSample: LiveRgbSample?
	private var latestRgbSamplePoint: CGPoint?

	package init() {}

	package var latestChrome: LiveChromeSample? {
		latestChromeSample
	}

	package var latestRgb: LiveRgbSample? {
		latestRgbSample
	}

	package mutating func reset() {
		latestChromeSample = nil
		latestChromeSamplePoint = nil
		latestRgbSample = nil
		latestRgbSamplePoint = nil
	}

	package mutating func seedChrome(_ sample: LiveChromeSample, point: CGPoint?) {
		latestChromeSample = sample
		latestChromeSamplePoint = point
	}

	package mutating func seedRgb(_ rgbSample: LiveRgbSample, point: CGPoint?) {
		latestRgbSample = rgbSample
		latestRgbSamplePoint = point
	}

	package func chromeSample(matching point: CGPoint?) -> LiveChromeSample? {
		guard Self.pointsMatch(latestChromeSamplePoint, point) else {
			return nil
		}
		guard let latestChromeSample else {
			return nil
		}
		guard latestChromeSample.rgb == nil || latestChromeSample.rgb?.isFresh() == true
		else {
			return LiveChromeSample(rgb: nil, loupePatch: latestChromeSample.loupePatch)
		}
		return latestChromeSample
	}

	package func rgbSample(matching point: CGPoint?) -> LiveRgbSample? {
		guard Self.pointsMatch(latestRgbSamplePoint, point) else {
			return nil
		}
		guard latestRgbSample?.isFresh(maximumAge: LiveRgbSample.maximumReusableAge) == true else {
			return nil
		}
		return latestRgbSample
	}

	package static func pointsMatch(_ samplePoint: CGPoint?, _ point: CGPoint?) -> Bool {
		switch (samplePoint, point) {
		case (nil, nil):
			return true
		case (let samplePoint?, let point?):
			return abs(samplePoint.x - point.x) <= 0.5 && abs(samplePoint.y - point.y) <= 0.5
		default:
			return false
		}
	}
}
