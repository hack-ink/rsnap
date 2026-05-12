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
			if let export = try stitcher.exportImage() {
				guard let exportImage = NativeHostImageBridge.cgImage(from: export) else {
					return (
						nil,
						"scroll preview export returned no image",
						NativeHostTelemetry.milliseconds(since: previewStartedAt)
					)
				}

				return (
					NativeScrollCapturePreviewUpdate(
						image: exportImage,
						exportWidth: export.width,
						exportHeight: export.height,
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
