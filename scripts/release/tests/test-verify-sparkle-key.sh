#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
VERIFY_TOOL="$ROOT_DIR/scripts/release/verify-sparkle-key.swift"
KEY_GENERATOR="$ROOT_DIR/scripts/release/tests/generate-sparkle-test-keys.swift"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

swift "$KEY_GENERATOR" >"$TEST_ROOT/keys"

private_key="$(sed -n '1p' "$TEST_ROOT/keys")"
public_key="$(sed -n '2p' "$TEST_ROOT/keys")"
other_public_key="$(sed -n '4p' "$TEST_ROOT/keys")"
legacy_private_key="$(sed -n '5p' "$TEST_ROOT/keys")"
mismatched_legacy_private_key="$(sed -n '6p' "$TEST_ROOT/keys")"

printf '%s\n' "$private_key" | swift "$VERIFY_TOOL" "$public_key"
printf '%s\n' "$legacy_private_key" | swift "$VERIFY_TOOL" "$public_key"

if printf '%s\n' "$private_key" \
	| swift "$VERIFY_TOOL" "$other_public_key" >/dev/null 2>&1; then
	echo "key verifier accepted a mismatched public key" >&2
	exit 1
fi

if printf '%s\n' "$mismatched_legacy_private_key" \
	| swift "$VERIFY_TOOL" "$public_key" >/dev/null 2>&1; then
	echo "key verifier accepted a legacy key with a mismatched public key" >&2
	exit 1
fi

if printf '%s\n' "not-base64" \
	| swift "$VERIFY_TOOL" "$public_key" >/dev/null 2>&1; then
	echo "key verifier accepted a malformed private key" >&2
	exit 1
fi

if printf 'AA==\n' \
	| swift "$VERIFY_TOOL" "$public_key" >/dev/null 2>&1; then
	echo "key verifier accepted an unsupported private key length" >&2
	exit 1
fi

echo "verify-sparkle-key tests passed"
