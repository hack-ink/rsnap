#!/usr/bin/env swift
import CryptoKit
import Foundation

let firstPrivateKey = Curve25519.Signing.PrivateKey()
let secondPrivateKey = Curve25519.Signing.PrivateKey()

print(firstPrivateKey.rawRepresentation.base64EncodedString())
print(firstPrivateKey.publicKey.rawRepresentation.base64EncodedString())
print(secondPrivateKey.rawRepresentation.base64EncodedString())
print(secondPrivateKey.publicKey.rawRepresentation.base64EncodedString())

var legacyPrivateKey = Data(SHA512.hash(data: firstPrivateKey.rawRepresentation))
legacyPrivateKey[legacyPrivateKey.startIndex] &= 248
legacyPrivateKey[legacyPrivateKey.startIndex + 31] &= 63
legacyPrivateKey[legacyPrivateKey.startIndex + 31] |= 64
legacyPrivateKey.append(firstPrivateKey.publicKey.rawRepresentation)
print(legacyPrivateKey.base64EncodedString())

var mismatchedLegacyPrivateKey = legacyPrivateKey
mismatchedLegacyPrivateKey.replaceSubrange(
	mismatchedLegacyPrivateKey.index(mismatchedLegacyPrivateKey.endIndex, offsetBy: -32)...,
	with: secondPrivateKey.publicKey.rawRepresentation
)
print(mismatchedLegacyPrivateKey.base64EncodedString())
