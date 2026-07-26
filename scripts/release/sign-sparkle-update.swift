#!/usr/bin/env swift

import CryptoKit
import Foundation

func fail(_ message: String) -> Never {
	FileHandle.standardError.write(Data("error: \(message)\n".utf8))
	exit(1)
}

guard CommandLine.arguments.count == 3 else {
	fail("usage: sign-sparkle-update.swift ARCHIVE EXPECTED_PUBLIC_KEY")
}

let archiveURL = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: false)
let expectedPublicKeyText = CommandLine.arguments[2]
guard let expectedPublicKey = Data(base64Encoded: expectedPublicKeyText),
	expectedPublicKey.count == 32
else {
	fail("expected Sparkle public key is not a 32-byte base64 value")
}

let privateKeyInput = FileHandle.standardInput.readDataToEndOfFile()
guard let privateKeyText = String(data: privateKeyInput, encoding: .utf8) else {
	fail("Sparkle private key is not UTF-8")
}
let trimmedPrivateKey = privateKeyText.trimmingCharacters(in: .whitespacesAndNewlines)
guard trimmedPrivateKey.isEmpty == false else {
	fail("Sparkle private key is empty")
}
guard let privateKeyData = Data(base64Encoded: trimmedPrivateKey) else {
	fail("Sparkle private key is not valid base64")
}

do {
	let privateKey = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKeyData)
	guard privateKey.publicKey.rawRepresentation == expectedPublicKey else {
		fail("Rsnap Sparkle private key does not match the app public key")
	}
	let archive = try Data(contentsOf: archiveURL, options: .mappedIfSafe)
	guard archive.isEmpty == false else {
		fail("release archive is empty")
	}
	let signature = try privateKey.signature(for: archive)
	guard privateKey.publicKey.isValidSignature(signature, for: archive) else {
		fail("CryptoKit could not verify the generated Sparkle signature")
	}
	print(signature.base64EncodedString())
} catch {
	fail("Sparkle update signing failed")
}
