import CoreGraphics
import CoreML
import Foundation
import Vision

/// Serves Neural Engine OCR requests in a restartable Rsnap worker process.
package enum TextRecognitionHelper {
	package static let argument = "--rsnap-text-recognition-helper"
	static let maximumWorkerAttempts = 2
	private static let protocolVersion = 1
	private static let frameHeaderByteCount = MemoryLayout<UInt64>.size
	private static let maximumFrameByteCount = 1_073_741_824

	struct Input: Codable {
		let protocolVersion: Int
		let width: Int
		let height: Int
		let rgba: Data
		let usesLanguageCorrection: Bool
		let automaticallyDetectsLanguage: Bool

		init(
			width: Int,
			height: Int,
			rgba: Data,
			usesLanguageCorrection: Bool,
			automaticallyDetectsLanguage: Bool
		) {
			protocolVersion = TextRecognitionHelper.protocolVersion
			self.width = width
			self.height = height
			self.rgba = rgba
			self.usesLanguageCorrection = usesLanguageCorrection
			self.automaticallyDetectsLanguage = automaticallyDetectsLanguage
		}
	}

	package struct Output: Codable {
		package let protocolVersion: Int
		package let text: String
		package let observationCount: Int
		package let recognizedLines: Int
		package let recognizedCharacters: Int
		package let visionRequestMilliseconds: Double
		package let processingMilliseconds: Double
		package let failureDescription: String?
	}

	package static func runIfRequested() -> Bool {
		guard ProcessInfo.processInfo.arguments.dropFirst().first == argument else {
			return false
		}

		do {
			while let encodedInput = try readFrame(from: .standardInput) {
				let output = autoreleasepool {
					do {
						let input = try decodeInput(encodedInput)
						return perform(input)
					} catch {
						return failureOutput("Invalid OCR helper request: \(error)")
					}
				}
				try writeFrame(encode(output), to: .standardOutput)
			}
		} catch {
			return true
		}
		return true
	}

	package static func configureNeuralEngine(for request: VNRequest) -> Bool {
		guard let supportedDevices = try? request.supportedComputeStageDevices,
			let mainDevices = supportedDevices[.main],
			let neuralEngine = mainDevices.first(where: { device in
				if case .neuralEngine = device {
					return true
				}
				return false
			})
		else {
			return false
		}
		request.setComputeDevice(neuralEngine, for: .main)
		return true
	}

	package static func isE5RecompileRequired(_ description: String) -> Bool {
		let normalized = description.lowercased()
		guard normalized.contains("e5rt") else {
			return false
		}
		return normalized.contains("code: 13")
			|| normalized.contains(", 13)")
			|| normalized.contains("recompile e5")
	}

	static func encode(_ input: Input) throws -> Data {
		let encoder = PropertyListEncoder()
		encoder.outputFormat = .binary
		return try encoder.encode(input)
	}

	static func decodeOutput(_ data: Data) throws -> Output {
		let output = try PropertyListDecoder().decode(Output.self, from: data)
		guard output.protocolVersion == protocolVersion else {
			throw HelperError.unsupportedProtocol(output.protocolVersion)
		}
		return output
	}

	package static func writeFrame(_ payload: Data, to handle: FileHandle) throws {
		var payloadLength = UInt64(payload.count).bigEndian
		let header = withUnsafeBytes(of: &payloadLength) { Data($0) }
		try handle.write(contentsOf: header)
		try handle.write(contentsOf: payload)
	}

	package static func readFrame(from handle: FileHandle) throws -> Data? {
		guard
			let header = try readExactly(
				frameHeaderByteCount,
				from: handle,
				allowsCleanEndOfFile: true
			)
		else {
			return nil
		}
		let payloadLength = header.reduce(UInt64(0)) { partialResult, byte in
			(partialResult << 8) | UInt64(byte)
		}
		guard payloadLength <= UInt64(maximumFrameByteCount), payloadLength <= UInt64(Int.max)
		else {
			throw HelperError.invalidFrameLength(payloadLength)
		}
		return try readExactly(
			Int(payloadLength),
			from: handle,
			allowsCleanEndOfFile: false
		)
	}

	private static func encode(_ output: Output) throws -> Data {
		let encoder = PropertyListEncoder()
		encoder.outputFormat = .binary
		return try encoder.encode(output)
	}

	private static func readExactly(
		_ byteCount: Int,
		from handle: FileHandle,
		allowsCleanEndOfFile: Bool
	) throws -> Data? {
		var result = Data()
		result.reserveCapacity(byteCount)
		while result.count < byteCount {
			let remainingByteCount = byteCount - result.count
			let chunk = try handle.read(upToCount: remainingByteCount) ?? Data()
			guard chunk.isEmpty == false else {
				if result.isEmpty, allowsCleanEndOfFile {
					return nil
				}
				throw HelperError.truncatedFrame
			}
			result.append(chunk)
		}
		return result
	}

	private static func decodeInput(_ data: Data) throws -> Input {
		let input = try PropertyListDecoder().decode(Input.self, from: data)
		guard input.protocolVersion == protocolVersion else {
			throw HelperError.unsupportedProtocol(input.protocolVersion)
		}
		guard input.width > 0, input.height > 0 else {
			throw HelperError.invalidImageDimensions
		}
		let (pixelCount, pixelOverflow) = input.width.multipliedReportingOverflow(
			by: input.height)
		let (byteCount, byteOverflow) = pixelCount.multipliedReportingOverflow(by: 4)
		guard pixelOverflow == false, byteOverflow == false, input.rgba.count == byteCount else {
			throw HelperError.invalidImageData
		}
		return input
	}

	private static func perform(_ input: Input) -> Output {
		guard
			let cgImage = NativeHostImageBridge.cgImage(
				width: input.width,
				height: input.height,
				rgba: input.rgba
			)
		else {
			return failureOutput("OCR helper could not reconstruct the image.")
		}

		let request = VNRecognizeTextRequest()
		request.recognitionLevel = .accurate
		request.usesLanguageCorrection = input.usesLanguageCorrection
		request.automaticallyDetectsLanguage = input.automaticallyDetectsLanguage
		guard configureNeuralEngine(for: request) else {
			return failureOutput("Vision does not expose the Neural Engine for text recognition.")
		}

		let handler = VNImageRequestHandler(cgImage: cgImage)
		let visionStartedAt = ProcessInfo.processInfo.systemUptime
		do {
			try handler.perform([request])
		} catch {
			return failureOutput(
				String(describing: error),
				visionRequestMilliseconds: NativeHostTelemetry.milliseconds(
					since: visionStartedAt)
			)
		}

		let visionRequestMilliseconds = NativeHostTelemetry.milliseconds(since: visionStartedAt)
		let resultProcessingStartedAt = ProcessInfo.processInfo.systemUptime
		let observations = request.results ?? []
		let recognizedLines = observations.compactMap { observation -> String? in
			guard let line = observation.topCandidates(1).first?.string,
				line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
			else {
				return nil
			}
			return line
		}
		let text = recognizedLines.joined(separator: "\n")
		return Output(
			protocolVersion: protocolVersion,
			text: text,
			observationCount: observations.count,
			recognizedLines: recognizedLines.count,
			recognizedCharacters: text.count,
			visionRequestMilliseconds: visionRequestMilliseconds,
			processingMilliseconds: NativeHostTelemetry.milliseconds(
				since: resultProcessingStartedAt),
			failureDescription: nil
		)
	}

	private static func failureOutput(
		_ description: String,
		visionRequestMilliseconds: Double = 0
	) -> Output {
		Output(
			protocolVersion: protocolVersion,
			text: "",
			observationCount: 0,
			recognizedLines: 0,
			recognizedCharacters: 0,
			visionRequestMilliseconds: visionRequestMilliseconds,
			processingMilliseconds: 0,
			failureDescription: description
		)
	}

	private enum HelperError: Error, CustomStringConvertible {
		case invalidImageData
		case invalidImageDimensions
		case invalidFrameLength(UInt64)
		case truncatedFrame
		case unsupportedProtocol(Int)

		var description: String {
			switch self {
			case .invalidImageData:
				"image byte count does not match its dimensions"
			case .invalidImageDimensions:
				"image dimensions are invalid"
			case .invalidFrameLength(let byteCount):
				"frame byte count \(byteCount) is invalid"
			case .truncatedFrame:
				"frame ended before its declared byte count"
			case .unsupportedProtocol(let version):
				"unsupported protocol version \(version)"
			}
		}
	}
}
