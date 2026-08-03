import Darwin
import Foundation

/// Owns the restartable process that retains the warm Vision and E5 model state.
package final class TextRecognitionWorker: @unchecked Sendable {
	private static let requestTimeoutSeconds = 120

	private let executableURL: URL
	private let argument: String
	private var process: Process?
	private var channel: FileHandle?

	package init(
		executableURL: URL,
		argument: String = TextRecognitionHelper.argument
	) {
		self.executableURL = executableURL
		self.argument = argument
	}

	deinit {
		invalidate()
	}

	package func perform(encodedInput: Data) throws -> TextRecognitionHelper.Output {
		try ensureRunning()
		guard let channel else {
			throw WorkerError.channelUnavailable
		}

		let encodedOutput: Data
		do {
			try TextRecognitionHelper.writeFrame(encodedInput, to: channel)
			guard let response = try TextRecognitionHelper.readFrame(from: channel) else {
				throw WorkerError.workerClosedChannel
			}
			encodedOutput = response
		} catch {
			let ioErrorNumber = errno
			let errorDomain = (error as NSError).domain
			invalidate()
			if errorDomain == NSCocoaErrorDomain || errorDomain == NSPOSIXErrorDomain,
				ioErrorNumber == EAGAIN || ioErrorNumber == EWOULDBLOCK
			{
				throw WorkerError.timedOut
			}
			throw error
		}

		do {
			return try TextRecognitionHelper.decodeOutput(encodedOutput)
		} catch {
			invalidate()
			throw error
		}
	}

	package func restart() {
		invalidate()
	}

	private func ensureRunning() throws {
		if let process, process.isRunning, channel != nil {
			return
		}
		invalidate()

		var descriptors: [Int32] = [0, 0]
		guard socketpair(AF_UNIX, SOCK_STREAM, 0, &descriptors) == 0 else {
			throw WorkerError.socketPairFailed(errno)
		}
		let parentChannel = FileHandle(fileDescriptor: descriptors[0], closeOnDealloc: true)
		let childChannel = FileHandle(fileDescriptor: descriptors[1], closeOnDealloc: true)
		do {
			try Self.configureSocket(descriptors[0], usesTimeout: true)
			try Self.configureSocket(descriptors[1], usesTimeout: false)
			try Self.markCloseOnExec(descriptors[0])
			try Self.markCloseOnExec(descriptors[1])

			let process = Process()
			process.executableURL = executableURL
			process.arguments = [argument]
			process.standardInput = childChannel
			process.standardOutput = childChannel
			try process.run()
			try childChannel.close()
			self.process = process
			channel = parentChannel
		} catch {
			try? parentChannel.close()
			try? childChannel.close()
			throw error
		}
	}

	private func invalidate() {
		try? channel?.close()
		channel = nil
		if let process, process.isRunning {
			process.terminate()
		}
		process = nil
	}

	private static func configureSocket(_ descriptor: Int32, usesTimeout: Bool) throws {
		var noSignal: Int32 = 1
		guard
			setsockopt(
				descriptor,
				SOL_SOCKET,
				SO_NOSIGPIPE,
				&noSignal,
				socklen_t(MemoryLayout.size(ofValue: noSignal))
			) == 0
		else {
			throw WorkerError.socketOptionFailed(errno)
		}
		guard usesTimeout else {
			return
		}

		var timeout = timeval(tv_sec: requestTimeoutSeconds, tv_usec: 0)
		for option in [SO_RCVTIMEO, SO_SNDTIMEO] {
			guard
				setsockopt(
					descriptor,
					SOL_SOCKET,
					option,
					&timeout,
					socklen_t(MemoryLayout.size(ofValue: timeout))
				) == 0
			else {
				throw WorkerError.socketOptionFailed(errno)
			}
		}
	}

	private static func markCloseOnExec(_ descriptor: Int32) throws {
		let flags = fcntl(descriptor, F_GETFD)
		guard flags >= 0, fcntl(descriptor, F_SETFD, flags | FD_CLOEXEC) == 0 else {
			throw WorkerError.closeOnExecFailed(errno)
		}
	}

	private enum WorkerError: Error, CustomStringConvertible {
		case channelUnavailable
		case closeOnExecFailed(Int32)
		case socketOptionFailed(Int32)
		case socketPairFailed(Int32)
		case timedOut
		case workerClosedChannel

		var description: String {
			switch self {
			case .channelUnavailable:
				"worker channel is unavailable"
			case .closeOnExecFailed(let code):
				"worker close-on-exec setup failed (errno=\(code))"
			case .socketOptionFailed(let code):
				"worker socket setup failed (errno=\(code))"
			case .socketPairFailed(let code):
				"worker socket pair creation failed (errno=\(code))"
			case .timedOut:
				"worker request timed out"
			case .workerClosedChannel:
				"worker closed its channel before returning a result"
			}
		}
	}
}
