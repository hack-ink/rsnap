import Foundation

enum LiveHudHexRollPlan {
	static let hexWheel = Array("0123456789ABCDEF")

	private static let pendingHexRollBaseSeed: UInt64 = 0x5EED_71A5_C01D

	static func pendingSequence(index: Int) -> [Character] {
		var seed = pendingSeed(index: index)
		seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
		let visibleRows = 47 + Int((seed >> 57) & 0x1F) + index * 3
		var digits: [Character] = []
		digits.reserveCapacity(visibleRows + 1)
		var previous: Character?
		for offset in 0..<visibleRows {
			seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
			var wheelIndex = Int((seed >> 58) & 0xF)
			if let previousDigit = previous,
				hexWheel[wheelIndex] == previousDigit
			{
				wheelIndex = (wheelIndex + offset + index + 1) % hexWheel.count
			}
			let digit = hexWheel[wheelIndex]
			digits.append(digit)
			previous = digit
		}
		if let first = digits.first {
			digits.append(first)
		}
		return digits
	}

	static func pendingColumnDuration(index: Int) -> TimeInterval {
		let seed =
			pendingSeed(index: index)
			&* 2_862_933_555_777_941_757
			&+ 3_037_000_493
		return 1.58 + Double((seed >> 56) & 0x1F) * 0.031
	}

	static func pendingColumnPhase(index: Int, duration: TimeInterval) -> TimeInterval {
		let seed =
			pendingSeed(index: index)
			&* 11_400_714_819_323_198_485
			&+ 12_829_314
		let ratio = Double((seed >> 40) & 0xFFFF) / 65_535.0
		return duration * ratio
	}

	static func pendingColumnScrollsUp(index: Int) -> Bool {
		let uptimeBucket = UInt64((ProcessInfo.processInfo.systemUptime * 1_000).rounded(.down))
		let startsUp = ((pendingHexRollBaseSeed ^ uptimeBucket) & 1) == 0
		if index <= 1 {
			return index == 0 ? startsUp : !startsUp
		}
		let seed =
			pendingSeed(index: index)
			&* 3_202_034_522_624_059_733
			&+ 1_029
		return ((seed >> 63) & 1) == 0
	}

	static func resolveColumnScrollsUp(
		index: Int,
		startDigit: Character,
		targetDigit: Character
	) -> Bool {
		let startValue = UInt64(startDigit.unicodeScalars.first?.value ?? 0)
		let targetValue = UInt64(targetDigit.unicodeScalars.first?.value ?? 0)
		let seed =
			pendingSeed(index: index)
			^ (startValue &* 1_099_511_628_211)
			^ (targetValue &* 2_862_933_555_777_941_757)
		let startsUp = ((seed >> 63) & 1) == 0
		if index <= 1 {
			return index == 0 ? startsUp : !startsUp
		}
		return ((seed >> 59) & 1) == 0
	}

	static func resolveExtraLoops(index: Int, targetDigit: Character) -> Int {
		let targetValue = UInt64(targetDigit.unicodeScalars.first?.value ?? 0)
		let seed =
			pendingSeed(index: index)
			^ (targetValue &* 11_400_714_819_323_198_485)
		return 1 + Int((seed >> 60) & 1)
	}

	static func resolveSequence(
		from startDigit: Character,
		to targetDigit: Character,
		index: Int,
		scrollsUp: Bool
	) -> [Character] {
		let wheelCount = max(hexWheel.count, 1)
		let startIndex = hexWheel.firstIndex(of: startDigit) ?? 0
		let targetIndex = hexWheel.firstIndex(of: targetDigit) ?? startIndex
		let directedDistance =
			scrollsUp
			? (targetIndex - startIndex + wheelCount) % wheelCount
			: (startIndex - targetIndex + wheelCount) % wheelCount
		let extraSteps =
			resolveExtraLoops(index: index, targetDigit: targetDigit)
			* wheelCount
		let totalSteps =
			directedDistance + extraSteps
		return (0...totalSteps).map { offset in
			let wheelIndex =
				scrollsUp
				? (startIndex + offset) % wheelCount
				: (startIndex - offset + (totalSteps + wheelCount) * wheelCount) % wheelCount
			return hexWheel[wheelIndex]
		}
	}

	private static func pendingSeed(index: Int) -> UInt64 {
		let uptimeBucket = UInt64((ProcessInfo.processInfo.systemUptime * 1_000).rounded(.down))
		let mixedIndex = UInt64(index + 1) &* 0x9E37_79B9_7F4A_7C15
		return pendingHexRollBaseSeed ^ uptimeBucket ^ mixedIndex
	}
}
