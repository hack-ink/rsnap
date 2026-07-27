#!/usr/bin/env swift
import CryptoKit
import Foundation

func fail(_ message: String) -> Never {
	FileHandle.standardError.write(Data("error: \(message)\n".utf8))
	exit(1)
}

guard CommandLine.arguments.count == 2 else {
	fail("usage: printf PRIVATE_KEY | swift verify-sparkle-key.swift EXPECTED_PUBLIC_KEY")
}

let privateKeyInput = FileHandle.standardInput.readDataToEndOfFile()
guard
	let privateKeyText = String(data: privateKeyInput, encoding: .utf8)?
		.trimmingCharacters(in: .whitespacesAndNewlines),
	!privateKeyText.isEmpty,
	!privateKeyText.contains(where: \.isWhitespace),
	let privateKeyData = Data(base64Encoded: privateKeyText),
	[32, 96].contains(privateKeyData.count),
	privateKeyData.base64EncodedString() == privateKeyText
else {
	fail("Sparkle private key is not a canonical supported Sparkle Ed25519 key")
}

let expectedPublicKeyText = CommandLine.arguments[1]
guard
	!expectedPublicKeyText.contains(where: \.isWhitespace),
	let expectedPublicKeyData = Data(base64Encoded: expectedPublicKeyText),
	expectedPublicKeyData.count == 32,
	expectedPublicKeyData.base64EncodedString() == expectedPublicKeyText
else {
	fail("expected Sparkle public key is not a canonical Ed25519 public key")
}

if privateKeyData.count == 32 {
	do {
		let privateKey = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKeyData)
		guard privateKey.publicKey.rawRepresentation == expectedPublicKeyData else {
			fail("Sparkle private key does not match the expected public key")
		}
	} catch {
		fail("Sparkle private key is invalid")
	}
} else {
	// Sparkle's legacy format is its 64-byte orlp/Ed25519 private key followed
	// by the corresponding 32-byte public key. The release artifact validator
	// later verifies the generated signature with this expected public key.
	guard privateKeyData.suffix(32) == expectedPublicKeyData else {
		fail("Sparkle private key does not match the expected public key")
	}
}
