#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
VERIFIER="$ROOT_DIR/scripts/release/verify-sparkle-key.swift"
PUBLIC_KEY="11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo="
SEED="nWGxne/9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A="
# The zero prefix cannot be the seed for PUBLIC_KEY. This fixture proves that the
# legacy path treats its first 64 bytes as an expanded private value.
LEGACY_SECRET="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAANdamAGCsQq31Uv+08lkBzoO4XLz2qYjJa8CGmj3B1Ea"
WRONG_PUBLIC_KEY="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

printf '%s\n' "$SEED" | "$VERIFIER" "$PUBLIC_KEY"
printf '%s\n' "$LEGACY_SECRET" | "$VERIFIER" "$PUBLIC_KEY"

if printf '%s\n' "$LEGACY_SECRET" | "$VERIFIER" "$WRONG_PUBLIC_KEY" >/dev/null 2>&1; then
	echo "error: verifier accepted a legacy secret with the wrong trailing public key" >&2
	exit 1
fi

if printf '%s\n' 'AA==' | "$VERIFIER" "$PUBLIC_KEY" >/dev/null 2>&1; then
	echo "error: verifier accepted an invalid Sparkle secret length" >&2
	exit 1
fi
