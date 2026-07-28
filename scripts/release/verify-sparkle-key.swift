#!/usr/bin/env swift

import CryptoKit
import Foundation

func fail(_ message: String) -> Never {
	FileHandle.standardError.write(Data("error: \(message)\n".utf8))
	exit(1)
}

guard CommandLine.arguments.count == 2 else {
	fail("usage: verify-sparkle-key.swift EXPECTED_PUBLIC_KEY")
}

let expectedPublicKey = CommandLine.arguments[1]
guard let expectedPublicKeyData = Data(base64Encoded: expectedPublicKey),
	expectedPublicKeyData.count == 32
else {
	fail("expected Sparkle public key must be base64 that decodes to 32 bytes")
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

switch privateKeyData.count {
case 32:
	do {
		let privateKey = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKeyData)
		guard privateKey.publicKey.rawRepresentation == expectedPublicKeyData else {
			fail("Rsnap Sparkle private key does not match the app public key")
		}
	} catch {
		fail("Sparkle private key is invalid")
	}
case 96:
	// Sparkle's legacy secret stores an expanded private value followed by the public key.
	guard Data(privateKeyData.suffix(32)) == expectedPublicKeyData else {
		fail("Rsnap Sparkle private key does not match the app public key")
	}
default:
	fail("Sparkle private key must decode to a 32-byte seed or 96-byte legacy secret")
}
