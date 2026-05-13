import AppKit
@preconcurrency import CoreGraphics
import Foundation
import RsnapHostBridge

struct NativeScrollCaptureSampleFrame: Sendable {
	let region: RGBARegionSnapshot
	let source: String
	let frameSequence: UInt64
	let frameAgeMicroseconds: UInt64
}

struct NativeScrollCaptureFallbackRequest: Sendable {
	let rect: CGRect
	let pixelRect: CGRect
	let source: CaptureSessionController.FrozenCaptureJobSource
	let frameSequence: UInt64
}

struct NativeScrollCaptureLiveFrameRequest: @unchecked Sendable {
	let stream: LiveFrameStreamBroker
	let rect: CGRect
	let pixelRect: CGRect
	let afterFrameSequence: UInt64
	let maximumFrameAgeMicroseconds: UInt64?
	let maxFrames: Int
	let waitForFresh: Bool
}

struct NativeScrollCaptureSampleBatch: Sendable {
	let frames: [NativeScrollCaptureSampleFrame]
	let latestFrameSequence: UInt64?
}

struct NativeScrollCaptureObservation: Sendable {
	let sampledFrame: NativeScrollCaptureSampleFrame
	let registrationStrategy: String
	let result: ScrollObserveResult?
	let errorDescription: String?
}

struct NativeScrollCapturePreviewUpdate: @unchecked Sendable {
	let image: CGImage
	let exportWidth: Int
	let exportHeight: Int
	let result: ScrollObserveResult
	let viewportTopYPixels: Int
	let viewportHeightPixels: Int
}

struct NativeScrollCaptureObservationBatch: Sendable {
	let observations: [NativeScrollCaptureObservation]
	let preview: NativeScrollCapturePreviewUpdate?
	let previewErrorDescription: String?
	let previewExportMilliseconds: Double?
}

enum NativeScrollCaptureObservationPipeline {
	static func sampleBatch(
		liveFrameRequest: NativeScrollCaptureLiveFrameRequest?,
		fallbackRequest: NativeScrollCaptureFallbackRequest?
	) -> NativeScrollCaptureSampleBatch {
		let liveSample = liveFrameRequest.map(sampleFrames(from:))
		var sampledFrames = liveSample?.frames ?? []
		if sampledFrames.isEmpty {
			appendFallbackFrame(to: &sampledFrames, fallbackRequest: fallbackRequest)
		}

		return NativeScrollCaptureSampleBatch(
			frames: sampledFrames,
			latestFrameSequence: liveSample?.latestFrameSequence
		)
	}

	static func makeBatch(
		sampledFrames: [NativeScrollCaptureSampleFrame],
		stitcher: RsnapScrollCaptureSession,
		motionRowsHint: Int?,
		previewRefreshDue: Bool
	) -> NativeScrollCaptureObservationBatch {
		var observations: [NativeScrollCaptureObservation] = []
		var latestPreviewCandidate: NativeScrollCaptureObservation?
		for sampledFrame in sampledFrames {
			let observation = observe(
				sampledFrame,
				stitcher: stitcher,
				motionRowsHint: motionRowsHint
			)
			if observation.result?.outcome != .noChange {
				latestPreviewCandidate = observation
			}
			observations.append(observation)
		}
		let preview = previewUpdate(
			stitcher: stitcher,
			candidate: latestPreviewCandidate,
			previewRefreshDue: previewRefreshDue
		)
		return NativeScrollCaptureObservationBatch(
			observations: observations,
			preview: preview.update,
			previewErrorDescription: preview.errorDescription,
			previewExportMilliseconds: preview.exportMilliseconds
		)
	}

	private static func sampleFrames(
		from request: NativeScrollCaptureLiveFrameRequest
	) -> NativeScrollCaptureSampleBatch {
		var frames: [NativeScrollCaptureSampleFrame] = []
		var nextAfterFrameSequence = request.afterFrameSequence
		var latestFrameSequence: UInt64?

		for _ in 0..<request.maxFrames {
			guard
				let frame = request.stream.nextRegionFrame(
					in: request.rect,
					pixelRect: request.pixelRect,
					afterFrameSequence: nextAfterFrameSequence,
					waitForFresh: request.waitForFresh
				)
			else {
				break
			}
			nextAfterFrameSequence = frame.frameSequence
			latestFrameSequence = max(latestFrameSequence ?? 0, frame.frameSequence)
			if let maximumFrameAgeMicroseconds = request.maximumFrameAgeMicroseconds,
				frame.frameAgeMicroseconds > maximumFrameAgeMicroseconds
			{
				continue
			}
			guard frameMatchesPixelRect(frame.region, pixelRect: request.pixelRect) else {
				continue
			}
			writeNativeScrollCaptureDebugDump(
				frame.region,
				name: "sample-\(frame.frameSequence)"
			)
			frames.append(
				NativeScrollCaptureSampleFrame(
					region: frame.region,
					source: "ordered_live_stream_region",
					frameSequence: frame.frameSequence,
					frameAgeMicroseconds: frame.frameAgeMicroseconds
				))
		}

		return NativeScrollCaptureSampleBatch(
			frames: frames,
			latestFrameSequence: latestFrameSequence
		)
	}

