import CoreGraphics
import Foundation
import RsnapHostBridge

enum ChromeSamplePolicy {
	static func rgbSampleSource(
		frameRgbSample: LiveRgbSample?,
		streamRgbSample: LiveRgbSample?,
		reusableRgbSample: LiveRgbSample?
	) -> String {
		if frameRgbSample != nil {
			return "frame_authority"
		}
		if streamRgbSample != nil {
			return "live_stream"
		}
		if reusableRgbSample != nil {
			return "reusable_cache"
		}
		return "none"
	}

	static func reusableRgbSample(
		previousSample: LiveChromeSample?,
		previousPoint: CGPoint?,
		point: CGPoint,
		now: TimeInterval
	) -> LiveRgbSample? {
		reusableRgbSample(
			rgbSample: previousSample?.rgb,
			previousPoint: previousPoint,
			point: point,
			now: now
		)
	}

	static func reusableRgbSample(
		rgbSample: LiveRgbSample?,
		previousPoint: CGPoint?,
		point: CGPoint,
		now: TimeInterval
	) -> LiveRgbSample? {
		guard let previousPoint, pointsEquivalent(previousPoint, point) else {
			return nil
		}
		guard rgbSample?.isFresh(maximumAge: LiveRgbSample.maximumReusableAge, now: now) == true
		else {
			return nil
		}
		return rgbSample
	}

	static func reusablePatchSample(
		previousSample: LiveChromeSample?,
		previousPoint: CGPoint?,
		point: CGPoint,
		includeLoupePatch: Bool
	) -> LiveChromeSample? {
		guard includeLoupePatch, let previousPoint, pointsEquivalent(previousPoint, point) else {
			return nil
		}
		return previousSample
	}

	static func recentPatchSample(
		previousSample: LiveChromeSample?,
		canReuseRecentPatch: Bool
	) -> LiveChromeSample? {
		guard canReuseRecentPatch, let loupePatch = previousSample?.loupePatch else {
			return nil
		}
		return LiveChromeSample(rgb: nil, loupePatch: loupePatch)
	}

	static func pointsEquivalent(_ lhs: CGPoint, _ rhs: CGPoint) -> Bool {
		abs(lhs.x - rhs.x) <= 0.5 && abs(lhs.y - rhs.y) <= 0.5
	}

	static func backgroundSampleOutcome(hasRgb: Bool, hasPatch: Bool) -> String {
		if hasRgb, hasPatch {
			return "rgb_patch"
		}
		if hasRgb {
			return "rgb"
		}
		if hasPatch {
			return "patch"
		}
		return "empty"
	}

	static func shouldNotifySampleUpdated(
		now: TimeInterval,
		lastPointChangeUptime: TimeInterval,
		idleDelay: TimeInterval
	) -> Bool {
		now - lastPointChangeUptime >= idleDelay
	}

	static func isLikelyOverlayWhite(_ sample: RGBSample) -> Bool {
		sample.r >= 250 && sample.g >= 250 && sample.b >= 250
	}

	static func sampleWithUpdatedPatch(
		rgb: LiveRgbSample?,
		patchSample: LiveChromeSample?
	) -> LiveChromeSample? {
		sampleWithUpdatedPatch(rgb: rgb, loupePatch: patchSample?.loupePatch)
	}

	static func sampleWithUpdatedPatch(
		rgb: LiveRgbSample?,
		loupePatch: CGImage?
	) -> LiveChromeSample? {
		guard rgb != nil || loupePatch != nil else {
			return nil
		}
		return LiveChromeSample(
			rgb: rgb,
			loupePatch: loupePatch
		)
	}
}