	private static func appendFallbackFrame(
		to sampledFrames: inout [NativeScrollCaptureSampleFrame],
		fallbackRequest: NativeScrollCaptureFallbackRequest?
	) {
		guard
			let fallbackRequest,
			let image = CaptureOverlayController.captureImageBelowOverlay(
				in: fallbackRequest.rect,
				source: fallbackRequest.source
			),
			let snapshot = NativeHostImageBridge.rgbaSnapshot(from: image),
			frameMatchesPixelRect(snapshot, pixelRect: fallbackRequest.pixelRect)
		else {
			return
		}
		writeNativeScrollCaptureDebugDump(
			snapshot,
			name: "fallback-\(fallbackRequest.frameSequence)"
		)
		sampledFrames.append(
			NativeScrollCaptureSampleFrame(
				region: snapshot,
				source: "below_overlay_capture_region",
				frameSequence: fallbackRequest.frameSequence,
				frameAgeMicroseconds: 0
			))
	}

	private static func frameMatchesPixelRect(
		_ snapshot: RGBARegionSnapshot,
		pixelRect: CGRect
	) -> Bool {
		snapshot.width == Int(pixelRect.width.rounded())
			&& snapshot.height == Int(pixelRect.height.rounded())
	}

	private static func observe(
		_ sampledFrame: NativeScrollCaptureSampleFrame,
		stitcher: RsnapScrollCaptureSession,
		motionRowsHint: Int?
	) -> NativeScrollCaptureObservation {
		let registrationStrategy = "pairwise"
		do {
			let result = try stitcher.observeDownwardFrame(
				sampledFrame.region,
				motionRowsHint: motionRowsHint
			)
			return NativeScrollCaptureObservation(
				sampledFrame: sampledFrame,
				registrationStrategy: registrationStrategy,
				result: result,
				errorDescription: nil
			)
		} catch {
			return NativeScrollCaptureObservation(
				sampledFrame: sampledFrame,
				registrationStrategy: registrationStrategy,
				result: nil,
				errorDescription: String(describing: error)
			)
		}
	}

	private static func previewUpdate(
		stitcher: RsnapScrollCaptureSession,
		candidate: NativeScrollCaptureObservation?,
		previewRefreshDue: Bool
	) -> (
		update: NativeScrollCapturePreviewUpdate?,
		errorDescription: String?,
		exportMilliseconds: Double?
	) {
		guard previewRefreshDue, let candidate, let result = candidate.result else {
			return (nil, nil, nil)
		}
		let previewStartedAt = ProcessInfo.processInfo.systemUptime
		do {
			if let preview = try stitcher.previewImage() {
				guard let previewImage = NativeHostImageBridge.cgImage(from: preview) else {
					return (
						nil,
						"scroll preview export returned no image",
						NativeHostTelemetry.milliseconds(since: previewStartedAt)
					)
				}

				return (
					NativeScrollCapturePreviewUpdate(
						image: previewImage,
						exportWidth: result.exportWidth,
						exportHeight: result.exportHeight,
						result: result,
						viewportTopYPixels: result.currentViewportTopY,
						viewportHeightPixels: candidate.sampledFrame.region.height
					),
					nil,
					NativeHostTelemetry.milliseconds(since: previewStartedAt)
				)
			}
			return (
				nil,
				"scroll preview export returned no image",
				NativeHostTelemetry.milliseconds(since: previewStartedAt)
			)
		} catch {
			return (
				nil,
				String(describing: error),
				NativeHostTelemetry.milliseconds(since: previewStartedAt)
			)
		}
	}
}

func writeNativeScrollCaptureDebugDump(_ snapshot: RGBARegionSnapshot, name: String) {
	guard
		let rawDirectory = ProcessInfo.processInfo.environment["RSNAP_SCROLL_CAPTURE_DUMP_DIR"],
		rawDirectory.isEmpty == false,
		let pngData = try? RsnapExportEncoder.pngData(from: snapshot)
	else {
		return
	}
	let directory = URL(fileURLWithPath: rawDirectory, isDirectory: true)
	try? FileManager.default.createDirectory(
		at: directory,
		withIntermediateDirectories: true
	)
	let safeName = name.replacingOccurrences(of: "/", with: "_")
	try? pngData.write(to: directory.appendingPathComponent("\(safeName).png"))
}
